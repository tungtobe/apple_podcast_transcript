/* Setup Wizard logic */

const { invoke, event: tauriEvent } = window.__TAURI__.core;

// ── State ──────────────────────────────────────────────────────────────────
let setupStatus = null;
let unlisten = null;

// ── DOM refs ───────────────────────────────────────────────────────────────
const iconPython   = () => document.getElementById('icon-python');
const iconPkgs     = () => document.getElementById('icon-pkgs');
const iconFfmpeg   = () => document.getElementById('icon-ffmpeg');
const detailPython = () => document.getElementById('detail-python');
const detailPkgs   = () => document.getElementById('detail-pkgs');
const detailFfmpeg = () => document.getElementById('detail-ffmpeg');
const btnInstall   = () => document.getElementById('btn-install');
const btnRecheck   = () => document.getElementById('btn-recheck');
const btnContinue  = () => document.getElementById('btn-continue');
const installLog   = () => document.getElementById('install-log');
const installLogWrap = () => document.getElementById('install-log-wrap');

// ── Init ───────────────────────────────────────────────────────────────────
window.addEventListener('DOMContentLoaded', async () => {
  // Try to use status cached from main.js redirect first
  const cached = router.state('setup');
  if (cached) {
    applyStatus(cached);
  } else {
    await runCheck();
  }
});

async function runCheck() {
  const error = document.getElementById('error-banner');
  if (error) error.setAttribute('hidden', '');

  setAllIcons('⏳');
  btnRecheck()?.setAttribute('disabled', '');
  btnInstall()?.setAttribute('disabled', '');
  btnContinue()?.setAttribute('disabled', '');

  try {
    setupStatus = await invoke('check_setup');
    applyStatus(setupStatus);
  } catch (err) {
    showError(`Setup check failed: ${err}`);
  }
}

function applyStatus(status) {
  setupStatus = status;

  // Python
  if (status.python_ok) {
    iconPython().textContent = '✅';
    detailPython().textContent = `Python ${status.python_version}`;
  } else {
    iconPython().textContent = '❌';
    detailPython().innerHTML =
      `Python 3.10+ is required. Version found: ${status.python_version || 'none'}<br>
       Install with <code class="check-code">brew install python</code> or
       <a href="https://www.python.org/downloads/macos/" class="check-code">download Python for macOS ↗</a>.
       Then click Check Again.`;
  }

  // Packages
  if (!status.python_ok) {
    iconPkgs().textContent = '⏸';
    detailPkgs().textContent = 'Install Python first, then the app can check Python packages.';
    btnInstall()?.setAttribute('hidden', '');
    btnInstall()?.setAttribute('disabled', '');
  } else if (status.missing_packages.length === 0) {
    iconPkgs().textContent = '✅';
    detailPkgs().textContent = 'All packages installed.';
    btnInstall()?.setAttribute('hidden', '');
  } else {
    iconPkgs().textContent = '⚠️';
    detailPkgs().textContent = `Missing: ${status.missing_packages.join(', ')}`;
    btnInstall()?.removeAttribute('hidden');
    btnInstall()?.removeAttribute('disabled');
  }

  // ffmpeg
  if (status.ffmpeg_ok) {
    iconFfmpeg().textContent = '✅';
    detailFfmpeg().textContent = 'ffmpeg available.';
  } else {
    iconFfmpeg().textContent = '⚠️';
    detailFfmpeg().innerHTML =
      `ffmpeg not found. Install with:<br>
       <code class="check-code">brew install ffmpeg</code><br>
       If <code class="check-code">brew</code> is not available, install Homebrew first.`;
  }

  // Enable recheck always
  btnRecheck()?.removeAttribute('disabled');

  // Can continue only if Python OK (ffmpeg + packages are optional at launch)
  if (status.python_ok) {
    btnContinue()?.removeAttribute('disabled');
  }
}

function setAllIcons(icon) {
  ['icon-python', 'icon-pkgs', 'icon-ffmpeg'].forEach(id => {
    const el = document.getElementById(id);
    if (el) el.textContent = icon;
  });
}

// ── Install packages ───────────────────────────────────────────────────────
async function installPackages() {
  if (!setupStatus?.missing_packages?.length) return;

  btnInstall().disabled = true;
  btnInstall().textContent = '⏳ Installing...';
  installLogWrap().removeAttribute('hidden');
  installLog().textContent = '';
  iconPkgs().textContent = '⏳';

  if (unlisten) { unlisten(); unlisten = null; }

  unlisten = await window.__TAURI__.event.listen('install:progress', ({ payload }) => {
    installLog().textContent += payload + '\n';
    installLog().scrollTop = installLog().scrollHeight;
  });

  try {
    await invoke('install_deps', { packages: setupStatus.missing_packages });
    // Re-run check to confirm
    await runCheck();
    btnInstall().textContent = '📦 Install Missing Packages';
  } catch (err) {
    appendLog(`❌ Error: ${err}`);
    btnInstall().disabled = false;
    btnInstall().textContent = '📦 Install Missing Packages';
  } finally {
    if (unlisten) { unlisten(); unlisten = null; }
  }
}

function appendLog(line) {
  installLog().textContent += line + '\n';
  installLog().scrollTop = installLog().scrollHeight;
}

function showError(msg) {
  const el = document.getElementById('error-banner');
  if (el) { el.textContent = msg; el.removeAttribute('hidden'); }
}

// ── Expose to HTML ─────────────────────────────────────────────────────────
window.installPackages = installPackages;
window.recheckSetup = runCheck;
window.continueToApp = () => router.go('main');
