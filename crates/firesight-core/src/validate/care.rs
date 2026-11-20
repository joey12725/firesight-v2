//! Checks of the opt-in health, longevity and care features: health states,
//! linked lifespans, the income gradient, Medicaid and long-term-care insurance.

use super::Checker;
use crate::model::*;

impl Checker<'_> {
    /// Errors: values the features are not defined for.
    pub(super) fn care_errors(&mut self) {}

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
}
