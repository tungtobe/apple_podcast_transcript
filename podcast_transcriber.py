import streamlit as st
import os, tempfile, json, math, hashlib, platform, base64
from pathlib import Path
import openai
import streamlit.components.v1 as components

try:
    from faster_whisper import WhisperModel
    faster_whisper_installed = True
except:
    faster_whisper_installed = False

# --- Streamlit config ---
st.set_page_config(page_title="Podcast → Text", layout="wide")
st.title("🎧 Podcast → Japanese Text")

# --- Sidebar options ---
st.sidebar.header("⚙️ Settings")
mode = st.sidebar.radio("Model AI", ["Local Whisper", "OpenAI API"])
model_size = st.sidebar.selectbox("Model Whisper (local)", ["small", "medium"])
force_rerun = st.sidebar.checkbox("Chạy lại dù đã có cache", value=False)
language = st.sidebar.selectbox("Ngôn ngữ", ["ja", "auto"], index=0)

# OpenAI API key check
if mode == "OpenAI API":
    openai.api_key = os.getenv("OPENAI_API_KEY")
    if not openai.api_key:
        st.warning("⚠️ OPENAI_API_KEY chưa được set trong môi trường")

# --- File upload ---
uploaded_file = st.file_uploader("📂 Chọn file podcast (mp3, m4a...)", type=["mp3","m4a","wav","aac"])
if uploaded_file is not None:
    temp_audio = tempfile.NamedTemporaryFile(delete=False, suffix=".mp3")
    temp_audio.write(uploaded_file.read())
    temp_audio.flush()

    # --- Cache setup ---
    CACHE_DIR = ".cache_transcripts"
    os.makedirs(CACHE_DIR, exist_ok=True)

    def hash_file(file_path):
        BUF_SIZE = 65536
        sha256 = hashlib.sha256()
        with open(file_path, "rb") as f:
            while True:
                data = f.read(BUF_SIZE)
                if not data:
                    break
                sha256.update(data)
        return sha256.hexdigest()

    file_hash = hash_file(temp_audio.name)
    cache_json = os.path.join(CACHE_DIR, f"{file_hash}.json")
    cache_txt  = os.path.join(CACHE_DIR, f"{file_hash}.txt")
    cache_srt  = os.path.join(CACHE_DIR, f"{file_hash}.srt")

    use_cache = os.path.exists(cache_json) and not force_rerun

    if use_cache:
        with open(cache_json, "r", encoding="utf-8") as f:
            result = json.load(f)
        st.info("📌 Sử dụng transcript đã cache, không chạy model lại.")
    else:
        st.info("⚡ Chạy nhận dạng mới...")
        with st.spinner("Đang nhận dạng, vui lòng chờ..."):
            result = {"segments": []}
            if mode == "OpenAI API":
                with open(temp_audio.name, "rb") as f:
                    transcript = openai.audio.transcriptions.create(
                        model="gpt-4o-mini-transcribe",
                        file=f,
                        response_format="verbose_json",
                        language=language
                    )
                result["segments"] = transcript["segments"]
            else:
                if not faster_whisper_installed:
                    st.error("⚠️ faster-whisper chưa cài đặt. Chạy: pip install faster-whisper")
                else:
                    if "arm" in platform.machine().lower() or "mac" in platform.platform().lower():
                        device = "cpu"
                        compute_type = "int8"
                    else:
                        device = "auto"
                        compute_type = "float16"
                    model = WhisperModel(model_size, device=device, compute_type=compute_type)
                    segments, _ = model.transcribe(temp_audio.name, language=language)
                    for seg in segments:
                        result["segments"].append({
                            "start": seg.start,
                            "end": seg.end,
                            "text": seg.text.strip()
                        })

            # --- Save cache ---
            with open(cache_json, "w", encoding="utf-8") as f:
                json.dump(result, f, ensure_ascii=False, indent=2)

            # TXT cache
            txt_content = "\n".join([f"[{math.floor(s['start'])}s - {math.floor(s['end'])}s] {s['text']}" for s in result["segments"]])
            with open(cache_txt, "w", encoding="utf-8") as f:
                f.write(txt_content)

            # SRT cache
            def srt_time_format(seconds):
                h = int(seconds // 3600)
                m = int((seconds % 3600) // 60)
                s = int(seconds % 60)
                ms = int((seconds - int(seconds)) * 1000)
                return f"{h:02}:{m:02}:{s:02},{ms:03}"

            srt_lines = []
            for i, s in enumerate(result["segments"], 1):
                srt_lines.append(str(i))
                srt_lines.append(f"{srt_time_format(s['start'])} --> {srt_time_format(s['end'])}")
                srt_lines.append(s["text"])
                srt_lines.append("")
            srt_content = "\n".join(srt_lines)
            with open(cache_srt, "w", encoding="utf-8") as f:
                f.write(srt_content)

            st.success("✅ Nhận dạng hoàn tất!")

    # --- Convert audio to base64 ---
    def get_audio_base64(file_path):
        with open(file_path, "rb") as f:
            data = f.read()
        return base64.b64encode(data).decode("utf-8")

    audio_base64 = get_audio_base64(temp_audio.name)
    transcript_html = f'<audio id="audio" controls src="data:audio/mp3;base64,{audio_base64}"></audio><div style="margin-top:10px;">'

    # --- Clickable transcript ---
    for i, seg in enumerate(result["segments"]):
        start = round(seg["start"], 1)
        text = seg["text"].replace("\n"," ")
        transcript_html += f"""
        <p style="cursor:pointer;color:blue;margin:2px 0;" 
           onclick="document.getElementById('audio').currentTime={start}; document.getElementById('audio').play();">
           ▶️ [{start:.1f}s] {text}
        </p>
        """
    transcript_html += "</div>"

    components.html(transcript_html, height=400, scrolling=True)

    # --- Download buttons ---
    st.markdown("### 💾 Xuất transcript")
    st.download_button("📄 Tải transcript (.txt)", open(cache_txt).read(), file_name="transcript.txt", mime="text/plain")
    st.download_button("🎬 Tải phụ đề (.srt)", open(cache_srt).read(), file_name="transcript.srt", mime="text/plain")
    st.download_button("💾 Tải transcript (.json)", open(cache_json).read(), file_name="transcript.json", mime="application/json")

else:
    st.info("👆 Hãy upload một file podcast trước khi bắt đầu.")
