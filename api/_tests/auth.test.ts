import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { handle } from '../auth.js';
import { bearerApiKey, sessionUser, verifyApiKey } from '../_lib/credentials.js';
import { routedUrl } from '../_lib/http.js';
import { Browser, ORIGIN, PASSWORD, samplePlan, startRuntime } from './harness.js';
import type { TestRuntime } from './harness.js';

let t: TestRuntime;
let n = 0;
const email = () => `auth-${n++}@example.com`;

beforeAll(async () => {
  t = await startRuntime();
});

afterAll(() => t.close());

interface SessionReply {
  user: { id: string; email: string; name: string };
  session: { id: string };
}

describe('accounts', () => {
  it('signs up, holds a session in an HttpOnly cookie, signs out and signs back in', async () => {
    const browser = new Browser(t.state);
    const address = email();
    const signUp = await browser.post<{ user: { id: string; email: string } }>('/api/auth/sign-up/email', { email: address, password: PASSWORD, name: 'Ada' });
    expect(signUp.status).toBe(200);
    expect(signUp.body.user.email).toBe(address);
    const cookie = signUp.headers.getSetCookie().find((c) => c.includes('session_token')) ?? '';
    expect(cookie).toMatch(/^__Secure-firesight\.session_token=/);
    expect(cookie).toMatch(/HttpOnly/i);
    expect(cookie).toMatch(/Secure/i);
    expect(cookie).toMatch(/SameSite=Lax/i);
    expect(cookie).toMatch(/Path=\//i);
    expect(cookie).not.toMatch(/Domain=/i);
    expect(signUp.headers.get('cache-control')).toBe('no-store');

    const session = await browser.get<SessionReply>('/api/auth/get-session');
    expect(session.body.user).toMatchObject({ id: signUp.body.user.id, email: address, name: 'Ada' });

    expect((await browser.post('/api/auth/sign-out', {})).status).toBe(200);
    expect((await browser.get('/api/auth/get-session')).body).toBeNull();

    expect((await browser.post('/api/auth/sign-in/email', { email: address, password: 'not the password!' })).status).toBe(401);
    const signIn = await browser.post('/api/auth/sign-in/email', { email: address, password: PASSWORD });
    expect(signIn.status).toBe(200);
    expect((await browser.get<SessionReply>('/api/auth/get-session')).body.user.email).toBe(address);
  });

  it('requires a password of at least 12 characters', async () => {
    const browser = new Browser(t.state);
    const short = await browser.post<{ code: string }>('/api/auth/sign-up/email', { email: email(), password: 'elevenchars', name: 'A' });
    expect(short.status).toBe(400);
    expect(short.body.code).toBe('PASSWORD_TOO_SHORT');
    expect((await browser.post('/api/auth/sign-up/email', { email: email(), password: 'twelve chars', name: 'A' })).status).toBe(200);
  });

  it('sign-in does not say whether an email address has an account', async () => {
    const browser = new Browser(t.state);
    const address = email();
    await browser.signUp(address);
    const wrongPassword = await new Browser(t.state).post('/api/auth/sign-in/email', { email: address, password: 'wrong wrong wrong' });
    const noAccount = await new Browser(t.state).post('/api/auth/sign-in/email', { email: email(), password: 'wrong wrong wrong' });
    expect(wrongPassword.status).toBe(401);
    expect(noAccount.status).toBe(401);
    expect(wrongPassword.body).toEqual(noAccount.body);
  });

  // Known disclosure. With no email sender the server cannot answer "check your inbox"
  // for every address, so sign-up says when an address is taken (docs/accounts.md,
  // Limits). Revisit when `requireEmailVerification` is turned on: the library then
  // answers the same for a registered address as for a new one.
  it('sign-up with a registered address is refused (known disclosure until email verification exists)', async () => {
    const address = email();
    await new Browser(t.state).signUp(address);
    const again = await new Browser(t.state).post<{ code: string }>('/api/auth/sign-up/email', { email: address, password: 'another long password', name: 'Someone else' });
    expect(again.status).toBe(422);
    expect(again.body.code).toBe('USER_ALREADY_EXISTS_USE_ANOTHER_EMAIL');
    expect(again.headers.getSetCookie()).toEqual([]);
    // The first account is untouched: its password still works and the second one does not.
    expect((await new Browser(t.state).post('/api/auth/sign-in/email', { email: address, password: 'another long password' })).status).toBe(401);
    expect((await new Browser(t.state).post('/api/auth/sign-in/email', { email: address, password: PASSWORD })).status).toBe(200);
    expect((await t.runtime.db.query('select 1 from "user" where email = $1', [address])).rows).toHaveLength(1);
  });

  it('stores passwords hashed and session lookups keyed by the cookie only', async () => {
    const browser = new Browser(t.state);
    const id = await browser.signUp(email());
    const account = await t.runtime.db.query<{ password: string }>('select password from account where "userId" = $1', [id]);
    expect(account.rows[0]?.password).toBeTruthy();
    expect(account.rows[0]?.password).not.toContain(PASSWORD);

    const cookie = [...browser.cookies].map(([k, v]) => `${k}=${v}`).join('; ');
    const viaCookie = await sessionUser(t.runtime, new Request(ORIGIN, { headers: { cookie } }));
    expect(viaCookie?.id).toBe(id);
    expect(await sessionUser(t.runtime, new Request(ORIGIN))).toBeNull();
    expect(await sessionUser(t.runtime, new Request(ORIGIN, { headers: { cookie: 'firesight.session_token=forged.value' } }))).toBeNull();
  });

  it('rejects sign-in and sign-up posted from another site', async () => {
    const browser = new Browser(t.state);
    const address = email();
    await browser.signUp(address);
    // A browser that already holds a cookie for this site, driven by a page on another one.
    const forged = await browser.post('/api/auth/sign-in/email', { email: address, password: PASSWORD }, { origin: 'https://evil.example' });
    expect(forged.status).toBe(403);
    const signOut = await browser.post('/api/auth/sign-out', {}, { origin: 'https://evil.example' });
    expect(signOut.status).toBe(403);
    expect((await browser.get<SessionReply>('/api/auth/get-session')).body.user.email).toBe(address);
  });

  it('does not offer the session-JWT endpoint, or social sign-in that is not configured', async () => {
    const browser = new Browser(t.state);
    await browser.signUp(email());
    expect((await browser.get('/api/auth/token')).status).toBe(404);
    const social = await browser.post('/api/auth/sign-in/social', { provider: 'github', callbackURL: '/' });
    expect(social.status).toBeGreaterThanOrEqual(400);
  });

  it('works when the platform delivers the rewritten URL (?fs_path=) instead of the original path', async () => {
    const address = email();
    const response = await handle(
      new Request(`http://internal.invalid/api/auth?fs_path=sign-up/email`, { method: 'POST', headers: { origin: ORIGIN, 'content-type': 'application/json', 'x-forwarded-for': '198.51.100.7' }, body: JSON.stringify({ email: address, password: PASSWORD, name: 'Rewritten' }) }),
      {},
      { state: t.state },
    );
    expect(response.status).toBe(200);
    const cookie = response.headers.getSetCookie().map((c) => c.split(';')[0]).join('; ');
    const session = await handle(new Request(`http://internal.invalid/api/auth?fs_path=get-session&disableCookieCache=true`, { headers: { cookie } }), {}, { state: t.state });
    expect(((await session.json()) as SessionReply).user.email).toBe(address);
    expect((await handle(new Request(`${ORIGIN}/api/auth?fs_path=../plans`), {}, { state: t.state })).status).toBe(404);
    // Vercel's compiled rule repeats the matched path as `path=`; the copy goes, anything else called `path` stays.
    const routed = (query: string) => routedUrl(new Request(`http://internal.invalid/api/auth?${query}`), '/api/auth', ORIGIN);
    expect(routed('fs_path=oauth2/authorize&path=oauth2/authorize&state=x').href).toBe(`${ORIGIN}/api/auth/oauth2/authorize?state=x`);
    expect(routed('fs_path=get-session&path=other').href).toBe(`${ORIGIN}/api/auth/get-session?path=other`);
  });

  it('caps the request body', async () => {
    const reply = await new Browser(t.state).post('/api/auth/sign-up/email', { email: email(), password: PASSWORD, name: 'x'.repeat(70_000) });
    expect(reply.status).toBe(413);
  });
});

describe('account deletion', () => {
  it('needs the password, then removes the user and everything that hangs off them', async () => {
    const browser = new Browser(t.state);
    const id = await browser.signUp(email());
    const other = new Browser(t.state);
    const otherId = await other.signUp(email());

    const created = await browser.post<{ id: string }>('/api/plans', { plan: samplePlan('To be deleted') });
    expect(created.status).toBe(201);
    await other.post('/api/plans', { plan: samplePlan('Stays') });
    const key = await browser.post<{ key: string }>('/api/auth/api-key/create', { name: 'Doomed key', configId: 'read-write' });
    const otherKey = await other.post<{ key: string }>('/api/auth/api-key/create', { name: 'Surviving key' });
    expect((await verifyApiKey(t.runtime, key.body.key)).ok).toBe(true);

    const count = async (sql: string, user: string) => (await t.runtime.db.query<{ n: number }>(`select count(*)::int as n from ${sql}`, [user])).rows[0]?.n;
    expect(await count('plan where user_id = $1', id)).toBe(1);
    expect(await count('user_key where user_id = $1', id)).toBe(1);

    expect((await browser.post('/api/auth/delete-user', { password: 'not my password!' })).status).toBeGreaterThanOrEqual(400);
    expect(await count('"user" where id = $1', id)).toBe(1);
    expect((await browser.post('/api/auth/delete-user', { password: PASSWORD }, { origin: 'https://evil.example' })).status).toBe(403);
    // A failed attempt has cost the user nothing.
    expect((await verifyApiKey(t.runtime, key.body.key)).ok).toBe(true);
    expect(await count('plan where user_id = $1', id)).toBe(1);

    const deleted = await browser.post('/api/auth/delete-user', { password: PASSWORD });
    expect(deleted.status).toBe(200);

    expect(await count('"user" where id = $1', id)).toBe(0);
    expect(await count('plan where user_id = $1', id)).toBe(0);
    expect(await count('user_key where user_id = $1', id)).toBe(0);
    expect(await count('session where "userId" = $1', id)).toBe(0);
    expect(await count('account where "userId" = $1', id)).toBe(0);
    expect(await count('"apikey" where "referenceId" = $1', id)).toBe(0);

    // No table at all still mentions the user id.
    const tables = await t.runtime.db.query<{ table_name: string }>("select table_name from information_schema.tables where table_schema = current_schema() and table_type = 'BASE TABLE'");
    expect(tables.rows.length).toBeGreaterThan(10);
    for (const { table_name } of tables.rows) {
      const rows = await t.runtime.db.query<{ row: string }>(`select row_to_json(x)::text as row from "${table_name}" x`);
      expect(rows.rows.some((r) => r.row.includes(id)), `table ${table_name}`).toBe(false);
    }

    // The session and the key are dead; the other user is untouched.
    expect((await browser.get('/api/plans')).status).toBe(401);
    expect(await verifyApiKey(t.runtime, key.body.key)).toEqual({ ok: false, reason: 'invalid' });
    expect((await other.get<{ plans: unknown[] }>('/api/plans')).body.plans).toHaveLength(1);
    expect((await verifyApiKey(t.runtime, otherKey.body.key)).ok).toBe(true);
    expect(await count('user_key where user_id = $1', otherId)).toBe(1);
  });

  it('will not delete an account that has a password unless the password is sent', async () => {
    const browser = new Browser(t.state);
    const id = await browser.signUp(email());
    const exists = async () => (await t.runtime.db.query('select 1 from "user" where id = $1', [id])).rows.length === 1;

    // A fresh session alone is not enough: the library would accept this.
    for (const body of [{}, { password: '' }, { callbackURL: '/' }, { token: 'made-up-token' }]) {
      const refused = await browser.post<{ code: string }>('/api/auth/delete-user', body);
      expect(refused.status, JSON.stringify(body)).toBe(400);
      expect(refused.body.code, JSON.stringify(body)).toBe('PASSWORD_REQUIRED');
      expect(await exists()).toBe(true);
    }
    expect((await browser.get<SessionReply>('/api/auth/get-session')).body.user.id).toBe(id);
    // Signed out, the answer is the library's own 401, and says nothing about the account.
    expect((await new Browser(t.state).post('/api/auth/delete-user', {})).status).toBe(401);

    expect((await browser.post('/api/auth/delete-user', { password: PASSWORD })).status).toBe(200);
    expect(await exists()).toBe(false);
  });

  it('an account with no password can be deleted without one on a fresh session, and is refused on an old one', async () => {
    const browser = new Browser(t.state);
    const id = await browser.signUp(email());
    expect((await browser.post('/api/plans', { plan: samplePlan('Provider-only') })).status).toBe(201);
    // What signing in with GitHub leaves behind: one account row, no password.
    await t.runtime.db.query(`update account set "providerId" = 'github', "accountId" = 'gh-12345', password = null where "userId" = $1`, [id]);
    const accounts = await browser.get<{ providerId: string }[]>('/api/auth/list-accounts');
    expect(accounts.body.map((a) => a.providerId)).toEqual(['github']);
    const exists = async () => (await t.runtime.db.query('select 1 from "user" where id = $1', [id])).rows.length === 1;

    // There is no password to give, so any password is refused.
    const typed = await browser.post<{ code: string }>('/api/auth/delete-user', { password: 'anything-at-all' });
    expect(typed.status).toBe(400);
    expect(typed.body.code).toBe('CREDENTIAL_ACCOUNT_NOT_FOUND');

    // A session older than a day must sign in again first.
    await t.runtime.db.query(`update session set "createdAt" = now() - interval '2 days' where "userId" = $1`, [id]);
    const stale = await browser.post<{ code: string }>('/api/auth/delete-user', {});
    expect(stale.status).toBe(400);
    expect(stale.body.code).toBe('SESSION_EXPIRED');
    expect(await exists()).toBe(true);

    await t.runtime.db.query(`update session set "createdAt" = now() where "userId" = $1`, [id]);
    expect((await browser.post('/api/auth/delete-user', {}, { origin: 'https://evil.example' })).status).toBe(403);
    expect((await browser.post('/api/auth/delete-user', {})).status).toBe(200);
    expect(await exists()).toBe(false);
    expect((await t.runtime.db.query('select 1 from plan where user_id = $1', [id])).rows).toHaveLength(0);
    expect((await t.runtime.db.query('select 1 from user_key where user_id = $1', [id])).rows).toHaveLength(0);
  });

  it('also removes OAuth clients, consents and tokens that belong to the user', async () => {
    const browser = new Browser(t.state);
    const id = await browser.signUp(email());
    const fk = await t.runtime.db.query<{ tbl: string; ondelete: string }>(
      `select conrelid::regclass::text as tbl, confdeltype as ondelete from pg_constraint where contype = 'f' and confrelid = '"user"'::regclass and connamespace = current_schema()::regnamespace`,
    );
    const tables = fk.rows.map((r) => r.tbl.replaceAll('"', '')).sort();
    expect(tables).toEqual(['account', 'oauthAccessToken', 'oauthClient', 'oauthConsent', 'oauthRefreshToken', 'plan', 'session', 'user_key']);
    for (const row of fk.rows) expect(row.ondelete, row.tbl).toBe('c');
    expect((await browser.post('/api/auth/delete-user', { password: PASSWORD })).status).toBe(200);
    expect((await t.runtime.db.query('select 1 from "user" where id = $1', [id])).rows).toHaveLength(0);
  });
});

describe('API keys', () => {
  interface Key {
    id: string;
    key?: string;
    name: string;
    start: string;
    prefix: string;
    configId: string;
    permissions: Record<string, string[]>;
    expiresAt: string | null;
    lastRequest: string | null;
    createdAt: string;
  }

  it('creates read-only keys by default and read-write keys on request, shown once', async () => {
    const browser = new Browser(t.state);
    const id = await browser.signUp(email());

    const readOnly = await browser.post<Key>('/api/auth/api-key/create', { name: 'Reader' });
    expect(readOnly.status).toBe(200);
    expect(readOnly.body.key).toMatch(/^fs_[A-Za-z]{64}$/);
    expect(readOnly.body).toMatchObject({ name: 'Reader', prefix: 'fs_', configId: 'default', permissions: { plans: ['read'] }, expiresAt: null });
    expect(readOnly.body.start).toBe(readOnly.body.key?.slice(0, 6));

    const readWrite = await browser.post<Key>('/api/auth/api-key/create', { name: 'Claude Code', configId: 'read-write', expiresIn: 60 * 60 * 24 * 30 });
    expect(readWrite.body.permissions).toEqual({ plans: ['read', 'write'] });
    expect(Date.parse(readWrite.body.expiresAt ?? '')).toBeGreaterThan(Date.now() + 29 * 86_400_000);

    // An unknown kind fails safe: read-only.
    const typo = await browser.post<Key>('/api/auth/api-key/create', { name: 'Typo', configId: 'readwrite' });
    expect(typo.body.permissions).toEqual({ plans: ['read'] });

    const list = await browser.get<{ apiKeys: Key[]; total: number }>('/api/auth/api-key/list');
    expect(list.body.total).toBe(3);
    expect(list.body.apiKeys.map((k) => k.name).sort()).toEqual(['Claude Code', 'Reader', 'Typo']);
    for (const listed of list.body.apiKeys) {
      expect(listed).not.toHaveProperty('key');
      expect(listed.start).toMatch(/^fs_/);
    }
    expect(list.text).not.toContain(readOnly.body.key);

    expect(await verifyApiKey(t.runtime, readOnly.body.key ?? '')).toEqual({ ok: true, keyId: readOnly.body.id, principal: { userId: id, scopes: ['plans:read'], via: 'apikey:Reader' } });
    expect(await verifyApiKey(t.runtime, readWrite.body.key ?? '')).toEqual({ ok: true, keyId: readWrite.body.id, principal: { userId: id, scopes: ['plans:read', 'plans:write'], via: 'apikey:Claude Code' } });

    // Use is recorded, for the "last used" column.
    const after = await browser.get<{ apiKeys: Key[] }>('/api/auth/api-key/list');
    expect(after.body.apiKeys.find((k) => k.id === readOnly.body.id)?.lastRequest).not.toBeNull();
  });

  it('stores only a hash of the key', async () => {
    const browser = new Browser(t.state);
    await browser.signUp(email());
    const created = await browser.post<Key>('/api/auth/api-key/create', { name: 'Hashed' });
    const row = await t.runtime.db.query<{ key: string; start: string }>('select key, start from "apikey" where id = $1', [created.body.id]);
    expect(row.rows[0]?.key).toBeTruthy();
    expect(row.rows[0]?.key).not.toBe(created.body.key);
    expect(created.body.key).not.toContain(row.rows[0]?.key ?? 'x');
    const all = await t.runtime.db.query<{ row: string }>('select row_to_json(x)::text as row from "apikey" x');
    expect(all.rows.some((r) => r.row.includes(created.body.key ?? 'x'))).toBe(false);
  });

  it('will not let a browser choose permissions, an owner or rate limits', async () => {
    const browser = new Browser(t.state);
    await browser.signUp(email());
    const victim = await new Browser(t.state).signUp(email());
    for (const extra of [{ permissions: { plans: ['read', 'write'] } }, { rateLimitMax: 1_000_000 }, { rateLimitEnabled: false }, { userId: victim }, { remaining: 5 }]) {
      const reply = await browser.post('/api/auth/api-key/create', { name: 'Sneaky', ...extra });
      expect(reply.status, JSON.stringify(extra)).toBeGreaterThanOrEqual(400);
    }
    const created = await browser.post<Key>('/api/auth/api-key/create', { name: 'Plain' });
    const upgrade = await browser.post('/api/auth/api-key/update', { keyId: created.body.id, permissions: { plans: ['read', 'write'] } });
    expect(upgrade.status).toBeGreaterThanOrEqual(400);
    const check = await verifyApiKey(t.runtime, created.body.key ?? '');
    expect(check.ok && check.principal.scopes).toEqual(['plans:read']);
    expect((await t.runtime.db.query('select 1 from "apikey" where "referenceId" = $1', [victim])).rows).toHaveLength(0);
  });

  it('requires a session and a name to create a key', async () => {
    expect((await new Browser(t.state).post('/api/auth/api-key/create', { name: 'Anonymous' })).status).toBe(401);
    const browser = new Browser(t.state);
    await browser.signUp(email());
    expect((await browser.post('/api/auth/api-key/create', {})).status).toBe(400);
    expect((await browser.post('/api/auth/api-key/create', { name: 'n'.repeat(65) })).status).toBe(400);
    expect((await browser.post('/api/auth/api-key/create', { name: 'Forged' }, { origin: 'https://evil.example' })).status).toBe(403);
  });

  it('revokes a key at once, and only its owner can', async () => {
    const browser = new Browser(t.state);
    await browser.signUp(email());
    const stranger = new Browser(t.state);
    await stranger.signUp(email());
    const created = await browser.post<Key>('/api/auth/api-key/create', { name: 'Short-lived', configId: 'read-write' });
    expect((await verifyApiKey(t.runtime, created.body.key ?? '')).ok).toBe(true);

    expect((await stranger.post('/api/auth/api-key/delete', { keyId: created.body.id })).status).toBeGreaterThanOrEqual(400);
    expect((await stranger.get<{ apiKeys: Key[] }>('/api/auth/api-key/list')).body.apiKeys).toHaveLength(0);
    expect((await verifyApiKey(t.runtime, created.body.key ?? '')).ok).toBe(true);

    const revoked = await browser.post<{ success: boolean }>('/api/auth/api-key/delete', { keyId: created.body.id });
    expect(revoked.status).toBe(200);
    expect(revoked.body.success).toBe(true);
    expect(await verifyApiKey(t.runtime, created.body.key ?? '')).toEqual({ ok: false, reason: 'invalid' });
    expect((await browser.get<{ apiKeys: Key[] }>('/api/auth/api-key/list')).body.apiKeys).toHaveLength(0);
  });

  it('rejects keys that are wrong, malformed or expired, without saying which', async () => {
    const browser = new Browser(t.state);
    await browser.signUp(email());
    const created = await browser.post<Key>('/api/auth/api-key/create', { name: 'Expiring', expiresIn: 86_400 });
    const key = created.body.key ?? '';
    for (const bad of ['', 'fs_', `fs_${'a'.repeat(64)}`, key.slice(0, -1), `${key}x`, key.replace(/^fs_/, 'sk_'), 'x'.repeat(5000)]) {
      expect(await verifyApiKey(t.runtime, bad), bad.slice(0, 12)).toEqual({ ok: false, reason: 'invalid' });
    }
    await t.runtime.db.query(`update "apikey" set "expiresAt" = now() - interval '1 minute' where id = $1`, [created.body.id]);
    expect(await verifyApiKey(t.runtime, key)).toEqual({ ok: false, reason: 'invalid' });
  });

  it('rate-limits each key on its own counter', async () => {
    const browser = new Browser(t.state);
    await browser.signUp(email());
    const a = await browser.post<Key>('/api/auth/api-key/create', { name: 'Busy' });
    const b = await browser.post<Key>('/api/auth/api-key/create', { name: 'Quiet' });
    const row = await t.runtime.db.query<{ rateLimitMax: number; rateLimitTimeWindow: number; rateLimitEnabled: boolean }>('select "rateLimitMax", "rateLimitTimeWindow", "rateLimitEnabled" from "apikey" where id = $1', [a.body.id]);
    expect(row.rows[0]).toEqual({ rateLimitMax: 120, rateLimitTimeWindow: 60_000, rateLimitEnabled: true });
    // Lower the ceiling for this key rather than making 120 calls.
    await t.runtime.db.query('update "apikey" set "rateLimitMax" = 3 where id = $1', [a.body.id]);
    for (let i = 0; i < 3; i++) expect((await verifyApiKey(t.runtime, a.body.key ?? '')).ok).toBe(true);
    const limited = await verifyApiKey(t.runtime, a.body.key ?? '');
    expect(limited).toMatchObject({ ok: false, reason: 'rate_limited' });
    expect(limited.ok === false && limited.reason === 'rate_limited' && limited.retryAfterSeconds).toBeGreaterThan(0);
    expect((await verifyApiKey(t.runtime, b.body.key ?? '')).ok).toBe(true);
  });

  it('reads a key from an Authorization header only when it is one', () => {
    const request = (authorization?: string) => new Request(ORIGIN, { headers: authorization ? { authorization } : {} });
    expect(bearerApiKey(request('Bearer fs_abc'))).toBe('fs_abc');
    expect(bearerApiKey(request('bearer   fs_abc  '))).toBe('fs_abc');
    expect(bearerApiKey(request('Bearer eyJhbGciOi.x.y'))).toBeNull();
    expect(bearerApiKey(request('Basic fs_abc'))).toBeNull();
    expect(bearerApiKey(request())).toBeNull();
  });
});

describe('logging', () => {
  it('has written no password, session token, API key or plan content to the library log', async () => {
    const browser = new Browser(t.state);
    await browser.signUp(email());
    const key = await browser.post<{ key: string }>('/api/auth/api-key/create', { name: 'Logged?' });
    await browser.post('/api/plans', { plan: samplePlan('Not in logs') });
    await verifyApiKey(t.runtime, `${key.body.key}x`);
    await browser.post('/api/auth/sign-in/email', { email: 'nobody@example.com', password: 'wrong wrong wrong' });
    const log = t.logs.join('\n');
    expect(log).not.toContain(PASSWORD);
    expect(log).not.toContain(key.body.key);
    expect(log).not.toContain('Not in logs');
    for (const value of browser.cookies.values()) expect(log).not.toContain(decodeURIComponent(value).split('.')[0]);
  });
});
