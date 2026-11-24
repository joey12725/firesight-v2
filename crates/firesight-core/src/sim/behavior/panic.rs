//! Panic selling (`portfolio.behavior: panic_prone`).
//!
//! # What is modelled
//!
//! The engine already follows the US stock index and its running peak (the drawdown
//! flag of the cash bucket). A fall from the peak of at least `drawdown_trigger` is
//! one *fall*; the first month of each fall, the household decides once, with
//! probability `probability`, whether it panics. A fall ends when the index sets a
//! new high, and only then can the next one trigger another decision.
//!
//! A household that panics moves `share` of its stock weight (US and international
//! stocks) to cash in its target mix and trades to that target at once through the
//! engine's own rebalance: sales in a taxable account realise their gain or loss at
//! average cost, exactly as an ordinary rebalance does, and new savings buy the
//! panic mix. It buys back, again through a rebalance to the plan's own target, at
//! the first month that the index is within `recovered_within` of its old peak or
//! after `max_months` out of the market. A glide path keeps moving underneath: the
//! panic mix is always the current plan mix with stocks cut.
//!
//! # Calibration (a behavioural assumption)
//!
//! Vanguard (How America Saves 2009): in 2008 16% of its defined-contribution
//! participants traded, 2% abandoned equities entirely, and participants' shifts to
//! fixed income cut the equity share of plan assets by about 4 percentage points
//! (of 73%, so about 5.5% of equities held). The defaults (8% chance of selling half
//! the stocks in a fall of 25% or more) make the expected share of stock sold in such
//! a fall 4%, at the low end of that figure. Morningstar (Mind the Gap 2024) measures the cost
//! of investors' timing at 1.1 points a year over the ten years to 2023 across all
//! funds; that gap includes much more than panic selling and is a check on the order
//! of magnitude, not a calibration target. The re-entry rule is a judgement.
//!
//! # Random numbers
//!
//! One uniform from `Stream::PanicSelling` every month while the feature is on,
//! whatever the household does; a decision uses the draw of its month.
//!
//! # Not modelled
//!
//! Only US and international stocks are sold (not REITs, crypto, single stock or the
//! custom asset); a household protecting its marketplace premium credit trades only
//! inside its tax-sheltered accounts (as its rebalances do); a household never panics
//! twice in one fall, and the probability does not depend on age, wealth or past
//! panics.

use crate::accounts::Weights;
use crate::model::{AssetClass, PanicSelling};
use crate::stats::TraceEventKind;

use super::super::plan::Plan;
use super::super::state::State;

/// Compiled `portfolio.behavior: panic_prone`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanicPlan {
    pub trigger: f64,
    pub probability: f64,
    pub share: f64,
    pub recovered_within: f64,
    pub max_months: u32,
}

impl PanicPlan {
    pub(crate) fn compile(p: &PanicSelling) -> PanicPlan {
        PanicPlan {
            trigger: p.drawdown_trigger.clamp(0.0, 1.0),
            probability: p.probability.clamp(0.0, 1.0),
            share: p.share.clamp(0.0, 1.0),
            recovered_within: p.reentry.recovered_within.clamp(0.0, 1.0),
            max_months: p.reentry.max_months,
        }
    }

    /// The panic mix: `target` with `share` of its stock weight moved to cash.
    pub(crate) fn panic_mix(&self, target: &Weights) -> Weights {
        let mut w = *target;
        for a in [AssetClass::UsStocks, AssetClass::IntlStocks] {
            let sold = w[a.index()] * self.share;
            w[a.index()] -= sold;
            w[AssetClass::Cash.index()] += sold;
        }
        w
    }
}

/// Panic-selling state of one path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanicState {
    /// Out of the market (holding the panic mix).
    pub out: bool,
    /// The current fall has not been decided yet (false from a decision to the next
    /// new high).
    pub armed: bool,
    /// Months since the panic sale.
    pub months_out: u32,
    /// The plan's own target while out of the market.
    pub base: Weights,
}

impl Default for PanicState {
    fn default() -> Self {
        Self {
            out: false,
            armed: true,
            months_out: 0,
            base: [0.0; crate::model::N_ASSETS],
        }
    }
}

/// What the month's decision is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanicStep {
    Nothing,
    Sell,
    BuyBack,
}

impl PanicState {
    /// Advances the state by one month at drawdown `dd` (0 at a new high) with the
    /// month's uniform `u`; says what to trade.
    pub(crate) fn step(&mut self, p: &PanicPlan, dd: f64, u: f64) -> PanicStep {
        if dd <= 0.0 {
            self.armed = true;
        }
        if self.out {
            self.months_out += 1;
            if dd <= p.recovered_within || self.months_out >= p.max_months {
                self.out = false;
                return PanicStep::BuyBack;
            }
            return PanicStep::Nothing;
        }
        if self.armed && dd >= p.trigger && p.trigger > 0.0 {
            self.armed = false;
            if u < p.probability {
                self.out = true;
                self.months_out = 0;
                return PanicStep::Sell;
            }
        }
        PanicStep::Nothing
    }
}

impl State {
    /// Panic selling for month `t`, after the month's returns.
    pub(crate) fn panic_month(&mut self, plan: &Plan, t: usize, price: f64) {
        let Some(pp) = plan.beh.panic else { return };
        let u = self.beh.panic_rng.uniform();
        let dd = if self.eq_peak > 0.0 {
            (1.0 - self.eq_index / self.eq_peak).max(0.0)
        } else {
            0.0
        };
        let now = plan.start + t as i32;
        match self.beh.panic.step(&pp, dd, u) {
            PanicStep::Nothing => {}
            PanicStep::Sell => {
                self.beh.panic.base = self.target;
                self.target = pp.panic_mix(&self.target);
                let before = self.ty.long_term_gains;
                self.rebalance(plan);
                let realised = self.ty.long_term_gains - before;
                self.event(now, TraceEventKind::Market, Some(realised / price), || {
                    format!(
                        "Panic sale: {:.0}% of stocks moved to cash after a {:.0}% fall",
                        pp.share * 100.0,
                        dd * 100.0
                    )
                });
            }
            PanicStep::BuyBack => {
                self.target = self.beh.panic.base;
                self.rebalance(plan);
                let months = self.beh.panic.months_out;
                self.event(now, TraceEventKind::Market, None, || {
                    format!("Back into stocks after {months} months in cash")
                });
            }
        }
    }

    /// Called whenever the plan sets a new target mix (the glide path): while out of
    /// the market the new mix is the base and the panic mix is applied to it.
    pub(crate) fn panic_retarget(&mut self, plan: &Plan) {
        let Some(pp) = plan.beh.panic else { return };
        if self.beh.panic.out {
            self.beh.panic.base = self.target;
            self.target = pp.panic_mix(&self.target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> PanicPlan {
        PanicPlan::compile(&PanicSelling::default())
    }

    #[test]
    fn the_panic_mix_moves_half_the_stocks_to_cash() {
        let mut w = [0.0; crate::model::N_ASSETS];
        w[AssetClass::UsStocks.index()] = 0.6;
        w[AssetClass::IntlStocks.index()] = 0.2;
        w[AssetClass::Bonds.index()] = 0.2;
        let m = plan().panic_mix(&w);
        assert!((m[AssetClass::UsStocks.index()] - 0.3).abs() < 1e-12);
        assert!((m[AssetClass::IntlStocks.index()] - 0.1).abs() < 1e-12);
        assert!((m[AssetClass::Cash.index()] - 0.4).abs() < 1e-12);
        assert!((m[AssetClass::Bonds.index()] - 0.2).abs() < 1e-12);
        assert!((m.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn one_decision_per_fall_then_buy_back_on_recovery_or_timeout() {
        let p = plan();
        let mut s = PanicState::default();
        // A 20% fall does not trigger; 26% does, and a draw above 8% means no panic.
        assert_eq!(s.step(&p, 0.20, 0.0), PanicStep::Nothing);
        assert_eq!(s.step(&p, 0.26, 0.5), PanicStep::Nothing);
        // The same fall deepening is not a second decision.
        assert_eq!(s.step(&p, 0.40, 0.0), PanicStep::Nothing);
        // A new high re-arms; the next fall with a low draw panics.
        assert_eq!(s.step(&p, 0.0, 0.9), PanicStep::Nothing);
        assert_eq!(s.step(&p, 0.30, 0.07), PanicStep::Sell);
        // Out until within 5% of the peak.
        assert_eq!(s.step(&p, 0.20, 0.0), PanicStep::Nothing);
        assert_eq!(s.step(&p, 0.06, 0.0), PanicStep::Nothing);
        assert_eq!(s.step(&p, 0.05, 0.0), PanicStep::BuyBack);
        assert_eq!(s.months_out, 3);
        // Not re-armed until a new high.
        assert_eq!(s.step(&p, 0.30, 0.0), PanicStep::Nothing);
        // Timeout: out for 36 months at most.
        let mut s = PanicState::default();
        assert_eq!(s.step(&p, 0.30, 0.0), PanicStep::Sell);
        for _ in 0..35 {
            assert_eq!(s.step(&p, 0.30, 0.0), PanicStep::Nothing);
        }
        assert_eq!(s.step(&p, 0.30, 0.0), PanicStep::BuyBack);
    }

    #[test]
    fn defaults_match_the_data_file() {
        let d = &crate::data::Data::embedded().actuarial["household_behavior"]["panic_selling"];
        let p = PanicSelling::default();
        let num = |k: &str| d[k].as_f64().unwrap();
        assert_eq!(p.drawdown_trigger, num("drawdown_trigger"));
        assert_eq!(p.probability, num("probability"));
        assert_eq!(p.share, num("share"));
        assert_eq!(p.reentry.recovered_within, num("reentry_recovered_within"));
        assert_eq!(f64::from(p.reentry.max_months), num("reentry_max_months"));
    }
}
