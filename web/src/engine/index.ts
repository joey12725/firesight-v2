import { createWorkerAggregator, type CoordinatorLike } from './aggregator';
import { compileEngine, initEngine } from './api';
import { EnginePool } from './pool';
import type { WorkerLike } from './protocol';
import wasmUrl from './pkg/firesight_wasm_bg.wasm?url';

export * from './api';
export * from './errors';
export * from './pool';
export * from './output-types';
export type * from './types';
export { aggregatorStats } from './aggregator';

/** The engine's wasm module, compiled once (streaming) and shared with every worker. */
export function engineModule(): Promise<WebAssembly.Module> {
  return compileEngine(wasmUrl);
}

/** Loads the engine on the main thread. Await it once before calling anything in `api.ts`. */
export function startEngine(): Promise<void> {
  return initEngine(engineModule());
}

let shared: EnginePool | null = null;

/** `?workers=N` on the page URL sets the number of engine workers, for measuring (web/e2e/perf.mjs). */
function workersOverride(): number | undefined {
  const n = Number(new URLSearchParams(globalThis.location?.search ?? '').get('workers'));
  return Number.isInteger(n) && n >= 1 && n <= 32 ? n : undefined;
}

/** The app's one pool of engine workers and its coordinator, created on first use. */
export function getPool(): EnginePool {
  shared ??= new EnginePool({
    size: workersOverride(),
    aggregator: createWorkerAggregator(() => {
      const worker = new Worker(new URL('./coordinator.ts', import.meta.url), { type: 'module', name: 'firesight-coordinator' });
      void engineModule().then((module) => worker.postMessage({ type: 'init', module }));
      return worker as unknown as CoordinatorLike;
    }),
    createWorker: () => {
      const worker = new Worker(new URL('./worker.ts', import.meta.url), { type: 'module', name: 'firesight-engine' });
      void engineModule().then((module) => worker.postMessage({ type: 'init', module }));
      return worker as unknown as WorkerLike;
    },
  });
  return shared;
}
