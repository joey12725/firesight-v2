// Messages between the pool (main thread) and an engine worker.
import type { EngineErrorKind } from './errors';

export type WorkerRequest =
  | {
      type: 'runChunk';
      jobId: number;
      taskId: number;
      /** Hash of `profileJson`; the worker keeps compiled engines keyed by it. */
      profileKey: string;
      profileJson: string;
      firstPath: number;
      count: number;
    }
  | { type: 'trace'; taskId: number; profileKey: string; profileJson: string; path: number }
  | { type: 'cancel'; jobId: number };

export type WorkerResponse =
  | { type: 'ready' }
  | {
      type: 'chunk';
      jobId: number;
      taskId: number;
      firstPath: number;
      count: number;
      /** Serialised accumulator, transferred. */
      bytes: Uint8Array;
      /** Time spent inside `runChunk`, for chunk sizing. */
      ms: number;
    }
  | { type: 'trace'; taskId: number; json: string }
  | { type: 'cancelled'; jobId: number; taskId: number }
  | { type: 'error'; jobId?: number; taskId: number; kind: EngineErrorKind; message: string };

/** The part of `Worker` the pool uses; tests supply a fake. */
export interface WorkerLike {
  postMessage(message: WorkerRequest): void;
  onmessage: ((event: { data: WorkerResponse }) => void) | null;
  onerror: ((event: { message?: string }) => void) | null;
  terminate(): void;
}
