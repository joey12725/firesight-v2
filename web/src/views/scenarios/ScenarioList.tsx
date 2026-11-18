import { useState } from 'react';
import { describeOp } from '@/state/patches';
import { useProfileStore, type Scenario } from '@/state/profile-store';
import { useUi } from '@/state/ui-store';
import { Button, EmptyState, IconButton, Switch, Tag, TextInput } from '@/ui';

function ScenarioRow({ scenario }: { scenario: Scenario }) {
  const resolved = useProfileStore((s) => s.resolved[scenario.id]);
  const editing = useProfileStore((s) => s.editTarget === scenario.id);
  const { updateScenario, removeScenario, setEditTarget } = useProfileStore.getState();
  const [open, setOpen] = useState(false);
  const errors = resolved?.report?.errors.length ?? 0;
  return (
    <li className="border-b border-line py-3">
      <div className="flex items-center gap-3">
        <Switch aria-label={`Simulate ${scenario.name}`} checked={scenario.enabled} onChange={(enabled) => updateScenario(scenario.id, { enabled })} />
        <TextInput aria-label="Scenario name" className="w-64 max-w-full border-transparent bg-transparent font-medium hover:border-line" value={scenario.name} onChange={(name) => updateScenario(scenario.id, { name })} />
        <span className="mr-auto flex items-center gap-1.5">
          {resolved?.error && <Tag tone="bad" icon="alertCircle">No longer applies</Tag>}
          {errors > 0 && <Tag tone="bad" icon="alertCircle">{errors} to fix</Tag>}
          {editing && <Tag tone="cool" icon="pencil">Editing</Tag>}
        </span>
        <Button size="sm" variant="ghost" onClick={() => setOpen(!open)} aria-expanded={open}>
          {scenario.patch.length} {scenario.patch.length === 1 ? 'change' : 'changes'}
        </Button>
        <Button
          size="sm"
          icon="pencil"
          onClick={() => {
            setEditTarget(editing ? 'base' : scenario.id);
            if (!editing) useUi.getState().openSheet(window.matchMedia('(max-width: 767px)').matches ? 'editor' : null);
          }}
        >
          {editing ? 'Stop editing' : 'Edit'}
        </Button>
        <IconButton icon="trash" variant="danger" label={`Delete ${scenario.name}`} onClick={() => removeScenario(scenario.id)} />
      </div>
      {resolved?.error && <p className="mt-2 pl-11 text-xs text-bad">{resolved.error}</p>}
      {open && (
        <ul className="mt-2 space-y-1 pl-11">
          {scenario.patch.map((op, i) => (
            <li key={i} className="flex items-baseline justify-between gap-3 text-xs text-ink-2">
              <span className="min-w-0 truncate">{describeOp(op)}</span>
              <IconButton
                icon="x"
                size="sm"
                label="Remove this change"
                onClick={() => updateScenario(scenario.id, { patch: scenario.patch.filter((_, j) => j !== i) })}
              />
            </li>
          ))}
          {scenario.patch.length === 0 && <li className="text-xs text-ink-3">No changes yet. Choose Edit, then change anything in the plan.</li>}
        </ul>
      )}
    </li>
  );
}

/** Create, enable, rename, edit and delete scenarios. Scenarios are patches against the baseline. */
export function ScenarioList() {
  const scenarios = useProfileStore((s) => s.scenarios);
  const { addScenario, setEditTarget } = useProfileStore.getState();
  const create = () => {
    const result = addScenario({ name: `Scenario ${scenarios.length + 1}`, patch: [] });
    if (result.ok && result.id) setEditTarget(result.id);
  };
  return (
    <section aria-labelledby="scenario-list-title" className="mx-auto max-w-3xl pt-8">
      <div className="flex items-end justify-between border-b border-line-strong pb-2">
        <div>
          <h2 id="scenario-list-title" className="text-lg font-semibold tracking-tight">
            Scenarios
          </h2>
          <p className="text-sm text-ink-2">Variations of your plan, simulated through the same futures as the baseline.</p>
        </div>
        <Button icon="plus" onClick={create}>
          New scenario
        </Button>
      </div>
      {scenarios.length === 0 ? (
        <EmptyState compact icon="gitBranch" title="No scenarios yet">
          Create one and edit the plan while it is selected, or make any change to the baseline and use the branch button above the editor to turn that change into a scenario.
        </EmptyState>
      ) : (
        <ul>
          {scenarios.map((s) => (
            <ScenarioRow key={s.id} scenario={s} />
          ))}
        </ul>
      )}
    </section>
  );
}
