import { expect, test, type APIRequestContext } from '@playwright/test';

const HOST = process.env.OWASIDEA_HOST ?? 'http://127.0.0.1:8802';

async function clearNotes(request: APIRequestContext) {
  const response = await request.post(`${HOST}/plugin/notes/call`, {
    data: { function: 'clear-notes', args: [] },
  });
  expect(response.ok()).toBeTruthy();
}

test.beforeEach(async ({ request }) => {
  await clearNotes(request);
});

test('adds notes and lists them in order', async ({ page }) => {
  await page.goto('/');
  await page.getByLabel('New note').fill('buy milk');
  await page.getByRole('button', { name: 'Add' }).click();
  await page.getByLabel('New note').fill('ship it');
  await page.getByRole('button', { name: 'Add' }).click();
  await expect(page.getByTestId('notes').locator('li')).toHaveText(['buy milk', 'ship it']);
  await expect(page.getByTestId('count')).toHaveText('2 notes');
});

test('ignores blank notes', async ({ page }) => {
  await page.goto('/');
  await page.getByLabel('New note').fill('   ');
  await page.getByRole('button', { name: 'Add' }).click();
  await expect(page.getByTestId('count')).toHaveText('0 notes');
});

test('notes survive a page reload (host key/value store)', async ({ page }) => {
  await page.goto('/');
  await page.getByLabel('New note').fill('persist me');
  await page.getByRole('button', { name: 'Add' }).click();
  await expect(page.getByTestId('notes')).toContainText('persist me');
  await page.reload();
  await expect(page.getByTestId('notes')).toContainText('persist me');
});

test('clear removes every note', async ({ page }) => {
  await page.goto('/');
  await page.getByLabel('New note').fill('temporary');
  await page.getByRole('button', { name: 'Add' }).click();
  await expect(page.getByTestId('count')).toHaveText('1 note');
  await page.getByRole('button', { name: 'Clear all' }).click();
  await expect(page.getByTestId('count')).toHaveText('0 notes');
});
