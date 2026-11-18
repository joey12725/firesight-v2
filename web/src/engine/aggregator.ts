// The main-thread side of the coordinator worker: an `Aggregator` whose state lives in
// `coordinator.ts`. Messages to one worker are handled in order, so a summary always
// includes every chunk added before it was requested.
import { EngineError } from './errors';
import type { Comparison, Summary } from './output-types';
import type { AggregateResult, Aggregator } from './pool';
import type { CoordinatorRequest, CoordinatorResponse } from './protocol';

export interface CoordinatorLike {
  postMessage(message: CoordinatorRequest, transfer?: Transferable[]): void;
  onmessage: ((event: { data: CoordinatorResponse }) => void) | null;
  onerror: ((event: { message?: string }) => void) | null;
  terminate(): void;
}

interface Waiting {
  resolve: (value: never) => void;
  reject: (error: EngineError) => void;
}

/** Diagnostics for the performance report: what crossed threads and what it cost. */
export const aggregatorStats = { chunks: 0, chunkBytes: 0, summaries: 0, summaryBytes: 0, parseMs: 0, summarizeMs: 0, lastParseMs: 0, lastSummarizeMs: 0, coordinatorHeapBytes: 0 };

export function createWorkerAggregator(spawn: () => CoordinatorLike): Aggregator {
  let worker: CoordinatorLike | null = null;
  let nextId = 1;
  const waiting = new Map<number, Waiting>();

  const failAll = (message: string) => {
    for (const w of waiting.values()) w.reject(new EngineError('worker', message));
    waiting.clear();
  };

  const get = (): CoordinatorLike => {
    if (worker) return worker;
    const w = spawn();
    w.onmessage = ({ data }) => {
      const entry = waiting.get(data.id);
      if (!entry) return;
      waiting.delete(data.id);
      if (data.type === 'error') return entry.reject(new EngineError(data.kind, data.message));
      try {
        if (data.type === 'summary') {
          const t0 = performance.now();
          const summary = JSON.parse(data.json) as Summary;
          const parse = performance.now() - t0;
          aggregatorStats.summaries += 1;
          aggregatorStats.summaryBytes += data.json.length;
          aggregatorStats.parseMs += parse;
          aggregatorStats.summarizeMs += data.ms;
          aggregatorStats.lastParseMs = parse;
          aggregatorStats.lastSummarizeMs = data.ms;
          aggregatorStats.coordinatorHeapBytes = data.heapBytes;
          const result: AggregateResult = { summary, paths: data.paths, ms: data.ms, acc: data.bytes };
          entry.resolve(result as never);
        } else {
          entry.resolve(JSON.parse(data.json) as never);
        }
      } catch (error) {
        entry.reject(new EngineError('engine', error instanceof Error ? error.message : String(error)));
      }
    };
    w.onerror = (event) => {
      // The accumulators are gone with the worker; every live run fails and the next one starts a new worker.
      w.terminate();
      worker = null;
      failAll(`The results worker stopped: ${event.message ?? 'unknown error'}`);
    };
    worker = w;
    return w;
  };

  const ask = <T>(build: (id: number) => CoordinatorRequest): Promise<T> =>
    new Promise<T>((resolve, reject) => {
      const id = nextId++;
      waiting.set(id, { resolve: resolve as (value: never) => void, reject });
      get().postMessage(build(id));
    });

  return {
    open(job, profileJson) {
      get().postMessage({ type: 'open', job, profileJson });
    },
    add(job, bytes) {
      aggregatorStats.chunks += 1;
      aggregatorStats.chunkBytes += bytes.byteLength;
      get().postMessage({ type: 'add', job, bytes }, [bytes.buffer]);
    },
    summarize(job, withBytes) {
      return ask<AggregateResult>((id) => ({ type: 'summarize', job, id, withBytes }));
    },
    close(job) {
      worker?.postMessage({ type: 'close', job });
    },
    compare(a, b, profileJson) {
      // Not transferred: the callers keep their accumulators.
      return ask<Comparison>((id) => ({ type: 'compare', id, a, b, profileJson }));
    },
    dispose() {
      worker?.terminate();
      worker = null;
      failAll('The engine pool was shut down.');
    },
  };
}
