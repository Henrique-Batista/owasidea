import { test, expect, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const HOST = 'http://127.0.0.1:8787';
const WEBKIT_MARKER = join(process.cwd(), 'test-results', 'webkit-unavailable.txt');
const SDK_SOURCE = readFileSync(join(process.cwd(), 'ui', 'public', 'owasidea-bridge.js'), 'utf8');

// WebKit is only claimed as validated when the engine actually launches; the
// probe in globalSetup writes this marker when host libraries are missing.
test.skip(
  ({ browserName }) => browserName === 'webkit' && existsSync(WEBKIT_MARKER),
  'WebKit cannot launch on this host (missing browser dependencies); see test-results/webkit-unavailable.txt'
);

function bundle(html: string) {
  const bytes = Buffer.from(html);
  const fileDigest = createHash('sha256').update(bytes).digest('hex');
  const bundleDigest = createHash('sha256').update(`index.html\0${fileDigest}\n`).digest('hex');
  return { bytes, fileDigest, bundleDigest };
}

// Minimal SDK stand-in for tests that drive the raw protocol. It answers the
// parent's probe like the shipped SDK does, then speaks hello/request.
const READY_STUB = `
const __bridgeNonce = new URLSearchParams(location.search).get('__owasidea_bridge');
window.addEventListener('message', (e) => {
  if (e.source !== window.parent) return;
  if (e.data && e.data.type === 'owasidea:bridge:probe') {
    window.parent.postMessage(
      { type: 'owasidea:bridge:ready', version: 1, nonce: __bridgeNonce },
      '*'
    );
  }
});
`;

type MockOptions = { bridge?: string[]; timeoutMs?: number; hostDelayMs?: number; hostResult?: string };

async function mockUiPlugin(
  page: Page,
  html: string,
  options: MockOptions = {}
): Promise<{ hostCalls: () => number }> {
  const { bytes, fileDigest, bundleDigest } = bundle(html);
  let hostCalls = 0;

  await page.route(`${HOST}/ui-plugins`, route =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify([
        {
          name: 'bridged',
          language: 'yew',
          version: '1.0.0',
          entry: 'plugins/bridged/index.html',
          sha256: bundleDigest,
          files: { 'index.html': fileDigest },
          bridge: options.bridge ?? ['notes.greet'],
          ...(options.timeoutMs === undefined ? {} : { timeoutMs: options.timeoutMs }),
        },
      ]),
    })
  );
  await page.route('**/plugins/bridged/index.html*', route =>
    route.fulfill({ status: 200, contentType: 'text/html', body: bytes })
  );
  await page.route('**/plugins/bridged/owasidea-bridge.js', route =>
    route.fulfill({ status: 200, contentType: 'text/javascript', body: SDK_SOURCE })
  );
  await page.route(`${HOST}/plugin/notes/call`, async route => {
    hostCalls++;
    await new Promise(r => setTimeout(r, options.hostDelayMs ?? 250));
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ result: options.hostResult ?? 'Hello, Ada' }),
    });
  });

  return { hostCalls: () => hostCalls };
}

async function openPlugin(page: Page) {
  await page.goto('/#/ui/bridged');
  return page.frameLocator('iframe[title="bridged UI"]');
}

test('UI plugin bridge allowed call renders result in iframe', async ({ page }) => {
  // Uses the shipped SDK end to end (probe/ready, init source+nonce checks).
  const html = `<!doctype html><html><body><div id="out">pending</div><script type="module">
import { connect } from './owasidea-bridge.js';
const out = document.getElementById('out');
try {
  const bridge = await connect();
  out.textContent = String(await bridge.call('notes', 'greet', ['Ada']));
} catch (e) {
  out.textContent = (e && e.code) || String(e);
}
</script></body></html>`;
  await mockUiPlugin(page, html);
  const frame = await openPlugin(page);
  await expect(frame.locator('#out')).toHaveText('Hello, Ada', { timeout: 10000 });
});

test('UI plugin bridge cancel returns CANCELLED', async ({ page }) => {
  const html = `<!doctype html><html><body><div id="out">pending</div><script>${READY_STUB}
window.addEventListener('message', (e) => {
  if (!e.data || e.data.type !== 'owasidea:bridge:init') return;
  const port = e.data.port;
  port.postMessage({ type: 'owasidea:bridge:hello', nonce: e.data.nonce });
  port.onmessage = (ev) => {
    const d = ev.data;
    if (d && d.error) document.getElementById('out').textContent = d.error.code;
    else if (d && d.ok) document.getElementById('out').textContent = 'OK';
  };
  setTimeout(() => {
    port.postMessage({ id: 'c1', plugin: 'notes', function: 'greet', args: ['Ada'] });
    setTimeout(() => port.postMessage({ id: 'c1', type: 'cancel' }), 30);
  }, 50);
});
</script></body></html>`;
  await mockUiPlugin(page, html, { hostDelayMs: 5000 });
  const frame = await openPlugin(page);
  await expect(frame.locator('#out')).toHaveText('CANCELLED', { timeout: 10000 });
});

test('UI plugin bridge timeout uses the 10s default and returns TIMEOUT', async ({ page }) => {
  const html = `<!doctype html><html><body><div id="out">pending</div><script>${READY_STUB}
window.addEventListener('message', (e) => {
  if (!e.data || e.data.type !== 'owasidea:bridge:init') return;
  const port = e.data.port;
  port.postMessage({ type: 'owasidea:bridge:hello', nonce: e.data.nonce });
  port.onmessage = (ev) => {
    const d = ev.data;
    if (d && d.error) document.getElementById('out').textContent = d.error.code;
    else if (d && d.ok) document.getElementById('out').textContent = 'OK';
  };
  setTimeout(() => port.postMessage({ id: 't1', plugin: 'notes', function: 'greet', args: ['Ada'] }), 50);
});
</script></body></html>`;
  await mockUiPlugin(page, html, { hostDelayMs: 11000 });
  const started = Date.now();
  const frame = await openPlugin(page);
  await expect(frame.locator('#out')).toHaveText('TIMEOUT', { timeout: 15000 });
  // A manifest that declares no timeout must not shorten the 10 s default.
  expect(Date.now() - started).toBeGreaterThan(9000);
});

test('UI plugin bridge honours a manifest timeout below the 30s cap', async ({ page }) => {
  const html = `<!doctype html><html><body><div id="out">pending</div><script>${READY_STUB}
window.addEventListener('message', (e) => {
  if (!e.data || e.data.type !== 'owasidea:bridge:init') return;
  const port = e.data.port;
  port.postMessage({ type: 'owasidea:bridge:hello', nonce: e.data.nonce });
  port.onmessage = (ev) => {
    const d = ev.data;
    if (d && d.error) document.getElementById('out').textContent = d.error.code;
    else if (d && d.ok) document.getElementById('out').textContent = 'OK';
  };
  setTimeout(() => port.postMessage({ id: 'm1', plugin: 'notes', function: 'greet', args: ['Ada'] }), 50);
});
</script></body></html>`;
  await mockUiPlugin(page, html, { timeoutMs: 1200, hostDelayMs: 6000 });
  const started = Date.now();
  const frame = await openPlugin(page);
  await expect(frame.locator('#out')).toHaveText('TIMEOUT', { timeout: 5000 });
  expect(Date.now() - started).toBeLessThan(5000);
});

test('duplicate request ids are rejected (no replay)', async ({ page }) => {
  const html = `<!doctype html><html><body><div id="out">pending</div><script>${READY_STUB}
window.addEventListener('message', (e) => {
  if (!e.data || e.data.type !== 'owasidea:bridge:init') return;
  const port = e.data.port;
  port.postMessage({ type: 'owasidea:bridge:hello', nonce: e.data.nonce });
  const codes = [];
  port.onmessage = (ev) => {
    const d = ev.data;
    if (d && d.error) {
      codes.push(d.error.code);
      document.getElementById('out').textContent = codes.join(',');
    }
  };
  setTimeout(() => {
    port.postMessage({ id: 'dup', plugin: 'notes', function: 'greet', args: ['Ada'] });
    setTimeout(() => port.postMessage({ id: 'dup', plugin: 'notes', function: 'greet', args: ['Ada'] }), 60);
  }, 50);
});
</script></body></html>`;
  await mockUiPlugin(page, html);
  const frame = await openPlugin(page);
  await expect(frame.locator('#out')).toHaveText('INVALID_REQUEST', { timeout: 10000 });
});

test('navigating the verified iframe to a remote origin never receives bridge:init', async ({ page }) => {
  const html = `<!doctype html><html><body><div id="out">pending</div><script>${READY_STUB}
window.addEventListener('message', (e) => {
  if (!e.data || e.data.type !== 'owasidea:bridge:init') return;
  const port = e.data.port;
  port.postMessage({ type: 'owasidea:bridge:hello', nonce: e.data.nonce });
  port.onmessage = (ev) => {
    const d = ev.data;
    if (d && d.ok) {
      console.log('bridge:first-ok');
      setTimeout(() => { location.href = 'https://example.com/'; }, 50);
    }
  };
  setTimeout(() => port.postMessage({ id: 'n1', plugin: 'notes', function: 'greet', args: ['Ada'] }), 30);
});
</script></body></html>`;
  const remote = `<!doctype html><html><body><div id="remote">remote</div><script>
window.__inits = 0;
window.addEventListener('message', (e) => {
  if (e.data && e.data.type === 'owasidea:bridge:init') window.__inits++;
});
// A protocol-aware remote origin must still not obtain the port.
try { window.parent.postMessage({ type: 'owasidea:bridge:ready', version: 1 }, '*'); } catch (e) {}
</script></body></html>`;

  const mock = await mockUiPlugin(page, html);
  await page.route('https://example.com/**', route =>
    route.fulfill({ status: 200, contentType: 'text/html', body: remote })
  );

  const firstOk = page.waitForEvent('console', {
    predicate: m => m.text() === 'bridge:first-ok',
    timeout: 10000,
  });
  await openPlugin(page);
  await firstOk;
  expect(mock.hostCalls()).toBe(1);

  // Wait for the cross-origin navigation to settle, then inspect the remote doc.
  await expect
    .poll(() => page.frames().some(f => f.url().startsWith('https://example.com')), { timeout: 10000 })
    .toBe(true);
  const remoteFrame = page.frames().find(f => f.url().startsWith('https://example.com'))!;
  await page.waitForTimeout(1500);

  expect(await remoteFrame.evaluate(() => (window as any).__inits)).toBe(0);
  expect(mock.hostCalls()).toBe(1);
});

test('a plugin that navigates during its own load never receives bridge:init', async ({ page }) => {
  // The frame's only load event belongs to the remote document here, so the
  // parent cannot rely on load counting: the document-bound nonce is what stops
  // the port transfer.
  const html = `<!doctype html><html><body>plugin<script>
location.href = 'https://example.com/';
</script></body></html>`;
  const remote = `<!doctype html><html><body><div id="remote">remote</div><script>
window.__inits = 0;
window.addEventListener('message', (e) => {
  if (e.data && e.data.type === 'owasidea:bridge:init') window.__inits++;
});
try { window.parent.postMessage({ type: 'owasidea:bridge:ready', version: 1 }, '*'); } catch (e) {}
</script></body></html>`;
  const mock = await mockUiPlugin(page, html);
  await page.route('https://example.com/**', route =>
    route.fulfill({ status: 200, contentType: 'text/html', body: remote })
  );

  await openPlugin(page);
  await expect
    .poll(() => page.frames().some(f => f.url().startsWith('https://example.com')), { timeout: 10000 })
    .toBe(true);
  const remoteFrame = page.frames().find(f => f.url().startsWith('https://example.com'))!;
  await page.waitForTimeout(2000);

  expect(await remoteFrame.evaluate(() => (window as any).__inits)).toBe(0);
  expect(mock.hostCalls()).toBe(0);
});

test('control messages from a window other than the iframe do not start a handshake', async ({ page }) => {
  const html = `<!doctype html><html><body><div id="out">pending</div><script>
window.__inits = 0;
window.addEventListener('message', (e) => {
  if (e.data && e.data.type === 'owasidea:bridge:init') window.__inits++;
});
</script></body></html>`;
  const mock = await mockUiPlugin(page, html);
  await openPlugin(page);
  // No init is sent before the SDK proves it holds the mounted nonce.
  await page.waitForTimeout(1500);
  const frames = page.frames().filter(f => f.url().includes('/plugins/bridged/'));
  expect(frames).toHaveLength(1);
  expect(await frames[0].evaluate(() => (window as any).__inits)).toBe(0);

  // Even a ready message carrying the real nonce must be ignored when it comes
  // from the dashboard window instead of the iframe contentWindow.
  const nonce = await page.evaluate(() => {
    const src = (document.querySelector('iframe[title="bridged UI"]') as HTMLIFrameElement).src;
    return new URL(src).searchParams.get('__owasidea_bridge');
  });
  expect(nonce).toBeTruthy();
  await page.evaluate(n => {
    for (let i = 0; i < 5; i++) {
      window.postMessage({ type: 'owasidea:bridge:ready', version: 1, nonce: n }, '*');
    }
  }, nonce);
  await page.waitForTimeout(800);

  expect(await frames[0].evaluate(() => (window as any).__inits)).toBe(0);
  expect(mock.hostCalls()).toBe(0);
});

test('reloading the verified iframe does not re-arm the bridge', async ({ page }) => {
  const html = `<!doctype html><html><body><div id="out">pending</div><script>${READY_STUB}
window.__inits = 0;
window.addEventListener('message', (e) => {
  if (!e.data || e.data.type !== 'owasidea:bridge:init') return;
  window.__inits++;
  const port = e.data.port;
  port.postMessage({ type: 'owasidea:bridge:hello', nonce: e.data.nonce });
  port.onmessage = (ev) => {
    const d = ev.data;
    if (d && d.ok) {
      console.log('bridge:first-ok');
      setTimeout(() => location.reload(), 50);
    }
  };
  setTimeout(() => port.postMessage({ id: 'rl' + Math.random(), plugin: 'notes', function: 'greet', args: ['Ada'] }), 30);
});
</script></body></html>`;
  const mock = await mockUiPlugin(page, html);
  const navigations: string[] = [];
  page.on('framenavigated', f => {
    if (f.url().includes('/plugins/bridged/')) navigations.push(f.url());
  });
  const firstOk = page.waitForEvent('console', {
    predicate: m => m.text() === 'bridge:first-ok',
    timeout: 10000,
  });
  await openPlugin(page);
  await firstOk;
  await expect.poll(() => navigations.length, { timeout: 10000 }).toBeGreaterThanOrEqual(2);
  await page.waitForTimeout(1500);

  // The reloaded document starts with __inits = 0: the parent must not have
  // sent a second init, and the pre-reload call must be the only host call.
  const frames = page.frames().filter(f => f.url().includes('/plugins/bridged/'));
  expect(frames).toHaveLength(1);
  expect(await frames[0].evaluate(() => (window as any).__inits)).toBe(0);
  expect(mock.hostCalls()).toBe(1);
  expect(navigations.length).toBeGreaterThanOrEqual(2);
});
