// What the Assumptions view states and draws: the plan's settings in words, and the
// figures from the Summary, the plan preview and one traced life that show what those
// settings imply. Pure functions; the two JSON files beside this one are static tables
// copied from the repository's data at authoring time (the historical record, and the
// published 2026 federal brackets), and are labelled as such wherever they are shown.
import type { PathTrace, PlanMoments, PlanPreview, Summary } from '@/engine/output-types';
import { ofHundred } from '@/ui/charts';
import type { Profile } from '@/engine/types';
import history from './history.json';
import tax2026 from './tax-2026.json';

export const HISTORY = history;
export type HistoryAsset = (typeof history.assets)[number];

// ---- markets -----------------------------------------------------------------------

export type MarketType = Profile['assumptions']['market']['kind']['type'];

export const MARKET_NAME: Record<MarketType, string> = {
  bootstrap: 'History reshuffled',
  regime: 'Calm and turbulent regimes',
  lognormal: 'Textbook random walk',
  fat_tailed: 'Random walk with fat tails',
  historical: 'History as it happened',
};

/** What the market model in force does, in two sentences. */
export function marketStatement(profile: Profile): string {
  const kind = profile.assumptions.market.kind;
  switch (kind.type) {
    case 'bootstrap': {
      const years = kind.mean_block_months / 12;
      const block = years >= 1 ? `${Number.isInteger(years) ? years : years.toFixed(1)} ${years === 1 ? 'year' : 'years'}` : `${kind.mean_block_months} months`;
      return `Every simulated month is a real month from the US record${kind.from_year ? ` since ${kind.from_year}` : ', 1926 to 2026'}, taken in runs that average ${block}. Stocks, bonds, cash, inflation, house prices and recessions always come from the same month, so they move together as they did; only the order of the runs is new.`;
    }
    case 'regime':
      return 'Markets switch at random between a calm state and a turbulent one, fitted to the US record. Volatility clusters and crashes arrive with recessions, but nothing pulls stocks back after a long run, so this model has the widest range of long-term outcomes of the four main models.';
    case 'lognormal':
      return 'Every month is an independent draw with history’s average returns, volatilities and correlations. There are no regimes, no crashes that cluster and no multi-year inflation spells: the textbook model, kept for comparison.';
    case 'fat_tailed':
      return 'Every month is a draw with history’s average returns and volatilities, but from a distribution with fat tails: extreme months are more common, volatile spells cluster and fade over a few years, and stock markets fall together in a sell-off. Crashes are deeper and more frequent than in the textbook random walk; nothing pulls stocks back after a long run.';
    case 'historical':
      return 'Each simulated life starts in a real month between 1926 and 2026 and lives through exactly what followed. There are only 1,203 different market histories, they overlap heavily, and a life that starts late in the record runs off its end and carries on from 1926.';
  }
}

/** The engine's default for `assumptions.market.equity_premium_adjustment`: a judgement, and one that moves the headline. */
export const DEFAULT_EQUITY_ADJUSTMENT = -0.015;

export interface Adjustment {
  label: string;
  detail: string;
}

/** Departures from the model's defaults that the plan has dialled in. */
export function marketAdjustments(profile: Profile): Adjustment[] {
  const m = profile.assumptions.market;
  const out: Adjustment[] = [];
  const points = (v: number) => `${v > 0 ? '+' : '−'}${Math.abs(v * 100).toFixed(1)} points a year`;
  if (Math.abs(m.equity_premium_adjustment) > 1e-9) {
    const isDefault = Math.abs(m.equity_premium_adjustment - DEFAULT_EQUITY_ADJUSTMENT) < 1e-9;
    out.push({
      label: m.equity_premium_adjustment < 0 ? 'Stocks are planned below the record' : 'Stocks are planned above the record',
      detail: `${points(m.equity_premium_adjustment)} against the US record of 7.2% after inflation${isDefault ? ', the default' : ` (the default is ${points(DEFAULT_EQUITY_ADJUSTMENT)})`}. The record is that of the best large market of the century; the world outside the US returned about 4.5%, and estimates from today’s prices run 3 to 5%. Planning on the full record instead would make the plan hold in roughly 2 to 7 more lives in 100`,
    });
  }
  if (Math.abs(m.inflation_adjustment) > 1e-9) out.push({ label: 'Inflation shifted', detail: `${points(m.inflation_adjustment)} against history` });
  for (const s of m.stress) out.push({ label: 'Stress forced onto every life', detail: String((s as { type?: string }).type ?? 'stress').replace(/_/g, ' ') });
  return out;
}

export interface ModelRange {
  kind: string;
  label: string;
  p5: number;
  p25: number;
  p50: number;
  p75: number;
  p95: number;
  current: boolean;
  record: boolean;
  /** Points a year added to the calibration figures for this row: the plan's own stock-return adjustment. 0 for the others. */
  shift: number;
}

export function modelRanges(profile: Profile): ModelRange[] {
  const current = profile.assumptions.market.kind.type;
  const adjustment = profile.assumptions.market.equity_premium_adjustment;
  return history.models_30y_real_us_equity.rows.map((r) => {
    // The calibration figures carry no adjustment. The plan's own row is moved by its adjustment (a shift in
    // the annual return moves every long-run percentile by about the same amount), so the row reads as this plan.
    const shift = r.kind === current ? adjustment : 0;
    const [p5, p25, p50, p75, p95] = (r.values as [number, number, number, number, number]).map((v) => v + shift) as [number, number, number, number, number];
    return { kind: r.kind, label: r.label, p5, p25, p50, p75, p95, current: r.kind === current, record: r.kind === 'record', shift };
  });
}

const pct = (v: number, digits = 1) => `${(v * 100).toFixed(digits).replace(/^-(0(\.0+)?)$/, '$1').replace('-', '−')}%`;

export function modelTakeaway(rows: ModelRange[]): string {
  const mine = rows.find((r) => r.current);
  const record = rows.find((r) => r.record);
  if (!mine || !record) return '';
  const wider = (mine.p95 - mine.p5) / (record.p95 - record.p5);
  const versus = wider >= 1.3 ? `That is ${wider.toFixed(1)} times as wide as the record’s own 30-year windows (${pct(record.p5)} to ${pct(record.p95)}): the model allows futures worse and better than any the record contains.` : `The record’s own 30-year windows run from ${pct(record.p5)} to ${pct(record.p95)}, about as wide.`;
  const adjusted = Math.abs(mine.shift) > 1e-9 ? ` with its ${mine.shift > 0 ? '+' : '−'}${Math.abs(mine.shift * 100).toFixed(1).replace(/\.0$/, '')}-point adjustment` : '';
  return `Under this plan’s model${adjusted}, US stocks return about ${pct(mine.p50)} a year after inflation over 30 years in the middle case, and between ${pct(mine.p5)} and ${pct(mine.p95)} in 9 of 10. ${versus}`;
}

// ---- longevity -----------------------------------------------------------------------

export function longevityStatement(profile: Profile): string {
  const l = profile.assumptions.longevity;
  if (l.type === 'fixed_age') return `Everyone dies at exactly ${l.age}. There is no longevity risk in this plan: lives neither end early nor run long.`;
  return `Each person’s lifespan is drawn afresh in every simulated life from the Social Security Administration’s life tables for their sex and birth year, shifted by their stated health${l.improvement ? ', with mortality continuing to improve as it has historically' : ', with mortality frozen at today’s rates'}. The simulation stops when the youngest adult would reach ${profile.simulation.end_age}.`;
}

export interface PersonLongevity {
  name: string;
  /** The deterministic preview's expected age at death. */
  expected: number | null;
  /** Share of lives in which this person is alive at each year index. */
  survival: number[];
  /** Their own age at each year index. */
  ownAge: (yearIndex: number) => number;
  /** Own ages reached by 1 in 2, 1 in 4 and 1 in 10 lives (null when the plan ends first). */
  reach: { half: number | null; quarter: number | null; tenth: number | null };
}

const months = (ym: string | null) => {
  const [y, m] = (ym ?? '').split('-').map(Number);
  return (y ?? 0) * 12 + ((m ?? 1) - 1);
};

/** First own age at which the survival curve is at or below `share`, by linear interpolation. */
export function ageReachedBy(survival: number[], ages: number[], share: number): number | null {
  for (let i = 1; i < survival.length; i++) {
    const a = survival[i - 1] as number;
    const b = survival[i] as number;
    if (b <= share && a > share) return (ages[i - 1] as number) + ((a - share) / (a - b)) * ((ages[i] as number) - (ages[i - 1] as number));
  }
  return null;
}

/**
 * Own ages outlived by 1 in 2, 1 in 4 and 1 in 10 lives. Read from the engine's own
 * distribution of ages at death (exact, to the month); null where that many lives are
 * still going when the plan ends, since their age at death is then unknown. Without the
 * distribution, read off the survival curve: a year's entry counts lives alive at some
 * point in it, which is to say at its start, eleven months before that December's age.
 */
function reachAges(summary: Summary, person: number, survival: number[], ownAges: number[]): PersonLongevity['reach'] {
  const dist = summary.distributions.find((d) => d.scalar === (person === 0 ? 'death_age_0' : 'death_age_1'));
  const atEnd = survival[survival.length - 1] ?? 0;
  const [q50, q75, q90] = [dist?.quantiles[4], dist?.quantiles[5], dist?.quantiles[6]];
  if (person < 2 && dist && dist.count > 0 && typeof q50 === 'number' && typeof q75 === 'number' && typeof q90 === 'number') {
    return { half: atEnd >= 0.5 ? null : q50, quarter: atEnd >= 0.25 ? null : q75, tenth: atEnd >= 0.1 ? null : q90 };
  }
  const starts = ownAges.map((a, i) => (i === 0 ? a : a - 11 / 12));
  return { half: ageReachedBy(survival, starts, 0.5), quarter: ageReachedBy(survival, starts, 0.25), tenth: ageReachedBy(survival, starts, 0.1) };
}

export function longevity(profile: Profile, summary: Summary, preview: PlanPreview | null): PersonLongevity[] {
  const first = profile.household.people[0];
  return profile.household.people.flatMap((person, p) => {
    const survival = summary.survival.people[p];
    if (!survival || !first) return [];
    const offset = (months(first.birth) - months(person.birth)) / 12;
    const ownAges = summary.axis.ages.map((a) => a + offset);
    return [
      {
        name: person.name,
        expected: preview?.expected_age_at_death[p] ?? null,
        survival,
        ownAge: (y: number) => ownAges[y] ?? 0,
        reach: reachAges(summary, p, survival, ownAges),
      },
    ];
  });
}

export function longevityTakeaway(people: PersonLongevity[], summary: Summary, endAge: number): string {
  const one = people[0];
  if (!one) return '';
  const describe = (p: PersonLongevity) => {
    const half = p.reach.half;
    const quarter = p.reach.quarter;
    if (half === null) return `${p.name} is still alive when the plan ends in more than half of lives`;
    return `${p.name} lives past ${Math.round(half)} in half of lives${quarter !== null ? ` and past ${Math.round(quarter)} in one in four` : ` and is still alive at the plan’s end in more than one in four`}`;
  };
  const last = summary.survival.household[summary.survival.household.length - 1] ?? 0;
  const end = last >= 0.005 ? ` In ${Math.round(last * 100)} of 100 lives someone is still alive when the simulation stops at ${endAge}.` : '';
  return `${people.slice(0, 2).map(describe).join('; ')}.${end}`;
}

// ---- tax -----------------------------------------------------------------------------

export type FilingStatus = 'single' | 'married_joint' | 'head_of_household';
export const FILING_LABEL: Record<FilingStatus, string> = { single: 'single', married_joint: 'married filing jointly', head_of_household: 'head of household' };

/** The filing status the engine applies in the plan's first year: two adults file jointly, one adult with a child under 17 is a head of household. */
export function filingStatus(profile: Profile): FilingStatus {
  const start = months(profile.start);
  const adults = profile.household.people.filter((p) => !p.joins).length;
  if (adults >= 2) return 'married_joint';
  const young = profile.household.children.some((c) => {
    const age = (start - months(c.birth)) / 12;
    return age >= 0 && age < 17;
  });
  return young ? 'head_of_household' : 'single';
}

export interface Bracket {
  rate: number;
  from: number;
  /** Null for the top bracket. */
  to: number | null;
}

export function brackets(status: FilingStatus): Bracket[] {
  let from = 0;
  return tax2026.ordinary_brackets[status].map((b) => {
    const out = { rate: b.rate, from, to: b.up_to };
    from = b.up_to ?? from;
    return out;
  });
}

export const standardDeduction = (status: FilingStatus) => tax2026.standard_deduction[status];

export interface TaxPoint {
  year: number;
  /** Taxable income, today's dollars (the return's nominal figure divided by its price level). */
  taxable: number;
  agi: number;
  total: number;
  /** The engine's own marginal federal rate on ordinary income that year. */
  marginal: number;
  /** True when the year is taxed at a flat foreign rate (living abroad), outside the US ladder. */
  foreign: boolean;
}

function taxPoint(trace: PathTrace, year: number): TaxPoint | null {
  const r = trace.tax_returns.find((t) => t.year === year);
  if (!r) return null;
  const num = (key: string) => (typeof r.result[key] === 'number' ? (r.result[key] as number) : 0);
  const level = r.price_level > 0 ? r.price_level : 1;
  return { year, taxable: num('taxable_income') / level, agi: num('agi') / level, total: num('total') / level, marginal: num('marginal_ordinary_rate'), foreign: num('foreign') > 0 };
}

/** The first full calendar year of a traced life, and a settled retired year (five years in), from its tax returns. */
export function taxPoints(trace: PathTrace): { now: TaxPoint | null; retired: TaxPoint | null; retiredAge: number | null } {
  const years = trace.tax_returns.map((t) => t.year);
  if (years.length === 0) return { now: null, retired: null, retiredAge: null };
  const startYear = Number(trace.start.slice(0, 4));
  const startMonth = Number(trace.start.slice(5, 7));
  const now = taxPoint(trace, startMonth === 1 ? startYear : startYear + 1) ?? taxPoint(trace, years[0] as number);
  const retirementAge = trace.scalars.find((s) => s.scalar === 'retirement_age')?.value ?? null;
  if (retirementAge === null) return { now, retired: null, retiredAge: null };
  const retiredStatuses = new Set(trace.statuses.map((status, i) => (status.startsWith('retired') ? i : -1)).filter((i) => i >= 0));
  const firstRetired = trace.annual.status.findIndex((status) => retiredStatuses.has(status));
  if (firstRetired < 0) return { now, retired: null, retiredAge: retirementAge };
  // Five years into retirement, or as late as the life allows short of its (partial) last year.
  const lastFull = trace.annual.years.length - 2;
  const year = trace.annual.years[Math.min(firstRetired + 5, Math.max(firstRetired, lastFull))];
  return { now, retired: year === undefined ? null : taxPoint(trace, year), retiredAge: retirementAge };
}

/** Where a year's taxable income sits on the ladder: inside the bracket the engine says it is in. */
export function placeOnLadder(point: TaxPoint, ladder: Bracket[]): { bracket: Bracket | null; x: number } {
  if (point.marginal <= 0) return { bracket: null, x: 0 };
  const bracket = ladder.find((b) => Math.abs(b.rate - point.marginal) < 1e-6) ?? null;
  if (!bracket) return { bracket: null, x: point.taxable };
  const top = bracket.to ?? Math.max(bracket.from * 1.25, point.taxable);
  return { bracket, x: Math.min(top, Math.max(bracket.from, point.taxable)) };
}

export function taxStatement(profile: Profile): string {
  const t = profile.assumptions.taxes;
  if (t.kind.type === 'none') return 'No tax of any kind is charged. Every result on the other views is before tax.';
  if (t.kind.type === 'flat') return `All income is taxed at a flat ${pct(t.kind.rate, 0)}: no brackets, deductions, payroll tax or credits.`;
  const where = profile.household.location.country === 'US' ? `${profile.household.location.state}’s income tax where it has one` : 'a flat foreign rate while living abroad';
  return `Each simulated year is taxed as a full return under 2026 federal law (the brackets made permanent in 2025), with ${where}, payroll tax, capital gains stacked on ordinary income, the taxation of Social Security, health-insurance credits and Medicare surcharges. ${t.index_brackets ? 'Brackets, deductions and limits rise with simulated inflation, as the law provides, so they stay put in today’s dollars.' : 'Brackets and deductions are frozen in dollars, so inflation slowly pushes income into higher brackets.'}`;
}

// ---- Social Security, healthcare, careers ----------------------------------------------

export function socialSecurityFacts(profile: Profile, moments?: PlanMoments): string[] {
  const out = profile.household.people.map((p, i) => {
    const ss = p.social_security;
    const basis = ss.basis.type === 'from_earnings' ? `worked out from a simulated earnings record starting at ${(ss.basis as { career_start_age: number }).career_start_age}` : ss.basis.type.replace(/_/g, ' ');
    // What the run did, beside what the plan says: a benefit is only paid in lives that reach the claim age.
    const paid = moments?.social_security_claim_age[i];
    const share = moments?.social_security_claim_share[i];
    const ran = paid !== null && paid !== undefined && share !== null && share !== undefined ? ` In this run it starts at ${String(Math.round(paid * 10) / 10)} in ${ofHundred(share).text} lives; the rest die first.` : '';
    return `${p.name} claims at ${ss.claim_age}; the benefit is ${basis}.${ran}`;
  });
  const cut = profile.assumptions.taxes.social_security_haircut;
  out.push(cut ? `From ${cut.from_year}, benefits are paid at ${Math.round(cut.factor * 100)}% of the scheduled amount, the cut the trust fund’s shortfall would force.` : 'Benefits are paid in full for life: no cut is assumed when the trust fund runs short (the trustees project that for about 2033).');
  out.push('Benefits rise with simulated inflation every year; a surviving partner keeps the larger of the two benefits.');
  return out;
}

export function healthFacts(profile: Profile): string[] {
  const h = profile.spending.healthcare;
  const out: string[] = [];
  if (h.mode === 'modeled') {
    const early = h.early_retirement.type === 'aca' ? `a ${h.quality} marketplace plan with income-based credits` : h.early_retirement.type.replace(/_/g, ' ');
    out.push(`Health costs are modelled rather than budgeted: an employer plan while working, ${early} between retirement and 65, then Medicare with its income-related surcharges.`);
  } else {
    out.push('Health costs are whatever the budget says: the model adds nothing for premiums or out-of-pocket costs at any age.');
  }
  out.push(h.long_term_care ? 'Long-term care can strike late in life: at most one episode per life, of random length and cost.' : 'Long-term care is switched off: no life pays for care.');
  out.push(profile.assumptions.health_risk ? 'Medical costs vary at random from year to year.' : 'Medical costs are the average every year: health risk is switched off.');
  return out;
}

export interface CareShare {
  /** Share of all lives that pay anything for long-term care. */
  any: number;
  median: number | null;
  p90: number | null;
}

export function careShare(summary: Summary): CareShare | null {
  const d = summary.distributions.find((x) => x.scalar === 'long_term_care_cost');
  if (!d || d.count === 0) return null;
  const any = d.dots.filter((v) => v !== null && v > 0).length / Math.max(1, d.dots.length);
  const paying = d.dots.filter((v): v is number => v !== null && v > 0);
  return { any, median: paying.length ? (paying[Math.floor(paying.length / 2)] as number) : null, p90: d.quantiles[6] ?? null };
}

export function unemployment(summary: Summary): { any: number; p90: number | null } | null {
  const d = summary.distributions.find((x) => x.scalar === 'months_unemployed');
  if (!d || d.count === 0) return null;
  return { any: d.dots.filter((v) => v !== null && v >= 1).length / Math.max(1, d.dots.length), p90: d.quantiles[6] ?? null };
}

// ---- what the model does not know ---------------------------------------------------

export interface Omission {
  area: string;
  items: string[];
}

/** Deliberate omissions and simplifications, from docs/internal/module-*.md and docs/engine-output.md. */
export const OMISSIONS: Omission[] = [
  {
    area: 'Markets',
    items: [
      'Today’s valuations and interest rates. Reshuffled and historical paths begin in a random or historical state, not in the market as it stands now.',
      'Anything that has not happened in US markets since 1926: the record contains no lost half-century and no hyperinflation.',
      'The regime model has no pull back to the long-run average; the textbook model has no clustering and constant yields.',
      'Income from a custom asset: it is assumed to pay none.',
    ],
  },
  {
    area: 'Tax',
    items: [
      'Changes in the law. 2026 rules are assumed to last for ever, indexed to inflation.',
      'The alternative minimum tax, married-filing-separately, tax-exempt interest, and the child-care, saver’s, education, earned-income and foreign tax credits.',
      'Some state details: recapture in New York and Connecticut, income limits on several states’ retirement exclusions.',
      'Years abroad are taxed at one flat rate: no treaties, no foreign housing exclusion, no US filing on top.',
    ],
  },
  {
    area: 'Social Security and Medicare',
    items: [
      'Divorced-spouse, child and family-maximum rules; the 40-credit eligibility check.',
      'Benefit timing to the month: cost-of-living rises are continuous and the earnings test withholds evenly through the year.',
      'Any reform other than the across-the-board cut the plan can switch on.',
    ],
  },
  {
    area: 'Health and care',
    items: [
      'Medicaid and long-term care insurance: a life pays for care from its own money until it runs out.',
      'More than one care episode per life, and differences in health costs between states.',
      'Family premiums on employer cover: each adult pays the single-cover share.',
    ],
  },
  {
    area: 'Saving and spending',
    items: [
      'Behaviour: nobody panics and sells in a crash, spends a windfall, or helps a relative unless the plan says so.',
      'The 401(k) catch-up for ages 60 to 63, the compensation cap on employer plans, and employer true-up of the match.',
    ],
  },
  {
    area: 'Lives',
    items: [
      'Children’s health costs as a separate item: they are assumed to sit inside the standard cost of raising a child.',
      'Any link between health, wealth and lifespan: lifespans follow the national tables whatever the life’s finances.',
      'The world after the youngest adult would reach the plan’s end age: lives still going then are cut off, not concluded.',
    ],
  },
];
