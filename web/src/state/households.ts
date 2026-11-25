// Opening, creating, saving and deleting households. Ties the profile store to storage.
import { create } from 'zustand';
import type { Profile } from '@/engine/types';
import {
  deleteHouseholdRecord,
  listHouseholds,
  newHouseholdId,
  readHousehold,
  writeHousehold,
  type HouseholdRecord,
  type HouseholdSummary,
  type RemoteLink,
} from './persistence';
import { useProfileStore, type RunSettings, type Scenario } from './profile-store';
import { useSettings } from './settings-store';
import { toast } from './ui-store';

interface HouseholdsState {
  list: HouseholdSummary[];
  /** False when IndexedDB is unavailable: the app still works, nothing is kept. */
  storageAvailable: boolean;
  saving: boolean;
}

export const useHouseholds = create<HouseholdsState>(() => ({ list: [], storageAvailable: true, saving: false }));

// ---- links to the account -----------------------------------------------------------
//
// A synced household carries a `remote` link (which plan in the account, at which
// revision). Within a tab the link has one owner, this map, read and changed
// synchronously: a save and a finished upload can then never write stale copies of it
// over each other. The record in storage follows the map, and every record write goes
// through one queue.
//
// Between tabs, storage is what is shared, so storage decides WHETHER a household is
// linked, and to which plan. A tab that turns sync on or off tells the others at once
// (a BroadcastChannel), and every tab also checks the stored record before it writes
// one, before an upload and when it comes back into view. Without that a second tab
// would write its old link back, and upload, after the person had turned sync off.
// The revision and the dirty mark stay per tab: they describe what this tab has open.

const links = new Map<string, RemoteLink>();
/** Link changes made in this tab that have not reached storage yet. While one is waiting, the map is ahead of storage. */
const unsettled = new Map<string, number>();
let writes: Promise<unknown> = Promise.resolve();

function enqueue<T>(task: () => Promise<T>): Promise<T> {
  const run = writes.then(task, task);
  writes = run.catch(() => undefined);
  return run;
}

const hold = (id: string) => void unsettled.set(id, (unsettled.get(id) ?? 0) + 1);
function release(id: string) {
  const left = (unsettled.get(id) ?? 1) - 1;
  if (left > 0) unsettled.set(id, left);
  else unsettled.delete(id);
}

interface LinkNotice {
  id: string;
  link: RemoteLink | null;
}

const isLink = (value: unknown): value is RemoteLink => {
  if (value === null || typeof value !== 'object') return false;
  const link = value as Record<string, unknown>;
  return typeof link.id === 'string' && typeof link.userId === 'string' && typeof link.rev === 'number';
};

// Absent in some embedded browsers: the checks against storage still hold there.
const channel = typeof BroadcastChannel === 'undefined' ? null : new BroadcastChannel('firesight-links');
// Outside a browser (tests) an open channel would keep the process alive.
(channel as { unref?: () => void } | null)?.unref?.();

/** Tells the other tabs what this tab has just stored for a household's link. */
function announce(id: string) {
  try {
    channel?.postMessage({ id, link: links.get(id) ?? null } satisfies LinkNotice);
  } catch {
    // A closed channel: the other tabs find out from storage.
  }
}

const linkListeners = new Set<(id: string) => void>();
/** Called when another tab turned sync on or off for a household, or pointed it at a different plan. */
export function onRemoteLinkChanged(listener: (id: string) => void): () => void {
  linkListeners.add(listener);
  return () => linkListeners.delete(listener);
}

/**
 * Makes this tab agree with what is stored (or was announced) about a household's link.
 * Only whether it is linked, and to which plan of which account, is taken over: the
 * revision stays this tab's own, because it says which version this tab's copy is based on.
 */
function follow(id: string, stored: RemoteLink | undefined): boolean {
  const here = links.get(id);
  if (here?.id === stored?.id && here?.userId === stored?.userId) return false;
  if (stored) links.set(id, stored);
  else links.delete(id);
  useHouseholds.setState((s) => ({ list: s.list.map((h) => (h.id === id ? { ...h, remote: stored } : h)) }));
  linkListeners.forEach((listener) => listener(id));
  return true;
}

if (channel) {
  channel.onmessage = (event: MessageEvent) => {
    const notice = event.data as Partial<LinkNotice> | null;
    if (!notice || typeof notice.id !== 'string') return;
    if (!unsettled.has(notice.id)) follow(notice.id, isLink(notice.link) ? notice.link : undefined);
    // Names and the list itself may have changed too (a household deleted in the other tab).
    void refreshHouseholds();
  };
}

/** The account link of a household in this browser, if it is synced. */
export const remoteLink = (id: string): RemoteLink | undefined => links.get(id);
export const remoteLinks = (): [string, RemoteLink][] => [...links.entries()];

/** Sets or clears a household's account link, here and in its stored record. */
export function setRemoteLink(id: string, link: RemoteLink | null): Promise<void> {
  if (link) links.set(id, link);
  else links.delete(id);
  useHouseholds.setState((s) => ({ list: s.list.map((h) => (h.id === id ? { ...h, remote: link ?? undefined } : h)) }));
  hold(id);
  return enqueue(async () => {
    try {
      const record = await readHousehold(id);
      if (record) await writeHousehold({ ...record, remote: links.get(id) });
      announce(id);
    } finally {
      release(id);
    }
  }).catch(() => undefined);
}

/**
 * A stored household, read after every write queued before the call has landed. If
 * another tab changed its link meanwhile, this tab takes that over before answering.
 */
export const readHouseholdRecord = (id: string): Promise<HouseholdRecord | undefined> =>
  enqueue(async () => {
    const record = await readHousehold(id);
    if (record && !unsettled.has(id)) follow(id, record.remote);
    return record;
  }).catch(() => undefined);

/** Writes a whole record (a plan fetched from the account), link included. */
export async function writeHouseholdRecord(record: HouseholdRecord): Promise<void> {
  if (record.remote) links.set(record.id, record.remote);
  else links.delete(record.id);
  hold(record.id);
  await enqueue(async () => {
    try {
      await writeHousehold({ ...record, remote: links.get(record.id) });
      announce(record.id);
    } finally {
      release(record.id);
    }
  });
  await refreshHouseholds();
}

const savedListeners = new Set<(id: string) => void>();
/** Called after a synced household has been saved here with changes the account has not seen. */
export function onSyncedHouseholdSaved(listener: (id: string) => void): () => void {
  savedListeners.add(listener);
  return () => savedListeners.delete(listener);
}

export async function refreshHouseholds() {
  try {
    const list = await enqueue(async () => {
      const stored = await listHouseholds();
      // Storage is the truth about which households are linked, except where this tab has a change of its own still to write.
      const seen = new Set<string>();
      for (const h of stored) {
        seen.add(h.id);
        if (!unsettled.has(h.id)) follow(h.id, h.remote);
      }
      for (const id of [...links.keys()]) if (!seen.has(id) && !unsettled.has(id)) follow(id, undefined);
      return stored;
    });
    useHouseholds.setState({ list: list.map((h) => ({ ...h, remote: links.get(h.id) })), storageAvailable: true });
  } catch {
    useHouseholds.setState({ list: [], storageAvailable: false });
  }
}

export interface NewHousehold {
  profile: Profile;
  origin: string;
  scenarios?: Scenario[];
  run?: Partial<RunSettings>;
  migrationNotes?: string[];
}

/** Opens a new household in the editor. It is written to storage by the autosave. */
export function createHousehold(input: NewHousehold): string {
  const id = newHouseholdId();
  useProfileStore.getState().load({
    meta: { id, createdAt: Date.now(), origin: input.origin },
    profile: input.profile,
    scenarios: input.scenarios,
    run: input.run,
    migrationNotes: input.migrationNotes,
    unsaved: true,
  });
  useSettings.getState().set({ lastHouseholdId: id });
  return id;
}

export async function openHousehold(id: string): Promise<boolean> {
  await saveNow();
  const record = await readHousehold(id).catch(() => undefined);
  if (!record) return false;
  useProfileStore.getState().load({
    meta: { id: record.id, createdAt: record.createdAt, origin: record.origin },
    profile: record.profile,
    scenarios: record.scenarios,
    run: record.run,
  });
  useSettings.getState().set({ lastHouseholdId: id });
  return true;
}

/** Removes a household from this browser. Its copy in an account, if any, is not touched. */
export async function deleteHousehold(id: string) {
  links.delete(id);
  hold(id);
  await enqueue(async () => {
    try {
      await deleteHouseholdRecord(id);
      announce(id);
    } finally {
      release(id);
    }
  }).catch(() => undefined);
  // Its assistant conversation goes with it (kept in the same browser, keyed by household).
  void import('@/assistant/conversation').then((m) => m.deleteConversation(id)).catch(() => undefined);
  if (useProfileStore.getState().meta?.id === id) {
    useProfileStore.getState().close();
    useSettings.getState().set({ lastHouseholdId: null });
  }
  await refreshHouseholds();
}

/** Leaves the current household (saving it first) and returns to the landing state. */
export async function closeHousehold() {
  await saveNow();
  useProfileStore.getState().close();
  useSettings.getState().set({ lastHouseholdId: null });
}

/** Writes the open household if it has unsaved changes. */
export async function saveNow() {
  const s = useProfileStore.getState();
  if (!s.meta || !s.profile || s.revision === s.savedRevision) return;
  const revision = s.revision;
  const id = s.meta.id;
  // A synced household now differs from the account's copy until the upload is accepted.
  const link = links.get(id);
  if (link) links.set(id, { ...link, dirty: true });
  const record: HouseholdRecord = {
    id,
    name: s.profile.name || 'Untitled household',
    origin: s.meta.origin,
    createdAt: s.meta.createdAt,
    updatedAt: Date.now(),
    profile: s.profile,
    scenarios: s.scenarios,
    run: s.run,
  };
  useHouseholds.setState({ saving: true });
  try {
    await enqueue(async () => {
      // Another tab may have turned sync off (or on) for this household since this tab last looked.
      // What is stored wins: writing this tab's old link back would turn sync on again behind the person's back.
      if (!unsettled.has(id)) {
        const stored = (await readHousehold(id))?.remote;
        // A link taken over here covers content it has not seen: this save.
        if (follow(id, stored) && stored) links.set(id, { ...stored, dirty: true });
      }
      // The link is read when the write runs, not earlier: an upload may finish in between.
      await writeHousehold({ ...record, remote: links.get(id) });
    });
    useProfileStore.getState().markSaved(revision);
    await refreshHouseholds();
    if (links.has(id)) savedListeners.forEach((listener) => listener(id));
  } catch {
    if (useHouseholds.getState().storageAvailable) {
      useHouseholds.setState({ storageAvailable: false });
      toast({
        tone: 'warn',
        title: 'This browser is not letting FIRESight save',
        description: 'Your plan stays open in this tab. Export it as a file to keep it.',
        duration: 0,
      });
    }
  } finally {
    useHouseholds.setState({ saving: false });
  }
}

/** Saves a moment after each change, and when the tab is hidden. */
export function startAutosave() {
  let timer: ReturnType<typeof setTimeout> | null = null;
  useProfileStore.subscribe((state, previous) => {
    if (state.revision === previous.revision || state.revision === state.savedRevision) return;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void saveNow(), 600);
  });
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') void saveNow();
  });
}
