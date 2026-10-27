import { expect, test } from '@playwright/test';

test.describe('hello example', () => {
  test('greets through the greeter component', async ({ page }) => {
    await page.goto('/');
    await page.getByLabel('Name').fill('Ana');
    await page.getByRole('button', { name: 'Greet' }).click();
    await expect(page.getByTestId('greeting')).toHaveText('Hello, Ana! (from Rust)');
  });

  test('sums two numbers via the sum export', async ({ page }) => {
    await page.goto('/');
    await page.getByLabel('First number').fill('19');
    await page.getByLabel('Second number').fill('23');
    await page.getByRole('button', { name: 'Sum' }).click();
    await expect(page.getByTestId('total')).toHaveText('42');
  });

  test('shows the error state when the call fails', async ({ page }) => {
    await page.route('**/plugin/greeter/call', route =>
      route.fulfill({ status: 404, body: 'unknown plugin greeter' }),
    );
    await page.goto('/');
    await page.getByRole('button', { name: 'Greet' }).click();
    const output = page.getByTestId('greeting');
    await expect(output).toContainText('404');
    await expect(output).toHaveClass(/error/);
  });
});
