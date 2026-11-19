//! Simulation-side hooks of the opt-in career-realism extensions
//! (`crate::career_realism`). Every hook is behind `plan.rz.any`; with the features
//! off none of this code runs.

use crate::career::{CareerClimate, Employment, JobMonth};
use crate::stats::TraceEventKind;

use super::plan::Plan;
use super::state::State;

impl State {
    /// Start of every month: draws the realism variates (a fixed number per job,
    /// whatever the household has decided).
    #[inline]
    pub(super) fn rz_month_start(&mut self, plan: &Plan) {
        self.rz.draw_month(&plan.rz);
    }

    /// After job `j` has been stepped in month `t`: applies the realism extensions to
    /// the job's state and to what it paid this month.
    pub(super) fn rz_after_job_step(
        &mut self,
        plan: &Plan,
        j: usize,
        t: usize,
        climate: &CareerClimate,
        jm: &mut JobMonth,
    ) {
        let jr = &plan.rz.jobs[j];
        if !jr.any() {
            return;
        }
        let jp = &plan.jobs[j];
        let draw = self.rz.job_draw[j];
        let now = plan.start + t as i32;

        // Voluntary job change (F16).
        if let Some(mob) = &jr.mobility {
            let st = &mut self.jobs[j].st;
            if st.employment == Employment::Employed && !jm.laid_off && !jm.rehired {
                let step = mob.step(
                    climate.age,
                    climate.recession != 0,
                    draw.u_move,
                    draw.r_jump,
                );
                jp.model.scale_real_salary(st, step.factor);
                if step.moved {
                    // The new employer starts its own vesting clock: what had not
                    // vested with the old one is lost.
                    jp.model.forfeit_unvested(st);
                    self.rz.moves[j] += 1;
                    let change = step.factor - 1.0;
                    let salary = st.real_salary;
                    self.req_dirty = true;
                    self.event(now, TraceEventKind::LifeEvent, Some(salary), || {
                        format!(
                            "Changed jobs from {} (pay {:+.0}%)",
                            jp.name,
                            change * 100.0
                        )
                    });
                }
            }
        }
    }
}
