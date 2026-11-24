// Assumptions: "what is this model assuming about the world, and how do I know it is
// reasonable?" Not a settings form (the editor is that): a statement of the assumptions
// in force for this plan, each with a picture of what it implies, drawn from this plan's
// own simulation where the Summary carries it and from the historical record (labelled
// as such) where it does not, and a link to the place in the editor that changes it.
import { featuresOn } from '@/editor/realism';
import { useEffect, useMemo, useState, type ReactNode } from 'react';
import type { EnginePool } from '@/engine';
import type { Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { Icon, count, moneyCompact } from '@/ui';
import { ChartCard, ChartSyncProvider, Legend, TimeRangeControl, compositionSeries, ofHundred, planMoments, summaryTime, useChartTheme, type TableData } from '@/ui/charts';
import { QuantileDots } from '@/ui/charts/ext/outcomes-QuantileDots';
import { distribution, livesDots } from '../outcomes/data';
import { EditLink, SectionHead, Sharpness, ViewSkeleton } from '../outcomes/shared';
import { METHODOLOGY_URL } from '@/shell/actions';
import type { ViewProps } from '../types';
import { AgeSeriesChart, ModelRangeStrip, RecordRanges, TaxLadder, type AgeSeries } from './charts';
import {
  FILING_LABEL,
  HISTORY,
  MARKET_NAME,
  OMISSIONS,
  brackets,
  careShare,
  filingStatus,
  healthFacts,
  longevity,
  longevityStatement,
  longevityTakeaway,
  marketAdjustments,
  marketStatement,
  modelRanges,
  modelTakeaway,
  socialSecurityFacts,
  standardDeduction,
  taxPoints,
  taxStatement,
  unemployment,
} from './data';

const pct = (v: number, digits = 1) => `${(v * 100).toFixed(digits).replace(/^-(0(\.0+)?)$/, '$1').replace('-', '−')}%`;

/** One assumption: what is assumed (words, with the way to change it) beside what it implies (a picture). */
function Block({ title, statement, facts, edit, children }: { title: string; statement: ReactNode; facts?: ReactNode[]; edit: { path: string; label: string }; children: ReactNode }) {
  return (
    <section className="mt-14 grid grid-cols-1 items-start gap-x-12 gap-y-6 border-t border-line pt-5 @4xl:grid-cols-[minmax(0,4fr)_minmax(0,7fr)]">
      <div className="min-w-0 @4xl:sticky @4xl:top-4">
        <h2 className="text-base font-semibold tracking-tight text-ink-1">{title}</h2>
        <p className="mt-2 max-w-[38rem] text-sm leading-relaxed text-pretty text-ink-2">{statement}</p>
        {facts && facts.length > 0 && (
          <ul className="mt-3 max-w-[38rem] space-y-1.5 text-sm leading-relaxed text-ink-2">
            {facts.map((f, i) => (
              <li key={i} className="flex gap-2">
                <span aria-hidden className="mt-[0.6em] size-1 shrink-0 rounded-full bg-ink-3" />
                <span className="text-pretty">{f}</span>
              </li>
            ))}
          </ul>
        )}
        <div className="mt-4">
          <EditLink path={edit.path}>{edit.label}</EditLink>
        </div>
      </div>
      <div className="min-w-0 space-y-10">{children}</div>
    </section>
  );
}

/** The median life's first full year and a settled retired year, from its traced tax returns. */
function useTaxPoints(profile: Profile, summary: Summary | null, pool: EnginePool, seed: number, enabled: boolean) {
  const path = summary?.picks.find((p) => p.label === 'median')?.path_index ?? null;
  const [state, setState] = useState<{ key: string; points: ReturnType<typeof taxPoints> } | null>(null);
  const key = `${path}:${seed}`;
  useEffect(() => {
    if (!enabled || path === null) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      pool
        .trace(profile, path, seed)
        .then((trace) => !cancelled && setState({ key, points: taxPoints(trace) }))
        .catch(() => !cancelled && setState({ key, points: { now: null, retired: null, retiredAge: null } }));
    }, 300);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [profile, path, seed, pool, enabled, key]);
  return { path, points: state?.points ?? null, pending: enabled && path !== null && state === null };
}

function survivalTable(summary: Summary, names: string[]): TableData {
  const share = (v: number) => `${(v * 100).toFixed(1)}%`;
  return {
    caption: 'Share of simulated lives in which each person, and anyone in the household, is still alive, by age',
    columns: [{ key: 'age', label: 'Age' }, { key: 'year', label: 'Year', format: String }, ...names.map((n, i) => ({ key: `p${i}`, label: n, format: share })), { key: 'any', label: 'Anyone', format: share }],
    rows: summary.axis.ages.map((a, y) => ({ age: Math.floor(a), year: summary.axis.years[y] ?? null, ...Object.fromEntries(names.map((_, i) => [`p${i}`, summary.survival.people[i]?.[y] ?? null])), any: summary.survival.household[y] ?? null })),
  };
}

function seriesTable(summary: Summary, caption: string, series: AgeSeries[]): TableData {
  return {
    caption,
    columns: [{ key: 'age', label: 'Age' }, { key: 'year', label: 'Year', format: String }, ...series.map((s) => ({ key: s.key, label: s.label, format: moneyCompact }))],
    rows: summary.axis.ages.map((a, y) => ({ age: Math.floor(a), year: summary.axis.years[y] ?? null, ...Object.fromEntries(series.map((s) => [s.key, s.values[y] ?? null])) })),
  };
}

function scalarQuantiles(summary: Summary, scalar: 'inflation' | 'worst_drawdown', caption: string): TableData {
  const d = distribution(summary, scalar);
  const levels = ['1st', '5th', '10th', '25th', 'Median', '75th', '90th', '95th', '99th'];
  return { caption, columns: [{ key: 'level', label: 'Percentile of lives', align: 'left' }, { key: 'value', label: 'Value', format: (v) => pct(v) }], rows: levels.map((level, i) => ({ level, value: d?.quantiles[i] ?? null })) };
}

export default function AssumptionsView({ profile, summary, run, pool }: ViewProps) {
  const theme = useChartTheme();
  const time = useMemo(() => (summary ? summaryTime(summary) : null), [summary]);
  const usLaw = profile.assumptions.taxes.kind.type === 'us_current_law';
  const tax = useTaxPoints(profile, summary, pool, run.seed, usLaw && !run.stale);
  const ranges = useMemo(() => modelRanges(profile), [profile]);
  const people = useMemo(() => (summary ? longevity(profile, summary, run.preview) : []), [profile, summary, run.preview]);

  if (!summary || !time) return run.status === 'running' || run.status === 'queued' ? <ViewSkeleton /> : null;

  const dimmed = run.stale;
  const market = profile.assumptions.market;
  const kind = market.kind.type;
  const adjustments = marketAdjustments(profile);
  const realism = featuresOn(profile);
  const preview = run.preview;
  const status = filingStatus(profile);
  const ladder = brackets(status);
  const dotsOf = summary.paths < 100 ? `the ${summary.paths} simulated` : '100 equally likely';

  const inflation = distribution(summary, 'inflation');
  const drawdown = distribution(summary, 'worst_drawdown');
  const q = (d: typeof inflation, i: number) => d?.quantiles[i] ?? null;
  const stocks = HISTORY.assets.find((a) => a.key === 'equity_us');

  const survival: AgeSeries[] = [
    ...people.map((p, i) => ({ key: `p${i}`, label: p.name, color: theme.categorical[i] ?? theme.ink.secondary, values: p.survival })),
    ...(people.length > 1 ? [{ key: 'any', label: 'Either of them', short: 'Either', color: theme.ink.primary, values: summary.survival.household, strong: true }] : []),
  ];
  const ss: AgeSeries[] = [{ key: 'ss', label: 'Social Security', color: theme.categorical[2] ?? theme.baseline.line, values: compositionSeries(summary, 'income_social_security') }];
  const health: AgeSeries[] = [
    { key: 'health', label: 'Health insurance and medical costs', color: theme.categorical[3] ?? theme.baseline.line, values: compositionSeries(summary, 'spend_healthcare') },
    { key: 'care', label: 'Long-term care', color: theme.categorical[4] ?? theme.ink.secondary, values: compositionSeries(summary, 'spend_long_term_care') },
  ];
  const ssPeak = Math.max(0, ...ss[0]!.values.map((v) => v ?? 0));
  // When benefits typically begin, on the first person's age axis: the engine's median claim, not a reading of the curve.
  const ssStart = planMoments(summary, profile).find((m) => m.key === 'social_security')?.age;
  const care = careShare(summary);
  const jobs = unemployment(summary);
  const healthPeak = Math.max(0, ...health[0]!.values.map((v, i) => (v ?? 0) + (health[1]!.values[i] ?? 0)));
  const modelled = health[0]!.values.some((v) => (v ?? 0) > 0);
  const healthAt = (age: number) => {
    const i = summary.axis.ages.findIndex((a) => a >= age);
    return i < 0 ? null : (health[0]!.values[i] ?? 0) + (health[1]!.values[i] ?? 0);
  };

  const taxRows = [
    ...(tax.points?.now ? [{ key: 'now', label: `In ${tax.points.now.year}`, point: tax.points.now }] : []),
    ...(tax.points?.retired ? [{ key: 'retired', label: `Retired, in ${tax.points.retired.year}`, point: tax.points.retired }] : []),
  ];
  const now = tax.points?.now ?? null;
  const retired = tax.points?.retired ?? null;
  const taxTakeaway = !usLaw
    ? 'This plan does not use the US bracket system.'
    : tax.pending
      ? 'Reading the tax returns of the middle life…'
      : now
        ? `In the middle life’s first full year, ${moneyCompact(now.taxable)} of income is taxable after deductions${now.marginal > 0 ? `, the next dollar is taxed at ${Math.round(now.marginal * 100)}% federally` : ''}, and all taxes together take ${pct(now.agi > 0 ? now.total / now.agi : 0, 0)} of income.${
            retired ? (retired.foreign ? ` Five years into retirement it is living abroad and pays the flat foreign rate instead.` : retired.marginal > 0 ? ` Five years into retirement, ${moneyCompact(retired.taxable)} is taxable and the bracket is ${Math.round(retired.marginal * 100)}%.` : ' Five years into retirement its taxable income is below the deductions, so it pays no federal income tax.') : ''
          }`
        : 'The middle life has no tax return to show.';

  return (
    <ChartSyncProvider>
      <div className="@container mx-auto max-w-[76rem] pb-10 pt-7">
        <header>
          <div className="flex min-h-5 flex-wrap items-center gap-2">
            <p className="eyebrow">What is this model assuming about the world, and is that reasonable?</p>
            <Sharpness run={run} summary={summary} />
          </div>
          <p data-testid="assumptions-sentence" className="mt-3 max-w-[48rem] text-2xl font-medium tracking-tight text-pretty text-ink-2 @4xl:text-[1.75rem] @4xl:leading-[2.25rem]">
            Markets follow <b className="font-semibold text-ink-1">{MARKET_NAME[kind].toLowerCase()}</b>
            {Math.abs(market.equity_premium_adjustment) > 1e-9 ? (
              <>
                , with stocks <b className="font-semibold text-ink-1">{Math.abs(market.equity_premium_adjustment * 100).toFixed(1).replace(/\.0$/, '')} points a year {market.equity_premium_adjustment < 0 ? 'below' : 'above'} the record</b>
              </>
            ) : adjustments.length > 0 ? (
              ', adjusted'
            ) : (
              ''
            )}
            ; lifespans are <b className="font-semibold text-ink-1">{profile.assumptions.longevity.type === 'stochastic' ? 'drawn from life tables' : `fixed at ${profile.assumptions.longevity.age}`}</b>; tax is{' '}
            <b className="font-semibold text-ink-1">{usLaw ? '2026 US law, kept for ever' : profile.assumptions.taxes.kind.type === 'flat' ? 'a flat rate' : 'ignored'}</b>; {profile.assumptions.career_risk === profile.assumptions.health_risk ? (
              <>
                job loss and health shocks are <b className="font-semibold text-ink-1">{profile.assumptions.career_risk ? 'simulated' : 'switched off'}</b>.
              </>
            ) : (
              <>
                job loss is <b className="font-semibold text-ink-1">{profile.assumptions.career_risk ? 'simulated' : 'switched off'}</b> and health shocks are <b className="font-semibold text-ink-1">{profile.assumptions.health_risk ? 'simulated' : 'switched off'}</b>.
              </>
            )}
          </p>
          {realism.length > 0 && (
            <p data-testid="assumptions-realism" className="mt-3 max-w-[46rem] text-sm leading-relaxed text-pretty text-ink-2">
              On top of that this plan switches on <b className="font-semibold text-ink-1">{realism.length} of the 50 optional realism features</b>: {realism.map((f) => f.label).join('; ')}. Each adds a source of
              risk or a behaviour the standard model leaves out; they are set in the folded “optional” groups of the plan editor.
            </p>
          )}
          <p className="mt-3 max-w-[46rem] text-xs leading-relaxed text-pretty text-ink-3">
            Each section says what is assumed, shows what that implies for this plan across {count(summary.paths)} simulated lives, and links to where it can be changed. Every dollar figure is in today’s dollars. Figures from the historical record are
            labelled as such: they are data, not simulation output.
          </p>
        </header>

        <Block
          title="Markets"
          statement={marketStatement(profile)}
          facts={[
            ...adjustments.map((a) => (
              <>
                <b className="font-medium text-ink-1">{a.label}:</b> {a.detail}.
              </>
            )),
            ...(adjustments.length === 0 ? ['No adjustment: expected returns and inflation are history’s. The default plans for stocks 1.5 points a year below the US record; this plan has set that to zero, the most generous setting.'] : []),
            'Two more long-run levels are judgements rather than history: gold is assumed to return 1% a year after inflation and international stocks 1 point less than US stocks; a crypto holding is assumed to return nothing after inflation, at 73% volatility.',
            'Whether the answer depends on this choice can be tested at the foot of the Outcomes view, which runs the plan under all four models.',
          ]}
          edit={{ path: '/assumptions/market/equity_premium_adjustment', label: 'Change the market model or the stock-return adjustment' }}
        >
          <ChartCard
            title="What each market model implies for stocks over 30 years"
            takeaway={modelTakeaway(ranges)}
            caption={`Measured once from 5,000 simulated paths per model when the models were calibrated, with no adjustments and five-year blocks for the reshuffle${Math.abs(market.equity_premium_adjustment) > 1e-9 ? `; this plan’s row is those figures moved by its stock-return adjustment of ${(market.equity_premium_adjustment * 100).toFixed(1).replace('-', '−')} points a year` : ''}. The bottom row is every 30-year window that actually occurred between 1926 and 2026. Not output of this plan’s run.`}
            table={() => ({
              caption: 'Real return a year of US stocks over 30 years, by market model (calibration measurements) and in the historical record',
              columns: [{ key: 'model', label: 'Model', align: 'left' as const }, ...(['p5', 'p25', 'p50', 'p75', 'p95'] as const).map((k) => ({ key: k, label: k === 'p50' ? 'Median' : `${k.slice(1)}th percentile`, format: (v: number) => pct(v) }))],
              rows: ranges.map((r) => ({ model: r.label, p5: r.p5, p25: r.p25, p50: r.p50, p75: r.p75, p95: r.p95 })),
            })}
            filename="market-models-30-year-returns"
          >
            <ModelRangeStrip rows={ranges} />
            <p className="mt-1 text-2xs text-ink-3">Line: 9 in 10 outcomes. Bar: the middle half. Dot: the median.</p>
          </ChartCard>

          <ChartCard
            title="The record every model is built from"
            takeaway={stocks ? `In the historical record US stocks returned ${pct(stocks.mean_geo)} a year after inflation, but lost ground in one calendar year in three and once fell ${pct(-stocks.max_drawdown.depth, 0)} from peak to trough, taking until ${stocks.max_drawdown.recovered.slice(0, 4)} to recover.` : undefined}
            caption={`${HISTORY.note} Consumer prices rose ${pct(HISTORY.inflation.mean_geo)} a year on average, between ${pct(HISTORY.inflation.lowest[1] as number, 0)} (${HISTORY.inflation.lowest[0]}) and ${pct(HISTORY.inflation.worst[1] as number, 0)} (${HISTORY.inflation.worst[0]}).`}
            table={() => ({
              caption: 'Calendar-year returns after inflation in the historical record, 1927 to 2025',
              columns: [
                { key: 'asset', label: 'Asset', align: 'left' as const },
                { key: 'geo', label: 'Compound return a year', format: (v: number) => pct(v) },
                { key: 'sd', label: 'Standard deviation', format: (v: number) => pct(v) },
                { key: 'worst', label: 'Worst year', format: (v: number) => pct(v) },
                { key: 'worst_year', label: 'In', format: String },
                { key: 'best', label: 'Best year', format: (v: number) => pct(v) },
                { key: 'best_year', label: 'In', format: String },
                { key: 'drawdown', label: 'Deepest fall', format: (v: number) => pct(v, 0) },
              ],
              rows: HISTORY.assets.map((a) => ({ asset: a.label, geo: a.mean_geo, sd: a.sd, worst: a.worst[1] as number, worst_year: a.worst[0] as number, best: a.best[1] as number, best_year: a.best[0] as number, drawdown: a.max_drawdown.depth })),
            })}
            filename="historical-record"
          >
            <RecordRanges />
            <p className="mt-1 text-2xs text-ink-3">Line: the worst to the best year. Bar: two years in three. Dot: the average year.</p>
          </ChartCard>

          {drawdown && drawdown.count > 0 && (
            <ChartCard
              title="The deepest market fall each simulated life lives through"
              takeaway={`In this plan’s simulation the middle life sees its portfolio’s returns fall ${pct(q(drawdown, 4) ?? 0, 0)} from a peak at some point, and one in ten sees ${pct(q(drawdown, 6) ?? 0, 0)} or worse.`}
              caption={`Each dot is one of ${dotsOf} lives: the worst peak-to-trough fall of this plan’s own portfolio, after inflation, from returns alone (not counting what is added or withdrawn). From this run.`}
              table={() => scalarQuantiles(summary, 'worst_drawdown', 'Worst peak-to-trough fall of the portfolio’s real return index: percentiles over all simulated lives')}
              filename="deepest-market-fall"
              dimmed={dimmed}
            >
              <QuantileDots
                dots={livesDots(drawdown, summary.paths)}
                steps={[0.025, 0.05, 0.1]}
                format={(v) => pct(v, 0)}
                formatRange={(lo, hi) => `A fall of ${pct(lo, 0)} to ${pct(hi, 0)}`}
                axis={{ kind: 'value', format: 'percent', title: 'Deepest fall from a peak, after inflation' }}
                noun="Deepest market fall lived through"
                interval={{ p10: q(drawdown, 2), p50: q(drawdown, 4), p90: q(drawdown, 6) }}
                height={200}
              />
            </ChartCard>
          )}
        </Block>

        <Block
          title="Inflation"
          statement="Inflation is simulated, not assumed: it comes from the same market model as returns, so high-inflation spells arrive together with what they did to stocks and bonds. Every figure in the app is then divided by that life’s price level, which is what “today’s dollars” means."
          facts={[
            Math.abs(market.inflation_adjustment) > 1e-9 ? `The plan adds ${pct(market.inflation_adjustment)} a year to history’s inflation.` : 'No adjustment to history’s inflation.',
            profile.assumptions.taxes.index_brackets ? 'Tax brackets and Social Security benefits rise with it.' : 'Social Security benefits rise with it; tax brackets do not.',
          ]}
          edit={{ path: '/assumptions/market/inflation_adjustment', label: 'Plan for higher inflation' }}
        >
          {inflation && inflation.count > 0 && (
            <ChartCard
              title="Average inflation over each simulated life"
              takeaway={`Averaged over a whole life, inflation runs ${pct(q(inflation, 4) ?? 0)} a year in the middle case and between ${pct(q(inflation, 2) ?? 0)} and ${pct(q(inflation, 6) ?? 0)} in 8 of 10 lives. The record’s average is ${pct(HISTORY.inflation.mean_geo)}.`}
              caption={`Each dot is one of ${dotsOf} lives: consumer-price inflation a year, compounded over that life. From this run. Single years vary far more than lifetime averages: the record ranges from ${pct(HISTORY.inflation.lowest[1] as number, 0)} to ${pct(HISTORY.inflation.worst[1] as number, 0)}.`}
              table={() => scalarQuantiles(summary, 'inflation', 'Annualised inflation over each simulated life: percentiles')}
              filename="lifetime-inflation"
              dimmed={dimmed}
            >
              <QuantileDots
                dots={livesDots(inflation, summary.paths)}
                steps={[0.0025, 0.005, 0.01]}
                format={(v) => pct(v)}
                axis={{ kind: 'value', format: 'percent', title: 'Inflation a year, averaged over the life' }}
                noun="Average inflation over a life"
                interval={{ p10: q(inflation, 2), p50: q(inflation, 4), p90: q(inflation, 6) }}
                height={200}
              />
            </ChartCard>
          )}
        </Block>

        <SectionHead title="People" aside={<TimeRangeControl time={time} presets={[{ id: 'all', label: 'Whole plan', range: null }]} />}>
          The three charts over age below share one crosshair; drag across any of them to zoom all three.
        </SectionHead>

        <Block
          title="Lifespans"
          statement={longevityStatement(profile)}
          facts={people
            .filter((p) => p.expected !== null)
            .map((p, i) => `${p.name}: life expectancy ${Math.round(p.expected as number)} from the tables${i === 0 ? ', which is the average age at death' : ''}${p.reach.half !== null && p.reach.half - (p.expected as number) >= 1.5 ? `; half of lives run past ${Math.round(p.reach.half)}${i === 0 ? ', because early deaths pull an average down' : ''}` : ''}.`)}
          edit={{ path: '/assumptions/longevity', label: 'Change how lifespans are drawn' }}
        >
          <ChartCard
            title="Still alive, by age"
            takeaway={longevityTakeaway(people, summary, profile.simulation.end_age)}
            caption={`Share of the ${count(summary.paths)} simulated lives in which each person is alive, along ${people[0]?.name ? `${people[0].name}’s` : 'the first person’s'} age. A plan that only reaches life expectancy fails half the people who follow it.`}
            table={() => survivalTable(summary, people.map((p) => p.name))}
            filename="survival-by-age"
            dimmed={dimmed}
          >
            {survival.length > 1 && (
              <div className="mb-2" data-chart-legend>
                <Legend items={survival.map((s) => ({ key: s.key, label: s.label, color: s.color, swatch: 'line' as const }))} />
              </div>
            )}
            <AgeSeriesChart summary={summary} series={survival} format="share100" title="Lives, of 100" label="Share of lives in which each person is still alive, by age" />
          </ChartCard>
        </Block>

        <Block title="Social Security" statement="Benefits are computed from each person’s earnings as the simulation generates them, by the Social Security Administration’s own formula, so a life with a broken career earns a smaller benefit." facts={socialSecurityFacts(profile, summary.plan_moments)} edit={{ path: '/household/people/0/social_security', label: 'Change claiming ages or the benefit' }}>
          <ChartCard
            title="Social Security income, average by age"
            takeaway={
              ssPeak <= 0
                ? 'No simulated life receives Social Security.'
                : `Benefits average ${moneyCompact(ssPeak)} a year at their height${ssStart !== undefined ? `, from about age ${Math.floor(ssStart)}` : ''}${preview && preview.annual_spending_retired > 0 ? `: ${pct(ssPeak / preview.annual_spending_retired, 0)} of the ${moneyCompact(preview.annual_spending_retired)} this plan intends to spend a year in retirement` : ''}.`
            }
            caption={`Mean benefit per household with someone alive, each year, in today’s dollars.${people.length > 1 ? ' It falls late in life as partners die and the survivor keeps one benefit.' : ''}`}
            table={() => seriesTable(summary, 'Mean Social Security income per living household by age, today’s dollars', ss)}
            filename="social-security-by-age"
            dimmed={dimmed}
          >
            <AgeSeriesChart summary={summary} series={ss} stacked format="money" title="A year, today’s dollars" label="Mean Social Security income by age" height={190} />
          </ChartCard>
        </Block>

        <Block
          title="Health and care"
          statement={healthFacts(profile)[0]}
          facts={[
            ...healthFacts(profile).slice(1),
            ...(care && care.any > 0 ? [`In this run ${ofHundred(care.any).text} lives pay for long-term care${care.median !== null ? `, ${moneyCompact(care.median)} in the middle case` : ''}.`] : []),
            ...(jobs && profile.assumptions.career_risk ? [`Careers are simulated too: ${ofHundred(jobs.any).text} lives spend at least a month involuntarily out of work${jobs.p90 !== null && jobs.p90 >= 1 ? `, and one in ten more than ${Math.round(jobs.p90)} months` : ''}.`] : []),
          ]}
          edit={{ path: '/spending/healthcare', label: 'Change health and care assumptions' }}
        >
          <ChartCard
            title="Health and care costs, average by age"
            takeaway={
              healthPeak <= 0
                ? 'This plan carries no modelled health or care costs: they are whatever the budget already includes.'
                : modelled
                  ? `Health and care costs average ${moneyCompact(healthAt(65) ?? 0)} a year at 65 and ${moneyCompact(healthAt(90) ?? healthPeak)} at 90, in today’s dollars: they grow faster than prices, and care costs arrive late.`
                  : `Only long-term care is added to this plan’s budget: averaged over all lives it is ${moneyCompact(healthAt(80) ?? 0)} a year at 80 and ${moneyCompact(healthAt(90) ?? healthPeak)} at 90. Everyday health costs are whatever the budget already holds.`
            }
            caption="Mean per household with someone alive. Long-term care is an average over lives that pay nothing and lives that pay a great deal; the Outcomes view shows how it separates lives that fall short."
            table={() => seriesTable(summary, 'Mean health and long-term care spending per living household by age, today’s dollars', health)}
            filename="health-and-care-by-age"
            dimmed={dimmed}
          >
            <div className="mb-2" data-chart-legend>
              <Legend items={[...health].reverse().map((s) => ({ key: s.key, label: s.label, color: s.color }))} />
            </div>
            <AgeSeriesChart summary={summary} series={health} stacked format="money" title="A year, today’s dollars" label="Mean health and care costs by age" height={200} />
          </ChartCard>
        </Block>

        <Block
          title="Tax"
          statement={taxStatement(profile)}
          facts={
            usLaw
              ? [
                  `This household files as ${FILING_LABEL[status]}; the standard deduction is ${moneyCompact(standardDeduction(status))}.`,
                  ...(preview ? [`The plan’s own estimate for this year: ${moneyCompact(preview.annual_tax)} of tax on ${moneyCompact(preview.annual_income)} of earnings; in retirement, ${moneyCompact(preview.retirement_tax)} a year on withdrawals of ${moneyCompact(preview.retirement_need)}.`] : []),
                ]
              : []
          }
          edit={{ path: '/assumptions/taxes', label: 'Change the tax assumptions' }}
        >
          {usLaw && (
            <ChartCard
              title={`Federal brackets, ${FILING_LABEL[status]}, and where this plan’s income sits`}
              takeaway={taxTakeaway}
              caption={`Bracket thresholds are the published 2026 figures (${tax.path !== null ? `markers: the tax returns of life #${tax.path}, the one that ends with the median net worth` : 'no life to mark'}). The engine works out each year’s return in full; the markers are placed in the bracket it reports, at that year’s taxable income deflated to today’s dollars. Long-term gains and qualified dividends are taxed on their own schedule, stacked on top.`}
              filename="tax-brackets"
              table={() => ({
                caption: `Federal ordinary income brackets for tax year 2026, ${FILING_LABEL[status]}`,
                columns: [
                  { key: 'rate', label: 'Rate', format: (v: number) => pct(v, 0) },
                  { key: 'from', label: 'Taxable income from', format: moneyCompact },
                  { key: 'to', label: 'To', format: moneyCompact },
                ],
                rows: ladder.map((b) => ({ rate: b.rate, from: b.from, to: b.to })),
              })}
              dimmed={dimmed}
            >
              <TaxLadder ladder={ladder} points={taxRows} />
            </ChartCard>
          )}
        </Block>

        <SectionHead
          title="What this model does not know"
          aside={
            <a href={METHODOLOGY_URL} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 rounded-xs text-xs font-medium text-accent-ink hover:underline">
              About the model: the full methodology
              <Icon name="arrowRight" size={12} />
            </a>
          }
        >
          Left out on purpose, or simplified. None of it appears in any figure in the app, so judge the results with it in mind.
        </SectionHead>
        <div className="mt-6 grid grid-cols-1 gap-x-12 gap-y-8 @2xl:grid-cols-2 @5xl:grid-cols-3">
          {OMISSIONS.map((o) => (
            <section key={o.area}>
              <h3 className="text-sm font-medium text-ink-1">{o.area}</h3>
              <ul className="mt-2 space-y-2 text-sm leading-relaxed text-ink-2">
                {o.items.map((item) => (
                  <li key={item} className="flex gap-2">
                    <Icon name="minus" size={12} className="mt-1 shrink-0 text-ink-3" />
                    <span className="text-pretty">{item}</span>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
        <p className="mt-8 max-w-[46rem] text-xs leading-relaxed text-ink-3">
          Simulation settings: {count(run.total || summary.paths)} lives, seed {run.seed}, ending when the youngest adult would reach {profile.simulation.end_age}. The same seed always gives the same lives.
        </p>
      </div>
    </ChartSyncProvider>
  );
}
