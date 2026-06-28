import { test, expect } from '@playwright/test';

test('persists a session across reload and restores its messages', async ({ page }) => {
  await page.goto('/');

  // Send a message — lazily creates + persists a session (with messages).
  const input = page.getByRole('textbox').first();
  await expect(input).toBeVisible();
  await input.fill('hi there');
  await page.getByRole('button', { name: 'Send message' }).click();

  // Assistant reply streamed in (mock pushes "Hello, world!").
  await expect(page.getByText('Hello, world!')).toBeVisible({ timeout: 10_000 });

  // Reload — sessionStore.loadSessions() should re-read the persisted session.
  await page.reload();

  // Reload deliberately does NOT auto-select a past session; the restored
  // session appears in the Recents list. The session is auto-titled from the
  // first user message ("hi there"), so we locate it in the nav recents list
  // by that title text.
  const recent = page.locator('nav').getByText('hi there').first();
  await expect(recent).toBeVisible({ timeout: 10_000 });

  // Selecting it restores the persisted messages.
  await recent.click();
  // Assert messages are restored in the chat log (message bubbles).
  const chatLog = page.getByRole('log');
  await expect(chatLog.getByText('hi there')).toBeVisible();
  await expect(chatLog.getByText('Hello, world!')).toBeVisible();
});
