import { useEffect, useRef } from 'react';
import { selectCanRedo, selectCanUndo, useProfileStore } from '@/state/profile-store';
import { toast, useUi } from '@/state/ui-store';
import { Button, Icon, IconButton, Select, type IconName } from '@/ui';
import { useProfile } from './hooks';
import { PlanCheck, sectionForPath } from './PlanCheck';
import { SECTION_TITLE } from './realism';
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
  { id: 'household', icon: 'users', title: SECTION_TITLE.household, paths: ['/household', '/name'], summary: summary.householdSummary, Body: HouseholdSection },
  { id: 'income', icon: 'briefcase', title: SECTION_TITLE.income, paths: ['/incomes'], summary: summary.incomeSummary, Body: IncomeSection },
  { id: 'spending', icon: 'creditCard', title: SECTION_TITLE.spending, paths: ['/spending'], summary: summary.spendingSummary, Body: SpendingSection },
  { id: 'accounts', icon: 'landmark', title: SECTION_TITLE.accounts, paths: ['/accounts', '/portfolio'], summary: summary.accountsSummary, Body: AccountsSection },
  { id: 'property', icon: 'home', title: SECTION_TITLE.property, paths: ['/properties', '/debts', '/debt_policy'], summary: summary.propertySummary, Body: PropertySection },
  { id: 'events', icon: 'calendar', title: SECTION_TITLE.events, paths: ['/events'], summary: summary.eventsSummary, Body: EventsSection },
  { id: 'retirement', icon: 'sunrise', title: SECTION_TITLE.retirement, paths: ['/retirement'], summary: summary.retirementSummary, Body: RetirementSection },
  { id: 'assumptions', icon: 'sliders', title: SECTION_TITLE.assumptions, paths: ['/assumptions', '/simulation'], summary: summary.assumptionsSummary, Body: AssumptionsSection },
];

/** Opens the section holding `path`, scrolls its field into view and focuses it. */
function useFocusPath(container: React.RefObject<HTMLElement | null>) {
  const focusPath = useUi((s) => s.focusPath);
  useEffect(() => {
    if (!focusPath) return;
    const ui = useUi.getState();
    ui.toggleSection(sectionForPath(focusPath), true);
    const locate = () => {
      const nodes = [...(container.current?.querySelectorAll<HTMLElement>('[data-path]') ?? [])];
      return nodes
        .filter((n) => focusPath === n.dataset.path || focusPath.startsWith(`${n.dataset.path}/`) || n.dataset.path?.startsWith(`${focusPath}/`))
        .sort((a, b) => (b.dataset.path?.length ?? 0) - (a.dataset.path?.length ?? 0))[0];
    };
    const section = () => container.current?.querySelector<HTMLElement>(`[data-section="${sectionForPath(focusPath)}"]`);
    const show = (target: HTMLElement | null | undefined) => {
      target?.scrollIntoView({ block: 'center', behavior: 'smooth' });
      target?.querySelector<HTMLElement>('input, select, button')?.focus({ preventScroll: true });
      ui.setFocusPath(null);
    };
    // Wait for the section body to render.
    let frame = requestAnimationFrame(() => {
      const best = locate();
      if (best?.dataset.path === focusPath) return show(best);
      // The field may be in a folded group of optional realism: open those around it and look again.
      const folded = (best ?? section())?.querySelectorAll<HTMLElement>('.realism > button[aria-expanded="false"]');
      if (!folded?.length) return show(best ?? section());
      folded.forEach((button) => button.click());
      frame = requestAnimationFrame(() => show(locate() ?? section()));
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
