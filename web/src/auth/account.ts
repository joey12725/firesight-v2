// Whether this site has accounts, and who is signed in.
//
// Accounts are optional twice over. A deployment may not have them at all (no database
// behind /api): `GET /api/config` then says so, or is not there, and no account UI is
// shown anywhere. And where they exist nobody has to use one: signed out, the app is
// the local-first app it always was and makes no further request.
//
// The session is an HttpOnly cookie that this code cannot read. Nothing here, or
// anywhere in the app, keeps a token in localStorage.
import { create } from 'zustand';
import type * as CloudStore from '@/state/cloud-store';
import { toast } from '@/state/ui-store';
import { useAgent } from '@/webmcp/agent-store';

export interface AccountUser {
  id: string;
  email: string;
  name: string;
}

export interface AccountConfig {
  /** The hosted MCP endpoint for AI tools, when the server has one. */
  mcpUrl: string | null;
  /** Social sign-in providers the operator configured: 'github', 'google'. */
  social: string[];
  /** Whether MCP clients may register themselves (Dynamic Client Registration). */
  dcr: boolean;
}

interface AccountState {
  /**
   * `loading` until the server has answered; `off` when this site has no accounts
   * (the app then looks exactly as it does without this module).
   */
  status: 'loading' | 'off' | 'signed-out' | 'signed-in';
  config: AccountConfig | null;
  user: AccountUser | null;
}

export const useAccount = create<AccountState>(() => ({ status: 'loading', config: null, user: null }));

const TIMEOUT_MS = 8_000;

async function getJson(path: string): Promise<unknown> {
  const response = await fetch(path, { headers: { accept: 'application/json' }, credentials: 'same-origin', cache: 'no-store', signal: AbortSignal.timeout(TIMEOUT_MS) });
  // Under plain `vite dev` and on static hosts the path serves the app shell, not JSON.
  if (!response.ok || !(response.headers.get('content-type') ?? '').includes('application/json')) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

// The sync code (state/cloud-store.ts) is fetched the first time someone is signed in.
// A visitor who never signs in never downloads it, and nothing polls on their behalf.
let syncCode: Promise<typeof CloudStore> | null = null;
let following: Promise<typeof CloudStore | null> = Promise.resolve(null);

async function followAccount(): Promise<typeof CloudStore | null> {
  if (!syncCode && !useAccount.getState().user) return null;
  syncCode ??= import('@/state/cloud-store');
  const sync = await syncCode.catch(() => {
    // The chunk did not load (offline). The next change of session tries again.
    syncCode = null;
    return null;
  });
  if (!sync) return null;
  sync.startCloud();
  sync.onSessionLost(() => void sessionLost());
  // Read now, not when this started: the session may have changed while the code loaded.
  sync.cloud.setUser(useAccount.getState().user?.id ?? null);
  return sync;
}

let finding: Promise<void> | null = null;

/**
 * The server refused a plan request: the session has ended, or the browser's cookie now
 * belongs to someone else (another tab signed out and a different account signed in).
 * The sync code has already stopped. Ask who is signed in now and carry on as them: a
 * different account starts clean, and plans the previous one synced are left alone.
 */
function sessionLost(): Promise<void> {
  finding ??= (async () => {
    const before = useAccount.getState().user;
    if (useAccount.getState().status !== 'signed-in' || !before) return;
    // No answer counts as signed out here: carrying on as the account that was just refused would only be refused again.
    const now = await readSession().catch(() => null);
    // Signed out, or in as someone else, by this tab itself while the answer was on its way.
    if (useAccount.getState().user !== before) return;
    if (now && now.id !== before.id) {
      apply(now);
      toast({
        tone: 'warn',
        title: `This browser is now signed in as ${now.email || now.name || 'a different account'}`,
        description: 'Another tab changed the account. Plans synced by the previous account stay in this browser and are not sent to this one.',
        duration: 12_000,
      });
      return;
    }
    // The same account, yet its request was refused: treated as signed out, not asked again in a loop.
    apply(null);
    toast({ tone: 'warn', title: 'You have been signed out', description: 'Synced plans stay in this browser. Sign in again to carry on syncing them.', duration: 12_000 });
  })().finally(() => {
    finding = null;
  });
  return finding;
}

/** The sync code, once it has been told who is signed in. Null when nobody has been, or it could not load. */
export const accountSync = (): Promise<typeof CloudStore | null> => following;

function apply(user: AccountUser | null) {
  useAccount.setState({ status: user ? 'signed-in' : 'signed-out', user });
  useAgent.setState({ signedIn: user !== null });
  following = followAccount();
}

/** Who the server says is signed in. Throws when it cannot be reached. */
async function readSession(): Promise<AccountUser | null> {
  const body = (await getJson('/api/auth/get-session')) as { user?: { id?: unknown; email?: unknown; name?: unknown } } | null;
  if (!body?.user || typeof body.user.id !== 'string') return null;
  return { id: body.user.id, email: typeof body.user.email === 'string' ? body.user.email : '', name: typeof body.user.name === 'string' ? body.user.name : '' };
}

/** Asks the server who is signed in. Call after signing in or out; a failure counts as signed out. */
export async function refreshSession(): Promise<AccountUser | null> {
  if (useAccount.getState().status === 'off') return null;
  let user: AccountUser | null = null;
  try {
    user = await readSession();
  } catch {
    // Unreachable: keep whoever was signed in. A plan request that comes back 401 settles it.
    if (useAccount.getState().status !== 'loading') return useAccount.getState().user;
  }
  apply(user);
  return user;
}

let starting: Promise<void> | null = null;

/**
 * Finds out whether this site has accounts and, if it does, who is signed in. Runs once;
 * the app does not wait for it to show the landing page or a local plan.
 */
export function startAccount(): Promise<void> {
  starting ??= (async () => {
    let config: AccountConfig | null = null;
    try {
      const body = (await getJson('/api/config')) as { accounts?: unknown; mcp_url?: unknown; social?: unknown; dcr?: unknown };
      if (body.accounts === true) {
        config = {
          mcpUrl: typeof body.mcp_url === 'string' ? body.mcp_url : null,
          social: Array.isArray(body.social) ? body.social.filter((p): p is string => typeof p === 'string') : [],
          dcr: body.dcr === true,
        };
      }
    } catch {
      // No /api/config: a static deployment, or `vite dev` with no API server beside it.
    }
    if (!config) {
      useAccount.setState({ status: 'off', config: null, user: null });
      return;
    }
    useAccount.setState({ config });
    await refreshSession();
  })();
  return starting;
}

/** Resolves once it is known whether anyone is signed in. */
export const accountReady = (): Promise<void> => startAccount();

/** Ends the session on the server and stops syncing. Plans already in this browser stay in it. */
export async function signOut(): Promise<void> {
  try {
    await fetch('/api/auth/sign-out', { method: 'POST', credentials: 'same-origin', headers: { 'content-type': 'application/json' }, body: '{}' });
  } catch {
    // Offline: the cookie outlives this, so say what happened below by asking again.
  }
  const user = await refreshSession();
  if (user) toast({ tone: 'bad', title: 'Could not sign out', description: 'The server could not be reached. You are still signed in on this browser.' });
}

/** After the account has been deleted: the session is gone with it. */
export function accountDeleted() {
  apply(null);
}
