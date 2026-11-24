//! Taxes.
//!
//! Responsibility: one household's annual US tax: federal ordinary income tax,
//! long-term gains stacked on top, NIIT, FICA and self-employment tax, state income
//! tax, credits, taxation of Social Security benefits, standard versus itemised
//! deduction, the ACA premium tax credit, IRMAA surcharges and the early-withdrawal
//! penalty. AMT is ignored. Also RMD divisors and contribution limits.
//!
//! Contract:
//!
//! * [`TaxLaw`] is the law in base-year dollars, loaded once from `data/tax/*.json`
//!   ([`TaxLaw::from_data`], in `law.rs`). Every number comes from those files or from
//!   a named constant that says where it comes from.
//! * [`TaxLaw::for_year`] takes an [`Indexation`] (calendar year plus price, wage and
//!   health-cost factors relative to the base year) and returns a plain-data
//!   [`TaxYearParams`] (no heap, `Copy`), called once each January. Each parameter
//!   follows its own statutory rule: indexed, frozen, or on a schedule by year.
//! * [`compute`] is a pure function of `(params, inputs)`; all amounts are nominal
//!   dollars of the tax year. `sim` calls it once a year for the real return, a few
//!   times in January for planning (withholding, Roth-conversion sizing), never monthly.
//!   It does not allocate (`tests/tax_no_alloc.rs` asserts this).
//! * `assumptions.taxes.kind` of `flat` or `none` is handled inside `compute` via
//!   [`TaxYearParams::kind`], so callers never branch on the tax model.
//! * [`explain`] returns the same return as labelled lines.
//!
//! What is simplified is listed in `calc.rs`.

use std::sync::OnceLock;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::data::Data;
use crate::model::TaxKind;
use crate::MAX_PEOPLE;

mod calc;
mod explain;
mod law;
#[cfg(test)]
mod tests;

pub use calc::{
    compute, compute_no_irmaa, effective_marginal_rate, irmaa_monthly_surcharge, irmaa_tier,
    room_to_aca_cliff, room_to_bracket_top, taxable_income,
};
pub use explain::explain;

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

    /// Every status, in index order.
    pub const ALL: [FilingStatus; FilingStatus::COUNT] = [
        FilingStatus::Single,
        FilingStatus::MarriedJoint,
        FilingStatus::HeadOfHousehold,
    ];

    /// Dense index for per-status tables.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The key used for this status in `data/tax/*.json`.
    pub const fn key(self) -> &'static str {
        match self {
            FilingStatus::Single => "single",
            FilingStatus::MarriedJoint => "married_joint",
            FilingStatus::HeadOfHousehold => "head_of_household",
        }
    }
}

/// A US state or DC as two ASCII upper-case letters; [`StateCode::NONE`] means no
/// state income tax applies (abroad, or state not given). Serialises as the two-letter
/// string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

impl Serialize for StateCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StateCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.is_empty() || text == "--" {
            return Ok(StateCode::NONE);
        }
        StateCode::parse(&text)
            .ok_or_else(|| serde::de::Error::custom("state must be a two-letter code or \"--\""))
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
    #[inline]
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
    #[inline]
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

    /// The same schedule with every threshold multiplied by `factor` and rounded to
    /// the nearest `increment` (see [`index_rounded`]).
    pub fn indexed_rounded(&self, factor: f64, increment: f64) -> Brackets {
        let mut b = *self;
        for l in &mut b.lower[..b.len] {
            *l = index_rounded(*l, factor, increment);
        }
        b
    }
}

/// Indexes a base-year amount and rounds it to the statute's increment.
///
/// The statutes round the *unrounded* indexed amount down to a multiple of the
/// increment. Only the already-rounded base-year amount is published, so rounding its
/// product down again would lose up to one more increment every year; rounding to the
/// nearest increment is the unbiased equivalent. A factor of exactly 1 returns the
/// published amount untouched (some published thresholds are not multiples of the
/// increment, for example the $201,775 top of the single 24% bracket).
pub fn index_rounded(base: f64, factor: f64, increment: f64) -> f64 {
    if factor == 1.0 || !base.is_finite() {
        return base;
    }
    ((base * factor) / increment + 0.5).floor() * increment
}

/// Retirement and health account limits for one year, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ContributionLimits {
    /// Employee elective deferral limit for 401(k)/403(b).
    pub k401_employee: f64,
    /// Additional catch-up deferral from age 50.
    pub k401_catch_up: f64,
    /// Catch-up for people who turn 60 to 63 in the year; replaces `k401_catch_up`.
    pub k401_catch_up_60_63: f64,
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
    /// Pay above this amount is ignored when working out plan contributions and the
    /// employer match (IRC 401(a)(17)).
    pub compensation_limit: f64,
    /// MAGI at which the deduction for a traditional IRA contribution starts to
    /// phase out for someone covered by a workplace plan, by filing status.
    pub ira_deduction_phaseout_start: [f64; FilingStatus::COUNT],
    /// MAGI at which that deduction is gone.
    pub ira_deduction_phaseout_end: [f64; FilingStatus::COUNT],
    /// The same phase-out for someone who is not covered but whose spouse is
    /// (joint returns only): `[start, end]`.
    pub ira_deduction_phaseout_spouse: [f64; 2],
}

/// Share of a traditional IRA contribution that is deductible (1.0 = all of it).
///
/// Fully deductible when neither spouse is covered by a workplace plan; otherwise it
/// phases out linearly over the MAGI range for the contributor's situation
/// (`retirement_accounts.traditional_ira_deduction_phase_out_magi`). The statutory
/// $10 rounding and $200 minimum are ignored.
pub fn ira_deductible_fraction(
    params: &TaxYearParams,
    status: FilingStatus,
    magi: f64,
    covered: bool,
    spouse_covered: bool,
) -> f64 {
    let limits = &params.federal.limits;
    let (start, end) = if covered {
        let s = status.index();
        (
            limits.ira_deduction_phaseout_start[s],
            limits.ira_deduction_phaseout_end[s],
        )
    } else if spouse_covered && status == FilingStatus::MarriedJoint {
        (
            limits.ira_deduction_phaseout_spouse[0],
            limits.ira_deduction_phaseout_spouse[1],
        )
    } else {
        return 1.0;
    };
    if magi <= start || end <= 0.0 {
        1.0
    } else if magi >= end || end <= start {
        0.0
    } else {
        (end - magi) / (end - start)
    }
}

/// Maximum number of bands in the ACA applicable-percentage table.
pub const MAX_ACA_BANDS: usize = 8;

/// ACA premium tax credit parameters for one coverage year.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct AcaYear {
    /// Number of bands in use.
    pub len: usize,
    /// Lower edge of each band, as a multiple of the poverty line.
    pub from: [f64; MAX_ACA_BANDS],
    /// Upper edge of each band.
    pub to: [f64; MAX_ACA_BANDS],
    /// Share of income the household pays at the lower edge.
    pub initial: [f64; MAX_ACA_BANDS],
    /// Share of income the household pays at the upper edge (linear in between).
    pub final_: [f64; MAX_ACA_BANDS],
    /// Income as a multiple of the poverty line above which there is no credit when
    /// `cliff` is set (4.0).
    pub max_ratio: f64,
    /// Statutory lower limit for the credit (1.0). Recorded for reporting; see
    /// `calc.rs` for how incomes below it are treated.
    pub min_ratio: f64,
    /// True when the credit stops completely above `max_ratio` (current law). False
    /// models the 2021 to 2025 enhanced credit: the last band's share applies above.
    pub cliff: bool,
}

impl AcaYear {
    /// Share of household income the household is expected to pay for the benchmark
    /// plan at `ratio` times the poverty line.
    pub fn applicable_percentage(&self, ratio: f64) -> f64 {
        if self.len == 0 {
            return 0.0;
        }
        for i in 0..self.len {
            if ratio < self.to[i] {
                let width = self.to[i] - self.from[i];
                let t = if width > 0.0 {
                    ((ratio - self.from[i]) / width).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return self.initial[i] + (self.final_[i] - self.initial[i]) * t;
            }
        }
        self.final_[self.len - 1]
    }
}

/// Maximum number of IRMAA tiers (the standard premium tier plus five surcharges).
pub const MAX_IRMAA_TIERS: usize = 6;

/// Medicare income-related premium surcharges for one filing status and year.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct IrmaaTable {
    /// Number of tiers in use; tier 0 has no surcharge.
    pub len: usize,
    /// MAGI above which each tier applies (`over[0]` is 0). The top tier starts *at*
    /// its threshold, the others just above.
    pub over: [f64; MAX_IRMAA_TIERS],
    /// Part B plus Part D surcharge per person per month in each tier.
    pub monthly: [f64; MAX_IRMAA_TIERS],
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
    /// Age from which the additional standard deduction and the senior deduction apply.
    pub senior_age: f64,
    /// Senior deduction per qualifying person (0 outside the years it is in force).
    pub senior_deduction: f64,
    /// MAGI above which the senior deduction phases out.
    pub senior_phaseout: [f64; FilingStatus::COUNT],
    /// Reduction of each person's senior deduction per dollar of MAGI above the threshold.
    pub senior_phaseout_rate: f64,
    /// Cap on the state and local tax itemised deduction (before the phase-down).
    pub salt_cap: f64,
    /// MAGI above which the SALT cap shrinks (infinite when there is no phase-down).
    pub salt_phase_down_threshold: f64,
    /// Reduction of the SALT cap per dollar of MAGI above the threshold.
    pub salt_phase_down_rate: f64,
    /// The SALT cap never falls below this.
    pub salt_floor: f64,
    /// Charitable gifts are deductible by itemisers only above this fraction of AGI.
    pub charitable_floor_rate: f64,
    /// Cash gifts deductible on top of the standard deduction, by filing status.
    pub charitable_non_itemizer: [f64; FilingStatus::COUNT],
    /// Itemised deductions lose this fraction of the part that offsets income in the
    /// top bracket (2/37).
    pub itemized_limit_fraction: f64,
    /// Net capital loss deductible against ordinary income in one year.
    pub capital_loss_limit: f64,
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
    /// Share of net self-employment profit that is subject to SE tax (0.9235).
    pub se_net_earnings_factor: f64,
    /// OASDI part of the self-employment tax rate (0.124).
    pub se_oasdi_rate: f64,
    /// Medicare part of the self-employment tax rate (0.029).
    pub se_medicare_rate: f64,
    /// Net earnings below this owe no self-employment tax.
    pub se_minimum_earnings: f64,
    /// Share of the self-employment tax deductible from income (0.5).
    pub se_deduction_fraction: f64,
    /// Qualified business income deduction rate (0.20).
    pub qbi_rate: f64,
    /// Taxable income at which the QBI deduction starts to phase out for a sole
    /// proprietor without employees.
    pub qbi_threshold: [f64; FilingStatus::COUNT],
    /// Width of the QBI phase-out range.
    pub qbi_phase_in_width: [f64; FilingStatus::COUNT],
    /// Child tax credit per child under 17.
    pub child_tax_credit: f64,
    /// Refundable part of the child tax credit, per child.
    pub child_credit_refundable_max: f64,
    /// Earned income above which the refundable part phases in.
    pub child_credit_earned_floor: f64,
    /// Rate at which the refundable part phases in with earned income (0.15).
    pub child_credit_refundable_rate: f64,
    /// Credit per other dependant.
    pub other_dependent_credit: f64,
    /// MAGI at which the child tax credit starts to phase out.
    pub child_credit_phaseout: [f64; FilingStatus::COUNT],
    /// Credit lost per $1,000 (or part) of MAGI above the threshold.
    pub child_credit_phaseout_per_1000: f64,
    /// Provisional-income thresholds above which 50% of Social Security benefits
    /// become taxable (not indexed by law).
    pub ss_taxation_base: [f64; FilingStatus::COUNT],
    /// Provisional-income thresholds above which 85% become taxable.
    pub ss_taxation_upper: [f64; FilingStatus::COUNT],
    /// Largest share of benefits that can be taxable (0.85).
    pub ss_max_taxable_fraction: f64,
    /// Penalty rate on early distributions (0.10).
    pub early_withdrawal_penalty: f64,
    /// Penalty rate on non-medical HSA withdrawals before 65 (0.20).
    pub hsa_penalty: f64,
    /// Federal poverty line for a one-person household, as used for this coverage
    /// year's ACA credit (the guideline published the year before).
    pub fpl_base: f64,
    /// Federal poverty line increment per additional person.
    pub fpl_per_person: f64,
    /// ACA premium tax credit schedule.
    pub aca: AcaYear,
    /// IRMAA surcharge tiers by filing status.
    pub irmaa: [IrmaaTable; FilingStatus::COUNT],
    /// Retirement and HSA limits.
    pub limits: ContributionLimits,
}

/// State income tax parameters for one state and year.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct StateYear {
    /// The state these parameters describe ([`StateCode::NONE`] for no state tax).
    pub code: StateCode,
    /// Brackets by filing status (head of household uses the single schedule; the
    /// data has no separate one). `len == 0` means no tax on ordinary income.
    pub brackets: [Brackets; FilingStatus::COUNT],
    /// Standard deduction by filing status.
    pub standard_deduction: [f64; FilingStatus::COUNT],
    /// Federal AGI at which the standard deduction starts to phase out linearly
    /// (0 when it does not; South Carolina).
    pub standard_deduction_phaseout_start: [f64; FilingStatus::COUNT],
    /// Federal AGI at which the standard deduction is gone.
    pub standard_deduction_phaseout_end: [f64; FilingStatus::COUNT],
    /// Credit given in place of a standard deduction (Utah), by filing status.
    pub standard_credit: [f64; FilingStatus::COUNT],
    /// State taxable income above which `standard_credit` phases out.
    pub standard_credit_phaseout_start: [f64; FilingStatus::COUNT],
    /// Credit lost per dollar of income above the phase-out start.
    pub standard_credit_phaseout_rate: f64,
    /// True when the state lets a household deduct mortgage interest, property tax
    /// and gifts instead of its standard deduction.
    pub allows_itemizing: bool,
    /// Personal exemption for the filer(s) as a deduction, by filing status (the
    /// amount for the whole return). **Changed from the stub's per-filer scalar.**
    pub personal_exemption: [f64; FilingStatus::COUNT],
    /// Personal exemption for the filer(s) as a credit against tax, by filing status.
    pub personal_credit: [f64; FilingStatus::COUNT],
    /// Exemption per dependant, as a deduction.
    pub dependent_exemption: f64,
    /// Exemption per dependant, as a credit against tax.
    pub dependent_credit: f64,
    /// Fraction of federally taxable Social Security the state also taxes before the
    /// state's own exemptions below (0 in most states).
    pub social_security_taxed: f64,
    /// Federal AGI at or below which benefits are exempt (0 when there is no such rule).
    pub ss_exempt_below_agi: [f64; FilingStatus::COUNT],
    /// Federal AGI at which the exemption has phased out linearly (0 for a cliff).
    pub ss_phase_out_end_agi: [f64; FilingStatus::COUNT],
    /// Step size of a stepped phase-out of the exemption (Minnesota; 0 when unused).
    pub ss_phase_out_step: f64,
    /// Share of the exemption lost per step.
    pub ss_phase_out_fraction_per_step: f64,
    /// At most this share of gross benefits is taxed once above the exempt income
    /// (Connecticut 0.25; 1.0 when there is no such cap).
    pub ss_max_taxable_fraction_of_benefits: f64,
    /// Age from which a person's benefits are exempt regardless of income (Colorado;
    /// 0 when unused).
    pub ss_full_exemption_age: f64,
    /// True when the income-based exemption is only for people at full retirement
    /// age (Rhode Island).
    pub ss_exemption_requires_fra: bool,
    /// Full retirement age used for `ss_exemption_requires_fra`.
    pub full_retirement_age: f64,
    /// Retirement income (traditional distributions and pensions) excluded per person
    /// at or above `retirement_exclusion_age`; infinite when the state does not tax
    /// retirement income at all.
    pub retirement_exclusion: f64,
    /// Age from which the retirement income exclusion applies (0 = any age).
    pub retirement_exclusion_age: f64,
    /// Highest rate the state charges on long-term gains where it is below the
    /// bracket schedule (negative when gains are taxed as ordinary income).
    pub capital_gains_rate: f64,
    /// Fraction of net long-term gains excluded from state income.
    pub capital_gains_exclusion_fraction: f64,
    /// Dollar amount of net long-term gains excluded from state income.
    pub capital_gains_exclusion_amount: f64,
    /// Extra rate on net capital gains when federal AGI is above
    /// `capital_gains_surtax_agi_threshold` (Maryland).
    pub capital_gains_surtax_rate: f64,
    /// Federal AGI above which the capital gains surtax applies.
    pub capital_gains_surtax_agi_threshold: f64,
    /// Extra rate on net investment income above `investment_surtax_threshold`
    /// (Minnesota).
    pub investment_surtax_rate: f64,
    /// Net investment income above which the investment surtax applies.
    pub investment_surtax_threshold: f64,
    /// Stand-alone excise on long-term gains above `capital_gains_excise_deduction`
    /// (Washington). `len == 0` when the state has none.
    pub capital_gains_excise: Brackets,
    /// Gains exempt from the excise.
    pub capital_gains_excise_deduction: f64,
    /// Local income tax as a flat share of state adjusted income (the statewide
    /// average; apply [`TaxLaw::local_tax`] for a known city).
    pub local_rate: f64,
    /// True when the local tax reaches only wages and self-employment profit (city
    /// wage, earnings and occupational taxes); false when it follows state income.
    pub local_earned_only: bool,
    /// A graduated local schedule on state taxable income by filing status (New York
    /// City; the Portland area's taxes, which start above a threshold). When it has
    /// brackets it replaces `local_rate`.
    pub local_brackets: [Brackets; FilingStatus::COUNT],
    /// True when the state taxes wages before 401(k) deferrals (Pennsylvania): the
    /// year's pre-tax deferrals are added back to state income.
    pub taxes_deferrals: bool,
    /// Average effective property tax rate, used when a property gives none.
    pub property_tax_rate: f64,
}

/// The local income tax of one place, to be laid over a state's parameters with
/// [`LocalTax::apply`].
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LocalTax {
    /// Flat rate on the base (the top rate when `brackets` apply).
    pub rate: f64,
    /// True when only wages and self-employment profit are taxed.
    pub earned_only: bool,
    /// Graduated schedule on state taxable income (empty: the flat rate applies).
    pub brackets: [Brackets; FilingStatus::COUNT],
}

impl LocalTax {
    /// Writes this local tax into a state's parameters.
    pub fn apply(&self, state: &mut StateYear) {
        state.local_rate = self.rate;
        state.local_earned_only = self.earned_only;
        state.local_brackets = self.brackets;
    }
}

/// The law for one tax year after indexing, as plain data.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TaxYearParams {
    /// Which tax system applies (`us_current_law`, `flat` or `none`).
    pub kind: TaxKind,
    /// Calendar year these parameters are for.
    pub year: i32,
    /// The CPI factor the thresholds were indexed by (1.0 in the base year, and 1.0
    /// whenever `index_brackets` is off).
    pub cpi_factor: f64,
    /// Federal parameters.
    pub federal: FederalYear,
    /// Parameters of the household's state of residence at year end.
    pub state: StateYear,
    /// Flat effective rate applied instead of the US system when the household lives
    /// abroad (`None` inside the US). Set with [`TaxYearParams::with_foreign_rate`].
    pub foreign_tax_rate: Option<f64>,
}

impl TaxYearParams {
    /// The same parameters for a household living outside the US: `compute` charges
    /// `rate` on all income and nothing else. `None` restores the US system.
    pub fn with_foreign_rate(mut self, rate: Option<f64>) -> TaxYearParams {
        self.foreign_tax_rate = rate;
        self
    }
}

/// Real growth of economy-wide wages per year, used to derive the wage index from
/// the price index when the caller has no simulated wage series. About 1% a year:
/// `data/actuarial/params.json`, `earnings.note_on_aggregate_growth`; the Social
/// Security Trustees' intermediate real-wage differential is about 1.1%.
pub const DEFAULT_REAL_WAGE_GROWTH: f64 = 0.01;

/// Where a tax year stands relative to the base year of the law.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Indexation {
    /// Calendar year of the tax return. Drives everything the statute puts on a
    /// calendar: the SALT cap schedule, the senior deduction sunset, the IRMAA
    /// top-tier freeze.
    pub year: i32,
    /// Price level relative to the base year, for everything indexed to CPI or
    /// chained CPI (brackets, standard deduction, gains thresholds, limits, IRMAA
    /// tiers). The statute uses the 12 months to August of the year before, so pass
    /// the simulated CPI at the start of the tax year (or the prior autumn) divided
    /// by the same point for the base year.
    pub cpi_factor: f64,
    /// Average wage level relative to the base year, for the OASDI wage base.
    pub wage_factor: f64,
    /// Price level for the poverty guideline, which lags one more year (coverage year
    /// Y uses the guideline published in Y-1): pass the previous year's `cpi_factor`.
    pub fpl_factor: f64,
    /// Medicare premium level relative to the base year, for IRMAA surcharge amounts
    /// (they are multiples of the Part B and Part D premiums).
    pub health_factor: f64,
}

impl Indexation {
    /// The base year itself, or any year with nothing indexed.
    pub fn base(year: i32) -> Indexation {
        Indexation {
            year,
            cpi_factor: 1.0,
            wage_factor: 1.0,
            fpl_factor: 1.0,
            health_factor: 1.0,
        }
    }

    /// Derives the other factors from the price level alone: wages grow
    /// [`DEFAULT_REAL_WAGE_GROWTH`] a year faster than prices from `base_year`, the
    /// poverty guideline and Medicare premiums follow prices.
    pub fn from_cpi(base_year: i32, year: i32, cpi_factor: f64) -> Indexation {
        Indexation {
            year,
            cpi_factor,
            wage_factor: cpi_factor
                * crate::math::powi(1.0 + DEFAULT_REAL_WAGE_GROWTH, year - base_year),
            fpl_factor: cpi_factor,
            health_factor: cpi_factor,
        }
    }

    /// Replaces the wage factor.
    pub fn with_wage_factor(mut self, wage_factor: f64) -> Indexation {
        self.wage_factor = wage_factor;
        self
    }

    /// Replaces the poverty-guideline factor.
    pub fn with_fpl_factor(mut self, fpl_factor: f64) -> Indexation {
        self.fpl_factor = fpl_factor;
        self
    }

    /// Replaces the Medicare premium factor.
    pub fn with_health_factor(mut self, health_factor: f64) -> Indexation {
        self.health_factor = health_factor;
        self
    }
}

/// IRS Uniform Lifetime Table and the RMD starting ages, from
/// `required_minimum_distributions` in the federal data.
#[derive(Clone, Debug, PartialEq)]
pub struct RmdTable {
    /// Age of the first row of `divisors`.
    pub first_age: u32,
    /// Distribution period by age from `first_age`; the last row applies to every
    /// older age.
    pub divisors: Vec<f64>,
    /// `(first birth year, last birth year, starting age)`, bounds inclusive and
    /// open when `None`.
    pub start_ages: Vec<(Option<i32>, Option<i32>, f64)>,
}

impl RmdTable {
    /// The age at which RMDs begin for someone born in `birth_year` (whole years;
    /// the historical 70 and a half is reported as 70).
    pub fn start_age(&self, birth_year: i32) -> u32 {
        for &(from, to, age) in &self.start_ages {
            if from.map_or(true, |f| birth_year >= f) && to.map_or(true, |t| birth_year <= t) {
                return age as u32;
            }
        }
        self.start_ages.last().map_or(0, |r| r.2 as u32)
    }

    /// Uniform Lifetime Table divisor for `age` (the age reached in the distribution
    /// year), or `None` before the starting age for that birth year.
    pub fn divisor(&self, age: u32, birth_year: i32) -> Option<f64> {
        if age < self.start_age(birth_year) || self.divisors.is_empty() {
            return None;
        }
        let row = (age.saturating_sub(self.first_age) as usize).min(self.divisors.len() - 1);
        Some(self.divisors[row])
    }
}

/// The tax law in base-year dollars.
#[derive(Clone, Debug)]
pub struct TaxLaw {
    /// Tax year of the base figures (2026).
    pub base_year: i32,
    /// From `assumptions.taxes.kind`.
    pub kind: TaxKind,
    /// From `assumptions.taxes.index_brackets`; when false `for_year` ignores the
    /// CPI and wage factors (statutory schedules by calendar year still apply).
    pub index_brackets: bool,
    /// The factors implied by the officially published figures for `base_year + 1`
    /// (brackets, wage base, poverty guideline, Part B premium). The simulation starts
    /// late in the base year, so the first simulated January can use these instead of
    /// a simulated price change.
    pub published_next_year: Indexation,
    /// Uniform Lifetime Table and RMD starting ages.
    pub rmd: RmdTable,
    federal: law::FederalBase,
    states: Vec<law::StateBase>,
    national_property_tax_rate: f64,
}

/// Everything the return needs, in nominal dollars for the year. Per-person arrays are
/// indexed like `household.people`; unused slots are zero.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
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
    /// Net short-term capital gains (may be negative; add last year's
    /// `capital_loss_carryforward` here as a negative amount).
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
    /// Mortgage interest paid on the primary and second home, already limited to the
    /// deductible debt ceiling by the caller.
    pub mortgage_interest: f64,
    /// Property tax paid on non-rental property.
    pub property_tax: f64,
    /// State income tax paid (for the SALT deduction). Leave 0 to have `compute` use
    /// the state and local income tax it calculates.
    pub state_tax_paid: f64,
    /// Charitable gifts.
    pub charitable: f64,
    /// Early distributions subject to the 10% penalty (traditional withdrawals and
    /// Roth earnings before 59 and a half without an exception). A penalty base
    /// only: the income itself is in `traditional_distributions` or `other_ordinary`.
    pub early_distributions: f64,
    /// Non-medical HSA withdrawals before 65. `compute` both taxes this amount as
    /// ordinary income and charges the 20% penalty on it; do not also add it to
    /// `other_ordinary`.
    pub hsa_nonqualified: f64,
    /// People covered by an ACA marketplace plan (0 if none).
    pub aca_household_size: u32,
    /// Months of the year on the marketplace plan.
    pub aca_months: u32,
    /// Annual premium of the benchmark silver plan for the covered people.
    pub aca_benchmark_premium: f64,
    /// Whether each person is on Medicare (pays IRMAA when MAGI is high).
    pub on_medicare: [bool; MAX_PEOPLE],
    /// Above-the-line deductions not taken through payroll: deductible traditional
    /// IRA contributions and HSA contributions made directly. They reduce income tax
    /// (and AGI), not FICA.
    pub adjustments: f64,
    /// Depreciation recaptured on the sale of a rental property (unrecaptured
    /// section 1250 gain): ordinary income, taxed at no more than
    /// [`UNRECAPTURED_1250_MAX_RATE`].
    pub depreciation_recapture: f64,
    /// Suspended passive rental losses released by the sale of the property: an
    /// ordinary deduction in the year of sale (IRC 469(g)).
    pub passive_loss_released: f64,
}

/// Highest federal rate on unrecaptured section 1250 gain (IRC 1(h)(1)(E)): 25%.
pub const UNRECAPTURED_1250_MAX_RATE: f64 = 0.25;

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
    /// Medicare tax on wages.
    pub fica: [f64; MAX_PEOPLE],
    /// Self-employment tax per person, including the additional Medicare tax on
    /// self-employment earnings.
    pub se_tax: [f64; MAX_PEOPLE],
    /// State income tax, including `local`.
    pub state: f64,
    /// Local income tax (already inside `state`).
    pub local: f64,
    /// Early-withdrawal and HSA penalties.
    pub penalties: f64,
    /// Non-refundable and refundable credits applied (positive number; already
    /// subtracted from `total`).
    pub credits: f64,
    /// ACA premium tax credit for the year (positive; reduces premiums, not `total`).
    pub aca_premium_credit: f64,
    /// ACA household income as a multiple of the poverty line (0 when nobody is on a
    /// marketplace plan). Below 1.0 the household would be on Medicaid in most states.
    pub aca_fpl_ratio: f64,
    /// Medicare IRMAA surcharge for a year implied by this MAGI, for everyone marked
    /// `on_medicare` (charged two years later; `sim` schedules it; not in `total`).
    pub irmaa_surcharge: f64,
    /// IRMAA tier this MAGI falls in (0 = standard premium).
    pub irmaa_tier: u32,
    /// Foreign income tax when the household lives abroad.
    pub foreign: f64,
    /// Everything owed for the year: federal after credits, NIIT, FICA, SE tax, state,
    /// penalties, foreign. Can be negative when refundable credits exceed the rest.
    pub total: f64,
    /// Federal bracket rate on one more dollar of ordinary income (0 while deductions
    /// exceed income). See [`effective_marginal_rate`] for the all-in rate.
    pub marginal_ordinary_rate: f64,
    /// Federal taxable income.
    pub taxable_income: f64,
    /// Adjusted gross income.
    pub agi: f64,
    /// Modified AGI as used for the ACA credit (AGI plus untaxed Social Security).
    pub magi: f64,
    /// Modified AGI as used for IRMAA (AGI plus tax-exempt interest, which the engine
    /// does not model, so this equals AGI).
    pub magi_irmaa: f64,
    /// Portion of Social Security benefits included in income.
    pub taxable_social_security: f64,
    /// Deduction taken (the larger of standard and itemised), excluding the senior
    /// and business-income deductions.
    pub deduction: f64,
    /// True if itemised deductions exceeded the standard deduction.
    pub itemized: bool,
    /// Net capital loss beyond this year's limit (positive). Carry it into next
    /// year's `short_term_gains` as a negative amount.
    pub capital_loss_carryforward: f64,
    /// Rental loss not allowed this year under the passive-loss limit (positive).
    /// Carry it into next year's `rental_net` as a negative amount.
    pub rental_loss_suspended: f64,
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

impl TaxDetail {
    /// The amount on the line with this key, if the return has one.
    pub fn amount(&self, key: &str) -> Option<f64> {
        self.lines.iter().find(|l| l.key == key).map(|l| l.amount)
    }
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

fn embedded_rmd() -> &'static RmdTable {
    static TABLE: OnceLock<RmdTable> = OnceLock::new();
    TABLE.get_or_init(|| {
        law::parse_rmd(&Data::embedded().tax_federal)
            .unwrap_or_else(|e| panic!("embedded data: {e}"))
    })
}

/// IRS Uniform Lifetime Table divisor for an age (RMD = prior year-end balance /
/// divisor). Returns `None` below the RMD starting age for that birth year. Reads the
/// embedded federal data; [`TaxLaw::rmd`] is the same table for a custom [`Data`].
pub fn rmd_divisor(age: u32, birth_year: i32) -> Option<f64> {
    embedded_rmd().divisor(age, birth_year)
}

/// The age at which RMDs begin for someone born in `birth_year` (73, or 75 for those
/// born in 1960 or later), from the embedded federal data.
pub fn rmd_start_age(birth_year: i32) -> u32 {
    embedded_rmd().start_age(birth_year)
}
