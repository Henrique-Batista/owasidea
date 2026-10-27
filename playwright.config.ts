import { defineConfig, devices } from '@playwright/test';

const uiUrl = process.env.OWASIDEA_UI_URL ?? 'http://127.0.0.1:5174';
const hostUrl = process.env.OWASIDEA_HOST ?? 'http://127.0.0.1:8787';
const uiPort = new URL(uiUrl).port || '5174';

export default defineConfig({
  testDir: './tests',
  testMatch: '**/*.spec.ts',
  // Probes browser launchability once per run. WebKit needs host libraries
  // (`npx playwright install-deps webkit`); when they are missing the bridge
  // suite skips WebKit with an explicit reason instead of claiming coverage.
  globalSetup: './tests/support/browser-probe.ts',
  use: { baseURL: uiUrl },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
  webServer: [
    {
      command: `node node_modules/vite/bin/vite.js --host 127.0.0.1 --port ${uiPort} --strictPort`,
      cwd: './ui',
      url: uiUrl,
      reuseExistingServer: !process.env.CI,
      timeout: 30000,
      env: {
        OWASIDEA_HOST: hostUrl,
      },
    },
  ],
});
