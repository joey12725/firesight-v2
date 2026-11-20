//! Simulation-side hooks of business cycles and failure
//! (`crate::career_realism::business`). Every hook is behind `plan.rz.any`.

use crate::stats::{TraceEventKind, YearSeries};

use super::plan::Plan;
use super::state::{State, TAX_PAYROLL};

impl State {
    /// Multiplier on self-employment income `k`'s profit this month: 0 while the owner
    /// is disabled or out of work for good, the business regime and recession effect
    /// when `business_risk` is on, and 0 from the month the business fails (which also
    /// ends the income, so plans stop counting on it).
    pub(super) fn rz_se_factor(
        &mut self,
        plan: &Plan,
        k: usize,
        p: usize,
        t: usize,
        recession: bool,
    ) -> f64 {
        if self.rz.dis.get(p).is_some_and(|d| d.disabled)
            || self.rz.never.get(p).copied().unwrap_or(false)
        {
            return 0.0;
        }
        let Some(model) = plan.rz.selfemp.get(k).copied().flatten() else {
            return 1.0;
        };
        let sp = &plan.selfemp[k];
        let now = plan.start + t as i32;
        let rm = self.retire_month;
        let ti = t as i32;
        let rt = &self.selfemp[k];
        let active = self.alive[p]
            && self.present[p]
            && !rt.ended
            && ti >= sp.start.month(rm)
            && ti < sp.end.month(rm)
            && !(sp.ends_at_retirement && self.person_retired[p]);
        // Business age: the years it had run at the start (only for work already
        // running then) plus the years since.
        let running_at_start = sp.start.month(rm) <= 0;
        let age = self.se_years(plan, k, t)
            + if running_at_start {
                model.years_operating
            } else {
                0.0
            };
        let draw = self.rz.se_draw[k];
        let m = model.step(
            &mut self.rz.biz[k],
            age,
            recession,
            now.month() == 1,
            active,
            draw.u_fail,
            draw.z,
        );
        if m.failed {
            self.selfemp[k].ended = true;
            self.req_dirty = true;
            self.event(now, TraceEventKind::LifeEvent, None, || {
                format!(
                    "Business failed: self-employment income of person {} ends",
                    p + 1
                )
            });
        }
        m.profit_factor
    }

    /// Pays the fallback wage of business `k` once its search spell is over.
    pub(super) fn rz_se_fallback(&mut self, plan: &Plan, k: usize, p: usize, price: f64) {
        let Some(model) = plan.rz.selfemp.get(k).copied().flatten() else {
            return;
        };
        let Some((annual, _)) = model.fallback else {
            return;
        };
        if !self.alive[p] || !self.present[p] || self.person_retired[p] {
            return;
        }
        if !model.fallback_pays(&mut self.rz.biz[k]) {
            return;
        }
        let gross = annual / 12.0 * price;
        if gross <= 0.0 {
            return;
        }
        self.recv(YearSeries::IncomeWages, gross);
        self.ty.wages[p] += gross;
        self.year_earned[p] += gross;
        self.wage_like += gross;
        if self.fica_on {
            let f = &self.params.federal;
            let before = self.ytd_fica_wages[p];
            let after = before + gross;
            let fica = f.oasdi_rate
                * (after.min(f.oasdi_wage_base) - before.min(f.oasdi_wage_base))
                + f.medicare_rate * gross;
            self.ytd_fica_wages[p] = after;
            self.pay(YearSeries::TaxPayroll, fica);
            self.paid[TAX_PAYROLL] += fica;
        }
    }
}
