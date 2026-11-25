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
//!            raise  = the penalised amount; in a retired household's annual plan,
//!                     the year's penalised withdrawals times the years until the
//!                     youngest adult is 59 and a half (one refinance, not one a
//!                     year); once retired at least 12 months of planned spending
//! terms      market 30-year rate + rate_premium, amortised over term_years
//! cash       L - payoff - costs, into cash (booked as cash from property)
//! only if    10% x used > costs
//!            + max(0, new rate - old rate) x old balance / 2 x years the old
//!            loan had left
//!            + max(0, new rate - 10-year yield) x used x years it is outstanding
//!            until 59 and a half
//!            (used = min(penalised money it replaces, cash less the rise in loan
//!            payments while withdrawals would be penalised), spent at the planned
//!            pace; the savings it spares are valued at the safe yield, as in the
//!            prepayment rule)
//!            (the penalised money it replaces is the shortfall at hand mid-year,
//!            and the penalised withdrawals until 59 and a half in the annual plan)
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
use crate::accounts::EARLY_WITHDRAWAL_AGE;

/// Months between two cash-out refinances. A judgement: closing costs make a
/// refinance more often than yearly uneconomic, and lenders commonly require some
/// months of seasoning.
pub const CASH_OUT_MIN_MONTHS: i32 = 12;
/// Once retired, a cash-out refinance raises at least this many months of planned
/// spending, so that one refinance carries the household for a while. A judgement.
pub const CASH_OUT_RAISE_MONTHS: f64 = 12.0;
/// Smallest cash-out worth its closing costs, today's dollars. A judgement.
pub const CASH_OUT_MIN_CASH: f64 = 10_000.0;
/// Penalised dollars below this are ignored (rounding of the withdrawal plan).
const PENALTY_DUST: f64 = 1.0;
/// The additional tax on early distributions from retirement plans and IRAs that a
/// cash-out avoids (IRC section 72(t)(1)): 10%.
pub const EARLY_WITHDRAWAL_PENALTY: f64 = 0.10;

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

/// Months a mortgage has left at its payment (at most 40 years).
fn remaining_months(m: &MortgageState) -> f64 {
    const MAX: f64 = 480.0;
    let r = m.rate / 12.0;
    if m.balance <= 0.0 || m.payment <= 0.0 {
        return 0.0;
    }
    if r <= 0.0 {
        return (m.balance / m.payment).min(MAX);
    }
    let x = 1.0 - r * m.balance / m.payment;
    if x <= 0.0 {
        return MAX;
    }
    (-crate::math::ln(x) / crate::math::ln_1p(r)).min(MAX)
}

impl State {
    /// Hook in the annual plan, before the year's withdrawals are planned: if the
    /// plan as it stands would withdraw money with the penalty, refinance for cash
    /// first (the plan then spends that cash before selling anything).
    pub(crate) fn hd_cash_out_before_plan(
        &mut self,
        plan: &Plan,
        t: usize,
        price: f64,
        mortgage_rate: f64,
        base_result: &TaxResult,
    ) {
        let Some(terms) = plan.hd.cash_out else {
            return;
        };
        if !self.hd_cash_out_possible(t) {
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
            let per_year = Some(probe.penalized);
            self.hd_cash_out(
                plan,
                &terms,
                t,
                price,
                mortgage_rate,
                probe.penalized,
                per_year,
            );
        }
    }

    /// Hook in settlement, before a shortfall is covered by selling investments: if
    /// covering it would take penalised money, refinance for cash first.
    pub(crate) fn hd_cash_out_before_shortfall(&mut self, plan: &Plan, t: usize, price: f64) {
        let Some(terms) = plan.hd.cash_out else {
            return;
        };
        if !self.hd_cash_out_possible(t) {
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
            let rate = self.hd.mortgage_rate;
            self.hd_cash_out(plan, &terms, t, price, rate, probe.penalty_base, None);
        }
    }

    /// Cheap test before any probe: a main home is owned and the last cash-out is
    /// long enough ago.
    fn hd_cash_out_possible(&self, t: usize) -> bool {
        t as i32 - self.hd.last_cash_out >= CASH_OUT_MIN_MONTHS
            && self
                .props
                .iter()
                .any(|p| p.owned && p.usage == PropertyUse::Primary)
    }

    /// Refinances the main home for about `wanted` dollars if the loan-to-value limit
    /// leaves room and the penalty avoided pays for it. `per_year` is the year's
    /// penalised withdrawals when the annual plan asks; a retired household then
    /// raises enough for every year until the youngest adult is 59 and a half.
    /// Returns true if it did.
    #[allow(clippy::too_many_arguments)]
    fn hd_cash_out(
        &mut self,
        plan: &Plan,
        terms: &CashOutPlan,
        t: usize,
        price: f64,
        mortgage_rate: f64,
        wanted: f64,
        per_year: Option<f64>,
    ) -> bool {
        let Some(slot) = (0..self.props.len())
            .find(|&s| self.props[s].owned && self.props[s].usage == PropertyUse::Primary)
        else {
            return false;
        };
        let line = self.hd.lines[slot];
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
        let monthly = (self.ess_real + self.disc_real) * price;
        // What the refinance is for: the penalised money it replaces. A shortfall in
        // the middle of a year (a spell without income, an expense shock) is only the
        // shortfall at hand; the year's plan of a retired household is that year's
        // penalised withdrawals for every year until the youngest adult is 59 and a
        // half, so that one refinance does the job.
        let mut penalised_years = 0.0;
        let replaces = match per_year {
            Some(year) if self.retired => {
                let mut youngest = f64::INFINITY;
                for i in 0..self.n {
                    if self.present[i] && self.alive[i] {
                        youngest = youngest.min(self.age(plan, i, t));
                    }
                }
                let years = (EARLY_WITHDRAWAL_AGE - youngest).clamp(1.0, 100.0);
                penalised_years = years;
                wanted.max(year * years)
            }
            _ => wanted,
        };
        let raise = if self.retired {
            replaces.max(CASH_OUT_RAISE_MONTHS * monthly)
        } else {
            replaces
        };
        let horizon = if self.retired {
            (raise / monthly.max(1.0)).min(1200.0)
        } else {
            0.0
        };
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
        // Worth it only if the penalty avoided beats the closing costs and the dearer
        // rate on what was already owed, over the old loan's remaining life (on its
        // average balance, half the balance today).
        let dearer = st.mortgage.map_or(0.0, |m| {
            (rate - m.rate).max(0.0) * old * 0.5 * remaining_months(&m) / 12.0
        });
        // The penalised withdrawals it spares: the cash, less the larger loan
        // payments it must also meet while withdrawals would be penalised (the
        // shortfall's own time, or the years to 59 and a half).
        let payment = amortized_payment(loan, rate, terms.months);
        let extra = (payment - st.mortgage.map_or(0.0, |m| m.payment)).max(0.0) * 12.0;
        let years = if penalised_years > 0.0 {
            penalised_years
        } else {
            cash / (12.0 * monthly.max(1.0))
        };
        let used = (cash - extra * years).max(0.0).min(replaces);
        // And the carry: the cash is borrowed at the loan's rate while the savings it
        // spares would earn about the safe 10-year yield, from when it is spent (at
        // the planned pace) until withdrawals are free of the penalty and could repay
        // it.
        let years_used = used / (12.0 * monthly.max(1.0));
        let outstanding = (years - 0.5 * years_used).max(0.5 * years_used);
        let carry = (rate - self.hd.long_yield).max(0.0) * used * outstanding;
        if EARLY_WITHDRAWAL_PENALTY * used <= costs + dearer + carry {
            return false;
        }
        self.props[slot].mortgage = Some(MortgageState {
            balance: loan,
            rate,
            payment,
            original_value: st.value,
            // Within 80% loan-to-value (validation): no mortgage insurance.
            pmi_monthly: 0.0,
            // The new loan is fixed-rate, whatever the old one was.
            arm: None,
        });
        if self.check {
            self.chk_mort_flow += loan - old;
        }
        if heloc > 0.0 || line.kind == LineKind::Heloc {
            self.hd.lines[slot] = Default::default();
            self.hd_book(slot);
        }
        self.recv(YearSeries::PropertySale, loan - payoff);
        self.spend(YearSeries::SpendOneOff, costs);
        self.hd.last_cash_out = t as i32;
        // The cash is there to be spent: it does not go back into the loan.
        self.hd.no_prepay_until = t as i32 + horizon as i32;
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
    pub(crate) fn hd_prepay(&mut self, plan: &Plan, t: usize, cap: f64) {
        let rule = plan.hd.prepay;
        if matches!(rule, Prepay::Never) {
            return;
        }
        let surplus = self.acc.cash - cap;
        if surplus <= 0.0 || (t as i32) < self.hd.no_prepay_until {
            return;
        }
        let threshold_ok = |rate: f64, s: &State| match rule {
            Prepay::Never => false,
            Prepay::AfterTaxYield => {
                rate > s.hd.long_yield * (1.0 - s.sctx.marginal_rate.clamp(0.0, 1.0))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_months_inverts_the_payment() {
        let payment = amortized_payment(100_000.0, 0.03, 240);
        let m = MortgageState {
            balance: 100_000.0,
            rate: 0.03,
            payment,
            original_value: 0.0,
            pmi_monthly: 0.0,
            arm: None,
        };
        assert!((remaining_months(&m) - 240.0).abs() < 1e-6);
        let zero = MortgageState {
            rate: 0.0,
            payment: 500.0,
            ..m
        };
        assert_eq!(remaining_months(&zero), 200.0);
        let interest_only = MortgageState {
            payment: 250.0,
            ..m
        };
        assert_eq!(remaining_months(&interest_only), 480.0);
    }
}
