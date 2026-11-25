// The account sync, against a fake API and a fake browser store: uploads, conflicts,
// newer revisions arriving, retrying when the server is unreachable, and signing out.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Profile } from '@/engine/types';
import { createCloudSync, describeWriter, safeScenarios, toPlanFile, type CloudEvent, type CloudSync } from './cloud';
import { ACCOUNT_HEADER, CloudError, httpPlansApi, type CloudErrorKind, type PlansApi, type RemoteMeta, type RemotePlan } from './cloud-api';
import type { PlanFile } from './files';
import type { HouseholdRecord, RemoteLink } from './persistence';

const USER = 'user_a';
const PUSH_DELAY = 800;

const profile = (name: string, salary = 100_000): Profile => ({ name, salary, simulation: { paths: 10_000, seed: 1 } }) as unknown as Profile;
const salaryOf = (p: Profile | PlanFile['profile']) => (p as unknown as { salary: number }).salary;

/** A stand-in for api/plans.ts: revisions, If-Match, and failures on demand. */
function fakeApi() {
  const plans = new Map<string, RemotePlan>();
  const calls: string[] = [];
  let nextId = 1;
  let failing: CloudErrorKind | null = null;
  const gate = () => {
    if (failing) throw new CloudError(failing, `fake ${failing}`);
  };
  const meta = (doc: RemotePlan): RemoteMeta => {
    const { plan: _plan, ...rest } = doc;
    return rest;
  };
  const store = (id: string, plan: PlanFile, by: string, name?: string): RemotePlan => {
    const before = plans.get(id);
    const at = new Date(Date.now()).toISOString();
    const doc: RemotePlan = { id, name: name ?? before?.name ?? (plan.profile.name || 'Untitled plan'), rev: (before?.rev ?? 0) + 1, created_at: before?.created_at ?? at, updated_at: at, updated_by: by, bytes: JSON.stringify(plan).length, plan: structuredClone(plan) };
    plans.set(id, doc);
    return doc;
  };
  const api: PlansApi = {
    async list() {
      calls.push('list');
      gate();
      return [...plans.values()].map(meta);
    },
    async get(id) {
      calls.push(`get ${id}`);
      gate();
      const doc = plans.get(id);
      if (!doc) throw new CloudError('not_found', 'gone');
      return structuredClone(doc);
    },
    async create(plan, name) {
      calls.push('create');
      gate();
      return meta(store(`pl_${String(nextId++).padStart(22, '0')}`, plan, 'web', name));
    },
    async update(id, plan, rev, name) {
      calls.push(`update ${id} @${rev}`);
      gate();
      const doc = plans.get(id);
      if (!doc) throw new CloudError('not_found', 'gone');
      if (doc.rev !== rev) throw new CloudError('conflict', 'stale', meta(doc));
      return meta(store(id, plan, 'web', name));
    },
    async remove(id) {
      calls.push(`remove ${id}`);
      gate();
      if (!plans.delete(id)) throw new CloudError('not_found', 'gone');
    },
    async exportAll() {
      return { plans: [...plans.values()] };
    },
  };
  return {
    api,
    plans,
    calls,
    /** Every call fails this way until cleared. */
    fail: (kind: CloudErrorKind | null) => void (failing = kind),
    /** An agent with an API key writes straight to the account. */
    agentWrites: (id: string, plan: PlanFile, by = 'apikey:Claude Code') => store(id, plan, by),
    agentCreates: (plan: PlanFile, by = 'apikey:Claude Code') => store(`pl_${String(nextId++).padStart(22, '0')}`, plan, by),
    uploads: () => calls.filter((c) => c.startsWith('update')),
  };
}

/** A stand-in for households.ts: records, and links owned by a map. */
function fakeBrowser() {
  const records = new Map<string, HouseholdRecord>();
  const links = new Map<string, RemoteLink>();
  /** What another tab has stored for a household's link since this one last looked (null: sync turned off there). */
  const elsewhere = new Map<string, RemoteLink | null>();
  let nextId = 1;
  const view = { openId: null as string | null, dirty: false, shown: [] as HouseholdRecord[] };
  const add = (name: string, salary = 100_000): string => {
    const id = `h_${nextId++}`;
    records.set(id, { id, name, origin: 'blank', createdAt: Date.now(), updatedAt: Date.now(), profile: profile(name, salary), scenarios: [], run: { paths: 10_000, seed: 1 } });
    return id;
  };
  return {
    records,
    links,
    elsewhere,
    view,
    add,
    /** What households.saveNow does for a synced household: write, mark the link dirty. */
    edit(id: string, salary: number) {
      const record = records.get(id) as HouseholdRecord;
      const link = links.get(id);
      if (link) links.set(id, { ...link, dirty: true });
      records.set(id, { ...record, updatedAt: Date.now(), profile: profile(record.name, salary), remote: links.get(id) });
    },
    local: {
      read: async (id: string) => {
        const record = records.get(id);
        return record && { ...record, remote: elsewhere.has(id) ? (elsewhere.get(id) ?? undefined) : links.get(id) };
      },
      write: async (record: HouseholdRecord) => {
        if (record.remote) links.set(record.id, record.remote);
        else links.delete(record.id);
        records.set(record.id, record);
      },
      link: (id: string) => links.get(id),
      links: () => [...links.entries()],
      setLink: async (id: string, link: RemoteLink | null) => {
        if (link) links.set(id, link);
        else links.delete(id);
      },
      newId: () => `h_${nextId++}`,
      flush: async () => {
        view.dirty = false;
      },
    },
    open: {
      id: () => view.openId,
      dirty: () => view.dirty,
      replace: (record: HouseholdRecord) => void view.shown.push(record),
    },
  };
}

function setup(apiFor?: (account: () => string | null) => PlansApi) {
  const server = fakeApi();
  const browser = fakeBrowser();
  const events: CloudEvent[] = [];
  let signedOut = 0;
  const sync: CloudSync = createCloudSync(
    {
      api: apiFor ? apiFor(() => sync.store.getState().userId) : server.api,
      local: browser.local,
      open: browser.open,
      read: (plan) => {
        if ((plan.profile as unknown as { broken?: boolean }).broken) throw new Error('unknown field');
        return { profile: plan.profile, scenarios: plan.scenarios, run: plan.run };
      },
      emit: (event) => void events.push(event),
      unauthenticated: () => void (signedOut += 1),
      now: () => Date.now(),
    },
    { pushDelayMs: PUSH_DELAY, retryBaseMs: 2_000, retryMaxMs: 60_000 },
  );
  const state = () => sync.store.getState();
  const wait = (ms = 1_000) => vi.advanceTimersByTimeAsync(ms);
  /** Signs in and lets the first look at the account finish. */
  const signIn = async (user = USER) => {
    sync.setUser(user);
    await wait(1);
  };
  /** A synced household, level with the account. */
  const synced = async (name = 'Ours', salary = 100_000) => {
    const id = browser.add(name, salary);
    await sync.enable(id);
    await wait();
    return { id, remoteId: (browser.links.get(id) as RemoteLink).id };
  };
  const save = (id: string, salary: number) => {
    browser.edit(id, salary);
    sync.noteSaved(id);
  };
  return { server, browser, events, sync, state, wait, signIn, synced, save, signedOut: () => signedOut };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-11-20T12:00:00Z'));
});
afterEach(() => {
  vi.useRealTimers();
});

describe('turning sync on and off', () => {
  it('uploads the household as a new plan and links it', async () => {
    const t = setup();
    await t.signIn();
    const id = t.browser.add('Ours', 120_000);
    await t.sync.enable(id);
    const link = t.browser.links.get(id) as RemoteLink;
    expect(link).toMatchObject({ rev: 1, userId: USER, updatedBy: 'web', dirty: false });
    expect(t.server.plans.get(link.id)?.plan).toEqual(toPlanFile(t.browser.records.get(id) as HouseholdRecord));
    expect(t.server.plans.get(link.id)?.name).toBe('Ours');
    expect(t.state().status[id]).toBe('synced');
    expect(t.state().remote?.map((m) => m.id)).toEqual([link.id]);
  });

  it('refuses when signed out and uploads nothing', async () => {
    const t = setup();
    const id = t.browser.add('Ours');
    await expect(t.sync.enable(id)).rejects.toMatchObject({ kind: 'unauthenticated' });
    expect(t.server.calls).toEqual([]);
    expect(t.browser.links.size).toBe(0);
  });

  it('reports the account being full without linking', async () => {
    const t = setup();
    await t.signIn();
    const id = t.browser.add('Ours');
    t.server.fail('too_many_plans');
    await expect(t.sync.enable(id)).rejects.toMatchObject({ kind: 'too_many_plans' });
    expect(t.browser.links.has(id)).toBe(false);
    expect(t.state().status[id]).toBeUndefined();
  });

  it('turning it off keeps the household here and, when asked, deletes the account copy', async () => {
    const t = setup();
    await t.signIn();
    const a = await t.synced('A');
    const b = await t.synced('B');
    await t.sync.disable(a.id, true);
    await t.sync.disable(b.id, false);
    expect(t.browser.links.size).toBe(0);
    expect(t.browser.records.has(a.id) && t.browser.records.has(b.id)).toBe(true);
    expect(t.server.plans.has(a.remoteId)).toBe(false);
    expect(t.server.plans.has(b.remoteId)).toBe(true);
    // No longer synced: a later save uploads nothing.
    t.save(a.id, 1);
    t.save(b.id, 1);
    await t.wait(5_000);
    expect(t.server.uploads()).toEqual([]);
  });
});

describe('push', () => {
  it('uploads a saved change with the revision it was based on', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.save(id, 130_000);
    expect(t.state().status[id]).toBe('pending');
    await t.wait(PUSH_DELAY + 10);
    expect(t.server.uploads()).toEqual([`update ${remoteId} @1`]);
    expect(salaryOf(t.server.plans.get(remoteId)?.plan.profile as Profile)).toBe(130_000);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 2, dirty: false });
    expect(t.state().status[id]).toBe('synced');
  });

  it('settles a burst of saves into one upload', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.save(id, 1);
    await t.wait(300);
    t.save(id, 2);
    await t.wait(300);
    t.save(id, 3);
    await t.wait(PUSH_DELAY + 10);
    expect(t.server.uploads()).toHaveLength(1);
    expect(salaryOf(t.server.plans.get(remoteId)?.plan.profile as Profile)).toBe(3);
  });

  it('a change saved while an upload is in flight is uploaded afterwards', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    const update = t.server.api.update;
    let release: () => void = () => undefined;
    t.server.api.update = async (...args) => {
      await new Promise<void>((resolve) => (release = resolve));
      return update(...args);
    };
    t.save(id, 1);
    await t.wait(PUSH_DELAY + 10);
    // The first upload is on its way with salary 1 when the next save lands.
    t.save(id, 2);
    t.server.api.update = update;
    release();
    await t.wait(1);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 2, dirty: true });
    await t.wait(PUSH_DELAY * 2 + 10);
    expect(t.server.uploads()).toEqual([`update ${remoteId} @1`, `update ${remoteId} @2`]);
    expect(salaryOf(t.server.plans.get(remoteId)?.plan.profile as Profile)).toBe(2);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 3, dirty: false });
  });

  it('a plan the account refuses is not retried until it changes', async () => {
    const t = setup();
    await t.signIn();
    const { id } = await t.synced();
    t.server.fail('too_large');
    t.save(id, 1);
    await t.wait(PUSH_DELAY + 10);
    expect(t.state().status[id]).toBe('error');
    expect(t.events.at(-1)).toMatchObject({ type: 'push-failed', localId: id });
    await t.wait(120_000);
    await t.sync.refresh();
    await t.wait(10);
    expect(t.server.uploads()).toHaveLength(1);
    t.server.fail(null);
    t.save(id, 2);
    await t.wait(PUSH_DELAY + 10);
    expect(t.state().status[id]).toBe('synced');
    expect(t.state().errors[id]).toBeUndefined();
  });
});

describe('conflict', () => {
  async function conflicted() {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced('Ours', 100_000);
    t.browser.view.openId = id;
    t.server.agentWrites(remoteId, toPlanFile({ profile: profile('Ours', 555), scenarios: [], run: { paths: 10_000, seed: 1 } }));
    t.save(id, 777);
    await t.wait(PUSH_DELAY + 10);
    return { t, id, remoteId };
  }

  it('a stale upload is refused, kept here, and nothing is overwritten', async () => {
    const { t, id, remoteId } = await conflicted();
    expect(t.state().status[id]).toBe('conflict');
    expect(t.state().conflicts[id]).toMatchObject({ rev: 2, updated_by: 'apikey:Claude Code' });
    expect(t.events.at(-1)).toMatchObject({ type: 'conflict', localId: id });
    expect(salaryOf(t.server.plans.get(remoteId)?.plan.profile as Profile)).toBe(555);
    expect(salaryOf((t.browser.records.get(id) as HouseholdRecord).profile)).toBe(777);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 1, dirty: true });
  });

  it('later saves and refreshes do not upload or pull until the person chooses', async () => {
    const { t, id } = await conflicted();
    const before = t.server.uploads().length;
    t.save(id, 888);
    await t.wait(30_000);
    await t.sync.refresh();
    await t.wait(10);
    expect(t.server.uploads()).toHaveLength(before);
    expect(t.browser.view.shown).toEqual([]);
    expect(salaryOf((t.browser.records.get(id) as HouseholdRecord).profile)).toBe(888);
  });

  it('"load the newer version" replaces the local copy, in the editor too', async () => {
    const { t, id } = await conflicted();
    expect(await t.sync.resolve(id, 'theirs')).toBe(id);
    expect(salaryOf((t.browser.records.get(id) as HouseholdRecord).profile)).toBe(555);
    expect(t.browser.view.shown.map((r) => salaryOf(r.profile))).toEqual([555]);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 2, dirty: false, updatedBy: 'apikey:Claude Code' });
    expect(t.state().conflicts[id]).toBeUndefined();
    expect(t.state().status[id]).toBe('synced');
    // Syncing carries on from the account's revision.
    t.save(id, 999);
    await t.wait(PUSH_DELAY + 10);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 3, dirty: false });
  });

  it('"keep mine as a copy" keeps both: mine as an unsynced household, theirs in the synced one', async () => {
    const { t, id, remoteId } = await conflicted();
    const copyId = await t.sync.resolve(id, 'copy');
    expect(copyId).not.toBe(id);
    const copy = t.browser.records.get(copyId) as HouseholdRecord;
    expect(copy.name).toBe('Ours (my copy)');
    expect(copy.profile.name).toBe('Ours (my copy)');
    expect(salaryOf(copy.profile)).toBe(777);
    expect(copy.remote).toBeUndefined();
    expect(t.browser.links.has(copyId)).toBe(false);
    expect(salaryOf((t.browser.records.get(id) as HouseholdRecord).profile)).toBe(555);
    expect(t.browser.links.get(id)).toMatchObject({ id: remoteId, rev: 2, dirty: false });
    // The editor is left alone: the caller opens the copy.
    expect(t.browser.view.shown).toEqual([]);
    expect(t.state().conflicts[id]).toBeUndefined();
  });

  it('an account version the engine cannot read costs nothing local', async () => {
    const { t, id, remoteId } = await conflicted();
    t.server.agentWrites(remoteId, { firesight: 2, profile: { ...profile('Ours'), broken: true } as unknown as Profile, scenarios: [] });
    await expect(t.sync.resolve(id, 'copy')).rejects.toThrow('unknown field');
    expect(t.browser.records.size).toBe(1);
    expect(salaryOf((t.browser.records.get(id) as HouseholdRecord).profile)).toBe(777);
    expect(t.state().conflicts[id]).toBeDefined();
  });
});

describe('a newer revision in the account', () => {
  it('replaces the open plan when it has no changes of its own, and says who wrote it', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced('Ours', 100_000);
    t.browser.view.openId = id;
    t.server.agentWrites(remoteId, toPlanFile({ profile: profile('Ours', 250_000), scenarios: [], run: { paths: 2_000, seed: 7 } }));
    await t.sync.refresh();
    await t.wait(10);
    expect(t.browser.view.shown.map((r) => salaryOf(r.profile))).toEqual([250_000]);
    const record = t.browser.records.get(id) as HouseholdRecord;
    expect(salaryOf(record.profile)).toBe(250_000);
    expect(record.run).toEqual({ paths: 2_000, seed: 7 });
    expect(record.origin).toBe('blank');
    expect(t.browser.links.get(id)).toMatchObject({ rev: 2, dirty: false, updatedBy: 'apikey:Claude Code' });
    expect(t.events).toContainEqual({ type: 'pulled', localId: id, name: 'Ours', by: 'apikey:Claude Code', open: true });
    // Loading it is not a local change: nothing is uploaded back.
    await t.wait(10_000);
    expect(t.server.uploads()).toEqual([]);
  });

  it('keeps the run settings chosen here when the account version has none', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.server.agentWrites(remoteId, { firesight: 2, profile: profile('Ours', 5), scenarios: [] });
    await t.sync.refresh();
    await t.wait(10);
    expect((t.browser.records.get(id) as HouseholdRecord).run).toEqual({ paths: 10_000, seed: 1 });
  });

  it('updates a copy that is not open without touching the editor', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.browser.view.openId = 'h_other';
    t.server.agentWrites(remoteId, toPlanFile({ profile: profile('Ours', 5), scenarios: [], run: { paths: 10_000, seed: 1 } }));
    await t.sync.refresh();
    await t.wait(10);
    expect(salaryOf((t.browser.records.get(id) as HouseholdRecord).profile)).toBe(5);
    expect(t.browser.view.shown).toEqual([]);
    expect(t.events).toContainEqual(expect.objectContaining({ type: 'pulled', open: false }));
  });

  it('does not replace an open plan with unsaved edits; the upload that follows reports the conflict', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.browser.view.openId = id;
    t.browser.view.dirty = true;
    t.server.agentWrites(remoteId, toPlanFile({ profile: profile('Ours', 5), scenarios: [], run: { paths: 10_000, seed: 1 } }));
    await t.sync.refresh();
    await t.wait(10);
    expect(t.browser.view.shown).toEqual([]);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 1 });
    t.browser.view.dirty = false;
    t.save(id, 42);
    await t.wait(PUSH_DELAY + 10);
    expect(t.state().status[id]).toBe('conflict');
  });

  it('an unreadable newer version leaves the local copy alone and says so', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced('Ours', 100_000);
    t.server.agentWrites(remoteId, { firesight: 2, profile: { ...profile('Ours'), broken: true } as unknown as Profile, scenarios: [] });
    await t.sync.refresh();
    await t.wait(10);
    expect(salaryOf((t.browser.records.get(id) as HouseholdRecord).profile)).toBe(100_000);
    expect(t.state().status[id]).toBe('error');
    expect(t.state().errors[id]).toContain('unknown field');
  });

  it('does not mistake its own upload for someone else’s change', async () => {
    const t = setup();
    await t.signIn();
    const { id } = await t.synced();
    t.save(id, 1);
    await t.wait(PUSH_DELAY + 10);
    await t.wait();
    await t.sync.refresh();
    await t.wait(10);
    expect(t.server.calls.filter((c) => c.startsWith('get'))).toEqual([]);
    expect(t.events).toEqual([]);
  });
});

describe('plans that come and go in the account', () => {
  it('announces a plan an agent created, once, and not on the first look', async () => {
    const t = setup();
    t.server.agentCreates(toPlanFile({ profile: profile('Already there'), scenarios: [], run: { paths: 1, seed: 1 } }));
    await t.signIn();
    expect(t.events).toEqual([]);
    const made = t.server.agentCreates(toPlanFile({ profile: profile('From the agent'), scenarios: [], run: { paths: 1, seed: 1 } }));
    await t.sync.refresh();
    await t.sync.refresh();
    expect(t.events).toEqual([{ type: 'new-remote', meta: expect.objectContaining({ id: made.id, name: 'From the agent', updated_by: 'apikey:Claude Code' }) }]);
    expect(t.state().remote).toHaveLength(2);
  });

  it('says nothing about a plan this account synced from another browser', async () => {
    const t = setup();
    await t.signIn();
    t.server.agentCreates(toPlanFile({ profile: profile('Other browser'), scenarios: [], run: { paths: 1, seed: 1 } }), 'web');
    await t.sync.refresh();
    expect(t.events).toEqual([]);
    expect(t.state().remote).toHaveLength(1);
  });

  it('adopts a plan from the account as a synced household, once', async () => {
    const t = setup();
    await t.signIn();
    const made = t.server.agentCreates(toPlanFile({ profile: profile('From the agent', 9), scenarios: [], run: { paths: 2_000, seed: 3 } }));
    const [first, second] = await Promise.all([t.sync.adopt(made.id), t.sync.adopt(made.id)]);
    expect(first).toBe(second);
    expect(t.server.calls.filter((c) => c.startsWith('get'))).toHaveLength(1);
    const record = t.browser.records.get(first) as HouseholdRecord;
    expect(record).toMatchObject({ name: 'From the agent', origin: 'account', run: { paths: 2_000, seed: 3 } });
    expect(record.remote).toMatchObject({ id: made.id, rev: 1, userId: USER, updatedBy: 'apikey:Claude Code', dirty: false });
    expect(await t.sync.adopt(made.id)).toBe(first);
    expect(t.browser.records.size).toBe(1);
  });

  it('adopting a plan that is not in the account fails and stores nothing', async () => {
    const t = setup();
    await t.signIn();
    await expect(t.sync.adopt('pl_0000000000000000000099')).rejects.toMatchObject({ kind: 'not_found' });
    expect(t.browser.records.size).toBe(0);
  });

  it('a plan deleted from the account elsewhere stays here, unsynced', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.browser.view.openId = id;
    t.server.plans.delete(remoteId);
    await t.sync.refresh();
    expect(t.browser.links.has(id)).toBe(false);
    expect(t.browser.records.has(id)).toBe(true);
    expect(t.events).toEqual([{ type: 'unlinked', localId: id, name: 'Ours', open: true }]);
    expect(t.state().remote).toEqual([]);
  });

  it('a household linked while the list was on its way is not taken for deleted', async () => {
    const t = setup();
    await t.signIn();
    await t.wait();
    const list = t.server.api.list;
    let release: () => void = () => undefined;
    t.server.api.list = async () => {
      const snapshot = await list();
      await new Promise<void>((resolve) => (release = resolve));
      return snapshot;
    };
    const looking = t.sync.refresh();
    await t.wait(5);
    const id = t.browser.add('Just synced');
    await t.sync.enable(id);
    release();
    await looking;
    expect(t.browser.links.has(id)).toBe(true);
    expect(t.events).toEqual([]);
  });

  it('deleting from the account unlinks the local copy and keeps it', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    await t.sync.removeRemote(remoteId);
    expect(t.server.plans.size).toBe(0);
    expect(t.browser.links.has(id)).toBe(false);
    expect(t.browser.records.has(id)).toBe(true);
    expect(t.state().remote).toEqual([]);
  });
});

describe('offline', () => {
  it('keeps the change, retries with a growing delay, and catches up', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.server.fail('offline');
    t.save(id, 42);
    await t.wait(PUSH_DELAY + 10);
    expect(t.state().status[id]).toBe('offline');
    expect(t.browser.links.get(id)).toMatchObject({ rev: 1, dirty: true });
    await t.wait(2_000);
    expect(t.server.uploads()).toHaveLength(2);
    // The second retry waits twice as long as the first.
    await t.wait(3_900);
    expect(t.server.uploads()).toHaveLength(2);
    await t.wait(200);
    expect(t.server.uploads()).toHaveLength(3);
    t.server.fail(null);
    await t.wait(8_000);
    expect(salaryOf(t.server.plans.get(remoteId)?.plan.profile as Profile)).toBe(42);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 2, dirty: false });
    expect(t.state().status[id]).toBe('synced');
  });

  it('a change left unsent by an earlier session is sent on the next look at the account', async () => {
    const first = setup();
    await first.signIn();
    const { id, remoteId } = await first.synced();
    first.server.fail('offline');
    first.save(id, 42);
    await first.wait(PUSH_DELAY + 10);
    first.sync.stop();

    // The page is reloaded: a new sync over the same storage and the same server.
    first.server.fail(null);
    const events: CloudEvent[] = [];
    const again = createCloudSync({ api: first.server.api, local: first.browser.local, open: first.browser.open, read: (plan) => ({ profile: plan.profile, scenarios: plan.scenarios, run: plan.run }), emit: (e) => void events.push(e), unauthenticated: () => undefined, now: () => Date.now() });
    await first.wait(1_000);
    again.setUser(USER);
    await first.wait(10);
    expect(salaryOf(first.server.plans.get(remoteId)?.plan.profile as Profile)).toBe(42);
    expect(first.browser.links.get(id)).toMatchObject({ rev: 2, dirty: false });
    expect(events).toEqual([]);
  });

  it('an unreachable list is not a sign-out and changes nothing', async () => {
    const t = setup();
    await t.signIn();
    const { id } = await t.synced();
    t.server.fail('offline');
    await t.sync.refresh();
    expect(t.state().reachable).toBe(false);
    expect(t.browser.links.has(id)).toBe(true);
    expect(t.signedOut()).toBe(0);
    t.server.fail(null);
    await t.sync.refresh();
    expect(t.state().reachable).toBe(true);
  });
});

describe('signing out', () => {
  it('stops uploads and polling, clears what was known of the account, and keeps local copies', async () => {
    const t = setup();
    await t.signIn();
    const { id } = await t.synced();
    t.save(id, 1);
    t.sync.setUser(null);
    await t.wait(60_000);
    await t.sync.refresh();
    expect(t.server.uploads()).toEqual([]);
    expect(t.state()).toMatchObject({ userId: null, remote: null, status: {}, conflicts: {} });
    expect(t.browser.records.has(id)).toBe(true);
    expect(t.browser.links.get(id)).toMatchObject({ userId: USER, dirty: true });
    // Saves made while signed out stay local.
    t.save(id, 2);
    await t.wait(60_000);
    expect(t.server.calls.filter((c) => c !== 'list' && c !== 'create')).toEqual([]);
  });

  it('signing back in as the same account sends what was held back', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.sync.setUser(null);
    t.save(id, 64);
    await t.wait(5_000);
    await t.signIn();
    await t.wait(10);
    expect(salaryOf(t.server.plans.get(remoteId)?.plan.profile as Profile)).toBe(64);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 2, dirty: false });
  });

  it('another account on the same browser leaves the first one’s households alone', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.sync.setUser(null);
    t.save(id, 64);
    // The other account's server view: none of the first one's plans.
    const kept = t.server.plans.get(remoteId) as RemotePlan;
    t.server.plans.clear();
    await t.signIn('user_b');
    await t.wait(30_000);
    expect(t.browser.links.get(id)).toMatchObject({ userId: USER, dirty: true });
    expect(t.server.uploads()).toEqual([]);
    expect(t.events).toEqual([]);
    // It can sync the household to itself; that is a new plan there.
    t.server.plans.set(remoteId, kept);
    await t.sync.enable(id);
    expect(t.browser.links.get(id)).toMatchObject({ userId: 'user_b' });
    expect((t.browser.links.get(id) as RemoteLink).id).not.toBe(remoteId);
  });

  it('an answer that arrives after signing out is dropped', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.browser.view.openId = id;
    t.server.agentWrites(remoteId, toPlanFile({ profile: profile('Ours', 5), scenarios: [], run: { paths: 10_000, seed: 1 } }));
    const get = t.server.api.get;
    let release: () => void = () => undefined;
    t.server.api.get = async (...args) => {
      const doc = await get(...args);
      await new Promise<void>((resolve) => (release = resolve));
      return doc;
    };
    await t.sync.refresh();
    await t.wait(5);
    t.sync.setUser(null);
    release();
    await t.wait(10);
    expect(t.browser.view.shown).toEqual([]);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 1 });
  });

  it('tells the app when the server says the session has ended, and stops by itself', async () => {
    const t = setup();
    await t.signIn();
    const { id } = await t.synced();
    t.server.fail('unauthenticated');
    await t.sync.refresh();
    expect(t.signedOut()).toBe(1);
    expect(t.state()).toMatchObject({ userId: null, remote: null, status: {} });
    // Nothing more is asked or sent until the app says who is signed in.
    t.save(id, 1);
    await t.wait(120_000);
    await t.sync.refresh();
    expect(t.signedOut()).toBe(1);
    expect(t.server.uploads()).toEqual([]);
    expect(t.browser.links.get(id)).toMatchObject({ userId: USER, dirty: true });
    // The same account signs in again: what was held back goes up.
    t.server.fail(null);
    await t.signIn();
    await t.wait(10);
    expect(t.server.uploads()).toHaveLength(1);
    expect(t.browser.links.get(id)).toMatchObject({ rev: 2, dirty: false });
  });

  it('an upload refused for want of a session stops everything too', async () => {
    const t = setup();
    await t.signIn();
    const { id } = await t.synced();
    t.save(id, 1);
    t.server.fail('unauthenticated');
    await t.wait(PUSH_DELAY + 10);
    expect(t.signedOut()).toBe(1);
    expect(t.state().userId).toBeNull();
    await t.wait(120_000);
    expect(t.server.uploads()).toHaveLength(1);
    expect(t.browser.links.get(id)).toMatchObject({ dirty: true });
  });
});

/**
 * A stand-in for api/plans.ts behind the browser's cookie, which every tab shares: it
 * answers for whoever the cookie names, and refuses a request that says it is made for
 * a different account (the `x-firesight-user` header).
 */
function cookieServer(signedIn: string | null) {
  const json = (status: number, body: unknown) => new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } });
  const accounts = new Map<string, Map<string, RemotePlan>>();
  const writes: string[] = [];
  let cookie = signedIn;
  let nextId = 1;
  const meta = ({ plan: _plan, ...rest }: RemotePlan): RemoteMeta => rest;
  const fetchImpl: typeof fetch = async (input, init) => {
    const method = init?.method ?? 'GET';
    const id = String(input).split('/')[3];
    const believed = new Headers(init?.headers).get(ACCOUNT_HEADER);
    if (!cookie) return json(401, { error: 'unauthenticated' });
    if (believed !== null && believed !== cookie) return json(401, { error: 'account_changed' });
    const plans = accounts.get(cookie) ?? new Map<string, RemotePlan>();
    accounts.set(cookie, plans);
    if (method !== 'GET') writes.push(`${method} as ${cookie}`);
    if (method === 'GET' && !id) return json(200, { plans: [...plans.values()].map(meta) });
    const existing = id ? plans.get(id) : undefined;
    if (id && !existing) return json(404, { error: 'not_found' });
    if (method === 'GET') return json(200, existing);
    if (method === 'DELETE') return (plans.delete(id as string), new Response(null, { status: 204 }));
    const body = JSON.parse(String(init?.body)) as { plan: PlanFile; name?: string };
    const doc: RemotePlan = { id: id ?? `pl_${cookie}_${nextId++}`, name: body.name ?? 'Untitled plan', rev: (existing?.rev ?? 0) + 1, created_at: 'a', updated_at: 'b', updated_by: 'web', bytes: 1, plan: body.plan };
    plans.set(doc.id, doc);
    return json(method === 'POST' ? 201 : 200, meta(doc));
  };
  return { fetchImpl, writes, signIn: (user: string | null) => void (cookie = user), plansOf: (user: string) => [...(accounts.get(user)?.values() ?? [])] };
}

describe('the account changes in another tab', () => {
  // Tab A is signed in as u1 with a synced plan. In tab B, u1 signs out and u2 signs in:
  // the cookie both tabs send is now u2's, and tab A has not been told.
  const switched = async () => {
    const server = cookieServer('u1');
    const t = setup((account) => httpPlansApi(account, server.fetchImpl));
    await t.signIn('u1');
    const id = t.browser.add('Mine');
    await t.sync.enable(id);
    await t.wait();
    const link = t.browser.links.get(id) as RemoteLink;
    expect(server.plansOf('u1')).toHaveLength(1);
    server.signIn('u2');
    server.writes.length = 0;
    return { ...t, server, id, link };
  };

  it('the stale tab unlinks nothing and stops, instead of taking the other account’s list for its own', async () => {
    const t = await switched();
    await t.sync.refresh();
    expect(t.browser.links.get(t.id)).toEqual(t.link);
    expect(t.events).toEqual([]);
    expect(t.signedOut()).toBe(1);
    expect(t.state()).toMatchObject({ userId: null, remote: null });
  });

  it('uploads nothing into the other account: not by the switch, not by a save', async () => {
    const t = await switched();
    // Before the stale tab has looked at the account at all.
    await expect(t.sync.enable(t.browser.add('Another of mine'))).rejects.toMatchObject({ kind: 'unauthenticated' });
    t.save(t.id, 5);
    await t.wait(120_000);
    await expect(t.sync.enable(t.id)).rejects.toMatchObject({ kind: 'unauthenticated' });
    expect(t.server.writes).toEqual([]);
    expect(t.server.plansOf('u2')).toEqual([]);
    expect(salaryOf(t.server.plansOf('u1')[0]?.plan.profile as Profile)).toBe(100_000);
    expect(t.browser.links.get(t.id)).toMatchObject({ id: t.link.id, userId: 'u1' });
    expect(t.events).toEqual([]);
  });

  it('once the app has adopted the new account, the first one’s plans are still left alone', async () => {
    const t = await switched();
    await t.sync.refresh();
    await t.signIn('u2');
    t.save(t.id, 6);
    await t.wait(60_000);
    expect(t.state()).toMatchObject({ userId: 'u2', remote: [] });
    expect(t.browser.links.get(t.id)).toMatchObject({ id: t.link.id, userId: 'u1', dirty: true });
    expect(t.server.writes).toEqual([]);
    expect(t.events).toEqual([]);
  });
});

describe('sync turned on or off in another tab', () => {
  it('uploads nothing once the stored record no longer carries the link', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    // The other tab turned sync off and kept the account's copy: same revision there, no link in storage.
    t.browser.elsewhere.set(id, null);
    t.save(id, 777);
    await t.wait(60_000);
    expect(t.server.uploads()).toEqual([]);
    expect(t.server.plans.get(remoteId)).toMatchObject({ rev: 1 });
    expect(t.state().status[id]).toBeUndefined();
  });

  it('uploads nothing to the old plan once storage points at a different one', async () => {
    const t = setup();
    await t.signIn();
    const { id, remoteId } = await t.synced();
    t.browser.elsewhere.set(id, { ...(t.browser.links.get(id) as RemoteLink), id: 'pl_other' });
    t.save(id, 777);
    await t.wait(60_000);
    expect(t.server.uploads()).toEqual([]);
    expect(t.server.plans.get(remoteId)).toMatchObject({ rev: 1 });
  });

  it('a waiting upload is dropped when the tab is told sync was turned off', async () => {
    const t = setup();
    await t.signIn();
    const { id } = await t.synced();
    t.save(id, 777);
    expect(t.state().status[id]).toBe('pending');
    // households.ts has taken the link out of the map, as the other tab announced.
    t.browser.links.delete(id);
    t.sync.noteLinkChanged(id);
    await t.wait(60_000);
    expect(t.server.uploads()).toEqual([]);
    expect(t.state().status[id]).toBeUndefined();
  });

  it('a household another tab started syncing is followed from here', async () => {
    const t = setup();
    await t.signIn();
    const id = t.browser.add('Ours');
    const doc = t.server.agentCreates(toPlanFile(t.browser.records.get(id) as HouseholdRecord), 'web');
    t.browser.links.set(id, { id: doc.id, rev: 1, syncedAt: Date.now(), updatedBy: 'web', userId: USER, dirty: false });
    t.sync.noteLinkChanged(id);
    await t.wait(10);
    expect(t.state().status[id]).toBe('synced');
    t.save(id, 9);
    await t.wait(PUSH_DELAY + 10);
    expect(t.server.uploads()).toEqual([`update ${doc.id} @1`]);
    expect(t.events).toEqual([]);
  });
});

describe('safeScenarios', () => {
  const scenario = (id: string, name: string) => ({ id, name, patch: [], enabled: true });

  it('keeps well-formed scenarios as they are', () => {
    const list = [scenario('part_time', 'Part time'), scenario('move', 'Move')];
    expect(safeScenarios(list)).toEqual(list);
  });

  it('gives a new id to a scenario that claims the baseline’s id or repeats another’s', () => {
    const out = safeScenarios([scenario('base', 'Salary cut'), scenario('base', 'x'), scenario('move', 'Move'), scenario('move', 'Move'), scenario('', 'Base')]);
    const ids = out.map((s) => s.id);
    expect(ids).toEqual(['salary_cut', 'x', 'move', 'move_2', 'base_2']);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids).not.toContain('base');
    expect(out.map((s) => s.name)).toEqual(['Salary cut', 'x', 'Move', 'Move', 'Base']);
  });

  it('a new id never takes one that a later scenario already has', () => {
    const out = safeScenarios([scenario('base', 'Move'), scenario('move', 'Move')]);
    expect(out.map((s) => s.id)).toEqual(['move_2', 'move']);
  });

  it('drops what is not a scenario', () => {
    expect(safeScenarios([null, 'x', { id: 'a' }, { id: 1, name: 'n', patch: [], enabled: true }, scenario('ok', 'OK')])).toEqual([scenario('ok', 'OK')]);
    expect(safeScenarios(undefined)).toEqual([]);
    expect(safeScenarios({ length: 1 })).toEqual([]);
  });
});

describe('describeWriter', () => {
  it('names who wrote a revision', () => {
    expect(describeWriter('web')).toBe('the website');
    expect(describeWriter('apikey:Claude Code')).toBe('Claude Code key');
    expect(describeWriter('oauth:Claude')).toBe('Claude');
    expect(describeWriter('oauth:')).toBe('a connected app');
    expect(describeWriter('')).toBe('someone');
  });
});

describe('toPlanFile', () => {
  it('is the plan-file envelope, with the run settings in the profile as an export has them', () => {
    const file = toPlanFile({ profile: profile('Ours'), scenarios: [{ id: 's', name: 'S', patch: [], enabled: true }], run: { paths: 2_000, seed: 9 } });
    expect(file).toMatchObject({ firesight: 2, run: { paths: 2_000, seed: 9 }, scenarios: [{ id: 's' }] });
    expect(file.profile.simulation).toMatchObject({ paths: 2_000, seed: 9 });
  });
});

describe('httpPlansApi', () => {
  const json = (status: number, body: unknown, headers: Record<string, string> = {}) => new Response(status === 204 ? null : JSON.stringify(body), { status, headers: { 'content-type': 'application/json', ...headers } });
  const meta: RemoteMeta = { id: 'pl_1', name: 'Ours', rev: 3, created_at: 'a', updated_at: 'b', updated_by: 'web', bytes: 10 };

  it('sends If-Match on updates, JSON bodies on writes, and the session cookie only', async () => {
    const seen: { url: string; init: RequestInit }[] = [];
    const api = httpPlansApi(() => USER, async (url, init) => {
      seen.push({ url: String(url), init: init ?? {} });
      return json(200, meta);
    });
    const plan = toPlanFile({ profile: profile('Ours'), scenarios: [], run: { paths: 1, seed: 1 } });
    await api.update('pl_1', plan, 2, 'Ours');
    await api.create(plan);
    const [update, create] = seen;
    expect(update?.url).toBe('/api/plans/pl_1');
    expect(update?.init.method).toBe('PUT');
    expect(update?.init.headers).toMatchObject({ 'if-match': '2', 'content-type': 'application/json' });
    expect(JSON.parse(String(update?.init.body))).toEqual({ plan, name: 'Ours' });
    expect(JSON.parse(String(create?.init.body))).toEqual({ plan });
    for (const call of seen) {
      expect(call.init.credentials).toBe('same-origin');
      expect(JSON.stringify(call.init.headers).toLowerCase()).not.toContain('authorization');
    }
  });

  it('turns each failure into a kind the sync can act on', async () => {
    const answer = (response: () => Response | Promise<Response>) => httpPlansApi(() => USER, async () => response());
    await expect(answer(() => json(409, { error: 'conflict', message: 'm', current: meta })).update('pl_1', {} as PlanFile, 1)).rejects.toMatchObject({ kind: 'conflict', current: meta, retryable: false });
    await expect(answer(() => json(401, { error: 'unauthenticated' })).list()).rejects.toMatchObject({ kind: 'unauthenticated' });
    await expect(answer(() => json(404, { error: 'not_found' })).get('pl_1')).rejects.toMatchObject({ kind: 'not_found' });
    await expect(answer(() => json(409, { error: 'too_many_plans' })).create({} as PlanFile)).rejects.toMatchObject({ kind: 'too_many_plans' });
    await expect(answer(() => json(413, { error: 'too_large' })).create({} as PlanFile)).rejects.toMatchObject({ kind: 'too_large', retryable: false });
    await expect(answer(() => json(429, { error: 'rate_limited' })).list()).rejects.toMatchObject({ kind: 'rate_limited', retryable: true });
    await expect(answer(() => json(503, { error: 'accounts_unavailable' })).list()).rejects.toMatchObject({ kind: 'unavailable', retryable: true });
    await expect(answer(() => new Response('<html>', { status: 502 })).list()).rejects.toMatchObject({ kind: 'unavailable' });
    await expect(answer(() => Promise.reject(new TypeError('Failed to fetch'))).list()).rejects.toMatchObject({ kind: 'offline', retryable: true });
    // A static host answers every path with the app shell.
    await expect(answer(() => new Response('<!doctype html>', { status: 200, headers: { 'content-type': 'text/html' } })).list()).rejects.toMatchObject({ kind: 'unavailable' });
  });

  it('treats a 204 delete as done', async () => {
    await expect(httpPlansApi(() => USER, async () => new Response(null, { status: 204 })).remove('pl_1')).resolves.toBeUndefined();
  });

  it('says on every request which account it is made for', async () => {
    const seen: { method: string; believed: string | null }[] = [];
    let user = 'u1';
    const api = httpPlansApi(
      () => user,
      async (_url, init) => {
        seen.push({ method: init?.method ?? 'GET', believed: new Headers(init?.headers).get(ACCOUNT_HEADER) });
        return init?.method === 'DELETE' ? new Response(null, { status: 204 }) : json(200, { ...meta, plans: [] });
      },
    );
    const plan = toPlanFile({ profile: profile('Ours'), scenarios: [], run: { paths: 1, seed: 1 } });
    await api.list();
    await api.get('pl_1');
    await api.create(plan);
    await api.update('pl_1', plan, 1);
    await api.remove('pl_1');
    await api.exportAll();
    expect(seen.map((r) => r.method)).toEqual(['GET', 'GET', 'POST', 'PUT', 'DELETE', 'GET']);
    expect(seen.every((r) => r.believed === 'u1')).toBe(true);
    // Asked for on each call, not once: the tab's account can change.
    user = 'u2';
    await api.list();
    expect(seen.at(-1)?.believed).toBe('u2');
    expect(ACCOUNT_HEADER).toBe('x-firesight-user');
  });

  it('takes a refusal for the wrong account as the session being lost', async () => {
    const api = httpPlansApi(() => USER, async () => json(401, { error: 'account_changed', message: 'A different account is signed in on this browser now.' }));
    await expect(api.list()).rejects.toMatchObject({ kind: 'unauthenticated', retryable: false });
    await expect(api.create({} as PlanFile)).rejects.toMatchObject({ kind: 'unauthenticated' });
  });

  it('sends nothing when this tab has no account to name', async () => {
    let sent = 0;
    const api = httpPlansApi(() => null, async () => ((sent += 1), json(200, { plans: [] })));
    await expect(api.list()).rejects.toMatchObject({ kind: 'unauthenticated' });
    await expect(api.create({} as PlanFile)).rejects.toMatchObject({ kind: 'unauthenticated' });
    expect(sent).toBe(0);
  });
});
