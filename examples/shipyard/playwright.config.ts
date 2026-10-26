import { defineConfig } from '@playwright/test';
import { join } from 'node:path';
import { tmpdir } from 'node:os';

const owasidea = process.env.OWASIDEA_BIN ?? 'owasidea';

export default defineConfig({
  testDir: './tests',
  timeout: 45_000,
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: 'http://127.0.0.1:5180',
    trace: 'retain-on-failure',
  },
  webServer: [
    {
      command: `"${owasidea}" --config wasm-app.toml --http-addr 127.0.0.1:8790 run`,
      url: 'http://127.0.0.1:8790/health',
      reuseExistingServer: true,
      timeout: 120_000,
      env: {
        OWASIDEA_DATA_DIR: join(tmpdir(), 'owasidea-shipyard-playwright'),
        OWASIDEA_ALLOWED_ORIGINS: 'http://127.0.0.1:5180',
      },
    },
    {
      command: 'node node_modules/vite/bin/vite.js --host 127.0.0.1 --port 5180 --strictPort',
      cwd: './ui',
      url: 'http://127.0.0.1:5180',
      reuseExistingServer: true,
      timeout: 30_000,
    },
  ],
});
