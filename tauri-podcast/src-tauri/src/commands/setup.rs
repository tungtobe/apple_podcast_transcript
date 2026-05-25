use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncBufReadExt;

#[derive(Serialize, Deserialize, Clone)]
pub struct SetupStatus {
    pub python_ok: bool,
    pub python_version: Option<String>,
    pub ffmpeg_ok: bool,
    pub missing_packages: Vec<String>,
}

async fn check_ffmpeg_available() -> bool {
    tokio::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Check Python version, ffmpeg, and required pip packages.
/// Runs python/setup_check.py and returns parsed JSON.
#[tauri::command]
pub async fn check_setup(app: AppHandle) -> Result<SetupStatus, String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Cannot find resource dir: {e}"))?;

    let script = crate::python::resolve_script(&resource_dir, "setup_check.py");
    let script_str = script.to_string_lossy().to_string();

    // Find any available python3 for initial check (before venv exists)
    let app_data = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
    let Some(python) = crate::python::find_python(&app_data) else {
        return Ok(SetupStatus {
            python_ok: false,
            python_version: None,
            ffmpeg_ok: check_ffmpeg_available().await,
            missing_packages: Vec::new(),
        });
    };

    let output = tokio::process::Command::new(&python)
        .arg(&script_str)
        .output()
        .await
        .map_err(|e| format!("Failed to run setup_check.py with '{python}': {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Strip any Python warnings (non-JSON lines)
    let json_line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .ok_or_else(|| {
            format!(
                "No JSON in setup_check output.\nstdout: {stdout}\nstderr: {}",
                String::from_utf8_lossy(&output.stderr)
            )
        })?;

    serde_json::from_str::<SetupStatus>(json_line)
        .map_err(|e| format!("Bad setup_check JSON: {e}\nRaw: {json_line}"))
}

/// Install missing pip packages into the app-managed venv.
/// Streams pip output as "install:progress" events to the frontend.
#[tauri::command]
pub async fn install_deps(app: AppHandle, packages: Vec<String>) -> Result<(), String> {
    if packages.is_empty() {
        return Ok(());
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot find app data dir: {e}"))?;

    // Create venv if it doesn't exist yet
    let venv_dir = app_data_dir.join("venv");
    if !venv_dir.exists() {
        app.emit("install:progress", "⚙️ Creating isolated Python environment...").ok();

        let python = crate::python::find_python(&app_data_dir).ok_or_else(|| {
            "Python 3.10+ is required before packages can be installed. Install Python from https://www.python.org/downloads/macos/ or run `brew install python`, then click Check Again.".to_string()
        })?;
        let status = tokio::process::Command::new(&python)
            .args(["-m", "venv", venv_dir.to_str().unwrap_or("/tmp/venv")])
            .status()
            .await
            .map_err(|e| format!("Failed to create venv: {e}"))?;

        if !status.success() {
            return Err("Failed to create Python virtual environment.".to_string());
        }
        app.emit("install:progress", "✅ Virtual environment created.").ok();
    }

    // Use `venv_python -m pip install` — works regardless of whether pip3/pip binary exists
    let venv_python = app_data_dir.join("venv/bin/python3");

    app.emit(
        "install:progress",
        format!("📦 Installing: {}", packages.join(", ")),
    )
    .ok();

    let mut child = tokio::process::Command::new(&venv_python)
        .arg("-m").arg("pip").arg("install")
        .args(&packages)
        .stdout(std::process::Stdio::piped())
        // Discard stderr to prevent pipe-buffer deadlock
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("pip install failed to start: {e}"))?;

    // Stream stdout lines
    if let Some(stdout) = child.stdout.take() {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            app.emit("install:progress", &line).ok();
        }
    }

    let status = child.wait().await.map_err(|e| e.to_string())?;
    if status.success() {
        app.emit("install:progress", "✅ All packages installed successfully!").ok();
        Ok(())
    } else {
        Err("pip install failed. Check the log above for details.".to_string())
    }
}
