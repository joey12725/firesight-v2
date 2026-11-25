// Overview: "when can work become optional, and how sure is that?" at a glance. The
// reference implementation for the other views: every chart sits in a ChartCard with a
// takeaway computed from the data, the three charts over time share one crosshair and
// one time range, and everything refines in place as the run sharpens.
import { useMemo, useState } from 'react';
import type { Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { navigateToLife, navigateToView } from '@/state/route';
import type { RunState } from '@/state/run-store';
import { Icon, SegmentedControl, Skeleton, Tag, count } from '@/ui';
import { ChartCard, ChartSyncProvider, TimeRangeControl, summaryTime, type TimeRangePreset } from '@/ui/charts';
import { equityCounted, hasEquity, primaryCounted } from '../equity/model';
import type { ViewProps } from '../types';
import { FiAgeChart, fiAgeTable, type FiAgeMode } from './FiAgeChart';
import { FAN_SERIES_LABEL, NetWorthFan, defaultFanScale, fanTable, namedLives, type FanScale, type FanSeries, type NamedLife } from './NetWorthFan';
import { SpendingChart, SpendingLegend, spendingTable } from './SpendingChart';
import { StatusLegend, StatusStack, statusTable } from './StatusStack';
import { WhatCouldGoWrong, failureTable } from './WhatCouldGoWrong';
import { equitySentence, failureTakeaway, fanTakeaway, fiTakeaway, homeSaleSentence, outcomeSentence, samplingNote, spendingTakeaway, statusTakeaway } from './text';

function OverviewSkeleton() {
  return (
    <div className="mx-auto max-w-[76rem] space-y-10 py-8" aria-busy="true" aria-label="Simulating">
      <div className="space-y-3">
        <Skeleton className="h-3 w-28" />
        <Skeleton className="h-8 w-full max-w-2xl" />
        <Skeleton className="h-8 w-2/3 max-w-xl" />
      </div>
      <Skeleton className="h-48 w-full" />
      <Skeleton className="h-80 w-full" />
    </div>
  );
}

/** How far along the picture is: nothing while it is settled, a quiet tag while it is still coarse. */
function Sharpness({ run, summary }: { run: RunState; summary: Summary }) {
  if (run.stale) return <Tag>Updating…</Tag>;
  if (run.status === 'running' && summary.paths < run.total) return <Tag tone={summary.paths < 1000 ? 'warn' : 'neutral'}>{summary.paths < 1000 ? `Early picture: ${count(summary.paths)} lives so far` : `Sharpening: ${count(summary.paths)} lives so far`}</Tag>;
  if (summary.paths < 100) return <Tag tone="warn" icon="alertTriangle">{`Only ${summary.paths} lives simulated: treat every figure as rough`}</Tag>;
  if (summary.paths < 1000) return <Tag tone="warn">{`${count(summary.paths)} lives: shares are good to about ±${Math.round(100 / Math.sqrt(summary.paths))} in 100`}</Tag>;
  return null;
}

function Hero({ summary, profile, run }: { summary: Summary; profile: Profile; run: RunState }) {
  const [mode, setMode] = useState<FiAgeMode>('dots');
  const sentence = useMemo(() => outcomeSentence(summary), [summary]);
  const homeSale = useMemo(() => homeSaleSentence(summary), [summary]);
  // Only for a plan the Equity tab is shown for: a company with nothing in it yet has no view to open and nothing to count.
  const equity = useMemo(() => (hasEquity(profile) ? equitySentence(summary, equityCounted(profile), primaryCounted(profile)) : null), [summary, profile]);
  const who = profile.household.people[0]?.name;
  return (
    <div className="grid grid-cols-1 items-start gap-x-12 gap-y-8 @4xl:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
      <div>
        <div className="flex min-h-5 flex-wrap items-center gap-2">
          <p className="eyebrow">The plan as entered</p>
          <Sharpness run={run} summary={summary} />
        </div>
        <p data-testid="outcome-sentence" className="mt-3 max-w-[34rem] text-2xl font-medium tracking-tight text-pretty text-ink-1 @4xl:text-[1.75rem] @4xl:leading-[2.25rem]">
          {sentence.map((s, i) =>
            s.strong ? (
              <b key={i} className="font-semibold">
                {s.text}
              </b>
            ) : (
              <span key={i} className="text-ink-2">
                {s.text}
              </span>
            ),
          )}
        </p>
        {homeSale && (
          <p data-testid="home-sale-sentence" className="mt-3 max-w-[34rem] text-base leading-relaxed text-pretty text-ink-2">
            {homeSale.map((seg, i) => (seg.strong ? <b key={i} className="font-semibold text-ink-1">{seg.text}</b> : <span key={i}>{seg.text}</span>))}
          </p>
        )}
        {equity && (
          // One card, no side run: the Equity view holds the comparison with the equity at zero.
          <button
            type="button"
            data-testid="overview-equity"
            onClick={() => navigateToView('equity')}
            className="group mt-4 block w-full max-w-[34rem] rounded-md border border-line bg-surface-1 px-4 py-3 text-left transition-colors duration-[var(--duration-fast)] hover:border-line-strong hover:bg-surface-2"
          >
            <span className="flex items-center justify-between gap-3 text-xs font-medium text-ink-2">
              Startup equity
              <span className="inline-flex items-center gap-1 text-accent-ink">
                Open the Equity view
                <Icon name="arrowRight" size={12} className="transition-transform duration-[var(--duration-base)] group-hover:translate-x-0.5" />
              </span>
            </span>
            <span className="mt-1 block text-sm leading-relaxed text-pretty text-ink-1">{equity}</span>
          </button>
        )}
        <p className="mt-3 max-w-[34rem] text-xs leading-relaxed text-pretty text-ink-3">{samplingNote(summary)}</p>
      </div>
      <ChartCard
        title="Age work becomes optional"
        takeaway={fiTakeaway(summary)}
        caption={
          mode === 'dots'
            ? `Each dot is one of ${summary.paths < 100 ? `the ${summary.paths} simulated` : '100 equally likely'} lives, placed at ${who ? `${who}’s` : 'the'} age when savings first meet the plan’s funded test. Hollow dots never get there.`
            : 'The share of lives in which work has become optional by each age, with its 95% sampling interval.'
        }
        controls={
          <SegmentedControl
            aria-label="Show as"
            size="sm"
            value={mode}
            onChange={setMode}
            options={[
              { value: 'dots', label: summary.paths < 100 ? `${summary.paths} lives` : '100 lives' },
              { value: 'cumulative', label: 'By this age' },
            ]}
          />
        }
        table={() => fiAgeTable(summary.fi)}
        filename="age-work-becomes-optional"
        explain={{ context: () => ({ home_sale_rate: summary.headline.home_sale_rate, fi_age: summary.headline.fi_age, fi_rate: summary.headline.fi_rate, never: summary.fi.never, paths: summary.paths }) }}
        dimmed={run.stale}
      >
        <FiAgeChart curve={summary.fi} spread={summary.headline.fi_age} ages={summary.axis.ages} years={summary.axis.years} mode={mode} lives={summary.paths} />
      </ChartCard>
    </div>
  );
}

function presets(summary: Summary): TimeRangePreset[] {
  const [start, end] = [summary.axis.ages[0] ?? 0, summary.axis.ages[summary.axis.ages.length - 1] ?? 0];
  const retire = summary.headline.retirement_age.p50 ?? summary.headline.median_fi_age;
  const out: TimeRangePreset[] = [{ id: 'all', label: 'Whole plan', range: null }];
  if (retire !== null && retire - start >= 4 && end - retire >= 6) {
    const p90 = summary.headline.retirement_age.p90 ?? retire;
    out.push({ id: 'working', label: 'To retirement', range: [start, Math.min(end, Math.ceil(p90) + 2)] });
    out.push({ id: 'retired', label: 'Retirement', range: [Math.max(start, Math.floor(summary.headline.retirement_age.p10 ?? retire) - 2), end] });
  }
  return out;
}

function LifeLinks({ summary, onHighlight }: { summary: Summary; onHighlight: (why: NamedLife['why'] | null) => void }) {
  const pick = (label: string) => summary.picks.find((p) => p.label === label);
  const links = [
    { pick: pick('p10'), why: 'p10' as const, text: 'an unlucky life' },
    { pick: pick('median'), why: 'median' as const, text: 'a middling one' },
    { pick: pick('p90'), why: 'p90' as const, text: 'a lucky one' },
  ].filter((l) => l.pick);
  if (links.length === 0) return null;
  return (
    <span>
      {' '}
      Open{' '}
      {links.map((l, i) => (
        <span key={l.why}>
          {i > 0 && (i === links.length - 1 ? ' or ' : ', ')}
          <button
            type="button"
            className="rounded-xs font-medium text-accent-ink hover:underline"
            onClick={() => l.pick && navigateToLife(l.pick.path_index, l.why)}
            onMouseEnter={() => onHighlight(l.why)}
            onMouseLeave={() => onHighlight(null)}
            onFocus={() => onHighlight(l.why)}
            onBlur={() => onHighlight(null)}
          >
            {l.text}
          </button>
        </span>
      ))}{' '}
      (ranked 10th, 50th and 90th of 100 by wealth at the end).
    </span>
  );
}

export default function OverviewView({ profile, summary, run }: ViewProps) {
  const [series, setSeries] = useState<FanSeries>('net_worth');
  const [chosenScale, setScale] = useState<FanScale | null>(null);
  const [highlight, setHighlight] = useState<NamedLife['why'] | null>(null);
  const named = useMemo(() => (summary ? namedLives(summary).length : 0), [summary]);
  const autoScale = useMemo(() => (summary ? defaultFanScale(summary, series) : 'linear'), [summary, series]);
  const scale = chosenScale ?? autoScale;
  const time = useMemo(() => (summary ? summaryTime(summary) : null), [summary]);
  const ranges = useMemo(() => (summary ? presets(summary) : []), [summary]);

  if (!summary || !time) {
    // Errors and blocked plans are explained by the workspace above; nothing to draw here.
    return run.status === 'running' || run.status === 'queued' ? <OverviewSkeleton /> : null;
  }
  const dimmed = run.stale;
  const context = () => ({ paths: summary.paths, success_rate: summary.headline.success_rate, home_sale_rate: summary.headline.home_sale_rate, median_fi_age: summary.headline.median_fi_age });

  return (
    <ChartSyncProvider>
      <div className="@container mx-auto max-w-[76rem] pb-8 pt-7">
        <Hero summary={summary} profile={profile} run={run} />

        <div className="mt-12 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-line pt-5">
          <h2 className="text-base font-semibold tracking-tight">Across a lifetime</h2>
          <TimeRangeControl time={time} presets={ranges} className="ml-auto" />
          <p className="hidden basis-full text-xs text-ink-3 @2xl:block">
            The three charts below share one age axis and one crosshair. Drag across the first to zoom all of them. Every figure is in today’s dollars.
          </p>
        </div>

        <ChartCard
          className="mt-6"
          title={`${FAN_SERIES_LABEL[series]} among the living`}
          takeaway={fanTakeaway(summary, series)}
          caption={
            <>
              Shading shows how likely each level is at that age (darker is more likely, each age scaled to its own peak), over households with someone still alive: the strip beneath shows how many
              that is. Lines are the median and the 5th, 25th, 75th and 95th percentiles; no single life follows one. The thin grey lines are real simulated lives
              {named > 0 ? ', the darker ones the named lives below' : ''}: hover to pick one out, click to read it.
              <LifeLinks summary={summary} onHighlight={setHighlight} />
            </>
          }
          captionText="Shading: likelihood at each age among households with someone alive (darker is more likely). Lines: median and 5th, 25th, 75th, 95th percentiles. Thin grey lines: real simulated lives. Today's dollars."
          controls={
            <>
              <SegmentedControl
                aria-label="Quantity"
                size="sm"
                value={series}
                onChange={setSeries}
                options={[
                  { value: 'net_worth', label: 'Net worth' },
                  { value: 'investable', label: 'Investable' },
                ]}
              />
              <SegmentedControl
                aria-label="Scale"
                size="sm"
                value={scale}
                onChange={setScale}
                options={[
                  { value: 'linear', label: 'Linear' },
                  { value: 'log', label: 'Log' },
                ]}
              />
            </>
          }
          table={() => fanTable(summary, series)}
          filename={series === 'net_worth' ? 'net-worth-by-age' : 'investable-assets-by-age'}
          explain={{ context: () => ({ ...context(), series, terminal_net_worth: summary.headline.terminal_net_worth, investable_at_retirement: summary.headline.investable_at_retirement }) }}
          dimmed={dimmed}
        >
          <NetWorthFan summary={summary} profile={profile} series={series} scale={scale} highlight={highlight} />
        </ChartCard>

        <div className="mt-12 grid grid-cols-1 gap-x-12 gap-y-12 @5xl:grid-cols-2">
          <ChartCard
            title="Life stages by age"
            takeaway={statusTakeaway(summary)}
            caption="Every simulated life is in exactly one stage each year. “On plan” is spending at least 95% of the plan; “wealth doubled” is on plan with investable assets at least twice their level at retirement, in today’s dollars; “short of essentials” means liquid assets could not cover essential spending."
            table={() => statusTable(summary)}
            filename="life-stages-by-age"
            explain={{ context: () => ({ ...context(), depletion_rate: summary.headline.depletion_rate, depletion_age: summary.headline.depletion_age, years_cutting_share: summary.headline.years_cutting_share }) }}
            dimmed={dimmed}
          >
            <StatusLegend summary={summary} className="mb-2" />
            <StatusStack summary={summary} />
          </ChartCard>

          <ChartCard
            title="Spending against the plan"
            takeaway={spendingTakeaway(summary)}
            caption="The dashed line is what the plan intends to spend in the median life (it includes home, healthcare and care costs, so it moves with age). Bands are what lives actually spend; spending above the line is the spending rule sharing good markets, and is counted apart from the share of plan delivered. The plan can hold while spending is cut, which is why this chart sits beside the success figure."
            table={() => spendingTable(summary)}
            filename="spending-by-age"
            explain={{ context: () => ({ ...context(), plan_delivered_share: summary.headline.plan_delivered_share, extra_spending_ratio: summary.headline.extra_spending_ratio, paths_below_90: summary.spending.paths_below_90, paths_below_75: summary.spending.paths_below_75, years_cut: summary.spending.years_cut, worst_cut: summary.spending.worst_cut }) }}
            dimmed={dimmed}
          >
            <div className="mb-2">
              <SpendingLegend />
            </div>
            <SpendingChart summary={summary} />
          </ChartCard>
        </div>

        <ChartCard
          className="mt-12 border-t border-line pt-5"
          title="What could go wrong"
          takeaway={failureTakeaway(summary)}
          caption={
            summary.failure.failed > 0 ? (
              <>
                <Icon name="info" size={12} className="mr-1 inline align-[-2px]" />
                One first cause per life, by the first rule that applies in the order shown in the engine’s documentation; real failures usually have several. {count(summary.failure.failed)} of{' '}
                {count(summary.paths)} lives fall short.
              </>
            ) : undefined
          }
          table={() => failureTable(summary)}
          filename="what-could-go-wrong"
          explain={{ context: () => ({ ...context(), failed: summary.failure.failed, causes: summary.failure.causes.filter((c) => c.count > 0), factors: summary.failure.factors }) }}
          dimmed={dimmed}
        >
          <WhatCouldGoWrong summary={summary} />
        </ChartCard>
      </div>
    </ChartSyncProvider>
  );
}
