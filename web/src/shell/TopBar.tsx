import { closeHousehold, deleteHousehold, openHousehold, useHouseholds } from '@/state/households';
import { selectDirty, useProfileStore } from '@/state/profile-store';
import { rerun, useRunStore } from '@/state/run-store';
import { useSettings } from '@/state/settings-store';
import { useUi } from '@/state/ui-store';
import { Button, Icon, IconButton, NumberInput, Popover, SegmentedControl, Select, Tooltip, count, countCompact, cx, useMediaQuery } from '@/ui';
import { exportCurrentPlan } from './actions';

const PATH_PRESETS = [2_000, 10_000, 50_000, 100_000, 250_000];

function Wordmark() {
  return (
    <span className="flex items-center gap-2 pr-1">
      <svg width="20" height="20" viewBox="0 0 64 64" aria-hidden>
        <path d="M10 50 C24 47 31 39 35 29 C39 20 46 14 56 12" fill="none" stroke="var(--accent)" strokeWidth="7" strokeLinecap="round" />
        <path d="M10 50 C26 50 40 47 56 36" fill="none" stroke="var(--accent)" strokeOpacity=".4" strokeWidth="7" strokeLinecap="round" />
      </svg>
      <span className="hidden text-base font-semibold tracking-tight sm:inline">FIRESight</span>
    </span>
  );
}

function HouseholdSwitcher() {
  const name = useProfileStore((s) => s.profile?.name);
  const currentId = useProfileStore((s) => s.meta?.id);
  const dirty = useProfileStore(selectDirty);
  const { list, storageAvailable } = useHouseholds();
  const openDialog = useUi((s) => s.openDialog);
  return (
    <Popover
      label="Households"
      className="w-80"
      trigger={
        <button type="button" className="flex h-8 min-w-0 max-w-[16rem] items-center gap-1.5 rounded-sm px-2 text-sm font-medium hover:bg-surface-3">
          <span className="truncate">{name || 'Untitled household'}</span>
          <Icon name="chevronDown" size={13} className="text-ink-3" />
        </button>
      }
    >
      {(close) => (
        <div>
          <div className="flex items-center justify-between px-3 pt-3">
            <span className="eyebrow">Saved in this browser</span>
            <span className="text-2xs text-ink-3">{!storageAvailable ? 'Storage unavailable' : dirty ? 'Saving…' : 'Saved'}</span>
          </div>
          <ul className="scroll-thin max-h-64 overflow-y-auto p-1.5">
            {list.map((h) => (
              <li key={h.id} className="group flex items-center">
                <button
                  type="button"
                  aria-current={h.id === currentId || undefined}
                  onClick={() => {
                    close();
                    if (h.id !== currentId) void openHousehold(h.id);
                  }}
                  className={cx('flex min-w-0 flex-1 items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-surface-3', h.id === currentId && 'font-medium')}
                >
                  <Icon name="check" size={13} className={h.id === currentId ? 'text-accent' : 'invisible'} />
                  <span className="min-w-0 flex-1 truncate">{h.name}</span>
                  {h.origin.startsWith('demo:') && <span className="text-2xs text-ink-3">demo</span>}
                </button>
                <IconButton
                  icon="trash"
                  size="sm"
                  variant="danger"
                  label={`Delete ${h.name}`}
                  className="opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
                  onClick={() => {
                    if (window.confirm(`Delete "${h.name}" from this browser? This cannot be undone.`)) void deleteHousehold(h.id);
                  }}
                />
              </li>
            ))}
            {list.length === 0 && <li className="px-2 py-1.5 text-xs text-ink-3">Nothing saved yet.</li>}
          </ul>
          <div className="grid grid-cols-3 gap-1 border-t border-line p-1.5">
            <Button
              size="sm"
              variant="ghost"
              icon="plus"
              onClick={() => {
                close();
                void closeHousehold();
              }}
            >
              New
            </Button>
            <Button
              size="sm"
              variant="ghost"
              icon="upload"
              onClick={() => {
                close();
                openDialog('import');
              }}
            >
              Import
            </Button>
            <Button
              size="sm"
              variant="ghost"
              icon="download"
              onClick={() => {
                close();
                exportCurrentPlan();
              }}
            >
              Export
            </Button>
          </div>
        </div>
      )}
    </Popover>
  );
}

function RunStatus() {
  const run = useRunStore();
  const seed = useProfileStore((s) => s.run.seed);
  const paths = useProfileStore((s) => s.run.paths);
  const setRun = useProfileStore((s) => s.setRun);
  const state =
    run.status === 'running'
      ? { dot: 'bg-accent animate-pulse', text: `${count(run.done)} / ${count(run.total)}` }
      : run.status === 'done'
        ? { dot: 'bg-good', text: `${count(run.total)} lives` }
        : run.status === 'queued'
          ? { dot: 'bg-ink-3', text: 'Queued' }
          : run.status === 'blocked'
            ? { dot: 'bg-bad', text: 'Plan has errors' }
            : run.status === 'error'
              ? { dot: run.error?.kind === 'not_implemented' ? 'bg-warn' : 'bg-bad', text: 'Not simulated' }
              : { dot: 'bg-ink-3', text: 'Idle' };
  return (
    <Popover
      label="Run details"
      placement="bottom-end"
      className="w-64 p-3"
      trigger={
        <button type="button" className="flex h-8 items-center gap-2 rounded-sm px-2 text-xs text-ink-2 hover:bg-surface-3" aria-label={`Run status: ${state.text}`}>
          <span aria-hidden className={cx('size-1.5 rounded-full', state.dot)} />
          <span className="num hidden md:inline">{state.text}</span>
          <span className="num sm:hidden">{countCompact(paths)}</span>
          {run.pathsPerSecond > 0 && (run.status === 'running' || run.status === 'done') && (
            <span className="num hidden text-ink-3 lg:inline">{countCompact(run.pathsPerSecond)}/s</span>
          )}
          <span className="num hidden text-ink-3 xl:inline">seed {seed}</span>
        </button>
      }
    >
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
        <dt className="text-ink-3">Lives simulated</dt>
        <dd className="num text-right">
          {count(run.done)} of {count(run.total)}
        </dd>
        <dt className="text-ink-3">Speed</dt>
        <dd className="num text-right">{run.pathsPerSecond > 0 ? `${count(Math.round(run.pathsPerSecond))} lives/s` : 'n/a'}</dd>
        <dt className="text-ink-3">Time</dt>
        <dd className="num text-right">{run.elapsedMs > 0 ? `${(run.elapsedMs / 1000).toFixed(1)} s` : 'n/a'}</dd>
      </dl>
      <div className="mt-3 border-t border-line pt-3 sm:hidden">
        <span className="mb-1 block text-xs font-medium text-ink-2">Lives to simulate</span>
        <PathControl inPopover />
      </div>
      <div className="mt-3 border-t border-line pt-3">
        <label htmlFor="run-seed" className="mb-1 block text-xs font-medium text-ink-2">
          Random seed
        </label>
        <div className="flex gap-1.5">
          <NumberInput id="run-seed" kind="integer" min={0} value={seed} onChange={(v) => v !== null && v >= 0 && setRun({ seed: v })} />
          <Button onClick={() => setRun({ seed: Math.floor(Math.random() * 1_000_000) })}>New</Button>
        </div>
        <p className="mt-1.5 text-xs leading-snug text-ink-3">The same seed replays the same futures, so two plans can be compared life by life.</p>
        <Button size="sm" variant="ghost" icon="rotate" className="-ml-2 mt-2" onClick={rerun}>
          Run again
        </Button>
      </div>
    </Popover>
  );
}

function PathControl({ inPopover }: { inPopover?: boolean }) {
  const paths = useProfileStore((s) => s.run.paths);
  const setRun = useProfileStore((s) => s.setRun);
  const wide = useMediaQuery('(min-width: 1100px)') && !inPopover;
  const options = PATH_PRESETS.map((n) => ({ value: String(n), label: countCompact(n) }));
  if (!PATH_PRESETS.includes(paths)) options.push({ value: String(paths), label: countCompact(paths) });
  return wide ? (
    <Tooltip content="Lives to simulate. More lives sharpen the picture and take longer.">
      <span className="inline-flex">
        <SegmentedControl aria-label="Lives to simulate" size="sm" value={String(paths)} options={options} onChange={(v) => setRun({ paths: Number(v) })} />
      </span>
    </Tooltip>
  ) : (
    <Select aria-label="Lives to simulate" className={inPopover ? 'w-full' : 'w-[6.5rem]'} value={String(paths)} options={options.map((o) => ({ ...o, label: `${o.label} lives` }))} onChange={(v) => setRun({ paths: Number(v) })} />
  );
}

export function TopBar({ compact }: { compact: boolean }) {
  const hasPlan = useProfileStore((s) => s.profile !== null);
  const openDialog = useUi((s) => s.openDialog);
  const openSheet = useUi((s) => s.openSheet);
  const { editorOpen, assistantOpen, set } = useSettings();
  return (
    <header className="flex h-[var(--topbar-height)] shrink-0 items-center gap-1.5 border-b border-line bg-surface-1 px-3">
      {hasPlan &&
        (compact ? (
          <IconButton icon="menu" label="Edit the plan" onClick={() => openSheet('editor')} />
        ) : (
          <IconButton icon="panelLeft" label={editorOpen ? 'Hide the plan editor' : 'Show the plan editor'} pressed={editorOpen} onClick={() => set({ editorOpen: !editorOpen })} />
        ))}
      <Wordmark />
      {hasPlan && (
        <>
          <span aria-hidden className="mx-1 h-4 w-px bg-line" />
          <HouseholdSwitcher />
        </>
      )}
      <span className="flex-1" />
      {hasPlan && (
        <>
          <RunStatus />
          <span className="hidden sm:inline-flex">
            <PathControl />
          </span>
          <span aria-hidden className="mx-1 hidden h-4 w-px bg-line sm:block" />
          <IconButton icon="share" label="Share this plan" onClick={() => openDialog('share')} />
        </>
      )}
      <IconButton icon="settings" label="Settings" onClick={() => openDialog('settings')} />
      {hasPlan && (
        <IconButton
          icon="sparkles"
          label={assistantOpen ? 'Hide the assistant' : 'Show the assistant'}
          pressed={!compact && assistantOpen}
          onClick={() => (window.matchMedia('(min-width: 1280px)').matches ? set({ assistantOpen: !assistantOpen }) : openSheet('assistant'))}
        />
      )}
    </header>
  );
}
