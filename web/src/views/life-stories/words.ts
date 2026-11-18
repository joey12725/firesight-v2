// The takeaways of the Life stories charts, computed from the life (and the crowd it is
// drawn against). Plain strings, so they also serve the image export and the assistant.
import { moneyCompact } from '@/ui';
import type { Life } from './life';

/** Net-worth quantiles of all lives, re-indexed to one life's years (null where the summary has none). */
export interface Crowd {
  p5: (number | null)[];
  p25: (number | null)[];
  p50: (number | null)[];
  p75: (number | null)[];
  p95: (number | null)[];
}

const ageWord = (life: Life) => (life.hasAges ? 'age' : 'year');
const at = (life: Life, month: number) => String(Math.floor((life.monthAges[month] ?? 0) + 1e-6));

/** Where this life sits among all the others, year by year. `ends` is the last month of each year. */
export function timelineTakeaway(life: Life, crowd: Crowd | null, ends: number[]): string {
  let peak = 0;
  life.netWorth.forEach((v, m) => {
    if (v > (life.netWorth[peak] ?? 0)) peak = m;
  });
  const lead = `Net worth peaks at ${moneyCompact(life.netWorth[peak] ?? 0)} at ${ageWord(life)} ${at(life, peak)}.`;
  if (!crowd) return lead;
  let inside = 0;
  let above = 0;
  let below = 0;
  ends.forEach((m, y) => {
    const lo = crowd.p25[y];
    const hi = crowd.p75[y];
    const v = life.netWorth[m] ?? 0;
    if (lo === null || lo === undefined || hi === null || hi === undefined) return;
    if (v < lo) below++;
    else if (v > hi) above++;
    else inside++;
  });
  const total = inside + above + below;
  if (total === 0) return lead;
  const parts = [`${inside} inside the middle half of all simulated lives`];
  if (above > 0) parts.push(`${above} above it`);
  if (below > 0) parts.push(`${below} below it`);
  const list = parts.length > 1 ? `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}` : parts[0];
  return `${lead} Of its ${total} year-ends, it spends ${list}.`;
}

export function flowTakeaway(life: Life): string {
  const income = life.annual('gross_income');
  const spending = life.annual('spending');
  const tax = life.annual('total_tax');
  // The years income falls short of spending and tax, and the longest unbroken run of them.
  let short = 0;
  let full = 0;
  let run: [number, number] | null = null;
  let best: [number, number] | null = null;
  for (let y = 0; y < life.time.length; y++) {
    if (!life.fullYear[y]) continue;
    full++;
    if ((income[y] ?? 0) < (spending[y] ?? 0) + (tax[y] ?? 0)) {
      short++;
      run = run ? [run[0], y] : [y, y];
      if (!best || run[1] - run[0] > best[1] - best[0]) best = run;
    } else run = null;
  }
  const spent = life.scalar('lifetime_spending');
  const taxed = life.scalar('lifetime_tax');
  const totals = spent !== null && taxed !== null ? `Over the whole life it spends ${moneyCompact(spent)} and pays ${moneyCompact(taxed)} in tax.` : '';
  const ageOf = (y: number) => Math.floor(life.time.ages[y] ?? 0);
  const gap =
    short === 0 || !best
      ? 'Income covers spending and tax in every full year.'
      : short === full
        ? 'Income never covers spending and tax: savings make up the difference every year.'
        : `In ${short} of its ${full} full years income does not cover spending and tax, and savings make up the difference${best[1] > best[0] ? `: the longest stretch runs from ${ageWord(life)} ${ageOf(best[0])} to ${ageOf(best[1])}` : ''}.`;
  return `${gap} ${totals}`.trim();
}

export function accountTakeaway(life: Life): string {
  let peak = 0;
  life.investable.forEach((v, m) => {
    if (v > (life.investable[peak] ?? 0)) peak = m;
  });
  const lastYear = life.time.length - 1;
  const kinds: [string, string][] = [
    ['balance_roth', 'Roth accounts'],
    ['balance_traditional', 'pre-tax accounts'],
    ['balance_taxable', 'taxable accounts'],
    ['balance_cash', 'cash'],
    ['balance_hsa', 'the HSA'],
  ];
  const end = kinds.map(([key, label]) => ({ label, v: life.annual(key)[lastYear] ?? 0 })).sort((a, b) => b.v - a.v);
  const total = end.reduce((t, k) => t + k.v, 0);
  const top = end[0];
  const lead = `Savings peak at ${moneyCompact(life.investable[peak] ?? 0)} at ${ageWord(life)} ${at(life, peak)}.`;
  if (!top || total <= 0) return `${lead} Nothing is left in the accounts at the end.`;
  return `${lead} At the end ${moneyCompact(total)} remains, ${Math.round((top.v / total) * 100)}% of it in ${top.label}.`;
}

export function marketTakeaway(life: Life): string {
  const g = life.growth[life.growth.length - 1] ?? 1;
  const p = life.prices[life.prices.length - 1] ?? 1;
  const years = Math.max(1, life.trace.months / 12);
  const yearly = (Math.pow(g, 1 / years) - 1) * 100;
  const lead = `A dollar invested at the start is worth $${g >= 10 ? g.toFixed(0) : g.toFixed(2)} after inflation at the end (${yearly.toFixed(1)}% a year), while prices rise ${p.toFixed(1)}-fold.`;
  const d = life.drawdown;
  if (!d || d.depth < 0.15) return `${lead} The investments never lose more than 15% of their real value.`;
  return `${lead} The worst fall is ${Math.round(d.depth * 100)}%, from ${ageWord(life)} ${at(life, d.peak)} to ${at(life, d.trough)}.`;
}
