//! Simulation-side hook of involuntary retirement (`crate::career_realism::late_career`).

use crate::stats::TraceEventKind;

use super::plan::Plan;
use super::state::State;

impl State {
    /// Job `j`'s owner was laid off in month `t` and will not work again: they retire
    /// now, and the household retires next month if nobody else is still working.
    pub(super) fn rz_never_works_again(&mut self, plan: &Plan, j: usize, t: usize, age: f64) {
        let jp = &plan.jobs[j];
        let p = jp.owner;
        let now = plan.start + t as i32;
        if self.rz.never[p] {
            return;
        }
        self.rz.never[p] = true;
        if !self.person_retired[p] {
            self.retire_person(plan, p, t);
        }
        if !self.retired {
            let others_working =
                (0..self.n).any(|o| o != p && self.present[o] && !self.person_retired[o]);
            if !others_working {
                self.retire_month = self.retire_month.min(t as i32 + 1);
            }
        }
        self.event(now, TraceEventKind::LifeEvent, None, || {
            format!(
                "Involuntary retirement at {:.0}: laid off from {} and does not find work again",
                age.floor(),
                jp.name
            )
        });
    }
}
