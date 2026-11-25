//! Adjustable-rate mortgages.
//!
//! After `fixed_months` at the starting rate the rate resets every
//! `adjust_every_months` to `index + margin`, where the index is the simulated
//! Treasury bill yield (`EconomyPath::tbill`, a proxy for the one-year CMT and SOFR
//! indices that ARMs are written on), limited by:
//!
//! * the periodic cap: no more than `periodic_cap` up or down from the current rate
//!   (the first reset included: a 2/2/5 ARM is `periodic_cap` 0.02, `lifetime_cap`
//!   0.05);
//! * the lifetime cap: never above the starting rate plus `lifetime_cap`;
//! * the floor: never below the margin (Fannie Mae's standard ARM terms: interest
//!   rates may never decrease to less than the ARM's margin).
//!
//! The payment is then recomputed to repay the balance over the months left, so
//! the loan still ends on schedule. Typical terms (5/1 or 5/6, 2/2/5 caps, margin
//! 2.25 to 3 points) are from Fannie Mae's published ARM plans (Selling Guide
//! B2-1.4-02, and notices accepting 2/2/5 and 5/2/5 hybrid ARMs); see params.json
//! `housing_realism.arm`.

use crate::model::MortgageKind;

/// An ARM with no explicit starting rate starts this far below the 30-year fixed
/// rate. JUDGEMENT: 5/1 ARMs were typically 0.5 to 1 point below the 30-year fixed
/// rate through most of 2010 to 2019 (recalled, not re-fetched).
pub const INITIAL_DISCOUNT: f64 = 0.0075;

/// The contract terms of an ARM.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArmTerms {
    /// Months at the starting rate.
    pub fixed_months: u32,
    /// Months between resets.
    pub every: u32,
    /// Margin over the index.
    pub margin: f64,
    /// Periodic cap, up and down.
    pub periodic_cap: f64,
    /// Lifetime cap above the starting rate.
    pub lifetime_cap: f64,
}

impl ArmTerms {
    /// The terms of a mortgage kind; `None` for a fixed-rate loan.
    pub fn from_kind(kind: MortgageKind) -> Option<ArmTerms> {
        match kind {
            MortgageKind::Fixed => None,
            MortgageKind::Arm {
                fixed_months,
                adjust_every_months,
                margin,
                periodic_cap,
                lifetime_cap,
            } => Some(ArmTerms {
                fixed_months,
                every: adjust_every_months.max(1),
                margin: margin.max(0.0),
                periodic_cap: periodic_cap.max(0.0),
                lifetime_cap: lifetime_cap.max(0.0),
            }),
        }
    }
}

/// The running state of an ARM.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArmState {
    /// Contract terms.
    pub terms: ArmTerms,
    /// Months since the loan (or the plan) began.
    pub age: u32,
    /// The starting rate, for the lifetime cap.
    pub start_rate: f64,
    /// Months of payments left.
    pub remaining: u32,
}

impl ArmState {
    /// A new ARM.
    pub fn new(terms: ArmTerms, start_rate: f64, remaining: u32) -> ArmState {
        ArmState {
            terms,
            age: 0,
            start_rate,
            remaining,
        }
    }

    /// True if this month is a reset month.
    pub fn reset_due(&self) -> bool {
        self.age >= self.terms.fixed_months
            && (self.age - self.terms.fixed_months) % self.terms.every == 0
    }

    /// The rate after a reset at `index`, given the `current` rate.
    pub fn reset_rate(&self, current: f64, index: f64) -> f64 {
        let t = &self.terms;
        let fully_indexed = index + t.margin;
        let lo = (current - t.periodic_cap).max(t.margin);
        let hi = (current + t.periodic_cap).min(self.start_rate + t.lifetime_cap);
        fully_indexed.max(lo).min(hi)
    }
}
