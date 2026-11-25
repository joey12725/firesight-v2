// Turning a presented credential into "who is this and what may they do".
//
// The website's API accepts the session cookie and nothing else. The MCP endpoint
// accepts an API key or an OAuth access token and never a cookie. Keeping the three
// apart means a leaked credential of one kind is useless against the other surface.

import { API_KEY_PREFIX } from './auth.js';
import type { PlanScope } from './env.js';
import { PLAN_SCOPES } from './env.js';
import type { PlanWriter } from './plans.js';
import { writerLabel } from './plans.js';
import type { Runtime } from './runtime.js';

export interface SessionUser {
  id: string;
  email: string;
  name: string;
}

/**
 * The signed-in user, from the session cookie only. Any `Authorization` or `x-api-key`
 * header on the request is not passed on, so a bearer credential can never
 * authenticate a call to the website's API.
 */
export async function sessionUser(runtime: Runtime, request: Request): Promise<SessionUser | null> {
  const cookie = request.headers.get('cookie');
  if (!cookie) return null;
  const session = await runtime.auth.api.getSession({ headers: new Headers({ cookie }) });
  if (!session) return null;
  return { id: session.user.id, email: session.user.email, name: session.user.name };
}

/** An authenticated MCP caller. `via` is what a write is attributed to. */
export interface Principal {
  userId: string;
  scopes: PlanScope[];
  via: PlanWriter;
}

export type KeyCheck = { ok: true; principal: Principal; keyId: string } | { ok: false; reason: 'invalid' } | { ok: false; reason: 'rate_limited'; retryAfterSeconds: number };

/** `Authorization: Bearer fs_…` → the key, or null when the header carries something else. */
export function bearerApiKey(request: Request): string | null {
  const match = /^Bearer\s+(\S+)\s*$/i.exec(request.headers.get('authorization') ?? '');
  const token = match?.[1];
  return token?.startsWith(API_KEY_PREFIX) ? token : null;
}

/**
 * Verifies an API key and counts the call against the key's own rate limit. Invalid,
 * revoked, disabled and expired keys are all just `invalid`: the caller learns nothing
 * about which.
 */
export async function verifyApiKey(runtime: Runtime, key: string): Promise<KeyCheck> {
  if (!key.startsWith(API_KEY_PREFIX) || key.length > 256) return { ok: false, reason: 'invalid' };
  const result = await runtime.auth.api.verifyApiKey({ body: { key } });
  if (!result.valid || !result.key) {
    if (result.error?.code === 'RATE_LIMITED') {
      const tryAgainIn = (result.error as { details?: { tryAgainIn?: unknown } }).details?.tryAgainIn;
      return { ok: false, reason: 'rate_limited', retryAfterSeconds: typeof tryAgainIn === 'number' ? Math.max(1, Math.ceil(tryAgainIn / 1000)) : 60 };
    }
    return { ok: false, reason: 'invalid' };
  }
  const granted = result.key.permissions?.plans ?? [];
  const scopes = PLAN_SCOPES.filter((scope) => granted.includes(scope.slice('plans:'.length)));
  return { ok: true, keyId: result.key.id, principal: { userId: result.key.referenceId, scopes, via: writerLabel('apikey', result.key.name) } };
}

/**
 * The principal behind a verified OAuth access token (the JWT claims that
 * `requireMcpAuth` hands to its handler). Signature, issuer, audience and expiry have
 * been checked by then. An access token is a signed statement that stays valid for an
 * hour whatever happens afterwards, so one more thing is checked here, against the
 * database: that the user still consents to this client. Disconnecting an app, or
 * deleting the account, therefore takes effect on the app's very next call. Null means
 * "not a caller": answer 401.
 */
export async function oauthPrincipal(runtime: Runtime, claims: { sub?: unknown; scope?: unknown; azp?: unknown; client_id?: unknown }): Promise<Principal | null> {
  if (typeof claims.sub !== 'string' || !claims.sub) return null;
  const granted = typeof claims.scope === 'string' ? claims.scope.split(' ') : [];
  const clientId = typeof claims.azp === 'string' ? claims.azp : typeof claims.client_id === 'string' ? claims.client_id : '';
  if (!clientId) return null;
  const consent = await runtime.db.query('select 1 from "oauthConsent" where "userId" = $1 and "clientId" = $2 limit 1', [claims.sub, clientId]);
  if (consent.rows.length === 0) return null;
  return { userId: claims.sub, scopes: PLAN_SCOPES.filter((scope) => granted.includes(scope)), via: await oauthWriter(runtime, clientId) };
}

/**
 * `oauth:<client name>` for attribution. The name is what the client says about itself,
 * so it is cleaned and length-capped, and falls back to the host of a URL client id.
 */
export async function oauthWriter(runtime: Runtime, clientId: string): Promise<PlanWriter> {
  let name: unknown;
  if (clientId) {
    const found = await runtime.db.query<{ name: string | null }>('select name from "oauthClient" where "clientId" = $1', [clientId]);
    name = found.rows[0]?.name;
  }
  if (typeof name !== 'string' || !name.trim()) {
    try {
      name = new URL(clientId).host;
    } catch {
      name = clientId;
    }
  }
  return writerLabel('oauth', name);
}
