//! Checks of the housing decisions (`model::housing_choices`): borrowing against the
//! home, downsizing, cash-out refinancing, prepayment and rental dynamics.

use crate::model::{HomeEquityBorrowing, PropertyUse};

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
                if !has_home {
                    self.r.warn(
                        "borrowing_without_home",
                        path,
                        "Borrowing against a home is set, but the household owns no main home and buys none.",
                    );
                }
            }
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
