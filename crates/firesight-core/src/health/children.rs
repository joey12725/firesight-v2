//! Children on the household's health plan (`spending.healthcare.cover_children`).
//!
//! A child has no coverage phase of their own: they are on whatever plan the parents
//! are on this month. The prices come from the same tables as the adults':
//!
//! * **Employer plan**: the worker pays the family tier instead of the single tier,
//!   once per household however many children (KFF Employer Health Benefits Survey
//!   2026: $7,471 against $1,765 a year, the two worker contributions in
//!   `params.json` `healthcare.employer_phase`). The increment is
//!   `family - single`, charged once while any child is covered.
//! * **COBRA**: the full price of the family plan over the single plan, plus the 2%
//!   load ($28,303 against $9,804 a year, same table).
//! * **ACA marketplace**: the benchmark premium at the child's age on the CMS age
//!   curve (0.765 of the age-21 price up to 14, rising to 1.0 at 21) times the tier
//!   factor, per child; the rating rule charges at most three children under 21.
//!
//! Out-of-pocket costs are the mean of the age band (MEPS, `params.json`
//! `out_of_pocket_by_age_pre_65`: $400 a year under 18, $800 from 18) for the plan.
//! They are expected values: children draw no random numbers, so the health streams
//! of the adults are untouched.

use super::{CoveragePhase, HealthModel, COBRA_PREMIUM_LOAD};

/// ACA rating rule: premiums are charged for at most this many children under 21
/// (45 CFR 147.102(c)(1)).
pub const ACA_MAX_CHILDREN_CHARGED: usize = 3;

/// Age under which the three-child rating rule counts a child.
pub const ACA_CHILD_RATING_AGE: f64 = 21.0;

/// Monthly increments, today's dollars before excess medical inflation.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct ChildCoverage {
    /// Family-tier worker contribution less single-tier, per month.
    employer_increment: f64,
    /// Full family plan cost less single plan cost, with the COBRA load, per month.
    cobra_increment: f64,
}

impl ChildCoverage {
    pub(super) fn new(employer: &serde_json::Value) -> Self {
        let n = |k: &str| {
            employer[k]
                .as_f64()
                .unwrap_or_else(|| panic!("embedded data: missing number at {k}"))
        };
        ChildCoverage {
            employer_increment: (n("worker_contribution_family") - n("worker_contribution_single"))
                / 12.0,
            cobra_increment: (n("average_total_premium_family")
                - n("average_total_premium_single"))
                * COBRA_PREMIUM_LOAD
                / 12.0,
        }
    }
}

/// What one covered child costs this month, nominal dollars.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChildMonth {
    /// Marketplace premium (gross, before the credit), for a child on an ACA plan.
    pub aca_premium: f64,
    /// Benchmark premium at the child's age, for the premium credit.
    pub aca_benchmark: f64,
    /// Out-of-pocket costs.
    pub out_of_pocket: f64,
}

impl HealthModel {
    /// Household-level increment per month of moving from single to family coverage
    /// while at least one child is covered, nominal dollars, `phase` Employer or
    /// COBRA (zero otherwise). `growth` is `price level * excess medical growth`.
    pub fn child_family_increment(&self, phase: CoveragePhase, growth: f64) -> f64 {
        if !self.modeled {
            return 0.0;
        }
        growth
            * match phase {
                CoveragePhase::Employer => self.child.employer_increment,
                CoveragePhase::Cobra => self.child.cobra_increment,
                _ => 0.0,
            }
    }

    /// One child's per-child costs at `age` (years) in `phase`, nominal. `growth` is
    /// `price level * excess medical growth`. Employer and COBRA phases carry no
    /// per-child premium (the family tier is charged by
    /// [`HealthModel::child_family_increment`]); ACA adds the age-curve premium.
    pub fn child_month(&self, age: f64, phase: CoveragePhase, growth: f64) -> ChildMonth {
        if !self.modeled {
            return ChildMonth::default();
        }
        let (_, oop, _) = self.price(age, phase);
        let mut m = ChildMonth {
            out_of_pocket: oop.mean / 12.0 * growth,
            ..ChildMonth::default()
        };
        if phase == CoveragePhase::Aca {
            m.aca_premium = self.aca_benchmark_monthly(age) * self.tier_premium_factor * growth;
            m.aca_benchmark = self.aca_benchmark_monthly(age) * growth;
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;
    use crate::model::Healthcare;

    fn model() -> HealthModel {
        HealthModel::new(&Healthcare::default(), None, false, Data::embedded())
    }

    #[test]
    fn family_tier_increments_are_hand_computed() {
        let m = model();
        // (7471 - 1765) / 12
        let e = m.child_family_increment(CoveragePhase::Employer, 1.0);
        assert!((e - 5706.0 / 12.0).abs() < 1e-9, "{e}");
        // (28303 - 9804) * 1.02 / 12
        let c = m.child_family_increment(CoveragePhase::Cobra, 1.0);
        assert!((c - 18499.0 * 1.02 / 12.0).abs() < 1e-9, "{c}");
        assert_eq!(m.child_family_increment(CoveragePhase::Aca, 1.0), 0.0);
        assert!((m.child_family_increment(CoveragePhase::Employer, 2.0) - 2.0 * e).abs() < 1e-9);
    }

    #[test]
    fn aca_child_is_the_age_curve_share_of_the_benchmark() {
        let m = model();
        let kid = m.child_month(8.0, CoveragePhase::Aca, 1.0);
        let adult = m.child_month(21.0, CoveragePhase::Aca, 1.0);
        // Curve: 0.765 under 15, 1.0 at 21.
        assert!((kid.aca_premium / adult.aca_premium - 0.765).abs() < 1e-9);
        assert_eq!(
            kid.aca_premium, kid.aca_benchmark,
            "silver is the benchmark"
        );
        // Out of pocket: $400 a year under 18, $800 from 18 (silver, expected value).
        assert!((kid.out_of_pocket - 400.0 / 12.0).abs() < 1e-9);
        assert!((adult.out_of_pocket - 800.0 / 12.0).abs() < 1e-9);
    }

    #[test]
    fn employer_child_has_only_out_of_pocket() {
        let m = model().child_month(5.0, CoveragePhase::Employer, 1.0);
        assert_eq!(m.aca_premium, 0.0);
        assert!((m.out_of_pocket - 400.0 / 12.0).abs() < 1e-9);
    }

    #[test]
    fn in_budget_mode_charges_nothing() {
        let hc = Healthcare {
            mode: crate::model::HealthcareMode::InBudget,
            ..Healthcare::default()
        };
        let m = HealthModel::new(&hc, None, false, Data::embedded());
        assert_eq!(
            m.child_month(5.0, CoveragePhase::Aca, 1.0),
            ChildMonth::default()
        );
        assert_eq!(m.child_family_increment(CoveragePhase::Employer, 1.0), 0.0);
    }
}
