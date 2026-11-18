// The real engine for tests, in this thread: the same wasm module the app runs, without
// the worker pool. Call `initEngineSync` first. Node simulates about 2,000 lives a
// second on one thread, so tests ask for a few hundred lives where the number is not
// the point; identical runs are served from a cache shared by the whole test file.
import { applyPatch, compare, digest, levers, planPreview, schema } from '@/engine/api';
import { EngineError, toEngineError } from '@/engine/errors';
import type { PathTrace, Summary } from '@/engine/output-types';
import { engineInput, type ManyJob, type ManyOptions, type RunResult } from '@/engine/pool';
import * as wasm from '@/engine/pkg/firesight_wasm.js';
import type { Profile } from '@/engine/types';
import type { EngineBridge } from '../tools/types';

export interface NodeEngine extends EngineBridge {
  /** How many profiles the tools asked to simulate, for cache assertions. */
  simulated: number;
  /** The number of jobs in each `runMany` call. */
  batches: number[];
  /** How many paired comparisons were computed. */
  compared: number;
}

const finished = new Map<string, { summary: Summary; acc: Uint8Array }>();

function simulate(json: string, key: string, paths: number): { summary: Summary; acc: Uint8Array } {
  const id = `${key}|${paths}`;
  let hit = finished.get(id);
  if (!hit) {
    const engine = new wasm.Engine(json);
    try {
      const acc = engine.runChunk(0, paths);
      hit = { summary: JSON.parse(wasm.summarize(acc, json)) as Summary, acc };
    } finally {
      engine.free();
    }
    finished.set(id, hit);
  }
  return hit;
}

/** `fail` returns the error the next run should reject with, or null to simulate. */
export function nodeEngine(options: { fail?: () => Error | null } = {}): NodeEngine {
  const engine: NodeEngine = {
    simulated: 0,
    batches: [],
    compared: 0,
    pool: {
      async runMany(jobs: ManyJob[], opts: ManyOptions = {}) {
        const failure = options.fail?.();
        if (failure) throw failure;
        if (opts.signal?.aborted) throw new EngineError('aborted', 'The run was cancelled.');
        engine.simulated += jobs.length;
        engine.batches.push(jobs.length);
        try {
          return new Map(
            jobs.map((job): [string, RunResult] => {
              const { json, key, seed } = engineInput(job.profile, opts.seed);
              return [job.id, { ...simulate(json, key, job.paths), paths: job.paths, seed, elapsedMs: 1, pathsPerSecond: 1, profileKey: key }];
            }),
          );
        } catch (error) {
          throw toEngineError(error);
        }
      },
      async trace(profile: Profile, path: number, seed?: number) {
        const compiled = new wasm.Engine(engineInput(profile, seed).json);
        try {
          return JSON.parse(compiled.trace(path)) as PathTrace;
        } catch (error) {
          throw toEngineError(error);
        } finally {
          compiled.free();
        }
      },
    },
    applyPatch,
    levers,
    schema,
    planPreview,
    digest: (run) => digest(run.acc, engineInput(run.profile, run.seed).json),
    async compare(a, b) {
      engine.compared += 1;
      return compare(a.acc, b.acc, engineInput(a.profile, a.seed).json);
    },
  };
  return engine;
}
