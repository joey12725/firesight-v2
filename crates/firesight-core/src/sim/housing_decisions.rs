//! Housing decisions on a path: borrowing against the home when savings run out, and
//! a planned move to a cheaper home (`downsize.rs`).
//!
//! Everything here is reached only through opt-in profile fields, compiled into
//! [`HdPlan`]. With all of them off, the month loop calls none of these functions
//! (each hook is behind a flag of the compiled plan) and the path is bit-identical
//! to an engine without them.
//!
//! # Where the hooks sit in the month
//!
//! * step 7 (housing), after every property has stepped: [`State::hd_month`] pays
//!   the interest and repayments of open home-equity lines and updates the lenders'
//!   freeze state; reverse mortgages accrue, and a standby reverse mortgage is opened
//!   when savings fall below its threshold; a planned downsizing is checked
//!   ([`State::hd_downsize`]);
//! * step 9 (settlement), when liquid savings cannot pay the month: before property
//!   is sold or an essential is cut, [`State::hd_borrow`] draws on a line;
//! * step 9, when there is a surplus to invest: [`State::hd_surplus`] first repays
//!   home-equity debt;
//! * any sale (`sell_slot`): [`State::hd_repay_at_sale`] repays the line out of the
//!   proceeds.
//!
//! # Long-term care
//!
//! A reverse mortgage requires the borrower to live in the home (a HECM falls due
//! once the last borrower has been in a care facility for twelve months), so none is
//! opened and no more is drawn on one while every adult of the household lives in a
//! care facility ([`State::hd_nobody_home`]). In a month in which Medicaid pays for
//! a nursing-home resident with no spouse at home, the shortfall is covered by the
//! safety net (`care.rs`), so nothing is borrowed on a home-equity line either.
//! A home-equity line may otherwise be drawn by a household in care: its lender does
//! not ask who lives in the home. An existing reverse mortgage is not called in when
//! its borrower moves to a facility: it keeps accruing until the home is sold or the
//! path ends (not modelled: the twelve-month due date).
//!
//! # Accounting
//!
//! A draw is cash received from property: it is booked in `property_sale` ("net cash
//! from property"), which is a transfer, not income. Interest and repayments are
//! `spend_housing`, like mortgage payments. Balances are part of the `mortgage`
//! series (liabilities secured by property). For the conservation check every change
//! of a line's liability is booked as a mortgage flow ([`State::hd_book`]).
//!
//! A reverse mortgage costs no cash while the household lives in the home: its
//! financed closing costs, interest and insurance are added to the balance, so they
//! show as a rising `mortgage` (and lower net worth), not as spending. Its liability
//! is never booked above the home's value (the loan is non-recourse), so the shortfall
//! of an underwater loan is the FHA insurer's loss, not the estate's.

use crate::economy::EconomyPath;
use crate::home_equity::{self, EquityLine, HelocTerms, LineKind, HOUSING_SLUMP_DRAWDOWN};
use crate::housing::HousingMonth;
use crate::model::{
    EventKind, HomeEquityBorrowing, Profile, PropertyUse, RentalDynamics, ReverseMortgageOpen,
};
use crate::rental_market::{RentalDraws, RentalMarketParams, RentalMarketState};
use crate::rng::{Rng, Stream};
use crate::stats::{TraceEventKind, YearSeries};

use super::cash_out::{CashOutPlan, Prepay};
use super::downsize::DownsizePlan;

/// `last_cash_out` before any cash-out refinance.
const NEVER_CASHED_OUT: i32 = i32::MIN / 2;
use super::plan::{Plan, PropPlan};
use super::state::State;

/// Compiled borrowing terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Borrow {
    None,
    Heloc(HelocTerms),
    Hecm(HecmTerms),
}

/// Compiled reverse-mortgage terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HecmTerms {
    /// Lender's margin over the Treasury rate.
    pub margin: f64,
    /// Other closing costs, today's dollars.
    pub other_costs_real: f64,
    /// Open early (the standby line) when savings fall below this many years of
    /// planned spending; `None` opens only as a last resort.
    pub standby_years: Option<f64>,
}

/// The housing decisions of a profile, compiled.
#[derive(Clone, Debug)]
pub(crate) struct HdPlan {
    pub borrow: Borrow,
    /// True when some line can be opened (borrowing is on).
    pub lines: bool,
    /// True when [`State::hd_month`] has work to do.
    pub monthly: bool,
    /// True when [`State::hd_surplus`] has work to do.
    pub surplus: bool,
    /// A planned downsizing (`retirement.downsize`).
    pub downsize: Option<DownsizePlan>,
    /// Per child, whether a college is planned (for `when_children_leave`).
    pub college_planned: Vec<bool>,
    /// Cash-out refinancing (`debt_policy.cash_out_refinance`, `cash_out.rs`).
    pub cash_out: Option<CashOutPlan>,
    /// Mortgage prepayment (`debt_policy.mortgage_prepayment`).
    pub prepay: Prepay,
    /// Realistic rental dynamics, per property slot.
    pub rentals: Vec<Option<RentalMarketParams>>,
    /// True when some slot has realistic rental dynamics.
    pub live_rentals: bool,
}

impl HdPlan {
    pub(crate) fn compile(profile: &Profile, props: &[PropPlan]) -> HdPlan {
        let borrow = match profile.retirement.home_equity_borrowing {
            HomeEquityBorrowing::None => Borrow::None,
            HomeEquityBorrowing::Heloc(h) => Borrow::Heloc(HelocTerms {
                max_cltv: h.max_combined_ltv.clamp(0.0, 1.0),
                spread: h.spread,
                draw_months: (h.draw_years.max(0.0) * 12.0).round() as u32,
                repay_months: (h.repay_years.max(0.0) * 12.0).round().max(1.0) as u32,
                freeze_probability: h.freeze_probability.clamp(0.0, 1.0),
            }),
            HomeEquityBorrowing::ReverseMortgage(r) => Borrow::Hecm(HecmTerms {
                margin: r.lender_margin,
                other_costs_real: r.other_closing_costs.max(0.0),
                standby_years: match r.open_when {
                    ReverseMortgageOpen::LastResort => None,
                    ReverseMortgageOpen::SavingsBelow { years } => Some(years),
                },
            }),
        };
        let lines = !matches!(borrow, Borrow::None);
        let downsize = profile
            .retirement
            .downsize
            .as_ref()
            .map(DownsizePlan::compile);
        let cash_out = profile
            .debt_policy
            .cash_out_refinance
            .as_ref()
            .map(CashOutPlan::compile);
        let prepay = Prepay::compile(&profile.debt_policy.mortgage_prepayment);
        let prepaying = !matches!(prepay, Prepay::Never);
        // Slots: the profile's properties, then one per purchase event (plan.rs).
        let usages =
            profile
                .properties
                .iter()
                .map(|p| p.usage)
                .chain(profile.events.iter().filter_map(|e| match &e.kind {
                    EventKind::BuyHome(b) => Some(b.usage),
                    _ => None,
                }));
        let mut rentals: Vec<Option<RentalMarketParams>> = usages
            .map(|u| match u {
                PropertyUse::Rental(terms) => match terms.dynamics {
                    RentalDynamics::Simple => None,
                    RentalDynamics::Realistic { annual_turnover } => {
                        Some(RentalMarketParams::new(terms.vacancy, annual_turnover))
                    }
                },
                _ => None,
            })
            .collect();
        rentals.resize(props.len(), None);
        let live_rentals = rentals.iter().any(Option::is_some);
        HdPlan {
            borrow,
            lines,
            monthly: lines || downsize.is_some() || cash_out.is_some() || prepaying,
            surplus: lines || prepaying,
            downsize,
            college_planned: super::downsize::college_planned(profile),
            cash_out,
            prepay,
            rentals,
            live_rentals,
        }
    }
}

/// Path state of the housing decisions. Lives in [`State`]; reset per path.
#[derive(Clone, Debug)]
pub(crate) struct HdState {
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
    pub long_yield: f64,
    /// True once the planned downsizing has happened (or lapsed).
    pub downsized: bool,
    /// The market's 30-year mortgage rate this month.
    pub mortgage_rate: f64,
    /// Month of the last cash-out refinance.
    pub last_cash_out: i32,
    /// No prepayment before this month (the cash of a cash-out is for spending).
    pub no_prepay_until: i32,
    /// Market state and stream of each rental with realistic dynamics.
    pub rentals: Vec<RentalMarketState>,
    pub rental_rng: Vec<Rng>,
}

impl HdState {
    pub(crate) fn new(plan: &Plan) -> HdState {
        HdState {
            lines: vec![EquityLine::default(); plan.props.len()],
            reported: vec![0.0; plan.props.len()],
            any_line: false,
            rng: Rng::from_seed(0),
            frozen: false,
            downturn: false,
            home_peak: 1.0,
            tbill: 0.0,
            long_yield: 0.0,
            downsized: false,
            mortgage_rate: 0.0,
            last_cash_out: NEVER_CASHED_OUT,
            no_prepay_until: 0,
            rentals: vec![RentalMarketState::default(); plan.props.len()],
            rental_rng: vec![Rng::from_seed(0); plan.props.len()],
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
        self.long_yield = 0.0;
        self.downsized = false;
        self.mortgage_rate = 0.0;
        self.last_cash_out = NEVER_CASHED_OUT;
        self.no_prepay_until = 0;
        self.rentals.fill(RentalMarketState::default());
        for (i, r) in plan.hd.rentals.iter().enumerate() {
            if r.is_some() {
                self.rental_rng[i] = Rng::stream(seed, path, Stream::RentalMarket(i));
            }
        }
        if let Borrow::Heloc(_) = plan.hd.borrow {
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
    /// Step 7 for property `i` when some rental has realistic dynamics: draws the
    /// rental's market month (five values, owned or not) and steps the property with
    /// it; other slots step as usual.
    pub(crate) fn hd_property_step(
        &mut self,
        plan: &Plan,
        i: usize,
        t: usize,
        home_national: f64,
        price: f64,
        recession: bool,
    ) -> HousingMonth {
        let model = &plan.props[i].model;
        let Some(params) = plan.hd.rentals[i] else {
            return model.step(
                &mut self.props[i],
                home_national,
                price,
                &mut self.rng_housing[i],
            );
        };
        let draws = RentalDraws::draw(&mut self.hd.rental_rng[i]);
        let (hm, o) = model.step_realistic(
            &mut self.props[i],
            home_national,
            price,
            &mut self.rng_housing[i],
            &params,
            &mut self.hd.rentals[i],
            &draws,
            recession,
        );
        if self.detail && self.props[i].owned {
            let now = plan.start + t as i32;
            let name = &plan.props[i].name;
            if o.eviction_months > 0.0 {
                self.event(
                    now,
                    TraceEventKind::LifeEvent,
                    Some(hm.maintenance / price),
                    || format!("Evicted a tenant from {name}"),
                );
            } else if o.moved_out {
                self.event(now, TraceEventKind::LifeEvent, None, || {
                    format!("Tenant moved out of {name}")
                });
            }
            if o.repair {
                self.event(
                    now,
                    TraceEventKind::MajorExpense,
                    Some(hm.maintenance / price),
                    || format!("Large repair on {name}"),
                );
            }
        }
        hm
    }

    /// Books the liability of the line on `slot` after any change to it or to the
    /// home's value: the change is a mortgage flow for the conservation check.
    pub(crate) fn hd_book(&mut self, slot: usize) {
        let st = &self.props[slot];
        let value = if st.owned { st.value } else { 0.0 };
        let liability = self.hd.lines[slot].liability(value);
        if self.check {
            self.chk_mort_flow += liability - self.hd.reported[slot];
        }
        self.hd.reported[slot] = liability;
        self.hd.any_line = self.hd.lines.iter().any(EquityLine::is_open)
            || self.hd.reported.iter().any(|r| *r != 0.0);
    }

    /// Step 7 hook: lenders' freeze state, then interest and repayments on every open
    /// line. Draws exactly one uniform from the lenders' stream each month while a
    /// home-equity line is part of the plan, whatever the household does. A reverse
    /// mortgage accrues, its non-recourse liability is rebooked against the home's new
    /// value, and a standby line is opened when savings fall low enough.
    pub(crate) fn hd_month(&mut self, plan: &Plan, econ: &EconomyPath, t: usize, price: f64) {
        let now = plan.start + t as i32;
        self.hd.tbill = econ.tbill[t];
        self.hd.long_yield = econ.long_yield[t];
        self.hd.mortgage_rate = econ.mortgage_rate[t];
        if let Borrow::Heloc(terms) = plan.hd.borrow {
            let u = self.hd.rng.uniform();
            if self.home_index > self.hd.home_peak {
                self.hd.home_peak = self.home_index;
            }
            let downturn = econ.recession[t] != 0
                || self.home_index < (1.0 - HOUSING_SLUMP_DRAWDOWN) * self.hd.home_peak;
            let was = self.hd.frozen;
            if downturn && !self.hd.downturn {
                self.hd.frozen = u < terms.freeze_probability;
            } else if !downturn {
                self.hd.frozen = false;
            }
            self.hd.downturn = downturn;
            if self.hd.frozen != was {
                let frozen = self.hd.frozen;
                self.event(now, TraceEventKind::Market, None, || {
                    if frozen {
                        "Lenders freeze home-equity lines".to_string()
                    } else {
                        "Home-equity lines open again".to_string()
                    }
                });
            }
            for slot in 0..self.hd.lines.len() {
                if self.hd.lines[slot].kind != LineKind::Heloc {
                    continue;
                }
                let m = terms.month(&mut self.hd.lines[slot], self.hd.tbill);
                let paid = m.interest + m.principal;
                if paid > 0.0 {
                    self.spend(YearSeries::SpendHousing, paid);
                }
                self.hd_book(slot);
            }
        }
        if let Borrow::Hecm(terms) = plan.hd.borrow {
            for slot in 0..self.hd.lines.len() {
                if self.hd.lines[slot].kind != LineKind::Hecm {
                    continue;
                }
                home_equity::hecm_month(&mut self.hd.lines[slot], self.hd.tbill, terms.margin);
                self.hd_book(slot);
            }
            if let Some(years) = terms.standby_years {
                let annual = (self.ess_real + self.disc_real) * 12.0 * price;
                if self.acc.investable() < years * annual
                    && !self.hd.lines.iter().any(EquityLine::is_open)
                {
                    for slot in 0..self.props.len() {
                        if self.hd_open_hecm(plan, &terms, t, price, slot) {
                            break;
                        }
                    }
                }
            }
        }
        if let Some(d) = &plan.hd.downsize {
            self.hd_downsize(plan, d, t, price, econ.mortgage_rate[t]);
        }
    }

    /// Age of the youngest adult present, or `None` if nobody is.
    fn hd_youngest_age(&self, plan: &Plan, t: usize) -> Option<f64> {
        let mut youngest: Option<f64> = None;
        for i in 0..self.n {
            if self.present[i] && self.alive[i] {
                let a = self.age(plan, i, t);
                youngest = Some(youngest.map_or(a, |y| y.min(a)));
            }
        }
        youngest
    }

    /// True when nobody lives in the home: every adult present is in a care
    /// facility. False for an empty household.
    pub(crate) fn hd_nobody_home(&self) -> bool {
        let mut anyone = false;
        for i in 0..self.n {
            if self.present[i] && self.alive[i] {
                if !self.in_facility[i] {
                    return false;
                }
                anyone = true;
            }
        }
        anyone
    }

    /// Opens a reverse mortgage on property `slot` if it is the owned main home, has
    /// no line yet, an adult still lives in it, the youngest adult is at least 62 and
    /// the principal limit covers the costs and any mortgage (which it repays).
    /// Returns true if one was opened.
    fn hd_open_hecm(
        &mut self,
        plan: &Plan,
        terms: &HecmTerms,
        t: usize,
        price: f64,
        slot: usize,
    ) -> bool {
        let st = &self.props[slot];
        if !st.owned || !matches!(st.usage, PropertyUse::Primary) || self.hd.lines[slot].is_open() {
            return false;
        }
        if self.hd_nobody_home() {
            return false;
        }
        let Some(age) = self.hd_youngest_age(plan, t) else {
            return false;
        };
        // The FHA limit moves with house prices (HUD indexes it to the FHFA index).
        let limit = home_equity::HECM_MAX_CLAIM_2026 * self.home_index;
        let Some(o) = home_equity::open_hecm(
            age,
            st.value,
            st.mortgage_balance(),
            limit,
            self.hd.long_yield,
            terms.margin,
            terms.other_costs_real * price,
        ) else {
            return false;
        };
        if o.mortgage_payoff > 0.0 {
            // The reverse mortgage repays the old loan: that debt moves onto the line.
            if self.check {
                self.chk_mort_flow -= o.mortgage_payoff;
            }
            self.props[slot].mortgage = None;
        }
        self.hd.lines[slot] = o.line;
        self.hd_book(slot);
        self.req_dirty = true;
        self.budget_dirty = true;
        let now = plan.start + t as i32;
        self.event(
            now,
            TraceEventKind::LifeEvent,
            Some(o.line.limit / price),
            || format!("Took a reverse mortgage on {}", plan.props[slot].name),
        );
        true
    }

    /// Step 9 hook: savings cannot pay the month and `missing` dollars are short.
    /// Draws on a line against the main home, opening one if needed and allowed.
    /// Returns what was raised.
    pub(crate) fn hd_borrow(&mut self, plan: &Plan, t: usize, price: f64, missing: f64) -> f64 {
        if missing <= 0.0 {
            return 0.0;
        }
        if plan.care.active && self.care_safety_net(t) {
            // Medicaid's safety net covers this month: nothing is borrowed for it.
            return 0.0;
        }
        let nobody_home = self.hd_nobody_home();
        let now = plan.start + t as i32;
        let mut raised = 0.0;
        for slot in 0..self.props.len() {
            let st = self.props[slot];
            if !st.owned || !matches!(st.usage, PropertyUse::Primary) {
                continue;
            }
            let (value, mortgage) = (st.value, st.mortgage_balance());
            let take = match plan.hd.borrow {
                Borrow::None => 0.0,
                Borrow::Heloc(terms) => {
                    let line = &mut self.hd.lines[slot];
                    if !line.is_open() {
                        if self.hd.frozen {
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
                    let line = &self.hd.lines[slot];
                    terms
                        .available(line, value, mortgage, self.hd.frozen)
                        .min(missing - raised)
                }
                Borrow::Hecm(terms) => {
                    if nobody_home {
                        // A reverse mortgage pays out only to a borrower living in
                        // the home.
                        continue;
                    }
                    if !self.hd.lines[slot].is_open()
                        && !self.hd_open_hecm(plan, &terms, t, price, slot)
                    {
                        continue;
                    }
                    home_equity::hecm_available(&self.hd.lines[slot]).min(missing - raised)
                }
            };
            if take > 0.0 {
                home_equity::draw(&mut self.hd.lines[slot], take);
                self.recv(YearSeries::PropertySale, take);
                self.hd_book(slot);
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
    pub(crate) fn hd_surplus(&mut self, plan: &Plan, t: usize, cap: f64) {
        let now = plan.start + t as i32;
        for slot in 0..self.hd.lines.len() {
            let spare = self.acc.cash - cap;
            if spare <= 0.0 {
                break;
            }
            let line = &mut self.hd.lines[slot];
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
            self.hd_book(slot);
            if cleared {
                self.event(now, TraceEventKind::DebtPaidOff, None, || {
                    format!("Repaid the home-equity line on {}", plan.props[slot].name)
                });
            }
        }
        if plan.hd.surplus && !matches!(plan.hd.prepay, Prepay::Never) {
            self.hd_prepay(plan, t, cap);
        }
    }

    /// A property is being sold for `net` (after selling costs and the mortgage):
    /// its line is repaid from the proceeds and closed. Returns what is left. A
    /// home-equity line is repaid in full (the seller brings cash if the proceeds do
    /// not cover it); a reverse mortgage takes at most the proceeds.
    pub(crate) fn hd_repay_at_sale(&mut self, slot: usize, net: f64) -> f64 {
        let line = self.hd.lines[slot];
        if !line.is_open() {
            return net;
        }
        let repaid = match line.kind {
            LineKind::Hecm => line.balance.min(net.max(0.0)),
            _ => line.balance,
        };
        self.hd.lines[slot] = EquityLine::default();
        self.hd_book(slot);
        net - repaid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy::EconomyPath;
    use crate::model::{ReverseMortgage, EXAMPLE_START};

    /// The full-life couple with a reverse mortgage as a last resort, at the start
    /// of path 0, with the mortgage on their home cleared.
    fn reverse_mortgage_household() -> (Plan, State, usize) {
        let mut p = Profile::examples(EXAMPLE_START)
            .into_iter()
            .find(|e| e.slug == "full-life")
            .unwrap()
            .profile;
        p.retirement.home_equity_borrowing =
            HomeEquityBorrowing::ReverseMortgage(ReverseMortgage::default());
        p.spending.healthcare.medicaid = true;
        let plan = Plan::compile(&p, crate::data::Data::embedded()).unwrap();
        let mut st = State::new(&plan);
        let mut econ = EconomyPath::with_capacity(plan.horizon_months() as usize);
        st.begin(&plan, 3, 0, &mut econ, false, false);
        let slot = (0..st.props.len())
            .find(|&s| st.props[s].owned && st.props[s].usage == PropertyUse::Primary)
            .unwrap();
        st.props[slot].mortgage = None;
        st.hd.long_yield = 0.04;
        (plan, st, slot)
    }

    #[test]
    fn no_reverse_mortgage_once_nobody_lives_in_the_home() {
        let (plan, mut st, slot) = reverse_mortgage_household();
        // A month in which both are past 62.
        let t = 45 * 12;
        assert!(st.present[0] && st.present[1] && !st.hd_nobody_home());
        // Both in a care facility: no reverse mortgage is opened.
        st.in_facility = [true, true];
        assert!(st.hd_nobody_home());
        assert_eq!(st.hd_borrow(&plan, t, 1.0, 5_000.0), 0.0);
        assert!(!st.hd.lines[slot].is_open());
        // One of them at home: it is, and pays the shortfall.
        st.in_facility = [true, false];
        assert!(!st.hd_nobody_home());
        assert_eq!(st.hd_borrow(&plan, t, 1.0, 5_000.0), 5_000.0);
        assert!(st.hd.lines[slot].is_open());
        // The second moves to a facility too: nothing more is drawn.
        st.in_facility = [true, true];
        assert_eq!(st.hd_borrow(&plan, t, 1.0, 5_000.0), 0.0);
        // A widow in a facility is nobody at home; a widow at home is somebody.
        st.present[0] = false;
        st.alive[0] = false;
        assert!(st.hd_nobody_home());
        st.in_facility[1] = false;
        assert!(!st.hd_nobody_home());
        assert_eq!(st.hd_borrow(&plan, t, 1.0, 5_000.0), 5_000.0);
    }

    #[test]
    fn nothing_is_borrowed_in_a_month_the_safety_net_covers() {
        let (plan, mut st, slot) = reverse_mortgage_household();
        let t = 45 * 12;
        assert!(plan.care.active);
        // Medicaid pays this month for a nursing-home resident with no spouse at
        // home (`care.rs` sets the month): the shortfall is not borrowed.
        st.care.safety_net_month = t as i32;
        assert_eq!(st.hd_borrow(&plan, t, 1.0, 5_000.0), 0.0);
        assert!(!st.hd.lines[slot].is_open());
        // The month after, with someone at home, it is.
        assert_eq!(st.hd_borrow(&plan, t + 1, 1.0, 5_000.0), 5_000.0);
    }
}
