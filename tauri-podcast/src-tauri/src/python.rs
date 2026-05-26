use std::path::{Path, PathBuf};
use std::process::Command;

const MIN_MINOR: u32 = 10;
// Newer versions first — we prefer the latest available
const VERSIONS: &[&str] = &["3.14", "3.13", "3.12", "3.11", "3.10"];

fn default_python_candidates() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    // Versioned binaries (Homebrew + python.org installer)
    for v in VERSIONS {
        // Homebrew Apple Silicon
        paths.push(PathBuf::from(format!("/opt/homebrew/bin/python{v}")));
        // Homebrew Intel
        paths.push(PathBuf::from(format!("/usr/local/bin/python{v}")));
        // python.org installer
        paths.push(PathBuf::from(format!(
            "/Library/Frameworks/Python.framework/Versions/{v}/bin/python3"
        )));
    }

    // Generic python3 fallbacks
    paths.push(PathBuf::from("/opt/homebrew/bin/python3"));
    paths.push(PathBuf::from("/usr/local/bin/python3"));
    // System Python (3.9 on macOS — usually too old, kept as last resort)
    paths.push(PathBuf::from("/usr/bin/python3"));

    paths
}

/// Return (major, minor) by running `python --version`. None if unrunnable.
fn python_version(path: &Path) -> Option<(u32, u32)> {
    let out = Command::new(path).arg("--version").output().ok()?;
    let text = if !out.stdout.is_empty() {
        String::from_utf8_lossy(&out.stdout)
    } else {
        String::from_utf8_lossy(&out.stderr)
    };
    // Format: "Python 3.12.4"
    let v = text.trim().strip_prefix("Python ")?;
    let mut parts = v.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

pub fn find_python(app_data_dir: &Path) -> Option<String> {
    find_python_from_candidates(app_data_dir, &default_python_candidates())
}

fn find_python_from_candidates(app_data_dir: &Path, candidates: &[PathBuf]) -> Option<String> {
    let venv_python = app_data_dir.join("venv/bin/python3");
    if venv_python.exists() {
        return Some(venv_python.to_string_lossy().to_string());
    }

    // Prefer a candidate whose version is >= 3.10
    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        if let Some((major, minor)) = python_version(candidate) {
            if major > 3 || (major == 3 && minor >= MIN_MINOR) {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }

    // Nothing >= 3.10 found — return the first existing candidate so the UI
    // can report the version it did find (e.g. 3.9.x) instead of "no python".
    candidates
        .iter()
        .find(|c| c.exists())
        .map(|c| c.to_string_lossy().to_string())
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
