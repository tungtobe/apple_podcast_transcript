import streamlit as st
import os, tempfile, json, math, hashlib, platform, base64, wave, struct, subprocess
from pathlib import Path
import google.generativeai as genai
import streamlit.components.v1 as components
from dotenv import load_dotenv

# Load environment variables
load_dotenv()

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
mode = st.sidebar.radio("Model AI", ["Local Whisper", "Gemini API"])
model_size = st.sidebar.selectbox("Model Whisper (local)", ["small", "medium"])
force_rerun = st.sidebar.checkbox("Chạy lại dù đã có cache", value=False)
language = st.sidebar.selectbox("Ngôn ngữ", ["ja", "auto"], index=0)

# Gemini API key check
gemini_available = False
gemini_api_key = os.getenv("GEMINI_API_KEY")
gemini_model = os.getenv("GEMINI_MODEL")
if gemini_api_key:
    genai.configure(api_key=gemini_api_key)
    gemini_available = True
else:
    if mode == "Gemini API":
        st.warning("⚠️ GEMINI_API_KEY chưa được set trong file .env")

# --- Video formats supported ---
VIDEO_FORMATS = ["mp4", "mov", "avi", "mkv", "webm", "flv", "wmv", "m2ts", "ts"]
AUDIO_FORMATS = ["mp3", "m4a", "wav", "aac"]

def is_video_file(filename: str) -> bool:
    return Path(filename).suffix.lstrip(".").lower() in VIDEO_FORMATS

def ffmpeg_extract_audio(video_path: str, out_path: str) -> tuple[bool, str]:
    """Extract audio track from video using ffmpeg. Returns (success, error_msg)."""
    cmd = [
        "ffmpeg", "-y",
        "-i", video_path,
        "-vn",                  # no video
        "-ar", "16000",         # 16kHz — optimal for Whisper
        "-ac", "1",             # mono
        "-c:a", "libmp3lame",
        "-q:a", "4",
        out_path
    ]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
        if result.returncode != 0:
            return False, result.stderr[-1000:]  # last 1000 chars of stderr
        return True, ""
    except FileNotFoundError:
        return False, "ffmpeg không tìm thấy. Hãy cài: brew install ffmpeg"
    except subprocess.TimeoutExpired:
        return False, "ffmpeg timeout sau 5 phút"

# --- File upload ---
uploaded_file = st.file_uploader(
    "📂 Chọn file podcast hoặc video (mp3, m4a, wav, aac, mp4, mov, avi, mkv...)",
    type=AUDIO_FORMATS + VIDEO_FORMATS
)
if uploaded_file is not None:
    file_ext = Path(uploaded_file.name).suffix.lower()  # e.g. ".mp4"

    # Save uploaded file to a temp file with its original extension
    temp_source = tempfile.NamedTemporaryFile(delete=False, suffix=file_ext)
    temp_source.write(uploaded_file.read())
    temp_source.flush()

    # Convert video → audio if needed
    if is_video_file(uploaded_file.name):
        with st.spinner(f"🎬 Đang extract audio từ video `{uploaded_file.name}`..."):
            temp_audio = tempfile.NamedTemporaryFile(delete=False, suffix=".mp3")
            temp_audio.close()  # close so ffmpeg can write to it
            ok, err = ffmpeg_extract_audio(temp_source.name, temp_audio.name)
        if not ok:
            st.error(f"❌ Lỗi convert video: {err}")
            st.stop()
        st.success(f"✅ Extract audio thành công từ `{uploaded_file.name}`")
    else:
        # Audio file — rename temp file to .mp3 for consistent handling downstream
        temp_audio = temp_source

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
    cache_memo = os.path.join(CACHE_DIR, f"{file_hash}_memo.txt")

    use_cache = os.path.exists(cache_json) and not force_rerun

    if use_cache:
        with open(cache_json, "r", encoding="utf-8") as f:
            result = json.load(f)
        st.info("📌 Sử dụng transcript đã cache, không chạy model lại.")
    else:
        st.info("⚡ Chạy nhận dạng mới...")
        result = {"segments": []}

        # --- Helper: get audio duration (seconds) ---
        def get_audio_duration(file_path):
            """Try to get duration via wave (WAV only), else fallback to None."""
            try:
                with wave.open(file_path, 'r') as wf:
                    frames = wf.getnframes()
                    rate = wf.getframerate()
                    return frames / rate
            except Exception:
                return None

        if mode == "Gemini API":
            if not gemini_available:
                st.error("⚠️ GEMINI_API_KEY không tồn tại. Vui lòng thêm vào file .env")
            else:
                # --- Gemini: 4 steps progress ---
                progress_bar = st.progress(0, text="⏳ Bắt đầu xử lý...")
                status_text = st.empty()

                # Step 1: Upload
                status_text.markdown("📤 **Bước 1/4:** Đang upload file lên Gemini...")
                progress_bar.progress(10, text="📤 Đang upload file lên Gemini...")
                audio_file = genai.upload_file(temp_audio.name)
                progress_bar.progress(30, text="✅ Upload xong. Đang gửi yêu cầu nhận dạng...")

                # Step 2: Transcribe
                status_text.markdown("🤖 **Bước 2/4:** Đang gửi yêu cầu nhận dạng tới Gemini AI...")
                model = genai.GenerativeModel(gemini_model)
                prompt = f"""Transcribe this audio file to text. 
                Return the result as a JSON array of segments with this exact format:
                [{{"start": 0.0, "end": 5.2, "text": "transcribed text"}}, ...]
                
                Important:
                - Each segment should be 5-15 seconds long
                - Include accurate timestamps in seconds
                - Return ONLY the JSON array, no other text
                - Language: {"Japanese" if language == "ja" else "auto-detect"}
                """
                response = model.generate_content([prompt, audio_file])
                progress_bar.progress(70, text="✅ Nhận dạng xong. Đang xử lý kết quả...")

                # Step 3: Parse
                status_text.markdown("🔍 **Bước 3/4:** Đang phân tích kết quả JSON...")
                try:
                    response_text = response.text.strip()
                    if response_text.startswith("```"):
                        response_text = response_text.split("```")[1]
                        if response_text.startswith("json"):
                            response_text = response_text[4:]
                    response_text = response_text.strip()
                    segments = json.loads(response_text)
                    result["segments"] = segments
                    progress_bar.progress(85, text="✅ Phân tích JSON thành công!")
                except json.JSONDecodeError:
                    st.error("❌ Lỗi parse JSON từ Gemini response")
                    st.text(response.text)

                # Step 4: Cleanup
                status_text.markdown("🧹 **Bước 4/4:** Đang dọn dẹp file tạm trên Gemini...")
                genai.delete_file(audio_file.name)
                progress_bar.progress(100, text="✅ Hoàn tất nhận dạng Gemini!")
                status_text.empty()

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

                # --- Whisper: segment-level progress ---
                progress_bar = st.progress(0, text="⏳ Đang tải model Whisper...")
                status_text = st.empty()

                status_text.markdown(f"🔧 **Đang tải model Whisper `{model_size}`...**")
                model = WhisperModel(model_size, device=device, compute_type=compute_type)
                progress_bar.progress(5, text="✅ Model đã sẵn sàng. Bắt đầu nhận dạng...")

                # Try to estimate total audio duration for progress
                audio_duration = get_audio_duration(temp_audio.name)

                status_text.markdown("🎤 **Đang nhận dạng giọng nói...** (tiến trình cập nhật theo từng đoạn)")
                segments_gen, _ = model.transcribe(temp_audio.name, language=language)

                for seg in segments_gen:
                    result["segments"].append({
                        "start": seg.start,
                        "end": seg.end,
                        "text": seg.text.strip()
                    })
                    n_segs = len(result["segments"])
                    elapsed_str = f"{int(seg.end // 3600):02d}:{int((seg.end % 3600) // 60):02d}:{int(seg.end % 60):02d}"
                    # Update progress based on timestamp
                    if audio_duration and audio_duration > 0:
                        pct = min(int((seg.end / audio_duration) * 90) + 5, 95)
                        pct_display = min(int((seg.end / audio_duration) * 100), 100)
                        total_str = f"{int(audio_duration // 3600):02d}:{int((audio_duration % 3600) // 60):02d}:{int(audio_duration % 60):02d}"
                        label = f"🎤 [{elapsed_str} / {total_str}] ({pct_display}%) | {n_segs} đoạn"
                    else:
                        pct = min(n_segs * 2 + 5, 95)
                        label = f"🎤 [{elapsed_str}] | {n_segs} đoạn"
                    progress_bar.progress(pct, text=label)
                    short_text = seg.text.strip()
                    display_text = f"{short_text[:80]}..." if len(short_text) > 80 else short_text
                    status_text.markdown(f"📝 **Đoạn {n_segs}:** `[{elapsed_str}]` {display_text}")

                progress_bar.progress(100, text=f"✅ Hoàn tất! {len(result['segments'])} đoạn được nhận dạng.")
                status_text.empty()

        # --- Save cache ---
        with open(cache_json, "w", encoding="utf-8") as f:
            json.dump(result, f, ensure_ascii=False, indent=2)

        # TXT cache
        def fmt_hms(seconds):
            h = int(seconds // 3600)
            m = int((seconds % 3600) // 60)
            s = int(seconds % 60)
            return f"{h:02}:{m:02}:{s:02}"

        txt_content = "\n".join([f"[{fmt_hms(s['start'])} - {fmt_hms(s['end'])}] {s['text']}" for s in result["segments"]])
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
    transcript_html = f'''
        <style>
        #fixed-audio {{
            position: fixed;
            top: 0px;
            left: 0px;
            right: 0px;
            z-index: 9999;
            background: #fff;
            padding: 0px 0 0px 0;
        }}
        body {{
            padding-top: 60px;
        }}
        </style>
        <div id="fixed-audio">
            <audio id="audio" controls src="data:audio/mp3;base64,{audio_base64}"></audio>
        </div>
        <div id="transcript" style="margin-top:10px;">
        '''

    # --- Clickable transcript ---
    def format_time(seconds):
        h = int(seconds // 3600)
        m = int((seconds % 3600) // 60)
        s = int(seconds % 60)
        if h > 0:
            return f"{h:02}:{m:02}:{s:02}"
        else:
            return f"{m:02}:{s:02}"

    for i, seg in enumerate(result["segments"]):
        start = round(seg["start"], 1)
        end = round(seg["end"], 1)
        text = seg["text"].replace("\n"," ")
        time_str = format_time(start)
        transcript_html += f"""
        <p id="seg{i}" data-start="{start}" data-end="{end}" 
           style="cursor:pointer;color:blue;margin:2px 0;" 
           onclick="document.getElementById('audio').currentTime={start}; document.getElementById('audio').play();">
           ▶️ [{time_str}] {text}
        </p>
        """

    transcript_html += """
            </div>
            <script>
        const audio = document.getElementById('audio');
        const segments = Array.from(document.querySelectorAll('#transcript p'));
        let lastActive = null;

        audio.addEventListener('timeupdate', function() {
            const current = audio.currentTime;
            segments.forEach(seg => {
                const start = parseFloat(seg.dataset.start);
                const end = parseFloat(seg.dataset.end);
                if (current >= start && current < end) {
                    if (lastActive !== seg) {
                        // Remove previous highlight
                        if (lastActive) lastActive.style.background = '';
                        // Highlight current line
                        seg.style.background = '#ffe066';
                        seg.scrollIntoView({ behavior: 'smooth', block: 'center' });
                        lastActive = seg;
                    }
                } else {
                    seg.style.background = '';
                }
            });
        });
        </script>
    """

    components.html(transcript_html, height=400, scrolling=True)

    # --- Generate Memo Section ---
    st.markdown("---")
    st.markdown("### 📝 議事録メモ生成")
    
    if gemini_available:
        memo_exists = os.path.exists(cache_memo)
        
        col1, col2 = st.columns([1, 4])
        with col1:
            generate_memo = st.button("📋 メモを生成", disabled=memo_exists and not force_rerun)
        with col2:
            if memo_exists and not force_rerun:
                st.info("✓ メモは既に生成されています")
        
        if generate_memo or (memo_exists and not force_rerun):
            if generate_memo:
                with st.spinner("AIがメモを生成中..."):
                    # Get full transcript text
                    full_text = " ".join([seg["text"] for seg in result["segments"]])
                    
                    # Generate memo using Gemini
                    try:
                        model = genai.GenerativeModel(gemini_model)
                        
                        prompt = f"""あなたは議事録作成のプロフェッショナルです。
会議やポッドキャストの内容から、以下のフォーマットで日本語のメモを作成してください：

## 主な内容
* [トピック1]
   * 詳細なポイント、重要な発言、具体的な内容
   * 関連する情報やメモ
* [トピック2]
   * 詳細なポイント
   
## Next Action
* 具体的なアクションアイテムがあればリストアップ
* 担当者や期限が言及されていれば記載

## まとめ
全体の要約と重要なポイントを簡潔にまとめる

箇条書きを効果的に使用し、読みやすく構造化してください。

以下のトランスクリプトから議事録メモを作成してください：

{full_text}"""
                        
                        response = model.generate_content(prompt)
                        memo_content = response.text
                        
                        # Save memo to cache
                        with open(cache_memo, "w", encoding="utf-8") as f:
                            f.write(memo_content)
                        
                        st.success("✅ メモ生成完了！")
                    except Exception as e:
                        st.error(f"❌ エラーが発生しました: {str(e)}")
                        memo_content = None
            else:
                # Load cached memo
                with open(cache_memo, "r", encoding="utf-8") as f:
                    memo_content = f.read()
            
            if memo_content:
                st.markdown("---")
                st.markdown(memo_content)
                st.download_button(
                    "📥 メモをダウンロード (.txt)", 
                    memo_content, 
                    file_name="meeting_memo.txt", 
                    mime="text/plain"
                )
    else:
        st.warning("⚠️ メモ生成にはGEMINI_API_KEYが必要です (.envファイルに設定してください)")

    # --- Download buttons ---
    st.markdown("---")
    st.markdown("### 💾 Xuất transcript")
    st.download_button("📄 Tải transcript (.txt)", open(cache_txt).read(), file_name="transcript.txt", mime="text/plain")
    st.download_button("🎬 Tải phụ đề (.srt)", open(cache_srt).read(), file_name="transcript.srt", mime="text/plain")
    st.download_button("💾 Tải transcript (.json)", open(cache_json).read(), file_name="transcript.json", mime="application/json")

else:
    st.info("👆 Hãy upload một file podcast trước khi bắt đầu.")