import { defineConfig } from '@playwright/test';

const uiUrl = process.env.OWASIDEA_UI_URL ?? 'http://127.0.0.1:5174';
const hostUrl = process.env.OWASIDEA_HOST ?? 'http://127.0.0.1:8787';
const uiPort = new URL(uiUrl).port || '5174';

export default defineConfig({
  testDir: './tests',
  testMatch: '**/*.spec.ts',
  use: { baseURL: uiUrl },
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
