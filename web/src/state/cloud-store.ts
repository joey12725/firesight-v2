// The account sync (cloud.ts) wired to the real things: the plans API on this origin,
// the households in IndexedDB, the open plan, the engine, and toasts for what happens.
// Nothing here runs until someone is signed in: signed out, `refresh` returns at once
// and no request is made.
import { useStore } from 'zustand';
import { normalize } from '@/engine';
import { CloudError, httpPlansApi, type RemoteMeta } from './cloud-api';
import { createCloudSync, describeWriter, safeScenarios, type CloudEvent, type CloudState, type CloudSync, type PlanContent } from './cloud';
import type { PlanFile } from './files';
import {
  deleteHousehold,
  onRemoteLinkChanged,
  onSyncedHouseholdSaved,
  openHousehold,
  readHouseholdRecord,
  refreshHouseholds,
  remoteLink,
  remoteLinks,
  saveNow,
  setRemoteLink,
  useHouseholds,
  writeHouseholdRecord,
} from './households';
import { newHouseholdId } from './persistence';
import { useProfileStore, type RunSettings } from './profile-store';
import { toast } from './ui-store';

/** While the tab is visible. An agent's write shows up within this long. */
const POLL_MS = 10_000;

/** A plan from the account, checked by the engine before it is shown or stored. */
export function readRemotePlan(plan: PlanFile): PlanContent {
  if (plan === null || typeof plan !== 'object' || plan.firesight !== 2 || plan.profile === null || typeof plan.profile !== 'object') {
    throw new Error('it is not a FIRESight plan');
  }
  const result = normalize(plan.profile);
  const schemaError = result.errors.find((e) => e.code === 'schema');
  if (schemaError) throw new Error(schemaError.message);
  const run = plan.run as Partial<RunSettings> | undefined;
  const validRun = run && Number.isFinite(run.paths) && Number.isFinite(run.seed) && (run.paths as number) > 0 ? { paths: run.paths as number, seed: run.seed as number } : undefined;
  return { profile: result.profile, scenarios: safeScenarios(plan.scenarios), run: validRun };
}

let signedOut: () => void = () => undefined;
/**
 * The session code registers what to do when the server says the session has ended, or
 * is no longer this account's. Syncing has stopped by the time the handler runs.
 */
export function onSessionLost(handler: () => void) {
  signedOut = handler;
}

function report(event: CloudEvent) {
  switch (event.type) {
    case 'pulled':
      // A copy that is not open is brought up to date quietly.
      if (event.open) toast({ tone: 'good', title: event.by === 'web' ? 'Updated from another browser' : `Updated by ${describeWriter(event.by)}`, description: 'The newer version from your account is now open, and the views are running again.' });
      break;
    case 'new-remote':
      toast({
        title: `New plan in your account: ${event.meta.name}`,
        description: `Created by ${describeWriter(event.meta.updated_by)}.`,
        action: { label: 'Open', run: () => void openCloudPlan(event.meta.id) },
        duration: 20_000,
      });
      break;
    case 'unlinked':
      toast({ tone: 'warn', title: `"${event.name}" is no longer in your account`, description: 'It was deleted there. The copy in this browser is kept and is no longer synced.', duration: 12_000 });
      break;
    case 'push-failed':
      toast({ tone: 'bad', title: `"${event.name}" could not be saved to your account`, description: `${event.message} It is still saved in this browser.`, duration: 12_000 });
      break;
    case 'conflict':
      // The conflict dialog (shell/cloud-ui.tsx) is the notice.
      break;
  }
}

export const cloud: CloudSync = createCloudSync({
  // Every request names the account this tab is syncing for; the server refuses it if the cookie says otherwise.
  api: httpPlansApi(() => cloud.store.getState().userId),
  local: {
    read: readHouseholdRecord,
    write: writeHouseholdRecord,
    link: remoteLink,
    links: remoteLinks,
    setLink: setRemoteLink,
    newId: newHouseholdId,
    flush: saveNow,
  },
  open: {
    id: () => useProfileStore.getState().meta?.id ?? null,
    dirty: () => {
      const s = useProfileStore.getState();
      return s.revision !== s.savedRevision;
    },
    replace: (record) => useProfileStore.getState().load({ meta: { id: record.id, createdAt: record.createdAt, origin: record.origin }, profile: record.profile, scenarios: record.scenarios, run: record.run }),
  },
  read: readRemotePlan,
  emit: report,
  unauthenticated: () => signedOut(),
  now: Date.now,
});

export function useCloud<T>(selector: (state: CloudState) => T): T {
  return useStore(cloud.store, selector);
}

let started = false;
/** Starts following the account while someone is signed in. Call once at startup. */
export function startCloud() {
  if (started) return;
  started = true;
  onSyncedHouseholdSaved((id) => cloud.noteSaved(id));
  // Sync turned on or off for a household in another tab.
  onRemoteLinkChanged((id) => cloud.noteLinkChanged(id));
  const look = () => {
    if (document.visibilityState === 'visible') void cloud.refresh();
  };
  // Coming back into view: first see what other tabs stored about which plans are synced, then ask the account.
  const lookAgain = () => {
    if (document.visibilityState === 'visible') void refreshHouseholds().finally(look);
  };
  setInterval(look, POLL_MS);
  window.addEventListener('focus', lookAgain);
  window.addEventListener('online', look);
  document.addEventListener('visibilitychange', lookAgain);
}

const reason = (error: unknown) => (error instanceof Error ? error.message : String(error));

/** Opens a plan from the account, fetching a copy into this browser if there is none. */
export async function openCloudPlan(remoteId: string): Promise<boolean> {
  try {
    const localId = await cloud.adopt(remoteId);
    if (useProfileStore.getState().meta?.id !== localId && !(await openHousehold(localId))) {
      toast({ tone: 'bad', title: 'Could not open that plan', description: 'This browser is not letting FIRESight store data, and a plan from your account needs a copy here.' });
      return false;
    }
    // A copy that was already here may be behind the account.
    void cloud.refresh();
    return true;
  } catch (error) {
    const missing = error instanceof CloudError && error.kind === 'not_found';
    toast({ tone: 'bad', title: 'Could not open that plan', description: missing ? 'It is not in the account you are signed in to.' : error instanceof CloudError ? error.message : `What your account holds could not be read: ${reason(error)}.` });
    return false;
  }
}

/** Turns "Sync to account" on for a household in this browser. */
export async function startSync(localId: string): Promise<boolean> {
  try {
    await cloud.enable(localId);
    toast({ tone: 'good', title: 'Synced to your account', description: 'Changes you make here are saved there too, and AI tools you connect can read and edit this plan.' });
    return true;
  } catch (error) {
    toast({ tone: 'bad', title: 'Could not sync this plan', description: reason(error) });
    return false;
  }
}

/** Turns sync off. The household stays in this browser either way. */
export async function stopSync(localId: string, removeFromAccount: boolean): Promise<boolean> {
  try {
    await cloud.disable(localId, removeFromAccount);
    toast({ title: 'No longer synced', description: removeFromAccount ? 'The copy in your account was deleted. The plan stays in this browser.' : 'The plan stays in this browser. Your account keeps its copy as it is now.' });
    return true;
  } catch (error) {
    toast({ tone: 'bad', title: 'Could not stop syncing', description: reason(error) });
    return false;
  }
}

/** Deletes a household here and, when asked, its plan in the account too. */
export async function deleteSyncedHousehold(localId: string, fromAccount: boolean): Promise<boolean> {
  const link = remoteLink(localId);
  try {
    if (fromAccount && link) await cloud.removeRemote(link.id);
    else if (link) await cloud.disable(localId, false);
  } catch (error) {
    toast({ tone: 'bad', title: 'Could not delete it from your account', description: `${reason(error)} Nothing was deleted.` });
    return false;
  }
  await deleteHousehold(localId);
  return true;
}

/** Deletes a plan that is only in the account. */
export async function deleteCloudPlan(meta: RemoteMeta): Promise<boolean> {
  try {
    await cloud.removeRemote(meta.id);
    toast({ title: `Deleted "${meta.name}" from your account` });
    return true;
  } catch (error) {
    toast({ tone: 'bad', title: 'Could not delete that plan', description: reason(error) });
    return false;
  }
}

/** Settles a conflict on a household: load the account's version, or keep this one as a separate copy. */
export async function resolveConflict(localId: string, choice: 'theirs' | 'copy'): Promise<boolean> {
  try {
    const openAfter = await cloud.resolve(localId, choice);
    if (useProfileStore.getState().meta?.id === localId && openAfter !== localId) await openHousehold(openAfter);
    toast(
      choice === 'copy'
        ? { tone: 'good', title: 'Kept your version as a copy', description: 'The copy is open now and is saved in this browser only. The synced plan holds the newer version from your account.' }
        : { tone: 'good', title: 'Loaded the newer version', description: 'Your changes to this plan since the last sync were discarded.' },
    );
    return true;
  } catch (error) {
    toast({ tone: 'bad', title: 'Could not load the newer version', description: reason(error) });
    return false;
  }
}

/** Plans in the account that have no copy in this browser yet. */
export function useAccountOnlyPlans(): RemoteMeta[] {
  const remote = useCloud((s) => s.remote);
  const userId = useCloud((s) => s.userId);
  const list = useHouseholds((s) => s.list);
  if (!remote) return [];
  const here = new Set(list.filter((h) => h.remote?.userId === userId).map((h) => h.remote?.id));
  return remote.filter((meta) => !here.has(meta.id));
}

/** After an account is deleted: its plans are gone from the server, so local copies become ordinary local plans. */
export async function unlinkAccount(userId: string): Promise<void> {
  for (const [localId, link] of remoteLinks()) if (link.userId === userId) await setRemoteLink(localId, null);
}
