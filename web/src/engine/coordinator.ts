/// <reference lib="webworker" />
// Coordinator worker: holds the merged accumulator of every live run in a wasm `Merger`
// and turns it into a Summary on request, so neither the merge of 10 MB accumulators
// nor the 50 to 200 ms summarise of a large run happens on the main thread.
import init, { Merger, compare, type InitOutput } from './pkg/firesight_wasm.js';
import { toEngineError } from './errors';
import type { CoordinatorRequest, CoordinatorResponse } from './protocol';

const scope = self as unknown as DedicatedWorkerGlobalScope;
let instance: InitOutput | null = null;
const jobs = new Map<number, { merger: Merger; json: string; error: string | null }>();

function post(message: CoordinatorResponse, transfer: Transferable[] = []) {
  scope.postMessage(message, transfer);
}

function fail(id: number, error: unknown) {
  const e = toEngineError(error);
  post({ type: 'error', id, kind: e.kind, message: e.message });
}

type Work = Exclude<CoordinatorRequest, { type: 'init' }>;

function handle(request: Work) {
  switch (request.type) {
    case 'open':
      jobs.set(request.job, { merger: new Merger(), json: request.profileJson, error: null });
      return;
    case 'add': {
      const entry = jobs.get(request.job);
      if (!entry) return; // closed (cancelled) while the chunk was in flight
      try {
        entry.merger.add(request.bytes);
      } catch (error) {
        entry.error ??= toEngineError(error).message;
      }
      return;
    }
    case 'summarize': {
      const entry = jobs.get(request.job);
      if (!entry) return fail(request.id, new Error('The run was closed.'));
      if (entry.error) return fail(request.id, new Error(entry.error));
      try {
        const t0 = performance.now();
        const json = entry.merger.summarize(entry.json);
        const bytes = request.withBytes ? entry.merger.bytes() : null;
        const ms = performance.now() - t0;
        const heapBytes = instance?.memory.buffer.byteLength ?? 0;
        post({ type: 'summary', id: request.id, json, paths: entry.merger.paths(), ms, bytes, heapBytes }, bytes ? [bytes.buffer] : []);
      } catch (error) {
        fail(request.id, error);
      }
      return;
    }
    case 'close': {
      jobs.get(request.job)?.merger.free();
      jobs.delete(request.job);
      return;
    }
    case 'compare':
      try {
        post({ type: 'comparison', id: request.id, json: compare(request.a, request.b, request.profileJson) });
      } catch (error) {
        fail(request.id, error);
      }
      return;
  }
}

const backlog: Work[] = [];
let ready = false;
let failed: string | null = null;

function refuse(r: Work) {
  if (r.type === 'summarize' || r.type === 'compare') post({ type: 'error', id: r.id, kind: 'worker', message: `The engine failed to load: ${failed}` });
}

scope.onmessage = (event: MessageEvent<CoordinatorRequest>) => {
  const r = event.data;
  if (r.type === 'init') {
    init({ module_or_path: r.module })
      .then((output) => {
        instance = output;
        ready = true;
        for (const queued of backlog.splice(0)) handle(queued);
      })
      .catch((error: unknown) => {
        failed = error instanceof Error ? error.message : String(error);
        for (const queued of backlog.splice(0)) refuse(queued);
      });
    return;
  }
  if (failed !== null) refuse(r);
  else if (ready) handle(r);
  else backlog.push(r);
};
