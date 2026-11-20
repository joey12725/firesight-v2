//! Checks of the opt-in health, longevity and care features: health states,
//! linked lifespans, the income gradient, Medicaid and long-term-care insurance.

use super::Checker;
use crate::model::*;

impl Checker<'_> {
    /// Errors: values the features are not defined for.
    pub(super) fn care_errors(&mut self) {
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
}
