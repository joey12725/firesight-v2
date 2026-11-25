// Small helpers shared by the account functions. Like api/chat.ts, nothing here logs.

import type { ServerConfig } from './env.js';

export const NO_STORE = { 'cache-control': 'no-store' };

export function json(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json', ...NO_STORE, ...headers } });
}

/** `{ error: <stable code>, message: <for a person> }`, plus any extra fields. */
export function problem(status: number, error: string, message: string, extra: Record<string, unknown> = {}, headers: Record<string, string> = {}): Response {
  return json(status, { error, message, ...extra }, headers);
}

export const notConfigured = (): Response => problem(503, 'accounts_unavailable', 'Accounts are not set up on this site.');

export const misconfigured = (message: string): Response => problem(503, 'accounts_misconfigured', message);

/**
 * True when a state-changing request was made by a page of this site.
 *
 * Browsers attach `Origin` to every POST, PUT and DELETE that a page makes with fetch,
 * and a page cannot forge it, so an exact match against the site's own origin stops
 * another website from riding a visitor's session cookie. A request with no `Origin`
 * at all is not from a browser page and has no business presenting a cookie here.
 */
export function fromThisSite(request: Request, config: Pick<ServerConfig, 'origin' | 'extraOrigins'>): boolean {
  const origin = request.headers.get('origin');
  if (!origin) return false;
  return origin === config.origin || config.extraOrigins.includes(origin);
}

/**
 * For reads: refuses only what is positively another site. A cross-site page cannot
 * read the response anyway (there is no CORS grant) and SameSite=Lax keeps the cookie
 * off its subrequests; this is a second line behind both.
 */
export function notFromAnotherSite(request: Request, config: Pick<ServerConfig, 'origin' | 'extraOrigins'>): boolean {
  const origin = request.headers.get('origin');
  if (origin) return fromThisSite(request, config);
  return request.headers.get('sec-fetch-site') !== 'cross-site';
}

export type BodyResult = { ok: true; value: unknown } | { ok: false; response: Response };

/** Reads a JSON body of at most `maxBytes`. The content type must say JSON. */
export async function readJson(request: Request, maxBytes: number): Promise<BodyResult> {
  const type = (request.headers.get('content-type') ?? '').split(';')[0]?.trim().toLowerCase();
  if (type !== 'application/json') return { ok: false, response: problem(415, 'unsupported_media_type', 'Send the body as application/json.') };
  const tooLarge = () => ({ ok: false as const, response: problem(413, 'too_large', `The request body is larger than ${maxBytes} bytes.`) });
  if (Number(request.headers.get('content-length') ?? '0') > maxBytes) return tooLarge();
  let bytes: ArrayBuffer;
  try {
    bytes = await request.arrayBuffer();
  } catch {
    return { ok: false, response: problem(400, 'bad_request', 'The request body could not be read.') };
  }
  if (bytes.byteLength > maxBytes) return tooLarge();
  try {
    return { ok: true, value: JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes)) as unknown };
  } catch {
    return { ok: false, response: problem(400, 'bad_request', 'The body is not valid JSON.') };
  }
}

/**
 * The path a function was asked for, whatever the platform did on the way in.
 *
 * vercel.json rewrites `/api/auth/sign-in/email` to the function at `/api/auth` and
 * passes the rest of the path in the query (`?fs_path=sign-in/email`), because whether
 * a rewritten function still sees the original path in `request.url` is not something
 * the platform documents. Both shapes are handled: when `fs_path` is present it is
 * authoritative and is removed from the query; otherwise the path is used as it came.
 *
 * Vercel's build also compiles the rule to pass the matched segments a second time
 * under the parameter's own name (`&path=sign-in/email`). That copy is removed when it
 * repeats `fs_path`, so it does not travel on into the signed OAuth query.
 */
export function routedUrl(request: Request, base: string, origin: string): URL {
  const seen = new URL(request.url);
  const url = new URL(seen.pathname + seen.search, origin);
  const rest = url.searchParams.get('fs_path');
  if (rest !== null) {
    url.searchParams.delete('fs_path');
    if (url.searchParams.get('path') === rest) url.searchParams.delete('path');
    url.pathname = rest ? `${base}/${rest.replace(/^\/+/, '')}` : base;
  }
  return url;
}
