import { defineConfig } from 'vite';

const host = (globalThis as any).process?.env?.OWASIDEA_HOST ?? 'http://127.0.0.1:8787';

export default defineConfig({
  define: { __OWASIDEA_HOST__: JSON.stringify(host) },
  server: { host: '127.0.0.1', port: 5173, strictPort: true },
});
