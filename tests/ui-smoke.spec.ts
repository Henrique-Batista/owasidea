import { test, expect } from '@playwright/test';

test('plugin list renders', async ({ page }) => {
  await page.goto('http://localhost:5173/#/plugins');
  await expect(page.locator('body')).toContainText('notes');
  await expect(page.locator('body')).toContainText('chat-go');
});