import { Suspense, useEffect, useMemo, useRef } from 'react';
import { getPool } from '@/engine';
import type { PlanPreview, Summary } from '@/engine/output-types';
import { useProfileStore } from '@/state/profile-store';
import { navigateToView, useActiveView } from '@/state/route';
import { rerun, useRunStore, type RunState } from '@/state/run-store';
import { useUi } from '@/state/ui-store';
import { Button, EmptyState, Icon, Skeleton, TabPanel, Tabs, count, cx, moneyCompact } from '@/ui';
import { AnimatedNumber, age, ofHundred } from '@/ui/charts';
import { VIEWS } from '@/views/registry';
import { pinnedHeight } from './scroll';
import type { ScenarioComparison, ViewProps } from '@/views/types';

const TABS = VIEWS.map((v) => ({ id: v.id, label: v.label }));

function statusLine(run: RunState): string {
  const speed = run.pathsPerSecond > 0 ? ` · ${count(Math.round(run.pathsPerSecond / 100) * 100)} lives a second` : '';
  switch (run.status) {
    case 'running':
      return run.done === 0 ? 'Starting the simulation…' : `Simulating: ${count(run.done)} of ${count(run.total)} lives${speed}`;
    case 'done':
      return `${count(run.total)} lives simulated in ${(run.elapsedMs / 1000).toFixed(1)} s${speed}`;
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

function Figure({ label, children, note, testId }: { label: string; children: React.ReactNode; note: React.ReactNode; testId: string }) {
  return (
    <div className="min-w-0" data-testid={testId}>
      <dt className="text-xs text-ink-3">{label}</dt>
      <dd className="mt-0.5 text-xl font-semibold tracking-tight whitespace-nowrap text-ink-1">{children}</dd>
      <dd className="mt-0.5 text-xs leading-snug text-pretty text-ink-2">{note}</dd>
    </div>
  );
}

/** A share as an animated count: "83 of 100" (of 1,000 when the share is under 1 in 100 either way). */
function Lives({ share, paths }: { share: number; paths: number }) {
  // Fewer than 100 lives simulated: count the lives themselves rather than imply a hundred.
  const of = paths < 100 ? paths : share > 0 && share < 1 && (share < 0.01 || share > 0.99) ? 1000 : 100;
  return (
    <>
      <AnimatedNumber value={share * of} format={(v) => String(Math.round(v))} />
      <span className="ml-1.5 text-sm font-medium text-ink-2">of {of === 1000 ? '1,000' : of}</span>
    </>
  );
}

/** The four figures every view answers to, tweened as the run refines. */
function HeadlineFigures({ summary, stale }: { summary: Summary; stale: boolean }) {
  const h = summary.headline;
  const fi = h.fi_age;
  const sp = summary.spending;
  const success = h.success_rate;
  const short = h.depletion_rate.value;
  const delivered = h.plan_delivered_share.value;
  const extra = h.extra_spending_ratio.value;
  const cutYears = sp.years_below_90.value;
  const homeSold = h.home_sale_rate.value;
  const interval = `${(success.lo * 100).toFixed(1)}% to ${(success.hi * 100).toFixed(1)}%, 95% sampling interval`;
  const cutNote = cutYears === null ? 'no retired years to measure' : cutYears < 0.005 ? 'never more than 10% under plan' : `under 90% of plan in ${Math.round(cutYears * 100)}% of retired years`;
  return (
    <dl className={cx('mt-4 grid grid-cols-2 gap-x-6 gap-y-4 transition-opacity duration-[var(--duration-slow)] @xl:grid-cols-4', stale && 'opacity-60')} data-testid="headline">
      <Figure
        testId="headline-fi-age"
        label="Work becomes optional"
        note={
          fi.p50 === null
            ? `It happens in only ${ofHundred(h.fi_rate.value).text} lives`
            : fi.p10 !== null && fi.p90 !== null
              ? `${age(fi.p10)} to ${age(fi.p90)} in 80 of 100 lives`
              : `from ${fi.p10 === null ? 'later' : age(fi.p10)}; 1 in 10 never`
        }
      >
        {fi.p50 === null ? <span className="text-ink-2">No typical age</span> : <AnimatedNumber value={fi.p50} format={(v) => v.toFixed(1)} />}
        {fi.p50 !== null && <span className="ml-1.5 text-sm font-medium text-ink-2">median age</span>}
      </Figure>
      <Figure
        testId="headline-success"
        label="The plan holds in"
        note={
          homeSold >= 0.005 ? (
            // Success counts a last-resort home sale as holding, so the two are never shown apart.
            <span title={interval} data-testid="headline-home-sale">
              but in {ofHundred(homeSold).text} a home is sold as a last resort
            </span>
          ) : (
            interval
          )
        }
      >
        <Lives share={success.value} paths={summary.paths} />
      </Figure>
      <Figure
        testId="headline-short"
        label="Money runs short in"
        note={short === 0 ? 'in no simulated life' : h.depletion_age.p50 === null ? 'of the simulated lives' : `typically around age ${Math.round(h.depletion_age.p50)}`}
      >
        <Lives share={short} paths={summary.paths} />
      </Figure>
      <Figure
        testId="headline-spending"
        label="Spending delivered"
        note={extra !== null && extra >= 0.005 ? `${cutNote}; ${Math.round(extra * 100)}% extra spent above plan` : cutNote}
      >
        {delivered === null ? <span className="text-ink-2">n/a</span> : <AnimatedNumber value={delivered * 100} format={(v) => `${v >= 99.5 && v < 100 ? '99' : Math.round(v)}%`} />}
        {delivered !== null && <span className="ml-1.5 text-sm font-medium text-ink-2">of plan</span>}
      </Figure>
    </dl>
  );
}

/** What the plan says before any simulation: the deterministic line. */
function PlanFacts({ preview }: { preview: PlanPreview }) {
  const facts: string[] = [`Spending ${moneyCompact(preview.annual_spending)} a year now, ${moneyCompact(preview.annual_spending_retired)} in retirement`];
  if (preview.funded_requirement > 0) {
    facts.push(`funded at ${moneyCompact(preview.funded_requirement)}${preview.funded_ratio !== null ? ` (${Math.round(Math.min(preview.funded_ratio, 9.99) * 100)}% there today)` : ''}`);
  }
  if (preview.annual_income > 0) facts.push(`saving ${Math.round(preview.savings_rate * 100)}% of income`);
  return (
    <p className="mt-3 text-xs leading-relaxed text-pretty text-ink-2" data-testid="plan-facts">
      <span className="text-ink-3">As entered, in today’s dollars: </span>
      {facts.join(' · ')}.
    </p>
  );
}

/** Plan title, the headline figures, run status and a hairline progress indicator. */
function HeadlineStrip({ run }: { run: RunState }) {
  const name = useProfileStore((s) => s.profile?.name);
  const description = useProfileStore((s) => s.profile?.description);
  const scenarioCount = useProfileStore((s) => s.scenarios.filter((x) => x.enabled).length);
  const fraction = run.total > 0 ? run.done / run.total : 0;
  const busy = run.status === 'running' || run.status === 'queued';
  return (
    <div className="@container relative px-5 pb-4 pt-5 lg:px-8">
      <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1">
        <h1 className="text-2xl font-semibold tracking-tight text-balance">{name || 'Untitled household'}</h1>
        <p className={cx('num text-xs text-pretty [&>span+span]:ml-2', run.status === 'blocked' || (run.status === 'error' && run.error?.kind !== 'not_implemented') ? 'text-bad' : 'text-ink-3')} role="status" data-testid="run-status">
          <span>{statusLine(run)}</span>
          {scenarioCount > 0 && run.status !== 'idle' && (
            <span className="text-ink-3">
              · baseline and {scenarioCount} {scenarioCount === 1 ? 'scenario' : 'scenarios'}
            </span>
          )}
        </p>
      </div>
      {description && <p className="mt-1 hidden max-w-2xl text-sm text-pretty text-ink-2 @2xl:block">{description}</p>}
      {run.summary && <HeadlineFigures summary={run.summary} stale={run.stale} />}
      {run.preview && <PlanFacts preview={run.preview} />}
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
  const pane = useRef<HTMLElement>(null);
  const view = VIEWS.find((v) => v.id === active) ?? VIEWS[0];

  const comparison = useMemo<ScenarioComparison[]>(
    () => run.scenarios.map((s) => ({ id: s.id, name: s.name, profile: s.profile, summary: s.summary, paired: s.paired })),
    [run.scenarios],
  );

  // A new view starts at its top: just under the tabs, keeping the figures out of the way if they had scrolled off.
  const viewId = view?.id;
  useEffect(() => {
    const el = pane.current;
    if (!el) return;
    const head = el.querySelector<HTMLElement>('[data-results-head]');
    const strip = el.querySelector<HTMLElement>('[data-headline-strip]');
    const pinned = head ? getComputedStyle(head).display !== 'contents' : true;
    el.scrollTop = pinned ? 0 : Math.min(el.scrollTop, strip?.offsetHeight ?? 0);
  }, [viewId]);

  // Whatever is pinned at the top covers that much of the pane: tell the browser, so focus and
  // scroll-into-view (keyboard users, "show me where") never land underneath it.
  const hasPlan = profile !== null;
  useEffect(() => {
    const el = pane.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const apply = () => {
      el.style.scrollPaddingTop = `${pinnedHeight(el) + 8}px`;
    };
    const observer = new ResizeObserver(apply);
    const head = el.querySelector('[data-headline-strip]');
    if (head) observer.observe(head);
    observer.observe(el);
    apply();
    return () => observer.disconnect();
  }, [hasPlan]);

  if (!profile || !view) return <EmptyState icon="folder" title="No plan is open" />;
  const props: ViewProps = { profile, summary: run.summary, comparison, run, pool: getPool() };
  const View = view.Component;

  return (
    <main ref={pane} data-results-scroll className="scroll-thin h-full min-w-0 overflow-y-auto bg-surface-0">
      {/* Pinned on roomy screens. On a phone, or a short window, the figures scroll away and only the tabs stay. */}
      <div data-results-head className="results-head z-20 bg-surface-0">
        <div data-headline-strip>
          <HeadlineStrip run={run} />
        </div>
        <div data-results-tabs className="sticky top-0 z-20 bg-surface-0">
          <Tabs tabs={TABS} active={view.id} onChange={navigateToView} idBase="view" aria-label="Results" className="px-5 lg:px-8" />
        </div>
      </div>
      {/* At least a pane tall, so the scroll position that hides the figures survives a change of view. */}
      <div className="min-h-full px-5 pb-16 lg:px-8">
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
