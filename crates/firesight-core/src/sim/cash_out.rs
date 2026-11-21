//! Cash-out refinancing and rate-aware mortgage prepayment
//! (`debt_policy.cash_out_refinance`, `debt_policy.mortgage_prepayment`).
//!
//! # Cash-out refinance
//!
//! ```text
//! when       money would be withdrawn with the 10% early-withdrawal penalty: the
//!            year's withdrawal plan contains penalised withdrawals, or a mid-year
//!            shortfall would be covered from penalised sources (both found by
//!            running the withdrawal on a copy of the accounts)
//! where      the main home, with no reverse mortgage on it; at most once in 12 months
//! new loan   L = min(max_ltv x value, payoff + costs + raise)
//!            payoff = mortgage + any home-equity line, repaid out of L
//!            costs  = 2% x L + $2,000 (today's dollars), the same as a rate refinance
//!            raise  = the penalised amount, at least 12 months of planned spending
//! terms      market 30-year rate + rate_premium, amortised over term_years
//! cash       L - payoff - costs, into cash (booked as cash from property)
//! ```
//!
//! Nothing is drawn at random. The new loan is an ordinary mortgage afterwards: it
//! is refinanced when rates fall, prepaid under the prepayment rule and repaid at a
//! sale.
//!
//! # Prepayment
//!
//! When surplus cash is about to be invested, the share of it the savings waterfall
//! would put in the taxable account (found by running the waterfall on a copy) pays
//! down the highest-rate mortgage on a home or second home instead, while
//!
//! ```text
//! default            mortgage rate > 10-year Treasury yield x (1 - marginal rate)
//! real_rate given    mortgage rate - last year's inflation > real_rate
//! ```
//!
//! (with the default, expected inflation is on both sides and cancels). Tax-advantaged
//! contributions come first, as before.
//!
//! Simplifications: the lender asks for nothing but equity (no income test, no
//! credit score); interest on the cash-out part is deducted like acquisition debt
//! (it is not deductible unless spent on the home, but few households itemise);
//! lenders do not freeze refinancing in downturns; the mortgage interest deduction
//! is ignored in the prepayment comparison.

use crate::accounts::Accounts;
use crate::decumulation::{cover_shortfall, plan_year_from};
use crate::home_equity::LineKind;
use crate::housing::{amortized_payment, MortgageState};
use crate::model::{CashOutRefinance, MortgagePrepayment, PropertyUse};
use crate::stats::{TraceEventKind, YearSeries};
use crate::tax::TaxResult;

use super::plan::Plan;
use super::state::State;
use super::year::{REFINANCE_COST_FIXED, REFINANCE_COST_FRACTION};

/// Months between two cash-out refinances. A judgement: closing costs make a
/// refinance more often than yearly uneconomic, and lenders commonly require some
/// months of seasoning.
pub const CASH_OUT_MIN_MONTHS: i32 = 12;
/// A cash-out refinance raises at least this many months of planned spending, so
/// that one refinance carries the household for a while. A judgement.
pub const CASH_OUT_RAISE_MONTHS: f64 = 12.0;
/// Smallest cash-out worth its closing costs, today's dollars. A judgement.
pub const CASH_OUT_MIN_CASH: f64 = 10_000.0;
/// Penalised dollars below this are ignored (rounding of the withdrawal plan).
const PENALTY_DUST: f64 = 1.0;

/// Compiled cash-out terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CashOutPlan {
    pub max_ltv: f64,
    pub premium: f64,
    pub months: u32,
}

impl CashOutPlan {
    pub(crate) fn compile(c: &CashOutRefinance) -> CashOutPlan {
        CashOutPlan {
            max_ltv: c.max_ltv.clamp(0.0, 1.0),
            premium: c.rate_premium,
            months: c.term_years.max(1) * 12,
        }
    }
}

/// Compiled prepayment rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Prepay {
    Never,
    /// Mortgage rate above the after-tax 10-year yield.
    AfterTaxYield,
    /// Mortgage real rate above this.
    RealRate(f64),
}

impl Prepay {
    pub(crate) fn compile(m: &MortgagePrepayment) -> Prepay {
        match *m {
            MortgagePrepayment::Never => Prepay::Never,
            MortgagePrepayment::WhenRateAbove { real_rate: None } => Prepay::AfterTaxYield,
            MortgagePrepayment::WhenRateAbove { real_rate: Some(r) } => Prepay::RealRate(r),
        }
    }
}

impl State {
    /// Hook in the annual plan, before the year's withdrawals are planned: if the
    /// plan as it stands would withdraw money with the penalty, refinance for cash
    /// first (the plan then spends that cash before selling anything).
    pub(crate) fn hx_cash_out_before_plan(
        &mut self,
        plan: &Plan,
        t: usize,
        price: f64,
        mortgage_rate: f64,
        base_result: &TaxResult,
    ) {
        let Some(terms) = plan.hx.cash_out else {
            return;
        };
        if !self.hx_cash_out_possible(t) {
            return;
        }
        let probe = plan_year_from(
            &plan.profile.retirement.withdrawal_order,
            plan.profile.retirement.early_access,
            &self.acc,
            &self.pinputs,
            &self.params,
            base_result,
        );
        if probe.penalized > PENALTY_DUST {
            self.hx_cash_out(plan, &terms, t, price, mortgage_rate, probe.penalized);
        }
    }

    /// Hook in settlement, before a shortfall is covered by selling investments: if
    /// covering it would take penalised money, refinance for cash first.
    pub(crate) fn hx_cash_out_before_shortfall(&mut self, plan: &Plan, t: usize, price: f64) {
        let Some(terms) = plan.hx.cash_out else {
            return;
        };
        if !self.hx_cash_out_possible(t) {
            return;
        }
        let sell_target = if plan.plain_mix {
            None
        } else {
            Some(&self.target)
        };
        let mut acc: Accounts = self.acc;
        let mut ty = self.ty;
        let probe = cover_shortfall(
            -self.acc.cash,
            plan.profile.retirement.early_access,
            &mut acc,
            &self.pinputs,
            &mut ty,
            sell_target,
        );
        if probe.penalty_base > PENALTY_DUST {
            let rate = self.hx.mortgage_rate;
            self.hx_cash_out(plan, &terms, t, price, rate, probe.penalty_base);
        }
    }

    /// Cheap test before any probe: a main home is owned and the last cash-out is
    /// long enough ago.
    fn hx_cash_out_possible(&self, t: usize) -> bool {
        t as i32 - self.hx.last_cash_out >= CASH_OUT_MIN_MONTHS
            && self
                .props
                .iter()
                .any(|p| p.owned && p.usage == PropertyUse::Primary)
    }

    /// Refinances the main home for about `wanted` dollars (at least a year of
    /// planned spending) if the loan-to-value limit leaves room. Returns true if it
    /// did.
    fn hx_cash_out(
        &mut self,
        plan: &Plan,
        terms: &CashOutPlan,
        t: usize,
        price: f64,
        mortgage_rate: f64,
        wanted: f64,
    ) -> bool {
        let Some(slot) = (0..self.props.len())
            .find(|&s| self.props[s].owned && self.props[s].usage == PropertyUse::Primary)
        else {
            return false;
        };
        let line = self.hx.lines[slot];
        if line.kind == LineKind::Hecm {
            return false;
        }
        let st = self.props[slot];
        let old = st.mortgage_balance();
        let heloc = if line.kind == LineKind::Heloc {
            line.balance
        } else {
            0.0
        };
        let payoff = old + heloc;
        let raise = wanted.max(CASH_OUT_RAISE_MONTHS * (self.ess_real + self.disc_real) * price);
        let fixed = REFINANCE_COST_FIXED * price;
        // L = payoff + raise + f L + fixed, capped by the lender's limit.
        let wanted_loan = (payoff + raise + fixed) / (1.0 - REFINANCE_COST_FRACTION);
        let loan = wanted_loan.min(terms.max_ltv * st.value);
        let costs = REFINANCE_COST_FRACTION * loan + fixed;
        let cash = loan - payoff - costs;
        if cash < CASH_OUT_MIN_CASH * price {
            return false;
        }
        let rate = (mortgage_rate + terms.premium).max(0.0);
        self.props[slot].mortgage = Some(MortgageState {
            balance: loan,
            rate,
            payment: amortized_payment(loan, rate, terms.months),
            original_value: st.value,
            // Within 80% loan-to-value (validation): no mortgage insurance.
            pmi_monthly: 0.0,
        });
        if self.check {
            self.chk_mort_flow += loan - old;
        }
        if heloc > 0.0 || line.kind == LineKind::Heloc {
            self.hx.lines[slot] = Default::default();
            self.hx_book(slot);
        }
        self.recv(YearSeries::PropertySale, loan - payoff);
        self.spend(YearSeries::SpendOneOff, costs);
        self.hx.last_cash_out = t as i32;
        self.req_dirty = true;
        let now = plan.start + t as i32;
        self.event(now, TraceEventKind::LifeEvent, Some(cash / price), || {
            format!(
                "Cash-out refinance of {} at {:.2}%",
                plan.props[slot].name,
                rate * 100.0
            )
        });
        true
    }

    /// Hook before surplus cash above `cap` is invested: the part the waterfall
    /// would put in the taxable account prepays the highest-rate mortgage while the
    /// rule says it beats investing.
    pub(crate) fn hx_prepay(&mut self, plan: &Plan, t: usize, cap: f64) {
        let rule = plan.hx.prepay;
        if matches!(rule, Prepay::Never) {
            return;
        }
        let surplus = self.acc.cash - cap;
        if surplus <= 0.0 {
            return;
        }
        let threshold_ok = |rate: f64, s: &State| match rule {
            Prepay::Never => false,
            Prepay::AfterTaxYield => {
                rate > s.hx.long_yield * (1.0 - s.sctx.marginal_rate.clamp(0.0, 1.0))
            }
            Prepay::RealRate(r) => rate - s.prior_inflation > r,
        };
        let mut best: Option<(usize, f64)> = None;
        for (i, p) in self.props.iter().enumerate() {
            if !p.owned || matches!(p.usage, PropertyUse::Rental(_)) {
                continue;
            }
            if let Some(m) = p.mortgage {
                if m.balance > 0.0
                    && threshold_ok(m.rate, self)
                    && best.map_or(true, |(_, r)| m.rate > r)
                {
                    best = Some((i, m.rate));
                }
            }
        }
        let Some((slot, _)) = best else {
            return;
        };
        let mut probe = self.acc;
        let flows = probe.save_surplus(surplus, &self.sctx, &self.target);
        if flows.taxable <= 0.0 {
            return;
        }
        let applied = self.props[slot].prepay(flows.taxable);
        if applied <= 0.0 {
            return;
        }
        if self.check {
            self.chk_mort_flow -= applied;
        }
        self.spend(YearSeries::SpendDebt, applied);
        self.req_dirty = true;
        if self.props[slot].mortgage.is_none() {
            let now = plan.start + t as i32;
            self.event(now, TraceEventKind::DebtPaidOff, None, || {
                format!("Paid off the mortgage on {}", plan.props[slot].name)
            });
        }
    }
}
