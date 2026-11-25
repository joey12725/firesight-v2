// Equity: "what is the stock in my employer worth to the plan, and what if it is worth
// nothing?" Every panel reads `summary.equity` (the live run) and is a ChartCard with a
// sentence computed from the data, a caption and a table. Two things here need a second
// simulation on the same lives: the standing comparison with the equity at zero, which
// starts when the view is first shown, and each decision, which runs only when asked.
import { useEffect, useMemo, useState } from 'react';
import { applyPatch, engineInput, type EnginePool } from '@/engine';
import { exerciseCostVested } from '@/engine/equity-math';
import type { EquitySummary, Summary } from '@/engine/output-types';
import type { PatchOp, Profile } from '@/engine/types';
import { METHODOLOGY_URL } from '@/shell/actions';
import { set } from '@/state/patches';
import { at, join } from '@/state/pointer';
import { useProfileStore } from '@/state/profile-store';
import { navigateToLife, navigateToView } from '@/state/route';
import type { RunState } from '@/state/run-store';
import { toast } from '@/state/ui-store';
import { Button, EmptyState, Icon, SegmentedControl, Skeleton, count, money, moneyCompact } from '@/ui';
import { ChartCard } from '@/ui/charts';
import { SectionHead, Sharpness, ViewSkeleton, openEditorAt } from '../outcomes/shared';
import { samePatch, type QuickScenario } from '../scenarios/quick';
import { useScenarioUi } from '../scenarios/store';
import type { ViewProps } from '../types';
import {
  ByOutcomeTable,
  CashChart,
  CashLegend,
  DecisionTable,
  OutcomeBar,
  OutcomeLegend,
  PaysDots,
  WhenChart,
  WhenLegend,
  WithWithoutChart,
  byOutcomeTable,
  cashTable,
  outcomeTable,
  paysTable,
  whenTable,
  withWithoutSides,
  withWithoutTable,
} from './charts';
import {
  cashModel,
  companyScope,
  decisionRows,
  decisionTable,
  equityCounted,
  equityLives,
  fiAgeShift,
  firstExercise,
  outcomeCounts,
  paysModel,
  primaryCounted,
  sidePaths,
  successOf,
  typicalOddsScenario,
  uncountedCompanies,
  usesOwnOdds,
  vestedOptions,
  visiblePanels,
  whenModel,
  withWithout,
  zeroScenario,
  type FirstExercise,
  type PaysView,
} from './model';
import {
  BY_OUTCOME_CAPTION,
  CASH_CAPTION,
  DECISIONS_CAPTION,
  DECISION_GERUND,
  AGE_SHIFT_MIN,
  EXCLUDED_LINE,
  LEFT_OUT_LABEL,
  WHEN_CAPTION,
  age1,
  byOutcomeTakeaway,
  cashTakeaway,
  companyScopeNote,
  decisionSentence,
  exerciseShortfall,
  equityCardSentence,
  outcomesCaption,
  outcomesTakeaway,
  paysCaption,
  paysTakeaway,
  span,
  whenTakeaway,
  withWithoutCaption,
  withWithoutPending,
  withWithoutTakeaway,
} from './text';
import { usePairedRun } from './usePairedRun';

/** Puts a what-if on the Scenarios bench (or switches on the one already there) and says so. */
function keepScenario(q: QuickScenario) {
  const store = useProfileStore.getState();
  const existing = store.scenarios.find((s) => samePatch(s.patch, q.patch));
  let id = existing?.id ?? null;
  if (existing) {
    if (!existing.enabled) store.updateScenario(existing.id, { enabled: true });
  } else {
    const result = store.addScenario({ name: q.name, patch: q.patch });
    if (!result.ok || !result.id) return void toast({ title: 'That what-if does not fit this plan', description: result.ok ? undefined : result.error, tone: 'bad' });
    id = result.id;
  }
  if (id) useScenarioUi.getState().select(id);
  toast({
    title: existing ? `“${existing.name}” is already a scenario` : `“${q.name}” added to Scenarios`,
    description: 'It runs through every simulated life beside the plan.',
    tone: 'good',
    action: { label: 'Open Scenarios', run: () => navigateToView('scenarios') },
  });
}

/** Counts every company with holdings in the baseline plan again. */
function countEquity(profile: Profile) {
  const ops = profile.private_equity.flatMap((c, i) => (!c.included && c.holdings.length > 0 ? [set(join('/private_equity', at(c, i), 'included'), true)] : []));
  if (ops.length > 0) useProfileStore.getState().edit(ops, { label: 'Count the startup equity in the plan', target: 'base' });
}

function Medians({ rows }: { rows: { label: string; fi: number | null; wealth: number | null }[] }) {
  return (
    <dl className="mt-3 grid grid-cols-[minmax(0,1fr)_auto_auto] gap-x-6 gap-y-1 text-xs @xl:max-w-[34rem]">
      <dt className="text-ink-3">Middle life, on those same lives</dt>
      <dd className="text-right text-ink-3">Work optional</dd>
      <dd className="text-right text-ink-3">Net worth at the end</dd>
      {rows.map((r) => (
        <div key={r.label} className="contents">
          <dt className="text-ink-2">{r.label}</dt>
          <dd className="num text-right text-ink-1">{r.fi === null ? 'never, in most' : age1(r.fi)}</dd>
          <dd className="num text-right text-ink-1">{r.wealth === null ? 'n/a' : moneyCompact(r.wealth)}</dd>
        </div>
      ))}
    </dl>
  );
}

function WithWithoutPanel({ profile, summary, run, pool, funded }: { profile: Profile; summary: Summary; run: RunState; pool: ViewProps['pool']; funded: boolean }) {
  const counted = equityCounted(profile);
  const zero = useMemo(() => zeroScenario(profile), [profile]);
  const typical = useMemo(() => typicalOddsScenario(profile), [profile]);
  const paired = usePairedRun(profile, run, pool, counted ? (zero?.patch ?? null) : null, { auto: true });
  const ownOdds = usesOwnOdds(profile);

  if (!counted) {
    const names = uncountedCompanies(profile).map((c) => c.name || 'the company');
    return (
      <ChartCard title="With the equity, and without it" takeaway={EXCLUDED_LINE} quiet={false}>
        <p className="max-w-[40rem] text-sm leading-relaxed text-pretty text-ink-2">
          {names.length ? `The stock and options in ${names.join(' and ')} are left out` : 'The stock and options are left out'}: no exercises, no proceeds, no tax. Every figure in the plan is the answer to “what if it is worth nothing”. The company’s fate is still
          simulated, because it can cost the job.
        </p>
        <Button className="mt-3" size="sm" variant="primary" icon="plus" onClick={() => countEquity(profile)} data-testid="equity-count">
          Count it in the plan
        </Button>
      </ChartCard>
    );
  }

  const result = paired.result;
  const w = result ? withWithout(summary, result.comparison) : null;
  const sides = w ? withWithoutSides(w, ownOdds) : null;
  const medians = result
    ? { fi: [result.base.headline.fi_age.p50, result.scenario.headline.fi_age.p50] as [number | null, number | null], wealth: [result.base.headline.terminal_net_worth.p50, result.scenario.headline.terminal_net_worth.p50] as [number | null, number | null] }
    : null;
  const busy = paired.status === 'running' || (paired.status === 'idle' && !result);
  return (
    <ChartCard
      title="With the equity, and without it"
      takeaway={<span data-testid="equity-with-without">{w ? withWithoutTakeaway(w, funded) : withWithoutPending(summary, busy)}</span>}
      takeawayText={w ? withWithoutTakeaway(w, funded) : withWithoutPending(summary, busy)}
      caption={w ? withWithoutCaption(funded, w.fiShift) : undefined}
      table={w && sides && medians ? () => withWithoutTable(w, sides, medians) : undefined}
      filename="equity-with-and-without"
      explain={
        w
          ? {
              context: () => ({
                paths_paired: w.paths,
                plan_holds_as_modelled: w.modelled,
                plan_holds_with_equity_left_out: w.zero,
                difference: w.difference,
                within_noise: w.withinNoise,
                holds_only_with_equity: w.rescued,
                holds_only_with_equity_and_company_failed: w.rescuedFailed,
                holds_only_without_equity: w.harmed,
                median_work_optional_age_shift_years: w.fiShift,
                retires_when_funded: funded,
              }),
            }
          : undefined
      }
      dimmed={run.stale || paired.outdated}
    >
      {sides && medians ? (
        <>
          <WithWithoutChart sides={sides} />
          <Medians
            rows={[
              { label: 'As modelled', fi: medians.fi[0], wealth: medians.wealth[0] },
              { label: LEFT_OUT_LABEL, fi: medians.fi[1], wealth: medians.wealth[1] },
            ]}
          />
        </>
      ) : paired.status === 'error' ? (
        <p className="text-sm text-bad" role="alert">
          {paired.error}
        </p>
      ) : (
        <div aria-busy="true" role="status">
          <Skeleton className="h-24 w-full" />
          <p className="mt-2 text-xs text-ink-3">{run.status === 'done' && !run.stale ? `Simulating ${count(sidePaths(run.total))} of the same lives with the equity left out…` : 'Waiting for the plan’s own run to finish, then the same lives are simulated with the equity left out.'}</p>
        </div>
      )}
      <div className="mt-4 flex flex-wrap items-center gap-2">
        {zero && (
          <Button size="sm" icon="plus" onClick={() => keepScenario(zero)} data-testid="equity-keep-zero">
            Keep as a scenario
          </Button>
        )}
        {ownOdds && typical && (
          <Button size="sm" icon="plus" onClick={() => keepScenario(typical)} data-testid="equity-typical-odds">
            Compare with typical odds
          </Button>
        )}
        {paired.outdated && <span className="text-xs text-ink-3">The plan has changed: these figures are being brought up to date.</span>}
      </div>
    </ChartCard>
  );
}

const firstExercises = new Map<string, Promise<FirstExercise>>();

/**
 * What the plan with `patch` applied exercises in its first month, from one traced life
 * (a few milliseconds; the first month is the same in every life). Null until it
 * arrives, or when `patch` is null or does not apply.
 */
function useFirstExercise(pool: EnginePool, profile: Profile, seed: number, patch: PatchOp[] | null): FirstExercise | null {
  const patched = useMemo(() => {
    if (!patch) return null;
    const applied = applyPatch(profile, patch);
    return applied.errors.length > 0 ? null : applied.profile;
  }, [profile, patch]);
  const key = patched ? engineInput(patched, seed).key : null;
  const [state, setState] = useState<{ key: string; value: FirstExercise } | null>(null);
  useEffect(() => {
    if (!patched || !key) return;
    let live = true;
    let hit = firstExercises.get(key);
    if (!hit) {
      hit = pool.trace(patched, 0, seed).then(firstExercise);
      firstExercises.set(key, hit);
      hit.catch(() => firstExercises.delete(key));
      if (firstExercises.size > 12) firstExercises.delete(firstExercises.keys().next().value as string);
    }
    hit.then(
      (value) => live && setState({ key, value }),
      () => undefined,
    );
    return () => {
      live = false;
    };
    // `patched` is represented by `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pool, key, seed]);
  return state && state.key === key ? state.value : null;
}

function Decision({ q, profile, run, pool, company, funded, tenderRate }: { q: QuickScenario; profile: Profile; run: RunState; pool: ViewProps['pool']; company: string; funded: boolean; tenderRate: number }) {
  const paired = usePairedRun(profile, run, pool, q.patch);
  const result = paired.result;
  const rows = useMemo(() => (result ? decisionTable(result.comparison) : []), [result]);
  const overall = result ? successOf(result.comparison) : null;
  const fiShift = result ? fiAgeShift(result.comparison) : null;
  const tender = q.key === 'equity_tender_max' || q.key === 'equity_tender_none';
  const occasion = tender ? `A tender offer pays something in about ${Math.round(tenderRate * 100)} of 100 lives as the plan stands.` : null;
  const sentence = result
    ? decisionSentence(DECISION_GERUND[q.key] ?? q.label, company, rows, overall ? { value: overall.difference.value, withinNoise: overall.within_noise } : null, { fiShift, funded, occasion })
    : null;
  // "Exercise now" is capped by what the household can pay: say how much of it the plan really makes.
  const exercising = q.key === 'equity_exercise_now';
  const targets = useMemo(() => (exercising ? profile.private_equity.filter((c) => c.included && c.policy.exercise === 'at_liquidity') : []), [exercising, profile]);
  const cost = targets.reduce((t, c) => t + exerciseCostVested(c), 0);
  const done = useFirstExercise(pool, profile, run.seed, exercising ? q.patch : null);
  const shortfall = exercising && done ? exerciseShortfall(done, vestedOptions(targets), cost) : null;
  const running = paired.status === 'running';
  const fi = result ? ([result.base.headline.fi_age.p50, result.scenario.headline.fi_age.p50] as const) : null;
  return (
    <li className="border-b border-line py-4" data-testid={`decision-${q.key}`}>
      <div className="flex flex-wrap items-start gap-x-4 gap-y-2">
        <div className="min-w-0 flex-1 basis-64">
          <h4 className="text-sm font-medium text-ink-1">
            {q.label}
            {cost > 0 && (
              <span className="font-normal text-ink-2">
                {' '}
                · {shortfall ? 'would need' : 'needs'} {money(Math.round(cost))} now, before tax
              </span>
            )}
          </h4>
          <p className="mt-0.5 max-w-[44rem] text-xs leading-relaxed text-pretty text-ink-3">{q.hint}</p>
          {shortfall && (
            <p className="mt-1.5 flex max-w-[44rem] items-start gap-1.5 text-xs leading-relaxed text-pretty text-ink-1" data-testid="exercise-shortfall">
              <Icon name="alertTriangle" size={13} className="mt-0.5 shrink-0 text-warn" />
              <span>{shortfall}</span>
            </p>
          )}
        </div>
        <Button size="sm" variant={result ? 'secondary' : 'primary'} icon={result ? 'rotate' : 'layers'} disabled={running || run.status === 'blocked' || run.status === 'idle' || (result !== null && !paired.outdated)} onClick={paired.start} data-testid={`compare-${q.key}`}>
          {running ? 'Comparing…' : result ? (paired.outdated ? 'Compare again' : 'Compared') : 'Compare'}
        </Button>
      </div>
      {paired.status === 'error' && (
        <p className="mt-2 text-xs text-bad" role="alert">
          {paired.error}
        </p>
      )}
      {running && !result && (
        <p className="mt-2 text-xs text-ink-3" role="status">
          Simulating {count(sidePaths(run.total))} of the same lives both ways…
        </p>
      )}
      {result && sentence && (
        <div className={paired.outdated ? 'mt-3 opacity-60 transition-opacity' : 'mt-3 transition-opacity'}>
          <p className="max-w-[44rem] text-base leading-snug font-medium tracking-tight text-pretty text-ink-1" data-testid="decision-sentence">
            {sentence}
          </p>
          <div className="mt-3 max-w-[44rem]">
            <DecisionTable rows={rows} label={`${q.label}: the change among the lives with each outcome`}>
              {fi && fi[0] !== null && fi[1] !== null && (
                <p className="mt-2 text-xs text-ink-2" data-testid="decision-fi">
                  Work optional, middle life over all outcomes: <span className="num text-ink-1">{age1(fi[0])}</span> as the plan stands, <span className="num text-ink-1">{age1(fi[1])}</span> with this
                  {fiShift !== null && Math.abs(fiShift) >= AGE_SHIFT_MIN ? ` (${span(fiShift)} ${fiShift > 0 ? 'later' : 'earlier'})` : ' (about the same)'}.
                </p>
              )}
            </DecisionTable>
          </div>
          <div className="mt-3 flex flex-wrap items-center gap-3">
            <Button size="sm" icon="plus" onClick={() => keepScenario(q)}>
              Keep as a scenario
            </Button>
            <span className="text-xs text-ink-3">
              {paired.outdated ? 'The plan has changed since this was compared.' : `Measured on ${count(result.paths)} lives, the same ones on both sides.`}
            </span>
          </div>
        </div>
      )}
    </li>
  );
}

function Lead({ summary, equity, counted, primary, scopeNote, run }: { summary: Summary; equity: EquitySummary; counted: boolean; primary: boolean; scopeNote: string | null; run: RunState }) {
  return (
    <header>
      <div className="flex min-h-5 flex-wrap items-center gap-2">
        <p className="eyebrow">Startup equity · {equity.company || 'the company'}</p>
        <Sharpness run={run} summary={summary} />
      </div>
      <p data-testid="equity-lead" className="mt-3 max-w-[44rem] text-2xl font-medium tracking-tight text-pretty text-ink-1 @4xl:text-[1.75rem] @4xl:leading-[2.25rem]">
        {equityCardSentence(summary, counted, primary)}
      </p>
      {scopeNote && (
        <p data-testid="equity-scope" className="mt-3 flex max-w-[44rem] items-start gap-2 text-sm leading-relaxed text-pretty text-ink-1">
          <Icon name="alertTriangle" size={14} className="mt-1 shrink-0 text-warn" />
          <span>{scopeNote}</span>
        </p>
      )}
      <p data-testid="equity-odds-note" className="mt-3 max-w-[44rem] text-sm leading-relaxed text-pretty text-ink-2">
        Nobody knows what happens to one company. The odds here are the ones set in the plan. By default they are our own estimates for a company at this stage: judgements anchored to published figures on how venture-backed companies
        have fared, not measured rates. Being off by ten of 100 futures either way on failure alone would not be surprising, and the Cautious lean in the editor shows what worse odds do. Each simulated life draws one fate from them, so read
        this page as a spread of futures, with the equity left out always in view.{' '}
        <button type="button" className="inline-flex items-center gap-1 rounded-xs font-medium text-accent-ink hover:underline" onClick={() => openEditorAt('/private_equity')}>
          Change the holdings or the odds
          <Icon name="arrowRight" size={12} />
        </button>{' '}
        <a href={`${METHODOLOGY_URL}#61-private-company-equity`} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 rounded-xs font-medium text-accent-ink hover:underline">
          How the odds were set
          <Icon name="arrowRight" size={12} />
        </a>
      </p>
    </header>
  );
}

export default function EquityView({ profile, summary, run, pool }: ViewProps) {
  const [view, setView] = useState<PaysView>('net');
  const equity = summary?.equity ?? null;
  const panels = useMemo(() => visiblePanels(profile, summary), [profile, summary]);
  const outcomes = useMemo(() => (equity ? outcomeCounts(equity) : null), [equity]);
  const pays = useMemo(() => (summary?.equity ? paysModel(summary, profile, view) : null), [summary, profile, view]);
  const when = useMemo(() => (equity ? whenModel(equity) : null), [equity]);
  const cash = useMemo(() => (summary?.equity ? cashModel(summary) : null), [summary]);
  const lives = useMemo(() => (summary ? equityLives(summary) : []), [summary]);
  const decisions = useMemo(() => decisionRows(profile), [profile]);

  if (!summary) return run.status === 'running' || run.status === 'queued' ? <ViewSkeleton /> : null;
  if (!equity || !outcomes || !when || !cash || !pays) {
    return (
      <EmptyState icon="info" title="This plan holds no private-company equity.">
        Add a company and what you hold in it under Startup equity in the plan editor, and this view shows what it does to the plan.
      </EmptyState>
    );
  }
  const counted = equityCounted(profile);
  const scope = companyScope(profile);
  const dimmed = run.stale;
  const company = equity.company || 'the company';
  const has = (id: (typeof panels)[number]) => panels.includes(id);
  const funded = profile.retirement.trigger.type === 'when_funded' || profile.retirement.trigger.type === 'fixed_number';

  return (
    <div className="@container mx-auto max-w-[76rem] pb-8 pt-7" data-testid="equity-view">
      <Lead summary={summary} equity={equity} counted={counted} primary={primaryCounted(profile)} scopeNote={companyScopeNote(scope)} run={run} />

      <div className="mt-10 grid grid-cols-1 items-start gap-x-12 gap-y-12 @5xl:grid-cols-2">
        {has('with_without') && <WithWithoutPanel profile={profile} summary={summary} run={run} pool={pool} funded={funded} />}
        {has('outcomes') && (
          <ChartCard
            title={`What happens to ${company}`}
            takeaway={outcomesTakeaway(equity)}
            caption={outcomesCaption(equity)}
            table={() => outcomeTable(equity, outcomes.rows, outcomes.of)}
            filename="equity-company-outcomes"
            explain={{ context: () => ({ company: equity.company, paths: equity.paths, outcomes: equity.outcomes.map((o) => ({ outcome: o.outcome, share: o.share.value })) }) }}
            dimmed={dimmed}
          >
            <OutcomeBar rows={outcomes.rows} of={outcomes.of} />
            <OutcomeLegend rows={outcomes.rows} className="mt-2" />
          </ChartCard>
        )}
      </div>

      {has('pays') && (
        <>
          <SectionHead title="What it pays, and when">Money in today’s dollars, over each whole simulated life.</SectionHead>
          <div className="mt-6 grid grid-cols-1 items-start gap-x-12 gap-y-12 @5xl:grid-cols-2">
            <ChartCard
              title="What the equity pays"
              takeaway={<span data-testid="equity-pays">{paysTakeaway(pays, equity)}</span>}
              takeawayText={paysTakeaway(pays, equity)}
              caption={paysCaption(pays)}
              controls={
                <SegmentedControl
                  aria-label="Proceeds"
                  size="sm"
                  value={view}
                  onChange={setView}
                  options={[
                    { value: 'net', label: 'After cost and tax' },
                    { value: 'gross', label: 'Before' },
                  ]}
                />
              }
              table={() => paysTable(equity, pays)}
              filename={view === 'net' ? 'equity-net-proceeds' : 'equity-gross-proceeds'}
              explain={{ context: () => ({ paths: equity.paths, view, no_cash_received_rate: equity.nothing_rate.value, out_of_pocket_rate: equity.loss_rate.value, exercised_rate: equity.exercised_rate.value, net_proceeds: equity.net_proceeds, net_proceeds_mean: equity.net_proceeds_mean, fewest_lives_carrying_half_the_gains: equity.net_proceeds_mean_half_from, net_proceeds_if_paid: equity.net_proceeds_if_paid, gross_proceeds: equity.gross_proceeds, exercise_cost: equity.exercise_cost }) }}
              dimmed={dimmed}
            >
              <PaysDots model={pays} />
            </ChartCard>
            {has('when') && (
              <ChartCard
                title="When"
                takeaway={whenTakeaway(when)}
                caption={WHEN_CAPTION}
                table={() => whenTable(when)}
                filename="equity-when"
                explain={{ context: () => ({ never_sold_or_listed: equity.liquidity_by_age.never, never_failed: equity.failed_by_age.never, takeaway: whenTakeaway(when) }) }}
                dimmed={dimmed}
              >
                <WhenLegend />
                <WhenChart model={when} ages={summary.axis.ages} years={summary.axis.years} />
              </ChartCard>
            )}
          </div>
        </>
      )}

      {has('by_outcome') && (
        <>
          <SectionHead title="What it does to the plan" />
          <ChartCard
            className="mt-6"
            title="The plan, by what happens to the company"
            takeaway={<span data-testid="equity-by-outcome">{byOutcomeTakeaway(equity, funded)}</span>}
            takeawayText={byOutcomeTakeaway(equity, funded)}
            caption={BY_OUTCOME_CAPTION}
            table={() => byOutcomeTable(equity)}
            filename="plan-by-company-outcome"
            explain={{ context: () => ({ company: equity.company, by_outcome: equity.by_outcome.map((o) => ({ outcome: o.outcome, lives: o.paths, enough: o.enough, plan_holds: o.success_rate.value, median_fi_age: o.fi_age.p50, median_terminal_net_worth: o.terminal_net_worth.p50, median_net_proceeds: o.net_proceeds.p50 })) }) }}
            dimmed={dimmed}
          >
            <ByOutcomeTable equity={equity} />
          </ChartCard>
        </>
      )}

      {has('cash') && (
        <ChartCard
          className="mt-12"
          title="Cash in and out"
          takeaway={cashTakeaway(cash)}
          caption={CASH_CAPTION}
          table={() => cashTable(cash)}
          filename="equity-cash-by-age"
          explain={{ context: () => ({ mean_cash_received_per_life: cash.meanIn, mean_exercise_cost_per_life: cash.meanOut, mean_paper_value_now: cash.paperNow }) }}
          dimmed={dimmed}
        >
          <CashLegend />
          <CashChart model={cash} />
        </ChartCard>
      )}

      {has('lives') && lives.length > 0 && (
        <section aria-labelledby="equity-lives-title" className="mt-12 border-t border-line pt-5">
          <h3 id="equity-lives-title" className="text-sm font-medium text-ink-2">
            See a life
          </h3>
          <p className="mt-1 max-w-[44rem] text-sm leading-relaxed text-pretty text-ink-2">
            Each is one simulated life, chosen by a stated rule: the middle life by wealth at the end among those where {company} fails, and the middle and the 90th of 100 by net proceeds among those where the equity pays.
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            {lives.map((l) => (
              <Button key={l.label} size="sm" iconRight="arrowRight" onClick={() => navigateToLife(l.path)} data-testid={`life-${l.label}`}>
                {l.button}
              </Button>
            ))}
          </div>
        </section>
      )}

      {has('decisions') && decisions.length > 0 && (
        <section aria-labelledby="equity-decisions-title" className="mt-14 border-t border-line-strong pt-6">
          <h2 id="equity-decisions-title" className="text-base font-semibold tracking-tight text-ink-1">
            Your decisions
          </h2>
          <p className="mt-1 max-w-[46rem] text-sm leading-relaxed text-pretty text-ink-2">
            What you can choose about the equity, each compared with the plan as it stands on the same simulated lives and broken down by what happens to {company}. Nothing runs until you press Compare.
          </p>
          <ul className="mt-2 border-t border-line">
            {decisions.map((q) => (
              <Decision key={q.key} q={q} profile={profile} run={run} pool={pool} company={company} funded={funded} tenderRate={equity.tender_rate.value} />
            ))}
          </ul>
          <p className="mt-3 max-w-[48rem] text-xs leading-relaxed text-pretty text-ink-3">{DECISIONS_CAPTION}</p>
        </section>
      )}
    </div>
  );
}
