import { expect, test, type APIRequestContext } from '@playwright/test';

const HOST = process.env.OWASIDEA_HOST ?? 'http://127.0.0.1:8803';

async function stopService(request: APIRequestContext) {
  await request.post(`${HOST}/plugin/ticker/service/stop`);
}

test.beforeEach(async ({ request }) => {
  await stopService(request);
});

test.afterEach(async ({ request }) => {
  await stopService(request);
});

test('start and stop move the status', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByTestId('status')).toHaveText('stopped');
  await page.getByRole('button', { name: 'Start' }).click();
  await expect(page.getByTestId('status')).toHaveText('running');
  await page.getByRole('button', { name: 'Stop' }).click();
  await expect(page.getByTestId('status')).toHaveText('stopped');
});

test('start is repeat-safe', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Start' }).click();
  await expect(page.getByTestId('status')).toHaveText('running');
  await page.getByRole('button', { name: 'Start' }).click();
  await expect(page.getByTestId('status')).toHaveText('running');
  await expect(page.getByRole('status')).toHaveText('start ok');
});

test('reload is refused with 409 while the service runs', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Start' }).click();
  await expect(page.getByTestId('status')).toHaveText('running');
  await page.getByRole('button', { name: 'Reload component' }).click();
  await expect(page.getByRole('status')).toContainText('409');
  await expect(page.getByRole('status')).toHaveClass(/error/);
});

test('reload succeeds once the service is stopped', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Start' }).click();
  await expect(page.getByTestId('status')).toHaveText('running');
  await page.getByRole('button', { name: 'Stop' }).click();
  await expect(page.getByTestId('status')).toHaveText('stopped');
  await page.getByRole('button', { name: 'Reload component' }).click();
  await expect(page.getByRole('status')).toHaveText('reload ok');
});
