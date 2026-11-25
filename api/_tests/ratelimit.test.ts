import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { consumeRate, forgetUserRates, sweepRates, userRateKey } from '../_lib/ratelimit.js';
import { Browser, PASSWORD, startRuntime } from './harness.js';
import type { TestRuntime } from './harness.js';

let t: TestRuntime;

beforeAll(async () => {
  t = await startRuntime({ rateLimit: true });
});

afterAll(() => t.close());

describe('consumeRate', () => {
  const T0 = 1_800_000_000_000;

  it('allows up to the limit in a window, then refuses until the window ends', async () => {
    const key = userRateKey('test:a', 'user1');
    for (let i = 1; i <= 5; i++) {
      expect(await consumeRate(t.runtime.db, key, 5, 60, T0 + i * 1000)).toMatchObject({ allowed: true, remaining: 5 - i });
    }
    const refused = await consumeRate(t.runtime.db, key, 5, 60, T0 + 20_000);
    expect(refused).toEqual({ allowed: false, remaining: 0, retryAfter: 40 });
    expect((await consumeRate(t.runtime.db, key, 5, 60, T0 + 59_999)).retryAfter).toBe(1);
    expect(await consumeRate(t.runtime.db, key, 5, 60, T0 + 60_000)).toMatchObject({ allowed: true, remaining: 4 });
  });

  it('counts keys independently', async () => {
    expect((await consumeRate(t.runtime.db, 'test:b:u1', 1, 60, T0)).allowed).toBe(true);
    expect((await consumeRate(t.runtime.db, 'test:b:u1', 1, 60, T0)).allowed).toBe(false);
    expect((await consumeRate(t.runtime.db, 'test:b:u2', 1, 60, T0)).allowed).toBe(true);
  });

  it('cannot be raced past: of many simultaneous requests exactly the limit get through', async () => {
    const results = await Promise.all(Array.from({ length: 25 }, () => consumeRate(t.runtime.db, 'test:c:u1', 10, 60, T0)));
    expect(results.filter((r) => r.allowed)).toHaveLength(10);
  });

  it('keeps one row per key, sweeps stale ones and forgets a user', async () => {
    await consumeRate(t.runtime.db, 'test:d:old', 5, 60, T0 - 3 * 86_400_000);
    await consumeRate(t.runtime.db, 'test:d:new', 5, 60, T0);
    await consumeRate(t.runtime.db, 'test:d:new', 5, 60, T0 + 1);
    const rows = async (like: string) => (await t.runtime.db.query<{ key: string }>('select key from firesight_rate where key like $1 order by key', [like])).rows.map((r) => r.key);
    expect(await rows('test:d:%')).toEqual(['test:d:new', 'test:d:old']);
    expect(await sweepRates(t.runtime.db, T0)).toBeGreaterThanOrEqual(1);
    expect(await rows('test:d:%')).toEqual(['test:d:new']);

    await consumeRate(t.runtime.db, userRateKey('test:e', 'abc'), 5, 60, T0);
    await consumeRate(t.runtime.db, userRateKey('test:e:write', 'abc'), 5, 60, T0);
    await consumeRate(t.runtime.db, userRateKey('test:e', 'xabc'), 5, 60, T0);
    await forgetUserRates(t.runtime.db, 'abc');
    expect(await rows('test:e%')).toEqual(['test:e:xabc']);
  });
});

describe('the auth endpoints, with rate limiting on', () => {
  it('slows password guessing from one address, counted in the database', async () => {
    const victim = new Browser(t.state);
    await victim.signUp('ratelimited@example.com');
    const attacker = new Browser(t.state);
    const statuses: number[] = [];
    for (let i = 0; i < 12; i++) statuses.push((await attacker.post('/api/auth/sign-in/email', { email: 'ratelimited@example.com', password: `guess number ${i} !!` })).status);
    expect(statuses.slice(0, 10)).toEqual(Array.from({ length: 10 }, () => 401));
    expect(statuses.slice(10)).toEqual([429, 429]);
    const limited = await attacker.post('/api/auth/sign-in/email', { email: 'ratelimited@example.com', password: PASSWORD });
    expect(limited.status).toBe(429);
    expect(Number(limited.headers.get('retry-after') ?? limited.headers.get('x-retry-after'))).toBeGreaterThan(0);

    // The counter is a row, so another function instance sees it too.
    const counters = await t.runtime.db.query<{ key: string; count: number }>('select key, count from "rateLimit"');
    expect(counters.rows.some((r) => r.key.includes(attacker.address) && r.count >= 10)).toBe(true);

    // Someone at another address is not locked out.
    expect((await new Browser(t.state).post('/api/auth/sign-in/email', { email: 'ratelimited@example.com', password: PASSWORD })).status).toBe(200);
  });
});
