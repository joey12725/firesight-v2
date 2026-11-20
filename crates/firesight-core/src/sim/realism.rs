//! Simulation-side hooks of the opt-in career-realism extensions
//! (`crate::career_realism`). Every hook is behind `plan.rz.any`; with the features
//! off none of this code runs.

use crate::career::{CareerClimate, Employment, JobMonth};
use crate::stats::TraceEventKind;

use crate::economy::EconomyPath;

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
        econ: &EconomyPath,
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

        // Recession pay freezes, cuts and bonuses (F17).
        if let Some(rp) = &jr.recession_pay {
            let recession = climate.recession != 0;
            let employed = self.jobs[j].st.employment == Employment::Employed;
            // The raise a freeze withholds: the curve's real growth over the next
            // year plus the path's inflation over the last twelve months. Worked out
            // only in the month a freeze can start.
            let mut forgone = 0.0;
            if recession && employed && draw.u_freeze < rp.freeze_monthly {
                let growth = jp.model.curve_factor(climate.age + 1.0)
                    / jp.model.curve_factor(climate.age).max(f64::MIN_POSITIVE)
                    - 1.0;
                let inflation = econ.price_level[t] / econ.price_level[t.saturating_sub(12)] - 1.0;
                forgone = growth.max(0.0) + inflation.max(0.0);
            }
            let f = rp.step(
                &mut self.rz.pay[j],
                employed,
                recession,
                forgone,
                (draw.u_freeze, draw.u_cut, draw.u_size),
            );
            jm.wages *= f.wage;
            jm.commission *= f.wage;
            jm.bonus *= f.bonus;
            if f.froze {
                self.event(now, TraceEventKind::LifeEvent, None, || {
                    format!("Pay freeze at {}: no raise this year", jp.name)
                });
            }
            if f.cut {
                let cut = self.rz.pay[j].cut_fraction;
                self.event(now, TraceEventKind::LifeEvent, None, || {
                    format!(
                        "Pay cut of {:.0}% at {} for nine months",
                        cut * 100.0,
                        jp.name
                    )
                });
            }
        }

        // Older workers laid off this month (F19).
        if let (Some(late), true) = (&jr.late_career, jm.laid_off) {
            let st = &mut self.jobs[j].st;
            if let Some(o) = late.adjust(
                climate.age,
                st.spell_months_left,
                st.pending_scar,
                draw.u_never,
            ) {
                st.spell_months_left = o.spell_months;
                st.pending_scar = o.scar;
                if o.never {
                    self.rz_never_works_again(plan, j, t, climate.age);
                }
            }
        }

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
