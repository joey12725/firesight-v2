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
