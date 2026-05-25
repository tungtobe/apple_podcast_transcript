/** Minimal hash-based page router for Tauri static HTML pages. */
const router = {
  /**
   * Navigate to a page.
   * @param {'main'|'settings'|'setup'} page
   * @param {object} [state] - optional state passed via sessionStorage
   */
  go(page, state) {
    if (state) {
      sessionStorage.setItem(`nav:${page}`, JSON.stringify(state));
    }
    const map = { main: 'index.html', settings: 'settings.html', setup: 'setup.html' };
    window.location.href = map[page] || 'index.html';
  },

  /** Read state passed by the previous page. */
  state(key) {
    const raw = sessionStorage.getItem(`nav:${key}`);
    if (raw) {
      try { return JSON.parse(raw); } catch { return null; }
    }
    return null;
  },
};
