//! Checks for the family features: children on the health plan, uncertain births,
//! college cost uncertainty, adult-child support and parental leave.

use crate::model::*;

use super::{seg, Checker};

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
        for (i, child) in p.household.children.iter().enumerate() {
            let path = format!("/household/children/{}", seg(&child.id, i));
            self.arrival(&path, child);
            self.college_cost(&path, child);
            self.adult_support(&path, child);
        }
    }

    fn adult_support(&mut self, path: &str, child: &Child) {
        let Some(s) = child.adult_support else {
            return;
        };
        let at = format!("{path}/adult_support");
        self.r.range(
            &format!("{at}/probability"),
            "Support probability",
            s.probability,
            0.0,
            1.0,
        );
        self.r
            .money(&format!("{at}/annual"), "Yearly support", s.annual);
        self.r.range(
            &format!("{at}/mean_years"),
            "Mean support years",
            s.mean_years,
            0.0,
            30.0,
        );
        if s.annual > 60_000.0 {
            self.r.warn(
                "support_implausible",
                &format!("{at}/annual"),
                "Support of more than $60,000 a year for an adult child is far above typical help (the extra cost of an adult living at home is $10,000 to $15,000 a year).",
            );
        }
    }

    fn college_cost(&mut self, path: &str, child: &Child) {
        let CollegeCost::Stochastic {
            attend_probability,
            real_tuition_growth,
            tuition_volatility,
            aid_scale,
        } = child.college_cost
        else {
            return;
        };
        let at = format!("{path}/college_cost");
        self.r.range(
            &format!("{at}/attend_probability"),
            "Attendance probability",
            attend_probability,
            0.0,
            1.0,
        );
        self.r.range(
            &format!("{at}/real_tuition_growth"),
            "Real tuition growth",
            real_tuition_growth,
            -0.1,
            0.2,
        );
        self.r.range(
            &format!("{at}/tuition_volatility"),
            "Tuition growth volatility",
            tuition_volatility,
            0.0,
            0.1,
        );
        self.r
            .range(&format!("{at}/aid_scale"), "Aid scale", aid_scale, 0.0, 2.0);
        if child.college == College::None {
            self.r.warn(
                "college_cost_no_plan",
                &at,
                "Stochastic college costs are set but the child has no college plan, so nothing is charged. Choose a college type first.",
            );
        }
        if real_tuition_growth > 0.05 {
            self.r.warn(
                "tuition_growth_high",
                &format!("{at}/real_tuition_growth"),
                "Real tuition growth above 5% a year above inflation is far above the long-run record of about 2% (and about 0 over the last decade).",
            );
        }
    }

    fn arrival(&mut self, path: &str, child: &Child) {
        let Arrival::Uncertain {
            probability,
            delay_mean_months,
            twins_probability,
        } = child.arrival
        else {
            return;
        };
        self.r.range(
            &format!("{path}/arrival/probability"),
            "Arrival probability",
            probability,
            0.0,
            1.0,
        );
        self.r.range(
            &format!("{path}/arrival/delay_mean_months"),
            "Mean arrival delay (months)",
            delay_mean_months,
            0.0,
            120.0,
        );
        self.r.range(
            &format!("{path}/arrival/twins_probability"),
            "Twins probability",
            twins_probability,
            0.0,
            1.0,
        );
        if twins_probability > 0.2 {
            self.r.warn(
                "twins_implausible",
                &format!("{path}/arrival/twins_probability"),
                "A twins probability above 20% is far above the population rate (about 3% of births, higher with fertility treatment).",
            );
        }
        if let Some(start) = self.p.start {
            if child.birth - start <= 0 {
                self.r.warn(
                    "arrival_born_child",
                    &format!("{path}/arrival"),
                    "Uncertain arrival applies only to a child whose birth month is after the start; this child is already born, so the setting is ignored.",
                );
            }
        }
    }
}
