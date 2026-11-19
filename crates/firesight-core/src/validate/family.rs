//! Checks for the family features: children on the health plan, uncertain births,
//! college cost uncertainty, adult-child support and parental leave.

use crate::model::*;

use super::Checker;

impl Checker<'_> {
    pub(super) fn family(&mut self) {
        let p = self.p;
        let hc = &p.spending.healthcare;
        self.r.range(
            "/spending/healthcare/children_coverage_end_age",
            "Children's coverage end age",
            hc.children_coverage_end_age,
            18.0,
            30.0,
        );
        if hc.cover_children && hc.mode == HealthcareMode::InBudget {
            self.r.warn(
                "cover_children_in_budget",
                "/spending/healthcare/cover_children",
                "Children's health cover is switched on but healthcare is \"in_budget\", so the engine charges nothing for it. Switch the mode to \"modeled\" or leave cover_children off.",
            );
        }
        if hc.cover_children && p.household.children.is_empty() {
            self.r.warn(
                "cover_children_no_children",
                "/spending/healthcare/cover_children",
                "Children's health cover is on but the household has no children.",
            );
        }
    }
}
