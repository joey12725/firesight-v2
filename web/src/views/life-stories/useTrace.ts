// Fetches one life's full ledger from the engine pool. Traces are deterministic (the
// same plan, seed and path index always give the same life) and cost a few
// milliseconds, so they are cached by (plan hash, path) and the previous life stays on
// screen, dimmed, while the next one arrives.
import { useCallback, useEffect, useMemo, useState } from 'react';
import { engineInput, type EnginePool } from '@/engine';
import type { PathTrace } from '@/engine/output-types';
import type { Profile } from '@/engine/types';

const cache = new Map<string, Promise<PathTrace>>();
const LIMIT = 48;

function load(pool: EnginePool, profile: Profile, seed: number, key: string, path: number): Promise<PathTrace> {
  const id = `${key}:${path}`;
  let hit = cache.get(id);
  if (!hit) {
    hit = pool.trace(profile, path, seed);
    cache.set(id, hit);
    hit.catch(() => cache.delete(id));
    if (cache.size > LIMIT) cache.delete(cache.keys().next().value as string);
  } else {
    // Most recently used last.
    cache.delete(id);
    cache.set(id, hit);
  }
  return hit;
}

export interface TraceState {
  /** The latest trace that has arrived; it may still be the previous life while `loading`. */
  trace: PathTrace | null;
  /** The path index `trace` belongs to. */
  path: number | null;
  loading: boolean;
  error: string | null;
  retry: () => void;
}

export function useTrace(pool: EnginePool, profile: Profile, seed: number, path: number | null): TraceState {
  const key = useMemo(() => engineInput(profile, seed).key, [profile, seed]);
  const [state, setState] = useState<{ id: string; trace: PathTrace | null; path: number | null; error: string | null }>({ id: '', trace: null, path: null, error: null });
  const [attempt, setAttempt] = useState(0);
  const id = path === null ? '' : `${key}:${path}`;

  useEffect(() => {
    if (path === null) return;
    let live = true;
    load(pool, profile, seed, key, path).then(
      (trace) => live && setState({ id, trace, path, error: null }),
      (error: unknown) => live && setState((s) => ({ ...s, id, error: error instanceof Error ? error.message : 'The engine could not replay this life.' })),
    );
    return () => {
      live = false;
    };
    // `profile` is represented by `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pool, key, seed, path, id, attempt]);

  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  return { trace: state.trace, path: state.path, loading: path !== null && state.id !== id, error: state.id === id ? state.error : null, retry };
}
