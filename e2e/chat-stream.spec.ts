import { test, expect } from '@playwright/test';

test('sends a message and renders the streamed assistant reply', async ({ page }) => {
  await page.goto('/');

  // App boots; provider auto-configured via mocked get_api_key. Locate composer.
  const input = page.getByRole('textbox').first();
  await expect(input).toBeVisible();
  await input.fill('hi there');

  await page.getByRole('button', { name: 'Send message' }).click();

  // Mocked stream_chat pushes "Hello, world!" in deltas.
  await expect(page.getByText('Hello, world!')).toBeVisible({ timeout: 10_000 });
});
