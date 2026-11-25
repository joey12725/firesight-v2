// Whether the app shows any account UI, decided from what the server answers. The
// signed-in path (which loads the sync code and needs a browser) is covered by
// e2e/accounts.mjs. What happens when the server refuses a plan request as not this
// account's is tested here, with the sync code replaced by a stand-in.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

/** The sync code as account.ts sees it: who it was told is signed in, and the handler it was given. */
const sync = vi.hoisted(() => ({ users: [] as (string | null)[], lost: null as (() => void) | null }));
vi.mock('@/state/cloud-store', () => ({
  startCloud: () => undefined,
  onSessionLost: (handler: () => void) => void (sync.lost = handler),
  cloud: { setUser: (id: string | null) => void sync.users.push(id) },
}));

const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
const html = () => new Response('<!doctype html><div id="root"></div>', { status: 200, headers: { 'content-type': 'text/html' } });

async function start(answer: (path: string) => Response | Promise<Response>) {
  const asked: string[] = [];
  vi.stubGlobal('fetch', async (input: RequestInfo | URL, init?: RequestInit) => {
    asked.push(String(input));
    expect(init?.headers).not.toHaveProperty('authorization');
    return answer(String(input));
  });
  const account = await import('./account');
  await account.startAccount();
  const agent = await import('@/webmcp/agent-store');
  return { asked, state: account.useAccount.getState(), account, signedInFlag: agent.useAgent.getState().signedIn };
}

beforeEach(() => {
  vi.resetModules();
  sync.users = [];
  sync.lost = null;
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe('startAccount', () => {
  it('is off, and asks nothing more, when /api/config is the app shell (a static host, or vite dev alone)', async () => {
    const t = await start(() => html());
    expect(t.state).toMatchObject({ status: 'off', config: null, user: null });
    expect(t.asked).toEqual(['/api/config']);
  });

  it('is off when the server says it has no accounts', async () => {
    const t = await start(() => json({ accounts: false, mcp_url: null, social: [], dcr: false }));
    expect(t.state.status).toBe('off');
    expect(t.asked).toEqual(['/api/config']);
  });

  it('is off when nothing answers or the answer is an error', async () => {
    expect((await start(() => Promise.reject(new TypeError('Failed to fetch')))).state.status).toBe('off');
    vi.resetModules();
    expect((await start(() => json({ error: 'server_error' }, 500))).state.status).toBe('off');
    vi.resetModules();
    expect((await start(() => new Response('not json', { status: 200, headers: { 'content-type': 'application/json' } }))).state.status).toBe('off');
  });

  it('is signed out, with the server configuration, when there is no session', async () => {
    const t = await start((path) => (path === '/api/config' ? json({ accounts: true, mcp_url: 'https://fs.example/api/mcp', social: ['github', 7], dcr: true }) : json(null)));
    expect(t.state).toMatchObject({ status: 'signed-out', user: null, config: { mcpUrl: 'https://fs.example/api/mcp', social: ['github'], dcr: true } });
    expect(t.asked).toEqual(['/api/config', '/api/auth/get-session']);
    expect(t.signedInFlag).toBe(false);
  });

  it('counts an unreachable session check at startup as signed out', async () => {
    const t = await start((path) => (path === '/api/config' ? json({ accounts: true, mcp_url: null, social: [], dcr: false }) : Promise.reject(new TypeError('Failed to fetch'))));
    expect(t.state).toMatchObject({ status: 'signed-out', user: null, config: { mcpUrl: null } });
  });

  it('runs once, however often it is called', async () => {
    const t = await start(() => json({ accounts: false }));
    await t.account.startAccount();
    await t.account.accountReady();
    expect(t.asked).toEqual(['/api/config']);
  });

  it('refreshSession does nothing on a site without accounts', async () => {
    const t = await start(() => html());
    expect(await t.account.refreshSession()).toBeNull();
    expect(t.asked).toEqual(['/api/config']);
  });
});

describe('a plan request refused as not this account’s', () => {
  const CONFIG = { accounts: true, mcp_url: null, social: [], dcr: false };
  const session = (id: string) => ({ user: { id, email: `${id}@example.com`, name: id }, session: {} });

  /** Signed in as u1, with the sync code following; `now` is what the server will say from here on. */
  async function signedIn(now: () => Response | Promise<Response>) {
    let switched = false;
    const t = await start((path) => (path === '/api/config' ? json(CONFIG) : switched ? now() : json(session('u1'))));
    await t.account.accountSync();
    expect(t.state).toMatchObject({ status: 'signed-in', user: { id: 'u1' } });
    expect(sync.users).toEqual(['u1']);
    switched = true;
    const lost = sync.lost as () => void;
    const state = () => t.account.useAccount.getState();
    return { ...t, lost, state, sessionChecks: () => t.asked.filter((path) => path === '/api/auth/get-session').length - 1 };
  }

  it('another account signed in from another tab: this tab becomes that account, and sync restarts for it alone', async () => {
    const t = await signedIn(() => json(session('u2')));
    t.lost();
    await vi.waitFor(() => expect(t.state().user?.id).toBe('u2'));
    await t.account.accountSync();
    expect(t.state()).toMatchObject({ status: 'signed-in', user: { id: 'u2', email: 'u2@example.com' } });
    expect(sync.users).toEqual(['u1', 'u2']);
  });

  it('nobody is signed in any more: signed out, and the sync code is told', async () => {
    const t = await signedIn(() => json(null));
    t.lost();
    await vi.waitFor(() => expect(t.state().status).toBe('signed-out'));
    await t.account.accountSync();
    expect(t.state().user).toBeNull();
    expect(sync.users).toEqual(['u1', null]);
  });

  it('no answer about who is signed in counts as signed out, not as the account that was just refused', async () => {
    const t = await signedIn(() => Promise.reject(new TypeError('Failed to fetch')));
    t.lost();
    await vi.waitFor(() => expect(t.state().status).toBe('signed-out'));
    await t.account.accountSync();
    expect(sync.users).toEqual(['u1', null]);
  });

  it('the same account refused is signed out rather than restarted in a loop', async () => {
    const t = await signedIn(() => json(session('u1')));
    t.lost();
    await vi.waitFor(() => expect(t.state().status).toBe('signed-out'));
    await t.account.accountSync();
    expect(sync.users).toEqual(['u1', null]);
    expect(t.sessionChecks()).toBe(1);
  });

  it('several refusals at once ask the server once', async () => {
    const t = await signedIn(() => json(session('u2')));
    t.lost();
    t.lost();
    t.lost();
    await vi.waitFor(() => expect(t.state().user?.id).toBe('u2'));
    await t.account.accountSync();
    expect(t.sessionChecks()).toBe(1);
    expect(sync.users).toEqual(['u1', 'u2']);
  });
});
