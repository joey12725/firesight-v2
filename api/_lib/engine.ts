// The simulation engine inside a function: the same WebAssembly module the website
// runs in the browser, loaded once per function instance.
//
// The module and its JavaScript glue are a copy of web/src/engine/pkg/ placed in
// api/_engine/ by `npm run engine` (scripts/copy-engine.ts), so the engine a function
// was built with sits beside it. A run here and a run in the browser with the same
// profile, seed and number of lives are the same lives and give the same figures: both
// hand the engine the JSON that `engineInput` builds, and the engine is deterministic
// per life.
//
// The browser spreads a run over worker threads. A function has one thread, so a run is
// cut into short stretches with a pause between them: other requests are served in the
// gaps, and a cancelled or overlong run stops at the next gap. A stretch is sized by
// the time the last one took, not by a number of lives, because a life of a large plan
// costs many times a life of a small one.

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
// The glue is generated (it is not in git), and the platform's type check of a function
// looks only at the files that were there when the build began, so there it cannot see
// this module's declarations. The import is therefore left unchecked and given the type
// below, which a test holds the real module to (api/_tests/engine.test.ts).
// eslint-disable-next-line @typescript-eslint/ban-ts-comment -- explained above
// @ts-ignore
import * as glue from '../_engine/firesight_wasm.js';

declare const process: { cwd(): string };
declare const performance: { now(): number };

interface WasmEngine {
  /** Simulates lives `first .. first + count` and returns the chunk's serialised accumulator. */
  runChunk(first: number, count: number): Uint8Array;
  free(): void;
}

interface WasmMerger {
  /** Merges one chunk. Chunks may arrive in any order. */
  add(bytes: Uint8Array): void;
  /** The merged accumulator, serialised. */
  bytes(): Uint8Array;
  free(): void;
}

/** The part of the wasm package (crates/firesight-wasm) that the functions use. Every string is JSON. */
export interface WasmGlue {
  initSync(module: { module: Uint8Array }): unknown;
  validate(profile: string): string;
  applyPatch(profile: string, patch: string): string;
  schema(): string;
  planPreview(profile: string): string;
  digest(acc: Uint8Array, profile: string): string;
  summarize(acc: Uint8Array, profile: string): string;
  compare(a: Uint8Array, b: Uint8Array, baselineProfile: string): string;
  /** Compiles a profile. Throws the validation report as JSON when it cannot run. */
  Engine: new (profile: string) => WasmEngine;
  Merger: new () => WasmMerger;
}

const wasm = glue as unknown as WasmGlue;

export interface Issue {
  code: string;
  path: string;
  message: string;
}

export interface Report {
  errors: Issue[];
  warnings: Issue[];
}

/** A plan's profile. The engine owns its shape (see `schema()`); here it is opaque JSON. */
export type Profile = Record<string, unknown>;

export interface PatchOp {
  op: 'set' | 'remove' | 'append';
  path: string;
  value?: unknown;
}

export interface PatchResult extends Report {
  /** The patched profile with every default filled in. */
  profile: Profile;
}

export type EngineErrorKind =
  /** The profile has validation errors; `report` carries them. */
  | 'invalid_profile'
  /** The run was cancelled by the caller. */
  | 'aborted'
  /** The run took longer than it was allowed. */
  | 'timeout'
  /** The engine's files are missing from this deployment. */
  | 'unavailable'
  /** Anything else the engine threw (a patch that does not apply, JSON it cannot read). */
  | 'engine';

export class EngineError extends Error {
  constructor(
    readonly kind: EngineErrorKind,
    message: string,
    readonly report?: Report,
  ) {
    super(message);
    this.name = 'EngineError';
  }
}

/** Classifies what the wasm module threw. A validation failure arrives as the report's JSON. */
export function toEngineError(error: unknown): EngineError {
  if (error instanceof EngineError) return error;
  const message = error instanceof Error ? error.message : String(error);
  if (message.startsWith('{')) {
    try {
      const report = JSON.parse(message) as Partial<Report>;
      if (Array.isArray(report.errors)) {
        return new EngineError('invalid_profile', report.errors[0]?.message ?? 'The profile has errors.', { errors: report.errors, warnings: Array.isArray(report.warnings) ? report.warnings : [] });
      }
    } catch {
      // Not a report after all.
    }
  }
  return new EngineError('engine', message);
}

// ---- loading ------------------------------------------------------------------------

const WASM_FILE = 'firesight_wasm_bg.wasm';

function readWasm(): Uint8Array {
  // Beside this module is where the file is locally and where the bundler puts it. The
  // second place covers a bundler that moves the code but keeps `includeFiles` at
  // their project paths.
  const candidates: (URL | string)[] = [new URL(`../_engine/${WASM_FILE}`, import.meta.url), join(process.cwd(), 'api', '_engine', WASM_FILE)];
  for (const candidate of candidates) {
    try {
      return readFileSync(candidate);
    } catch {
      // Try the next place.
    }
  }
  throw new EngineError('unavailable', 'The simulation engine is missing from this deployment (api/_engine was not built; see `npm run engine`).');
}

let failure: EngineError | null = null;
try {
  // At module scope, so a warm instance never pays for it again. About five milliseconds.
  wasm.initSync({ module: readWasm() });
} catch (error) {
  failure = error instanceof EngineError ? error : new EngineError('unavailable', `The simulation engine could not start: ${error instanceof Error ? error.message : String(error)}`);
}

/** Null when the engine is loaded; otherwise why it is not. */
export const engineFailure = (): EngineError | null => failure;

function call<T>(fn: () => string): T {
  if (failure) throw failure;
  try {
    return JSON.parse(fn()) as T;
  } catch (error) {
    throw toEngineError(error);
  }
}

// ---- deterministic calls --------------------------------------------------------------

/** Errors and warnings for a profile. Never throws for bad input: that is a `schema` error. */
export function validate(profile: unknown): Report {
  return call(() => wasm.validate(JSON.stringify(profile)));
}

/**
 * Applies patch operations, all or nothing, and returns the complete patched profile
 * with its report. Throws an `EngineError` whose message names the failing operation
 * (written for a language model to read and retry) when the patch does not apply or the
 * result does not match the schema.
 */
export function applyPatch(profile: unknown, ops: readonly PatchOp[]): PatchResult {
  return call(() => wasm.applyPatch(JSON.stringify(profile), JSON.stringify(ops)));
}

/** Any profile-shaped JSON, full or partial, with every default filled in. */
export function normalise(profile: unknown): PatchResult {
  return applyPatch(profile, []);
}

let cachedSchema: unknown;

/** The JSON Schema of the profile format (about 90 KB; parsed once). */
export function schema(): unknown {
  return (cachedSchema ??= call(() => wasm.schema()));
}

/** What `planPreview` returns: what the plan implies before any simulation, in today's dollars. */
export interface PlanPreview {
  annual_spending: number;
  annual_spending_retired: number;
  annual_income: number;
  annual_tax: number;
  savings_rate: number;
  investable: number;
  wealth: number;
  funded_requirement: number;
  funded_ratio: number | null;
  retirement_need: number;
  retirement_tax: number;
  future_income_value: number;
  horizon_years: number;
  life_expectancy: number[];
  expected_age_at_death: number[];
}

/** Throws `invalid_profile` when the plan cannot be compiled. */
export function planPreview(profile: Profile): PlanPreview {
  return call(() => wasm.planPreview(JSON.stringify(profile)));
}

/** This month as the engine writes months, in UTC. */
export function currentYearMonth(now: Date = new Date()): string {
  return `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, '0')}`;
}

// ---- simulation -----------------------------------------------------------------------

/** Lives per run when the caller does not say. */
export const DEFAULT_PATHS = 2_000;
export const MAX_PATHS = 5_000;
export const MIN_PATHS = 200;
/** A run that has not finished by then is stopped: about ten times what the largest run takes. */
export const RUN_BUDGET_MS = 60_000;

/** How long one uninterrupted stretch of work should take, in milliseconds. */
export const CHUNK_TARGET_MS = 30;
/** Lives in a run's first stretch, before anything is known about what a life of this plan costs. */
export const FIRST_CHUNK = 4;
/** The most lives in one stretch, however cheap they are. */
export const MAX_CHUNK = 200;

/**
 * How many lives the next stretch should hold, given that the last one ran `lives` in
 * `elapsedMs`: as many as fit the target, at most four times as many as last time (one
 * fast measurement is not trusted far), and between 1 and `MAX_CHUNK`. The sizes do
 * not affect the result: merging is exact, whatever ranges the lives arrive in.
 */
export function nextChunk(lives: number, elapsedMs: number): number {
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) return lives;
  const fitting = elapsedMs > 0 ? Math.round((lives * CHUNK_TARGET_MS) / elapsedMs) : lives * 4;
  return Math.max(1, Math.min(MAX_CHUNK, lives * 4, fitting));
}
/** The website compiles every profile with this path count, which does not affect a life (web/src/engine/pool.ts). */
const CANONICAL_PATHS = 10_000;

export function clampPaths(requested: number | undefined, fallback: number = DEFAULT_PATHS): number {
  const wanted = requested ?? Math.min(fallback, DEFAULT_PATHS);
  return Math.min(MAX_PATHS, Math.max(MIN_PATHS, Math.round(wanted)));
}

/** The JSON the engine compiles, exactly as the website builds it, and the seed it carries. */
export function engineInput(profile: Profile, seed?: number): { json: string; seed: number } {
  const simulation = (typeof profile.simulation === 'object' && profile.simulation !== null ? profile.simulation : {}) as Record<string, unknown>;
  const resolved = seed ?? (typeof simulation.seed === 'number' ? simulation.seed : 1);
  return { json: JSON.stringify({ ...profile, simulation: { ...simulation, seed: resolved, paths: CANONICAL_PATHS } }), seed: resolved };
}

/** A finished run. `acc` is the engine's serialised accumulator: every figure is derived from it. */
export interface Run {
  /** The profile as compiled (`engineInput`). */
  json: string;
  seed: number;
  paths: number;
  acc: Uint8Array;
}

export interface RunOptions {
  paths: number;
  seed?: number;
  signal?: AbortSignal;
  /** Milliseconds this run may take. */
  budgetMs?: number;
  /** A clock for tests. */
  now?: () => number;
}

// Finished runs of this instance, newest last. A what-if reuses the baseline that
// `run_simulation` just produced. The key is a digest of everything that determines the
// result, so a hit is by definition the same answer, whoever asks.
const MAX_CACHED_RUNS = 8;
const finished = new Map<string, Run>();

const pause = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

/**
 * Simulates lives 0 to `paths - 1` of a profile. Rejects with an `EngineError`:
 * `invalid_profile` when the plan cannot be compiled, `aborted` when the signal fires,
 * `timeout` when the budget runs out.
 */
export async function simulate(profile: Profile, options: RunOptions): Promise<Run> {
  if (failure) throw failure;
  const { json, seed } = engineInput(profile, options.seed);
  const paths = options.paths;
  const key = `${createHash('sha256').update(json).digest('base64url')}|${paths}`;
  const hit = finished.get(key);
  if (hit) {
    // Most recently used last.
    finished.delete(key);
    finished.set(key, hit);
    return hit;
  }

  const now = options.now ?? Date.now;
  const deadline = now() + (options.budgetMs ?? RUN_BUDGET_MS);
  let engine: WasmEngine | undefined;
  let merger: WasmMerger | undefined;
  try {
    engine = new wasm.Engine(json);
    merger = new wasm.Merger();
    let size = FIRST_CHUNK;
    for (let first = 0; first < paths; ) {
      if (options.signal?.aborted) throw new EngineError('aborted', 'The run was cancelled.');
      if (now() > deadline) throw new EngineError('timeout', `The run did not finish in time (${first} of ${paths} lives). Ask for fewer lives.`);
      const count = Math.min(size, paths - first);
      // Timed with the real clock, whatever clock the deadline uses.
      const began = performance.now();
      merger.add(engine.runChunk(first, count));
      size = nextChunk(count, performance.now() - began);
      first += count;
      await pause();
    }
    const run: Run = { json, seed, paths, acc: merger.bytes() };
    finished.set(key, run);
    while (finished.size > MAX_CACHED_RUNS) finished.delete(finished.keys().next().value as string);
    return run;
  } catch (error) {
    throw toEngineError(error);
  } finally {
    engine?.free();
    merger?.free();
  }
}

/** Forgets every cached run. For tests. */
export function forgetRuns(): void {
  finished.clear();
}

/** The engine's own small form of a run (about 2 KB): rates with intervals, spreads, causes, notes on units. */
export function digest(run: Run): Record<string, unknown> {
  return call(() => wasm.digest(run.acc, run.json));
}

interface Estimate {
  value: number;
  lo: number;
  hi: number;
}

/** The headline figures of the engine's full summary that the digest leaves out. */
export interface Headline {
  home_sale_rate: Estimate;
  plan_delivered_share: { value: number | null };
  initial_withdrawal_rate: { p50: number | null };
}

/** From the full summary (hundreds of kilobytes), which never leaves this function. */
export function headline(run: Run): Headline {
  return call<{ headline: Headline }>(() => wasm.summarize(run.acc, run.json)).headline;
}

export interface NullableEstimate {
  value: number | null;
  std_error: number | null;
  lo: number | null;
  hi: number | null;
}

export interface OutcomeComparison {
  outcome: string;
  a: { value: number };
  b: { value: number };
  both: number;
  a_only: number;
  b_only: number;
  neither: number;
  difference: NullableEstimate;
  z: number | null;
  within_noise: boolean;
}

export interface ScalarComparison {
  scalar: string;
  unit: string;
  both: number;
  a_only: number;
  b_only: number;
  mean_delta: NullableEstimate;
  quantiles: (number | null)[];
  b_higher: number | null;
  a_higher: number | null;
  within_noise: boolean;
}

/** The engine's paired comparison of two runs on the same lives: B minus A. */
export interface Comparison {
  paths: number;
  outcomes: OutcomeComparison[];
  scalars: ScalarComparison[];
  quantile_levels: number[];
  headline: { metric: string; a: number | null; b: number | null; delta: number | null }[];
}

/** `a` is the baseline. Both runs must use the same seed and number of lives. */
export function compare(a: Run, b: Run): Comparison {
  return call(() => wasm.compare(a.acc, b.acc, a.json));
}
