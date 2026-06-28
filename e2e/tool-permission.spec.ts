import { test, expect } from '@playwright/test';

test('tool call triggers permission dialog; approve continues', async ({ page }) => {
  await page.addInitScript(() => { window.__E2E_SCENARIO__ = 'tool'; });
  await page.goto('/');

  const input = page.getByRole('textbox').first();
  await input.fill('write a file');
  await page.getByRole('button', { name: 'Send message' }).click();

  // Side-effect tool → permission dialog.
  const dialog = page.getByRole('dialog', { name: 'Permission request' });
  await expect(dialog).toBeVisible({ timeout: 10_000 });
  await expect(dialog.getByText('write_file').first()).toBeVisible();

  await dialog.getByRole('button', { name: 'Approve Once' }).click();

  // Tool runs (mocked), second stream round renders the follow-up.
  await expect(page.getByText('完成。')).toBeVisible({ timeout: 10_000 });
});
