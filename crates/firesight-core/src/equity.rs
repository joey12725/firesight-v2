//! Private-company equity: one company's fate, vesting, exercises, sales and the tax
//! character of each, as dated flows. Like `career`, this module knows nothing of
//! `sim`: it turns a monthly climate into an [`EquityMonth`] that the simulation books.
//!
//! # How the simulation drives it
//!
//! Once per plan (`Plan::compile`; allocation is fine here):
//!
//! ```text
//! let params  = EquityParams::from_data(data);
//! let model   = CompanyModel::compile(&company, &params, [fed.qsbs_cap, fed.qsbs_cap_old], start);
//! let primary = primary_company(&profile.private_equity);   // the company the outcome scalars describe
//! ```
//!
//! `CompanyModel` is `Copy` and carries the parameters, the outcome table in force and
//! the policy. A company with `included: false` compiles with no holdings and no lots.
//!
//! Once per path (`State::begin`, after the economy path exists), for every company in
//! profile order, whatever its settings:
//!
//! ```text
//! let link = Link::Active or Link::Pending           // has the linked job begun? Ignored
//!                                                    // when the company has no job.
//! st = model.begin(link, &mut rng_company[i], Some(&econ));    // exactly 4 words
//! ty.amt_iso_adjustment += model.start_amt_adjustment();       // ISO shares exercised
//!                                                              // earlier this calendar year
//! ```
//!
//! `begin` is [`CompanyModel::initial_state`], then [`draw_fate`], then [`ipo_month`]
//! for an IPO. The fate (`st.fate`) is fixed from here on.
//!
//! Every month, for every company, one word, drawn before anything else and before any
//! branch on state: `u_tender = rng_company[i].uniform()`. Then, for companies whose
//! status is not `Failed` or `Done`:
//!
//! ```text
//! let climate = EquityClimate { t, year, employed, job_started, job_ended, left_job,
//!     owner_died, window_open: window_open(recession, eq_index, eq_peak, &params),
//!     u_tender, single_stock_return, liquid, wage_tax_rate, wage_withheld_rate, iso_tax_rate };
//! let em = model.step(&mut st, &climate);      // draws nothing, allocates nothing
//! ```
//!
//! and when the owner has died with nobody to inherit, `model.abandon(&mut st)` instead
//! (it returns the basis dropped). For an owner who has not joined the household yet,
//! call `step` with `liquid: 0.0` and discard every flow except `basis_in - basis_out`.
//! `step` on a `Failed` or `Done` company returns an all-zero month, so calling it is
//! harmless.
//!
//! What the simulation does with an [`EquityMonth`] (all nominal dollars):
//!
//! | Field | Goes to |
//! |---|---|
//! | `exercise_cost` | cash out, ledger `equity_exercise` |
//! | `cash_in` | cash in, ledger `income_equity` (it already contains `wages_cash`, cashless ISO income and `share_proceeds`) |
//! | `wages_cash + wages_noncash` | `TaxInputs.wages[owner]`; FICA and supplemental withholding at the event |
//! | `iso_income` | `TaxInputs.wages_no_fica[owner]` (no withholding) |
//! | `short_term_gain`, `long_term_gain` | `TaxInputs.short_term_gains`, `long_term_gains` (either may be negative) |
//! | `amt_adjustment`, `amt_gain_adjustment` | `TaxInputs.amt_iso_adjustment`, `amt_gain_adjustment` |
//! | `qsbs_excluded` | `TaxInputs.qsbs_excluded_gain` |
//! | `to_sleeve_value`, `to_sleeve_basis` | the single-stock sleeve of the taxable account (booked as `income_equity` and a taxable saving) |
//! | `tax_estimate` | held out of the savings sweep this month (the AMT estimate on an ISO exercise, the tax beyond withholding on an NSO exercise) |
//! | `tender_proceeds` | the tender scalar (already inside `cash_in`) |
//! | `basis_in - basis_out` | the lot-basis identity in check mode (`CompanyState::lot_basis` is the stock) |
//! | `units_out` | nothing in the simulation; the unit-conservation test |
//! | `paper_value` | the paper-value series (never a balance) |
//! | `failed_now` | forces a layoff from the linked job when the plan says so |
//! | `replan` | re-plan the tax year next month |
//! | `events[..n_events]` | trace events, at most one of each [`EqEventKind`] |
//!
//! The outcome scalars read the state, not the events: `st.outcome` is set at the
//! terminal event (`None` until then, and `StillPrivate` after a write-off) and
//! `st.event_month` is its month. A company that is not included logs no event except
//! `Failed`, but its state still records the outcome.
//!
//! # What is simplified
//!
//! The share price is a straight line in logs from the latest round to the outcome; the
//! 409A discount closes in a straight line; a tender is priced at the path price times
//! a fixed fraction and happens only on a path heading to or above the last round; one overhang number stands for the preference stack; options and
//! RSUs in the money once a company is listed are settled for cash. The full list is in
//! `data/actuarial/params.json` under `equity_compensation.startup_equity.left_out`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::data::Data;
use crate::economy::EconomyPath;
use crate::math::{self, normal_cdf};
use crate::model::{
    AssetClass, CompanyStage, DeadlinePolicy, EquityHolding, ExerciseTiming, HoldingKind,
    OutcomeKind, OutcomeRow, Outlook, PrivateCompany, SellDown, YearMonth,
};
use crate::rng::{inverse_normal_cdf, normal_from_bits, Rng, Stream};
use crate::stats::EquityOutcome;

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// Bounds and constants
// ---------------------------------------------------------------------------

/// Most private companies in a plan.
pub const MAX_COMPANIES: usize = 4;
/// Most holdings of one company.
pub const MAX_HOLDINGS: usize = 8;
/// Most rows in a custom outcome table.
pub const MAX_OUTCOMES: usize = 8;
/// Share lots tracked per company on a path (exercises pool by calendar year).
pub const MAX_LOTS: usize = 16;
/// Most events one company logs in a month.
pub const MAX_EVENTS: usize = 8;

/// Months of lockup after an IPO when the plan does not say: the usual 180 days.
/// Listed in `params.json` under `unverified_from_memory.startup_equity`.
pub const DEFAULT_LOCKUP_MONTHS: u32 = 6;
/// Options lapse this many months after the grant: ten years (IRC 422(b)(3) for
/// incentive options; non-qualified plans use the same term by practice).
pub const OPTION_TERM_MONTHS: i32 = 120;
/// An incentive option must be exercised within three months of leaving to keep its
/// tax treatment (IRC 422(a)(2)).
pub const ISO_POST_TERMINATION_MONTHS: u32 = 3;
/// Incentive-option shares must be held more than this many months after exercise
/// (IRC 422(a)(1)).
pub const ISO_HOLD_FROM_EXERCISE_MONTHS: i32 = 12;
/// Incentive-option shares must be held more than this many months after the grant
/// (IRC 422(a)(1)).
pub const ISO_HOLD_FROM_GRANT_MONTHS: i32 = 24;
/// A gain is long-term when the shares were held more than this many months
/// (IRC 1222).
pub const LONG_TERM_MONTHS: i32 = 12;
/// Dollar amount of incentive options (shares times strike) that may first become
/// exercisable in a calendar year (IRC 422(d)). Used by validation only.
pub const ISO_ANNUAL_LIMIT: f64 = 100_000.0;
/// Stock acquired from this month on falls under the tiers of [`QSBS_TIERS`]
/// (acquired after July 4, 2025: IRC 1202(a)(6) as amended by P.L. 119-21).
pub const QSBS_NEW_REGIME_FROM: YearMonth = YearMonth::ym(2025, 7);
/// Months held (at least) and the fraction of the gain excluded, for stock acquired
/// after July 4, 2025 (IRC 1202(a)(1)(B), (a)(5), (a)(6)). Earlier stock is excluded
/// in full once held more than five years, and not at all before.
pub const QSBS_TIERS: [(i32, f64); 3] = [(36, 0.5), (48, 0.75), (60, 1.0)];
/// The 90th percentile of the standard normal.
pub const Z90: f64 = 1.281_551_565_544_600_4;
/// Largest preference overhang the overhang rule accepts.
pub const OVERHANG_MAX: f64 = 0.95;
/// Seed of the standalone preview's draws ("EQUY").
pub const PREVIEW_SEED: u64 = 0x4551_5559;

/// Share counts below this are treated as none (rounding residue of pro-rata sales).
const DUST: f64 = 1e-9;

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

/// Typed view of `equity_compensation.startup_equity` in `data/actuarial/params.json`.
/// Per-stage arrays are indexed by [`CompanyStage::index`]. Build it once per plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EquityParams {
    /// Probability that a company of each stage fails within a round.
    pub p_fails: [f64; 6],
    /// Probability that it is acquired within a round.
    pub p_acquired: [f64; 6],
    /// Probability that it goes public within a round.
    pub p_ipo: [f64; 6],
    /// Probability that it is still private at the end of a round.
    pub p_still_private: [f64; 6],
    /// Median years to a failure.
    pub fails_median: [f64; 6],
    /// Median years to an acquisition.
    pub acquired_median: [f64; 6],
    /// Median years to an IPO.
    pub ipo_median: [f64; 6],
    /// Log standard deviation of the years to an outcome.
    pub timing_sigma: [f64; 6],
    /// Median acquisition price of the whole company over its last valuation.
    pub acquisition_median: [f64; 6],
    /// Log standard deviation of that multiple.
    pub acquisition_sigma: [f64; 6],
    /// Median IPO price per share over the latest preferred price.
    pub ipo_multiple_median: [f64; 6],
    /// Log standard deviation of that multiple.
    pub ipo_multiple_sigma: [f64; 6],
    /// Preferred capital raised over the latest post-money valuation.
    pub overhang: [f64; 6],
    /// 409A value of a common share over the latest preferred price, today.
    pub fmv_ratio: [f64; 6],
    /// Chance per year of a tender offer.
    pub tender_probability: [f64; 6],
    /// Length of one round of the outcome table, in months.
    pub horizon_months: i32,
    /// Rounds before a company with no event is written off.
    pub rounds: u32,
    /// Bound on the standard normal behind every price multiple and the lockup factor.
    pub z_cap: f64,
    /// Log standard deviation of the price at lockup end over the IPO price.
    pub lockup_log_sd: f64,
    /// Mean of that ratio.
    pub lockup_mean: f64,
    /// Mean of `exp(lockup_log_sd * Z)` with `Z` held within `z_cap` (derived, so the
    /// lockup factor has mean `lockup_mean` exactly).
    pub lockup_norm: f64,
    /// Share of months with the exit window open; the monthly tender chance is
    /// divided by it.
    pub window_open_share: f64,
    /// Fewest months between two tender offers.
    pub tender_min_gap_months: i32,
    /// Where the price of a company that will fail is heading, over the last round.
    pub fail_path_end_multiple: f64,
    /// The exit window is shut while US stocks are this far below their high.
    pub ipo_window_drawdown: f64,
    /// How far an IPO month may move to find an open window, in months.
    pub ipo_window_search_months: i32,
    /// Shift of the failure probability under a pessimistic or optimistic outlook.
    pub outlook_shift: f64,
    /// Bounds on the shifted failure probability.
    pub outlook_failure_bounds: [f64; 2],
    /// Yearly inflation of the standalone preview.
    pub preview_inflation: f64,
    /// Draws in the standalone preview when the caller asks for the default.
    pub preview_draws: u32,
}

fn num(root: &Value, path: &[&str]) -> f64 {
    let mut v = root;
    for key in path {
        v = &v[*key];
    }
    v.as_f64().unwrap_or_else(|| {
        panic!(
            "data/actuarial/params.json: expected a number at equity_compensation.startup_equity.{}",
            path.join(".")
        )
    })
}

fn stage_array(root: &Value, path: &[&str]) -> [f64; 6] {
    let mut v = root;
    for key in path {
        v = &v[*key];
    }
    let mut out = [0.0; 6];
    let list = v.as_array().filter(|a| a.len() == 6).unwrap_or_else(|| {
        panic!(
            "data/actuarial/params.json: expected six numbers at equity_compensation.startup_equity.{}",
            path.join(".")
        )
    });
    for (slot, x) in out.iter_mut().zip(list) {
        *slot = x.as_f64().unwrap_or_else(|| {
            panic!(
                "data/actuarial/params.json: expected a number in equity_compensation.startup_equity.{}",
                path.join(".")
            )
        });
    }
    out
}

/// Mean of `exp(s * clamp(Z, -c, c))` for a standard normal `Z`.
fn clamped_mean(s: f64, c: f64) -> f64 {
    math::exp(0.5 * s * s) * (normal_cdf(c - s) - normal_cdf(-c - s))
        + (1.0 - normal_cdf(c)) * (math::exp(s * c) + math::exp(-s * c))
}

impl EquityParams {
    /// Reads the parameters from the embedded data.
    ///
    /// # Panics
    /// If `params.json` lacks one of the expected numbers (a build error: the file is
    /// embedded and covered by this module's tests).
    pub fn from_data(data: &Data) -> EquityParams {
        let a = &data.actuarial["equity_compensation"]["startup_equity"];
        let stages: Vec<&str> = a["stages"]
            .as_array()
            .map(|v| v.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let expected: Vec<&str> = CompanyStage::ALL.iter().map(|s| s.key()).collect();
        assert_eq!(
            stages, expected,
            "data/actuarial/params.json: startup_equity.stages must list the stages in schema order"
        );
        let z_cap = num(a, &["multiple_z_cap", "value"]);
        let lockup_log_sd = num(a, &["lockup_log_sd", "value"]);
        EquityParams {
            p_fails: stage_array(a, &["outcome_probability", "fails"]),
            p_acquired: stage_array(a, &["outcome_probability", "acquired"]),
            p_ipo: stage_array(a, &["outcome_probability", "ipo"]),
            p_still_private: stage_array(a, &["outcome_probability", "still_private"]),
            fails_median: stage_array(a, &["years_to_outcome", "fails_median"]),
            acquired_median: stage_array(a, &["years_to_outcome", "acquired_median"]),
            ipo_median: stage_array(a, &["years_to_outcome", "ipo_median"]),
            timing_sigma: stage_array(a, &["years_to_outcome", "sigma"]),
            acquisition_median: stage_array(a, &["acquisition_multiple", "median"]),
            acquisition_sigma: stage_array(a, &["acquisition_multiple", "sigma"]),
            ipo_multiple_median: stage_array(a, &["ipo_multiple", "median"]),
            ipo_multiple_sigma: stage_array(a, &["ipo_multiple", "sigma"]),
            overhang: stage_array(a, &["preference_overhang", "value"]),
            fmv_ratio: stage_array(a, &["fmv_to_preferred", "value"]),
            tender_probability: stage_array(a, &["tender", "annual_probability", "value"]),
            horizon_months: (12.0 * num(a, &["horizon_years", "value"])).round() as i32,
            rounds: num(a, &["outcome_rounds", "value"]).round().max(1.0) as u32,
            z_cap,
            lockup_log_sd,
            lockup_mean: num(a, &["lockup_mean", "value"]),
            lockup_norm: clamped_mean(lockup_log_sd, z_cap),
            window_open_share: num(a, &["window_open_share", "value"]),
            tender_min_gap_months: num(a, &["tender", "min_gap_months", "value"]).round() as i32,
            fail_path_end_multiple: num(a, &["fail_path_end_multiple", "value"]),
            ipo_window_drawdown: num(a, &["ipo_window_drawdown", "value"]),
            ipo_window_search_months: num(a, &["ipo_window_search_months", "value"]).round() as i32,
            outlook_shift: num(a, &["outlook_shift", "value"]),
            outlook_failure_bounds: [
                a["outlook_failure_bounds"]["value"][0]
                    .as_f64()
                    .expect("params.json: startup_equity.outlook_failure_bounds.value[0]"),
                a["outlook_failure_bounds"]["value"][1]
                    .as_f64()
                    .expect("params.json: startup_equity.outlook_failure_bounds.value[1]"),
            ],
            preview_inflation: num(a, &["preview_inflation", "value"]),
            preview_draws: num(a, &["preview_draws", "value"]).round() as u32,
        }
    }

    /// The stage's four probabilities after the outlook shift, in the order fails,
    /// acquired, ipo, still private. They sum to 1.
    pub fn stage_probabilities(&self, stage: CompanyStage, outlook: Outlook) -> [f64; 4] {
        let s = stage.index();
        let base = [
            self.p_fails[s],
            self.p_acquired[s],
            self.p_ipo[s],
            self.p_still_private[s],
        ];
        let d = match outlook {
            Outlook::Pessimistic => self.outlook_shift,
            Outlook::Typical => return base,
            Outlook::Optimistic => -self.outlook_shift,
        };
        let [lo, hi] = self.outlook_failure_bounds;
        let fail = (base[0] + d).clamp(lo, hi);
        if base[0] >= 1.0 {
            return base;
        }
        let k = (1.0 - fail) / (1.0 - base[0]);
        [fail, base[1] * k, base[2] * k, base[3] * k]
    }

    /// Monthly chance of a tender offer in an open window for a yearly chance `p`:
    /// `min(1, (1 - (1 - p)^(1/12)) / window_open_share)`.
    pub fn tender_monthly_hazard(&self, annual_probability: f64) -> f64 {
        let p = annual_probability.clamp(0.0, 1.0);
        ((1.0 - math::pow(1.0 - p, 1.0 / 12.0)) / self.window_open_share).min(1.0)
    }
}

// ---------------------------------------------------------------------------
// The outcome table
// ---------------------------------------------------------------------------

/// When an outcome happens within a round of the table.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Timing {
    /// No date: a `still_private` row.
    #[default]
    None,
    /// Lognormal months from today, cut off at `cap_months` (the preset tables).
    LogNormal {
        /// Median of the uncut distribution, in months.
        median_months: f64,
        /// Log standard deviation.
        sigma: f64,
        /// The cut-off, in months.
        cap_months: f64,
    },
    /// Equally likely anywhere in the range (custom tables).
    Uniform {
        /// Earliest month.
        month_from: f64,
        /// Latest month.
        month_to: f64,
    },
}

/// One row of the table in force, ready to draw from.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct OutcomeModel {
    /// What happens.
    pub kind: OutcomeKind,
    /// Probability of the row within a round (the rows sum to 1).
    pub probability: f64,
    /// Probability of this row and every row before it.
    pub cumulative: f64,
    /// When it happens.
    pub timing: Timing,
    /// Mean of the log price multiple.
    pub mu: f64,
    /// Standard deviation of the log price multiple.
    pub sigma: f64,
}

/// The outcome table in force for one company: the stage preset with its outlook, or
/// the custom table, normalised.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ResolvedTable {
    /// The rows; only the first `n` are in use.
    pub rows: [OutcomeModel; MAX_OUTCOMES],
    /// Number of rows.
    pub n: usize,
    /// Length of one round of the table in months.
    pub round_months: i32,
    /// Preference overhang in force for this company, within `0..=OVERHANG_MAX`.
    pub overhang: f64,
}

impl ResolvedTable {
    /// The rows in use.
    pub fn rows(&self) -> &[OutcomeModel] {
        &self.rows[..self.n]
    }

    /// Total probability of the `still_private` rows: the chance of another round.
    pub fn still_private_probability(&self) -> f64 {
        self.rows()
            .iter()
            .filter(|r| r.kind == OutcomeKind::StillPrivate)
            .fold(0.0, |a, r| a + r.probability)
    }

    /// Total probability of the rows of one kind within a round.
    pub fn probability_of(&self, kind: OutcomeKind) -> f64 {
        self.rows()
            .iter()
            .filter(|r| r.kind == kind)
            // Folded from +0.0: the sum of no rows must not print as "-0".
            .fold(0.0, |a, r| a + r.probability)
    }
}

/// How many rounds' worth of a row's probability a long life sees:
/// `(1 - q^R) / (1 - q)`, with `q` the chance of another round.
pub fn lifetime_factor(q: f64, rounds: u32) -> f64 {
    if 1.0 - q < 1e-12 {
        f64::from(rounds)
    } else {
        (1.0 - math::powi(q, rounds as i32)) / (1.0 - q)
    }
}

/// The preference overhang in force: the company's own or the stage's typical value.
pub fn overhang_in_force(company: &PrivateCompany, params: &EquityParams) -> f64 {
    let l = company
        .preference_overhang
        .unwrap_or(params.overhang[company.stage.index()]);
    if l.is_finite() {
        l.clamp(0.0, OVERHANG_MAX)
    } else {
        params.overhang[company.stage.index()]
    }
}

/// The 409A value per share in force: the company's own or the stage's typical
/// fraction of the preferred price.
pub fn fmv_in_force(company: &PrivateCompany, params: &EquityParams) -> f64 {
    match company.fmv_409a {
        Some(v) if v.is_finite() && v >= 0.0 => v,
        _ => params.fmv_ratio[company.stage.index()] * company.preferred_price,
    }
}

/// The yearly tender chance in force: the company's own or the stage's typical value.
pub fn tender_probability_in_force(company: &PrivateCompany, params: &EquityParams) -> f64 {
    match company.tenders.annual_probability {
        Some(v) if v.is_finite() => v.clamp(0.0, 1.0),
        _ => params.tender_probability[company.stage.index()],
    }
}

/// Mean months to the outcome within a round, for a row's timing.
fn mean_months(timing: Timing, round_months: i32) -> f64 {
    let h = f64::from(round_months.max(1));
    match timing {
        Timing::None => h,
        Timing::Uniform {
            month_from,
            month_to,
        } => (0.5 * (month_from + month_to)).clamp(1.0, h),
        Timing::LogNormal {
            median_months,
            sigma,
            cap_months,
        } => {
            if sigma <= 0.0 || median_months <= 0.0 {
                return median_months.min(cap_months).clamp(1.0, h);
            }
            // Mean of a lognormal cut off at the cap.
            let a = math::ln(cap_months / median_months) / sigma;
            let mean = median_months * math::exp(0.5 * sigma * sigma) * normal_cdf(a - sigma)
                / normal_cdf(a).max(1e-300);
            mean.clamp(1.0, h)
        }
    }
}

/// Probability that a row's price multiple is at least 1: the path then heads to or
/// above the last round's price. 0 for a row with no price.
fn share_at_or_above_last_round(row: &OutcomeModel, z_cap: f64) -> f64 {
    if !matches!(row.kind, OutcomeKind::Acquired | OutcomeKind::Ipo) {
        return 0.0;
    }
    if row.sigma <= 0.0 {
        return if row.mu >= 0.0 { 1.0 } else { 0.0 };
    }
    // m >= 1 when the standard normal behind it, held within the cap, is at least z.
    let z = -row.mu / row.sigma;
    if z > z_cap {
        0.0
    } else if z <= -z_cap {
        1.0
    } else {
        1.0 - normal_cdf(z)
    }
}

/// Expected months a company spends private on a path heading to or above its last
/// round's price, and on one heading below it, over every round of the table. A
/// company that will be written off counts in neither.
pub fn tender_exposure(table: &ResolvedTable, params: &EquityParams) -> [f64; 2] {
    let h = f64::from(table.round_months.max(1));
    let q = table.still_private_probability();
    // Sums over the rounds of q^r and of r q^r.
    let (mut s0, mut s1, mut qr) = (0.0, 0.0, 1.0);
    for r in 0..params.rounds.max(1) {
        s0 += qr;
        s1 += f64::from(r) * qr;
        qr *= q;
    }
    let (mut up, mut down) = (0.0, 0.0);
    for row in table.rows() {
        if row.kind == OutcomeKind::StillPrivate || row.probability <= 0.0 {
            continue;
        }
        let months = row.probability * (s1 * h + s0 * mean_months(row.timing, table.round_months));
        let share = share_at_or_above_last_round(row, params.z_cap);
        up += share * months;
        down += (1.0 - share) * months;
    }
    [up, down]
}

/// The yearly tender chance on a path heading to or above the last round's price,
/// for a yearly chance `p` meant over all companies. Only such companies run
/// tenders (one heading below its last round, or to a write-off, runs none), so the
/// chance is `p` divided by their share of the months companies spend private:
/// tenders per private company-year then average `p` over every future except a
/// write-off. Never above 1, so a high `p` cannot be met in full.
pub fn tender_rate(table: &ResolvedTable, params: &EquityParams, p: f64) -> f64 {
    let p = if p.is_finite() {
        p.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let [up, down] = tender_exposure(table, params);
    if up > 0.0 {
        (p * (up + down) / up).min(1.0)
    } else {
        p
    }
}

/// The yearly tender chance the model delivers on average over every future for a
/// chance `p` entered: `p` itself unless [`tender_rate`] hits its cap of once a year
/// on the paths that run tenders, and then their share of the months spent private.
pub fn tender_probability_met(table: &ResolvedTable, params: &EquityParams, p: f64) -> f64 {
    let p = if p.is_finite() {
        p.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let [up, down] = tender_exposure(table, params);
    if up > 0.0 {
        p.min(up / (up + down))
    } else {
        p
    }
}

/// What common stock gets, as a multiple of the preferred price, when the whole
/// company sells for `m` times its last valuation and investors hold a 1x preference
/// over the fraction `overhang` of it: `m` at or above 1, otherwise
/// `max(0, (m - overhang) / (1 - overhang))`.
pub fn common_multiple(m: f64, overhang: f64) -> f64 {
    let l = overhang.clamp(0.0, OVERHANG_MAX);
    if m >= 1.0 {
        m
    } else {
        ((m - l) / (1.0 - l)).max(0.0)
    }
}

fn log_range(low: f64, high: f64) -> (f64, f64) {
    let lo = math::ln(low.max(1e-4));
    let hi = math::ln(high.max(low).max(1e-4));
    (0.5 * (lo + hi), (hi - lo) / (2.0 * Z90))
}

fn preset_rows(
    stage: CompanyStage,
    outlook: Outlook,
    params: &EquityParams,
) -> [OutcomeModel; MAX_OUTCOMES] {
    let s = stage.index();
    let p = params.stage_probabilities(stage, outlook);
    let cap_months = f64::from(params.horizon_months);
    let timing = |median_years: f64| Timing::LogNormal {
        median_months: 12.0 * median_years,
        sigma: params.timing_sigma[s],
        cap_months,
    };
    let mut rows = [OutcomeModel::default(); MAX_OUTCOMES];
    rows[0] = OutcomeModel {
        kind: OutcomeKind::Fails,
        probability: p[0],
        timing: timing(params.fails_median[s]),
        ..OutcomeModel::default()
    };
    rows[1] = OutcomeModel {
        kind: OutcomeKind::Acquired,
        probability: p[1],
        timing: timing(params.acquired_median[s]),
        mu: math::ln(params.acquisition_median[s]),
        sigma: params.acquisition_sigma[s],
        ..OutcomeModel::default()
    };
    rows[2] = OutcomeModel {
        kind: OutcomeKind::Ipo,
        probability: p[2],
        timing: timing(params.ipo_median[s]),
        mu: math::ln(params.ipo_multiple_median[s]),
        sigma: params.ipo_multiple_sigma[s],
        ..OutcomeModel::default()
    };
    rows[3] = OutcomeModel {
        kind: OutcomeKind::StillPrivate,
        probability: p[3],
        ..OutcomeModel::default()
    };
    rows
}

fn finish_table(
    mut rows: [OutcomeModel; MAX_OUTCOMES],
    n: usize,
    round_months: i32,
    overhang: f64,
) -> ResolvedTable {
    let mut cumulative = 0.0;
    for row in rows.iter_mut().take(n) {
        cumulative += row.probability;
        row.cumulative = cumulative;
    }
    if n > 0 {
        // The last row catches every draw, whatever the rounding of the sum.
        rows[n - 1].cumulative = 1.0;
    }
    ResolvedTable {
        rows,
        n,
        round_months,
        overhang,
    }
}

/// The stage preset with the given outlook, as a table.
pub fn preset_table(
    stage: CompanyStage,
    outlook: Outlook,
    overhang: f64,
    params: &EquityParams,
) -> ResolvedTable {
    finish_table(
        preset_rows(stage, outlook, params),
        4,
        params.horizon_months,
        overhang.clamp(0.0, OVERHANG_MAX),
    )
}

/// Whether the company's own outcome table is in force (it has a row with a positive
/// probability), not the stage preset.
pub fn custom_in_force(company: &PrivateCompany) -> bool {
    company
        .outcomes
        .iter()
        .take(MAX_OUTCOMES)
        .any(|r| r.probability.is_finite() && r.probability > 0.0)
}

/// The outcome table in force for a company: its custom table with probabilities
/// divided by their sum, or the stage preset shifted by the outlook. A custom table
/// that validation would reject (no positive probability) falls back to the preset.
pub fn resolved_table(company: &PrivateCompany, params: &EquityParams) -> ResolvedTable {
    let overhang = overhang_in_force(company, params);
    let custom = &company.outcomes[..company.outcomes.len().min(MAX_OUTCOMES)];
    let sum: f64 = custom
        .iter()
        .map(|r| r.probability)
        .filter(|p| p.is_finite() && *p > 0.0)
        .sum();
    if custom.is_empty() || sum <= 0.0 {
        return preset_table(company.stage, company.outlook, overhang, params);
    }
    let mut rows = [OutcomeModel::default(); MAX_OUTCOMES];
    let mut round_months = params.horizon_months;
    for (slot, row) in rows.iter_mut().zip(custom) {
        let probability = if row.probability.is_finite() && row.probability > 0.0 {
            row.probability / sum
        } else {
            0.0
        };
        let dated = row.kind != OutcomeKind::StillPrivate;
        let priced = matches!(row.kind, OutcomeKind::Acquired | OutcomeKind::Ipo);
        let (mu, sigma) = if priced {
            log_range(row.multiple_low, row.multiple_high)
        } else {
            (0.0, 0.0)
        };
        let timing = if dated {
            let from = (12.0 * row.years_from).round().clamp(0.0, 12_000.0);
            let to = (12.0 * row.years_to).round().clamp(from, 12_000.0);
            round_months = round_months.max(to as i32);
            Timing::Uniform {
                month_from: from,
                month_to: to,
            }
        } else {
            Timing::None
        };
        *slot = OutcomeModel {
            kind: row.kind,
            probability,
            cumulative: 0.0,
            timing,
            mu,
            sigma,
        };
    }
    finish_table(rows, custom.len(), round_months, overhang)
}

/// Quantile `q` of the years to an outcome under a cut-off lognormal.
fn timing_quantile_years(median_years: f64, sigma: f64, cap_years: f64, q: f64) -> f64 {
    let top = normal_cdf(math::ln(cap_years / median_years) / sigma);
    median_years * math::exp(sigma * inverse_normal_cdf((q * top).max(1e-300)))
}

/// The table in force as schema rows, for display: probabilities exact, and for a
/// preset the years are the 10th and 90th percentiles of its timing (rounded to half
/// years) and the multiples the 10th and 90th percentiles of its price.
pub fn table_rows(company: &PrivateCompany, params: &EquityParams) -> Vec<OutcomeRow> {
    let table = resolved_table(company, params);
    if custom_in_force(company) {
        return company
            .outcomes
            .iter()
            .zip(table.rows())
            .map(|(row, model)| OutcomeRow {
                probability: model.probability,
                ..row.clone()
            })
            .collect();
    }
    display_rows(&table, params)
}

/// The stage preset with a typical outlook as schema rows, for comparison.
pub fn typical_rows(stage: CompanyStage, params: &EquityParams) -> Vec<OutcomeRow> {
    let table = preset_table(
        stage,
        Outlook::Typical,
        params.overhang[stage.index()],
        params,
    );
    display_rows(&table, params)
}

fn display_rows(table: &ResolvedTable, params: &EquityParams) -> Vec<OutcomeRow> {
    let half = |years: f64| (years * 2.0).round() / 2.0;
    let horizon_years = f64::from(params.horizon_months) / 12.0;
    table
        .rows()
        .iter()
        .map(|m| {
            let (years_from, years_to) = match m.timing {
                Timing::LogNormal {
                    median_months,
                    sigma,
                    cap_months,
                } => (
                    half(timing_quantile_years(
                        median_months / 12.0,
                        sigma,
                        cap_months / 12.0,
                        0.10,
                    )),
                    half(timing_quantile_years(
                        median_months / 12.0,
                        sigma,
                        cap_months / 12.0,
                        0.90,
                    )),
                ),
                Timing::Uniform {
                    month_from,
                    month_to,
                } => (month_from / 12.0, month_to / 12.0),
                Timing::None => (horizon_years, horizon_years),
            };
            let priced = matches!(m.kind, OutcomeKind::Acquired | OutcomeKind::Ipo);
            let (multiple_low, multiple_high) = if priced {
                (
                    math::exp(m.mu - Z90 * m.sigma),
                    math::exp(m.mu + Z90 * m.sigma),
                )
            } else {
                (1.0, 1.0)
            };
            OutcomeRow {
                id: m.kind.key().to_string(),
                kind: m.kind,
                probability: m.probability,
                years_from,
                years_to,
                multiple_low,
                multiple_high,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// The fate
// ---------------------------------------------------------------------------

/// What happens to one company on one path, fixed at path start.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Fate {
    /// The outcome. `StillPrivate` means the company is written off at `month`.
    pub kind: OutcomeKind,
    /// Month of the outcome, in months since the start. At least 1.
    pub month: i32,
    /// Round of the table in which the outcome fell (0 is the first ten years).
    pub round: u32,
    /// Price multiple at the outcome: the whole company over its last valuation for
    /// an acquisition, the IPO price over the preferred price for an IPO.
    pub m: f64,
    /// Common-stock price at the outcome over the preferred price.
    pub x: f64,
    /// Where the price path is heading, over the preferred price.
    pub m_end: f64,
    /// Price at lockup end over the IPO price (used by an IPO only).
    pub lockup: f64,
}

/// Draws a company's fate: exactly four words from its stream, whatever the table.
pub fn draw_fate(table: &ResolvedTable, params: &EquityParams, rng: &mut Rng) -> Fate {
    let u1 = rng.uniform();
    let u2 = rng.uniform();
    let w3 = rng.next_u64();
    let w4 = rng.next_u64();
    fate_from_words(table, params, u1, u2, w3, w4)
}

/// The fate implied by four random words (two uniforms, two raw words): which row and
/// round, when, and at what price. Pure.
pub fn fate_from_words(
    table: &ResolvedTable,
    params: &EquityParams,
    u1: f64,
    u2: f64,
    w3: u64,
    w4: u64,
) -> Fate {
    let c = params.z_cap;
    let z_lock = normal_from_bits(w4).clamp(-c, c);
    let lockup = (params.lockup_mean * math::exp(params.lockup_log_sd * z_lock)
        / params.lockup_norm)
        .clamp(0.05, 20.0);
    let h = table.round_months.max(1);
    let last_round = params.rounds.max(1) - 1;
    let written_off = |round: u32| Fate {
        kind: OutcomeKind::StillPrivate,
        month: (round as i32 + 1) * h,
        round,
        m: 1.0,
        x: 0.0,
        m_end: 1.0,
        lockup,
    };
    if table.n == 0 {
        return written_off(last_round);
    }

    // Which row, and in which round. A draw that lands on "still private" is spread
    // over the table again: within the row it is uniform, so no new word is needed.
    let mut u = u1;
    let mut round = 0u32;
    let row = loop {
        let mut k = table.n - 1;
        for (i, r) in table.rows().iter().enumerate() {
            if u < r.cumulative {
                k = i;
                break;
            }
        }
        let r = table.rows[k];
        if r.kind != OutcomeKind::StillPrivate {
            break r;
        }
        if round == last_round || r.probability <= 0.0 {
            return written_off(last_round);
        }
        let below = if k == 0 {
            0.0
        } else {
            table.rows[k - 1].cumulative
        };
        u = ((u - below) / r.probability).clamp(0.0, 1.0 - f64::EPSILON);
        round += 1;
    };

    // When.
    let months = match row.timing {
        Timing::LogNormal {
            median_months,
            sigma,
            cap_months,
        } => {
            let top = normal_cdf(math::ln(cap_months / median_months) / sigma);
            let q = (u2 * top).max(1e-300);
            (median_months * math::exp(sigma * inverse_normal_cdf(q))).min(cap_months)
        }
        Timing::Uniform {
            month_from,
            month_to,
        } => month_from + u2 * (month_to - month_from),
        Timing::None => 0.0,
    };
    let within = if months.is_finite() {
        (months.round() as i32).clamp(1, h)
    } else {
        h
    };
    let month = round as i32 * h + within;

    // At what price.
    let z = normal_from_bits(w3).clamp(-c, c);
    let m = math::exp(row.mu + row.sigma * z).clamp(1e-4, 1e4);
    let (x, m_end) = match row.kind {
        OutcomeKind::Acquired => {
            let x = common_multiple(m, table.overhang);
            (x, x.max(params.fail_path_end_multiple))
        }
        OutcomeKind::Ipo => (m, m),
        _ => (0.0, params.fail_path_end_multiple),
    };
    Fate {
        kind: row.kind,
        month,
        round,
        m,
        x,
        m_end,
        lockup,
    }
}

/// Whether the exit window is open: no recession, and US stocks within
/// `ipo_window_drawdown` of their previous high. `eq_index` and `eq_peak` are the
/// stock index after this month's return and its running high (starting at 1).
#[inline]
pub fn window_open(recession: u8, eq_index: f64, eq_peak: f64, params: &EquityParams) -> bool {
    recession == 0 && eq_index >= (1.0 - params.ipo_window_drawdown) * eq_peak
}

/// Places an IPO drawn for `month` in an open window: `month` itself if the window is
/// open then, otherwise the nearest open month within `ipo_window_search_months`
/// either way (the earlier on a tie, never before month 1), otherwise `month`
/// unchanged. A month beyond the path is left alone. One scan, no allocation.
pub fn ipo_month(econ: &EconomyPath, month: i32, params: &EquityParams) -> i32 {
    if month < 1 || month as usize >= econ.months {
        return month;
    }
    let search = params.ipo_window_search_months.max(0);
    let lo = (month - search).max(1);
    let hi = ((month + search) as usize).min(econ.months - 1) as i32;
    let us = &econ.returns[AssetClass::UsStocks.index()];
    let (mut index, mut peak) = (1.0_f64, 1.0_f64);
    let mut before: Option<i32> = None;
    for t in 0..=hi {
        index *= 1.0 + us[t as usize];
        if index > peak {
            peak = index;
        }
        if t < lo || !window_open(econ.recession[t as usize], index, peak, params) {
            continue;
        }
        if t < month {
            before = Some(t);
        } else if t == month {
            return month;
        } else {
            // The first open month after; an earlier one wins a tie.
            return match before {
                Some(b) if month - b <= t - month => b,
                _ => t,
            };
        }
    }
    before.unwrap_or(month)
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// One option grant or RSU grant on a path.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoldingState {
    /// Vested and not yet settled: unexercised options, or RSU units.
    pub vested: f64,
    /// Not vested yet.
    pub unvested: f64,
    /// Units that vest each month once any cliff has passed.
    pub per_month: f64,
    /// Months of vesting left.
    pub vest_months_left: u32,
    /// Months until the cliff passes; 0 if it has.
    pub cliff_months_left: u32,
}

/// A block of shares with one tax history.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lot {
    /// Shares in the lot.
    pub shares: f64,
    /// Regular-tax basis of the whole lot, nominal dollars.
    pub cost: f64,
    /// AMT basis above the regular basis (the ISO spread at exercise), whole lot.
    pub amt_extra: f64,
    /// Month acquired relative to the start (negative: before it).
    pub acquired: i32,
    /// Grant month of the options it came from (ISO two-year rule).
    pub granted: i32,
    /// Calendar year in which `amt_extra` was reported; `i32::MIN` if none.
    pub amt_year: i32,
    /// Whether the shares came from incentive options.
    pub iso: bool,
    /// Whether the shares are qualified small business stock.
    pub qsbs: bool,
}

/// Where a company stands on a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CompanyStatus {
    /// Not sold, listed or failed yet.
    #[default]
    Private,
    /// Listed; employees may not sell yet.
    Lockup,
    /// Listed and past the lockup, with something still to vest, sell or move.
    Public,
    /// Sold; unvested units are still being paid out as they vest.
    Acquired,
    /// Failed.
    Failed,
    /// Nothing left to do: sold out, paid out, written off or abandoned.
    Done,
}

/// The holder's tie to the company.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Link {
    /// Job not started yet.
    Pending,
    /// Employed there: holdings vest.
    #[default]
    Active,
    /// Left: options lapse at `deadline`.
    Severed,
}

/// Everything about one company on one path. Fixed size, `Copy`, no heap.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CompanyState {
    /// Where the company stands.
    pub status: CompanyStatus,
    /// The holder's tie to it.
    pub link: Link,
    /// Month vested options lapse after leaving; `i32::MAX` while not severed.
    pub deadline: i32,
    /// Month the link was severed.
    pub severed_month: i32,
    /// What will happen, fixed at path start.
    pub fate: Fate,
    /// Share price in the month of the last step, nominal dollars.
    pub price: f64,
    /// One plus the single-stock return of the month of the last step, applied to the
    /// price of a listed company at the next step; 1 otherwise.
    pub price_growth: f64,
    /// Month the terminal event happened; `i32::MAX` if not yet.
    pub event_month: i32,
    /// Month the lockup ends.
    pub lockup_end: i32,
    /// Yearly sales still to make (`SellDown::OverYears`).
    pub sales_left: u32,
    /// Month of the next yearly sale.
    pub next_sale: i32,
    /// Month of the last tender; `i32::MIN / 2` if none.
    pub last_tender: i32,
    /// Cumulative gain excluded as qualified small business stock, nominal dollars.
    pub qsbs_excluded: f64,
    /// Set at the terminal event.
    pub outcome: Option<EquityOutcome>,
    /// Option and RSU grants, in profile order (a `Shares` holding keeps an empty slot).
    pub holdings: [HoldingState; MAX_HOLDINGS],
    /// Share lots; only the first `n_lots` are in use.
    pub lots: [Lot; MAX_LOTS],
    /// Number of lots in use.
    pub n_lots: usize,
}

impl CompanyState {
    /// The lots in use.
    pub fn lots(&self) -> &[Lot] {
        &self.lots[..self.n_lots]
    }

    /// Shares held in lots.
    pub fn lot_shares(&self) -> f64 {
        self.lots().iter().map(|l| l.shares).sum()
    }

    /// Regular-tax basis of all lots.
    pub fn lot_basis(&self) -> f64 {
        self.lots().iter().map(|l| l.cost).sum()
    }

    /// Every unit in the state: lot shares, vested and unvested options and RSUs.
    pub fn total_units(&self) -> f64 {
        self.lot_shares()
            + self
                .holdings
                .iter()
                .map(|h| h.vested + h.unvested)
                .sum::<f64>()
    }

    fn remove_lot(&mut self, index: usize) {
        for i in index..self.n_lots - 1 {
            self.lots[i] = self.lots[i + 1];
        }
        self.n_lots -= 1;
        self.lots[self.n_lots] = Lot::default();
    }

    /// Index of the lot acquired first (the lower slot on a tie).
    fn oldest_lot(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, lot) in self.lots().iter().enumerate() {
            if !best.is_some_and(|b| lot.acquired >= self.lots[b].acquired) {
                best = Some(i);
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// The step interface
// ---------------------------------------------------------------------------

/// What the company's step needs to know about the month. Built by the simulation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EquityClimate {
    /// Month since the start.
    pub t: i32,
    /// Calendar year.
    pub year: i32,
    /// The linked job is employed or on a career break this month.
    pub employed: bool,
    /// The linked job has begun.
    pub job_started: bool,
    /// The linked job has ended, or will never start.
    pub job_ended: bool,
    /// Laid off from the linked job since the last equity step.
    pub left_job: bool,
    /// The owner died this month and a survivor is present.
    pub owner_died: bool,
    /// The exit window is open ([`window_open`]).
    pub window_open: bool,
    /// This month's word from the company's stream, as a uniform.
    pub u_tender: f64,
    /// This month's return of the single-stock asset.
    pub single_stock_return: f64,
    /// Cash and taxable savings the household can spend on an exercise this month
    /// (above its cash reserve and tax owed), nominal.
    pub liquid: f64,
    /// Tax per dollar of NSO spread exercised this month, everything included: what
    /// the spread adds to the year's return (income tax, federal and state, and
    /// payroll tax), never less than `wage_withheld_rate`. An exercise is affordable
    /// when savings cover the strike and this.
    pub wage_tax_rate: f64,
    /// The part of `wage_tax_rate` taken at the event: payroll tax and withholding.
    /// The rest is set aside (`EquityMonth::tax_estimate`) until it is paid.
    pub wage_withheld_rate: f64,
    /// Estimate of the AMT per dollar of ISO spread (affordability and reserve).
    pub iso_tax_rate: f64,
}

/// A kind of event a company can log in a month.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EqEventKind {
    /// Unvested units were forfeited when the link to the job ended, and with them
    /// vested RSUs that require employment if the company was still private.
    #[default]
    Forfeited,
    /// Vested options lapsed unexercised when the exercise window after leaving closed.
    Lapsed,
    /// Options, vested and unvested, lapsed at the grant's ten-year term.
    Expired,
    /// Options were exercised for cash.
    Exercised,
    /// The company ran a tender offer.
    Tender,
    /// The company was acquired.
    Acquired,
    /// The company went public.
    Ipo,
    /// The company failed.
    Failed,
    /// The company was written off after the last round without an event.
    WrittenOff,
    /// Shares, options or RSUs were sold at lockup end or on the sale schedule.
    LockupSale,
    /// Shares were moved to the single-stock sleeve.
    LockupKept,
}

/// One logged event: totals over the company's holdings and lots for the month.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct EqEvent {
    /// What happened.
    pub kind: EqEventKind,
    /// Options, units or shares involved.
    pub units: f64,
    /// The part of `units` that had vested (kept for `Forfeited`, `Lapsed` and
    /// `Expired`, where the two are told apart; 0 for the other kinds).
    pub vested: f64,
    /// Price per unit, nominal dollars (for an exercise: cost over units).
    pub price: f64,
    /// Cash paid or received, the value moved or given up, or the cost lost.
    pub amount: f64,
}

/// One company's flows for one month, nominal dollars. `Copy`, no heap.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct EquityMonth {
    /// Strike paid in cash.
    pub exercise_cost: f64,
    /// Wage-type income received in cash (FICA applies).
    pub wages_cash: f64,
    /// NSO spread on exercise-and-hold (FICA applies, no cash).
    pub wages_noncash: f64,
    /// ISO disqualifying-disposition income (no FICA); in cash.
    pub iso_income: f64,
    /// Cash from selling share lots (includes any `iso_income` from lots).
    pub share_proceeds: f64,
    /// All cash received: `wages_cash`, ISO income from options and `share_proceeds`.
    pub cash_in: f64,
    /// Short-term capital gain; may be negative.
    pub short_term_gain: f64,
    /// Long-term capital gain; may be negative.
    pub long_term_gain: f64,
    /// Change to ordinary AMT income; signed.
    pub amt_adjustment: f64,
    /// Change to the gain taxed at capital-gains rates in the AMT; never positive.
    pub amt_gain_adjustment: f64,
    /// Gain excluded as qualified small business stock (federal).
    pub qsbs_excluded: f64,
    /// Tax on this month's exercises that is not taken at the event, to be set
    /// aside: `iso_tax_rate` times the ISO spread exercised and held, and for an NSO
    /// `wage_tax_rate` less `wage_withheld_rate`, times its spread.
    pub tax_estimate: f64,
    /// Value of shares moved to the single-stock sleeve.
    pub to_sleeve_value: f64,
    /// Basis of those shares.
    pub to_sleeve_basis: f64,
    /// The part of `cash_in` that came from a tender offer.
    pub tender_proceeds: f64,
    /// Basis added to the lots this month.
    pub basis_in: f64,
    /// Basis that left the lots this month.
    pub basis_out: f64,
    /// Units that left the state this month by any route: sold, cashed out, moved to
    /// the sleeve, forfeited, lapsed, cancelled or lost. An exercise moves no unit out.
    pub units_out: f64,
    /// Month-end value of vested holdings at the month's price, before tax.
    pub paper_value: f64,
    /// The company failed this month (drives the job); not set by a write-off.
    pub failed_now: bool,
    /// A taxable event that warrants re-planning the year.
    pub replan: bool,
    /// Events of the month; only the first `n_events` are in use.
    pub events: [EqEvent; MAX_EVENTS],
    /// Number of events.
    pub n_events: usize,
}

impl EquityMonth {
    /// Logs an event, adding to an entry of the same kind if the month has one. An
    /// exercise keeps `price = amount / units`; other kinds keep the given price.
    pub fn push(&mut self, kind: EqEventKind, units: f64, price: f64, amount: f64) {
        self.push_split(kind, units, 0.0, price, amount);
    }

    /// [`EquityMonth::push`] for an event that tells vested units from unvested ones:
    /// `vested` is the part of `units` that had vested.
    pub fn push_split(
        &mut self,
        kind: EqEventKind,
        units: f64,
        vested: f64,
        price: f64,
        amount: f64,
    ) {
        for e in &mut self.events[..self.n_events] {
            if e.kind == kind {
                e.units += units;
                e.vested += vested;
                e.amount += amount;
                e.price = if kind == EqEventKind::Exercised {
                    if e.units > 0.0 {
                        e.amount / e.units
                    } else {
                        0.0
                    }
                } else {
                    price
                };
                return;
            }
        }
        debug_assert!(self.n_events < MAX_EVENTS);
        if self.n_events < MAX_EVENTS {
            self.events[self.n_events] = EqEvent {
                kind,
                units,
                vested,
                price,
                amount,
            };
            self.n_events += 1;
        }
    }

    /// The events of the month.
    pub fn events(&self) -> &[EqEvent] {
        &self.events[..self.n_events]
    }

    /// The event of one kind, if the month has one.
    pub fn event(&self, kind: EqEventKind) -> Option<EqEvent> {
        self.events().iter().copied().find(|e| e.kind == kind)
    }
}

// ---------------------------------------------------------------------------
// The compiled company
// ---------------------------------------------------------------------------

/// What a holding slot is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HoldingClass {
    /// Shares already owned: a lot from the start, the slot stays empty.
    #[default]
    Shares,
    /// Incentive options.
    Iso,
    /// Non-qualified options.
    Nso,
    /// Double-trigger RSUs.
    Rsu,
}

/// One holding, compiled.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HoldingModel {
    /// What it is.
    pub class: HoldingClass,
    /// Strike per share (options).
    pub strike: f64,
    /// Grant month relative to the start (negative).
    pub granted: i32,
    /// Month the options reach their ten-year term.
    pub term_month: i32,
    /// RSUs: vested units are lost on leaving before an event.
    pub must_be_employed: bool,
    /// State at the start.
    pub initial: HoldingState,
}

impl HoldingModel {
    fn is_option(&self) -> bool {
        matches!(self.class, HoldingClass::Iso | HoldingClass::Nso)
    }
}

fn initial_holding(h: &EquityHolding) -> HoldingState {
    if h.kind.is_shares() {
        return HoldingState::default();
    }
    let shares = if h.shares.is_finite() {
        h.shares.max(0.0)
    } else {
        0.0
    };
    let vested = if h.vested.is_finite() {
        h.vested.clamp(0.0, shares)
    } else {
        0.0
    };
    let unvested = shares - vested;
    HoldingState {
        vested,
        unvested,
        per_month: unvested / f64::from(h.vest_months_left.max(1)),
        vest_months_left: h.vest_months_left,
        cliff_months_left: h.cliff_months.min(h.vest_months_left),
    }
}

/// Units of a holding that have vested once the step of month `t` has run, for
/// someone who stays: the arithmetic of [`CompanyModel::step`] in closed form.
pub fn units_vested_by(h: &EquityHolding, t: i32) -> f64 {
    if h.kind.is_shares() {
        return h.shares.max(0.0);
    }
    let init = initial_holding(h);
    let steps = i64::from(t.max(-1)) + 1;
    if init.vest_months_left == 0 || steps < i64::from(init.cliff_months_left) {
        init.vested
    } else if steps >= i64::from(init.vest_months_left) {
        init.vested + init.unvested
    } else {
        init.vested + init.per_month * steps as f64
    }
}

/// Dollar amount of an incentive-option grant (shares times strike) that becomes
/// exercisable per year, measured over the whole grant and its vesting span (at most
/// four years, at least one). 0 for other holdings. Compared with
/// [`ISO_ANNUAL_LIMIT`] by validation.
pub fn iso_yearly_amount(h: &EquityHolding) -> f64 {
    match h.kind {
        HoldingKind::Iso {
            strike,
            granted_months_ago,
        } => {
            let span_months = granted_months_ago
                .saturating_add(h.vest_months_left)
                .min(48);
            let span_years = (f64::from(span_months) / 12.0).max(1.0);
            strike * h.shares / span_years
        }
        _ => 0.0,
    }
}

/// Value of a company's holdings at the latest preferred price: shares and RSUs at
/// the price, options at the price less the strike (not below 0). Returns the vested
/// part and the total.
pub fn position_value(company: &PrivateCompany) -> (f64, f64) {
    let p0 = company.preferred_price;
    let (mut vested, mut total) = (0.0, 0.0);
    for h in &company.holdings {
        let each = match h.kind.strike() {
            Some(k) => (p0 - k).max(0.0),
            None => p0,
        };
        let v = if h.kind.is_shares() {
            h.shares
        } else {
            h.vested.clamp(0.0, h.shares.max(0.0))
        };
        vested += v * each;
        total += h.shares * each;
    }
    (vested, total)
}

/// The company the outcome statistics describe: the one with the largest position
/// at the latest preferred price, vested and unvested, included or not. Ties go to
/// the first. 0 for an empty list.
pub fn primary_company(companies: &[PrivateCompany]) -> usize {
    let mut best = 0;
    let mut best_value = f64::NEG_INFINITY;
    for (i, company) in companies.iter().enumerate() {
        let value = position_value(company).1;
        if value > best_value {
            best = i;
            best_value = value;
        }
    }
    best
}

/// One company compiled for the month loop: facts, table, policy and the holdings'
/// starting state. `Copy`, no heap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompanyModel {
    /// The parameters it was compiled with.
    pub params: EquityParams,
    /// The outcome table in force.
    pub table: ResolvedTable,
    /// Whether the holdings count. When false there are no holdings and no lots, and
    /// the step logs no event except a failure.
    pub included: bool,
    /// Whether the company is linked to a job.
    pub has_job: bool,
    /// Latest preferred price per share.
    pub p0: f64,
    /// 409A value over the preferred price, today.
    pub fmv_ratio: f64,
    /// Monthly chance of a tender offer in an open window, on a path heading to or
    /// above the last round's price ([`tender_rate`]). Other paths have none.
    pub tender_hazard: f64,
    /// Month of an announced tender; `i32::MIN` if none.
    pub tender_planned: i32,
    /// Largest fraction of vested holdings that may be sold in one tender.
    pub tender_max_fraction: f64,
    /// Tender price as a fraction of the share price.
    pub tender_price_fraction: f64,
    /// Whether people who have left may sell in a tender.
    pub tender_former_employees: bool,
    /// Share of the allowed amount the household sells in a tender.
    pub tender_sell_fraction: f64,
    /// Months of lockup after an IPO.
    pub lockup_months: i32,
    /// Months to exercise after leaving.
    pub exercise_window_months: i32,
    /// When options are exercised.
    pub exercise: ExerciseTiming,
    /// What happens to options about to lapse.
    pub at_deadline: DeadlinePolicy,
    /// What happens to shares after the lockup.
    pub sell_down: SellDown,
    /// Month of a planned departure; `i32::MAX` if none.
    pub leave_month: i32,
    /// Whether the shares are qualified small business stock.
    pub qsbs: bool,
    /// Per-issuer cap on excluded gain: stock acquired after July 4, 2025, and before.
    pub qsbs_caps: [f64; 2],
    /// The month the plan starts.
    pub start: YearMonth,
    /// The holdings, in profile order.
    pub holdings: [HoldingModel; MAX_HOLDINGS],
    /// Number of holdings.
    pub n_holdings: usize,
    /// Lots held at the start (from `Shares` holdings).
    pub lots: [Lot; MAX_HOLDINGS],
    /// Number of starting lots.
    pub n_lots: usize,
}

impl CompanyModel {
    /// Compiles one company. `qsbs_caps` is `[FederalYear::qsbs_cap,
    /// FederalYear::qsbs_cap_old]`; `start` is the plan's first month. Holdings beyond
    /// [`MAX_HOLDINGS`] are ignored (validation rejects such a profile).
    pub fn compile(
        company: &PrivateCompany,
        params: &EquityParams,
        qsbs_caps: [f64; 2],
        start: YearMonth,
    ) -> CompanyModel {
        let p0 = if company.preferred_price.is_finite() {
            company.preferred_price.max(0.0)
        } else {
            0.0
        };
        let fmv_ratio = if p0 > 0.0 {
            fmv_in_force(company, params) / p0
        } else {
            params.fmv_ratio[company.stage.index()]
        };
        let mut holdings = [HoldingModel::default(); MAX_HOLDINGS];
        let mut lots = [Lot::default(); MAX_HOLDINGS];
        let (mut n_holdings, mut n_lots) = (0, 0);
        if company.included {
            for h in company.holdings.iter().take(MAX_HOLDINGS) {
                let slot = &mut holdings[n_holdings];
                n_holdings += 1;
                match h.kind {
                    HoldingKind::Iso {
                        strike,
                        granted_months_ago,
                    }
                    | HoldingKind::Nso {
                        strike,
                        granted_months_ago,
                    } => {
                        let ago = granted_months_ago.min(1_200) as i32;
                        *slot = HoldingModel {
                            class: if matches!(h.kind, HoldingKind::Iso { .. }) {
                                HoldingClass::Iso
                            } else {
                                HoldingClass::Nso
                            },
                            strike: strike.max(0.0),
                            granted: -ago,
                            term_month: OPTION_TERM_MONTHS - ago,
                            must_be_employed: false,
                            initial: initial_holding(h),
                        };
                    }
                    HoldingKind::Rsu {
                        must_be_employed_at_event,
                    } => {
                        *slot = HoldingModel {
                            class: HoldingClass::Rsu,
                            strike: 0.0,
                            granted: 0,
                            term_month: i32::MAX,
                            must_be_employed: must_be_employed_at_event,
                            initial: initial_holding(h),
                        };
                    }
                    HoldingKind::Shares {
                        basis_per_share,
                        months_held,
                        from_iso,
                        fmv_at_exercise,
                    } => {
                        let shares = if h.shares.is_finite() {
                            h.shares.max(0.0)
                        } else {
                            0.0
                        };
                        if shares <= 0.0 {
                            continue;
                        }
                        let held = months_held.min(1_200) as i32;
                        let amt_extra = if from_iso {
                            shares * (fmv_at_exercise - basis_per_share).max(0.0)
                        } else {
                            0.0
                        };
                        // Exercised earlier in the start's calendar year: the spread
                        // belongs to this year's AMT income.
                        let this_year =
                            from_iso && amt_extra > 0.0 && (held as u32) < start.month();
                        lots[n_lots] = Lot {
                            shares,
                            cost: shares * basis_per_share.max(0.0),
                            amt_extra,
                            acquired: -held,
                            granted: -held - 12,
                            amt_year: if this_year { start.year() } else { i32::MIN },
                            iso: from_iso,
                            qsbs: company.qsbs,
                        };
                        n_lots += 1;
                    }
                }
            }
        }
        let table = resolved_table(company, params);
        let tender_yearly =
            tender_rate(&table, params, tender_probability_in_force(company, params));
        CompanyModel {
            params: *params,
            table,
            included: company.included,
            has_job: company.job.is_some(),
            p0,
            fmv_ratio,
            tender_hazard: params.tender_monthly_hazard(tender_yearly),
            tender_planned: company
                .tenders
                .next_in_months
                .map_or(i32::MIN, |m| m.min(100_000) as i32),
            tender_max_fraction: company.tenders.max_fraction.clamp(0.0, 1.0),
            tender_price_fraction: company.tenders.price_fraction.max(0.0),
            tender_former_employees: company.tenders.former_employees,
            tender_sell_fraction: company.policy.tender_sell_fraction.clamp(0.0, 1.0),
            lockup_months: company.lockup_months.min(1_200) as i32,
            exercise_window_months: company.exercise_window_months.min(1_200) as i32,
            exercise: company.policy.exercise,
            at_deadline: company.policy.at_deadline,
            sell_down: company.policy.sell_down,
            leave_month: company
                .policy
                .leave_after_months
                .map_or(i32::MAX, |m| m.min(100_000) as i32),
            qsbs: company.qsbs,
            qsbs_caps,
            start,
            holdings,
            n_holdings,
            lots,
            n_lots,
        }
    }

    /// The state at the start of a path, before the fate is drawn. `start_link` says
    /// whether the linked job has begun (`Active`) or starts later (`Pending`); it is
    /// ignored for a company with no job, which starts `Severed` with the exercise
    /// window running. A window over three months marks the link as severed long ago,
    /// so incentive options of a former employer are taxed as non-qualified ones.
    pub fn initial_state(&self, start_link: Link) -> CompanyState {
        let mut st = CompanyState {
            status: CompanyStatus::Private,
            link: start_link,
            deadline: i32::MAX,
            severed_month: i32::MAX,
            price: self.p0,
            price_growth: 1.0,
            event_month: i32::MAX,
            lockup_end: i32::MAX,
            next_sale: i32::MAX,
            last_tender: i32::MIN / 2,
            ..CompanyState::default()
        };
        if !self.has_job {
            st.link = Link::Severed;
            st.deadline = self.exercise_window_months;
            st.severed_month = if self.exercise_window_months <= ISO_POST_TERMINATION_MONTHS as i32
            {
                0
            } else {
                -(ISO_POST_TERMINATION_MONTHS as i32 + 1)
            };
        }
        for (slot, h) in st
            .holdings
            .iter_mut()
            .zip(&self.holdings[..self.n_holdings])
        {
            *slot = h.initial;
        }
        st.lots[..self.n_lots].copy_from_slice(&self.lots[..self.n_lots]);
        st.n_lots = self.n_lots;
        st
    }

    /// The state at path start with the fate drawn: [`CompanyModel::initial_state`],
    /// then exactly four words from the company's stream ([`draw_fate`]), then the
    /// IPO month placed in an open window when an economy path is given.
    pub fn begin(
        &self,
        start_link: Link,
        rng: &mut Rng,
        econ: Option<&EconomyPath>,
    ) -> CompanyState {
        let mut st = self.initial_state(start_link);
        st.fate = draw_fate(&self.table, &self.params, rng);
        if let (OutcomeKind::Ipo, Some(econ)) = (st.fate.kind, econ) {
            st.fate.month = ipo_month(econ, st.fate.month, &self.params);
        }
        st
    }

    /// ISO spread of shares exercised earlier in the start's calendar year: the
    /// simulation adds it to the first year's AMT adjustment.
    pub fn start_amt_adjustment(&self) -> f64 {
        self.lots[..self.n_lots]
            .iter()
            .filter(|l| l.amt_year == self.start.year())
            .map(|l| l.amt_extra)
            .sum()
    }

    /// Share price in month `t` while the company is private: a straight line in logs
    /// from the preferred price to where the fate is heading.
    pub fn price_at(&self, fate: &Fate, t: i32) -> f64 {
        let frac = (f64::from(t) / f64::from(fate.month.max(1))).clamp(0.0, 1.0);
        self.p0 * math::exp(frac * math::ln(fate.m_end))
    }

    /// 409A value over the share price in month `t` while private.
    pub fn fmv_ratio_at(&self, fate: &Fate, t: i32) -> f64 {
        let frac = (f64::from(t) / f64::from(fate.month.max(1))).clamp(0.0, 1.0);
        let end = match fate.kind {
            OutcomeKind::Ipo => 1.0,
            OutcomeKind::Acquired => (fate.x / fate.m_end).min(1.0),
            _ => self.fmv_ratio,
        };
        self.fmv_ratio + (end - self.fmv_ratio) * frac
    }

    /// Value of a common share for exercise tax in month `t` while private.
    pub fn fmv_at(&self, fate: &Fate, t: i32) -> f64 {
        self.fmv_ratio_at(fate, t) * self.price_at(fate, t)
    }

    /// Share price at lockup end.
    pub fn lockup_end_price(&self, fate: &Fate) -> f64 {
        if self.lockup_months == 0 {
            self.p0 * fate.m
        } else {
            self.p0 * fate.m * fate.lockup
        }
    }

    fn price_now(&self, st: &CompanyState, t: i32) -> f64 {
        match st.status {
            CompanyStatus::Private => self.price_at(&st.fate, t),
            CompanyStatus::Lockup => {
                let n = self.lockup_months.max(1);
                let k = (t - st.event_month).clamp(0, n);
                if k == n {
                    self.lockup_end_price(&st.fate)
                } else {
                    self.p0
                        * st.fate.m
                        * math::exp(f64::from(k) / f64::from(n) * math::ln(st.fate.lockup))
                }
            }
            CompanyStatus::Public => st.price * st.price_growth,
            CompanyStatus::Acquired => st.price,
            CompanyStatus::Failed | CompanyStatus::Done => 0.0,
        }
    }

    /// Whether an incentive option exercised or sold in month `t` still counts as
    /// one: not when the link was severed more than three months before.
    fn iso_treatment(&self, st: &CompanyState, hi: usize, t: i32) -> bool {
        self.holdings[hi].class == HoldingClass::Iso
            && !(st.link == Link::Severed
                && i64::from(t) - i64::from(st.severed_month)
                    > i64::from(ISO_POST_TERMINATION_MONTHS))
    }

    /// Spread (409A value less strike) of the options that the step of month `t`
    /// may exercise for cash and that would be taxed as pay: the amount the
    /// simulation prices the tax on before the step ([`EquityClimate::wage_tax_rate`]).
    /// An upper bound on what the step exercises that way; 0 in a month with no
    /// such exercise, which is nearly every month.
    pub fn pay_spread_at_stake(&self, st: &CompanyState, t: i32) -> f64 {
        if !self.included || st.status != CompanyStatus::Private {
            return 0.0;
        }
        let now = t == 0 && self.exercise != ExerciseTiming::AtLiquidity;
        let as_vesting = t > 0 && self.exercise == ExerciseTiming::AsTheyVest;
        // The window's end: reached, or reached this month by a link that is cut
        // this month with no window at all.
        let window_due = if st.link == Link::Severed {
            t >= st.deadline
        } else {
            self.exercise_window_months == 0
        };
        let at_deadline = self.at_deadline == DeadlinePolicy::Exercise;
        // Priced only when something is at stake: this runs every month.
        let mut fmv = f64::NAN;
        let mut spread = 0.0;
        for hi in 0..self.n_holdings {
            let m = &self.holdings[hi];
            if !m.is_option() || m.strike >= self.p0 || self.iso_treatment(st, hi, t) {
                continue;
            }
            let h = &st.holdings[hi];
            // What vests in this month's step, for a link that is not cut.
            let waits =
                st.link == Link::Severed || h.vest_months_left == 0 || h.cliff_months_left > 1;
            let vesting = if waits {
                0.0
            } else {
                (h.unvested - h.per_month * f64::from(h.vest_months_left - 1)).max(0.0)
            };
            let units = if now || (at_deadline && (window_due || t >= m.term_month)) {
                h.vested + vesting
            } else if as_vesting {
                vesting
            } else {
                0.0
            };
            if units <= 0.0 {
                continue;
            }
            if fmv.is_nan() {
                fmv = self.fmv_at(&st.fate, t);
            }
            spread += units * (fmv - m.strike).max(0.0);
        }
        spread
    }

    fn sever(&self, st: &mut CompanyState, t: i32, price: f64, em: &mut EquityMonth) {
        st.link = Link::Severed;
        st.severed_month = t;
        st.deadline = t.saturating_add(self.exercise_window_months);
        // Vested RSUs that require employment are lost only on leaving before the
        // company is sold or listed. Once it has listed the event has happened: the
        // units wait for the lockup's end and are paid then, to whoever holds them.
        let rsus_at_risk = st.status == CompanyStatus::Private;
        let (mut units, mut vested, mut value) = (0.0, 0.0, 0.0);
        for (h, m) in st
            .holdings
            .iter_mut()
            .zip(&self.holdings[..self.n_holdings])
        {
            let each = if m.is_option() {
                (price - m.strike).max(0.0)
            } else {
                price
            };
            let mut lost = h.unvested;
            h.unvested = 0.0;
            h.vest_months_left = 0;
            h.cliff_months_left = 0;
            if m.class == HoldingClass::Rsu && m.must_be_employed && rsus_at_risk {
                lost += h.vested;
                vested += h.vested;
                h.vested = 0.0;
            }
            units += lost;
            value += lost * each;
        }
        if units > 0.0 {
            em.units_out += units;
            self.log_split(em, EqEventKind::Forfeited, units, vested, price, value);
        }
    }

    fn log(&self, em: &mut EquityMonth, kind: EqEventKind, units: f64, price: f64, amount: f64) {
        self.log_split(em, kind, units, 0.0, price, amount);
    }

    fn log_split(
        &self,
        em: &mut EquityMonth,
        kind: EqEventKind,
        units: f64,
        vested: f64,
        price: f64,
        amount: f64,
    ) {
        if self.included || kind == EqEventKind::Failed {
            em.push_split(kind, units, vested, price, amount);
        }
    }

    /// Adds an exercised lot, pooling by calendar year (4.4 of the design).
    fn add_lot(&self, st: &mut CompanyState, new: Lot) {
        let year_of = |acquired: i32| self.start.add_months(acquired).year();
        for lot in &mut st.lots[..st.n_lots] {
            if lot.iso == new.iso
                && lot.qsbs == new.qsbs
                && lot.amt_year == new.amt_year
                && year_of(lot.acquired) == year_of(new.acquired)
            {
                lot.shares += new.shares;
                lot.cost += new.cost;
                lot.amt_extra += new.amt_extra;
                lot.acquired = lot.acquired.max(new.acquired);
                lot.granted = lot.granted.max(new.granted);
                return;
            }
        }
        if st.n_lots == MAX_LOTS {
            merge_two_oldest(st, new.iso);
        }
        st.lots[st.n_lots] = new;
        st.n_lots += 1;
    }

    /// The exercise rule: exercises up to `want` vested options of holding `hi` for
    /// cash, as far as `liquid` covers the strike and the tax the exercise causes.
    /// Returns the options exercised.
    #[allow(clippy::too_many_arguments)]
    fn exercise(
        &self,
        st: &mut CompanyState,
        hi: usize,
        want: f64,
        fmv: f64,
        liquid: &mut f64,
        c: &EquityClimate,
        em: &mut EquityMonth,
    ) -> f64 {
        let m = &self.holdings[hi];
        let k = m.strike;
        // The household compares the strike with the last round's price, the last
        // price it has seen: never with the path, which knows the outcome.
        if !m.is_option() || want <= DUST || k >= self.p0 {
            return 0.0;
        }
        let iso = self.iso_treatment(st, hi, c.t);
        let spread = (fmv - k).max(0.0);
        let rate = if iso { c.iso_tax_rate } else { c.wage_tax_rate }.max(0.0);
        let each = k + rate * spread;
        let needed = want * each;
        let n = if needed <= *liquid || needed <= 0.0 {
            want
        } else {
            want * (*liquid / needed)
        };
        if n <= DUST {
            return 0.0;
        }
        *liquid = (*liquid - n * each).max(0.0);
        st.holdings[hi].vested -= n;
        if st.holdings[hi].vested < DUST {
            st.holdings[hi].vested = 0.0;
        }
        let lot = if iso {
            em.amt_adjustment += n * spread;
            em.tax_estimate += rate * n * spread;
            Lot {
                shares: n,
                cost: n * k,
                amt_extra: n * spread,
                acquired: c.t,
                granted: m.granted,
                amt_year: c.year,
                iso: true,
                qsbs: self.qsbs,
            }
        } else {
            em.wages_noncash += n * spread;
            // Withholding is not the tax: the rest of what the spread adds to the
            // return is set aside now, like the minimum tax on an incentive option.
            em.tax_estimate += (rate - c.wage_withheld_rate.max(0.0)).max(0.0) * n * spread;
            Lot {
                shares: n,
                cost: n * fmv.max(k),
                amt_extra: 0.0,
                acquired: c.t,
                granted: m.granted,
                amt_year: i32::MIN,
                iso: false,
                qsbs: self.qsbs,
            }
        };
        em.exercise_cost += n * k;
        em.basis_in += lot.cost;
        self.add_lot(st, lot);
        self.log(em, EqEventKind::Exercised, n, k, n * k);
        n
    }

    /// The lot-sale rule: sells `s` shares of lot `li` at `price`.
    fn sell_from_lot(
        &self,
        st: &mut CompanyState,
        li: usize,
        s: f64,
        price: f64,
        c: &EquityClimate,
        em: &mut EquityMonth,
    ) {
        let lot = st.lots[li];
        let part = if lot.shares > 0.0 {
            (s / lot.shares).min(1.0)
        } else {
            1.0
        };
        let proceeds = s * price;
        let basis = lot.cost * part;
        let extra = lot.amt_extra * part;
        let held = c.t - lot.acquired;
        let long = held > LONG_TERM_MONTHS;
        let qualifying = !lot.iso
            || (held > ISO_HOLD_FROM_EXERCISE_MONTHS
                && c.t - lot.granted > ISO_HOLD_FROM_GRANT_MONTHS);
        let mut gain = proceeds - basis;
        if lot.iso && !qualifying {
            // Disqualifying disposition: the spread at exercise, or the gain if less,
            // is ordinary income.
            let ordinary = extra.min(proceeds - basis).max(0.0);
            em.iso_income += ordinary;
            gain -= ordinary;
        }
        // The spread counted for the AMT at exercise comes back at the sale. In the
        // year of exercise the adjustment is simply undone. Later, the AMT measures
        // the gain from the value at exercise: `release` less gain than the regular
        // tax sees (all of the gain if it is smaller than the spread).
        let mut release = 0.0;
        if extra > 0.0 {
            if lot.amt_year == c.year {
                em.amt_adjustment -= extra;
            } else {
                release = extra.min((proceeds - basis).max(0.0));
                if !qualifying {
                    em.amt_adjustment -= release;
                    release = 0.0;
                }
            }
        }
        // The gain as the AMT sees it (a qualifying sale in a later year).
        let mut amt_gain = gain - release;
        if lot.qsbs && gain > 0.0 {
            let (fraction, cap) = self.qsbs_exclusion(st, &lot, c.t);
            // Section 1202 applies to each tax's own gain, under one dollar cap.
            let excluded = (fraction * gain).min(cap);
            amt_gain -= (fraction * amt_gain.max(0.0)).min(cap);
            gain -= excluded;
            em.qsbs_excluded += excluded;
            st.qsbs_excluded += excluded;
        }
        if release > 0.0 {
            // The whole spread when nothing is excluded or the cap binds, the part
            // not excluded otherwise, nothing when the gain is excluded in full.
            em.amt_gain_adjustment += (amt_gain - gain).min(0.0);
        }
        if long {
            em.long_term_gain += gain;
        } else {
            em.short_term_gain += gain;
        }
        em.share_proceeds += proceeds;
        em.cash_in += proceeds;
        em.basis_out += basis;
        em.units_out += s;
        let lot = &mut st.lots[li];
        lot.shares -= s;
        lot.cost -= basis;
        lot.amt_extra -= extra;
    }

    /// The fraction of a qualified small business stock lot's gain that section 1202
    /// excludes when sold in month `t`, and the dollar cap still unused for it.
    /// `(0, 0)` for a lot that is not such stock.
    fn qsbs_exclusion(&self, st: &CompanyState, lot: &Lot, t: i32) -> (f64, f64) {
        if !lot.qsbs {
            return (0.0, 0.0);
        }
        let held = t - lot.acquired;
        let new_regime = self.start.add_months(lot.acquired) >= QSBS_NEW_REGIME_FROM;
        let fraction = if new_regime {
            let mut f = 0.0;
            for (months, share) in QSBS_TIERS {
                if held >= months {
                    f = share;
                }
            }
            f
        } else if held > QSBS_TIERS[2].0 {
            1.0
        } else {
            0.0
        };
        let cap = self.qsbs_caps[usize::from(!new_regime)] - st.qsbs_excluded;
        (fraction, cap.max(0.0))
    }

    /// Sells up to `want` shares, first in first out. Returns the shares sold.
    fn sell_shares(
        &self,
        st: &mut CompanyState,
        want: f64,
        price: f64,
        c: &EquityClimate,
        em: &mut EquityMonth,
    ) -> f64 {
        let mut left = want;
        let mut sold = 0.0;
        while left > DUST {
            let Some(li) = st.oldest_lot() else { break };
            let have = st.lots[li].shares;
            let whole = left >= have * (1.0 - 1e-12);
            let s = if whole { have } else { left };
            self.sell_from_lot(st, li, s, price, c, em);
            sold += s;
            left -= s;
            if whole {
                st.remove_lot(li);
            }
        }
        sold
    }

    /// Sells every lot, oldest first. Returns the shares sold.
    fn sell_all(
        &self,
        st: &mut CompanyState,
        price: f64,
        c: &EquityClimate,
        em: &mut EquityMonth,
    ) -> f64 {
        let mut sold = 0.0;
        while let Some(li) = st.oldest_lot() {
            let s = st.lots[li].shares;
            self.sell_from_lot(st, li, s, price, c, em);
            sold += s;
            st.remove_lot(li);
        }
        sold
    }

    /// Cashes out `units` of holding `hi` at `price`: options pay the price less the
    /// strike. `cashless_iso` says whether an incentive option's payout is income
    /// without payroll tax (a same-day sale) or wages (a cancellation in a deal).
    /// Returns the cash.
    #[allow(clippy::too_many_arguments)]
    fn cash_out(
        &self,
        st: &CompanyState,
        hi: usize,
        units: f64,
        price: f64,
        cashless_iso: bool,
        t: i32,
        em: &mut EquityMonth,
    ) -> f64 {
        let m = &self.holdings[hi];
        let each = if m.is_option() {
            (price - m.strike).max(0.0)
        } else {
            price
        };
        let cash = units * each;
        if cashless_iso && self.iso_treatment(st, hi, t) {
            em.iso_income += cash;
        } else {
            em.wages_cash += cash;
        }
        em.cash_in += cash;
        em.units_out += units;
        cash
    }

    /// Settles every vested RSU and every vested option in the money for cash at
    /// `price` (a listed company). Returns the units and the cash.
    fn settle_vested(
        &self,
        st: &mut CompanyState,
        price: f64,
        t: i32,
        em: &mut EquityMonth,
    ) -> (f64, f64) {
        let (mut units, mut cash) = (0.0, 0.0);
        for hi in 0..self.n_holdings {
            let m = &self.holdings[hi];
            let v = st.holdings[hi].vested;
            if v <= 0.0 || (m.is_option() && price <= m.strike) {
                continue;
            }
            cash += self.cash_out(st, hi, v, price, true, t, em);
            units += v;
            st.holdings[hi].vested = 0.0;
        }
        (units, cash)
    }

    /// Moves to the single-stock sleeve every lot past its holding periods.
    ///
    /// The sleeve carries a value and a basis and nothing else, so a lot that moves
    /// there loses the section 1202 exclusion. A qualified small business stock lot
    /// with cap left is therefore not moved: it waits here, as paper value, until
    /// its gain is excluded in full (five years held), and is then sold. Returns the
    /// shares sold that way.
    fn move_qualifying_lots(
        &self,
        st: &mut CompanyState,
        price: f64,
        c: &EquityClimate,
        em: &mut EquityMonth,
    ) -> f64 {
        let t = c.t;
        let (mut units, mut value, mut sold) = (0.0, 0.0, 0.0);
        let mut i = 0;
        while i < st.n_lots {
            let lot = st.lots[i];
            let ready = t - lot.acquired > LONG_TERM_MONTHS
                && (!lot.iso || t - lot.granted > ISO_HOLD_FROM_GRANT_MONTHS);
            if !ready {
                i += 1;
                continue;
            }
            let (fraction, cap) = self.qsbs_exclusion(st, &lot, t);
            if lot.qsbs && cap > 0.0 && price * lot.shares > lot.cost {
                if fraction < 1.0 {
                    i += 1;
                } else {
                    self.sell_from_lot(st, i, lot.shares, price, c, em);
                    sold += lot.shares;
                    st.remove_lot(i);
                }
                continue;
            }
            units += lot.shares;
            value += lot.shares * price;
            em.to_sleeve_value += lot.shares * price;
            em.to_sleeve_basis += lot.cost;
            em.basis_out += lot.cost;
            em.units_out += lot.shares;
            st.remove_lot(i);
        }
        if units > 0.0 {
            self.log(em, EqEventKind::LockupKept, units, price, value);
        }
        sold
    }

    fn anything_left(&self, st: &CompanyState) -> bool {
        st.sales_left > 0
            || st.n_lots > 0
            || st.holdings[..self.n_holdings]
                .iter()
                .any(|h| h.vested > 0.0 || (h.unvested > 0.0 && st.link != Link::Severed))
    }

    /// Every lot becomes a capital loss of its cost and every holding is cleared.
    ///
    /// Incentive-option shares exercised in the same calendar year lose their AMT
    /// adjustment too: the household is taken to sell or abandon worthless shares
    /// before the year closes, and a disposition in the year of exercise limits the
    /// AMT income to the gain, here nil (IRC 56(b)(3), 422(c)(2)). Whether bare
    /// worthlessness counts as a disposition is not settled; any advised holder
    /// makes it one. In a later year the adjustment stands and only the credit is left.
    fn wipe_out(
        &self,
        st: &mut CompanyState,
        c: &EquityClimate,
        em: &mut EquityMonth,
    ) -> (f64, f64) {
        let t = c.t;
        let (mut shares, mut cost) = (0.0, 0.0);
        for lot in &st.lots[..st.n_lots] {
            if lot.iso && lot.amt_year == c.year {
                em.amt_adjustment -= lot.amt_extra;
            }
            if t - lot.acquired > LONG_TERM_MONTHS {
                em.long_term_gain -= lot.cost;
            } else {
                em.short_term_gain -= lot.cost;
            }
            shares += lot.shares;
            cost += lot.cost;
        }
        em.basis_out += cost;
        em.units_out += st.total_units();
        if st.n_lots > 0 {
            em.replan = true;
        }
        st.lots = [Lot::default(); MAX_LOTS];
        st.n_lots = 0;
        st.holdings = [HoldingState::default(); MAX_HOLDINGS];
        st.sales_left = 0;
        (shares, cost)
    }

    /// One month of one company. Draws nothing and allocates nothing; returns the
    /// month's flows. A company whose status is `Failed` or `Done` returns zeros.
    pub fn step(&self, st: &mut CompanyState, c: &EquityClimate) -> EquityMonth {
        let mut em = EquityMonth::default();
        if matches!(st.status, CompanyStatus::Failed | CompanyStatus::Done) {
            return em;
        }
        let t = c.t;
        let mut price = self.price_now(st, t);
        let mut fmv = if st.status == CompanyStatus::Private {
            self.fmv_at(&st.fate, t)
        } else {
            price
        };
        st.price = price;
        let mut liquid = if c.liquid.is_finite() {
            c.liquid.max(0.0)
        } else {
            0.0
        };

        // 1. The link to the job.
        if st.link == Link::Pending {
            if c.job_started {
                st.link = Link::Active;
            } else if c.job_ended {
                self.sever(st, t, price, &mut em);
            }
        }
        if st.link == Link::Active
            && (c.left_job || !c.employed || c.job_ended || t >= self.leave_month || c.owner_died)
        {
            self.sever(st, t, price, &mut em);
        }

        // 2. Death of the owner: inherited shares are stepped up.
        if c.owner_died {
            for lot in &mut st.lots[..st.n_lots] {
                em.basis_out += lot.cost;
                lot.cost = lot.shares * fmv;
                em.basis_in += lot.cost;
                lot.amt_extra = 0.0;
                lot.iso = false;
                lot.amt_year = i32::MIN;
                lot.acquired = t - (LONG_TERM_MONTHS + 1);
                lot.qsbs = false;
            }
        }

        // 3. Vesting.
        if st.link == Link::Active {
            for hi in 0..self.n_holdings {
                let mut h = st.holdings[hi];
                if h.vest_months_left == 0 {
                    continue;
                }
                h.vest_months_left -= 1;
                h.cliff_months_left = h.cliff_months_left.saturating_sub(1);
                // The cliff month pays the accrued lump.
                let newly = if h.cliff_months_left > 0 {
                    0.0
                } else if h.vest_months_left == 0 {
                    h.unvested
                } else {
                    (h.unvested - h.per_month * f64::from(h.vest_months_left)).max(0.0)
                };
                h.unvested -= newly;
                let paid_now = st.status == CompanyStatus::Acquired;
                if !paid_now {
                    h.vested += newly;
                }
                st.holdings[hi] = h;
                if newly <= 0.0 {
                    continue;
                }
                if paid_now {
                    // Paid at the deal price as it vests; under-water options are
                    // cancelled.
                    self.cash_out(st, hi, newly, price, false, t, &mut em);
                } else if st.status == CompanyStatus::Private
                    && self.exercise == ExerciseTiming::AsTheyVest
                    && t > 0
                {
                    self.exercise(st, hi, newly, fmv, &mut liquid, c, &mut em);
                }
            }
        }
        if st.status == CompanyStatus::Public {
            // Whatever is vested and in the money is sold, whenever it vested.
            self.settle_vested(st, price, t, &mut em);
        }

        // 4. Exercise now.
        if t == 0
            && st.status == CompanyStatus::Private
            && self.exercise != ExerciseTiming::AtLiquidity
            && st.link != Link::Pending
        {
            let mut any = 0.0;
            for hi in 0..self.n_holdings {
                let want = st.holdings[hi].vested;
                any += self.exercise(st, hi, want, fmv, &mut liquid, c, &mut em);
            }
            if any > 0.0 {
                em.replan = true;
            }
        }

        // 5. The terminal event.
        if st.status == CompanyStatus::Private && t >= st.fate.month {
            st.event_month = t;
            match st.fate.kind {
                OutcomeKind::Fails | OutcomeKind::StillPrivate => {
                    let (shares, cost) = self.wipe_out(st, c, &mut em);
                    if st.fate.kind == OutcomeKind::Fails {
                        st.status = CompanyStatus::Failed;
                        st.outcome = Some(EquityOutcome::Failed);
                        em.failed_now = true;
                        self.log(&mut em, EqEventKind::Failed, shares, 0.0, cost);
                    } else {
                        st.status = CompanyStatus::Done;
                        st.outcome = Some(EquityOutcome::StillPrivate);
                        self.log(&mut em, EqEventKind::WrittenOff, shares, 0.0, cost);
                    }
                    price = 0.0;
                    st.price = 0.0;
                }
                OutcomeKind::Acquired => {
                    price = self.p0 * st.fate.x;
                    st.price = price;
                    let before = em.cash_in;
                    let mut units = self.sell_all(st, price, c, &mut em);
                    for hi in 0..self.n_holdings {
                        let v = st.holdings[hi].vested;
                        if v > 0.0 {
                            // A cancelled option is paid as wages, an ISO too.
                            self.cash_out(st, hi, v, price, false, t, &mut em);
                            st.holdings[hi].vested = 0.0;
                            units += v;
                        }
                    }
                    let unvested: f64 = st.holdings.iter().map(|h| h.unvested).sum();
                    if st.link == Link::Active && unvested > 0.0 && price > 0.0 {
                        st.status = CompanyStatus::Acquired;
                    } else {
                        em.units_out += unvested;
                        st.holdings = [HoldingState::default(); MAX_HOLDINGS];
                        st.status = CompanyStatus::Done;
                    }
                    st.outcome = Some(EquityOutcome::Acquired);
                    em.replan = true;
                    let cash = em.cash_in - before;
                    self.log(&mut em, EqEventKind::Acquired, units, price, cash);
                }
                OutcomeKind::Ipo => {
                    price = self.p0 * st.fate.m;
                    fmv = price;
                    st.price = price;
                    st.status = CompanyStatus::Lockup;
                    st.lockup_end = t.saturating_add(self.lockup_months);
                    st.outcome = Some(EquityOutcome::Ipo);
                    self.log(&mut em, EqEventKind::Ipo, 0.0, price, 0.0);
                }
            }
        }

        // 6. A tender offer.
        if st.status == CompanyStatus::Private {
            // Only a company heading to or above its last round's price runs
            // tenders, and one that will be written off (it sits at that price for
            // thirty years) runs none: a property of the fate, not of the household.
            let random = t >= 1
                && st.fate.m_end >= 1.0
                && st.fate.kind != OutcomeKind::StillPrivate
                && t - st.last_tender >= self.params.tender_min_gap_months
                && c.window_open
                && c.u_tender < self.tender_hazard;
            if random || self.tender_planned == t {
                st.last_tender = t;
                self.tender(st, price * self.tender_price_fraction, c, &mut em);
            }
        }

        // 7. The lockup ends.
        let mut lockup_ended = false;
        if st.status == CompanyStatus::Lockup && t >= st.lockup_end {
            lockup_ended = true;
            price = self.lockup_end_price(&st.fate);
            st.price = price;
            let before = em.cash_in;
            let (mut units, _) = self.settle_vested(st, price, t, &mut em);
            match self.sell_down {
                SellDown::AllAtLockupEnd => {
                    units += self.sell_all(st, price, c, &mut em);
                }
                SellDown::OverYears { years } => {
                    let n = years.max(1);
                    units += if n == 1 {
                        self.sell_all(st, price, c, &mut em)
                    } else {
                        let part = st.lot_shares() / f64::from(n);
                        self.sell_shares(st, part, price, c, &mut em)
                    };
                    st.sales_left = n - 1;
                    st.next_sale = t.saturating_add(12);
                    if st.n_lots == 0 {
                        st.sales_left = 0;
                    }
                }
                SellDown::Hold => units += self.move_qualifying_lots(st, price, c, &mut em),
            }
            if units > 0.0 {
                let cash = em.cash_in - before;
                self.log(&mut em, EqEventKind::LockupSale, units, price, cash);
            }
            st.status = CompanyStatus::Public;
            em.replan = true;
        }

        // 8. Deadlines: the exercise window after leaving, and the ten-year term.
        if matches!(st.status, CompanyStatus::Private | CompanyStatus::Public) {
            let severed_due = st.link == Link::Severed && t >= st.deadline;
            // [units, of which vested, paper value]: at the window's end, at the term.
            let (mut lapsed, mut expired) = ([0.0; 3], [0.0; 3]);
            for hi in 0..self.n_holdings {
                let m = self.holdings[hi];
                if !m.is_option() {
                    continue;
                }
                let term_due = t >= m.term_month;
                if !(severed_due || term_due) {
                    continue;
                }
                if st.status == CompanyStatus::Private
                    && self.at_deadline == DeadlinePolicy::Exercise
                {
                    let want = st.holdings[hi].vested;
                    if self.exercise(st, hi, want, fmv, &mut liquid, c, &mut em) > 0.0 {
                        em.replan = true;
                    }
                }
                let h = &mut st.holdings[hi];
                let mut lost = h.vested;
                let tally = if term_due { &mut expired } else { &mut lapsed };
                tally[1] += h.vested;
                h.vested = 0.0;
                if term_due {
                    lost += h.unvested;
                    h.unvested = 0.0;
                    h.vest_months_left = 0;
                }
                tally[0] += lost;
                tally[2] += lost * (price - m.strike).max(0.0);
            }
            for (kind, [units, vested, value]) in [
                (EqEventKind::Lapsed, lapsed),
                (EqEventKind::Expired, expired),
            ] {
                if units > 0.0 {
                    em.units_out += units;
                    self.log_split(&mut em, kind, units, vested, price, value);
                }
            }
        }

        // 9. After the lockup: the sale schedule, and shares waiting to qualify.
        if st.status == CompanyStatus::Public && !lockup_ended {
            if st.sales_left > 0 && t >= st.next_sale {
                let before = em.cash_in;
                // The last sale takes whatever is left, to the last fraction of a share.
                let units = if st.sales_left == 1 {
                    self.sell_all(st, price, c, &mut em)
                } else {
                    let part = st.lot_shares() / f64::from(st.sales_left);
                    self.sell_shares(st, part, price, c, &mut em)
                };
                st.sales_left -= 1;
                st.next_sale = st.next_sale.saturating_add(12);
                if units > 0.0 {
                    em.replan = true;
                    let cash = em.cash_in - before;
                    self.log(&mut em, EqEventKind::LockupSale, units, price, cash);
                }
            }
            if self.sell_down == SellDown::Hold {
                let before = em.cash_in;
                let units = self.move_qualifying_lots(st, price, c, &mut em);
                if units > 0.0 {
                    em.replan = true;
                    let cash = em.cash_in - before;
                    self.log(&mut em, EqEventKind::LockupSale, units, price, cash);
                }
            }
        }

        // 10. Close.
        match st.status {
            CompanyStatus::Public if !self.anything_left(st) => st.status = CompanyStatus::Done,
            CompanyStatus::Acquired
                if st.link != Link::Active || st.holdings.iter().all(|h| h.unvested <= 0.0) =>
            {
                em.units_out += st.holdings.iter().map(|h| h.unvested).sum::<f64>();
                st.holdings = [HoldingState::default(); MAX_HOLDINGS];
                st.status = CompanyStatus::Done;
            }
            _ => {}
        }
        st.price_growth = if st.status == CompanyStatus::Public {
            1.0 + c.single_stock_return
        } else {
            1.0
        };
        if !matches!(st.status, CompanyStatus::Failed | CompanyStatus::Done) {
            let mut value = price * st.lot_shares();
            for (h, m) in st.holdings.iter().zip(&self.holdings[..self.n_holdings]) {
                value += h.vested
                    * if m.is_option() {
                        (price - m.strike).max(0.0)
                    } else {
                        price
                    };
            }
            em.paper_value = value;
        }
        em
    }

    /// A tender offer at `price`: logged whether or not anything is sold. Lots sell
    /// first (first in, first out), then vested NSOs, then vested ISOs, up to the cap.
    fn tender(&self, st: &mut CompanyState, price: f64, c: &EquityClimate, em: &mut EquityMonth) {
        let eligible = self.included
            && price > 0.0
            && self.tender_sell_fraction > 0.0
            && (st.link == Link::Active || self.tender_former_employees);
        let before = em.cash_in;
        let mut sold = 0.0;
        if eligible {
            let mut base = st.lot_shares();
            for (h, m) in st.holdings.iter().zip(&self.holdings[..self.n_holdings]) {
                if m.is_option() && price > m.strike {
                    base += h.vested;
                }
            }
            let mut allow = self.tender_sell_fraction * self.tender_max_fraction * base;
            let want = allow.min(st.lot_shares());
            let from_lots = self.sell_shares(st, want, price, c, em);
            sold += from_lots;
            allow -= from_lots;
            for class in [HoldingClass::Nso, HoldingClass::Iso] {
                for hi in 0..self.n_holdings {
                    let m = self.holdings[hi];
                    if m.class != class || price <= m.strike || allow <= DUST {
                        continue;
                    }
                    let n = st.holdings[hi].vested.min(allow);
                    if n <= 0.0 {
                        continue;
                    }
                    // Exercised and sold together: wages for an NSO, income without
                    // payroll tax for an ISO (a same-day disqualifying disposition).
                    self.cash_out(st, hi, n, price, true, c.t, em);
                    st.holdings[hi].vested -= n;
                    sold += n;
                    allow -= n;
                }
            }
        }
        let cash = em.cash_in - before;
        em.tender_proceeds += cash;
        if sold > 0.0 {
            em.replan = true;
        }
        self.log(em, EqEventKind::Tender, sold, price, cash);
    }

    /// The owner has died and nobody present inherits: everything is dropped.
    /// Returns the basis that left (for the lot-basis identity).
    pub fn abandon(&self, st: &mut CompanyState) -> f64 {
        let basis = st.lot_basis();
        st.lots = [Lot::default(); MAX_LOTS];
        st.n_lots = 0;
        st.holdings = [HoldingState::default(); MAX_HOLDINGS];
        st.sales_left = 0;
        st.status = CompanyStatus::Done;
        basis
    }
}

/// Frees a slot by merging the two oldest lots with the given `iso` flag (or, if
/// there are not two, the two oldest of any kind): sums, the later dates, the later
/// AMT year, QSBS only if both.
fn merge_two_oldest(st: &mut CompanyState, iso: bool) {
    let pick = |st: &CompanyState, only: Option<bool>, skip: Option<usize>| -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, lot) in st.lots().iter().enumerate() {
            if Some(i) == skip || only.is_some_and(|flag| lot.iso != flag) {
                continue;
            }
            if !best.is_some_and(|b| lot.acquired >= st.lots[b].acquired) {
                best = Some(i);
            }
        }
        best
    };
    let mut pair = pick(st, Some(iso), None).and_then(|a| Some((a, pick(st, Some(iso), Some(a))?)));
    if pair.is_none() {
        pair = pick(st, Some(!iso), None).and_then(|a| Some((a, pick(st, Some(!iso), Some(a))?)));
    }
    let Some((a, b)) = pair else { return };
    let (keep, drop) = (a.min(b), a.max(b));
    let other = st.lots[drop];
    let lot = &mut st.lots[keep];
    lot.shares += other.shares;
    lot.cost += other.cost;
    lot.amt_extra += other.amt_extra;
    lot.acquired = lot.acquired.max(other.acquired);
    lot.granted = lot.granted.max(other.granted);
    lot.amt_year = lot.amt_year.max(other.amt_year);
    lot.qsbs = lot.qsbs && other.qsbs;
    st.remove_lot(drop);
}

// ---------------------------------------------------------------------------
// The standalone preview
// ---------------------------------------------------------------------------

/// Mean common-stock price at the outcome over the preferred price today, for a
/// table: what the odds imply a share is worth. Computed by quadrature, not from
/// draws.
pub fn implied_multiple(table: &ResolvedTable, overhang: f64, params: &EquityParams) -> f64 {
    const POINTS: usize = 801;
    let c = params.z_cap;
    let step = 2.0 * c / (POINTS - 1) as f64;
    let tail = 1.0 - normal_cdf(c);
    let density = |z: f64| math::exp(-0.5 * z * z) / (2.0 * std::f64::consts::PI).sqrt();
    let factor = lifetime_factor(table.still_private_probability(), params.rounds);
    let mut total = 0.0;
    for row in table.rows() {
        let value = |z: f64| {
            let m = math::exp(row.mu + row.sigma * z).clamp(1e-4, 1e4);
            match row.kind {
                OutcomeKind::Acquired => common_multiple(m, overhang),
                OutcomeKind::Ipo => m * params.lockup_mean,
                _ => 0.0,
            }
        };
        if !matches!(row.kind, OutcomeKind::Acquired | OutcomeKind::Ipo) || row.probability <= 0.0 {
            continue;
        }
        // Simpson's rule inside the cap, plus the mass held at each end.
        let mut sum = 0.0;
        for i in 0..POINTS {
            let z = -c + step * i as f64;
            let weight = if i == 0 || i == POINTS - 1 {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            sum += weight * value(z) * density(z);
        }
        let expected = sum * step / 3.0 + tail * (value(-c) + value(c));
        total += row.probability * factor * expected;
    }
    total
}

/// JSON helpers: NaN is written as `null` and read back as NaN.
mod nan_json {
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
        if v.is_finite() {
            s.serialize_f64(*v)
        } else {
            s.serialize_none()
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
        Ok(Option::<f64>::deserialize(d)?.unwrap_or(f64::NAN))
    }

    pub(super) mod triple {
        use serde::{Deserialize, Deserializer, Serializer};

        pub(in super::super) fn serialize<S: Serializer>(
            v: &[f64; 3],
            s: S,
        ) -> Result<S::Ok, S::Error> {
            s.collect_seq(v.iter().map(|x| x.is_finite().then_some(*x)))
        }

        pub(in super::super) fn deserialize<'de, D: Deserializer<'de>>(
            d: D,
        ) -> Result<[f64; 3], D::Error> {
            let raw = <[Option<f64>; 3]>::deserialize(d)?;
            Ok(raw.map(|x| x.unwrap_or(f64::NAN)))
        }
    }
}

/// What one company's holdings could pay, before tax, with no household simulated.
/// All dollar amounts are today's dollars.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct EquityPreview {
    /// Draws made.
    pub draws: u32,
    /// The outcome table in force (preset with outlook, or the custom table,
    /// normalised). Probabilities are exact, not rounded. For a preset the years
    /// are the 10th and 90th percentiles of its timing.
    pub table: Vec<OutcomeRow>,
    /// The stage preset with a typical outlook, for comparison.
    pub typical: Vec<OutcomeRow>,
    /// 409A value per share in force after defaults.
    pub fmv_409a: f64,
    /// Preference overhang in force after defaults.
    pub preference_overhang: f64,
    /// Tender chance per year in force after defaults, as an average over every
    /// future. Lower than the chance entered when that is more than the model can
    /// deliver (tenders happen only where the company ends at or above its last
    /// round's price, at most once a year).
    pub tender_annual_probability: f64,
    /// Value today of the vested holdings at the preferred price (shares and RSUs
    /// at the price, options at price less strike, not below 0).
    pub paper_value_vested: f64,
    /// The same for all holdings, vested or not.
    pub paper_value_total: f64,
    /// Cash to exercise the vested options (strike only).
    pub exercise_cost_vested: f64,
    /// Cash to exercise all options (strike only).
    pub exercise_cost_total: f64,
    /// Spread of the vested incentive options at the 409A value: the amount counted
    /// for AMT on exercising now.
    pub spread_iso_vested: f64,
    /// Spread of the vested non-qualified options at the 409A value: the amount
    /// taxed as pay on exercising now.
    pub spread_nso_vested: f64,
    /// Share of draws that pay nothing at the outcome.
    pub nothing_share: f64,
    /// Share of draws by what finally happens, over every round: fails, written
    /// off after thirty years, acquired, ipo.
    pub outcome_shares: [f64; 4],
    /// Among acquisitions: share in which common stock gets nothing.
    pub acquisition_zero_share: f64,
    /// Among IPOs: share priced below today's preferred price.
    pub ipo_below_last_round_share: f64,
    /// Share of draws in which at least one tender offer is expected before the
    /// outcome, for someone still employed there.
    pub tender_before_share: f64,
    /// Months from today until the first option deadline: the end of the exercise
    /// window after leaving (a former employer, or a planned departure) or a
    /// grant's ten-year term, whichever comes first. Null when there are no
    /// options. 0 means the deadline is this month or already past.
    pub option_deadline_months: Option<u32>,
    /// Cash (strike only, before tax) to exercise the vested options that fall due
    /// at that first deadline. 0 when the plan lets them lapse (`at_deadline:
    /// forfeit`), when their strike is at or above the preferred price, or when
    /// there are no options.
    pub option_deadline_cost: f64,
    /// Share of draws in which an option deadline comes before the company's
    /// outcome: those options are exercised with cash then or lapse, as the policy
    /// says. The payoffs assume the exercise is affordable.
    pub deadline_before_outcome_share: f64,
    /// 100 equally likely payoffs, ascending (zeros included).
    pub dots: Vec<f64>,
    /// Quantiles over all draws: p10, p25, p50, p75, p90.
    pub quantiles: [f64; 5],
    /// Quantiles among draws that pay something: p10, p50, p90. Null when no draw
    /// pays.
    #[serde(with = "nan_json::triple")]
    #[schemars(with = "[Option<f64>; 3]")]
    pub if_paid: [f64; 3],
    /// Median years to the payout among draws that pay something. Null when no
    /// draw pays.
    #[serde(with = "nan_json")]
    #[schemars(with = "Option<f64>")]
    pub years_to_payout_p50: f64,
    /// Mean common-stock price at the outcome over the preferred price (0 where
    /// nothing is paid): what the table in force implies a share is worth.
    pub implied_multiple: f64,
    /// The same for the stage's typical table.
    pub typical_implied_multiple: f64,
}

/// Fewest draws a preview requested through [`preview_json`] makes.
pub const PREVIEW_MIN_DRAWS: u32 = 100;
/// Most draws a preview requested through [`preview_json`] makes.
pub const PREVIEW_MAX_DRAWS: u32 = 50_000;

/// The preview of one company given as JSON (one element of
/// `profile.private_equity`), as `EquityPreview` JSON, with the parameters shipped
/// with the engine. `draws` of 0 means the default; any other number is held
/// between [`PREVIEW_MIN_DRAWS`] and [`PREVIEW_MAX_DRAWS`]. A company that does not
/// parse is reported with the path of the offending field, like a profile, and so is
/// one the simulation would reject ([`crate::validate::validate_company`]): the
/// first error's path and message. A bare company (a stage and a price, no
/// holdings) is valid.
pub fn preview_json(company_json: &str, draws: u32) -> Result<String, String> {
    let company = PrivateCompany::from_json(company_json)?;
    if let Some(issue) = crate::validate::validate_company(&company).errors.first() {
        return Err(format!(
            "{}: {}",
            issue.path.trim_start_matches('/'),
            issue.message
        ));
    }
    let params = EquityParams::from_data(Data::embedded());
    let draws = if draws == 0 {
        0
    } else {
        draws.clamp(PREVIEW_MIN_DRAWS, PREVIEW_MAX_DRAWS)
    };
    serde_json::to_string(&preview(&company, &params, draws)).map_err(|e| e.to_string())
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let pos = q.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

/// When a holding's options stop vesting and when they lapse, for the preview.
#[derive(Clone, Copy)]
struct PreviewDeadline {
    /// Last month whose step still vests units (`i32::MAX`: no end).
    vest_stop: i32,
    /// Month the vested options lapse unless exercised (`i32::MAX`: none).
    month: i32,
}

/// The deadline of one holding as the simulation applies it with no household:
/// options lapse at the grant's ten-year term, and at the end of the exercise window
/// when the company has no job (the window runs from today) or a departure is
/// planned (it runs from that month). Vesting stops when the link does.
fn preview_deadline(company: &PrivateCompany, h: &EquityHolding) -> PreviewDeadline {
    let window = company.exercise_window_months.min(1_200) as i32;
    let (vest_stop, window_end) = if company.job.is_none() {
        (-1, window)
    } else if let Some(leave) = company.policy.leave_after_months {
        let leave = leave.min(100_000) as i32;
        (leave - 1, leave.saturating_add(window))
    } else {
        (i32::MAX, i32::MAX)
    };
    let term = match h.kind {
        HoldingKind::Iso {
            granted_months_ago, ..
        }
        | HoldingKind::Nso {
            granted_months_ago, ..
        } => OPTION_TERM_MONTHS - granted_months_ago.min(1_200) as i32,
        _ => i32::MAX,
    };
    let month = if h.kind.strike().is_some() {
        term.min(window_end).max(0)
    } else {
        i32::MAX
    };
    PreviewDeadline { vest_stop, month }
}

/// What one company's holdings could pay, before tax, with no household
/// simulated. The holder never exercises early and sells everything at the outcome
/// (after the lockup for an IPO), and stays in the linked job until then unless the
/// policy plans a departure. Deadlines are applied as the simulation applies them:
/// options whose exercise window or ten-year term ends before the outcome pay
/// nothing under `at_deadline: forfeit`, and under `exercise` pay the outcome price
/// less the strike on what had vested by the deadline (the exercise is taken to be
/// affordable, and a draw is never counted below zero). Vested RSUs that require
/// employment are lost by a planned departure before the outcome. Tender offers
/// before the outcome are left out of the amounts. Today's dollars at a fixed rate
/// of inflation. `draws` of 0 uses the default in the data.
pub fn preview(company: &PrivateCompany, params: &EquityParams, draws: u32) -> EquityPreview {
    let draws = if draws == 0 {
        params.preview_draws
    } else {
        draws
    }
    .max(1);
    let table = resolved_table(company, params);
    let p0 = if company.preferred_price.is_finite() {
        company.preferred_price.max(0.0)
    } else {
        0.0
    };
    let fmv = fmv_in_force(company, params);
    let overhang = table.overhang;
    let p_tender = tender_probability_in_force(company, params);
    let monthly_tender = params.tender_monthly_hazard(tender_rate(&table, params, p_tender))
        * params.window_open_share;
    let lockup_months = company.lockup_months.min(1_200) as i32;
    let exercises_at_deadline = company.policy.at_deadline == DeadlinePolicy::Exercise;
    let leave_month = match (&company.job, company.policy.leave_after_months) {
        (Some(_), Some(m)) => m.min(100_000) as i32,
        _ => i32::MAX,
    };
    let deadlines: Vec<PreviewDeadline> = company
        .holdings
        .iter()
        .map(|h| preview_deadline(company, h))
        .collect();
    // The first deadline and the strike due then.
    let first_deadline = deadlines.iter().map(|d| d.month).min().unwrap_or(i32::MAX);
    let mut option_deadline_cost = 0.0;
    if first_deadline != i32::MAX && exercises_at_deadline {
        for (h, d) in company.holdings.iter().zip(&deadlines) {
            if let (Some(k), true) = (h.kind.strike(), d.month == first_deadline) {
                if k < p0 {
                    option_deadline_cost += units_vested_by(h, d.month.min(d.vest_stop)) * k;
                }
            }
        }
    }

    let (paper_value_vested, paper_value_total) = position_value(company);
    let (mut cost_vested, mut cost_total) = (0.0, 0.0);
    let (mut spread_iso, mut spread_nso) = (0.0, 0.0);
    for h in &company.holdings {
        let Some(k) = h.kind.strike() else { continue };
        let vested = h.vested.clamp(0.0, h.shares.max(0.0));
        cost_vested += vested * k;
        cost_total += h.shares * k;
        let spread = vested * (fmv - k).max(0.0);
        if matches!(h.kind, HoldingKind::Iso { .. }) {
            spread_iso += spread;
        } else {
            spread_nso += spread;
        }
    }

    let n = draws as usize;
    let mut payoffs = Vec::with_capacity(n);
    let mut paid_years = Vec::new();
    let mut counts = [0u32; 4];
    let (mut acquisitions_zero, mut ipos_below) = (0u32, 0u32);
    let mut tender_sum = 0.0;
    let mut deadline_draws = 0u32;
    for i in 0..draws {
        let mut rng = Rng::stream(PREVIEW_SEED, u64::from(i), Stream::PrivateCompany(0));
        let fate = draw_fate(&table, params, &mut rng);
        let (slot, price, payout_month) = match fate.kind {
            OutcomeKind::Fails => (0, 0.0, fate.month),
            OutcomeKind::StillPrivate => (1, 0.0, fate.month),
            OutcomeKind::Acquired => {
                if fate.x <= 0.0 {
                    acquisitions_zero += 1;
                }
                (2, p0 * fate.x, fate.month)
            }
            OutcomeKind::Ipo => {
                if fate.m < 1.0 {
                    ipos_below += 1;
                }
                let factor = if lockup_months == 0 { 1.0 } else { fate.lockup };
                (
                    3,
                    p0 * fate.m * factor,
                    fate.month.saturating_add(lockup_months),
                )
            }
        };
        counts[slot] += 1;
        // Payoffs in today's dollars: the sale at the payout month, a strike paid
        // at a deadline in that month.
        let deflate = |amount: f64, month: i32| {
            amount / math::pow(1.0 + params.preview_inflation, f64::from(month) / 12.0)
        };
        let mut pay = 0.0;
        let mut deadline_first = false;
        for (h, d) in company.holdings.iter().zip(&deadlines) {
            match h.kind {
                HoldingKind::Iso { strike: k, .. } | HoldingKind::Nso { strike: k, .. } => {
                    // The deadline step runs before the outcome's month, never in it.
                    if d.month < fate.month {
                        deadline_first = true;
                        // Exercised for cash then (never when the strike is at or
                        // above the last round's price) and held to the outcome, or
                        // lapsed. What had not vested by the term lapses with it.
                        if exercises_at_deadline && k < p0 {
                            let units = units_vested_by(h, d.month.min(d.vest_stop));
                            pay +=
                                deflate(units * price, payout_month) - deflate(units * k, d.month);
                        }
                    } else {
                        let units = units_vested_by(h, payout_month.min(d.vest_stop));
                        pay += deflate(units * (price - k).max(0.0), payout_month);
                    }
                }
                HoldingKind::Rsu {
                    must_be_employed_at_event,
                } => {
                    // A departure in or before the outcome's month forfeits vested
                    // units that require employment.
                    let lost = must_be_employed_at_event && leave_month <= fate.month;
                    if !lost {
                        let units = units_vested_by(h, payout_month.min(d.vest_stop));
                        pay += deflate(units * price, payout_month);
                    }
                }
                HoldingKind::Shares { .. } => {
                    pay += deflate(h.shares.max(0.0) * price, payout_month);
                }
            }
        }
        deadline_draws += u32::from(deadline_first);
        let years = f64::from(payout_month) / 12.0;
        // A draw is never counted below zero: strike lost to a failure after a
        // deadline exercise is a cost of the policy, not a payout.
        let real = if pay.is_finite() { pay.max(0.0) } else { 0.0 };
        if real > 0.0 {
            paid_years.push(years);
        }
        payoffs.push(real);

        tender_sum += match company.tenders.next_in_months {
            Some(m) if (m as i64) < i64::from(fate.month) => 1.0,
            _ if fate.m_end >= 1.0 && fate.kind != OutcomeKind::StillPrivate => {
                1.0 - math::powi(1.0 - monthly_tender, (fate.month - 1).max(0))
            }
            _ => 0.0,
        };
    }
    payoffs.sort_by(f64::total_cmp);
    paid_years.sort_by(f64::total_cmp);
    let first_paid = payoffs.partition_point(|v| *v <= 0.0);
    let paid = &payoffs[first_paid..];
    let total = f64::from(draws);
    let share = |k: u32, of: u32| {
        if of == 0 {
            0.0
        } else {
            f64::from(k) / f64::from(of)
        }
    };
    let dots = (0..100)
        .map(|i| payoffs[((i as f64 + 0.5) / 100.0 * n as f64) as usize])
        .collect();
    let typical_table = preset_table(
        company.stage,
        Outlook::Typical,
        params.overhang[company.stage.index()],
        params,
    );
    EquityPreview {
        draws,
        table: table_rows(company, params),
        typical: typical_rows(company.stage, params),
        fmv_409a: fmv,
        preference_overhang: overhang,
        tender_annual_probability: tender_probability_met(&table, params, p_tender),
        paper_value_vested,
        paper_value_total,
        exercise_cost_vested: cost_vested,
        exercise_cost_total: cost_total,
        spread_iso_vested: spread_iso,
        spread_nso_vested: spread_nso,
        nothing_share: first_paid as f64 / total,
        outcome_shares: counts.map(|k| f64::from(k) / total),
        acquisition_zero_share: share(acquisitions_zero, counts[2]),
        ipo_below_last_round_share: share(ipos_below, counts[3]),
        tender_before_share: tender_sum / total,
        option_deadline_months: (first_deadline != i32::MAX).then_some(first_deadline as u32),
        option_deadline_cost,
        deadline_before_outcome_share: f64::from(deadline_draws) / total,
        dots,
        quantiles: [0.10, 0.25, 0.50, 0.75, 0.90].map(|q| quantile(&payoffs, q)),
        if_paid: [0.10, 0.50, 0.90].map(|q| quantile(paid, q)),
        years_to_payout_p50: quantile(&paid_years, 0.5),
        implied_multiple: implied_multiple(&table, overhang, params),
        typical_implied_multiple: implied_multiple(&typical_table, typical_table.overhang, params),
    }
}
