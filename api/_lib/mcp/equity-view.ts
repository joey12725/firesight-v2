// The private-company part of the compact plan view: the same figures and wording as
// the website's (web/src/assistant/tools/plan-view.ts and web/src/engine/equity-math.ts),
// on the opaque profile JSON the server holds. api/_tests/mcp-parity.test.ts keeps the
// two identical.

type Json = Record<string, unknown>;

const record = (value: unknown): Json => (value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Json) : {});
const list = (value: unknown): Json[] => (Array.isArray(value) ? value.map(record) : []);
const number = (value: unknown): number => (typeof value === 'number' && Number.isFinite(value) ? value : 0);

/** One row of an outcome table, as the engine writes it. */
export interface OutcomeRow {
  kind: string;
  probability: number;
  years_from: number;
  years_to: number;
  multiple_low: number;
  multiple_high: number;
}

/** The engine's preview of one private company (core `equity::preview`); the fields read here. */
export interface EquityPreview {
  table: OutcomeRow[];
  typical: OutcomeRow[];
  implied_multiple: number;
  typical_implied_multiple: number;
  nothing_share: number;
  tender_before_share: number;
  spread_iso_vested: number;
  spread_nso_vested: number;
  fmv_409a: number;
  tender_annual_probability: number;
}

const whole = (v: number) => Math.round(v);
const places = (v: number, dp: number) => Math.round(v * 10 ** dp) / 10 ** dp;
const price = (v: number) => Number(v.toPrecision(4));

/** The strike of an option holding, or null for RSUs and shares. */
function strikeOf(holding: Json): number | null {
  const kind = record(holding.kind);
  return kind.type === 'iso' || kind.type === 'nso' ? number(kind.strike) : null;
}

/** Units of a holding that are vested today: never more than it has, never below 0. */
function vestedUnits(holding: Json): number {
  return Math.min(Math.max(number(holding.vested), 0), Math.max(number(holding.shares), 0));
}

/** Value today at the latest preferred price (equity-math.ts `paperValue`). */
function paperValue(company: Json): { vested: number; total: number } {
  const preferred = number(company.preferred_price);
  let vested = 0;
  let total = 0;
  for (const holding of list(company.holdings)) {
    const strike = strikeOf(holding);
    const each = strike === null ? preferred : Math.max(preferred - strike, 0);
    vested += (record(holding.kind).type === 'shares' ? number(holding.shares) : vestedUnits(holding)) * each;
    total += number(holding.shares) * each;
  }
  return { vested, total };
}

/** Cash to exercise the vested options (equity-math.ts `exerciseCostVested`). */
function exerciseCostVested(company: Json): number {
  let cost = 0;
  for (const holding of list(company.holdings)) {
    const strike = strikeOf(holding);
    if (strike !== null) cost += vestedUnits(holding) * strike;
  }
  return cost;
}

const ODDS_KINDS = ['fails', 'acquired', 'ipo', 'still_private'] as const;
export type OddsOf100 = Record<(typeof ODDS_KINDS)[number], number>;

/** An outcome table as futures of 100 by kind, to one decimal, adding up to exactly 100. */
export function oddsOf100(rows: OutcomeRow[]): OddsOf100 {
  const sums = ODDS_KINDS.map((kind) => rows.reduce((n, row) => (row.kind === kind ? n + row.probability : n), 0));
  const total = sums.reduce((a, b) => a + b, 0);
  const tenths = sums.map((p) => (total > 0 ? (p / total) * 1000 : 0));
  const floors = tenths.map(Math.floor);
  let left = total > 0 ? 1000 - floors.reduce((a, b) => a + b, 0) : 0;
  const order = tenths.map((t, i) => ({ i, rest: t - Math.floor(t) })).sort((a, b) => b.rest - a.rest || a.i - b.i);
  for (const { i } of order) {
    if (left <= 0) break;
    floors[i] = (floors[i] as number) + 1;
    left -= 1;
  }
  return Object.fromEntries(ODDS_KINDS.map((kind, i) => [kind, (floors[i] as number) / 10])) as OddsOf100;
}

/** The stage's typical table as rows a custom `outcomes` table can be built from. */
export function typicalRows(rows: OutcomeRow[]) {
  const half = (v: number) => Math.round(v * 2) / 2;
  const sig2 = (v: number) => Number(v.toPrecision(2));
  return rows.map((row) => {
    const from = Math.min(30, Math.max(0, half(row.years_from)));
    const low = sig2(row.multiple_low);
    return {
      id: row.kind,
      kind: row.kind,
      probability: places(row.probability, 4),
      years_from: from,
      years_to: Math.min(30, Math.max(from, half(row.years_to))),
      multiple_low: low,
      multiple_high: Math.max(low, sig2(row.multiple_high)),
    };
  });
}

export const EQUITY_NOTE =
  'Paper value is at the latest preferred price, options at price less strike, before tax; it is in no balance above and in no net worth figure. odds_in_force and typical_odds are futures of 100 over the next ten years (a company still private then faces the same odds again); typical_rows is the stage\'s typical table with each row\'s timing (years from today) and price range (multiples of the last round\'s price for an IPO, of the last valuation for a sale), ready to copy into a custom outcomes table; implied_multiple is the mean common-stock price at the outcome over the preferred price, counting futures that pay nothing as 0; nothing_share and tender_before_share are shares of futures over every round. These are the model\'s inputs, not results of a run.';

/** What the plan implies about one private company before any simulation; null for a company with no price yet. */
export function companyView(company: Json, otherNetWorth: number, equityPreview: ((company: Json) => EquityPreview) | undefined) {
  const preferred = number(company.preferred_price);
  if (!(preferred > 0)) return null;
  const paper = paperValue(company);
  const denominator = otherNetWorth + paper.total;
  const base = {
    id: company.id,
    name: company.name,
    included: company.included,
    paper_value_vested: whole(paper.vested),
    paper_value_total: whole(paper.total),
    exercise_cost_vested: whole(exerciseCostVested(company)),
    share_of_net_worth: denominator > 0 ? places(paper.total / denominator, 3) : null,
  };
  let p: EquityPreview;
  try {
    if (!equityPreview) return base;
    p = equityPreview(company);
  } catch {
    return base;
  }
  return {
    ...base,
    odds_in_force: oddsOf100(p.table),
    typical_odds: oddsOf100(p.typical),
    typical_rows: typicalRows(p.typical),
    implied_multiple: places(p.implied_multiple, 3),
    typical_implied_multiple: places(p.typical_implied_multiple, 3),
    nothing_share: places(p.nothing_share, 3),
    tender_before_share: places(p.tender_before_share, 3),
    spread_iso_vested: whole(p.spread_iso_vested),
    spread_nso_vested: whole(p.spread_nso_vested),
    fmv_409a: price(p.fmv_409a),
    fmv_fraction: places(p.fmv_409a / preferred, 3),
    fmv_409a_basis: company.fmv_409a === null ? 'typical for the stage' : 'stated in the plan',
    tender_annual_probability: places(p.tender_annual_probability, 3),
  };
}

/** The `private_equity` entries of the view's `derived` block: one per priced company. */
export function companiesView(profile: Json, otherNetWorth: number, equityPreview: ((company: Json) => EquityPreview) | undefined) {
  return list(profile.private_equity).flatMap((company) => companyView(company, otherNetWorth, equityPreview) ?? []);
}
