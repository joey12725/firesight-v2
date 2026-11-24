// Typed wrappers for the wasm calls that run on the main thread. Call `initEngine()`
// (and await it) once before anything else here.
import init, * as wasm from './pkg/firesight_wasm.js';
import { completeProfile } from './complete';
import { toEngineError } from './errors';
import type { Comparison, PlanPreview, Summary, SummaryDigest } from './output-types';
import type { Aggregator } from './pool';
import type { ExampleProfile, Lever, PatchOp, Profile, Report } from './types';

let loading: Promise<void> | null = null;
let compiled: Promise<WebAssembly.Module> | null = null;

/**
 * Compiles the wasm module once, streaming, for the main thread and every worker
 * (a `WebAssembly.Module` can be posted to a worker, which then only instantiates it).
 */
export function compileEngine(url: string | URL): Promise<WebAssembly.Module> {
  compiled ??= (async () => {
    try {
      return await WebAssembly.compileStreaming(fetch(url));
    } catch {
      // A server that does not send `application/wasm` cannot stream.
      return WebAssembly.compile(await (await fetch(url)).arrayBuffer());
    }
  })();
  return compiled;
}

/** Instantiates the wasm module on this thread. Safe to call more than once. */
export function initEngine(module?: WebAssembly.Module | Promise<WebAssembly.Module>): Promise<void> {
  loading ??= (module ? Promise.resolve(module).then((m) => init({ module_or_path: m })) : init()).then(() => undefined);
  return loading;
}

/** For Node (tests): instantiate from bytes read off disk. */
export function initEngineSync(bytes: BufferSource) {
  wasm.initSync({ module: bytes });
  loading = Promise.resolve();
}

function text(profile: Profile | string): string {
  return typeof profile === 'string' ? profile : JSON.stringify(profile);
}

function call<T>(fn: () => string): T {
  try {
    return JSON.parse(fn()) as T;
  } catch (error) {
    throw toEngineError(error);
  }
}

/** Errors and warnings for a profile. Never throws: unparseable input is a `schema` error. */
export function validate(profile: Profile | string): Report {
  return call(() => wasm.validate(text(profile)));
}

export interface PatchResult extends Report {
  profile: Profile;
}

/**
 * Applies patch operations and returns the complete patched profile with its report.
 * Throws an `EngineError` whose message names the failing operation when the patch
 * cannot be applied or the result does not match the schema.
 */
export function applyPatch(profile: Profile | string, ops: PatchOp[]): PatchResult {
  const result = call<PatchResult>(() => wasm.applyPatch(text(profile), JSON.stringify(ops)));
  complete(result.profile);
  return result;
}

/** Normalises any profile-shaped JSON through the engine (fills every default). */
export function normalize(profile: unknown): PatchResult {
  return applyPatch(JSON.stringify(profile), []);
}

export interface MigrationResult {
  profile: Profile;
  /** The assumptions the migration had to make, in plain language. */
  notes: string[];
}

export function migrateV1(v1Json: string): MigrationResult {
  const result = call<MigrationResult>(() => wasm.migrateV1(v1Json));
  complete(result.profile);
  return result;
}

export function levers(profile: Profile | string): Lever[] {
  return call(() => wasm.levers(text(profile)));
}

export function schema(): unknown {
  return call(() => wasm.schema());
}

let cachedSchema: unknown;

/**
 * The engine omits the optional realism fields while they hold their defaults; the app
 * reads and patches the complete form, so every profile the engine hands back is
 * completed here (see `complete.ts`).
 */
function complete<T>(profile: T): T {
  return completeProfile((cachedSchema ??= schema()), profile);
}

export function defaultProfile(): Profile {
  return complete(call<Profile>(() => wasm.defaultProfile()));
}

export function exampleProfiles(): ExampleProfile[] {
  const examples = call<ExampleProfile[]>(() => wasm.exampleProfiles());
  for (const example of examples) complete(example.profile);
  return examples;
}

/**
 * The deterministic facts of a plan (spending now and in retirement, the funded number,
 * savings rate): about 1 ms, so it can run on every edit. Throws an `EngineError` of kind
 * `invalid_profile` when the plan cannot be compiled.
 */
export function planPreview(profile: Profile | string): PlanPreview {
  return call(() => wasm.planPreview(text(profile)));
}

/** The small form of a run for an LLM tool result, from its serialised accumulator. */
export function digest(acc: Uint8Array, profile: Profile | string): SummaryDigest {
  return call(() => wasm.digest(acc, text(profile)));
}

/** Paired comparison on the calling thread (decodes both accumulators: prefer `pool.comparison`). */
export function compare(a: Uint8Array, b: Uint8Array, baseline: Profile | string): Comparison {
  return call(() => wasm.compare(a, b, text(baseline)));
}

/**
 * An `Aggregator` that merges and summarises on the calling thread, with the same wasm
 * `Merger` the coordinator worker uses. For Node (tests, benchmarks) and as the
 * fallback where workers are unavailable.
 */
export function createLocalAggregator(): Aggregator {
  const jobs = new Map<number, { merger: wasm.Merger; json: string }>();
  return {
    open(job, profileJson) {
      jobs.set(job, { merger: new wasm.Merger(), json: profileJson });
    },
    add(job, bytes) {
      jobs.get(job)?.merger.add(bytes);
    },
    summarize(job, withBytes) {
      try {
        const entry = jobs.get(job);
        if (!entry) throw new Error('The run was closed.');
        const t0 = performance.now();
        const summary = JSON.parse(entry.merger.summarize(entry.json)) as Summary;
        const acc = withBytes ? entry.merger.bytes() : null;
        return Promise.resolve({ summary, paths: entry.merger.paths(), ms: performance.now() - t0, acc });
      } catch (error) {
        return Promise.reject(toEngineError(error));
      }
    },
    close(job) {
      jobs.get(job)?.merger.free();
      jobs.delete(job);
    },
    compare(a, b, baselineProfileJson) {
      try {
        return Promise.resolve(compare(a, b, baselineProfileJson));
      } catch (error) {
        return Promise.reject(toEngineError(error));
      }
    },
    dispose() {
      for (const { merger } of jobs.values()) merger.free();
      jobs.clear();
    },
  };
}
