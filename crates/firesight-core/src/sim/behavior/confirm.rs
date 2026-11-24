//! Not retiring into a crash (`retirement.trigger.when_funded.confirmation` and
//! `one_more_year`).
//!
//! # What is modelled
//!
//! The plain `when_funded` rule retires the month after the funded test is first met
//! (once `min_age` is reached). Two opt-in refinements change that month:
//!
//! * **Confirmation.** The test (with `min_age`) must have held for `months`
//!   consecutive months, and US stocks must not be more than `max_equity_drawdown`
//!   below their peak. A household that becomes funded at the top of a bubble and
//!   loses it again in the crash keeps working.
//! * **One more year.** When the household would retire, with probability
//!   `probability` it decides to work one more year; after the year it tests again
//!   (and may decide the same again). This is a behavioural assumption, often called
//!   the "one more year syndrome"; the probability is the user's.
//!
//! `max_age` still forces retirement whatever either rule says, and the FI age (the
//! first month the test is met) is unchanged: only the retirement age moves.
//!
//! # Random numbers
//!
//! One uniform from `Stream::OneMoreYear` every month while `one_more_year` is set,
//! whatever the household does; a decision uses the draw of its month.
//!
//! # Not modelled
//!
//! The household does not weigh the size of its margin over the target, its age or
//! its job; a delay is always a whole year; once retired it does not reconsider
//! (see `retirement.return_to_work` for that).

use crate::model::{Profile, RetirementTrigger};
use crate::stats::TraceEventKind;

use super::super::plan::Plan;
use super::super::state::State;

/// Compiled confirmation and one-more-year settings of a `when_funded` trigger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ConfirmPlan {
    /// `(months, max_equity_drawdown)`.
    pub confirm: Option<(u32, f64)>,
    /// One-more-year probability.
    pub one_more_year: Option<f64>,
}

impl ConfirmPlan {
    pub(crate) fn compile(profile: &Profile) -> Option<ConfirmPlan> {
        let RetirementTrigger::WhenFunded(rule) = &profile.retirement.trigger else {
            return None;
        };
        if rule.confirmation.is_none() && rule.one_more_year.is_none() {
            return None;
        }
        Some(ConfirmPlan {
            confirm: rule
                .confirmation
                .map(|c| (c.months, c.max_equity_drawdown.clamp(0.0, 1.0))),
            one_more_year: rule.one_more_year.map(|o| o.probability.clamp(0.0, 1.0)),
        })
    }
}

/// Confirmation state of one path.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ConfirmState {
    /// Months in a row the test has held (with `min_age`).
    pub run: u32,
    /// Month index before which a "one more year" decision keeps the household at
    /// work.
    pub delay_until: i32,
    /// This month's uniform for the one-more-year decision.
    pub u: f64,
    /// Last month's decision was held back by the stock market (for the trace).
    pub held_by_market: bool,
}

/// What the rule says in a month in which the plain rule would retire (`go`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Retire,
    NotYet,
    HeldByMarket,
    OneMoreYear,
}

impl ConfirmState {
    /// Advances by one month: `go` is the plain rule's decision, `ti` the month
    /// index, `dd` the US stock drawdown from its peak.
    pub(crate) fn step(&mut self, p: &ConfirmPlan, go: bool, ti: i32, dd: f64) -> Verdict {
        if !go {
            self.run = 0;
            return Verdict::NotYet;
        }
        self.run = self.run.saturating_add(1);
        if ti < self.delay_until {
            return Verdict::NotYet;
        }
        if let Some((months, max_dd)) = p.confirm {
            if self.run < months {
                return Verdict::NotYet;
            }
            if dd > max_dd {
                return Verdict::HeldByMarket;
            }
        }
        if let Some(prob) = p.one_more_year {
            if self.u < prob {
                self.delay_until = ti + 12;
                return Verdict::OneMoreYear;
            }
        }
        Verdict::Retire
    }
}

impl State {
    /// Draws this month's one-more-year uniform (every month while the setting is
    /// on, whatever the household does).
    pub(crate) fn confirm_draw(&mut self, plan: &Plan) {
        if let Some(ConfirmPlan {
            one_more_year: Some(_),
            ..
        }) = plan.beh.confirm
        {
            self.beh.confirm.u = self.beh.confirm_rng.uniform();
        }
    }

    /// The retirement decision of a funded household in month `t`, given the plain
    /// rule's `go`.
    pub(crate) fn confirm_retirement(&mut self, plan: &Plan, t: usize, go: bool) -> bool {
        let Some(cp) = plan.beh.confirm else {
            return go;
        };
        let ti = t as i32;
        let dd = if self.eq_peak > 0.0 {
            (1.0 - self.eq_index / self.eq_peak).max(0.0)
        } else {
            0.0
        };
        let verdict = self.beh.confirm.step(&cp, go, ti, dd);
        let now = plan.start + ti + 1;
        let held = verdict == Verdict::HeldByMarket;
        if held && !self.beh.confirm.held_by_market {
            self.event(now, TraceEventKind::LifeEvent, Some(dd), || {
                format!(
                    "Retirement postponed: stocks are {:.0}% below their peak",
                    dd * 100.0
                )
            });
        }
        self.beh.confirm.held_by_market = held;
        if verdict == Verdict::OneMoreYear {
            self.event(now, TraceEventKind::LifeEvent, None, || {
                "Funded, but working one more year".to_string()
            });
        }
        verdict == Verdict::Retire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(confirm: Option<(u32, f64)>, omy: Option<f64>) -> ConfirmPlan {
        ConfirmPlan {
            confirm,
            one_more_year: omy,
        }
    }

    #[test]
    fn confirmation_needs_a_run_of_funded_months_and_a_calm_market() {
        let p = plan(Some((3, 0.2)), None);
        let mut s = ConfirmState::default();
        assert_eq!(s.step(&p, true, 0, 0.0), Verdict::NotYet);
        assert_eq!(s.step(&p, true, 1, 0.0), Verdict::NotYet);
        // A month unfunded starts the count again.
        assert_eq!(s.step(&p, false, 2, 0.0), Verdict::NotYet);
        assert_eq!(s.step(&p, true, 3, 0.0), Verdict::NotYet);
        assert_eq!(s.step(&p, true, 4, 0.0), Verdict::NotYet);
        // Third month in a row, but stocks are 25% down.
        assert_eq!(s.step(&p, true, 5, 0.25), Verdict::HeldByMarket);
        assert_eq!(s.step(&p, true, 6, 0.15), Verdict::Retire);
    }

    #[test]
    fn one_more_year_delays_by_twelve_months_then_decides_again() {
        let p = plan(None, Some(0.25));
        let mut s = ConfirmState {
            u: 0.1,
            ..ConfirmState::default()
        };
        assert_eq!(s.step(&p, true, 10, 0.0), Verdict::OneMoreYear);
        for ti in 11..22 {
            assert_eq!(s.step(&p, true, ti, 0.0), Verdict::NotYet);
        }
        s.u = 0.3;
        assert_eq!(s.step(&p, true, 22, 0.0), Verdict::Retire);
    }

    #[test]
    fn defaults_match_the_data_file() {
        use crate::model::{OneMoreYear, RetirementConfirmation};
        let d = &crate::data::Data::embedded().actuarial["household_behavior"]
            ["retirement_confirmation"];
        let c = RetirementConfirmation::default();
        assert_eq!(d["months"].as_f64(), Some(f64::from(c.months)));
        assert_eq!(
            d["max_equity_drawdown"].as_f64(),
            Some(c.max_equity_drawdown)
        );
        assert_eq!(
            d["one_more_year_probability"].as_f64(),
            Some(OneMoreYear::default().probability)
        );
    }
}
