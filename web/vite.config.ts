import { request as httpRequest } from 'node:http';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { request as httpsRequest } from 'node:https';
import { fileURLToPath } from 'node:url';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';

/**
 * Social-card links must be absolute for most crawlers. On Vercel the production host is
 * known at build time; elsewhere set SITE_URL, or the links stay root-relative.
 */
const siteUrl = (process.env.SITE_URL ?? (process.env.VERCEL_PROJECT_PRODUCTION_URL ? `https://${process.env.VERCEL_PROJECT_PRODUCTION_URL}` : '')).replace(/\/$/, '');

/**
 * WebMCP (the in-page tools for browser agents, web/src/webmcp) is an origin trial in Chrome: a site
 * gets `document.modelContext` for ordinary visitors only by serving a trial token for its origin.
 * Set VITE_WEBMCP_ORIGIN_TRIAL_TOKEN to the token at build time and it is written into the page;
 * unset, nothing is added. See docs/webmcp.md for where the token comes from.
 */
const originTrialToken = (process.env.VITE_WEBMCP_ORIGIN_TRIAL_TOKEN ?? '').trim();
// Tokens are base64. Anything else is ignored rather than written into the HTML.
const originTrialMeta = /^[A-Za-z0-9+/=_-]{40,4000}$/.test(originTrialToken) ? `    <meta http-equiv="origin-trial" content="${originTrialToken}" />\n` : '';

/**
 * Accounts, synced plans and the MCP endpoint are serverless functions (../api). In
 * development `npm run dev:api` at the repository root serves them on port 8787, and
 * this sends the browser's `/api/*` and `/.well-known/*` requests there, so the app and
 * its API share one origin as they do when deployed. FIRESIGHT_API_ORIGIN moves it.
 *
 * It is a middleware rather than `server.proxy` for one reason: when no API server is
 * running, the request falls through to Vite as if this were not here. Plain
 * `npm run dev` then behaves as it always has, with no proxy errors in the terminal.
 * The one exception is `/api/config`, which the app asks for on every load: with no API
 * server it is answered here with "no accounts", which is the truth, rather than left
 * to become a 404 in the browser console.
 */
function devApi(): Plugin {
  const origin = new URL(process.env.FIRESIGHT_API_ORIGIN ?? 'http://localhost:8787');
  const send = origin.protocol === 'https:' ? httpsRequest : httpRequest;
  const forward = (req: IncomingMessage, res: ServerResponse, next: () => void) => {
    const url = req.url ?? '/';
    if (!url.startsWith('/api/') && !url.startsWith('/.well-known/')) return next();
    // Headers go through untouched, Host included: the API must see the origin the browser is on.
    const upstream = send({ protocol: origin.protocol, hostname: origin.hostname, port: origin.port, method: req.method, path: url, headers: req.headers }, (reply) => {
      res.writeHead(reply.statusCode ?? 502, reply.headers);
      // Piped, so streamed answers (the assistant proxy, MCP) arrive as they are produced.
      reply.pipe(res);
    });
    upstream.on('error', () => {
      if (res.headersSent) return void res.destroy();
      if (req.method !== 'GET' || url.split('?')[0] !== '/api/config') return next();
      res.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
      res.end(JSON.stringify({ accounts: false, mcp_url: null, social: [], dcr: false }));
    });
    res.on('close', () => upstream.destroy());
    req.pipe(upstream);
  };
  return {
    name: 'firesight-dev-api',
    configureServer: (server) => void server.middlewares.use(forward),
    configurePreviewServer: (server) => void server.middlewares.use(forward),
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), devApi(), {
      name: 'firesight-index',
      transformIndexHtml: {
        order: 'post',
        handler(html, ctx) {
          // The engine is the largest download and nothing runs without it: start fetching it with the page,
          // not after the script that asks for it has been downloaded and parsed.
          const wasm = Object.keys(ctx.bundle ?? {}).find((file) => file.endsWith('.wasm'));
          const preload = wasm ? `    <link rel="preload" href="/${wasm}" as="fetch" type="application/wasm" crossorigin />\n` : '';
          return html.replaceAll('%SITE_URL%', siteUrl).replace('  </head>', `${preload}${originTrialMeta}  </head>`);
        },
      },
    }],
  resolve: { alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) } },
  worker: { format: 'es' },
  build: { target: 'es2023' },
  server: { port: 5173, strictPort: false },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
