# Transcriber-kun (Apple Podcast Transcript)

Ứng dụng desktop (Tauri + Rust + Python) để transcribe podcast/audio/video sang text, hỗ trợ tiếng Nhật và auto-detect. Có 2 engine:

- **Gemini API** (cloud, nhanh) — tự động chia audio dài thành chunk
- **Local Whisper** (offline, riêng tư) — `faster-whisper`

Tính năng:

- Click-to-seek transcript
- Export `.txt`, `.srt`, `.json`
- Cache theo SHA-256, có force rerun
- Sinh memo/議事録 từ transcript (Gemini)
- Debug log panel cho Gemini chunking

---

## 1. Yêu cầu

- macOS / Linux / Windows
- Python ≥ 3.10
- `ffmpeg` (cắt audio, extract từ video)
  - macOS: `brew install ffmpeg`
  - Linux: `apt install ffmpeg`
- Rust toolchain (để dev/build Tauri)
- Node ≥ 18 (Tauri CLI)

---

## 2. Cài đặt

```bash
cd tauri-podcast
./setup.sh          # cài Python deps + Tauri CLI
```

Hoặc thủ công:

```bash
cd tauri-podcast
python3 -m venv ../venv && source ../venv/bin/activate
pip install -r requirements.txt
npm install
```

`.env` không bắt buộc — API key Gemini cấu hình trong app (Settings → AI Model).

### Lưu ý trên macOS

App mở từ Finder không nhận đầy đủ `PATH` từ shell (`.zshrc`, `.zprofile`), nên app tự dò các vị trí phổ biến của Homebrew, MacPorts, python.org, pyenv/asdf/mise và truyền `PATH` đó cho Python child process.

Nếu Setup vẫn báo Python 3.9.x sau khi đã cài Python mới, nguyên nhân thường là venv cũ ở:

```bash
~/Library/Application Support/com.transcriberkun.app/venv/
```

Chạy lại `./setup.sh` sẽ tự xoá/recreate venv lỗi thời. Hoặc xoá thủ công venv này rồi mở app và bấm **Check Again**.

---

## 3. Chạy

Dev mode:

```bash
cd tauri-podcast
npm run tauri dev
```

Build production:

```bash
npm run tauri build
```

---

## 4. Cấu hình chính trong Settings

| Mục | Ý nghĩa |
|---|---|
| AI Backend | `gemini` hoặc `whisper` |
| Gemini API Key | https://aistudio.google.com/app/apikey |
| Gemini Model | vd `gemini-2.5-flash`, `gemini-3.5-flash` |
| Whisper Model Size | `small` (~500MB) / `medium` (~1.5GB) |
| Language | `ja` / `auto` |
| **Chunk size (minutes)** | Audio dài hơn được cắt thành chunk (Gemini). Mặc định 10 phút |
| Force re-transcribe | Bỏ qua cache |
| Cache Directory | Mặc định `~/Library/Application Support/com.transcriberkun.app/cache/` |

---

## 5. Audio dài → chunking

Audio dài (>10 phút mặc định) được `ffmpeg` cắt thành chunk, upload tuần tự lên Gemini, rồi merge segment với offset thời gian tuyệt đối. Tránh lỗi JSON cắt cụt khi response quá dài.

Nếu một chunk fail (rate limit, safety block, JSON vỡ), chunk khác vẫn ra kết quả; segment fail được salvage qua regex thay vì mất sạch.

**Debug log**: mở panel "🐞 Debug log" dưới progress bar, hoặc tail file `<cache_dir>/transcriber_debug.log`.

---

## 6. Sử dụng file Apple Podcasts đã download

Apple Podcasts cache audio tại:

```
~/Library/Group Containers/243LU875E5.groups.com.apple.podcasts/Library/Cache
```

Finder → Go → Go to Folder → dán đường dẫn. Copy file `.mp3`/`.m4a` ra thư mục khác rồi mở trong app.

---

## 7. Cấu trúc thư mục

```
apple_podcast_transcript/
├─ tauri-podcast/
│  ├─ python/            # transcriber.py, memo_generator.py, ...
│  ├─ src/               # frontend (HTML/CSS/JS)
│  ├─ src-tauri/         # Rust backend
│  ├─ setup.sh
│  ├─ requirements.txt
│  └─ package.json
├─ docs/
└─ README.md
```
