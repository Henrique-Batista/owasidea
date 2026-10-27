import { defineConfig } from '@playwright/test';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const owasidea = process.env.OWASIDEA_BIN ?? 'owasidea';
const uiUrl = process.env.OWASIDEA_UI_URL ?? 'http://127.0.0.1:5180';
const hostUrl = process.env.OWASIDEA_HOST ?? 'http://127.0.0.1:8790';
const uiPort = new URL(uiUrl).port || '5180';
const hostPort = new URL(hostUrl).port || '8790';

export default defineConfig({
  testDir: './tests',
  timeout: 45_000,
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: uiUrl,
    trace: 'retain-on-failure',
  },
  webServer: [
    {
      command: `"${owasidea}" --config wasm-app.toml --http-addr 127.0.0.1:${hostPort} run`,
      url: `http://127.0.0.1:${hostPort}/health`,
      reuseExistingServer: true,
      timeout: 120_000,
      env: {
        OWASIDEA_DATA_DIR: join(tmpdir(), 'owasidea-shipyard-playwright'),
        OWASIDEA_ALLOWED_ORIGINS: uiUrl,
        OWASIDEA_PUBLIC_KEY: 'keys/owasidea.pub',
        OWASIDEA_REQUIRE_SIGNATURES: 'true',
      },
    },
    {
      command: `node node_modules/vite/bin/vite.js --host 127.0.0.1 --port ${uiPort} --strictPort`,
      cwd: './ui',
      url: uiUrl,
      reuseExistingServer: true,
      timeout: 30_000,
      env: {
        OWASIDEA_HOST: hostUrl,
      },
    },
  ],
});
