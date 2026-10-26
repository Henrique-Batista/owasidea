import { defineConfig } from 'vite';

const host = process.env.OWASIDEA_HOST ?? 'http://127.0.0.1:8790';

export default defineConfig({
  define: { __OWASIDEA_HOST__: JSON.stringify(host) },
  server: { host: '127.0.0.1', port: 5180, strictPort: true },
  preview: { host: '127.0.0.1', port: 5180, strictPort: true },
});
