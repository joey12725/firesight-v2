// The hosted MCP server (Vercel Function, Node.js runtime, Web handler).
//
//   POST /api/mcp     JSON-RPC over Streamable HTTP: one request, one answer, no session
//
// This is how an AI client outside the browser (Claude, Claude Code, ChatGPT, Cursor,
// VS Code) works with the plans in a user's account: read them, create and change
// them, and simulate them with the same engine the website runs. What it writes shows
// up on the website for that user, and for nobody else.
//
// In order, a request must:
//   1. come from an allowed Origin, if it names one at all (clients that are not
//      browsers send none, and are not blocked for that);
//   2. carry a credential in `Authorization: Bearer …`: an API key or an OAuth access
//      token issued for this endpoint. Cookies are never read. Without one the answer
//      is 401 and the RFC 9728 challenge that tells the client where to sign in;
//   3. be within the account's rate limit.
// Only then does the MCP server see it. The tools are in _lib/mcp/tools.ts.
//
// Like the other functions, this one writes no logs.

import { validateOriginHeader } from '@modelcontextprotocol/server';
import type { Env, ServerConfig } from './_lib/env.js';
import { NO_STORE, misconfigured, notConfigured, problem } from './_lib/http.js';
import { authenticate } from './_lib/mcp/gate.js';
import { createHostedMcp } from './_lib/mcp/server.js';
import type { HostedMcp } from './_lib/mcp/server.js';
import { consumeRate, userRateKey } from './_lib/ratelimit.js';
import { runtimeFor } from './_lib/runtime.js';
import type { Runtime, RuntimeState } from './_lib/runtime.js';

declare const process: { env: Env };

export interface Deps {
  state?: RuntimeState;
  now?: () => number;
  report?: (error: unknown) => void;
  /** A server built with other limits or clocks. Tests use this; the function builds its own. */
  mcp?: HostedMcp;
}

/**
 * Hosts whose pages may call this endpoint from a browser, besides the site itself.
 * Loopback covers developer tools that run in a local page (the MCP Inspector). The
 * hosted assistants call from their servers and are not known to send an Origin; their
 * sites are listed so that they are not turned away if they do. None of this grants
 * anything: every call still needs a bearer credential, and a browser attaches none
 * by itself.
 */
const DEFAULT_ALLOWED_HOSTS = ['localhost', '127.0.0.1', '[::1]', 'claude.ai', 'claude.com', 'chatgpt.com'];

/** The hostnames an `Origin` header may name. `FIRESIGHT_MCP_ALLOWED_ORIGINS` adds to them. */
export function allowedOriginHosts(config: Pick<ServerConfig, 'origin' | 'extraOrigins'>, env: Env): string[] {
  const hostOf = (value: string): string | null => {
    try {
      return new URL(value.includes('://') ? value : `https://${value}`).hostname.toLowerCase();
    } catch {
      return null;
    }
  };
  const extra = (env.FIRESIGHT_MCP_ALLOWED_ORIGINS ?? '').split(',').map((s) => s.trim()).filter(Boolean);
  const hosts = [config.origin, ...config.extraOrigins, ...extra].map(hostOf).filter((host): host is string => host !== null);
  return [...new Set([...hosts, ...DEFAULT_ALLOWED_HOSTS])];
}

// A page that passed the Origin check may read the answer. No credentials mode: the
// bearer token is put on the request by the page's own code, never by the browser.
function cors(request: Request): Record<string, string> {
  const origin = request.headers.get('origin');
  if (!origin) return {};
  return { 'access-control-allow-origin': origin, 'access-control-expose-headers': 'www-authenticate, mcp-protocol-version, retry-after', vary: 'origin' };
}

function preflight(request: Request): Response {
  return new Response(null, {
    status: 204,
    headers: {
      ...cors(request),
      'access-control-allow-methods': 'POST, OPTIONS',
      // Whatever the client says it will send: the protocol's own headers vary by revision.
      'access-control-allow-headers': request.headers.get('access-control-request-headers') ?? 'authorization, content-type, accept, mcp-protocol-version, mcp-method, mcp-name',
      'access-control-max-age': '86400',
    },
  });
}

function finish(request: Request, response: Response): Response {
  const headers = new Headers(response.headers);
  for (const [name, value] of Object.entries(cors(request))) headers.set(name, value);
  // Answers are about one account. Nothing between here and the client may keep them.
  if (!headers.has('cache-control')) headers.set('cache-control', NO_STORE['cache-control']);
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
}

const servers = new WeakMap<Runtime, HostedMcp>();

function serverFor(runtime: Runtime, deps: Deps): HostedMcp {
  if (deps.mcp) return deps.mcp;
  let server = servers.get(runtime);
  if (!server) servers.set(runtime, (server = createHostedMcp(runtime, { now: deps.now, report: deps.report })));
  return server;
}

export async function handle(request: Request, env: Env, deps: Deps = {}): Promise<Response> {
  const state = deps.state ?? runtimeFor(env);
  if (state.status === 'off') return notConfigured();
  if (state.status === 'misconfigured') return misconfigured(state.message);
  const runtime = state.runtime;
  const now = deps.now ?? Date.now;

  const origin = validateOriginHeader(request.headers.get('origin'), allowedOriginHosts(runtime.config, env));
  if (!origin.ok) return problem(403, 'forbidden_origin', 'This endpoint does not accept requests from pages of that site.');
  if (request.method === 'OPTIONS') return preflight(request);

  try {
    // From here on the request has no cookies: nothing downstream can be tempted by one.
    const headers = new Headers(request.headers);
    headers.delete('cookie');
    const hasBody = request.method !== 'GET' && request.method !== 'HEAD';
    const clean = new Request(request.url, { method: request.method, headers, body: hasBody ? request.body : undefined, signal: request.signal, ...(hasBody ? { duplex: 'half' } : {}) } as RequestInit);

    const gate = await authenticate(runtime, clean, now());
    if (!gate.ok) return finish(request, gate.response);

    const mcp = serverFor(runtime, deps);
    const rate = await consumeRate(runtime.db, userRateKey('mcp', gate.principal.userId), mcp.limits.requestsPerMinute, 60, now());
    if (!rate.allowed) {
      return finish(request, problem(429, 'rate_limited', 'Too many requests for this account in the last minute. Try again shortly.', {}, { 'retry-after': String(rate.retryAfter) }));
    }
    return finish(request, await mcp.fetch(clean, gate.principal, { clientId: gate.clientId, expiresAt: gate.expiresAt }));
  } catch (error) {
    deps.report?.(error);
    return finish(request, problem(500, 'server_error', 'Something went wrong on the server.'));
  }
}

export default {
  fetch(request: Request): Promise<Response> {
    return handle(request, process.env);
  },
};
