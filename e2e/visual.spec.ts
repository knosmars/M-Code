import { test, expect } from '@playwright/test';

// Visual regression baselines (local-only; run via `npm run test:visual`).
// Not part of the CI `test:e2e` (chromium project) — see playwright.config.ts.
//
// Snapshotted views boot from a clean, deterministic state with no dynamic
// content (no timestamps / active "thinking"), so the baselines are stable.
// The Settings panel renders via the settings IPC stubbed in mockBackend.ts
// (empty MCP servers, not-indexed semantic, configured providers).

test('welcome dashboard visual', async ({ page }) => {
  await page.goto('/');
  // app booted: composer textbox visible
  await expect(page.getByRole('textbox').first()).toBeVisible();
  // settle (animations disabled globally; small wait for layout)
  await page.waitForTimeout(300);
  await expect(page).toHaveScreenshot('welcome.png');
});

test('settings panel visual', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('textbox').first()).toBeVisible();
  await page.getByRole('button', { name: 'Menu' }).click();
  await page.getByRole('menuitem', { name: 'Settings' }).click();
  // settle on settings view (panel's hardcoded h1 is stable, non-i18n)
  await expect(page.getByRole('heading', { name: 'Settings', level: 1 })).toBeVisible();
  await page.waitForTimeout(300);
  await expect(page).toHaveScreenshot('settings.png');
});
