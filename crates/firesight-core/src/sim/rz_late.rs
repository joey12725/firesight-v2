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
        // Every job of theirs ends, also one that would have outlasted the planned
        // retirement or was still to start.
        for (rt, other) in self.jobs.iter_mut().zip(&plan.jobs) {
            if other.owner == p && !rt.ended {
                other.model.end(&mut rt.st);
                rt.ended = true;
            }
        }
        if !self.retired {
            let ti = t as i32;
            let rm = self.retire_month;
            // Someone else still has work to do: a job that has not ended, or
            // self-employment that is running or still to come.
            let others_working =
                (0..self.n).any(|o| {
                    o != p
                        && self.present[o]
                        && !self.person_retired[o]
                        && !self.rz.never[o]
                        && !self.rz.dis.get(o).is_some_and(|d| d.disabled)
                        && (self
                            .jobs
                            .iter()
                            .zip(&plan.jobs)
                            .any(|(rt, jp)| jp.owner == o && !rt.ended)
                            || self.selfemp.iter().zip(&plan.selfemp).any(|(rt, sp)| {
                                sp.owner == o && !rt.ended && ti < sp.end.month(rm)
                            }))
                });
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
