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
import os
import platform
import subprocess
import sys
import tempfile
import wave
from pathlib import Path


# ─── helpers ──────────────────────────────────────────────────────────────────

def emit(obj: dict):
    """Write a JSON event line to stdout and flush immediately."""
    print(json.dumps(obj, ensure_ascii=False), flush=True)


def emit_progress(step: int, total: int, message: str, percent: int = None):
    if percent is None:
        percent = int((step / total) * 100) if total else 0
    emit({"type": "progress", "step": step, "total": total,
          "message": message, "percent": percent})


def emit_result(segments: list, cached: bool = False):
    emit({"type": "result", "segments": segments, "cached": cached})


def emit_error(message: str):
    emit({"type": "error", "message": message})


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

def transcribe_gemini(audio_path: str, language: str, api_key: str, gemini_model: str) -> list:
    """Transcribe using Google Gemini API. Returns list of segment dicts."""
    import google.generativeai as genai
    genai.configure(api_key=api_key)

    # Step 1/4: Upload
    emit_progress(1, 4, "📤 Uploading file to Gemini...", percent=10)
    audio_file = genai.upload_file(audio_path)
    emit_progress(1, 4, "✅ Upload complete.", percent=30)

    # Step 2/4: Transcribe
    emit_progress(2, 4, "🤖 Sending transcription request to Gemini AI...", percent=35)
    model = genai.GenerativeModel(
        gemini_model,
        generation_config={"response_mime_type": "application/json"},
    )
    lang_hint = "Japanese" if language == "ja" else "auto-detect"
    prompt = f"""Transcribe this audio file to text.
Return the result as a JSON array of segments with this exact format:
[{{"start": 0.0, "end": 5.2, "text": "transcribed text"}}, ...]

Important:
- Each segment should be 5-15 seconds long
- Include accurate timestamps in seconds
- Return ONLY valid JSON
- Do not include markdown fences, explanations, or trailing text
- Language: {lang_hint}
"""
    response = model.generate_content([prompt, audio_file])
    emit_progress(2, 4, "✅ Transcription received.", percent=70)

    # Step 3/4: Parse
    emit_progress(3, 4, "🔍 Parsing JSON result...", percent=75)
    response_text = response.text.strip()
    try:
        segments = parse_gemini_segments(response_text)
    except (json.JSONDecodeError, ValueError) as e:
        raise ValueError(f"Failed to parse Gemini JSON response: {e}\nRaw: {response_text[:500]}")

    emit_progress(3, 4, "✅ JSON parsed successfully.", percent=85)

    # Step 4/4: Cleanup
    emit_progress(4, 4, "🧹 Cleaning up Gemini temp file...", percent=90)
    try:
        genai.delete_file(audio_file.name)
    except Exception:
        pass  # Non-fatal

    emit_progress(4, 4, "✅ Done!", percent=100)
    return segments


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

def write_cache(cache_dir: str, file_hash: str, segments: list):
    os.makedirs(cache_dir, exist_ok=True)

    # JSON
    json_path = os.path.join(cache_dir, f"{file_hash}.json")
    with open(json_path, "w", encoding="utf-8") as f:
        json.dump({"segments": segments}, f, ensure_ascii=False, indent=2)

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
    args = parser.parse_args()

    # ── Video → audio extraction ────────────────────────────────────────────
    source_path = args.file
    audio_path = source_path
    tmp_audio_path = None  # track temp file for cleanup

    if is_video_file(source_path):
        emit_progress(0, 4, "🎬 Extracting audio from video...", percent=2)
        tmp_audio = tempfile.NamedTemporaryFile(delete=False, suffix=".mp3")
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
            emit_result(cached.get("segments", cached if isinstance(cached, list) else []), cached=True)
            return

        # ── Transcription ──────────────────────────────────────────────────
        if args.mode == "gemini":
            if not args.api_key:
                emit_error("Gemini API key required. Set it in Settings.")
                sys.exit(1)
            segments = transcribe_gemini(audio_path, args.language, args.api_key, args.gemini_model)
        else:
            segments = transcribe_whisper(audio_path, args.language, args.model_size)

        # ── Save cache ─────────────────────────────────────────────────────
        write_cache(args.cache_dir, file_hash, segments)
        emit_result(segments, cached=False)

    except Exception as e:
        emit_error(str(e))
        sys.exit(1)
    finally:
        # Always clean up temp audio file extracted from video
        _cleanup_temp(tmp_audio_path)


if __name__ == "__main__":
    main()
