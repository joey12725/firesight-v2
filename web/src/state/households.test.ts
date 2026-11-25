// Two tabs and one browser store: whether a household is synced is decided by what is
// stored, so a tab that has not heard that sync was turned off can never turn it back on.
// Each "tab" here is a fresh copy of households.ts (with its own in-memory links) over
// one shared stand-in for IndexedDB. The browser itself is covered by e2e/accounts.mjs.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Profile } from '@/engine/types';
import type { HouseholdRecord, RemoteLink } from './persistence';

const disk = vi.hoisted(() => ({ records: new Map<string, unknown>() }));

vi.mock('./persistence', () => ({
  listHouseholds: async () => [...(disk.records as Map<string, HouseholdRecord>).values()].map((r) => ({ id: r.id, name: r.name, origin: r.origin, createdAt: r.createdAt, updatedAt: r.updatedAt, remote: r.remote })),
  readHousehold: async (id: string) => structuredClone(disk.records.get(id)),
  writeHousehold: async (record: HouseholdRecord) => void disk.records.set(record.id, structuredClone(record)),
  deleteHouseholdRecord: async (id: string) => void disk.records.delete(id),
  newHouseholdId: () => 'h_new',
}));
// The editor's store, cut down to what saving reads. The real one needs the engine.
vi.mock('./profile-store', async () => {
  const { create } = await import('zustand');
  const useProfileStore = create<Record<string, unknown>>((set) => ({
    meta: null,
    profile: null,
    scenarios: [],
    run: { paths: 10_000, seed: 1 },
    revision: 0,
    savedRevision: 0,
    markSaved: (revision: number) => set({ savedRevision: revision }),
    close: () => set({ meta: null, profile: null }),
  }));
  return { useProfileStore };
});
vi.mock('./settings-store', () => ({ useSettings: { getState: () => ({ set: () => undefined }) } }));
vi.mock('./ui-store', () => ({ toast: () => undefined }));

/** BroadcastChannel as browsers have it: a message reaches every other channel of the same name, later. */
class FakeChannel {
  static open: FakeChannel[] = [];
  onmessage: ((event: { data: unknown }) => void) | null = null;
  constructor(readonly name: string) {
    FakeChannel.open.push(this);
  }
  postMessage(data: unknown) {
    const copy = structuredClone(data);
    for (const other of FakeChannel.open) if (other !== this && other.name === this.name) queueMicrotask(() => other.onmessage?.({ data: copy }));
  }
}

const ID = 'h_1';
const LINK: RemoteLink = { id: 'pl_1', rev: 1, syncedAt: 1, updatedBy: 'web', userId: 'u1', dirty: false };
const profile = (salary: number) => ({ name: 'Ours', salary }) as unknown as Profile;
const record = (remote?: RemoteLink): HouseholdRecord => ({ id: ID, name: 'Ours', origin: 'blank', createdAt: 1, updatedAt: 1, profile: profile(100), scenarios: [], run: { paths: 10_000, seed: 1 }, remote });
const stored = () => disk.records.get(ID) as HouseholdRecord | undefined;
const settle = async () => {
  for (let i = 0; i < 20; i++) await new Promise((resolve) => setTimeout(resolve, 0));
};

/** A tab with the household open, as after a page load. */
async function openTab() {
  vi.resetModules();
  const households = await import('./households');
  const { useProfileStore } = (await import('./profile-store')) as unknown as { useProfileStore: { getState(): { revision: number }; setState(state: Record<string, unknown>): void } };
  await households.refreshHouseholds();
  const saved: string[] = [];
  const changed: string[] = [];
  households.onSyncedHouseholdSaved((id) => void saved.push(id));
  households.onRemoteLinkChanged((id) => void changed.push(id));
  // One editor store serves every tab here (the mock outlives the module reset), so each edit says which household is open.
  const meta = { id: ID, createdAt: 1, origin: 'blank' };
  return {
    ...households,
    /** What is asked of the sync code: uploads follow `saved`. */
    saved,
    changed,
    /** An edit in the editor, then the autosave. */
    async edit(salary: number) {
      useProfileStore.setState({ meta, profile: profile(salary), revision: useProfileStore.getState().revision + 1 });
      await households.saveNow();
    },
  };
}

beforeEach(() => {
  disk.records.clear();
  FakeChannel.open = [];
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe('without a channel between tabs: storage decides', () => {
  beforeEach(() => vi.stubGlobal('BroadcastChannel', undefined));

  it('a save in a second tab does not turn sync back on after the first tab turned it off', async () => {
    disk.records.set(ID, record(LINK));
    const a = await openTab();
    const b = await openTab();
    expect(b.remoteLink(ID)).toEqual(LINK);

    await a.setRemoteLink(ID, null);
    expect(stored()?.remote).toBeUndefined();

    await b.edit(777);
    expect((stored()?.profile as unknown as { salary: number }).salary).toBe(777);
    expect(stored()?.remote).toBeUndefined();
    expect(b.remoteLink(ID)).toBeUndefined();
    expect(b.useHouseholds.getState().list.find((h) => h.id === ID)?.remote).toBeUndefined();
    // Nothing is handed to the sync code to upload, and it is told to drop what it had waiting.
    expect(b.saved).toEqual([]);
    expect(b.changed).toEqual([ID]);
  });

  it('a save in a second tab does not erase the link after the first tab turned sync on', async () => {
    disk.records.set(ID, record());
    const a = await openTab();
    const b = await openTab();
    await a.setRemoteLink(ID, LINK);

    await b.edit(555);
    expect(stored()?.remote).toMatchObject({ id: 'pl_1', userId: 'u1', rev: 1, dirty: true });
    expect(b.remoteLink(ID)).toMatchObject({ id: 'pl_1', dirty: true });
    // This tab now syncs the household too: its change goes to the plan the other tab created.
    expect(b.saved).toEqual([ID]);
  });

  it('reading the record before an upload takes over what the other tab stored', async () => {
    disk.records.set(ID, record(LINK));
    const a = await openTab();
    const b = await openTab();
    await a.setRemoteLink(ID, null);
    expect((await b.readHouseholdRecord(ID))?.remote).toBeUndefined();
    expect(b.remoteLink(ID)).toBeUndefined();
    expect(b.changed).toEqual([ID]);
  });

  it('coming back to a tab re-reads which households are linked', async () => {
    disk.records.set(ID, record(LINK));
    const a = await openTab();
    const b = await openTab();
    await a.setRemoteLink(ID, null);
    await b.refreshHouseholds();
    expect(b.remoteLinks()).toEqual([]);
    await a.setRemoteLink(ID, { ...LINK, id: 'pl_2' });
    await b.refreshHouseholds();
    expect(b.remoteLink(ID)).toMatchObject({ id: 'pl_2' });
    expect(b.changed).toEqual([ID, ID]);
  });

  it('a household deleted in another tab comes back from a save here as a local plan, not a synced one', async () => {
    disk.records.set(ID, record(LINK));
    const a = await openTab();
    const b = await openTab();
    await a.deleteHousehold(ID);
    expect(stored()).toBeUndefined();
    await b.edit(3);
    expect(stored()).toBeDefined();
    expect(stored()?.remote).toBeUndefined();
    expect(b.saved).toEqual([]);
  });

  it('keeps its own revision: a newer one stored by another tab describes that tab’s copy, not this one’s', async () => {
    disk.records.set(ID, record(LINK));
    const a = await openTab();
    const b = await openTab();
    await a.setRemoteLink(ID, { ...LINK, rev: 5 });
    await b.edit(9);
    // Uploaded with the revision this copy was based on, so the account can refuse it as stale.
    expect(b.remoteLink(ID)).toMatchObject({ id: 'pl_1', rev: 1, dirty: true });
    expect(b.saved).toEqual([ID]);
    expect(b.changed).toEqual([]);
  });

  it('a tab’s own link change is not undone by a save or a read queued behind it', async () => {
    disk.records.set(ID, record());
    const a = await openTab();
    // Not awaited: the map is ahead of storage, as it is between `enable` answering and the write landing.
    const linking = a.setRemoteLink(ID, LINK);
    const reading = a.readHouseholdRecord(ID);
    const saving = a.edit(4);
    await Promise.all([linking, reading, saving]);
    expect(a.remoteLink(ID)).toMatchObject({ id: 'pl_1' });
    expect(stored()?.remote).toMatchObject({ id: 'pl_1' });
    expect(a.changed).toEqual([]);

    const unlinking = a.setRemoteLink(ID, null);
    await Promise.all([unlinking, a.refreshHouseholds(), a.edit(5)]);
    expect(a.remoteLink(ID)).toBeUndefined();
    expect(stored()?.remote).toBeUndefined();
    expect(a.changed).toEqual([]);
  });

  it('a plan fetched from the account keeps its link through the list refresh that follows', async () => {
    const a = await openTab();
    await a.writeHouseholdRecord({ ...record(LINK), id: 'h_2' });
    expect(a.remoteLink('h_2')).toEqual(LINK);
    expect((disk.records.get('h_2') as HouseholdRecord).remote).toEqual(LINK);
    expect(a.useHouseholds.getState().list.find((h) => h.id === 'h_2')?.remote).toEqual(LINK);
  });
});

describe('with a channel between tabs: the other tab hears at once', () => {
  beforeEach(() => vi.stubGlobal('BroadcastChannel', FakeChannel));

  it('sync turned off in one tab is off in the other before it saves anything', async () => {
    disk.records.set(ID, record(LINK));
    const a = await openTab();
    const b = await openTab();
    await a.setRemoteLink(ID, null);
    await settle();
    expect(b.remoteLink(ID)).toBeUndefined();
    expect(b.useHouseholds.getState().list.find((h) => h.id === ID)?.remote).toBeUndefined();
    expect(b.changed).toEqual([ID]);
    await b.edit(777);
    expect(stored()?.remote).toBeUndefined();
    expect(b.saved).toEqual([]);
    // The tab that made the change is not told about its own change.
    expect(a.changed).toEqual([]);
  });

  it('sync turned on in one tab is on in the other', async () => {
    disk.records.set(ID, record());
    const a = await openTab();
    const b = await openTab();
    await a.setRemoteLink(ID, LINK);
    await settle();
    expect(b.remoteLink(ID)).toEqual(LINK);
    expect(b.useHouseholds.getState().list.find((h) => h.id === ID)?.remote).toEqual(LINK);
    expect(b.changed).toEqual([ID]);
  });

  it('a plan fetched or deleted in one tab is linked or gone in the other', async () => {
    const a = await openTab();
    const b = await openTab();
    await a.writeHouseholdRecord(record(LINK));
    await settle();
    expect(b.remoteLink(ID)).toEqual(LINK);
    await a.deleteHousehold(ID);
    await settle();
    expect(b.remoteLink(ID)).toBeUndefined();
    expect(b.useHouseholds.getState().list).toEqual([]);
  });

  it('ignores a message that is not a link notice', async () => {
    disk.records.set(ID, record(LINK));
    const b = await openTab();
    const outside = new FakeChannel('firesight-links');
    outside.postMessage('hello');
    outside.postMessage({ id: 7, link: null });
    outside.postMessage({ id: ID, link: { id: 'pl_evil' } });
    await settle();
    // A malformed link is no link, and storage, read straight after, still says it is linked.
    expect(b.remoteLink(ID)).toEqual(LINK);
  });
});
