// The Equity view's one side-run hook: the plan and the plan with a patch applied, on
// the same seed and the same lives, then the engine's paired comparison (patched minus
// plan). It runs below the live run's priority at a reduced number of lives, and its
// result is cached on the plan and the patch, so leaving the view and coming back, or
// pressing Compare twice, costs nothing.
import { useCallback, useEffect, useRef, useState } from 'react';
import { applyPatch, engineInput, isAbort, type EnginePool } from '@/engine';
import type { Comparison, Summary } from '@/engine/output-types';
import type { PatchOp, Profile } from '@/engine/types';
import type { RunState } from '@/state/run-store';
import { sidePaths } from './model';

export interface PairedResult {
  /** The plan as it stands, on the side run's lives. */
  base: Summary;
  /** The plan with the patch applied, on the same lives. */
  scenario: Summary;
  /** Patched minus plan, life by life. */
  comparison: Comparison;
  paths: number;
}

export interface PairedRun {
  status: 'idle' | 'running' | 'done' | 'error';
  result: PairedResult | null;
  error: string | null;
  /** True when the result on show belongs to an earlier version of the plan. */
  outdated: boolean;
  start: () => void;
}

const CACHE_LIMIT = 12;
const cache = new Map<string, PairedResult>();

function remember(key: string, result: PairedResult) {
  cache.delete(key);
  cache.set(key, result);
  while (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value as string);
}

interface Settled {
  key: string;
  result: PairedResult | null;
  error: string | null;
}

/**
 * `patch` null means there is nothing to compare (the hook stays idle). With `auto` the
 * run starts as soon as the live run has finished and again whenever the plan changes;
 * otherwise only `start()` runs it.
 */
export function usePairedRun(profile: Profile, run: RunState, pool: EnginePool, patch: PatchOp[] | null, options: { auto?: boolean } = {}): PairedRun {
  const paths = sidePaths(run.total);
  const key = patch ? `${engineInput(profile, run.seed).key}:${paths}:${JSON.stringify(patch)}` : null;
  // The last comparison to finish (or fail), and the key a press of the button asked for.
  const [settled, setSettled] = useState<Settled | null>(null);
  const [requested, setRequested] = useState<string | null>(null);
  const controller = useRef<AbortController | null>(null);

  /** Starts the two runs. Nothing is set until they finish, so it is safe to call from an effect. */
  const launch = useCallback(() => {
    if (!patch || !key || cache.has(key)) return;
    controller.current?.abort();
    const abort = new AbortController();
    controller.current = abort;
    const fail = (error: unknown) => {
      if (isAbort(error) || abort.signal.aborted) return;
      setSettled({ key, result: null, error: error instanceof Error ? error.message : 'The comparison could not be run.' });
    };
    // Off the current call stack, so a patch that does not apply is reported like any other failure.
    Promise.resolve()
      .then(() => {
        const applied = applyPatch(profile, patch);
        if (applied.errors.length > 0) throw new Error(applied.errors[0]?.message ?? 'The change does not fit this plan.');
        return pool.compare(profile, applied.profile, { paths, seed: run.seed }, { signal: abort.signal, progressive: false });
      })
      .then((r) => {
        const result: PairedResult = { base: r.base.summary, scenario: r.scenario.summary, comparison: r.comparison, paths };
        remember(key, result);
        if (!abort.signal.aborted) setSettled({ key, result, error: null });
      })
      .catch(fail);
  }, [key, patch, paths, pool, profile, run.seed]);

  // Leaving the view abandons the work.
  useEffect(
    () => () => {
      controller.current?.abort();
      controller.current = null;
    },
    [],
  );

  // The standing comparison waits for the live run, so the two never compete for the workers.
  const ready = run.status === 'done' && !run.stale;
  const auto = options.auto === true;
  useEffect(() => {
    if (auto && ready) launch();
  }, [auto, ready, launch]);

  const start = useCallback(() => {
    setRequested(key);
    launch();
  }, [key, launch]);

  const cached = key ? (cache.get(key) ?? null) : null;
  if (cached) return { status: 'done', result: cached, error: null, outdated: false, start };
  const failed = settled !== null && settled.key === key && settled.error !== null;
  const wanted = key !== null && (auto ? ready : requested === key);
  // An earlier plan's result stays on show, marked, until this plan's own arrives.
  const earlier = settled?.result ?? null;
  return { status: failed ? 'error' : wanted ? 'running' : earlier ? 'done' : 'idle', result: earlier, error: failed ? settled.error : null, outdated: earlier !== null, start };
}
