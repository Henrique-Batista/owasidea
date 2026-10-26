import { defineConfig } from 'vite';

const host = process.env.OWASIDEA_HOST;
if (!host) {
  throw new Error(
    'OWASIDEA_HOST is not set; run `owasidea build`/`owasidea dev` or set OWASIDEA_HOST=http://127.0.0.1:8787'
  );
}

export default defineConfig({
  define: { __OWASIDEA_HOST__: JSON.stringify(host) },
  server: { host: '127.0.0.1', port: 5173, strictPort: true },
});
