// Cash flow: "where does the money come from and go, year by year, across my life?".
// Every chart here draws the Summary's composition series: averages over the households
// with someone still alive that year. The page leads with money in against money out,
// lets any one year be opened as a flow, then shows what those flows build (the balance
// sheet), which accounts they pass through, how fast, and what tax takes.
import { useMemo, useState } from 'react';
import type { Summary } from '@/engine/output-types';
import type { RunState } from '@/state/run-store';
import { Skeleton, Tag, count, moneyCompact } from '@/ui';
import { ChartCard, ChartSyncProvider, TimeRangeControl, planMoments, type TimeRangePreset } from '@/ui/charts';
import type { ViewProps } from '../types';
import { RatesChart, RatesLegend, ratesTable } from './RatesChart';
import { YearControls, YearFlow, yearTable, type YearJump } from './YearFlow';
import { AccountFlows, AccountsLegend, BalanceLegend, BalanceSheet, FlowsLegend, MoneyFlows, TaxChart, TaxLegend, accountsTable, balanceTable, flowsTable, taxTable } from './charts';
import { cashFlowModel, handoverIndex, sharesOfUses, yearNodes, type CashFlowModel } from './model';
import { accountsTakeaway, balanceTakeaway, flowsTakeaway, leadSentence, ratesTakeaway, retirementIndex, taxTakeaway, yearTakeaway } from './text';

function CashFlowSkeleton() {
  return (
    <div className="mx-auto max-w-[76rem] space-y-10 py-8" aria-busy="true" aria-label="Simulating">
      <div className="space-y-3">
        <Skeleton className="h-3 w-28" />
        <Skeleton className="h-8 w-full max-w-2xl" />
        <Skeleton className="h-8 w-2/3 max-w-xl" />
      </div>
      <Skeleton className="h-96 w-full" />
      <Skeleton className="h-80 w-full" />
    </div>
  );
}

function Sharpness({ run, summary }: { run: RunState; summary: Summary }) {
  if (run.stale) return <Tag>Updating…</Tag>;
  if (run.status === 'running' && summary.paths < run.total) return <Tag tone={summary.paths < 1000 ? 'warn' : 'neutral'}>{summary.paths < 1000 ? `Early picture: ${count(summary.paths)} lives so far` : `Sharpening: ${count(summary.paths)} lives so far`}</Tag>;
  if (summary.paths < 100) return <Tag tone="warn" icon="alertTriangle">{`Only ${summary.paths} lives simulated: these averages are rough`}</Tag>;
  if (summary.paths < 1000) return <Tag tone="warn">{`${count(summary.paths)} lives: averages late in life rest on few of them`}</Tag>;
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

/** The years worth jumping to in the flow: now, when savings take over, well into retirement, late in life. */
function jumps(model: CashFlowModel): YearJump[] {
  const out: YearJump[] = [{ index: model.flowStart, label: 'Now' }];
  const add = (index: number | null, label: string) => {
    if (index !== null && index > model.flowStart && index <= model.end && !out.some((j) => Math.abs(j.index - index) < 2)) out.push({ index, label });
  };
  const h = handoverIndex(model);
  add(h === null ? null : Math.max(model.flowStart, h - 1), 'Last working years');
  add(h === null ? null : h + 1, 'Early retirement');
  add(retirementIndex(model), `Age ${Math.floor(model.time.ages[retirementIndex(model) ?? 0] ?? 0)}`);
  const late = model.time.indexAt(88);
  add((model.time.ages[model.end] ?? 0) >= 90 ? late : null, `Age ${Math.floor(model.time.ages[late] ?? 0)}`);
  return out.sort((a, b) => a.index - b.index);
}

function LifetimeTax({ summary }: { summary: Summary }) {
  const t = summary.headline.lifetime_tax;
  const s = summary.headline.lifetime_spending;
  if (t.p50 === null) return null;
  const rows = [
    { label: '1 life in 10 pays less than', value: t.p10 },
    { label: 'The middle life pays', value: t.p50, strong: true },
    { label: '1 life in 10 pays more than', value: t.p90 },
  ];
  return (
    <div className="min-w-0">
      <h3 className="text-sm font-medium text-ink-2">Tax over a whole life</h3>
      <dl className="mt-3 divide-y divide-line border-y border-line">
        {rows.map(
          (r) =>
            r.value !== null && (
              <div key={r.label} className="flex items-baseline justify-between gap-4 py-2.5">
                <dt className={r.strong ? 'text-sm text-ink-1' : 'text-sm text-ink-2'}>{r.label}</dt>
                <dd className={r.strong ? 'text-xl font-semibold tracking-tight text-ink-1' : 'num text-sm font-medium text-ink-1'}>{moneyCompact(r.value)}</dd>
              </div>
            ),
        )}
      </dl>
      <p className="mt-3 text-xs leading-relaxed text-pretty text-ink-3">
        Each life’s own total, in today’s dollars, ranked across {count(summary.paths)} lives: unlike the chart, these are not averages.
        {s.p50 !== null && s.p50 > 0 ? ` The middle life spends ${moneyCompact(s.p50)} over the same years, so tax comes to about $${Math.round((t.p50 / s.p50) * 100)} for every $100 spent.` : ''} Lives that are long or lucky pay the most.
      </p>
    </div>
  );
}

export default function CashFlowView({ profile, summary, run }: ViewProps) {
  const model = useMemo(() => (summary ? cashFlowModel(summary) : null), [summary]);
  const nodes = useMemo(() => (model ? yearNodes(model) : null), [model]);
  const moments = useMemo(() => (summary ? planMoments(summary, profile) : []), [summary, profile]);
  const ranges = useMemo(() => (summary ? presets(summary) : []), [summary]);
  const jumpList = useMemo(() => (model ? jumps(model) : []), [model]);
  const [chosen, setYear] = useState<number | null>(null);

  if (!summary || !model || !nodes) {
    // Errors and blocked plans are explained by the workspace above; nothing to draw here.
    return run.status === 'running' || run.status === 'queued' ? <CashFlowSkeleton /> : null;
  }
  const year = Math.min(model.end, Math.max(model.flowStart, chosen ?? model.flowStart));
  const dimmed = run.stale;
  const lead = leadSentence(model);
  const ageOf = (i: number) => Math.floor(model.time.ages[i] ?? 0);
  const context = () => ({ paths: summary.paths, basis: 'means per living household, today’s dollars', median_retirement_age: summary.headline.median_retirement_age });
  const at = (i: number) => ({ age: ageOf(i), money_in: Math.round(model.totalIn[i] ?? 0), ...Object.fromEntries(model.moneyIn.map((g) => [g.key, Math.round(g.values[i] ?? 0)])), ...sharesOfUses(model, i) });
  const r = retirementIndex(model);

  return (
    <ChartSyncProvider>
      <div className="@container mx-auto max-w-[76rem] pb-8 pt-7">
        <div className="flex min-h-5 flex-wrap items-center gap-2">
          <p className="eyebrow">Where the money comes from, and where it goes</p>
          <Sharpness run={run} summary={summary} />
        </div>
        <p data-testid="cash-flow-lead" className="mt-3 max-w-[46rem] text-2xl font-medium tracking-tight text-pretty text-ink-1 @4xl:text-[1.75rem] @4xl:leading-[2.25rem]">
          {lead.map((s, i) =>
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
          Every figure on this page is an average over the simulated households with someone still alive that year, in today’s dollars. An average is not a life: around retirement it blends lives still earning with lives
          already drawing on savings, and late in life it is pulled up by the luckiest, so the charts stop at {ageOf(model.end)}, {model.end < model.time.length - 2 ? 'while at least 1 household in 4 is still alive' : 'a year short of the plan’s end'}. Life stories shows the same ledger for one life at a time.
        </p>

        <div className="mt-10 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-line pt-5">
          <h2 className="text-base font-semibold tracking-tight">Across a lifetime</h2>
          <TimeRangeControl time={model.time} presets={ranges} className="ml-auto" />
          <p className="hidden basis-full text-xs text-ink-3 @2xl:block">The charts over age share one axis and one crosshair. Drag across any of them to zoom all of them; click the first to open that year as a flow.</p>
        </div>

        <ChartCard
          className="mt-6"
          title="Money in against money out"
          takeaway={flowsTakeaway(model)}
          caption={`Money in is drawn above the line and money out below it on one scale, so the eye can read straight down from what arrives to what it pays for. The two outlines mirror each other because every dollar that comes in is spent, taxed or saved. Withdrawals count as money in; money that only passes through the cash account is netted out.${model.flowStart > 0 ? ` The plan’s first, part-year (${summary.axis.years[0]}) is left out.` : ''}`}
          table={() => flowsTable(model)}
          filename="money-in-and-out-by-age"
          explain={{ context: () => ({ ...context(), now: at(model.flowStart), in_retirement: r === null ? null : at(r), savings_take_over_at_age: handoverIndex(model) === null ? null : ageOf(handoverIndex(model) as number) }) }}
          dimmed={dimmed}
        >
          <FlowsLegend model={model} />
          <MoneyFlows model={model} moments={moments} selected={year} onSelect={setYear} />
        </ChartCard>

        <ChartCard
          className="mt-12"
          title={`One year of money: age ${ageOf(year)}`}
          takeaway={yearTakeaway(model, year)}
          caption="Ribbons are as thick as the dollars they carry and every one passes through the bar in the middle: the simulation records how much came from each source and how much went to each use, not which dollar paid for what. Amounts are the average over households alive that year, so a rare cost (long-term care, a home purchase) appears as a thin ribbon rather than as the large bill it is for the lives that meet it."
          controls={<YearControls model={model} year={year} onChange={setYear} jumps={jumpList} />}
          table={() => yearTable(model, nodes, year)}
          filename={`money-flow-age-${ageOf(year)}`}
          explain={{ context: () => ({ ...context(), year: at(year), uses: Object.fromEntries(model.moneyOut.map((g) => [g.key, Math.round(g.values[year] ?? 0)])) }) }}
          dimmed={dimmed}
        >
          <YearFlow model={model} nodes={nodes} year={year} />
        </ChartCard>

        <div className="mt-12 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-line pt-5">
          <h2 className="text-base font-semibold tracking-tight">What the flows build</h2>
        </div>

        <ChartCard
          className="mt-6"
          title="What is owned and what is owed"
          takeaway={balanceTakeaway(model, summary)}
          caption="Year-end balances by tax treatment, with property at market value above the line and mortgages and other debt below it. The dark line is the net worth of the middle life (the median), which sits below the top of the stack minus the debts whenever a few very wealthy lives lift the average."
          table={() => balanceTable(model, summary)}
          filename="balance-sheet-by-age"
          explain={{ context: () => ({ ...context(), at_85_or_end: Object.fromEntries([...model.assets, ...model.liabilities].map((g) => [g.key, Math.round(g.values[Math.min(model.end, model.time.indexAt(85))] ?? 0)])), terminal_net_worth: summary.headline.terminal_net_worth }) }}
          dimmed={dimmed}
        >
          <BalanceLegend model={model} summary={summary} />
          <BalanceSheet model={model} summary={summary} moments={moments} />
        </ChartCard>

        <div className="mt-12 grid grid-cols-1 gap-x-12 gap-y-12 @4xl:grid-cols-2">
          <ChartCard
            title="Paid into and drawn from each account"
            takeaway={accountsTakeaway(model)}
            caption="Saving by destination above the line, withdrawals by source below, each account in the colour it wears in the balance sheet. Roth conversions move money between accounts and are not shown; required distributions count as withdrawals from pre-tax accounts."
            table={() => accountsTable(model)}
            filename="account-flows-by-age"
            explain={{ context: () => ({ ...context(), takeaway_basis: 'saving by destination and withdrawals by account, means' }) }}
            dimmed={dimmed}
          >
            <AccountsLegend model={model} />
            <AccountFlows model={model} />
          </ChartCard>

          <ChartCard
            title="How fast money is saved, then drawn"
            takeaway={ratesTakeaway(model)}
            caption="Saved: all saving, employer money included, over gross income, drawn while at least a quarter of living households still work. Drawn: withdrawals less anything saved, over investable assets a year earlier, drawn once at least a quarter have retired. Both are ratios of averages, not the rate of any one life; the dashed line is the 4% rule of thumb."
            table={() => ratesTable(model)}
            filename="saving-and-withdrawal-rates"
            explain={{ context: () => ({ ...context(), saving_rate_now: model.savingRate[model.flowStart], withdrawal_rate_in_retirement: r === null ? null : model.withdrawalRate[r] }) }}
            dimmed={dimmed}
          >
            <RatesLegend />
            <RatesChart model={model} moments={moments} />
          </ChartCard>
        </div>

        <div className="mt-12 grid grid-cols-1 items-start gap-x-12 gap-y-8 border-t border-line pt-5 @4xl:grid-cols-[minmax(0,7fr)_minmax(0,4fr)]">
          <ChartCard
            title="Tax, year by year"
            takeaway={taxTakeaway(model, summary)}
            caption="Tax in the year it is paid: withholding and estimates during the year, the balance of the return the following April. Payroll tax is Social Security, Medicare and self-employment tax. The strip beneath is total tax over all money coming in, withdrawals included, since in retirement taxable income and money in hand are very different things."
            table={() => taxTable(model)}
            filename="tax-by-age"
            explain={{ context: () => ({ ...context(), lifetime_tax: summary.headline.lifetime_tax, tax_share_now: model.taxShare[model.flowStart], tax_share_in_retirement: r === null ? null : model.taxShare[r] }) }}
            dimmed={dimmed}
          >
            <TaxLegend model={model} />
            <TaxChart model={model} />
          </ChartCard>
          <LifetimeTax summary={summary} />
        </div>
      </div>
    </ChartSyncProvider>
  );
}
