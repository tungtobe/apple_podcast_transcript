use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

const MIN_MINOR: u32 = 10;
// Newer versions first — we prefer the latest available
const VERSIONS: &[&str] = &["3.14", "3.13", "3.12", "3.11", "3.10"];

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.as_os_str().is_empty() || paths.iter().any(|existing| existing == &path) {
        return;
    }
    paths.push(path);
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

pub fn tool_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for dir in [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/opt/local/bin",
        "/opt/local/sbin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        push_unique(&mut dirs, PathBuf::from(dir));
    }

    if let Some(home) = home_dir() {
        push_unique(&mut dirs, home.join(".pyenv/shims"));
        push_unique(&mut dirs, home.join(".pyenv/bin"));
        push_unique(&mut dirs, home.join(".asdf/shims"));
        push_unique(&mut dirs, home.join(".local/share/mise/shims"));
        push_unique(&mut dirs, home.join(".local/bin"));
    }

    for v in VERSIONS {
        push_unique(
            &mut dirs,
            PathBuf::from(format!("/opt/homebrew/opt/python@{v}/bin")),
        );
        push_unique(
            &mut dirs,
            PathBuf::from(format!("/usr/local/opt/python@{v}/bin")),
        );
    }

    if let Some(existing_path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&existing_path) {
            push_unique(&mut dirs, dir);
        }
    }

    dirs
}

pub fn tool_path_env() -> OsString {
    std::env::join_paths(tool_search_dirs())
        .unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}

pub fn tool_candidates(binary: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for dir in tool_search_dirs() {
        push_unique(&mut paths, dir.join(binary));
    }
    push_unique(&mut paths, PathBuf::from(binary));
    paths
}

fn default_python_candidates() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    // Versioned binaries (Homebrew + python.org installer)
    for v in VERSIONS {
        // Homebrew Apple Silicon
        paths.push(PathBuf::from(format!("/opt/homebrew/bin/python{v}")));
        // Homebrew Intel
        paths.push(PathBuf::from(format!("/usr/local/bin/python{v}")));
        // Homebrew versioned formulas when they are not linked into bin
        paths.push(PathBuf::from(format!(
            "/opt/homebrew/opt/python@{v}/bin/python{v}"
        )));
        paths.push(PathBuf::from(format!(
            "/opt/homebrew/opt/python@{v}/bin/python3"
        )));
        paths.push(PathBuf::from(format!(
            "/usr/local/opt/python@{v}/bin/python{v}"
        )));
        paths.push(PathBuf::from(format!(
            "/usr/local/opt/python@{v}/bin/python3"
        )));
        // MacPorts
        paths.push(PathBuf::from(format!("/opt/local/bin/python{v}")));
        // python.org installer
        paths.push(PathBuf::from(format!(
            "/Library/Frameworks/Python.framework/Versions/{v}/bin/python3"
        )));
    }

    if let Some(home) = home_dir() {
        paths.push(home.join(".pyenv/shims/python3"));
        paths.push(home.join(".asdf/shims/python3"));
        paths.push(home.join(".local/share/mise/shims/python3"));
    }

    // Generic python3 fallbacks
    for dir in tool_search_dirs() {
        paths.push(dir.join("python3"));
        paths.push(dir.join("python"));
    }

    let mut unique = Vec::new();
    for path in paths {
        push_unique(&mut unique, path);
    }

    unique
}

/// Return (major, minor) by running `python --version`. None if unrunnable.
fn python_version(path: &Path) -> Option<(u32, u32)> {
    let out = Command::new(path)
        .arg("--version")
        .env("PATH", tool_path_env())
        .output()
        .ok()?;
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

pub fn is_supported_python(path: &Path) -> bool {
    python_version(path)
        .map(|(major, minor)| major > 3 || (major == 3 && minor >= MIN_MINOR))
        .unwrap_or(false)
}

pub fn find_python(app_data_dir: &Path) -> Option<String> {
    find_python_from_candidates(app_data_dir, &default_python_candidates())
}

pub fn find_supported_python(app_data_dir: &Path) -> Option<String> {
    find_supported_python_from_candidates(app_data_dir, &default_python_candidates())
}

fn find_supported_python_from_candidates(
    app_data_dir: &Path,
    candidates: &[PathBuf],
) -> Option<String> {
    let venv_python = app_data_dir.join("venv/bin/python3");
    if venv_python.exists() && is_supported_python(&venv_python) {
        return Some(venv_python.to_string_lossy().to_string());
    }

    candidates
        .iter()
        .find(|candidate| candidate.exists() && is_supported_python(candidate))
        .map(|c| c.to_string_lossy().to_string())
}

fn find_python_from_candidates(app_data_dir: &Path, candidates: &[PathBuf]) -> Option<String> {
    if let Some(python) = find_supported_python_from_candidates(app_data_dir, candidates) {
        return Some(python);
    }

    let venv_python = app_data_dir.join("venv/bin/python3");
    if venv_python.exists() {
        return Some(venv_python.to_string_lossy().to_string());
    }

    // Nothing >= 3.10 found — return the first existing candidate so the UI
    // can report the version it did find (e.g. 3.9.x) instead of "no python".
    candidates
        .iter()
        .find(|c| c.exists())
        .map(|c| c.to_string_lossy().to_string())
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
    use std::os::unix::fs::PermissionsExt;

    fn write_mock_python(path: &Path, version: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\necho Python {version}\n")).unwrap();
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

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

    #[test]
    fn find_python_from_candidates_ignores_stale_venv_when_supported_python_exists() {
        let temp_dir = std::env::temp_dir().join(format!(
            "transcriber-kun-stale-venv-{}",
            std::process::id()
        ));
        let venv_python = temp_dir.join("venv/bin/python3");
        write_mock_python(&venv_python, "3.9.6");

        let supported_python = temp_dir.join("python3.11");
        write_mock_python(&supported_python, "3.11.9");

        let found = find_python_from_candidates(&temp_dir, &[supported_python.clone()]);

        assert_eq!(found, Some(supported_python.to_string_lossy().to_string()));

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
