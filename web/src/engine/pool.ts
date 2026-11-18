// The worker pool: splits runs into chunks, hands them to idle workers, merges the
// accumulators as they come back and summarises at a throttled cadence so charts
// sharpen progressively.
//
// Everything that touches wasm on the calling thread goes through `Reducer`, whose
// methods may be async. Today it is the main-thread implementation in `api.ts`; if
// merge + summarise at 100k+ paths turns out to jank the UI once the simulation exists,
// implement `Reducer` over a coordinator worker and pass it in. Nothing else changes.
import { EngineError, toEngineError } from './errors';
import { hashString } from './hash';
import type { Comparison, PathTrace, Summary } from './output-types';
import type { WorkerLike, WorkerResponse } from './protocol';
import type { Profile } from './types';

export interface Reducer {
  /** Merges two serialised accumulators. Commutative and associative in the engine. */
  merge(a: Uint8Array, b: Uint8Array): Uint8Array | Promise<Uint8Array>;
  summarize(acc: Uint8Array, profileJson: string): Summary | Promise<Summary>;
}

export interface RunSettings {
  /** Number of paths to simulate: path indices 0 .. paths - 1. */
  paths: number;
  /** Overrides `profile.simulation.seed`. Same seed + same path index = same world. */
  seed?: number;
}

export interface RunProgress {
  /** Summary of the paths merged so far. */
  summary: Summary;
  /** The merged accumulator behind `summary`. Do not mutate. */
  acc: Uint8Array;
  done: number;
  total: number;
  pathsPerSecond: number;
  /** True for the last call, when `done === total`. */
  final: boolean;
}

export interface RunOptions {
  onProgress?: (progress: RunProgress) => void;
  /** Aborting rejects the promise with an `EngineError` of kind `aborted`. */
  signal?: AbortSignal;
  /** Higher runs first. The interactive run uses 1; background sweeps leave it at 0. */
  priority?: number;
  /** When false, only the final summary is computed (sweeps do not need the others). */
  progressive?: boolean;
}

export interface RunResult {
  summary: Summary;
  acc: Uint8Array;
  paths: number;
  seed: number;
  elapsedMs: number;
  pathsPerSecond: number;
  /** Identifies the compiled profile + seed; equal keys mean identical inputs. */
  profileKey: string;
}

export interface ManyJob {
  id: string;
  profile: Profile;
  paths: number;
}

export interface ManyOptions {
  /** One seed for every job, so results are paired (common random numbers). */
  seed?: number;
  onProgress?: (id: string, progress: RunProgress) => void;
  signal?: AbortSignal;
  priority?: number;
  progressive?: boolean;
}

export interface CompareResult {
  base: RunResult;
  scenario: RunResult;
  /**
   * TODO(wasm): null until the wasm wrapper exports core's `compare(a, b)`. Both runs
   * already share seed and path indices, so the two accumulators line up path by path;
   * add `compare` to `Reducer` and call it here.
   */
  comparison: Comparison | null;
}

export interface PoolOptions {
  createWorker: () => WorkerLike;
  reducer: Reducer;
  /** Defaults to `navigator.hardwareConcurrency - 1`, clamped to 1..12. */
  size?: number;
  /** Minimum time between progressive summaries. */
  summaryIntervalMs?: number;
  now?: () => number;
}

/** First chunks are small so the first picture arrives within about 100 ms. */
export const FIRST_CHUNK = 32;
const DEFAULT_MAX_CHUNK = 512;
const MIN_MAX_CHUNK = 64;
const HARD_MAX_CHUNK = 4096;
/** Chunks are sized to take about this long, which bounds how stale a cancel can be. */
const TARGET_CHUNK_MS = 120;
/** The engine's cache key ignores the path count, which does not affect compilation. */
const CANONICAL_PATHS = 10_000;

export function defaultPoolSize(hardwareConcurrency: number | undefined): number {
  return Math.min(12, Math.max(1, (hardwareConcurrency ?? 4) - 1));
}

/**
 * Size of the next chunk for a job: a quarter of what has been handed out so far,
 * between `FIRST_CHUNK` and `maxChunk`, never more than what remains.
 */
export function nextChunkSize(dispatched: number, total: number, maxChunk: number): number {
  const grown = Math.max(FIRST_CHUNK, Math.min(maxChunk, Math.floor(dispatched / 4)));
  return Math.min(grown, total - dispatched);
}

/** The JSON a worker compiles, and its cache key. */
export function engineInput(profile: Profile, seed?: number): { json: string; key: string; seed: number } {
  const resolved = seed ?? profile.simulation.seed;
  const json = JSON.stringify({
    ...profile,
    simulation: { ...profile.simulation, seed: resolved, paths: CANONICAL_PATHS },
  });
  return { json, key: hashString(json), seed: resolved };
}

interface Job {
  id: number;
  key: string;
  json: string;
  seed: number;
  total: number;
  priority: number;
  progressive: boolean;
  onProgress?: (progress: RunProgress) => void;
  /** Paths handed to workers. */
  dispatched: number;
  /** Paths merged into `acc`. */
  done: number;
  /** Chunks returned by workers, waiting to be merged. */
  pending: { bytes: Uint8Array; count: number }[];
  acc: Uint8Array | null;
  flushing: boolean;
  flushes: number;
  lastFlushAt: number;
  lastFlushCost: number;
  timer: ReturnType<typeof setTimeout> | null;
  startedAt: number;
  active: boolean;
  resolve: (result: RunResult) => void;
  reject: (error: EngineError) => void;
  detach: () => void;
}

interface TraceTask {
  key: string;
  json: string;
  path: number;
  resolve: (trace: PathTrace) => void;
  reject: (error: EngineError) => void;
}

type Task = { kind: 'chunk'; job: Job; count: number } | { kind: 'trace'; trace: TraceTask };

interface Slot {
  worker: WorkerLike;
  task: Task | null;
  taskId: number;
}

export class EnginePool {
  readonly size: number;
  private readonly options: PoolOptions;
  private readonly now: () => number;
  private readonly slots: Slot[] = [];
  private readonly jobs = new Map<number, Job>();
  private readonly traces: TraceTask[] = [];
  private nextJobId = 1;
  private nextTaskId = 1;
  private maxChunk = DEFAULT_MAX_CHUNK;
  private msPerPath: number | null = null;
  private disposed = false;

  constructor(options: PoolOptions) {
    this.options = options;
    this.now = options.now ?? (() => performance.now());
    this.size = options.size ?? defaultPoolSize(globalThis.navigator?.hardwareConcurrency);
    for (let i = 0; i < this.size; i++) this.slots.push(this.spawn());
  }

  /** Simulates `settings.paths` lives of `profile` and resolves with the final summary. */
  run(profile: Profile, settings: RunSettings, options: RunOptions = {}): Promise<RunResult> {
    const { json, key, seed } = engineInput(profile, settings.seed);
    const total = Math.max(0, Math.floor(settings.paths));
    return new Promise<RunResult>((resolve, reject) => {
      if (this.disposed) return reject(new EngineError('worker', 'The engine pool has been shut down.'));
      if (options.signal?.aborted) return reject(new EngineError('aborted', 'Run cancelled.'));
      if (total === 0) return reject(new EngineError('engine', 'A run needs at least one path.'));
      const job: Job = {
        id: this.nextJobId++,
        key,
        json,
        seed,
        total,
        priority: options.priority ?? 0,
        progressive: options.progressive ?? true,
        onProgress: options.onProgress,
        dispatched: 0,
        done: 0,
        pending: [],
        acc: null,
        flushing: false,
        flushes: 0,
        lastFlushAt: 0,
        lastFlushCost: 0,
        timer: null,
        startedAt: this.now(),
        active: true,
        resolve,
        reject,
        detach: () => {},
      };
      if (options.signal) {
        const signal = options.signal;
        const onAbort = () => this.fail(job, new EngineError('aborted', 'Run cancelled.'));
        signal.addEventListener('abort', onAbort, { once: true });
        job.detach = () => signal.removeEventListener('abort', onAbort);
      }
      this.jobs.set(job.id, job);
      this.pump();
    });
  }

  /**
   * Runs several profiles on the same seed with fair scheduling: workers always take
   * the next chunk from the job that is furthest behind, so all results sharpen
   * together. Rejects on the first failure and cancels the rest.
   */
  async runMany(jobs: ManyJob[], options: ManyOptions = {}): Promise<Map<string, RunResult>> {
    const inner = new AbortController();
    const outer = options.signal;
    const forward = () => inner.abort();
    if (outer?.aborted) inner.abort();
    outer?.addEventListener('abort', forward, { once: true });
    try {
      const results = await Promise.all(
        jobs.map((job) =>
          this.run(
            job.profile,
            { paths: job.paths, seed: options.seed ?? jobs[0]?.profile.simulation.seed },
            {
              signal: inner.signal,
              priority: options.priority,
              progressive: options.progressive,
              onProgress: options.onProgress && ((p) => options.onProgress?.(job.id, p)),
            },
          ),
        ),
      );
      return new Map(jobs.map((job, i) => [job.id, results[i] as RunResult]));
    } catch (error) {
      inner.abort();
      throw error;
    } finally {
      outer?.removeEventListener('abort', forward);
    }
  }

  /** Baseline and scenario on the same seed and path indices. */
  async compare(
    base: Profile,
    scenario: Profile,
    settings: RunSettings,
    options: Omit<ManyOptions, 'seed'> = {},
  ): Promise<CompareResult> {
    const results = await this.runMany(
      [
        { id: 'base', profile: base, paths: settings.paths },
        { id: 'scenario', profile: scenario, paths: settings.paths },
      ],
      { ...options, seed: settings.seed ?? base.simulation.seed },
    );
    return {
      base: results.get('base') as RunResult,
      scenario: results.get('scenario') as RunResult,
      comparison: null,
    };
  }

  /** Re-simulates one path with its full ledger. Jumps the queue. */
  trace(profile: Profile, pathIndex: number, seed?: number): Promise<PathTrace> {
    const { json, key } = engineInput(profile, seed);
    return new Promise<PathTrace>((resolve, reject) => {
      if (this.disposed) return reject(new EngineError('worker', 'The engine pool has been shut down.'));
      this.traces.push({ key, json, path: pathIndex, resolve, reject });
      this.pump();
    });
  }

  /** Jobs that have not finished, for diagnostics. */
  get activeJobs(): number {
    return this.jobs.size;
  }

  dispose() {
    this.disposed = true;
    const error = new EngineError('aborted', 'The engine pool was shut down.');
    for (const job of [...this.jobs.values()]) this.fail(job, error);
    for (const trace of this.traces.splice(0)) trace.reject(error);
    for (const slot of this.slots) slot.worker.terminate();
    this.slots.length = 0;
  }

  // ---- scheduling ---------------------------------------------------------------

  private spawn(): Slot {
    const slot: Slot = { worker: this.options.createWorker(), task: null, taskId: 0 };
    slot.worker.onmessage = (event) => this.onMessage(slot, event.data);
    slot.worker.onerror = (event) => this.onCrash(slot, event.message ?? 'worker crashed');
    return slot;
  }

  /** The job that should get the next idle worker: priority first, then furthest behind. */
  private pick(): Job | null {
    let best: Job | null = null;
    for (const job of this.jobs.values()) {
      if (!job.active || job.dispatched >= job.total) continue;
      if (
        !best ||
        job.priority > best.priority ||
        (job.priority === best.priority && job.dispatched / job.total < best.dispatched / best.total)
      ) {
        best = job;
      }
    }
    return best;
  }

  private pump() {
    for (const slot of this.slots) {
      if (slot.task) continue;
      const trace = this.traces.shift();
      if (trace) {
        slot.task = { kind: 'trace', trace };
        slot.taskId = this.nextTaskId++;
        slot.worker.postMessage({
          type: 'trace',
          taskId: slot.taskId,
          profileKey: trace.key,
          profileJson: trace.json,
          path: trace.path,
        });
        continue;
      }
      const job = this.pick();
      if (!job) return;
      const count = nextChunkSize(job.dispatched, job.total, this.maxChunk);
      const firstPath = job.dispatched;
      job.dispatched += count;
      slot.task = { kind: 'chunk', job, count };
      slot.taskId = this.nextTaskId++;
      slot.worker.postMessage({
        type: 'runChunk',
        jobId: job.id,
        taskId: slot.taskId,
        profileKey: job.key,
        profileJson: job.json,
        firstPath,
        count,
      });
    }
  }

  private onMessage(slot: Slot, message: WorkerResponse) {
    if (message.type === 'ready') return;
    if (!slot.task || message.taskId !== slot.taskId) return;
    const task = slot.task;
    slot.task = null;
    if (task.kind === 'trace') {
      if (message.type === 'trace') {
        try {
          task.trace.resolve(JSON.parse(message.json) as PathTrace);
        } catch (error) {
          task.trace.reject(toEngineError(error));
        }
      } else if (message.type === 'error') {
        task.trace.reject(new EngineError(message.kind, message.message));
      }
    } else if (task.job.active) {
      if (message.type === 'chunk') {
        this.observe(message.ms, message.count);
        task.job.pending.push({ bytes: message.bytes, count: message.count });
        this.scheduleFlush(task.job);
      } else if (message.type === 'error') {
        this.fail(task.job, new EngineError(message.kind, message.message));
      }
    }
    this.pump();
  }

  private onCrash(slot: Slot, message: string) {
    const task = slot.task;
    slot.worker.terminate();
    const index = this.slots.indexOf(slot);
    if (index >= 0 && !this.disposed) this.slots[index] = this.spawn();
    const error = new EngineError('worker', `An engine worker stopped: ${message}`);
    if (task?.kind === 'trace') task.trace.reject(error);
    else if (task) this.fail(task.job, error);
    this.pump();
  }

  /** Adapts the chunk ceiling to the measured speed of this profile on this machine. */
  private observe(ms: number, count: number) {
    if (!(ms > 0) || count <= 0) return;
    const sample = ms / count;
    this.msPerPath = this.msPerPath === null ? sample : this.msPerPath * 0.8 + sample * 0.2;
    this.maxChunk = Math.round(
      Math.min(HARD_MAX_CHUNK, Math.max(MIN_MAX_CHUNK, TARGET_CHUNK_MS / this.msPerPath)),
    );
  }

  // ---- merging ------------------------------------------------------------------

  private scheduleFlush(job: Job) {
    if (job.flushing || !job.active || job.pending.length === 0) return;
    const arrived = job.done + job.pending.reduce((n, p) => n + p.count, 0);
    const interval = Math.max(this.options.summaryIntervalMs ?? 180, job.lastFlushCost * 4);
    const due = job.lastFlushAt + interval;
    const now = this.now();
    if (arrived >= job.total || job.flushes === 0 || now >= due) {
      if (job.timer) clearTimeout(job.timer);
      job.timer = null;
      void this.flush(job);
    } else if (!job.timer) {
      job.timer = setTimeout(() => {
        job.timer = null;
        this.scheduleFlush(job);
      }, due - now);
    }
  }

  private async flush(job: Job) {
    job.flushing = true;
    const started = this.now();
    try {
      const batch = job.pending.splice(0);
      // Fold the small new chunks together first, then touch the large accumulator once.
      let part = (batch[0] as { bytes: Uint8Array }).bytes;
      for (let i = 1; i < batch.length; i++) {
        part = await this.options.reducer.merge(part, (batch[i] as { bytes: Uint8Array }).bytes);
      }
      const acc = job.acc ? await this.options.reducer.merge(job.acc, part) : part;
      if (!job.active) return;
      job.acc = acc;
      job.done += batch.reduce((n, p) => n + p.count, 0);
      const final = job.done >= job.total;
      if (final || job.progressive) {
        const summary = await this.options.reducer.summarize(acc, job.json);
        if (!job.active) return;
        const elapsedMs = Math.max(this.now() - job.startedAt, 0.001);
        const pathsPerSecond = (job.done / elapsedMs) * 1000;
        job.onProgress?.({ summary, acc, done: job.done, total: job.total, pathsPerSecond, final });
        if (final) {
          this.finish(job);
          job.resolve({
            summary,
            acc,
            paths: job.total,
            seed: job.seed,
            elapsedMs,
            pathsPerSecond,
            profileKey: job.key,
          });
        }
      }
    } catch (error) {
      this.fail(job, toEngineError(error));
    } finally {
      job.flushing = false;
      job.flushes += 1;
      job.lastFlushAt = this.now();
      job.lastFlushCost = job.lastFlushAt - started;
      this.scheduleFlush(job);
    }
  }

  private finish(job: Job) {
    job.active = false;
    job.detach();
    if (job.timer) clearTimeout(job.timer);
    job.timer = null;
    job.pending.length = 0;
    this.jobs.delete(job.id);
  }

  private fail(job: Job, error: EngineError) {
    if (!job.active) return;
    this.finish(job);
    // Chunks already running finish and are dropped; queued ones are skipped by the worker.
    for (const slot of this.slots) {
      if (slot.task?.kind === 'chunk' && slot.task.job === job) {
        slot.worker.postMessage({ type: 'cancel', jobId: job.id });
      }
    }
    job.reject(error);
    this.pump();
  }
}
