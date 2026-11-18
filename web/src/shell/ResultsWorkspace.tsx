import { Suspense, useMemo } from 'react';
import { getPool } from '@/engine';
import { useProfileStore } from '@/state/profile-store';
import { navigateToView, useActiveView } from '@/state/route';
import { rerun, useRunStore, type RunState } from '@/state/run-store';
import { useUi } from '@/state/ui-store';
import { Button, EmptyState, Icon, Skeleton, TabPanel, Tabs, count, cx } from '@/ui';
import { VIEWS } from '@/views/registry';
import type { ScenarioComparison, ViewProps } from '@/views/types';

const TABS = VIEWS.map((v) => ({ id: v.id, label: v.label }));

function statusLine(run: RunState): string {
  switch (run.status) {
    case 'running':
      return run.done === 0 ? 'Starting the simulation…' : `Simulating: ${count(run.done)} of ${count(run.total)} lives`;
    case 'done':
      return `${count(run.total)} lives simulated in ${(run.elapsedMs / 1000).toFixed(1)} s`;
    case 'queued':
      return 'Change noted. Simulating in a moment…';
    case 'blocked':
      return 'The plan has errors, so it cannot be simulated yet';
    case 'error':
      return run.error?.kind === 'not_implemented' ? 'The simulation engine is not part of this build yet' : 'The simulation stopped';
    case 'idle':
      return '';
  }
}

/** Plan title, run status and a hairline progress indicator. Headline figures land here. */
function HeadlineStrip({ run }: { run: RunState }) {
  const name = useProfileStore((s) => s.profile?.name);
  const description = useProfileStore((s) => s.profile?.description);
  const scenarioCount = useProfileStore((s) => s.scenarios.filter((x) => x.enabled).length);
  const fraction = run.total > 0 ? run.done / run.total : 0;
  const busy = run.status === 'running' || run.status === 'queued';
  return (
    <div className="relative px-5 pb-4 pt-5 lg:px-8">
      <h1 className="text-2xl font-semibold tracking-tight text-balance">{name || 'Untitled household'}</h1>
      {description && <p className="mt-1 max-w-2xl text-sm text-pretty text-ink-2">{description}</p>}
      <p className={cx('mt-2 flex items-center gap-2 text-xs', run.status === 'blocked' || (run.status === 'error' && run.error?.kind !== 'not_implemented') ? 'text-bad' : 'text-ink-3')} role="status">
        <span>{statusLine(run)}</span>
        {scenarioCount > 0 && run.status !== 'idle' && (
          <span className="text-ink-3">
            · baseline and {scenarioCount} {scenarioCount === 1 ? 'scenario' : 'scenarios'}
          </span>
        )}
      </p>
      {/* Headline figures (summary.headline: success_rate, fi_age, ...) render here once the engine can run. */}
      <div aria-hidden className="absolute inset-x-0 bottom-0 h-px overflow-hidden bg-line">
        {busy && run.done === 0 ? (
          <div className="h-full w-1/4 animate-progress bg-accent" />
        ) : (
          <div
            className={cx('h-full origin-left bg-accent transition-[transform,opacity] duration-[var(--duration-slow)]', !busy && 'opacity-0')}
            style={{ transform: `scaleX(${fraction})` }}
          />
        )}
      </div>
    </div>
  );
}

/** Shown in the results area when there is nothing to draw, and why. */
function RunNotice({ run }: { run: RunState }) {
  const errorCount = useProfileStore((s) => s.report.errors.length);
  const first = useProfileStore((s) => s.report.errors[0]);
  if (run.status === 'blocked') {
    return (
      <Notice tone="bad" icon="alertCircle" title={`Fix ${errorCount} ${errorCount === 1 ? 'problem' : 'problems'} in the plan to simulate it`}>
        <p>{first?.message}</p>
        {first && (
          <Button size="sm" className="mt-3" onClick={() => useUi.getState().setFocusPath(first.path)}>
            Show me where
          </Button>
        )}
      </Notice>
    );
  }
  if (run.status === 'error' && run.error) {
    if (run.error.kind === 'not_implemented') {
      return (
        <Notice tone="warn" icon="info" title="No results yet: the simulation engine is not part of this build">
          <p>
            The plan editor, validation, scenarios, saving and sharing all work. The part of the engine that simulates lives is still being built, so there are no figures to show, and this app never
            shows made-up ones. Results will appear here as soon as the engine can run.
          </p>
          <p className="num mt-2 text-xs text-ink-3">{run.error.message}</p>
        </Notice>
      );
    }
    return (
      <Notice tone="bad" icon="alertCircle" title="The simulation stopped">
        <p>{run.error.message}</p>
        <Button size="sm" icon="rotate" className="mt-3" onClick={rerun}>
          Try again
        </Button>
      </Notice>
    );
  }
  return null;
}

function Notice({ tone, icon, title, children }: { tone: 'bad' | 'warn'; icon: 'alertCircle' | 'info'; title: string; children: React.ReactNode }) {
  return (
    <div role={tone === 'bad' ? 'alert' : 'status'} data-testid="run-notice" className="mx-auto mt-6 flex max-w-3xl gap-3 border-y border-line py-4">
      <Icon name={icon} className={cx('mt-0.5', tone === 'bad' ? 'text-bad' : 'text-warn')} />
      <div className="min-w-0 text-sm leading-relaxed text-ink-2">
        <h2 className="font-medium text-ink-1">{title}</h2>
        <div className="mt-1">{children}</div>
      </div>
    </div>
  );
}

function ViewFallback() {
  return (
    <div className="mx-auto max-w-3xl space-y-3 py-8">
      <Skeleton className="h-3 w-32" />
      <Skeleton className="h-7 w-96 max-w-full" />
      <Skeleton className="h-48 w-full" />
    </div>
  );
}

/** The centre column: headline strip, view tabs, and the active view. */
export function ResultsWorkspace() {
  const profile = useProfileStore((s) => s.profile);
  const run = useRunStore();
  const active = useActiveView();
  const view = VIEWS.find((v) => v.id === active) ?? VIEWS[0];

  const comparison = useMemo<ScenarioComparison[]>(
    () => run.scenarios.map((s) => ({ id: s.id, name: s.name, profile: s.profile, summary: s.summary, paired: null })),
    [run.scenarios],
  );

  if (!profile || !view) return <EmptyState icon="folder" title="No plan is open" />;
  const props: ViewProps = { profile, summary: run.summary, comparison, run, pool: getPool() };
  const View = view.Component;

  return (
    <main className="flex h-full min-h-0 min-w-0 flex-col bg-surface-0">
      <HeadlineStrip run={run} />
      <Tabs tabs={TABS} active={view.id} onChange={navigateToView} idBase="view" aria-label="Results" className="shrink-0 px-5 lg:px-8" />
      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-5 pb-16 lg:px-8">
        <RunNotice run={run} />
        <TabPanel idBase="view" id={view.id}>
          <Suspense fallback={<ViewFallback />}>
            <View {...props} />
          </Suspense>
        </TabPanel>
      </div>
    </main>
  );
}
