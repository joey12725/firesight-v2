//! Checks of the housing decisions (`model::housing_choices`): borrowing against the
//! home, downsizing, cash-out refinancing, prepayment and rental dynamics.

use crate::model::{Downsize, DownsizeWhen, HomeEquityBorrowing, PropertyUse, ReverseMortgageOpen};

use super::Checker;

impl Checker<'_> {
    pub(super) fn housing_decisions(&mut self) {
        let p = self.p;
        let path = "/retirement/home_equity_borrowing";
        let has_home = p
            .properties
            .iter()
            .any(|x| matches!(x.usage, PropertyUse::Primary))
            || p.events.iter().any(|e| {
                matches!(&e.kind, crate::model::EventKind::BuyHome(b)
                    if matches!(b.usage, PropertyUse::Primary))
            });
        match p.retirement.home_equity_borrowing {
            HomeEquityBorrowing::None => {}
            HomeEquityBorrowing::Heloc(h) => {
                self.r.range(
                    &format!("{path}/max_combined_ltv"),
                    "Combined loan-to-value",
                    h.max_combined_ltv,
                    0.0,
                    1.0,
                );
                self.r
                    .range(&format!("{path}/spread"), "Spread", h.spread, -0.05, 0.2);
                self.r.range(
                    &format!("{path}/draw_years"),
                    "Draw period",
                    h.draw_years,
                    0.0,
                    40.0,
                );
                self.r.range(
                    &format!("{path}/repay_years"),
                    "Repayment period",
                    h.repay_years,
                    0.5,
                    40.0,
                );
                self.r.range(
                    &format!("{path}/freeze_probability"),
                    "Freeze probability",
                    h.freeze_probability,
                    0.0,
                    1.0,
                );
                if h.max_combined_ltv > 0.9 && h.max_combined_ltv <= 1.0 {
                    self.r.warn(
                        "heloc_ltv_high",
                        &format!("{path}/max_combined_ltv"),
                        "Few lenders allow a combined loan-to-value above 0.90; 0.80 to 0.85 is usual.",
                    );
                }
                if h.spread.is_finite() && !(0.01..=0.08).contains(&h.spread) {
                    self.r.warn(
                        "heloc_spread_unusual",
                        &format!("{path}/spread"),
                        "A home-equity line usually costs 3 to 5 points above the Treasury bill rate (the prime rate plus a margin).",
                    );
                }
            }
            HomeEquityBorrowing::ReverseMortgage(r) => {
                self.r.range(
                    &format!("{path}/lender_margin"),
                    "Lender margin",
                    r.lender_margin,
                    -0.05,
                    0.2,
                );
                self.r.range(
                    &format!("{path}/other_closing_costs"),
                    "Other closing costs",
                    r.other_closing_costs,
                    0.0,
                    1e6,
                );
                if r.lender_margin.is_finite() && !(0.01..=0.04).contains(&r.lender_margin) {
                    self.r.warn(
                        "hecm_margin_unusual",
                        &format!("{path}/lender_margin"),
                        "Reverse-mortgage lenders' margins are usually 1.5 to 3 points over the Treasury rate.",
                    );
                }
                if r.other_closing_costs > 20_000.0 {
                    self.r.warn(
                        "hecm_costs_high",
                        &format!("{path}/other_closing_costs"),
                        "Closing costs other than insurance and the origination fee are usually a few thousand dollars.",
                    );
                }
                if let ReverseMortgageOpen::SavingsBelow { years } = r.open_when {
                    let at = format!("{path}/open_when/years");
                    self.r.range(&at, "Years of spending", years, 0.0, 100.0);
                    if years > 25.0 {
                        self.r.warn(
                            "hecm_standby_early",
                            &at,
                            "The line opens while savings still cover more than 25 years of spending.",
                        );
                    }
                }
            }
        }
        if !p.retirement.home_equity_borrowing.is_none() && !has_home {
            self.r.warn(
                "borrowing_without_home",
                path,
                "Borrowing against a home is set, but the household owns no main home and buys none.",
            );
        }
        if let Some(d) = &p.retirement.downsize {
            self.downsize(d, has_home);
        }
    }

    fn downsize(&mut self, d: &Downsize, has_home: bool) {
        let p = self.p;
        let path = "/retirement/downsize";
        let at = |f: &str| format!("{path}/{f}");
        if self.r.range(
            &at("to_value_fraction"),
            "Value of the new home",
            d.to_value_fraction,
            0.0,
            10.0,
        ) && d.to_value_fraction >= 1.0
        {
            self.r.warn(
                "downsize_not_smaller",
                &at("to_value_fraction"),
                "The new home is worth as much as the old one or more: this is a move, not a downsizing.",
            );
        }
        if self.r.range(
            &at("selling_cost_fraction"),
            "Selling costs",
            d.selling_cost_fraction,
            0.0,
            0.5,
        ) && d.selling_cost_fraction > 0.10
        {
            self.r.warn(
                "downsize_selling_costs_high",
                &at("selling_cost_fraction"),
                "Selling a home usually costs 5 to 8% of the price.",
            );
        }
        self.r.range(
            &at("buying_cost_fraction"),
            "Buying costs",
            d.buying_cost_fraction,
            0.0,
            0.5,
        );
        match d.when {
            DownsizeWhen::AtAge { person, age } => {
                self.person_ref(&at("when/person"), person);
                self.r.range(&at("when/age"), "Age", age, 0.0, 120.0);
            }
            DownsizeWhen::WhenChildrenLeave => {
                if p.household.children.is_empty() {
                    self.r.warn(
                        "downsize_no_children",
                        &at("when"),
                        "The move waits for the children to leave, but the household has none: it never happens.",
                    );
                }
            }
            DownsizeWhen::WhenWidowed => {
                if p.household.people.len() < 2 {
                    self.r.warn(
                        "downsize_single",
                        &at("when"),
                        "The move waits for the first death in a couple, but the household has one adult: it never happens.",
                    );
                }
            }
            DownsizeWhen::WhenFundedRatioBelow { ratio } => {
                if self
                    .r
                    .range(&at("when/ratio"), "Funded ratio", ratio, 0.0, 10.0)
                    && ratio > 1.5
                {
                    self.r.warn(
                        "downsize_ratio_high",
                        &at("when/ratio"),
                        "The household moves while savings are still more than one and a half times what the plan needs.",
                    );
                }
            }
        }
        if !has_home {
            self.r.warn(
                "downsize_without_home",
                path,
                "A downsizing is planned, but the household owns no main home and buys none.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::*;
    use crate::validate::validate;

    /// (code, path) of each finding.
    type Findings = Vec<(String, String)>;

    fn codes(p: &Profile) -> (Findings, Findings) {
        let r = validate(p);
        let pairs = |v: &[crate::validate::Issue]| {
            v.iter()
                .map(|i| (i.code.clone(), i.path.clone()))
                .collect::<Vec<_>>()
        };
        (pairs(&r.errors), pairs(&r.warnings))
    }

    fn has(v: &[(String, String)], code: &str, path: &str) -> bool {
        v.iter().any(|(c, p)| c == code && p == path)
    }

    #[test]
    fn heloc_terms_are_checked() {
        let mut p = Profile::example();
        p.retirement.home_equity_borrowing = HomeEquityBorrowing::Heloc(Heloc::default());
        let (errors, warnings) = codes(&p);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(!warnings.iter().any(|(c, _)| c.starts_with("heloc")));
        let base = "/retirement/home_equity_borrowing";
        p.retirement.home_equity_borrowing = HomeEquityBorrowing::Heloc(Heloc {
            max_combined_ltv: 80.0,
            freeze_probability: -0.1,
            ..Heloc::default()
        });
        let (errors, _) = codes(&p);
        assert!(has(
            &errors,
            "out_of_range",
            &format!("{base}/max_combined_ltv")
        ));
        assert!(has(
            &errors,
            "out_of_range",
            &format!("{base}/freeze_probability")
        ));
        p.retirement.home_equity_borrowing = HomeEquityBorrowing::Heloc(Heloc {
            max_combined_ltv: 0.95,
            spread: 0.12,
            ..Heloc::default()
        });
        let (errors, warnings) = codes(&p);
        assert!(errors.is_empty());
        assert!(has(
            &warnings,
            "heloc_ltv_high",
            &format!("{base}/max_combined_ltv")
        ));
        assert!(has(
            &warnings,
            "heloc_spread_unusual",
            &format!("{base}/spread")
        ));
        // A renter with a line.
        p.properties.clear();
        p.events.clear();
        let (_, warnings) = codes(&p);
        assert!(has(&warnings, "borrowing_without_home", base));
    }

    #[test]
    fn reverse_mortgage_terms_are_checked() {
        let mut p = Profile::example();
        let base = "/retirement/home_equity_borrowing";
        p.retirement.home_equity_borrowing =
            HomeEquityBorrowing::ReverseMortgage(ReverseMortgage::default());
        let (errors, warnings) = codes(&p);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(!warnings.iter().any(|(c, _)| c.starts_with("hecm")));
        p.retirement.home_equity_borrowing =
            HomeEquityBorrowing::ReverseMortgage(ReverseMortgage {
                open_when: ReverseMortgageOpen::SavingsBelow { years: -1.0 },
                other_closing_costs: -5.0,
                ..ReverseMortgage::default()
            });
        let (errors, _) = codes(&p);
        assert!(has(
            &errors,
            "out_of_range",
            &format!("{base}/open_when/years")
        ));
        assert!(has(
            &errors,
            "out_of_range",
            &format!("{base}/other_closing_costs")
        ));
        p.retirement.home_equity_borrowing =
            HomeEquityBorrowing::ReverseMortgage(ReverseMortgage {
                open_when: ReverseMortgageOpen::SavingsBelow { years: 40.0 },
                lender_margin: 0.06,
                other_closing_costs: 50_000.0,
            });
        let (errors, warnings) = codes(&p);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(has(
            &warnings,
            "hecm_standby_early",
            &format!("{base}/open_when/years")
        ));
        assert!(has(
            &warnings,
            "hecm_margin_unusual",
            &format!("{base}/lender_margin")
        ));
        assert!(has(
            &warnings,
            "hecm_costs_high",
            &format!("{base}/other_closing_costs")
        ));
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""type":"reverse_mortgage""#));
        assert!(json.contains(r#""open_when":{"type":"savings_below","years":40.0}"#));
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn downsize_is_checked_and_round_trips() {
        let mut p = Profile::example();
        let base = "/retirement/downsize";
        p.retirement.downsize = Some(Downsize::default());
        let (errors, warnings) = codes(&p);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(
            !warnings.iter().any(|(c, _)| c.starts_with("downsize")),
            "{warnings:?}"
        );
        p.retirement.downsize = Some(Downsize {
            when: DownsizeWhen::AtAge {
                person: 5,
                age: 70.0,
            },
            to_value_fraction: -0.1,
            selling_cost_fraction: 0.7,
            ..Downsize::default()
        });
        let (errors, _) = codes(&p);
        assert!(has(&errors, "ref_person", &format!("{base}/when/person")));
        assert!(has(
            &errors,
            "out_of_range",
            &format!("{base}/to_value_fraction")
        ));
        assert!(has(
            &errors,
            "out_of_range",
            &format!("{base}/selling_cost_fraction")
        ));
        p.retirement.downsize = Some(Downsize {
            when: DownsizeWhen::WhenFundedRatioBelow { ratio: 2.0 },
            to_value_fraction: 1.2,
            selling_cost_fraction: 0.15,
            ..Downsize::default()
        });
        let (errors, warnings) = codes(&p);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(has(
            &warnings,
            "downsize_ratio_high",
            &format!("{base}/when/ratio")
        ));
        assert!(has(
            &warnings,
            "downsize_not_smaller",
            &format!("{base}/to_value_fraction")
        ));
        assert!(has(
            &warnings,
            "downsize_selling_costs_high",
            &format!("{base}/selling_cost_fraction")
        ));
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            json.contains(r#""downsize":{"when":{"type":"when_funded_ratio_below","ratio":2.0}"#),
            "{json}"
        );
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        let mut q = Profile::example();
        q.household.children.clear();
        q.retirement.downsize = Some(Downsize::default());
        let (_, warnings) = codes(&q);
        assert!(has(
            &warnings,
            "downsize_no_children",
            &format!("{base}/when")
        ));
        assert!(!serde_json::to_string(&Profile::example())
            .unwrap()
            .contains("downsize"));
    }

    #[test]
    fn borrowing_round_trips_and_is_left_out_by_default() {
        let p = Profile::example();
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("home_equity_borrowing"));
        let mut q = p.clone();
        q.retirement.home_equity_borrowing = HomeEquityBorrowing::Heloc(Heloc::default());
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains(r#""home_equity_borrowing":{"type":"heloc""#));
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, q);
    }
}
