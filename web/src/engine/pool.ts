// The worker pool: splits runs into chunks, hands them to idle workers, and passes each
// chunk to an `Aggregator` that holds the run's merged accumulator (the wasm `Merger`)
// and summarises it at a throttled cadence, so charts sharpen progressively.
//
// In the app the aggregator is the coordinator worker (`coordinator.ts`): at 100,000
// paths one summarise costs about 70 ms and at 250,000 about 180 ms, which would drop
// frames on the main thread several times a second. The pool itself never touches wasm;
// tests and Node scripts pass `createLocalAggregator()` from `api.ts` instead.
import { EngineError, toEngineError } from './errors';
import { hashString } from './hash';
import type { Comparison, PathTrace, Summary } from './output-types';
import type { WorkerLike, WorkerResponse } from './protocol';
import type { Profile } from './types';

export interface AggregateResult {
  summary: Summary;
  /** Paths merged into `summary`. */
  paths: number;
  /** Time spent merging and summarising, where it ran. */
  ms: number;
  /** The serialised accumulator, when asked for. */
  acc: Uint8Array | null;
}

/** Holds one running accumulator per job. Calls for a job are applied in order. */
export interface Aggregator {
  open(job: number, profileJson: string): void;
  /** Merges a chunk. Ownership of `bytes` passes to the aggregator. */
  add(job: number, bytes: Uint8Array): void;
  /** Summary of everything added so far; with `withBytes`, the accumulator as well. */
  summarize(job: number, withBytes: boolean): Promise<AggregateResult>;
  close(job: number): void;
  /** Paired comparison (B minus A) of two finished runs on the same seed and paths. */
  compare(a: Uint8Array, b: Uint8Array, baselineProfileJson: string): Promise<Comparison>;
  dispose?(): void;
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
  done: number;
  total: number;
  pathsPerSecond: number;
  /** Time since the run was requested. */
  elapsedMs: number;
  /** What the last merge + summarise cost off the main thread. */
  summarizeMs: number;
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
  /**
   * Asked before each progressive summary: return false while nothing would show it (the
   * tab is hidden, the view that draws this run is not open) and the merge + summarise is
   * skipped. The final summary is always computed.
   */
  wanted?: () => boolean;
  /** Called as chunks arrive, with the paths merged so far: progress without the cost of a summary. */
  onArrived?: (done: number, total: number) => void;
}

export interface RunResult {
  summary: Summary;
  /** The serialised accumulator (104 bytes per path): pass two of these to `comparison`. */
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
  /** As `RunOptions.wanted`, for this job alone. */
  wanted?: () => boolean;
}

export interface ManyOptions {
  /** One seed for every job, so results are paired (common random numbers). */
  seed?: number;
  onProgress?: (id: string, progress: RunProgress) => void;
  signal?: AbortSignal;
  priority?: number;
  progressive?: boolean;
  onArrived?: (id: string, done: number, total: number) => void;
}

export interface CompareResult {
  base: RunResult;
  scenario: RunResult;
  /** Core's paired statistics, scenario minus base, path by path. */
  comparison: Comparison;
}

export interface PoolOptions {
  createWorker: () => WorkerLike;
  aggregator: Aggregator;
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
  wanted?: () => boolean;
  onProgress?: (progress: RunProgress) => void;
  onArrived?: (done: number, total: number) => void;
  /** Paths handed to workers. */
  dispatched: number;
  /** Paths handed to the aggregator. */
  arrived: number;
  /** Paths that had arrived when the last summary was requested. */
  requested: number;
  /** Paths in the last summary reported. */
  done: number;
  summarizing: boolean;
  summaries: number;
  lastSummaryAt: number;
  lastSummaryCost: number;
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
  /** Diagnostics: time workers spent simulating and the paths that took, since the pool started. */
  readonly stats = { workerMs: 0, paths: 0, chunks: 0 };

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
        wanted: options.wanted,
        onProgress: options.onProgress,
        onArrived: options.onArrived,
        dispatched: 0,
        arrived: 0,
        requested: 0,
        done: 0,
        summarizing: false,
        summaries: 0,
        lastSummaryAt: 0,
        lastSummaryCost: 0,
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
      this.options.aggregator.open(job.id, json);
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
              wanted: job.wanted,
              onProgress: options.onProgress && ((p) => options.onProgress?.(job.id, p)),
              onArrived: options.onArrived && ((done, total) => options.onArrived?.(job.id, done, total)),
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
    const a = results.get('base') as RunResult;
    const b = results.get('scenario') as RunResult;
    return { base: a, scenario: b, comparison: await this.comparison(a, b, base) };
  }

  /**
   * Paired statistics of two finished runs made with the same seed and path count
   * (B minus A). Computed off the main thread.
   */
  comparison(a: Pick<RunResult, 'acc' | 'seed'>, b: Pick<RunResult, 'acc'>, baseline: Profile): Promise<Comparison> {
    return this.options.aggregator.compare(a.acc, b.acc, engineInput(baseline, a.seed).json);
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
    this.options.aggregator.dispose?.();
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
        task.job.arrived += message.count;
        this.options.aggregator.add(task.job.id, message.bytes);
        task.job.onArrived?.(task.job.arrived, task.job.total);
        this.scheduleSummary(task.job);
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
    this.stats.workerMs += ms;
    this.stats.paths += count;
    this.stats.chunks += 1;
    const sample = ms / count;
    this.msPerPath = this.msPerPath === null ? sample : this.msPerPath * 0.8 + sample * 0.2;
    this.maxChunk = Math.round(
      Math.min(HARD_MAX_CHUNK, Math.max(MIN_MAX_CHUNK, TARGET_CHUNK_MS / this.msPerPath)),
    );
  }

  // ---- summarising ---------------------------------------------------------------

  private scheduleSummary(job: Job) {
    if (job.summarizing || !job.active || job.arrived === job.requested) return;
    const complete = job.arrived >= job.total;
    if (!complete && (!job.progressive || (job.wanted && !job.wanted()))) return;
    // Never spend more than about a fifth of the time summarising.
    const interval = Math.max(this.options.summaryIntervalMs ?? 200, job.lastSummaryCost * 4);
    const due = job.lastSummaryAt + interval;
    const now = this.now();
    if (complete || job.summaries === 0 || now >= due) {
      if (job.timer) clearTimeout(job.timer);
      job.timer = null;
      void this.summarize(job, complete);
    } else if (!job.timer) {
      job.timer = setTimeout(() => {
        job.timer = null;
        this.scheduleSummary(job);
      }, due - now);
    }
  }

  private async summarize(job: Job, final: boolean) {
    job.summarizing = true;
    job.requested = job.arrived;
    const started = this.now();
    try {
      const { summary, paths, ms, acc } = await this.options.aggregator.summarize(job.id, final);
      if (!job.active) return;
      job.done = paths;
      const elapsedMs = Math.max(this.now() - job.startedAt, 0.001);
      const pathsPerSecond = (paths / elapsedMs) * 1000;
      job.onProgress?.({ summary, done: paths, total: job.total, pathsPerSecond, elapsedMs, summarizeMs: ms, final });
      if (final) {
        this.finish(job);
        job.resolve({
          summary,
          acc: acc ?? new Uint8Array(),
          paths: job.total,
          seed: job.seed,
          elapsedMs,
          pathsPerSecond,
          profileKey: job.key,
        });
      }
    } catch (error) {
      this.fail(job, toEngineError(error));
    } finally {
      job.summarizing = false;
      job.summaries += 1;
      job.lastSummaryAt = this.now();
      job.lastSummaryCost = job.lastSummaryAt - started;
      this.scheduleSummary(job);
    }
  }

  private finish(job: Job) {
    job.active = false;
    job.detach();
    if (job.timer) clearTimeout(job.timer);
    job.timer = null;
    this.jobs.delete(job.id);
    this.options.aggregator.close(job.id);
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
