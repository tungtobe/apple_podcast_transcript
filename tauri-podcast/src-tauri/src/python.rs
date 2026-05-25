use std::path::{Path, PathBuf};

/// Returns path to Python executable to use.
///
/// Priority order:
/// 1. App-managed venv (created by install_deps command)
/// 2. Homebrew Python (Apple Silicon: /opt/homebrew/bin, Intel: /usr/local/bin)
/// 3. System Python
pub fn resolve_python(app_data_dir: &PathBuf) -> String {
    // 1. App-managed venv (most isolated, preferred)
    let venv_python = app_data_dir.join("venv/bin/python3");
    if venv_python.exists() {
        return venv_python.to_string_lossy().to_string();
    }

    // 2. Homebrew Python (Apple Silicon)
    let homebrew_arm = PathBuf::from("/opt/homebrew/bin/python3");
    if homebrew_arm.exists() {
        return homebrew_arm.to_string_lossy().to_string();
    }

    // 3. Homebrew Python (Intel Mac)
    let homebrew_intel = PathBuf::from("/usr/local/bin/python3");
    if homebrew_intel.exists() {
        return homebrew_intel.to_string_lossy().to_string();
    }

    // 4. System Python
    "/usr/bin/python3".to_string()
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
