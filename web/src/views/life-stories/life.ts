// From one PathTrace (a single simulated life, month by month) to what the Life stories
// view draws and says: the age axis, the curated events along it, the market this life
// lived through, a year's ledger and tax return, and the narrative. Everything is
// derived deterministically from the trace, so the same life always reads the same.
import type { PathScalar, PathTrace, Status, Summary, TraceEvent, TraceEventKind, TraceTaxReturn } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import { moneyCompact } from '@/ui';
import { createTimeAxis, type TimeAxis } from '@/ui/charts';

const monthsOf = (ym: string) => {
  const [y = 0, m = 1] = ym.split('-').map(Number);
  return y * 12 + (m - 1);
};
const MONTH = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

export interface LifeEvent {
  /** Month index into the trace. */
  month: number;
  /** Year index into the trace's annual table. */
  year: number;
  age: number;
  kind: TraceEventKind;
  /** A few words for the marker on the timeline. */
  title: string;
  /** A full sentence for the tooltip and the list. */
  detail: string;
  /** 5 = life-defining, 1 = housekeeping, 0 = a repeat that is marked but never labelled. Decides which labels survive on a crowded timeline. */
  priority: number;
  /** True when the event is trouble (drawn in the status colour for trouble). */
  trouble?: boolean;
}

export interface Drawdown {
  peak: number;
  trough: number;
  /** Fraction lost from peak to trough, after inflation. */
  depth: number;
}

export interface Life {
  trace: PathTrace;
  /** Display names of the people ("Person 1" when the plan has none). */
  names: string[];
  /** False when no birth month is known: the axis is then calendar years. */
  hasAges: boolean;
  /** The annual axis: age of the first person each December, and the calendar year. */
  time: TimeAxis;
  /** Age of the first person at the end of each month. */
  monthAges: number[];
  /** Year index of each month. */
  monthYear: number[];
  netWorth: number[];
  investable: number[];
  /** Status of each month. */
  status: Status[];
  /** Growth of one dollar held in this life's portfolio mix, after inflation, by month (returns only). */
  growth: number[];
  /** Price level by month (1 at the start). */
  prices: number[];
  drawdown: Drawdown | null;
  events: LifeEvent[];
  /** Unplanned expenses too small to mark. */
  smallShocks: { count: number; largest: number };
  /** Annual columns, as recorded (part-years are not annualised). */
  annual: (key: string) => number[];
  monthly: (key: string) => number[];
  scalar: (key: PathScalar) => number | null;
  /** Year indices whose flows cover twelve months. */
  fullYear: boolean[];
}

const zeros = (n: number) => new Array<number>(n).fill(0);

export function buildLife(trace: PathTrace, profile: Profile | null): Life {
  const people = profile?.household.people ?? [];
  const names = [0, 1].map((i) => people[i]?.name?.trim() || `Person ${i + 1}`);
  const start = monthsOf(trace.start);
  const birth = people[0]?.birth ? monthsOf(people[0].birth) : null;
  const hasAges = birth !== null;
  const years = trace.annual.years;
  // Without a birth month the "age" is the calendar year, and the axis says so.
  const ageAtMonth = (m: number) => (birth !== null ? (start + m - birth) / 12 : (start + m) / 12);
  const monthAges = Array.from({ length: trace.months }, (_, m) => ageAtMonth(m));
  const firstYear = years[0] ?? Math.floor(start / 12);
  const monthYear = Array.from({ length: trace.months }, (_, m) => Math.min(years.length - 1, Math.floor((start + m) / 12) - firstYear));
  const time = createTimeAxis({ years, ages: years.map((y) => (birth !== null ? (y * 12 + 11 - birth) / 12 : y + 11 / 12)) });

  // Charts start at the first simulated month, not at the first December.
  time.domain = [Math.min(time.domain[0], monthAges[0] ?? time.domain[0]), time.domain[1]];

  const monthly = (key: string) => trace.columns.find((c) => c.key === key)?.values.map((v) => v ?? 0) ?? zeros(trace.months);
  const annual = (key: string) => trace.annual.columns.find((c) => c.key === key)?.values.map((v) => v ?? 0) ?? zeros(years.length);
  const scalar = (key: PathScalar) => trace.scalars.find((s) => s.scalar === key)?.value ?? null;

  const returns = monthly('portfolio_return');
  const prices = monthly('price_level').map((p) => (p > 0 ? p : 1));
  const growth: number[] = [];
  let nominal = 1;
  for (let m = 0; m < trace.months; m++) {
    nominal *= 1 + (returns[m] ?? 0);
    growth.push(nominal / (prices[m] ?? 1));
  }
  let drawdown: Drawdown | null = null;
  let peak = 0;
  for (let m = 1; m < growth.length; m++) {
    if ((growth[m] as number) > (growth[peak] as number)) peak = m;
    const depth = 1 - (growth[m] as number) / (growth[peak] as number);
    if (depth > (drawdown?.depth ?? 0.05)) drawdown = { peak, trough: m, depth };
  }

  const life: Life = {
    trace,
    names,
    hasAges,
    time,
    monthAges,
    monthYear,
    netWorth: monthly('net_worth'),
    investable: monthly('investable'),
    status: trace.status.map((s) => trace.statuses[s] ?? 'working'),
    growth,
    prices,
    drawdown,
    events: [],
    smallShocks: { count: 0, largest: 0 },
    annual,
    monthly,
    scalar,
    fullYear: trace.annual.months.map((m) => m >= 12),
  };
  const curated = curateEvents(life);
  life.events = curated.events;
  life.smallShocks = curated.smallShocks;
  return life;
}

/** The fall of this life's holdings from their earlier peak, at a month (0 when at a peak). */
export function fallAt(life: Life, month: number): number {
  let peak = 0;
  for (let m = 0; m <= month && m < life.growth.length; m++) peak = Math.max(peak, life.growth[m] as number);
  return peak > 0 ? 1 - (life.growth[Math.min(month, life.growth.length - 1)] ?? peak) / peak : 0;
}

const wholeAge = (age: number) => String(Math.floor(age + 1e-6));

/** Replaces "Person 2" with that person's name. */
function named(text: string, names: string[]): string {
  return text.replace(/Person (\d)/g, (_, n: string) => names[Number(n) - 1] ?? `Person ${n}`);
}

/**
 * The trace's event log, edited for a reader: pairs joined (laid off and rehired),
 * yearly repeats folded into one entry (Roth conversions, spending-rule changes), small
 * unplanned expenses counted instead of marked, and each given a priority.
 */
function curateEvents(life: Life): { events: LifeEvent[]; smallShocks: { count: number; largest: number } } {
  const { trace, names } = life;
  const start = monthsOf(trace.start);
  const at = (e: TraceEvent) => Math.min(trace.months - 1, Math.max(0, monthsOf(e.month) - start));
  const out: LifeEvent[] = [];
  const who = life.hasAges ? 'age' : 'year';
  const push = (month: number, kind: TraceEventKind, title: string, detail: string, priority: number, trouble?: boolean) =>
    out.push({ month, year: life.monthYear[month] ?? 0, age: life.monthAges[month] ?? 0, kind, title, detail, priority, trouble });
  const ageOf = (month: number) => wholeAge(life.monthAges[month] ?? 0);
  const spending = life.annual('spending');
  const raw = trace.events;
  const used = new Set<number>();
  const smallShocks = { count: 0, largest: 0 };

  /** Folds a yearly repeat into runs (events at most `gap` months apart). */
  const runs = (kind: TraceEventKind, gap: number) => {
    const list = raw.map((e, i) => ({ e, i })).filter((x) => x.e.kind === kind);
    const groups: (typeof list)[] = [];
    for (const x of list) {
      const last = groups[groups.length - 1];
      const previous = last?.[last.length - 1];
      if (last && previous && at(x.e) - at(previous.e) <= gap) last.push(x);
      else groups.push([x]);
    }
    return groups;
  };
  const span = (group: { e: TraceEvent }[]) => {
    const a = at((group[0] as { e: TraceEvent }).e);
    const b = at((group[group.length - 1] as { e: TraceEvent }).e);
    return { a, b, text: ageOf(a) === ageOf(b) ? `at ${who} ${ageOf(a)}` : `between ${who}s ${ageOf(a)} and ${ageOf(b)}` };
  };

  for (const group of runs('roth_conversion', 14)) {
    const s = span(group);
    const total = group.reduce((t, x) => t + (x.e.amount ?? 0), 0);
    const n = new Set(group.map((x) => life.monthYear[at(x.e)])).size;
    push(s.a, 'roth_conversion', 'Roth conversions', n === 1 ? `Converts ${moneyCompact(total)} from pre-tax to Roth accounts ${s.text}.` : `Converts ${moneyCompact(total)} from pre-tax to Roth accounts over ${n} years, ${s.text}.`, 2);
  }
  const pctOf = (label: string) => Number(/([+-]?[\d.]+)%/.exec(label)?.[1] ?? 0);
  runs('spending_cut', 26).forEach((group, g) => {
    const s = span(group);
    const deepest = Math.min(...group.map((x) => pctOf(x.e.label)));
    push(s.a, 'spending_cut', 'Spending cut', `The spending rule cuts spending ${group.length === 1 ? 'once' : `${group.length} times`} ${s.text}${Number.isFinite(deepest) && deepest < 0 ? `; the largest single step is ${Math.abs(deepest).toFixed(1)}%` : ''}.`, g === 0 ? 3.5 : 0, true);
  });
  runs('spending_raise', 26).forEach((group, g) => {
    const s = span(group);
    const biggest = Math.max(...group.map((x) => pctOf(x.e.label)));
    push(s.a, 'spending_raise', 'Spending raised', `The spending rule raises spending ${group.length === 1 ? 'once' : `in ${group.length} years`} ${s.text}${Number.isFinite(biggest) && biggest > 0 ? `; the largest single step is ${biggest.toFixed(1)}%` : ''}.`, g === 0 ? 1 : 0);
  });

  raw.forEach((e, i) => {
    if (used.has(i)) return;
    const m = at(e);
    const label = named(e.label, names);
    const amount = e.amount !== null && e.amount > 0 ? moneyCompact(e.amount) : null;
    switch (e.kind) {
      case 'roth_conversion':
      case 'spending_cut':
      case 'spending_raise':
        return;
      case 'expense_shock': {
        const year = life.monthYear[m] ?? 0;
        const big = (e.amount ?? 0) >= Math.max(10_000, (spending[year] ?? 0) * 0.1);
        if (!big) {
          smallShocks.count++;
          smallShocks.largest = Math.max(smallShocks.largest, e.amount ?? 0);
          return;
        }
        return void push(m, e.kind, 'Unplanned expense', `An unplanned expense of ${amount ?? 'unknown size'} at ${who} ${ageOf(m)}.`, 2, true);
      }
      case 'health_coverage': {
        // Coverage at the start and the move off an employer's plan at retirement go without saying; Medicare is a milestone.
        if (!/medicare/i.test(e.label)) return;
        const partner = raw.findIndex((o, j) => j > i && !used.has(j) && o.kind === 'health_coverage' && /medicare/i.test(o.label) && at(o) - m <= 18);
        if (partner >= 0) used.add(partner);
        return void push(m, e.kind, 'Medicare', partner >= 0 ? `Both move to Medicare from ${who} ${ageOf(m)}.` : `${label.replace(/ health coverage: Medicare/i, '')} moves to Medicare at ${who} ${ageOf(m)}.`, 1);
      }
      case 'laid_off': {
        const job = /Laid off from (.*)/.exec(e.label)?.[1];
        const back = raw.findIndex((o, j) => j > i && !used.has(j) && o.kind === 'rehired' && (!job || o.label.includes(job)));
        const fall = fallAt(life, m);
        const during = fall >= 0.1 ? ` during a market fall of ${Math.round(fall * 100)}%` : '';
        if (back >= 0) {
          used.add(back);
          const rehired = raw[back] as TraceEvent;
          const months = Math.max(1, at(rehired) - m);
          const pay = rehired.amount ? ` at ${moneyCompact(rehired.amount)} a year` : '';
          return void push(m, e.kind, `Laid off, ${months} mo`, `Laid off${job ? ` as ${job.toLowerCase()}` : ''} at ${who} ${ageOf(m)}${during}; back at work after ${months} ${months === 1 ? 'month' : 'months'}${pay}.`, 4, true);
        }
        return void push(m, e.kind, 'Laid off', `Laid off${job ? ` as ${job.toLowerCase()}` : ''} at ${who} ${ageOf(m)}${during}, and not rehired.`, 4, true);
      }
      case 'rehired':
        return void push(m, e.kind, 'Back at work', `${label} at ${who} ${ageOf(m)}${amount ? `, at ${amount} a year` : ''}.`, 2);
      case 'financially_independent': {
        const retired = raw.findIndex((o, j) => j !== i && !used.has(j) && o.kind === 'retired' && Math.abs(at(o) - m) <= 1);
        if (retired >= 0) used.add(retired);
        const saved = amount ? ` with ${amount} invested` : '';
        return void push(m, e.kind, retired >= 0 ? 'Work optional, retires' : 'Work optional', `Savings first cover the retirement budget at ${who} ${(life.monthAges[m] ?? 0).toFixed(1)}${saved}${retired >= 0 ? ', and retirement starts that month' : ''}.`, 5);
      }
      case 'retired':
        return void push(m, e.kind, 'Retires', `Retirement starts at ${who} ${(life.monthAges[m] ?? 0).toFixed(1)}${amount ? ` with ${amount} invested` : ''}.`, 5);
      case 'bought_home': {
        const parsed = /^(.*?) \(([\d.]+)% mortgage\)$/.exec(e.label);
        return void push(m, e.kind, 'Buys a home', `“${parsed?.[1] ?? e.label}” goes ahead at ${who} ${ageOf(m)}${amount ? `: a ${amount} home` : ''}${parsed ? ` on a ${parsed[2]}% mortgage` : ''}.`, 4);
      }
      case 'sold_home':
        return void push(m, e.kind, 'Sells a home', `${label} at ${who} ${ageOf(m)}${amount ? ` for ${amount}` : ''}.`, 4);
      case 'relocated':
        return void push(m, e.kind, label.length <= 22 ? label : 'Moves', `${label} at ${who} ${ageOf(m)}.`, 4);
      case 'claimed_social_security': {
        const partner = raw.findIndex((o, j) => j > i && !used.has(j) && o.kind === 'claimed_social_security' && at(o) - m <= 12);
        if (partner >= 0) used.add(partner);
        const other = partner >= 0 ? (raw[partner] as TraceEvent) : null;
        const total = (e.amount ?? 0) + (other?.amount ?? 0);
        return void push(m, e.kind, 'Social Security', other ? `Both claim Social Security from ${who} ${ageOf(m)}: ${moneyCompact(total)} a year between them.` : `${label} at ${who} ${ageOf(m)}${amount ? `: ${amount} a year` : ''}.`, 4);
      }
      case 'rmd_started': {
        const partner = raw.findIndex((o, j) => j > i && !used.has(j) && o.kind === 'rmd_started' && at(o) - m <= 30);
        if (partner >= 0) used.add(partner);
        return void push(m, e.kind, 'Required withdrawals', `Required distributions from pre-tax accounts begin at ${who} ${ageOf(m)}${amount ? ` (${amount} in the first year)` : ''}.`, 2);
      }
      case 'long_term_care':
        return void push(m, e.kind, 'Long-term care', `${label} at ${who} ${ageOf(m)}.`, 4, true);
      case 'depleted':
        return void push(m, e.kind, 'Savings run out', `Savings are exhausted at ${who} ${ageOf(m)}: spending is cut to what income covers.`, 5, true);
      case 'recovered':
        return void push(m, e.kind, 'Essentials covered again', `${label} at ${who} ${ageOf(m)}.`, 3);
      case 'died':
        return void push(m, e.kind, label.replace(/ at \d+$/, ''), `${label}.`, 5);
      case 'windfall':
        return void push(m, e.kind, label.length <= 22 ? label : 'Windfall', `${label} at ${who} ${ageOf(m)}${amount ? `: ${amount}` : ''}.`, 3);
      case 'major_expense':
        return void push(m, e.kind, label.length <= 22 ? label : 'Major expense', `${label} at ${who} ${ageOf(m)}${amount ? `: ${amount}` : ''}.`, 3);
      case 'career_break':
        return void push(m, e.kind, label.replace(/\s*\(.*\)$/, '').length <= 24 ? label.replace(/\s*\(.*\)$/, '') : 'Career break', `${label}, from ${who} ${ageOf(m)}.`, 3);
      case 'life_event': {
        const refinance = /^Refinanced .* at ([\d.]+%)$/.exec(e.label);
        if (refinance) return void push(m, e.kind, `Refinances at ${refinance[1]}`, `The mortgage is refinanced at ${refinance[1]} at ${who} ${ageOf(m)}.`, 2);
        return void push(m, e.kind, label.length <= 24 ? label : 'Plan event', `${label} at ${who} ${ageOf(m)}.`, 3);
      }
      case 'debt_paid_off':
        return void push(m, e.kind, label.length <= 24 ? label : 'Debt paid off', `${label} at ${who} ${ageOf(m)}.`, 1);
      default:
        return void push(m, e.kind, label.length <= 24 ? label : label.slice(0, 22) + '…', `${label} at ${who} ${ageOf(m)}${amount ? `: ${amount}` : ''}.`, e.kind === 'child' || e.kind === 'partner_joined' ? 4 : 2);
    }
  });

  out.sort((a, b) => a.month - b.month || b.priority - a.priority);
  return { events: out, smallShocks };
}

// ---- the narrative --------------------------------------------------------------------

export interface Sentence {
  text: string;
  /** Year index the sentence is about, for opening that year's ledger. */
  year?: number;
}

const pct = (v: number, digits = 0) => `${(v * 100).toFixed(digits)}%`;

/** The life in plain sentences, in order. Deterministic: the same trace gives the same words. */
export function narrative(life: Life, summary: Summary | null): Sentence[] {
  const out: Sentence[] = [];
  const { trace, names } = life;
  const s = life.scalar;
  const single = s('death_age_1') === null;
  const start = monthsOf(trace.start);
  const they = single ? names[0] : 'they';
  const ages = life.monthAges;
  const firstAge = ages[0] ?? 0;
  if (life.hasAges) {
    out.push({ text: `This life starts in ${MONTH[start % 12]} ${Math.floor(start / 12)}, when ${names[0]} is ${wholeAge(firstAge)}.`, year: 0 });
  }

  const byKind = (kind: TraceEventKind) => life.events.filter((e) => e.kind === kind);
  const tell = (kinds: TraceEventKind[], before: number) => life.events.filter((e) => kinds.includes(e.kind) && e.month < before).forEach((e) => out.push({ text: e.detail, year: e.year }));

  const fi = byKind('financially_independent')[0];
  const retired = byKind('retired')[0] ?? (fi?.title.includes('retires') ? fi : undefined);
  const turning = retired?.month ?? fi?.month ?? trace.months;
  // Working years: the things that happened on the way.
  tell(['career_break', 'bought_home', 'sold_home', 'relocated', 'laid_off', 'windfall', 'major_expense', 'child', 'partner_joined', 'equity_event'], turning);

  const fiAge = s('fi_age');
  if (fi && fiAge !== null) {
    let rank = '';
    if (summary) {
      const i = summary.fi.ages.indexOf(Math.floor(fiAge));
      const before = i > 0 ? (summary.fi.cumulative[i - 1] ?? 0) : 0;
      const later = Math.round((1 - (summary.fi.cumulative[i] ?? before)) * 100);
      if (i >= 0) rank = before < 0.5 ? `, sooner than in ${Math.max(1, Math.min(99, later))} of 100 simulated lives` : `, later than in ${Math.max(1, Math.min(99, Math.round(before * 100)))} of 100 simulated lives`;
    }
    const invested = s('investable_at_retirement');
    out.push({ text: `Work becomes optional at ${fiAge.toFixed(1)}${invested !== null && retired && Math.abs(retired.month - fi.month) <= 1 ? ` with ${moneyCompact(invested)} invested, and ${they} retire${single ? 's' : ''} that month` : ''}${rank}.`, year: fi.year });
    if (retired && Math.abs(retired.month - fi.month) > 1) out.push({ text: retired.detail, year: retired.year });
  } else if (retired) {
    out.push({ text: retired.detail, year: retired.year });
    out.push({ text: 'Savings never reach the level the plan counts as enough to stop working.' });
  } else {
    out.push({ text: 'Work never becomes optional in this life, and retirement never starts.' });
  }

  const early = s('early_retirement_return');
  if (early !== null && retired) {
    const expected = early >= 0.05 ? 'a kind start' : early >= 0.02 ? 'an ordinary start' : early >= 0 ? 'a thin start' : 'a damaging start';
    out.push({ text: `The first ten years of retirement return ${pct(early, 1)} a year after inflation: ${expected}.`, year: retired.year });
  }
  if (life.drawdown && life.drawdown.depth >= 0.15) {
    const d = life.drawdown;
    out.push({ text: `At their worst the investments lose ${pct(d.depth)} of their real value, from a peak at ${wholeAge(ages[d.peak] ?? 0)} to a low at ${wholeAge(ages[d.trough] ?? 0)}.`, year: life.monthYear[d.trough] });
  }

  // Retirement: what happened after.
  life.events
    .filter((e) => e.month >= turning && ['laid_off', 'windfall', 'major_expense', 'sold_home', 'bought_home', 'relocated', 'roth_conversion', 'claimed_social_security', 'rmd_started', 'long_term_care', 'depleted', 'recovered', 'expense_shock'].includes(e.kind))
    .forEach((e) => out.push({ text: e.detail, year: e.year }));

  // The spending rule, in one sentence rather than one per adjustment.
  const cuts = trace.events.filter((e) => e.kind === 'spending_cut');
  const raises = trace.events.filter((e) => e.kind === 'spending_raise');
  if (cuts.length + raises.length > 0) {
    const firstCut = byKind('spending_cut')[0];
    const times = (n: number) => (n === 1 ? 'once' : `${n} times`);
    const text =
      cuts.length && raises.length
        ? `The spending rule steps in often: it cuts spending ${times(cuts.length)}, first at ${wholeAge(firstCut?.age ?? 0)}, and raises it ${times(raises.length)}.`
        : cuts.length
          ? `The spending rule cuts spending ${times(cuts.length)}, first at ${wholeAge(firstCut?.age ?? 0)}, and never raises it.`
          : `The spending rule never has to cut spending, and raises it ${times(raises.length)}.`;
    out.push({ text, year: (firstCut ?? byKind('spending_raise')[0])?.year });
  }

  const delivered = s('delivered_spending_ratio');
  const worst = s('worst_spending_ratio');
  if (delivered !== null && retired) {
    const lean = worst !== null && worst < 0.97 ? `; the leanest year delivers ${pct(worst)}` : '';
    out.push({ text: `Over the whole retirement, spending comes to ${pct(delivered)} of what the plan intended${lean}.` });
  }
  const care = s('long_term_care_cost');
  if (care !== null && care > 0) out.push({ text: `Long-term care costs ${moneyCompact(care)} in all.` });
  const short = s('years_depleted');
  if (short !== null && short > 0) out.push({ text: `For ${Math.round(short)} ${Math.round(short) === 1 ? 'year' : 'years'} the household cannot cover its essential spending.` });
  if (life.smallShocks.count > 0) out.push({ text: `Along the way come ${life.smallShocks.count} smaller unplanned ${life.smallShocks.count === 1 ? 'expense' : 'expenses'}, the largest ${moneyCompact(life.smallShocks.largest)}.` });

  const deaths = byKind('died');
  const terminal = s('terminal_net_worth');
  const last = deaths[deaths.length - 1];
  const endAge = ages[ages.length - 1] ?? 0;
  const leaving = terminal !== null ? `, leaving ${moneyCompact(terminal)}` : '';
  // The trace stops with the last death, which the event log does not record: a life that
  // ends before the plan's horizon ended because its last member died.
  const horizon = summary?.axis.years[summary.axis.years.length - 1];
  const endedEarly = horizon !== undefined && (life.time.years[life.time.length - 1] ?? horizon) < horizon;
  const earlier = deaths.filter((d) => d.month < trace.months - 2);
  earlier.forEach((d) => out.push({ text: d.detail, year: d.year }));
  if (last && last.month >= trace.months - 2) {
    out.push({ text: `${last.detail.replace(/\.$/, '')}${leaving}.`, year: last.year });
  } else if (endedEarly && life.hasAges) {
    const survivor = single ? names[0] : (names.find((name) => !earlier.some((d) => d.detail.startsWith(name))) ?? 'The survivor');
    out.push({ text: `${survivor} dies at ${wholeAge(single || survivor === names[0] ? endAge : (s('death_age_1') ?? endAge))}${leaving}.`, year: life.time.length - 1 });
  } else {
    out.push({ text: `The simulation ends at ${life.hasAges ? `age ${wholeAge(endAge)}` : String(Math.floor(endAge))}${terminal !== null ? ` with ${moneyCompact(terminal)}` : ''}, with ${single ? names[0] : 'someone'} still alive.`, year: life.time.length - 1 });
  }
  return out;
}

/** One line for the head of the page: the shape of this life. */
export function headline(life: Life): string {
  const s = life.scalar;
  const fi = s('fi_age');
  const depleted = s('depletion_age');
  const terminal = s('terminal_net_worth');
  const end = life.monthAges[life.monthAges.length - 1] ?? 0;
  const parts = [fi !== null ? `work optional at ${fi.toFixed(1)}` : 'work never optional'];
  if (depleted !== null) parts.push(`short of money from ${wholeAge(depleted)}`);
  parts.push(`${life.hasAges ? `ends at ${wholeAge(end)}` : 'ends'}${terminal !== null ? ` with ${moneyCompact(terminal)}` : ''}`);
  const text = parts.join(', ');
  return text.charAt(0).toUpperCase() + text.slice(1);
}

// ---- a year's ledger --------------------------------------------------------------------

export const LEDGER_LABEL: Record<string, string> = {
  income_wages: 'Wages',
  income_self_employment: 'Self-employment',
  income_social_security: 'Social Security',
  income_pensions: 'Pensions and annuities',
  income_rental: 'Rental income',
  income_other: 'Other income',
  withdraw_cash: 'From cash',
  withdraw_taxable: 'From taxable accounts',
  withdraw_traditional: 'From pre-tax accounts',
  withdraw_roth: 'From Roth accounts',
  withdraw_hsa: 'From the HSA',
  property_sale: 'Property sale proceeds',
  save_employer: 'Employer contributions',
  tax_federal: 'Federal income tax',
  tax_state: 'State income tax',
  tax_payroll: 'Payroll tax',
  tax_penalties: 'Penalties',
  spend_essential: 'Essentials',
  spend_discretionary: 'Discretionary',
  spend_housing: 'Housing (owned homes)',
  spend_healthcare: 'Healthcare',
  spend_long_term_care: 'Long-term care',
  spend_children: 'Children',
  spend_debt: 'Debt payments',
  spend_one_off: 'One-off costs',
  save_cash: 'To cash',
  save_taxable: 'To taxable accounts',
  save_traditional: 'To pre-tax accounts',
  save_roth: 'To Roth accounts',
  save_hsa: 'To the HSA',
  property_purchase: 'Property purchase',
  roth_conversion: 'Roth conversion (pre-tax to Roth)',
  investment_return: 'Investment return, after inflation',
  investment_income: 'of which interest and dividends',
  spend_planned: 'Spending the plan intended',
  balance_cash: 'Cash',
  balance_taxable: 'Taxable accounts',
  balance_traditional: 'Pre-tax accounts',
  balance_roth: 'Roth accounts',
  balance_hsa: 'HSA',
  property_value: 'Property (market value)',
  mortgage: 'Mortgages',
  debt: 'Other debt',
  net_worth: 'Net worth',
  investable: 'Investable assets',
};

export interface LedgerLine {
  key: string;
  label: string;
  amount: number;
}

export interface LedgerSection {
  title: string;
  lines: LedgerLine[];
  total?: number;
}

export interface Ledger {
  year: number;
  age: number;
  /** Months of the calendar year this record covers. */
  months: number;
  status: Status;
  moneyIn: LedgerSection[];
  moneyOut: LedgerSection[];
  totalIn: number;
  totalOut: number;
  memo: LedgerLine[];
  balances: LedgerLine[];
}

const IN: [string, string[]][] = [
  ['Income', ['income_wages', 'income_self_employment', 'income_social_security', 'income_pensions', 'income_rental', 'income_other', 'save_employer', 'property_sale']],
  ['Withdrawals', ['withdraw_taxable', 'withdraw_traditional', 'withdraw_roth', 'withdraw_hsa', 'withdraw_cash']],
];
const OUT: [string, string[]][] = [
  ['Spending', ['spend_essential', 'spend_housing', 'spend_children', 'spend_healthcare', 'spend_long_term_care', 'spend_discretionary', 'spend_debt', 'spend_one_off', 'property_purchase']],
  ['Taxes', ['tax_federal', 'tax_state', 'tax_payroll', 'tax_penalties']],
  ['Saving', ['save_taxable', 'save_traditional', 'save_roth', 'save_hsa', 'save_employer', 'save_cash']],
];

/** Every line the trace recorded for one year, grouped as money in and money out. The two totals are equal by construction. */
export function yearLedger(life: Life, y: number): Ledger {
  const value = (key: string) => life.annual(key)[y] ?? 0;
  const section = ([title, keys]: [string, string[]]): LedgerSection => {
    const lines = keys.map((key) => ({ key, label: LEDGER_LABEL[key] ?? key, amount: value(key) })).filter((l) => l.amount !== 0);
    return { title, lines, total: lines.reduce((t, l) => t + l.amount, 0) };
  };
  const moneyIn = IN.map(section).filter((s) => s.lines.length > 0);
  const moneyOut = OUT.map(section).filter((s) => s.lines.length > 0);
  const lines = (keys: string[], keepZero = false) => keys.map((key) => ({ key, label: LEDGER_LABEL[key] ?? key, amount: value(key) })).filter((l) => keepZero || l.amount !== 0);
  return {
    year: life.time.years[y] ?? 0,
    age: life.time.ages[y] ?? 0,
    months: life.trace.annual.months[y] ?? 12,
    status: life.trace.statuses[life.trace.annual.status[y] ?? 0] ?? 'working',
    moneyIn,
    moneyOut,
    totalIn: moneyIn.reduce((t, s) => t + (s.total ?? 0), 0),
    totalOut: moneyOut.reduce((t, s) => t + (s.total ?? 0), 0),
    memo: lines(['spend_planned', 'roth_conversion', 'investment_return', 'investment_income']),
    balances: [...lines(['balance_cash', 'balance_taxable', 'balance_traditional', 'balance_roth', 'balance_hsa', 'property_value']), ...lines(['mortgage', 'debt']).map((l) => ({ ...l, amount: -l.amount })), ...lines(['net_worth'], true)],
  };
}

export interface ReturnLine {
  key: string;
  label: string;
  note: string | null;
  /** As filed, in the dollars of the tax year. */
  nominal: number;
  /** The same amount in today's dollars. */
  real: number;
  /** A subtotal or total: set in stronger type. */
  strong: boolean;
}

const STRONG = new Set(['agi', 'taxable_income', 'federal_income_tax', 'total', 'income']);

/** A traced tax return as readable lines, with each amount also in today's dollars. */
export function taxReturn(life: Life, y: number): { year: number; priceLevel: number; marginal: number | null; lines: ReturnLine[] } | null {
  const year = life.time.years[y];
  const r: TraceTaxReturn | undefined = life.trace.tax_returns.find((t) => t.year === year);
  if (!r) return null;
  const level = r.price_level > 0 ? r.price_level : 1;
  const marginal = typeof r.result.marginal_ordinary_rate === 'number' ? r.result.marginal_ordinary_rate : null;
  return {
    year: r.year,
    priceLevel: level,
    marginal,
    lines: r.lines.map((l) => ({ key: l.key, label: named(l.label, life.names).replace(/, person (\d)/, (_, n: string) => `, ${life.names[Number(n) - 1] ?? `person ${n}`}`), note: l.note, nominal: l.amount, real: l.amount / level, strong: STRONG.has(l.key) })),
  };
}

// ---- which lives to offer -------------------------------------------------------------

export interface LifeChoice {
  key: string;
  pathIndex: number;
  why: 'p10' | 'median' | 'p90' | 'median_fi' | 'failing' | 'scatter';
  title: string;
  /** The stated rule by which this life was picked. */
  rule: string;
  /** What happens in it, from its scalars. */
  describes: string;
}

/** The representative lives: the Summary's picks, plus the earliest and latest to reach independence among the scatter sample. */
export function lifeChoices(summary: Summary): LifeChoice[] {
  const column = (scalar: PathScalar) => summary.scatter.columns.find((c) => c.scalar === scalar)?.values ?? [];
  const fi = column('fi_age');
  const depleted = column('depletion_age');
  const terminal = column('terminal_net_worth');
  const death = column('death_age_0');
  const describe = (path: number) => {
    const row = summary.scatter.paths.indexOf(path);
    if (row < 0) return '';
    const f = fi[row];
    const d = depleted[row];
    const t = terminal[row];
    const parts = [f === null || f === undefined ? 'Work never optional' : `Work optional at ${Math.floor(f)}`];
    if (d !== null && d !== undefined) parts.push(`short of money from ${Math.floor(d)}`);
    if (t !== null && t !== undefined) parts.push(`ends with ${moneyCompact(t)}`);
    return parts.join(', ') + '.';
  };
  const pick = (label: string) => summary.picks.find((p) => p.label === label);
  const out: LifeChoice[] = [];
  const add = (key: string, why: LifeChoice['why'], path: number | undefined, title: string, rule: string) => {
    if (path === undefined || out.some((c) => c.pathIndex === path)) return;
    out.push({ key, why, pathIndex: path, title, rule, describes: describe(path) });
  };
  add('p10', 'p10', pick('p10')?.path_index, 'An unlucky life', 'Ranked 10th of 100 by wealth at the end');
  add('median', 'median', pick('median')?.path_index, 'A middling life', 'Ranked 50th of 100 by wealth at the end');
  add('p90', 'p90', pick('p90')?.path_index, 'A lucky life', 'Ranked 90th of 100 by wealth at the end');
  add('median_fi', 'median_fi', pick('median_fi')?.path_index, 'A typical path to independence', 'Work becomes optional at the median age, among lives where it does');
  if (summary.failure.depleted > 0 || summary.failure.failed > 0) add('failing', 'failing', pick('failing')?.path_index, 'A life that ran short', 'The typical age of running short, among lives that do');

  let earliest = -1;
  let latest = -1;
  fi.forEach((v, i) => {
    if (v === null || death[i] === null) return;
    if (earliest < 0 || v < (fi[earliest] as number)) earliest = i;
    if (latest < 0 || v > (fi[latest] as number)) latest = i;
  });
  const sample = summary.scatter.paths.length;
  const among = `among ${sample.toLocaleString('en-US')} lives`;
  if (earliest >= 0) add('earliest', 'scatter', summary.scatter.paths[earliest], 'The earliest to stop needing work', `The lowest age at which work became optional, ${among}`);
  if (latest >= 0 && latest !== earliest) add('latest', 'scatter', summary.scatter.paths[latest], 'The latest to stop needing work', `The highest age at which work became optional, ${among}`);
  return out;
}
