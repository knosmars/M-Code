import { defineConfig, devices } from '@playwright/test';

// Bypass any system proxy for localhost — prevents the webServer readiness
// probe from hitting a forwarding proxy instead of the Vite dev server.
process.env.NO_PROXY = [process.env.NO_PROXY, 'localhost,127.0.0.1']
  .filter(Boolean).join(',');
process.env.no_proxy = [process.env.no_proxy, 'localhost,127.0.0.1']
  .filter(Boolean).join(',');

export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: 'list',
  use: {
    baseURL: 'http://localhost:5173',
    trace: 'on-first-retry',
  },
  expect: {
    toHaveScreenshot: { animations: 'disabled', maxDiffPixels: 200 },
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
      testIgnore: /visual\.spec\.ts/,
    },
    {
      name: 'visual',
      use: { ...devices['Desktop Chrome'] },
      testMatch: /visual\.spec\.ts/,
    },
  ],
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    env: {
      VITE_E2E: '1',
      // Bypass any system proxy for localhost so the readiness probe works
      NO_PROXY: 'localhost,127.0.0.1',
      no_proxy: 'localhost,127.0.0.1',
    },
  },
});
