//! Checks of the tax and policy realism options: `state_detail`, `policy_risk`,
//! `portfolio.tax_loss_harvesting` and `retirement.qcd`.

use super::Checker;
use crate::model::*;

impl Checker<'_> {
    pub(super) fn realism(&mut self) {
        self.state_detail();
        self.chained_cpi();
        self.policy_risk();
        self.qcd();
        self.tax_loss_harvesting();
    }

    /// `chained_cpi` slows indexing, so it needs indexing to be on.
    fn chained_cpi(&mut self) {
        let t = &self.p.assumptions.taxes;
        if t.chained_cpi && !t.index_brackets {
            self.r.warn(
                "chained_cpi_no_effect",
                "/assumptions/taxes/chained_cpi",
                "chained_cpi is on but index_brackets is off, so thresholds are not indexed at all and chained_cpi changes nothing.",
            );
        }
    }

    /// `portfolio.tax_loss_harvesting` acts only on taxable holdings.
    fn tax_loss_harvesting(&mut self) {
        let p = self.p;
        if p.portfolio.tax_loss_harvesting
            && !p.accounts.iter().any(|a| a.kind == AccountKind::Taxable)
        {
            self.r.warn(
                "tlh_no_taxable",
                "/portfolio/tax_loss_harvesting",
                "tax_loss_harvesting is on but the household has no taxable account, so there are no losses to harvest.",
            );
        }
    }

    /// `retirement.qcd` needs Giving lines to pay and an IRA to pay them from.
    fn qcd(&mut self) {
        let p = self.p;
        if !p.retirement.qcd {
            return;
        }
        if !p
            .spending
            .items
            .iter()
            .any(|i| i.category == Category::Giving)
        {
            self.r.warn(
                "qcd_no_giving",
                "/retirement/qcd",
                "qcd is on but the budget has no Giving line, so there is nothing to pay as a qualified charitable distribution. Add a budget item with category \"giving\".",
            );
        }
        let has_traditional = p
            .accounts
            .iter()
            .any(|a| a.kind == AccountKind::Traditional)
            || p.incomes
                .iter()
                .any(|i| matches!(&i.kind, IncomeKind::Job(j) if j.plan.is_some()));
        if !has_traditional {
            self.r.warn(
                "qcd_no_ira",
                "/retirement/qcd",
                "qcd is on but the household has no traditional account and no employer plan, so there is no IRA to give from.",
            );
        }
    }

    /// `policy_risk`: probabilities and the factor are fractions; the years and the
    /// tax increase must be sensible.
    fn policy_risk(&mut self) {
        let a = &self.p.assumptions;
        let Some(risk) = a.taxes.policy_risk else {
            return;
        };
        let base = "/assumptions/taxes/policy_risk";
        let r = &mut self.r;
        r.range(
            &format!("{base}/ss_haircut_probability"),
            "Social Security cut probability",
            risk.ss_haircut_probability,
            0.0,
            1.0,
        );
        r.range(
            &format!("{base}/ss_haircut_factor"),
            "Benefit factor after a cut",
            risk.ss_haircut_factor,
            0.0,
            1.0,
        );
        r.range(
            &format!("{base}/tax_increase_probability"),
            "Tax increase probability",
            risk.tax_increase_probability,
            0.0,
            1.0,
        );
        r.range(
            &format!("{base}/tax_increase_points"),
            "Tax increase",
            risk.tax_increase_points,
            0.0,
            0.5,
        );
        if !(2000..=2200).contains(&risk.earliest_year) {
            r.error(
                "out_of_range",
                &format!("{base}/earliest_year"),
                "earliest_year must be between 2000 and 2200.",
            );
        }
        if risk.ss_haircut_probability <= 0.0 && risk.tax_increase_probability <= 0.0 {
            r.warn(
                "policy_risk_no_effect",
                base,
                "Both probabilities are 0, so policy_risk changes nothing. Set ss_haircut_probability and/or tax_increase_probability above 0, or remove policy_risk.",
            );
        }
        if risk.ss_haircut_probability > 0.0 && risk.ss_haircut_factor < 0.5 {
            r.warn(
                "policy_risk_deep_cut",
                &format!("{base}/ss_haircut_factor"),
                "A cut that leaves under half of scheduled benefits is far beyond what the Trustees project (77% to 81% payable); check the value is a share, 0.77 for a 23% cut.",
            );
        }
        if risk.tax_increase_points > 0.1 {
            r.warn(
                "policy_risk_big_tax",
                &format!("{base}/tax_increase_points"),
                "More than 10 points on every ordinary bracket is a very large increase; tax_increase_points is a decimal, 0.02 for 2 points.",
            );
        }
        if risk.ss_haircut_probability > 0.0 && a.taxes.social_security_haircut.is_some() {
            r.warn(
                "policy_risk_stacks",
                base,
                "social_security_haircut is also set, so a path that draws a cut pays both reductions. Remove one of them if that is not what you mean.",
            );
        }
    }

    /// `state_detail: full` only changes California, New York and Utah.
    fn state_detail(&mut self) {
        let a = &self.p.assumptions;
        if a.taxes.state_detail != StateDetail::Full {
            return;
        }
        let loc = &self.p.household.location;
        let applies = |state: Option<&str>| {
            state.is_some_and(|s| {
                ["CA", "NY", "UT"]
                    .iter()
                    .any(|code| s.eq_ignore_ascii_case(code))
            })
        };
        let relocates = self.p.events.iter().any(|e| match &e.kind {
            EventKind::Relocate(r) => applies(r.state.as_deref()),
            _ => false,
        });
        if !applies(loc.state.as_deref()) && !relocates {
            self.r.warn(
                "state_detail_no_effect",
                "/assumptions/taxes/state_detail",
                "state_detail is \"full\" but the household does not live in California, New York or Utah (and no relocation goes there), so it changes nothing. It adds California's disability insurance tax, New York's recapture of lower brackets and Utah's Social Security credit.",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::model::*;
    use crate::validate::validate;

    fn with_state(state: Option<&str>, detail: StateDetail) -> Profile {
        let mut p = Profile::example();
        p.household.location.state = state.map(str::to_string);
        p.assumptions.taxes.state_detail = detail;
        p
    }

    #[test]
    fn qcd_warns_without_giving_or_an_ira() {
        let mut p = Profile::example();
        p.retirement.qcd = true;
        let r = validate(&p);
        assert!(r.errors.is_empty());
        // The dual-income example gives $150 a month and has employer plans.
        assert!(!r.warnings.iter().any(|w| w.code.starts_with("qcd_")));
        p.spending.items.retain(|i| i.category != Category::Giving);
        p.accounts.retain(|a| a.kind != AccountKind::Traditional);
        for i in &mut p.incomes {
            if let IncomeKind::Job(j) = &mut i.kind {
                j.plan = None;
            }
        }
        let r = validate(&p);
        assert!(r.warnings.iter().any(|w| w.code == "qcd_no_giving"));
        assert!(r.warnings.iter().any(|w| w.code == "qcd_no_ira"));
    }

    #[test]
    fn chained_cpi_warns_when_indexing_is_off() {
        let mut p = Profile::example();
        p.assumptions.taxes.chained_cpi = true;
        let has = |p: &Profile| {
            validate(p)
                .warnings
                .iter()
                .any(|w| w.code == "chained_cpi_no_effect")
        };
        assert!(!has(&p));
        p.assumptions.taxes.index_brackets = false;
        assert!(has(&p));
    }

    #[test]
    fn tlh_warns_without_a_taxable_account() {
        let mut p = Profile::example();
        p.portfolio.tax_loss_harvesting = true;
        p.accounts.retain(|a| a.kind != AccountKind::Taxable);
        assert!(validate(&p)
            .warnings
            .iter()
            .any(|w| w.code == "tlh_no_taxable"));
        let mut q = Profile::example();
        q.portfolio.tax_loss_harvesting = true;
        assert!(!validate(&q)
            .warnings
            .iter()
            .any(|w| w.code == "tlh_no_taxable"));
    }

    #[test]
    fn policy_risk_checks() {
        let mut p = Profile::example();
        p.assumptions.taxes.policy_risk = Some(PolicyRisk::default());
        let r = validate(&p);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r
            .warnings
            .iter()
            .all(|w| !w.code.starts_with("policy_risk")));

        let mut bad = p.clone();
        bad.assumptions.taxes.policy_risk = Some(PolicyRisk {
            ss_haircut_probability: 1.5,
            ss_haircut_factor: 77.0,
            earliest_year: 33,
            tax_increase_points: 2.0,
            ..PolicyRisk::default()
        });
        let r = validate(&bad);
        let paths: Vec<&str> = r.errors.iter().map(|e| e.path.as_str()).collect();
        for field in [
            "ss_haircut_probability",
            "ss_haircut_factor",
            "earliest_year",
            "tax_increase_points",
        ] {
            let path = format!("/assumptions/taxes/policy_risk/{field}");
            assert!(paths.contains(&path.as_str()), "{field}: {paths:?}");
        }

        let mut idle = p.clone();
        idle.assumptions.taxes.policy_risk = Some(PolicyRisk {
            ss_haircut_probability: 0.0,
            tax_increase_probability: 0.0,
            ..PolicyRisk::default()
        });
        assert!(validate(&idle)
            .warnings
            .iter()
            .any(|w| w.code == "policy_risk_no_effect"));

        let mut both = p;
        both.assumptions.taxes.social_security_haircut = Some(SocialSecurityHaircut::default());
        assert!(validate(&both)
            .warnings
            .iter()
            .any(|w| w.code == "policy_risk_stacks"));
    }

    #[test]
    fn state_detail_warns_only_where_it_does_nothing() {
        let r = validate(&with_state(Some("TX"), StateDetail::Full));
        assert!(r
            .warnings
            .iter()
            .any(|w| w.code == "state_detail_no_effect"));
        let r = validate(&with_state(Some("ny"), StateDetail::Full));
        assert!(!r
            .warnings
            .iter()
            .any(|w| w.code == "state_detail_no_effect"));
        let r = validate(&with_state(Some("TX"), StateDetail::Basic));
        assert!(!r
            .warnings
            .iter()
            .any(|w| w.code == "state_detail_no_effect"));
    }
}
