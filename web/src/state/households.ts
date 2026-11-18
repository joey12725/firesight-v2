// Opening, creating, saving and deleting households. Ties the profile store to storage.
import { create } from 'zustand';
import type { Profile } from '@/engine/types';
import {
  deleteHouseholdRecord,
  listHouseholds,
  newHouseholdId,
  readHousehold,
  writeHousehold,
  type HouseholdSummary,
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

export async function refreshHouseholds() {
  try {
    useHouseholds.setState({ list: await listHouseholds(), storageAvailable: true });
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

export async function deleteHousehold(id: string) {
  await deleteHouseholdRecord(id).catch(() => undefined);
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
  useHouseholds.setState({ saving: true });
  try {
    await writeHousehold({
      id: s.meta.id,
      name: s.profile.name || 'Untitled household',
      origin: s.meta.origin,
      createdAt: s.meta.createdAt,
      updatedAt: Date.now(),
      profile: s.profile,
      scenarios: s.scenarios,
      run: s.run,
    });
    useProfileStore.getState().markSaved(revision);
    await refreshHouseholds();
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
