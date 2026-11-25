// Every sentence of the Equity view, and the two sentences about the equity that sit
// elsewhere (the note under the pinned success figure, the card on the Overview). Plain
// strings computed from the model, so they are tested and reused for image exports.
import type { EquityOutcome, EquitySummary, Summary } from '@/engine/output-types';
import { count, moneyCompact } from '@/ui/format';
import { age } from '@/ui/charts/format';
import { livesOf, outcomeCounts, outcomeEnds, sameWithinNoise, soldOrListed, type CashModel, type CompanyScope, type DecisionRow, type FirstExercise, type PaysModel, type WhenModel, type WithWithout, decisionEnds } from './model';

/** Legend and table labels, in the engine's outcome order. */
export const OUTCOME_LABEL: Record<EquityOutcome, string> = {
  failed: 'Shuts down',
  still_private: 'No outcome within the life',
  acquired: 'Is acquired',
  ipo: 'Goes public',
};

/** "the lives where Northwind ..." */
const OUTCOME_VERB: Record<EquityOutcome, string> = {
  failed: 'fails',
  still_private: 'has no outcome within the life',
  acquired: 'is acquired',
  ipo: 'goes public',
};

/** A share of lives as a whole count of 100 (of the lives themselves under 100). */
const per = (share: number, of = 100) => String(Math.round(Math.min(1, Math.max(0, share)) * of));
/**
 * A difference in lives per 100, one decimal. Rounded first, so a value that rounds to
 * nothing reads "0.0" with no sign (never "−0.0").
 */
export function points(v: number, plus = false): string {
  const r = Math.round(v * 1000) / 10;
  if (r === 0) return '0.0';
  return `${r < 0 ? '−' : plus ? '+' : ''}${Math.abs(r).toFixed(1)}`;
}
/** An age with one decimal always, so a column of ages lines up ("47.0" under "45.8"). */
export const age1 = (v: number) => v.toFixed(1);
const list = (parts: string[]) => (parts.length <= 1 ? (parts[0] ?? '') : `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`);
/**
 * The least shift in a median age worth stating, in years: two months. The median is
 * read to the month, and a single month either way is within what the same lives give
 * for a change that does nothing to the retirement date.
 */
export const AGE_SHIFT_MIN = 0.125;

/** "3 months", "1 month", "2.5 years": a shift in an age. */
export function span(years: number): string {
  const months = Math.round(Math.abs(years) * 12);
  if (months < 24) return `${months} ${months === 1 ? 'month' : 'months'}`;
  return `${(Math.round(Math.abs(years) * 10) / 10).toFixed(1).replace(/\.0$/, '')} years`;
}
const signedMoney = (v: number) => `${v < 0 ? '−' : ''}${moneyCompact(Math.abs(v))}`;
const ofLives = (of: number) => (of === 100 ? 'of 100' : `of ${count(of)}`);

// ---- panel 1 ------------------------------------------------------------------------------

export const EXCLUDED_LINE = 'The equity is not counted in this plan.';

/** The other side of the standing comparison: the equity left out of the plan altogether (nothing exercised, nothing received, no tax). */
export const LEFT_OUT_LABEL = 'With the equity left out';

/** Before the side run has finished: the live figure alone. */
export function withWithoutPending(summary: Summary, running: boolean): string {
  const of = livesOf(summary.paths);
  const lead = `The plan holds in ${per(summary.headline.success_rate.value, of)} ${ofLives(of)} lives with the equity as modelled.`;
  return running ? `${lead} The same lives are being simulated with the equity left out.` : lead;
}

/**
 * Whether the two success figures of the standing comparison need a decimal: when they
 * round to the same whole count, or when the gap between the rounded counts disagrees
 * with the measured difference ("89 and 88, difference 0.4").
 */
export function withWithoutDecimals(w: Pick<WithWithout, 'modelled' | 'zero' | 'difference'>): 0 | 1 {
  const a = Math.round(w.modelled.value * 100);
  const b = Math.round(w.zero.value * 100);
  return a === b || a - b !== Math.round(w.difference.value * 100) ? 1 : 0;
}

/** A share of lives per 100 with the standing comparison's decimals. */
export const holdsText = (share: number, decimals: 0 | 1) => (decimals === 1 ? (share * 100).toFixed(1) : per(share));

/**
 * `funded`: the plan retires when the savings are enough, so the equity also moves the
 * retirement date. The sentence never credits the payout with lives in which the company
 * failed, and when the equity measurably hurts it leads with that.
 */
export function withWithoutTakeaway(w: WithWithout, funded = false): string {
  const d = withWithoutDecimals(w);
  const lead = `The plan holds in ${holdsText(w.modelled.value, d)} of 100 lives with the equity as modelled, and in about ${holdsText(w.zero.value, d)} with it left out (difference ${points(w.difference.value)}, 95% interval ${points(w.difference.lo)} to ${points(w.difference.hi)}, measured on ${count(w.paths)} of the same lives).`;
  const rescued = Math.round(w.rescued * 100);
  const harmed = Math.round(w.harmed * 100);
  const failed = w.rescuedFailed === null ? 0 : Math.min(rescued, Math.round(w.rescuedFailed * 100));
  if (w.withinNoise) {
    const flips = rescued >= 1 || harmed >= 1 ? ` The result still differs in some lives: the plan holds only with the equity in ${rescued} of 100, and only without it in ${harmed}.` : '';
    return `${lead} Counting the equity or leaving it out makes no measurable difference to whether the plan holds.${flips}`;
  }
  if (w.difference.value < 0) {
    return `${lead} Counting the equity, the plan holds in fewer lives: only without it in ${harmed} of 100, and only with it in ${rescued}. The note below gives the reasons.`;
  }
  const why = funded ? ': what made the difference there is the money spent on exercising and a later retirement, not a payout' : ', so a payout is not what made the difference there';
  const inFailed = failed >= 1 ? ` (in ${failed} of those the company failed and paid nothing${why})` : '';
  return `${lead} The result differs in ${rescued + harmed} of 100 lives: the plan holds only with the equity in ${rescued}${inFailed} and only without it in ${harmed}.`;
}

const WITH_WITHOUT_BASE =
  'Each bar is that plan’s own 95% sampling interval. The difference between them is measured life by life on the same simulated futures, so it is known more precisely than the two bars suggest. Left out means nothing is exercised, received or taxed. The equity can also hurt: paying to exercise options that end up worthless, or retiring earlier on a payout that then has to last longer. Ages and net worth beneath are medians over those same lives.';

/**
 * The caption of the standing comparison. A plan that retires when the savings are
 * enough also has its retirement date moved by the equity, which the caption says, with
 * the measured shift when there is one.
 */
export function withWithoutCaption(funded: boolean, fiShift: number | null = null): string {
  if (!funded) return WITH_WITHOUT_BASE;
  const measured = fiShift !== null && Math.abs(fiShift) >= AGE_SHIFT_MIN ? ` Here work becomes optional ${span(fiShift)} ${fiShift > 0 ? 'later' : 'earlier'} in the middle life with the equity than without.` : '';
  return `${WITH_WITHOUT_BASE} This plan retires when the savings are enough, so the equity also moves the retirement date: a payout brings it forward, and money spent on exercising options that end up worthless pushes it back. A later retirement by itself makes a plan hold more often and an earlier one less often, so part of any difference in whether the plan holds is that timing, not the money.${measured}`;
}

/**
 * Under the lead when the page's outcomes and its dollars are not about one and the
 * same company: which company the outcomes follow and which the dollar figures total.
 */
export function companyScopeNote(scope: CompanyScope | null): string | null {
  if (!scope) return null;
  const others = scope.counted.filter((n) => n !== scope.primary);
  if (!scope.primaryCounted) {
    return `What happens on this page (shut down, acquired, public) is what happens to ${scope.primary}, the largest position, which is not counted in the plan. The dollar figures are for ${list(scope.counted)} only, and the odds of ${others.length === 1 ? 'that company' : 'those companies'} are not shown here. Count ${scope.primary}, or remove it from the plan, to change which company this page follows.`;
  }
  return `What happens on this page (shut down, acquired, public) is what happens to ${scope.primary}, the largest position. The dollar figures total ${list(scope.counted)}, so a life where ${scope.primary} fails can still include a payout from ${list(others)}.`;
}

// ---- panel 2 ------------------------------------------------------------------------------

export function outcomesTakeaway(equity: EquitySummary): string {
  const { of, rows } = outcomeCounts(equity);
  const n = (o: EquityOutcome) => rows.find((r) => r.outcome === o)?.lives ?? 0;
  const name = equity.company || 'the company';
  return `In ${n('failed')} ${ofLives(of)} lives ${name} shuts down, in ${n('acquired')} it is acquired and in ${n('ipo')} it goes public. In ${n('still_private')} none of those happens within the life.`;
}

export const outcomesCaption = (equity: EquitySummary) =>
  `The odds are the ones set under Startup equity, as drawn in ${count(equity.paths)} simulated lives. A company still private after ten years faces the same odds again; after thirty it is written off.`;

// ---- panel 3 ------------------------------------------------------------------------------

/**
 * Words for the two partitions, kept apart: "no cash" is only ever the gross figure
 * (nothing was received), and the net groups are out of pocket, about even and ahead.
 * The net sentence opens with the gross count so the two can be reconciled.
 */
export function paysTakeaway(m: PaysModel, equity: EquitySummary): string {
  const of = m.dots.length;
  const lives = ofLives(of);
  if (of === 0) return 'No simulated life has a figure for what the equity pays.';
  // The count of lives that received no cash is the same figure wherever the page states it: the engine's share, as in the lead.
  const noCash = Math.round(Math.min(1, Math.max(0, equity.nothing_rate.value)) * of);
  if (m.view === 'gross') {
    const lead = `Before the cost of exercising and before tax, the equity pays out no cash in ${noCash} ${lives} lives.`;
    return noCash < of && m.paidMedian !== null ? `${lead} In the other ${of - noCash} it pays ${moneyCompact(m.paidMedian)} in the middle one.` : lead;
  }
  const parts: string[] = [];
  parts.push(`No cash ever comes back in ${noCash} ${lives} lives.`);
  const groups = [
    m.loss > 0 && `is out of pocket in ${m.loss}${m.lossMedian !== null ? ` (median ${moneyCompact(Math.abs(m.lossMedian))})` : ''}`,
    m.nothing > 0 && `ends within ${moneyCompact(m.band)} of even in ${m.nothing}`,
    m.paid > 0 && `comes out ahead in ${m.paid}${m.paidMedian !== null ? ` (${moneyCompact(m.paidMedian)} in the middle one)` : ''}`,
  ].filter((x): x is string => Boolean(x));
  parts.push(`After the cost of exercising and the tax it caused, the household ${list(groups)}.`);
  parts.push(meanSentence(equity));
  const cost = equity.exercise_cost.p50;
  if (equity.exercised_rate.value >= 0.01 && cost !== null) parts.push(`Options are exercised for cash in ${per(equity.exercised_rate.value)} of 100 lives, for ${moneyCompact(cost)} in the middle one.`);
  return parts.join(' ');
}

/** Under this many lives carrying half of the gains, the mean's sampling interval is not a 95% bound and is not quoted as one. */
export const MEAN_CARRIED_BY_FEW = 100;

/**
 * The average, with how far to trust it. It is carried by the best few lives; when
 * fewer than 100 of them hold half of all the gains the run's own interval understates
 * how much the average moves from run to run, so "give or take" is not printed.
 */
export function meanSentence(equity: EquitySummary): string {
  const mean = equity.net_proceeds_mean;
  const half = equity.net_proceeds_mean_half_from;
  if (half > 0 && half < MEAN_CARRIED_BY_FEW) {
    return `The average over all lives is ${signedMoney(mean.value)}, but half of all the gains come from the best ${count(half)} of ${count(equity.paths)} simulated lives, so it moves a lot from one run to the next. Do not plan on it.`;
  }
  const carried = half > 0 ? `, carried by the best few: half of all the gains come from ${count(half)} of ${count(equity.paths)} lives` : '';
  return `The average over all lives is ${signedMoney(mean.value)} (give or take ${moneyCompact(2 * mean.std_error)})${carried}.`;
}

export function paysCaption(m: PaysModel): string {
  if (m.view === 'gross') return 'Cash received from the equity over the whole life, before the cost of exercising and before tax, today’s dollars. Each dot is one equally likely life; the axis is logarithmic.';
  return `After the cost of exercising and the tax the equity caused, today’s dollars. Each dot is one equally likely life; the axis is logarithmic. Out of pocket counts the strike paid and the tax the exercise caused where less came back: the minimum tax on incentive options can be several times the strike. Lives within ${moneyCompact(m.band)} of zero count as about even: when the company fails, the cost of shares already held is a capital loss that saves a little tax, so a life that received no cash can still sit slightly above zero.`;
}

// ---- panel 4 ------------------------------------------------------------------------------

export function whenTakeaway(m: WhenModel): string {
  if (m.half === null) {
    const end = m.failed[m.failed.length - 1] ?? 0;
    const i = Math.max(
      0,
      m.failed.findIndex((v) => v >= end / 2),
    );
    if (end < 0.01) return 'The company is neither sold, listed nor shut down in as many as 1 life in 100.';
    return `By age ${(m.ages[i] ?? 0) + 1} the company has failed in ${per(m.failed[i] ?? 0)} of 100 lives. It is sold or listed in fewer than 1.`;
  }
  const x = (m.ages[m.half] ?? 0) + 1;
  return `By age ${x} the company has been sold or listed in ${per(m.liquidity[m.half] ?? 0)} of 100 lives and has failed in ${per(m.failed[m.half] ?? 0)}.`;
}

export const WHEN_CAPTION =
  'The share of all simulated lives in which each thing has happened by the end of each age. A sale or listing is when the equity can turn into cash; a listing is followed by a lockup before shares can be sold. Lives in which neither has happened by the end are not on either line.';

// ---- panel 5 ------------------------------------------------------------------------------

export function byOutcomeTakeaway(equity: EquitySummary, retiresWhenFunded: boolean): string {
  const { low, high } = outcomeEnds(equity.by_outcome);
  const name = equity.company || 'the company';
  const holds = (v: number | null) => per(v ?? 0);
  if (!low && !high) return `Too few simulated lives have any one outcome to say how the plan fares in each.`;
  if (!low && high) return `If ${name} ${OUTCOME_VERB[high.outcome]} the plan holds in ${holds(high.success_rate.value)} of 100 of those lives. Too few lives have it fail to say how the plan fares then.`;
  if (low && !high) return `If ${name} fails the plan holds in ${holds(low.success_rate.value)} of 100 of those lives. Too few lives have it sold or listed to say how the plan fares then.`;
  if (!low || !high) return '';
  const lead = `If ${name} fails the plan holds in ${holds(low.success_rate.value)} of 100 of those lives; if it ${OUTCOME_VERB[high.outcome]}, in ${holds(high.success_rate.value)}.`;
  const noise = sameWithinNoise(low, high) ? ' That gap is within sampling noise.' : '';
  const [a, b] = [low.fi_age.p50, high.fi_age.p50];
  const timing = a !== null && b !== null && Math.abs(a - b) >= 1 ? ` ${retiresWhenFunded ? 'What the outcome changes most is when' : 'It also changes when'}: work becomes optional at ${age(a)} in the middle life if it fails and at ${age(b)} if it ${OUTCOME_VERB[high.outcome]}.` : '';
  return `${lead}${noise}${timing}`;
}

export const BY_OUTCOME_CAPTION =
  'Each row is the lives with that outcome and nothing else: the same plan, sorted by what happened to the company. Plan holds is the share of those lives, with its 95% sampling interval. Ages and dollars are medians within the row, in today’s dollars; net proceeds are after the cost of exercising and the tax the equity caused. Rows with fewer than 30 lives are not summarised.';

// ---- panel 6 ------------------------------------------------------------------------------

export function cashTakeaway(m: CashModel): string {
  if (m.meanIn === null || m.meanOut === null) return 'No simulated life has a figure for the cash the equity moves.';
  return `Averaged over all lives, the equity brings in ${moneyCompact(m.meanIn)} and costs ${moneyCompact(m.meanOut)} to exercise; on paper the vested part is worth ${moneyCompact(m.paperNow)} at the end of ${m.paperYear}.`;
}

export const CASH_CAPTION =
  'Averages mix lives where it paid and lives where it did not: see the panels above for how likely each is. Bars are a year’s cash received (up) and paid to exercise options (down), before tax, per household still living. Paper value is vested holdings at the company’s share price that year, options at the price less the strike; it is not part of net worth. Today’s dollars.';

// ---- panel 8 ------------------------------------------------------------------------------

/** How each decision reads at the head of its sentence, by quick-scenario key. */
export const DECISION_GERUND: Record<string, string> = {
  equity_exercise_now: 'Exercising now',
  equity_tender_max: 'Selling the maximum in tenders',
  equity_tender_none: 'Selling nothing in tenders',
  equity_lapse: 'Letting options lapse',
  equity_zero: 'Counting the equity at zero',
  equity_typical_odds: 'Using typical odds',
  equity_leave: 'Leaving in a year',
};

const better = (v: number) => (Math.abs(v) < 500 ? 'no better or worse off' : `${moneyCompact(Math.abs(v))} ${v < 0 ? 'poorer' : 'richer'}`);

export interface DecisionContext {
  /** Change in the median age at which work becomes optional, with the decision minus without, in years. */
  fiShift?: number | null;
  /** The plan retires when the savings are enough, so a decision that costs cash also moves the retirement date. */
  funded?: boolean;
  /** A sentence on how often the occasion for the decision arises ("A tender offer pays in about 12 of 100 lives."). */
  occasion?: string | null;
}

/** "in about 1 more life in 200": a change under half a life in 100, without fractional lives. */
function rareLives(v: number): string {
  const one = Number((1 / Math.abs(v)).toPrecision(1));
  return `the plan holds in about 1 ${v > 0 ? 'more' : 'fewer'} life in ${count(one)} overall`;
}

/** What moved in the lives where the occasion arose, when the middle life of every outcome is unchanged. */
function movedClauses(rows: DecisionRow[], name: string): string[] {
  const ok = rows.filter((r) => r.enough);
  const where = (r: DecisionRow) => `where ${name} ${OUTCOME_VERB[r.outcome]}`;
  const biggest = <T>(list: T[], size: (x: T) => number) => list.reduce<T | null>((best, x) => (best === null || size(x) > size(best) ? x : best), null);
  const holds = biggest(
    ok.filter((r) => r.holds && !r.holds.withinNoise && Math.round(Math.abs(r.holds.value) * 1000) > 0),
    (r) => Math.abs(r.holds?.value ?? 0),
  );
  const mean = biggest(
    ok.filter((r) => r.mean && !r.mean.withinNoise && Math.abs(r.mean.value) >= 500),
    (r) => Math.abs(r.mean?.value ?? 0),
  );
  const median = biggest(
    ok.filter((r) => r.wealth !== null && Math.abs(r.wealth) >= 500),
    (r) => Math.abs(r.wealth ?? 0),
  );
  const out: string[] = [];
  if (holds?.holds) out.push(`the plan holds in ${points(Math.abs(holds.holds.value))} ${holds.holds.value > 0 ? 'more' : 'fewer'} of 100 of the lives ${where(holds)}`);
  if (mean?.mean) {
    const [lo, hi] = [Math.min(Math.abs(mean.mean.lo), Math.abs(mean.mean.hi)), Math.max(Math.abs(mean.mean.lo), Math.abs(mean.mean.hi))];
    out.push(`on average the household ends ${moneyCompact(Math.abs(mean.mean.value))} ${mean.mean.value < 0 ? 'poorer' : 'richer'} ${where(mean)} (95% interval ${moneyCompact(lo)} to ${moneyCompact(hi)}), an average carried by the few lives it touches`);
  }
  if (median && median.wealth !== null) out.push(`the middle life ends ${better(median.wealth)} ${where(median)}`);
  return out;
}

/**
 * "Exercising now leaves the household $40k poorer in the lives where Northwind fails
 * and $310k richer where it goes public; the plan holds in 2 more of 100 lives overall."
 * `overall` is the paired change in the share of all lives in which the plan holds.
 * A decision that leaves the middle life of both classes alone is said to change nothing
 * only when nothing else moved either: otherwise the sentence leads with what did, from
 * the changes in "plan holds" and the mean changes whose intervals exclude zero.
 */
export function decisionSentence(lead: string, company: string, rows: DecisionRow[], overall: { value: number; withinNoise: boolean } | null, context: DecisionContext = {}): string {
  const { low, high } = decisionEnds(rows);
  const name = company || 'the company';
  const n = overall ? Math.round(Math.abs(overall.value) * 100) : 0;
  const total = !overall
    ? ''
    : overall.withinNoise || overall.value === 0
      ? 'no measurable difference to whether the plan holds overall'
      : n === 0
        ? rareLives(overall.value)
        : `the plan holds in ${n} ${overall.value > 0 ? 'more' : 'fewer'} of 100 lives overall`;
  const tail = total ? `; ${total}.` : '.';
  const after = decisionAfter(overall, context);
  if (low && high && low.wealth !== null && high.wealth !== null) {
    if (Math.abs(low.wealth) < 500 && Math.abs(high.wealth) < 500) {
      const middle = `${lead} changes nothing in the middle life, whether ${name} fails or ${OUTCOME_VERB[high.outcome]}`;
      const moved = movedClauses(rows, name);
      // Nothing moves anywhere: the decision acts in a minority of lives (a tender offer, an expiry) and too weakly to measure.
      if (moved.length === 0) return `${middle}: it matters only in the fewer lives where the occasion for it arises${tail}${after}`;
      const overallSentence = total ? ` ${total.charAt(0).toUpperCase()}${total.slice(1)}.` : '';
      return `${middle}, but it does move the lives where the occasion for it arises: ${moved.join('; ')}.${overallSentence}${after}`;
    }
    return `${lead} leaves the household ${better(low.wealth)} in the lives where ${name} fails and ${better(high.wealth)} where it ${OUTCOME_VERB[high.outcome]}${tail}${after}`;
  }
  const one = low ?? high;
  if (one && one.wealth !== null) return `${lead} leaves the household ${better(one.wealth)} in the lives where ${name} ${OUTCOME_VERB[one.outcome]}${tail}${after}`;
  return total ? `${lead}: ${total}. Too few lives have any one outcome to say more.` : `${lead}: too few lives have any one outcome to say what it changes.`;
}

/** The sentences after the main one: how often the occasion arises, and what the decision does to the retirement date. */
function decisionAfter(overall: { value: number; withinNoise: boolean } | null, context: DecisionContext): string {
  const parts: string[] = [];
  if (context.occasion) parts.push(context.occasion);
  const shift = context.fiShift ?? null;
  if (shift !== null && Math.abs(shift) >= AGE_SHIFT_MIN) {
    const moves = `Work becomes optional ${span(shift)} ${shift > 0 ? 'later' : 'earlier'} in the middle life`;
    // Later retirement with more lives holding (or earlier with fewer) is the pattern the retirement date alone produces.
    const dateExplains = context.funded === true && overall !== null && !overall.withinNoise && overall.value !== 0 && shift > 0 === overall.value > 0;
    parts.push(
      dateExplains
        ? `${moves}. ${shift > 0 ? 'A later' : 'An earlier'} retirement by itself makes a plan hold ${shift > 0 ? 'more' : 'less'} often, so the change in whether the plan holds reflects that date as much as the shares.`
        : `${moves}.`,
    );
  }
  return parts.length ? ` ${parts.join(' ')}` : '';
}

/**
 * Under "Exercise vested options now": what the plan really exercises in the first
 * month, when that is less than every vested option. Null when it exercises them all
 * (or there is nothing to compare).
 */
export function exerciseShortfall(done: FirstExercise, vested: number, wanted: number): string | null {
  if (!(vested > 0) || !(wanted > 0)) return null;
  if (done.options >= vested - 0.5 || done.cost >= wanted * 0.995) return null;
  if (done.options <= 0) return 'The plan cannot pay for any of it: savings above the emergency fund do not cover the strike and the tax, so no option is exercised now and the comparison shows no change from it.';
  return `The plan can pay for ${count(done.options)} of the ${count(vested)} vested options: ${moneyCompact(done.cost)} of the ${moneyCompact(wanted)}, plus the tax held back, is what savings above the emergency fund cover. The comparison is for that smaller exercise. The other ${count(vested - done.options)} are not exercised later: they wait for a sale or IPO, or for the deadline.`;
}

export const DECISIONS_CAPTION =
  'Decisions mostly move money between futures: they cost in some and pay in others. The same lives are used for both sides. Net worth is the median change at the end of life, in today’s dollars, among the lives with that outcome, with the average change beneath it where that is outside sampling noise; the change in “plan holds” is in lives per 100, with its 95% interval. A decision that costs cash can also move when work becomes optional, which by itself changes how often a plan holds.';

// ---- outside the view -----------------------------------------------------------------------

/**
 * The note under the pinned success figure, so "can I retire if it goes to zero" is
 * answered on every view: "and in 89 of 100 of the lives where Northwind fails". Null
 * when no company is counted or too few lives have it fail.
 */
export function equityHeadlineNote(summary: Summary | null | undefined, counted: boolean): string | null {
  const equity = summary?.equity;
  if (!equity || !counted) return null;
  const { low } = outcomeEnds(equity.by_outcome);
  if (!low || low.success_rate.value === null) return null;
  return `and in ${per(low.success_rate.value)} of 100 of the lives where ${equity.company || 'the company'} fails`;
}

export const OVERVIEW_EXCLUDED = 'Startup equity is not counted in this plan.';

/**
 * The Overview's card: "If Northwind fails the plan holds in 89 of 100 of those lives;
 * if it is sold or listed, in 90. The equity pays out no cash in 55 of 100 lives, and in
 * 29 it costs more than it returns." A class with too few lives is left out of its
 * clause. "No cash" is the gross count; the lives that end out of pocket are named
 * beside it, because they are what a planner most needs. `primaryCounted` false (the
 * company the outcomes describe is itself left out of the plan) drops the clauses about
 * that company. Null when the plan has no company.
 */
export function equityCardSentence(summary: Summary | null | undefined, counted: boolean, primaryCounted = counted): string | null {
  const equity = summary?.equity;
  if (!equity) return null;
  if (!counted) return OVERVIEW_EXCLUDED;
  const name = equity.company || 'the company';
  const { low } = outcomeEnds(equity.by_outcome);
  const good = soldOrListed(equity.by_outcome);
  const fails = primaryCounted && low && low.success_rate.value !== null ? `If ${name} fails the plan holds in ${per(low.success_rate.value)} of 100 of those lives` : null;
  const paid = primaryCounted && good ? `${fails ? 'if' : `If ${name}`} ${fails ? 'it ' : ''}is sold or listed, ${fails ? 'in' : 'the plan holds in'} ${per(good.success)}${fails ? '' : ' of 100 of those lives'}` : null;
  const clauses = [fails, paid].filter((c): c is string => c !== null);
  const lead = clauses.length ? `${clauses.join('; ')}. ` : '';
  const of = livesOf(equity.paths);
  const loss = Math.round(Math.min(1, Math.max(0, equity.loss_rate.value)) * of);
  const out = loss >= 1 ? `, and in ${loss} it costs more than it returns` : '';
  return `${lead}The equity pays out no cash in ${per(equity.nothing_rate.value, of)} ${ofLives(of)} lives${out}.`;
}
