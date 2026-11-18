// The words on the Overview, computed from the Summary. Kept apart from the components
// so they can be tested, and reused as plain text for image exports and the assistant.
import type { FailureCause, Status, Summary } from '@/engine/output-types';
import { moneyCompact } from '@/ui';
import { age, indexAtAge, ofHundred, quantileSeries, statusShares } from '@/ui/charts';
import type { FanSeries } from './NetWorthFan';

export interface Segment {
  text: string;
  /** A figure: set in stronger type. */
  strong?: boolean;
}

export const plain = (segments: Segment[]) => segments.map((s) => s.text).join('');

/** A share as a count of lives: "83 of 100", or of the lives actually simulated when there are fewer than 100. */
export function lives(share: number, paths: number): { n: string; of: string } {
  if (paths < 100) return { n: String(Math.round(share * paths)), of: String(paths) };
  const f = ofHundred(share);
  return { n: f.n, of: f.of };
}

/** "In 83 of 100 simulated lives the plan holds to the end. In 17 you run short, typically around age 93." */
export function outcomeSentence(summary: Summary): Segment[] {
  const h = summary.headline;
  const paths = summary.paths;
  const success = h.success_rate.value;
  const out: Segment[] = [];
  if (success >= 1) {
    out.push({ text: 'In ' }, { text: `all ${paths < 100 ? paths : '100 of 100'}`, strong: true }, { text: ' simulated lives the plan holds to the end.' });
    return out;
  }
  const ok = lives(success, paths);
  out.push({ text: 'In ' }, { text: `${ok.n} of ${ok.of}`, strong: true }, { text: ' simulated lives the plan holds to the end.' });

  const depleted = h.depletion_rate.value;
  if (depleted > 0) {
    const short = lives(depleted, paths);
    const around = h.depletion_age.p50;
    out.push({ text: ' In ' }, { text: short.of === ok.of ? short.n : `${short.n} of ${short.of}`, strong: true }, { text: ' you run short of money' });
    if (around !== null) out.push({ text: ', typically around age ' }, { text: String(Math.round(around)), strong: true });
    out.push({ text: '.' });
  }
  // Lives that fail without running out: still working at the latest age the plan allows, or short of the bequest.
  const other = Math.max(0, 1 - success - depleted);
  if (other >= 0.005) {
    const n = lives(other, paths);
    const neverRetired = summary.failure.never_retired > 0;
    out.push({ text: ' In ' }, { text: n.of === ok.of ? n.n : `${n.n} of ${n.of}`, strong: true }, { text: neverRetired ? ' the money lasts but work never becomes optional in time.' : ' the money lasts but the plan’s other goals are missed.' });
  }
  return out;
}

/** The sampling interval and sample size, as the small print under the sentence. */
export function samplingNote(summary: Summary): string {
  const s = summary.headline.success_rate;
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  return `${summary.paths.toLocaleString('en-US')} lives simulated. The plan holds in ${pct(s.lo)} to ${pct(s.hi)} of them (95% interval for sampling error alone; the choice of market model moves it more).`;
}

export function fiTakeaway(summary: Summary): string {
  const { p10, p50, p90 } = summary.headline.fi_age;
  const rate = summary.headline.fi_rate.value;
  if (p50 === null) {
    return rate <= 0 ? 'Work does not become optional in any simulated life.' : `Work becomes optional in only ${ofHundred(rate).text} lives, so there is no typical age.`;
  }
  const spread = p10 !== null && p90 !== null ? `, and between ${age(p10)} and ${age(p90)} in 80 of 100` : '';
  const never = summary.fi.never >= 0.005 ? ` In ${ofHundred(summary.fi.never).text} it never does.` : '';
  return `Work becomes optional at ${age(p50)} in the middle life${spread}.${never}`;
}

export function fanTakeaway(summary: Summary, series: FanSeries): string {
  const name = series === 'net_worth' ? 'net worth' : 'investable assets';
  const p50 = quantileSeries(summary, series, 0.5);
  const p5 = quantileSeries(summary, series, 0.05);
  const p95 = quantileSeries(summary, series, 0.95);
  const ages = summary.axis.ages;
  const last = ages.length - 1;
  // Read it at 85 when the plan gets that far (most lives still going), else near the end.
  const at = (ages[last] ?? 0) >= 86 ? indexAtAge(summary, 85) : Math.max(0, last - 2);
  const m = p50[at];
  const lo = p5[at];
  const hi = p95[at];
  if (m === null || m === undefined || lo === null || lo === undefined || hi === null || hi === undefined) return `The range of ${name} by age.`;
  const fi = summary.headline.median_fi_age;
  const atFi = fi === null ? null : p50[indexAtAge(summary, fi)];
  const lead = atFi !== null && atFi !== undefined && fi !== null ? `Median ${name} is ${moneyCompact(atFi)} when work becomes optional and ${moneyCompact(m)} at ${Math.floor(ages[at] as number)}` : `Median ${name} at ${Math.floor(ages[at] as number)} is ${moneyCompact(m)}`;
  return `${lead}, but 1 life in 20 has less than ${moneyCompact(lo)} by then and 1 in 20 more than ${moneyCompact(hi)}.`;
}

export function statusTakeaway(summary: Summary): string {
  const ages = summary.axis.ages;
  const last = ages.length - 1;
  const at = (ages[last] ?? 0) >= 82 ? indexAtAge(summary, 80) : Math.max(0, last - 5);
  const share = (s: Status) => statusShares(summary, s)[at] ?? 0;
  const fine = share('retired_on_plan') + share('retired_surplus');
  const trouble = share('retired_cutting') + share('depleted');
  const dead = share('dead');
  const working = share('working');
  const parts = [`${ofHundred(fine).n} are retired and on plan or better`, `${ofHundred(trouble).n} are cutting back or short`];
  if (working >= 0.005) parts.push(`${ofHundred(working).n} are still working`);
  parts.push(`${ofHundred(dead).n} have died`);
  const worst = Math.max(0, ...statusShares(summary, 'depleted'));
  const worstAge = ages[statusShares(summary, 'depleted').indexOf(worst)];
  const tail = worst >= 0.005 && worstAge !== undefined ? ` Being short of essentials peaks at ${ofHundred(worst).text} lives around ${Math.floor(worstAge)}.` : '';
  return `At ${Math.floor(ages[at] as number)}, of 100 lives: ${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}.${tail}`;
}

export function spendingTakeaway(summary: Summary): string {
  const sp = summary.spending;
  const below = sp.paths_below_90.value;
  const years = sp.years_cut.p50;
  const ratio = summary.headline.delivered_spending_ratio.value;
  if (sp.retired_paths === 0 || below === null) return 'No simulated life reaches retirement, so there is no retirement spending to compare with the plan.';
  const cut =
    below < 0.005
      ? 'Spending never falls more than 10% below the plan in any retired life'
      : `In ${ofHundred(below).text} retired lives spending falls more than 10% below the plan at some point${years !== null ? `, typically for ${Math.round(years)} ${Math.round(years) === 1 ? 'year' : 'years'}` : ''}`;
  const worst = sp.worst_cut[sp.worst_cut_levels.indexOf(0.9)];
  const deep = worst !== null && worst !== undefined && worst > 0.1 ? `; the worst year is a cut of ${Math.round(worst * 100)}% or less in 9 of 10` : '';
  const average = ratio === null ? '' : ratio >= 1.03 ? ` Averaged over retirement, lives spend ${Math.round((ratio - 1) * 100)}% more than planned, because the spending rule raises spending when markets are kind.` : ratio <= 0.97 ? ` Averaged over retirement, lives spend ${Math.round((1 - ratio) * 100)}% less than planned.` : ' Averaged over retirement, spending matches the plan.';
  return `${cut}${deep}.${average}`;
}

export interface CauseText {
  label: string;
  /** The engine's rule for attributing a failed life to this cause, in plain words. */
  rule: string;
  /** The view that helps with it. */
  view: string;
  action: string;
}

export const CAUSES: Record<FailureCause, CauseText> = {
  health: { label: 'Long-term care', rule: 'Care costs before the money ran out came to at least a quarter of the savings held at retirement.', view: 'assumptions', action: 'Check the care assumptions' },
  longevity: { label: 'A long life', rule: 'The money ran out at 90 or later, with nothing else to blame.', view: 'outcomes', action: 'See how late and how deep' },
  market_sequence: { label: 'Poor returns early in retirement', rule: 'Returns over the first ten retirement years ran more than two points a year below expectations.', view: 'what-matters', action: 'See what moves the result' },
  job_loss: { label: 'Long unemployment', rule: 'At least 18 months out of work, or a year of it before the money ran out.', view: 'life-stories', action: 'Read one of these lives' },
  inflation: { label: 'High inflation', rule: 'Inflation averaged 4.5% a year or more over the life.', view: 'assumptions', action: 'Check the inflation assumptions' },
  overspending: { label: 'Spending too much, too soon', rule: 'The first-year withdrawal rate was above 5%, or the spending rule had raised spending above the plan.', view: 'cash-flow', action: 'See the cash flow' },
  other: { label: 'Something else', rule: 'None of the rules above applied.', view: 'life-stories', action: 'Read one of these lives' },
};

export function failureTakeaway(summary: Summary): string {
  const f = summary.failure;
  if (f.failed === 0) return 'No simulated life falls short of the plan.';
  const ranked = [...f.causes].filter((c) => c.count > 0).sort((a, b) => b.count - a.count);
  const top = ranked[0];
  if (!top) return 'Some lives fall short of the plan.';
  const share = top.share_of_failed ?? 0;
  const second = ranked[1];
  const also = second && (second.share_of_failed ?? 0) >= 0.15 ? `, then ${CAUSES[second.cause].label.toLowerCase()} (${Math.round((second.share_of_failed ?? 0) * 100)}%)` : '';
  return `When the plan falls short, the first thing to go wrong is most often ${CAUSES[top.cause].label.toLowerCase()}: ${Math.round(share * 100)}% of those lives${also}.`;
}
