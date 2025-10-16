# Podcast Transcriber (Japanese)

Ứng dụng Streamlit giúp chuyển file audio podcast (mp3/m4a/wav/aac) sang text tiếng Nhật, hỗ trợ:

- Click-to-seek transcript
- Export transcript: `.txt`, `.srt`, `.json`
- Cache tự động + force rerun
- Sử dụng Local Whisper hoặc OpenAI API
- Nhúng audio trực tiếp (base64)

---

## 1️⃣ Yêu cầu hệ thống

- Python >= 3.10
- macOS / Windows / Linux
- (Optional) GPU nếu muốn Local Whisper nhanh hơn

---

## 2️⃣ Cài đặt thư viện

Tạo virtual environment:

```bash
python3 -m venv venv
source venv/bin/activate   # Mac/Linux
venv\Scripts\activate      # Windows
````

Cài đặt thư viện cần thiết:

```bash
pip install streamlit openai faster-whisper google-generativeai python-dotenv
pip install streamlit_javascript    # nếu muốn thử phiên bản cũ (không cần base64)
```

> Lưu ý: Nếu không dùng Local Whisper, bạn chỉ cần `openai` và `streamlit`.

---

## 3️⃣ Chạy ứng dụng 

```bash
source venv/bin/activate
streamlit run podcast_transcriber.py --server.maxUploadSize=1024
```
Chạy trực tiếp từ dòng lệnh với giới hạn 1GB

Giao diện Streamlit sẽ mở trên trình duyệt.

* Upload file podcast (mp3/m4a/wav/aac)
* Chọn model (Local Whisper hoặc OpenAI API)
* Click vào transcript để nhảy audio
* Download transcript dưới dạng `.txt`, `.srt`, `.json`

---

## 4️⃣ Tùy chọn cache & force rerun

* App tự động lưu cache tại thư mục `.cache_transcripts` (cùng thư mục project)
* Nếu file podcast đã được phân tích, app sẽ load từ cache → tiết kiệm thời gian
* Nếu muốn chạy lại model, tick **Force rerun**

---

## 5️⃣ Sử dụng file Apple Podcasts đã download

Apple Podcasts lưu file cache audio tại:

```
/Library/Group Containers/243LU875E5.groups.com.apple.podcasts/Library/Cache
```

* Trên Finder: `Go` → `Go to Folder...` → nhập đường dẫn trên
* Trong thư mục Cache, các file audio podcast thường có tên dạng hash (`.mp3`/`.m4a`)
* Bạn có thể **copy file đó** vào project hoặc dùng trực tiếp khi upload vào app

> ⚠️ Không thể load trực tiếp từ thư mục này trong HTML component nếu chưa copy file.
> Nên copy file ra `/tmp` hoặc thư mục khác trước khi dùng trong Streamlit.

---

## 6️⃣ Cấu trúc thư mục project

```
podcast_transcriber/
│
├─ podcast_transcriber.py
├─ README.md
├─ .cache_transcripts/        # cache transcript
└─ venv/                      # virtual environment
```

---

## 7️⃣ Lưu ý quan trọng

* Trên Mac, audio nhúng bằng **base64** để tránh lỗi `NotSupportedError` trong Streamlit component
* Nếu dùng Local Whisper trên M1/M2/M4, app tự chọn `cpu/int8` để tránh lỗi float16 không hỗ trợ

---

## 8️⃣ Tham khảo

* [Streamlit Components](https://docs.streamlit.io/library/components)
* [OpenAI Whisper API](https://platform.openai.com/docs/guides/speech-to-text)
* [Faster Whisper GitHub](https://github.com/guillaumekln/faster-whisper)

