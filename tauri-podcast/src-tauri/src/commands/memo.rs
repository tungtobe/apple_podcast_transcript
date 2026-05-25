use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncBufReadExt;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoRequest {
    pub transcript_json_path: String,
    pub api_key: String,
    pub gemini_model: String,
    pub memo_prompt_template: String,
    pub cache_memo_path: String,
    pub force_rerun: bool,
}

/// Generate a meeting memo using Gemini.
///
/// Progress is streamed as Tauri events named `memo:{job_id}`.
/// Final event payload: {"type":"result","content":"...","cached":false}
///                   or {"type":"error","message":"..."}
#[tauri::command]
pub async fn generate_memo(
    app: AppHandle,
    job_id: String,
    req: MemoRequest,
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
    let script = crate::python::resolve_script(&resource_dir, "memo_generator.py");

    let mut cmd = tokio::process::Command::new(&python);
    cmd.arg(script.to_str().unwrap_or(""))
        .arg("--transcript").arg(&req.transcript_json_path)
        .arg("--api-key").arg(&req.api_key)
        .arg("--model").arg(&req.gemini_model)
        .arg("--prompt-template").arg(&req.memo_prompt_template)
        .arg("--output").arg(&req.cache_memo_path);

    if req.force_rerun {
        cmd.arg("--force-rerun");
    }

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null()); // discard stderr to prevent pipe-buffer deadlock

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start memo generator: {e}"))?;

    let event_name = format!("memo:{job_id}");

    if let Some(stdout) = child.stdout.take() {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('{') {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    app.emit(&event_name, val).ok();
                }
            }
        }
    }

    if let Ok(status) = child.wait().await {
        if !status.success() {
            app.emit(
                &event_name,
                serde_json::json!({
                    "type": "error",
                    "message": format!(
                        "Memo generator exited with code {}",
                        status.code().unwrap_or(-1)
                    )
                }),
            )
            .ok();
        }
    }

    Ok(())
}
