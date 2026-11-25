// Pure rules of the Startup equity editor: what a new company or holding looks like,
// what an edit has to write so the plan stays valid, and the few figures the editor
// derives itself. Nothing here touches React or the store; `model.test.ts` checks the
// rules against the engine.
import type { CompanyStage, EquityHolding, HoldingKind, OutcomeRow, PatchOp, PrivateCompany, Profile } from '@/engine/types';
import { set, append } from '@/state/patches';
import { uniqueId } from '@/state/pointer';

/** The engine's bounds (core `equity.rs`), so the editor can stop before an error. */
export const LIMITS = {
  /** Most private companies in a plan. */
  companies: 4,
  /** Most holdings of one company. */
  holdings: 8,
  /** Most rows in a custom outcome table. */
  outcomes: 8,
  /** Most yearly sales after an IPO lockup. */
  sellDownYears: 5,
  /** Dollars of incentive options (shares times strike) that may first become exercisable in a year. */
  isoYearly: 100_000,
  /** Most months since an option grant: options lapse at ten years, so 120 is already an error. */
  grantAgeMonths: 119,
  /** Most dollars per share for a strike, a basis or a 409A value at exercise. */
  perShare: 1_000_000,
  /** The tender price as a fraction of the company's share price: lowest and highest. */
  tenderPriceMin: 0.05,
  tenderPriceMax: 2,
} as const;

/** The grant age the engine gives a new option holding, in months. */
export const DEFAULT_GRANT_AGE = 24;
/** The vesting span the editor assumes when it estimates: four years. */
export const VESTING_MONTHS = 48;

export type HoldingType = HoldingKind['type'];

export const isOption = (h: EquityHolding): boolean => h.kind.type === 'iso' || h.kind.type === 'nso';
/** Options and RSUs vest; shares already owned do not. */
export const vests = (h: EquityHolding): boolean => h.kind.type !== 'shares';
export const unvested = (h: EquityHolding): number => (vests(h) ? Math.max(0, h.shares - h.vested) : 0);

/**
 * What common stock gets, as a multiple of the preferred price, when the whole company
 * sells for `m` times its last valuation and investors are paid first on the fraction
 * `overhang` of it: `m` at or above 1, otherwise `max(0, (m - overhang) / (1 - overhang))`.
 * The engine's rule (core `equity.rs`, `common_multiple`).
 */
export function commonMultiple(m: number, overhang: number): number {
  const l = Math.min(0.95, Math.max(0, overhang));
  return m >= 1 ? m : Math.max(0, (m - l) / (1 - l));
}

/** Value of one holding at the preferred price: options at price less strike, not below 0. */
export function holdingValue(holding: EquityHolding, price: number): number {
  const k = holding.kind;
  const each = k.type === 'iso' || k.type === 'nso' ? Math.max(price - k.strike, 0) : price;
  return holding.shares * each;
}

/**
 * The job a new company is linked to: the first job of the first person, else the
 * first job in the plan; null when there is none. A job that has already started is
 * preferred over one that starts later.
 */
export function currentJob(profile: Profile): { id: string; owner: number } | null {
  const jobs = profile.incomes.filter((i) => i.kind.type === 'job');
  const pick = jobs.find((j) => j.owner === 0 && j.start === null) ?? jobs.find((j) => j.start === null) ?? jobs.find((j) => j.owner === 0) ?? jobs[0];
  return pick ? { id: pick.id, owner: pick.owner } : null;
}

/** What "Add a company" appends, less its id. The engine fills in every other field. */
export function newCompany(profile: Profile): { name: string; stage: CompanyStage; preferred_price: number; job: string | null; owner: number } {
  const job = currentJob(profile);
  return { name: 'Company', stage: 'series_b', preferred_price: 0, job: job?.id ?? null, owner: job?.owner ?? 0 };
}

/**
 * What each entry of the "Add a holding" menu appends. Options and RSUs of a company
 * with no linked job are made fully vested, because nothing vests at a company the
 * person does not work for.
 */
export function newHolding(type: HoldingType, linked: boolean): { name: string } & Record<string, unknown> {
  const vesting = (shares: number) => (linked ? { shares, vested: 0, vest_months_left: VESTING_MONTHS } : { shares, vested: shares, vest_months_left: 0 });
  switch (type) {
    case 'iso':
      return { name: 'ISO grant', kind: { type: 'iso', strike: 1 }, ...vesting(10_000) };
    case 'nso':
      return { name: 'NSO grant', kind: { type: 'nso', strike: 1 }, ...vesting(10_000) };
    case 'rsu':
      return { name: 'RSUs', kind: { type: 'rsu' }, ...vesting(5_000) };
    case 'shares':
      return { name: 'Shares', kind: { type: 'shares', basis_per_share: 1 }, shares: 5_000 };
  }
}

/**
 * The operations for a change of a holding's type. Strike and grant age carry between
 * the two kinds of option, and the strike becomes the basis of shares (and back).
 * Shares have no vesting, so a holding that turns from shares into options or RSUs is
 * made fully vested: otherwise it would be all unvested with no schedule.
 */
export function switchKindOps(path: string, holding: EquityHolding, type: HoldingType): PatchOp[] {
  const from = holding.kind;
  if (from.type === type) return [];
  const price = from.type === 'iso' || from.type === 'nso' ? from.strike : from.type === 'shares' ? from.basis_per_share : 1;
  const age = from.type === 'iso' || from.type === 'nso' ? from.granted_months_ago : DEFAULT_GRANT_AGE;
  const kind: Record<string, unknown> =
    type === 'iso' || type === 'nso' ? { type, strike: price, granted_months_ago: age } : type === 'rsu' ? { type } : { type, basis_per_share: price };
  const ops = [set(`${path}/kind`, kind)];
  if (from.type === 'shares') ops.push(set(`${path}/vested`, holding.shares), set(`${path}/vest_months_left`, 0), set(`${path}/cliff_months`, 0));
  return ops;
}

/**
 * The operations for the "Linked job" field. Linking sets the company's owner to the
 * job's owner. Unlinking makes every option and RSU holding fully vested, because
 * unvested units at a company with no linked job are an error.
 */
export function linkJobOps(path: string, company: PrivateCompany, job: { id: string; owner: number } | null): { ops: PatchOp[]; vestedChanged: boolean } {
  if (job) return { ops: [set(`${path}/job`, job.id), set(`${path}/owner`, job.owner)], vestedChanged: false };
  const ops = [set(`${path}/job`, null)];
  let vestedChanged = false;
  for (const h of company.holdings) {
    if (!vests(h)) continue;
    const hp = `${path}/holdings/@${h.id}`;
    if (unvested(h) > 0) {
      ops.push(set(`${hp}/vested`, h.shares));
      vestedChanged = true;
    }
    if (h.vest_months_left !== 0) ops.push(set(`${hp}/vest_months_left`, 0));
    if (h.cliff_months !== 0) ops.push(set(`${hp}/cliff_months`, 0));
  }
  return { ops, vestedChanged };
}

/** A price per share, rounded to cents (four significant figures under ten cents). */
export function roundPrice(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 0;
  return value < 0.1 ? Number(value.toPrecision(4)) : Math.round(value * 100) / 100;
}

/**
 * "I only know the 409A value": writes the 409A value and a preferred price estimated
 * from it, in one edit. `ratio` is the stage's typical 409A value as a fraction of the
 * preferred price, read from the engine.
 */
export function from409aOps(path: string, fmv: number, ratio: number): PatchOp[] {
  return [set(`${path}/fmv_409a`, fmv), set(`${path}/preferred_price`, roundPrice(fmv / ratio))];
}

/** True when the preferred price is what `from409aOps` would write for this 409A value. */
export function priceIsEstimated(company: Pick<PrivateCompany, 'preferred_price' | 'fmv_409a'>, ratio: number): boolean {
  return company.fmv_409a !== null && company.fmv_409a > 0 && ratio > 0 && company.preferred_price === roundPrice(company.fmv_409a / ratio);
}

/**
 * Months since the grant, estimated from how much has vested over a four-year
 * schedule: at least a year. Null when nothing has vested (no basis for an estimate).
 */
export function estimateGrantAge(vested: number, shares: number): number | null {
  if (!(vested > 0) || !(shares > 0)) return null;
  return Math.max(12, Math.round((VESTING_MONTHS * Math.min(vested, shares)) / shares));
}

/**
 * Months since the grant, estimated from the months left to vest on a four-year
 * schedule. Used when the household has typed its own months left: that is better
 * evidence of the grant date than the vested share.
 */
export function grantAgeFromMonthsLeft(left: number): number {
  return Math.min(LIMITS.grantAgeMonths, Math.max(0, VESTING_MONTHS - Math.round(left)));
}

/**
 * Where a holding's grant age stands: still the editor's estimate from how much has
 * vested (or the default while nothing has), its estimate from the months left to vest,
 * or the household's own figure (null). Read from the holding itself, so it is the same
 * after undo, redo, a reload or a switch of plan.
 */
export function grantAgeBasis(holding: EquityHolding): 'vested' | 'months_left' | null {
  const k = holding.kind;
  if (k.type !== 'iso' && k.type !== 'nso') return null;
  if (k.granted_months_ago === (estimateGrantAge(holding.vested, holding.shares) ?? DEFAULT_GRANT_AGE)) return 'vested';
  if (unvested(holding) > 0 && k.granted_months_ago === grantAgeFromMonthsLeft(holding.vest_months_left)) return 'months_left';
  return null;
}

/** True while the months left to vest are what the editor would estimate from the vested share. */
export function monthsLeftIsEstimate(holding: EquityHolding): boolean {
  return vests(holding) && holding.vest_months_left === estimateVestMonthsLeft(holding.vested, holding.shares);
}

/**
 * The further operations of an edit to a holding's count or vested count: with no
 * linked job a fully vested holding stays fully vested, and the grant age and months
 * left to vest follow while they are still the editor's own estimates.
 */
export function followCountOps(path: string, holding: EquityHolding, linked: boolean, shares: number, vested: number): PatchOp[] {
  const ops: PatchOp[] = [];
  // With no linked job nothing can be unvested: a fully vested holding stays fully vested when its count changes.
  if (!linked && shares !== holding.shares && holding.vested >= holding.shares) {
    ops.push(set(`${path}/vested`, shares));
    vested = shares;
  }
  const age = grantAgeBasis(holding) === 'vested' ? estimateGrantAge(vested, shares) : null;
  if (age !== null) ops.push(set(`${path}/kind/granted_months_ago`, age));
  const left = monthsLeftIsEstimate(holding) ? estimateVestMonthsLeft(vested, shares) : null;
  if (left !== null && (linked || left === 0)) {
    ops.push(set(`${path}/vest_months_left`, left));
    if (left < holding.cliff_months) ops.push(set(`${path}/cliff_months`, 0));
  }
  return ops;
}

/**
 * The further operations of an edit to the months left to vest: while the grant age is
 * still an estimate it is re-estimated from the months left, so the two cannot
 * contradict each other (29 months since the grant with 24 left of a four-year grant).
 */
export function followMonthsLeftOps(path: string, holding: EquityHolding, left: number): PatchOp[] {
  if (grantAgeBasis(holding) === null || !(unvested(holding) > 0)) return [];
  return [set(`${path}/kind/granted_months_ago`, grantAgeFromMonthsLeft(left))];
}

/**
 * The operations for a change of stage. A price that was estimated from the 409A value
 * at the old stage's typical fraction follows to the new stage's, so the estimate and
 * the note about it stay true. `ratio` gives a stage's typical 409A fraction (0 when unknown).
 */
export function stageOps(path: string, company: Pick<PrivateCompany, 'stage' | 'preferred_price' | 'fmv_409a'>, stage: CompanyStage, ratio: (stage: CompanyStage) => number): PatchOp[] {
  const ops = [set(`${path}/stage`, stage)];
  const next = ratio(stage);
  if (company.fmv_409a !== null && next > 0 && priceIsEstimated(company, ratio(company.stage))) ops.push(set(`${path}/preferred_price`, roundPrice(company.fmv_409a / next)));
  return ops;
}

export interface TenderSale {
  /** Shares and in-the-money vested options a tender counts: what the limit is a fraction of. */
  base: number;
  /** Units sold: the limit times the share of it the household sells. */
  sold: number;
  /** The tender price per share if the company's price is still the last round's. */
  price: number;
  /** Cash before tax, after the strike on options sold. */
  proceeds: number;
}

/**
 * Units of a holding vested by the end of month `months` from today (0 is this month),
 * on the engine's straight-line schedule: the unvested part vests evenly over the
 * months left, nothing before the cliff, and only while the job lasts (`until`: the
 * month a planned departure stops it, or null).
 */
export function vestedBy(holding: EquityHolding, months: number, linked: boolean, until: number | null = null): number {
  const now = Math.min(Math.max(holding.vested, 0), Math.max(holding.shares, 0));
  const left = holding.vest_months_left;
  if (!linked || !(left > 0)) return now;
  const elapsed = Math.min(months + 1, until ?? Infinity);
  if (elapsed < holding.cliff_months || elapsed <= 0) return now;
  return now + (Math.max(holding.shares, 0) - now) * Math.min(1, elapsed / left);
}

/**
 * What one tender offer `months` from today would sell, by the engine's rule (core
 * `equity.rs`, `tender`): the base is shares owned plus the options vested by then
 * whose strike is under the tender price (RSUs cannot be sold); the household sells its
 * chosen share of the limit; shares go first, then non-qualified options, then
 * incentive options. Priced at the last round's price times the tender's price
 * fraction: the engine uses the company's simulated price in that month, which starts
 * there.
 */
export function tenderSale(company: Pick<PrivateCompany, 'job' | 'preferred_price' | 'holdings' | 'tenders' | 'policy'>, months = 0): TenderSale {
  const price = Math.max(0, company.preferred_price) * Math.max(0, company.tenders.price_fraction);
  const linked = company.job !== null;
  const vestedOf = (h: EquityHolding) => vestedBy(h, months, linked, linked ? company.policy.leave_after_months : null);
  const lots = company.holdings.filter((h) => h.kind.type === 'shares');
  const options = (type: 'nso' | 'iso') => company.holdings.filter((h) => h.kind.type === type && price > h.kind.strike);
  const queue: { units: number; each: number }[] = [
    ...lots.map((h) => ({ units: Math.max(h.shares, 0), each: price })),
    ...[...options('nso'), ...options('iso')].map((h) => ({ units: vestedOf(h), each: price - (h.kind as { strike: number }).strike })),
  ];
  const base = queue.reduce((t, q) => t + q.units, 0);
  let allow = price > 0 ? Math.min(1, Math.max(0, company.policy.tender_sell_fraction)) * Math.min(1, Math.max(0, company.tenders.max_fraction)) * base : 0;
  let sold = 0;
  let proceeds = 0;
  for (const q of queue) {
    const n = Math.min(q.units, allow);
    if (n <= 0) continue;
    sold += n;
    proceeds += n * q.each;
    allow -= n;
  }
  return { base, sold, price, proceeds };
}

/**
 * Months left to vest, estimated the same way: the unvested share of four years. At
 * least one month while anything is unvested, and 0 when everything has vested.
 */
export function estimateVestMonthsLeft(vested: number, shares: number): number | null {
  if (!(shares > 0) || vested < 0) return null;
  if (vested >= shares) return 0;
  return Math.max(1, Math.round((VESTING_MONTHS * (shares - vested)) / shares));
}

/**
 * Dollars of an incentive-option grant (shares times strike) that become exercisable
 * per year, measured over the whole grant and its vesting span (one to four years).
 * 0 for other holdings. The engine's `iso_yearly_amount`, which drives the
 * `iso_over_100k_limit` warning.
 */
export function isoYearlyAmount(holding: EquityHolding): number {
  if (holding.kind.type !== 'iso') return 0;
  const spanMonths = Math.min(holding.kind.granted_months_ago + holding.vest_months_left, VESTING_MONTHS);
  return (holding.kind.strike * holding.shares) / Math.max(1, spanMonths / 12);
}

/**
 * Splits an incentive-option grant that is over the $100,000 yearly limit into an ISO
 * part at the limit and an NSO part with the rest, in the same proportion for vested
 * and unvested units. An estimate: the grant documents show the exact split. Null when
 * the holding is not an ISO grant or is within the limit.
 */
export function splitIsoAtLimit(holding: EquityHolding, taken: Iterable<string> = []): { iso: EquityHolding; nso: EquityHolding } | null {
  const kind = holding.kind;
  if (kind.type !== 'iso') return null;
  const yearly = isoYearlyAmount(holding);
  if (!(yearly > LIMITS.isoYearly)) return null;
  const fraction = LIMITS.isoYearly / yearly;
  const isoShares = Math.floor(holding.shares * fraction);
  const nsoShares = holding.shares - isoShares;
  const vested = Math.min(Math.max(holding.vested, 0), holding.shares);
  // Whole units, and never more vested than either part holds.
  const isoVested = Math.min(isoShares, Math.max(Math.floor(vested * fraction), vested - nsoShares));
  const nsoVested = vested - isoVested;
  return {
    // Both parts keep the grant's dates: the yearly amount is measured over its vesting span.
    iso: { ...holding, shares: isoShares, vested: isoVested },
    nso: {
      ...holding,
      id: uniqueId(`${holding.id}_nso`, [holding.id, ...taken]),
      name: `${holding.name || 'Grant'} (NSO part)`,
      kind: { type: 'nso', strike: kind.strike, granted_months_ago: kind.granted_months_ago },
      shares: nsoShares,
      vested: nsoVested,
    },
  };
}

/** The one edit behind "Split at the $100,000 limit". Empty when there is nothing to split. */
export function splitIsoOps(companyPath: string, company: PrivateCompany, holdingId: string): PatchOp[] {
  const holding = company.holdings.find((h) => h.id === holdingId);
  const parts = holding && splitIsoAtLimit(holding, company.holdings.map((h) => h.id));
  if (!parts) return [];
  return [set(`${companyPath}/holdings/@${holdingId}`, parts.iso), append(`${companyPath}/holdings`, parts.nso)];
}

const sig2 = (v: number) => Number(v.toPrecision(2));
const half = (v: number) => Math.round(v * 2) / 2;

const SEED_ID: Record<OutcomeRow['kind'], string> = { fails: 'fails', acquired: 'acquired', ipo: 'ipo', still_private: 'still_private' };

/**
 * A custom outcome table that starts as the table in force ("Use my own odds"): the
 * exact probabilities, years rounded to the half year and price multiples to two
 * significant figures (0.17, 2.2, 7.4), which is how the typical table shows them and as
 * much as they are known. Seeding leaves the odds exactly as they were; prices and
 * timing move a little with the rounding, and a custom row's date is even between its
 * two years.
 */
export function seedCustomTable(table: OutcomeRow[]): OutcomeRow[] {
  const ids: string[] = [];
  return table.map((row) => {
    const id = uniqueId(SEED_ID[row.kind], ids);
    ids.push(id);
    const from = Math.min(30, Math.max(0, half(row.years_from)));
    const to = Math.min(30, Math.max(from, half(row.years_to)));
    const low = sig2(row.multiple_low);
    return { id, kind: row.kind, probability: row.probability, years_from: from, years_to: to, multiple_low: low, multiple_high: Math.max(low, sig2(row.multiple_high)) };
  });
}

/** A new dated row for a custom table, at 0 of 100, shaped like the stage's typical row of that kind when there is one. */
export function newOutcomeRow(kind: Exclude<OutcomeRow['kind'], 'still_private'>, rows: OutcomeRow[], typical: OutcomeRow[]): OutcomeRow {
  const like = seedCustomTable(typical.filter((r) => r.kind === kind))[0];
  const id = uniqueId(kind, rows.map((r) => r.id));
  return {
    id,
    kind,
    probability: 0,
    years_from: like?.years_from ?? 1,
    years_to: like?.years_to ?? 5,
    multiple_low: like?.multiple_low ?? 1,
    multiple_high: like?.multiple_high ?? 1,
  };
}

/** The stages, in the engine's order. */
export const STAGE_ORDER: CompanyStage[] = ['seed', 'series_a', 'series_b', 'series_c', 'late', 'unicorn'];
