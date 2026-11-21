//! Long-term-care insurance (`person.ltc_insurance`).
//!
//! A policy charges a level premium (in dollars of the plan's start month, fixed in
//! nominal terms as real policies are) until the owner's care episode begins, when
//! premiums are waived. Once care has lasted the elimination period, it pays the
//! cost of care up to the daily benefit (times 365/12 a month) until the benefit
//! pool (daily benefit times 365 times the benefit years) is used up. An inflation
//! rider compounds the daily benefit and what is left of the pool every year.
//!
//! **Rate increases.** Premiums of a policy class are not guaranteed: with
//! `premium_increase_risk` on, each policy year the class receives an approved
//! increase with probability [`RATE_INCREASE_PROBABILITY`], of a size uniform
//! between [`RATE_INCREASE_MIN`] and [`RATE_INCREASE_MAX`] (one class for the
//! household: both partners' policies rise together). Judgements calibrated to the
//! industry's record: an average cumulative increase of 31% since 2001 on policies
//! sold under the rate-stability rules, 55% on older ones (LTC Shop data, reported
//! by InvestmentNews, 13 September 2018). Draws come
//! from the new `Stream::LtcPremiumRates`, two a year whatever the household does.
//!
//! **Lapse.** At a policy anniversary the policy lapses (no further premiums, no
//! benefits) if the household could not pay its essentials at any time in the
//! past year, or if the year's premium is above [`LAPSE_INCOME_SHARE`] of the
//! household's resources: its wage-like income over the past year (wages,
//! self-employment, Social Security, taxable pensions) plus [`LAPSE_ASSET_DRAW`]
//! of its investable assets (both judgements of "unaffordable"). There is no
//! non-forfeiture benefit and no reduced-benefit landing spot.
//!
//! Benefits of a tax-qualified policy are not taxable income (IRC 7702B) and are
//! treated as such; premiums are not deducted (the medical-expense deduction is not
//! modelled).

use crate::math;
use crate::model::{InflationRider, LtcInsurance};
use crate::rng::Rng;

/// Annual probability that the policy class gets an approved premium increase. A
/// judgement (see the module documentation).
pub const RATE_INCREASE_PROBABILITY: f64 = 0.08;

/// Smallest approved increase, as a fraction. A judgement.
pub const RATE_INCREASE_MIN: f64 = 0.1;

/// Largest approved increase, as a fraction. A judgement.
pub const RATE_INCREASE_MAX: f64 = 0.3;

/// The policy lapses at an anniversary when the year's premium is above this
/// share of the household's resources (see the module documentation). A judgement.
pub const LAPSE_INCOME_SHARE: f64 = 0.1;

/// Share of investable assets counted as a year's resources in the lapse test (a
/// sustainable draw). A judgement.
pub const LAPSE_ASSET_DRAW: f64 = 0.04;

/// Days in a month of benefits.
const DAYS_PER_MONTH: f64 = 365.0 / 12.0;

/// A compiled policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LtcPolicy {
    /// Premium per month at the start, nominal.
    pub monthly_premium: f64,
    /// Benefit cap per month at the start, nominal.
    pub monthly_benefit: f64,
    /// Benefit pool at the start, nominal.
    pub pool: f64,
    /// Months of paid care before benefits start.
    pub elimination_months: u32,
    /// Annual compound growth of the benefit and the remaining pool.
    pub rider_rate: f64,
    /// True when premiums can be raised.
    pub increase_risk: bool,
}

impl LtcPolicy {
    /// Compiles a policy from the profile.
    pub fn new(p: &LtcInsurance) -> LtcPolicy {
        LtcPolicy {
            monthly_premium: p.annual_premium.max(0.0) / 12.0,
            monthly_benefit: p.daily_benefit.max(0.0) * DAYS_PER_MONTH,
            pool: p.daily_benefit.max(0.0) * 365.0 * p.benefit_years.max(0.0),
            elimination_months: (p.elimination_days.max(0.0) / DAYS_PER_MONTH).ceil() as u32,
            rider_rate: match p.inflation_rider {
                InflationRider::None => 0.0,
                InflationRider::Compound { rate } => rate.max(0.0),
            },
            increase_risk: p.premium_increase_risk,
        }
    }

    /// `(1 + rider)^year`: growth of the benefit and the pool by policy year.
    #[inline]
    pub fn rider_factor(&self, year: usize) -> f64 {
        if self.rider_rate == 0.0 {
            1.0
        } else {
            math::powi(1.0 + self.rider_rate, year as i32)
        }
    }
}

/// Cumulative premium factor of the household's policy class by policy year:
/// entry `k` is the multiple of the original premium charged in year `k`
/// (`out[0] = 1`). Two uniforms per year, always.
pub fn rate_path(rng: &mut Rng, out: &mut [f64]) {
    let mut level = 1.0;
    for (k, slot) in out.iter_mut().enumerate() {
        let occurs = rng.uniform();
        let size = rng.uniform();
        if k > 0 && occurs < RATE_INCREASE_PROBABILITY {
            level *= 1.0 + RATE_INCREASE_MIN + (RATE_INCREASE_MAX - RATE_INCREASE_MIN) * size;
        }
        *slot = level;
    }
}

/// A policy on one path.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PolicyState {
    /// Premiums are being paid or benefits can be claimed.
    pub in_force: bool,
    /// The policy lapsed.
    pub lapsed: bool,
    /// Months of care so far in the episode.
    pub care_months: u32,
    /// Unused pool in dollars of the start (divide out the rider factor).
    pub pool_base: f64,
    /// Benefits paid so far, nominal.
    pub paid: f64,
}

/// What a policy does in one month.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PolicyMonth {
    /// Premium due, nominal.
    pub premium: f64,
    /// Benefit paid, nominal.
    pub benefit: f64,
    /// True in the month benefits begin.
    pub first_benefit: bool,
    /// True in the month the pool runs out.
    pub exhausted: bool,
}

impl PolicyState {
    /// A policy in force with its whole pool.
    pub fn start(policy: &LtcPolicy) -> PolicyState {
        PolicyState {
            in_force: true,
            lapsed: false,
            care_months: 0,
            pool_base: policy.pool,
            paid: 0.0,
        }
    }

    /// One month: `care_cost` is this month's cost of paid care (0 outside an
    /// episode), `year` the policy year and `rate` that year's premium factor.
    pub fn month(
        &mut self,
        policy: &LtcPolicy,
        care_cost: f64,
        year: usize,
        rate: f64,
    ) -> PolicyMonth {
        let mut out = PolicyMonth::default();
        if !self.in_force {
            return out;
        }
        if care_cost <= 0.0 && self.care_months == 0 {
            out.premium = policy.monthly_premium * if policy.increase_risk { rate } else { 1.0 };
            return out;
        }
        if care_cost > 0.0 {
            self.care_months += 1;
        }
        if self.care_months > policy.elimination_months && care_cost > 0.0 && self.pool_base > 0.0 {
            let growth = policy.rider_factor(year);
            let benefit = care_cost
                .min(policy.monthly_benefit * growth)
                .min(self.pool_base * growth);
            out.first_benefit = self.paid == 0.0 && benefit > 0.0;
            self.pool_base -= benefit / growth;
            if self.pool_base <= 1e-9 {
                self.pool_base = 0.0;
                out.exhausted = true;
            }
            self.paid += benefit;
            out.benefit = benefit;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Stream;

    fn policy() -> LtcPolicy {
        LtcPolicy::new(&LtcInsurance {
            annual_premium: 3_600.0,
            daily_benefit: 200.0,
            benefit_years: 2.0,
            elimination_days: 90.0,
            inflation_rider: InflationRider::None,
            premium_increase_risk: true,
        })
    }

    #[test]
    fn compiles_the_contract_terms() {
        let p = policy();
        assert_eq!(p.monthly_premium, 300.0);
        assert!((p.monthly_benefit - 200.0 * 365.0 / 12.0).abs() < 1e-9);
        assert_eq!(p.pool, 146_000.0);
        // 90 days is 2.96 months: benefits from the fourth month of care.
        assert_eq!(p.elimination_months, 3);
    }

    #[test]
    fn premiums_until_claim_then_benefits_until_the_pool_is_gone() {
        let p = policy();
        let mut s = PolicyState::start(&p);
        let m = s.month(&p, 0.0, 0, 1.0);
        assert_eq!((m.premium, m.benefit), (300.0, 0.0));
        // After an increase of 25% the premium is 375.
        assert_eq!(s.month(&p, 0.0, 3, 1.25).premium, 375.0);
        // Care at $9,000 a month: three months of elimination, no premium.
        for _ in 0..3 {
            let m = s.month(&p, 9_000.0, 4, 1.25);
            assert_eq!((m.premium, m.benefit), (0.0, 0.0));
        }
        // Then 6,083.33 a month until 146,000 is used: 24 full months.
        let cap = 200.0 * 365.0 / 12.0;
        let mut total = 0.0;
        let mut months = 0;
        loop {
            let m = s.month(&p, 9_000.0, 4, 1.25);
            if m.benefit == 0.0 {
                break;
            }
            assert!(m.benefit <= cap + 1e-9);
            total += m.benefit;
            months += 1;
            if m.exhausted {
                break;
            }
        }
        assert!((total - 146_000.0).abs() < 1e-6, "{total}");
        assert_eq!(months, 24);
        // Care cheaper than the cap is paid in full.
        let mut s = PolicyState::start(&p);
        for _ in 0..3 {
            s.month(&p, 4_000.0, 0, 1.0);
        }
        assert_eq!(s.month(&p, 4_000.0, 0, 1.0).benefit, 4_000.0);
    }

    #[test]
    fn the_inflation_rider_grows_benefit_and_pool() {
        let mut terms = LtcInsurance {
            annual_premium: 3_600.0,
            daily_benefit: 200.0,
            benefit_years: 2.0,
            elimination_days: 0.0,
            inflation_rider: InflationRider::Compound { rate: 0.03 },
            premium_increase_risk: false,
        };
        let p = LtcPolicy::new(&terms);
        let mut s = PolicyState::start(&p);
        // In policy year 20 the cap is 1.03^20 = 1.80611 times the original.
        let m = s.month(&p, 50_000.0, 20, 2.0);
        let cap = 200.0 * 365.0 / 12.0 * 1.806_111_234_669_6;
        assert!((m.benefit - cap).abs() < 1e-6, "{}", m.benefit);
        // Premium risk off: the class factor is ignored.
        terms.inflation_rider = InflationRider::None;
        let p = LtcPolicy::new(&terms);
        assert_eq!(PolicyState::start(&p).month(&p, 0.0, 5, 2.0).premium, 300.0);
    }

    #[test]
    fn rate_increases_match_the_calibration() {
        // Over 17 policy years a policy sees about 28% of increases (the post-2001
        // record is 31%): E[ln factor] = 17 x 0.08 x E[ln(1 + U(0.1, 0.3))]
        // = 1.36 x 0.18116 = 0.2464, a factor of 1.28 at the mean of the log.
        let n = 20_000;
        let mut logs = 0.0;
        let mut out = [0.0; 18];
        for path in 0..n {
            let mut rng = Rng::stream(1, path, Stream::LtcPremiumRates);
            rate_path(&mut rng, &mut out);
            assert_eq!(out[0], 1.0);
            assert!(out.windows(2).all(|w| w[1] >= w[0]));
            logs += math::ln(out[17]);
        }
        let mean = logs / n as f64;
        assert!((mean - 0.2464).abs() < 0.01, "{mean}");
    }
}
