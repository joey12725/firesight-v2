// A minimal router: the active results view is the first path segment; a view may keep
// its own selection in the query string (Life stories: `?path=1234&why=median`).
import { useSyncExternalStore } from 'react';
import { DEFAULT_VIEW, isViewId } from '@/views/registry';

const listeners = new Set<() => void>();

function current(): string {
  const segment = window.location.pathname.split('/').filter(Boolean)[0] ?? '';
  return isViewId(segment) ? segment : DEFAULT_VIEW;
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  window.addEventListener('popstate', listener);
  return () => {
    listeners.delete(listener);
    window.removeEventListener('popstate', listener);
  };
}

export function useActiveView(): string {
  return useSyncExternalStore(subscribe, current);
}

export function navigateToView(id: string) {
  if (id === current()) return;
  const path = id === DEFAULT_VIEW ? '/' : `/${id}`;
  // A view's own query parameters do not carry to another view.
  window.history.pushState(null, '', path + window.location.hash);
  listeners.forEach((l) => l());
}

function param(name: string): string | null {
  return new URLSearchParams(window.location.search).get(name);
}

/** A query-string parameter of the current route; re-renders when navigation changes it. */
export function useRouteParam(name: string): string | null {
  return useSyncExternalStore(subscribe, () => param(name));
}

/** Why a life was picked, shown with it so no single life is mistaken for a forecast. */
export type LifePick = 'p10' | 'median' | 'p90' | 'median_fi' | 'failing' | 'sample' | 'scatter';

/**
 * Opens one simulated life in Life stories: `/life-stories?path=<index>&why=<pick>`.
 * The path index means the same world in every chart and every scenario (same seed).
 */
export function navigateToLife(pathIndex: number, why: LifePick = 'sample') {
  const query = new URLSearchParams({ path: String(Math.round(pathIndex)), why });
  window.history.pushState(null, '', `/life-stories?${query.toString()}${window.location.hash}`);
  listeners.forEach((l) => l());
}

/** The life selected by `navigateToLife`, for the Life stories view. Null when none is. */
export function useSelectedLife(): { pathIndex: number; why: LifePick } | null {
  const path = useRouteParam('path');
  const why = useRouteParam('why');
  if (path === null || !/^\d+$/.test(path)) return null;
  return { pathIndex: Number(path), why: (why ?? 'sample') as LifePick };
}
