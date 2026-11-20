//! Housing decisions on a path: borrowing against the home when savings run out.
//!
//! Everything here is reached only through opt-in profile fields, compiled into
//! [`HxPlan`]. With all of them off, the month loop calls none of these functions
//! (each hook is behind a flag of the compiled plan) and the path is bit-identical
//! to an engine without them.
//!
//! # Where the hooks sit in the month
//!
//! * step 7 (housing), after every property has stepped: [`State::hx_month`] pays
//!   the interest and repayments of open lines and updates the lenders' freeze
//!   state;
//! * step 9 (settlement), when liquid savings cannot pay the month: before property
//!   is sold or an essential is cut, [`State::hx_borrow`] draws on a line;
//! * step 9, when there is a surplus to invest: [`State::hx_surplus`] first repays
//!   home-equity debt;
//! * any sale (`sell_slot`): [`State::hx_repay_at_sale`] repays the line out of the
//!   proceeds.
//!
//! # Accounting
//!
//! A draw is cash received from property: it is booked in `property_sale` ("net cash
//! from property"), which is a transfer, not income. Interest and repayments are
//! `spend_housing`, like mortgage payments. Balances are part of the `mortgage`
//! series (liabilities secured by property). For the conservation check every change
//! of a line's liability is booked as a mortgage flow ([`State::hx_book`]).

use crate::economy::EconomyPath;
use crate::home_equity::{self, EquityLine, HelocTerms, LineKind, HOUSING_SLUMP_DRAWDOWN};
use crate::model::{HomeEquityBorrowing, Profile, PropertyUse};
use crate::rng::{Rng, Stream};
use crate::stats::{TraceEventKind, YearSeries};

use super::plan::{Plan, PropPlan};
use super::state::State;

/// Compiled borrowing terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Borrow {
    None,
    Heloc(HelocTerms),
}

/// The housing decisions of a profile, compiled.
#[derive(Clone, Debug)]
pub(crate) struct HxPlan {
    pub borrow: Borrow,
    /// True when some line can be opened (borrowing is on).
    pub lines: bool,
    /// True when [`State::hx_month`] has work to do.
    pub monthly: bool,
    /// True when [`State::hx_surplus`] has work to do.
    pub surplus: bool,
}

impl HxPlan {
    pub(crate) fn compile(profile: &Profile, _props: &[PropPlan]) -> HxPlan {
        let borrow = match profile.retirement.home_equity_borrowing {
            HomeEquityBorrowing::None => Borrow::None,
            HomeEquityBorrowing::Heloc(h) => Borrow::Heloc(HelocTerms {
                max_cltv: h.max_combined_ltv.clamp(0.0, 1.0),
                spread: h.spread,
                draw_months: (h.draw_years.max(0.0) * 12.0).round() as u32,
                repay_months: (h.repay_years.max(0.0) * 12.0).round().max(1.0) as u32,
                freeze_probability: h.freeze_probability.clamp(0.0, 1.0),
            }),
        };
        let lines = !matches!(borrow, Borrow::None);
        HxPlan {
            borrow,
            lines,
            monthly: lines,
            surplus: lines,
        }
    }
}

/// Path state of the housing decisions. Lives in [`State`]; reset per path.
#[derive(Clone, Debug)]
pub(crate) struct HxState {
    /// One line per property slot.
    pub lines: Vec<EquityLine>,
    /// Liability of each line as last booked (what `property_totals` reports).
    pub reported: Vec<f64>,
    /// True while any line has a liability booked.
    pub any_line: bool,
    /// The lenders' stream (`Stream::HomeEquityCredit`).
    pub rng: Rng,
    /// True while lenders have frozen home-equity lines.
    pub frozen: bool,
    /// True while the economy is in a downturn (recession or housing slump).
    pub downturn: bool,
    /// Running peak of the national home price index.
    pub home_peak: f64,
    /// Rates of the current month, kept for the settlement step.
    pub tbill: f64,
}

impl HxState {
    pub(crate) fn new(plan: &Plan) -> HxState {
        HxState {
            lines: vec![EquityLine::default(); plan.props.len()],
            reported: vec![0.0; plan.props.len()],
            any_line: false,
            rng: Rng::from_seed(0),
            frozen: false,
            downturn: false,
            home_peak: 1.0,
            tbill: 0.0,
        }
    }

    /// Resets for path `path`.
    pub(crate) fn begin(&mut self, plan: &Plan, seed: u64, path: u64) {
        self.lines.fill(EquityLine::default());
        self.reported.fill(0.0);
        self.any_line = false;
        self.frozen = false;
        self.downturn = false;
        self.home_peak = 1.0;
        self.tbill = 0.0;
        if let Borrow::Heloc(_) = plan.hx.borrow {
            self.rng = Rng::stream(seed, path, Stream::HomeEquityCredit);
        }
    }

    /// Sum of the liabilities booked on every line.
    pub(crate) fn liability(&self) -> f64 {
        let mut total = 0.0;
        for r in &self.reported {
            total += r;
        }
        total
    }

    /// Liability booked on the line of property `slot`.
    pub(crate) fn lien(&self, slot: usize) -> f64 {
        self.reported[slot]
    }
}

impl State {
    /// Books the liability of the line on `slot` after any change to it or to the
    /// home's value: the change is a mortgage flow for the conservation check.
    pub(crate) fn hx_book(&mut self, slot: usize) {
        let st = &self.props[slot];
        let value = if st.owned { st.value } else { 0.0 };
        let liability = self.hx.lines[slot].liability(value);
        if self.check {
            self.chk_mort_flow += liability - self.hx.reported[slot];
        }
        self.hx.reported[slot] = liability;
        self.hx.any_line = self.hx.lines.iter().any(EquityLine::is_open)
            || self.hx.reported.iter().any(|r| *r != 0.0);
    }

    /// Step 7 hook: lenders' freeze state, then interest and repayments on every open
    /// line. Draws exactly one uniform from the lenders' stream each month while a
    /// home-equity line is part of the plan, whatever the household does.
    pub(crate) fn hx_month(&mut self, plan: &Plan, econ: &EconomyPath, t: usize) {
        let now = plan.start + t as i32;
        self.hx.tbill = econ.tbill[t];
        if let Borrow::Heloc(terms) = plan.hx.borrow {
            let u = self.hx.rng.uniform();
            if self.home_index > self.hx.home_peak {
                self.hx.home_peak = self.home_index;
            }
            let downturn = econ.recession[t] != 0
                || self.home_index < (1.0 - HOUSING_SLUMP_DRAWDOWN) * self.hx.home_peak;
            let was = self.hx.frozen;
            if downturn && !self.hx.downturn {
                self.hx.frozen = u < terms.freeze_probability;
            } else if !downturn {
                self.hx.frozen = false;
            }
            self.hx.downturn = downturn;
            if self.hx.frozen != was {
                let frozen = self.hx.frozen;
                self.event(now, TraceEventKind::Market, None, || {
                    if frozen {
                        "Lenders freeze home-equity lines".to_string()
                    } else {
                        "Home-equity lines open again".to_string()
                    }
                });
            }
            for slot in 0..self.hx.lines.len() {
                if self.hx.lines[slot].kind != LineKind::Heloc {
                    continue;
                }
                let m = terms.month(&mut self.hx.lines[slot], self.hx.tbill);
                let paid = m.interest + m.principal;
                if paid > 0.0 {
                    self.spend(YearSeries::SpendHousing, paid);
                }
                self.hx_book(slot);
            }
        }
    }

    /// Step 9 hook: savings cannot pay the month and `missing` dollars are short.
    /// Draws on a line against the main home, opening one if needed and allowed.
    /// Returns what was raised.
    pub(crate) fn hx_borrow(&mut self, plan: &Plan, t: usize, price: f64, missing: f64) -> f64 {
        if missing <= 0.0 {
            return 0.0;
        }
        let now = plan.start + t as i32;
        let mut raised = 0.0;
        for slot in 0..self.props.len() {
            let st = self.props[slot];
            if !st.owned || !matches!(st.usage, PropertyUse::Primary) {
                continue;
            }
            let (value, mortgage) = (st.value, st.mortgage_balance());
            let take = match plan.hx.borrow {
                Borrow::None => 0.0,
                Borrow::Heloc(terms) => {
                    let line = &mut self.hx.lines[slot];
                    if !line.is_open() {
                        if self.hx.frozen {
                            continue;
                        }
                        let limit = terms.credit_limit(value, mortgage);
                        if limit <= 0.0 {
                            continue;
                        }
                        *line = EquityLine {
                            kind: LineKind::Heloc,
                            limit,
                            ..EquityLine::default()
                        };
                        self.event(now, TraceEventKind::LifeEvent, Some(limit / price), || {
                            format!("Opened a home-equity line on {}", plan.props[slot].name)
                        });
                    }
                    let line = &self.hx.lines[slot];
                    terms
                        .available(line, value, mortgage, self.hx.frozen)
                        .min(missing - raised)
                }
            };
            if take > 0.0 {
                home_equity::draw(&mut self.hx.lines[slot], take);
                self.recv(YearSeries::PropertySale, take);
                self.hx_book(slot);
                raised += take;
                self.req_dirty = true;
                self.event(now, TraceEventKind::LifeEvent, Some(take / price), || {
                    format!("Borrowed against {}", plan.props[slot].name)
                });
            }
            if raised >= missing {
                break;
            }
        }
        raised
    }

    /// Step 9 hook: cash above `cap` is about to be invested. Home-equity debt is
    /// repaid from it first.
    pub(crate) fn hx_surplus(&mut self, plan: &Plan, t: usize, cap: f64) {
        let now = plan.start + t as i32;
        for slot in 0..self.hx.lines.len() {
            let spare = self.acc.cash - cap;
            if spare <= 0.0 {
                return;
            }
            let line = &mut self.hx.lines[slot];
            if line.kind != LineKind::Heloc || line.balance <= 0.0 {
                continue;
            }
            let pay = spare.min(line.balance);
            line.balance -= pay;
            let cleared = line.balance <= 0.005;
            if cleared {
                line.balance = 0.0;
            }
            self.spend(YearSeries::SpendHousing, pay);
            self.hx_book(slot);
            if cleared {
                self.event(now, TraceEventKind::DebtPaidOff, None, || {
                    format!("Repaid the home-equity line on {}", plan.props[slot].name)
                });
            }
        }
    }

    /// A property is being sold for `net` (after selling costs and the mortgage):
    /// its line is repaid from the proceeds and closed. Returns what is left. A
    /// home-equity line is repaid in full (the seller brings cash if the proceeds do
    /// not cover it); a reverse mortgage takes at most the proceeds.
    pub(crate) fn hx_repay_at_sale(&mut self, slot: usize, net: f64) -> f64 {
        let line = self.hx.lines[slot];
        if !line.is_open() {
            return net;
        }
        let repaid = match line.kind {
            LineKind::Hecm => line.balance.min(net.max(0.0)),
            _ => line.balance,
        };
        self.hx.lines[slot] = EquityLine::default();
        self.hx_book(slot);
        net - repaid
    }
}
