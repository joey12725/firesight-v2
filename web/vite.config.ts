import { fileURLToPath } from 'node:url';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

/**
 * Social-card links must be absolute for most crawlers. On Vercel the production host is
 * known at build time; elsewhere set SITE_URL, or the links stay root-relative.
 */
const siteUrl = (process.env.SITE_URL ?? (process.env.VERCEL_PROJECT_PRODUCTION_URL ? `https://${process.env.VERCEL_PROJECT_PRODUCTION_URL}` : '')).replace(/\/$/, '');

export default defineConfig({
  plugins: [react(), tailwindcss(), {
      name: 'firesight-index',
      transformIndexHtml: {
        order: 'post',
        handler(html, ctx) {
          // The engine is the largest download and nothing runs without it: start fetching it with the page,
          // not after the script that asks for it has been downloaded and parsed.
          const wasm = Object.keys(ctx.bundle ?? {}).find((file) => file.endsWith('.wasm'));
          const preload = wasm ? `    <link rel="preload" href="/${wasm}" as="fetch" type="application/wasm" crossorigin />\n` : '';
          return html.replaceAll('%SITE_URL%', siteUrl).replace('  </head>', `${preload}  </head>`);
        },
      },
    }],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  worker: { format: 'es' },
  build: { target: 'es2023' },
  server: { port: 5173, strictPort: false },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
