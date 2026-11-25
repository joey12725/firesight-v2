// A fixed-window request counter in Postgres, shared by every function instance (the
// in-memory limiter in api/chat.ts is per instance and forgets on a cold start).
//
// One row per key. The window a request falls in is derived from the clock, so the
// check and the increment are a single atomic statement: concurrent requests cannot
// all read a stale count and slip through.

import type { Queryable } from './db.js';

export interface RateDecision {
  allowed: boolean;
  /** Requests left in this window after this one. */
  remaining: number;
  /** Seconds until the window ends. */
  retryAfter: number;
}

/**
 * The counter key for something a user does, e.g. `userRateKey('plans:w', id)`. Keys
 * that count a user's requests must be built with this: account deletion removes every
 * counter whose key ends in `:<userId>`, so that no trace of the id is left behind.
 */
export const userRateKey = (namespace: string, userId: string): string => `${namespace}:${userId}`;

/** Removes a deleted user's counters. */
export async function forgetUserRates(db: Queryable, userId: string): Promise<void> {
  await db.query(`delete from firesight_rate where right(key, length($1) + 1) = ':' || $1`, [userId]);
}

/**
 * Counts one request against `key` and says whether it is within `max` per
 * `windowSeconds`. Use `userRateKey` for per-user limits.
 */
export async function consumeRate(db: Queryable, key: string, max: number, windowSeconds: number, now: number = Date.now()): Promise<RateDecision> {
  const windowMs = windowSeconds * 1000;
  const start = Math.floor(now / windowMs) * windowMs;
  const result = await db.query<{ count: number }>(
    `insert into firesight_rate as r (key, window_start, count) values ($1, $2, 1)
     on conflict (key) do update set
       count = case when r.window_start = excluded.window_start then r.count + 1 else 1 end,
       window_start = excluded.window_start
     returning count`,
    [key, start],
  );
  const count = result.rows[0]?.count ?? 1;
  return { allowed: count <= max, remaining: Math.max(0, max - count), retryAfter: Math.max(1, Math.ceil((start + windowMs - now) / 1000)) };
}

/** Removes counters whose window ended more than a day ago. Cheap; call it now and then. */
export async function sweepRates(db: Queryable, now: number = Date.now()): Promise<number> {
  const result = await db.query('delete from firesight_rate where window_start < $1', [now - 86_400_000]);
  return result.rowCount;
}
