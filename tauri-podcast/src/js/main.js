/* Main transcription page logic */

const { invoke } = window.__TAURI__.core;

// ── State ──────────────────────────────────────────────────────────────────
let settings       = null;
let currentFile    = null;   // absolute path
let currentSegments = null;
let currentFileHash = null;
let currentEngine  = '';     // "gemini" | "whisper" — drives timestamp-offset application
let jobId          = null;
let unlistenFn     = null;
let audioEl        = null;
let lastActive     = null;

function timestampOffset() {
  if (currentEngine !== 'gemini') return 0;
  return parseFloat(settings?.geminiTimestampOffset) || 0;
}

// ── Init ───────────────────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', async () => {
  audioEl = document.getElementById('audio');
  audioEl?.addEventListener('error', () => {
    const err = audioEl.error;
    console.error('[audio] failed to load', {
      file: currentFile,
      src: audioEl.currentSrc || audioEl.src,
      code: err?.code,
      message: err?.message,
    });
    showAlert('Audio preview could not be loaded, but transcript timestamps are still available.', 'warn');
  });

  // Load settings first
  try {
    settings = await invoke('get_settings');
  } catch {
    settings = null;
  }

  // Check setup on every launch
  try {
    const status = await invoke('check_setup');
    if (!status.python_ok || status.missing_packages.length > 0) {
      window.router.go('setup', status);
      return;
    }
  } catch {
    // If check_setup fails (e.g. no Python at all) go to setup wizard
    window.router.go('setup', { python_ok: false, ffmpeg_ok: false, missing_packages: [] });
    return;
  }

  initDropZone();
  await tryResumeActiveJob();
  if (!jobId) renderHistory();
});

// ── History (recent completed transcripts) ────────────────────────────────
async function resolveCacheDir() {
  let cacheDir = settings?.cacheDir || '';
  if (!cacheDir) {
    cacheDir = await invoke('get_cache_dir').catch(() => '');
  }
  return cacheDir;
}

function formatRelativeDate(iso) {
  if (!iso) return '';
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  const diff = (Date.now() - t) / 1000;
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 86400 * 7) return `${Math.floor(diff / 86400)}d ago`;
  const d = new Date(t);
  return d.toLocaleDateString();
}

function formatDuration(sec) {
  if (!sec || sec <= 0) return '';
  const m = Math.floor(sec / 60), s = Math.floor(sec % 60);
  if (m >= 60) {
    const h = Math.floor(m / 60); return `${h}h${m % 60}m`;
  }
  return `${m}m${pad(s)}s`;
}

async function renderHistory() {
  const section = document.getElementById('history-section');
  const list    = document.getElementById('history-list');
  const empty   = document.getElementById('history-empty');
  if (!section || !list) return;
  const cacheDir = await resolveCacheDir();
  let entries = [];
  try { entries = await invoke('list_transcripts', { cacheDir }); }
  catch (err) { console.warn('list_transcripts failed', err); entries = []; }

  list.innerHTML = '';
  if (entries.length === 0) {
    empty.removeAttribute('hidden');
    return;
  }
  empty.setAttribute('hidden', '');

  for (const e of entries) {
    const li = document.createElement('div');
    li.className = 'history-item';
    const dur = formatDuration(e.durationSec);
    const date = formatRelativeDate(e.createdAt);
    const engineLabel = e.engine ? `<span class="badge">${e.engine}</span>` : '';
    const memoLabel = e.hasMemo ? `<span class="badge badge-memo">memo</span>` : '';
    li.innerHTML = `
      <div class="history-item-main">
        <div class="history-item-title" title="${escapeHtml(e.sourcePath || e.sourceName)}">${escapeHtml(e.sourceName || 'Unknown')}</div>
        <div class="history-item-meta">
          ${engineLabel}${memoLabel}
          <span>${e.segmentCount} segs</span>
          ${dur ? `<span>${dur}</span>` : ''}
          <span>${date}</span>
        </div>
      </div>
      <div class="history-item-actions">
        <button class="btn-secondary btn-open">📂 Open</button>
        <button class="btn-danger btn-del" title="Delete">🗑</button>
      </div>
    `;
    li.querySelector('.btn-open').onclick = () => openHistoryEntry(e);
    li.querySelector('.btn-del').onclick  = () => deleteHistoryEntry(e);
    list.appendChild(li);
  }
}

function escapeHtml(s) {
  return String(s ?? '').replace(/[&<>"']/g, c => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;'
  }[c]));
}

async function openHistoryEntry(entry) {
  const cacheDir = await resolveCacheDir();
  let data;
  try {
    data = await invoke('load_transcript', { cacheDir, fileHash: entry.fileHash });
  } catch (err) {
    showAlert(`Failed to load transcript: ${err}`, 'error');
    return;
  }

  currentFile     = data.meta?.sourcePath || '';
  currentSegments = Array.isArray(data.segments) ? [...data.segments] : [];
  currentEngine   = data.engine || data.meta?.engine || '';

  document.getElementById('drop-zone').setAttribute('hidden', '');
  document.getElementById('history-section').setAttribute('hidden', '');
  document.getElementById('file-info').removeAttribute('hidden');
  document.getElementById('current-file-name').textContent =
    (currentFile && currentFile.split('/').pop()) || data.meta?.sourceName || 'Transcript';

  renderTranscript(currentSegments);
  showAlert('📌 Loaded from history.', 'info');

  if (data.memo) {
    showMemoContent(data.memo);
    const btn = document.getElementById('btn-generate-memo');
    if (btn) btn.textContent = '📋 Regenerate Memo';
  }
}

async function deleteHistoryEntry(entry) {
  const name = entry.sourceName || entry.fileHash;
  if (!window.confirm(`Delete transcript for "${name}"? This removes cached transcript, memo and exports.`)) return;
  const cacheDir = await resolveCacheDir();
  try {
    await invoke('delete_transcript', {
      cacheDir,
      fileHash: entry.fileHash,
      sourceName: entry.sourceName || '',
    });
  } catch (err) {
    showAlert(`Delete failed: ${err}`, 'error');
    return;
  }
  renderHistory();
}

function clearCurrentFile() {
  console.log('[clearCurrentFile] running, jobId=', jobId);
  if (jobId) {
    showAlert('A transcription is in progress. Cancel it first.', 'warn');
    return;
  }
  currentFile = null;
  currentSegments = null;
  currentEngine = '';
  if (audioEl) {
    try { audioEl.pause(); } catch {}
    audioEl.removeAttribute('src');
    try { audioEl.load(); } catch {}
  }
  lastActive = null;
  const body = document.getElementById('transcript-body');
  if (body) body.innerHTML = '';
  const memoOut = document.getElementById('memo-output');
  if (memoOut) { memoOut.textContent = ''; memoOut.setAttribute('hidden', ''); }
  showAlert(null);
  document.getElementById('drop-zone').removeAttribute('hidden');
  document.getElementById('history-section').removeAttribute('hidden');
  document.getElementById('file-info').setAttribute('hidden', '');
  document.getElementById('transcript-section').setAttribute('hidden', '');
  document.getElementById('memo-section').setAttribute('hidden', '');
  document.getElementById('progress-section')?.setAttribute('hidden', '');
  renderHistory();
}
window.clearCurrentFile = clearCurrentFile;
window.renderHistory = renderHistory;

// ── Resume in-flight job after page navigation ─────────────────────────────
const JOB_STATE_KEY = 'transcribeJobState';

function persistJobState(extra = {}) {
  if (!jobId) {
    sessionStorage.removeItem(JOB_STATE_KEY);
    return;
  }
  const state = {
    jobId,
    filePath: currentFile,
    message: document.getElementById('progress-message')?.textContent || '',
    percent: parseFloat(document.getElementById('progress-bar-inner')?.style.width || '0'),
    ...extra,
  };
  sessionStorage.setItem(JOB_STATE_KEY, JSON.stringify(state));
}

function clearJobState() {
  sessionStorage.removeItem(JOB_STATE_KEY);
}

async function tryResumeActiveJob() {
  let saved;
  try { saved = JSON.parse(sessionStorage.getItem(JOB_STATE_KEY) || 'null'); }
  catch { saved = null; }
  if (!saved || !saved.jobId) return;

  // Verify the job is still tracked by Rust (running OR has buffered final).
  let activeIds = [];
  try { activeIds = await invoke('list_active_jobs'); } catch { activeIds = []; }
  const isActive = activeIds.includes(saved.jobId);
  const buffered = await invoke('poll_job_result', { jobId: saved.jobId }).catch(() => null);
  if (!isActive && !buffered) {
    clearJobState();
    return;
  }

  // Restore minimal UI: file info + progress bar
  jobId       = saved.jobId;
  currentFile = saved.filePath;
  if (currentFile) {
    document.getElementById('current-file-name').textContent = currentFile.split('/').pop();
    document.getElementById('drop-zone').setAttribute('hidden', '');
    document.getElementById('history-section')?.setAttribute('hidden', '');
    document.getElementById('file-info').removeAttribute('hidden');
  }
  showProgress(true, saved.message || '⏳ Resuming...', saved.percent || 0);
  showAlert('🔄 Resumed in-flight transcription.', 'info');

  // Subscribe to future events before applying any buffered terminal event,
  // so we don't miss a progress emitted right after poll.
  if (unlistenFn) { unlistenFn(); unlistenFn = null; }
  unlistenFn = await window.__TAURI__.event.listen(
    `transcribe:${saved.jobId}`,
    handleTranscribeEvent,
  );

  if (buffered) {
    // Apply the missed terminal event as if it had just arrived.
    handleTranscribeEvent({ payload: buffered });
  }
}

// ── Drop zone ──────────────────────────────────────────────────────────────
function initDropZone() {
  const zone = document.getElementById('drop-zone');
  if (!zone) return;

  zone.addEventListener('click', pickFile);

  // Visual feedback for drag-over (works with Web DragEvent)
  zone.addEventListener('dragover',  (e) => { e.preventDefault(); zone.classList.add('drag-over'); });
  zone.addEventListener('dragleave', ()  => zone.classList.remove('drag-over'));
  // NOTE: We do NOT use the Web drop event because `file.path` is not available
  // in Tauri WebView. Actual file paths come from the Tauri drag-drop plugin below.
  zone.addEventListener('drop', (e) => { e.preventDefault(); zone.classList.remove('drag-over'); });

  // Tauri drag-drop plugin provides real filesystem paths
  window.__TAURI__.event.listen('tauri://drag-drop', (event) => {
    const paths = event.payload?.paths;
    if (Array.isArray(paths) && paths.length > 0) {
      const filePath = paths[0]; // take first dropped file
      loadFilePath(filePath);
    }
  }).catch(() => {
    // Plugin not available in some dev configs — silent fallback to file picker
  });
}

async function pickFile() {
  const path = await invoke('pick_audio_file').catch(() => null);
  if (path) loadFilePath(path);
}

function loadFilePath(path) {
  currentFile = path;
  const name = path.split('/').pop();
  document.getElementById('current-file-name').textContent = name;
  document.getElementById('drop-zone').setAttribute('hidden', '');
  document.getElementById('history-section')?.setAttribute('hidden', '');
  document.getElementById('file-info').removeAttribute('hidden');
  startTranscription();
}

// ── Transcription ──────────────────────────────────────────────────────────
async function startTranscription() {
  if (!settings) {
    try { settings = await invoke('get_settings'); } catch { settings = {}; }
  }

  // Resolve cache dir
  let cacheDir = settings.cacheDir || '';
  if (!cacheDir) {
    cacheDir = await invoke('get_cache_dir').catch(() => '/tmp/cache_transcripts');
  }

  // New job
  jobId = `job-${Date.now()}`;
  document.getElementById('transcript-section').setAttribute('hidden', '');
  document.getElementById('memo-section').setAttribute('hidden', '');
  showProgress(true, '⏳ Starting...', 0);
  showAlert(null);
  const debugEl = document.getElementById('debug-log');
  if (debugEl) debugEl.textContent = '';
  persistJobState();

  // Subscribe to events BEFORE invoking
  if (unlistenFn) { unlistenFn(); unlistenFn = null; }
  unlistenFn = await window.__TAURI__.event.listen(
    `transcribe:${jobId}`,
    handleTranscribeEvent
  );

  try {
    await invoke('transcribe', {
      jobId,
      settings: {
        filePath:      currentFile,
        mode:          settings.aiMode           || 'whisper',
        modelSize:     settings.whisperModelSize  || 'small',
        language:      settings.language          || 'ja',
        apiKey:        settings.geminiApiKey      || null,
        geminiModel:   settings.geminiModel       || 'gemini-3.5-flash',
        forceRerun:    settings.forceRerun        || false,
        cacheDir:      cacheDir,
        chunkMinutes:  settings.chunkMinutes       || 10,
      }
    });
  } catch (err) {
    showProgress(false);
    showAlert(`❌ Transcription error: ${err}`, 'error');
    if (unlistenFn) { unlistenFn(); unlistenFn = null; }
  }
}

function handleTranscribeEvent({ payload }) {
  const p = typeof payload === 'string' ? JSON.parse(payload) : payload;

  if (p.type === 'progress') {
    showProgress(true, p.message, p.percent ?? 0);
    persistJobState();

  } else if (p.type === 'log') {
    appendDebugLog(p.message);

  } else if (p.type === 'result') {
    if (unlistenFn) { unlistenFn(); unlistenFn = null; }
    showProgress(false);
    clearJobState();
    const segs = Array.isArray(p.segments) ? [...p.segments] : [];
    segs.sort((a, b) => (parseFloat(a.start) || 0) - (parseFloat(b.start) || 0));
    currentSegments = segs;
    currentEngine   = p.engine || '';
    if (p.cached) showAlert('📌 Loaded from cache.', 'info');
    renderTranscript(segs);

  } else if (p.type === 'cancelled') {
    if (unlistenFn) { unlistenFn(); unlistenFn = null; }
    showProgress(false);
    clearJobState();
    showAlert('⛔ Transcription cancelled.', 'warn');

  } else if (p.type === 'error') {
    if (unlistenFn) { unlistenFn(); unlistenFn = null; }
    showProgress(false);
    clearJobState();
    showAlert(`❌ ${p.message}`, 'error');
  }
}

async function cancelTranscribe() {
  if (!jobId) return;
  const btn = document.getElementById('btn-cancel-transcribe');
  if (btn) { btn.disabled = true; btn.textContent = '⏳ Cancelling...'; }
  try {
    await invoke('cancel_transcribe', { jobId });
  } catch (err) {
    showAlert(`Failed to cancel: ${err}`, 'error');
    if (btn) { btn.disabled = false; btn.textContent = '✖ Cancel'; }
  }
}
window.cancelTranscribe = cancelTranscribe;

function appendDebugLog(line) {
  const el = document.getElementById('debug-log');
  if (!el) return;
  el.textContent += (el.textContent ? '\n' : '') + line;
  el.scrollTop = el.scrollHeight;
}

// ── Progress UI ────────────────────────────────────────────────────────────
function showProgress(visible, msg = '', pct = 0) {
  const section = document.getElementById('progress-section');
  const bar     = document.getElementById('progress-bar-inner');
  const msgEl   = document.getElementById('progress-message');
  const cancelBtn = document.getElementById('btn-cancel-transcribe');

  if (visible) {
    section.removeAttribute('hidden');
    bar.style.width = `${pct}%`;
    msgEl.textContent = msg;
    if (cancelBtn) { cancelBtn.disabled = false; cancelBtn.textContent = '✖ Cancel'; }
  } else {
    section.setAttribute('hidden', '');
    jobId = null;
  }
}

// ── Transcript rendering ───────────────────────────────────────────────────
function renderTranscript(segments) {
  const section = document.getElementById('transcript-section');
  const body    = document.getElementById('transcript-body');

  // Set audio source using Tauri asset protocol (avoids base64 embedding)
  if (currentFile) {
    const audioSrc = window.__TAURI__.core.convertFileSrc(currentFile);
    console.info('[audio] loading source', { file: currentFile, src: audioSrc });
    audioEl.src = audioSrc;
    audioEl.load();
    document.getElementById('audio-filename').textContent = currentFile.split('/').pop();
  }

  // Clear previous transcript
  body.innerHTML = '';
  lastActive = null;

  // Apply timestamp offset (only meaningful for Gemini — `timestampOffset()`
  // returns 0 otherwise). Cache stores raw model timestamps; the shift is
  // purely cosmetic so the user can re-tune without re-running the job.
  const off = timestampOffset();
  segments.forEach((seg, i) => {
    const rawStart = parseFloat(seg.start ?? 0);
    const rawEnd   = parseFloat(seg.end   ?? 0);
    const start = Math.max(0, rawStart + off);
    const end   = Math.max(0, rawEnd   + off);
    const text  = (seg.text || '').replace(/\n/g, ' ');
    const timeStr = formatTime(start);

    const p = document.createElement('p');
    p.id = `seg${i}`;
    p.dataset.start = start;
    p.dataset.end   = end;
    p.textContent   = `▶ [${timeStr}] ${text}`;
    p.onclick       = () => {
      audioEl.currentTime = start;
      audioEl.play();
    };
    body.appendChild(p);
  });

  // Auto-highlight on playback (exact same logic from Streamlit version)
  audioEl.removeEventListener('timeupdate', onTimeUpdate);
  audioEl.addEventListener('timeupdate', onTimeUpdate);

  section.removeAttribute('hidden');
  buildExportButtons();
  buildMemoSection();
}

function onTimeUpdate() {
  const current = audioEl.currentTime;
  document.querySelectorAll('#transcript-body p').forEach(seg => {
    const start = parseFloat(seg.dataset.start);
    const end   = parseFloat(seg.dataset.end);
    if (current >= start && current < end) {
      if (lastActive !== seg) {
        if (lastActive) lastActive.classList.remove('active');
        seg.classList.add('active');
        seg.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
        lastActive = seg;
      }
    } else {
      seg.classList.remove('active');
    }
  });
}

function formatTime(seconds) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (h > 0) return `${pad(h)}:${pad(m)}:${pad(s)}`;
  return `${pad(m)}:${pad(s)}`;
}
function pad(n) { return String(n).padStart(2, '0'); }

// ── Export ─────────────────────────────────────────────────────────────────
function buildExportButtons() {
  // Buttons are always visible; just enable them
  document.getElementById('btn-export-txt')?.removeAttribute('disabled');
  document.getElementById('btn-export-srt')?.removeAttribute('disabled');
  document.getElementById('btn-export-json')?.removeAttribute('disabled');
}

async function exportAs(format) {
  if (!currentSegments) return;
  let content, filename;

  if (format === 'txt') {
    const fmtHms = s => {
      const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = Math.floor(s % 60);
      return `${pad(h)}:${pad(m)}:${pad(sec)}`;
    };
    content = currentSegments
      .map(s => `[${fmtHms(s.start)} - ${fmtHms(s.end)}] ${s.text}`)
      .join('\n');
    filename = 'transcript.txt';

  } else if (format === 'srt') {
    const srtTime = s => {
      const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60),
            sec = Math.floor(s % 60), ms = Math.round((s % 1) * 1000);
      return `${pad(h)}:${pad(m)}:${pad(sec)},${String(ms).padStart(3,'0')}`;
    };
    content = currentSegments.map((s, i) =>
      `${i+1}\n${srtTime(s.start)} --> ${srtTime(s.end)}\n${s.text}\n`
    ).join('\n');
    filename = 'transcript.srt';

  } else {
    content  = JSON.stringify({ segments: currentSegments }, null, 2);
    filename = 'transcript.json';
  }

  try {
    const saved = await invoke('export_file', { content, defaultFilename: filename });
    if (saved) showAlert(`✅ Saved to ${saved.split('/').pop()}`, 'success');
  } catch (err) {
    showAlert(`Export failed: ${err}`, 'error');
  }
}

window.exportAs = exportAs;

// ── Memo ───────────────────────────────────────────────────────────────────
function buildMemoSection() {
  const memoSection = document.getElementById('memo-section');
  const btn = document.getElementById('btn-generate-memo');
  const hint = document.getElementById('memo-hint');
  const output = document.getElementById('memo-output');
  const btnDl = document.getElementById('btn-memo-download');

  memoSection.removeAttribute('hidden');

  if (!settings?.geminiApiKey) {
    btn.textContent = '⚙️ Set Gemini API Key';
    btn.onclick = () => window.router.go('settings');
    hint.textContent = 'Meeting memo generation uses Gemini. Add your Gemini API key in Settings to enable summaries and action items.';
    hint.removeAttribute('hidden');
    output.setAttribute('hidden', '');
    btnDl?.setAttribute('hidden', '');
    return;
  }

  btn.textContent = '📋 Generate Memo';
  btn.onclick = generateMemo;
  hint.setAttribute('hidden', '');
  output.textContent = '';
  output.setAttribute('hidden', '');
  btnDl?.setAttribute('hidden', '');
}

async function generateMemo() {
  if (!settings?.geminiApiKey) {
    showAlert('⚠️ Gemini API key required for memo generation. Set it in Settings.', 'warn');
    return;
  }
  if (!currentSegments) return;

  const btn = document.getElementById('btn-generate-memo');
  btn.disabled = true;
  btn.textContent = '⏳ Generating...';

  let cacheDir = settings.cacheDir || '';
  if (!cacheDir) {
    cacheDir = await invoke('get_cache_dir').catch(() => '/tmp');
  }

  // 1. Silently write current segments to a transcript-specific cache path.
  const transcriptPayload = JSON.stringify({ segments: currentSegments });
  const memoPromptTemplate = settings.memoPromptTemplate || '';
  const memoCacheKey = buildMemoCacheKey(
    currentFile,
    `${transcriptPayload}\n${memoPromptTemplate}`
  );
  const transcriptJsonPath = `${cacheDir}/${memoCacheKey}.transcript.json`;
  const memoOutputPath     = `${cacheDir}/${memoCacheKey}.memo.txt`;

  try {
    await invoke('write_json_file', {
      path:    transcriptJsonPath,
      content: JSON.stringify({ segments: currentSegments }, null, 2),
    });
  } catch (err) {
    btn.disabled = false;
    btn.textContent = '📋 Generate Memo';
    showAlert(`Cannot write transcript temp file: ${err}`, 'error');
    return;
  }

  // 2. Subscribe to memo events, then invoke generate_memo
  const memoJobId = `memo-${Date.now()}`;

  let unlistenMemo = await window.__TAURI__.event.listen(`memo:${memoJobId}`, ({ payload }) => {
    const p = typeof payload === 'string' ? JSON.parse(payload) : payload;
    if (p.type === 'result') {
      unlistenMemo();
      btn.disabled = false;
      btn.textContent = '📋 Regenerate Memo';
      showMemoContent(p.content);
    } else if (p.type === 'error') {
      unlistenMemo();
      btn.disabled = false;
      btn.textContent = '📋 Generate Memo';
      showAlert(`Memo error: ${p.message}`, 'error');
    }
  });

  try {
    await invoke('generate_memo', {
      jobId: memoJobId,
      req: {
        transcriptJsonPath,
        apiKey:        settings.geminiApiKey,
        geminiModel:   settings.geminiModel || 'gemini-3.5-flash',
        memoPromptTemplate,
        cacheMemoPath: memoOutputPath,
        forceRerun:    settings.forceRerun || false,
      }
    });
  } catch (err) {
    unlistenMemo();
    btn.disabled = false;
    btn.textContent = '📋 Generate Memo';
    showAlert(`Memo failed: ${err}`, 'error');
  }
}

function buildMemoCacheKey(filePath, transcriptPayload) {
  const fileName = (filePath || 'transcript')
    .split('/')
    .pop()
    .replace(/\.[^.]+$/, '')
    .replace(/[^a-zA-Z0-9._-]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 80) || 'transcript';

  return `${fileName}-${hashString(transcriptPayload)}`;
}

function hashString(text) {
  let hash = 5381;
  for (let i = 0; i < text.length; i += 1) {
    hash = ((hash << 5) + hash) ^ text.charCodeAt(i);
  }
  return (hash >>> 0).toString(16).padStart(8, '0');
}

function showMemoContent(content) {
  const output = document.getElementById('memo-output');
  output.textContent = content;
  output.removeAttribute('hidden');

  const btnDl = document.getElementById('btn-memo-download');
  btnDl?.removeAttribute('hidden');
  btnDl.onclick = () => {
    invoke('export_file', { content, defaultFilename: 'meeting_memo.txt' });
  };
}

window.generateMemo = generateMemo;

// ── Alert banner ───────────────────────────────────────────────────────────
let alertTimer;
function showAlert(msg, type = 'info') {
  const el = document.getElementById('main-alert');
  if (!el) return;
  if (!msg) { el.setAttribute('hidden', ''); return; }
  el.textContent = msg;
  el.className = `alert alert-${type}`;
  el.removeAttribute('hidden');
  clearTimeout(alertTimer);
  if (type !== 'error') {
    alertTimer = setTimeout(() => el.setAttribute('hidden', ''), 4000);
  }
}
