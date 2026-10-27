import { test, expect } from '@playwright/test';
import { createHash } from 'node:crypto';

test('UI plugin bridge allowed call renders result in iframe', async ({ page }) => {
  const html = `<!doctype html><html><body><div id="out">pending</div><script>
window.addEventListener('message', (e) => {
  if (e.data && e.data.type === 'owasidea:bridge:init') {
    const port = e.data.port;
    port.postMessage({ type: 'owasidea:bridge:hello', nonce: e.data.nonce });
    port.onmessage = (ev) => {
      const d = ev.data;
      if (d && d.ok) {
        document.getElementById('out').textContent = (d.result != null ? d.result : JSON.stringify(d.results));
      } else if (d && d.error) {
        document.getElementById('out').textContent = d.error.code;
      }
    };
    setTimeout(() => port.postMessage({ id: 'r1', plugin: 'notes', function: 'greet', args: ['Ada'] }), 50);
  }
});
</script></body></html>`;
  const bytes = Buffer.from(html);
  const fileDigest = createHash('sha256').update(bytes).digest('hex');
  const bundleDigest = createHash('sha256').update(`index.html\0${fileDigest}\n`).digest('hex');

  await page.route('http://127.0.0.1:8787/ui-plugins', route => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify([{
      name: 'bridged',
      language: 'yew',
      version: '1.0.0',
      entry: 'plugins/bridged/index.html',
      sha256: bundleDigest,
      files: { 'index.html': fileDigest },
      bridge: ['notes.greet'],
    }]),
  }));
  await page.route('**/plugins/bridged/index.html', route => route.fulfill({
    status: 200,
    contentType: 'text/html',
    body: bytes,
  }));
  await page.route('http://127.0.0.1:8787/plugin/notes/call', async route => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ result: 'Hello, Ada' }),
    });
  });

  await page.goto('/#/ui/bridged');
  const frame = page.frameLocator('iframe[title="bridged UI"]');
  await expect(frame.locator('#out')).toHaveText('Hello, Ada', { timeout: 10000 });
});
