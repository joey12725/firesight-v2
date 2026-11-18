/// <reference lib="webworker" />
// Engine worker: loads the wasm module once, keeps compiled engines keyed by profile
// hash, and simulates chunks of paths on request.
import init, { Engine } from './pkg/firesight_wasm.js';
import { toEngineError } from './errors';
import type { WorkerRequest, WorkerResponse } from './protocol';

const scope = self as unknown as DedicatedWorkerGlobalScope;

/** Compiled engines, most recently used last. A sweep touches many profiles at once. */
const MAX_ENGINES = 24;
const engines = new Map<string, Engine>();
const cancelled = new Set<number>();

function engineFor(key: string, profileJson: string): Engine {
  const hit = engines.get(key);
  if (hit) {
    engines.delete(key);
    engines.set(key, hit);
    return hit;
  }
  const engine = new Engine(profileJson);
  engines.set(key, engine);
  if (engines.size > MAX_ENGINES) {
    const oldest = engines.keys().next().value;
    if (oldest !== undefined) {
      engines.get(oldest)?.free();
      engines.delete(oldest);
    }
  }
  return engine;
}

function post(message: WorkerResponse, transfer: Transferable[] = []) {
  scope.postMessage(message, transfer);
}

function handle(request: WorkerRequest) {
  switch (request.type) {
    case 'cancel':
      cancelled.add(request.jobId);
      // Job ids only grow, so old entries can never match again.
      if (cancelled.size > 256) cancelled.delete(cancelled.values().next().value as number);
      return;
    case 'runChunk': {
      const { jobId, taskId, firstPath, count } = request;
      if (cancelled.has(jobId)) return post({ type: 'cancelled', jobId, taskId });
      try {
        const engine = engineFor(request.profileKey, request.profileJson);
        const t0 = performance.now();
        const bytes = engine.runChunk(firstPath, count);
        const ms = performance.now() - t0;
        post({ type: 'chunk', jobId, taskId, firstPath, count, bytes, ms }, [bytes.buffer]);
      } catch (error) {
        const e = toEngineError(error);
        post({ type: 'error', jobId, taskId, kind: e.kind, message: e.message });
      }
      return;
    }
    case 'trace': {
      const { taskId } = request;
      try {
        const engine = engineFor(request.profileKey, request.profileJson);
        post({ type: 'trace', taskId, json: engine.trace(request.path) });
      } catch (error) {
        const e = toEngineError(error);
        post({ type: 'error', taskId, kind: e.kind, message: e.message });
      }
      return;
    }
  }
}

// Requests that arrive while the wasm module is still loading wait here.
const backlog: WorkerRequest[] = [];
let ready = false;

scope.onmessage = (event: MessageEvent<WorkerRequest>) => {
  if (ready) handle(event.data);
  else backlog.push(event.data);
};

init()
  .then(() => {
    ready = true;
    post({ type: 'ready' });
    // Cancels first, so a queued chunk for a cancelled job is skipped.
    for (const r of backlog) if (r.type === 'cancel') handle(r);
    for (const r of backlog) if (r.type !== 'cancel') handle(r);
    backlog.length = 0;
  })
  .catch((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    for (const r of backlog) {
      if (r.type !== 'cancel') {
        post({ type: 'error', taskId: r.taskId, kind: 'worker', message: `The engine failed to load: ${message}` });
      }
    }
    scope.onmessage = (event: MessageEvent<WorkerRequest>) => {
      const r = event.data;
      if (r.type !== 'cancel') {
        post({ type: 'error', taskId: r.taskId, kind: 'worker', message: `The engine failed to load: ${message}` });
      }
    };
  });
