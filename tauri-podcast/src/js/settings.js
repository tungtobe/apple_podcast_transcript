/* Settings page logic */

const { invoke } = window.__TAURI__.core;

// ── State ──────────────────────────────────────────────────────────────────
let currentSettings = null;

// ── Sidebar scroll-spy ─────────────────────────────────────────────────────
function setActiveNav(id) {
  document.querySelectorAll('.settings-nav .nav-link').forEach(a => {
    a.classList.toggle('active', a.dataset.target === id);
  });
}

function initScrollSpy() {
  const panel = document.querySelector('.settings-panel');
  const sections = Array.from(document.querySelectorAll('.settings-panel .tab-panel'));
  if (!panel || !sections.length) return;

  // Anchor clicks: smooth-scroll within the panel container instead of the page.
  document.querySelectorAll('.settings-nav .nav-link').forEach(link => {
    link.addEventListener('click', (e) => {
      e.preventDefault();
      const target = document.getElementById(link.dataset.target);
      if (target) {
        panel.scrollTo({ top: target.offsetTop - 8, behavior: 'smooth' });
        setActiveNav(link.dataset.target);
      }
    });
  });

  // Highlight nav based on which section is closest to top.
  const onScroll = () => {
    const top = panel.scrollTop;
    let current = sections[0].id;
    for (const s of sections) {
      if (s.offsetTop - 40 <= top) current = s.id;
    }
    setActiveNav(current);
  };
  panel.addEventListener('scroll', onScroll, { passive: true });
  onScroll();
}

// ── Load settings ──────────────────────────────────────────────────────────
async function loadSettings() {
  try {
    currentSettings = await invoke('get_settings');
    populateForm(currentSettings);
  } catch (err) {
    showBanner(`Failed to load settings: ${err}`, 'error');
  }
}

function populateForm(s) {
  setValue('ai-mode', s.aiMode);
  setValue('gemini-api-key', s.geminiApiKey);
  setValue('gemini-model', s.geminiModel);
  setValue('whisper-model-size', s.whisperModelSize);
  setValue('language', s.language);
  document.getElementById('force-rerun').checked = s.forceRerun;
  setValue('cache-dir', s.cacheDir);
  setValue('chunk-minutes', s.chunkMinutes ?? 10);
  setValue('gemini-timestamp-offset', s.geminiTimestampOffset ?? 0);
  setValue('memo-prompt-template', s.memoPromptTemplate);
}

function setValue(id, val) {
  const el = document.getElementById(id);
  if (el) el.value = val ?? '';
}

// ── Gemini models ──────────────────────────────────────────────────────────
async function loadGeminiModels() {
  const apiKey = document.getElementById('gemini-api-key').value.trim();
  const modelInput = document.getElementById('gemini-model');
  const list = document.getElementById('gemini-models-list');
  const status = document.getElementById('gemini-model-status');
  const btn = document.getElementById('btn-load-gemini-models');

  if (!apiKey) {
    status.textContent = 'Enter a Gemini API key before loading models.';
    status.className = 'hint text-warn';
    return;
  }

  btn.disabled = true;
  btn.textContent = 'Loading...';
  status.textContent = 'Loading models from Gemini...';
  status.className = 'hint';
  list.innerHTML = '';

  try {
    const models = await invoke('list_gemini_models', { apiKey });
    models.forEach((model) => {
      const option = document.createElement('option');
      option.value = model;
      list.appendChild(option);
    });

    if (models.length === 0) {
      status.textContent = 'No generateContent models were returned for this API key.';
      status.className = 'hint text-warn';
    } else {
      status.textContent = `${models.length} models available. Type or select from suggestions.`;
      status.className = 'hint text-success';
      if (!modelInput.value) {
        modelInput.value = models.find((m) => m.includes('flash')) || models[0];
      }
    }
  } catch (err) {
    status.textContent = `Cannot load Gemini models: ${err}`;
    status.className = 'hint text-danger';
  } finally {
    btn.disabled = false;
    btn.textContent = '↻ Load Models';
  }
}

// ── Save settings ──────────────────────────────────────────────────────────
async function saveSettings() {
  const settings = {
    aiMode:           document.getElementById('ai-mode').value,
    geminiApiKey:     document.getElementById('gemini-api-key').value,
    geminiModel:      document.getElementById('gemini-model').value,
    whisperModelSize: document.getElementById('whisper-model-size').value,
    language:         document.getElementById('language').value,
    forceRerun:       document.getElementById('force-rerun').checked,
    cacheDir:         document.getElementById('cache-dir').value,
    chunkMinutes:     Math.max(2, Math.min(30, parseInt(document.getElementById('chunk-minutes').value, 10) || 10)),
    geminiTimestampOffset: Math.max(-30, Math.min(30, parseFloat(document.getElementById('gemini-timestamp-offset').value) || 0)),
    memoPromptTemplate: document.getElementById('memo-prompt-template').value,
  };

  try {
    await invoke('save_settings', { settings });
    showBanner('✅ Settings saved.', 'success');
  } catch (err) {
    showBanner(`❌ Failed to save: ${err}`, 'error');
  }
}

// ── Cache actions ──────────────────────────────────────────────────────────
async function openCacheFolder() {
  const dir = document.getElementById('cache-dir').value;
  try {
    await invoke('open_cache_folder', { cacheDir: dir });
  } catch (err) {
    showBanner(`Cannot open folder: ${err}`, 'error');
  }
}

async function clearCache() {
  const dir = document.getElementById('cache-dir').value;
  if (!confirm(
    `Clear cache in:\n${dir}\n\n` +
    `This wipes:\n` +
    `  • transcripts (.json/.txt/.srt)\n` +
    `  • debug log\n` +
    `  • leftover chunk/audio temp files\n\n` +
    `Original audio/video files are NOT touched.\n` +
    `Cannot be undone.`
  )) return;
  try {
    const count = await invoke('clear_cache', { cacheDir: dir });
    showBanner(`✅ Removed ${count} item(s).`, 'success');
  } catch (err) {
    showBanner(`${err}`, 'error');
  }
}

// ── Toggle API key visibility ──────────────────────────────────────────────
function toggleApiKeyVisibility() {
  const input = document.getElementById('gemini-api-key');
  const btn   = document.getElementById('btn-toggle-key');
  if (input.type === 'password') {
    input.type = 'text';
    btn.textContent = '🙈 Hide';
  } else {
    input.type = 'password';
    btn.textContent = '👁 Show';
  }
}

// ── Banner ─────────────────────────────────────────────────────────────────
let bannerTimer;
function showBanner(msg, type = 'info') {
  const el = document.getElementById('settings-banner');
  if (!el) return;
  el.textContent = msg;
  el.className = `alert alert-${type}`;
  el.removeAttribute('hidden');
  clearTimeout(bannerTimer);
  bannerTimer = setTimeout(() => el.setAttribute('hidden', ''), 3500);
}

// ── Action dispatch ────────────────────────────────────────────────────────
const ACTIONS = {
  'save-settings':       saveSettings,
  'open-cache-folder':   openCacheFolder,
  'clear-cache':         clearCache,
  'toggle-api-key':      toggleApiKeyVisibility,
  'load-gemini-models':  loadGeminiModels,
};

function bindActions() {
  document.querySelectorAll('[data-action]').forEach((el) => {
    const handler = ACTIONS[el.dataset.action];
    if (handler) el.addEventListener('click', handler);
  });
}

// ── Init ───────────────────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', () => {
  bindActions();
  loadSettings();
  initScrollSpy();
});
