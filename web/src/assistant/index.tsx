// The assistant: Claude working inside FIRESight, on the user's plan, through the
// engine. This file is what the rest of the app imports; the panel, the agent loop and
// the model client load on demand. See docs/assistant.md for how it works.
//
//   <AssistantPanel onClose />         the right column on desktop, a sheet elsewhere
//   askAssistant(prompt, context?)     open the panel and ask, from any view
//   reviewPlan()                       the "Review my plan" audit
//   registerAssistantChart(kind, ...)  give the chat a chart to draw (see ./charts)
import { lazy, Suspense } from 'react';
import { useProfileStore } from '@/state/profile-store';
import { useSettings } from '@/state/settings-store';
import { useUi } from '@/state/ui-store';
import { Icon, IconButton, Skeleton } from '@/ui';
import { REVIEW_PROMPT } from './prompts';

export { registerAssistantChart, CHART_KINDS, CHART_SPECS, type AssistantChartProps, type ChartKind } from './charts';
export { REVIEW_PROMPT };

const Panel = lazy(() => import('./ui/Panel'));

export interface AssistantPanelProps {
  /** Present when the panel can be closed (always, today). */
  onClose?: () => void;
}

function Loading({ onClose }: AssistantPanelProps) {
  return (
    <aside aria-label="Assistant" aria-busy className="flex h-full min-h-0 flex-col bg-surface-1">
      <header className="flex h-11 shrink-0 items-center gap-1 border-b border-line pl-4 pr-2">
        <Icon name="sparkles" size={14} className="text-accent" />
        <h2 className="eyebrow mr-auto pl-1">Assistant</h2>
        {onClose && <IconButton icon="x" size="sm" label="Close the assistant" onClick={onClose} />}
      </header>
      <div className="space-y-3 px-4 py-5">
        <Skeleton className="h-5 w-3/4" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-5/6" />
      </div>
    </aside>
  );
}

export function AssistantPanel(props: AssistantPanelProps) {
  return (
    <Suspense fallback={<Loading {...props} />}>
      <Panel {...props} />
    </Suspense>
  );
}

function openPanel() {
  useSettings.getState().set({ assistantOpen: true });
  if (typeof window !== 'undefined' && !window.matchMedia('(min-width: 1280px)').matches) useUi.getState().openSheet('assistant');
}

/**
 * Opens the assistant and asks it something on the user's behalf. For entry points in
 * other views, such as a button under a chart: "Why do 9% of lives run short?".
 *
 * `context` is any small JSON-serialisable object describing what the user was looking
 * at (the view, the series, the figures on screen, a path id). The model receives it
 * as data alongside the question; keep it under a few kilobytes. If a turn is already
 * running the question waits its turn. If the assistant is not connected, the panel
 * opens with the question shown and explains how to connect.
 */
export function askAssistant(prompt: string, context?: unknown): void {
  openPanel();
  void import('./session').then(async ({ getSession }) => {
    const session = getSession();
    await session.load(useProfileStore.getState().meta?.id ?? null);
    await session.send({ text: prompt, context });
  });
}

/** Asks for an audit of the plan's assumptions: the three most consequential questionable inputs. */
export function reviewPlan(): void {
  askAssistant(REVIEW_PROMPT);
}

/** Removes a household's saved conversation, for when the household itself is deleted. */
export function deleteConversation(householdId: string): void {
  void import('./conversation').then((m) => m.deleteConversation(householdId));
}
