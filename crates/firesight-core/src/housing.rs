//! Property and mortgages. **Interface stub: `PropertyState` methods are `todo!()`;
//! [`amortized_payment`] is implemented.**
//!
//! Responsibility: each owned property through time: value (national index times a
//! relative drift times idiosyncratic noise), the mortgage (fixed nominal payment,
//! interest/principal split, PMI until 20% equity), carrying costs, rental operations
//! with depreciation, purchase (down payment, closing costs, mortgage at the simulated
//! market rate) and sale (selling costs, gain, primary-residence exclusion,
//! depreciation recapture).
//!
//! Contract:
//!
//! * The plan has a fixed table of property slots: `profile.properties` first, then
//!   one slot per `buy_home` event in event order. A slot is inactive until bought and
//!   after sale. Slot `i` draws only from `Stream::Housing(i)`, one normal per month
//!   whether active or not.
//! * All amounts are nominal dollars. Mortgage payments are fixed in nominal terms;
//!   property tax, insurance, maintenance and HOA scale with the property's value or
//!   with CPI as documented on each field.
//! * `step` returns flows; it does not touch the accounts. `sim` pays them.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use crate::math;
use crate::model::{BuyHome, Property, PropertyUse};
use crate::rng::Rng;

/// Level monthly payment that repays `balance` over `months` at an annual `rate`
/// (compounded monthly, the US mortgage convention). Zero for an empty loan.
pub fn amortized_payment(balance: f64, rate: f64, months: u32) -> f64 {
    if balance <= 0.0 || months == 0 {
        return 0.0;
    }
    let r = rate / 12.0;
    if r.abs() < 1e-12 {
        return balance / f64::from(months);
    }
    balance * r / (1.0 - math::powi(1.0 + r, -(months as i32)))
}

/// A fixed-rate mortgage on a path.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MortgageState {
    /// Principal outstanding.
    pub balance: f64,
    /// Annual rate.
    pub rate: f64,
    /// Fixed monthly principal-and-interest payment.
    pub payment: f64,
    /// Value of the home when the loan was made, for PMI removal at 80% loan-to-value.
    pub original_value: f64,
}

/// One property slot on a path.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct PropertyState {
    /// True while the household owns the property.
    pub owned: bool,
    /// How the property is used.
    pub usage: PropertyUse,
    /// Market value.
    pub value: f64,
    /// Tax basis (purchase price plus closing costs, less depreciation taken).
    pub basis: f64,
    /// Depreciation claimed so far (recaptured on sale).
    pub depreciation_taken: f64,
    /// The mortgage, if any.
    pub mortgage: Option<MortgageState>,
    /// Month index at which the household moved in or bought, for the two-of-five-year
    /// exclusion test.
    pub owned_since_month: i32,
}

/// Compiled, time-invariant parameters of a property slot.
#[derive(Clone, Debug)]
pub struct PropertyModel {
    _private: (),
}

/// One property's flows for one month, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct HousingMonth {
    /// Mortgage interest paid (deductible for a home; an expense for a rental).
    pub interest: f64,
    /// Mortgage principal repaid.
    pub principal: f64,
    /// Private mortgage insurance.
    pub pmi: f64,
    /// Property tax.
    pub property_tax: f64,
    /// Insurance.
    pub insurance: f64,
    /// Maintenance and repairs.
    pub maintenance: f64,
    /// HOA fees.
    pub hoa: f64,
    /// Rent collected after vacancy (rentals only).
    pub rent: f64,
    /// Management fee (rentals only).
    pub management: f64,
    /// Depreciation allowed this month (rentals only; not a cash flow).
    pub depreciation: f64,
}

impl HousingMonth {
    /// Cash paid out by the owner this month.
    pub fn cash_out(&self) -> f64 {
        self.interest
            + self.principal
            + self.pmi
            + self.property_tax
            + self.insurance
            + self.maintenance
            + self.hoa
            + self.management
    }

    /// Net rental income for tax: rent less operating costs, interest and depreciation.
    pub fn rental_taxable(&self) -> f64 {
        self.rent
            - self.property_tax
            - self.insurance
            - self.maintenance
            - self.hoa
            - self.management
            - self.interest
            - self.depreciation
    }
}

/// The result of a purchase, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Purchase {
    /// Price paid.
    pub price: f64,
    /// Cash needed at closing: down payment plus closing costs.
    pub cash_needed: f64,
    /// Amount borrowed.
    pub loan: f64,
    /// Mortgage rate obtained.
    pub rate: f64,
}

/// The result of a sale, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sale {
    /// Sale price.
    pub price: f64,
    /// Cash to the seller after selling costs and paying off the mortgage.
    pub net_proceeds: f64,
    /// Taxable long-term gain after the primary-residence exclusion.
    pub taxable_gain: f64,
    /// Depreciation recaptured (taxed as ordinary income up to 25%).
    pub depreciation_recapture: f64,
    /// Gain excluded under the primary-residence rule.
    pub excluded_gain: f64,
}

impl PropertyModel {
    /// Compiles an existing property. `default_tax_rate` is the state average used
    /// when the property gives none.
    pub fn from_property(property: &Property, default_tax_rate: f64) -> PropertyModel {
        todo!("housing owner")
    }

    /// Compiles the slot a `buy_home` event will fill.
    pub fn from_purchase(event: &BuyHome, default_tax_rate: f64) -> PropertyModel {
        todo!("housing owner")
    }

    /// State at `profile.start`: owned for existing properties, empty for future
    /// purchases.
    pub fn initial_state(&self) -> PropertyState {
        todo!("housing owner")
    }

    /// Buys the property in month `t`: scales the today's-dollar price by home-price
    /// growth since the start (`home_index`), takes the mortgage at `market_rate`
    /// unless the event fixes one.
    pub fn buy(
        &self,
        state: &mut PropertyState,
        t: usize,
        home_index: f64,
        market_rate: f64,
    ) -> Purchase {
        todo!("housing owner")
    }

    /// Advances one month: appreciation, amortisation, costs, rent. Consumes exactly
    /// one normal from `rng` (the slot's `Stream::Housing`), owned or not.
    /// `home_national` is the national index change this month; `price_level` scales
    /// CPI-linked costs and rent.
    pub fn step(
        &self,
        state: &mut PropertyState,
        home_national: f64,
        price_level: f64,
        rng: &mut Rng,
    ) -> HousingMonth {
        todo!("housing owner")
    }

    /// Sells the property in month `t`. `owners` is the number of living owners for
    /// the exclusion (250,000 each).
    pub fn sell(
        &self,
        state: &mut PropertyState,
        t: usize,
        selling_cost_fraction: f64,
        owners: u32,
    ) -> Sale {
        todo!("housing owner")
    }
}

impl PropertyState {
    /// Equity: value less mortgage balance (0 when not owned).
    pub fn equity(&self) -> f64 {
        if self.owned {
            self.value - self.mortgage.map_or(0.0, |m| m.balance)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amortisation_matches_published_tables() {
        // 300,000 at 6% over 30 years: 1,798.65 a month.
        assert!((amortized_payment(300_000.0, 0.06, 360) - 1_798.65).abs() < 0.01);
        // 20,000 at 0% over 40 months.
        assert_eq!(amortized_payment(20_000.0, 0.0, 40), 500.0);
        assert_eq!(amortized_payment(0.0, 0.05, 360), 0.0);
    }

    #[test]
    fn equity_is_value_less_debt() {
        let mut p = PropertyState {
            owned: true,
            value: 400_000.0,
            ..PropertyState::default()
        };
        p.mortgage = Some(MortgageState {
            balance: 250_000.0,
            ..MortgageState::default()
        });
        assert_eq!(p.equity(), 150_000.0);
        p.owned = false;
        assert_eq!(p.equity(), 0.0);
    }
}
