use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

/// Open a native file picker filtered for audio and video formats.
/// Returns the selected file path or null if cancelled.
#[tauri::command]
pub async fn pick_audio_file(app: AppHandle) -> Result<Option<String>, String> {
    let result = app
        .dialog()
        .file()
        .add_filter(
            "Audio & Video",
            &["mp3", "m4a", "wav", "aac", "mp4", "mov", "avi", "mkv", "webm", "flv", "ts"],
        )
        .blocking_pick_file();

    Ok(result.map(|f| f.to_string()))
}

/// Open a native Save dialog and write content to the chosen path.
/// Returns the saved path or null if cancelled.
#[tauri::command]
pub async fn export_file(
    app: AppHandle,
    content: String,
    default_filename: String,
) -> Result<Option<String>, String> {
    let ext = default_filename
        .rsplit('.')
        .next()
        .unwrap_or("txt")
        .to_string();

    let path = app
        .dialog()
        .file()
        .set_file_name(&default_filename)
        .add_filter("Export", &[ext.as_str()])
        .blocking_save_file();

    if let Some(p) = path {
        let path_str = p.to_string();
        std::fs::write(&path_str, &content)
            .map_err(|e| format!("Failed to write file: {e}"))?;
        Ok(Some(path_str))
    } else {
        Ok(None)
    }
}

/// Open a folder in Finder.
#[tauri::command]
pub async fn open_cache_folder(cache_dir: String) -> Result<(), String> {
    // Create the folder if it doesn't exist yet
    let _ = std::fs::create_dir_all(&cache_dir);

    std::process::Command::new("open")
        .arg(&cache_dir)
        .spawn()
        .map_err(|e| format!("Cannot open Finder: {e}"))?;
    Ok(())
}

/// Read a text file from the cache directory (for transcript export).
#[tauri::command]
pub async fn read_cache_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read file '{path}': {e}"))
}

/// Silently write a JSON string to a file path (no dialog).
/// Used by memo generation to write transcript JSON to a known temp location.
#[tauri::command]
pub async fn write_json_file(path: String, content: String) -> Result<(), String> {
    // Create parent directories if needed
    if let Some(parent) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Cannot create directory: {e}"))?;
    }
    std::fs::write(&path, content.as_bytes())
        .map_err(|e| format!("Cannot write file '{path}': {e}"))
}

/// Clear all files in the cache directory.
#[tauri::command]
pub async fn clear_cache(cache_dir: String) -> Result<u32, String> {
    let dir = std::path::Path::new(&cache_dir);
    if !dir.exists() {
        return Ok(0);
    }

    let mut count = 0u32;
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("Cannot read cache dir: {e}"))?;

    for entry in entries.flatten() {
        if entry.path().is_file() {
            let _ = std::fs::remove_file(entry.path());
            count += 1;
        }
    }
    Ok(count)
}
