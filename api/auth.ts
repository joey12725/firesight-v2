// FIRESight accounts (Vercel Function, Node.js runtime, Web handler).
//
//   /api/auth/*   Better Auth: sign-up, sign-in, sign-out, sessions, account deletion,
//                 API keys, and the OAuth 2.1 authorisation server MCP clients use
//                 (authorize, token, consent, JWKS, client registration when enabled).
//
// The whole surface is the library's; this file only puts the request back on the
// site's own origin and path before handing it over (see `routedUrl`), and answers
// 503 when accounts are not set up. It writes no logs.

import { AUTH_BASE_PATH } from './_lib/env.js';
import type { Env } from './_lib/env.js';
import { json, misconfigured, notConfigured, problem, routedUrl } from './_lib/http.js';
import { runtimeFor } from './_lib/runtime.js';
import type { RuntimeState } from './_lib/runtime.js';

declare const process: { env: Env };

export interface Deps {
  /** The runtime to use instead of the one built from `env`. */
  state?: RuntimeState;
  /** Told about unexpected failures. The dev server prints them; production has no sink. */
  report?: (error: unknown) => void;
}

/** Longest request body the auth endpoints accept. Vercel's own cap is 4.5 MB; nothing here needs 1% of it. */
const MAX_BODY_BYTES = 64 * 1024;

export async function handle(request: Request, env: Env, deps: Deps = {}): Promise<Response> {
  const state = deps.state ?? runtimeFor(env);
  if (state.status === 'off') return notConfigured();
  if (state.status === 'misconfigured') return misconfigured(state.message);
  const { auth, config } = state.runtime;

  const url = routedUrl(request, AUTH_BASE_PATH, config.origin);
  if (url.pathname !== AUTH_BASE_PATH && !url.pathname.startsWith(`${AUTH_BASE_PATH}/`)) return problem(404, 'not_found', 'There is nothing at this address.');
  if (Number(request.headers.get('content-length') ?? '0') > MAX_BODY_BYTES) return problem(413, 'too_large', 'The request is too large.');

  try {
    const hasBody = request.method !== 'GET' && request.method !== 'HEAD';
    const body = hasBody ? await request.arrayBuffer() : undefined;
    if (body && body.byteLength > MAX_BODY_BYTES) return problem(413, 'too_large', 'The request is too large.');
    const response = await auth.handler(new Request(url, { method: request.method, headers: request.headers, body, redirect: 'manual', signal: request.signal }));
    // Sessions, tokens and consent state must never sit in a shared cache. Discovery
    // documents and the public signing keys set their own caching and keep it.
    if (response.headers.has('cache-control')) return response;
    const headers = new Headers(response.headers);
    headers.set('cache-control', 'no-store');
    return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
  } catch (error) {
    deps.report?.(error);
    return json(500, { error: 'server_error', message: 'Something went wrong on the server.' });
  }
}

export default {
  fetch(request: Request): Promise<Response> {
    return handle(request, process.env);
  },
};
