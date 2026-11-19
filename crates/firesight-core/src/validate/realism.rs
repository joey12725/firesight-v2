//! Checks of the tax and policy realism options: `state_detail`, `policy_risk`,
//! `portfolio.tax_loss_harvesting` and `retirement.qcd`.

use super::Checker;
use crate::model::*;

impl Checker<'_> {
    pub(super) fn realism(&mut self) {
        self.state_detail();
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
