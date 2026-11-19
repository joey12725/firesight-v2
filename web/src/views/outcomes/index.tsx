// Outcomes: "how wide is the range of lives, and what separates the good from the bad?"
// The page runs from the spread of endings (dot plots of each per-life outcome), to
// whether the money outlasts the people, to what the lives that fell short had in
// common, and ends by asking whether any of it survives a change of market model.
import { useMemo, useState } from 'react';
import { count, moneyCompact, SegmentedControl } from '@/ui';
import { ChartCard, ChartSyncProvider, TimeRangeControl, summaryTime } from '@/ui/charts';
import type { ViewProps } from '../types';
import { Cohorts, CohortLegend, cohortTable, useCohortTraces } from './Cohorts';
import { cohortYears, contrasts, isHistorical, luckSplit, scatterLives } from './data';
import { AgesChart, HomeSaleChart, ShortYearsChart, TaxChart, WealthChart, WithdrawalRateChart, WorstCutChart, agesCoincide, agesTable, scalarTable } from './Distributions';
import { ContrastLegend, FailedVsHeld, contrastTable } from './FailedVsHeld';
import { LastingCurves, LastingLegend, lastingTable } from './LastingCurves';
import { ScatterLegend, SequenceScatter, scatterTable, type ScatterMode } from './SequenceScatter';
import { SectionHead, Sharpness, ViewSkeleton } from './shared';
import { agesTakeaway, cohortTakeaway, contrastTakeaway, cutTakeaway, homeSaleTakeaway, lastingTakeaway, luckTakeaway, rangeSentence, shortYearsTakeaway, taxTakeaway, wealthTakeaway, withdrawalTakeaway } from './text';
import { ModelGlossary, TriangulationIntro, TriangulationStrip, triangulationSentence, triangulationTable, useTriangulation } from './Triangulation';

export default function OutcomesView({ profile, summary, run, pool }: ViewProps) {
  const [mode, setMode] = useState<ScatterMode>('wealth');
  const time = useMemo(() => (summary ? summaryTime(summary) : null), [summary]);
  const historical = isHistorical(profile);
  const traces = useCohortTraces(profile, run.seed, pool, historical && summary !== null && !run.stale);
  const triangulation = useTriangulation(profile, run, pool);
  const lives = useMemo(() => (summary ? scatterLives(summary) : []), [summary]);
  const split = useMemo(() => luckSplit(lives), [lives]);
  const rows = useMemo(() => (summary ? contrasts(summary) : []), [summary]);
  const cohorts = useMemo(() => (summary && historical ? cohortYears(summary) : []), [summary, historical]);

  if (!summary || !time) return run.status === 'running' || run.status === 'queued' ? <ViewSkeleton /> : null;

  const dimmed = run.stale;
  const sentence = rangeSentence(summary);
  const same = agesCoincide(summary);
  const who = profile.household.people[0]?.name;
  const dotsOf = summary.paths < 100 ? `the ${summary.paths} simulated` : '100 equally likely';
  const context = () => ({ paths: summary.paths, success_rate: summary.headline.success_rate, home_sale_rate: summary.headline.home_sale_rate, depletion_rate: summary.headline.depletion_rate });
  const failed = summary.failure.failed;
  const marketKind = profile.assumptions.market.kind.type;

  return (
    <ChartSyncProvider>
      <div className="@container mx-auto max-w-[76rem] pb-10 pt-7">
        <header>
          <div className="flex min-h-5 flex-wrap items-center gap-2">
            <p className="eyebrow">How wide is the range of lives, and what separates the good from the bad?</p>
            <Sharpness run={run} summary={summary} />
          </div>
          <p data-testid="range-sentence" className="mt-3 max-w-[46rem] text-2xl font-medium tracking-tight text-pretty text-ink-1 @4xl:text-[1.75rem] @4xl:leading-[2.25rem]">
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
          <p className="mt-3 max-w-[46rem] text-xs leading-relaxed text-pretty text-ink-3">
            {count(summary.paths)} lives simulated. In the dot plots each dot is one of {dotsOf} lives, so counting dots is reading a probability. Every dollar figure is in today’s dollars; net worth includes the home.
          </p>
        </header>

        <div className="mt-10 grid grid-cols-1 items-start gap-x-12 gap-y-12 @4xl:grid-cols-[minmax(0,6fr)_minmax(0,5fr)]">
          <ChartCard
            title={same ? 'Age at retirement' : 'Age work becomes optional, and age actually retired'}
            takeaway={agesTakeaway(summary, same)}
            caption={
              same
                ? `Each dot is one of ${dotsOf} lives, at ${who ? `${who}’s` : 'the first person’s'} age when savings first meet the plan’s funded test, which is also when this plan retires. Hollow dots never get there.`
                : `Each dot is one of ${dotsOf} lives, by ${who ? `${who}’s` : 'the first person’s'} age. Top: when savings first meet the funded test. Bottom: when the plan actually stops work. Hollow dots: never.`
            }
            table={() => agesTable(summary)}
            filename="retirement-ages"
            explain={{ context: () => ({ ...context(), fi_age: summary.headline.fi_age, retirement_age: summary.headline.retirement_age, never_retired: summary.retirement.never }) }}
            dimmed={dimmed}
          >
            <AgesChart summary={summary} />
          </ChartCard>

          <ChartCard
            title="Net worth at the end of life"
            takeaway={wealthTakeaway(summary)}
            caption="What is left when the last person dies or the plan ends, home included. The axis is logarithmic because lives differ by multiples, not by fixed amounts: equal distances are equal ratios."
            table={() => scalarTable(summary, 'terminal_net_worth', 'Net worth at the end of life, today’s dollars: percentiles over all simulated lives', moneyCompact)}
            filename="net-worth-at-end"
            explain={{ context: () => ({ ...context(), terminal_net_worth: summary.headline.terminal_net_worth }) }}
            dimmed={dimmed}
          >
            <WealthChart summary={summary} />
          </ChartCard>
        </div>

        <div className="mt-12 grid grid-cols-1 items-start gap-x-10 gap-y-12 @3xl:grid-cols-3">
          <ChartCard
            title="Deepest spending cut in retirement"
            takeaway={cutTakeaway(summary)}
            caption="The worst single retired year of each life, as a cut below what the plan intended to spend that year."
            table={() => scalarTable(summary, 'worst_spending_ratio', 'Lowest single retired year of spending delivered as a share of plan: percentiles over retired lives', (v) => `${(v * 100).toFixed(0)}%`)}
            filename="deepest-spending-cut"
            explain={{ context: () => ({ ...context(), worst_cut_levels: summary.spending.worst_cut_levels, worst_cut: summary.spending.worst_cut, paths_below_90: summary.spending.paths_below_90 }) }}
            dimmed={dimmed}
            quiet
          >
            <WorstCutChart summary={summary} />
          </ChartCard>
          <ChartCard
            title="Years short of essentials"
            takeaway={shortYearsTakeaway(summary)}
            caption="Years in which liquid assets could not cover essential spending. Most lives have none."
            table={() => scalarTable(summary, 'years_depleted', 'Years ending short of essentials: percentiles over all simulated lives', (v) => v.toFixed(0))}
            filename="years-short-of-essentials"
            explain={{ context: () => ({ ...context(), years_depleted: summary.headline.years_depleted, depletion_age: summary.headline.depletion_age }) }}
            dimmed={dimmed}
            quiet
          >
            <ShortYearsChart summary={summary} />
          </ChartCard>
          <ChartCard
            title="Tax paid over a lifetime"
            takeaway={taxTakeaway(summary)}
            caption="Federal, state and payroll tax and penalties, summed over the whole life. Logarithmic axis."
            table={() => scalarTable(summary, 'lifetime_tax', 'Lifetime tax, today’s dollars: percentiles over all simulated lives', moneyCompact)}
            filename="lifetime-tax"
            explain={{ context: () => ({ ...context(), lifetime_tax: summary.headline.lifetime_tax }) }}
            dimmed={dimmed}
            quiet
          >
            <TaxChart summary={summary} />
          </ChartCard>
        </div>

        <div className="mt-12 grid grid-cols-1 items-start gap-x-10 gap-y-12 @3xl:grid-cols-2">
          <ChartCard
            title="Withdrawal rate in the first retired year"
            takeaway={withdrawalTakeaway(summary)}
            caption="What each life actually draws from savings in its first retired year (the budget less after-tax income), over its savings on the day it retires. It differs from the plan’s trigger rate because income, tax and the market on the day all differ by life."
            table={() => scalarTable(summary, 'initial_withdrawal_rate', 'First retired year’s draw over savings at retirement: percentiles over retired lives', (v) => `${(v * 100).toFixed(1)}%`)}
            filename="initial-withdrawal-rate"
            explain={{ context: () => ({ ...context(), initial_withdrawal_rate: summary.headline.initial_withdrawal_rate, investable_at_retirement: summary.headline.investable_at_retirement }) }}
            dimmed={dimmed}
            quiet
          >
            <WithdrawalRateChart summary={summary} />
          </ChartCard>
          <ChartCard
            title="Home sold as a last resort"
            takeaway={homeSaleTakeaway(summary)}
            caption={
              profile.retirement.home_equity === 'never'
                ? 'This plan never sells a home to fund spending (Retirement, home equity), so a life whose savings run out is short of essentials instead.'
                : 'When savings run out, the plan sells property before cutting essentials (Retirement, home equity: last resort). The simulation counts that as the plan holding, so read this beside the success figure.'
            }
            table={() => scalarTable(summary, 'home_sale_age', 'Age at which a home is sold because savings ran out: percentiles over the lives in which it happens', (v) => v.toFixed(0))}
            filename="home-sold-as-last-resort"
            explain={{ context: () => ({ ...context(), home_sale_rate: summary.headline.home_sale_rate, home_equity: profile.retirement.home_equity }) }}
            dimmed={dimmed}
            quiet
          >
            <HomeSaleChart summary={summary} />
          </ChartCard>
        </div>

        <SectionHead title="Does the money outlast the people?" aside={<TimeRangeControl time={time} presets={[{ id: 'all', label: 'Whole plan', range: null }]} />}>
          Running out only matters while someone is alive to feel it. The two curves are read against each other; the strip beneath is the share of lives in which both happen at once.
        </SectionHead>
        <ChartCard
          className="mt-6"
          title="Survival, and how long the money lasts"
          takeaway={lastingTakeaway(summary)}
          caption="“The money has lasted” counts every life whose liquid assets have always covered essentials up to that age, including lives that have ended. “Alive and short” is counted life by life in the simulation (it is not the two curves multiplied), and a life can leave it again if income returns. Drag to zoom."
          table={() => lastingTable(summary)}
          filename="survival-and-money-lasting"
          explain={{ context: () => ({ ...context(), depletion_age: summary.headline.depletion_age, years_depleted: summary.headline.years_depleted }) }}
          dimmed={dimmed}
        >
          <div className="mb-2">
            <LastingLegend />
          </div>
          <LastingCurves summary={summary} />
        </ChartCard>

        <SectionHead title="Luck in the first decade of retirement">
          The same average return can end very differently depending on when the bad years fall. Each dot is one retired life from a sample of {count(lives.length)}.
        </SectionHead>
        <ChartCard
          className="mt-6"
          title="Early retirement returns against how the life ended"
          takeaway={luckTakeaway(split, lives.length)}
          caption={
            mode === 'wealth'
              ? 'Returns are the portfolio’s own, after inflation, annualised over the ten years from retirement. The line joins the median ending of twelve equal-sized groups of lives ranked by that return; the bars count how many in each group ran short. The vertical axis is logarithmic.'
              : 'Lives that ran short are placed at the age it happened; lives that never did sit in the band along the top. The line joins the median age among the lives that ran short in each group (where there are at least three).'
          }
          controls={
            <SegmentedControl
              aria-label="How the life ended"
              size="sm"
              value={mode}
              onChange={setMode}
              options={[
                { value: 'wealth', label: 'Wealth at the end' },
                { value: 'age', label: 'Age the money ran short' },
              ]}
            />
          }
          table={() => scatterTable(summary)}
          filename="early-returns-and-endings"
          explain={{ context: () => ({ ...context(), sample: lives.length, unluckiest_fifth: split }) }}
          dimmed={dimmed}
        >
          <div className="mb-2">
            <ScatterLegend mode={mode} />
          </div>
          <SequenceScatter summary={summary} mode={mode} />
        </ChartCard>

        <SectionHead title="What was different about the lives that fell short">
          {failed > 0
            ? `${count(failed)} of ${count(summary.paths)} lives fall short of the plan. Each panel sets their median against the median of the lives that held, over the spread of all lives. Panels are ordered by how far apart the two groups sit.`
            : 'No simulated life falls short of the plan.'}
        </SectionHead>
        <ChartCard
          className="mt-6"
          title="Lives that fell short against lives that held"
          takeaway={contrastTakeaway(summary, rows)}
          caption={failed > 0 ? 'These are the circumstances a life was dealt, not its results. A difference here is an association across simulated lives: several usually act together, and none is the single cause.' : undefined}
          table={() => contrastTable(summary)}
          filename="fell-short-against-held"
          explain={{ context: () => ({ ...context(), failed, contrasts: rows.map((c) => ({ scalar: c.scalar, failed_median: c.failed, held_median: c.succeeded, separation: Math.round(c.separation * 100) / 100 })) }) }}
          dimmed={dimmed}
        >
          {rows.length > 0 && (
            <div className="mb-4">
              <ContrastLegend />
            </div>
          )}
          <FailedVsHeld summary={summary} />
        </ChartCard>

        {historical && (
          <>
            <SectionHead title="By the year history starts">
              This plan uses history as it happened: each simulated life begins in a real month between 1926 and 2026 and lives through what followed, with its own career, health and lifespan.
            </SectionHead>
            <ChartCard
              className="mt-6"
              title="One life from each starting year, and how each year’s lives fared"
              takeaway={cohortTakeaway(cohorts)}
              caption="Top: one traced life per starting year (the one beginning that January). Bottom: of all sampled lives that begin in each year, how many ran short. A life that starts late in the record runs past its end and carries on from 1926, so those years (fainter) are not a clean test of what followed them. Hover either chart to pick out a starting year; click to open that life."
              table={() => cohortTable(summary)}
              filename="outcomes-by-starting-year"
              explain={{ context: () => ({ ...context(), worst_years: [...cohorts].filter((c) => c.lives >= 4).sort((a, b) => (b.shortShare ?? 0) - (a.shortShare ?? 0)).slice(0, 5) }) }}
              dimmed={dimmed}
            >
              <div className="mb-2">
                <CohortLegend />
              </div>
              <Cohorts summary={summary} traces={traces} />
            </ChartCard>
          </>
        )}

        <SectionHead title="Does the answer depend on the market model?" />
        <ChartCard
          className="mt-4"
          title="The same plan under four market models"
          takeaway={triangulationSentence(triangulation.state, marketKind)}
          caption={
            triangulation.state.status === 'idle'
              ? undefined
              : 'Dots are the estimate under each model; bars are 95% intervals for sampling error (for the two medians, a distribution-free interval). Where the bars of two models do not overlap, the models genuinely disagree.'
          }
          table={Object.keys(triangulation.state.results).length ? () => triangulationTable(triangulation.state) : undefined}
          filename="market-model-comparison"
          dimmed={triangulation.outdated}
          controls={<TriangulationIntro state={triangulation.state} run={run} onStart={triangulation.start} onStop={triangulation.stop} outdated={triangulation.outdated} />}
        >
          {triangulation.state.status !== 'idle' && Object.keys(triangulation.state.results).length > 0 && (
            <div className="mt-2">
              <TriangulationStrip state={triangulation.state} current={marketKind} />
            </div>
          )}
          <ModelGlossary />
        </ChartCard>
      </div>
    </ChartSyncProvider>
  );
}
