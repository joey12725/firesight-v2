// Figures about a private-company position that need no engine call: what it is worth
// on paper at the last round's price and what exercising the options costs. The same
// formulas as the engine's preview (core `equity.rs`, `position_value` and `preview`);
// `equity-math.test.ts` compares the two. They live under `engine/` because the editor,
// the results views and the assistant all use them.
import type { EquityHolding, PrivateCompany, Profile } from './types';

/** The strike of an option holding, or null for RSUs and shares. */
function strikeOf(holding: EquityHolding): number | null {
  return holding.kind.type === 'iso' || holding.kind.type === 'nso' ? holding.kind.strike : null;
}

/** Units of a holding that are vested today: never more than it has, never below 0. */
function vestedUnits(holding: EquityHolding): number {
  return Math.min(Math.max(holding.vested, 0), Math.max(holding.shares, 0));
}

/**
 * Value today at the latest preferred price: shares and RSUs at the price, options at
 * the price less the strike (not below 0). `vested` counts vested units only (shares
 * already owned count in full); `total` counts every unit, vested or not. Before tax
 * and before the cost of exercising. Equals `EquityPreview.paper_value_vested` and
 * `paper_value_total`.
 */
export function paperValue(company: PrivateCompany): { vested: number; total: number } {
  const price = company.preferred_price;
  let vested = 0;
  let total = 0;
  for (const holding of company.holdings) {
    const strike = strikeOf(holding);
    const each = strike === null ? price : Math.max(price - strike, 0);
    vested += (holding.kind.type === 'shares' ? holding.shares : vestedUnits(holding)) * each;
    total += holding.shares * each;
  }
  return { vested, total };
}

/**
 * Cash to exercise the options that are vested today: the strike only, before any tax
 * the exercise causes. Equals `EquityPreview.exercise_cost_vested`.
 */
export function exerciseCostVested(company: PrivateCompany): number {
  let cost = 0;
  for (const holding of company.holdings) {
    const strike = strikeOf(holding);
    if (strike !== null) cost += vestedUnits(holding) * strike;
  }
  return cost;
}

/**
 * Cash to exercise every option, vested or not: the strike only, before any tax the
 * exercise causes. Equals `EquityPreview.exercise_cost_total`.
 */
export function exerciseCostTotal(company: PrivateCompany): number {
  let cost = 0;
  for (const holding of company.holdings) {
    const strike = strikeOf(holding);
    if (strike !== null) cost += holding.shares * strike;
  }
  return cost;
}

/** The companies the plan counts (`included` is on), in profile order. */
export function includedCompanies(profile: Profile): PrivateCompany[] {
  // A profile saved before the feature existed may reach a caller before the engine has filled the key in.
  const companies = (profile as { private_equity?: PrivateCompany[] }).private_equity ?? [];
  return companies.filter((company) => company.included);
}
