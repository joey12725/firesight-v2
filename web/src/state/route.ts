// A minimal router: the active results view is the first path segment; a view may keep
// its own selection in the query string (Life stories: `?path=1234&why=median`).
//
// A few paths are not views. `/login`, `/consent` and `/account` are the account pages,
// and `/p/<plan id>` opens a plan from the signed-in person's account (the link an AI
// agent hands back after it has written one).
import { useMemo, useSyncExternalStore } from 'react';
import { DEFAULT_VIEW, isViewId } from '@/views/registry';

const listeners = new Set<() => void>();

export type Page =
  | { kind: 'app' }
  | { kind: 'login' }
  | { kind: 'consent' }
  | { kind: 'account' }
  /** A plan in the account, optionally at one results view. */
  | { kind: 'plan'; id: string; view: string | null };

const APP: Page = { kind: 'app' };
const CLOUD_PLAN_ID = /^pl_[0-9A-Za-z]{22}$/;

/** Which page an address names. Anything that is not a reserved path is the app itself. */
export function parsePage(pathname: string, search = ''): Page {
  const segments = pathname.split('/').filter(Boolean);
  const [first, second] = segments;
  if (segments.length === 1 && (first === 'login' || first === 'consent' || first === 'account')) return { kind: first };
  if (segments.length === 2 && first === 'p' && second !== undefined && CLOUD_PLAN_ID.test(second)) {
    const view = new URLSearchParams(search).get('view');
    return { kind: 'plan', id: second, view: view !== null && isViewId(view) ? view : null };
  }
  return APP;
}

/**
 * Where to go after signing in. Only a path on this site is accepted: a `next` that
 * names another origin (`//evil.example`, `https://…`, a backslash trick) would turn
 * the login page into an open redirect, so it becomes `/`.
 */
export function safeNext(next: string | null | undefined, origin: string): string {
  if (!next || next[0] !== '/' || next[1] === '/' || next[1] === '\\') return '/';
  // eslint-disable-next-line no-control-regex -- control characters are what is being refused
  if (/[\u0000-\u001f\u007f\\]/.test(next)) return '/';
  let url: URL;
  try {
    url = new URL(next, origin);
  } catch {
    return '/';
  }
  if (url.origin !== origin) return '/';
  const page = parsePage(url.pathname).kind;
  // Back to the login or consent page would be a loop.
  if (page === 'login' || page === 'consent') return '/';
  return url.pathname + url.search + url.hash;
}

/** The login page's address, returning to `next` afterwards. */
export const loginPath = (next?: string): string => (next && next !== '/' ? `/login?next=${encodeURIComponent(next)}` : '/login');

/** The address of a plan in the account: what an agent gives the user, and what `/p/…` parses. */
export const planPath = (id: string, view?: string | null): string => `/p/${id}${view && view !== DEFAULT_VIEW ? `?view=${encodeURIComponent(view)}` : ''}`;

function current(): string {
  const segment = window.location.pathname.split('/').filter(Boolean)[0] ?? '';
  return isViewId(segment) ? segment : DEFAULT_VIEW;
}

/**
 * An address that names no view (a mistyped or stale link) shows the Overview, so the
 * address bar is put right to match: the app never sits on a path it does not know.
 * The plan in the fragment, if any, is kept. Call once at startup.
 */
export function normaliseLocation() {
  // The account pages and plan links are real addresses, and their query strings matter.
  if (parsePage(window.location.pathname, window.location.search).kind !== 'app') return;
  const segments = window.location.pathname.split('/').filter(Boolean);
  if (segments.length === 0 || (segments.length === 1 && isViewId(segments[0] as string))) return;
  const first = segments[0] as string;
  window.history.replaceState(null, '', (isViewId(first) && first !== DEFAULT_VIEW ? `/${first}` : '/') + window.location.hash);
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  window.addEventListener('popstate', listener);
  return () => {
    listeners.delete(listener);
    window.removeEventListener('popstate', listener);
  };
}

/** The active results view, outside React. */
export const activeView = (): string => current();

export function useActiveView(): string {
  return useSyncExternalStore(subscribe, current);
}

const locationKey = () => `${window.location.pathname}\n${window.location.search}`;

/** The page the address names; re-renders on navigation. */
export function usePage(): Page {
  const key = useSyncExternalStore(subscribe, locationKey);
  return useMemo(() => {
    const [pathname = '/', search = ''] = key.split('\n');
    return parsePage(pathname, search);
  }, [key]);
}

/** The page the address names, outside React. */
export const activePage = (): Page => parsePage(window.location.pathname, window.location.search);

/** Goes to a path on this site without reloading. The plan in the fragment, if any, is dropped. */
export function navigate(path: string, options: { replace?: boolean } = {}) {
  if (options.replace) window.history.replaceState(null, '', path);
  else window.history.pushState(null, '', path);
  listeners.forEach((l) => l());
}

export function navigateToView(id: string) {
  // From an account page the view may already be "current" while not being on screen.
  if (id === current() && activePage().kind === 'app') return;
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
