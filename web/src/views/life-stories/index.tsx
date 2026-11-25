// Life stories: "what does one simulated life actually look like?". A percentile band is
// not a life; here one life is replayed in full from the engine (`pool.trace`), told in
// sentences, drawn against all the others, and opened up year by year down to its tax
// return. Every life shown states the rule by which it was picked.
import { useMemo, useState } from 'react';
import { askAssistant } from '@/assistant';
import type { Summary } from '@/engine/output-types';
import { scrollResultsTo } from '@/shell/scroll';
import { useProfileStore } from '@/state/profile-store';
import { navigateToLife, useSelectedLife, type LifePick } from '@/state/route';
import { Button, EmptyState, Skeleton, Tag, count } from '@/ui';
import { ChartCard, ChartSyncProvider, TimeRangeControl, type TimeRangePreset } from '@/ui/charts';
import type { ViewProps } from '../types';
import { AccountLane, AccountLaneLegend, FlowLane, FlowLaneLegend, MarketLane, MarketLaneLegend, TaxLane, TaxLaneLegend, accountLaneTable, flowLaneTable, marketLaneTable, paperValueLine, taxLaneTable } from './Lanes';
import { Picker } from './Picker';
import { Timeline, TimelineLegend, crowdFor, timelineTable, yearEnds } from './Timeline';
import { YearInspector } from './YearInspector';
import { buildLife, headline, lifeChoices, narrative, type Life, type LifeChoice } from './life';
import { useTrace } from './useTrace';
import { accountTakeaway, flowTakeaway, marketTakeaway, taxLaneTakeaway, timelineTakeaway } from './words';

function LifeSkeleton({ label }: { label: string }) {
  return (
    <div className="space-y-8 py-2" aria-busy="true" aria-label={label}>
      <div className="space-y-3">
        <Skeleton className="h-7 w-full max-w-2xl" />
        <Skeleton className="h-4 w-2/3 max-w-xl" />
      </div>
      <Skeleton className="h-80 w-full" />
      <Skeleton className="h-40 w-full" />
    </div>
  );
}

const WHY: Record<LifePick, string> = {
  p10: 'it ranks 10th of 100 by wealth at the end: an unlucky life, not the worst',
  median: 'it ranks 50th of 100 by wealth at the end',
  p90: 'it ranks 90th of 100 by wealth at the end: a lucky life, not the best',
  median_fi: 'work becomes optional in it at the typical age',
  failing: 'it runs short of money at the typical age among lives that do',
  sample: 'you chose it',
  scatter: 'you picked it out of a chart',
};

function whyText(choice: LifeChoice | undefined, why: LifePick): string {
  if (choice?.because) return choice.because;
  return WHY[choice?.why ?? why] ?? WHY.sample;
}

/** The years worth zooming to in one life. */
function presets(life: Life): TimeRangePreset[] {
  const out: TimeRangePreset[] = [{ id: 'all', label: 'Whole life', range: null }];
  const retire = life.scalar('retirement_age');
  const [start, end] = life.time.domain;
  if (life.hasAges && retire !== null && retire - start >= 4 && end - retire >= 6) {
    out.push({ id: 'working', label: 'Working years', range: [start, Math.min(end, Math.ceil(retire) + 1)] });
    out.push({ id: 'retired', label: 'Retirement', range: [Math.max(start, Math.floor(retire) - 1), end] });
  }
  return out;
}

function Story({ life, summary, selected, onSelect, why, dimmed }: { life: Life; summary: Summary; selected: number; onSelect: (year: number) => void; why: string; dimmed: boolean }) {
  const crowd = useMemo(() => crowdFor(life, summary), [life, summary]);
  const ends = useMemo(() => yearEnds(life), [life]);
  const sentences = useMemo(() => narrative(life, summary), [life, summary]);
  const ranges = useMemo(() => presets(life), [life]);
  const id = life.trace.path_index;
  const context = () => ({ life: id, picked_because: why, scalars: Object.fromEntries(life.trace.scalars.filter((s) => s.value !== null).map((s) => [s.scalar, s.value])) });
  const ask = () =>
    askAssistant(`Tell me about simulated life #${id}: why it went the way it did, and what in my plan would have made a life like this one go better.`, {
      ...context(),
      note: 'One simulated life of many, not a forecast. Money is in today’s dollars.',
      story: sentences.map((s) => s.text),
      events: life.events.filter((e) => e.priority >= 3).map((e) => e.detail),
    });

  return (
    <div className={dimmed ? 'opacity-60 transition-opacity duration-[var(--duration-slow)]' : 'transition-opacity duration-[var(--duration-slow)]'}>
      <div className="grid grid-cols-1 items-start gap-x-12 gap-y-6 @4xl:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
        <div>
          <p className="eyebrow">Life #{id}</p>
          <h2 data-testid="life-headline" className="mt-2 max-w-[34rem] text-2xl font-medium tracking-tight text-pretty text-ink-1 @4xl:text-[1.75rem] @4xl:leading-[2.25rem]">
            {headline(life)}.
          </h2>
          <p className="mt-3 max-w-[34rem] text-xs leading-relaxed text-pretty text-ink-3">
            Shown because {why}. It is one of {count(summary.paths)} simulated lives, each as likely as the next: read it as an example of what can happen, not as a forecast.
          </p>
          <Button className="mt-4" size="sm" icon="sparkles" onClick={ask}>
            Ask Claude about this life
          </Button>
        </div>
        <div>
          <h3 className="text-sm font-medium text-ink-2">What happens in it</h3>
          <p data-testid="life-narrative" className="mt-2 max-w-[44rem] text-sm leading-relaxed text-pretty text-ink-1">
            {sentences.map((s, i) =>
              s.year === undefined ? (
                <span key={i}>{s.text} </span>
              ) : (
                <span key={i}>
                  <a
                    href="#year-ledger"
                    className="rounded-xs decoration-line-strong decoration-1 underline-offset-[3px] hover:underline focus-visible:underline"
                    title="Open this year’s ledger"
                    onClick={(e) => {
                      e.preventDefault();
                      onSelect(s.year as number);
                      scrollResultsTo(document.getElementById('year-ledger'));
                    }}
                  >
                    {s.text}
                  </a>{' '}
                </span>
              ),
            )}
          </p>
          <p className="mt-2 text-xs text-ink-3">Select a sentence to open that year’s ledger below.</p>
        </div>
      </div>

      <div className="mt-10 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-line pt-5">
        <h2 className="text-base font-semibold tracking-tight">The whole life</h2>
        <TimeRangeControl time={life.time} presets={ranges} className="ml-auto" />
        <p className="hidden basis-full text-xs text-ink-3 @2xl:block">The five charts share one age axis and one crosshair. Drag across any of them to zoom all of them; click a year to open its ledger. Every figure is in today’s dollars.</p>
      </div>

      <ChartCard
        className="mt-6"
        title="One life"
        takeaway={timelineTakeaway(life, crowd, ends)}
        caption="The dark line is this life’s net worth month by month; the lighter line its investable assets. Behind them, the bands show where all simulated lives still going stand at each age, so a line outside the darker band is in the top or bottom quarter. Dots mark the events of this life; the ribbon under the axis is its stage. Labels that would collide are left out: point at a year to read everything that happened in it."
        table={() => timelineTable(life, crowd)}
        filename={`life-${id}`}
        explain={{ prompt: `Explain the timeline of simulated life #${id}: what shaped it and how it compares with the other lives.`, context: () => ({ ...context(), events: life.events.filter((e) => e.priority >= 3).map((e) => e.detail) }) }}
      >
        <TimelineLegend life={life} crowd={crowd} className="mb-1" />
        <Timeline life={life} crowd={crowd} selected={selected} onSelect={onSelect} />
      </ChartCard>

      <div className="mt-10 grid grid-cols-1 gap-y-10">
        <ChartCard
          title="What came in, what went out"
          takeaway={flowTakeaway(life)}
          caption="Gross income (wages, self-employment, Social Security, pensions, rent), spending and tax for each full calendar year. The dashed line appears when the spending rule or a shortfall moved spending away from what the plan intended. Part-years at the start and end are left out."
          table={() => flowLaneTable(life)}
          filename={`life-${id}-income-spending-tax`}
        >
          <FlowLaneLegend life={life} />
          <FlowLane life={life} selected={selected} onSelect={onSelect} />
        </ChartCard>

        <ChartCard
          title="The tax bracket, year by year"
          takeaway={taxLaneTakeaway(life)}
          caption="From each year’s tax return: the federal rate on one more dollar of ordinary income (the bracket, drawn as steps), and everything owed that year, payroll and state tax included, as a share of adjusted gross income. A year of Roth conversions or large pre-tax withdrawals shows as a step up. Open a year below to read its return line by line."
          table={() => taxLaneTable(life)}
          filename={`life-${id}-tax-bracket`}
        >
          <TaxLaneLegend life={life} />
          <TaxLane life={life} selected={selected} onSelect={onSelect} />
        </ChartCard>

        <ChartCard title="Where the money sat" takeaway={accountTakeaway(life)} caption={`Year-end balances by tax treatment, property at market value, debts below the line; the dark line is net worth.${paperValueLine(life) ? ' The dashed line is what vested startup holdings are worth on paper at the company’s share price that year, options at the price less the strike: it is not part of net worth until it turns into cash.' : ''}`} table={() => accountLaneTable(life)} filename={`life-${id}-accounts`}>
          <AccountLaneLegend life={life} />
          <AccountLane life={life} selected={selected} onSelect={onSelect} />
        </ChartCard>

        <ChartCard
          title="The market this life lived through"
          takeaway={marketTakeaway(life)}
          caption="What one dollar left in this life’s mix of investments would have grown to after inflation, from its monthly returns alone (no saving, no withdrawals), and how far prices rose. The axis is logarithmic so the same percentage move looks the same at any age; the shaded span is the deepest fall from a peak."
          table={() => marketLaneTable(life)}
          filename={`life-${id}-market`}
        >
          <MarketLaneLegend life={life} />
          <MarketLane life={life} selected={selected} onSelect={onSelect} />
        </ChartCard>
      </div>

      <div id="year-ledger" className="mt-12 scroll-mt-4 border-t border-line pt-5">
        <h2 className="text-base font-semibold tracking-tight">One year, line by line</h2>
        <p className="mt-1 mb-4 text-xs text-ink-3">The year marked on the charts above. Step through the years here, or click any year on a chart.</p>
        <YearInspector life={life} year={selected} onChange={onSelect} />
      </div>
    </div>
  );
}

/** The year whose ledger opens first: the year retirement starts, else the first full year. */
function defaultYear(life: Life): number {
  const retired = life.events.find((e) => e.kind === 'retired' || e.kind === 'financially_independent');
  if (retired) return retired.year;
  return Math.min(life.time.length - 1, life.fullYear[0] ? 0 : 1);
}

export default function LifeStoriesView({ profile, summary, run, pool }: ViewProps) {
  const route = useSelectedLife();
  const choices = useMemo(() => (summary ? lifeChoices(summary) : []), [summary]);
  const fallback = choices.find((c) => c.key === 'median') ?? choices[0];
  const path = route?.pathIndex ?? fallback?.pathIndex ?? null;
  // The lives of this run are path indices 0 .. paths - 1: anything else is not one of them and is not replayed.
  const planned = useProfileStore((s) => s.run.paths);
  const outside = path !== null && path >= Math.max(planned, summary?.paths ?? 0);
  const state = useTrace(pool, profile, run.seed, summary && !outside ? path : null);
  const life = useMemo(() => (state.trace ? buildLife(state.trace, profile) : null), [state.trace, profile]);
  const [chosen, setChosen] = useState<{ path: number; year: number } | null>(null);

  if (!summary || path === null) {
    // Errors and blocked plans are explained by the workspace above; nothing to draw here.
    return run.status === 'running' || run.status === 'queued' ? (
      <div className="mx-auto max-w-[76rem] py-8">
        <LifeSkeleton label="Simulating" />
      </div>
    ) : null;
  }
  const choice = choices.find((c) => c.pathIndex === path);
  const why = whyText(choice, route?.why ?? 'median');
  const year = life ? Math.min(life.time.length - 1, chosen && chosen.path === state.path ? chosen.year : defaultYear(life)) : 0;

  return (
    <div className="@container mx-auto max-w-[76rem] pb-8 pt-7">
      <div className="flex min-h-5 flex-wrap items-center gap-2">
        <p className="eyebrow">What does one of these lives actually look like?</p>
        {run.stale ? <Tag>Updating…</Tag> : state.loading && life ? <Tag>Replaying life #{path}…</Tag> : null}
        {summary.paths < 100 && !run.stale && <Tag tone="warn" icon="alertTriangle">{`Only ${summary.paths} lives simulated: “typical” and “unlucky” are rough`}</Tag>}
      </div>
      <div className="mt-3">
        <Picker choices={choices} path={path} paths={Math.max(summary.paths, 1)} onPick={(p, w) => navigateToLife(p, w)} />
      </div>

      <div className="mt-8 border-t border-line pt-6">
        {outside ? (
          <EmptyState
            icon="alertCircle"
            title={`There is no life #${count(path)} in this run`}
            action={
              fallback && (
                <Button size="sm" onClick={() => navigateToLife(fallback.pathIndex, 'median')}>
                  Open the middling life
                </Button>
              )
            }
          >
            This run simulates {count(Math.max(planned, summary.paths))} lives, numbered 0 to {count(Math.max(planned, summary.paths) - 1)}. Pick one above, or raise the number of lives in the top bar.
          </EmptyState>
        ) : state.error && !state.loading ? (
          <EmptyState icon="alertTriangle" tone="bad" title={`Life #${path} could not be replayed`} action={<Button size="sm" icon="rotate" onClick={state.retry}>Try again</Button>}>
            {state.error}
          </EmptyState>
        ) : life ? (
          <ChartSyncProvider key={`${state.path}:${life.time.length}`}>
            <Story life={life} summary={summary} selected={year} onSelect={(y) => setChosen({ path: state.path ?? path, year: y })} why={why} dimmed={run.stale || state.loading} />
          </ChartSyncProvider>
        ) : (
          <LifeSkeleton label={`Replaying life ${path}`} />
        )}
      </div>
    </div>
  );
}
