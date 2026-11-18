// Typed wrappers for the wasm calls that run on the main thread. Call `initEngine()`
// (and await it) once before anything else here.
import init, * as wasm from './pkg/firesight_wasm.js';
import { toEngineError } from './errors';
import type { Summary } from './output-types';
import type { Reducer } from './pool';
import type { ExampleProfile, Lever, PatchOp, Profile, Report } from './types';

let loading: Promise<void> | null = null;

/** Loads and instantiates the wasm module. Safe to call more than once. */
export function initEngine(): Promise<void> {
  loading ??= init().then(() => undefined);
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
  return call(() => wasm.applyPatch(text(profile), JSON.stringify(ops)));
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
  return call(() => wasm.migrateV1(v1Json));
}

export function levers(profile: Profile | string): Lever[] {
  return call(() => wasm.levers(text(profile)));
}

export function schema(): unknown {
  return call(() => wasm.schema());
}

export function defaultProfile(): Profile {
  return call(() => wasm.defaultProfile());
}

export function exampleProfiles(): ExampleProfile[] {
  return call(() => wasm.exampleProfiles());
}

/** Merge and summarise on the calling thread. See the note at the top of `pool.ts`. */
export const mainThreadReducer: Reducer = {
  merge(a, b) {
    try {
      return wasm.mergeChunks(a, b);
    } catch (error) {
      throw toEngineError(error);
    }
  },
  summarize(acc, profileJson) {
    return call<Summary>(() => wasm.summarize(acc, profileJson));
  },
};
