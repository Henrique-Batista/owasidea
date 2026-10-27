import { expect, test } from '@playwright/test';

test('workbench calls every language and persists a report', async ({ page }) => {
  await page.goto('/#/workbench');
  await expect(page.locator('body')).toContainText('SHIPYARD');
  await expect(page.locator('#host-pill')).toContainText('host ok');
  await page.locator('#run-workbench').click();
  await expect(page.locator('#run-status')).toContainText('completed', { timeout: 30_000 });

  const results = page.locator('#run-results');
  await expect(results).toContainText('Readiness score');
  await expect(results).toContainText('composed Go→Rust pipeline');
  await expect(results).toContainText('Dependency audit');
  await expect(results).toContainText('Changelog renderer');
  await expect(results).toContainText('Policy classification');
  await expect(results).toContainText('Next semantic version');
  await expect(results).toContainText('Release fingerprint');
  await expect(results).toContainText('Changelog metrics');
  await expect(results).toContainText('reports/');
});

test('host status and page recover after a delayed host start', async ({ page }) => {
  const failFirst = (pattern: string) => {
    let remaining = 2;
    return page.route(pattern, route => {
      if (remaining-- > 0) return route.abort('failed');
      return route.continue();
    });
  };
  await failFirst('http://127.0.0.1:8790/health');
  await failFirst('http://127.0.0.1:8790/plugins');

  await page.goto('/#/workbench');
  await expect(page.locator('#host-pill')).toContainText('host offline');
  await expect(page.locator('#host-pill')).toContainText('host ok', { timeout: 15_000 });
  await expect(page.locator('#run-workbench')).toBeVisible();
});

test('isolated Svelte and React bundles verify before mounting', async ({ page }) => {
  await page.goto('/#/ui');
  await expect(page.locator('[data-verify="board-svelte"]')).toContainText('assets verified', { timeout: 30_000 });
  await expect(page.locator('[data-verify="map-react"]')).toContainText('assets verified', { timeout: 30_000 });

  await page.locator('button.open-ui[data-ui="board-svelte"]').click();
  await expect(page.locator('iframe[title="board-svelte isolated UI"]')).toHaveAttribute('sandbox', 'allow-scripts allow-forms');

  await page.locator('button.open-ui[data-ui="map-react"]').click();
  await expect(page.locator('iframe[title="map-react isolated UI"]')).toBeVisible();
});

test('runtime controls drive the service and executable plugin', async ({ page }) => {
  await page.goto('/#/runtime');
  await page.locator('#service-start').click();
  await expect(page.locator('#service-output')).toContainText('persistent', { timeout: 15_000 });
  await page.locator('#service-status').click();
  await expect(page.locator('#service-output')).toContainText('running', { timeout: 15_000 });
  await page.locator('#service-stop').click();
  await expect(page.locator('#service-output')).toContainText('stopped', { timeout: 15_000 });

  await page.locator('#webhook-health').click();
  await expect(page.locator('#webhook-output')).toContainText('shipyard-webhook', { timeout: 15_000 });
  await page.locator('#webhook-event').click();
  await expect(page.locator('#webhook-output')).toContainText('accepted', { timeout: 15_000 });
});

test('isolated UI still verifies when WebCrypto is unavailable', async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(Crypto.prototype, 'subtle', { get: () => undefined });
  });
  await page.goto('/#/ui');
  await expect(page.locator('[data-verify="board-svelte"]')).toContainText('JS fallback', { timeout: 30_000 });
  await expect(page.locator('button.open-ui[data-ui="board-svelte"]')).toBeEnabled();
  await page.locator('button.open-ui[data-ui="board-svelte"]').click();
  await expect(page.locator('iframe[title="board-svelte isolated UI"]')).toBeVisible();
});

test('board-svelte calls changelog-ts.render via bridge', async ({ page }) => {
  await page.goto('/#/ui');
  await page.locator('button.open-ui[data-ui="board-svelte"]').click();
  const frame = page.frameLocator('iframe[title="board-svelte isolated UI"]');
  await page.waitForTimeout(2000);
  await frame.locator('button:has-text("Render changelog (bridge)")').click();
  await expect(frame.locator('pre')).toContainText('release notes', { timeout: 30_000 });
});

test('inventory exposes manifests and logs explain their source', async ({ page }) => {
  await page.goto('/#/inventory');
  await expect(page.locator('.plugin-card')).toHaveCount(9);
  await expect(page.locator('body')).toContainText('Go dependency audit');
  await page.locator('button.inspect[data-plugin="core-rust"]').click();
  await expect(page.locator('#manifest-json')).toContainText('shipyard:core/readiness');

  await page.goto('/#/logs');
  await expect(page.locator('#log-plugin')).toBeVisible();
  await expect(page.locator('#log-output')).not.toHaveText('connecting…', { timeout: 15_000 });
  await expect(page.locator('body')).toContainText('host-log');
});
