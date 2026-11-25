import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { handle } from '../plans.js';
import { MAX_BODY_BYTES, READS_PER_MINUTE, WRITES_PER_MINUTE, expectedRev, planIdOf } from '../plans.js';
import { MAX_PLAN_BYTES } from '../_lib/plans.js';
import type { PlanMeta, StoredPlan } from '../_lib/plans.js';
import { Browser, ORIGIN, samplePlan, startRuntime } from './harness.js';
import type { TestRuntime } from './harness.js';

let t: TestRuntime;
let alice: Browser;
let bob: Browser;
let n = 0;

const signedIn = async () => {
  const browser = new Browser(t.state);
  await browser.signUp(`api-${n++}@example.com`);
  return browser;
};

beforeAll(async () => {
  t = await startRuntime();
  alice = await signedIn();
  bob = await signedIn();
});

afterAll(() => t.close());

describe('request parsing', () => {
  it('finds the plan id in the path or in the rewrite query', () => {
    expect(planIdOf(new URL('/api/plans', ORIGIN))).toBe('');
    expect(planIdOf(new URL('/api/plans?export=1', ORIGIN))).toBe('');
    expect(planIdOf(new URL('/api/plans/pl_abc', ORIGIN))).toBe('pl_abc');
    expect(planIdOf(new URL('/api/plans?id=pl_abc', ORIGIN))).toBe('pl_abc');
    expect(planIdOf(new URL('/api/plans/pl_abc/extra', ORIGIN))).toBeNull();
    expect(planIdOf(new URL('/api/other', ORIGIN))).toBeNull();
  });

  it('reads the rev from If-Match in plain, quoted and weak forms', () => {
    const rev = (value?: string) => expectedRev(new Request(ORIGIN, { headers: value === undefined ? {} : { 'if-match': value } }));
    expect(rev()).toBeUndefined();
    expect(rev('3')).toBe(3);
    expect(rev('"12"')).toBe(12);
    expect(rev('W/"7"')).toBe(7);
    for (const bad of ['*', '0', '-1', 'abc', '1.5', '"1", "2"', '99999999999']) expect(rev(bad), bad).toBeNull();
  });
});

describe('plans over HTTP', () => {
  it('runs the whole life of a plan', async () => {
    const browser = await signedIn();
    expect((await browser.get('/api/plans')).body).toEqual({ plans: [] });

    const created = await browser.post<PlanMeta>('/api/plans', { plan: samplePlan('Household'), name: 'Our plan' });
    expect(created.status).toBe(201);
    expect(created.body).toMatchObject({ name: 'Our plan', rev: 1, updated_by: 'web' });
    expect(Object.keys(created.body).sort()).toEqual(['bytes', 'created_at', 'id', 'name', 'rev', 'updated_at', 'updated_by']);
    expect(created.headers.get('location')).toBe(`/api/plans/${created.body.id}`);
    expect(created.headers.get('etag')).toBe('"1"');
    const id = created.body.id;

    const fetched = await browser.get<StoredPlan>(`/api/plans/${id}`);
    expect(fetched.status).toBe(200);
    expect(fetched.body.plan).toEqual(samplePlan('Household'));
    expect(fetched.body).toMatchObject(created.body);
    expect(fetched.headers.get('etag')).toBe('"1"');

    const updated = await browser.put<PlanMeta>(`/api/plans/${id}`, { plan: samplePlan('Household', 140_000) }, { headers: { 'if-match': '1' } });
    expect(updated.status).toBe(200);
    expect(updated.body).toMatchObject({ id, rev: 2, name: 'Our plan' });

    const list = await browser.get<{ plans: PlanMeta[] }>('/api/plans');
    expect(list.body.plans).toHaveLength(1);
    expect(list.body.plans[0]).toEqual(updated.body);

    const exported = await browser.get<{ exported_at: string; plans: StoredPlan[] }>('/api/plans?export=1');
    expect(exported.status).toBe(200);
    expect(exported.headers.get('content-disposition')).toContain('attachment');
    expect(exported.body.plans[0]?.plan).toEqual(samplePlan('Household', 140_000));
    expect(Date.parse(exported.body.exported_at)).not.toBeNaN();

    const deleted = await browser.delete(`/api/plans/${id}`);
    expect(deleted.status).toBe(204);
    expect(deleted.text).toBe('');
    expect((await browser.get(`/api/plans/${id}`)).status).toBe(404);
    expect((await browser.delete(`/api/plans/${id}`)).status).toBe(404);
  });

  it('works when the platform delivers the rewritten URL instead of the original path', async () => {
    const created = await alice.post<PlanMeta>('/api/plans', { plan: samplePlan('Rewritten') });
    const viaQuery = await alice.get<StoredPlan>(`/api/plans?id=${created.body.id}`);
    expect(viaQuery.status).toBe(200);
    expect(viaQuery.body.name).toBe('Rewritten');
    const put = await alice.put<PlanMeta>(`/api/plans?id=${created.body.id}`, { plan: samplePlan('Rewritten') }, { headers: { 'if-match': '"1"' } });
    expect(put.body.rev).toBe(2);
    expect((await alice.delete(`/api/plans?id=${created.body.id}`)).status).toBe(204);
  });

  it('never lets a response be cached', async () => {
    const created = await alice.post<PlanMeta>('/api/plans', { plan: samplePlan() });
    for (const reply of [created, await alice.get('/api/plans'), await alice.get(`/api/plans/${created.body.id}`), await alice.get('/api/plans/pl_nope'), await alice.delete(`/api/plans/${created.body.id}`), await new Browser(t.state).get('/api/plans')]) {
      expect(reply.headers.get('cache-control')).toBe('no-store');
    }
  });
});

describe('optimistic concurrency', () => {
  it('answers 428 without If-Match, 400 for a bad one, 409 with the current state for a stale one', async () => {
    const created = await alice.post<PlanMeta>('/api/plans', { plan: samplePlan('Shared') });
    const path = `/api/plans/${created.body.id}`;

    const missing = await alice.put(path, { plan: samplePlan('Shared') });
    expect(missing.status).toBe(428);
    expect(missing.body).toMatchObject({ error: 'precondition_required' });
    expect((await alice.put(path, { plan: samplePlan('Shared') }, { headers: { 'if-match': '*' } })).status).toBe(400);

    // An agent writes through the shared store while the browser still holds rev 1.
    const userId = (await alice.get<{ user: { id: string } }>('/api/auth/get-session')).body.user.id;
    await t.runtime.plans.updatePlan(userId, created.body.id, samplePlan('Shared', 99), 1, 'apikey:Claude Code');

    const stale = await alice.put<{ error: string; current: PlanMeta }>(path, { plan: samplePlan('Shared', 1) }, { headers: { 'if-match': '1' } });
    expect(stale.status).toBe(409);
    expect(stale.body.error).toBe('conflict');
    expect(stale.body.current).toMatchObject({ id: created.body.id, rev: 2, updated_by: 'apikey:Claude Code' });

    // Having seen the current rev, the browser can write.
    const retry = await alice.put<PlanMeta>(path, { plan: samplePlan('Shared', 1) }, { headers: { 'if-match': String(stale.body.current.rev) } });
    expect(retry.status).toBe(200);
    expect(retry.body).toMatchObject({ rev: 3, updated_by: 'web' });
  });
});

describe('authentication and isolation', () => {
  it('requires a session for everything', async () => {
    const anonymous = new Browser(t.state);
    for (const [method, path] of [['GET', '/api/plans'], ['GET', '/api/plans?export=1'], ['POST', '/api/plans'], ['GET', '/api/plans/pl_0000000000000000000000'], ['PUT', '/api/plans/pl_0000000000000000000000'], ['DELETE', '/api/plans/pl_0000000000000000000000']] as const) {
      const reply = await anonymous.call(method, path, method === 'GET' || method === 'DELETE' ? {} : { body: { plan: samplePlan() }, headers: { 'if-match': '1' } });
      expect(reply.status, `${method} ${path}`).toBe(401);
      expect(reply.body).toMatchObject({ error: 'unauthenticated' });
    }
  });

  it('ignores bearer credentials: an API key or a session token in Authorization is not a login', async () => {
    const key = await alice.post<{ key: string }>('/api/auth/api-key/create', { name: 'Agent', configId: 'read-write' });
    expect(key.status).toBe(200);
    const token = alice.cookies.get('__Secure-firesight.session_token') ?? '';
    expect(token).not.toBe('');
    const outsider = new Browser(t.state);
    const attempts: Record<string, string>[] = [{ authorization: `Bearer ${key.body.key}` }, { 'x-api-key': key.body.key }, { authorization: `Bearer ${decodeURIComponent(token).split('.')[0]}` }];
    for (const headers of attempts) {
      expect((await outsider.get('/api/plans', { headers })).status).toBe(401);
      expect((await outsider.post('/api/plans', { plan: samplePlan() }, { headers })).status).toBe(401);
    }
  });

  it("answers 404, not 403, for another user's plan, and leaves it alone", async () => {
    const mine = await alice.post<PlanMeta>('/api/plans', { plan: samplePlan("Alice's") });
    const path = `/api/plans/${mine.body.id}`;
    const nonexistent = await bob.get('/api/plans/pl_0000000000000000000000');

    const read = await bob.get(path);
    expect(read.status).toBe(404);
    // Indistinguishable from a plan that does not exist.
    expect(read.body).toEqual(nonexistent.body);
    expect(read.text).not.toContain("Alice's");
    expect((await bob.put(path, { plan: samplePlan('Mine now') }, { headers: { 'if-match': '1' } })).status).toBe(404);
    expect((await bob.delete(path)).status).toBe(404);
    expect((await bob.get<{ plans: PlanMeta[] }>('/api/plans')).body.plans.some((p) => p.id === mine.body.id)).toBe(false);
    expect((await bob.get<{ plans: PlanMeta[] }>('/api/plans?export=1')).body.plans.some((p) => p.id === mine.body.id)).toBe(false);

    expect((await alice.get<StoredPlan>(path)).body).toMatchObject({ rev: 1, name: "Alice's" });
  });

  // The page says which account it believes it is acting for. If another tab has since
  // signed in as someone else, the cookie no longer agrees, and the request must not be
  // served from (or written into) the other account.
  it('answers 401 account_changed when the page names a different account from the cookie', async () => {
    const browser = new Browser(t.state);
    const id = await browser.signUp(`api-${n++}@example.com`);
    const mine = await browser.post<PlanMeta>('/api/plans', { plan: samplePlan('Mine') });
    const stranger = await new Browser(t.state).signUp(`api-${n++}@example.com`);
    const stale = { headers: { 'x-firesight-user': stranger } };

    const attempts = [
      await browser.get<{ error: string }>('/api/plans', stale),
      await browser.get<{ error: string }>('/api/plans?export=1', stale),
      await browser.get<{ error: string }>(`/api/plans/${mine.body.id}`, stale),
      await browser.post<{ error: string }>('/api/plans', { plan: samplePlan('Leaked') }, stale),
      await browser.put<{ error: string }>(`/api/plans/${mine.body.id}`, { plan: samplePlan('Overwritten') }, { headers: { ...stale.headers, 'if-match': '1' } }),
      await browser.delete<{ error: string }>(`/api/plans/${mine.body.id}`, stale),
    ];
    for (const reply of attempts) {
      expect(reply.status).toBe(401);
      expect(reply.body.error).toBe('account_changed');
      expect(reply.text).not.toContain('Mine');
    }
    // Nothing was written, to either account.
    const after = await browser.get<{ plans: PlanMeta[] }>('/api/plans');
    expect(after.body.plans.map((p) => [p.name, p.rev])).toEqual([['Mine', 1]]);
    expect((await t.runtime.db.query('select 1 from plan where user_id = $1', [stranger])).rows).toHaveLength(0);

    // The right id, or no header at all (an older page), is served as before.
    expect((await browser.get('/api/plans', { headers: { 'x-firesight-user': id } })).status).toBe(200);
    expect((await browser.put(`/api/plans/${mine.body.id}`, { plan: samplePlan('Mine') }, { headers: { 'x-firesight-user': id, 'if-match': '1' } })).status).toBe(200);
    // Signed out, the header changes nothing: the answer is the ordinary 401.
    const signedOut = await new Browser(t.state).get<{ error: string }>('/api/plans', stale);
    expect(signedOut.status).toBe(401);
    expect(signedOut.body.error).toBe('unauthenticated');
  });

  it('stops serving a session the moment the user signs out', async () => {
    const browser = await signedIn();
    const cookie = [...browser.cookies].map(([k, v]) => `${k}=${v}`).join('; ');
    expect((await browser.post('/api/auth/sign-out', {})).status).toBe(200);
    expect((await browser.get('/api/plans')).status).toBe(401);
    // Replaying the old cookie does not work either.
    expect((await new Browser(t.state).get('/api/plans', { headers: { cookie } })).status).toBe(401);
  });
});

describe('cross-site request forgery', () => {
  it('refuses writes that do not come from this site', async () => {
    const created = await alice.post<PlanMeta>('/api/plans', { plan: samplePlan('Target') });
    const path = `/api/plans/${created.body.id}`;
    for (const origin of ['https://evil.example', 'https://firesight.test.evil.example', 'http://firesight.test', 'null', null]) {
      const post = await alice.post('/api/plans', { plan: samplePlan('Forged') }, { origin });
      expect(post.status, `POST from ${origin}`).toBe(403);
      expect(post.body).toMatchObject({ error: 'forbidden_origin' });
      expect((await alice.put(path, { plan: samplePlan('Forged') }, { origin, headers: { 'if-match': '1' } })).status).toBe(403);
      expect((await alice.delete(path, { origin })).status).toBe(403);
    }
    expect((await alice.get<StoredPlan>(path)).body).toMatchObject({ rev: 1, name: 'Target' });
  });

  it('refuses reads that are positively from another site, and allows ordinary ones', async () => {
    expect((await alice.get('/api/plans', { origin: 'https://evil.example' })).status).toBe(403);
    expect((await alice.get('/api/plans', { headers: { 'sec-fetch-site': 'cross-site' } })).status).toBe(403);
    expect((await alice.get('/api/plans', { headers: { 'sec-fetch-site': 'same-origin' } })).status).toBe(200);
    expect((await alice.get('/api/plans', { origin: ORIGIN })).status).toBe(200);
  });

  it('accepts JSON only, so a cross-site form cannot post a plan', async () => {
    for (const type of ['text/plain', 'application/x-www-form-urlencoded', 'multipart/form-data']) {
      const reply = await alice.post('/api/plans', JSON.stringify({ plan: samplePlan() }), { headers: { 'content-type': type } });
      expect(reply.status, type).toBe(415);
    }
    expect((await alice.post('/api/plans', JSON.stringify({ plan: samplePlan() }), { headers: { 'content-type': 'application/json; charset=utf-8' } })).status).toBe(201);
  });

  it('accepts an origin the operator has listed', async () => {
    const extra = { ...t.state, runtime: { ...t.runtime, config: { ...t.config, extraOrigins: ['https://staging.firesight.test'] }, auth: t.runtime.auth } } as typeof t.state;
    const request = (origin: string) =>
      handle(new Request(`${ORIGIN}/api/plans`, { method: 'POST', headers: { origin, 'content-type': 'application/json' }, body: '{}' }), {}, { state: extra });
    expect((await request('https://staging.firesight.test')).status).toBe(401);
    expect((await request('https://other.firesight.test')).status).toBe(403);
  });
});

describe('bad input and limits', () => {
  it('rejects malformed bodies with a reason', async () => {
    expect((await alice.post('/api/plans', '{not json')).status).toBe(400);
    expect((await alice.post('/api/plans', '[1,2]')).status).toBe(400);
    expect((await alice.post('/api/plans', { plan: samplePlan(), name: 5 })).status).toBe(400);
    const invalid = await alice.post<{ error: string; message: string }>('/api/plans', { plan: { firesight: 1 } });
    expect(invalid.status).toBe(400);
    expect(invalid.body.error).toBe('invalid_plan');
    expect((await alice.post('/api/plans', {})).body).toMatchObject({ error: 'invalid_plan' });
  });

  it('caps the plan at 256 KB and the request body a little above that', async () => {
    expect(MAX_PLAN_BYTES).toBe(256 * 1024);
    const tooBigPlan = await alice.post('/api/plans', { plan: { firesight: 2, profile: { note: 'x'.repeat(MAX_PLAN_BYTES) }, scenarios: [] } });
    expect(tooBigPlan.status).toBe(413);
    expect(tooBigPlan.body).toMatchObject({ error: 'too_large' });

    const huge = await alice.post('/api/plans', JSON.stringify({ plan: { firesight: 2, profile: { note: 'x'.repeat(MAX_BODY_BYTES) }, scenarios: [] } }));
    expect(huge.status).toBe(413);
    // A declared length over the cap is refused before the body is read.
    const declared = await handle(
      new Request(`${ORIGIN}/api/plans`, { method: 'POST', headers: { origin: ORIGIN, 'content-type': 'application/json', 'content-length': String(MAX_BODY_BYTES + 1), cookie: [...alice.cookies].map(([k, v]) => `${k}=${v}`).join('; ') }, body: '{}' }),
      {},
      { state: t.state },
    );
    expect(declared.status).toBe(413);
  });

  it('answers 405 with the allowed methods', async () => {
    const collection = await alice.call('DELETE', '/api/plans');
    expect(collection.status).toBe(405);
    expect(collection.headers.get('allow')).toBe('GET, POST');
    const item = await alice.call('POST', '/api/plans/pl_0000000000000000000000', { body: {} });
    expect(item.status).toBe(405);
    expect(item.headers.get('allow')).toBe('GET, PUT, DELETE');
    expect((await alice.get('/api/plans/a/b')).status).toBe(404);
  });

  it('rate-limits per user, reads and writes separately, and recovers in the next minute', async () => {
    const browser = await signedIn();
    const cookie = [...browser.cookies].map(([k, v]) => `${k}=${v}`).join('; ');
    let clock = 1_800_000_000_000;
    const get = () => handle(new Request(`${ORIGIN}/api/plans`, { headers: { cookie } }), {}, { state: t.state, now: () => clock });
    const post = () => handle(new Request(`${ORIGIN}/api/plans`, { method: 'POST', headers: { cookie, origin: ORIGIN, 'content-type': 'application/json' }, body: '{}' }), {}, { state: t.state, now: () => clock });

    for (let i = 0; i < WRITES_PER_MINUTE; i++) expect((await post()).status).toBe(400);
    const limited = await post();
    expect(limited.status).toBe(429);
    expect(Number(limited.headers.get('retry-after'))).toBeGreaterThan(0);
    // Reads have their own allowance, and another user is unaffected.
    expect((await get()).status).toBe(200);
    expect((await alice.post('/api/plans', {})).status).toBe(400);

    clock += 60_000;
    expect((await post()).status).toBe(400);
    expect(READS_PER_MINUTE).toBeGreaterThan(WRITES_PER_MINUTE);
  });
});
