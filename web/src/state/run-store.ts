// The live simulation: re-runs (debounced) whenever the plan, an enabled scenario or the
// run settings change. The first chunks give a quick picture within moments; the same
// run then refines up to the chosen path count, so no work is thrown away.
import { create } from 'zustand';
import { getPool, isAbort, toEngineError, type EngineErrorKind } from '@/engine';
import type { Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { useProfileStore } from './profile-store';

export type RunStatus =
  /** No plan is open. */
  | 'idle'
  /** The plan has validation errors and cannot be simulated. */
  | 'blocked'
  /** A change was made; waiting for the debounce. */
  | 'queued'
  | 'running'
  | 'done'
  | 'error';

export interface ScenarioRun {
  id: string;
  name: string;
  profile: Profile;
  summary: Summary | null;
  done: number;
}

export interface RunState {
  status: RunStatus;
  error: { kind: EngineErrorKind; message: string } | null;
  done: number;
  total: number;
  pathsPerSecond: number;
  seed: number;
  /** Baseline result. Kept from the previous run (see `stale`) until the new one reports. */
  summary: Summary | null;
  /** Paths behind `summary`. */
  summaryPaths: number;
  /** True while `summary` still describes the plan before the latest edit. */
  stale: boolean;
  /** Enabled scenarios, simulated on the same seed and path indices as the baseline. */
  scenarios: ScenarioRun[];
  elapsedMs: number;
}

const initial: RunState = {
  status: 'idle',
  error: null,
  done: 0,
  total: 0,
  pathsPerSecond: 0,
  seed: 0,
  summary: null,
  summaryPaths: 0,
  stale: false,
  scenarios: [],
  elapsedMs: 0,
};

export const useRunStore = create<RunState>(() => initial);

const DEBOUNCE_MS = 250;
let controller: AbortController | null = null;
let timer: ReturnType<typeof setTimeout> | null = null;

function start() {
  controller?.abort();
  controller = null;
  const s = useProfileStore.getState();
  if (!s.profile) return useRunStore.setState(initial);
  if (s.report.errors.length > 0) {
    return useRunStore.setState({ status: 'blocked', error: null, done: 0, total: 0, stale: true });
  }

  const scenarios: ScenarioRun[] = s.scenarios.flatMap((sc) => {
    const r = s.resolved[sc.id];
    if (!sc.enabled || !r?.profile || (r.report?.errors.length ?? 0) > 0) return [];
    return [{ id: sc.id, name: sc.name, profile: r.profile, summary: null, done: 0 }];
  });
  const { paths, seed } = s.run;
  const jobs = [{ id: 'base', profile: s.profile, paths }, ...scenarios.map((x) => ({ id: x.id, profile: x.profile, paths }))];
  const total = paths * jobs.length;
  const mine = (controller = new AbortController());
  const startedAt = performance.now();
  const doneBy = new Map<string, number>();

  useRunStore.setState((prev) => ({
    status: 'running',
    error: null,
    done: 0,
    total,
    seed,
    pathsPerSecond: 0,
    stale: prev.summary !== null,
    scenarios: scenarios.map((x) => ({ ...x, summary: prev.scenarios.find((p) => p.id === x.id)?.summary ?? null })),
  }));

  getPool()
    .runMany(jobs, {
      seed,
      priority: 1,
      signal: mine.signal,
      onProgress: (id, p) => {
        if (mine.signal.aborted) return;
        doneBy.set(id, p.done);
        const done = [...doneBy.values()].reduce((a, b) => a + b, 0);
        const elapsedMs = performance.now() - startedAt;
        useRunStore.setState((prev) => ({
          done,
          elapsedMs,
          pathsPerSecond: (done / Math.max(elapsedMs, 1)) * 1000,
          ...(id === 'base'
            ? { summary: p.summary, summaryPaths: p.done, stale: false }
            : { scenarios: prev.scenarios.map((x) => (x.id === id ? { ...x, summary: p.summary, done: p.done } : x)) }),
        }));
      },
    })
    .then(() => {
      if (!mine.signal.aborted) useRunStore.setState({ status: 'done', elapsedMs: performance.now() - startedAt });
    })
    .catch((error: unknown) => {
      if (mine.signal.aborted || isAbort(error)) return;
      const e = toEngineError(error);
      useRunStore.setState({ status: 'error', error: { kind: e.kind, message: e.message } });
    });
}

function schedule() {
  if (timer) clearTimeout(timer);
  controller?.abort();
  controller = null;
  if (useProfileStore.getState().profile) useRunStore.setState({ status: 'queued' });
  timer = setTimeout(start, DEBOUNCE_MS);
}

/** Runs the current plan again from scratch (same seed, so the same result). */
export function rerun() {
  if (timer) clearTimeout(timer);
  start();
}

/** Wires the run to the profile store. Call once at startup. */
export function startRunController() {
  useProfileStore.subscribe((s, p) => {
    if (s.profile === p.profile && s.run === p.run && s.report === p.report && s.meta === p.meta) {
      // Scenario changes matter only when they alter what is simulated.
      const key = (x: typeof s) =>
        x.scenarios.filter((sc) => sc.enabled).map((sc) => `${sc.id}:${JSON.stringify(sc.patch)}`).join('|');
      if (s.scenarios === p.scenarios || key(s) === key(p)) return;
    }
    if (!s.profile) {
      if (timer) clearTimeout(timer);
      controller?.abort();
      controller = null;
      useRunStore.setState(initial);
      return;
    }
    if (s.meta !== p.meta) useRunStore.setState({ ...initial });
    schedule();
  });
  if (useProfileStore.getState().profile) schedule();
}
