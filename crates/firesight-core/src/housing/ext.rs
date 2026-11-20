//! Per-property state and hooks of the opt-in housing realism options (lumpy
//! maintenance, capped assessment, insurance drift, adjustable-rate mortgages).
//!
//! The simulation calls [`PropertyModel::prepare`] once a month before
//! [`PropertyModel::step`], only for properties where [`PropertyModel::has_ext`] is
//! true. `prepare` draws a fixed number of words from the slot's own extra streams
//! (whether or not the property is owned), advances the extra state and leaves its
//! results in [`PropExt`] for `step` to charge.

use super::{insurance, maintenance, ArmTerms, PropertyModel, PropertyState};
use crate::model::{Assessment, InsuranceGrowth, MaintenanceModel, MortgageKind};
use crate::rng::Rng;

/// Extra per-slot state; all zero (off) by default.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PropExt {
    /// Component ages of a lumpy-maintenance home.
    pub maint: maintenance::MaintState,
    /// The path's insurance premium index.
    pub ins: insurance::InsState,
    /// Major repairs falling in this month, nominal dollars, set by `prepare` and
    /// cleared by `step`.
    pub repair: f64,
    /// The simulated short-term index rate this month, set by the simulation before
    /// `step` for adjustable-rate mortgages.
    pub index: f64,
}

impl PropertyModel {
    /// Applies the realism options of a property or purchase.
    pub(super) fn set_realism(
        &mut self,
        maintenance: MaintenanceModel,
        assessment: Assessment,
        insurance: InsuranceGrowth,
        kind: MortgageKind,
    ) {
        self.lumpy = maintenance == MaintenanceModel::Lumpy;
        self.capped = match assessment {
            Assessment::Market => None,
            Assessment::Capped { annual_cap, .. } => Some(annual_cap.max(0.0)),
        };
        self.assessed0 = match assessment {
            Assessment::Market => None,
            Assessment::Capped { assessed_value, .. } => assessed_value,
        };
        self.ins_stochastic = insurance == InsuranceGrowth::Stochastic;
        self.arm = ArmTerms::from_kind(kind);
    }

    /// True if `prepare` has anything to do for this property.
    pub fn has_ext(&self) -> bool {
        self.lumpy || self.ins_stochastic || self.arm.is_some()
    }

    /// True if the property draws from `Stream::Maintenance`.
    pub fn has_lumpy_maintenance(&self) -> bool {
        self.lumpy
    }

    /// True if the property draws from `Stream::InsuranceDrift`.
    pub fn has_insurance_drift(&self) -> bool {
        self.ins_stochastic
    }

    /// True if the mortgage (existing or to be bought) is adjustable.
    pub fn has_arm(&self) -> bool {
        self.arm.is_some()
    }

    /// Monthly pre-step: draws and advances the extra state. `index` is the
    /// simulated short-term rate for adjustable-rate resets. Returns the insurance
    /// jump (a log increment) that happened this month, 0 if none.
    pub fn prepare(
        &self,
        state: &mut PropertyState,
        index: f64,
        rng_maint: &mut Rng,
        rng_ins: &mut Rng,
    ) -> f64 {
        state.ext.index = index;
        let mut jump = 0.0;
        if self.ins_stochastic {
            let mut w = [0u64; insurance::DRAWS_PER_MONTH];
            for x in &mut w {
                *x = rng_ins.next_u64();
            }
            jump = insurance::advance(&mut state.ext.ins, &w);
        }
        if self.lumpy {
            let mut w = [0u64; maintenance::DRAWS_PER_MONTH];
            for x in &mut w {
                *x = rng_maint.next_u64();
            }
            if state.owned {
                state.ext.repair = maintenance::advance(
                    &mut state.ext.maint,
                    self.maintenance_rate,
                    state.value,
                    &w,
                );
            }
        }
        jump
    }
}
