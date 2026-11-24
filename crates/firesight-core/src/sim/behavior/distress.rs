//! Stopping saving in distress (`portfolio.pause_deferrals_in_distress`).
//!
//! # What is modelled
//!
//! At the end of each month's settlement the household is *in distress* when any of
//! these held that month:
//!
//! * it paid an early-withdrawal penalty (its penalised early distributions for the
//!   year grew) within the last [`PENALTY_MONTHS`] months;
//! * it cut essential spending (savings exhausted);
//! * it carries a debt (not a mortgage) at `distress.high_interest_rate` or more.
//!
//! Cash below the emergency-fund target is *not* distress. A household whose cash
//! sits a little under six months of spending (most working households in most
//! months, since the target grows with prices and spending) is not in trouble, and
//! the month loop already keeps its surplus in cash until the fund is full. Counting
//! it would hold the payroll deferral at the match for most of a career.
//!
//! In distress, first any cash above the month's cash target pays down the
//! high-interest debts, highest rate first. Then nothing more is invested that month
//! (no IRA, HSA, after-tax 401(k) or taxable contribution from the surplus): the
//! remaining surplus stays in cash. In the following
//! month's pay, the employee's payroll deferral is cut to what earns the full
//! employer match (`distress.keep_match`, the default) or to nothing, and the
//! after-tax "mega backdoor" route is closed. Saving resumes in the first month after
//! one without distress. Starting and ending a pause are trace events.
//!
//! No random numbers.
//!
//! # Not modelled
//!
//! Solo 401(k) deferrals from self-employment and HSA payroll contributions continue;
//! the employer's own contributions are unaffected; there is no hardship withdrawal
//! or 401(k) loan; the household does not cut spending to save.

use crate::model::{EmployerPlan, PortfolioPolicy};
use crate::stats::{TraceEventKind, YearSeries};

use super::super::plan::Plan;
use super::super::state::State;

/// Amounts below half a cent are rounding (as in the month loop).
const DUST: f64 = 0.005;

/// Months a penalty keeps the household in distress, the month it is paid
/// included: a retired household draws a quarter's money at a time, so a penalty
/// paid in one month covers the next two as well. A judgement.
pub const PENALTY_MONTHS: u8 = 3;

/// Compiled distress rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DistressPlan {
    pub keep_match: bool,
    pub high_interest_rate: f64,
}

impl DistressPlan {
    pub(crate) fn compile(p: &PortfolioPolicy) -> Option<DistressPlan> {
        p.pause_deferrals_in_distress.then_some(DistressPlan {
            keep_match: p.distress.keep_match,
            high_interest_rate: p.distress.high_interest_rate,
        })
    }

    /// The employer plan in a month of distress: the deferral cut to the share of pay
    /// that earns the whole match (or to nothing), no maximum, no after-tax route.
    pub(crate) fn paused(&self, ep: &EmployerPlan) -> EmployerPlan {
        let wanted = if ep.employee_max {
            f64::INFINITY
        } else {
            ep.employee_fraction
        };
        let keep = if self.keep_match && ep.match_rate > 0.0 {
            wanted.min(ep.match_up_to.max(0.0))
        } else {
            0.0
        };
        EmployerPlan {
            employee_fraction: keep,
            employee_max: false,
            after_tax_mega: false,
            ..*ep
        }
    }
}

/// Why a month was one of distress (for the trace).
pub(crate) fn reason(penalty: bool, cut: bool, debt: bool) -> &'static str {
    if penalty {
        "paying early-withdrawal penalties"
    } else if cut {
        "cutting essential spending"
    } else if debt {
        "carrying high-interest debt"
    } else {
        ""
    }
}

impl State {
    /// The employer plan to use for this month's payroll deferral.
    #[inline]
    pub(crate) fn deferral_plan(&self, plan: &Plan, ep: EmployerPlan) -> EmployerPlan {
        match plan.beh.distress {
            Some(d) if self.beh.distress => d.paused(&ep),
            _ => ep,
        }
    }

    /// Step 9, before the surplus is invested: updates the distress flag, pays down
    /// high-interest debt from cash above `cap`, and says whether the surplus is to
    /// stay in cash this month.
    pub(crate) fn distress_settle(&mut self, plan: &Plan, t: usize, cap: f64) -> bool {
        let Some(d) = plan.beh.distress else {
            return false;
        };
        let early = self.ty.early_distributions;
        let seen = if early < self.beh.penalty_seen {
            0.0
        } else {
            self.beh.penalty_seen
        };
        self.beh.penalty_seen = early;
        if early > seen + DUST {
            self.beh.penalty_months = PENALTY_MONTHS;
        }
        let penalty = self.beh.penalty_months > 0;
        self.beh.penalty_months = self.beh.penalty_months.saturating_sub(1);
        let cut = self.depleted_now;
        let now = plan.start + t as i32;

        // Cash above the target pays the dearest debt first.
        let mut surplus = self.acc.cash - cap;
        let mut prepaid = 0.0;
        while surplus > DUST {
            let mut pick: Option<usize> = None;
            for (i, debt) in self.debts.iter().enumerate() {
                if debt.balance > 0.0
                    && debt.rate >= d.high_interest_rate
                    && pick.map_or(true, |b| debt.rate > self.debts[b].rate)
                {
                    pick = Some(i);
                }
            }
            let Some(i) = pick else { break };
            let paid = surplus.min(self.debts[i].balance);
            self.debts[i].balance -= paid;
            surplus -= paid;
            prepaid += paid;
            if self.check {
                self.chk_debt_flow -= paid;
            }
            if self.debts[i].balance <= DUST {
                self.debts[i].balance = 0.0;
                self.freed_payments += self.debts[i].payment;
                self.req_dirty = true;
                self.event(now, TraceEventKind::DebtPaidOff, None, || {
                    format!("Paid off {}", plan.debts[i].name)
                });
            }
        }
        if prepaid > 0.0 {
            self.spend(YearSeries::SpendDebt, prepaid);
        }
        let debt = self
            .debts
            .iter()
            .any(|x| x.balance > 0.0 && x.rate >= d.high_interest_rate);
        let distress = penalty || cut || debt;
        if distress != self.beh.distress {
            let why = reason(penalty, cut, debt);
            self.event(now, TraceEventKind::Other, None, || {
                if distress {
                    format!("Saving paused: {why}")
                } else {
                    "Saving resumed".to_string()
                }
            });
        }
        self.beh.distress = distress;
        distress
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(keep_match: bool) -> DistressPlan {
        DistressPlan {
            keep_match,
            high_interest_rate: 0.08,
        }
    }

    #[test]
    fn a_pause_keeps_the_deferral_that_earns_the_match() {
        let ep = EmployerPlan {
            employee_fraction: 0.15,
            match_rate: 0.5,
            match_up_to: 0.06,
            after_tax_mega: true,
            ..EmployerPlan::default()
        };
        let p = plan(true).paused(&ep);
        assert_eq!(p.employee_fraction, 0.06);
        assert!(!p.after_tax_mega && !p.employee_max);
        assert_eq!(p.match_rate, 0.5);
        // A deferral already below the match limit is kept as it is.
        let small = EmployerPlan {
            employee_fraction: 0.04,
            ..ep
        };
        assert_eq!(plan(true).paused(&small).employee_fraction, 0.04);
        // "Max out" becomes the match limit.
        let max = EmployerPlan {
            employee_max: true,
            ..ep
        };
        assert_eq!(plan(true).paused(&max).employee_fraction, 0.06);
        // Without keep_match, or without a match, nothing is deferred.
        assert_eq!(plan(false).paused(&ep).employee_fraction, 0.0);
        let unmatched = EmployerPlan {
            match_rate: 0.0,
            ..ep
        };
        assert_eq!(plan(true).paused(&unmatched).employee_fraction, 0.0);
    }

    #[test]
    fn the_reason_names_the_most_serious_sign() {
        assert_eq!(
            reason(true, true, true),
            "paying early-withdrawal penalties"
        );
        assert_eq!(reason(false, true, true), "cutting essential spending");
        assert_eq!(reason(false, false, true), "carrying high-interest debt");
        assert_eq!(reason(false, false, false), "");
    }

    #[test]
    fn constants_match_the_data_file() {
        let d = &crate::data::Data::embedded().actuarial["household_behavior"]["distress_pause"];
        assert_eq!(
            d["high_interest_rate"]["value"].as_f64(),
            Some(crate::model::DistressRules::default().high_interest_rate)
        );
        assert_eq!(
            d["penalty_months"]["value"].as_f64(),
            Some(f64::from(PENALTY_MONTHS))
        );
    }
}
