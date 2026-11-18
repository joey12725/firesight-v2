// Shared interaction state for the charts of one view: the focused position (the
// crosshair) and the visible time range (brush / zoom). It is an external store, so
// moving the pointer re-renders only the components that read the focus, never the view.
import { createContext, createElement, useContext, useMemo, useSyncExternalStore, type ReactNode } from 'react';
import type { TimeAxis } from './scales';

export interface ChartSync {
  /** Focused index (a year index on time charts), or null. */
  getFocus(): number | null;
  /** `source` identifies the chart the pointer or keyboard is on. */
  setFocus(index: number | null, source?: string): void;
  getSource(): string | null;
  /** Visible age range, or null for the whole plan. */
  getRange(): [number, number] | null;
  setRange(range: [number, number] | null): void;
  subscribe(listener: () => void): () => void;
}

export function createChartSync(): ChartSync {
  let focus: number | null = null;
  let source: string | null = null;
  let range: [number, number] | null = null;
  const listeners = new Set<() => void>();
  const emit = () => listeners.forEach((l) => l());
  return {
    getFocus: () => focus,
    getSource: () => source,
    setFocus(index, from) {
      if (index === focus && (from ?? null) === source) return;
      focus = index;
      source = index === null ? null : (from ?? null);
      emit();
    },
    getRange: () => range,
    setRange(next) {
      if (next === range || (next && range && next[0] === range[0] && next[1] === range[1])) return;
      range = next;
      emit();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}

const SyncContext = createContext<ChartSync | null>(null);

/** Charts inside share one crosshair and one time range. Put it around a view's charts. */
export function ChartSyncProvider({ children, sync }: { children: ReactNode; sync?: ChartSync }) {
  const own = useMemo(() => createChartSync(), []);
  return createElement(SyncContext.Provider, { value: sync ?? own }, children);
}

/** The enclosing sync store, or a private one when the chart stands alone. */
export function useChartSync(): ChartSync {
  const shared = useContext(SyncContext);
  const own = useMemo(() => createChartSync(), []);
  return shared ?? own;
}

/** The focused index; re-renders the caller when it changes. */
export function useFocus(sync: ChartSync): number | null {
  return useSyncExternalStore(sync.subscribe, sync.getFocus);
}

export function useFocusSource(sync: ChartSync): string | null {
  return useSyncExternalStore(sync.subscribe, sync.getSource);
}

export interface TimeRange {
  /** Visible ages, clamped to the axis. */
  domain: [number, number];
  /** First and last visible year index. */
  first: number;
  last: number;
  zoomed: boolean;
  set(range: [number, number] | null): void;
  reset(): void;
}

/** The view's visible time range applied to an axis. Every chart over time should use it for its x domain. */
export function useTimeRange(time: TimeAxis, sync: ChartSync): TimeRange {
  const range = useSyncExternalStore(sync.subscribe, sync.getRange);
  return useMemo(() => {
    const [lo, hi] = time.domain;
    const a = range ? Math.max(lo, Math.min(range[0], hi - 2)) : lo;
    const b = range ? Math.min(hi, Math.max(range[1], a + 2)) : hi;
    return {
      domain: [a, b],
      first: Math.max(0, Math.floor(a - lo)),
      last: Math.min(time.length - 1, Math.ceil(b - lo)),
      zoomed: range !== null && (a > lo || b < hi),
      set: (next) => sync.setRange(next),
      reset: () => sync.setRange(null),
    };
  }, [range, time, sync]);
}
