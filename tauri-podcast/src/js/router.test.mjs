import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

function createSessionStorage() {
  const data = new Map();
  return {
    getItem(key) {
      return data.has(key) ? data.get(key) : null;
    },
    setItem(key, value) {
      data.set(key, String(value));
    },
  };
}

function loadRouter({ elements = [] } = {}) {
  const listeners = new Map();
  const window = {
    location: {
      href: 'tauri://localhost/index.html',
      assign(value) {
        this.href = value;
      },
    },
    sessionStorage: createSessionStorage(),
    addEventListener(type, callback) {
      listeners.set(type, callback);
    },
  };
  const document = {
    querySelectorAll(selector) {
      return selector === '[data-route]' ? elements : [];
    },
  };

  const context = {
    URL,
    document,
    sessionStorage: window.sessionStorage,
    window,
  };
  const code = readFileSync(new URL('./router.js', import.meta.url), 'utf8');
  vm.runInNewContext(code, context);

  return { listeners, window };
}

test('exposes router on window for inline and dynamic handlers', () => {
  const { window } = loadRouter();

  assert.equal(typeof window.router?.go, 'function');
  assert.equal(typeof window.router?.state, 'function');
});

test('routes data-route clicks through the shared router', () => {
  let clickHandler;
  const routeButton = {
    dataset: { route: 'settings' },
    addEventListener(type, callback) {
      if (type === 'click') clickHandler = callback;
    },
  };
  const event = {
    defaultPrevented: false,
    preventDefault() {
      this.defaultPrevented = true;
    },
  };
  const { listeners, window } = loadRouter({ elements: [routeButton] });

  listeners.get('DOMContentLoaded')();
  clickHandler(event);

  assert.equal(event.defaultPrevented, true);
  assert.equal(window.location.href, 'tauri://localhost/settings.html');
});
