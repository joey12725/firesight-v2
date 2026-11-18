// The scenario bench: every what-if as a row that says, in words, what it changes
// against the plan, with the controls to rename, switch on and off, edit, copy, delete
// or adopt it; and one-press buttons for the what-ifs people ask most. A scenario is a
// patch against the baseline, so "before" is always the plan as it stands now.
import { useMemo } from 'react';
import type { Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { useProfileStore, type Scenario } from '@/state/profile-store';
import type { RunState } from '@/state/run-store';
import { toast, useUi } from '@/state/ui-store';
import { Button, Icon, IconButton, Popover, Switch, Tag, TextInput, Tooltip, cx } from '@/ui';
import type { ScenarioComparison } from '../types';
import { describePatch, type ChangeLine } from './describe';
import { flipsOf, formatDelta } from './model';
import { QUICK_GROUPS, quickScenarios, samePatch, type QuickScenario } from './quick';
import { useScenarioUi } from './store';

function openEditor(id: string) {
  const store = useProfileStore.getState();
  const editing = store.editTarget === id;
  store.setEditTarget(editing ? 'base' : id);
  if (!editing && window.matchMedia('(max-width: 767px)').matches) useUi.getState().openSheet('editor');
}

function duplicate(scenario: Scenario) {
  const result = useProfileStore.getState().addScenario({ name: `${scenario.name} (copy)`, patch: scenario.patch, enabled: scenario.enabled });
  if (result.ok && result.id) useScenarioUi.getState().select(result.id);
  else if (!result.ok) toast({ title: 'The scenario could not be copied', description: result.error, tone: 'bad' });
}

/** Makes the scenario the plan: its changes are applied to the baseline and the scenario is retired. Undo restores both. */
function promote(scenario: Scenario) {
  const store = useProfileStore.getState();
  const result = store.edit(scenario.patch, { target: 'base', label: `Make “${scenario.name}” the plan` });
  if (!result.ok) {
    toast({ title: 'The scenario could not become the plan', description: result.error, tone: 'bad' });
    return;
  }
  useProfileStore.getState().removeScenario(scenario.id);
  if (useScenarioUi.getState().selected === scenario.id) useScenarioUi.getState().select(null);
  toast({
    title: `“${scenario.name}” is now the plan`,
    description: 'Its changes were applied to the baseline. Other scenarios now compare against the new plan.',
    tone: 'good',
    action: {
      label: 'Undo',
      run: () => {
        // Two steps: the scenario's removal, then the baseline edit.
        useProfileStore.getState().undo();
        useProfileStore.getState().undo();
      },
    },
  });
}

function remove(scenario: Scenario) {
  useProfileStore.getState().removeScenario(scenario.id);
  if (useScenarioUi.getState().selected === scenario.id) useScenarioUi.getState().select(null);
  toast({ title: `“${scenario.name}” deleted`, action: { label: 'Undo', run: () => useProfileStore.getState().undo() } });
}

function Change({ line, onRemove }: { line: ChangeLine; onRemove?: () => void }) {
  return (
    <li className="group flex items-baseline gap-2 text-sm leading-relaxed">
      <span className="min-w-0 text-pretty text-ink-2">
        {line.label}:{' '}
        {line.before !== null && (
          <>
            <span className="text-ink-3 line-through decoration-ink-3/50">{line.before}</span>
            <span aria-hidden className="mx-1.5 text-ink-3">
              →
            </span>
            <span className="sr-only"> becomes </span>
          </>
        )}
        <span className="font-medium text-ink-1">{line.after}</span>
      </span>
      {onRemove && <IconButton icon="x" size="sm" className="-my-1 shrink-0 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 max-md:opacity-100" label="Remove this change from the scenario" onClick={onRemove} />}
    </li>
  );
}

/** What the run says about this scenario so far, as a short tag. */
function Outcome({ scenario, result, running }: { scenario: Scenario; result: ScenarioComparison | undefined; running: boolean }) {
  if (!scenario.enabled) return <span className="text-xs text-ink-3">Not simulated</span>;
  if (!result) return running ? <span className="text-xs text-ink-3">Queued…</span> : null;
  const flips = result.paired ? flipsOf(result.paired) : null;
  if (!flips) return <span className="text-xs text-ink-3">{result.summary ? 'Pairing lives…' : 'Simulating…'}</span>;
  const moved = flips.rescued + flips.harmed > 0;
  return (
    <span className="num text-xs whitespace-nowrap text-ink-2">
      Plan holds: <b className="font-semibold text-ink-1">{moved ? formatDelta(flips.net.value, 'share') : 'no change'}</b>
      {moved && flips.withinNoise && <span className="text-ink-3"> · within noise</span>}
    </span>
  );
}

function Row({ scenario, base, result, running, selected }: { scenario: Scenario; base: Profile; result: ScenarioComparison | undefined; running: boolean; selected: boolean }) {
  const resolved = useProfileStore((s) => s.resolved[scenario.id]);
  const editing = useProfileStore((s) => s.editTarget === scenario.id);
  const { updateScenario } = useProfileStore.getState();
  const lines = useMemo(() => describePatch(base, scenario.patch, resolved?.profile ?? null), [base, scenario.patch, resolved?.profile]);
  const errors = resolved?.report?.errors.length ?? 0;
  const broken = Boolean(resolved?.error) || errors > 0;
  const select = () => useScenarioUi.getState().select(scenario.id);

  return (
    <li className={cx('relative border-b border-line py-4 pl-4', !scenario.enabled && 'opacity-80')} data-testid={`scenario-${scenario.id}`}>
      <span aria-hidden className={cx('absolute top-4 bottom-4 left-0 w-0.5 rounded-full', selected ? 'bg-ink-1' : 'bg-transparent')} />
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <Switch aria-label={`Simulate ${scenario.name}`} checked={scenario.enabled} onChange={(enabled) => updateScenario(scenario.id, { enabled })} />
        <TextInput
          aria-label="Scenario name"
          className="h-8 min-w-0 flex-1 basis-40 border-transparent bg-transparent px-1.5 text-base font-semibold tracking-tight hover:border-line focus:border-line-strong"
          value={scenario.name}
          onChange={(name) => updateScenario(scenario.id, { name })}
        />
        <span className="flex items-center gap-1.5">
          {resolved?.error && (
            <Tag tone="bad" icon="alertCircle">
              No longer applies
            </Tag>
          )}
          {errors > 0 && (
            <Tag tone="bad" icon="alertCircle">
              {errors} to fix
            </Tag>
          )}
          {editing && (
            <Tag tone="cool" icon="pencil">
              Editing
            </Tag>
          )}
        </span>
        <span className="ml-auto flex items-center gap-1">
          <Button size="sm" icon="pencil" onClick={() => openEditor(scenario.id)} aria-pressed={editing}>
            {editing ? 'Stop editing' : <>Edit<span className="hidden @xl:inline"> in the plan editor</span></>}
          </Button>
          <Popover label={`More actions for ${scenario.name}`} placement="bottom-end" trigger={<IconButton icon="more" size="sm" label="More actions" />} className="w-56 p-1">
            {(close) => (
              <div className="flex flex-col" role="menu">
                <MenuItem
                  icon="copy"
                  onClick={() => {
                    close();
                    duplicate(scenario);
                  }}
                >
                  Duplicate
                </MenuItem>
                <MenuItem
                  icon="checkCircle"
                  disabled={broken || scenario.patch.length === 0}
                  onClick={() => {
                    close();
                    promote(scenario);
                  }}
                >
                  Make this the plan
                </MenuItem>
                <MenuItem
                  icon="trash"
                  danger
                  onClick={() => {
                    close();
                    remove(scenario);
                  }}
                >
                  Delete
                </MenuItem>
              </div>
            )}
          </Popover>
        </span>
      </div>

      {resolved?.error && <p className="mt-2 text-xs text-bad">{resolved.error}</p>}
      <ul className="mt-2 space-y-0.5">
        {lines.map((line) => (
          <Change key={line.key} line={line} onRemove={scenario.patch.length > 0 ? () => updateScenario(scenario.id, { patch: scenario.patch.filter((_, j) => !line.ops.includes(j)) }) : undefined} />
        ))}
        {scenario.patch.length === 0 && (
          <li className="text-sm text-ink-3">
            No changes yet, so it is the plan itself. Choose <b className="font-medium text-ink-2">Edit</b>, then change anything: the edits land here and the plan stays as it is.
          </li>
        )}
      </ul>
      <div className="mt-2.5 flex flex-wrap items-center gap-x-4 gap-y-1">
        <Outcome scenario={scenario} result={result} running={running} />
        {scenario.enabled && result?.summary && !selected && (
          <button type="button" className="inline-flex items-center gap-1 rounded-xs text-xs font-medium text-accent-ink hover:underline" onClick={select}>
            Compare with the plan
            <Icon name="arrowRight" size={12} />
          </button>
        )}
        {selected && <span className="text-xs text-ink-3">Compared with the plan below</span>}
      </div>
    </li>
  );
}

function MenuItem({ icon, children, onClick, disabled, danger }: { icon: 'copy' | 'checkCircle' | 'trash'; children: string; onClick: () => void; disabled?: boolean; danger?: boolean }) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cx('flex h-8 items-center gap-2 rounded-xs px-2 text-left text-sm disabled:pointer-events-none disabled:opacity-45', danger ? 'text-bad hover:bg-bad-soft' : 'text-ink-1 hover:bg-surface-3')}
    >
      <Icon name={icon} size={14} />
      {children}
    </button>
  );
}

function QuickCreate({ profile, summary, scenarios, empty }: { profile: Profile; summary: Summary | null; scenarios: Scenario[]; empty: boolean }) {
  const quick = useMemo(() => quickScenarios(profile, summary), [profile, summary]);
  const create = (q: QuickScenario) => {
    const existing = scenarios.find((s) => samePatch(s.patch, q.patch));
    if (existing) {
      // Already on the bench: switch it on and show it instead of making a twin.
      if (!existing.enabled) useProfileStore.getState().updateScenario(existing.id, { enabled: true });
      useScenarioUi.getState().select(existing.id);
      return;
    }
    const result = useProfileStore.getState().addScenario({ name: q.name, patch: q.patch });
    if (result.ok && result.id) useScenarioUi.getState().select(result.id);
    else if (!result.ok) toast({ title: 'That what-if does not fit this plan', description: result.error, tone: 'bad' });
  };
  const groups = QUICK_GROUPS.map((g) => ({ group: g, items: quick.filter((q) => q.group === g) })).filter((g) => g.items.length > 0);
  if (groups.length === 0) return null;
  return (
    <div className={cx(empty ? 'mt-5' : 'mt-6')}>
      <h3 className="text-sm font-medium text-ink-1">{empty ? 'Start with a common what-if' : 'Try another what-if'}</h3>
      <p className="mt-0.5 text-xs text-ink-3">One press makes the scenario and runs it through the same simulated lives as the plan.</p>
      <dl className="mt-3 grid grid-cols-1 gap-x-8 gap-y-3 @3xl:grid-cols-[max-content_minmax(0,1fr)]">
        {groups.map(({ group, items }) => (
          <div key={group} className="contents">
            <dt className="eyebrow pt-1.5">{group}</dt>
            <dd className="flex flex-wrap gap-1.5">
              {items.map((q) => {
                const made = scenarios.some((s) => samePatch(s.patch, q.patch));
                return (
                  <Tooltip key={q.key} content={<span className="block max-w-64 text-pretty">{q.hint}</span>}>
                    <Button size="sm" icon={made ? 'check' : 'plus'} onClick={() => create(q)} data-testid={`quick-${q.key}`} className={cx(made && 'text-ink-2')}>
                      {q.label}
                    </Button>
                  </Tooltip>
                );
              })}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

export interface BenchProps {
  profile: Profile;
  summary: Summary | null;
  comparison: ScenarioComparison[];
  run: RunState;
  selected: string | null;
}

export function Bench({ profile, summary, comparison, run, selected }: BenchProps) {
  const scenarios = useProfileStore((s) => s.scenarios);
  const blank = () => {
    const store = useProfileStore.getState();
    const result = store.addScenario({ name: `Scenario ${scenarios.length + 1}`, patch: [] });
    if (result.ok && result.id) {
      store.setEditTarget(result.id);
      useScenarioUi.getState().select(result.id);
      if (window.matchMedia('(max-width: 767px)').matches) useUi.getState().openSheet('editor');
    }
  };
  const running = run.status === 'running' || run.status === 'queued';
  const enabled = scenarios.filter((s) => s.enabled).length;

  return (
    <section aria-labelledby="scenario-bench-title">
      <div className="flex flex-wrap items-end justify-between gap-x-4 gap-y-2 border-b border-line-strong pb-2">
        <div>
          <h2 id="scenario-bench-title" className="text-base font-semibold tracking-tight">
            {scenarios.length === 0 ? 'No scenarios yet' : 'Your scenarios'}
          </h2>
          {scenarios.length > 0 && (
            <p className="text-xs text-ink-3">
              {enabled} of {scenarios.length} switched on. Each one is simulated in full beside the plan, so every extra scenario adds a run.
            </p>
          )}
        </div>
        <Button icon="plus" size="sm" onClick={blank}>
          Blank scenario
        </Button>
      </div>
      {scenarios.length === 0 ? (
        <p className="mt-3 max-w-[40rem] text-sm leading-relaxed text-pretty text-ink-2">
          A scenario is the plan with one decision changed. Press a what-if below, or make a blank scenario and edit the plan while it is selected: the edits go to the scenario and the plan stays as it is.
        </p>
      ) : (
        <ul>
          {scenarios.map((s) => (
            <Row key={s.id} scenario={s} base={profile} result={comparison.find((c) => c.id === s.id)} running={running} selected={selected === s.id} />
          ))}
        </ul>
      )}
      <QuickCreate profile={profile} summary={summary} scenarios={scenarios} empty={scenarios.length === 0} />
    </section>
  );
}
