// The engine's standalone preview of one private company (core `equity.rs`,
// `EquityPreview`), mirrored by hand: the output shapes have no generated TypeScript.
// `equity-types.test.ts` runs the real engine and fails when a field is added, renamed
// or removed on the Rust side.
import type { OutcomeRow } from './types';

/**
 * What one company's holdings could pay, before tax, with no household simulated: the
 * holder stays until the outcome, never exercises early and sells everything at the
 * outcome (after the lockup for an IPO). Tender offers before the outcome are left out
 * of the amounts. Dollar amounts are today's dollars at a fixed rate of inflation.
 */
export interface EquityPreview {
  /** Draws made. */
  draws: number;
  /**
   * The outcome table in force (the stage preset with its outlook, or the custom table,
   * normalised). Probabilities are exact, not rounded. For a preset the years are the
   * 10th and 90th percentiles of its timing.
   */
  table: OutcomeRow[];
  /** The stage preset with a typical outlook, for comparison. */
  typical: OutcomeRow[];
  /** 409A value per share in force after defaults. */
  fmv_409a: number;
  /** Preference overhang in force after defaults. */
  preference_overhang: number;
  /** Tender chance per year in force after defaults. */
  tender_annual_probability: number;
  /**
   * Value today of the vested holdings at the preferred price (shares and RSUs at the
   * price, options at price less strike, not below 0).
   */
  paper_value_vested: number;
  /** The same for all holdings, vested or not. */
  paper_value_total: number;
  /** Cash to exercise the vested options (strike only). */
  exercise_cost_vested: number;
  /** Cash to exercise all options (strike only). */
  exercise_cost_total: number;
  /** Spread of the vested incentive options at the 409A value: the amount counted for AMT on exercising now. */
  spread_iso_vested: number;
  /** Spread of the vested non-qualified options at the 409A value: the amount taxed as pay on exercising now. */
  spread_nso_vested: number;
  /** Share of draws that pay nothing at the outcome. */
  nothing_share: number;
  /** Share of draws by what finally happens, over every round, in the order of `PREVIEW_OUTCOMES`. */
  outcome_shares: [number, number, number, number];
  /** Among acquisitions: share in which common stock gets nothing. */
  acquisition_zero_share: number;
  /** Among IPOs: share priced below today's preferred price. */
  ipo_below_last_round_share: number;
  /**
   * Share of draws in which at least one tender offer is expected before the outcome,
   * for someone still employed there.
   */
  tender_before_share: number;
  /**
   * Months from today until the first option deadline: the end of the exercise window
   * after leaving (a former employer, or a planned departure) or a grant's ten-year
   * term, whichever comes first. Null when there are no options. 0 means the deadline
   * is this month or already past.
   */
  option_deadline_months: number | null;
  /**
   * Cash (strike only, before tax) to exercise the vested options that fall due at that
   * first deadline. 0 when the plan lets them lapse, when their strike is at or above
   * the preferred price, or when there are no options.
   */
  option_deadline_cost: number;
  /**
   * Share of draws in which an option deadline comes before the company's outcome:
   * those options are exercised with cash then or lapse, as the policy says. The
   * payoffs assume the exercise is affordable.
   */
  deadline_before_outcome_share: number;
  /** 100 equally likely payoffs, ascending (zeros included). */
  dots: number[];
  /** Quantiles over all draws: p10, p25, p50, p75, p90. */
  quantiles: [number, number, number, number, number];
  /** Quantiles among draws that pay something: p10, p50, p90. Null when no draw pays. */
  if_paid: [number | null, number | null, number | null];
  /** Median years to the payout among draws that pay something. Null when no draw pays. */
  years_to_payout_p50: number | null;
  /**
   * Mean common-stock price at the outcome over the preferred price (0 where nothing is
   * paid): what the table in force implies a share is worth.
   */
  implied_multiple: number;
  /** The same for the stage's typical table. */
  typical_implied_multiple: number;
}

/**
 * What each entry of `EquityPreview.outcome_shares` counts, in order. `written_off` is a
 * company still private after thirty years.
 */
export const PREVIEW_OUTCOMES = ['fails', 'written_off', 'acquired', 'ipo'] as const;
export type PreviewOutcome = (typeof PREVIEW_OUTCOMES)[number];
