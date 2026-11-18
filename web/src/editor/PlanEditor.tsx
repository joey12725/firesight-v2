import { useEffect, useRef } from 'react';
import { selectCanRedo, selectCanUndo, useProfileStore } from '@/state/profile-store';
import { toast, useUi } from '@/state/ui-store';
import { Button, Icon, IconButton, Select, type IconName } from '@/ui';
import { useProfile } from './hooks';
import { PlanCheck, sectionForPath } from './PlanCheck';
import { Section } from './Section';
import { AccountsSection } from './sections/Accounts';
import { AssumptionsSection } from './sections/Assumptions';
import { EventsSection } from './sections/Events';
import { HouseholdSection } from './sections/Household';
import { IncomeSection } from './sections/Income';
import { PropertySection } from './sections/Property';
import { RetirementSection } from './sections/Retirement';
import { SpendingSection } from './sections/Spending';
import * as summary from './summaries';
import type { Profile } from '@/engine/types';

interface SectionDef {
  id: string;
  icon: IconName;
  title: string;
  paths: string[];
  summary: (p: Profile) => string;
  Body: () => React.ReactNode;
}

export const SECTIONS: SectionDef[] = [
  { id: 'household', icon: 'users', title: 'Household', paths: ['/household', '/name'], summary: summary.householdSummary, Body: HouseholdSection },
  { id: 'income', icon: 'briefcase', title: 'Income', paths: ['/incomes'], summary: summary.incomeSummary, Body: IncomeSection },
  { id: 'spending', icon: 'creditCard', title: 'Spending', paths: ['/spending'], summary: summary.spendingSummary, Body: SpendingSection },
  { id: 'accounts', icon: 'landmark', title: 'Savings and accounts', paths: ['/accounts', '/portfolio'], summary: summary.accountsSummary, Body: AccountsSection },
  { id: 'property', icon: 'home', title: 'Property and debts', paths: ['/properties', '/debts', '/debt_policy'], summary: summary.propertySummary, Body: PropertySection },
  { id: 'events', icon: 'calendar', title: 'Life events', paths: ['/events'], summary: summary.eventsSummary, Body: EventsSection },
  { id: 'retirement', icon: 'sunrise', title: 'Retirement plan', paths: ['/retirement'], summary: summary.retirementSummary, Body: RetirementSection },
  { id: 'assumptions', icon: 'sliders', title: 'Assumptions', paths: ['/assumptions', '/simulation'], summary: summary.assumptionsSummary, Body: AssumptionsSection },
];

/** Opens the section holding `path`, scrolls its field into view and focuses it. */
function useFocusPath(container: React.RefObject<HTMLElement | null>) {
  const focusPath = useUi((s) => s.focusPath);
  useEffect(() => {
    if (!focusPath) return;
    const ui = useUi.getState();
    ui.toggleSection(sectionForPath(focusPath), true);
    // Wait for the section body to render.
    const frame = requestAnimationFrame(() => {
      const nodes = [...(container.current?.querySelectorAll<HTMLElement>('[data-path]') ?? [])];
      const best = nodes
        .filter((n) => focusPath === n.dataset.path || focusPath.startsWith(`${n.dataset.path}/`) || n.dataset.path?.startsWith(`${focusPath}/`))
        .sort((a, b) => (b.dataset.path?.length ?? 0) - (a.dataset.path?.length ?? 0))[0];
      const target = best ?? container.current?.querySelector<HTMLElement>(`[data-section="${sectionForPath(focusPath)}"]`);
      target?.scrollIntoView({ block: 'center', behavior: 'smooth' });
      target?.querySelector<HTMLElement>('input, select, button')?.focus({ preventScroll: true });
      ui.setFocusPath(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [focusPath, container]);
}

function EditorToolbar({ onClose }: { onClose?: () => void }) {
  const canUndo = useProfileStore(selectCanUndo);
  const canRedo = useProfileStore(selectCanRedo);
  const undoLabel = useProfileStore((s) => s.past.at(-1)?.label);
  const lastIsBaseEdit = useProfileStore((s) => {
    const e = s.past.at(-1);
    return Boolean(e && e.target === 'base' && e.ops.length > 0);
  });
  const scenarios = useProfileStore((s) => s.scenarios);
  const editTarget = useProfileStore((s) => s.editTarget);
  const { undo, redo, setEditTarget, moveLastEditToScenario } = useProfileStore.getState();

  const toScenario = () => {
    const result = moveLastEditToScenario();
    if (result.ok) {
      toast({ tone: 'good', title: 'Moved to a scenario', description: 'The baseline is back as it was. The scenario is simulated alongside it.' });
    } else {
      toast({ tone: 'bad', title: 'Could not make a scenario', description: result.error });
    }
  };

  return (
    <div className="flex h-11 shrink-0 items-center gap-1 border-b border-line px-3">
      <span className="eyebrow mr-auto pl-1">Plan</span>
      {scenarios.length > 0 && (
        <Select
          aria-label="Editing"
          className="w-40"
          value={editTarget}
          options={[{ value: 'base', label: 'Editing: baseline' }, ...scenarios.map((s) => ({ value: s.id, label: `Editing: ${s.name}` }))]}
          onChange={setEditTarget}
        />
      )}
      <IconButton icon="gitBranch" label="Make the last change a scenario instead" disabled={!lastIsBaseEdit} onClick={toScenario} />
      <IconButton icon="undo" label={canUndo ? `Undo: ${undoLabel}` : 'Undo'} hint="⌘Z" disabled={!canUndo} onClick={undo} />
      <IconButton icon="redo" label="Redo" hint="⇧⌘Z" disabled={!canRedo} onClick={redo} />
      {onClose && <IconButton icon="x" label="Close the editor" onClick={onClose} />}
    </div>
  );
}

function ScenarioBanner() {
  const editTarget = useProfileStore((s) => s.editTarget);
  const scenario = useProfileStore((s) => s.scenarios.find((x) => x.id === s.editTarget));
  const error = useProfileStore((s) => s.resolved[s.editTarget]?.error);
  if (editTarget === 'base' || !scenario) return null;
  return (
    <div className="border-b border-line bg-cool-soft px-4 py-2.5">
      <div className="flex items-center gap-2">
        <Icon name="gitBranch" size={13} className="text-cool" />
        <p className="min-w-0 flex-1 truncate text-xs text-cool-ink">
          Editing scenario <span className="font-medium">{scenario.name}</span>. Changes here leave the baseline alone.
        </p>
        <Button size="sm" variant="ghost" onClick={() => useProfileStore.getState().setEditTarget('base')}>
          Back to baseline
        </Button>
      </div>
      {error && <p className="mt-1 text-xs text-bad">This scenario no longer applies to the baseline: {error}</p>}
    </div>
  );
}

/** The left column: the whole plan as collapsible, hand-built sections. */
export function PlanEditor({ onClose }: { onClose?: () => void }) {
  const profile = useProfile();
  const ref = useRef<HTMLDivElement>(null);
  useFocusPath(ref);
  return (
    <div className="flex h-full min-h-0 flex-col bg-surface-1">
      <EditorToolbar onClose={onClose} />
      <div ref={ref} className="scroll-thin min-h-0 flex-1 overflow-y-auto">
        <ScenarioBanner />
        <PlanCheck />
        {SECTIONS.map(({ id, icon, title, paths, summary: summarise, Body }) => (
          <Section key={id} id={id} icon={icon} title={title} paths={paths} summary={summarise(profile)}>
            <Body />
          </Section>
        ))}
        <p className="px-4 py-4 text-xs leading-relaxed text-ink-3">Every amount is in today's dollars. Rates are per year.</p>
      </div>
    </div>
  );
}
