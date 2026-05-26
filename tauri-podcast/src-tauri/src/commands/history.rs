use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub file_hash: String,
    pub source_path: String,
    pub source_name: String,
    pub engine: String,
    pub mode: String,
    pub language: String,
    pub model_size: String,
    pub created_at: String,
    pub segment_count: u64,
    pub duration_sec: f64,
    pub has_memo: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedTranscript {
    pub segments: serde_json::Value,
    pub engine: String,
    pub meta: HistoryEntry,
    pub memo: Option<String>,
}

/// Mirror the sanitization in `buildMemoCacheKey` ([main.js]) so we can locate
/// memo files written for a given source name. Logic: strip extension, replace
/// non-`[a-zA-Z0-9._-]` runs with `-`, trim leading/trailing `-`, max 80 chars.
fn sanitize_for_memo_key(file_name: &str) -> String {
    let stem = match file_name.rfind('.') {
        Some(i) => &file_name[..i],
        None => file_name,
    };
    let mut out = String::with_capacity(stem.len());
    let mut prev_dash = false;
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    let cut: String = trimmed.chars().take(80).collect();
    if cut.is_empty() { "transcript".to_string() } else { cut }
}

fn mtime_iso(path: &Path) -> String {
    let st = match fs::metadata(path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return String::new(),
    };
    let dur = st.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs() as i64;
    // Minimal ISO-8601 UTC formatter without pulling chrono.
    let (year, month, day, hh, mm, ss) = epoch_to_ymdhms(secs);
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, hh, mm, ss)
}

fn epoch_to_ymdhms(mut secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let ss = (secs % 60) as u32;
    secs /= 60;
    let mm = (secs % 60) as u32;
    secs /= 60;
    let hh = (secs % 24) as u32;
    let mut days = secs / 24;
    let mut year: i32 = 1970;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let yd = if leap { 366 } else { 365 };
        if days >= yd { days -= yd; year += 1; } else { break; }
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let mdays = [31u32, if leap {29} else {28}, 31,30,31,30,31,31,30,31,30,31];
    let mut month: u32 = 1;
    let mut d = days as u32;
    for &md in &mdays {
        if d >= md { d -= md; month += 1; } else { break; }
    }
    (year, month, d + 1, hh, mm, ss)
}

fn has_memo_for(cache_dir: &Path, source_name: &str) -> bool {
    if source_name.is_empty() { return false; }
    let key = sanitize_for_memo_key(source_name);
    let prefix = format!("{}-", key);
    let Ok(rd) = fs::read_dir(cache_dir) else { return false; };
    for e in rd.flatten() {
        let n = e.file_name();
        let Some(name) = n.to_str() else { continue; };
        if name.starts_with(&prefix) && name.ends_with(".memo.txt") {
            return true;
        }
    }
    false
}

fn read_latest_memo(cache_dir: &Path, source_name: &str) -> Option<String> {
    if source_name.is_empty() { return None; }
    let key = sanitize_for_memo_key(source_name);
    let prefix = format!("{}-", key);
    let mut best: Option<(SystemTime, PathBuf)> = None;
    let rd = fs::read_dir(cache_dir).ok()?;
    for e in rd.flatten() {
        let n = e.file_name();
        let Some(name) = n.to_str() else { continue; };
        if !(name.starts_with(&prefix) && name.ends_with(".memo.txt")) { continue; }
        let path = e.path();
        let mtime = fs::metadata(&path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
        if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
            best = Some((mtime, path));
        }
    }
    best.and_then(|(_, p)| fs::read_to_string(p).ok())
}

#[tauri::command]
pub fn list_transcripts(cache_dir: String) -> Result<Vec<HistoryEntry>, String> {
    let dir = Path::new(&cache_dir);
    if !dir.exists() { return Ok(vec![]); }

    let mut entries: Vec<HistoryEntry> = Vec::new();
    let mut hashes_with_meta: std::collections::HashSet<String> = Default::default();

    let rd = fs::read_dir(dir).map_err(|e| format!("Cannot read cache dir: {e}"))?;
    let files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();

    // First pass: meta files
    for path in &files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue; };
        if !name.ends_with(".meta.json") { continue; }
        let Ok(text) = fs::read_to_string(path) else { continue; };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue; };
        let file_hash = v.get("file_hash").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if file_hash.is_empty() { continue; }
        // Ensure corresponding transcript still exists
        let tj = dir.join(format!("{}.json", file_hash));
        if !tj.exists() { continue; }
        let source_name = v.get("source_name").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let entry = HistoryEntry {
            file_hash: file_hash.clone(),
            source_path: v.get("source_path").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            source_name: source_name.clone(),
            engine: v.get("engine").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            mode: v.get("mode").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            language: v.get("language").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            model_size: v.get("model_size").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            created_at: v.get("created_at").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            segment_count: v.get("segment_count").and_then(|x| x.as_u64()).unwrap_or(0),
            duration_sec: v.get("duration_sec").and_then(|x| x.as_f64()).unwrap_or(0.0),
            has_memo: has_memo_for(dir, &source_name),
        };
        hashes_with_meta.insert(file_hash);
        entries.push(entry);
    }

    // Second pass: legacy .json without .meta.json
    for path in &files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue; };
        if !name.ends_with(".json") || name.ends_with(".meta.json") || name.ends_with(".transcript.json") {
            continue;
        }
        let file_hash = name.trim_end_matches(".json").to_string();
        // Skip obvious non-hash names (transcript hashes here are sha256 hex = 64 chars,
        // but we accept anything that isn't already tracked and isn't a memo sidecar).
        if hashes_with_meta.contains(&file_hash) { continue; }
        // Try to read engine/segment_count for fallback display
        let (engine, segment_count) = match fs::read_to_string(path).ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        {
            Some(v) => {
                let segs = v.get("segments").and_then(|s| s.as_array()).map(|a| a.len() as u64).unwrap_or(0);
                let eng = v.get("engine").and_then(|s| s.as_str()).unwrap_or("").to_string();
                (eng, segs)
            }
            None => (String::new(), 0),
        };
        entries.push(HistoryEntry {
            file_hash,
            source_path: String::new(),
            source_name: "Unknown".to_string(),
            engine,
            mode: String::new(),
            language: String::new(),
            model_size: String::new(),
            created_at: mtime_iso(path),
            segment_count,
            duration_sec: 0.0,
            has_memo: false,
        });
    }

    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(entries)
}

#[tauri::command]
pub fn load_transcript(cache_dir: String, file_hash: String) -> Result<LoadedTranscript, String> {
    let dir = Path::new(&cache_dir);
    let tj_path = dir.join(format!("{}.json", file_hash));
    let text = fs::read_to_string(&tj_path)
        .map_err(|e| format!("Cannot read transcript: {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Cannot parse transcript: {e}"))?;
    let (segments, engine) = if v.is_array() {
        (v, String::new())
    } else {
        let segs = v.get("segments").cloned().unwrap_or(serde_json::Value::Array(vec![]));
        let eng = v.get("engine").and_then(|s| s.as_str()).unwrap_or("").to_string();
        (segs, eng)
    };

    // Load meta sidecar (best-effort)
    let meta_path = dir.join(format!("{}.meta.json", file_hash));
    let meta: HistoryEntry = match fs::read_to_string(&meta_path).ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    {
        Some(mv) => HistoryEntry {
            file_hash: file_hash.clone(),
            source_path: mv.get("source_path").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            source_name: mv.get("source_name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            engine: mv.get("engine").and_then(|x| x.as_str()).unwrap_or(&engine).to_string(),
            mode: mv.get("mode").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            language: mv.get("language").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            model_size: mv.get("model_size").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            created_at: mv.get("created_at").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            segment_count: mv.get("segment_count").and_then(|x| x.as_u64()).unwrap_or(0),
            duration_sec: mv.get("duration_sec").and_then(|x| x.as_f64()).unwrap_or(0.0),
            has_memo: false,
        },
        None => HistoryEntry {
            file_hash: file_hash.clone(),
            engine: engine.clone(),
            ..Default::default()
        },
    };

    let memo = read_latest_memo(dir, &meta.source_name);

    Ok(LoadedTranscript { segments, engine, meta, memo })
}

#[tauri::command]
pub fn delete_transcript(
    cache_dir: String,
    file_hash: String,
    source_name: String,
) -> Result<u32, String> {
    let dir = Path::new(&cache_dir);
    let mut count = 0u32;
    for ext in &["json", "txt", "srt", "meta.json"] {
        let p = dir.join(format!("{}.{}", file_hash, ext));
        if p.exists() && fs::remove_file(&p).is_ok() { count += 1; }
    }
    if !source_name.is_empty() {
        let key = sanitize_for_memo_key(&source_name);
        let prefix = format!("{}-", key);
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let n = e.file_name();
                let Some(name) = n.to_str() else { continue; };
                if name.starts_with(&prefix)
                    && (name.ends_with(".memo.txt") || name.ends_with(".transcript.json"))
                {
                    if fs::remove_file(e.path()).is_ok() { count += 1; }
                }
            }
        }
    }
    Ok(count)
}
