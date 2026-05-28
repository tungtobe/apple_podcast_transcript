import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const config = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const css = readFileSync(new URL('./css/style.css', import.meta.url), 'utf8');
const mainHtml = readFileSync(new URL('./index.html', import.meta.url), 'utf8');
const settingsHtml = readFileSync(new URL('./settings.html', import.meta.url), 'utf8');
const setupHtml = readFileSync(new URL('./setup.html', import.meta.url), 'utf8');

test('uses the native macOS titlebar instead of overlaying web content', () => {
  const [mainWindow] = config.app.windows;

  assert.notEqual(mainWindow.titleBarStyle, 'Overlay');
});

test('web toolbar does not try to act as the window frame', () => {
  assert.doesNotMatch(css, /-webkit-app-region:\s*drag/);
  assert.doesNotMatch(css, /\bapp-region:\s*drag/);
  assert.doesNotMatch(css, /width:\s*100vw/);
  assert.doesNotMatch(css, /margin-left:\s*calc\(50% - 50vw\)/);
  for (const html of [mainHtml, settingsHtml, setupHtml]) {
    assert.doesNotMatch(html, /data-tauri-drag-region/);
  }
});
