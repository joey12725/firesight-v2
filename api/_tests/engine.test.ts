// The engine as the functions run it: the same wasm module as the website, on one thread.

import { describe, expect, it } from 'vitest';
import * as wasm from '../_engine/firesight_wasm.js';
import { CHUNK_TARGET_MS, DEFAULT_PATHS, EngineError, MAX_CHUNK, MAX_PATHS, MIN_PATHS, applyPatch, clampPaths, compare, currentYearMonth, digest, engineFailure, engineInput, forgetRuns, headline, nextChunk, normalise, planPreview, schema, simulate, toEngineError, validate } from '../_lib/engine.js';
import type { Profile, WasmGlue } from '../_lib/engine.js';
import { sampleProfile } from './mcp-harness.js';

const profile = (): Profile => normalise(sampleProfile()).profile;

/** The sample household with `copies` more accounts, incomes and spending items: a plan whose every life is costly. */
function largeProfile(copies: number): Profile {
  const base = sampleProfile();
  const many = <T extends { id: string }>(items: T[]): T[] => [...items, ...Array.from({ length: copies }, (_, i) => ({ ...(items[0] as T), id: `${(items[0] as T).id}_${i}` }))];
  const result = normalise({ ...base, incomes: many(base.incomes), accounts: many(base.accounts), spending: { items: many(base.spending.items) } });
  expect(result.errors).toEqual([]);
  return result.profile;
}

// Checked by the compiler, not at run time: the generated module has everything the
// engine wrapper says it uses, with those types. If the wasm crate's interface changes,
// `npm run typecheck` fails here.
const typed: WasmGlue = wasm;

describe('loading', () => {
  it('loads the wasm module at import, from api/_engine', () => {
    expect(engineFailure()).toBeNull();
    expect(typeof typed.initSync).toBe('function');
    for (const name of ['validate', 'applyPatch', 'schema', 'planPreview', 'digest', 'summarize', 'compare', 'Engine', 'Merger'] as const) {
      expect(typeof typed[name], name).toBe('function');
    }
    expect((schema() as { $defs: Record<string, unknown> }).$defs).toHaveProperty('Person');
  });
});

describe('profiles', () => {
  it('fills every default of a partial profile and reports what is wrong with an empty one', () => {
    const filled = normalise(sampleProfile());
    expect(filled.errors).toEqual([]);
    expect(filled.profile).toHaveProperty('retirement.trigger');
    expect(filled.profile).toHaveProperty('schema_version', 2);

    const empty = normalise({});
    expect(empty.errors.map((e) => e.code)).toEqual(['people_count']);
    expect(validate({}).errors[0]).toMatchObject({ code: 'people_count', path: '/household/people' });
  });

  it('never throws from validate, and names the field a profile gets wrong', () => {
    const report = validate({ bogus: 1 });
    expect(report.errors[0]?.code).toBe('schema');
    expect(report.errors[0]?.message).toContain('unknown field `bogus`');
    expect(() => normalise({ bogus: 1 })).toThrow(/unknown field `bogus`/);
    expect(() => normalise('not a profile')).toThrow(EngineError);
  });

  it('applies a patch all or nothing, with an error a model can act on', () => {
    const base = profile();
    const patched = applyPatch(base, [{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 150_000 }]);
    expect(patched.profile).toHaveProperty('incomes.0.kind.salary', 150_000);
    expect(base).toHaveProperty('incomes.0.kind.salary', 128_000);

    let message = '';
    try {
      applyPatch(base, [
        { op: 'set', path: '/incomes/@lena_job/kind/salary', value: 1 },
        { op: 'set', path: '/incomes/@nobody/kind/salary', value: 2 },
      ]);
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toContain('patch operation 1');
    expect(message).toContain('no element with id "nobody"');
    expect(message).toContain('lena_job');
  });

  it('previews a valid plan and refuses an invalid one with its report', () => {
    const preview = planPreview(profile());
    expect(preview.annual_income).toBe(128_000);
    expect(preview.annual_spending).toBeGreaterThan(50_000);
    let thrown: unknown;
    try {
      planPreview(normalise({}).profile);
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(EngineError);
    expect((thrown as EngineError).kind).toBe('invalid_profile');
    expect((thrown as EngineError).report?.errors[0]?.code).toBe('people_count');
  });

  it('classifies what the engine throws', () => {
    expect(toEngineError(new Error('{"errors":[{"code":"x","path":"/a","message":"Bad."}],"warnings":[]}'))).toMatchObject({ kind: 'invalid_profile', message: 'Bad.' });
    expect(toEngineError(new Error('{not json'))).toMatchObject({ kind: 'engine' });
    expect(toEngineError('plain')).toMatchObject({ kind: 'engine', message: 'plain' });
  });

  it('writes this month the way the engine does', () => {
    expect(currentYearMonth(new Date(Date.UTC(2026, 0, 31, 23, 59)))).toBe('2026-01');
    expect(currentYearMonth(new Date(Date.UTC(2026, 11, 1)))).toBe('2026-12');
  });
});

describe('simulation', () => {
  it('keeps the number of lives within bounds', () => {
    expect(clampPaths(undefined)).toBe(DEFAULT_PATHS);
    expect(clampPaths(undefined, 400)).toBe(400);
    expect(clampPaths(undefined, 10_000)).toBe(DEFAULT_PATHS);
    expect(clampPaths(1_000_000)).toBe(MAX_PATHS);
    expect(clampPaths(1)).toBe(MIN_PATHS);
    expect(clampPaths(333.4)).toBe(333);
  });

  it('compiles the profile with the plan seed, or the one it is given', () => {
    const base = profile();
    expect(engineInput(base).seed).toBe(11);
    const other = engineInput(base, 99);
    expect(other.seed).toBe(99);
    expect(JSON.parse(other.json).simulation).toMatchObject({ seed: 99, paths: 10_000 });
    // The path count asked for does not change what is compiled.
    expect(engineInput({ ...base, simulation: { ...(base.simulation as object), paths: 5 } }).json).toBe(engineInput(base).json);
  });

  it('gives, in chunks, exactly what the engine gives in one piece', async () => {
    forgetRuns();
    const base = profile();
    const run = await simulate(base, { paths: 450 });
    expect(run).toMatchObject({ paths: 450, seed: 11 });

    const whole = new wasm.Engine(run.json);
    const acc = whole.runChunk(0, 450);
    whole.free();
    expect(Buffer.from(run.acc).equals(Buffer.from(acc))).toBe(true);
    expect(digest(run)).toEqual(JSON.parse(wasm.digest(acc, run.json)));

    const figures = digest(run) as { paths: number; success: { p: number; lo: number; hi: number } };
    expect(figures.paths).toBe(450);
    expect(figures.success.lo).toBeLessThanOrEqual(figures.success.p);
    expect(figures.success.hi).toBeGreaterThanOrEqual(figures.success.p);
    expect(headline(run).home_sale_rate.value).toBeGreaterThanOrEqual(0);
  });

  it('sizes each stretch of work by the time the last one took', () => {
    // A stretch that hit the target keeps its size; a slow one shrinks; a fast one grows, but no more than fourfold at a time.
    expect(nextChunk(40, CHUNK_TARGET_MS)).toBe(40);
    expect(nextChunk(200, CHUNK_TARGET_MS * 10)).toBe(20);
    expect(nextChunk(8, CHUNK_TARGET_MS / 2)).toBe(16);
    expect(nextChunk(8, 0)).toBe(32);
    expect(nextChunk(100, 0.001)).toBe(MAX_CHUNK);
    expect(nextChunk(1, 60_000)).toBe(1);
    expect(nextChunk(3, Number.NaN)).toBe(3);
  });

  it('gives the same bytes however the lives are cut into stretches', async () => {
    const base = profile();
    const { json } = engineInput(base, 21);
    const pieces = (size: number): Buffer => {
      const engine = new wasm.Engine(json);
      const merger = new wasm.Merger();
      for (let first = 0; first < 1_000; first += size) merger.add(engine.runChunk(first, Math.min(size, 1_000 - first)));
      const bytes = Buffer.from(merger.bytes());
      engine.free();
      merger.free();
      return bytes;
    };
    const fixed = pieces(200);
    expect(pieces(7).equals(fixed)).toBe(true);
    forgetRuns();
    // Whatever sizes the clock leads to here, the result is that of 200 lives at a time.
    const adaptive = await simulate(base, { paths: 1_000, seed: 21 });
    expect(Buffer.from(adaptive.acc).equals(fixed)).toBe(true);
  });

  it('keeps a costly plan from holding the thread: a cancelled run stops within a fraction of a 200-life stretch', async () => {
    const big = largeProfile(200);
    expect(JSON.stringify(big).length).toBeGreaterThan(100_000);
    const { json } = engineInput(big, 5);
    // What 200 lives of this plan cost in one piece, on this machine, now.
    const whole = new wasm.Engine(json);
    const began = performance.now();
    whole.runChunk(0, 200);
    const stretch = performance.now() - began;
    whole.free();

    forgetRuns();
    const cancel = new AbortController();
    const pending = simulate(big, { paths: 2_000, seed: 5, signal: cancel.signal });
    // `simulate` compiles the plan and runs its first stretch before it first yields.
    // From here on, a timer can only fire in a gap between stretches, so how late one
    // fires is how long the thread was held.
    const DELAY = 5;
    const asked = performance.now();
    let last = asked;
    let longestGap = 0;
    const watch = setInterval(() => {
      const at = performance.now();
      longestGap = Math.max(longestGap, at - last);
      last = at;
    }, 1);
    const trigger = setTimeout(() => cancel.abort(), DELAY);
    let stopped: number;
    try {
      await expect(pending).rejects.toMatchObject({ kind: 'aborted' });
      stopped = performance.now();
    } finally {
      clearInterval(watch);
      clearTimeout(trigger);
    }
    // With 200 lives to a stretch, the cancellation waited for a whole one.
    expect(stopped - (asked + DELAY)).toBeLessThan(stretch / 3);
    expect(longestGap).toBeLessThan(stretch / 3);
  }, 120_000);

  it('is deterministic: the same plan and seed are the same lives', async () => {
    const base = profile();
    forgetRuns();
    const first = await simulate(base, { paths: 200 });
    forgetRuns();
    const second = await simulate(base, { paths: 200 });
    expect(second).not.toBe(first);
    expect(Buffer.from(second.acc).equals(Buffer.from(first.acc))).toBe(true);
    const reseeded = await simulate(base, { paths: 200, seed: 12 });
    expect(Buffer.from(reseeded.acc).equals(Buffer.from(first.acc))).toBe(false);
  });

  it('serves an identical run from memory', async () => {
    const base = profile();
    const first = await simulate(base, { paths: 200 });
    expect(await simulate(base, { paths: 200 })).toBe(first);
    expect(await simulate(base, { paths: 201 })).not.toBe(first);
  });

  it('compares two runs life by life', async () => {
    const base = profile();
    const later = applyPatch(base, [{ op: 'set', path: '/spending/items/@living/amount', value: 6_000 }]).profile;
    const a = await simulate(base, { paths: 200 });
    const b = await simulate(later, { paths: 200 });
    const comparison = compare(a, b);
    expect(comparison.paths).toBe(200);
    const success = comparison.outcomes.find((o) => o.outcome === 'success');
    if (!success) throw new Error('The comparison has no success outcome.');
    // Every life is in exactly one of the four cells.
    expect(success.both + success.a_only + success.b_only + success.neither).toBe(200);
    expect(success.difference.value).toBeCloseTo(success.b.value - success.a.value, 10);
    // A run against itself changes no life.
    const same = compare(a, a).outcomes.find((o) => o.outcome === 'success');
    expect(same).toMatchObject({ a_only: 0, b_only: 0, within_noise: true });
    expect(same?.difference.value).toBe(0);
    expect(comparison.scalars.map((s) => s.scalar)).toContain('terminal_net_worth');
  });

  it('stops when cancelled, when out of time, and when the plan cannot run', async () => {
    forgetRuns();
    const base = profile();
    const cancelled = new AbortController();
    cancelled.abort();
    await expect(simulate(base, { paths: 200, signal: cancelled.signal })).rejects.toMatchObject({ kind: 'aborted' });

    // Cancelled part way: the run stops at the next gap between chunks.
    const midway = new AbortController();
    const pending = simulate(base, { paths: 5_000, seed: 3, signal: midway.signal });
    setImmediate(() => midway.abort());
    await expect(pending).rejects.toMatchObject({ kind: 'aborted' });

    let clock = 0;
    await expect(simulate(base, { paths: 1_000, seed: 4, budgetMs: 10, now: () => (clock += 6) })).rejects.toMatchObject({ kind: 'timeout', message: expect.stringContaining('of 1000 lives') });

    await expect(simulate(normalise({}).profile, { paths: 200 })).rejects.toMatchObject({ kind: 'invalid_profile' });
    // Nothing half-done was kept.
    const clean = await simulate(base, { paths: 200, seed: 3 });
    expect(clean.paths).toBe(200);
  });
});
