//! Spousal caregiving: when one partner needs paid care at home, the other
//! provides part of it.
//!
//! With `spending.healthcare.spousal_care` on, in every month of a home-care
//! episode (the household's own long-term care model, `health.rs`) in which the
//! other partner is in the household and not in care themselves, the bill is
//! multiplied by `1 - SPOUSAL_CARE_SHARE`. When the episode begins, a partner who
//! still has a job cuts their hours with probability
//! [`SPOUSAL_CARE_REDUCE_PROBABILITY`] (to half time, for the length of the
//! episode, through the career-break mechanism) or stops working with
//! probability [`SPOUSAL_CARE_STOP_PROBABILITY`] (their jobs end). The choice is
//! one uniform per person drawn on `Stream::SpousalCare` at the start of every
//! path, whether or not it is used.
//!
//! Assisted living and nursing homes are not affected: the partner cannot stand in
//! for them. With `health_risk` off the episode is an expected cost, not home
//! care, and nothing changes.

use crate::career::Employment;
use crate::health::CareType;
use crate::model::{
    SPOUSAL_CARE_HOURS_CUT, SPOUSAL_CARE_REDUCE_PROBABILITY, SPOUSAL_CARE_SHARE,
    SPOUSAL_CARE_STOP_PROBABILITY,
};
use crate::rng::{Rng, Stream};
use crate::stats::TraceEventKind;
use crate::MAX_PEOPLE;

use super::super::plan::Plan;
use super::super::state::State;

impl State {
    /// Draws who would cut back to care for the other partner on this path.
    pub(super) fn begin_spousal_care(&mut self, seed: u64, path: u64) {
        let mut rng = Rng::stream(seed, path, Stream::SpousalCare);
        for p in 0..MAX_PEOPLE {
            self.dynamics.care_u[p] = rng.uniform();
        }
    }

    /// The partner who can care for person `i` at home in month `t`, if any.
    fn home_carer(&self, i: usize, t: usize) -> Option<usize> {
        let h = &self.health[i];
        let tu = t as u32;
        if h.ltc_care != CareType::Home || tu < h.ltc_start_month || tu >= h.ltc_end_month {
            return None;
        }
        (0..self.n).find(|&o| {
            let ho = &self.health[o];
            o != i
                && self.present[o]
                && !self.in_facility[o]
                && !(tu >= ho.ltc_start_month && tu < ho.ltc_end_month)
        })
    }

    /// Multiplier on person `i`'s care bill in month `t`: `1 - SPOUSAL_CARE_SHARE`
    /// while a partner cares for them at home, 1 otherwise.
    pub(crate) fn spousal_care_factor(&self, i: usize, t: usize) -> f64 {
        if self.home_carer(i, t).is_some() {
            1.0 - SPOUSAL_CARE_SHARE
        } else {
            1.0
        }
    }

    /// At the start of a home-care episode, the partner may cut back at work.
    pub(super) fn spousal_care_month(&mut self, plan: &Plan, t: usize) -> bool {
        let mut replan = false;
        for i in 0..self.n {
            if self.health[i].ltc_start_month != t as u32 {
                continue;
            }
            let Some(o) = self.home_carer(i, t) else {
                continue;
            };
            let u = self.dynamics.care_u[o];
            let months = self.health[i].ltc_end_month - self.health[i].ltc_start_month;
            let reduce = u < SPOUSAL_CARE_REDUCE_PROBABILITY;
            let stop =
                !reduce && u < SPOUSAL_CARE_REDUCE_PROBABILITY + SPOUSAL_CARE_STOP_PROBABILITY;
            let mut changed = false;
            for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
                if jp.owner != o || rt.ended || rt.st.employment != Employment::Employed {
                    continue;
                }
                if reduce {
                    jp.model
                        .start_break(&mut rt.st, months, 1.0 - SPOUSAL_CARE_HOURS_CUT);
                    changed = true;
                } else if stop {
                    jp.model.end(&mut rt.st);
                    rt.ended = true;
                    changed = true;
                }
            }
            let now = plan.start + t as i32;
            let what = if !changed {
                "and keeps working as before"
            } else if reduce {
                "and works half time"
            } else {
                "and stops working"
            };
            self.event(now, TraceEventKind::LongTermCare, None, || {
                format!(
                    "Person {} cares for person {} at home, half the hours, {what}",
                    o + 1,
                    i + 1
                )
            });
            if changed {
                self.req_dirty = true;
                replan = true;
            }
        }
        replan
    }
}
