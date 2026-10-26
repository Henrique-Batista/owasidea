import { test, expect } from '@playwright/test';
import { createHash } from 'node:crypto';

test('plugin list renders', async ({ page }) => {
  await page.route('http://127.0.0.1:8787/plugins', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify([
      { name: 'notes', language: 'rust', type: 'library', runtime: 'wasi-component', exports: ['greet'] },
      { name: 'chat-go', language: 'go', type: 'executable', runtime: 'wasi-component', exports: ['incoming-handler'] },
    ]),
  }));
  await page.goto('/#/plugins');
  await expect(page.locator('body')).toContainText('notes');
  await expect(page.locator('body')).toContainText('chat-go');
});

test('plugin detail calls a WIT export through the host API', async ({ page }) => {
  await page.route('http://127.0.0.1:8787/plugin/notes/manifest', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({
      name: 'notes',
      exports: [{ name: 'greet', signature: 'func(name: string) -> string' }],
    }),
  }));
  await page.route('http://127.0.0.1:8787/plugin/notes/logs', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({ logs: [] }),
  }));
  await page.route('http://127.0.0.1:8787/plugin/notes/call', async route => {
    const request = route.request().postDataJSON();
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ result: `Hello, ${request.args[0]}` }),
    });
  });

  await page.goto('/#/plugins/notes');
  await expect(page.locator('body')).toContainText('func(name: string)');
  await page.locator('input[data-input="greet"]').fill('Ada');
  await page.locator('button[data-call="greet"]').click();
  await expect(page.locator('pre[data-out="greet"]')).toContainText('Hello, Ada');
});

test('dev panel shows watcher paths and reload result', async ({ page }) => {
  await page.route('http://127.0.0.1:8787/dev/status', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({
      active: true,
      hot_reload: true,
      watch_paths: ['plugins', 'manifests'],
      last_reload: { notes: { ok: false, http_status: 409, timestamp_unix: 1 } },
    }),
  }));
  await page.route('http://127.0.0.1:8787/plugins', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify([
      { name: 'notes', runtime: 'wasi-component' },
      { name: 'dashboard', runtime: 'js-wasm' },
    ]),
  }));
  await page.goto('/#/dev');
  await expect(page.locator('body')).toContainText('plugins');
  await expect(page.locator('body')).toContainText('failed (HTTP 409)');
  await expect(page.locator('button[data-dev-reload="notes"]')).toBeVisible();
  await expect(page.locator('button[data-dev-reload="dashboard"]')).toHaveCount(0);
});

test('UI plugin bundle integrity verified before sandboxed iframe loads', async ({ page }) => {
  const bytes = Buffer.from('<!doctype html><title>Demo</title>');
  const fileDigest = createHash('sha256').update(bytes).digest('hex');
  const bundleDigest = createHash('sha256').update(`index.html\0${fileDigest}\n`).digest('hex');
  await page.route('http://127.0.0.1:8787/ui-plugins', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify([{
      name: 'demo',
      language: 'yew',
      version: '1.0.0',
      entry: 'plugins/demo/index.html',
      sha256: bundleDigest,
      files: { 'index.html': fileDigest },
    }]),
  }));
  await page.route('**/plugins/demo/index.html', route => route.fulfill({
    status: 200,
    contentType: 'text/html',
    body: bytes,
  }));
  await page.goto('/#/ui/demo');
  const frame = page.locator('iframe[title="demo UI"]');
  await expect(frame).toBeVisible();
  await expect(frame).toHaveAttribute('sandbox', 'allow-scripts allow-forms');
});
