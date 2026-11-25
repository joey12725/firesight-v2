//! Self-employment cycles and business failure (`business_risk` on a self-employment
//! income).
//!
//! # What is modelled
//!
//! The base self-employment model (`career.rs`) already draws a log-normal profit
//! factor each January, half persistent (0.5) and correlated with the stock market.
//! With `business_risk` on, three things are added on top of it:
//!
//! * **A business regime**: a persistent standardised state `z` redrawn each January,
//!   `z <- PHI z + sqrt(1 - PHI^2) e` with [`REGIME_PERSISTENCE`] 0.7, which multiplies
//!   profit by `exp(sd z - sd^2 Var[z] / 2)` (unit mean). Good and bad years therefore
//!   come in runs.
//! * **Recession co-movement**: in every month the path's recession flag is set, profit
//!   is lower by [`RECESSION_PROFIT_DROP`] (typical 10%, high 20%).
//! * **Failure**: each month the business may fail, with a yearly hazard that depends on
//!   its age (see [`FAIL_HAZARD_BY_YEAR`]), 1.3 times higher in a recession
//!   ([`RECESSION_HAZARD_MULTIPLIER`]) and higher in a bad regime
//!   (`exp(-0.25 z)`). `high` multiplies the hazard by 1.5. When it fails profit stops
//!   for good. With a fallback wage the person finds paid work after a search spell and
//!   earns the wage until they retire.
//!
//! # Parameters and sources
//!
//! The failure hazards come from the BLS Business Employment Dynamics survival data
//! (establishments, private sector): 79.6% of the March 1994 cohort was still open after
//! one year (BLS, TED 2018), 20.4 points of the 2013 cohort closed in its first year
//! and 34.7% of that cohort was open after ten years (BLS, TED 2024). The yearly
//! hazards for years 2 to 5 and 6 to 10 are JUDGEMENTS fitted so that survival after
//! ten years is 34%; the two-year and five-year survival rates that BLS publishes in
//! its survival tables could not be retrieved. These are establishments, most of which
//! have employees; sole proprietorships fail more often, which `high` stands in for.
//! The regime size, persistence, recession drop and hazard sensitivities are
//! JUDGEMENTS.

use crate::math;
use crate::model::BusinessRisk;

/// Yearly failure probability in business years 1 to 5 (year 1 verified against BLS BED
/// 79.6% and 20.4 points; years 2 to 5 are JUDGEMENTS, see the module documentation).
pub const FAIL_HAZARD_BY_YEAR: [f64; 5] = [0.204, 0.15, 0.12, 0.10, 0.09];
/// Yearly failure probability from the sixth year on (JUDGEMENT, chosen so that ten-year
/// survival is about 34%, the BLS figure).
pub const FAIL_HAZARD_LATER: f64 = 0.07;
/// Multiplier on the failure hazard in a recession month (JUDGEMENT).
pub const RECESSION_HAZARD_MULTIPLIER: f64 = 1.3;
/// Failure hazard is multiplied by `exp(-REGIME_HAZARD_BETA z)` (JUDGEMENT).
pub const REGIME_HAZARD_BETA: f64 = 0.25;
/// Multiplier on the hazard for `high` (JUDGEMENT).
pub const HIGH_HAZARD_MULTIPLIER: f64 = 1.5;
/// Yearly persistence of the regime (JUDGEMENT).
pub const REGIME_PERSISTENCE: f64 = 0.7;
/// Log standard deviation of the regime effect on profit, typical and high (JUDGEMENT).
pub const REGIME_SD: [f64; 2] = [0.20, 0.35];
/// Fraction of profit lost in a recession month, typical and high (JUDGEMENT).
pub const RECESSION_PROFIT_DROP: [f64; 2] = [0.10, 0.20];
/// Largest yearly failure probability after the multipliers.
pub const MAX_ANNUAL_FAILURE: f64 = 0.95;

/// Compiled settings of one business.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusinessModel {
    regime_sd: f64,
    recession_drop: f64,
    hazard_multiplier: f64,
    /// Fallback wage in today's dollars a year, and months of search before it starts.
    pub fallback: Option<(f64, u32)>,
    /// Years the business had been running at the start of the profile.
    pub years_operating: f64,
}

/// Per-path state of one business.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BusinessState {
    /// Standardised regime.
    pub z: f64,
    /// Variance of `z` so far (for the unit-mean correction).
    pub z_var: f64,
    /// True once the business has failed.
    pub failed: bool,
    /// Months of search left after a failure.
    pub search_left: u32,
}

/// What one month of the business did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusinessMonth {
    /// Multiplier on this month's profit.
    pub profit_factor: f64,
    /// True if the business failed this month.
    pub failed: bool,
}

impl BusinessModel {
    /// Compiles a model; `None` for `Off`.
    pub fn new(
        level: BusinessRisk,
        fallback: Option<(f64, u32)>,
        years_operating: f64,
    ) -> Option<BusinessModel> {
        let i = match level {
            BusinessRisk::Off => return None,
            BusinessRisk::Typical => 0,
            BusinessRisk::High => 1,
        };
        Some(BusinessModel {
            regime_sd: REGIME_SD[i],
            recession_drop: RECESSION_PROFIT_DROP[i],
            hazard_multiplier: if i == 1 { HIGH_HAZARD_MULTIPLIER } else { 1.0 },
            fallback,
            years_operating,
        })
    }

    /// Yearly failure probability of a business of `age` years, before the cycle.
    pub fn annual_failure(&self, age: f64) -> f64 {
        let base = FAIL_HAZARD_BY_YEAR
            .get(age.max(0.0) as usize)
            .copied()
            .unwrap_or(FAIL_HAZARD_LATER);
        (base * self.hazard_multiplier).min(MAX_ANNUAL_FAILURE)
    }

    /// Monthly failure probability at the given state.
    pub fn monthly_failure(&self, st: &BusinessState, age: f64, recession: bool) -> f64 {
        let mut a = self.annual_failure(age) * math::exp(-REGIME_HAZARD_BETA * st.z);
        if recession {
            a *= RECESSION_HAZARD_MULTIPLIER;
        }
        let a = a.min(MAX_ANNUAL_FAILURE);
        1.0 - math::exp(math::ln(1.0 - a) / 12.0)
    }

    /// One month. `january` redraws the regime from `z_normal`; `u_fail` decides
    /// failure when `active` (the business is operating this month). Both variates are
    /// always drawn by the caller.
    #[allow(clippy::too_many_arguments)]
    pub fn step(
        &self,
        st: &mut BusinessState,
        age: f64,
        recession: bool,
        january: bool,
        active: bool,
        u_fail: f64,
        z_normal: f64,
    ) -> BusinessMonth {
        if january {
            let phi = REGIME_PERSISTENCE;
            let innovation = 1.0 - phi * phi;
            st.z = phi * st.z + innovation.sqrt() * z_normal;
            st.z_var = phi * phi * st.z_var + innovation;
        }
        let mut failed = false;
        if active && !st.failed && u_fail < self.monthly_failure(st, age, recession) {
            st.failed = true;
            st.search_left = self.fallback.map_or(0, |f| f.1);
            failed = true;
        }
        let mut f =
            math::exp(self.regime_sd * st.z - 0.5 * self.regime_sd * self.regime_sd * st.z_var);
        if recession {
            f *= 1.0 - self.recession_drop;
        }
        BusinessMonth {
            profit_factor: if st.failed { 0.0 } else { f },
            failed,
        }
    }

    /// After a failure: advances the search spell by a month and returns whether the
    /// fallback wage is paid this month.
    pub fn fallback_pays(&self, st: &mut BusinessState) -> bool {
        if !st.failed || self.fallback.is_none() {
            return false;
        }
        if st.search_left > 0 {
            st.search_left -= 1;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typical() -> BusinessModel {
        BusinessModel::new(BusinessRisk::Typical, Some((30_000.0, 4)), 0.0).unwrap()
    }

    #[test]
    fn off_has_no_model() {
        assert!(BusinessModel::new(BusinessRisk::Off, None, 0.0).is_none());
    }

    #[test]
    fn annual_failure_follows_the_table_by_age() {
        let m = typical();
        assert_eq!(m.annual_failure(0.0), 0.204);
        assert_eq!(m.annual_failure(2.9), 0.12);
        assert_eq!(m.annual_failure(7.0), 0.07);
        let h = BusinessModel::new(BusinessRisk::High, None, 0.0).unwrap();
        assert!((h.annual_failure(0.0) - 0.306).abs() < 1e-12);
        assert_eq!(h.annual_failure(0.0).min(1.0), 0.306);
    }

    #[test]
    fn table_reproduces_ten_year_survival() {
        let mut s = 1.0;
        for a in FAIL_HAZARD_BY_YEAR {
            s *= 1.0 - a;
        }
        assert!((s - 0.796 * 0.85 * 0.88 * 0.90 * 0.91).abs() < 1e-12);
        for _ in 5..10 {
            s *= 1.0 - FAIL_HAZARD_LATER;
        }
        // BLS BED: 34.7% of the 2013 cohort open after ten years.
        assert!((s - 0.347).abs() < 0.02, "{s}");
    }

    #[test]
    fn monthly_hazard_compounds_to_the_annual_one() {
        let m = typical();
        let st = BusinessState::default();
        let h = m.monthly_failure(&st, 0.0, false);
        assert!((math::powi(1.0 - h, 12) - 0.796).abs() < 1e-9);
        // Recession: annual 0.204 * 1.3 = 0.2652.
        let hr = m.monthly_failure(&st, 0.0, true);
        assert!((math::powi(1.0 - hr, 12) - (1.0 - 0.2652)).abs() < 1e-9);
        // Bad regime (z = -2): annual 0.204 * exp(0.5).
        let bad = BusinessState { z: -2.0, ..st };
        let a = 0.204 * math::exp(0.5);
        let hb = m.monthly_failure(&bad, 0.0, false);
        assert!((math::powi(1.0 - hb, 12) - (1.0 - a)).abs() < 1e-9);
    }

    #[test]
    fn failure_zeroes_profit_and_starts_the_search() {
        let m = typical();
        let mut st = BusinessState::default();
        let r = m.step(&mut st, 0.0, false, false, true, 0.0, 0.0);
        assert!(r.failed && r.profit_factor == 0.0 && st.failed && st.search_left == 4);
        // Four search months, then the wage.
        for _ in 0..4 {
            assert!(!m.fallback_pays(&mut st));
        }
        assert!(m.fallback_pays(&mut st));
        // An inactive business cannot fail.
        let mut st = BusinessState::default();
        assert!(!m.step(&mut st, 0.0, false, false, false, 0.0, 0.0).failed);
    }

    #[test]
    fn regime_is_persistent_and_recession_lowers_profit() {
        let m = typical();
        let mut st = BusinessState::default();
        // January with z_normal = 1: z = sqrt(0.51) = 0.714..., var = 0.51.
        let r = m.step(&mut st, 5.0, false, true, true, 1.0, 1.0);
        let z = 0.51_f64.sqrt();
        assert!((st.z - z).abs() < 1e-12);
        let expect = math::exp(0.2 * z - 0.5 * 0.04 * 0.51);
        assert!((r.profit_factor - expect).abs() < 1e-12);
        // Next month, a recession: the same regime, 10% lower.
        let r2 = m.step(&mut st, 5.0, true, false, true, 1.0, 5.0);
        assert!((r2.profit_factor - 0.9 * expect).abs() < 1e-12);
        // Next January: persistence 0.7 of the old state with a zero innovation.
        m.step(&mut st, 5.0, false, true, true, 1.0, 0.0);
        assert!((st.z - 0.7 * z).abs() < 1e-12);
    }
}
