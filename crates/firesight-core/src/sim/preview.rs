//! Deterministic facts about a compiled plan, for the editor to show while the user
//! types: no paths are simulated.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::economy::EconomyPath;

use super::plan::Plan;
use super::state::State;

/// What a plan implies before any randomness, in today's dollars.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlanPreview {
    /// Spending per year now: the budget, owned-home costs, modelled healthcare,
    /// modelled child costs and debt payments.
    pub annual_spending: f64,
    /// Spending per year if the household retired today, on the same basis.
    pub annual_spending_retired: f64,
    /// Gross earned income per year now (pay, target bonus, commission, business
    /// profit).
    pub annual_income: f64,
    /// Share of gross income not spent or paid in tax this year (an estimate from
    /// the first month's plan; includes retirement-plan contributions).
    pub savings_rate: f64,
    /// Investable assets today: cash, taxable, retirement and health accounts.
    pub investable: f64,
    /// Wealth the funded test counts today.
    pub wealth: f64,
    /// The amount the funded test asks for today (`when_funded` with the profile's
    /// rule, or the default rule for other triggers).
    pub funded_requirement: f64,
    /// `wealth / funded_requirement`; null when the requirement is zero.
    pub funded_ratio: Option<f64>,
    /// Annual spending the funded test has to finance from savings, before tax.
    pub retirement_need: f64,
    /// Tax per year on the withdrawals that pay for it.
    pub retirement_tax: f64,
    /// Present value of Social Security and pensions not yet started that the
    /// funded test counts.
    pub future_income_value: f64,
    /// Estimated tax this year on current income.
    pub annual_tax: f64,
    /// Number of calendar years in the plan (the length of every per-year array).
    pub horizon_years: u32,
    /// Remaining life expectancy of each adult, years.
    pub life_expectancy: Vec<f64>,
    /// Age each adult is expected to reach (age now plus life expectancy).
    pub expected_age_at_death: Vec<f64>,
}

impl Plan {
    /// Computes the [`PlanPreview`]. Costs about as much as one simulated month
    /// plus the workspace allocation.
    pub fn preview(&self) -> PlanPreview {
        let mut econ = EconomyPath::with_capacity(self.horizon_months() as usize);
        let mut state = State::new(self);
        // The preview is deterministic: it reads the state at the first month, when
        // the price level is 1 and nothing random has been used.
        state.begin(self, 0, 0, &mut econ, false, false);
        // A flat economy for the first month's plan (interest estimates only).
        econ.tbill.fill(0.03);
        if !state.retired && state.retire_month <= 0 {
            state.retired = true;
            state.retire_month = 0;
            state.person_retired = [true; crate::MAX_PEOPLE];
        }
        state.annual_plan(self, &econ, 0, false);
        let rule = state.funding_rule(self);
        let req = state.requirement(self, 0, 1.0, &rule);
        let wealth = state.wealth(rule.basis, 1.0);
        let rate_now = state.run_rate(self, 0, 1.0, state.retired);
        let rate_retired = state.run_rate(self, 0, 1.0, true);
        let (ess, disc, _) = state.budget_as(self, 0, state.retired, state.retire_month);
        let (ess_r, disc_r, _) = state.budget_as(self, 0, true, 0);
        let annual_spending = (ess + disc + rate_now.total()) * 12.0;
        let annual_spending_retired = (ess_r + disc_r + rate_retired.total()) * 12.0;
        let annual_income = self.income_rate0;
        let annual_tax = (state.wh_rate * annual_income + state.installment * 12.0)
            + annual_income.min(state.params.federal.oasdi_wage_base)
                * if state.fica_on { 0.0765 } else { 0.0 };
        let savings_rate = if annual_income > 0.0 {
            ((annual_income - annual_tax - annual_spending) / annual_income).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let life_expectancy = self.life_expectancies();
        let expected_age_at_death = self
            .people
            .iter()
            .zip(&life_expectancy)
            .map(|(p, e)| f64::from(p.age0) / 12.0 + e)
            .collect();
        PlanPreview {
            annual_spending,
            annual_spending_retired,
            annual_income,
            savings_rate,
            investable: state.acc.investable(),
            wealth,
            funded_requirement: req.total,
            funded_ratio: (req.total > 0.0).then(|| wealth / req.total),
            retirement_need: req.need,
            retirement_tax: req.tax,
            future_income_value: req.pv_income,
            annual_tax,
            horizon_years: self.horizon_years(),
            life_expectancy,
            expected_age_at_death,
        }
    }
}
