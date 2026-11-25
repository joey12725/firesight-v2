//! Life insurance: premiums while a policy is in force and the death benefit.
//!
//! A policy is in force from the start until the first of: the end of its term,
//! the insured's death (the benefit is paid), the insured leaving the household
//! in a divorce (the policy goes with them), or the first month the household is
//! out of savings, when it cannot pay the premium and the policy lapses. Premiums
//! and benefits are level in nominal dollars. The benefit is not income (IRC
//! 101(a)): it goes to the person left in the household, or, when the insured was
//! the last, to the estate, where it counts toward the bequest target.

use crate::model::{PolicyKind, Profile, MAX_POLICIES};
use crate::stats::{TraceEventKind, YearSeries};

use super::super::plan::Plan;
use super::super::state::State;

/// One policy, compiled.
#[derive(Clone, Debug)]
pub(crate) struct PolicyPlan {
    pub name: String,
    pub insured: usize,
    /// First month the policy is no longer in force (`i32::MAX`: permanent).
    pub end_month: i32,
    /// Nominal death benefit.
    pub benefit: f64,
    /// Nominal monthly premium.
    pub premium: f64,
}

impl PolicyPlan {
    pub(crate) fn compile_all(profile: &Profile) -> Vec<PolicyPlan> {
        let mut out = Vec::new();
        for (i, person) in profile.household.people.iter().enumerate() {
            for p in &person.life_insurance {
                if out.len() == MAX_POLICIES {
                    return out;
                }
                out.push(PolicyPlan {
                    name: if p.name.is_empty() {
                        p.id.clone()
                    } else {
                        p.name.clone()
                    },
                    insured: i,
                    end_month: match p.kind {
                        PolicyKind::Term { years_remaining } => {
                            (years_remaining.max(0.0) * 12.0).round() as i32
                        }
                        PolicyKind::Permanent => i32::MAX,
                    },
                    benefit: p.death_benefit.max(0.0),
                    premium: p.annual_premium.max(0.0) / 12.0,
                });
            }
        }
        out
    }
}

impl State {
    /// The month's life insurance: benefits for deaths at the start of the month,
    /// lapses, then premiums. Returns true when the year must be re-planned.
    pub(super) fn insurance_month(&mut self, plan: &Plan, price: f64, t: usize) -> bool {
        let ti = t as i32;
        let now = plan.start + ti;
        let mut replan = false;
        for (k, pol) in plan.dynamics.policies.iter().enumerate() {
            if !self.dynamics.in_force[k] {
                continue;
            }
            let who = pol.insured;
            // Not yet in the household: the policy is not the household's yet (no
            // premiums from it) and a death before joining pays it nothing.
            let joined = ti >= plan.people[who].joins;
            if !self.alive[who] {
                self.dynamics.in_force[k] = false;
                if joined && ti < pol.end_month && pol.benefit > 0.0 {
                    // Into the household's cash: the survivor's, or the estate's
                    // when nobody is left at home.
                    self.recv(YearSeries::IncomeOther, pol.benefit);
                    replan = true;
                    self.event(
                        now,
                        TraceEventKind::Windfall,
                        Some(pol.benefit / price),
                        || format!("Life insurance pays out: {}", pol.name),
                    );
                }
                continue;
            }
            let why = if self.dynamics.left[who] {
                Some("goes with the partner who left")
            } else if ti >= pol.end_month {
                Some("term ends")
            } else if self.depleted_now {
                Some("lapses: the premium cannot be paid")
            } else {
                None
            };
            if why.is_none()
                && joined
                && t + 1 == self.months
                && self.death_month[who] as usize == self.months
                && (self.months as i32) < pol.end_month
            {
                // The insured is the last to die, at the end of this month, which
                // ends the path: the benefit goes to the estate now, so that it is
                // in the final balances and counts toward the bequest.
                self.dynamics.in_force[k] = false;
                if pol.benefit > 0.0 {
                    self.recv(YearSeries::IncomeOther, pol.benefit);
                    self.event(
                        now,
                        TraceEventKind::Windfall,
                        Some(pol.benefit / price),
                        || format!("Life insurance pays the estate: {}", pol.name),
                    );
                }
                continue;
            }
            if let Some(why) = why {
                self.dynamics.in_force[k] = false;
                self.req_dirty = true;
                self.event(now, TraceEventKind::LifeEvent, None, || {
                    format!("Life insurance {}: {why}", pol.name)
                });
                continue;
            }
            if joined && pol.premium > 0.0 {
                self.spend(YearSeries::SpendEssential, pol.premium);
            }
        }
        replan
    }

    /// Premiums due this month, nominal (for the year's plan).
    pub(super) fn premium_rate(&self, plan: &Plan, t: usize) -> f64 {
        let mut r = 0.0;
        for (k, pol) in plan.dynamics.policies.iter().enumerate() {
            if self.dynamics.in_force[k] && t as i32 >= plan.people[pol.insured].joins {
                r += pol.premium;
            }
        }
        r
    }
}
