use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncBufReadExt;

#[derive(Serialize, Deserialize, Clone)]
pub struct SetupStatus {
    pub python_ok: bool,
    pub python_version: Option<String>,
    pub python_path: Option<String>,
    pub ffmpeg_ok: bool,
    pub ffmpeg_path: Option<String>,
    pub missing_packages: Vec<String>,
}

async fn find_tool_available(binary: &str) -> Option<PathBuf> {
    let path_env = crate::python::tool_path_env();
    for path in crate::python::tool_candidates(binary) {
        // For absolute paths, verify the file exists first to avoid noisy fork errors
        if path.is_absolute() && !path.exists() {
            continue;
        }
        let ok = tokio::process::Command::new(&path)
            .arg("-version")
            .env("PATH", &path_env)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Some(path);
        }
    }
    None
}

async fn find_ffmpeg_available() -> Option<PathBuf> {
    let ffmpeg = find_tool_available("ffmpeg").await?;
    let _ffprobe = find_tool_available("ffprobe").await?;
    Some(ffmpeg)
}

fn venv_python_path(venv_dir: &Path) -> PathBuf {
    venv_dir.join("bin/python3")
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
        let ffmpeg_path = find_ffmpeg_available().await;
        return Ok(SetupStatus {
            python_ok: false,
            python_version: None,
            python_path: None,
            ffmpeg_ok: ffmpeg_path.is_some(),
            ffmpeg_path: ffmpeg_path.map(|p| p.to_string_lossy().to_string()),
            missing_packages: Vec::new(),
        });
    };

    let output = tokio::process::Command::new(&python)
        .arg(&script_str)
        .env("PATH", crate::python::tool_path_env())
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

    let mut status: SetupStatus = serde_json::from_str(json_line)
        .map_err(|e| format!("Bad setup_check JSON: {e}\nRaw: {json_line}"))?;

    status.python_path = Some(python);

    // Override Python script's ffmpeg check with Rust-side discovery so Finder
    // launches use the same normalized PATH as terminal launches.
    let ffmpeg_path = find_ffmpeg_available().await;
    status.ffmpeg_ok = ffmpeg_path.is_some();
    status.ffmpeg_path = ffmpeg_path.map(|p| p.to_string_lossy().to_string());

    Ok(status)
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
    let venv_python = venv_python_path(&venv_dir);
    if venv_dir.exists() && !crate::python::is_supported_python(&venv_python) {
        app.emit(
            "install:progress",
            "♻️ Recreating outdated Python environment...",
        )
        .ok();
        std::fs::remove_dir_all(&venv_dir)
            .map_err(|e| format!("Failed to remove outdated Python virtual environment: {e}"))?;
    }

    if !venv_dir.exists() {
        app.emit("install:progress", "⚙️ Creating isolated Python environment...").ok();

        let python = crate::python::find_supported_python(&app_data_dir).ok_or_else(|| {
            "Python 3.10+ is required before packages can be installed. Install Python from https://www.python.org/downloads/macos/ or run `brew install python`, then click Check Again.".to_string()
        })?;
        let status = tokio::process::Command::new(&python)
            .args(["-m", "venv", venv_dir.to_str().unwrap_or("/tmp/venv")])
            .env("PATH", crate::python::tool_path_env())
            .status()
            .await
            .map_err(|e| format!("Failed to create venv: {e}"))?;

        if !status.success() {
            return Err("Failed to create Python virtual environment.".to_string());
        }
        app.emit("install:progress", "✅ Virtual environment created.").ok();
    }

    // Use `venv_python -m pip install` — works regardless of whether pip3/pip binary exists
    let venv_python = venv_python_path(&venv_dir);
    if !crate::python::is_supported_python(&venv_python) {
        return Err("Python virtual environment is not Python 3.10+.".to_string());
    }

    app.emit(
        "install:progress",
        format!("📦 Installing: {}", packages.join(", ")),
    )
    .ok();

    // Upgrade pip first to avoid outdated pip errors
    app.emit("install:progress", "⚙️ Upgrading pip...").ok();
    let _ = tokio::process::Command::new(&venv_python)
        .args(["-m", "pip", "install", "--upgrade", "pip"])
        .env("PATH", crate::python::tool_path_env())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    let mut child = tokio::process::Command::new(&venv_python)
        .arg("-m").arg("pip").arg("install")
        .args(&packages)
        .env("PATH", crate::python::tool_path_env())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("pip install failed to start: {e}"))?;

    // Take stdout/stderr before spawning tasks
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    // Stream stdout and stderr concurrently to prevent pipe-buffer deadlock
    let app_stdout = app.clone();
    let app_stderr = app.clone();

    let stdout_task = tokio::spawn(async move {
        if let Some(stdout) = stdout {
            let reader = tokio::io::BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                app_stdout.emit("install:progress", &line).ok();
            }
        }
    });

    // Capture stderr and forward as error lines
    let stderr_lines = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let stderr_lines_clone = stderr_lines.clone();
    let stderr_task = tokio::spawn(async move {
        if let Some(stderr) = stderr {
            let reader = tokio::io::BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    app_stderr.emit("install:progress", format!("⚠️ {line}")).ok();
                    stderr_lines_clone.lock().await.push(line);
                }
            }
        }
    });

    let status = child.wait().await.map_err(|e| e.to_string())?;
    let _ = tokio::join!(stdout_task, stderr_task);

    if status.success() {
        app.emit("install:progress", "✅ All packages installed successfully!").ok();
        Ok(())
    } else {
        let errors = stderr_lines.lock().await.join("\n");
        Err(format!("pip install failed:\n{errors}"))
    }
}
