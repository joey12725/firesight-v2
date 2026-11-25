// Scenarios: "if I change this decision, what happens to the same simulated lives?"
// The bench lists the what-ifs; the board ranks them all on one measure with honest
// intervals; the detail compares one of them with the plan life by life, because every
// scenario runs through exactly the futures the plan did (same seed, same path indices).
import { useMemo, useState } from 'react';
import type { Summary } from '@/engine/output-types';
import { useProfileStore } from '@/state/profile-store';
import type { RunState } from '@/state/run-store';
import { SegmentedControl, Select, Skeleton, Tag, count } from '@/ui';
import { ChartCard, ChartSyncProvider, TimeRangeControl, summaryTime, type TimeRangePreset } from '@/ui/charts';
import { DecisionTable, decisionTableData } from '../equity/charts';
import { decisionTable, fiAgeShift } from '../equity/model';
import { DECISIONS_CAPTION, decisionSentence } from '../equity/text';
import type { ScenarioComparison, ViewProps } from '../types';
import { Bench } from './Bench';
import { Board, BoardKey } from './Board';
import { DeltaDots, deltaTable } from './DeltaDots';
import { FlipBar, FlipLegend, flipTable } from './FlipBar';
import { MetricTable, metricTableData, metricTakeaway } from './MetricTable';
import { PAIRED_LABEL, PairedFan, PairedLegend, pairedFanTable, type PairedSeries } from './PairedFan';
import { Rank, rankTable } from './Rank';
import { SameLife } from './SameLife';
import { StageLegend, StageShift, stageTable } from './StageShift';
import { METRICS, flipsOf, metricOf, rankRows, scalarOf, stageDiffs } from './model';
import { scrollResultsTo } from '@/shell/scroll';
import { useScenarioUi } from './store';
import { deltaTakeaway, fanTakeaway, flipNote, flipSentence, homeSaleLine, pairedNote, provisionalSentence, rankTakeaway, stageTakeaway, type Segment } from './text';

const DELTA_SCALARS = [
  { value: 'terminal_net_worth', label: 'Wealth at the end' },
  { value: 'fi_age', label: 'Work optional' },
  { value: 'lifetime_spending', label: 'Spending' },
  { value: 'lifetime_tax', label: 'Tax' },
] as const;
type DeltaScalar = (typeof DELTA_SCALARS)[number]['value'];

function Sentence({ segments, testId }: { segments: Segment[]; testId?: string }) {
  return (
    <p data-testid={testId} className="mt-3 max-w-[36rem] text-xl font-medium tracking-tight text-pretty text-ink-1 @4xl:text-2xl @4xl:leading-8">
      {segments.map((s, i) =>
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
  );
}

/** How far to trust the comparison right now. */
function Sharpness({ run, scenario, base }: { run: RunState; scenario: ScenarioComparison; base: Summary }) {
  if (run.stale) return <Tag>Updating…</Tag>;
  const lives = scenario.summary?.paths ?? 0;
  if (run.status === 'running' || run.status === 'queued') return <Tag tone={lives < 1000 ? 'warn' : 'neutral'}>{`${lives < 1000 ? 'Early picture' : 'Sharpening'}: ${count(lives)} lives so far`}</Tag>;
  if (!scenario.paired) return <Tag>Pairing lives…</Tag>;
  if (scenario.paired.paths < 100) return <Tag tone="warn" icon="alertTriangle">{`Only ${scenario.paired.paths} lives paired: every difference here is rough`}</Tag>;
  if (scenario.paired.paths < 1000) return <Tag tone="warn">{`${count(scenario.paired.paths)} lives paired: small differences will read as noise`}</Tag>;
  if (base.paths !== scenario.paired.paths) return <Tag>{`${count(scenario.paired.paths)} lives paired`}</Tag>;
  return null;
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

/**
 * Brings the detail's heading to the top of the results pane. Not `scrollIntoView`: that
 * also scrolls the document behind the app shell, which pushes the toolbar off screen.
 */
function scrollToDetail() {
  scrollResultsTo(document.getElementById('scenario-detail-title'));
}

function ViewSkeleton() {
  return (
    <div className="space-y-6 py-6" aria-busy="true" aria-label="Simulating">
      <Skeleton className="h-8 w-full max-w-2xl" />
      <Skeleton className="h-8 w-2/3 max-w-xl" />
      <Skeleton className="h-56 w-full" />
    </div>
  );
}

function Detail({ base, scenario, run, profile, pool, many }: { base: Summary; scenario: ScenarioComparison & { summary: Summary }; run: RunState; profile: ViewProps['profile']; pool: ViewProps['pool']; many: ScenarioComparison[] }) {
  const [scalarKey, setScalarKey] = useState<DeltaScalar>('terminal_net_worth');
  const time = useMemo(() => summaryTime(base), [base]);
  const ranges = useMemo(() => presets(base), [base]);
  const paired = scenario.paired;
  const flips = useMemo(() => (paired ? flipsOf(paired) : null), [paired]);
  const diffs = useMemo(() => stageDiffs(base, scenario.summary), [base, scenario.summary]);
  const settled = run.status === 'done' && !run.stale;
  // Paired figures describe finished runs: hold them at reduced strength while a new run is in flight.
  const dimmed = run.stale;
  const pairedDimmed = !settled;
  const name = scenario.name;
  const options = DELTA_SCALARS.filter((o) => (paired ? (scalarOf(paired, o.value)?.both ?? 0) > 0 : true));
  const scalar = paired ? (scalarOf(paired, options.some((o) => o.value === scalarKey) ? scalarKey : (options[0]?.value ?? 'terminal_net_worth')) ?? null) : null;
  const headline = flips ? flipSentence(flips) : provisionalSentence(base, scenario.summary);
  const saleLine = homeSaleLine(paired, base, scenario.summary);
  // A plan with a private company: the same change, among the lives with each outcome for it.
  const byOutcome = useMemo(() => (paired?.by_equity_outcome?.length ? decisionTable(paired) : []), [paired]);
  const company = base.equity?.company ?? '';
  const overall = flips ? { value: flips.net.value, withinNoise: flips.withinNoise } : null;
  // A change that costs cash also moves when work becomes optional, which by itself changes how often a plan holds.
  const decisionContext = { fiShift: paired ? fiAgeShift(paired) : null, funded: profile.retirement.trigger.type === 'when_funded' || profile.retirement.trigger.type === 'fixed_number' };
  const fan = (series: PairedSeries) => (
    <ChartCard
      title={`${PAIRED_LABEL[series]} among the living: the plan against the change`}
      takeaway={fanTakeaway(base, scenario.summary, series, name)}
      caption={
        series === 'net_worth'
          ? 'Lines are each plan’s median; bands hold the middle half of its lives, among households with someone alive. Two full fans on one axis cannot be told apart, so the bands stop at the middle half and the gap between the medians has its own strip and scale beneath. That gap is the difference between two medians, not the median of per-life differences, which is in the dot plot above.'
          : 'What lives actually spend each year under each plan, with the same encoding: medians as lines, the middle half as bands, and the gap between the medians beneath on its own scale.'
      }
      table={() => pairedFanTable(base, scenario.summary, series, name)}
      filename={`${series === 'net_worth' ? 'net-worth' : 'spending'}-plan-against-scenario`}
      explain={{ context: () => ({ scenario: name, series, paths: base.paths, plan_median_retirement_age: base.headline.retirement_age.p50, scenario_median_retirement_age: scenario.summary.headline.retirement_age.p50, plan_terminal_net_worth: base.headline.terminal_net_worth, scenario_terminal_net_worth: scenario.summary.headline.terminal_net_worth }) }}
      dimmed={dimmed}
    >
      <PairedLegend name={name} className="mb-2" />
      <PairedFan base={base} scenario={scenario.summary} series={series} name={name} />
    </ChartCard>
  );

  return (
    <section aria-labelledby="scenario-detail-title" className="mt-14 border-t border-line-strong pt-6" data-testid="scenario-detail">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <h2 id="scenario-detail-title" className="eyebrow">
          The plan against
        </h2>
        {many.length > 1 ? (
          <Select aria-label="Scenario to compare with the plan" className="h-7 max-w-full text-sm font-medium" value={scenario.id} onChange={(id) => useScenarioUi.getState().select(id)} options={many.map((c) => ({ value: c.id, label: c.name }))} />
        ) : (
          <span className="text-sm font-semibold text-ink-1">{name}</span>
        )}
        <Sharpness run={run} scenario={scenario} base={base} />
      </div>

      <div className="mt-1 grid grid-cols-1 items-start gap-x-12 gap-y-8 @4xl:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
        <div className="min-w-0">
          <Sentence segments={headline} testId="flip-sentence" />
          {saleLine && (
            <p data-testid="scenario-home-sale" className="mt-3 max-w-[36rem] text-sm leading-relaxed text-pretty text-ink-2">
              {saleLine}
            </p>
          )}
          <p className="mt-3 max-w-[36rem] text-xs leading-relaxed text-pretty text-ink-3">{flips ? flipNote(flips, name) : `Both plans are living through the same ${count(run.total / Math.max(1, run.scenarios.length + 1))} simulated futures. The two rates above are each still sharpening.`}</p>
          {flips && (
            <ChartCard
              className="mt-6"
              title="Every simulated life, by whether the plan holds"
              table={() => flipTable(flips)}
              filename="rescued-harmed-unchanged"
              explain={{ context: () => ({ scenario: name, paths: flips.paths, rescued: flips.rescued, harmed: flips.harmed, holds_either_way: flips.both, short_either_way: flips.neither, net_change: flips.net }) }}
              dimmed={pairedDimmed}
            >
              <FlipBar flips={flips} />
              <FlipLegend flips={flips} className="mt-2" />
            </ChartCard>
          )}
        </div>

        {scalar ? (
          <ChartCard
            title="The difference, life by life"
            takeaway={deltaTakeaway(scalar, paired?.quantile_levels ?? [])}
            caption={`${scalar.both < 100 ? `The ${scalar.both} paired lives are drawn as 100 evenly spaced dots, each placed at a difference` : 'Each dot is one of 100 equally likely lives, placed at its own difference'}: that life with the change, minus the same life without it. Hollow dots are lives the change leaves exactly as they were. The axis covers the middle of the range; the most extreme lives at each end are stacked in the end columns.${scalar.unit === 'dollars' ? ' Today’s dollars.' : ''}`}
            controls={<SegmentedControl aria-label="Quantity" size="sm" value={scalar.scalar as DeltaScalar} onChange={setScalarKey} options={options} />}
            table={() => deltaTable(scalar, paired?.quantile_levels ?? [])}
            filename={`per-life-difference-${scalar.scalar.replace(/_/g, '-')}`}
            explain={{ context: () => ({ scenario: name, quantity: scalar.scalar, paired: scalar.both, mean_delta: scalar.mean_delta, quantile_levels: paired?.quantile_levels, delta_quantiles: scalar.quantiles, share_higher: scalar.b_higher, share_lower: scalar.a_higher, share_unchanged: scalar.equal, within_noise: scalar.within_noise }) }}
            dimmed={pairedDimmed}
          >
            <DeltaDots scalar={scalar} levels={paired?.quantile_levels ?? []} />
          </ChartCard>
        ) : (
          <div className="min-w-0" aria-busy="true">
            <h3 className="text-sm font-medium text-ink-2">The difference, life by life</h3>
            <p className="mt-1 max-w-[30rem] text-sm leading-relaxed text-pretty text-ink-2">Lives are matched one to one once both runs have finished. Until then the charts below compare the two plans as wholes.</p>
            <Skeleton className="mt-4 h-40 w-full" />
          </div>
        )}
      </div>

      {paired && (
        <ChartCard
          className="mt-12"
          title="Every measure, paired"
          takeaway={metricTakeaway(paired)}
          caption={`${pairedNote(paired)} Differences are scenario minus plan, with 95% intervals for sampling error. Dollar and age rows are averages over lives, which a few extreme lives can pull: the dot plot above shows the spread. The small drawing places each interval against no change, on that row’s own scale; a hollow dot means the interval includes zero. Today’s dollars.`}
          table={() => metricTableData(paired, name)}
          filename="scenario-measures-paired"
          explain={{ context: () => ({ scenario: name, paths: paired.paths, headline: paired.headline, outcomes: paired.outcomes.map((o) => ({ outcome: o.outcome, plan: o.a.value, scenario: o.b.value, difference: o.difference, within_noise: o.within_noise })) }) }}
          dimmed={pairedDimmed}
        >
          <MetricTable comparison={paired} />
        </ChartCard>
      )}

      <div className="mt-12 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-line pt-5">
        <h3 className="text-base font-semibold tracking-tight">Across a lifetime</h3>
        <TimeRangeControl time={time} presets={ranges} className="ml-auto" />
        <p className="hidden basis-full text-xs text-ink-3 @2xl:block">The charts below share one age axis and one crosshair. Drag across one to zoom all of them. Every figure is in today’s dollars.</p>
      </div>

      <div className="mt-6 grid grid-cols-1 gap-x-12 gap-y-12 @5xl:grid-cols-2">
        {fan('net_worth')}
        {fan('spending')}
      </div>

      {paired && byOutcome.length > 0 && (
        <ChartCard
          className="mt-12"
          title={`The change, by what happens to ${company || 'the company'}`}
          takeaway={<span data-testid="scenario-by-outcome">{decisionSentence('This change', company, byOutcome, overall, decisionContext)}</span>}
          takeawayText={decisionSentence('This change', company, byOutcome, overall, decisionContext)}
          caption={`${DECISIONS_CAPTION} The outcome is the one the life has under the plan.`}
          table={() => decisionTableData(byOutcome, name)}
          filename="scenario-by-company-outcome"
          explain={{ context: () => ({ scenario: name, company, by_outcome: byOutcome.map((r) => ({ outcome: r.outcome, paired_lives: r.paths, median_change_in_terminal_net_worth: r.wealth, change_in_plan_holds: r.holds })) }) }}
          dimmed={pairedDimmed}
        >
          <div className="max-w-[44rem]">
            <DecisionTable rows={byOutcome} label={`${name}: the change among the lives with each outcome for ${company || 'the company'}`} />
          </div>
        </ChartCard>
      )}

      <ChartCard
        className="mt-12"
        title="Lives moved between life stages"
        takeaway={stageTakeaway(diffs, base)}
        caption="Each line is the number of lives per 100 in that stage with the change, minus without it; above zero means more lives are in the stage. The lines sum to zero at every age, because a life that leaves one stage enters another. Two stacked charts side by side cannot show a shift of a few lives in a hundred, so the difference is drawn instead. Stages that never differ by 1 life in 200 are left out."
        table={() => stageTable(base, scenario.summary)}
        filename="life-stages-change"
        explain={{ context: () => ({ scenario: name, paths: base.paths, largest_shifts: diffs.map((d) => ({ stage: d.status, lives_per_100: Math.round(d.peak * 1000) / 10, at_age: Math.floor(base.axis.ages[d.peakIndex] ?? 0) })) }) }}
        dimmed={dimmed}
      >
        <StageLegend diffs={diffs} className="mb-2" />
        <StageShift base={base} scenario={scenario.summary} />
      </ChartCard>

      <div className="mt-12 border-t border-line pt-5">
        <SameLife pool={pool} seed={run.seed} ready={settled && paired !== null} dimmed={dimmed} base={{ profile, summary: base }} scenario={{ id: scenario.id, name, profile: scenario.profile, summary: scenario.summary }} />
      </div>
    </section>
  );
}

export default function ScenariosView({ profile, summary, comparison: ran, run, pool }: ViewProps) {
  // The run remembers each scenario's name from when it started; a rename does not (and
  // should not) re-run anything, so names are read live from the plan.
  const scenarios = useProfileStore((s) => s.scenarios);
  const comparison = useMemo(() => ran.map((c) => ({ ...c, name: scenarios.find((x) => x.id === c.id)?.name || c.name })), [ran, scenarios]);
  const chosen = useScenarioUi((s) => s.selected);
  const metricKey = useScenarioUi((s) => s.metric);
  const metric = metricOf(metricKey);
  const selected = comparison.find((c) => c.id === chosen) ?? comparison.find((c) => c.summary) ?? comparison[0] ?? null;
  const select = (id: string) => {
    useScenarioUi.getState().select(id);
    requestAnimationFrame(scrollToDetail);
  };
  const rows = useMemo(() => rankRows(comparison, metric), [comparison, metric]);
  const waiting = run.status === 'running' || run.status === 'queued';

  return (
    <ChartSyncProvider>
      <div className="@container mx-auto max-w-[76rem] pb-8 pt-7">
        <header>
          <p className="eyebrow">Scenarios</p>
          <p data-testid="scenarios-lead" className="mt-3 max-w-[40rem] text-2xl font-medium tracking-tight text-pretty text-ink-1 @4xl:text-[1.75rem] @4xl:leading-[2.25rem]">If I change this decision, what happens to the same simulated lives?</p>
          <p className="mt-3 max-w-[40rem] text-sm leading-relaxed text-pretty text-ink-2">
            Every scenario lives through exactly the futures the plan does: the same markets, jobs, health and lifespans, life for life. So a difference between them is the decision itself and never market luck, and each
            life can be asked what the change did to it.
          </p>
        </header>

        <div className="mt-8">
          <Bench profile={profile} summary={summary} comparison={comparison} run={run} selected={selected?.id ?? null} />
        </div>

        {!summary && waiting && comparison.length > 0 && <ViewSkeleton />}

        {summary && comparison.length >= 2 && (
          <section aria-labelledby="scenario-board-title" className="mt-14 border-t border-line-strong pt-6">
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
              <h2 id="scenario-board-title" className="text-base font-semibold tracking-tight">
                All {comparison.length} scenarios against the plan
              </h2>
              <SegmentedControl aria-label="Rank scenarios by" size="sm" className="ml-auto @max-xl:hidden" value={metricKey} onChange={(m) => useScenarioUi.getState().setMetric(m)} options={METRICS.map((m) => ({ value: m.key, label: m.label }))} />
              {/* Four labels do not fit a phone: the same choice as a menu. */}
              <Select aria-label="Rank scenarios by" className="ml-auto h-7 text-sm @xl:hidden" value={metricKey} onChange={(m) => useScenarioUi.getState().setMetric(m)} options={METRICS.map((m) => ({ value: m.key, label: m.label }))} />
            </div>
            <ChartCard
              className="mt-5"
              title={`Difference from the plan: ${metric.label.toLowerCase()}`}
              takeaway={rankTakeaway(rows, metric)}
              caption={`Each dot is a scenario’s difference from the plan over the same lives, ${metric.unit === 'share' ? 'in lives per 100' : metric.unit === 'age' ? 'averaged over lives in which work becomes optional under both' : 'averaged over lives, in today’s dollars'}; the line through it is the 95% interval for sampling error. A hollow grey dot means the interval includes zero: that scenario cannot be told from the plan on this measure, and its place in the order means nothing. Best first${metric.higherIsBetter ? '' : ' (earlier is better)'}. Choose a row to compare it with the plan in detail.`}
              table={() => rankTable(comparison, metric)}
              filename={`scenarios-ranked-${metric.key}`}
              explain={{ context: () => ({ measure: metric.label, rows: rows.map((r) => ({ scenario: r.name, difference: r.value, lo: r.lo, hi: r.hi, within_noise: r.withinNoise })) }) }}
              dimmed={run.status !== 'done' || run.stale}
            >
              <Rank comparison={comparison} metric={metric} selected={selected?.id ?? null} onSelect={select} />
            </ChartCard>
            <div className="mt-8">
              <h3 className="text-sm font-medium text-ink-2">Each scenario at a glance</h3>
              <p className="mt-1 max-w-[44rem] text-xs leading-relaxed text-pretty text-ink-3">
                In the same order. Figures are each scenario’s own, with the change from the plan at the right (medians, so they need not match the averages above). The small chart is median net worth by age for the
                scenario and for the plan, on one scale across all panels.
              </p>
              <BoardKey className="mt-3" />
              <div className={run.stale ? 'mt-4 opacity-60 transition-opacity' : 'mt-4 transition-opacity'}>
                <Board base={summary} comparison={comparison} metric={metric} selected={selected?.id ?? null} onSelect={select} />
              </div>
            </div>
          </section>
        )}

        {summary && selected && selected.summary && <Detail base={summary} scenario={{ ...selected, summary: selected.summary }} run={run} profile={profile} pool={pool} many={comparison.filter((c) => c.summary)} />}
        {summary && selected && !selected.summary && (
          <div className="mt-14 border-t border-line-strong pt-6">
            <p className="eyebrow">The plan against {selected.name}</p>
            <ViewSkeleton />
          </div>
        )}
      </div>
    </ChartSyncProvider>
  );
}
