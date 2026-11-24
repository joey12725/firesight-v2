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
            self.parental_leave(&path, child, "parental_leave", child.parental_leave);
            self.parental_leave(
                &path,
                child,
                "second_parental_leave",
                child.second_parental_leave,
            );
            if let (Some(a), Some(b)) = (child.parental_leave, child.second_parental_leave) {
                if a.person == b.person {
                    self.r.error(
                        "leave_same_person",
                        &format!("{path}/second_parental_leave/person"),
                        "The second parental leave names the same person as the first; it is for the other parent. Lengthen the first leave instead.",
                    );
                }
            }
        }
    }

    fn parental_leave(
        &mut self,
        path: &str,
        child: &Child,
        field: &str,
        leave: Option<ParentalLeave>,
    ) {
        let Some(l) = leave else {
            return;
        };
        let at = format!("{path}/{field}");
        let people = self.p.household.people.len();
        if l.person >= people {
            self.r.error(
                "leave_person",
                &format!("{at}/person"),
                format!(
                    "Parental leave names person {} but the household has {people}.",
                    l.person
                ),
            );
        }
        self.r.range(
            &format!("{at}/pay_fraction"),
            "Leave pay fraction",
            l.pay_fraction,
            0.0,
            1.0,
        );
        if l.months > 36 {
            self.r.error(
                "leave_months",
                &format!("{at}/months"),
                "Parental leave of more than 36 months is not modelled as leave; use a career break event or end the job.",
            );
        } else if l.months > 12 {
            self.r.warn(
                "leave_long",
                &format!("{at}/months"),
                "Parental leave of more than 12 months is longer than almost any employer or state programme.",
            );
        }
        if let Some(start) = self.p.start {
            if child.birth - start < 0 {
                self.r.warn(
                    "leave_born_child",
                    &at,
                    "Parental leave starts at a birth during the plan; this child is already born, so no leave is taken.",
                );
            }
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
        if let Some(start) = self.p.start {
            // School ends at 22 for a four-year student and a spell starts within the
            // six years after that.
            if start - child.birth >= 28 * 12 {
                self.r.warn(
                    "support_window_passed",
                    &at,
                    "Adult support is drawn as a spell that starts within six years of the end of school; this child is already past that window, so only the tail of a spell that would already be under way is charged (usually nothing). Enter support you are paying now as a spending item.",
                );
            }
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
        if aid_scale > 0.0 && child.college_share < 1.0 && child.college != College::None {
            self.r.warn(
                "college_aid_twice",
                &format!("{at}/aid_scale"),
                "The parents' share of college is below 100% and stochastic costs also take an aid discount off the published price. If the share already allows for aid or scholarships, set aid_scale to 0 so aid is not counted twice.",
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
