//! Retirement spending rules and the annual withdrawal plan.
//!
//! Responsibility, once a year in January for a retired household:
//!
//! 1. [`SpendingPlanner`]: this year's spending under the chosen rule (constant real,
//!    percent of portfolio, guardrails, Vanguard dynamic, VPW, floor and ceiling),
//!    expressed as a multiple of the retirement budget so `sim` can scale
//!    non-essential lines first.
//! 2. [`plan_year`]: where the money comes from: RMDs first, then the chosen
//!    withdrawal order, early-access rules before 59 and a half, a Roth conversion
//!    amount when tax-aware, respecting bracket and ACA limits, with the tax the
//!    withdrawals cause grossed up into the amounts.
//! 3. [`execute_plan`]: carries out the plan (or a twelfth of it) on the accounts and
//!    writes the tax character of what it did into the year's `TaxInputs`.
//!    [`cover_shortfall`] is the unplanned version for a mid-year deficit.
//!
//! Contract:
//!
//! * Pure functions of their inputs plus the small [`SpendingState`]; no randomness,
//!   no allocation.
//! * The plan is a set of annual targets. `sim` executes it and deviates when a
//!   bucket runs dry; the plan is not a guarantee.
//! * `plan_year` calls `tax::compute` a handful of times (typically five to eight, never more than eleven); it must not
//!   be called more than once a year per path.
//! * All amounts are nominal dollars of the planning year unless a field says "real".
//!
//! How the spending rules treat a budget that changes over time: every rule that
//! carries last year's spending forward does so as a *multiple of the budget*, so a
//! planned change in the budget itself (the mortgage ends, a child leaves) passes
//! straight through and only market-driven adjustments are limited by the rule. With
//! a constant budget and no other income each rule reduces exactly to its published
//! definition in `data/actuarial/params.json` (`withdrawal_rules`).

use crate::accounts::{
    Accounts, Bucket, Weights, WithdrawContext, Withdrawal, EARLY_WITHDRAWAL_AGE,
};
use crate::data::Data;
use crate::math;
use crate::model::{AssetClass, EarlyAccess, SpendingRule, TaxKind, WithdrawalOrder};
use crate::tax::{self, Brackets, TaxInputs, TaxResult, TaxYearParams};
use crate::MAX_PEOPLE;

/// Guyton-Klinger: the capital preservation rule is switched off in the last 15 years
/// of the plan (`withdrawal_rules.guyton_klinger.thresholds`).
pub const GUARDRAIL_SUNSET_YEARS: f64 = 15.0;
/// Vanguard dynamic spending: target share of the portfolio when the household's own
/// initial withdrawal rate is not known (`vanguard_dynamic_spending.default_p`).
pub const VANGUARD_DEFAULT_RATE: f64 = 0.04;
/// Floor and ceiling: target share of the portfolio when the initial withdrawal rate
/// is not known (`floor_and_ceiling.default_p`).
pub const FLOOR_CEILING_DEFAULT_RATE: f64 = 0.045;
/// VPW: the withdrawal percentage is capped at 10% (`vpw.max_percentage_cap`).
pub const VPW_MAX_RATE: f64 = 0.10;
/// VPW: assumed real return of growth assets (`vpw.assumed_real_returns.stocks`).
pub const VPW_GROWTH_REAL_RETURN: f64 = 0.05;
/// VPW: assumed real return of defensive assets (`vpw.assumed_real_returns.bonds`).
pub const VPW_DEFENSIVE_REAL_RETURN: f64 = 0.019;

/// Highest interest rate always allowed for a 72(t) amortisation schedule: 5%
/// (`early_withdrawal.sepp_72t.interest_rate_floor_cap` in
/// `data/tax/us-federal-2026.json`, IRS Notice 2022-6). The law allows 120% of the
/// federal mid-term rate when that is higher; the model uses the 5% that is always
/// available.
pub const SEPP_MAX_RATE: f64 = 0.05;

/// The ACA premium tax credit ends at 400% of the federal poverty line
/// (`aca.premium_tax_credit.eligibility_max_pct_fpl`).
pub const ACA_CLIFF_FPL_MULTIPLE: f64 = 4.0;

/// Planning cushion under the ACA cliff. Not a statutory number: the plan is made in
/// January on estimated dividends and gains, and one dollar over the cliff loses the
/// whole credit, so the plan aims 8% below it (measured January-estimate errors in
/// investment income run to about 3% of the cliff; `sim` also re-plans in October on
/// the year's actual income).
pub const ACA_PLANNING_MARGIN: f64 = 0.08;

/// Tolerance when a traditional withdrawal or a Roth conversion is judged against
/// the bracket rate being filled: the federal tax and lost premium credit it causes
/// per dollar may exceed `fill_to_bracket` by this much (rounding of thresholds,
/// small phase-outs) before the amount is cut back.
pub const FILL_RATE_TOLERANCE: f64 = 0.02;

/// A 0% gain harvest is kept only if it costs less than this fraction of the gain in
/// total tax and lost premium credit. The federal 0% bracket alone does not show
/// state tax, extra Social Security taxation or credit phase-outs; this check does.
pub const HARVEST_MAX_COST: f64 = 0.001;

/// Upper bound on the marginal tax rate assumed when extrapolating the tax gross-up
/// (keeps the extrapolation finite across cliffs and phase-outs).
const MAX_GROSS_UP_RATE: f64 = 0.9;

/// Secant refinements of the gross-up after the first two evaluations.
const GROSS_UP_SECANT_STEPS: usize = 4;

/// Dollar tolerance below which the gross-up is considered converged and amounts are
/// considered zero.
const CENT: f64 = 0.01;

/// IRS Single Life Table (26 CFR 1.401(a)(9)-9(b), effective 2022) at ages 20, 25,
/// ..., 70; life expectancy at ages in between is interpolated linearly (the table is
/// nearly straight over five years). Used for the 72(t) amortisation method, where
/// the single-life table gives the largest allowed payment. Quoted from memory:
/// recorded in `data/actuarial/params.json` under `unverified_from_memory` (a test
/// keeps the two equal) until someone checks it against the regulation.
pub const SINGLE_LIFE_EXPECTANCY: [f64; 11] = [
    65.0, 60.2, 55.3, 50.5, 45.7, 41.0, 36.2, 31.6, 27.1, 22.9, 18.8,
];
/// Age of the first entry of [`SINGLE_LIFE_EXPECTANCY`].
const SINGLE_LIFE_FIRST_AGE: f64 = 20.0;
/// Age step between entries of [`SINGLE_LIFE_EXPECTANCY`].
const SINGLE_LIFE_AGE_STEP: f64 = 5.0;

/// Parameters of the spending rules that are not part of the user's rule choice.
/// [`RuleParams::default`] holds the values of `data/actuarial/params.json`
/// (a unit test keeps the two in step); [`RuleParams::from_data`] reads them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RuleParams {
    /// Guardrails: no capital-preservation cut with this many years or fewer left.
    pub guardrail_sunset_years: f64,
    /// Vanguard dynamic: portfolio share targeted when the initial rate is unknown.
    pub vanguard_default_rate: f64,
    /// Floor and ceiling: portfolio share targeted when the initial rate is unknown.
    pub floor_ceiling_default_rate: f64,
    /// VPW: cap on the withdrawal percentage.
    pub vpw_max_rate: f64,
    /// VPW: real return assumed for growth assets.
    pub vpw_growth_real_return: f64,
    /// VPW: real return assumed for defensive assets.
    pub vpw_defensive_real_return: f64,
}

impl Default for RuleParams {
    fn default() -> Self {
        RuleParams {
            guardrail_sunset_years: GUARDRAIL_SUNSET_YEARS,
            vanguard_default_rate: VANGUARD_DEFAULT_RATE,
            floor_ceiling_default_rate: FLOOR_CEILING_DEFAULT_RATE,
            vpw_max_rate: VPW_MAX_RATE,
            vpw_growth_real_return: VPW_GROWTH_REAL_RETURN,
            vpw_defensive_real_return: VPW_DEFENSIVE_REAL_RETURN,
        }
    }
}

impl RuleParams {
    /// Reads the parameters from `data.actuarial` (`withdrawal_rules`); a missing
    /// entry keeps its default.
    pub fn from_data(data: &Data) -> RuleParams {
        let rules = &data.actuarial["withdrawal_rules"];
        let d = RuleParams::default();
        let get = |v: &serde_json::Value, default: f64| v.as_f64().unwrap_or(default);
        RuleParams {
            guardrail_sunset_years: get(
                &rules["guyton_klinger"]["thresholds"]
                    ["capital_preservation_sunset_years_before_end"],
                d.guardrail_sunset_years,
            ),
            vanguard_default_rate: get(
                &rules["vanguard_dynamic_spending"]["default_p"],
                d.vanguard_default_rate,
            ),
            floor_ceiling_default_rate: get(
                &rules["floor_and_ceiling"]["default_p"],
                d.floor_ceiling_default_rate,
            ),
            vpw_max_rate: get(&rules["vpw"]["max_percentage_cap"], d.vpw_max_rate),
            vpw_growth_real_return: get(
                &rules["vpw"]["assumed_real_returns"]["stocks"],
                d.vpw_growth_real_return,
            ),
            vpw_defensive_real_return: get(
                &rules["vpw"]["assumed_real_returns"]["bonds"],
                d.vpw_defensive_real_return,
            ),
        }
    }

    /// The expected real return VPW assumes for an allocation: the growth share at
    /// the stock return plus the defensive share at the bond return. Pass the result
    /// as [`SpendingInputs::expected_real_return`].
    pub fn vpw_real_return(&self, weights: &Weights) -> f64 {
        let mut growth = 0.0;
        let mut total = 0.0;
        for a in AssetClass::ALL {
            let w = weights[a.index()].max(0.0);
            total += w;
            if !a.is_defensive() {
                growth += w;
            }
        }
        if total <= 0.0 {
            return self.vpw_defensive_real_return;
        }
        let g = growth / total;
        g * self.vpw_growth_real_return + (1.0 - g) * self.vpw_defensive_real_return
    }
}

/// State a spending rule carries from year to year on a path.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SpendingState {
    /// Withdrawal rate in the first year the portfolio was drawn on (the guardrails
    /// reference, and the target percentage of the Vanguard and floor-and-ceiling
    /// rules). 0 until then.
    pub initial_withdrawal_rate: f64,
    /// Last year's spending in today's dollars (for reporting).
    pub last_real_spending: f64,
    /// Current multiple of the retirement budget being spent (1.0 = on plan).
    pub budget_multiple: f64,
    /// Number of years retired so far (0 before the first [`SpendingPlanner::decide`]).
    pub years_retired: u32,
}

/// The funded test's view of the household, which the guardrails, Vanguard and
/// floor-and-ceiling rules measure their withdrawal rate against.
///
/// The rate a rule compares with its reference is `need / base`, not this year's
/// cash withdrawal over the portfolio: in a real household the first differs from
/// year to year for reasons that are part of the plan (a mortgage being paid off,
/// children leaving, health insurance before Medicare, Social Security not yet
/// claimed, a partner still earning), and a rule anchored on it cuts spending when
/// the plan is exactly on track.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Funding {
    /// Wealth available for the long run, today's dollars: investable assets less
    /// other debts, less the sums set aside for temporary costs (mortgage balances,
    /// child costs still due), plus the present value of income not yet started.
    pub base_real: f64,
    /// The steady annual draw on that wealth at a budget multiple of 1, today's
    /// dollars: the retirement budget and its tax, less income already in payment.
    pub need_real: f64,
}

/// Inputs to the spending rule for one year.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SpendingInputs {
    /// The retirement budget for this year in today's dollars, including modelled
    /// healthcare, before any rule adjustment.
    pub budget_real: f64,
    /// The part of `budget_real` that is essential and will not be cut by a rule.
    pub essential_real: f64,
    /// Investable portfolio in today's dollars at the start of the year.
    pub portfolio_real: f64,
    /// Income expected this year that needs no withdrawal (Social Security, pensions,
    /// rent), after tax, in today's dollars.
    pub other_income_real: f64,
    /// Years left until the end of the plan, counting this one (for VPW and the
    /// guardrails sunset).
    pub years_remaining: f64,
    /// Expected real return of the current allocation (for VPW; see
    /// [`RuleParams::vpw_real_return`]).
    pub expected_real_return: f64,
    /// True if the portfolio's total return over the previous calendar year was
    /// negative (guardrails: the inflation raise may be skipped).
    pub prior_return_negative: bool,
    /// Inflation over the previous calendar year as a decimal (guardrails: the size
    /// of the raise that is skipped).
    pub prior_inflation: f64,
    /// The funded test's quantities (see [`Funding`]). `None`: the rules fall back
    /// to `portfolio_real` and `budget_real - other_income_real`, which is the
    /// published definition for a household with a constant budget.
    pub funding: Option<Funding>,
    /// Guardrails: the capital-preservation cut is measured against the larger of
    /// the household's own initial rate and this rate (the funded test's withdrawal
    /// rate), so a household drawing far less than it could is not told to cut
    /// because a small rate became a slightly less small one. 0 disables the floor.
    pub reference_floor: f64,
    /// True while spending must not be raised (a long-term care episode is under
    /// way): the rules may cut but not increase the multiple.
    pub hold_raises: bool,
}

impl SpendingInputs {
    /// `(funding base, steady draw at a multiple of 1)`.
    fn anchor(&self) -> (f64, f64) {
        let clean = |x: f64| if x.is_finite() { x.max(0.0) } else { 0.0 };
        match self.funding {
            Some(f) => (clean(f.base_real), clean(f.need_real)),
            None => (
                clean(self.portfolio_real),
                clean(clean(self.budget_real) - clean(self.other_income_real)),
            ),
        }
    }
}

/// The rule's decision for one year.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SpendingDecision {
    /// Total spending for the year in today's dollars.
    pub spending_real: f64,
    /// `spending_real / budget_real`: 1.0 on plan, below 1 when the rule cuts.
    pub budget_multiple: f64,
    /// Relative change of the multiple against last year's (-0.1 = "cut spending
    /// 10%"), for the event log; 0 when unchanged and in the first year.
    pub adjustment: f64,
}

/// A compiled spending rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpendingPlanner {
    /// The rule from the profile.
    pub rule: SpendingRule,
    /// Rule parameters from the research data.
    pub params: RuleParams,
}

/// The share of the portfolio VPW withdraws with `years` left (counting this one) at
/// an expected real return: the level start-of-year payment that exhausts the
/// portfolio, `r / (1 - (1 + r)^-n) / (1 + r)`, or `1 / n` when `r` is zero. Not
/// capped; [`SpendingPlanner::decide`] applies the cap.
pub fn vpw_rate(expected_real_return: f64, years: f64) -> f64 {
    let n = if years.is_nan() { 1.0 } else { years.max(1.0) };
    let r = expected_real_return;
    if r.is_nan() || r <= -1.0 {
        return 1.0 / n;
    }
    if r.abs() < 1e-9 {
        return 1.0 / n;
    }
    let annuity = 1.0 - math::pow(1.0 + r, -n);
    (r / annuity / (1.0 + r)).min(1.0)
}

impl SpendingPlanner {
    /// Wraps a rule with the default (sourced) parameters.
    pub fn new(rule: SpendingRule) -> SpendingPlanner {
        SpendingPlanner {
            rule,
            params: RuleParams::default(),
        }
    }

    /// Wraps a rule with parameters read from the embedded research data.
    pub fn from_data(rule: SpendingRule, data: &Data) -> SpendingPlanner {
        SpendingPlanner {
            rule,
            params: RuleParams::from_data(data),
        }
    }

    /// Initialises the state at retirement. Call once, then [`Self::decide`] for the
    /// first retirement year and every January after.
    pub fn start(&self, inputs: &SpendingInputs) -> SpendingState {
        let (base, need) = inputs.anchor();
        SpendingState {
            initial_withdrawal_rate: if base > 0.0 { need / base } else { 0.0 },
            last_real_spending: inputs.budget_real.max(0.0),
            budget_multiple: 1.0,
            years_retired: 0,
        }
    }

    /// This year's spending; updates `state`.
    ///
    /// Notation: `budget` this year's budget, `m` last year's multiple, `planned =
    /// m * budget`, `other` income needing no withdrawal, `P` the investable
    /// portfolio. For the three rules that follow a withdrawal *rate*: `A` the
    /// funding base and `N` the steady draw at a multiple of 1 ([`Funding`]; without
    /// it `A = P` and `N = budget - other`), `W = N + (m - 1) * budget` the draw
    /// that keeps last year's standard of living, `IWR` the value of `W / A` in the
    /// first year with a draw.
    ///
    /// * `ConstantReal` (Bengen): the budget, whatever the portfolio does.
    /// * `PercentOfPortfolio`: `rate * P + other`.
    /// * `Guardrails` (Guyton-Klinger 2006), from the second year: (1) withdrawal
    ///   rule: spending keeps its real value, except that the inflation raise on the
    ///   draw is skipped (`W / (1 + inflation)`) after a year of negative return if
    ///   `W / A > IWR`; (2) capital preservation: if `W / A > (1 + upper) *
    ///   max(IWR, reference_floor)`, cut spending by `adjust`, unless 15 or fewer
    ///   years remain; (3) prosperity: if `W / A < (1 - lower) * IWR`, raise
    ///   spending by `adjust`. With no other income, spending is the draw and this
    ///   is the published rule.
    /// * `VanguardDynamic`, from the second year: target `rate * A + (budget - N)`
    ///   (the spending at which the draw is `rate * A`; `rate` defaults to `IWR`),
    ///   limited to between `planned * (1 - floor)` and `planned * (1 + ceiling)`.
    /// * `Vpw`: `min(vpw_rate(r, n), 10%) * P + other`.
    /// * `FloorCeiling`, from the second year: the same target, limited to between
    ///   `floor * budget` and `ceiling * budget`.
    ///
    /// Whatever the rule says, spending is never set below the essential part of the
    /// budget; if the portfolio cannot pay for that, `sim` records the shortfall.
    pub fn decide(&self, state: &mut SpendingState, inputs: &SpendingInputs) -> SpendingDecision {
        let budget = if inputs.budget_real.is_finite() {
            inputs.budget_real.max(0.0)
        } else {
            0.0
        };
        let other = if inputs.other_income_real.is_finite() {
            inputs.other_income_real.max(0.0)
        } else {
            0.0
        };
        let portfolio = if inputs.portfolio_real.is_finite() {
            inputs.portfolio_real.max(0.0)
        } else {
            0.0
        };
        let first = state.years_retired == 0;
        let previous = if state.budget_multiple > 0.0 && state.budget_multiple.is_finite() {
            state.budget_multiple
        } else {
            1.0
        };
        let planned = previous * budget;
        let (base, steady) = inputs.anchor();
        let planned_withdrawal = (steady + (previous - 1.0) * budget).max(0.0);
        if state.initial_withdrawal_rate <= 0.0 && planned_withdrawal > 0.0 && base > 0.0 {
            state.initial_withdrawal_rate = planned_withdrawal / base;
        }
        // Spending at which the steady draw is zero: what income covers.
        let covered = (budget - steady).max(0.0);
        let iwr = state.initial_withdrawal_rate;
        let p = &self.params;

        let rule_spending = match self.rule {
            SpendingRule::ConstantReal => budget,
            SpendingRule::PercentOfPortfolio { rate } => rate.max(0.0) * portfolio + other,
            SpendingRule::Guardrails {
                upper,
                lower,
                adjust,
            } => {
                if first || base <= 0.0 || iwr <= 0.0 {
                    planned
                } else {
                    let mut w = planned_withdrawal;
                    let mut spend = planned;
                    if inputs.prior_return_negative && w / base > iwr {
                        let kept = w / (1.0 + inputs.prior_inflation.max(0.0));
                        spend -= w - kept;
                        w = kept;
                    }
                    let rate = w / base;
                    let floor = if inputs.reference_floor.is_finite() {
                        inputs.reference_floor.max(0.0)
                    } else {
                        0.0
                    };
                    if rate > iwr.max(floor) * (1.0 + upper) {
                        if inputs.years_remaining > p.guardrail_sunset_years {
                            spend *= 1.0 - adjust;
                        }
                    } else {
                        // Restoring a cut is judged on the household's own rate.
                        // Spending *above* the plan is judged on the lower of that
                        // and the plan's safe rate: a household that began above
                        // its own safe rate has not become prosperous because the
                        // rate fell a little.
                        let reference = if previous >= 1.0 - 1e-9 && floor > 0.0 {
                            iwr.min(floor)
                        } else {
                            iwr
                        };
                        if rate < reference * (1.0 - lower) {
                            spend *= 1.0 + adjust;
                        }
                    }
                    spend
                }
            }
            SpendingRule::VanguardDynamic {
                ceiling,
                floor,
                rate,
            } => {
                if first {
                    planned
                } else {
                    let rate = if let Some(r) = rate.filter(|r| r.is_finite() && *r > 0.0) {
                        r
                    } else if iwr > 0.0 {
                        iwr
                    } else {
                        p.vanguard_default_rate
                    };
                    let target = rate * base + covered;
                    target
                        .max(planned * (1.0 - floor))
                        .min(planned * (1.0 + ceiling))
                }
            }
            SpendingRule::Vpw => {
                let rate = vpw_rate(inputs.expected_real_return, inputs.years_remaining)
                    .min(p.vpw_max_rate);
                rate * portfolio + other
            }
            SpendingRule::FloorCeiling {
                floor,
                ceiling,
                rate,
            } => {
                if first {
                    planned
                } else {
                    let rate = if let Some(r) = rate.filter(|r| r.is_finite() && *r > 0.0) {
                        r
                    } else if iwr > 0.0 {
                        iwr
                    } else {
                        p.floor_ceiling_default_rate
                    };
                    let target = rate * base + covered;
                    target.max(floor * budget).min(ceiling * budget)
                }
            }
        };

        let essential = inputs.essential_real.max(0.0).min(budget);
        let mut spending = if rule_spending.is_finite() {
            rule_spending.max(essential)
        } else {
            budget
        };
        if inputs.hold_raises && !first {
            spending = spending.min(planned.max(essential));
        }
        let multiple = if budget > 0.0 { spending / budget } else { 1.0 };
        let adjustment = if first || (multiple / previous - 1.0).abs() < 1e-12 {
            0.0
        } else {
            multiple / previous - 1.0
        };
        state.budget_multiple = multiple;
        state.last_real_spending = spending;
        state.years_retired = state.years_retired.saturating_add(1);
        SpendingDecision {
            spending_real: spending,
            budget_multiple: multiple,
            adjustment,
        }
    }
}

/// What the household needs from its accounts this year and what constrains it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PlanInputs {
    /// Cash needed from accounts this year: spending plus the tax due on
    /// `base_tax_inputs`, less all other income. Excludes the tax caused by the
    /// withdrawals themselves, which the plan adds. May be negative (a surplus).
    pub net_need: f64,
    /// Age of each person at the start of the year (0 for none).
    pub ages: [f64; MAX_PEOPLE],
    /// Required minimum distribution per person, already computed from last year-end
    /// balances.
    pub rmd: [f64; MAX_PEOPLE],
    /// The return as it stands before any withdrawal (wages of a still-working
    /// partner, Social Security, pensions, rent, dividends).
    pub base_tax_inputs: TaxInputs,
    /// True if anyone is on an ACA plan this year (MAGI matters for the subsidy).
    pub on_aca: bool,
    /// Annual payment per person that a 72(t) schedule already running commits them
    /// to this year (0 if none). It is always taken and never penalised.
    pub sepp_available: [f64; MAX_PEOPLE],
    /// True if the person's traditional and Roth withdrawals are exempt from the
    /// early-distribution penalty this year for a reason other than 72(t) (rule of
    /// 55, disability).
    pub penalty_exempt: [bool; MAX_PEOPLE],
    /// Qualified medical spending included in `net_need`, which HSA money may pay
    /// tax-free.
    pub medical_need: f64,
    /// Retirement cash bucket target: `retirement_cash_years` times this year's
    /// spending, in dollars. 0 disables the bucket.
    pub cash_target: f64,
    /// Cash the plan must leave untouched except as a last resort (0 if none).
    pub cash_floor: f64,
    /// True if equities are in a drawdown: the cash bucket is spent before anything
    /// is sold and is not refilled. False: cash above the target is spent and a
    /// bucket below target is refilled from withdrawals.
    pub in_drawdown: bool,
}

/// The withdrawal plan for one year: annual targets per source, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WithdrawalPlan {
    /// Spent out of the cash bucket (already cash; [`execute_plan`] moves nothing).
    pub from_cash: f64,
    /// From the taxable bucket.
    pub from_taxable: f64,
    /// From each person's traditional bucket, including RMDs and 72(t) payments.
    pub from_traditional: [f64; MAX_PEOPLE],
    /// From each person's Roth bucket.
    pub from_roth: [f64; MAX_PEOPLE],
    /// From each person's HSA for medical costs (tax-free).
    pub from_hsa: [f64; MAX_PEOPLE],
    /// From each person's HSA for other spending (taxable; a last resort).
    pub from_hsa_other: [f64; MAX_PEOPLE],
    /// Traditional-to-Roth conversion per person.
    pub roth_conversion: [f64; MAX_PEOPLE],
    /// The part of `from_traditional` taken under a 72(t) schedule (running or
    /// started by this plan), never penalised. A schedule, once started, must be
    /// continued: pass this back as `PlanInputs::sepp_available` every year until
    /// the later of five years and age 59 and a half.
    pub sepp: [f64; MAX_PEOPLE],
    /// Withdrawals in excess of the year's need that top the cash bucket back up to
    /// its target (included in the amounts above).
    pub cash_refill: f64,
    /// Forced withdrawals (RMDs, 72(t)) beyond the year's need. [`execute_plan`]
    /// leaves them in cash; `sim` reinvests them in the taxable bucket.
    pub excess: f64,
    /// Unrealised long-term gain to realise in the 0% bracket by selling and
    /// rebuying ([`Accounts::harvest_gains`]).
    pub gain_harvest: f64,
    /// Portion of the traditional and Roth withdrawals expected to bear the early
    /// penalty.
    pub penalized: f64,
    /// Tax estimated for the whole year under this plan (base income included), for
    /// withholding.
    pub estimated_tax: f64,
    /// Modified AGI estimated under this plan.
    pub estimated_magi: f64,
    /// Need the plan could not cover from any source (the path is heading for
    /// depletion).
    pub shortfall: f64,
}

impl WithdrawalPlan {
    /// Dollars the plan takes out of the invested buckets (everything except
    /// `from_cash` and conversions).
    pub fn withdrawals(&self) -> f64 {
        let mut t = self.from_taxable;
        for p in 0..MAX_PEOPLE {
            t += self.from_traditional[p]
                + self.from_roth[p]
                + self.from_hsa[p]
                + self.from_hsa_other[p];
        }
        t
    }
}

/// What [`execute_plan`] or [`cover_shortfall`] actually did.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Executed {
    /// Dollars moved from invested buckets into the cash bucket.
    pub to_cash: f64,
    /// Of which from the taxable bucket.
    pub from_taxable: f64,
    /// Of which from traditional buckets.
    pub from_traditional: f64,
    /// Of which from Roth buckets.
    pub from_roth: f64,
    /// Of which from HSAs.
    pub from_hsa: f64,
    /// Dollars converted from traditional to Roth.
    pub converted: f64,
    /// Long-term gain realised by sales and harvesting (negative for a loss).
    pub long_term_gain: f64,
    /// Traditional and Roth dollars that bear the 10% early-distribution penalty.
    pub penalty_base: f64,
    /// Requested dollars that no bucket could supply ([`cover_shortfall`] only).
    pub shortfall: f64,
}

/// Adds a withdrawal's tax character to the year's return.
///
/// * Taxable: `long_term_gains` (all gains are treated as long-term).
/// * Traditional: `traditional_distributions`; penalty base to `early_distributions`.
/// * Roth: taxable earnings to `other_ordinary`; penalty base (earnings and
///   unseasoned conversions before 59 and a half) to `early_distributions`.
/// * HSA: the non-medical part before 65 to `hsa_nonqualified` (taxed and penalised
///   at 20% by `tax::compute`), after 65 to `other_ordinary`.
pub fn record_withdrawal(inputs: &mut TaxInputs, bucket: Bucket, w: &Withdrawal) {
    match bucket {
        Bucket::Cash => {}
        Bucket::Taxable => inputs.long_term_gains += w.long_term_gain,
        Bucket::Traditional(_) => {
            inputs.traditional_distributions += w.ordinary;
            inputs.early_distributions += w.penalty_base;
        }
        Bucket::Roth(_) => {
            inputs.other_ordinary += w.ordinary;
            inputs.early_distributions += w.penalty_base;
        }
        Bucket::Hsa(_) => {
            inputs.hsa_nonqualified += w.penalty_base;
            inputs.other_ordinary += w.ordinary - w.penalty_base;
        }
    }
}

/// Carries out `fraction` of a plan (1.0 for the whole year at once, 1/12 for a
/// monthly instalment): sells in each bucket, moves the proceeds into the cash
/// bucket, converts traditional money to Roth and harvests gains. The tax character
/// of everything it does is added to `tax_inputs`. `sell_target` is the household
/// target mix; when given, sales take overweight asset classes first.
///
/// The plan's `from_cash` is cash already; nothing moves for it. Money is conserved:
/// [`Accounts::investable`] does not change. When a bucket holds less than planned
/// the shortfall simply is not raised (`to_cash` is lower); follow up with
/// [`cover_shortfall`] if the month cannot be paid.
pub fn execute_plan(
    plan: &WithdrawalPlan,
    fraction: f64,
    accounts: &mut Accounts,
    inputs: &PlanInputs,
    tax_inputs: &mut TaxInputs,
    sell_target: Option<&Weights>,
) -> Executed {
    let mut done = Executed::default();
    if fraction.is_nan() || fraction <= 0.0 {
        return done;
    }
    let prefer = sell_target.copied();
    let mut pull = |accounts: &mut Accounts,
                    done: &mut Executed,
                    bucket: Bucket,
                    amount: f64,
                    ctx: &WithdrawContext| {
        if amount <= 0.0 {
            return;
        }
        let w = accounts.withdraw(bucket, amount, ctx);
        accounts.cash += w.amount;
        record_withdrawal(tax_inputs, bucket, &w);
        done.to_cash += w.amount;
        done.long_term_gain += w.long_term_gain;
        match bucket {
            Bucket::Cash => {}
            Bucket::Taxable => done.from_taxable += w.amount,
            Bucket::Traditional(_) => {
                done.from_traditional += w.amount;
                done.penalty_base += w.penalty_base;
            }
            Bucket::Roth(_) => {
                done.from_roth += w.amount;
                done.penalty_base += w.penalty_base;
            }
            Bucket::Hsa(_) => done.from_hsa += w.amount,
        }
    };

    // A context is built only for a withdrawal that will be made (`pull` ignores
    // amounts that are not positive): most sources of most plans are untouched.
    let wanted = |amount: f64| amount > 0.0 || amount.is_nan();
    for p in 0..MAX_PEOPLE {
        let base = || WithdrawContext {
            owner_age: inputs.ages[p],
            qualified_medical: 0.0,
            penalty_exempt: inputs.penalty_exempt[p],
            prefer_overweight: prefer,
        };
        let medical = plan.from_hsa[p] * fraction;
        if wanted(medical) {
            pull(
                accounts,
                &mut done,
                Bucket::Hsa(p),
                medical,
                &WithdrawContext {
                    qualified_medical: medical,
                    ..base()
                },
            );
        }
        // RMDs go to people long past 59 and a half, so the only part of a
        // traditional withdrawal that needs an explicit exemption is the 72(t) one.
        let sepp = plan.sepp[p].min(plan.from_traditional[p]).max(0.0);
        if wanted(sepp * fraction) {
            pull(
                accounts,
                &mut done,
                Bucket::Traditional(p),
                sepp * fraction,
                &WithdrawContext {
                    penalty_exempt: true,
                    ..base()
                },
            );
        }
        let traditional = (plan.from_traditional[p] - sepp) * fraction;
        if wanted(traditional) {
            pull(
                accounts,
                &mut done,
                Bucket::Traditional(p),
                traditional,
                &base(),
            );
        }
        let roth = plan.from_roth[p] * fraction;
        if wanted(roth) {
            pull(accounts, &mut done, Bucket::Roth(p), roth, &base());
        }
        let other = plan.from_hsa_other[p] * fraction;
        if wanted(other) {
            pull(accounts, &mut done, Bucket::Hsa(p), other, &base());
        }
    }
    let taxable = plan.from_taxable * fraction;
    if wanted(taxable) {
        let taxable_ctx = WithdrawContext {
            prefer_overweight: prefer,
            ..WithdrawContext::default()
        };
        pull(accounts, &mut done, Bucket::Taxable, taxable, &taxable_ctx);
    }
    for (p, amount) in plan.roth_conversion.iter().enumerate() {
        let c = accounts.convert_to_roth(p, amount * fraction);
        tax_inputs.roth_conversions += c;
        done.converted += c;
    }
    let harvested = accounts.harvest_gains(plan.gain_harvest * fraction);
    tax_inputs.long_term_gains += harvested;
    done.long_term_gain += harvested;
    done
}

/// Annual amount of a 72(t) substantially-equal-periodic-payment schedule by the
/// fixed amortisation method: the level payment that amortises `balance` over the
/// owner's single life expectancy at `age` at interest `rate` (use
/// [`SEPP_MAX_RATE`] for the largest allowed payment).
pub fn sepp_payment(balance: f64, age: f64, rate: f64) -> f64 {
    if balance.is_nan() || age.is_nan() || rate.is_nan() || balance <= 0.0 {
        return 0.0;
    }
    let n = single_life_expectancy(age);
    if rate.abs() < 1e-9 {
        return balance / n;
    }
    balance * rate / (1.0 - math::pow(1.0 + rate, -n))
}

/// Single life expectancy at `age` from the IRS table, interpolated between the
/// five-year anchors and extended at the end slopes outside them (never below one
/// year).
fn single_life_expectancy(age: f64) -> f64 {
    let last = SINGLE_LIFE_EXPECTANCY.len() - 1;
    let x = (age - SINGLE_LIFE_FIRST_AGE) / SINGLE_LIFE_AGE_STEP;
    let i = if x <= 0.0 {
        0
    } else {
        (x.floor() as usize).min(last - 1)
    };
    let (a, b) = (SINGLE_LIFE_EXPECTANCY[i], SINGLE_LIFE_EXPECTANCY[i + 1]);
    (a + (b - a) * (x - i as f64)).max(1.0)
}

// Sources the allocator can draw on. Traditional and Roth have one slot per person;
// the Roth "extra" slots are the part a restricted (under 59 and a half) owner can
// only reach with tax or penalty.
const CASH: usize = 0;
const TAXABLE: usize = 1;
const TRAD: usize = 2;
const ROTH: usize = 4;
const ROTH_EXTRA: usize = 6;
const HSA: usize = 8;
const CASH_LOCKED: usize = 10;
const N_SOURCES: usize = 11;

/// Remaining capacity and amounts drawn per source while a plan is being built.
#[derive(Clone, Copy)]
struct Draw {
    rem: [f64; N_SOURCES],
    out: [f64; N_SOURCES],
    want: f64,
}

impl Draw {
    /// Draws up to `cap` of what is still wanted from the sources `idx`, in
    /// proportion to their remaining capacity. Returns the amount drawn.
    fn group(&mut self, idx: &[usize], cap: f64) -> f64 {
        if self.want <= 0.0 || cap.is_nan() || cap <= 0.0 {
            return 0.0;
        }
        let mut avail = 0.0;
        for &i in idx {
            avail += self.rem[i];
        }
        if avail <= 0.0 {
            return 0.0;
        }
        let take = self.want.min(avail).min(cap);
        let all = take >= avail;
        // Return the sum of what was actually moved, so callers that track a part of
        // a source (the 72(t) share of a traditional withdrawal) agree with it to the
        // last bit.
        let mut drawn = 0.0;
        for &i in idx {
            let x = if all {
                self.rem[i]
            } else {
                self.rem[i] * (take / avail)
            };
            self.rem[i] -= x;
            self.out[i] += x;
            drawn += x;
        }
        self.want = if take >= self.want {
            0.0
        } else {
            self.want - drawn
        };
        drawn
    }
}

/// What each source holds when a plan is made: the bucket totals the allocator
/// works from, summed once per plan rather than once per candidate.
#[derive(Clone, Copy)]
struct Balances {
    cash: f64,
    taxable: f64,
    traditional: [f64; MAX_PEOPLE],
    roth: [f64; MAX_PEOPLE],
    /// Roth money the owner can take at any age without tax or penalty.
    roth_accessible: [f64; MAX_PEOPLE],
    hsa: [f64; MAX_PEOPLE],
}

impl Balances {
    fn of(a: &Accounts) -> Balances {
        let mut b = Balances {
            cash: a.cash,
            taxable: a.total(Bucket::Taxable),
            traditional: [0.0; MAX_PEOPLE],
            roth: [0.0; MAX_PEOPLE],
            roth_accessible: [0.0; MAX_PEOPLE],
            hsa: [0.0; MAX_PEOPLE],
        };
        for p in 0..MAX_PEOPLE {
            b.traditional[p] = a.total(Bucket::Traditional(p));
            b.roth[p] = a.total(Bucket::Roth(p));
            b.roth_accessible[p] = a.roth_accessible(p);
            b.hsa[p] = a.total(Bucket::Hsa(p));
        }
        b
    }
}

/// Everything about the year that does not change between gross-up iterations.
struct Setup<'a> {
    /// The order actually followed (see [`effective_order`]).
    order: WithdrawalOrder,
    balances: Balances,
    inputs: &'a PlanInputs,
    /// Under 59 and a half, not exempt, and the plan avoids penalties (`Auto`).
    restricted: [bool; MAX_PEOPLE],
    /// Traditional dollars that fit under the configured bracket top, after forced
    /// withdrawals (infinite when there is no limit, 0 when not tax-aware).
    bracket_room: f64,
    /// MAGI dollars left under the ACA cliff after forced withdrawals (infinite when
    /// the cliff does not bind).
    aca_room: f64,
    /// Whether Roth conversions may be planned.
    convert: bool,
    /// The bracket rate being filled, to size the tax a conversion will cost.
    fill_rate: f64,
    /// Share of a taxable sale that is gain (0 when the bucket is at a loss).
    gain_share: f64,
    /// Most that may be converted to Roth this year (infinite until the plan has
    /// been costed: see `plan_year`).
    conversion_cap: f64,
}

impl Setup<'_> {
    /// Sources by person that this strategy may use freely.
    fn free_traditional(&self) -> ([usize; MAX_PEOPLE], usize) {
        let mut idx = [0; MAX_PEOPLE];
        let mut n = 0;
        for (p, restricted) in self.restricted.iter().enumerate() {
            if !restricted {
                idx[n] = TRAD + p;
                n += 1;
            }
        }
        (idx, n)
    }

    /// Chooses sources for a cash need of `need` (tax on the withdrawals included).
    fn allocate(&self, need: f64) -> WithdrawalPlan {
        let a = &self.balances;
        let inp = self.inputs;
        let mut plan = WithdrawalPlan::default();
        let mut d = Draw {
            rem: [0.0; N_SOURCES],
            out: [0.0; N_SOURCES],
            want: 0.0,
        };

        // Cash: in a drawdown the whole bucket above the floor is spendable; out of
        // one, only what exceeds the bucket target, and a low bucket is refilled.
        let cash = a.cash.max(0.0);
        let floor = inp.cash_floor.max(0.0);
        let target = inp.cash_target.max(0.0);
        let keep = if inp.in_drawdown {
            floor
        } else {
            floor.max(target)
        };
        d.rem[CASH] = (cash - keep).max(0.0);
        d.rem[CASH_LOCKED] = cash - d.rem[CASH];
        let refill = if inp.in_drawdown {
            0.0
        } else {
            (target - cash).max(0.0)
        };
        d.rem[TAXABLE] = a.taxable.max(0.0);
        for p in 0..MAX_PEOPLE {
            d.rem[TRAD + p] = a.traditional[p].max(0.0);
            let roth = a.roth[p].max(0.0);
            let free = if self.restricted[p] {
                a.roth_accessible[p]
            } else {
                roth
            };
            d.rem[ROTH + p] = free;
            d.rem[ROTH_EXTRA + p] = roth - free;
            d.rem[HSA + p] = a.hsa[p].max(0.0);
        }

        // Forced: RMDs and running 72(t) schedules.
        let mut left = need;
        for p in 0..MAX_PEOPLE {
            let sepp = inp.sepp_available[p].max(0.0).min(d.rem[TRAD + p]);
            let forced = (inp.rmd[p].max(0.0) + sepp).min(d.rem[TRAD + p]);
            d.rem[TRAD + p] -= forced;
            d.out[TRAD + p] = forced;
            plan.sepp[p] = sepp;
            left -= forced;
        }
        // HSA money pays medical costs first: it is the only tax-free use it has.
        let mut medical = inp.medical_need.max(0.0).min(left.max(0.0));
        for p in 0..MAX_PEOPLE {
            let x = d.rem[HSA + p].min(medical);
            d.rem[HSA + p] -= x;
            plan.from_hsa[p] = x;
            medical -= x;
            left -= x;
        }

        d.want = left + refill;
        plan.cash_refill = refill;
        let (free_trad, n_free) = self.free_traditional();
        let free_trad = &free_trad[..n_free];
        let mut bracket_used = 0.0;
        let mut costly = false;
        if d.want <= 0.0 {
            plan.excess = -d.want;
        } else {
            match self.order {
                WithdrawalOrder::Conventional => {
                    d.group(&[CASH], f64::INFINITY);
                    d.group(&[TAXABLE], f64::INFINITY);
                    d.group(free_trad, f64::INFINITY);
                    d.group(&[ROTH, ROTH + 1], f64::INFINITY);
                }
                WithdrawalOrder::Proportional => {
                    d.group(&[CASH], f64::INFINITY);
                    let mut idx = [TAXABLE, ROTH, ROTH + 1, 0, 0];
                    idx[3..3 + n_free].copy_from_slice(free_trad);
                    d.group(&idx[..3 + n_free], f64::INFINITY);
                }
                WithdrawalOrder::TaxAware { .. } => {
                    d.group(&[CASH], f64::INFINITY);
                    bracket_used = d.group(free_trad, self.bracket_room.min(self.aca_room));
                    let aca_left = self.aca_room - bracket_used;
                    let taxable_cap = if self.gain_share > 0.0 {
                        aca_left / self.gain_share
                    } else {
                        f64::INFINITY
                    };
                    d.group(&[TAXABLE], taxable_cap);
                    d.group(&[ROTH, ROTH + 1], f64::INFINITY);
                    d.group(&[TAXABLE], f64::INFINITY);
                    // Traditional money beyond the limits uses up any room a
                    // conversion might have had.
                    bracket_used += d.group(free_trad, f64::INFINITY);
                }
            }
            // Refilling the cash bucket is not worth a penalty or a depleted HSA:
            // drop it before touching the costly sources.
            let dropped = refill.min(d.want);
            plan.cash_refill = refill - dropped;
            d.want -= dropped;
            costly = d.want > 0.0;
            // 72(t): start a schedule of at most the amortisation payment. A
            // schedule already running is fixed: what its owner needs beyond it is
            // an ordinary early withdrawal.
            for p in 0..MAX_PEOPLE {
                if self.restricted[p] && d.want > 0.0 && inp.sepp_available[p] <= 0.0 {
                    let cap = sepp_payment(d.rem[TRAD + p], inp.ages[p], SEPP_MAX_RATE);
                    plan.sepp[p] += d.group(&[TRAD + p], cap);
                }
            }
            // Last resorts, in order: penalised traditional money, Roth earnings and
            // unseasoned conversions, HSA money for non-medical spending, the cash
            // floor.
            d.group(&[TRAD, TRAD + 1], f64::INFINITY);
            d.group(&[ROTH_EXTRA, ROTH_EXTRA + 1], f64::INFINITY);
            for (p, taken) in plan.from_hsa_other.iter_mut().enumerate() {
                *taken = d.group(&[HSA + p], f64::INFINITY);
            }
            d.group(&[CASH_LOCKED], f64::INFINITY);
            plan.shortfall = d.want.max(0.0);
        }

        // Roth conversions use what is left of the bracket, only when the year is
        // comfortably funded and taxable money can pay the tax on them.
        if self.convert && !costly && self.bracket_room.is_finite() {
            let aca_left = self.aca_room - bracket_used - d.out[TAXABLE] * self.gain_share.max(0.0);
            let mut room = (self.bracket_room - bracket_used)
                .min(aca_left)
                .min(self.conversion_cap)
                .max(0.0);
            if self.fill_rate > 0.0 {
                room = room.min((d.rem[CASH] + d.rem[TAXABLE]) / self.fill_rate);
            }
            let mut convertible = 0.0;
            for p in 0..MAX_PEOPLE {
                if plan.sepp[p] <= 0.0 {
                    convertible += d.rem[TRAD + p];
                }
            }
            if room > CENT && convertible > 0.0 {
                let share = (room / convertible).min(1.0);
                for p in 0..MAX_PEOPLE {
                    if plan.sepp[p] <= 0.0 {
                        plan.roth_conversion[p] = d.rem[TRAD + p] * share;
                    }
                }
            }
        }
        plan.from_cash = d.out[CASH] + d.out[CASH_LOCKED];
        plan.from_taxable = d.out[TAXABLE];
        for p in 0..MAX_PEOPLE {
            plan.from_traditional[p] = d.out[TRAD + p];
            plan.from_roth[p] = d.out[ROTH + p] + d.out[ROTH_EXTRA + p];
        }
        plan
    }
}

/// Federal taxable income that is taxed at ordinary rates.
fn ordinary_taxable(result: &TaxResult, inputs: &TaxInputs) -> f64 {
    let preferential = inputs.qualified_dividends.max(0.0) + inputs.long_term_gains.max(0.0);
    (result.taxable_income - preferential.min(result.taxable_income.max(0.0))).max(0.0)
}

/// Taxable income at which the highest bracket taxed at `rate` or less ends
/// (infinite when that is the top bracket, 0 when every bracket is taxed more).
fn bracket_top(brackets: &Brackets, rate: f64) -> f64 {
    let mut top = 0.0;
    for i in 0..brackets.len {
        if brackets.rate[i] <= rate + 1e-9 {
            top = if i + 1 < brackets.len {
                brackets.lower[i + 1]
            } else {
                f64::INFINITY
            };
        }
    }
    top
}

/// The order actually followed: bracket management only means something under US
/// law, so with a flat tax, no tax or a foreign flat rate `TaxAware` falls back to the
/// conventional order.
fn effective_order(order: &WithdrawalOrder, tax: &TaxYearParams) -> WithdrawalOrder {
    let us = tax.kind == TaxKind::UsCurrentLaw && tax.foreign_tax_rate.is_none();
    match order {
        WithdrawalOrder::TaxAware { .. } if !us => WithdrawalOrder::Conventional,
        other => *other,
    }
}

/// Builds the year's withdrawal plan.
///
/// 1. RMDs and running 72(t) payments are taken first; HSA money pays medical costs.
/// 2. The rest of the need is drawn in the configured order:
///    * `Conventional`: spendable cash, taxable, traditional, Roth.
///    * `Proportional`: spendable cash, then taxable, traditional and Roth in
///      proportion to their size.
///    * `TaxAware`: spendable cash; traditional up to the top of the
///      `fill_to_bracket` bracket (and, with `mind_aca` on an ACA plan, up to the
///      400%-of-poverty MAGI cliff); taxable (also only as far as its gains fit under
///      the cliff); Roth; then, the limits having been honoured as far as possible,
///      more taxable and more traditional. If the year is funded without costly
///      sources and `roth_conversions` is on, what is left of the bracket is filled
///      with a Roth conversion, limited to what taxable money can pay the tax on.
///      Finally, unrealised gains are harvested in the 0% bracket when that is free.
///      Under a flat tax, no tax or a foreign flat rate there are no brackets to
///      manage and `TaxAware` behaves like `Conventional`.
/// 3. Owners under 59 and a half (unless `penalty_exempt`): with `EarlyAccess::Auto`
///    their traditional money and the taxable-or-penalised part of their Roth are
///    kept out of step 2. If the need is still not met: a 72(t) schedule (at most the
///    amortisation payment at 5%), then penalised traditional withdrawals, then Roth
///    earnings and unseasoned conversions. With `PayPenalty` they are treated like
///    anyone else and the penalty is paid.
/// 4. Last resorts for everyone: HSA money for non-medical spending, then cash below
///    `cash_floor`; what is still missing is the `shortfall`.
/// 5. The retirement cash bucket: see [`PlanInputs::in_drawdown`].
///
/// The tax the withdrawals cause is estimated with `tax::compute` on the amounts as
/// they would actually be classified by [`Accounts::withdraw`], and the plan is
/// grossed up until it also covers that tax (a fixed-point step, then secant steps
/// until within a cent: three evaluations when the marginal rate is constant over
/// the range, at most six).
pub fn plan_year(
    order: &WithdrawalOrder,
    early_access: EarlyAccess,
    accounts: &Accounts,
    inputs: &PlanInputs,
    tax: &TaxYearParams,
) -> WithdrawalPlan {
    plan_year_impl(
        order,
        early_access,
        accounts,
        inputs,
        tax,
        tax::compute,
        None,
    )
}

/// [`plan_year`] when the caller has already computed the return on
/// `inputs.base_tax_inputs` with the same parameters (saves one tax computation).
pub fn plan_year_from(
    order: &WithdrawalOrder,
    early_access: EarlyAccess,
    accounts: &Accounts,
    inputs: &PlanInputs,
    tax: &TaxYearParams,
    base_result: &TaxResult,
) -> WithdrawalPlan {
    plan_year_impl(
        order,
        early_access,
        accounts,
        inputs,
        tax,
        tax::compute,
        Some(*base_result),
    )
}

/// [`plan_year`] with the tax function supplied by the caller (tests, what-if tools).
pub fn plan_year_with(
    order: &WithdrawalOrder,
    early_access: EarlyAccess,
    accounts: &Accounts,
    inputs: &PlanInputs,
    tax: &TaxYearParams,
    compute: &dyn Fn(&TaxYearParams, &TaxInputs) -> TaxResult,
) -> WithdrawalPlan {
    plan_year_impl(order, early_access, accounts, inputs, tax, compute, None)
}

/// One evaluation of a candidate plan: the plan, the return under it, and what
/// carrying it out did to a copy of the accounts.
struct Eval {
    plan: WithdrawalPlan,
    t: TaxInputs,
    done: Executed,
    after: Accounts,
    result: TaxResult,
}

/// What a discretionary dollar of income is judged by when it is compared with the
/// bracket being filled: federal tax plus the premium credit it loses. State tax is
/// left out because `fill_to_bracket` names a federal bracket and the state will tax
/// the money whenever it comes out.
fn burden(r: &TaxResult) -> f64 {
    r.total - r.state - r.foreign - r.aca_premium_credit
}

/// Grosses a plan up for the tax its own withdrawals cause: finds the cash amount
/// `g` with `g = net_need + extra_tax(g)`, by a fixed-point step followed by secant
/// steps on `f(g) = g - net_need - extra_tax(g)`. The result is left in `best`.
fn gross_up<F: Fn(&TaxYearParams, &TaxInputs) -> TaxResult>(
    setup: &Setup,
    accounts: &Accounts,
    tax: &TaxYearParams,
    compute: &F,
    base_total: f64,
    best: &mut Eval,
) {
    let evaluate = |need: f64, e: &mut Eval| {
        e.plan = setup.allocate(need);
        e.after = *accounts;
        e.t = setup.inputs.base_tax_inputs;
        e.done = execute_plan(&e.plan, 1.0, &mut e.after, setup.inputs, &mut e.t, None);
        e.result = compute(tax, &e.t);
    };
    let n = setup.inputs.net_need;
    let residual = |g: f64, r: &TaxResult| g - n - (r.total - base_total);
    let mut ga = n;
    evaluate(ga, best);
    let mut fa = residual(ga, &best.result);
    if fa.abs() > CENT {
        let mut gb = ga - fa;
        evaluate(gb, best);
        let mut fb = residual(gb, &best.result);
        for _ in 0..GROSS_UP_SECANT_STEPS {
            if fb.abs() <= CENT || (gb - ga).abs() <= CENT {
                break;
            }
            // f' = 1 - marginal rate; keep it away from zero across cliffs.
            let slope = ((fb - fa) / (gb - ga)).max(1.0 - MAX_GROSS_UP_RATE);
            let gc = gb - fb / slope;
            ga = gb;
            fa = fb;
            gb = gc;
            evaluate(gb, best);
            fb = residual(gb, &best.result);
        }
    }
}

#[inline]
fn plan_year_impl<F: Fn(&TaxYearParams, &TaxInputs) -> TaxResult>(
    order: &WithdrawalOrder,
    early_access: EarlyAccess,
    accounts: &Accounts,
    inputs: &PlanInputs,
    tax: &TaxYearParams,
    compute: F,
    known_base: Option<TaxResult>,
) -> WithdrawalPlan {
    let base = &inputs.base_tax_inputs;
    let base_result = match known_base {
        Some(r) => r,
        None => compute(tax, base),
    };
    let status = base.filing_status.index();

    let mut restricted = [false; MAX_PEOPLE];
    for p in 0..MAX_PEOPLE {
        restricted[p] = early_access == EarlyAccess::Auto
            && inputs.ages[p] < EARLY_WITHDRAWAL_AGE
            && !inputs.penalty_exempt[p];
    }
    let balances = Balances::of(accounts);
    let taxable_value = balances.taxable;
    let order = &effective_order(order, tax);
    let mut setup = Setup {
        order: *order,
        balances,
        inputs,
        restricted,
        bracket_room: 0.0,
        aca_room: f64::INFINITY,
        convert: false,
        fill_rate: 0.0,
        gain_share: if taxable_value > 0.0 {
            (accounts.unrealized_gain() / taxable_value).clamp(0.0, 1.0)
        } else {
            0.0
        },
        conversion_cap: f64::INFINITY,
    };

    let mut aca_cliff = f64::INFINITY;
    if let WithdrawalOrder::TaxAware {
        fill_to_bracket,
        roth_conversions,
        mind_aca,
    } = *order
    {
        // The return with only the forced withdrawals on it.
        let mut forced = *base;
        let mut any_forced = false;
        for p in 0..MAX_PEOPLE {
            let f = (inputs.rmd[p].max(0.0) + inputs.sepp_available[p].max(0.0))
                .min(balances.traditional[p]);
            if f > 0.0 {
                forced.traditional_distributions += f;
                any_forced = true;
            }
        }
        let forced_result = if any_forced {
            compute(tax, &forced)
        } else {
            base_result
        };

        {
            let top = bracket_top(&tax.federal.ordinary[status], fill_to_bracket);
            let at = |x: f64| {
                let mut t = forced;
                t.traditional_distributions += x;
                ordinary_taxable(&compute(tax, &t), &t)
            };
            let start = ordinary_taxable(&forced_result, &forced);
            let gap = top - start;
            setup.bracket_room = if gap <= 0.0 {
                0.0
            } else if !gap.is_finite() {
                f64::INFINITY
            } else {
                // One traditional dollar is usually one dollar of taxable income, but
                // not below the standard deduction or while it drags Social Security
                // into tax: two secant steps find the amount that lands on the top.
                let x0 = gap;
                let y0 = at(x0);
                let slope = (y0 - start) / x0;
                let x1 = if slope > 0.0 { gap / slope } else { x0 };
                if (x1 - x0).abs() <= CENT {
                    x1
                } else {
                    let y1 = at(x1);
                    let slope = (y1 - y0) / (x1 - x0);
                    if slope > 0.0 {
                        (x1 + (top - y1) / slope).max(0.0)
                    } else {
                        x1
                    }
                }
            };
            setup.convert = roth_conversions;
            setup.fill_rate = fill_to_bracket.max(0.0);
        }

        if mind_aca && inputs.on_aca && base.aca_household_size > 0 {
            let people = f64::from(base.aca_household_size);
            let poverty_line = tax.federal.fpl_base + tax.federal.fpl_per_person * (people - 1.0);
            let cliff = ACA_CLIFF_FPL_MULTIPLE * poverty_line;
            // Already over the cliff on other income: there is nothing to protect.
            if cliff > 0.0 && forced_result.magi < cliff {
                aca_cliff = cliff * (1.0 - ACA_PLANNING_MARGIN);
                setup.aca_room = (aca_cliff - forced_result.magi).max(0.0);
                // On the marketplace a traditional dollar costs its tax and the
                // premium credit it loses (10 to 17 cents between 138% and 400% of
                // the poverty line). Fill the bracket only as far as the two
                // together stay within the rate being filled: first the unused
                // deduction, then the rest of the bracket.
                let limit = setup.bracket_room.min(setup.aca_room);
                if limit > CENT {
                    let allowed = fill_to_bracket.max(0.0) + FILL_RATE_TOLERANCE;
                    let unused_deduction = if forced_result.taxable_income <= 0.0 {
                        (forced_result.deduction - forced_result.agi)
                            .max(0.0)
                            .min(limit)
                    } else {
                        0.0
                    };
                    let mut room = 0.0;
                    let mut cost = burden(&forced_result);
                    for x in [unused_deduction, limit] {
                        if x - room <= CENT {
                            continue;
                        }
                        let mut t = forced;
                        t.traditional_distributions += x;
                        let c = burden(&compute(tax, &t));
                        if c - cost > allowed * (x - room) {
                            break;
                        }
                        room = x;
                        cost = c;
                    }
                    setup.bracket_room = room;
                }
            }
        }
    }

    let mut best = Eval {
        plan: WithdrawalPlan::default(),
        t: *base,
        done: Executed::default(),
        after: *accounts,
        result: base_result,
    };
    gross_up(
        &setup,
        accounts,
        tax,
        &compute,
        base_result.total,
        &mut best,
    );

    // A conversion is sized on the ordinary bracket, but its real cost includes
    // what it does to the rest of the return: gains realised by this year's sales
    // (and qualified dividends) are pushed from the 0% rate to 15%, Social Security
    // becomes taxable, the premium credit shrinks. Keep only the part whose federal
    // tax and lost credit per converted dollar stay within the rate being filled.
    let converted: f64 = best.plan.roth_conversion.iter().sum();
    if converted > CENT {
        let allowed = setup.fill_rate + FILL_RATE_TOLERANCE;
        let mut without = best.t;
        without.roth_conversions = (without.roth_conversions - converted).max(0.0);
        let r0 = compute(tax, &without);
        let preferential = without.long_term_gains.max(0.0) + without.qualified_dividends.max(0.0);
        let zero_top = bracket_top(&tax.federal.capital_gains[status], 0.0);
        // Up to here the conversion leaves the 0% gains bracket alone.
        let kink = if preferential > CENT && zero_top.is_finite() {
            (zero_top - r0.taxable_income).clamp(0.0, converted)
        } else {
            0.5 * converted
        };
        let mut cap = 0.0;
        let mut cost = burden(&r0);
        for x in [kink, converted] {
            if x - cap <= CENT {
                continue;
            }
            let c = if x >= converted {
                burden(&best.result)
            } else {
                let mut t = without;
                t.roth_conversions += x;
                burden(&compute(tax, &t))
            };
            if c - cost > allowed * (x - cap) {
                break;
            }
            cap = x;
            cost = c;
        }
        if cap < converted - CENT {
            setup.conversion_cap = cap;
            gross_up(
                &setup,
                accounts,
                tax,
                &compute,
                base_result.total,
                &mut best,
            );
        }
    }
    let Eval {
        mut plan,
        t,
        done,
        after,
        mut result,
    } = best;

    // Harvest gains in the 0% bracket, if that is really free.
    if matches!(order, WithdrawalOrder::TaxAware { .. }) && plan.shortfall <= 0.0 {
        let zero_top = bracket_top(&tax.federal.capital_gains[status], 0.0);
        let mut room = zero_top - result.taxable_income;
        if aca_cliff.is_finite() {
            room = room.min(aca_cliff - result.magi);
        }
        if room.is_finite() && room > CENT {
            let mut probe = after;
            let gain = probe.harvest_gains(room);
            if gain > CENT {
                let mut th = t;
                th.long_term_gains += gain;
                let rh = compute(tax, &th);
                let cost =
                    (rh.total - rh.aca_premium_credit) - (result.total - result.aca_premium_credit);
                if cost <= HARVEST_MAX_COST * gain {
                    plan.gain_harvest = gain;
                    result = rh;
                }
            }
        }
    }

    plan.penalized = done.penalty_base;
    plan.estimated_tax = result.total;
    plan.estimated_magi = result.magi;
    plan
}

/// Raises `amount` of cash for an unplanned mid-year deficit and moves it into the
/// cash bucket: taxable first, then traditional, then Roth, with the same early-access
/// rules as [`plan_year`] (`ages` and `penalty_exempt` are read from `inputs`; a new
/// 72(t) schedule is never started here). No tax gross-up: the tax character is
/// added to `tax_inputs` and settles with the year's return.
pub fn cover_shortfall(
    amount: f64,
    early_access: EarlyAccess,
    accounts: &mut Accounts,
    inputs: &PlanInputs,
    tax_inputs: &mut TaxInputs,
    sell_target: Option<&Weights>,
) -> Executed {
    if amount.is_nan() || amount <= 0.0 {
        return Executed::default();
    }
    let mut restricted = [false; MAX_PEOPLE];
    for p in 0..MAX_PEOPLE {
        restricted[p] = early_access == EarlyAccess::Auto
            && inputs.ages[p] < EARLY_WITHDRAWAL_AGE
            && !inputs.penalty_exempt[p];
    }
    // Only ages and exemptions carry over: no forced amounts, and the cash bucket is
    // left alone (the caller has already used what it wanted to use of it).
    let bare = PlanInputs {
        ages: inputs.ages,
        penalty_exempt: inputs.penalty_exempt,
        ..PlanInputs::default()
    };
    let mut balances = Balances::of(accounts);
    balances.cash = 0.0;
    let setup = Setup {
        order: WithdrawalOrder::Conventional,
        balances,
        inputs: &bare,
        restricted,
        bracket_room: 0.0,
        aca_room: f64::INFINITY,
        convert: false,
        fill_rate: 0.0,
        gain_share: 0.0,
        conversion_cap: f64::INFINITY,
    };
    let mut plan = setup.allocate(amount);
    // Without a schedule the "72(t)" tier is just an early withdrawal.
    plan.sepp = [0.0; MAX_PEOPLE];
    let mut done = execute_plan(&plan, 1.0, accounts, &bare, tax_inputs, sell_target);
    done.shortfall = (amount - done.to_cash).max(0.0);
    done
}

#[cfg(test)]
mod tests;
