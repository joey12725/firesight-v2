//! Policy risk: one path's draw of future law changes.
//!
//! `assumptions.taxes.policy_risk` turns two policy questions from a switch into a
//! distribution. On each path, with the stated probabilities, Social Security
//! benefits are cut from a random year at or after the trust-fund date, and ordinary
//! income tax rates rise from a random year. The draw is made once per path from its
//! own random stream ([`Stream::PolicyRisk`]) and always consumes the same four
//! uniforms, so it does not depend on anything the household decides and does not
//! disturb any other stream. The household does not foresee the change: its
//! planning (funded test, withholding estimates) uses the law as it stands when
//! each year starts, so a cut is a surprise. Once a cut has happened the funded
//! test counts the reduced benefit (`sim/trigger.rs`).
//!
//! The cut multiplies the benefit the schedule pays (on top of any deterministic
//! `social_security_haircut`). The tax increase adds points to every federal
//! ordinary-income bracket rate; capital gains, payroll tax and state tax are
//! unchanged.

use crate::benefits::MonthlyBenefits;
use crate::model::PolicyRisk;
use crate::rng::{Rng, Stream};
use crate::tax::TaxYearParams;
use crate::MAX_PEOPLE;

/// The year a change begins is drawn uniformly from this many years starting at
/// `earliest_year`. A judgement: Congress has historically acted close to a
/// depletion date (the 1983 amendments came months before the fund would have run
/// short), so most of the mass is in the first decade after it; fifteen years keeps
/// a tail. Registered in `data/actuarial/params.json` under `unverified_from_memory`.
pub const POLICY_WINDOW_YEARS: i32 = 15;

/// Default `ss_haircut_probability`: a judgement (even odds that the shortfall is
/// not legislated away before benefits are cut). Registered in `unverified_from_memory`.
pub const DEFAULT_SS_HAIRCUT_PROBABILITY: f64 = 0.5;

/// Default `ss_haircut_factor`: 77% of scheduled benefits are payable from the year
/// the OASI trust fund is depleted, 2033 (2025 Social Security Trustees Report,
/// summary table; the combined OASDI figure is 81% in 2034).
pub const DEFAULT_SS_HAIRCUT_FACTOR: f64 = 0.77;

/// Default `earliest_year`: the 2025 Trustees Report's OASI depletion year.
pub const DEFAULT_EARLIEST_YEAR: i32 = 2033;

/// Default `tax_increase_probability`: a judgement. Registered in
/// `unverified_from_memory`.
pub const DEFAULT_TAX_INCREASE_PROBABILITY: f64 = 0.25;

/// Default `tax_increase_points`: two points on every ordinary bracket, a judgement
/// (CBO's long-term budget outlooks show debt rising faster than output under
/// current law, so some revenue increase is plausible; its size is not forecast
/// here). Registered in `unverified_from_memory`.
pub const DEFAULT_TAX_INCREASE_POINTS: f64 = 0.02;

/// One path's policy outcome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PolicyDraw {
    /// First calendar year of reduced benefits (`i32::MAX` when none).
    pub ss_from_year: i32,
    /// Share of benefits paid from then on.
    pub ss_factor: f64,
    /// First calendar year of higher ordinary rates (`i32::MAX` when none).
    pub tax_from_year: i32,
    /// Points added to every ordinary bracket rate from then on.
    pub tax_points: f64,
}

impl PolicyDraw {
    /// No change in policy.
    pub const NONE: PolicyDraw = PolicyDraw {
        ss_from_year: i32::MAX,
        ss_factor: 1.0,
        tax_from_year: i32::MAX,
        tax_points: 0.0,
    };

    /// Draws the policy outcome of path `path`: four uniforms from the path's
    /// policy stream, whatever the settings.
    pub fn draw(risk: &PolicyRisk, seed: u64, path: u64) -> PolicyDraw {
        let mut rng = Rng::stream(seed, path, Stream::PolicyRisk);
        let (cut, cut_when, raise, raise_when) =
            (rng.uniform(), rng.uniform(), rng.uniform(), rng.uniform());
        let year = |u: f64| risk.earliest_year + (u * f64::from(POLICY_WINDOW_YEARS)) as i32;
        let mut out = PolicyDraw::NONE;
        if cut < risk.ss_haircut_probability {
            out.ss_from_year = year(cut_when);
            out.ss_factor = risk.ss_haircut_factor.clamp(0.0, 1.0);
        }
        if raise < risk.tax_increase_probability {
            out.tax_from_year = year(raise_when);
            out.tax_points = risk.tax_increase_points;
        }
        out
    }

    /// Share of scheduled benefits paid in `year`.
    #[inline]
    pub fn ss_scale(&self, year: i32) -> f64 {
        if year >= self.ss_from_year {
            self.ss_factor
        } else {
            1.0
        }
    }

    /// Benefits for a month of `year` after this path's cut, if it has begun. The
    /// earnings-test share withheld is a ratio and does not change.
    #[inline]
    pub fn scale_benefits(&self, year: i32, mut b: MonthlyBenefits) -> MonthlyBenefits {
        let f = self.ss_scale(year);
        if f != 1.0 {
            for i in 0..MAX_PEOPLE {
                b.paid[i] *= f;
                b.withheld[i] *= f;
            }
        }
        b
    }

    /// Adds this path's rate increase to the federal ordinary brackets of the
    /// filing statuses in the bit mask `statuses`, when it has begun by `year`.
    #[inline]
    pub fn raise_rates(&self, year: i32, params: &mut TaxYearParams, statuses: u8) {
        if year < self.tax_from_year || self.tax_points == 0.0 {
            return;
        }
        for (s, schedule) in params.federal.ordinary.iter_mut().enumerate() {
            if statuses & (1 << s) != 0 {
                for rate in &mut schedule.rate[..schedule.len] {
                    *rate += self.tax_points;
                }
            }
        }
    }

    /// True when anything changes on this path.
    pub fn any(&self) -> bool {
        self.ss_from_year != i32::MAX || self.tax_from_year != i32::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tax::{Brackets, FederalYear, FilingStatus};

    fn risk(ss_p: f64, tax_p: f64) -> PolicyRisk {
        PolicyRisk {
            ss_haircut_probability: ss_p,
            ss_haircut_factor: 0.77,
            earliest_year: 2033,
            tax_increase_probability: tax_p,
            tax_increase_points: 0.02,
        }
    }

    #[test]
    fn judgement_constants_are_registered_as_unverified() {
        let section = &crate::data::Data::embedded().actuarial["unverified_from_memory"]
            ["policy_risk_defaults"];
        let num = |key: &str| section[key].as_f64().unwrap();
        assert_eq!(f64::from(POLICY_WINDOW_YEARS), num("window_years"));
        assert_eq!(
            DEFAULT_SS_HAIRCUT_PROBABILITY,
            num("ss_haircut_probability")
        );
        assert_eq!(
            DEFAULT_TAX_INCREASE_PROBABILITY,
            num("tax_increase_probability")
        );
        assert_eq!(DEFAULT_TAX_INCREASE_POINTS, num("tax_increase_points"));
    }

    #[test]
    fn certain_and_impossible_probabilities() {
        for path in 0..50 {
            let none = PolicyDraw::draw(&risk(0.0, 0.0), 1, path);
            assert_eq!(none, PolicyDraw::NONE);
            let all = PolicyDraw::draw(&risk(1.0, 1.0), 1, path);
            assert!((2033..2033 + POLICY_WINDOW_YEARS).contains(&all.ss_from_year));
            assert!((2033..2033 + POLICY_WINDOW_YEARS).contains(&all.tax_from_year));
            assert_eq!(all.ss_factor, 0.77);
            assert_eq!(all.tax_points, 0.02);
        }
    }

    #[test]
    fn frequencies_match_the_probabilities() {
        let r = risk(0.3, 0.6);
        let n = 20_000;
        let (mut cuts, mut raises) = (0, 0);
        let mut years = [0u32; POLICY_WINDOW_YEARS as usize];
        for path in 0..n {
            let d = PolicyDraw::draw(&r, 9, path);
            cuts += u32::from(d.ss_from_year != i32::MAX);
            raises += u32::from(d.tax_from_year != i32::MAX);
            if d.ss_from_year != i32::MAX {
                years[(d.ss_from_year - 2033) as usize] += 1;
            }
        }
        let n = n as f64;
        // Binomial standard errors: 0.0032 and 0.0035.
        assert!((f64::from(cuts) / n - 0.3).abs() < 0.015);
        assert!((f64::from(raises) / n - 0.6).abs() < 0.015);
        // The year is uniform over the window: each of 15 years holds about 1/15.
        let total: u32 = years.iter().sum();
        for count in years {
            let share = f64::from(count) / f64::from(total);
            assert!((share - 1.0 / 15.0).abs() < 0.015, "{years:?}");
        }
    }

    #[test]
    fn the_draw_does_not_depend_on_the_settings_that_follow_it() {
        // The same uniforms decide the cut whatever the factor, and the year of the
        // tax rise whatever its probability (pairing between scenarios).
        let a = PolicyDraw::draw(&risk(0.5, 0.5), 3, 17);
        let mut other = risk(0.5, 0.9);
        other.ss_haircut_factor = 0.5;
        let b = PolicyDraw::draw(&other, 3, 17);
        assert_eq!(a.ss_from_year, b.ss_from_year);
        if a.tax_from_year != i32::MAX {
            assert_eq!(a.tax_from_year, b.tax_from_year);
        }
    }

    #[test]
    fn scale_and_raise() {
        let d = PolicyDraw {
            ss_from_year: 2035,
            ss_factor: 0.8,
            tax_from_year: 2040,
            tax_points: 0.03,
        };
        assert_eq!(d.ss_scale(2034), 1.0);
        assert_eq!(d.ss_scale(2035), 0.8);
        let b = MonthlyBenefits {
            paid: [1000.0, 500.0],
            withheld: [100.0, 0.0],
            withheld_months: [0.1, 0.0],
        };
        let early = d.scale_benefits(2034, b);
        assert_eq!(early, b);
        let late = d.scale_benefits(2036, b);
        assert_eq!(late.paid, [800.0, 400.0]);
        assert_eq!(late.withheld, [80.0, 0.0]);
        assert_eq!(late.withheld_months, [0.1, 0.0]);

        let mut p = TaxYearParams {
            federal: FederalYear {
                ordinary: [Brackets::from_pairs(&[(0.0, 0.10), (10_000.0, 0.12)]); 3],
                ..FederalYear::default()
            },
            ..TaxYearParams::default()
        };
        d.raise_rates(2039, &mut p, 0b111);
        assert_eq!(p.federal.ordinary[0].rate[..2], [0.10, 0.12]);
        // Only the statuses in the mask, and only the brackets in use.
        d.raise_rates(2040, &mut p, 1 << FilingStatus::MarriedJoint.index());
        assert_eq!(p.federal.ordinary[1].rate[..2], [0.13, 0.15]);
        assert_eq!(p.federal.ordinary[1].rate[2], 0.0);
        assert_eq!(p.federal.ordinary[0].rate[..2], [0.10, 0.12]);
        // The raised schedule taxes more: 10,000 x 0.13 + 5,000 x 0.15 = 2,050.
        assert!((p.federal.ordinary[1].tax(15_000.0) - 2_050.0).abs() < 1e-9);
    }
}
