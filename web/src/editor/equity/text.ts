// Every sentence the Startup equity editor writes from numbers, as pure functions so
// the wording can be tested. Probabilities read as "N of 100 futures"; per-share prices
// use `moneyPrice`, never whole dollars.
import type { EquityHolding, EquityPreview, OutcomeRow, PrivateCompany } from '@/engine';
import { count, money, moneyCompact, moneyPrice, percent } from '@/ui/format';
import { futuresText } from './futures';
import { commonMultiple, isOption, tenderSale, unvested } from './model';

const plural = (n: number, one: string, many = `${one}s`) => (n === 1 ? one : many);
const list = (parts: string[]) => (parts.length <= 1 ? (parts[0] ?? '') : `${parts.slice(0, -1).join(', ')}${parts.length > 2 ? ',' : ''} and ${parts.at(-1)}`);

/** Whole counts that add up to `total`, in proportion to `weights` (largest remainder). */
export function apportion(total: number, weights: number[]): number[] {
  const sum = weights.reduce((a, b) => a + Math.max(0, b), 0);
  if (!(sum > 0) || total <= 0) return weights.map(() => 0);
  const exact = weights.map((w) => (Math.max(0, w) / sum) * total);
  const out = exact.map(Math.floor);
  let left = total - out.reduce((a, b) => a + b, 0);
  const order = exact.map((v, i) => ({ i, rest: v - Math.floor(v) })).sort((a, b) => b.rest - a.rest || a.i - b.i);
  for (const { i } of order) {
    if (left <= 0) break;
    out[i] = (out[i] ?? 0) + 1;
    left -= 1;
  }
  return out;
}

export interface NothingCounts {
  /** Futures of 100 that pay nothing. */
  nothing: number;
  /** Of those: the company shuts down. */
  fails: number;
  /** Still private after thirty years, and written off. */
  writtenOff: number;
  /** Sold for too little to reach common stock. */
  soldForTooLittle: number;
  /** The rest: there is a sale or listing, but the holdings still pay nothing. */
  other: number;
}

/** The futures that pay nothing, split by why. The four parts always add up to `nothing`. */
export function nothingCounts(preview: EquityPreview): NothingCounts {
  const nothing = Math.round(preview.nothing_share * 100);
  const fails = preview.outcome_shares[0];
  const writtenOff = preview.outcome_shares[1];
  const sold = preview.outcome_shares[2] * preview.acquisition_zero_share;
  const rest = preview.nothing_share - fails - writtenOff - sold;
  const [a = 0, b = 0, c = 0, e = 0] = apportion(nothing, [fails, writtenOff, sold, rest > 0.005 ? rest : 0]);
  return { nothing, fails: a, writtenOff: b, soldForTooLittle: c, other: e };
}

/** "in 14 months", "in about 8 years", "this month". */
export function monthsAhead(months: number): string {
  const m = Math.max(0, Math.round(months));
  if (m === 0) return 'this month';
  if (m < 24) return `in ${m} ${plural(m, 'month')}`;
  const y = Math.round(m / 12);
  return `in about ${y} years`;
}

type PreviewCompany = Pick<PrivateCompany, 'job' | 'included' | 'holdings' | 'tenders' | 'policy' | 'exercise_window_months'>;

/** The tender the household has entered as planned: months from now, or null. */
const plannedTender = (company: Pick<PrivateCompany, 'tenders'>): number | null => company.tenders.next_in_months;
/** A planned departure counts only with a linked job (there is nothing to leave otherwise). */
const plannedLeave = (company: Pick<PrivateCompany, 'job' | 'policy'>): number | null => (company.job === null ? null : company.policy.leave_after_months);

/**
 * The collapsed row's figure: "Not counted" for a company left out of the plan, else
 * "{k} of 100 pay nothing". With a planned tender the count is of the company's outcome
 * only, and says so: the tender pays before it.
 */
export function nothingFigure(preview: EquityPreview, company?: Pick<PrivateCompany, 'included' | 'tenders'>): string {
  if (company && !company.included) return 'Not counted';
  const n = Math.round(preview.nothing_share * 100);
  return company && plannedTender(company) !== null ? `${n} of 100 pay nothing at the outcome` : `${n} of 100 pay nothing`;
}

/**
 * The sentences above the preview chart: how many futures pay nothing and why, then
 * what the middle and the best futures pay. `hasOptions` picks the wording for futures
 * where something happens but the holdings still pay nothing. With a planned tender the
 * lead says the count leaves it out. Amounts are rounded as the chart's axis rounds
 * them: they are quantiles of a rough model, not figures known to the dollar.
 */
export function previewSentences(preview: EquityPreview, hasOptions: boolean, company?: Pick<PrivateCompany, 'tenders'>): string[] {
  const n = nothingCounts(preview);
  const p50 = preview.quantiles[2];
  const p90 = preview.quantiles[4];
  const top = preview.dots.at(-1) ?? 0;
  const tender = company ? plannedTender(company) : null;
  if (n.nothing === 0) {
    return ['In every one of 100 futures this pays something.', `The middle future pays ${moneyCompact(p50)}; the best ten pay more than ${moneyCompact(p90)}.`];
  }
  const parts = [
    n.fails > 0 && `the company shuts down within thirty years in ${n.fails}`,
    n.soldForTooLittle > 0 && `is sold for too little to reach common stock in ${n.soldForTooLittle}`,
    n.writtenOff > 0 && `is still private after thirty years in ${n.writtenOff}`,
  ].filter((x): x is string => Boolean(x));
  // The second and third parts share the subject "the company"; alone, each needs it back.
  if (parts[0] && !parts[0].startsWith('the company')) parts[0] = `the company ${parts[0]}`;
  const lapse = preview.deadline_before_outcome_share >= 0.005;
  const rest = hasOptions ? (lapse ? 'the options end up under water or lapse first' : 'the options end up under water') : 'nothing is left to pay out by then';
  let first = `In ${n.nothing} of 100 futures this pays nothing`;
  if (tender !== null) first += ` at the company's outcome, apart from the tender you plan ${monthsAhead(tender)}`;
  if (parts.length > 0) {
    first += `: ${list(parts)}.`;
    if (n.other >= 1) first += ` In ${n.other} more ${rest}.`;
  } else {
    first += n.other >= 1 ? `: ${rest}.` : '.';
  }
  if (n.nothing >= 100) return [first];
  if (p50 > 0) return [first, `The middle future pays ${moneyCompact(p50)}; the best ten pay more than ${moneyCompact(p90)}.`];
  if (p90 > 0) return [first, `The middle future is one of those. The best ten pay more than ${moneyCompact(p90)}.`];
  return [first, `In the rest it pays up to ${moneyCompact(top)}.`];
}

/**
 * Full-size sentences under the lead for what the amounts already count: a planned
 * departure, and option deadlines that come before the company's outcome. Empty when
 * neither applies.
 */
export function previewNotes(preview: EquityPreview, company: PreviewCompany): string[] {
  const out: string[] = [];
  const leave = plannedLeave(company);
  if (leave !== null) {
    const rsu = company.holdings.some((h) => h.kind.type === 'rsu' && h.kind.must_be_employed_at_event);
    out.push(
      `These amounts count your plan to leave ${monthsAhead(leave)}: what is unvested then is forfeited${company.holdings.some(isOption) ? ' and the exercise window starts' : ''}${rsu ? ', and vested RSUs that require staying are lost if the company is still private' : ''}.`,
    );
  }
  const months = preview.option_deadline_months;
  const before = Math.round(preview.deadline_before_outcome_share * 100);
  if (months !== null && before >= 1) {
    const when = months === 0 ? 'is this month or already past' : `is ${monthsAhead(months)}`;
    // Which deadline it is: the exercise window (from today for a former employer, from a planned departure) or a grant's ten-year term.
    const windowEnd = company.job === null ? company.exercise_window_months : leave !== null ? leave + company.exercise_window_months : null;
    const which = windowEnd !== null && months === windowEnd ? (company.job === null ? 'the end of your exercise window' : 'the end of the exercise window after you leave') : "the end of a grant's ten-year term";
    if (company.policy.at_deadline === 'forfeit') {
      out.push(`The first option deadline, ${which}, ${when}. In ${before} of 100 futures a deadline comes before the company's outcome, and those options lapse, as you chose.`);
    } else {
      const cost = preview.option_deadline_cost > 0 ? ` Exercising at the first one costs about ${moneyCompact(preview.option_deadline_cost)} before tax, and the amounts here assume you can pay it.` : '';
      out.push(`The first option deadline, ${which}, ${when}. In ${before} of 100 futures a deadline comes before the company's outcome and the options have to be exercised with cash or lost.${cost}`);
    }
  }
  return out;
}

/** "Middle 80 of 100: $0 to $3.8M", under the dots. */
export function intervalText(preview: EquityPreview): string {
  return `Middle 80 of 100: ${moneyCompact(preview.quantiles[0])} to ${moneyCompact(preview.quantiles[4])}`;
}

const oneDecimal = (v: number) => (Math.round(v * 10) / 10).toFixed(1);

/**
 * The small print under the preview. `custom` adds the typical odds' figure for
 * comparison. `company` picks what is said about staying: nothing for a company with no
 * linked job or with a planned departure (the amounts already count it), and otherwise
 * that staying is assumed and that the full results are lower where the job ends first.
 */
export function previewCaption(preview: EquityPreview, custom: boolean, hasOptions: boolean, company?: PreviewCompany): string {
  const tender = Math.round(preview.tender_before_share * 100);
  const basis = hasOptions ? 'Before tax, with the strike price already taken off the options' : 'Before tax';
  const employed = company ? company.job !== null && plannedLeave(company) === null : true;
  const stays = employed
    ? "Assumes you stay until the outcome and sell then; where the job ends first (a layoff, or leaving) less has vested, so the plan's own results are lower."
    : 'Assumes you sell at the outcome.';
  const tenders =
    company && plannedTender(company) !== null
      ? `Tender offers before the outcome are not in these amounts, the planned one included; one comes first in about ${tender} of 100 futures.`
      : `Tender offers before the outcome are not in these amounts; one comes first in about ${tender} of 100 futures.`;
  const together = company?.holdings.some((h) => unvested(h) > 0) ? ', vested and unvested together' : '';
  return (
    `${basis}, in today's dollars at typical inflation. ${stays} ${tenders} ` +
    `Worth ${money(preview.paper_value_total)} on paper at the last round's price${together}; these odds put a share at about ${oneDecimal(preview.implied_multiple)} times that price` +
    (custom ? ` (typical odds: ${oneDecimal(preview.typical_implied_multiple)}).` : '.')
  );
}

/**
 * What the planned tender would sell, under "A tender is planned", so the entry can be
 * checked: units, price and cash before tax, by the engine's rule. Null when no tender
 * is planned or there is no price yet.
 */
export function tenderLine(company: Pick<PrivateCompany, 'job' | 'preferred_price' | 'holdings' | 'tenders' | 'policy'>): string | null {
  const months = plannedTender(company);
  if (months === null || !(company.preferred_price > 0)) return null;
  const same = 'The same limit and price apply to any later tender.';
  const leave = plannedLeave(company);
  const gone = company.job === null || (leave !== null && leave <= months);
  if (gone && !company.tenders.former_employees) return `Nothing is sold in it: it is not open to people who have left (see Price and eligibility). ${same}`;
  const sale = tenderSale(company, months);
  if (!(sale.base > 0)) return `Nothing is sold in it: a tender takes shares you own and vested options whose strike is below the price, and there are none yet. RSUs cannot be sold in one. ${same}`;
  if (!(sale.sold > 0)) return `Nothing is sold in it, because you chose to sell 0% of what is allowed (under Your decisions). ${same}`;
  const options = company.holdings.some((h) => isOption(h) && h.vested > 0);
  const rsus = company.holdings.some((h) => h.kind.type === 'rsu');
  const what = `${count(Math.round(sale.base))} ${options ? 'shares and vested options' : 'shares'}`;
  return (
    `Sells about ${count(Math.round(sale.sold))} of the ${what} you will have then, at ${moneyPrice(sale.price)} a share: about ${moneyCompact(sale.proceeds)} before tax${options ? ', after the strike' : ''}. ` +
    `That price is today's; the tender is priced from the company's simulated share price in that month. ${rsus ? 'RSUs cannot be sold in a tender. ' : ''}${same}`
  );
}

/** "50,000 vested, 10,000 unvested, $1.50 strike": the collapsed row of a holding. */
export function holdingDetail(holding: EquityHolding): string {
  const k = holding.kind;
  if (k.type === 'shares') return `${count(holding.shares)} ${plural(holding.shares, 'share')}, ${moneyPrice(k.basis_per_share)} basis`;
  const left = unvested(holding);
  const vested = Math.min(Math.max(holding.vested, 0), holding.shares);
  if (k.type === 'rsu') {
    if (vested <= 0) return `${count(holding.shares)} ${plural(holding.shares, 'unit')}, none vested`;
    return left > 0 ? `${count(vested)} vested, ${count(left)} unvested` : `${count(holding.shares)} ${plural(holding.shares, 'unit')}, all vested`;
  }
  const strike = `${moneyPrice(k.strike)} strike`;
  if (vested <= 0) return `${count(holding.shares)} ${plural(holding.shares, 'option')}, none vested, ${strike}`;
  return left > 0 ? `${count(vested)} vested, ${count(left)} unvested, ${strike}` : `${count(vested)} vested, ${strike}`;
}

/** The read-only line under an option holding's fields: what exercising costs and what it is worth on paper. */
export function optionLine(holding: EquityHolding, price: number): string | null {
  const k = holding.kind;
  if (k.type !== 'iso' && k.type !== 'nso') return null;
  const vested = Math.min(Math.max(holding.vested, 0), holding.shares);
  const worth = holding.shares * Math.max(price - k.strike, 0);
  const cost = `Costs ${money(holding.shares * k.strike)} to exercise in full (${money(vested * k.strike)} for the vested part), before tax.`;
  if (!(price > 0)) return cost;
  return `${cost} ${worth > 0 ? `Worth ${money(worth)} after the strike at the last round's price.` : "Worth nothing at the last round's price: the strike is not below it."}`;
}

/** The read-only line under RSUs and shares. */
export function unitLine(holding: EquityHolding, price: number): string | null {
  if (isOption(holding) || !(price > 0)) return null;
  return `Worth ${money(holding.shares * price)} at the last round's price.`;
}

const years = (v: number) => String(Math.round(v * 10) / 10);
const multiple = (v: number) => String(Number(v.toPrecision(2)));

/** "in 1.5 to 6 years". */
export function yearsRange(row: Pick<OutcomeRow, 'years_from' | 'years_to'>): string {
  return row.years_from === row.years_to ? `in ${years(row.years_from)} ${plural(row.years_from, 'year')}` : `in ${years(row.years_from)} to ${years(row.years_to)} years`;
}

/** Per-share prices common stock gets at the two ends of a row's range, in dollars. */
export function sharePrices(row: OutcomeRow, price: number, overhang: number): { low: number; high: number } {
  const x = (m: number) => (row.kind === 'acquired' ? commonMultiple(m, overhang) : m);
  return { low: x(row.multiple_low) * price, high: x(row.multiple_high) * price };
}

/**
 * The second line of an outcome block under typical odds: when, and at what price per
 * share. A sale's price is what common stock gets after investors are paid, never the
 * company's multiple times the round price. Without a price it speaks in multiples.
 */
export function outcomeDetail(row: OutcomeRow, price: number, overhang: number): string {
  switch (row.kind) {
    case 'fails':
      return `${yearsRange(row)}`;
    case 'still_private':
      return 'The same odds then apply to the next ten years. After thirty the equity is counted as worth nothing.';
    case 'acquired': {
      const sale = `the company sells for ${multiple(row.multiple_low)} to ${multiple(row.multiple_high)} times its last valuation`;
      if (!(price > 0)) return `${yearsRange(row)}; ${sale}`;
      const p = sharePrices(row, price, overhang);
      return `${yearsRange(row)}, ${moneyPrice(p.low)} to ${moneyPrice(p.high)} a share for common stock (${sale})`;
    }
    case 'ipo': {
      if (!(price > 0)) return `${yearsRange(row)}, at ${multiple(row.multiple_low)} to ${multiple(row.multiple_high)} times the last round's price`;
      const p = sharePrices(row, price, overhang);
      return `${yearsRange(row)}, ${moneyPrice(p.low)} to ${moneyPrice(p.high)} a share at the IPO`;
    }
  }
}

/** The hint under a custom row's two price multiples. */
export function multipleHint(row: OutcomeRow, price: number, overhang: number, zeroOfHundred: number | null): string {
  if (row.kind === 'ipo') {
    const range = price > 0 ? `${moneyPrice(row.multiple_low * price)} to ${moneyPrice(row.multiple_high * price)} a share at the IPO; one` : 'One';
    return `${range} in ten below, one in ten above. The price when the lockup ends varies around it.`;
  }
  const p = sharePrices(row, price, overhang);
  const range = price > 0 ? `${moneyPrice(p.low)} to ${moneyPrice(p.high)} a share for common stock; one` : 'One';
  return `${range} in ten below, one in ten above.${zeroOfHundred === null ? '' : ` Common stock gets nothing in ${zeroOfHundred} of 100 such sales.`}`;
}

/** "Is acquired: 27 of 100 in your table, 27 typical". */
export function kindHeading(label: string, yours: number, typical: number | null, decimals: 0 | 1): string {
  return `${label}: ${futuresText(yours, decimals)} of 100 in your table, ${typical === null || typical <= 0 ? 'none' : futuresText(typical, decimals)} typical`;
}

/**
 * The hint under "Chance per year" of a tender offer. `p` is the value in force, a
 * yearly chance averaged over every future of the company; `before` is the preview's
 * share of futures in which a tender comes before the outcome, when there is one.
 */
export function tenderHint(p: number, before: number | null = null, met: number | null = null): string {
  if (!(p > 0)) return 'No tender offers, apart from one you plan below.';
  const first = before === null ? '' : ` One comes before the outcome in about ${Math.round(before * 100)} of 100 futures.`;
  const where = "They come only in futures where the company ends at or above its last round's price, and more often there";
  // The engine's preview reports the chance it can deliver; below the one entered, the entry is capped.
  if (met !== null && met < p - 0.005) {
    return `More than the model can deliver: it runs at about ${Math.round(met * 100)}% a year on average over every future, a tender every year where the company ends at or above its last round's price and none elsewhere.${first}`;
  }
  if (p >= 0.95) return `As often as the model allows: at most one a year. ${where}.${first}`;
  return `An average over every future of the company: about one tender every ${Math.round(1 / p)} years. ${where}.${first}`;
}

/** What the household holds, in words: "110,000 options, RSUs and shares". */
export function holdingsNoun(company: Pick<PrivateCompany, 'holdings'>): string {
  const total = company.holdings.reduce((n, h) => n + Math.max(0, h.shares), 0);
  const kinds = [company.holdings.some(isOption) && 'options', company.holdings.some((h) => h.kind.type === 'rsu') && 'RSUs', company.holdings.some((h) => h.kind.type === 'shares') && 'shares'].filter((x): x is string => Boolean(x));
  return `${count(total)} ${list(kinds)}`;
}

/** "Series C, 4 holdings". */
export function companyDetail(stage: string, holdings: number): string {
  return `${stage}, ${holdings === 0 ? 'no holdings' : `${holdings} ${plural(holdings, 'holding')}`}`;
}

/** "35": a fraction as whole percent points, for hints. */
export const wholePercent = (v: number) => percent(v, 0).replace('%', '');
