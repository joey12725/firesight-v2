// npm run dev:api
//
// Serves the functions in api/ on http://localhost:8787 (PORT to change it), routed by
// the rewrites in vercel.json, so accounts, API keys, synced plans and the MCP endpoint
// run locally with no cloud resource. The web app's dev server proxies /api and
// /.well-known here.
//
//   DATABASE_URL unset   an embedded Postgres under .local/ (FIRESIGHT_DEV_DATA=<dir>
//                        to move it, FIRESIGHT_DEV_DATA=memory to keep nothing), with
//                        generated dev secrets beside it, migrated on start
//   DATABASE_URL set     that database and the secrets in the environment
//   BETTER_AUTH_URL      the origin people browse; defaults to the web dev server,
//                        http://localhost:5173. Set it to this server's own address
//                        to use the API without the web app.
//
// Each function is called through the same `export default { fetch }` the deployment
// uses. A rewritten request reaches its function with the destination URL, not the
// original one: the stricter of the two things the platform might do.

import { existsSync, readFileSync } from 'node:fs';
import { createServer } from 'node:http';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { pathToFileURL } from 'node:url';
import { provideRuntime } from '../api/_lib/runtime.js';
import { REPO_ROOT, startDevRuntime } from './_lib/dev-runtime.js';
import { applyRewrites } from './_lib/rewrites.js';
import type { Rewrite } from './_lib/rewrites.js';

type FunctionModule = { default: { fetch(request: Request): Response | Promise<Response> } };

for (const file of ['.env.local', '.env']) {
  const path = join(REPO_ROOT, file);
  // Values already in the environment win.
  if (existsSync(path)) process.loadEnvFile(path);
}

const port = Number(process.env.PORT ?? 8787);
const host = process.env.HOST ?? 'localhost';
const quiet = process.env.FIRESIGHT_DEV_QUIET === '1';
const say = (line: string) => {
  if (!quiet) console.log(line);
};

const dev = await startDevRuntime(process.env, { log: (level, message) => console.error(`[auth ${level}] ${message}`) }).catch((error: unknown) => {
  // A database already in use, an unreachable DATABASE_URL, a malformed key: say which and stop.
  console.error(`The dev API server could not start: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
});
// The functions build their runtime from process.env; hand them this one instead.
provideRuntime(process.env, dev.runtime);

const rewrites = (JSON.parse(readFileSync(join(REPO_ROOT, 'vercel.json'), 'utf8')) as { rewrites?: Rewrite[] }).rewrites ?? [];

// Every file directly in api/ is a function at /api/<name>, loaded on first use so
// that a function added later (api/mcp.ts) is picked up without touching this file.
const loaded = new Map<string, Promise<FunctionModule> | null>();
function functionAt(pathname: string): Promise<FunctionModule> | null {
  const match = /^\/api\/([a-z][a-z0-9-]*)$/.exec(pathname);
  const name = match?.[1];
  if (!name) return null;
  if (!loaded.has(name)) {
    const file = join(REPO_ROOT, 'api', `${name}.ts`);
    loaded.set(name, existsSync(file) ? (import(pathToFileURL(file).href) as Promise<FunctionModule>) : null);
  }
  return loaded.get(name) ?? null;
}

function toRequest(req: IncomingMessage, target: string, signal: AbortSignal): Request {
  const headers = new Headers();
  for (const [name, value] of Object.entries(req.headers)) {
    if (value === undefined) continue;
    for (const item of Array.isArray(value) ? value : [value]) headers.append(name, item);
  }
  // As on Vercel: the platform states the client address, whatever the client sent.
  headers.set('x-forwarded-for', req.socket.remoteAddress ?? '127.0.0.1');
  const hasBody = req.method !== 'GET' && req.method !== 'HEAD';
  return new Request(new URL(target, `http://${req.headers.host ?? `${host}:${port}`}`), {
    method: req.method,
    headers,
    body: hasBody ? (Readable.toWeb(req) as ReadableStream<Uint8Array>) : undefined,
    signal,
    // Required by Node when the body is a stream.
    ...(hasBody ? { duplex: 'half' } : {}),
  } as RequestInit);
}

async function send(res: ServerResponse, response: Response): Promise<void> {
  res.statusCode = response.status;
  const cookies = response.headers.getSetCookie();
  response.headers.forEach((value, name) => {
    if (name !== 'set-cookie') res.setHeader(name, value);
  });
  if (cookies.length > 0) res.setHeader('set-cookie', cookies);
  if (!response.body) {
    res.end();
    return;
  }
  // Streamed, so a server-sent-events response (the assistant proxy, MCP) flows through.
  for await (const chunk of response.body) res.write(chunk);
  res.end();
}

const server = createServer((req, res) => {
  const started = Date.now();
  const url = new URL(req.url ?? '/', 'http://localhost');
  const abort = new AbortController();
  res.on('close', () => {
    if (!res.writableEnded) abort.abort();
  });
  void (async () => {
    const direct = functionAt(url.pathname);
    const target = direct ? url.pathname + url.search : applyRewrites(rewrites, url.pathname, url.search);
    const module = direct ?? (target ? functionAt(new URL(target, 'http://localhost').pathname) : null);
    let response: Response;
    if (!target || !module) {
      response = new Response(JSON.stringify({ error: 'not_found', message: 'The dev API server serves /api/* and /.well-known/* only.' }), { status: 404, headers: { 'content-type': 'application/json' } });
    } else {
      try {
        response = await (await module).default.fetch(toRequest(req, target, abort.signal));
      } catch (error) {
        console.error(`[dev-api] ${req.method} ${url.pathname} threw:`, error);
        response = new Response(JSON.stringify({ error: 'server_error', message: 'The function threw. See the dev server output.' }), { status: 500, headers: { 'content-type': 'application/json' } });
      }
    }
    // The path only: a query string can carry an authorisation code.
    say(`${req.method} ${url.pathname} ${response.status} ${Date.now() - started}ms`);
    await send(res, response);
  })().catch((error: unknown) => {
    if (!abort.signal.aborted) console.error('[dev-api] response failed:', error);
    res.destroy();
  });
});

server.on('error', (error: NodeJS.ErrnoException) => {
  console.error(error.code === 'EADDRINUSE' ? `Port ${port} is in use. Set PORT to another one.` : `The server failed: ${error.message}`);
  void dev.close().finally(() => process.exit(1));
});

server.listen(port, host, () => {
  const { config } = dev.runtime;
  console.log(`FIRESight dev API on http://${host}:${port}`);
  say(`  database       ${dev.database}`);
  say(`  migrations     ${[...dev.migration.authTablesCreated.map((t) => `+${t}`), ...dev.migration.applied].join(', ') || 'up to date'}`);
  say(`  public origin  ${config.origin}  (BETTER_AUTH_URL; the address people browse)`);
  say(`  MCP endpoint   ${config.mcpUrl}${existsSync(join(REPO_ROOT, 'api', 'mcp.ts')) ? '' : '  (api/mcp.ts not present yet)'}`);
  say(`  client registration  ${config.dcr ? 'metadata documents and dynamic registration' : 'metadata documents only (FIRESIGHT_OAUTH_DCR=1 adds dynamic registration)'}`);
});

let closing = false;
const shutdown = () => {
  if (closing) return;
  closing = true;
  server.close();
  server.closeAllConnections();
  void dev.close().finally(() => process.exit(0));
};
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
