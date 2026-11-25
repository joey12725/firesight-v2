// End-to-end check of the account API over real HTTP.
//   npm run e2e:api
//
// Starts its own dev API server (scripts/dev-api.ts) on E2E_PORT (default 8790) with a
// throwaway in-memory database, then drives it the way a browser and an MCP client
// would: through the rewrites in vercel.json, with cookies, Origin headers, form posts
// and redirects. Set BASE_URL to check a server that is already running instead (it
// must have FIRESIGHT_OAUTH_DCR=1 for the OAuth steps, which are otherwise skipped).

import { spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { request as httpRequest } from 'node:http';
import { request as httpsRequest } from 'node:https';
import { join } from 'node:path';
import { REPO_ROOT } from './_lib/dev-runtime.js';

const port = Number(process.env.E2E_PORT ?? 8790);
const external = process.env.BASE_URL?.replace(/\/+$/, '');
const base = external ?? `http://localhost:${port}`;
const PASSWORD = 'correct horse battery staple';

const steps: string[] = [];
const ok = (name: string) => {
  steps.push(name);
  console.log(`  ok  ${name}`);
};
const skipped: string[] = [];
const skip = (name: string, why: string) => {
  skipped.push(name);
  console.log(`  --  ${name} (skipped: ${why})`);
};
function expect(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

interface Reply<T> {
  status: number;
  headers: Headers;
  body: T;
  text: string;
}

/** One browser profile: a cookie jar, and `Origin` on writes as a page of the site would send. */
class Client {
  private readonly jar = new Map<string, string>();
  constructor(private readonly origin: string | null) {}

  async call<T = Record<string, unknown>>(method: string, path: string, options: { body?: unknown; form?: URLSearchParams; headers?: Record<string, string>; origin?: string | null } = {}): Promise<Reply<T>> {
    const headers = new Headers(options.headers);
    const origin = options.origin === undefined ? this.origin : options.origin;
    if (origin && method !== 'GET') headers.set('origin', origin);
    if (this.jar.size > 0) headers.set('cookie', [...this.jar].map(([k, v]) => `${k}=${v}`).join('; '));
    let body: string | undefined;
    if (options.form) {
      body = options.form.toString();
      headers.set('content-type', 'application/x-www-form-urlencoded');
    } else if (options.body !== undefined) {
      body = JSON.stringify(options.body);
      headers.set('content-type', 'application/json');
    }
    const response = await fetch(`${base}${path}`, { method, headers, body, redirect: 'manual' });
    for (const cookie of response.headers.getSetCookie()) {
      const [pair = '', ...attributes] = cookie.split(';');
      const eq = pair.indexOf('=');
      if (attributes.some((a) => /^\s*max-age=0\s*$/i.test(a)) || pair.slice(eq + 1) === '') this.jar.delete(pair.slice(0, eq).trim());
      else this.jar.set(pair.slice(0, eq).trim(), pair.slice(eq + 1));
    }
    const text = await response.text();
    let parsed: unknown = text;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {
      // Not JSON.
    }
    return { status: response.status, headers: response.headers, body: parsed as T, text };
  }

  /**
   * A top-level navigation, as when an MCP client opens the authorise URL in the user's
   * browser. Node's `fetch` cannot stand in for one: it labels every request
   * `sec-fetch-mode: cors`, and the server rightly answers a script differently from a
   * page load. So this goes out on a plain socket with the headers a browser sends.
   */
  navigate(path: string): Promise<{ status: number; location: string | null }> {
    const url = new URL(`${base}${path}`);
    const headers: Record<string, string> = { accept: 'text/html,application/xhtml+xml', 'sec-fetch-mode': 'navigate', 'sec-fetch-dest': 'document', 'sec-fetch-site': 'cross-site' };
    if (this.jar.size > 0) headers.cookie = [...this.jar].map(([k, v]) => `${k}=${v}`).join('; ');
    return new Promise((resolve, reject) => {
      const send = url.protocol === 'https:' ? httpsRequest : httpRequest;
      const req = send(url, { method: 'GET', headers }, (res) => {
        res.resume();
        res.on('end', () => resolve({ status: res.statusCode ?? 0, location: res.headers.location ?? null }));
      });
      req.on('error', reject);
      req.end();
    });
  }

  get = <T = Record<string, unknown>>(path: string, headers?: Record<string, string>) => this.call<T>('GET', path, { headers });
  post = <T = Record<string, unknown>>(path: string, body?: unknown, headers?: Record<string, string>) => this.call<T>('POST', path, { body, headers });
  get cookieNames() {
    return [...this.jar.keys()];
  }
}

const plan = (name: string, salary: number) => ({
  firesight: 2,
  profile: { schema_version: 2, name, household: { people: [{ name: 'Ada', birth_year: 1988 }] }, income: { jobs: [{ person: 0, salary }] } },
  scenarios: [],
  run: { paths: 2000 },
});

async function waitForServer(child: ChildProcess): Promise<void> {
  const deadline = Date.now() + 180_000;
  let exited: number | null = null;
  child.once('exit', (code) => (exited = code ?? 1));
  while (Date.now() < deadline) {
    if (exited !== null) throw new Error(`The dev API server exited with code ${exited} before it was ready.`);
    try {
      if ((await fetch(`${base}/api/config`)).ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error('The dev API server did not start within three minutes.');
}

let server: ChildProcess | null = null;
let serverOutput = '';

try {
  if (!external) {
    server = spawn(process.execPath, ['--import', 'tsx', join(REPO_ROOT, 'scripts', 'dev-api.ts')], {
      cwd: REPO_ROOT,
      env: { ...process.env, PORT: String(port), BETTER_AUTH_URL: base, FIRESIGHT_DEV_DATA: 'memory', FIRESIGHT_OAUTH_DCR: '1', DATABASE_URL: '', FIRESIGHT_DEV_QUIET: '1' },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    server.stdout?.on('data', (chunk: Buffer) => (serverOutput += chunk.toString()));
    server.stderr?.on('data', (chunk: Buffer) => (serverOutput += chunk.toString()));
    await waitForServer(server);
  }

  // ---- what the deployment offers ----------------------------------------------------
  const anonymous = new Client(base);
  const config = await anonymous.get<{ accounts: boolean; mcp_url: string; social: string[]; dcr: boolean }>('/api/config');
  expect(config.status === 200 && config.body.accounts === true, `GET /api/config: ${config.status} ${config.text}`);
  expect(config.body.mcp_url === `${base}/api/mcp`, `mcp_url should be ${base}/api/mcp, got ${config.body.mcp_url}`);
  ok(`GET /api/config says accounts are on and names the MCP endpoint (${config.body.mcp_url})`);

  const prm = await anonymous.get<{ resource: string; authorization_servers: string[]; scopes_supported: string[] }>('/.well-known/oauth-protected-resource/api/mcp');
  expect(prm.status === 200 && prm.body.resource === `${base}/api/mcp` && prm.body.authorization_servers[0] === `${base}/api/auth`, `protected resource metadata: ${prm.text}`);
  expect(prm.headers.get('access-control-allow-origin') === '*', 'discovery documents must be readable cross-origin');
  const as = await anonymous.get<Record<string, unknown>>('/.well-known/oauth-authorization-server/api/auth');
  expect(as.status === 200 && as.body.issuer === `${base}/api/auth`, `authorization server metadata: ${as.text}`);
  expect(as.body.client_id_metadata_document_supported === true, 'client_id_metadata_document_supported should be true');
  expect((as.body.token_endpoint_auth_methods_supported as string[]).includes('none'), 'public clients ("none") must be supported');
  expect(JSON.stringify(as.body.code_challenge_methods_supported) === '["S256"]', 'PKCE S256 only');
  expect(as.body.authorization_response_iss_parameter_supported === true, 'the iss parameter should be advertised');
  const oidc = await anonymous.get<Record<string, unknown>>('/api/auth/.well-known/openid-configuration');
  const oidcInserted = await anonymous.get<Record<string, unknown>>('/.well-known/openid-configuration/api/auth');
  expect(oidc.status === 200 && oidcInserted.status === 200 && oidc.body.issuer === as.body.issuer, 'OpenID configuration at both locations');
  expect((await anonymous.get('/.well-known/oauth-authorization-server/some/other')).status === 404, 'another issuer path is a 404');
  const jwks = await anonymous.get<{ keys: unknown[] }>(new URL(String(as.body.jwks_uri)).pathname);
  expect(jwks.status === 200 && jwks.body.keys.length > 0, `JWKS at ${String(as.body.jwks_uri)}`);
  ok('the three discovery documents and the JWKS are served through the vercel.json rewrites');

  // ---- sign-up, session, sign-out, sign-in -------------------------------------------
  const email = `e2e-${randomBytes(4).toString('hex')}@example.com`;
  const browser = new Client(base);
  expect((await browser.get('/api/plans')).status === 401, 'plans need a session');
  const short = await browser.post('/api/auth/sign-up/email', { email, password: 'short', name: 'E2E' });
  expect(short.status === 400, `a short password should be refused, got ${short.status}`);
  const signUp = await browser.post<{ user: { id: string; email: string } }>('/api/auth/sign-up/email', { email, password: PASSWORD, name: 'E2E Person' });
  expect(signUp.status === 200 && signUp.body.user.email === email, `sign-up: ${signUp.status} ${signUp.text}`);
  const setCookie = signUp.headers.getSetCookie().find((c) => c.includes('session_token')) ?? '';
  expect(/HttpOnly/i.test(setCookie) && /SameSite=Lax/i.test(setCookie), `session cookie attributes: ${setCookie.replace(/=[^;]+/, '=…')}`);
  const session = await browser.get<{ user: { id: string; email: string } }>('/api/auth/get-session');
  expect(session.body?.user.email === email, 'the session is readable after sign-up');
  const userId = session.body.user.id;
  ok('sign-up creates the account and an HttpOnly, SameSite=Lax session cookie');

  expect((await browser.post('/api/auth/sign-out', {})).status === 200, 'sign-out');
  expect((await browser.get('/api/auth/get-session')).body === null, 'no session after sign-out');
  expect((await browser.post('/api/auth/sign-in/email', { email, password: 'the wrong password' })).status === 401, 'a wrong password is refused');
  expect((await browser.post('/api/auth/sign-in/email', { email, password: PASSWORD })).status === 200, 'sign-in');
  ok('sign-out ends the session; sign-in with the password starts a new one');

  // ---- plans: CRUD, concurrency, isolation, CSRF -------------------------------------
  type Meta = { id: string; name: string; rev: number; updated_by: string; bytes: number };
  const created = await browser.post<Meta>('/api/plans', { plan: plan('Household', 120_000), name: 'Our plan' });
  expect(created.status === 201 && /^pl_[0-9A-Za-z]{22}$/.test(created.body.id) && created.body.rev === 1, `create: ${created.status} ${created.text}`);
  const id = created.body.id;
  const fetched = await browser.get<Meta & { plan: unknown }>(`/api/plans/${id}`);
  expect(fetched.status === 200 && JSON.stringify(fetched.body.plan) === JSON.stringify(plan('Household', 120_000)), 'the plan reads back unchanged');
  expect(fetched.headers.get('cache-control') === 'no-store', 'plans are never cached');
  const updated = await browser.call<Meta>('PUT', `/api/plans/${id}`, { body: { plan: plan('Household', 130_000) }, headers: { 'if-match': '1' } });
  expect(updated.status === 200 && updated.body.rev === 2 && updated.body.name === 'Our plan', `update: ${updated.status} ${updated.text}`);
  const list = await browser.get<{ plans: Meta[] }>('/api/plans');
  expect(list.body.plans.length === 1 && list.body.plans[0]?.rev === 2, 'the list shows the plan at rev 2');
  ok('POST, GET and PUT /api/plans round-trip a plan and bump its rev');

  const stale = await browser.call<{ error: string; current: Meta }>('PUT', `/api/plans/${id}`, { body: { plan: plan('Household', 1) }, headers: { 'if-match': '1' } });
  expect(stale.status === 409 && stale.body.error === 'conflict' && stale.body.current.rev === 2, `a stale write should be a 409 with the current meta: ${stale.status} ${stale.text}`);
  expect((await browser.call('PUT', `/api/plans/${id}`, { body: { plan: plan('Household', 1) } })).status === 428, 'a write without If-Match is a 428');
  ok('a stale If-Match is a 409 carrying the current rev; a missing one is a 428');

  const forged = await browser.call('POST', '/api/plans', { body: { plan: plan('Forged', 1) }, origin: 'https://evil.example' });
  expect(forged.status === 403, `a write from another origin should be a 403, got ${forged.status}`);
  expect((await browser.call('DELETE', `/api/plans/${id}`, { origin: null })).status === 403, 'a write with no Origin is refused');
  const stranger = new Client(base);
  await stranger.post('/api/auth/sign-up/email', { email: `e2e-${randomBytes(4).toString('hex')}@example.com`, password: PASSWORD, name: 'Stranger' });
  expect((await stranger.get(`/api/plans/${id}`)).status === 404, "another user's plan is a 404");
  expect((await stranger.call('PUT', `/api/plans/${id}`, { body: { plan: plan('Mine', 1) }, headers: { 'if-match': '2' } })).status === 404, "another user's plan cannot be written");
  expect((await stranger.call('DELETE', `/api/plans/${id}`)).status === 404, "another user's plan cannot be deleted");
  expect((await stranger.get<{ plans: Meta[] }>('/api/plans')).body.plans.length === 0, "another user's list is empty");
  ok('cross-origin writes are refused and another user sees a 404 for every operation');

  const exported = await browser.get<{ plans: Array<Meta & { plan: unknown }> }>('/api/plans?export=1');
  expect(exported.status === 200 && exported.body.plans.length === 1 && exported.body.plans[0]?.plan !== undefined, 'export returns every plan with its body');
  ok('GET /api/plans?export=1 returns the plans with their bodies');

  // ---- API keys ----------------------------------------------------------------------
  type Key = { id: string; key: string; name: string; start: string; configId: string; permissions: Record<string, string[]> };
  const readOnly = await browser.post<Key>('/api/auth/api-key/create', { name: 'Reader' });
  const readWrite = await browser.post<Key>('/api/auth/api-key/create', { name: 'Claude Code', configId: 'read-write', expiresIn: 60 * 60 * 24 * 90 });
  expect(readOnly.status === 200 && readOnly.body.key.startsWith('fs_') && JSON.stringify(readOnly.body.permissions) === '{"plans":["read"]}', `read-only key: ${readOnly.status}`);
  expect(readWrite.status === 200 && JSON.stringify(readWrite.body.permissions) === '{"plans":["read","write"]}', `read-write key: ${readWrite.status}`);
  const keys = await browser.get<{ apiKeys: Array<Omit<Key, 'key'> & { key?: string }>; total: number }>('/api/auth/api-key/list');
  expect(keys.body.total === 2 && keys.body.apiKeys.every((k) => k.key === undefined && k.start.startsWith('fs_')), 'the list shows names and prefixes, never secrets');
  expect(!keys.text.includes(readWrite.body.key), 'the key secret is not in the list response');
  expect((await new Client(null).get('/api/plans', { authorization: `Bearer ${readWrite.body.key}` })).status === 401, 'an API key is not a login for the website API');
  expect((await stranger.post('/api/auth/api-key/delete', { keyId: readWrite.body.id })).status >= 400, 'a stranger cannot revoke the key');
  const revoked = await browser.post<{ success: boolean }>('/api/auth/api-key/delete', { keyId: readWrite.body.id });
  expect(revoked.status === 200 && revoked.body.success === true, `revoke: ${revoked.status} ${revoked.text}`);
  expect((await browser.get<{ total: number }>('/api/auth/api-key/list')).body.total === 1, 'one key left after revoking');
  ok('API keys: read-only by default, read-write on request, listed without secrets, revoked by id');

  // ---- the OAuth flow an MCP client runs ---------------------------------------------
  if (config.body.dcr) {
    const mcpClient = new Client(null);
    const redirect = 'http://127.0.0.1:33418/callback';
    const registered = await mcpClient.post<{ client_id: string }>('/api/auth/oauth2/register', { client_name: 'E2E MCP client', redirect_uris: [redirect], token_endpoint_auth_method: 'none', grant_types: ['authorization_code', 'refresh_token'], response_types: ['code'] });
    expect(registered.status === 201, `client registration: ${registered.status} ${registered.text}`);
    const verifier = randomBytes(32).toString('base64url');
    const query = new URLSearchParams({
      response_type: 'code',
      client_id: registered.body.client_id,
      redirect_uri: redirect,
      scope: 'plans:read plans:write offline_access',
      state: 'e2e-state',
      code_challenge: createHash('sha256').update(verifier).digest('base64url'),
      code_challenge_method: 'S256',
      resource: `${base}/api/mcp`,
    });

    // A browser with no session is sent to the login page with a signed query.
    const fresh = new Client(base);
    const toLogin = await fresh.navigate(`/api/auth/oauth2/authorize?${query}`);
    const login = new URL(toLogin.location ?? '', base);
    expect(toLogin.status === 302 && login.pathname === '/login' && login.searchParams.has('sig'), `authorize should redirect to /login: ${toLogin.status} ${toLogin.location}`);
    // Signing in with that query (what the Better Auth client plugin sends) continues to consent.
    const continued = await fresh.post<{ redirect: boolean; url: string }>('/api/auth/sign-in/email', { email, password: PASSWORD, oauth_query: login.search.slice(1) });
    const consentPage = new URL(continued.body.url ?? '', base);
    expect(continued.status === 200 && consentPage.pathname === '/consent', `sign-in should continue to /consent: ${continued.status} ${continued.text}`);
    const shown = await fresh.get<{ client_name: string }>(`/api/auth/oauth2/public-client?client_id=${encodeURIComponent(registered.body.client_id)}`);
    expect(shown.body.client_name === 'E2E MCP client', 'the consent page can look up who is asking');
    const consent = await fresh.post<{ redirect: boolean; url: string }>('/api/auth/oauth2/consent', { accept: true, oauth_query: consentPage.search.slice(1) });
    const callback = new URL(consent.body.url ?? '', base);
    expect(callback.href.startsWith(redirect) && callback.searchParams.get('state') === 'e2e-state' && callback.searchParams.get('iss') === `${base}/api/auth`, `consent should return to the client with code, state and iss: ${consent.text}`);
    ok('OAuth: authorize → /login → sign-in continues → /consent → Allow returns code, state and iss');

    const tokenRequest = (form: Record<string, string>) => mcpClient.call<{ access_token: string; refresh_token: string; error?: string }>('POST', '/api/auth/oauth2/token', { form: new URLSearchParams(form) });
    const tokens = await tokenRequest({ grant_type: 'authorization_code', code: callback.searchParams.get('code') ?? '', code_verifier: verifier, client_id: registered.body.client_id, redirect_uri: redirect, resource: `${base}/api/mcp` });
    expect(tokens.status === 200 && tokens.body.access_token.split('.').length === 3 && tokens.body.refresh_token, `token exchange: ${tokens.status} ${tokens.text.slice(0, 200)}`);
    const claims = JSON.parse(Buffer.from(tokens.body.access_token.split('.')[1] ?? '', 'base64url').toString()) as { sub: string; aud: string[]; iss: string; scope: string };
    expect(claims.sub === userId && claims.aud.includes(`${base}/api/mcp`) && claims.iss === `${base}/api/auth`, `access token claims: ${JSON.stringify({ ...claims, sub: '…' })}`);
    expect(claims.scope === 'plans:read plans:write offline_access', `granted scope: ${claims.scope}`);
    const refreshed = await tokenRequest({ grant_type: 'refresh_token', refresh_token: tokens.body.refresh_token, client_id: registered.body.client_id, resource: `${base}/api/mcp` });
    expect(refreshed.status === 200 && refreshed.body.refresh_token !== tokens.body.refresh_token, `refresh should rotate: ${refreshed.status}`);
    const replay = await tokenRequest({ grant_type: 'authorization_code', code: callback.searchParams.get('code') ?? '', code_verifier: verifier, client_id: registered.body.client_id, redirect_uri: redirect, resource: `${base}/api/mcp` });
    expect(replay.status === 400 && replay.body.error === 'invalid_grant', 'a code works once');
    ok('OAuth: the code exchanges (PKCE) for a JWT bound to the MCP URL, and the refresh token rotates');

    const consents = await browser.get<Array<{ id: string; clientId: string }>>('/api/auth/oauth2/get-consents');
    expect(consents.body.length === 1 && consents.body[0]?.clientId === registered.body.client_id, 'the connected app is listed');
    expect((await browser.post('/api/auth/oauth2/delete-consent', { id: consents.body[0]?.id })).status === 200, 'the connected app can be revoked');
    const cutOff = await tokenRequest({ grant_type: 'refresh_token', refresh_token: refreshed.body.refresh_token, client_id: registered.body.client_id, resource: `${base}/api/mcp` });
    expect(cutOff.status === 400, `a disconnected app's refresh token should be dead, got ${cutOff.status}`);
    ok('connected apps are listed, and disconnecting one kills its refresh token');
  } else {
    skip('OAuth authorisation code flow', 'dynamic client registration is off on this server, so the script cannot create a client');
  }

  // ---- account deletion --------------------------------------------------------------
  expect((await browser.post('/api/auth/delete-user', { password: 'not the password' })).status >= 400, 'deleting needs the password');
  expect((await browser.post('/api/auth/delete-user', {})).status === 400, 'deleting without the password is refused, fresh session or not');
  const deleted = await browser.post('/api/auth/delete-user', { password: PASSWORD });
  expect(deleted.status === 200, `delete account: ${deleted.status} ${deleted.text}`);
  expect((await browser.get('/api/plans')).status === 401, 'the session is gone with the account');
  expect((await new Client(base).post('/api/auth/sign-in/email', { email, password: PASSWORD })).status === 401, 'the account can no longer sign in');
  // The address is free again and the new account starts empty: nothing was left behind.
  const reborn = new Client(base);
  expect((await reborn.post('/api/auth/sign-up/email', { email, password: PASSWORD, name: 'Again' })).status === 200, 'the address can sign up again');
  expect((await reborn.get<{ plans: unknown[] }>('/api/plans')).body.plans.length === 0, 'the new account has no plans');
  expect((await reborn.get<{ total: number }>('/api/auth/api-key/list')).body.total === 0, 'the new account has no keys');
  expect((await reborn.get(`/api/plans/${id}`)).status === 404, 'the old plan is gone');
  ok('deleting the account removes the session, the plans and the keys');

  if (server) expect(!/\[dev-api\].*threw|server_error/.test(serverOutput), `the server reported errors:\n${serverOutput}`);
  console.log(`\ne2e-api: ${steps.length} checks passed${skipped.length ? `, ${skipped.length} skipped` : ''} against ${base}.`);
} catch (error) {
  console.error(`\ne2e-api FAILED after ${steps.length} checks: ${error instanceof Error ? error.message : String(error)}`);
  if (serverOutput) console.error(`--- dev API server output ---\n${serverOutput}`);
  process.exitCode = 1;
} finally {
  if (server && server.exitCode === null) {
    server.kill('SIGTERM');
    await new Promise((resolve) => server?.once('exit', resolve));
  }
}
