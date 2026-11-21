//! Checks of the opt-in health, longevity and care features: health states,
//! linked lifespans, the income gradient, Medicaid and long-term-care insurance.

use super::Checker;
use crate::model::*;

impl Checker<'_> {
    /// Errors: values the features are not defined for.
    pub(super) fn care_errors(&mut self) {
        for (i, person) in self.p.household.people.iter().enumerate() {
            let Some(policy) = &person.ltc_insurance else {
                continue;
            };
            let base = format!(
                "/household/people/{}/ltc_insurance",
                super::seg(&person.id, i)
            );
            self.r.money(
                &format!("{base}/annual_premium"),
                "Annual premium",
                policy.annual_premium,
            );
            self.r.money(
                &format!("{base}/daily_benefit"),
                "Daily benefit",
                policy.daily_benefit,
            );
            self.r.range(
                &format!("{base}/benefit_years"),
                "Benefit years",
                policy.benefit_years,
                0.0,
                100.0,
            );
            self.r.range(
                &format!("{base}/elimination_days"),
                "Elimination period",
                policy.elimination_days,
                0.0,
                3_650.0,
            );
            if let InflationRider::Compound { rate } = policy.inflation_rider {
                self.r.range(
                    &format!("{base}/inflation_rider/rate"),
                    "Inflation rider",
                    rate,
                    0.0,
                    0.2,
                );
            }
        }
        if let Longevity::Stochastic {
            income_percentile: Some(pct),
            ..
        } = self.p.assumptions.longevity
        {
            self.r.range(
                "/assumptions/longevity/income_percentile",
                "Income percentile",
                pct,
                1.0,
                100.0,
            );
        }
    }

    /// Warnings: combinations that run but probably do not mean what was intended.
    pub(super) fn care_warnings(&mut self) {
        let p = self.p;
        if p.assumptions.health_model == HealthModelKind::States && !p.assumptions.health_risk {
            self.r.warn(
                "health_states_without_risk",
                "/assumptions/health_model",
                "Health states are on but health_risk is off, so costs are averages: the states still shape who needs care late in life only through mortality, not costs. Turn health_risk on for the states to change costs.",
            );
        }
        let hc = &p.spending.healthcare;
        for (i, person) in p.household.people.iter().enumerate() {
            let Some(policy) = &person.ltc_insurance else {
                continue;
            };
            let base = format!(
                "/household/people/{}/ltc_insurance",
                super::seg(&person.id, i)
            );
            if !hc.long_term_care || hc.mode != HealthcareMode::Modeled {
                self.r.warn(
                    "ltc_insurance_without_care",
                    &base,
                    "A long-term-care policy only pays for care the engine models; with long-term care off or healthcare in the budget it does nothing.",
                );
            }
            if policy.daily_benefit > 1_000.0 {
                self.r.warn(
                    "ltc_daily_benefit_high",
                    &format!("{base}/daily_benefit"),
                    "A daily benefit above $1,000 is far above what policies sell (typically $150 to $250 a day). Enter the benefit per day, not per month.",
                );
            }
            if policy.benefit_years > 10.0 {
                self.r.warn(
                    "ltc_benefit_years_long",
                    &format!("{base}/benefit_years"),
                    "Policies with more than 10 years of benefits (or unlimited ones) are rarely sold today.",
                );
            }
            if policy.annual_premium > 20_000.0 {
                self.r.warn(
                    "ltc_premium_high",
                    &format!("{base}/annual_premium"),
                    "An annual premium above $20,000 is unusual for one person. Enter the premium per year.",
                );
            }
        }
        if hc.medicaid && (!hc.long_term_care || hc.mode != HealthcareMode::Modeled) {
            self.r.warn(
                "medicaid_without_care",
                "/spending/healthcare/medicaid",
                "Medicaid only pays for long-term care the engine models; with long-term care off or healthcare in the budget it does nothing.",
            );
        }
        if let Longevity::Stochastic {
            couple_dependence,
            income_gradient,
            income_percentile,
            ..
        } = p.assumptions.longevity
        {
            if couple_dependence == CoupleDependence::Linked && p.household.people.len() < 2 {
                self.r.warn(
                    "linked_lifespans_single",
                    "/assumptions/longevity/couple_dependence",
                    "Linked lifespans only affect a couple; this household has one adult, so the setting does nothing.",
                );
            }
            if income_gradient
                && income_percentile.is_none()
                && crate::longevity_links::household_income(p).is_none()
            {
                self.r.warn(
                    "income_gradient_no_earnings",
                    "/assumptions/longevity/income_percentile",
                    "The income gradient is on but no earnings run at the start, so the household is placed at the 50th income percentile. Set income_percentile to the household's rank during its working years.",
                );
            }
            if income_percentile.is_some() && !income_gradient {
                self.r.warn(
                    "income_percentile_unused",
                    "/assumptions/longevity/income_percentile",
                    "income_percentile is set but income_gradient is off, so it is ignored.",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::patch::{apply_to_profile, PatchOp};
    use crate::validate::validate;
    use serde_json::json;

    fn codes(ops: &[PatchOp]) -> (Vec<String>, Vec<String>) {
        let p = apply_to_profile(&crate::model::Profile::example(), ops).unwrap();
        let r = validate(&p);
        (
            r.errors.into_iter().map(|i| i.code).collect(),
            r.warnings.into_iter().map(|i| i.code).collect(),
        )
    }

    #[test]
    fn health_states_without_health_risk_warns() {
        let (e, w) = codes(&[
            PatchOp::set("/assumptions/health_model", json!({"type": "states"})),
            PatchOp::set("/assumptions/health_risk", json!(false)),
        ]);
        assert!(e.is_empty(), "{e:?}");
        assert!(w.contains(&"health_states_without_risk".to_string()));
        let (e, w) = codes(&[PatchOp::set(
            "/assumptions/health_model",
            json!({"type": "states"}),
        )]);
        assert!(e.is_empty());
        assert!(!w.contains(&"health_states_without_risk".to_string()));
    }

    #[test]
    fn longevity_options_are_checked() {
        let stochastic = |extra: serde_json::Value| {
            let mut v = json!({"type": "stochastic", "improvement": true});
            for (k, x) in extra.as_object().unwrap() {
                v[k] = x.clone();
            }
            PatchOp::set("/assumptions/longevity", v)
        };
        let (e, _) = codes(&[stochastic(
            json!({"income_gradient": true, "income_percentile": 0.0}),
        )]);
        assert!(e.contains(&"out_of_range".to_string()), "{e:?}");
        let (e, w) = codes(&[stochastic(
            json!({"income_gradient": true, "income_percentile": 90.0}),
        )]);
        assert!(e.is_empty() && !w.contains(&"income_percentile_unused".to_string()));
        let (_, w) = codes(&[stochastic(json!({"income_percentile": 90.0}))]);
        assert!(w.contains(&"income_percentile_unused".to_string()));
        // The default example is a single adult.
        let (e, w) = codes(&[stochastic(json!({"couple_dependence": {"type": "linked"}}))]);
        assert!(e.is_empty(), "{e:?}");
        let single = crate::model::Profile::example().household.people.len() < 2;
        assert_eq!(w.contains(&"linked_lifespans_single".to_string()), single);
    }

    #[test]
    fn ltc_insurance_is_checked() {
        let policy = |extra: serde_json::Value| {
            let mut v = json!({"annual_premium": 2500.0, "daily_benefit": 200.0});
            for (k, x) in extra.as_object().unwrap() {
                v[k] = x.clone();
            }
            PatchOp::set("/household/people/0/ltc_insurance", v)
        };
        let (e, w) = codes(&[policy(json!({}))]);
        assert!(
            e.is_empty() && w.iter().all(|c| !c.starts_with("ltc_")),
            "{e:?} {w:?}"
        );
        let (e, _) = codes(&[policy(json!({"annual_premium": -1.0}))]);
        assert!(e.contains(&"negative_amount".to_string()), "{e:?}");
        let (e, _) = codes(&[policy(
            json!({"inflation_rider": {"type": "compound", "rate": 3.0}}),
        )]);
        assert!(e.contains(&"out_of_range".to_string()), "{e:?}");
        let (_, w) = codes(&[policy(
            json!({"daily_benefit": 6000.0, "benefit_years": 20.0}),
        )]);
        assert!(w.contains(&"ltc_daily_benefit_high".to_string()), "{w:?}");
        assert!(w.contains(&"ltc_benefit_years_long".to_string()), "{w:?}");
        let (_, w) = codes(&[
            policy(json!({})),
            PatchOp::set("/spending/healthcare/long_term_care", json!(false)),
        ]);
        assert!(
            w.contains(&"ltc_insurance_without_care".to_string()),
            "{w:?}"
        );
    }

    #[test]
    fn income_gradient_without_earnings_and_idle_medicaid_warn() {
        let p = crate::model::Profile::example();
        let ranked = crate::longevity_links::household_income(&p).is_some();
        let (_, w) = codes(&[PatchOp::set(
            "/assumptions/longevity",
            json!({"type": "stochastic", "income_gradient": true}),
        )]);
        assert_eq!(
            w.contains(&"income_gradient_no_earnings".to_string()),
            !ranked
        );
        let (_, w) = codes(&[
            PatchOp::set("/spending/healthcare/medicaid", json!(true)),
            PatchOp::set("/spending/healthcare/long_term_care", json!(false)),
        ]);
        assert!(w.contains(&"medicaid_without_care".to_string()), "{w:?}");
        let (_, w) = codes(&[PatchOp::set("/spending/healthcare/medicaid", json!(true))]);
        assert!(!w.contains(&"medicaid_without_care".to_string()), "{w:?}");
    }
}
