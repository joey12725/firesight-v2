//! Retirement spending rules and the annual withdrawal plan. **Interface stub: bodies
//! are `todo!()`.**
//!
//! Responsibility, once a year in January for a retired household:
//!
//! 1. [`SpendingPlanner`]: this year's spending under the chosen rule (constant real,
//!    percent of portfolio, guardrails, Vanguard dynamic, VPW, floor and ceiling),
//!    expressed as a multiple of the retirement budget so `sim` can scale
//!    non-essential lines first.
//! 2. [`plan_year`]: where the money comes from: RMDs first, then the chosen
//!    withdrawal order, early-access rules before 59 and a half, a Roth conversion
//!    amount when tax-aware, respecting bracket and ACA limits.
//!
//! Contract:
//!
//! * Pure functions of their inputs plus the small [`SpendingState`]; no randomness.
//! * The plan is a set of annual targets. `sim` executes it month by month and
//!   deviates when a bucket runs dry; the plan is not a guarantee.
//! * `plan_year` may call `tax::compute` a handful of times (bracket search); it must
//!   not be called more than once a year per path.
//! * All amounts are nominal dollars of the planning year unless a field says "real".

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use crate::accounts::Accounts;
use crate::model::{EarlyAccess, SpendingRule, WithdrawalOrder};
use crate::tax::{TaxInputs, TaxYearParams};
use crate::MAX_PEOPLE;

/// State a spending rule carries from year to year on a path.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SpendingState {
    /// Withdrawal rate in the first year of retirement (guardrails reference).
    pub initial_withdrawal_rate: f64,
    /// Last year's spending in today's dollars (Vanguard dynamic reference).
    pub last_real_spending: f64,
    /// Current multiple of the retirement budget being spent (1.0 = on plan).
    pub budget_multiple: f64,
    /// Number of years retired so far.
    pub years_retired: u32,
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
    /// Years left until the end of the plan (for VPW).
    pub years_remaining: f64,
    /// Expected real return of the current allocation (for VPW).
    pub expected_real_return: f64,
}

/// The rule's decision for one year.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SpendingDecision {
    /// Total spending for the year in today's dollars.
    pub spending_real: f64,
    /// `spending_real / budget_real`: 1.0 on plan, below 1 when the rule cuts.
    pub budget_multiple: f64,
    /// Change relative to last year's multiple, for the event log ("guardrail cut
    /// spending 10%"); 0 when unchanged.
    pub adjustment: f64,
}

/// A compiled spending rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpendingPlanner {
    /// The rule from the profile.
    pub rule: SpendingRule,
}

impl SpendingPlanner {
    /// Wraps a rule.
    pub fn new(rule: SpendingRule) -> SpendingPlanner {
        SpendingPlanner { rule }
    }

    /// Initialises the state in the first retirement year.
    pub fn start(&self, inputs: &SpendingInputs) -> SpendingState {
        todo!("decumulation owner")
    }

    /// This year's spending; updates `state`.
    pub fn decide(&self, state: &mut SpendingState, inputs: &SpendingInputs) -> SpendingDecision {
        todo!("decumulation owner")
    }
}

/// What the household needs from its accounts this year and what constrains it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PlanInputs {
    /// Cash needed from accounts this year after all other income, before the tax
    /// caused by the withdrawals themselves.
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
    /// Amount per person available under a 72(t) schedule this year (0 if none).
    pub sepp_available: [f64; MAX_PEOPLE],
}

/// The withdrawal plan for one year: annual targets per source, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct WithdrawalPlan {
    /// From the cash bucket.
    pub from_cash: f64,
    /// From the taxable bucket.
    pub from_taxable: f64,
    /// From each person's traditional bucket, including RMDs.
    pub from_traditional: [f64; MAX_PEOPLE],
    /// From each person's Roth bucket.
    pub from_roth: [f64; MAX_PEOPLE],
    /// From each person's HSA for medical costs.
    pub from_hsa: [f64; MAX_PEOPLE],
    /// Traditional-to-Roth conversion per person.
    pub roth_conversion: [f64; MAX_PEOPLE],
    /// Portion of the traditional and Roth withdrawals expected to bear the early
    /// penalty.
    pub penalized: f64,
    /// Tax estimated for the year under this plan, for withholding.
    pub estimated_tax: f64,
    /// Need the plan could not cover from any source (the path is heading for
    /// depletion).
    pub shortfall: f64,
}

/// Builds the year's withdrawal plan.
pub fn plan_year(
    order: &WithdrawalOrder,
    early_access: EarlyAccess,
    accounts: &Accounts,
    inputs: &PlanInputs,
    tax: &TaxYearParams,
) -> WithdrawalPlan {
    todo!("decumulation owner")
}

/// Annual amount of a 72(t) substantially-equal-periodic-payment schedule by the
/// amortisation method for a balance, age and interest rate.
pub fn sepp_payment(balance: f64, age: f64, rate: f64) -> f64 {
    todo!("decumulation owner")
}
