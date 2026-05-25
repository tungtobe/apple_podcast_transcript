use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::AsyncBufReadExt;
use tokio::process::Child;
use tokio::sync::Mutex as AsyncMutex;

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

/// Registry of running transcription jobs, keyed by job_id.
/// - `jobs`: child handle for cancellation; entry removed when job ends.
/// - `finals`: terminal event (result/error/cancelled) buffered so a UI that
///   navigated away during the job can poll once it returns.
#[derive(Default)]
pub struct JobRegistry {
    jobs: Mutex<HashMap<String, Arc<AsyncMutex<Option<Child>>>>>,
    finals: Mutex<HashMap<String, serde_json::Value>>,
}

impl JobRegistry {
    fn insert(&self, job_id: String, slot: Arc<AsyncMutex<Option<Child>>>) {
        self.jobs.lock().unwrap().insert(job_id, slot);
    }
    fn take(&self, job_id: &str) -> Option<Arc<AsyncMutex<Option<Child>>>> {
        self.jobs.lock().unwrap().remove(job_id)
    }
    fn store_final(&self, job_id: &str, value: serde_json::Value) {
        self.finals.lock().unwrap().insert(job_id.to_string(), value);
    }
    fn take_final(&self, job_id: &str) -> Option<serde_json::Value> {
        self.finals.lock().unwrap().remove(job_id)
    }
    fn active_ids(&self) -> Vec<String> {
        self.jobs.lock().unwrap().keys().cloned().collect()
    }
}

/// Start a transcription job.
///
/// Progress is streamed as Tauri events named `transcribe:{job_id}`.
/// Each event payload is a JSON object with a `type` field:
///   {"type":"progress","step":N,"total":4,"message":"...","percent":0-100}
///   {"type":"log","message":"..."}
///   {"type":"result","segments":[...],"cached":false}
///   {"type":"error","message":"..."}
///   {"type":"cancelled"}
#[tauri::command]
pub async fn transcribe(
    app: AppHandle,
    registry: State<'_, JobRegistry>,
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
        .stderr(std::process::Stdio::null())
        // Send SIGKILL automatically if the Child handle is dropped without wait.
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start Python transcriber: {e}"))?;

    let stdout = child.stdout.take();
    let slot: Arc<AsyncMutex<Option<Child>>> = Arc::new(AsyncMutex::new(Some(child)));
    registry.insert(job_id.clone(), slot.clone());

    let event_name = format!("transcribe:{job_id}");
    let mut cancelled = false;

    // Stream stdout JSON lines as Tauri events
    if let Some(stdout) = stdout {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || !trimmed.starts_with('{') {
                        continue;
                    }
                    match serde_json::from_str::<serde_json::Value>(trimmed) {
                        Ok(val) => {
                            let is_final = val.get("type")
                                .and_then(|t| t.as_str())
                                .map(|t| t == "result" || t == "error")
                                .unwrap_or(false);
                            if is_final {
                                registry.store_final(&job_id, val.clone());
                            }
                            app.emit(&event_name, &val).ok();
                            if is_final {
                                break;
                            }
                        }
                        Err(_) => {
                            app.emit(
                                &format!("transcribe:log:{job_id}"),
                                serde_json::json!({"line": trimmed}),
                            )
                            .ok();
                        }
                    }
                }
                // EOF: child stdout closed — either finished or was killed.
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }

    // Reap the child and decide what to emit
    {
        let mut guard = slot.lock().await;
        if let Some(mut child) = guard.take() {
            match child.wait().await {
                Ok(status) => {
                    if !status.success() {
                        // Distinguish cancel (we killed it) from real failure
                        // by checking whether the registry still has the slot
                        // — cancel_transcribe removes it before killing.
                        if registry.take(&job_id).is_none() {
                            cancelled = true;
                        }
                        if cancelled {
                            let payload = serde_json::json!({"type":"cancelled","message":"Transcription cancelled."});
                            registry.store_final(&job_id, payload.clone());
                            app.emit(&event_name, &payload).ok();
                        } else {
                            let payload = serde_json::json!({
                                "type": "error",
                                "message": format!(
                                    "Transcription process exited with code {}",
                                    status.code().unwrap_or(-1)
                                )
                            });
                            registry.store_final(&job_id, payload.clone());
                            app.emit(&event_name, &payload).ok();
                        }
                    }
                }
                Err(e) => {
                    app.emit(
                        &event_name,
                        serde_json::json!({"type":"error","message": format!("wait() failed: {e}")}),
                    ).ok();
                }
            }
        }
    }

    // Ensure registry entry is removed on the normal-finish path too
    registry.take(&job_id);

    Ok(())
}

/// Return the list of job_ids currently running. Used by the UI to resume
/// the progress display after a page navigation.
#[tauri::command]
pub fn list_active_jobs(registry: State<'_, JobRegistry>) -> Vec<String> {
    registry.active_ids()
}

/// Pop the buffered terminal event (result/error/cancelled) for a job, if any.
/// Used by the UI after re-attaching its listener to catch events emitted
/// while no listener was subscribed.
#[tauri::command]
pub fn poll_job_result(
    registry: State<'_, JobRegistry>,
    job_id: String,
) -> Option<serde_json::Value> {
    registry.take_final(&job_id)
}

/// Cancel an in-flight transcription job by killing its Python child process.
#[tauri::command]
pub async fn cancel_transcribe(
    registry: State<'_, JobRegistry>,
    job_id: String,
) -> Result<bool, String> {
    let slot = match registry.take(&job_id) {
        Some(s) => s,
        None => return Ok(false), // already finished or unknown
    };
    let mut guard = slot.lock().await;
    if let Some(child) = guard.as_mut() {
        let _ = child.start_kill();
    }
    Ok(true)
}
