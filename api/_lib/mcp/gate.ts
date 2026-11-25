// The door of the MCP endpoint: who is calling, decided from the `Authorization` header
// and nothing else.
//
// Two credentials open it. An API key (`fs_…`), which a user creates on their account
// page for clients that can send a header; and an OAuth access token, which clients
// that sign the user in (Claude, ChatGPT) obtain from this site's authorisation server
// and which is bound to this endpoint as its audience. A session cookie does not: the
// header that carries it is removed before anything looks at the request.
//
// A request without a usable credential gets 401 with the RFC 9728 challenge, which is
// how an MCP client learns where to send the user to sign in.

import { requireMcpAuth } from '@better-auth/mcp';
import type { Principal } from '../credentials.js';
import { bearerApiKey, oauthPrincipal, verifyApiKey } from '../credentials.js';
import { PLAN_SCOPES } from '../env.js';
import type { ServerConfig } from '../env.js';
import { NO_STORE } from '../http.js';
import { consumeRate, sweepRates } from '../ratelimit.js';
import type { Runtime } from '../runtime.js';

export type Gate = { ok: true; principal: Principal; clientId: string; expiresAt?: number } | { ok: false; response: Response };

/** Refused credentials per client address per minute, before further attempts are turned away unexamined. */
export const FAILED_ATTEMPTS_PER_MINUTE = 30;

/** Where the protected-resource metadata for the MCP endpoint lives (RFC 9728, path-qualified form). */
export function resourceMetadataUrl(config: Pick<ServerConfig, 'origin' | 'mcpUrl'>): string {
  return `${config.origin}/.well-known/oauth-protected-resource${new URL(config.mcpUrl).pathname}`;
}

// RFC 6750 allows printable ASCII in a quoted parameter, without quotes or backslashes.
const quotable = (text: string) => text.replace(/[^\x20-\x7e]|["\\]/g, '');

/**
 * 401 with `WWW-Authenticate: Bearer …`. Without `error`, the request simply carried
 * no credential (RFC 6750 §3.1); with it, the credential was refused.
 */
export function challenge(config: Pick<ServerConfig, 'origin' | 'mcpUrl'>, error?: { code: 'invalid_token'; description: string }): Response {
  const parts = [
    ...(error ? [`error="${error.code}"`, `error_description="${quotable(error.description)}"`] : []),
    `resource_metadata="${resourceMetadataUrl(config)}"`,
    `scope="${PLAN_SCOPES.join(' ')}"`,
  ];
  const message = error
    ? `${error.description}.`
    : `This endpoint needs a credential: an OAuth access token for ${config.mcpUrl}, or a FIRESight API key, as "Authorization: Bearer …". A key is created at ${config.origin}/account.`;
  return new Response(JSON.stringify({ error: error ? error.code : 'unauthenticated', message }), {
    status: 401,
    headers: { 'content-type': 'application/json', 'www-authenticate': `Bearer ${parts.join(', ')}`, ...NO_STORE },
  });
}

const tooMany = (retryAfter: number, message: string): Response =>
  new Response(JSON.stringify({ error: 'rate_limited', message }), { status: 429, headers: { 'content-type': 'application/json', 'retry-after': String(retryAfter), ...NO_STORE } });

/** The client's address as the platform reports it: the first entry of `x-forwarded-for`. */
function clientAddress(request: Request): string {
  const first = (request.headers.get('x-forwarded-for') ?? '').split(',')[0]?.trim() ?? '';
  // Bounded and plain, since it becomes part of a counter's key.
  return /^[0-9A-Fa-f.:]{2,45}$/.test(first) ? first.toLowerCase() : 'unknown';
}

/** Decides who is calling. Touches the database only when a credential was presented. */
export async function authenticate(runtime: Runtime, request: Request, now: number = Date.now()): Promise<Gate> {
  const { config } = runtime;
  if (!request.headers.get('authorization')?.trim()) return { ok: false, response: challenge(config) };

  // Refused credentials are counted per address, so a key cannot be guessed at speed
  // and a flood of junk tokens stops costing a signature check each.
  const failureKey = `mcp:deny:${clientAddress(request)}`;
  const refuse = async (response: Response): Promise<Gate> => {
    const rate = await consumeRate(runtime.db, failureKey, FAILED_ATTEMPTS_PER_MINUTE, 60, now);
    // These counters are keyed by address, not by account, so nothing else ever removes
    // them. The first refusal an address earns in a minute pays for clearing out every
    // counter that has been idle for a day.
    if (rate.remaining === FAILED_ATTEMPTS_PER_MINUTE - 1) await sweepRates(runtime.db, now);
    return { ok: false, response: rate.allowed ? response : tooMany(rate.retryAfter, 'Too many refused credentials from this address. Try again shortly.') };
  };

  const key = bearerApiKey(request);
  if (key) {
    const check = await verifyApiKey(runtime, key);
    if (check.ok) return { ok: true, principal: check.principal, clientId: `apikey:${check.keyId}` };
    if (check.reason === 'rate_limited') return { ok: false, response: tooMany(check.retryAfterSeconds, 'This API key has made too many calls in the last minute. Try again shortly.') };
    return refuse(challenge(config, { code: 'invalid_token', description: 'The API key is not valid, or it has expired or been revoked' }));
  }

  // Anything else must be an access token issued by this site for this endpoint. The
  // library checks signature, issuer, audience and expiry; its refusals are already
  // RFC 9728 challenges.
  let claims: Record<string, unknown> | undefined;
  const guard = requireMcpAuth(
    runtime.auth,
    (_request, verified) => {
      claims = verified;
      return new Response(null, { status: 204 });
    },
    { resource: config.mcpUrl, issuer: config.issuer, challengeScopes: [...PLAN_SCOPES] },
  );
  const answer = await guard(request);
  if (!claims) return refuse(answer);
  // A valid signature is not enough: the user must still consent to this client.
  const principal = await oauthPrincipal(runtime, claims);
  if (!principal) return refuse(challenge(config, { code: 'invalid_token', description: 'This app is no longer connected to the account' }));
  const clientId = typeof claims.azp === 'string' ? claims.azp : typeof claims.client_id === 'string' ? claims.client_id : principal.via;
  return { ok: true, principal, clientId, expiresAt: typeof claims.exp === 'number' ? claims.exp : undefined };
}
