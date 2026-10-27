import { defineConfig } from '@playwright/test';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const owasidea = process.env.OWASIDEA_BIN ?? 'owasidea';
const uiUrl = process.env.OWASIDEA_UI_URL ?? 'http://127.0.0.1:5203';
const hostUrl = process.env.OWASIDEA_HOST ?? 'http://127.0.0.1:8803';
const uiPort = new URL(uiUrl).port || '5203';
const hostPort = new URL(hostUrl).port || '8803';

export default defineConfig({
  testDir: './tests',
  timeout: 45_000,
  fullyParallel: false,
  workers: 1,
  use: { baseURL: uiUrl, trace: 'retain-on-failure' },
  webServer: [
    {
      command: `"${owasidea}" --config wasm-app.toml --http-addr 127.0.0.1:${hostPort} run`,
      url: `${hostUrl}/health`,
      reuseExistingServer: !process.env.CI,
      timeout: 120_000,
      env: {
        OWASIDEA_DATA_DIR: join(tmpdir(), 'owasidea-example-ticker'),
        OWASIDEA_ALLOWED_ORIGINS: uiUrl,
      },
    },
    {
      command: `node node_modules/vite/bin/vite.js --host 127.0.0.1 --port ${uiPort} --strictPort`,
      cwd: './ui',
      url: uiUrl,
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
      env: { OWASIDEA_HOST: hostUrl },
    },
  ],
});
