//! Opt-in housing realism at the level of the path: the local rent market, slow
//! home sales, and the hooks that drive the per-property options of
//! [`crate::housing`] (lumpy maintenance, insurance drift, adjustable rates).
//!
//! Everything here is behind compiled flags ([`HxPlan`]); a profile that uses none
//! of the options never reaches this code and draws nothing from the new streams.
//!
//! # Rent market (`rent_growth: market` on a rent line)
//!
//! One real rent index per path, from `Stream::RentMarket` (2 words a month):
//!
//! ```text
//! ln N += (RENT_REAL_TREND + HPI_PASSTHROUGH (12 e - HPI_REAL_TREND)) / 12
//! e    += (real home-price growth this month - e) / 12            (lagged home prices)
//! l'    = LOCAL_PERSISTENCE l + LOCAL_SD sqrt(1 - rho^2) z         (local noise)
//! market rent M = exp(ln N + l)
//! ```
//!
//! The household pays `paid`, set once a year at lease renewal (the budget's yearly
//! anniversary): usually `paid *= (M / paid)^RENEWAL_PASS_THROUGH`; with probability
//! `FORCED_MOVE_PROBABILITY` the lease is not renewed and the household takes a new
//! one at `M (1 + NEW_LEASE_PREMIUM)`. The budget line is its amount times `paid`.
//!
//! # Housing liquidity (`assumptions.housing_liquidity: realistic`)
//!
//! A sale becomes a listing that closes `months` later at a discount; see
//! [`Liquidity`]. `Stream::Liquidity` supplies 2 words per property slot every
//! month, so the draws do not depend on when a household decides to sell.

use crate::economy::EconomyPath;
use crate::math;
use crate::model::{Category, HousingLiquidity, Profile, RentGrowth};
use crate::rng::{normal_from_bits, Rng, Stream};
use crate::stats::TraceEventKind;

use super::plan::{Plan, PropPlan};
use super::state::State;

/// Long-run real growth of market rents a year; equals the engine's default real
/// growth of a rent line (`Category::Rent`) so that the option changes variability
/// and timing, not the expected level.
pub const RENT_REAL_TREND: f64 = 0.005;
/// How strongly rents follow home prices: each point of (lagged) real home-price
/// growth above [`HPI_REAL_TREND`] adds this many points of real rent growth.
/// JUDGEMENT: rents follow prices with a lag and a small coefficient (CPI shelter
/// responds to home prices with a delay of a year or more; the research file
/// `housing.rent_growth` has local rents co-moving with the local price factor).
pub const HPI_PASSTHROUGH: f64 = 0.15;
/// Long-run real growth of the national home price index a year, the centre for
/// the pass-through. JUDGEMENT (real prices rose about 1% a year since the 1970s).
pub const HPI_REAL_TREND: f64 = 0.01;
/// Monthly persistence of the local rent deviation (half-life about 4 years).
/// JUDGEMENT.
pub const LOCAL_PERSISTENCE: f64 = 0.985;
/// Stationary sd of the local rent deviation from the national path. JUDGEMENT:
/// the research file gives an annual sd of real rent growth of about 2%; metro
/// rents drift apart by more than that over several years.
pub const LOCAL_SD: f64 = 0.05;
/// Share of the gap between the rent paid and the market rent closed at a renewal.
/// JUDGEMENT: landlords smooth increases for sitting tenants.
pub const RENEWAL_PASS_THROUGH: f64 = 0.75;
/// Chance a lease is not renewed in a year (landlord sells, converts or raises past
/// what the household will pay) and the household moves to a new lease at market.
/// JUDGEMENT.
pub const FORCED_MOVE_PROBABILITY: f64 = 0.06;
/// A new lease costs this much above the market rent index. JUDGEMENT.
pub const NEW_LEASE_PREMIUM: f64 = 0.04;
/// Largest and smallest the paid rent may be relative to its start (a sanity bound).
pub const RENT_INDEX_RANGE: (f64, f64) = (0.25, 8.0);

/// Months a sale takes to close in a normal market: about 3 to 4 weeks on the market
/// plus 30 to 45 days to close. JUDGEMENT (NAR reported 10 weeks median time on
/// market at the 2009 peak for non-distressed sellers against 6 to 7 weeks in a
/// balanced market; see params.json `housing_realism.liquidity`).
pub const BASE_CLOSE_MONTHS: u32 = 2;
/// Mean extra months (exponentially distributed) in a normal market. JUDGEMENT.
pub const BASE_EXTRA_MONTHS: f64 = 0.5;
/// Additional mean extra months at full market stress. JUDGEMENT: puts the mean
/// at about 5.5 months in a 2009-style market.
pub const STRESS_EXTRA_MONTHS: f64 = 3.0;
/// Longest a sale may take.
pub const MAX_CLOSE_MONTHS: u32 = 24;
/// Mean price discount at full stress. Campbell, Giglio and Pathak (American
/// Economic Review 2011): forced sales (death, bankruptcy) close 3 to 7% below
/// comparable homes; 6% is used. The discount is scaled by `0.5 + u` (mean 1).
pub const FULL_STRESS_DISCOUNT: f64 = 0.06;
/// No discount exceeds this.
pub const MAX_DISCOUNT: f64 = 0.25;
/// A fall of the national price index over twelve months of this size counts as
/// full stress (2008 saw about 10 to 12%). JUDGEMENT.
pub const FULL_STRESS_PRICE_FALL: f64 = 0.10;

const F64_UNIT: f64 = 1.0 / (1u64 << 53) as f64;

#[inline]
fn unit(bits: u64) -> f64 {
    (bits >> 11) as f64 * F64_UNIT
}

/// Which options a plan uses.
#[derive(Clone, Debug, Default)]
pub(crate) struct HxPlan {
    /// Some property slot has lumpy maintenance, insurance drift or an ARM.
    pub props: bool,
    /// Some rent line follows the rent market.
    pub rent_market: bool,
    /// Home sales are slow.
    pub liquidity: bool,
    /// Any of the above.
    pub any: bool,
}

impl HxPlan {
    pub(crate) fn compile(profile: &Profile, props: &[PropPlan]) -> HxPlan {
        let props_on = props.iter().any(|p| p.model.has_ext());
        let rent_market = profile
            .spending
            .items
            .iter()
            .any(|i| i.category == Category::Rent && i.rent_growth == RentGrowth::Market);
        let liquidity = profile.assumptions.housing_liquidity == HousingLiquidity::Realistic;
        HxPlan {
            props: props_on,
            rent_market,
            liquidity,
            any: props_on || rent_market || liquidity,
        }
    }
}

/// The local rent market of one path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RentMarket {
    /// ln of the national real rent index.
    pub log_national: f64,
    /// Local deviation (log).
    pub local: f64,
    /// Smoothed real home-price growth per month.
    pub hpi_ema: f64,
    /// Market rent index, `exp(log_national + local)`.
    pub market: f64,
    /// The rent index the household pays (1 at the start).
    pub paid: f64,
}

impl Default for RentMarket {
    fn default() -> Self {
        RentMarket {
            log_national: 0.0,
            local: 0.0,
            hpi_ema: 0.0,
            market: 1.0,
            paid: 1.0,
        }
    }
}

impl RentMarket {
    /// Advances the market one month given the national home-price change, CPI
    /// inflation, and two random words.
    pub fn step_month(&mut self, home_national: f64, inflation: f64, words: [u64; 2]) {
        let real_hpi = (1.0 + home_national) / (1.0 + inflation) - 1.0;
        self.hpi_ema += (real_hpi - self.hpi_ema) / 12.0;
        self.log_national +=
            (RENT_REAL_TREND + HPI_PASSTHROUGH * (12.0 * self.hpi_ema - HPI_REAL_TREND)) / 12.0;
        let sd = LOCAL_SD * (1.0 - LOCAL_PERSISTENCE * LOCAL_PERSISTENCE).sqrt();
        self.local = LOCAL_PERSISTENCE * self.local + sd * normal_from_bits(words[0]);
        self.market = math::exp(self.log_national + self.local);
    }

    /// Lease renewal with the uniform draw `u`. Returns true if the household was
    /// forced to move.
    pub fn renew(&mut self, u: f64) -> bool {
        let forced = u < FORCED_MOVE_PROBABILITY;
        if forced {
            self.paid = self.market * (1.0 + NEW_LEASE_PREMIUM);
        } else {
            self.paid *= math::pow(self.market / self.paid, RENEWAL_PASS_THROUGH);
        }
        self.paid = self.paid.clamp(RENT_INDEX_RANGE.0, RENT_INDEX_RANGE.1);
        forced
    }
}

/// A sale that has been listed and will close.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingSale {
    /// Month index at which it closes.
    pub close_month: i32,
    /// Selling costs as a fraction of the price.
    pub cost_fraction: f64,
    /// Rent taken on after the sale, today's dollars a month.
    pub then_rent: f64,
    /// Price discount at closing.
    pub discount: f64,
    /// Market value when the sale was listed: the price is agreed then.
    pub list_value: f64,
    /// True for a last-resort sale (the household ran out of money).
    pub forced: bool,
}

/// Market stress in `[0, 1]`: half for a recession, half for a fall of the national
/// price index over the last twelve months (full at [`FULL_STRESS_PRICE_FALL`]).
pub fn market_stress(recession: bool, price_change_12m: f64) -> f64 {
    let fall = (-price_change_12m / FULL_STRESS_PRICE_FALL).clamp(0.0, 1.0);
    0.5 * f64::from(u8::from(recession)) + 0.5 * fall
}

/// Time to close and discount of a sale listed under `stress`, from two uniforms.
pub fn sale_terms(stress: f64, u_time: f64, u_discount: f64) -> (u32, f64) {
    let mean_extra = BASE_EXTRA_MONTHS + STRESS_EXTRA_MONTHS * stress;
    let extra = mean_extra * -math::ln(1.0 - u_time);
    let months = (BASE_CLOSE_MONTHS + extra.floor() as u32).min(MAX_CLOSE_MONTHS);
    let discount = (FULL_STRESS_DISCOUNT * stress * (0.5 + u_discount)).min(MAX_DISCOUNT);
    (months, discount)
}

/// Run-time state of the options on one path.
#[derive(Clone, Debug)]
pub(crate) struct HxState {
    pub rng_maint: Vec<Rng>,
    pub rng_ins: Vec<Rng>,
    pub rng_rent: Rng,
    pub rng_liq: Rng,
    pub rent: RentMarket,
    pub pending: Vec<Option<PendingSale>>,
    pub liq_words: Vec<[u64; 2]>,
    pub stress: f64,
}

impl HxState {
    pub(crate) fn new(plan: &Plan) -> HxState {
        let rng = Rng::from_seed(0);
        let n = plan.props.len();
        HxState {
            rng_maint: vec![rng.clone(); n],
            rng_ins: vec![rng.clone(); n],
            rng_rent: rng.clone(),
            rng_liq: rng,
            rent: RentMarket::default(),
            pending: vec![None; n],
            liq_words: vec![[0; 2]; n],
            stress: 0.0,
        }
    }

    /// Starts a path.
    pub(crate) fn reset(&mut self, plan: &Plan, seed: u64, path: u64) {
        self.rent = RentMarket::default();
        self.stress = 0.0;
        if !plan.hx.any {
            return;
        }
        for (i, pp) in plan.props.iter().enumerate() {
            if pp.model.has_lumpy_maintenance() {
                self.rng_maint[i] = Rng::stream(seed, path, Stream::Maintenance(i));
            }
            if pp.model.has_insurance_drift() {
                self.rng_ins[i] = Rng::stream(seed, path, Stream::InsuranceDrift(i));
            }
        }
        if plan.hx.rent_market {
            self.rng_rent = Rng::stream(seed, path, Stream::RentMarket);
        }
        if plan.hx.liquidity {
            self.rng_liq = Rng::stream(seed, path, Stream::Liquidity);
            self.pending.fill(None);
        }
    }
}

impl State {
    /// Start of month: moves the rent market and draws the sale terms.
    pub(crate) fn hx_month_start(&mut self, plan: &Plan, econ: &EconomyPath, t: usize) {
        if plan.hx.rent_market {
            let w = [self.hx.rng_rent.next_u64(), self.hx.rng_rent.next_u64()];
            self.hx
                .rent
                .step_month(econ.home_national[t], econ.inflation[t], w);
            if t > 0 && t % 12 == 0 {
                let before = self.hx.rent.paid;
                let forced = self.hx.rent.renew(unit(w[1]));
                self.budget_dirty = true;
                let now = plan.start + t as i32;
                let change = self.hx.rent.paid / before - 1.0;
                self.event(now, TraceEventKind::LifeEvent, None, || {
                    if forced {
                        format!(
                            "Rent: lease not renewed, new lease at market ({:+.1}%)",
                            change * 100.0
                        )
                    } else {
                        format!("Rent: lease renewed at market ({:+.1}%)", change * 100.0)
                    }
                });
            }
        }
        if plan.hx.liquidity {
            let mut growth = 1.0;
            for k in 1..=12usize.min(t) {
                growth *= 1.0 + econ.home_national[t - k];
            }
            self.hx.stress = market_stress(econ.recession[t] != 0, growth - 1.0);
            for w in &mut self.hx.liq_words {
                *w = [self.hx.rng_liq.next_u64(), self.hx.rng_liq.next_u64()];
            }
        }
    }

    /// Before a property's monthly step: draws and advances its extra state.
    /// Returns the mortgage rate before the step, to detect a reset afterwards.
    pub(crate) fn hx_prepare(
        &mut self,
        plan: &Plan,
        econ: &EconomyPath,
        t: usize,
        i: usize,
        price: f64,
    ) -> f64 {
        let model = &plan.props[i].model;
        let rate = self.props[i].mortgage.map_or(0.0, |m| m.rate);
        if !model.has_ext() {
            return rate;
        }
        let jump = model.prepare(
            &mut self.props[i],
            econ.tbill[t],
            &mut self.hx.rng_maint[i],
            &mut self.hx.rng_ins[i],
        );
        let now = plan.start + t as i32;
        let repair = self.props[i].ext.repair;
        if repair > 0.0 {
            self.event(now, TraceEventKind::LifeEvent, Some(repair / price), || {
                format!("Major repair on {}", plan.props[i].name)
            });
        }
        if jump > 0.0 {
            self.event(now, TraceEventKind::LifeEvent, None, || {
                format!(
                    "Insurance repriced: premiums up {:.0}%",
                    (math::exp(jump) - 1.0) * 100.0
                )
            });
        }
        rate
    }

    /// After a property's monthly step: logs an ARM reset.
    pub(crate) fn hx_after_step(&mut self, plan: &Plan, t: usize, i: usize, rate_before: f64) {
        if !plan.props[i].model.has_arm() {
            return;
        }
        let Some(m) = self.props[i].mortgage else {
            return;
        };
        if rate_before > 0.0 && (m.rate - rate_before).abs() > 1e-12 {
            let now = plan.start + t as i32;
            self.event(now, TraceEventKind::LifeEvent, None, || {
                format!(
                    "{}: adjustable rate reset from {:.2}% to {:.2}%, payment {:.0} a month",
                    plan.props[i].name,
                    rate_before * 100.0,
                    m.rate * 100.0,
                    m.payment
                )
            });
        }
    }

    /// Lists the sale of the property in `slot` if sales are slow. Returns true if
    /// it was listed (the caller must not sell now). A property already listed
    /// counts as handled.
    pub(crate) fn hx_list_sale(
        &mut self,
        plan: &Plan,
        t: usize,
        slot: usize,
        cost_fraction: f64,
        then_rent: f64,
        forced: bool,
    ) -> bool {
        if !plan.hx.liquidity {
            return false;
        }
        if self.hx.pending[slot].is_some() {
            return true;
        }
        let w = self.hx.liq_words[slot];
        let (months, discount) = sale_terms(self.hx.stress, unit(w[0]), unit(w[1]));
        self.hx.pending[slot] = Some(PendingSale {
            close_month: t as i32 + months as i32,
            cost_fraction,
            then_rent,
            discount,
            list_value: self.props[slot].value,
            forced,
        });
        let now = plan.start + t as i32;
        self.event(now, TraceEventKind::LifeEvent, None, || {
            format!(
                "Listed {} for sale: expected to close in {} months at {:.1}% below market",
                plan.props[slot].name,
                months,
                discount * 100.0
            )
        });
        true
    }

    /// True if the cash shortfall is covered by what home sales listed as a last
    /// resort will bring at closing: the household borrows against them until then
    /// (an overdraft, reported as debt), so it is neither cut nor depleted meanwhile.
    pub(crate) fn hx_bridged(&self, plan: &Plan) -> bool {
        if !plan.hx.liquidity {
            return false;
        }
        let mut expected = 0.0;
        for (slot, p) in self.hx.pending.iter().enumerate() {
            if let Some(p) = p {
                if p.forced {
                    let net = p.list_value * (1.0 - p.discount) * (1.0 - p.cost_fraction)
                        - self.props[slot].mortgage_balance();
                    expected += net.max(0.0);
                }
            }
        }
        expected > 0.0 && -self.acc.cash <= expected
    }

    /// Closes the sales whose time has come. Call after the month's housing costs.
    pub(crate) fn hx_close_sales(&mut self, plan: &Plan, price: f64, t: usize) {
        for slot in 0..self.hx.pending.len() {
            let Some(p) = self.hx.pending[slot] else {
                continue;
            };
            if (t as i32) < p.close_month {
                continue;
            }
            self.hx.pending[slot] = None;
            if !self.props[slot].owned {
                continue;
            }
            // The price was agreed at listing, less the discount of a weak market.
            let value = self.props[slot].value;
            let factor = if value > 0.0 {
                p.list_value / value * (1.0 - p.discount)
            } else {
                1.0
            };
            let net =
                self.sell_slot_discounted(plan, t, slot, p.cost_fraction, p.then_rent, factor);
            if p.forced && self.home_sale_month == i32::MAX {
                self.home_sale_month = t as i32;
            }
            let now = plan.start + t as i32;
            self.event(now, TraceEventKind::SoldHome, Some(net / price), || {
                format!("Sale of {} closed", plan.props[slot].name)
            });
        }
    }
}

#[cfg(test)]
#[path = "housing_ext_tests.rs"]
mod tests;
