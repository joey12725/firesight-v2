// The engine work behind the What matters view: paired batches of plan variants on the
// run's seed, staged from a quick coarse pass to a sharp one, cancelled the moment the
// plan changes, and cached by (compiled plan + seed, what was varied).
//
// Budget, per plan: the tornado is about 21 runs at 250, 1,000 and 4,000 lives (some
// 110,000 lives); the trade-off map is 25 cells at 300 lives, then 81 at 400 and 1,000
// (some 120,000 lives) and starts only when it scrolls into view; a solver search is
// at most ten runs of 4,000 lives and starts only on request. Sweeps run at pool
// priority 0, so the live run of the plan (priority 1) always goes first.
import { useSyncExternalStore } from 'react';
import { applyPatch, engineInput, isAbort, toEngineError, type EnginePool, type ManyJob } from '@/engine';
import type { Headline } from '@/engine/output-types';
import type { PatchOp, Profile } from '@/engine/types';
import { nextProbe, readLevels, trimComparison, type Goal, type LeverDef, type Levels, type PairedLite, type Probe, type SolveOutcome, type VarDef, type VarId } from './model';

/** Lives per variant in each pass of the tornado. The first is on screen within a second. */
export const LEVER_STAGES = [250, 1000, 4000];
/** Passes of the trade-off map: every `stride`-th value of each axis, at `paths` lives per cell. */
export const GRID_STAGES = [
  { stride: 2, paths: 300 },
  { stride: 1, paths: 400 },
  { stride: 1, paths: 1000 },
];
export const SOLVER_PATHS = 4000;
/** Engine workers keep 24 compiled plans each; larger batches would make them recompile on every chunk. */
const BATCH = 20;
/** Wait this long after an edit before spending cores on a sweep. */
const SETTLE_MS = 250;

export interface Progress {
  /** Lives simulated so far for this result, over every pass. */
  lives: number;
  total: number;
  perSecond: number;
  /** Lives per variant of the pass in flight. */
  paths: number;
}

class Store<T> {
  private listeners = new Set<() => void>();
  constructor(protected state: T) {}
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  get = () => this.state;
  protected set(next: Partial<T>) {
    this.state = { ...this.state, ...next };
    this.listeners.forEach((l) => l());
  }
}

export function useStore<T>(store: { subscribe: (l: () => void) => () => void; get: () => T }): T {
  return useSyncExternalStore(store.subscribe, store.get);
}

function remember<V>(cache: Map<string, V>, key: string, value: V, limit: number) {
  cache.delete(key);
  cache.set(key, value);
  if (cache.size > limit) cache.delete(cache.keys().next().value as string);
}

const pause = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/** Applies each patch to the plan; null (with the reason) where the result is not a runnable plan. Yields to the page as it goes. */
async function buildVariants(profile: Profile, patches: PatchOp[][], signal: AbortSignal): Promise<({ profile: Profile } | { skipped: string })[]> {
  const base = JSON.stringify(profile);
  const out: ({ profile: Profile } | { skipped: string })[] = [];
  for (let i = 0; i < patches.length; i++) {
    if (i > 0 && i % 6 === 0) {
      await pause();
      if (signal.aborted) return out;
    }
    try {
      const r = applyPatch(base, patches[i] as PatchOp[]);
      out.push(r.errors.length > 0 ? { skipped: r.errors[0]?.message ?? 'This setting is not a valid plan.' } : { profile: r.profile });
    } catch (error) {
      out.push({ skipped: toEngineError(error).message });
    }
  }
  return out;
}

// ---- tornado ------------------------------------------------------------------------

export interface LeverRun {
  key: string;
  seed: number;
  /** Lives per variant. */
  paths: number;
  /** The plan's own headline on these lives. */
  base: Headline;
  sides: Record<string, { headline: Headline; paired: PairedLite }>;
  /** Settings that could not be simulated, with the reason. */
  skipped: Record<string, string>;
  /** True when this is the sharpest pass. */
  final: boolean;
}

export interface LeverSweepState {
  run: LeverRun | null;
  /** `run` describes the plan before the latest edit. */
  stale: boolean;
  busy: boolean;
  progress: Progress | null;
  error: string | null;
}

const leverCache = new Map<string, LeverRun>();

export class LeverSweeper extends Store<LeverSweepState> {
  private controller: AbortController | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private wanted: string | null = null;

  constructor(private readonly pool: EnginePool) {
    super({ run: null, stale: false, busy: false, progress: null, error: null });
  }

  /** Cancels whatever is in flight and (after a pause for further edits) tests `levers` on `profile`. */
  request(profile: Profile, seed: number, levers: LeverDef[]) {
    const key = `${engineInput(profile, seed).key}|${levers.flatMap((l) => l.sides.map((s) => s.key)).join(',')}`;
    if (key === this.wanted) return;
    this.stop();
    this.wanted = key;
    const cached = leverCache.get(key);
    if (cached?.final) return this.set({ run: cached, stale: false, busy: false, progress: null, error: null });
    this.set({ run: cached ?? this.state.run, stale: !cached && this.state.run !== null, busy: true, progress: null, error: null });
    const controller = (this.controller = new AbortController());
    this.timer = setTimeout(() => void this.sweep(key, profile, seed, levers, cached ?? null, controller.signal), cached || !this.state.run ? 0 : SETTLE_MS);
  }

  stop() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.controller?.abort();
    this.controller = null;
    this.wanted = null;
    if (this.state.busy) this.set({ busy: false, progress: null, stale: this.state.run !== null && this.state.stale });
  }

  private async sweep(key: string, profile: Profile, seed: number, levers: LeverDef[], from: LeverRun | null, signal: AbortSignal) {
    try {
      const sides = levers.flatMap((l) => l.sides);
      const built = await buildVariants(profile, sides.map((s) => s.ops), signal);
      if (signal.aborted) return;
      const skipped: Record<string, string> = {};
      const jobs: { key: string; profile: Profile }[] = [];
      sides.forEach((s, i) => {
        const b = built[i];
        if (b && 'profile' in b) jobs.push({ key: s.key, profile: b.profile });
        else skipped[s.key] = b?.skipped ?? 'This setting is not a valid plan.';
      });
      const stages = LEVER_STAGES.filter((p) => !from || p > from.paths);
      const total = stages.reduce((n, p) => n + p * (jobs.length + 1), 0);
      const started = performance.now();
      let lives = 0;
      for (const paths of stages) {
        this.set({ progress: { lives, total, perSecond: lives / Math.max(0.05, (performance.now() - started) / 1000), paths } });
        const results = await this.pool.runMany([{ id: 'base', profile, paths }, ...jobs.map((j): ManyJob => ({ id: j.key, profile: j.profile, paths }))], {
          seed,
          signal,
          progressive: false,
          onProgress: (_, p) => {
            if (!p.final || signal.aborted) return;
            lives += paths;
            this.set({ progress: { lives, total, perSecond: lives / Math.max(0.05, (performance.now() - started) / 1000), paths } });
          },
        });
        const base = results.get('base');
        if (!base) return;
        const compared = await Promise.all(jobs.map((j) => this.pool.comparison(base, results.get(j.key) ?? base, profile)));
        if (signal.aborted) return;
        const run: LeverRun = {
          key,
          seed,
          paths,
          base: base.summary.headline,
          sides: Object.fromEntries(jobs.map((j, i) => [j.key, { headline: (results.get(j.key) ?? base).summary.headline, paired: trimComparison(compared[i] as NonNullable<(typeof compared)[number]>) }])),
          skipped,
          final: paths === LEVER_STAGES[LEVER_STAGES.length - 1],
        };
        remember(leverCache, key, run, 10);
        this.set({ run, stale: false, busy: !run.final, ...(run.final ? { progress: { lives, total, perSecond: lives / Math.max(0.05, (performance.now() - started) / 1000), paths } } : {}) });
      }
    } catch (error) {
      if (signal.aborted || isAbort(error)) return;
      this.set({ busy: false, progress: null, error: toEngineError(error).message });
    }
  }
}

// ---- trade-off map ------------------------------------------------------------------

export interface GridCell {
  /** Lives behind this cell. */
  paths: number;
  levels: Levels;
}

export interface GridRun {
  key: string;
  seed: number;
  x: VarId;
  y: VarId;
  xs: number[];
  ys: number[];
  /** `cells[yi][xi]`; null until measured, and for ever where the combination is not a valid plan. */
  cells: (GridCell | null)[][];
  invalid: boolean[][];
  /** Passes completed. */
  stage: number;
}

export interface GridSweepState {
  run: GridRun | null;
  stale: boolean;
  busy: boolean;
  progress: Progress | null;
  error: string | null;
}

const gridCache = new Map<string, GridRun>();

export class GridSweeper extends Store<GridSweepState> {
  private controller: AbortController | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private wanted: string | null = null;

  constructor(private readonly pool: EnginePool) {
    super({ run: null, stale: false, busy: false, progress: null, error: null });
  }

  request(profile: Profile, seed: number, x: VarDef, y: VarDef) {
    const key = `${engineInput(profile, seed).key}|${x.id}|${y.id}`;
    if (key === this.wanted) return;
    this.stop();
    this.wanted = key;
    const cached = gridCache.get(key);
    if (cached && cached.stage >= GRID_STAGES.length) return this.set({ run: cached, stale: false, busy: false, progress: null, error: null });
    const previous = this.state.run;
    // A different pair of axes is a different picture: do not hold the old one.
    const keep = cached ?? (previous && previous.x === x.id && previous.y === y.id ? previous : null);
    this.set({ run: keep, stale: !cached && keep !== null, busy: true, progress: null, error: null });
    const controller = (this.controller = new AbortController());
    this.timer = setTimeout(() => void this.sweep(key, profile, seed, x, y, cached ?? null, controller.signal), cached || !keep ? 0 : SETTLE_MS);
  }

  stop() {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.controller?.abort();
    this.controller = null;
    this.wanted = null;
    if (this.state.busy) this.set({ busy: false, progress: null });
  }

  private async sweep(key: string, profile: Profile, seed: number, x: VarDef, y: VarDef, from: GridRun | null, signal: AbortSignal) {
    try {
      const xs = x.values;
      const ys = y.values;
      const at = (yi: number, xi: number) => yi * xs.length + xi;
      const built = await buildVariants(profile, ys.flatMap((yv) => xs.map((xv) => [...x.ops(xv), ...y.ops(yv)])), signal);
      if (signal.aborted) return;
      const variant = (yi: number, xi: number) => {
        const b = built[at(yi, xi)];
        return b && 'profile' in b ? b.profile : null;
      };
      let run: GridRun = from ?? {
        key,
        seed,
        x: x.id,
        y: y.id,
        xs,
        ys,
        cells: ys.map(() => xs.map(() => null)),
        invalid: ys.map((_, yi) => xs.map((_, xi) => variant(yi, xi) === null)),
        stage: 0,
      };
      const plan = GRID_STAGES.map((stage, index) => {
        const cells: { yi: number; xi: number }[] = [];
        for (let yi = 0; yi < ys.length; yi += stage.stride) for (let xi = 0; xi < xs.length; xi += stage.stride) if (variant(yi, xi)) cells.push({ yi, xi });
        // Spread each pass over the whole map instead of filling it row by row.
        const stepBy = [37, 31, 29, 23, 19, 17, 13, 11, 7].find((s) => cells.length % s !== 0) ?? 1;
        const order = cells.map((_, i) => cells[(i * stepBy) % cells.length] as { yi: number; xi: number });
        return { ...stage, index, cells: order };
      }).filter((s) => s.index >= run.stage);
      const total = plan.reduce((n, s) => n + s.cells.length * s.paths, 0);
      const started = performance.now();
      let lives = 0;
      const progress = (paths: number): Progress => ({ lives, total, perSecond: lives / Math.max(0.05, (performance.now() - started) / 1000), paths });
      for (const stage of plan) {
        this.set({ run, stale: false, progress: progress(stage.paths) });
        for (let i = 0; i < stage.cells.length; i += BATCH) {
          const batch = stage.cells.slice(i, i + BATCH);
          const results = await this.pool.runMany(
            batch.map((c): ManyJob => ({ id: `${c.yi}:${c.xi}`, profile: variant(c.yi, c.xi) as Profile, paths: stage.paths })),
            { seed, signal, progressive: false },
          );
          if (signal.aborted) return;
          const cells = run.cells.map((row) => [...row]);
          for (const c of batch) {
            const r = results.get(`${c.yi}:${c.xi}`);
            if (r) (cells[c.yi] as (GridCell | null)[])[c.xi] = { paths: stage.paths, levels: readLevels(r.summary.headline) };
          }
          lives += batch.length * stage.paths;
          run = { ...run, cells };
          this.set({ run, stale: false, progress: progress(stage.paths) });
        }
        run = { ...run, stage: stage.index + 1 };
        remember(gridCache, key, run, 12);
        this.set({ run, busy: stage.index + 1 < GRID_STAGES.length });
      }
    } catch (error) {
      if (signal.aborted || isAbort(error)) return;
      this.set({ busy: false, progress: null, error: toEngineError(error).message });
    }
  }
}

// ---- solver -------------------------------------------------------------------------

export interface SolverState {
  status: 'idle' | 'running' | 'done' | 'error';
  /** The plan + seed the search ran on; compare with the current one to tell when it is out of date. */
  planKey: string | null;
  goal: Goal | null;
  variable: VarDef | null;
  probes: Probe[];
  outcome: SolveOutcome | null;
  error: string | null;
  lives: number;
  perSecond: number;
}

const probeCache = new Map<string, Levels>();

export class Solver extends Store<SolverState> {
  private controller: AbortController | null = null;

  constructor(private readonly pool: EnginePool) {
    super({ status: 'idle', planKey: null, goal: null, variable: null, probes: [], outcome: null, error: null, lives: 0, perSecond: 0 });
  }

  stop() {
    this.controller?.abort();
    this.controller = null;
    if (this.state.status === 'running') this.set({ status: this.state.probes.length > 0 ? 'error' : 'idle', error: this.state.probes.length > 0 ? 'The search was stopped before it finished.' : null });
  }

  async start(profile: Profile, seed: number, goal: Goal, variable: VarDef) {
    this.stop();
    if (!variable.bounds) return;
    const controller = (this.controller = new AbortController());
    const signal = controller.signal;
    const planKey = engineInput(profile, seed).key;
    const probes: Probe[] = [];
    const started = performance.now();
    let lives = 0;
    this.set({ status: 'running', planKey, goal, variable, probes, outcome: null, error: null, lives: 0, perSecond: 0 });
    try {
      for (;;) {
        const step = nextProbe(probes, variable.bounds, variable.tolerance, variable.round, goal);
        if ('outcome' in step) return this.set({ status: 'done', outcome: step.outcome });
        const x = step.x;
        const cacheKey = `${planKey}|${variable.id}|${x}`;
        let levels = probeCache.get(cacheKey);
        if (!levels) {
          const r = applyPatch(profile, variable.ops(x));
          if (r.errors.length > 0) throw new Error(`${variable.label} = ${variable.format(x)} is not a valid plan: ${r.errors[0]?.message ?? 'validation failed'}`);
          const result = await this.pool.run(r.profile, { paths: SOLVER_PATHS, seed }, { signal, progressive: false });
          if (signal.aborted) return;
          levels = readLevels(result.summary.headline);
          remember(probeCache, cacheKey, levels, 300);
          lives += SOLVER_PATHS;
        }
        const level = levels[goal.metric];
        probes.push({ order: probes.length + 1, x, level, met: goal.met(level.value) });
        this.set({ probes: [...probes], lives, perSecond: lives / Math.max(0.05, (performance.now() - started) / 1000) });
      }
    } catch (error) {
      if (signal.aborted || isAbort(error)) return;
      this.set({ status: 'error', error: toEngineError(error).message });
    }
  }
}

/** Identifies the compiled plan and seed a result belongs to. */
export const planKeyOf = (profile: Profile, seed: number) => engineInput(profile, seed).key;
