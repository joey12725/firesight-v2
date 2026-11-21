//! Checks of the household-dynamics fields: divorce, parents, caregiving and life
//! insurance.

use crate::model::*;

use super::{check_ids, Checker};

impl Checker<'_> {
    /// `household.married`, `household.divorce_risk`.
    pub(super) fn family(&mut self) {
        let p = self.p;
        let h = &p.household;
        if let Some(married) = h.married {
            let youngest_adult = h.people.iter().map(|x| x.birth).max();
            if let Some(birth) = youngest_adult {
                if married.year() - birth.year() < 14 {
                    self.r.error(
                        "date_range",
                        "/household/married",
                        format!("Married {married} is before both partners were 14."),
                    );
                }
            }
            if let Some(start) = p.start {
                if married > start + 12 * 60 {
                    self.r.error(
                        "date_range",
                        "/household/married",
                        "The marriage date is more than 60 years after the start.",
                    );
                }
            }
        }
        if let DivorceRisk::Typical(terms) = &h.divorce_risk {
            if self.n_people != 2 {
                self.r.error(
                    "divorce_single",
                    "/household/divorce_risk",
                    "A divorce risk needs two people in the household; set divorce_risk to {\"type\": \"off\"}.",
                );
            }
            if h.married.is_none() {
                self.r.warn(
                    "divorce_married_missing",
                    "/household/married",
                    "divorce_risk is on but household.married is empty, so the marriage is taken to begin at the start of the plan: a new marriage has the highest divorce risk. Give the month you married.",
                );
            }
            self.divorce_terms("/household/divorce_risk", terms);
        }
        self.parents();
    }

    /// `household.parents`.
    fn parents(&mut self) {
        let p = self.p;
        let parents = &p.household.parents;
        if parents.is_empty() {
            return;
        }
        if parents.len() > MAX_PARENTS {
            self.r.error(
                "parents_too_many",
                "/household/parents",
                format!("At most {MAX_PARENTS} parents can be listed."),
            );
        }
        check_ids(
            &mut self.r,
            "/household/parents",
            parents.iter().map(|x| x.id.as_str()),
        );
        for (i, parent) in parents.iter().enumerate() {
            let k = format!("/household/parents/{i}");
            if parent.child >= self.n_people {
                self.r.error(
                    "person_index",
                    &format!("{k}/child"),
                    format!(
                        "Parent {} names person {}, who is not in the household.",
                        parent.id, parent.child
                    ),
                );
            } else if let Some(child) = p.household.people.get(parent.child) {
                let gap = child.birth - parent.birth;
                if gap < 12 * 12 {
                    self.r.error(
                        "date_range",
                        &format!("{k}/birth"),
                        format!(
                            "{} is less than 12 years older than their child.",
                            parent.id
                        ),
                    );
                } else if !(15 * 12..=55 * 12).contains(&gap) {
                    self.r.warn(
                        "parent_age_gap",
                        &format!("{k}/birth"),
                        format!(
                            "{} is {} years older than their child; check the birth date.",
                            parent.id,
                            gap / 12
                        ),
                    );
                }
            }
            if let Some(start) = p.start {
                if parent.birth.age_months_at(start) >= 119 * 12 {
                    self.r.error(
                        "date_range",
                        &format!("{k}/birth"),
                        format!("{} would be 119 or older at the start.", parent.id),
                    );
                }
            }
            self.r
                .money(&format!("{k}/estate"), "Estate", parent.estate);
            self.r
                .range(&format!("{k}/share"), "Share", parent.share, 0.0, 1.0);
            self.r.range(
                &format!("{k}/inherited_ira_fraction"),
                "Inherited IRA fraction",
                parent.inherited_ira_fraction,
                0.0,
                1.0,
            );
            if parent.estate > 0.0 && parent.share == 0.0 {
                self.r.warn(
                    "parent_share_zero",
                    &format!("{k}/share"),
                    format!(
                        "{} has an estate but the household's share of it is 0.",
                        parent.id
                    ),
                );
            }
        }
    }

    /// A `divorce` event.
    pub(super) fn divorce_event(&mut self, path: &str, terms: &DivorceTerms) {
        if self.n_people != 2 {
            self.r.error(
                "divorce_single",
                path,
                "A divorce event needs two people in the household.",
            );
        }
        self.divorce_terms(path, terms);
    }

    fn divorce_terms(&mut self, path: &str, t: &DivorceTerms) {
        self.r.range(
            &format!("{path}/asset_split"),
            "Asset split",
            t.asset_split,
            0.0,
            1.0,
        );
        self.r
            .money(&format!("{path}/legal_costs"), "Legal costs", t.legal_costs);
        if self.r.range(
            &format!("{path}/spending_factor"),
            "Spending factor",
            t.spending_factor,
            0.0,
            2.0,
        ) && !(0.4..=1.0).contains(&t.spending_factor)
        {
            self.r.warn(
                "divorce_spending_factor",
                &format!("{path}/spending_factor"),
                format!(
                    "One adult alone usually spends 0.6 to 0.75 of what a couple spent on shared lines; {} looks implausible.",
                    t.spending_factor
                ),
            );
        }
        if let Some(rent) = t.then_rent_monthly {
            self.r
                .money(&format!("{path}/then_rent_monthly"), "Rent", rent);
        }
        let s = &t.support;
        self.r
            .money(&format!("{path}/support/annual"), "Support", s.annual);
        self.r.range(
            &format!("{path}/support/years"),
            "Support term",
            s.years,
            0.0,
            50.0,
        );
        if (t.asset_split > 0.0 && t.asset_split < 0.2) || t.asset_split > 0.8 {
            self.r.warn(
                "divorce_asset_split",
                &format!("{path}/asset_split"),
                "Courts usually divide marital property between 40/60 and 60/40; check the split.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::*;
    use crate::validate::validate;

    fn dual() -> Profile {
        Profile::examples(EXAMPLE_START)
            .into_iter()
            .find(|e| e.slug == "dual-income-family")
            .unwrap()
            .profile
    }

    fn codes(issues: &[crate::validate::Issue]) -> Vec<&str> {
        issues.iter().map(|i| i.code.as_str()).collect()
    }

    #[test]
    fn divorce_checks() {
        let mut p = dual();
        p.household.divorce_risk = DivorceRisk::Typical(DivorceTerms::default());
        let r = validate(&p);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(codes(&r.warnings).contains(&"divorce_married_missing"));

        p.household.married = Some(EXAMPLE_START - 24);
        let r = validate(&p);
        assert!(!codes(&r.warnings).contains(&"divorce_married_missing"));

        let mut single = p.clone();
        single.household.people.truncate(1);
        single.incomes.retain(|i| i.owner == 0);
        assert!(codes(&validate(&single).errors).contains(&"divorce_single"));

        let mut bad = p.clone();
        bad.household.divorce_risk = DivorceRisk::Typical(DivorceTerms {
            asset_split: 1.5,
            ..DivorceTerms::default()
        });
        assert!(!validate(&bad).errors.is_empty());

        let mut odd = p.clone();
        odd.household.divorce_risk = DivorceRisk::Typical(DivorceTerms {
            asset_split: 0.95,
            spending_factor: 1.3,
            ..DivorceTerms::default()
        });
        let w = validate(&odd).warnings;
        assert!(codes(&w).contains(&"divorce_asset_split"));
        assert!(codes(&w).contains(&"divorce_spending_factor"));

        let mut early = p;
        early.household.married = Some(YearMonth::ym(1995, 1));
        assert!(codes(&validate(&early).errors).contains(&"date_range"));
    }

    #[test]
    fn parent_checks() {
        let mut p = dual();
        let parent = Parent {
            id: "mom".into(),
            birth: p.household.people[0].birth - 30 * 12,
            estate: 100_000.0,
            share: 0.5,
            ..Parent::default()
        };
        p.household.parents = vec![parent.clone()];
        let r = validate(&p);
        assert!(r.errors.is_empty() && r.warnings.is_empty(), "{r:?}");
        let mut bad = p.clone();
        bad.household.parents[0].child = 2;
        assert!(codes(&validate(&bad).errors).contains(&"person_index"));
        let mut bad = p.clone();
        bad.household.parents[0].birth = p.household.people[0].birth - 60;
        assert!(codes(&validate(&bad).errors).contains(&"date_range"));
        let mut bad = p.clone();
        bad.household.parents[0].share = 1.2;
        assert!(!validate(&bad).errors.is_empty());
        let mut dup = p.clone();
        dup.household.parents.push(parent);
        assert!(codes(&validate(&dup).errors).contains(&"id_duplicate"));
        let mut odd = p;
        odd.household.parents[0].birth = odd.household.people[0].birth - 13 * 12;
        assert!(codes(&validate(&odd).warnings).contains(&"parent_age_gap"));
    }

    #[test]
    fn divorce_terms_parse_from_json() {
        let v: DivorceRisk = serde_json::from_str(
            r#"{"type": "typical", "asset_split": 0.4, "support": {"annual": 1000, "years": 3, "direction": "receives"}}"#,
        )
        .unwrap();
        let DivorceRisk::Typical(t) = v else { panic!() };
        assert_eq!(t.asset_split, 0.4);
        assert_eq!(t.support.direction, SupportDirection::Receives);
        assert_eq!(t.legal_costs, DIVORCE_LEGAL_COSTS);
        assert!(serde_json::from_str::<DivorceRisk>(r#"{"type": "typical", "nope": 1}"#).is_err());
    }
}
