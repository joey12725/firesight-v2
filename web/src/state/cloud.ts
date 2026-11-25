// Syncing households with the signed-in person's account.
//
// Local-first stays the rule. A household lives in this browser; it reaches the server
// only when its owner turns on "Sync to account" for it, or when it came from the
// account in the first place (an AI agent they authorised wrote it there). A synced
// household is still a full local copy: the app keeps working offline, and what the
// account holds is caught up afterwards.
//
// This module is the whole of that logic and touches nothing but its dependencies, so
// it runs in tests with a fake API and a fake store. The wiring to the real ones is
// in cloud-store.ts.
//
// The rules:
//   push      a saved change is uploaded with `If-Match: <rev>`. If the account moved
//             on meanwhile the write is refused (a conflict) and the person chooses.
//   pull      when the account holds a newer revision and this copy has no changes of
//             its own, the newer one replaces it, open in the editor or not.
//   offline   a failed upload leaves the household marked dirty, in storage, and is
//             retried with a growing delay and whenever the list is refreshed.
//   sign-out  everything stops. Local copies stay, with their links, and are picked up
//             again only by the same account. The same goes for the server refusing a
//             request because the browser's session is no longer this account's (another
//             tab signed in as someone else): syncing stops before anything is unlinked
//             or uploaded, and the app is told to find out who is signed in now.
//   tabs      whether a household is linked is shared through storage. A tab whose link
//             was removed or replaced by another tab uploads nothing from the old one.
import { createStore } from 'zustand/vanilla';
import type { Profile } from '@/engine/types';
import { CloudError, type PlansApi, type RemoteMeta, type RemotePlan } from './cloud-api';
import type { PlanFile } from './files';
import type { HouseholdRecord, RemoteLink } from './persistence';
import { uniqueId } from './pointer';
import type { RunSettings, Scenario } from './profile-store';

export type SyncStatus =
  /** Level with the account. */
  | 'synced'
  /** Saved here, upload about to start. */
  | 'pending'
  | 'syncing'
  /** The last upload did not get through; it will be retried. */
  | 'offline'
  /** The account has a newer revision and this copy has changes too. */
  | 'conflict'
  /** The account refused this copy (too large, for one). Retried after the next change. */
  | 'error';

export interface CloudState {
  /** The signed-in account, or null: nothing syncs while it is null. */
  userId: string | null;
  /** The plans in the account. Null until the first answer. */
  remote: RemoteMeta[] | null;
  /** Whether the last look at the account got an answer. */
  reachable: boolean;
  /** By local household id, for synced households only. */
  status: Record<string, SyncStatus>;
  /** By local household id: the account's revision that a local change collided with. */
  conflicts: Record<string, RemoteMeta>;
  errors: Record<string, string>;
}

export type CloudEvent =
  /** A newer revision from the account replaced the local copy. */
  | { type: 'pulled'; localId: string; name: string; by: string; open: boolean }
  /** A plan appeared in the account that this browser did not put there. */
  | { type: 'new-remote'; meta: RemoteMeta }
  /** The plan was deleted from the account elsewhere; the local copy stays, unsynced. */
  | { type: 'unlinked'; localId: string; name: string; open: boolean }
  | { type: 'conflict'; localId: string; name: string }
  | { type: 'push-failed'; localId: string; name: string; message: string };

/** A plan from the account after the engine has checked and completed it. */
export interface PlanContent {
  profile: Profile;
  scenarios: Scenario[];
  run?: RunSettings;
}

export interface CloudDeps {
  api: PlansApi;
  /** Households stored in this browser. */
  local: {
    /** Reads a record after any pending writes have landed. */
    read(id: string): Promise<HouseholdRecord | undefined>;
    write(record: HouseholdRecord): Promise<void>;
    link(id: string): RemoteLink | undefined;
    links(): [string, RemoteLink][];
    setLink(id: string, link: RemoteLink | null): Promise<void>;
    newId(): string;
    /** Saves the open household's unsaved changes, if any. */
    flush(): Promise<void>;
  };
  /** The household open in the editor. */
  open: {
    id(): string | null;
    /** True when it has edits not yet saved to storage. */
    dirty(): boolean;
    /** Shows this record in place of what is open (same household id). */
    replace(record: HouseholdRecord): void;
  };
  /** Checks a plan from the account with the engine. Throws an Error for the person to read. */
  read(plan: PlanFile): PlanContent;
  emit(event: CloudEvent): void;
  /**
   * The server said there is no session, or that the session is not this account's any
   * more. Syncing has already stopped (as after `setUser(null)`) when this is called;
   * the app should ask who is signed in and call `setUser` with the answer.
   */
  unauthenticated(): void;
  now(): number;
}

export interface CloudOptions {
  /** How long after a save the upload starts: typing settles into one upload. */
  pushDelayMs: number;
  retryBaseMs: number;
  retryMaxMs: number;
}

const DEFAULTS: CloudOptions = { pushDelayMs: 800, retryBaseMs: 2_000, retryMaxMs: 60_000 };
const INITIAL: CloudState = { userId: null, remote: null, reachable: true, status: {}, conflicts: {}, errors: {} };
const UNTITLED = 'Untitled household';

/** Who wrote a revision, for a sentence: "Updated by …", "from …". */
export function describeWriter(updatedBy: string): string {
  if (updatedBy === 'web') return 'the website';
  if (updatedBy.startsWith('apikey:')) return `${updatedBy.slice(7) || 'an API'} key`;
  if (updatedBy.startsWith('oauth:')) return updatedBy.slice(6) || 'a connected app';
  return updatedBy || 'someone';
}

/** The stored household as the account keeps it. Run settings ride in the profile too, as in an exported file. */
export function toPlanFile(record: Pick<HouseholdRecord, 'profile' | 'scenarios' | 'run'>): PlanFile {
  return { firesight: 2, profile: { ...record.profile, simulation: { ...record.profile.simulation, ...record.run } }, scenarios: record.scenarios, run: record.run };
}

/** The id the app keeps for the plan itself: a scenario may not use it (state/run-store.ts keys results by it). */
const BASELINE_ID = 'base';

/**
 * The scenarios of a plan from the account, safe to load. An AI tool may have written
 * anything there: entries that are not scenarios are dropped, and one whose id is
 * missing, is the reserved `base`, or repeats an earlier one gets a new id, so that no
 * scenario's results can be taken for the plan's own or for another scenario's.
 */
export function safeScenarios(value: unknown): Scenario[] {
  if (!Array.isArray(value)) return [];
  const entries = (value as unknown[]).filter((entry): entry is Scenario => {
    if (entry === null || typeof entry !== 'object') return false;
    const s = entry as Record<string, unknown>;
    return typeof s.id === 'string' && typeof s.name === 'string' && Array.isArray(s.patch) && typeof s.enabled === 'boolean';
  });
  // The first scenario to use a usable id keeps it; ids are handed out to the rest only afterwards, so a new one never takes an id a later scenario has.
  const taken = new Set<string>([BASELINE_ID]);
  const keeps = entries.map((s) => {
    if (s.id === '' || taken.has(s.id)) return false;
    taken.add(s.id);
    return true;
  });
  return entries.map((s, i) => {
    if (keeps[i]) return s;
    const id = uniqueId(s.name || s.id || 'scenario', taken);
    taken.add(id);
    return { ...s, id };
  });
}

const without = <T>(map: Record<string, T>, key: string): Record<string, T> => {
  if (!(key in map)) return map;
  const { [key]: _gone, ...rest } = map;
  return rest;
};

export type CloudSync = ReturnType<typeof createCloudSync>;

export function createCloudSync(deps: CloudDeps, overrides: Partial<CloudOptions> = {}) {
  const options = { ...DEFAULTS, ...overrides };
  const store = createStore<CloudState>(() => INITIAL);
  /** Saves seen per household: an upload that finishes after a later save leaves it dirty. */
  const saves = new Map<string, number>();
  const pushing = new Set<string>();
  const pulling = new Set<string>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const attempts = new Map<string, number>();
  const adopting = new Map<string, Promise<string>>();
  let refreshing: Promise<void> | null = null;
  /** Bumped when the account changes: answers to calls made for the previous one are dropped. */
  let epoch = 0;

  const userId = () => store.getState().userId;
  const mine = (link: RemoteLink | undefined): link is RemoteLink => link !== undefined && link.userId === userId();

  function mark(localId: string, status: SyncStatus | null) {
    store.setState((s) => ({ status: status ? { ...s.status, [localId]: status } : without(s.status, localId) }));
  }
  function setError(localId: string, message: string | null) {
    store.setState((s) => ({ errors: message ? { ...s.errors, [localId]: message } : without(s.errors, localId) }));
  }
  function setConflict(localId: string, meta: RemoteMeta | null) {
    store.setState((s) => ({ conflicts: meta ? { ...s.conflicts, [localId]: meta } : without(s.conflicts, localId) }));
  }
  function putRemote(meta: RemoteMeta) {
    const { plan: _body, ...rest } = meta as RemoteMeta & { plan?: unknown };
    store.setState((s) => (s.remote ? { remote: s.remote.some((m) => m.id === rest.id) ? s.remote.map((m) => (m.id === rest.id ? rest : m)) : [rest, ...s.remote] } : s));
  }
  function dropRemote(remoteId: string) {
    store.setState((s) => (s.remote ? { remote: s.remote.filter((m) => m.id !== remoteId) } : s));
  }
  function cancel(localId: string) {
    const timer = timers.get(localId);
    if (timer) clearTimeout(timer);
    timers.delete(localId);
  }
  function forget(localId: string) {
    cancel(localId);
    attempts.delete(localId);
    mark(localId, null);
    setError(localId, null);
    setConflict(localId, null);
  }

  function schedulePush(localId: string, delay: number, status: SyncStatus = 'pending') {
    cancel(localId);
    mark(localId, status);
    timers.set(
      localId,
      setTimeout(() => {
        timers.delete(localId);
        void push(localId);
      }, delay),
    );
  }

  /** Stops everything, as signing out does. Answers to calls already made are dropped. */
  function halt() {
    epoch += 1;
    for (const timer of timers.values()) clearTimeout(timer);
    timers.clear();
    attempts.clear();
    adopting.clear();
  }

  /**
   * The server refused a request as not coming from this account. Syncing stops here,
   * before any household can be unlinked or uploaded on the strength of an answer meant
   * for someone else; then the app finds out who is signed in.
   */
  function sessionLost() {
    // Already stopped: the app has been told, and is finding out.
    if (userId() === null) return;
    halt();
    store.setState({ ...INITIAL });
    deps.unauthenticated();
  }
  const lostSession = (error: unknown) => error instanceof CloudError && error.kind === 'unauthenticated';

  const nameOf = async (localId: string) => (await deps.local.read(localId))?.name ?? UNTITLED;

  /** The account no longer has the plan: the local copy stays and stops syncing. */
  async function unlinkRemoved(localId: string, link: RemoteLink) {
    const name = await nameOf(localId);
    forget(localId);
    await deps.local.setLink(localId, null);
    dropRemote(link.id);
    deps.emit({ type: 'unlinked', localId, name, open: deps.open.id() === localId });
  }

  /**
   * Puts a revision from the account into a local household. `showOpen` false leaves the
   * editor alone even when that household is open (the caller is about to open another).
   */
  async function install(localId: string, doc: RemotePlan, showOpen = true): Promise<HouseholdRecord> {
    const content = deps.read(doc.plan);
    const existing = await deps.local.read(localId);
    const at = deps.now();
    const account = userId();
    if (!account) throw new CloudError('unauthenticated', 'You are signed out.');
    const record: HouseholdRecord = {
      id: localId,
      name: content.profile.name || doc.name || UNTITLED,
      origin: existing?.origin ?? 'account',
      createdAt: existing?.createdAt ?? (Date.parse(doc.created_at) || at),
      updatedAt: at,
      profile: content.profile,
      scenarios: content.scenarios,
      // A plan an agent wrote may carry no run settings: keep the ones chosen here.
      run: content.run ?? existing?.run ?? { paths: content.profile.simulation.paths, seed: content.profile.simulation.seed },
      remote: { id: doc.id, rev: doc.rev, syncedAt: at, updatedBy: doc.updated_by, userId: account, dirty: false },
    };
    if (showOpen && deps.open.id() === localId) deps.open.replace(record);
    await deps.local.write(record);
    putRemote(doc);
    cancel(localId);
    attempts.delete(localId);
    setConflict(localId, null);
    setError(localId, null);
    mark(localId, 'synced');
    return record;
  }

  async function push(localId: string): Promise<void> {
    const link = deps.local.link(localId);
    if (!mine(link) || store.getState().conflicts[localId]) return;
    if (pushing.has(localId) || pulling.has(localId)) {
      schedulePush(localId, options.pushDelayMs);
      return;
    }
    const started = epoch;
    const savesAtStart = saves.get(localId) ?? 0;
    pushing.add(localId);
    mark(localId, 'syncing');
    try {
      const record = await deps.local.read(localId);
      if (!record) {
        // Deleted here while an upload was waiting: nothing to send.
        mark(localId, null);
        return;
      }
      // Sync may have been turned off, or pointed at another plan, while this was waiting: in this
      // tab, or in another one (the stored record then no longer carries this link). Nothing is sent.
      const now = deps.local.link(localId);
      if (epoch !== started) return;
      if (!now || now.id !== link.id || now.userId !== link.userId || record.remote?.id !== link.id || record.remote.userId !== link.userId) {
        if (!mine(now) || record.remote === undefined) forget(localId);
        else mark(localId, now.dirty ? 'pending' : 'synced');
        return;
      }
      const meta = await deps.api.update(link.id, toPlanFile(record), link.rev, record.profile.name || undefined);
      if (epoch !== started) return;
      const current = deps.local.link(localId);
      // Unlinked while the upload was in flight (sync turned off): the answer is not wanted.
      if (!current || current.id !== link.id) return;
      const dirty = (saves.get(localId) ?? 0) !== savesAtStart;
      await deps.local.setLink(localId, { ...current, rev: meta.rev, syncedAt: deps.now(), updatedBy: meta.updated_by, dirty });
      putRemote(meta);
      attempts.delete(localId);
      setError(localId, null);
      if (dirty) schedulePush(localId, options.pushDelayMs);
      else mark(localId, 'synced');
    } catch (error) {
      if (epoch !== started) return;
      await pushFailed(localId, link, error);
    } finally {
      pushing.delete(localId);
    }
  }

  async function pushFailed(localId: string, link: RemoteLink, error: unknown) {
    const kind = error instanceof CloudError ? error.kind : null;
    if (kind === 'conflict') {
      const current = (error as CloudError).current ?? store.getState().remote?.find((m) => m.id === link.id);
      if (current) {
        setConflict(localId, current);
        putRemote(current);
      }
      mark(localId, 'conflict');
      deps.emit({ type: 'conflict', localId, name: await nameOf(localId) });
      return;
    }
    if (kind === 'not_found') {
      await unlinkRemoved(localId, link);
      return;
    }
    if (kind === 'unauthenticated') {
      sessionLost();
      return;
    }
    if (error instanceof CloudError && error.retryable) {
      const attempt = (attempts.get(localId) ?? 0) + 1;
      attempts.set(localId, attempt);
      schedulePush(localId, Math.min(options.retryBaseMs * 2 ** (attempt - 1), options.retryMaxMs), 'offline');
      return;
    }
    const message = error instanceof Error ? error.message : 'The plan could not be saved to your account.';
    mark(localId, 'error');
    setError(localId, message);
    deps.emit({ type: 'push-failed', localId, name: await nameOf(localId), message });
  }

  async function pull(localId: string, remoteId: string): Promise<void> {
    if (pulling.has(localId) || pushing.has(localId)) return;
    const started = epoch;
    pulling.add(localId);
    try {
      const doc = await deps.api.get(remoteId);
      if (epoch !== started) return;
      const link = deps.local.link(localId);
      if (!mine(link) || link.id !== remoteId || link.dirty || doc.rev <= link.rev) return;
      const open = deps.open.id() === localId;
      // An edit made while the newer revision was on its way wins the editor; the upload that follows will report the conflict.
      if (open && deps.open.dirty()) return;
      const record = await install(localId, doc);
      deps.emit({ type: 'pulled', localId, name: record.name, by: doc.updated_by, open });
    } catch (error) {
      if (epoch !== started) return;
      const link = deps.local.link(localId);
      if (error instanceof CloudError) {
        if (error.kind === 'not_found' && link) await unlinkRemoved(localId, link);
        else if (error.kind === 'unauthenticated') sessionLost();
        // Anything else: the next look at the account tries again.
      } else {
        // The engine could not read what the account holds. The local copy is left as it is.
        mark(localId, 'error');
        setError(localId, `The newer version in your account could not be read (${error instanceof Error ? error.message : String(error)}). This copy is unchanged.`);
      }
    } finally {
      pulling.delete(localId);
    }
  }

  /**
   * Looks at the account: lists its plans, pulls the ones that moved on, retries uploads
   * that are waiting, and reports plans that someone else put there. Safe to call often;
   * calls made while one is running share it.
   */
  function refresh(): Promise<void> {
    if (!userId()) return Promise.resolve();
    refreshing ??= (async () => {
      const started = epoch;
      const startedAt = deps.now();
      const account = userId();
      const previous = store.getState().remote;
      let list: RemoteMeta[];
      try {
        list = await deps.api.list();
      } catch (error) {
        if (epoch !== started) return;
        // Not this account's session any more: nothing in the list could be trusted, so nothing is unlinked.
        if (lostSession(error)) sessionLost();
        else store.setState({ reachable: false });
        return;
      }
      if (epoch !== started) return;
      store.setState({ remote: list, reachable: true });
      const byId = new Map(list.map((meta) => [meta.id, meta]));
      const linked = new Set<string>();
      for (const [localId, link] of deps.local.links()) {
        if (link.userId !== account) continue;
        linked.add(link.id);
        // Linked or written after the list was asked for: the list says nothing about it.
        if (link.syncedAt >= startedAt || pushing.has(localId) || pulling.has(localId)) continue;
        const meta = byId.get(link.id);
        if (!meta) {
          await unlinkRemoved(localId, link);
          continue;
        }
        const state = store.getState();
        const waiting = timers.has(localId) || Boolean(state.conflicts[localId]) || Boolean(state.errors[localId]);
        if (link.dirty) {
          // Changes made here and not yet accepted: try the upload. If the account moved on, it answers with the conflict.
          if (!waiting) void push(localId);
        } else if (meta.rev > link.rev && !(deps.open.id() === localId && deps.open.dirty())) {
          void pull(localId, meta.id);
        }
      }
      if (previous) {
        const known = new Set(previous.map((meta) => meta.id));
        for (const meta of list) {
          if (!known.has(meta.id) && !linked.has(meta.id) && meta.updated_by !== 'web') deps.emit({ type: 'new-remote', meta });
        }
      }
    })().finally(() => {
      refreshing = null;
    });
    return refreshing;
  }

  return {
    store,
    refresh,

    /** Call when the session changes. Null stops everything; an id starts syncing for that account. */
    setUser(id: string | null) {
      if (id === userId()) return;
      halt();
      store.setState({ ...INITIAL, userId: id });
      if (id) void refresh();
    },

    /** Call after a synced household has been saved to storage with new changes. */
    noteSaved(localId: string) {
      saves.set(localId, (saves.get(localId) ?? 0) + 1);
      if (!mine(deps.local.link(localId))) return;
      setError(localId, null);
      // With a conflict outstanding the person decides first; uploading again would only be refused again.
      if (store.getState().conflicts[localId]) return;
      attempts.delete(localId);
      schedulePush(localId, options.pushDelayMs);
    },

    /**
     * Call when another tab changed a household's link: sync turned off there (nothing
     * more is uploaded from here), or turned on (this tab follows it from now on).
     */
    noteLinkChanged(localId: string) {
      forget(localId);
      const link = deps.local.link(localId);
      if (mine(link)) mark(localId, link.dirty ? 'pending' : 'synced');
      void refresh();
    },

    /** Turns sync on for a local household: its current state becomes a new plan in the account. */
    async enable(localId: string): Promise<void> {
      const account = userId();
      if (!account) throw new CloudError('unauthenticated', 'Sign in to sync a plan to your account.');
      if (mine(deps.local.link(localId))) return;
      const started = epoch;
      await deps.local.flush();
      const record = await deps.local.read(localId);
      if (!record) throw new Error('This plan is not saved in this browser, so it cannot be synced. Check that the browser allows the site to store data.');
      mark(localId, 'syncing');
      let meta: RemoteMeta;
      try {
        meta = await deps.api.create(toPlanFile(record), record.profile.name || undefined);
      } catch (error) {
        mark(localId, null);
        if (lostSession(error)) sessionLost();
        throw error;
      }
      if (epoch !== started) return;
      // A change saved while the plan was on its way up is not in the account yet.
      const latest = await deps.local.read(localId);
      const dirty = latest !== undefined && latest.updatedAt !== record.updatedAt;
      await deps.local.setLink(localId, { id: meta.id, rev: meta.rev, syncedAt: deps.now(), updatedBy: meta.updated_by, userId: account, dirty });
      putRemote(meta);
      if (dirty) schedulePush(localId, options.pushDelayMs);
      else mark(localId, 'synced');
    },

    /** Turns sync off for a household. With `removeFromAccount` the account's copy is deleted; otherwise it stays there. */
    async disable(localId: string, removeFromAccount: boolean): Promise<void> {
      const link = deps.local.link(localId);
      if (!link) return;
      if (removeFromAccount && mine(link)) {
        try {
          await deps.api.remove(link.id);
        } catch (error) {
          if (!(error instanceof CloudError) || error.kind !== 'not_found') {
            if (lostSession(error)) sessionLost();
            throw error;
          }
        }
        dropRemote(link.id);
      }
      forget(localId);
      await deps.local.setLink(localId, null);
    },

    /** Deletes a plan from the account. Local copies of it, if any, stay and stop syncing. */
    async removeRemote(remoteId: string): Promise<void> {
      try {
        await deps.api.remove(remoteId);
      } catch (error) {
        if (!(error instanceof CloudError) || error.kind !== 'not_found') {
          if (lostSession(error)) sessionLost();
          throw error;
        }
      }
      dropRemote(remoteId);
      for (const [localId, link] of deps.local.links()) {
        if (link.id !== remoteId || !mine(link)) continue;
        forget(localId);
        await deps.local.setLink(localId, null);
      }
    },

    /**
     * Makes sure a plan from the account has a copy in this browser and returns that
     * household's id. One that is already here is reused; `refresh` keeps it current.
     */
    adopt(remoteId: string): Promise<string> {
      for (const [localId, link] of deps.local.links()) if (link.id === remoteId && mine(link)) return Promise.resolve(localId);
      const running = adopting.get(remoteId);
      if (running) return running;
      const started = epoch;
      const task = (async () => {
        if (!userId()) throw new CloudError('unauthenticated', 'Sign in to open a plan from your account.');
        let doc: RemotePlan;
        try {
          doc = await deps.api.get(remoteId);
        } catch (error) {
          if (lostSession(error)) sessionLost();
          throw error;
        }
        if (epoch !== started) throw new CloudError('unauthenticated', 'You are signed out.');
        const localId = deps.local.newId();
        await install(localId, doc);
        return localId;
      })().finally(() => adopting.delete(remoteId));
      adopting.set(remoteId, task);
      return task;
    },

    /**
     * Settles a conflict. `theirs` loads the account's version over this copy. `copy`
     * first keeps this copy as a separate, unsynced household. Returns the household that
     * should be open afterwards.
     */
    async resolve(localId: string, choice: 'theirs' | 'copy'): Promise<string> {
      const link = deps.local.link(localId);
      if (!mine(link)) {
        setConflict(localId, null);
        return localId;
      }
      const started = epoch;
      await deps.local.flush();
      let doc: RemotePlan;
      try {
        doc = await deps.api.get(link.id);
      } catch (error) {
        if (error instanceof CloudError && error.kind === 'not_found') {
          // Deleted from the account meanwhile: this copy is the only one left, and stays.
          await unlinkRemoved(localId, link);
          return localId;
        }
        if (lostSession(error)) sessionLost();
        throw error;
      }
      // The account changed while the answer was on its way: it is not this account's to install.
      if (epoch !== started) throw new CloudError('unauthenticated', 'You are signed out.');
      // Checked before anything is written: an unreadable account version must not cost the local one.
      deps.read(doc.plan);
      let openAfter = localId;
      if (choice === 'copy') {
        const record = await deps.local.read(localId);
        if (record) {
          const name = `${record.name} (my copy)`;
          const at = deps.now();
          openAfter = deps.local.newId();
          await deps.local.write({ ...record, id: openAfter, name, profile: { ...record.profile, name }, origin: 'copy', createdAt: at, updatedAt: at, remote: undefined });
        }
      }
      await install(localId, doc, openAfter === localId);
      return openAfter;
    },

    /** Stops timers. For tests and teardown. */
    stop() {
      epoch += 1;
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
    },
  };
}
