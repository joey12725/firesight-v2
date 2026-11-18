//! Taxes. **Interface stub: `compute` and the law loader are `todo!()`.**
//!
//! Responsibility: one household's annual US tax: federal ordinary income tax,
//! long-term gains stacked on top, NIIT, FICA and self-employment tax, state income
//! tax, credits, taxation of Social Security benefits, standard versus itemised
//! deduction, the ACA premium tax credit, IRMAA surcharges and the early-withdrawal
//! penalty. AMT is ignored. Also RMD divisors and contribution limits.
//!
//! Contract:
//!
//! * [`TaxLaw`] is the law in base-year dollars, loaded once from `data/tax/*.json`.
//! * [`TaxLaw::for_year`] indexes every inflation-indexed threshold by a CPI factor and
//!   returns a plain-data [`TaxYearParams`] (no heap, `Copy`), called once each January.
//! * [`compute`] is a pure function of `(params, inputs)`; all amounts are nominal
//!   dollars of the tax year. `sim` calls it once a year for the real return, a few
//!   times in January for planning (withholding, Roth-conversion sizing), never monthly.
//! * `assumptions.taxes.kind` of `flat` or `none` is handled inside `compute` via
//!   [`TaxYearParams::kind`], so callers never branch on the tax model.
//! * Unit tests check complete returns against published worked examples.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use serde::{Deserialize, Serialize};

use crate::data::{Data, DataError};
use crate::model::TaxKind;
use crate::MAX_PEOPLE;

/// Federal filing status. A surviving spouse files jointly for the year of death and
/// single afterwards (head of household while a child under 17 is at home).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilingStatus {
    /// Unmarried, no dependants.
    #[default]
    Single,
    /// Married filing jointly.
    MarriedJoint,
    /// Unmarried with a dependent child.
    HeadOfHousehold,
}

impl FilingStatus {
    /// Number of statuses.
    pub const COUNT: usize = 3;

    /// Dense index for per-status tables.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// A US state or DC as two ASCII upper-case letters; [`StateCode::NONE`] means no
/// state income tax applies (abroad, or state not given).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StateCode(pub [u8; 2]);

impl StateCode {
    /// No state.
    pub const NONE: StateCode = StateCode(*b"--");

    /// Parses a two-letter code (case-insensitive). Does not check it is a real state.
    pub fn parse(s: &str) -> Option<StateCode> {
        let b = s.as_bytes();
        (b.len() == 2 && b.iter().all(u8::is_ascii_alphabetic))
            .then(|| StateCode([b[0].to_ascii_uppercase(), b[1].to_ascii_uppercase()]))
    }

    /// The code as text ("--" for none).
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("--")
    }
}

impl Default for StateCode {
    fn default() -> Self {
        Self::NONE
    }
}

/// Maximum number of brackets in any schedule (Hawaii has 12).
pub const MAX_BRACKETS: usize = 12;

/// A progressive rate schedule stored inline. Bracket `i` applies to income above
/// `lower[i]` and up to `lower[i + 1]` (or without limit for the last).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Brackets {
    /// Number of brackets in use.
    pub len: usize,
    /// Lower bound of each bracket; `lower[0]` is 0.
    pub lower: [f64; MAX_BRACKETS],
    /// Marginal rate of each bracket.
    pub rate: [f64; MAX_BRACKETS],
}

impl Brackets {
    /// Builds a schedule from `(lower_bound, rate)` pairs in ascending order.
    pub fn from_pairs(pairs: &[(f64, f64)]) -> Brackets {
        assert!(pairs.len() <= MAX_BRACKETS, "too many brackets");
        let mut b = Brackets {
            len: pairs.len(),
            ..Brackets::default()
        };
        for (i, &(lower, rate)) in pairs.iter().enumerate() {
            b.lower[i] = lower;
            b.rate[i] = rate;
        }
        b
    }

    /// Tax on `income` under this schedule.
    pub fn tax(&self, income: f64) -> f64 {
        let mut tax = 0.0;
        for i in 0..self.len {
            if income <= self.lower[i] {
                break;
            }
            let upper = if i + 1 < self.len {
                self.lower[i + 1]
            } else {
                f64::INFINITY
            };
            tax += (income.min(upper) - self.lower[i]) * self.rate[i];
        }
        tax
    }

    /// Marginal rate on the next dollar above `income`.
    pub fn marginal_rate(&self, income: f64) -> f64 {
        let mut rate = 0.0;
        for i in 0..self.len {
            if income >= self.lower[i] {
                rate = self.rate[i];
            }
        }
        rate
    }

    /// Income at which the bracket taxed at `rate` ends (infinity for the top bracket
    /// or an unknown rate). Used by "fill to the top of the 12% bracket" planning.
    pub fn top_of_rate(&self, rate: f64) -> f64 {
        for i in 0..self.len {
            if (self.rate[i] - rate).abs() < 1e-9 && i + 1 < self.len {
                return self.lower[i + 1];
            }
        }
        f64::INFINITY
    }

    /// The same schedule with every threshold multiplied by `factor`.
    pub fn indexed(&self, factor: f64) -> Brackets {
        let mut b = *self;
        for l in &mut b.lower[..b.len] {
            *l *= factor;
        }
        b
    }
}

/// Retirement and health account limits for one year, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ContributionLimits {
    /// Employee elective deferral limit for 401(k)/403(b).
    pub k401_employee: f64,
    /// Additional catch-up deferral from age 50.
    pub k401_catch_up: f64,
    /// Overall limit on employee plus employer plus after-tax additions.
    pub k401_total: f64,
    /// IRA contribution limit.
    pub ira: f64,
    /// IRA catch-up from age 50.
    pub ira_catch_up: f64,
    /// HSA limit with self-only coverage.
    pub hsa_self: f64,
    /// HSA limit with family coverage.
    pub hsa_family: f64,
    /// HSA catch-up from age 55.
    pub hsa_catch_up: f64,
    /// Roth IRA eligibility phase-out start, by filing status (MAGI).
    pub roth_ira_phaseout_start: [f64; FilingStatus::COUNT],
    /// Roth IRA eligibility phase-out end, by filing status (MAGI).
    pub roth_ira_phaseout_end: [f64; FilingStatus::COUNT],
}

/// Federal parameters for one year. Fields may be added by the tax owner; existing
/// ones are relied on by `decumulation` and `sim`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct FederalYear {
    /// Ordinary income brackets by filing status.
    pub ordinary: [Brackets; FilingStatus::COUNT],
    /// Long-term gain and qualified dividend brackets (0%, 15%, 20%) by filing status,
    /// in terms of total taxable income.
    pub capital_gains: [Brackets; FilingStatus::COUNT],
    /// Standard deduction by filing status.
    pub standard_deduction: [f64; FilingStatus::COUNT],
    /// Extra standard deduction per person aged 65 or over, by filing status.
    pub additional_deduction_65: [f64; FilingStatus::COUNT],
    /// Cap on the state and local tax itemised deduction.
    pub salt_cap: f64,
    /// MAGI above which the 3.8% net investment income tax applies.
    pub niit_threshold: [f64; FilingStatus::COUNT],
    /// Net investment income tax rate.
    pub niit_rate: f64,
    /// Social Security (OASDI) wage base.
    pub oasdi_wage_base: f64,
    /// Employee OASDI rate (0.062).
    pub oasdi_rate: f64,
    /// Employee Medicare rate (0.0145).
    pub medicare_rate: f64,
    /// Additional Medicare tax rate on earnings above the threshold (0.009).
    pub additional_medicare_rate: f64,
    /// Earnings threshold for the additional Medicare tax.
    pub additional_medicare_threshold: [f64; FilingStatus::COUNT],
    /// Child tax credit per child under 17.
    pub child_tax_credit: f64,
    /// MAGI at which the child tax credit starts to phase out.
    pub child_credit_phaseout: [f64; FilingStatus::COUNT],
    /// Provisional-income thresholds above which 50% of Social Security benefits
    /// become taxable (not indexed by law).
    pub ss_taxation_base: [f64; FilingStatus::COUNT],
    /// Provisional-income thresholds above which 85% become taxable.
    pub ss_taxation_upper: [f64; FilingStatus::COUNT],
    /// Penalty rate on early distributions (0.10).
    pub early_withdrawal_penalty: f64,
    /// Federal poverty line for a one-person household (ACA credit).
    pub fpl_base: f64,
    /// Federal poverty line increment per additional person.
    pub fpl_per_person: f64,
    /// Retirement and HSA limits.
    pub limits: ContributionLimits,
}

/// State income tax parameters for one state and year.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct StateYear {
    /// The state these parameters describe ([`StateCode::NONE`] for no state tax).
    pub code: StateCode,
    /// Brackets by filing status (head of household uses the single schedule unless
    /// the state publishes one). `len == 0` means no wage income tax.
    pub brackets: [Brackets; FilingStatus::COUNT],
    /// Standard deduction by filing status.
    pub standard_deduction: [f64; FilingStatus::COUNT],
    /// Personal exemption per filer, as a deduction.
    pub personal_exemption: f64,
    /// Exemption per dependant, as a deduction.
    pub dependent_exemption: f64,
    /// Fraction of federally taxable Social Security the state also taxes (0 in most).
    pub social_security_taxed: f64,
    /// Retirement income excluded per person at or above `retirement_exclusion_age`.
    pub retirement_exclusion: f64,
    /// Age from which the retirement income exclusion applies.
    pub retirement_exclusion_age: f64,
    /// Flat rate on long-term gains where it differs from the bracket schedule
    /// (negative when gains are taxed as ordinary income).
    pub capital_gains_rate: f64,
    /// Average effective property tax rate, used when a property gives none.
    pub property_tax_rate: f64,
}

/// The law for one tax year after indexing, as plain data.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TaxYearParams {
    /// Which tax system applies (`us_current_law`, `flat` or `none`).
    pub kind: TaxKind,
    /// The CPI factor the thresholds were indexed by (1.0 in the base year).
    pub cpi_factor: f64,
    /// Federal parameters.
    pub federal: FederalYear,
    /// Parameters of the household's state of residence at year end.
    pub state: StateYear,
    /// Flat effective rate applied instead of the US system when the household lives
    /// abroad (`None` inside the US).
    pub foreign_tax_rate: Option<f64>,
}

/// The tax law in base-year dollars.
#[derive(Clone, Debug)]
pub struct TaxLaw {
    /// Tax year of the base figures (2026).
    pub base_year: i32,
    /// From `assumptions.taxes.kind`.
    pub kind: TaxKind,
    /// From `assumptions.taxes.index_brackets`; when false `for_year` ignores the
    /// CPI factor.
    pub index_brackets: bool,
    federal: FederalYear,
    states: Vec<StateYear>,
}

impl TaxLaw {
    /// Builds the typed law from the embedded JSON (`data.tax_federal`,
    /// `data.tax_states`) and the profile's tax assumptions.
    pub fn from_data(data: &Data, model: &crate::model::TaxModel) -> Result<TaxLaw, DataError> {
        todo!("tax owner: map data/tax/us-federal-2026.json and us-states-2026.json")
    }

    /// The law for a year in which prices are `cpi_factor` times the base year's, for
    /// a household resident in `state`. Thresholds that the statute does not index
    /// (NIIT, additional Medicare, Social Security taxation) stay fixed. Cheap: copies
    /// and scales a few hundred numbers, no allocation.
    pub fn for_year(&self, cpi_factor: f64, state: StateCode) -> TaxYearParams {
        todo!("tax owner")
    }

    /// Average effective property tax rate for a state (national average for
    /// [`StateCode::NONE`]).
    pub fn property_tax_rate(&self, state: StateCode) -> f64 {
        todo!("tax owner")
    }
}

/// Everything the return needs, in nominal dollars for the year. Per-person arrays are
/// indexed like `household.people`; unused slots are zero.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TaxInputs {
    /// Filing status for the year.
    pub filing_status: FilingStatus,
    /// Age of each person at year end (0 for an empty slot or a deceased person).
    pub ages: [f64; MAX_PEOPLE],
    /// Number of living adults on the return.
    pub adults: u32,
    /// Children under 17 at year end.
    pub children_under_17: u32,
    /// Other dependants (children 17 to 23 in college).
    pub other_dependents: u32,
    /// State of residence; must equal `params.state.code`.
    pub state: StateCode,
    /// Gross wages per person, including bonuses, vested RSUs and option spread,
    /// before pre-tax deferrals.
    pub wages: [f64; MAX_PEOPLE],
    /// Net self-employment profit per person.
    pub self_employment: [f64; MAX_PEOPLE],
    /// Pre-tax 401(k)/403(b)/solo-401(k) deferrals per person (reduce income tax, not
    /// FICA).
    pub pretax_deferrals: [f64; MAX_PEOPLE],
    /// HSA contributions through payroll per person (reduce income tax and FICA).
    pub hsa_contributions: [f64; MAX_PEOPLE],
    /// Taxable interest.
    pub interest: f64,
    /// Ordinary (non-qualified) dividends.
    pub ordinary_dividends: f64,
    /// Qualified dividends.
    pub qualified_dividends: f64,
    /// Net short-term capital gains (may be negative).
    pub short_term_gains: f64,
    /// Net long-term capital gains (may be negative).
    pub long_term_gains: f64,
    /// Taxable distributions from traditional accounts, including RMDs.
    pub traditional_distributions: f64,
    /// Roth conversions (taxable, never penalised).
    pub roth_conversions: f64,
    /// Taxable pension and annuity income.
    pub pensions: f64,
    /// Gross Social Security benefits per person.
    pub social_security: [f64; MAX_PEOPLE],
    /// Net rental income after expenses and depreciation (may be negative).
    pub rental_net: f64,
    /// Other ordinary income (taxable windfalls, taxable "other" incomes).
    pub other_ordinary: f64,
    /// Mortgage interest paid on the primary and second home.
    pub mortgage_interest: f64,
    /// Property tax paid on non-rental property.
    pub property_tax: f64,
    /// State income tax paid (for the SALT deduction). Leave 0 to have `compute` use
    /// the state tax it calculates.
    pub state_tax_paid: f64,
    /// Charitable gifts.
    pub charitable: f64,
    /// Early distributions subject to the 10% penalty (traditional withdrawals and
    /// Roth earnings before 59 and a half without an exception).
    pub early_distributions: f64,
    /// Non-medical HSA withdrawals before 65 (taxable plus 20% penalty).
    pub hsa_nonqualified: f64,
    /// People covered by an ACA marketplace plan (0 if none).
    pub aca_household_size: u32,
    /// Months of the year on the marketplace plan.
    pub aca_months: u32,
    /// Annual premium of the benchmark silver plan for the covered people.
    pub aca_benchmark_premium: f64,
    /// Whether each person is on Medicare (pays IRMAA when MAGI is high).
    pub on_medicare: [bool; MAX_PEOPLE],
}

/// The computed return, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize)]
pub struct TaxResult {
    /// Federal tax on ordinary income.
    pub federal_ordinary: f64,
    /// Federal tax on long-term gains and qualified dividends.
    pub federal_capital_gains: f64,
    /// Net investment income tax.
    pub niit: f64,
    /// Employee Social Security and Medicare tax per person, including the additional
    /// Medicare tax.
    pub fica: [f64; MAX_PEOPLE],
    /// Self-employment tax per person.
    pub se_tax: [f64; MAX_PEOPLE],
    /// State income tax.
    pub state: f64,
    /// Early-withdrawal and HSA penalties.
    pub penalties: f64,
    /// Non-refundable and refundable credits applied (positive number; already
    /// subtracted from `total`).
    pub credits: f64,
    /// ACA premium tax credit for the year (positive; reduces premiums, not `total`).
    pub aca_premium_credit: f64,
    /// Medicare IRMAA surcharge for the year implied by this MAGI (charged two years
    /// later; `sim` schedules it; not in `total`).
    pub irmaa_surcharge: f64,
    /// Foreign income tax when the household lives abroad.
    pub foreign: f64,
    /// Everything owed for the year: federal after credits, NIIT, FICA, SE tax, state,
    /// penalties, foreign.
    pub total: f64,
    /// Federal marginal rate on one more dollar of ordinary income.
    pub marginal_ordinary_rate: f64,
    /// Federal taxable income.
    pub taxable_income: f64,
    /// Adjusted gross income.
    pub agi: f64,
    /// Modified AGI as used for ACA and IRMAA (AGI plus untaxed Social Security).
    pub magi: f64,
    /// Portion of Social Security benefits included in income.
    pub taxable_social_security: f64,
    /// Deduction taken (the larger of standard and itemised).
    pub deduction: f64,
    /// True if itemised deductions exceeded the standard deduction.
    pub itemized: bool,
}

/// Computes one household's tax for one year. Pure and allocation-free.
pub fn compute(params: &TaxYearParams, inputs: &TaxInputs) -> TaxResult {
    todo!("tax owner")
}

/// A line-by-line explanation of a return for the UI ("explain my tax") and for
/// `PathTrace`.
#[derive(Clone, Debug, PartialEq, Default, Serialize)]
pub struct TaxDetail {
    /// The result being explained.
    pub result: TaxResult,
    /// Form-like lines in order: label, amount, and a short note on how it was derived.
    pub lines: Vec<TaxLine>,
}

/// One line of a [`TaxDetail`].
#[derive(Clone, Debug, PartialEq, Default, Serialize)]
pub struct TaxLine {
    /// Stable key, for example "taxable_income".
    pub key: String,
    /// Human label, for example "Taxable income".
    pub label: String,
    /// Amount in nominal dollars.
    pub amount: f64,
    /// How it was derived ("AGI 142,300 less standard deduction 32,200").
    pub note: String,
}

/// Like [`compute`], but also returns the working. Allocates; never used in the loop.
pub fn explain(params: &TaxYearParams, inputs: &TaxInputs) -> TaxDetail {
    todo!("tax owner")
}

/// IRS Uniform Lifetime Table divisor for an age (RMD = prior year-end balance /
/// divisor). Returns `None` below the RMD starting age for that birth year.
pub fn rmd_divisor(age: u32, birth_year: i32) -> Option<f64> {
    todo!("tax owner")
}

/// The age at which RMDs begin for someone born in `birth_year` (73, or 75 for those
/// born in 1960 or later).
pub fn rmd_start_age(birth_year: i32) -> u32 {
    if birth_year >= 1960 {
        75
    } else {
        73
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brackets_tax_and_marginal_rate() {
        let b = Brackets::from_pairs(&[(0.0, 0.10), (10_000.0, 0.20), (50_000.0, 0.30)]);
        assert_eq!(b.tax(0.0), 0.0);
        assert_eq!(b.tax(10_000.0), 1_000.0);
        assert_eq!(b.tax(60_000.0), 1_000.0 + 8_000.0 + 3_000.0);
        assert_eq!(b.marginal_rate(9_999.0), 0.10);
        assert_eq!(b.marginal_rate(10_000.0), 0.20);
        assert_eq!(b.top_of_rate(0.20), 50_000.0);
        assert_eq!(b.top_of_rate(0.30), f64::INFINITY);
        let i = b.indexed(2.0);
        assert_eq!(i.tax(20_000.0), 2_000.0);
    }

    #[test]
    fn state_codes() {
        assert_eq!(StateCode::parse("co").unwrap().as_str(), "CO");
        assert!(StateCode::parse("Colorado").is_none());
        assert_eq!(StateCode::NONE.as_str(), "--");
        assert_eq!(rmd_start_age(1955), 73);
        assert_eq!(rmd_start_age(1989), 75);
    }
}
