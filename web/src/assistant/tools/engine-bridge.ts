// The real engine behind the tools: the worker pool for simulation and paired
// comparison, and the wasm module's main-thread calls for everything else.
import { applyPatch, digest, engineInput, equityPreview, getPool, levers, planPreview, schema, type RunResult } from '@/engine';
import type { RunRecord } from './runs';
import type { EngineBridge } from './types';

let cachedSchema: unknown;

/** The part of a pool result that `pool.comparison` reads, rebuilt from a stored run. */
const asResult = (run: RunRecord): RunResult => ({ summary: run.summary, acc: run.acc, paths: run.paths, seed: run.seed, elapsedMs: 0, pathsPerSecond: 0, profileKey: run.key });

export function realEngine(): EngineBridge {
  return {
    get pool() {
      return getPool();
    },
    applyPatch,
    levers,
    schema: () => (cachedSchema ??= schema()),
    planPreview,
    equityPreview: (company) => equityPreview(company),
    // About 10 ms at 10,000 lives; the accumulator is already on this thread.
    digest: (run) => digest(run.acc, engineInput(run.profile, run.seed).json),
    // Off the main thread: decoding two accumulators is the expensive part.
    compare: (a, b) => getPool().comparison(asResult(a), asResult(b), a.profile),
  };
}
