// Messages between the pool (main thread) and an engine worker.
import type { EngineErrorKind } from './errors';

export type WorkerRequest =
  /** First message to every worker: the wasm module, compiled once on the main thread. */
  | { type: 'init'; module: WebAssembly.Module }
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

// ---- coordinator worker (holds the merged accumulator of each run) -------------------

export type CoordinatorRequest =
  | { type: 'init'; module: WebAssembly.Module }
  | { type: 'open'; job: number; profileJson: string }
  /** `bytes` is transferred. */
  | { type: 'add'; job: number; bytes: Uint8Array }
  | { type: 'summarize'; job: number; id: number; withBytes: boolean }
  | { type: 'close'; job: number }
  | { type: 'compare'; id: number; a: Uint8Array; b: Uint8Array; profileJson: string };

export type CoordinatorResponse =
  /** `json` is the Summary; it is parsed on the main thread (a string crosses threads far faster than a tree). */
  | { type: 'summary'; id: number; json: string; paths: number; ms: number; bytes: Uint8Array | null; /** Size of the coordinator's wasm memory. */ heapBytes: number }
  | { type: 'comparison'; id: number; json: string }
  | { type: 'error'; id: number; kind: EngineErrorKind; message: string };
