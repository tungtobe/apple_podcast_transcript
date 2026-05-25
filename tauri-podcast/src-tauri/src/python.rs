use std::path::{Path, PathBuf};

fn default_python_candidates() -> Vec<PathBuf> {
    vec![
        // Homebrew Python (Apple Silicon)
        PathBuf::from("/opt/homebrew/bin/python3"),
        // Homebrew Python (Intel Mac)
        PathBuf::from("/usr/local/bin/python3"),
        // Apple Command Line Tools Python, when available
        PathBuf::from("/usr/bin/python3"),
    ]
}

pub fn find_python(app_data_dir: &Path) -> Option<String> {
    find_python_from_candidates(app_data_dir, &default_python_candidates())
}

fn find_python_from_candidates(app_data_dir: &Path, candidates: &[PathBuf]) -> Option<String> {
    let venv_python = app_data_dir.join("venv/bin/python3");
    if venv_python.exists() {
        return Some(venv_python.to_string_lossy().to_string());
    }

    candidates
        .iter()
        .find(|candidate| candidate.exists())
        .map(|candidate| candidate.to_string_lossy().to_string())
}

/// Returns path to Python executable to use.
///
/// Priority order:
/// 1. App-managed venv (created by install_deps command)
/// 2. Homebrew Python (Apple Silicon: /opt/homebrew/bin, Intel: /usr/local/bin)
/// 3. System Python
pub fn resolve_python(app_data_dir: &PathBuf) -> String {
    find_python(app_data_dir).unwrap_or_else(|| "/usr/bin/python3".to_string())
}

/// Resolve a bundled Python script.
///
/// In development, Tauri's resource_dir can point at target/debug, where the
/// resource copy is not always present. Prefer the source tree when available,
/// then fall back to the bundled resource path for packaged apps.
pub fn resolve_script(resource_dir: &Path, script_name: &str) -> PathBuf {
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("python")
        .join(script_name);

    if dev_path.exists() {
        dev_path
    } else {
        resource_dir.join("python").join(script_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_python_from_candidates_returns_none_when_no_candidate_exists() {
        let temp_dir = std::env::temp_dir().join(format!(
            "transcriber-kun-missing-python-{}",
            std::process::id()
        ));
        let missing_candidate = temp_dir.join("missing-python3");

        let found = find_python_from_candidates(&temp_dir, &[missing_candidate]);

        assert!(found.is_none());
    }

    #[test]
    fn find_python_from_candidates_prefers_app_managed_venv() {
        let temp_dir = std::env::temp_dir().join(format!(
            "transcriber-kun-python-{}",
            std::process::id()
        ));
        let venv_bin = temp_dir.join("venv/bin");
        std::fs::create_dir_all(&venv_bin).unwrap();
        let venv_python = venv_bin.join("python3");
        std::fs::write(&venv_python, "").unwrap();

        let fallback_python = temp_dir.join("fallback-python3");
        std::fs::write(&fallback_python, "").unwrap();

        let found = find_python_from_candidates(&temp_dir, &[fallback_python]);

        assert_eq!(found, Some(venv_python.to_string_lossy().to_string()));

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
