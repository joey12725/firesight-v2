import { describe, expect, it } from 'vitest';
import { EngineError } from './errors';
import type { Summary } from './output-types';
import { EnginePool, FIRST_CHUNK, defaultPoolSize, engineInput, nextChunkSize, type Aggregator } from './pool';
import type { WorkerLike, WorkerRequest, WorkerResponse } from './protocol';
import type { Profile } from './types';

// A fake accumulator is the sorted list of [firstPath, count] ranges it covers, tagged
// with the profile key, so tests can see exactly which paths were merged.
type Range = [number, number];
const encode = (key: string, ranges: Range[]) =>
  new TextEncoder().encode(JSON.stringify({ key, ranges: [...ranges].sort((a, b) => a[0] - b[0]) }));
const decode = (bytes: Uint8Array) => JSON.parse(new TextDecoder().decode(bytes)) as { key: string; ranges: Range[] };

/** An in-memory aggregator over the fake accumulators; records what it was asked to do. */
function fakeAggregator() {
  const jobs = new Map<number, { key: string; ranges: Range[] }>();
  const log: string[] = [];
  const aggregator: Aggregator = {
    open(job) {
      jobs.set(job, { key: '', ranges: [] });
      log.push(`open ${job}`);
    },
    add(job, bytes) {
      const entry = jobs.get(job);
      if (!entry) return;
      const chunk = decode(bytes);
      if (entry.key && entry.key !== chunk.key) throw new Error('merged accumulators of different profiles');
      entry.key = chunk.key;
      entry.ranges.push(...chunk.ranges);
    },
    async summarize(job, withBytes) {
      await Promise.resolve();
      const entry = jobs.get(job);
      if (!entry) throw new Error('closed');
      const ranges = [...entry.ranges].sort((a, b) => a[0] - b[0]);
      const paths = ranges.reduce((n, r) => n + r[1], 0);
      return { summary: { key: entry.key, ranges, paths } as unknown as Summary, paths, ms: 1, acc: withBytes ? encode(entry.key, ranges) : null };
    },
    close(job) {
      jobs.delete(job);
      log.push(`close ${job}`);
    },
    compare(a, b) {
      return Promise.resolve({ paths: decode(a).ranges.length, same: JSON.stringify(decode(a).ranges) === JSON.stringify(decode(b).ranges) } as never);
    },
  };
  return { aggregator, jobs, log };
}

type Mode = 'manual' | 'auto';

class FakeWorker implements WorkerLike {
  onmessage: ((event: { data: WorkerResponse }) => void) | null = null;
  onerror: ((event: { message?: string }) => void) | null = null;
  readonly received: WorkerRequest[] = [];
  readonly queue: Extract<WorkerRequest, { type: 'runChunk' }>[] = [];
  terminated = false;
  failWith: { kind: EngineError['kind']; message: string } | null = null;
  private readonly mode: Mode;

  constructor(mode: Mode) {
    this.mode = mode;
  }

  postMessage(message: WorkerRequest) {
    this.received.push(message);
    if (message.type === 'runChunk') {
      this.queue.push(message);
      if (this.mode === 'auto') queueMicrotask(() => this.complete());
    } else if (message.type === 'trace') {
      queueMicrotask(() =>
        this.onmessage?.({ data: { type: 'trace', taskId: message.taskId, json: JSON.stringify({ path: message.path }) } }),
      );
    }
  }

  /** Finishes the oldest outstanding chunk. */
  complete() {
    const m = this.queue.shift();
    if (!m) return false;
    if (this.failWith) {
      this.onmessage?.({ data: { type: 'error', jobId: m.jobId, taskId: m.taskId, ...this.failWith } });
    } else {
      this.onmessage?.({
        data: {
          type: 'chunk',
          jobId: m.jobId,
          taskId: m.taskId,
          firstPath: m.firstPath,
          count: m.count,
          bytes: encode(m.profileKey, [[m.firstPath, m.count]]),
          ms: m.count,
        },
      });
    }
    return true;
  }

  terminate() {
    this.terminated = true;
  }
}

function makePool(size: number, mode: Mode = 'auto') {
  const workers: FakeWorker[] = [];
  const agg = fakeAggregator();
  const pool = new EnginePool({
    size,
    aggregator: agg.aggregator,
    summaryIntervalMs: 0,
    createWorker: () => {
      const w = new FakeWorker(mode);
      workers.push(w);
      return w;
    },
  });
  return { pool, workers, agg };
}

const profile = (seed = 7, tag = 'a') =>
  ({ name: tag, simulation: { paths: 10_000, seed, end_age: 100 } }) as unknown as Profile;

const chunksOf = (workers: FakeWorker[]) =>
  workers.flatMap((w) => w.received.filter((m) => m.type === 'runChunk'));

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Asserts the ranges tile 0..total with no gap and no overlap. */
function expectTiling(ranges: Range[], total: number) {
  let next = 0;
  for (const [first, count] of [...ranges].sort((a, b) => a[0] - b[0])) {
    expect(first).toBe(next);
    expect(count).toBeGreaterThan(0);
    next = first + count;
  }
  expect(next).toBe(total);
}

describe('chunk sizing', () => {
  it('starts small, grows geometrically and never overshoots', () => {
    const sizes: number[] = [];
    let dispatched = 0;
    while (dispatched < 20_000) {
      const size = nextChunkSize(dispatched, 20_000, 512);
      sizes.push(size);
      dispatched += size;
    }
    expect(sizes[0]).toBe(FIRST_CHUNK);
    expect(Math.max(...sizes)).toBe(512);
    expect(dispatched).toBe(20_000);
    const growing = sizes.slice(0, -1);
    expect(growing).toEqual([...growing].sort((a, b) => a - b));
    // The first 1,000 paths arrive in small pieces for a quick first picture.
    expect(sizes.slice(0, 5).every((s) => s <= 64)).toBe(true);
  });

  it('clamps the pool size', () => {
    expect(defaultPoolSize(1)).toBe(1);
    expect(defaultPoolSize(8)).toBe(7);
    expect(defaultPoolSize(64)).toBe(12);
    expect(defaultPoolSize(undefined)).toBe(3);
  });

  it('keys the engine cache on profile and seed but not on path count', () => {
    const a = engineInput(profile(1));
    const b = engineInput({ ...profile(1), simulation: { paths: 250_000, seed: 1, end_age: 100 } } as Profile);
    expect(a.key).toBe(b.key);
    expect(engineInput(profile(1), 2).key).not.toBe(a.key);
    expect(engineInput(profile(1), 2).seed).toBe(2);
  });
});

describe('EnginePool.run', () => {
  it('covers every path exactly once and reports monotone progress', async () => {
    const { pool, workers } = makePool(4);
    const progress: number[] = [];
    const result = await pool.run(profile(), { paths: 5_000 }, { onProgress: (p) => progress.push(p.done) });
    const summary = result.summary as unknown as { ranges: Range[]; paths: number };
    expectTiling(summary.ranges, 5_000);
    expect(summary.paths).toBe(5_000);
    expect(progress.at(-1)).toBe(5_000);
    expect(progress).toEqual([...progress].sort((a, b) => a - b));
    expect(progress.length).toBeGreaterThan(1);
    expect(result.seed).toBe(7);
    expect(result.pathsPerSecond).toBeGreaterThan(0);
    // All four workers took part.
    expect(workers.every((w) => w.received.some((m) => m.type === 'runChunk'))).toBe(true);
    expect(pool.activeJobs).toBe(0);
  });

  it('skips progressive summaries nobody would see, still counts progress and always delivers the final one', async () => {
    const { pool } = makePool(4);
    const pictures: number[] = [];
    const arrived: number[] = [];
    const result = await pool.run(profile(), { paths: 5_000 }, { wanted: () => false, onProgress: (p) => pictures.push(p.done), onArrived: (done) => arrived.push(done) });
    expect(pictures).toEqual([5_000]);
    expect(arrived.length).toBeGreaterThan(3);
    expect(arrived.at(-1)).toBe(5_000);
    expect(arrived).toEqual([...arrived].sort((a, b) => a - b));
    expectTiling((result.summary as unknown as { ranges: Range[] }).ranges, 5_000);
    // A run that becomes wanted part-way starts reporting pictures again.
    let seen = 0;
    const later: number[] = [];
    await pool.run(profile(8), { paths: 5_000 }, { wanted: () => ++seen > 6, onProgress: (p) => later.push(p.done) });
    expect(later.length).toBeGreaterThan(1);
    expect(later.at(-1)).toBe(5_000);
  });

  it('gives each worker one chunk at a time', async () => {
    const { pool, workers } = makePool(3, 'manual');
    const run = pool.run(profile(), { paths: 400 });
    expect(workers.map((w) => w.queue.length)).toEqual([1, 1, 1]);
    expect(workers.map((w) => w.queue[0]?.firstPath)).toEqual([0, 32, 64]);
    workers[1]?.complete();
    expect(workers.map((w) => w.queue.length)).toEqual([1, 1, 1]);
    expect(workers[1]?.queue[0]?.firstPath).toBe(96);
    while (workers.some((w) => w.complete())) await tick();
    await run;
  });

  it('merges to the same bytes whatever order chunks come back in', async () => {
    const finish = async (order: (ws: FakeWorker[]) => FakeWorker[]) => {
      const { pool, workers } = makePool(4, 'manual');
      const run = pool.run(profile(), { paths: 3_000 });
      for (;;) {
        const busy = order(workers).filter((w) => w.queue.length > 0);
        if (busy.length === 0) break;
        busy[0]?.complete();
        await tick();
      }
      return run;
    };
    const forward = await finish((ws) => ws);
    const backward = await finish((ws) => [...ws].reverse());
    expect(decode(forward.acc)).toEqual(decode(backward.acc));
    expectTiling(decode(forward.acc).ranges, 3_000);
    expectTiling(decode(backward.acc).ranges, 3_000);
  });

  it('rejects when the engine reports an error and stops handing out work', async () => {
    const { pool, workers } = makePool(2, 'manual');
    const run = pool.run(profile(), { paths: 10_000 });
    for (const w of workers) w.failWith = { kind: 'not_implemented', message: 'not implemented: stub' };
    workers[0]?.complete();
    await expect(run).rejects.toMatchObject({ kind: 'not_implemented' });
    const before = chunksOf(workers).length;
    workers[1]?.complete();
    await tick();
    expect(chunksOf(workers).length).toBe(before);
    expect(pool.activeJobs).toBe(0);
  });

  it('rejects immediately for an already-aborted signal', async () => {
    const { pool, workers } = makePool(2);
    const controller = new AbortController();
    controller.abort();
    await expect(pool.run(profile(), { paths: 100 }, { signal: controller.signal })).rejects.toMatchObject({
      kind: 'aborted',
    });
    expect(chunksOf(workers)).toHaveLength(0);
  });
});

describe('cancellation', () => {
  it('stops a stale run, tells busy workers, and drops late results', async () => {
    const { pool, workers } = makePool(2, 'manual');
    const controller = new AbortController();
    const seen: number[] = [];
    const stale = pool.run(profile(1, 'stale'), { paths: 50_000 }, {
      signal: controller.signal,
      onProgress: (p) => seen.push(p.done),
    });
    workers[0]?.complete();
    await tick();
    expect(seen).toEqual([32]);

    controller.abort();
    await expect(stale).rejects.toMatchObject({ kind: 'aborted', name: 'AbortError' });
    expect(workers.every((w) => w.received.some((m) => m.type === 'cancel'))).toBe(true);

    // A newer run starts as workers free up; the stale chunks still in flight are ignored.
    const fresh = pool.run(profile(2, 'fresh'), { paths: 200 });
    while (workers.some((w) => w.complete())) await tick();
    const result = await fresh;
    expectTiling(decode(result.acc).ranges, 200);
    expect(seen).toEqual([32]);
    expect(decode(result.acc).key).toBe(engineInput(profile(2, 'fresh')).key);
  });

  it('releases the accumulator of a finished or cancelled run', async () => {
    const { pool, agg } = makePool(2);
    await pool.run(profile(), { paths: 500 });
    const controller = new AbortController();
    const stale = pool.run(profile(2), { paths: 50_000 }, { signal: controller.signal, onProgress: () => controller.abort() });
    await expect(stale).rejects.toMatchObject({ kind: 'aborted' });
    expect(agg.jobs.size).toBe(0);
    expect(agg.log.filter((l) => l.startsWith('open'))).toHaveLength(2);
    expect(agg.log.filter((l) => l.startsWith('close'))).toHaveLength(2);
  });

  it('does not report progress after abort even when a merge is in flight', async () => {
    const { pool } = makePool(3);
    const controller = new AbortController();
    let calls = 0;
    const run = pool.run(profile(), { paths: 100_000 }, {
      signal: controller.signal,
      onProgress: () => {
        calls += 1;
        if (calls === 2) controller.abort();
      },
    });
    await expect(run).rejects.toBeInstanceOf(EngineError);
    const after = calls;
    await tick();
    await tick();
    expect(calls).toBe(after);
  });
});

describe('runMany and compare', () => {
  it('shares one seed and advances jobs together', async () => {
    const { pool, workers } = makePool(2, 'manual');
    const jobs = ['a', 'b', 'c'].map((id) => ({ id, profile: profile(99, id), paths: 2_000 }));
    const done: Record<string, number> = { a: 0, b: 0, c: 0 };
    let worstSpread = 0;
    const run = pool.runMany(jobs, {
      seed: 5,
      onProgress: (id, p) => {
        done[id] = p.done;
        const values = Object.values(done);
        worstSpread = Math.max(worstSpread, Math.max(...values) - Math.min(...values));
      },
    });
    while (workers.some((w) => w.complete())) await tick();
    const results = await run;
    expect([...results.keys()]).toEqual(['a', 'b', 'c']);
    for (const r of results.values()) {
      expect(r.seed).toBe(5);
      expectTiling(decode(r.acc).ranges, 2_000);
    }
    // Fair scheduling: no job gets more than a couple of chunks ahead of another.
    expect(worstSpread).toBeLessThanOrEqual(1_100);
    expect(new Set([...results.values()].map((r) => r.profileKey)).size).toBe(3);
  });

  it('serves a higher-priority run before a background sweep', async () => {
    const { pool, workers } = makePool(1, 'manual');
    const sweep = pool.run(profile(1, 'sweep'), { paths: 64 });
    const main = pool.run(profile(1, 'main'), { paths: 64 }, { priority: 1 });
    const order: string[] = [];
    const mainKey = engineInput(profile(1, 'main')).key;
    for (;;) {
      const next = workers[0]?.queue[0];
      if (!next) break;
      order.push(next.profileKey === mainKey ? 'main' : 'sweep');
      workers[0]?.complete();
      await tick();
    }
    await Promise.all([sweep, main]);
    // The sweep's first chunk was already running; after that the main run goes first.
    expect(order).toEqual(['sweep', 'main', 'main', 'sweep']);
  });

  it('cancels sibling jobs when one fails', async () => {
    const { pool, workers } = makePool(2, 'manual');
    const run = pool.runMany([
      { id: 'a', profile: profile(1, 'a'), paths: 5_000 },
      { id: 'b', profile: profile(1, 'b'), paths: 5_000 },
    ]);
    const w = workers[0] as FakeWorker;
    w.failWith = { kind: 'engine', message: 'boom' };
    w.complete();
    await expect(run).rejects.toMatchObject({ kind: 'engine', message: 'boom' });
    await tick();
    expect(pool.activeJobs).toBe(0);
  });

  it('compare runs both sides on the same paths', async () => {
    const { pool } = makePool(3);
    const result = await pool.compare(profile(3, 'base'), profile(4, 'scenario'), { paths: 1_000 });
    expect(result.base.seed).toBe(3);
    expect(result.scenario.seed).toBe(3);
    expect(decode(result.base.acc).ranges).toEqual(decode(result.scenario.acc).ranges.map((r) => r));
    expect(result.comparison).toMatchObject({ same: true });
  });
});

describe('trace and worker failure', () => {
  it('traces jump the queue', async () => {
    const { pool, workers } = makePool(1, 'manual');
    const run = pool.run(profile(), { paths: 100 });
    const trace = pool.trace(profile(), 42);
    workers[0]?.complete();
    await expect(trace).resolves.toEqual({ path: 42 });
    while (workers.some((w) => w.complete())) await tick();
    await run;
  });

  it('replaces a crashed worker and fails the job that was on it', async () => {
    const { pool, workers } = makePool(1, 'manual');
    const run = pool.run(profile(), { paths: 100 });
    workers[0]?.onerror?.({ message: 'out of memory' });
    await expect(run).rejects.toMatchObject({ kind: 'worker' });
    expect(workers[0]?.terminated).toBe(true);
    expect(workers).toHaveLength(2);
    const again = pool.run(profile(), { paths: 64 });
    while (workers.some((w) => w.complete())) await tick();
    await expect(again).resolves.toMatchObject({ paths: 64 });
  });
});
