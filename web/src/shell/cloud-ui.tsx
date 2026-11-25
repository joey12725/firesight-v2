// Everything on screen that belongs to a synced plan: its switch and status in the
// household switcher, the list of plans in the account, and the questions syncing can
// raise (a conflict; whether deleting or un-syncing should also remove the account's
// copy). Loaded, with the sync code, only once someone is signed in.
import { useState } from 'react';
import { useAccount } from '@/auth/account';
import { describeWriter, type SyncStatus } from '@/state/cloud';
import { deleteCloudPlan, deleteSyncedHousehold, openCloudPlan, resolveConflict, startSync, stopSync, useAccountOnlyPlans, useCloud } from '@/state/cloud-store';
import { useHouseholds } from '@/state/households';
import { useProfileStore } from '@/state/profile-store';
import { Button, Dialog, Icon, IconButton, Switch, cx } from '@/ui';
import { useCloudUi } from './cloud-prompts';

function when(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '';
  const today = new Date().toDateString() === date.toDateString();
  return today ? `at ${date.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })}` : `on ${date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}`;
}

function ConflictDialog() {
  const localId = useProfileStore((s) => s.meta?.id ?? null);
  const name = useProfileStore((s) => s.profile?.name);
  const conflict = useCloud((s) => (localId ? s.conflicts[localId] : undefined));
  const postponed = useCloudUi((s) => s.postponed);
  const [busy, setBusy] = useState<'theirs' | 'copy' | null>(null);
  const key = localId && conflict ? `${localId}:${conflict.rev}` : null;
  const open = key !== null && key !== postponed;
  const later = () => {
    if (!busy) useCloudUi.setState({ postponed: key });
  };
  const choose = async (choice: 'theirs' | 'copy') => {
    if (!localId) return;
    setBusy(choice);
    await resolveConflict(localId, choice);
    setBusy(null);
  };
  return (
    <Dialog
      open={open}
      onClose={later}
      title="This plan was changed somewhere else"
      footer={
        <>
          <Button variant="ghost" disabled={busy !== null} onClick={later}>
            Decide later
          </Button>
          <Button disabled={busy !== null} onClick={() => void choose('copy')} data-testid="conflict-copy">
            {busy === 'copy' ? 'Copying…' : 'Keep mine as a copy'}
          </Button>
          <Button variant="primary" disabled={busy !== null} onClick={() => void choose('theirs')} data-testid="conflict-theirs">
            {busy === 'theirs' ? 'Loading…' : 'Load the newer version'}
          </Button>
        </>
      }
    >
      {conflict && (
        <div className="space-y-3 text-sm leading-relaxed text-ink-2" data-testid="conflict">
          <p>
            <span className="font-medium text-ink-1">{conflict.updated_by === 'web' ? 'Another browser' : describeWriter(conflict.updated_by)}</span> saved a newer version of "{name || conflict.name}" to your account{' '}
            {when(conflict.updated_at)}. This browser also has changes that had not been synced. One plan cannot hold both, so nothing has been overwritten on either side.
          </p>
          <ul className="space-y-1.5">
            <li>
              <span className="font-medium text-ink-1">Load the newer version</span> replaces what is open with the account's version. Your unsynced changes here are discarded.
            </li>
            <li>
              <span className="font-medium text-ink-1">Keep mine as a copy</span> saves what is open as a separate plan in this browser, and the synced plan takes the account's version. You can compare the two and carry over what
              you want.
            </li>
          </ul>
          <p className="text-xs text-ink-3">Until you choose, this plan keeps saving in this browser and is not sent to your account.</p>
        </div>
      )}
    </Dialog>
  );
}

function AccountCopyDialog() {
  const prompt = useCloudUi((s) => s.prompt);
  const [busy, setBusy] = useState(false);
  const close = () => {
    if (!busy) useCloudUi.setState({ prompt: null });
  };
  const run = async (fromAccount: boolean) => {
    if (!prompt) return;
    setBusy(true);
    if (prompt.kind === 'delete') await deleteSyncedHousehold(prompt.localId, fromAccount);
    else await stopSync(prompt.localId, fromAccount);
    setBusy(false);
    useCloudUi.setState({ prompt: null });
  };
  const deleting = prompt?.kind === 'delete';
  return (
    <Dialog
      open={prompt !== null}
      onClose={close}
      title={prompt ? (deleting ? `Delete "${prompt.name}"?` : `Stop syncing "${prompt.name}"?`) : ''}
      footer={
        <>
          <Button variant="ghost" disabled={busy} onClick={close}>
            Cancel
          </Button>
          <Button disabled={busy} onClick={() => void run(false)} data-testid="account-copy-keep">
            {deleting ? 'Only from this browser' : 'Keep the account copy'}
          </Button>
          <Button variant="danger" className="border border-bad" disabled={busy} onClick={() => void run(true)} data-testid="account-copy-remove">
            {deleting ? 'Delete everywhere' : 'Delete the account copy'}
          </Button>
        </>
      }
    >
      {prompt && (
        <div className="space-y-2 text-sm leading-relaxed text-ink-2">
          {deleting ? (
            <>
              <p>This plan is saved in this browser and synced to your account.</p>
              <p>
                <span className="font-medium text-ink-1">Only from this browser</span> leaves it in your account, where the AI tools you connected can still reach it and from where it can be opened here again.{' '}
                <span className="font-medium text-ink-1">Delete everywhere</span> removes it from the account too. Neither can be undone.
              </p>
            </>
          ) : (
            <>
              <p>The plan stays in this browser either way, and changes made here are no longer sent to your account.</p>
              <p>
                <span className="font-medium text-ink-1">Delete the account copy</span> takes it off the server, so your AI tools can no longer reach it. <span className="font-medium text-ink-1">Keep the account copy</span>{' '}
                leaves it there as it is now, as a plan of its own.
              </p>
            </>
          )}
        </div>
      )}
    </Dialog>
  );
}

const SYNC_NOTE: Record<SyncStatus, string> = {
  synced: 'Synced.',
  pending: 'Saving to your account…',
  syncing: 'Saving to your account…',
  offline: 'The server was not reached. Changes are kept here and sent when it answers.',
  conflict: 'Changed somewhere else as well. Nothing is sent until you choose a version.',
  error: 'Your account did not accept this plan.',
};

/** The open plan's "Sync to account" switch, with what is happening to it. */
function SyncRow({ id, name }: { id: string; name: string }) {
  const userId = useAccount((s) => s.user?.id);
  const link = useHouseholds((s) => s.list.find((h) => h.id === id)?.remote);
  const status = useCloud((s) => s.status[id]);
  const error = useCloud((s) => s.errors[id]);
  const storageAvailable = useHouseholds((s) => s.storageAvailable);
  const synced = link !== undefined && link.userId === userId;
  const note = !synced
    ? 'Off: this plan is stored in this browser only.'
    : status === 'error' && error
      ? error
      : status && status !== 'synced'
        ? SYNC_NOTE[status]
        : link.updatedBy === 'web'
          ? 'Synced.'
          : `Synced. Last saved by ${describeWriter(link.updatedBy)}.`;
  return (
    <div className="border-t border-line px-3 py-2.5" data-testid="sync-row" data-sync={synced ? (status ?? 'synced') : 'off'}>
      <div className="flex items-center justify-between gap-3">
        <span id="sync-label" className="flex items-center gap-1.5 text-xs font-medium text-ink-2">
          <Icon name="cloud" size={13} className="text-ink-3" />
          Sync this plan to your account
        </span>
        <Switch
          aria-labelledby="sync-label"
          aria-describedby="sync-note"
          checked={synced}
          disabled={!storageAvailable || (status === 'syncing' && !synced)}
          onChange={(on) => (on ? void startSync(id) : useCloudUi.getState().ask({ kind: 'stop-sync', localId: id, name }))}
        />
      </div>
      <p id="sync-note" className={cx('mt-1 text-xs leading-snug', status === 'conflict' || status === 'error' ? 'text-warn' : 'text-ink-3')}>
        {note}{' '}
        {status === 'conflict' && (
          <button type="button" className="font-medium text-accent-ink hover:underline" onClick={() => useCloudUi.getState().reopenConflict()}>
            Choose
          </button>
        )}
      </p>
    </div>
  );
}

/** The switcher's account part, for a signed-in person: the sync switch, then the plans that are only in the account. */
export function SwitcherAccountSection({ currentId, name, close }: { currentId: string | undefined; name: string; close: () => void }) {
  const accountOnly = useAccountOnlyPlans();
  return (
    <>
      {currentId && <SyncRow id={currentId} name={name} />}
      {accountOnly.length > 0 && (
        <div className="border-t border-line" data-testid="account-plans">
          <div className="px-3 pt-3">
            <span className="eyebrow">In your account</span>
          </div>
          <ul className="scroll-thin max-h-48 overflow-y-auto p-1.5">
            {accountOnly.map((plan) => (
              <li key={plan.id} className="group flex items-center">
                <button
                  type="button"
                  onClick={() => {
                    close();
                    void openCloudPlan(plan.id);
                  }}
                  className="flex min-w-0 flex-1 items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-surface-3"
                >
                  <Icon name="cloud" size={13} className="text-ink-3" />
                  <span className="min-w-0 flex-1 truncate">{plan.name}</span>
                  {plan.updated_by !== 'web' && <span className="max-w-[9rem] truncate text-2xs text-ink-3">from {describeWriter(plan.updated_by)}</span>}
                </button>
                <IconButton
                  icon="trash"
                  size="sm"
                  variant="danger"
                  label={`Delete ${plan.name} from your account`}
                  className="opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
                  onClick={() => {
                    if (window.confirm(`Delete "${plan.name}" from your account? It has no copy in this browser, so this cannot be undone.`)) void deleteCloudPlan(plan);
                  }}
                />
              </li>
            ))}
          </ul>
        </div>
      )}
    </>
  );
}

/** Landing page: the plans in the account that have no copy in this browser yet. */
export function LandingAccountPlans() {
  const inAccount = useAccountOnlyPlans();
  if (inAccount.length === 0) return null;
  return (
    <section aria-labelledby="account-title" className="mt-12" data-testid="landing-account-plans">
      <h2 id="account-title" className="eyebrow">
        In your account
      </h2>
      <ul className="mt-2 border-t border-line">
        {inAccount.slice(0, 8).map((plan) => (
          <li key={plan.id}>
            <button type="button" onClick={() => void openCloudPlan(plan.id)} className="group flex w-full items-center gap-3 border-b border-line py-2.5 text-left hover:bg-surface-3/50">
              <Icon name="cloud" size={14} className="text-ink-3" />
              <span className="min-w-0 flex-1 truncate text-sm font-medium">{plan.name}</span>
              {plan.updated_by !== 'web' && <span className="hidden max-w-[12rem] truncate text-xs text-ink-3 sm:inline">from {describeWriter(plan.updated_by)}</span>}
              <span className="num text-xs text-ink-3">{new Date(plan.updated_at).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' })}</span>
              <Icon name="arrowRight" size={14} className="text-ink-3 group-hover:text-accent" />
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** Mount once, beside the other dialogs. Renders nothing until a synced plan needs an answer. */
export function CloudDialogs() {
  return (
    <>
      <ConflictDialog />
      <AccountCopyDialog />
    </>
  );
}
