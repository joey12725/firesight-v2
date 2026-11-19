// The words of the Scenarios view, computed from the engine's Comparison and Summaries.
// Kept apart from the components so they can be tested and reused as plain text.
import type { Comparison, HistSeries, ScalarComparison, Summary } from '@/engine/output-types';
import { count, moneyCompact } from '@/ui';
import { indexAtAge, ofHundred, quantileSeries } from '@/ui/charts';
import type { Segment } from '../overview/text';
import { flipCounts, formatDelta, type Flips, type MetricDef, type RankRow, type StageDiff, deltaQuantile, homeSaleOf } from './model';

export type { Segment };
export const plain = (segments: Segment[]) => segments.map((s) => s.text).join('');

const of = (n: number) => (n === 100 ? '100' : count(n));

/**
 * "In 9 of 100 lives this change is the difference between falling short and not; in
 * 1 of 100 it makes things worse; in 90 it changes nothing about whether the plan holds."
 */
export function flipSentence(f: Flips): Segment[] {
  const c = flipCounts(f);
  const d = of(c.of);
  if (f.rescued === 0 && f.harmed === 0) {
    return [{ text: 'This change does not alter whether the plan holds in ' }, { text: `any of ${count(f.paths)}`, strong: true }, { text: ' simulated lives. It still changes how those lives go: see the differences below.' }];
  }
  // Lead with whichever the change does more of.
  const helps = f.rescued >= f.harmed;
  const [lead, other, otherLives] = helps ? [c.rescued, c.harmed, f.harmed] : [c.harmed, c.rescued, f.rescued];
  const out: Segment[] = [
    { text: 'In ' },
    { text: `${count(lead)} of ${d}`, strong: true },
    { text: helps ? ' lives this change is the difference between the plan falling short and holding' : ' lives this change is the difference between the plan holding and falling short' },
  ];
  if (otherLives === 0) out.push({ text: helps ? '; it harms none' : '; it rescues none' });
  else if (other >= 1) out.push({ text: '; in ' }, { text: count(other), strong: true }, { text: helps ? ' it is the other way round, and the change makes things worse' : ' it is the other way round, and the change rescues the plan' });
  else out.push({ text: '; it is the other way round in only ' }, { text: `${count(otherLives)} of ${count(f.paths)}`, strong: true });
  out.push({ text: '. In ' }, { text: count(c.unchanged), strong: true }, { text: ' it changes nothing about whether the plan holds.' });
  return out;
}

/** The net effect with its paired interval, as the small print under the sentence. */
export function flipNote(f: Flips, scenarioName: string): string {
  const pts = (v: number) => `${v > 0 ? '+' : v < 0 ? '−' : ''}${Math.abs(v * 100).toFixed(1)}`;
  const net = f.net.value;
  const verdict = f.withinNoise
    ? 'The interval includes zero, so with this many lives the net effect cannot be told from no effect.'
    : `The interval excludes zero: the net effect is real for this model, ${net > 0 ? 'in favour of the change' : 'against the change'}.`;
  return `Net: the plan holds in ${pts(net)} of 100 lives with “${scenarioName}” (95% interval ${pts(f.net.lo)} to ${pts(f.net.hi)}; standard error ${(f.net.std_error * 100).toFixed(2)}). Both plans lived through the same ${count(f.paths)} simulated futures, so the difference is the decision and not market luck. ${verdict}`;
}

/**
 * The qualifier beside the flip sentence: holding counts a last-resort home sale, so how
 * often that happens under each plan is stated with it. Null when neither plan sells.
 */
export function homeSaleLine(c: Comparison | null, base: Summary, scenario: Summary): string | null {
  const paired = c ? homeSaleOf(c) : null;
  const a = paired?.a ?? base.headline.home_sale_rate.value;
  const b = paired?.b ?? scenario.headline.home_sale_rate.value;
  if (a < 0.005 && b < 0.005) return null;
  const [fa, fb] = [ofHundred(a), ofHundred(b)];
  const same = fa.text === fb.text;
  const verdict = !paired ? '' : paired.withinNoise ? ' (a difference within noise)' : paired.difference.value < 0 ? ' (fewer, beyond noise)' : ' (more, beyond noise)';
  return same
    ? `Holding includes selling a home as a last resort: ${fa.text} lives do under both plans.`
    : `Holding includes selling a home as a last resort: ${fb.text} lives do with this change, against ${fa.of === fb.of ? fa.n : fa.text} without it${verdict}.`;
}

/** Before the life-by-life pairing arrives: the two headline rates side by side, clearly provisional. */
export function provisionalSentence(base: Summary, scenario: Summary): Segment[] {
  const a = ofHundred(base.headline.success_rate.value);
  const b = ofHundred(scenario.headline.success_rate.value);
  return [
    { text: 'So far the plan holds in ' },
    { text: b.text, strong: true },
    { text: ' lives with this change and ' },
    { text: a.of === b.of ? a.n : a.text, strong: true },
    { text: ' without it. Which lives those are, and how sure the difference is, arrives when both runs finish.' },
  ];
}

const SCALAR_NOUN: Partial<Record<ScalarComparison['scalar'], { noun: string; more: string; less: string }>> = {
  terminal_net_worth: { noun: 'net worth at the end of life', more: 'higher', less: 'lower' },
  fi_age: { noun: 'the age work becomes optional', more: 'later', less: 'earlier' },
  retirement_age: { noun: 'the retirement age', more: 'later', less: 'earlier' },
  lifetime_spending: { noun: 'lifetime spending', more: 'higher', less: 'lower' },
  lifetime_tax: { noun: 'lifetime tax', more: 'higher', less: 'lower' },
};

export const scalarWords = (scalar: ScalarComparison['scalar']) => SCALAR_NOUN[scalar] ?? { noun: scalar.replace(/_/g, ' '), more: 'higher', less: 'lower' };

function amount(v: number, s: ScalarComparison): string {
  if (s.unit === 'dollars') return moneyCompact(Math.abs(v));
  const years = Math.abs(v);
  return years < 1 ? `${Math.round(years * 12)} ${Math.round(years * 12) === 1 ? 'month' : 'months'}` : `${years.toFixed(1)} years`;
}

const andList = (parts: string[]) => (parts.length <= 1 ? (parts[0] ?? '') : `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`);

/** "With the change, net worth at the end of life is higher in 71 of 100 lives, lower in 22 and unchanged in 7. In the middle life it is $310k higher." */
export function deltaTakeaway(s: ScalarComparison, levels: readonly number[]): string {
  const w = scalarWords(s.scalar);
  if (s.both === 0 || s.b_higher === null || s.a_higher === null || s.equal === null) {
    return `No simulated life has a value for ${w.noun} under both plans, so there is nothing to pair.`;
  }
  if (s.equal >= 0.9995) return `The change leaves ${w.noun} exactly as it was in every paired life.`;
  const shares: [number, string][] = [
    [s.b_higher, w.more],
    [s.a_higher, w.less],
    [s.equal, 'unchanged'],
  ];
  let denominator: string | null = null;
  const parts = shares
    .filter(([share]) => share >= 0.0005)
    .map(([share, word]) => {
      const f = ofHundred(share);
      const text = denominator === f.of ? f.n : f.text;
      denominator = f.of;
      return `${word} in ${text}`;
    });
  const median = deltaQuantile(levels, s, 0.5);
  const moved = median !== null && Math.abs(median) > (s.unit === 'dollars' ? 500 : 0.04);
  const typical = moved ? ` In the middle life it is ${amount(median, s)} ${median > 0 ? w.more : w.less}.` : s.equal >= 0.5 ? '' : ' In the middle life it barely moves.';
  const oneSided = s.a_only + s.b_only;
  const censored = oneSided > 0 ? ` ${count(oneSided)} ${oneSided === 1 ? 'life has' : 'lives have'} a value under one plan only and ${oneSided === 1 ? 'is' : 'are'} left out.` : '';
  const text = `With the change, ${w.noun} is ${andList(parts)} lives.${typical}${censored}`;
  return text;
}

const SERIES_NOUN: Partial<Record<HistSeries, string>> = { net_worth: 'net worth', spending: 'spending' };

/** Where the two medians stand at a telling age, and whether the gap is visible against the spread. */
export function fanTakeaway(base: Summary, scenario: Summary, series: HistSeries, scenarioName: string): string {
  const noun = SERIES_NOUN[series] ?? series;
  const ages = base.axis.ages;
  const last = ages.length - 1;
  const want = series === 'spending' ? 75 : 85;
  const at = (ages[last] ?? 0) >= want + 1 ? indexAtAge(base, want) : Math.max(0, last - 2);
  const a = quantileSeries(base, series, 0.5)[at];
  const b = quantileSeries(scenario, series, 0.5)[at];
  const lo = quantileSeries(base, series, 0.25)[at];
  const hi = quantileSeries(base, series, 0.75)[at];
  if (a === null || a === undefined || b === null || b === undefined) return `Median ${noun} by age under each plan.`;
  const gap = b - a;
  const spread = lo !== null && lo !== undefined && hi !== null && hi !== undefined ? hi - lo : null;
  const age = Math.floor(ages[at] as number);
  const per = series === 'spending' ? ' a year' : '';
  if (Math.abs(gap) < Math.max(500, Math.abs(a) * 0.005)) return `Median ${noun} at ${age} is ${moneyCompact(a)}${per} either way: the two plans are indistinguishable on this measure.`;
  const small = spread !== null && spread > 0 && Math.abs(gap) < spread * 0.25 ? `, a gap far smaller than the ${moneyCompact(spread)} between a typical unlucky and lucky life` : '';
  return `At ${age}, median ${noun} is ${moneyCompact(b)}${per} with “${scenarioName}” against ${moneyCompact(a)} for the plan: ${moneyCompact(Math.abs(gap))} ${gap > 0 ? 'more' : 'less'}${small}.`;
}

const STAGE_PHRASE: Record<StageDiff['status'], string> = {
  working: 'still working',
  retired_surplus: 'retired with wealth doubled',
  retired_on_plan: 'retired and on plan',
  retired_cutting: 'retired and cutting back',
  depleted: 'short of essentials',
  dead: 'dead',
};

/** The largest shift between life stages and when it happens. */
export function stageTakeaway(diffs: StageDiff[], base: Summary): string {
  if (diffs.length === 0) return 'The change moves fewer than 1 life in 200 between life stages at any age.';
  const ranked = [...diffs].sort((a, b) => Math.abs(b.peak) - Math.abs(a.peak));
  const describe = (d: StageDiff) => {
    const n = Math.abs(d.peak) * 100;
    const lives = n < 0.95 ? n.toFixed(1) : String(Math.round(n));
    return `${lives} ${d.peak > 0 ? 'more' : 'fewer'} of 100 lives are ${STAGE_PHRASE[d.status]} at ${Math.floor(base.axis.ages[d.peakIndex] ?? 0)}`;
  };
  const first = ranked[0] as StageDiff;
  const trouble = ranked.find((d) => d !== first && (d.status === 'depleted' || d.status === 'retired_cutting'));
  const second = trouble ?? ranked[1];
  return `With the change, ${describe(first)}${second ? `, and ${describe(second)}` : ''}: the largest shifts at any age.`;
}

/** A difference on a ranking measure as a clause: "the plan holds in 4 more of 100 lives". */
export function effectPhrase(v: number, metric: MetricDef): string {
  const more = v > 0;
  switch (metric.key) {
    case 'success': {
      const n = Math.abs(v) * 100;
      return `the plan holds in ${n < 0.95 ? n.toFixed(1) : Math.round(n)} ${more ? 'more' : 'fewer'} of 100 lives`;
    }
    case 'fi_age':
      return `work becomes optional ${formatDelta(v, 'age')} on average`;
    case 'terminal':
      return `${formatDelta(Math.abs(v), 'dollars').replace('+', '')} ${more ? 'more' : 'less'} at the end of life on average`;
    case 'spending':
      return `${formatDelta(Math.abs(v), 'dollars').replace('+', '')} ${more ? 'more' : 'less'} lifetime spending on average`;
  }
}

/** Which scenario leads on the metric, and how many cannot be told from the plan. */
export function rankTakeaway(rows: RankRow[], metric: MetricDef): string {
  const ready = rows.filter((r) => r.value !== null);
  if (ready.length === 0) return 'The scenarios are still running; differences appear when each finishes.';
  const real = ready.filter((r) => !r.withinNoise && r.value !== 0);
  const noise = ready.length - real.length;
  const tail = noise === 0 ? '' : ` ${noise === 1 ? 'One scenario' : `${noise} scenarios`} cannot be told from the plan on this measure: ${noise === 1 ? 'its interval includes' : 'their intervals include'} zero.`;
  if (real.length === 0) return `No scenario moves this measure by more than sampling noise.`;
  const good = (r: RankRow) => (metric.higherIsBetter ? (r.value as number) > 0 : (r.value as number) < 0);
  const best = real[0] as RankRow;
  const worst = real[real.length - 1] as RankRow;
  if (!good(best)) return `Every scenario that differs from the plan is worse on this measure. “${best.name}” costs least: ${effectPhrase(best.value as number, metric)}.${tail}`;
  const also = worst !== best && !good(worst) ? ` “${worst.name}” costs most: ${effectPhrase(worst.value as number, metric)}.` : '';
  return `“${best.name}” helps most: ${effectPhrase(best.value as number, metric)}.${also}${tail}`;
}

/** For the paired table's heading. */
export function pairedNote(c: Comparison): string {
  const unpaired = c.unpaired_a + c.unpaired_b;
  return `${count(c.paths)} lives lived twice, once under each plan${unpaired > 0 ? ` (${count(unpaired)} more could not be paired and are left out)` : ''}.`;
}
