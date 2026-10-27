import { defineConfig } from 'vite';

const host = process.env.OWASIDEA_HOST;
if (!host) {
  throw new Error('OWASIDEA_HOST is not set; run `owasidea dev` or `owasidea build`');
}

export default defineConfig({
  define: { __OWASIDEA_HOST__: JSON.stringify(host) },
  server: { host: '127.0.0.1', port: 5201, strictPort: true },
});
