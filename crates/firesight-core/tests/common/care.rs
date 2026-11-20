//! Profiles with the opt-in health, longevity and care features switched on.

use firesight_core::model::*;

/// `p` with every opt-in health, longevity and care feature on.
pub fn all_care(mut p: Profile) -> Profile {
    p.assumptions.health_model = HealthModelKind::States;
    p
}
