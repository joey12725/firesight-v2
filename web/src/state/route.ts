// A minimal router: the active results view is the first path segment.
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
  window.history.pushState(null, '', path + window.location.hash);
  listeners.forEach((l) => l());
}
