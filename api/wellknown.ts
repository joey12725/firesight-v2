// Public documents (Vercel Function, Node.js runtime, Web handler). No credentials.
// Everything here is derived from configuration, is the same for every caller, and is
// cached: by this instance, by the CDN and by clients. /api/config and the
// protected-resource document never touch the database, so the first thing an MCP
// client asks for is answered even while the database is waking up.
//
//   GET /api/config                                   what this deployment offers
//   GET /.well-known/oauth-protected-resource[/api/mcp]         RFC 9728
//   GET /.well-known/oauth-authorization-server[/api/auth]      RFC 8414
//   GET /.well-known/openid-configuration[/api/auth]            OpenID Connect Discovery
//
// vercel.json rewrites those paths here with `?doc=<name>` (and `&suffix=<rest>`); a
// request that arrives with its original path is recognised too. The authorisation
// server's documents are produced by the auth library, so they describe what it
// actually serves; the protected-resource document is small enough to state here, and
// a test holds it equal to the library's own.

import { AUTH_BASE_PATH, MCP_PATH, PLAN_SCOPES } from './_lib/env.js';
import type { Env, ServerConfig } from './_lib/env.js';
import { json, problem } from './_lib/http.js';
import { runtimeFor } from './_lib/runtime.js';
import type { Runtime, RuntimeState } from './_lib/runtime.js';

declare const process: { env: Env };

export interface Deps {
  state?: RuntimeState;
  report?: (error: unknown) => void;
}

/** What the app needs to know before it shows any account UI. */
export interface PublicConfig {
  accounts: boolean;
  /** The MCP endpoint to give to an AI client, or null without accounts. */
  mcp_url: string | null;
  /** Social sign-in providers that are configured: 'github', 'google'. */
  social: string[];
  /** Whether OAuth Dynamic Client Registration is on. */
  dcr: boolean;
}

// Any site may read these: MCP clients that run in a browser fetch them cross-origin.
const CORS = { 'access-control-allow-origin': '*', 'access-control-allow-methods': 'GET, HEAD, OPTIONS', 'access-control-allow-headers': '*', 'access-control-max-age': '86400' };
// `s-maxage` lets the CDN answer without waking the function at all.
const DOC_CACHE = 'public, max-age=300, s-maxage=300, stale-while-revalidate=300';

/** doc name → the only path suffix that is ours (the issuer's path, or the resource's). */
const DOCS: Record<string, string> = {
  'oauth-protected-resource': MCP_PATH,
  'oauth-authorization-server': AUTH_BASE_PATH,
  'openid-configuration': AUTH_BASE_PATH,
};

/** DPoP proof algorithms the auth library accepts (its `DPOP_SIGNING_ALGORITHMS`). */
const DPOP_ALGORITHMS = ['EdDSA', 'ES256', 'ES512', 'PS256', 'RS256'];

/**
 * RFC 9728: which authorisation server protects the MCP endpoint, and the scopes that
 * mean something there. OpenID's identity scopes and `offline_access` are the
 * authorisation server's business and are not listed.
 */
export function protectedResourceMetadata(config: Pick<ServerConfig, 'mcpUrl' | 'issuer'>) {
  return {
    resource: config.mcpUrl,
    authorization_servers: [config.issuer],
    bearer_methods_supported: ['header'],
    dpop_signing_alg_values_supported: DPOP_ALGORITHMS,
    scopes_supported: [...PLAN_SCOPES],
  };
}

const built = new WeakMap<Runtime, Map<string, string>>();

async function documentText(runtime: Runtime, doc: string): Promise<string> {
  let cache = built.get(runtime);
  if (!cache) built.set(runtime, (cache = new Map()));
  let text = cache.get(doc);
  if (text === undefined) {
    const body: unknown =
      doc === 'oauth-protected-resource'
        ? protectedResourceMetadata(runtime.config)
        : doc === 'openid-configuration'
          ? await runtime.auth.api.getOpenIdConfig()
          : await runtime.auth.api.getOAuthServerConfig();
    text = JSON.stringify(body);
    cache.set(doc, text);
  }
  return text;
}

/** Which document a request is for: from the rewrite's query, or from the original path. */
export function requestedDoc(url: URL): { doc: string; suffix: string } | null {
  const named = url.searchParams.get('doc');
  if (named !== null) return { doc: named, suffix: `/${(url.searchParams.get('suffix') ?? '').replace(/^\/+|\/+$/g, '')}`.replace(/^\/$/, '') };
  if (url.pathname === '/api/config') return { doc: 'config', suffix: '' };
  const match = /^\/\.well-known\/([a-z-]+)(\/.*)?$/.exec(url.pathname);
  if (match?.[1]) return { doc: match[1], suffix: (match[2] ?? '').replace(/\/+$/, '') };
  // OpenID Connect's own form: <issuer>/.well-known/openid-configuration.
  if (url.pathname === `${AUTH_BASE_PATH}/.well-known/openid-configuration`) return { doc: 'openid-configuration', suffix: AUTH_BASE_PATH };
  return null;
}

export function publicConfig(state: RuntimeState): PublicConfig {
  if (state.status !== 'ready') return { accounts: false, mcp_url: null, social: [], dcr: false };
  const { config } = state.runtime;
  return { accounts: true, mcp_url: config.mcpUrl, social: Object.keys(config.social).sort(), dcr: config.dcr };
}

export async function handle(request: Request, env: Env, deps: Deps = {}): Promise<Response> {
  if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers: CORS });
  if (request.method !== 'GET' && request.method !== 'HEAD') return problem(405, 'method_not_allowed', 'Use GET.', {}, { allow: 'GET, HEAD, OPTIONS', ...CORS });
  const wanted = requestedDoc(new URL(request.url));
  if (!wanted) return problem(404, 'not_found', 'There is no such document.', {}, CORS);
  const headOnly = (response: Response) => (request.method === 'HEAD' ? new Response(null, { status: response.status, headers: response.headers }) : response);

  const state = deps.state ?? runtimeFor(env);
  if (wanted.doc === 'config') {
    // Short-lived: turning accounts on should show up within a minute.
    return headOnly(json(200, publicConfig(state), { 'cache-control': 'public, max-age=60' }));
  }

  const suffix = DOCS[wanted.doc];
  // One issuer and one resource live here, so each document has a bare form and one path-qualified form.
  if (suffix === undefined || (wanted.suffix !== '' && wanted.suffix !== suffix)) return problem(404, 'not_found', 'There is no such document.', {}, CORS);
  if (state.status !== 'ready') return problem(503, 'accounts_unavailable', 'Accounts are not set up on this site.', {}, CORS);

  try {
    const text = await documentText(state.runtime, wanted.doc);
    return headOnly(new Response(text, { status: 200, headers: { 'content-type': 'application/json', 'cache-control': DOC_CACHE, ...CORS } }));
  } catch (error) {
    deps.report?.(error);
    return problem(500, 'server_error', 'Something went wrong on the server.', {}, CORS);
  }
}

export default {
  fetch(request: Request): Promise<Response> {
    return handle(request, process.env);
  },
};
