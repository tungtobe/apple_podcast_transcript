use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const SETTINGS_KEY: &str = "app_settings";
const DEFAULT_MEMO_PROMPT_TEMPLATE: &str = r#"あなたは議事録作成のプロフェッショナルです。
会議やポッドキャストの内容から、以下のフォーマットで日本語のメモを作成してください：

## 主な内容
* [トピック1]
   * 詳細なポイント、重要な発言、具体的な内容
   * 関連する情報やメモ
* [トピック2]
   * 詳細なポイント

## Next Action
* 具体的なアクションアイテムがあればリストアップ
* 担当者や期限が言及されていれば記載

## まとめ
全体の要約と重要なポイントを簡潔にまとめる

箇条書きを効果的に使用し、読みやすく構造化してください。

以下のトランスクリプトから議事録メモを作成してください：

{transcript}"#;

fn default_memo_prompt_template() -> String {
    DEFAULT_MEMO_PROMPT_TEMPLATE.to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub ai_mode: String,           // "gemini" | "whisper"
    pub gemini_api_key: String,
    pub gemini_model: String,      // "gemini-3.5-flash" | "gemini-2.5-flash" | etc.
    pub whisper_model_size: String, // "small" | "medium"
    pub language: String,          // "ja" | "auto"
    pub force_rerun: bool,
    pub cache_dir: String,         // empty = use app_data_dir/cache
    #[serde(default = "default_chunk_minutes")]
    pub chunk_minutes: u32,        // Gemini audio chunk size in minutes
    #[serde(default = "default_memo_prompt_template")]
    pub memo_prompt_template: String,
}

fn default_chunk_minutes() -> u32 { 10 }

#[derive(Deserialize)]
struct GeminiModelsResponse {
    models: Option<Vec<String>>,
    error: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ai_mode: "gemini".to_string(),
            gemini_api_key: String::new(),
            gemini_model: "gemini-3.5-flash".to_string(),
            whisper_model_size: "small".to_string(),
            language: "ja".to_string(),
            force_rerun: false,
            cache_dir: String::new(),
            chunk_minutes: default_chunk_minutes(),
            memo_prompt_template: default_memo_prompt_template(),
        }
    }
}

fn normalize_settings(mut settings: AppSettings) -> AppSettings {
    if settings.gemini_model == "gemini-2.0-flash" {
        settings.gemini_model = "gemini-3.5-flash".to_string();
    }
    settings
}

/// Load settings from persistent store. Returns defaults if not yet saved.
#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, String> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("Cannot open settings store: {e}"))?;

    match store.get(SETTINGS_KEY) {
        Some(val) => serde_json::from_value(val.clone())
            .map(normalize_settings)
            .map_err(|e| format!("Corrupt settings: {e}")),
        None => {
            // First launch — resolve default cache dir
            let mut defaults = AppSettings::default();
            if let Ok(app_data) = app.path().app_data_dir() {
                defaults.cache_dir = app_data
                    .join("cache")
                    .to_string_lossy()
                    .to_string();
            }
            Ok(defaults)
        }
    }
}

/// Persist settings to store.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    let store = app
        .store(STORE_FILE)
        .map_err(|e| format!("Cannot open settings store: {e}"))?;

    store.set(SETTINGS_KEY, json!(settings));
    store
        .save()
        .map_err(|e| format!("Failed to save settings: {e}"))
}

/// Return the resolved cache directory (for use when cache_dir is empty).
#[tauri::command]
pub fn get_cache_dir(app: AppHandle) -> Result<String, String> {
    let settings = get_settings(app.clone())?;
    if !settings.cache_dir.is_empty() {
        return Ok(settings.cache_dir);
    }
    app.path()
        .app_data_dir()
        .map(|d| d.join("cache").to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

/// Return Gemini models that support generateContent.
#[tauri::command]
pub async fn list_gemini_models(app: AppHandle, api_key: String) -> Result<Vec<String>, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Gemini API key is required to load models.".to_string());
    }

    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("Cannot find resource dir: {e}"))?;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Cannot find app data dir: {e}"))?;

    let python = crate::python::resolve_python(&app_data_dir);
    let script = crate::python::resolve_script(&resource_dir, "list_gemini_models.py");

    let output = tokio::process::Command::new(&python)
        .arg(script.to_str().unwrap_or(""))
        .env("GEMINI_API_KEY", key)
        .output()
        .await
        .map_err(|e| format!("Failed to start Gemini model loader: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let response_line = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
        .ok_or_else(|| {
            format!(
                "No JSON in Gemini model loader output.\nstdout: {stdout}\nstderr: {stderr}"
            )
        })?;

    let response: GeminiModelsResponse = serde_json::from_str(response_line)
        .map_err(|e| format!("Invalid Gemini model loader output: {e}"))?;

    if !output.status.success() {
        return Err(response.error.unwrap_or_else(|| {
            format!(
                "Gemini model loader exited with code {}",
                output.status.code().unwrap_or(-1)
            )
        }));
    }

    if let Some(error) = response.error {
        return Err(error);
    }

    Ok(response.models.unwrap_or_default())
}
