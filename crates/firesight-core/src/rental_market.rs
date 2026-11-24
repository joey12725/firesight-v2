//! Realistic rental dynamics (`RentalTerms::dynamics: realistic`): tenancies that end
//! at random, vacancies that last months (longer in recessions), a market rent index
//! that wanders around the rent's trend, new leases and renewals at the market rent,
//! occasional evictions, and maintenance that comes partly as large repairs.
//!
//! Responsibility: one rental's month, given its draws. `housing.rs` applies the
//! outcome to the property's flows; the simulation draws from the rental's own
//! stream (`Stream::RentalMarket(slot)`), exactly [`DRAWS_PER_MONTH`] values a month
//! whether the slot is owned, let or empty.
//!
//! ```text
//! tenancy        ends each let month with p_leave = annual_turnover / 12 (turnover
//!                as landlords report it: move-outs per unit and year)
//! vacancy        mean spell M = v / (1 - v) / p_leave months for an entered vacancy v,
//!                so that the long-run vacant share is v; in a recession spells are
//!                RECESSION_SPELL_FACTOR times longer, normal spells shortened so that
//!                the average over a RECESSION_SHARE of months in recession is M.
//!                M >= 1: the unit stands empty and is re-let each month with
//!                probability 1 / M; M < 1: it is re-let within the month and loses
//!                that fraction of a month's rent
//! market rent    trend rent x exp(x - S^2/2), x(t+1) = phi x(t) + S sqrt(1 - phi^2) z
//! lease rent     set to the market rent at each new lease and each 12-month renewal
//!                (the entered rent is the current lease, with 12 months to run)
//! eviction       a share EVICTION_ANNUAL / annual_turnover of move-outs, costing
//!                EVICTION_COST_MONTHS of rent (legal fees, rent lost, damage)
//! repairs        CAPEX_SHARE of the maintenance rate comes as lumps, each month with
//!                probability CAPEX_MONTHLY_PROBABILITY, of the size that keeps the
//!                expected maintenance unchanged
//! ```
//!
//! Sources and judgements are in `data/actuarial/params.json`,
//! `housing_decisions.rental_dynamics`.

use crate::math;
use crate::rng::{normal_from_bits, Rng};

/// Values drawn from the rental's stream every month.
pub const DRAWS_PER_MONTH: usize = 5;
/// Spells are this many times longer in a recession. A judgement: the rental vacancy
/// rate rose by about a tenth to a quarter in 2008-09 (Census HVS), and longer spells
/// rather than more move-outs carried it.
pub const RECESSION_SPELL_FACTOR: f64 = 1.5;
/// Share of months in recession used to keep the average spell at the entered
/// vacancy. A judgement close to the postwar US record.
pub const RECESSION_SHARE: f64 = 0.13;
/// Monthly persistence of the market rent deviation: 0.8 a year. A judgement.
pub const RENT_INDEX_ANNUAL_PERSISTENCE: f64 = 0.8;
/// Long-run standard deviation of market rent around its trend (log). A judgement
/// informed by the Zillow Observed Rent Index, whose yearly growth ran from about
/// zero in 2020 to 17.2% in February 2022.
pub const RENT_INDEX_SD: f64 = 0.05;
/// Evictions per tenancy-year. A judgement: the Eviction Lab counts about 6 renter
/// households in 100 threatened with eviction a year (2018) and a filing rate near
/// 8%; fewer end with a judgement and a lost tenant.
pub const EVICTION_ANNUAL: f64 = 0.02;
/// Cost of an eviction in months of rent. A judgement.
pub const EVICTION_COST_MONTHS: f64 = 3.0;
/// Share of maintenance that comes as large repairs. A judgement.
pub const CAPEX_SHARE: f64 = 0.5;
/// Monthly probability of a large repair (one every five years on average). A
/// judgement.
pub const CAPEX_MONTHLY_PROBABILITY: f64 = 1.0 / 60.0;
/// Months between rent resets for a sitting tenant.
pub const LEASE_MONTHS: u32 = 12;

/// Compiled dynamics of one rental.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RentalMarketParams {
    /// Monthly probability a tenancy ends.
    pub p_leave: f64,
    /// Mean vacancy spell in months outside and inside recessions.
    pub spell_normal: f64,
    pub spell_recession: f64,
    /// Share of move-outs that are evictions.
    pub eviction_share: f64,
    /// AR(1) coefficient and innovation sd of the market rent deviation.
    pub phi: f64,
    pub sigma: f64,
}

impl RentalMarketParams {
    /// From the entered vacancy share and annual turnover.
    pub fn new(vacancy: f64, annual_turnover: f64) -> RentalMarketParams {
        let turnover = annual_turnover.clamp(1e-6, 1.0);
        let p_leave = turnover / 12.0;
        let v = vacancy.clamp(0.0, 0.999);
        let spell = v / (1.0 - v) / p_leave;
        let k = RECESSION_SPELL_FACTOR;
        let spell_normal = spell / ((1.0 - RECESSION_SHARE) + RECESSION_SHARE * k);
        let phi = math::pow(RENT_INDEX_ANNUAL_PERSISTENCE, 1.0 / 12.0);
        RentalMarketParams {
            p_leave,
            spell_normal,
            spell_recession: spell_normal * k,
            eviction_share: (EVICTION_ANNUAL / turnover).min(1.0),
            phi,
            sigma: RENT_INDEX_SD * (1.0 - phi * phi).sqrt(),
        }
    }

    /// Market rent relative to trend for deviation `x` (mean one at stationarity).
    pub fn market_factor(&self, x: f64) -> f64 {
        math::exp(x - 0.5 * RENT_INDEX_SD * RENT_INDEX_SD)
    }
}

/// State of one rental's market on a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RentalMarketState {
    /// True while the unit stands empty.
    pub vacant: bool,
    /// Rent of the current lease relative to the trend rent.
    pub lease_factor: f64,
    /// Months since the lease started or was renewed.
    pub lease_age: u32,
    /// Log deviation of the market rent from trend.
    pub x: f64,
}

impl Default for RentalMarketState {
    fn default() -> Self {
        RentalMarketState {
            vacant: false,
            lease_factor: 1.0,
            lease_age: 0,
            x: 0.0,
        }
    }
}

/// One month's draws.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RentalDraws {
    pub leave: f64,
    pub fill: f64,
    pub evict: f64,
    pub repair: f64,
    pub z: f64,
}

impl RentalDraws {
    /// Draws [`DRAWS_PER_MONTH`] values from `rng`.
    pub fn draw(rng: &mut Rng) -> RentalDraws {
        RentalDraws {
            leave: rng.uniform(),
            fill: rng.uniform(),
            evict: rng.uniform(),
            repair: rng.uniform(),
            z: normal_from_bits(rng.next_u64()),
        }
    }
}

/// What happened to a rental this month.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RentalOutcome {
    /// Rent of the lease in force relative to trend.
    pub rent_factor: f64,
    /// Share of the month's rent lost to vacancy (0, a fraction, or 1).
    pub vacant_share: f64,
    /// Eviction cost in months of trend rent.
    pub eviction_months: f64,
    /// True if a large repair falls due.
    pub repair: bool,
    /// True if a tenancy ended this month.
    pub moved_out: bool,
}

/// Advances one rental by a month. `owned` false only moves the market index.
pub fn month(
    p: &RentalMarketParams,
    st: &mut RentalMarketState,
    d: &RentalDraws,
    recession: bool,
    owned: bool,
) -> RentalOutcome {
    let x = st.x;
    st.x = p.phi * x + p.sigma * d.z;
    let mut out = RentalOutcome {
        rent_factor: st.lease_factor,
        ..RentalOutcome::default()
    };
    if !owned {
        return out;
    }
    out.repair = d.repair < CAPEX_MONTHLY_PROBABILITY;
    let spell = if recession {
        p.spell_recession
    } else {
        p.spell_normal
    };
    let market = p.market_factor(x);
    if st.vacant {
        out.vacant_share = 1.0;
        if d.fill * spell < 1.0 {
            // Re-let from next month at the market rent.
            st.vacant = false;
            st.lease_factor = market;
            st.lease_age = 0;
        }
        return out;
    }
    st.lease_age += 1;
    if d.leave < p.p_leave {
        out.moved_out = true;
        if d.evict < p.eviction_share {
            out.eviction_months = EVICTION_COST_MONTHS;
        }
        if spell < 1.0 {
            // Re-let within the month: that fraction of the rent is lost.
            out.vacant_share = spell;
            st.lease_factor = market;
            st.lease_age = 0;
        } else {
            st.vacant = true;
        }
    } else if st.lease_age >= LEASE_MONTHS {
        st.lease_factor = market;
        st.lease_age = 0;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Stream;

    #[test]
    fn parameters_follow_from_turnover_and_vacancy() {
        let p = RentalMarketParams::new(0.05, 0.25);
        assert_eq!(p.p_leave, 0.25 / 12.0);
        // M = (0.05 / 0.95) / (0.25 / 12) = 2.526 months, split so that 87% of months
        // at M_n and 13% at 1.5 M_n average to M: M_n = M / 1.065.
        let m = 0.05 / 0.95 / p.p_leave;
        assert!((p.spell_normal - m / 1.065).abs() < 1e-12);
        assert!((p.spell_recession - 1.5 * p.spell_normal).abs() < 1e-12);
        assert!((p.eviction_share - 0.08).abs() < 1e-12);
        // Stationary sd of the index is RENT_INDEX_SD.
        assert!((p.sigma * p.sigma / (1.0 - p.phi * p.phi) - 0.0025).abs() < 1e-12);
    }

    #[test]
    fn long_run_vacancy_rent_and_costs_match_their_targets() {
        let p = RentalMarketParams::new(0.08, 0.30);
        let mut st = RentalMarketState::default();
        let mut rng = Rng::stream(7, 0, Stream::RentalMarket(0));
        let n = 2_000_000;
        let (mut vac, mut rent, mut evictions, mut repairs, mut moves) = (0.0, 0.0, 0.0, 0, 0);
        for t in 0..n {
            let d = RentalDraws::draw(&mut rng);
            // Recessions in 13% of months, in runs of a year and a half.
            let recession = (t % 138) < 18;
            let o = month(&p, &mut st, &d, recession, true);
            vac += o.vacant_share;
            rent += o.rent_factor;
            evictions += o.eviction_months;
            repairs += usize::from(o.repair);
            moves += usize::from(o.moved_out);
        }
        let n = n as f64;
        let (v, r) = (vac / n, rent / n);
        eprintln!("vacancy {v:.4}, rent factor {r:.4}");
        assert!((v - 0.08).abs() < 0.004, "{v}");
        assert!((r - 1.0).abs() < 0.01, "{r}");
        // Turnover: 30% of tenancies a year, of occupied months.
        let per_year = moves as f64 / (n * (1.0 - v)) * 12.0;
        assert!((per_year - 0.30).abs() < 0.01, "{per_year}");
        // Evictions: 2% of tenancy-years at three months each.
        let ev = evictions / (n * (1.0 - v)) * 12.0;
        assert!((ev - 0.06).abs() < 0.006, "{ev}");
        assert!((repairs as f64 / n - 1.0 / 60.0).abs() < 0.001);
    }

    #[test]
    fn short_spells_lose_part_of_a_month_and_new_leases_start_at_market() {
        let p = RentalMarketParams::new(0.01, 0.25);
        assert!(p.spell_normal < 1.0);
        let mut st = RentalMarketState {
            x: 0.1,
            ..RentalMarketState::default()
        };
        let d = RentalDraws {
            leave: 0.0,
            fill: 0.5,
            evict: 0.99,
            repair: 0.5,
            z: 0.0,
        };
        let o = month(&p, &mut st, &d, false, true);
        assert!(o.moved_out && !st.vacant);
        assert!((o.vacant_share - p.spell_normal).abs() < 1e-12);
        assert_eq!(o.eviction_months, 0.0);
        assert!((st.lease_factor - p.market_factor(0.1)).abs() < 1e-12);
        // A sitting tenant renews at market after 12 months.
        let stay = RentalDraws { leave: 0.99, ..d };
        let mut st = RentalMarketState::default();
        for _ in 0..11 {
            month(&p, &mut st, &stay, false, true);
            assert_eq!(st.lease_factor, 1.0);
        }
        month(&p, &mut st, &stay, false, true);
        assert_ne!(st.lease_factor, 1.0);
        assert_eq!(st.lease_age, 0);
    }
}
