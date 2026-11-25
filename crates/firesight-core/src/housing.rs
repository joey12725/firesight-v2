//! Property and mortgages.
//!
//! Responsibility: each owned property through time: value (national index times a
//! relative drift times idiosyncratic noise), the mortgage (fixed nominal payment,
//! interest/principal split, PMI until enough equity), carrying costs, rental
//! operations with depreciation, purchase (down payment, closing costs, mortgage at
//! the simulated market rate) and sale (selling costs, gain, primary-residence
//! exclusion, depreciation recapture).
//!
//! Contract:
//!
//! * The plan has a fixed table of property slots: `profile.properties` first, then
//!   one slot per `buy_home` event in event order. A slot is inactive until bought and
//!   after sale. Slot `i` draws only from `Stream::Housing(i)`: three 64-bit words per
//!   month, whether owned or not.
//! * All amounts are nominal dollars. Mortgage payments are fixed in nominal terms;
//!   property tax, insurance, maintenance and HOA scale with the property's value or
//!   with CPI as documented on each field.
//! * `step` returns flows; it does not touch the accounts. `sim` pays them.
//!
//! # Value
//!
//! ```text
//! value(t+1) = value(t) * (1 + national(t)) * (1 + drift)^(1/12) * exp(s z - s^2/2)
//! sale price = value * exp(x w - x^2/2)                     drawn once, at the sale
//! ```
//!
//! Following the research (`data/actuarial/params.json`, `housing.
//! idiosyncratic_price_risk`), house-specific risk is a small local random walk
//! (`sigma_local_annual`, 4% a year) plus a one-off transaction shock (`sigma_sale`,
//! 10%), not a large annual random walk. Those are the values at the schema's default
//! `idiosyncratic_volatility`; a property's own setting scales both in proportion.
//! Both shocks are mean-preserving, so the expected value is the national index path
//! times the drift.
//!
//! # Simplifications
//!
//! * Costs are charged on the value at the start of the month. Property tax uses a
//!   value assessed every 12 months from the start (or purchase).
//! * PMI is a fixed monthly premium set at origination from the research's rate card
//!   (the 740-759 credit column), on the original loan. It stops for good in the
//!   first month the balance is at or below 78% of the original value (automatic
//!   termination) or at or below 80% of the current value (cancellation on request).
//!   For a mortgage that already exists at the start, the original value is the
//!   property's basis (or its value) and the premium is on the current balance.
//! * A property owned at the start is taken to have met the two-year ownership test.
//! * Vacancy is a two-state Markov chain with one-month spells (longer only when the
//!   configured vacancy is above 50%); the property starts let. A vacant month
//!   collects no rent and pays no management fee.
//! * Depreciation is straight line over 27.5 years on `basis * (1 - land_fraction)`,
//!   with no mid-month convention.
//! * Passive losses: a year's net rental loss is not deductible against other income
//!   (the $25,000 allowance for active participants is ignored). It is carried
//!   forward per property against that property's later rental income
//!   ([`PropertyState::close_tax_year`]) and whatever is left is released as an
//!   ordinary deduction when the property is sold ([`Sale::passive_loss_released`]).
//! * A loss on the sale of a home or second home is not deductible (taxable gain 0);
//!   a loss on a rental is reported as a negative `taxable_gain`.
//! * Refinancing is a helper ([`PropertyState::refinance`]) the simulation calls once
//!   a year; it has no closing costs. No insurance repricing above CPI, no rent
//!   sensitivity to the economy.

use serde_json::Value;

use crate::data::Data;
use crate::math;
use crate::model::{BuyHome, Property, PropertyUse};
use crate::rental_market::{
    self, RentalDraws, RentalMarketParams, RentalMarketState, RentalOutcome,
};
use crate::rng::{normal_from_bits, uniform_from_bits, Rng};

pub mod arm;
mod ext;
pub mod insurance;
pub mod maintenance;

pub use arm::{ArmState, ArmTerms};
pub use ext::PropExt;

/// Residential rental property is depreciated over 27.5 years (IRC section
/// 168(c)), i.e. 330 months.
pub const DEPRECIATION_MONTHS: f64 = 27.5 * 12.0;
/// The home-sale exclusion needs two years of ownership and use (IRC section 121;
/// `data/tax/us-federal-2026.json`, `home_sale_exclusion.ownership_and_use_test`).
pub const EXCLUSION_MIN_MONTHS: i32 = 24;
/// PMI ends automatically at 78% of the original value (Homeowners Protection Act;
/// params.json, `housing.pmi.cancellation`).
pub const PMI_AUTOMATIC_LTV: f64 = 0.78;
/// PMI can be cancelled on request at 80% loan-to-value.
pub const PMI_REQUEST_LTV: f64 = 0.80;
/// Loans above this loan-to-value carry PMI.
pub const PMI_REQUIRED_ABOVE_LTV: f64 = 0.80;
/// Column of the research's PMI rate card that is used: good credit, for which the
/// card gives 0.38% to 0.53% a year at 5 to 15% down, the research's "0.5% of the
/// loan per year" default.
const PMI_CREDIT_BAND: &str = "740-759";
/// A balance within half a cent of zero after a payment is treated as repaid.
const PAYOFF_TOLERANCE: f64 = 0.005;
/// Typical length of a vacancy between tenants, in months. Judgement: one month of
/// turnover; the frequency of vacancies is then set by the configured vacancy rate.
const VACANCY_SPELL_MONTHS: f64 = 1.0;

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

/// Typed view of the `housing` section of `data/actuarial/params.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct HousingParams {
    /// PMI rate card: `(upper loan-to-value of the band, annual premium as a
    /// fraction of the loan)`, ascending.
    pub pmi_bands: [(f64, f64); 4],
    /// Annual sd of the local price drift at the default idiosyncratic volatility.
    pub sigma_local_annual: f64,
    /// Sd of the one-off transaction shock at the default idiosyncratic volatility.
    pub sigma_sale: f64,
}

impl HousingParams {
    /// Reads the parameters from the embedded data.
    ///
    /// # Panics
    /// If `params.json` lacks one of the expected numbers (a build error: the file is
    /// embedded and covered by this module's tests).
    pub fn from_data(data: &Data) -> HousingParams {
        let h = &data.actuarial["housing"];
        let grid = &h["pmi"]["annual_rate_pct_by_ltv_and_fico"];
        let column = grid["fico_bands"]
            .as_array()
            .and_then(|bands| {
                bands
                    .iter()
                    .position(|b| b.as_str() == Some(PMI_CREDIT_BAND))
            })
            .unwrap_or_else(|| panic!("params.json: no PMI credit band {PMI_CREDIT_BAND}"));
        let band = |key: &str, upper: f64| {
            let pct = number(&grid[key][column], key);
            (upper, pct / 100.0)
        };
        let risk = &h["idiosyncratic_price_risk"];
        HousingParams {
            pmi_bands: [
                band("ltv_80.01_85", 0.85),
                band("ltv_85.01_90", 0.90),
                band("ltv_90.01_95", 0.95),
                band("ltv_95.01_97", 0.97),
            ],
            sigma_local_annual: number(&risk["sigma_local_annual"], "sigma_local_annual"),
            sigma_sale: number(&risk["sigma_sale"], "sigma_sale"),
        }
    }

    /// Annual PMI premium as a fraction of the loan for a loan-to-value at
    /// origination; zero at or below 80%. Above the last band the last rate applies.
    pub fn pmi_rate(&self, ltv: f64) -> f64 {
        if ltv <= PMI_REQUIRED_ABOVE_LTV {
            return 0.0;
        }
        for (upper, rate) in self.pmi_bands {
            if ltv <= upper {
                return rate;
            }
        }
        self.pmi_bands[3].1
    }
}

fn number(v: &Value, what: &str) -> f64 {
    v.as_f64()
        .unwrap_or_else(|| panic!("data/actuarial/params.json: housing: no number for {what}"))
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
    /// Value of the home when the loan was made, for PMI removal.
    pub original_value: f64,
    /// Monthly PMI premium; zero once PMI has been removed or if never needed.
    pub pmi_monthly: f64,
    /// Adjustable-rate terms and state; `None` for a fixed-rate loan.
    pub arm: Option<ArmState>,
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
    /// Tax basis before depreciation: purchase price plus closing costs. The
    /// adjusted basis is `basis - depreciation_taken`.
    pub basis: f64,
    /// Depreciation claimed so far (recaptured on sale).
    pub depreciation_taken: f64,
    /// The mortgage, if any. `None` once repaid.
    pub mortgage: Option<MortgageState>,
    /// Month index at which the household bought, for the two-year exclusion test
    /// (negative for a property owned before the start).
    pub owned_since_month: i32,
    /// Value on which property tax is charged, reset every 12 months.
    pub assessed_value: f64,
    /// Months since the last assessment (0 means assess this month).
    pub assessment_clock: u32,
    /// Cumulative real rent growth since `profile.start` (1.0 at start).
    pub rent_growth: f64,
    /// True while a rental stands empty.
    pub vacant: bool,
    /// The latest transaction-noise draw, as 64 random bits; turned into a normal
    /// (`rng::normal_from_bits`) if the property is sold this month.
    pub sale_noise: u64,
    /// Net rental income for tax accumulated since the last
    /// [`PropertyState::close_tax_year`].
    pub rental_year_to_date: f64,
    /// Rental losses of earlier years not yet used.
    pub passive_loss_carryforward: f64,
    /// State of the opt-in realism options (all zero when they are off).
    pub ext: PropExt,
}

/// What the slot's `buy_home` event says about the purchase.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PurchaseTerms {
    price_real: f64,
    down_payment_fraction: f64,
    closing_cost_fraction: f64,
    rate: Option<f64>,
    term_months: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RentalModel {
    rent_real: f64,
    /// Probability that a let property is empty next month.
    p_vacate: f64,
    /// Probability that an empty property is let next month.
    p_fill: f64,
    management_fee: f64,
    /// Monthly real rent growth factor.
    growth_monthly: f64,
    land_fraction: f64,
    years_in_service: f64,
}

/// Compiled, time-invariant parameters of a property slot.
#[derive(Clone, Debug)]
pub struct PropertyModel {
    usage: PropertyUse,
    /// State of a property owned at the start; `None` for a future purchase.
    existing: Option<PropertyState>,
    purchase: Option<PurchaseTerms>,
    tax_rate: f64,
    insurance_rate: f64,
    maintenance_rate: f64,
    /// The maintenance rate the household plans with (the funded test, the annual
    /// budget). Equal to `maintenance_rate` unless part of the upkeep is drawn as
    /// separate repair shocks (see [`PropertyModel::carve_out_maintenance`]): those
    /// are still an expected cost of owning the home.
    planned_maintenance_rate: f64,
    hoa_real: f64,
    /// `(1 + appreciation_vs_national)^(1/12)`.
    drift_monthly: f64,
    /// Monthly log sd of the local drift.
    sigma_local_monthly: f64,
    /// `0.5 * sigma_local_monthly^2`, the lognormal drift correction.
    half_var_local: f64,
    /// Log sd of the transaction shock.
    sigma_sale: f64,
    rental: Option<RentalModel>,
    params: HousingParams,
    /// Lumpy maintenance (see [`maintenance`]).
    lumpy: bool,
    /// Capped assessment: the largest yearly rise of the assessed value.
    capped: Option<f64>,
    /// Starting assessed value of a capped, existing property.
    assessed0: Option<f64>,
    /// Insurance premium drift (see [`insurance`]).
    ins_stochastic: bool,
    /// Adjustable-rate terms of the mortgage the slot has or will take.
    arm: Option<ArmTerms>,
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
    /// Rent the property would earn if let all month (rentals only).
    pub gross_rent: f64,
    /// Rent lost to vacancy this month (rentals only).
    pub vacancy_loss: f64,
    /// Rent collected: `gross_rent - vacancy_loss` (rentals only).
    pub rent: f64,
    /// Management fee (rentals only).
    pub management: f64,
    /// Depreciation allowed this month (rentals only; not a cash flow).
    pub depreciation: f64,
    /// Mortgage interest that counts as home mortgage interest for itemising:
    /// the interest on a primary or vacation home, zero for a rental (where it is a
    /// rental expense instead). The acquisition-debt cap is the tax module's.
    pub deductible_mortgage_interest: f64,
    /// Net rental income for tax this month, before the passive-loss rule:
    /// rent less operating costs, PMI, interest and depreciation. May be negative.
    /// Zero for a home. Use [`PropertyState::close_tax_year`] for the annual figure.
    pub rental_taxable_income: f64,
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

    /// Operating costs excluding the mortgage: tax, insurance, maintenance, HOA and
    /// management.
    pub fn operating_costs(&self) -> f64 {
        self.property_tax + self.insurance + self.maintenance + self.hoa + self.management
    }

    /// Net operating income of a rental: rent collected less operating costs,
    /// before the mortgage and depreciation.
    pub fn net_operating_income(&self) -> f64 {
        self.rent - self.operating_costs()
    }

    /// Net rental income for tax: rent less operating costs, PMI, interest and
    /// depreciation. Computed from the fields, so it is non-zero for a home too;
    /// prefer the `rental_taxable_income` field.
    pub fn rental_taxable(&self) -> f64 {
        self.net_operating_income() - self.pmi - self.interest - self.depreciation
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
    /// Down payment.
    pub down_payment: f64,
    /// Closing costs (added to the basis).
    pub closing_costs: f64,
    /// Monthly principal-and-interest payment of the new loan.
    pub payment: f64,
    /// Monthly PMI premium of the new loan (0 with 20% down or more).
    pub pmi_monthly: f64,
}

/// The result of a sale, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Sale {
    /// Sale price.
    pub price: f64,
    /// Selling costs paid out of the proceeds.
    pub selling_costs: f64,
    /// Mortgage balance repaid at closing.
    pub mortgage_payoff: f64,
    /// Cash to the seller after selling costs and paying off the mortgage. Negative
    /// if the home is under water: the seller must bring that much cash.
    pub net_proceeds: f64,
    /// Taxable long-term gain after recapture and the primary-residence exclusion.
    /// Negative only for a rental sold at a loss.
    pub taxable_gain: f64,
    /// Depreciation recaptured (unrecaptured section 1250 gain, taxed at up to 25%).
    pub depreciation_recapture: f64,
    /// Gain excluded under the primary-residence rule.
    pub excluded_gain: f64,
    /// Suspended rental losses released by the sale: deductible against ordinary
    /// income in the year of sale.
    pub passive_loss_released: f64,
}

impl PropertyModel {
    #[allow(clippy::too_many_arguments)]
    fn build(
        usage: PropertyUse,
        tax_rate: f64,
        insurance_rate: f64,
        maintenance_rate: f64,
        hoa_monthly: f64,
        appreciation_vs_national: f64,
        idiosyncratic_volatility: f64,
        data: &Data,
    ) -> PropertyModel {
        let params = HousingParams::from_data(data);
        let scale = idiosyncratic_volatility.max(0.0) / Property::DEFAULT_IDIOSYNCRATIC_VOLATILITY;
        let sigma_local_monthly = params.sigma_local_annual * scale / 12f64.sqrt();
        let rental = match usage {
            PropertyUse::Rental(terms) => {
                let vacancy = terms.vacancy.clamp(0.0, 1.0);
                let p_fill = if vacancy > 0.0 {
                    (1.0 / VACANCY_SPELL_MONTHS).min((1.0 - vacancy) / vacancy)
                } else {
                    1.0
                };
                let p_vacate = if vacancy < 1.0 {
                    (p_fill * vacancy / (1.0 - vacancy)).min(1.0)
                } else {
                    1.0
                };
                Some(RentalModel {
                    rent_real: terms.monthly_rent.max(0.0),
                    p_vacate,
                    p_fill,
                    management_fee: terms.management_fee.clamp(0.0, 1.0),
                    growth_monthly: 1.0
                        + math::annual_to_monthly(terms.rent_real_growth.max(-0.99)),
                    land_fraction: terms.land_fraction.clamp(0.0, 1.0),
                    years_in_service: terms.years_in_service.max(0.0),
                })
            }
            PropertyUse::Primary | PropertyUse::Vacation => None,
        };
        PropertyModel {
            usage,
            existing: None,
            purchase: None,
            tax_rate: tax_rate.max(0.0),
            insurance_rate: insurance_rate.max(0.0),
            maintenance_rate: maintenance_rate.max(0.0),
            planned_maintenance_rate: maintenance_rate.max(0.0),
            hoa_real: hoa_monthly.max(0.0),
            drift_monthly: 1.0 + math::annual_to_monthly(appreciation_vs_national.max(-0.99)),
            sigma_local_monthly,
            half_var_local: 0.5 * sigma_local_monthly * sigma_local_monthly,
            sigma_sale: params.sigma_sale * scale,
            rental,
            params,
            lumpy: false,
            capped: None,
            assessed0: None,
            ins_stochastic: false,
            arm: None,
        }
    }

    /// Compiles an existing property. `default_tax_rate` is the state average used
    /// when the property gives none.
    pub fn from_property(property: &Property, default_tax_rate: f64, data: &Data) -> PropertyModel {
        let mut model = PropertyModel::build(
            property.usage,
            property.property_tax_rate.unwrap_or(default_tax_rate),
            property.insurance_rate,
            property.maintenance_rate,
            property.hoa_monthly,
            property.appreciation_vs_national,
            property.idiosyncratic_volatility,
            data,
        );
        model.set_realism(
            property.maintenance,
            property.assessment,
            property.insurance_real_growth,
            property.mortgage.map(|m| m.kind).unwrap_or_default(),
        );
        let value = property.value.max(0.0);
        let basis = property.basis.unwrap_or(value).max(0.0);
        let mortgage = property.mortgage.filter(|m| m.balance > 0.0).map(|m| {
            let original_value = if basis > 0.0 { basis } else { value };
            let ltv = if original_value > 0.0 {
                m.balance / original_value
            } else {
                0.0
            };
            MortgageState {
                balance: m.balance,
                rate: m.rate,
                payment: m
                    .payment
                    .unwrap_or_else(|| amortized_payment(m.balance, m.rate, m.remaining_months)),
                original_value,
                pmi_monthly: model.params.pmi_rate(ltv) * m.balance / 12.0,
                arm: model
                    .arm
                    .map(|terms| ArmState::new(terms, m.rate, m.remaining_months)),
            }
        });
        let depreciation_taken = model.rental.map_or(0.0, |r| {
            basis
                * (1.0 - r.land_fraction)
                * (r.years_in_service * 12.0 / DEPRECIATION_MONTHS).min(1.0)
        });
        model.existing = Some(PropertyState {
            owned: true,
            usage: property.usage,
            value,
            basis,
            depreciation_taken,
            mortgage,
            owned_since_month: -EXCLUSION_MIN_MONTHS,
            assessed_value: if model.capped.is_some() {
                model.assessed0.unwrap_or(basis).max(0.0).min(value)
            } else {
                value
            },
            rent_growth: 1.0,
            ext: PropExt {
                assessed_base: model.assessed0.unwrap_or(basis).max(0.0),
                ..PropExt::default()
            },
            ..PropertyState::default()
        });
        model
    }

    /// Lowers the maintenance rate paid on an owner-occupied home by `rate` of its
    /// value a year, not below zero. Used when repairs are drawn as separate irregular
    /// expenses (`spending.shocks = "detailed"`), so that they are not paid twice.
    /// The rate the household plans with is left alone: the repairs still cost the
    /// same on average, so the funded test must keep counting them.
    pub(crate) fn carve_out_maintenance(&mut self, rate: f64) {
        if matches!(self.usage, PropertyUse::Primary) {
            self.maintenance_rate = (self.maintenance_rate - rate).max(0.0);
        }
    }

    /// Compiles the slot a `buy_home` event will fill.
    pub fn from_purchase(event: &BuyHome, default_tax_rate: f64, data: &Data) -> PropertyModel {
        let mut model = PropertyModel::build(
            event.usage,
            event.property_tax_rate.unwrap_or(default_tax_rate),
            event.insurance_rate,
            event.maintenance_rate,
            event.hoa_monthly,
            event.appreciation_vs_national,
            event.idiosyncratic_volatility,
            data,
        );
        model.set_realism(
            event.maintenance,
            event.assessment,
            event.insurance_real_growth,
            event.mortgage_kind,
        );
        model.purchase = Some(PurchaseTerms {
            price_real: event.price.max(0.0),
            down_payment_fraction: event.down_payment_fraction.clamp(0.0, 1.0),
            closing_cost_fraction: event.closing_cost_fraction.max(0.0),
            rate: event.rate,
            term_months: event.term_years * 12,
        });
        model
    }

    /// Expected cash flows of an owned property over one month at its current
    /// state, nominal dollars: `(operating costs, mortgage payment including PMI,
    /// expected rent collected net of vacancy and the management fee)`. All zero for
    /// a slot that is not owned. For planning (the funded test, the annual budget);
    /// draws nothing.
    pub fn expected_month(&self, state: &PropertyState, price_level: f64) -> (f64, f64, f64) {
        if !state.owned {
            return (0.0, 0.0, 0.0);
        }
        let taxed = if self.capped.is_some() {
            state.assessed_value
        } else {
            state.assessed_value.max(state.value)
        };
        let operating = (taxed * self.tax_rate
            + state.value * (self.insurance_rate + self.planned_maintenance_rate))
            / 12.0
            + self.hoa_real * price_level;
        let mortgage = state.mortgage.map_or(0.0, |m| {
            m.payment.min(m.balance * (1.0 + m.rate / 12.0)) + m.pmi_monthly
        });
        let rent = self.rental.as_ref().map_or(0.0, |r| {
            let occupied = if r.p_vacate + r.p_fill > 0.0 {
                r.p_fill / (r.p_vacate + r.p_fill)
            } else {
                1.0
            };
            r.rent_real * state.rent_growth * price_level * occupied * (1.0 - r.management_fee)
        });
        (operating, mortgage, rent)
    }

    /// True if the slot is (or will be) a rental.
    pub fn is_rental(&self) -> bool {
        self.rental.is_some()
    }

    /// State at `profile.start`: owned for existing properties, empty for future
    /// purchases.
    pub fn initial_state(&self) -> PropertyState {
        self.existing.unwrap_or(PropertyState {
            usage: self.usage,
            rent_growth: 1.0,
            ..PropertyState::default()
        })
    }

    /// Buys the property in month `t`: scales the today's-dollar price by home-price
    /// growth since the start (`home_index`, the cumulative national index, 1.0 at
    /// `profile.start`), takes the mortgage at `market_rate` unless the event fixes
    /// one. Does nothing (returns zeros) for a slot that is already owned or that
    /// has no purchase terms. Call it before the month's [`PropertyModel::step`] so
    /// that the first payment and the month's costs fall in the purchase month.
    pub fn buy(
        &self,
        state: &mut PropertyState,
        t: usize,
        home_index: f64,
        market_rate: f64,
    ) -> Purchase {
        let Some(terms) = self.purchase else {
            return Purchase::default();
        };
        if state.owned {
            return Purchase::default();
        }
        let price = terms.price_real * home_index;
        // Without a loan term there is nothing to amortise: a cash purchase.
        let loan = if terms.term_months == 0 {
            0.0
        } else {
            price * (1.0 - terms.down_payment_fraction)
        };
        let down_payment = price - loan;
        let closing_costs = price * terms.closing_cost_fraction;
        let rate = terms.rate.unwrap_or(if self.arm.is_some() {
            (market_rate - arm::INITIAL_DISCOUNT).max(0.0)
        } else {
            market_rate
        });
        let payment = amortized_payment(loan, rate, terms.term_months);
        let ltv = if price > 0.0 { loan / price } else { 0.0 };
        let pmi_monthly = self.params.pmi_rate(ltv) * loan / 12.0;
        *state = PropertyState {
            owned: true,
            usage: self.usage,
            value: price,
            basis: price + closing_costs,
            depreciation_taken: 0.0,
            mortgage: (loan > 0.0).then_some(MortgageState {
                balance: loan,
                rate,
                payment,
                original_value: price,
                pmi_monthly,
                arm: self.arm.map(|a| ArmState::new(a, rate, terms.term_months)),
            }),
            owned_since_month: t as i32,
            assessed_value: price,
            assessment_clock: 0,
            rent_growth: state.rent_growth,
            vacant: false,
            sale_noise: state.sale_noise,
            rental_year_to_date: 0.0,
            passive_loss_carryforward: 0.0,
            ext: PropExt {
                assessed_base: price,
                ..PropExt::default()
            },
        };
        Purchase {
            price,
            cash_needed: down_payment + closing_costs,
            loan,
            rate,
            down_payment,
            closing_costs,
            payment,
            pmi_monthly,
        }
    }

    /// Advances one month: costs, amortisation, rent, then appreciation. Consumes
    /// exactly three 64-bit words from `rng` (the slot's `Stream::Housing`),
    /// owned or not. `home_national` is the national index change this month;
    /// `price_level` scales CPI-linked costs (HOA) and rent.
    pub fn step(
        &self,
        state: &mut PropertyState,
        home_national: f64,
        price_level: f64,
        rng: &mut Rng,
    ) -> HousingMonth {
        // Three 64-bit words every month, owned or not; they become normals only
        // for a property that is owned.
        let r_local = rng.next_u64();
        let r_sale = rng.next_u64();
        let b_vacancy = rng.next_u64();
        let rent_growth = state.rent_growth;
        if let Some(r) = &self.rental {
            state.rent_growth *= r.growth_monthly;
        }
        let mut out = HousingMonth::default();
        if !state.owned {
            return out;
        }
        state.sale_noise = r_sale;

        let value = state.value;
        if state.assessment_clock == 0 {
            state.assessed_value = match self.capped {
                None => value,
                Some(cap) => {
                    // This year's capped base, or the market value if that is lower;
                    // the base goes on rising by the cap whatever the market does, so
                    // a fall in prices lowers the assessment only until they recover.
                    let base = state.ext.assessed_base;
                    state.ext.assessed_base = base * (1.0 + cap);
                    value.min(base)
                }
            };
        }
        state.assessment_clock = (state.assessment_clock + 1) % 12;
        out.property_tax = state.assessed_value * self.tax_rate / 12.0;
        out.insurance = value * self.insurance_rate / 12.0;
        if self.ins_stochastic && state.ext.ins_factor > 0.0 {
            out.insurance *= state.ext.ins_factor;
        }
        if self.lumpy {
            out.maintenance = value * self.maintenance_rate * (1.0 - maintenance::CAPITAL_SHARE)
                / 12.0
                + state.ext.repair;
            state.ext.repair = 0.0;
        } else {
            out.maintenance = value * self.maintenance_rate / 12.0;
        }
        out.hoa = self.hoa_real * price_level;

        let index_rate = state.ext.index;
        if let Some(m) = &mut state.mortgage {
            if let Some(a) = &mut m.arm {
                if a.reset_due() {
                    m.rate = a.reset_rate(m.rate, index_rate);
                    m.payment = amortized_payment(m.balance, m.rate, a.remaining.max(1));
                }
                a.age += 1;
                a.remaining = a.remaining.saturating_sub(1);
            }
            if m.pmi_monthly > 0.0 {
                if m.balance <= PMI_AUTOMATIC_LTV * m.original_value
                    || m.balance <= PMI_REQUEST_LTV * value
                {
                    m.pmi_monthly = 0.0;
                } else {
                    out.pmi = m.pmi_monthly;
                }
            }
            let interest = m.balance * m.rate / 12.0;
            let principal = if m.balance + interest <= m.payment + PAYOFF_TOLERANCE {
                m.balance
            } else {
                // A payment below the interest due pays the interest only.
                (m.payment - interest).max(0.0)
            };
            out.interest = interest;
            out.principal = principal;
            m.balance -= principal;
            if m.balance <= 0.0 {
                state.mortgage = None;
            }
        }

        if let Some(r) = &self.rental {
            let u_vacancy = uniform_from_bits(b_vacancy);
            if state.vacant {
                if u_vacancy < r.p_fill {
                    state.vacant = false;
                }
            } else if u_vacancy < r.p_vacate {
                state.vacant = true;
            }
            out.gross_rent = r.rent_real * rent_growth * price_level;
            out.vacancy_loss = if state.vacant { out.gross_rent } else { 0.0 };
            out.rent = out.gross_rent - out.vacancy_loss;
            out.management = out.rent * r.management_fee;
            let building = state.basis * (1.0 - r.land_fraction);
            out.depreciation = (building / DEPRECIATION_MONTHS)
                .min(building - state.depreciation_taken)
                .max(0.0);
            state.depreciation_taken += out.depreciation;
            out.rental_taxable_income = out.rental_taxable();
            state.rental_year_to_date += out.rental_taxable_income;
        } else {
            out.deductible_mortgage_interest = out.interest;
        }

        let mut growth = (1.0 + home_national).max(0.0) * self.drift_monthly;
        if self.sigma_local_monthly > 0.0 {
            // The word becomes a normal only here, where it is used.
            let s = self.sigma_local_monthly;
            growth *= math::exp(s * normal_from_bits(r_local) - self.half_var_local);
        }
        state.value = value * growth;
        out
    }

    /// [`PropertyModel::step`] for a rental with realistic dynamics
    /// (`rental_market.rs`): the same three words from `rng` and the same costs,
    /// mortgage and appreciation, but occupancy and the rent come from the rental's
    /// market (`market`, advanced here with this month's `draws`), part of the
    /// maintenance comes as large repairs, and evictions cost rent. For a slot that
    /// is not a rental it is `step`.
    #[allow(clippy::too_many_arguments)]
    pub fn step_realistic(
        &self,
        state: &mut PropertyState,
        home_national: f64,
        price_level: f64,
        rng: &mut Rng,
        params: &RentalMarketParams,
        market: &mut RentalMarketState,
        draws: &RentalDraws,
        recession: bool,
    ) -> (HousingMonth, RentalOutcome) {
        let rent_growth = state.rent_growth;
        let value = state.value;
        let owned = state.owned;
        let mut out = self.step(state, home_national, price_level, rng);
        let mut o = rental_market::month(params, market, draws, recession, owned);
        let Some(r) = &self.rental else {
            return (out, o);
        };
        if !owned {
            return (out, o);
        }
        state.rental_year_to_date -= out.rental_taxable_income;
        state.vacant = market.vacant;
        let trend = r.rent_real * rent_growth * price_level;
        out.gross_rent = trend * o.rent_factor;
        out.vacancy_loss = out.gross_rent * o.vacant_share;
        out.rent = out.gross_rent - out.vacancy_loss;
        out.management = out.rent * r.management_fee;
        if self.lumpy {
            // Lumpy maintenance already draws the capital repairs from the home's
            // components (`maintenance.rs`): keep them, and do not add a second
            // draw of large repairs on top.
            o.repair = false;
            out.maintenance += o.eviction_months * trend;
        } else {
            let smooth = value * self.maintenance_rate / 12.0;
            let repair = if o.repair {
                smooth * rental_market::CAPEX_SHARE / rental_market::CAPEX_MONTHLY_PROBABILITY
            } else {
                0.0
            };
            out.maintenance =
                smooth * (1.0 - rental_market::CAPEX_SHARE) + repair + o.eviction_months * trend;
        }
        out.rental_taxable_income = out.rental_taxable();
        state.rental_year_to_date += out.rental_taxable_income;
        (out, o)
    }

    /// Sells the property in month `t` at its value times the transaction shock.
    /// `exclusion` is the household's home-sale exclusion in dollars (250,000 single,
    /// 500,000 married filing jointly, from the tax data); it applies to a primary
    /// home owned for at least [`EXCLUSION_MIN_MONTHS`]. Returns zeros if the slot is
    /// not owned. The slot is empty afterwards.
    pub fn sell(
        &self,
        state: &mut PropertyState,
        t: usize,
        selling_cost_fraction: f64,
        exclusion: f64,
    ) -> Sale {
        self.sell_discounted(state, t, selling_cost_fraction, exclusion, 1.0)
    }

    /// [`PropertyModel::sell`] at `price_factor` times the price it would otherwise
    /// fetch (1 for none): a sale forced into a weak market.
    pub fn sell_discounted(
        &self,
        state: &mut PropertyState,
        t: usize,
        selling_cost_fraction: f64,
        exclusion: f64,
        price_factor: f64,
    ) -> Sale {
        if !state.owned {
            return Sale::default();
        }
        let x = self.sigma_sale;
        let mut price = if x > 0.0 {
            state.value * math::exp(x * normal_from_bits(state.sale_noise) - 0.5 * x * x)
        } else {
            state.value
        };
        if price_factor != 1.0 {
            price *= price_factor.max(0.0);
        }
        let selling_costs = price * selling_cost_fraction.max(0.0);
        let mortgage_payoff = state.mortgage.map_or(0.0, |m| m.balance);
        let adjusted_basis = state.basis - state.depreciation_taken;
        let gain = price - selling_costs - adjusted_basis;
        let depreciation_recapture = state.depreciation_taken.min(gain.max(0.0));
        let remaining = gain - depreciation_recapture;
        let is_rental = matches!(state.usage, PropertyUse::Rental(_));
        let qualifies = matches!(state.usage, PropertyUse::Primary)
            && t as i32 - state.owned_since_month >= EXCLUSION_MIN_MONTHS;
        let excluded_gain = if qualifies {
            remaining.max(0.0).min(exclusion.max(0.0))
        } else {
            0.0
        };
        let mut taxable_gain = remaining - excluded_gain;
        if !is_rental {
            taxable_gain = taxable_gain.max(0.0);
        }
        let sale = Sale {
            price,
            selling_costs,
            mortgage_payoff,
            net_proceeds: price - selling_costs - mortgage_payoff,
            taxable_gain,
            depreciation_recapture,
            excluded_gain,
            passive_loss_released: state.passive_loss_carryforward
                + (-state.rental_year_to_date).max(0.0),
        };
        *state = PropertyState {
            usage: self.usage,
            rent_growth: state.rent_growth,
            sale_noise: state.sale_noise,
            // Rental profit earned this year before the sale is still taxable.
            rental_year_to_date: state.rental_year_to_date.max(0.0),
            ..PropertyState::default()
        };
        sale
    }

    /// Buys a home into this (empty) slot at `price` with `closing_costs`, borrowing
    /// `loan` at `rate` over `term_months` (no loan if `loan` is zero): the slot keeps
    /// its own cost rates and drift. Used when the household moves to another home
    /// (planned downsizing), where the slot has no purchase terms of its own.
    #[allow(clippy::too_many_arguments)]
    pub fn buy_at(
        &self,
        state: &mut PropertyState,
        t: usize,
        price: f64,
        closing_costs: f64,
        loan: f64,
        rate: f64,
        term_months: u32,
    ) -> Purchase {
        let loan = loan.clamp(0.0, price.max(0.0));
        let payment = amortized_payment(loan, rate, term_months);
        let ltv = if price > 0.0 { loan / price } else { 0.0 };
        let pmi_monthly = self.params.pmi_rate(ltv) * loan / 12.0;
        *state = PropertyState {
            owned: true,
            usage: self.usage,
            value: price,
            basis: price + closing_costs,
            depreciation_taken: 0.0,
            mortgage: (loan > 0.0).then_some(MortgageState {
                balance: loan,
                rate,
                payment,
                original_value: price,
                pmi_monthly,
                // The caller fixes the rate: the new loan is fixed-rate.
                arm: None,
            }),
            owned_since_month: t as i32,
            assessed_value: price,
            assessment_clock: 0,
            rent_growth: state.rent_growth,
            vacant: false,
            sale_noise: state.sale_noise,
            rental_year_to_date: state.rental_year_to_date,
            passive_loss_carryforward: 0.0,
            ext: PropExt {
                assessed_base: price,
                ..PropExt::default()
            },
        };
        Purchase {
            price,
            cash_needed: price - loan + closing_costs,
            loan,
            rate,
            down_payment: price - loan,
            closing_costs,
            payment,
            pmi_monthly,
        }
    }
}

impl PropertyState {
    /// Equity: value less mortgage balance (0 when not owned).
    pub fn equity(&self) -> f64 {
        if self.owned {
            self.value - self.mortgage_balance()
        } else {
            0.0
        }
    }

    /// Mortgage principal outstanding (0 without a mortgage).
    pub fn mortgage_balance(&self) -> f64 {
        self.mortgage.map_or(0.0, |m| m.balance)
    }

    /// Pays extra principal, up to the balance. Returns the amount applied. The
    /// scheduled payment is unchanged, so the loan ends sooner.
    pub fn prepay(&mut self, amount: f64) -> f64 {
        let Some(m) = &mut self.mortgage else {
            return 0.0;
        };
        let applied = amount.max(0.0).min(m.balance);
        m.balance -= applied;
        if m.balance <= PAYOFF_TOLERANCE {
            self.mortgage = None;
        }
        applied
    }

    /// Refinances the mortgage at `market_rate` if that is at least `threshold`
    /// below the loan's rate and the interest saved over `payback_years` exceeds
    /// `closing_costs`: the balance is re-amortised at the new rate over the term
    /// the old loan had left, so the payment falls. Returns true if the loan was
    /// refinanced; the caller pays the closing costs.
    pub fn refinance(
        &mut self,
        market_rate: f64,
        threshold: f64,
        closing_costs: f64,
        payback_years: f64,
    ) -> bool {
        let Some(m) = &mut self.mortgage else {
            return false;
        };
        if !market_rate.is_finite() || market_rate < 0.0 || m.rate - market_rate < threshold {
            return false;
        }
        if (m.rate - market_rate) * m.balance * payback_years <= closing_costs {
            return false;
        }
        // Months left on the old schedule: n = -ln(1 - B r / P) / ln(1 + r).
        let r = m.rate / 12.0;
        let interest = m.balance * r;
        if m.payment <= interest || m.balance <= 0.0 {
            return false;
        }
        let months = if r > 0.0 {
            -math::ln(1.0 - interest / m.payment) / math::ln_1p(r)
        } else {
            m.balance / m.payment
        };
        let months = months.ceil().clamp(1.0, 480.0) as u32;
        // An adjustable loan keeps its contractual schedule; refinancing makes it fixed.
        let months = m.arm.map_or(months, |a| a.remaining.clamp(1, 480));
        m.arm = None;
        m.rate = market_rate;
        m.payment = amortized_payment(m.balance, market_rate, months);
        true
    }

    /// Closes the tax year for this property and returns its taxable rental income
    /// (never negative). A net loss for the year is added to the carryforward; net
    /// income is first reduced by losses carried forward. Call once a year after the
    /// December step (and after a sale in the year of sale).
    pub fn close_tax_year(&mut self) -> f64 {
        let net = self.rental_year_to_date;
        self.rental_year_to_date = 0.0;
        if net < 0.0 {
            self.passive_loss_carryforward -= net;
            0.0
        } else {
            let used = self.passive_loss_carryforward.min(net);
            self.passive_loss_carryforward -= used;
            net - used
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod realism_tests;
