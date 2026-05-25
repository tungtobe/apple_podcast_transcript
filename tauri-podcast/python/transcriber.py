#!/usr/bin/env python3
"""
CLI Transcriber — no Streamlit dependency.
Outputs newline-delimited JSON events to stdout.

Usage:
  python transcriber.py \\
    --file <path> \\
    --mode <gemini|whisper> \\
    --model-size <small|medium> \\
    --language <ja|auto> \\
    [--api-key <key>] \\
    [--gemini-model <model>] \\
    [--cache-dir <dir>] \\
    [--force-rerun]

Event types (one JSON per line, flushed immediately):
  {"type":"progress","step":N,"total":4,"message":"...","percent":0-100}
  {"type":"result","segments":[...],"cached":false}
  {"type":"error","message":"..."}
"""

import argparse
import hashlib
import json
import math
import os
import platform
import re
import subprocess
import sys
import tempfile
import time
import traceback
import wave
from pathlib import Path

# Mutable, set in main() so log() can write to cache_dir/transcriber_debug.log
_LOG_FILE_PATH: str | None = None


# ─── helpers ──────────────────────────────────────────────────────────────────

def emit(obj: dict):
    """Write a JSON event line to stdout and flush immediately."""
    print(json.dumps(obj, ensure_ascii=False), flush=True)


def emit_progress(step: int, total: int, message: str, percent: int = None):
    if percent is None:
        percent = int((step / total) * 100) if total else 0
    emit({"type": "progress", "step": step, "total": total,
          "message": message, "percent": percent})


def emit_result(segments: list, cached: bool = False, engine: str = ""):
    emit({"type": "result", "segments": segments, "cached": cached, "engine": engine})


def emit_error(message: str):
    emit({"type": "error", "message": message})


def emit_log(message: str):
    """Emit a log event (forwarded to UI) AND write to debug file."""
    ts = time.strftime("%H:%M:%S")
    line = f"[{ts}] {message}"
    emit({"type": "log", "message": line})
    if _LOG_FILE_PATH:
        try:
            with open(_LOG_FILE_PATH, "a", encoding="utf-8") as f:
                f.write(line + "\n")
        except Exception:
            pass


def _strip_markdown_fence(text: str) -> str:
    """Return fenced JSON content when Gemini wraps its response in ```json."""
    stripped = text.strip()
    if not stripped.startswith("```"):
        return stripped

    parts = stripped.split("```", 2)
    if len(parts) < 2:
        return stripped

    fenced = parts[1].strip()
    if fenced.lower().startswith("json"):
        fenced = fenced[4:].strip()
    return fenced


def _decode_first_json_value(text: str):
    """Decode the first JSON value, allowing benign Gemini text around it."""
    candidate = _strip_markdown_fence(text)
    decoder = json.JSONDecoder(strict=False)
    first_error = None
    for start, char in enumerate(candidate):
        if char not in "[{":
            continue
        try:
            return decoder.raw_decode(candidate[start:])[0]
        except json.JSONDecodeError as e:
            if first_error is None:
                first_error = e

    if first_error:
        raise first_error
    raise ValueError("Gemini response did not contain a JSON array or object.")


def _normalize_segment(segment: dict) -> dict:
    if not isinstance(segment, dict):
        raise ValueError("Gemini segment is not an object.")

    try:
        start = float(segment["start"])
        end = float(segment["end"])
        text = str(segment["text"]).strip()
    except (KeyError, TypeError, ValueError) as e:
        raise ValueError(f"Gemini segment has invalid fields: {segment}") from e

    return {"start": start, "end": end, "text": text}


def salvage_segments(raw: str) -> list:
    """Recover well-formed segment objects from a malformed JSON array.
    Used when an unescaped quote inside `text` breaks the full array, so we
    don't lose the whole chunk."""
    pattern = re.compile(
        r'\{\s*"start"\s*:\s*([0-9.]+)\s*,'
        r'\s*"end"\s*:\s*([0-9.]+)\s*,'
        r'\s*"text"\s*:\s*"((?:\\.|[^"\\])*)"\s*\}'
    )
    out = []
    for m in pattern.finditer(raw):
        try:
            out.append({
                "start": float(m.group(1)),
                "end": float(m.group(2)),
                "text": bytes(m.group(3), "utf-8").decode("unicode_escape"),
            })
        except Exception:
            continue
    return out


def parse_gemini_segments(response_text: str) -> list:
    """Parse Gemini transcript JSON from markdown, raw JSON, or object envelopes."""
    payload = _decode_first_json_value(response_text)
    if isinstance(payload, dict) and isinstance(payload.get("segments"), list):
        payload = payload["segments"]

    if not isinstance(payload, list):
        raise ValueError("Gemini response JSON must be an array of segments.")

    return [_normalize_segment(segment) for segment in payload]


def _cleanup_temp(path: str | None):
    """Silently remove a temp file if it exists."""
    if path:
        try:
            os.remove(path)
        except OSError:
            pass


# ─── file helpers ─────────────────────────────────────────────────────────────

VIDEO_FORMATS = {"mp4", "mov", "avi", "mkv", "webm", "flv", "wmv", "m2ts", "ts"}
AUDIO_FORMATS = {"mp3", "m4a", "wav", "aac"}


def is_video_file(filename: str) -> bool:
    return Path(filename).suffix.lstrip(".").lower() in VIDEO_FORMATS


def ffmpeg_extract_audio(video_path: str, out_path: str) -> tuple[bool, str]:
    """Extract 16kHz mono MP3 audio from video. Returns (success, error_msg)."""
    cmd = [
        "ffmpeg", "-y",
        "-i", video_path,
        "-vn",
        "-ar", "16000",
        "-ac", "1",
        "-c:a", "libmp3lame",
        "-q:a", "4",
        out_path
    ]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
        if result.returncode != 0:
            return False, result.stderr[-1000:]
        return True, ""
    except FileNotFoundError:
        return False, "ffmpeg not found. Install with: brew install ffmpeg"
    except subprocess.TimeoutExpired:
        return False, "ffmpeg timed out after 5 minutes"


def hash_file(file_path: str) -> str:
    """SHA-256 hash of file contents."""
    sha256 = hashlib.sha256()
    with open(file_path, "rb") as f:
        while chunk := f.read(65536):
            sha256.update(chunk)
    return sha256.hexdigest()


def get_audio_duration(file_path: str):
    """Return duration in seconds for WAV files; None otherwise."""
    try:
        with wave.open(file_path, "r") as wf:
            return wf.getnframes() / wf.getframerate()
    except Exception:
        return None


def ffprobe_duration(file_path: str):
    """Robust duration probe via ffprobe (works for mp3/m4a/aac/wav). Returns float or None."""
    try:
        out = subprocess.run(
            ["ffprobe", "-v", "error", "-show_entries", "format=duration",
             "-of", "default=noprint_wrappers=1:nokey=1", file_path],
            capture_output=True, text=True, timeout=30,
        )
        return float(out.stdout.strip()) if out.returncode == 0 else None
    except Exception:
        return None


def ffmpeg_split_audio(file_path: str, chunk_seconds: int, out_dir: str):
    """Split audio into fixed-length chunks. Returns (chunks, errors).
    chunks: list of (chunk_path, offset_seconds). errors: list of (index, stderr_tail).

    For mp3 inputs we use `-c copy` to avoid LAME encoder padding (~25-50ms per chunk)
    that otherwise shifts every Gemini timestamp earlier than the true audio position.
    """
    duration = ffprobe_duration(file_path)
    if not duration or duration <= chunk_seconds:
        return [(file_path, 0.0)], []
    chunks, errors = [], []
    n = math.ceil(duration / chunk_seconds)
    is_mp3 = file_path.lower().endswith(".mp3")
    out_ext = "mp3" if is_mp3 else "m4a"
    for i in range(n):
        start = i * chunk_seconds
        out_path = os.path.join(out_dir, f"chunk_{i:03d}.{out_ext}")
        if is_mp3:
            # Stream copy: no re-encode → no added encoder delay; frame-accurate enough
            # for our purposes (mp3 frame ≈ 26ms at 44.1kHz).
            cmd = [
                "ffmpeg", "-y", "-ss", str(start), "-i", file_path,
                "-t", str(chunk_seconds), "-vn", "-c:a", "copy", out_path,
            ]
        else:
            # Non-mp3 input — re-encode is unavoidable. Use AAC in m4a (smaller
            # encoder delay than LAME and Gemini accepts it).
            cmd = [
                "ffmpeg", "-y", "-i", file_path,
                "-ss", str(start), "-t", str(chunk_seconds),
                "-vn", "-ar", "16000", "-ac", "1",
                "-c:a", "aac", "-b:a", "64k", out_path,
            ]
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
        if r.returncode == 0 and os.path.exists(out_path) and os.path.getsize(out_path) > 0:
            chunks.append((out_path, float(start)))
        else:
            errors.append((i, (r.stderr or "")[-400:]))
    return chunks, errors


def fmt_hms(seconds: float) -> str:
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = int(seconds % 60)
    return f"{h:02}:{m:02}:{s:02}"


def srt_time(seconds: float) -> str:
    h = int(seconds // 3600)
    m = int((seconds % 3600) // 60)
    s = int(seconds % 60)
    ms = int((seconds - int(seconds)) * 1000)
    return f"{h:02}:{m:02}:{s:02},{ms:03}"


# ─── transcription modes ──────────────────────────────────────────────────────

def transcribe_gemini(audio_path: str, language: str, api_key: str,
                      gemini_model: str, chunk_minutes: int = 10,
                      tmp_root: str | None = None) -> list:
    """Transcribe using Google Gemini API with audio chunking for long files."""
    import google.generativeai as genai
    genai.configure(api_key=api_key)

    duration = ffprobe_duration(audio_path)
    if duration:
        emit_log(f"audio duration: {duration:.1f}s ({duration/60:.1f} min)")
    else:
        emit_log("WARNING: could not determine audio duration via ffprobe")

    chunk_seconds = max(60, chunk_minutes * 60)
    if tmp_root:
        os.makedirs(tmp_root, exist_ok=True)
        chunk_dir = tempfile.mkdtemp(prefix="podcast_chunks_", dir=tmp_root)
    else:
        chunk_dir = tempfile.mkdtemp(prefix="podcast_chunks_")
    emit_log(f"splitting into chunks of {chunk_seconds}s in {chunk_dir}")
    chunks, split_errors = ffmpeg_split_audio(audio_path, chunk_seconds, chunk_dir)
    total = len(chunks)
    emit_log(f"split done: {total} chunks created, {len(split_errors)} ffmpeg failures")
    for i, err in split_errors:
        emit_log(f"  ffmpeg fail chunk {i}: {err[:200]}")
    for cp, off in chunks:
        sz = os.path.getsize(cp) if os.path.exists(cp) else -1
        emit_log(f"  chunk: {os.path.basename(cp)} offset={off}s size={sz}B")

    lang_hint = "Japanese" if language == "ja" else "auto-detect"
    prompt = f"""Transcribe this audio file to text.
Return ONLY a JSON array of segments — no markdown, no prose.
Each segment: {{"start": <seconds>, "end": <seconds>, "text": "..."}}.

Important:
- Each segment 5-15 seconds long.
- Timestamps RELATIVE to the start of THIS audio file (which may be a chunk).
- Language: {lang_hint}
"""
    generation_config = {
        "response_mime_type": "application/json",
        "response_schema": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "start": {"type": "number"},
                    "end": {"type": "number"},
                    "text": {"type": "string"},
                },
                "required": ["start", "end", "text"],
            },
        },
    }
    model = genai.GenerativeModel(gemini_model, generation_config=generation_config)

    all_segments: list = []
    for idx, (chunk_path, offset) in enumerate(chunks, start=1):
        base_pct = int(((idx - 1) / total) * 90) + 5
        next_pct = int((idx / total) * 90) + 5
        audio_file = None
        try:
            emit_log(f"[chunk {idx}/{total}] uploading offset={offset}s")
            emit_progress(idx, total, f"📤 Upload chunk {idx}/{total}...", percent=base_pct)
            t0 = time.time()
            audio_file = genai.upload_file(chunk_path)
            emit_log(f"[chunk {idx}] uploaded in {time.time()-t0:.1f}s name={audio_file.name}")

            emit_progress(idx, total, f"🤖 Transcribing chunk {idx}/{total}...", percent=base_pct + 2)
            t1 = time.time()
            response = model.generate_content([prompt, audio_file])
            emit_log(f"[chunk {idx}] generate_content in {time.time()-t1:.1f}s")
            try:
                fr = response.candidates[0].finish_reason
                emit_log(f"[chunk {idx}] finish_reason={fr}")
            except Exception:
                pass

            try:
                response_text = response.text.strip()
            except Exception as e:
                emit_log(f"[chunk {idx}] response.text unavailable: {e}")
                continue

            try:
                segs = parse_gemini_segments(response_text)
            except (json.JSONDecodeError, ValueError) as e:
                emit_log(f"[chunk {idx}] JSON parse failed ({e}); salvaging segments")
                segs = salvage_segments(response_text)

            if not segs:
                emit_log(f"[chunk {idx}] NO SEGMENTS recovered. raw[:500]={response_text[:500]}")
                continue

            for s in segs:
                all_segments.append({
                    "start": float(s["start"]) + offset,
                    "end": float(s["end"]) + offset,
                    "text": s.get("text", ""),
                })
            emit_log(f"[chunk {idx}] +{len(segs)} segments (total={len(all_segments)})")
        except Exception as e:
            emit_log(f"[chunk {idx}] EXCEPTION {type(e).__name__}: {e}")
            emit_log(traceback.format_exc())
        finally:
            if audio_file is not None:
                try: genai.delete_file(audio_file.name)
                except Exception: pass
            if total > 1 and chunk_path != audio_path:
                try: os.remove(chunk_path)
                except Exception: pass
            emit_progress(idx, total,
                          f"✅ Chunk {idx}/{total} done ({len(all_segments)} segments)",
                          percent=next_pct)

    if total > 1:
        try: os.rmdir(chunk_dir)
        except Exception: pass

    all_segments.sort(key=lambda s: s["start"])
    emit_log(f"DONE. total segments={len(all_segments)}")
    emit_progress(total, total, f"✅ Done! {len(all_segments)} segments", percent=100)

    if not all_segments:
        raise ValueError("No segments recovered from any chunk. See log events for details.")
    return all_segments


def transcribe_whisper(audio_path: str, language: str, model_size: str) -> list:
    """Transcribe using faster-whisper. Returns list of segment dicts."""
    from faster_whisper import WhisperModel

    # Detect device for Apple Silicon
    if "arm" in platform.machine().lower() or "mac" in platform.platform().lower():
        device = "cpu"
        compute_type = "int8"
    else:
        device = "auto"
        compute_type = "float16"

    emit_progress(1, 0, f"🔧 Loading Whisper model '{model_size}' (first time downloads ~500MB)...", percent=3)
    model = WhisperModel(model_size, device=device, compute_type=compute_type)
    emit_progress(1, 0, "✅ Model ready. Starting transcription...", percent=5)

    audio_duration = get_audio_duration(audio_path)

    whisper_lang = None if language == "auto" else language
    segments_gen, _ = model.transcribe(audio_path, language=whisper_lang)

    segments = []
    for seg in segments_gen:
        segments.append({
            "start": seg.start,
            "end": seg.end,
            "text": seg.text.strip()
        })
        n = len(segments)
        elapsed = fmt_hms(seg.end)
        if audio_duration and audio_duration > 0:
            pct = min(int((seg.end / audio_duration) * 90) + 5, 95)
            total_str = fmt_hms(audio_duration)
            pct_display = min(int((seg.end / audio_duration) * 100), 100)
            label = f"🎤 [{elapsed} / {total_str}] ({pct_display}%) | {n} segments"
        else:
            pct = min(n * 2 + 5, 95)
            label = f"🎤 [{elapsed}] | {n} segments"

        short_text = seg.text.strip()
        display = f"{short_text[:80]}..." if len(short_text) > 80 else short_text
        emit_progress(n, 0, f"📝 [{elapsed}] {display}", percent=pct)

    emit_progress(len(segments), 0, f"✅ Done! {len(segments)} segments transcribed.", percent=100)
    return segments


# ─── cache helpers ─────────────────────────────────────────────────────────────

def write_cache(cache_dir: str, file_hash: str, segments: list, engine: str = ""):
    os.makedirs(cache_dir, exist_ok=True)

    # JSON
    json_path = os.path.join(cache_dir, f"{file_hash}.json")
    with open(json_path, "w", encoding="utf-8") as f:
        json.dump({"engine": engine, "segments": segments}, f, ensure_ascii=False, indent=2)

    # TXT
    txt_path = os.path.join(cache_dir, f"{file_hash}.txt")
    lines = [f"[{fmt_hms(s['start'])} - {fmt_hms(s['end'])}] {s['text']}" for s in segments]
    with open(txt_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))

    # SRT
    srt_path = os.path.join(cache_dir, f"{file_hash}.srt")
    srt_lines = []
    for i, s in enumerate(segments, 1):
        srt_lines.extend([
            str(i),
            f"{srt_time(s['start'])} --> {srt_time(s['end'])}",
            s["text"],
            ""
        ])
    with open(srt_path, "w", encoding="utf-8") as f:
        f.write("\n".join(srt_lines))

    return json_path, txt_path, srt_path


# ─── main ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="CLI audio transcriber")
    parser.add_argument("--file", required=True, help="Path to audio/video file")
    parser.add_argument("--mode", required=True, choices=["gemini", "whisper"])
    parser.add_argument("--model-size", default="small", choices=["small", "medium"])
    parser.add_argument("--language", default="ja", choices=["ja", "auto"])
    parser.add_argument("--api-key", default=None)
    parser.add_argument("--gemini-model", default="gemini-3.5-flash")
    parser.add_argument("--cache-dir", default=".cache_transcripts")
    parser.add_argument("--force-rerun", action="store_true")
    parser.add_argument("--chunk-minutes", type=int, default=10)
    args = parser.parse_args()

    # Set up debug log file
    global _LOG_FILE_PATH
    try:
        os.makedirs(args.cache_dir, exist_ok=True)
        _LOG_FILE_PATH = os.path.join(args.cache_dir, "transcriber_debug.log")
        with open(_LOG_FILE_PATH, "a", encoding="utf-8") as f:
            f.write(f"\n=== {time.strftime('%Y-%m-%d %H:%M:%S')} run started ===\n")
        emit_log(f"debug log: {_LOG_FILE_PATH}")
    except Exception as e:
        emit({"type": "log", "message": f"could not init log file: {e}"})

    # All temp work lives under <cache_dir>/tmp/ so clear_cache wipes it.
    tmp_root = os.path.join(args.cache_dir, "tmp")
    os.makedirs(tmp_root, exist_ok=True)

    # ── Video → audio extraction ────────────────────────────────────────────
    source_path = args.file
    audio_path = source_path
    tmp_audio_path = None  # track temp file for cleanup

    if is_video_file(source_path):
        emit_progress(0, 4, "🎬 Extracting audio from video...", percent=2)
        tmp_audio = tempfile.NamedTemporaryFile(delete=False, suffix=".mp3", dir=tmp_root)
        tmp_audio.close()
        tmp_audio_path = tmp_audio.name
        ok, err = ffmpeg_extract_audio(source_path, tmp_audio_path)
        if not ok:
            emit_error(f"ffmpeg error: {err}")
            _cleanup_temp(tmp_audio_path)
            sys.exit(1)
        audio_path = tmp_audio_path
        emit_progress(0, 4, "✅ Audio extracted.", percent=5)

    try:
        # ── Cache check ─────────────────────────────────────────────────────
        file_hash = hash_file(audio_path)
        cache_json = os.path.join(args.cache_dir, f"{file_hash}.json")

        if os.path.exists(cache_json) and not args.force_rerun:
            with open(cache_json, "r", encoding="utf-8") as f:
                cached = json.load(f)
            if isinstance(cached, list):
                cached_segments, cached_engine = cached, ""
            else:
                cached_segments = cached.get("segments", [])
                cached_engine = cached.get("engine", "")
            emit_result(cached_segments, cached=True, engine=cached_engine or args.mode)
            return

        # ── Transcription ──────────────────────────────────────────────────
        if args.mode == "gemini":
            if not args.api_key:
                emit_error("Gemini API key required. Set it in Settings.")
                sys.exit(1)
            segments = transcribe_gemini(audio_path, args.language, args.api_key,
                                         args.gemini_model, chunk_minutes=args.chunk_minutes,
                                         tmp_root=tmp_root)
        else:
            segments = transcribe_whisper(audio_path, args.language, args.model_size)

        # ── Save cache ─────────────────────────────────────────────────────
        write_cache(args.cache_dir, file_hash, segments, engine=args.mode)
        emit_result(segments, cached=False, engine=args.mode)

    except Exception as e:
        emit_error(str(e))
        sys.exit(1)
    finally:
        # Always clean up temp audio file extracted from video
        _cleanup_temp(tmp_audio_path)


if __name__ == "__main__":
    main()
