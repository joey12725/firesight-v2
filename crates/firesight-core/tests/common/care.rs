//! Profiles with the opt-in health, longevity and care features switched on.

use firesight_core::model::*;

/// `p` with every opt-in health, longevity and care feature on.
pub fn all_care(mut p: Profile) -> Profile {
    p.assumptions.health_model = HealthModelKind::States;
    p.spending.healthcare.medicaid = true;
    for (k, person) in p.household.people.iter_mut().enumerate() {
        person.ltc_insurance = Some(LtcInsurance {
            inflation_rider: if k == 0 {
                InflationRider::Compound { rate: 0.03 }
            } else {
                InflationRider::None
            },
            ..LtcInsurance::default()
        });
    }
    if let Longevity::Stochastic {
        couple_dependence,
        income_gradient,
        ..
    } = &mut p.assumptions.longevity
    {
        *couple_dependence = CoupleDependence::Linked;
        *income_gradient = true;
    }
    p
}
