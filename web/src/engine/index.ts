import { mainThreadReducer } from './api';
import { EnginePool } from './pool';
import type { WorkerLike } from './protocol';

export * from './api';
export * from './errors';
export * from './pool';
export type * from './output-types';
export type * from './types';

let shared: EnginePool | null = null;

/** The app's one pool of engine workers, created on first use. */
export function getPool(): EnginePool {
  shared ??= new EnginePool({
    reducer: mainThreadReducer,
    createWorker: () =>
      new Worker(new URL('./worker.ts', import.meta.url), { type: 'module', name: 'firesight-engine' }) as unknown as WorkerLike,
  });
  return shared;
}
