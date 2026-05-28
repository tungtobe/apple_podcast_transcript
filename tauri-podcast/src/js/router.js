/** Minimal page router for Tauri static HTML pages. */
(() => {
  const routes = { main: 'index.html', settings: 'settings.html', setup: 'setup.html' };

  function routeUrl(page) {
    return new URL(routes[page] || routes.main, window.location.href).toString();
  }

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
      const target = routeUrl(page);
      if (typeof window.location.assign === 'function') {
        window.location.assign(target);
      } else {
        window.location.href = target;
      }
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

  window.router = router;

  window.addEventListener('DOMContentLoaded', () => {
    document.querySelectorAll('[data-route]').forEach((el) => {
      el.addEventListener('click', (event) => {
        event.preventDefault();
        router.go(el.dataset.route);
      });
    });
  });
})();
