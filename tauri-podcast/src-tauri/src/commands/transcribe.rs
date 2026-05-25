use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncBufReadExt;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeSettings {
    pub file_path: String,
    pub mode: String,          // "gemini" | "whisper"
    pub model_size: String,    // "small" | "medium"
    pub language: String,      // "ja" | "auto"
    pub api_key: Option<String>,
    pub gemini_model: Option<String>,
    pub force_rerun: bool,
    pub cache_dir: String,
    #[serde(default = "default_chunk_minutes")]
    pub chunk_minutes: u32,
}

fn default_chunk_minutes() -> u32 { 10 }

/// Start a transcription job.
///
/// Progress is streamed as Tauri events named `transcribe:{job_id}`.
/// Each event payload is a JSON string with `type` field:
///   {"type":"progress","step":N,"total":4,"message":"...","percent":0-100}
///   {"type":"result","segments":[...],"cached":false}
///   {"type":"error","message":"..."}
#[tauri::command]
pub async fn transcribe(
    app: AppHandle,
    job_id: String,
    settings: TranscribeSettings,
) -> Result<(), String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Cannot find resource dir: {e}"))?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot find app data dir: {e}"))?;

    let python = crate::python::resolve_python(&app_data_dir);
    let script = crate::python::resolve_script(&resource_dir, "transcriber.py");

    // Ensure cache dir exists
    if !settings.cache_dir.is_empty() {
        let _ = std::fs::create_dir_all(&settings.cache_dir);
    }

    let mut cmd = tokio::process::Command::new(&python);
    cmd.arg(script.to_str().unwrap_or(""))
        .arg("--file").arg(&settings.file_path)
        .arg("--mode").arg(&settings.mode)
        .arg("--model-size").arg(&settings.model_size)
        .arg("--language").arg(&settings.language)
        .arg("--cache-dir").arg(if settings.cache_dir.is_empty() {
            app_data_dir.join("cache").to_string_lossy().to_string()
        } else {
            settings.cache_dir.clone()
        });

    if let Some(key) = &settings.api_key {
        if !key.is_empty() {
            cmd.arg("--api-key").arg(key);
        }
    }
    if let Some(model) = &settings.gemini_model {
        if !model.is_empty() {
            cmd.arg("--gemini-model").arg(model);
        }
    }
    if settings.force_rerun {
        cmd.arg("--force-rerun");
    }
    let chunk_minutes = if settings.chunk_minutes == 0 { 10 } else { settings.chunk_minutes };
    cmd.arg("--chunk-minutes").arg(chunk_minutes.to_string());

    cmd.stdout(std::process::Stdio::piped())
        // Discard stderr to prevent pipe-buffer deadlock.
        // Python warnings/tracebacks won't block the process.
        .stderr(std::process::Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start Python transcriber: {e}"))?;

    let event_name = format!("transcribe:{job_id}");

    // Stream stdout JSON lines as Tauri events
    if let Some(stdout) = child.stdout.take() {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('{') {
                match serde_json::from_str::<serde_json::Value>(trimmed) {
                    Ok(val) => {
                        let is_final = val.get("type")
                            .and_then(|t| t.as_str())
                            .map(|t| t == "result" || t == "error")
                            .unwrap_or(false);
                        app.emit(&event_name, &val).ok();
                        if is_final {
                            break;
                        }
                    }
                    Err(_) => {
                        // Non-JSON stdout line (e.g. Python import warning) — log only
                        app.emit(
                            &format!("transcribe:log:{job_id}"),
                            serde_json::json!({"line": trimmed}),
                        )
                        .ok();
                    }
                }
            }
        }
    }

    // If Python exited non-zero but never emitted an error event, emit one now
    if let Ok(status) = child.wait().await {
        if !status.success() {
            app.emit(
                &event_name,
                serde_json::json!({
                    "type": "error",
                    "message": format!(
                        "Transcription process exited with code {}",
                        status.code().unwrap_or(-1)
                    )
                }),
            )
            .ok();
        }
    }

    Ok(())
}
