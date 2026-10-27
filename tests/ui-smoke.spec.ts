import { test, expect } from '@playwright/test';
import { createHash } from 'node:crypto';

test('runtime host injection is preferred over build-time host', async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__OWASIDEA__ = { host: 'http://127.0.0.1:8790', app: 'Test', version: '1', build_id: 'bid' };
  });
  await page.route('http://127.0.0.1:8790/health', route => route.fulfill({ status: 200, body: 'ok' }));
  await page.route('http://127.0.0.1:8790/plugins', route => route.fulfill({ status: 200, contentType: 'application/json', body: '[]' }));
  await page.route('http://127.0.0.1:8790/overview', route => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ plugins: 0, libraries: 0, executables: 0, services: 0, runtimes: [] }) }));
  await page.goto('/');
  await page.waitForTimeout(200);
  // host label should reflect runtime host
  await expect(page.locator('body')).toContainText('127.0.0.1:8790');
});

test('non-loopback runtime host is rejected and no request is made', async ({ page }) => {
  const requests: string[] = [];
  page.on('request', req => requests.push(req.url()));
  await page.addInitScript(() => {
    (window as any).__OWASIDEA__ = { host: 'http://evil.example', app: 'Test', version: '1', build_id: null };
  });
  await page.route('http://127.0.0.1:8787/health', route => route.fulfill({ status: 200, body: 'ok' }));
  await page.goto('/');
  await page.waitForTimeout(200);
  expect(requests.some(url => url.includes('evil.example'))).toBeFalsy();
});

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

test('plugin call error keeps plain text and lifts hint/docs lines', async ({ page }) => {
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
  await page.route('http://127.0.0.1:8787/plugin/notes/call', route => route.fulfill({
    status: 400,
    contentType: 'text/plain',
    body: 'invalid args: expected u32\nhint: pass a number\ndocs: docs/API.md#errors',
  }));

  await page.goto('/#/plugins/notes');
  await page.locator('input[data-input="greet"]').fill('Ada');
  await page.locator('button[data-call="greet"]').click();
  const err = page.locator('pre[data-error="greet"]');
  await expect(err).toContainText('HTTP 400: invalid args: expected u32');
  await expect(err).toContainText('hint: pass a number');
  await expect(err).toContainText('docs: docs/API.md#errors');
});

test('plugin call error renders structured JSON code/message/hint/docs', async ({ page }) => {
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
  await page.route('http://127.0.0.1:8787/plugin/notes/call', route => route.fulfill({
    status: 400,
    contentType: 'application/json',
    body: JSON.stringify({
      code: 'OWASIDEA_E_HTTP_CALL',
      message: 'call rejected',
      hint: 'check the args',
      docs: 'docs/ERRORS.md#owasidea_e_http_call',
    }),
  }));

  await page.goto('/#/plugins/notes');
  await page.locator('input[data-input="greet"]').fill('Ada');
  await page.locator('button[data-call="greet"]').click();
  const err = page.locator('pre[data-error="greet"]');
  await expect(err).toContainText('[OWASIDEA_E_HTTP_CALL] call rejected');
  await expect(err).toContainText('hint: check the args');
  await expect(err).toContainText('docs: docs/ERRORS.md#owasidea_e_http_call');
});

test('services panel restarts a service and shows the failure reason', async ({ page }) => {
  await page.route('http://127.0.0.1:8787/plugins', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify([
      { name: 'heartbeat', language: 'rust', type: 'service', runtime: 'wasi-component', exports: [] },
    ]),
  }));
  let restarts = 0;
  await page.route('http://127.0.0.1:8787/plugin/heartbeat/service/status', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({ running: false, address: null, last_error: 'no start-service export', restarts }),
  }));
  await page.route('http://127.0.0.1:8787/plugin/heartbeat/service/restart', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({ running: true, address: null, last_error: null, restarts: ++restarts }),
  }));

  await page.goto('/#/services');
  await expect(page.locator('button[data-restart="heartbeat"]')).toBeVisible();
  await expect(page.locator('p[data-svc-state="heartbeat"]')).toContainText('last error: no start-service export');
  await page.locator('button[data-restart="heartbeat"]').click();
  await expect(page.locator('p[data-svc-state="heartbeat"]')).toContainText('running');
  await expect(page.locator('p[data-svc-state="heartbeat"]')).toContainText('restarts: 1');
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

test('build skew banner shown when UI and host build ids differ', async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__OWASIDEA__ = { host: 'http://127.0.0.1:8787', app: 'Test', version: '1', build_id: 'old' };
  });
  await page.route('http://127.0.0.1:8787/health', route => route.fulfill({ status: 200, body: 'ok' }));
  await page.route('http://127.0.0.1:8787/plugins', route => route.fulfill({ status: 200, contentType: 'application/json', body: '[]' }));
  await page.route('http://127.0.0.1:8787/overview', route => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ plugins: 0, libraries: 0, executables: 0, services: 0, runtimes: [], build_id: 'new' }) }));
  await page.goto('/');
  await expect(page.locator('body')).toContainText('old');
  await expect(page.locator('body')).toContainText('new');
  await expect(page.locator('body')).toContainText('anterior');
});

test('no build skew banner when ids match', async ({ page }) => {
  await page.addInitScript(() => {
    (window as any).__OWASIDEA__ = { host: 'http://127.0.0.1:8787', app: 'Test', version: '1', build_id: 'same' };
  });
  await page.route('http://127.0.0.1:8787/health', route => route.fulfill({ status: 200, body: 'ok' }));
  await page.route('http://127.0.0.1:8787/plugins', route => route.fulfill({ status: 200, contentType: 'application/json', body: '[]' }));
  await page.route('http://127.0.0.1:8787/overview', route => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ plugins: 0, libraries: 0, executables: 0, services: 0, runtimes: [], build_id: 'same' }) }));
  await page.goto('/');
  await expect(page.locator('body')).not.toContainText('anterior');
});
