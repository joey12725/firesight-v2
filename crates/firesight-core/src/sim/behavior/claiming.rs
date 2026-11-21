//! Adaptive Social Security claiming (`social_security.claiming: adaptive`).
//!
//! # What is modelled
//!
//! A person with adaptive claiming plans to claim at `latest` (70 by default), and
//! that is the claim age the funded test and the benefit model start from. Each month
//! from `earliest` (62) to `latest`, once the person has stopped working, the
//! household asks whether its savings can still *bridge* the delay:
//!
//! ```text
//! bridge  = months to latest x the benefit the person would get by claiming now
//!           (what the portfolio has to pay instead)
//! reserve = RESERVE_MONTHS x the household's monthly budget
//! claim now if  investable < reserve + bridge (+ the higher earner's bridge, for
//!                                               the lower earner of a couple)
//! ```
//!
//! A household that is cutting essentials this month (savings exhausted) claims at
//! once, working or not, rather than sell its home or go without while a benefit is
//! unclaimed.
//!
//! **Survivor benefit.** The higher earner's benefit becomes the survivor's benefit,
//! so delaying it is worth more to a couple than delaying the lower earner's. While
//! both are alive, the lower earner's test counts the higher earner's remaining bridge
//! too: when money is short the lower earner claims first and the higher earner keeps
//! delaying.
//!
//! No random numbers.
//!
//! # Not modelled
//!
//! The decision does not weigh life expectancy, tax or the marketplace premium credit;
//! benefits are not suspended once claimed; a person who is still working claims only
//! at `latest` or when the household is cutting essentials.

use crate::model::{Claiming, Profile};
use crate::stats::TraceEventKind;
use crate::MAX_PEOPLE;

use super::super::plan::Plan;
use super::super::state::State;

/// Months of the household's budget kept in reserve on top of the bridge before a
/// delay is judged affordable. A judgement (a year of spending is a common rule of
/// thumb for a cash reserve in retirement, not a measured quantity).
pub const RESERVE_MONTHS: f64 = 12.0;

/// Compiled adaptive claiming: per person, `(earliest, latest)` claim ages in
/// months, or `None` for a fixed claim age.
pub(crate) type ClaimPlan = [Option<(i32, i32)>; MAX_PEOPLE];

pub(crate) fn compile(profile: &Profile) -> Option<ClaimPlan> {
    let mut out: ClaimPlan = [None; MAX_PEOPLE];
    let mut any = false;
    for (i, p) in profile.household.people.iter().enumerate().take(MAX_PEOPLE) {
        if let Claiming::Adaptive { earliest, latest } = p.social_security.claiming {
            let latest = latest.clamp(62.0, 70.0);
            let earliest = earliest.clamp(62.0, latest);
            out[i] = Some((
                (earliest * 12.0).round() as i32,
                (latest * 12.0).round() as i32,
            ));
            any = true;
        }
    }
    any.then_some(out)
}

/// What a delay still costs: the benefit given up each month until `latest`
/// (today's dollars, summed, not discounted), for a person aged `age` months who
/// could claim `benefit_now` a month.
pub(crate) fn bridge(age: i32, earliest: i32, latest: i32, benefit_now: f64) -> f64 {
    f64::from((latest - age.max(earliest)).max(0)) * benefit_now.max(0.0)
}

/// The decision: claim now when savings no longer cover the reserve and the bridges
/// that have to be paid from them.
pub(crate) fn should_claim(
    wealth: f64,
    reserve: f64,
    own_bridge: f64,
    higher_earner_bridge: f64,
) -> bool {
    wealth < reserve + own_bridge + higher_earner_bridge
}

impl State {
    /// Adaptive claiming for month `t`, before Social Security is paid.
    pub(crate) fn adaptive_claiming_month(&mut self, plan: &Plan, t: usize, price: f64) {
        let Some(cp) = plan.beh.claim else { return };
        let mut bridges = [0.0; MAX_PEOPLE];
        let mut pias = [f64::NEG_INFINITY; MAX_PEOPLE];
        let mut open = [false; MAX_PEOPLE];
        for i in 0..self.n {
            let Some((earliest, latest)) = cp[i] else {
                continue;
            };
            if !self.present[i] || !self.alive[i] || self.ss_claimed[i] {
                continue;
            }
            let Some(c) = &self.claimants[i] else {
                continue;
            };
            let Some(pia) = c.pia_real else { continue };
            let age = self.age_months(plan, i, t);
            if age >= latest {
                continue;
            }
            let fra = plan.ss.full_retirement_age_months(c.birth.year());
            let now = plan.ss.claim_factor(age.max(earliest) as u32, fra) * pia.max(0.0);
            bridges[i] = bridge(age, earliest, latest, now);
            pias[i] = pia;
            open[i] = age >= earliest;
        }
        if !open.iter().any(|o| *o) {
            return;
        }
        let wealth = self.acc.investable() / price;
        let reserve = RESERVE_MONTHS * (self.ess_real + self.disc_real);
        for i in 0..self.n {
            if !open[i] {
                continue;
            }
            let j = MAX_PEOPLE - 1 - i;
            let higher = if j < self.n && pias[j] > pias[i] {
                bridges[j]
            } else {
                0.0
            };
            let stopped = self.person_retired[i] || self.retired;
            let short = stopped && should_claim(wealth, reserve, bridges[i], higher);
            if !(short || self.depleted_now) {
                continue;
            }
            let age = self.age_months(plan, i, t);
            if let Some(c) = &mut self.claimants[i] {
                c.claim_age = f64::from(age) / 12.0;
            }
            self.ss_dirty = true;
            self.req_dirty = true;
            let now = plan.start + t as i32;
            let why = if self.depleted_now {
                "savings are exhausted"
            } else {
                "savings can no longer bridge the delay"
            };
            self.event(now, TraceEventKind::LifeEvent, Some(wealth), || {
                format!(
                    "Person {} claims Social Security at {:.1}: {why}",
                    i + 1,
                    f64::from(age) / 12.0
                )
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_is_the_benefit_given_up_until_the_latest_age() {
        // At 64 with $1,800 a month available now, 72 months to 70: $129,600.
        assert_eq!(bridge(64 * 12, 62 * 12, 70 * 12, 1_800.0), 129_600.0);
        // Before the earliest age the bridge runs from the earliest age.
        assert_eq!(bridge(60 * 12, 62 * 12, 70 * 12, 1_000.0), 96_000.0);
        assert_eq!(bridge(70 * 12, 62 * 12, 70 * 12, 1_000.0), 0.0);
    }

    #[test]
    fn claim_when_savings_fall_below_reserve_plus_bridges() {
        // A $5,000-a-month budget: a $60,000 reserve. Own bridge $129,600.
        let reserve = RESERVE_MONTHS * 5_000.0;
        assert!(!should_claim(200_000.0, reserve, 129_600.0, 0.0));
        assert!(should_claim(189_000.0, reserve, 129_600.0, 0.0));
        // The lower earner also leaves room for the higher earner's delay.
        assert!(should_claim(200_000.0, reserve, 50_000.0, 150_000.0));
        assert!(!should_claim(300_000.0, reserve, 50_000.0, 150_000.0));
    }

    #[test]
    fn only_adaptive_people_are_compiled() {
        let mut p = Profile::default();
        p.household.people = vec![Default::default(), Default::default()];
        assert!(compile(&p).is_none());
        p.household.people[1].social_security.claiming = Claiming::Adaptive {
            earliest: 63.0,
            latest: 75.0,
        };
        let c = compile(&p).unwrap();
        assert_eq!(c[0], None);
        assert_eq!(c[1], Some((63 * 12, 70 * 12)));
    }

    #[test]
    fn the_reserve_matches_the_data_file() {
        let d = &crate::data::Data::embedded().actuarial["household_behavior"]["adaptive_claiming"];
        assert_eq!(d["reserve_months"]["value"].as_f64(), Some(RESERVE_MONTHS));
    }
}
