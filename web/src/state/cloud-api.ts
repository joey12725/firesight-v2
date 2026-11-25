// The website's own API for plans kept in an account (api/plans.ts): a thin, typed
// wrapper over fetch. The session cookie is the only credential; nothing here reads or
// stores a token. Every failure becomes a `CloudError` with a kind the sync code
// (state/cloud.ts) can act on.
//
// The cookie is shared by every tab, and this tab cannot read it. So each request also
// says which account this tab believes it is acting for (`x-firesight-user`). It is not
// a credential: the server still decides who is asking from the cookie alone, and
// refuses (401 `account_changed`) when the two differ, which is what happens after
// another tab signed out and someone else signed in. Without it this tab would take the
// other person's list of plans for its own.
import type { PlanFile } from './files';

/** A plan in the account, without its body. */
export interface RemoteMeta {
  id: string;
  name: string;
  rev: number;
  /** ISO 8601, UTC. */
  created_at: string;
  updated_at: string;
  /** 'web', 'apikey:<key name>' or 'oauth:<client name>'. */
  updated_by: string;
  bytes: number;
}

export interface RemotePlan extends RemoteMeta {
  plan: PlanFile;
}

export type CloudErrorKind =
  /** No session, it has ended, or it now belongs to a different account than this tab's (`account_changed`). */
  | 'unauthenticated'
  | 'not_found'
  /** The account holds a newer revision than the one this write was based on. */
  | 'conflict'
  | 'too_large'
  | 'too_many_plans'
  | 'invalid_plan'
  | 'rate_limited'
  /** The request never got an answer: no network, or the server is down. */
  | 'offline'
  /** Accounts are not set up on this site, or the server failed. */
  | 'unavailable';

export class CloudError extends Error {
  constructor(
    readonly kind: CloudErrorKind,
    message: string,
    /** For `conflict`: the plan as the account has it now. */
    readonly current?: RemoteMeta,
  ) {
    super(message);
    this.name = 'CloudError';
  }

  /** Worth trying again later without the person doing anything. */
  get retryable(): boolean {
    return this.kind === 'offline' || this.kind === 'unavailable' || this.kind === 'rate_limited';
  }
}

export interface PlansApi {
  list(): Promise<RemoteMeta[]>;
  get(id: string): Promise<RemotePlan>;
  create(plan: PlanFile, name?: string): Promise<RemoteMeta>;
  /** Writes only if the account is still at `rev`; otherwise a `conflict` error. */
  update(id: string, plan: PlanFile, rev: number, name?: string): Promise<RemoteMeta>;
  remove(id: string): Promise<void>;
  /** Everything the account holds, with plan bodies, as the server returns it. */
  exportAll(): Promise<unknown>;
}

/** An answer that has not come in this long is treated as no answer: syncing must not wait on it for ever. */
const TIMEOUT_MS = 30_000;

const KINDS: Record<number, CloudErrorKind> = { 401: 'unauthenticated', 404: 'not_found', 409: 'conflict', 413: 'too_large', 429: 'rate_limited' };

const MESSAGES: Record<CloudErrorKind, string> = {
  unauthenticated: 'You are signed out.',
  not_found: 'That plan is not in your account.',
  conflict: 'The plan in your account has changed since this copy was last synced.',
  too_large: 'The plan is too large to keep in an account.',
  too_many_plans: 'Your account already holds as many plans as it can. Delete one to make room.',
  invalid_plan: 'The server did not accept the plan.',
  rate_limited: 'Too many requests for a moment. Syncing will resume shortly.',
  offline: 'The server could not be reached.',
  unavailable: 'Accounts are not available on this site right now.',
};

async function failure(response: Response): Promise<CloudError> {
  let body: { error?: unknown; message?: unknown; current?: unknown } = {};
  try {
    body = (await response.json()) as typeof body;
  } catch {
    // Not JSON (a proxy's error page): the status alone decides.
  }
  const code = typeof body.error === 'string' ? body.error : '';
  const kind: CloudErrorKind =
    code === 'too_many_plans' ? 'too_many_plans' : code === 'invalid_plan' || code === 'bad_request' ? 'invalid_plan' : (KINDS[response.status] ?? 'unavailable');
  const current = kind === 'conflict' && body.current && typeof body.current === 'object' ? (body.current as RemoteMeta) : undefined;
  return new CloudError(kind, MESSAGES[kind], current);
}

/** The header that names the account a request is made for. Checked by api/plans.ts against the session. */
export const ACCOUNT_HEADER = 'x-firesight-user';

/**
 * The API over HTTP, on this origin. `account` gives the id of the account this tab is
 * signed in to, as it knows it; it is asked on every call. `fetchImpl` is injectable
 * for tests.
 */
export function httpPlansApi(account: () => string | null, fetchImpl: typeof fetch = (...args) => fetch(...args)): PlansApi {
  async function call(method: string, path: string, body?: unknown, headers: Record<string, string> = {}): Promise<Response> {
    let response: Response;
    const user = account();
    // Signed out as far as this tab knows: the request would be for nobody. Not sent.
    if (!user) throw new CloudError('unauthenticated', MESSAGES.unauthenticated);
    try {
      response = await fetchImpl(path, {
        method,
        credentials: 'same-origin',
        cache: 'no-store',
        headers: { accept: 'application/json', [ACCOUNT_HEADER]: user, ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...headers },
        body: body === undefined ? undefined : JSON.stringify(body),
        signal: AbortSignal.timeout(TIMEOUT_MS),
      });
    } catch {
      throw new CloudError('offline', MESSAGES.offline);
    }
    if (!response.ok) throw await failure(response);
    return response;
  }
  const json = async <T>(response: Response): Promise<T> => {
    // A static host answers unknown paths with the app shell: that is "no accounts here".
    if (!(response.headers.get('content-type') ?? '').includes('application/json')) throw new CloudError('unavailable', MESSAGES.unavailable);
    return (await response.json()) as T;
  };
  const at = (id: string) => `/api/plans/${encodeURIComponent(id)}`;
  return {
    list: async () => (await json<{ plans: RemoteMeta[] }>(await call('GET', '/api/plans'))).plans,
    get: async (id) => json<RemotePlan>(await call('GET', at(id))),
    create: async (plan, name) => json<RemoteMeta>(await call('POST', '/api/plans', name ? { plan, name } : { plan })),
    update: async (id, plan, rev, name) => json<RemoteMeta>(await call('PUT', at(id), name ? { plan, name } : { plan }, { 'if-match': String(rev) })),
    remove: async (id) => {
      await call('DELETE', at(id));
    },
    exportAll: async () => json<unknown>(await call('GET', '/api/plans?export=1')),
  };
}
