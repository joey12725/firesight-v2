//! Loading the law from `data/tax/*.json` and indexing it to a year.
//!
//! `from_data` reads every value the engine uses and fails with the file and JSON path
//! of the first key that is missing or malformed. `for_year` copies the base-year
//! parameters and applies each one's indexing rule; it does not allocate.

use serde_json::Value;

use super::{
    index_rounded, AcaYear, Brackets, ContributionLimits, FederalYear, FilingStatus, Indexation,
    IrmaaTable, RmdTable, StateCode, StateYear, TaxLaw, TaxYearParams, MAX_ACA_BANDS, MAX_BRACKETS,
    MAX_IRMAA_TIERS,
};
use crate::data::{Data, DataError};
use crate::model::TaxModel;

const FEDERAL_FILE: &str = "data/tax/us-federal-2026.json";
const STATES_FILE: &str = "data/tax/us-states-2026.json";

// Rounding increments of the inflation adjustments. Source: the `rounding` and
// `indexing` notes next to each block in us-federal-2026.json.

/// Brackets, standard deduction, capital-gain thresholds, QBI thresholds
/// (`income_tax.rounding`: multiples of $50).
const BRACKET_INCREMENT: f64 = 50.0;
/// Child tax credit amounts (`credits.child_tax_credit.indexing`: $100).
const CHILD_CREDIT_INCREMENT: f64 = 100.0;
/// 401(k) deferral and catch-up limits (`retirement_accounts...indexing`: $500).
const K401_INCREMENT: f64 = 500.0;
/// 415(c) total additions limit ($1,000 increments).
const K401_TOTAL_INCREMENT: f64 = 1_000.0;
/// IRA contribution limit ($500 increments).
const IRA_INCREMENT: f64 = 500.0;
/// IRA catch-up ($100 increments).
const IRA_CATCH_UP_INCREMENT: f64 = 100.0;
/// HSA contribution limits ($50 increments).
const HSA_INCREMENT: f64 = 50.0;
/// Roth IRA phase-out range ($1,000 increments).
const ROTH_PHASEOUT_INCREMENT: f64 = 1_000.0;
/// The 401(a)(17) compensation limit moves in $5,000 steps
/// (`retirement_accounts.employer_plans_401k_403b_457b_tsp.indexing`).
const COMPENSATION_INCREMENT: f64 = 5_000.0;
/// OASDI wage base (`fica.oasdi_wage_base_rounding`: nearest $300).
const WAGE_BASE_INCREMENT: f64 = 300.0;
/// IRMAA tier thresholds: nearest $1,000 (42 USC 1395r(i)(5)); the data file states
/// the CPI-U indexing but not the rounding.
const IRMAA_INCREMENT: f64 = 1_000.0;
/// Last year in which the top IRMAA tier threshold is frozen
/// (`medicare.irmaa.indexing.top_tier_threshold`: "frozen ... through 2027").
const IRMAA_TOP_TIER_FROZEN_THROUGH: i32 = 2027;
/// Rate at which the refundable child credit phases in with earned income. The data
/// file gives it only in words (`credits.child_tax_credit.refundable_formula`:
/// "15% of earned income over $2,500"); IRC 24(d)(1)(B)(i).
const CHILD_CREDIT_REFUNDABLE_RATE: f64 = 0.15;
/// Utah's taxpayer credit loses 1.3 cents per dollar of income above these amounts
/// (single, joint, head of household as single). Source: the `notes` of `UT` in
/// us-states-2026.json (2025 thresholds); the data file has no structured field for
/// them yet.
const STANDARD_CREDIT_PHASEOUT_RATE: f64 = 0.013;
const STANDARD_CREDIT_PHASEOUT_START: [f64; FilingStatus::COUNT] = [18_213.0, 36_426.0, 18_213.0];

/// A JSON value together with where it sits, for error messages.
struct Node<'a> {
    value: &'a Value,
    file: &'static str,
    path: String,
}

impl<'a> Node<'a> {
    fn root(value: &'a Value, file: &'static str) -> Node<'a> {
        Node {
            value,
            file,
            path: String::new(),
        }
    }

    fn err(&self, message: &str) -> DataError {
        DataError {
            file: self.file,
            message: format!("{}: {message}", self.path.trim_start_matches('.')),
        }
    }

    fn child(&self, key: &str, value: &'a Value) -> Node<'a> {
        Node {
            value,
            file: self.file,
            path: format!("{}.{key}", self.path),
        }
    }

    /// A required key.
    fn get(&self, key: &str) -> Result<Node<'a>, DataError> {
        match self.value.get(key) {
            Some(v) if !v.is_null() => Ok(self.child(key, v)),
            _ => Err(self.child(key, &Value::Null).err("missing")),
        }
    }

    /// An optional key (absent or null gives `None`).
    fn opt(&self, key: &str) -> Option<Node<'a>> {
        self.value
            .get(key)
            .filter(|v| !v.is_null())
            .map(|v| self.child(key, v))
    }

    fn num(&self) -> Result<f64, DataError> {
        self.value
            .as_f64()
            .filter(|x| x.is_finite())
            .ok_or_else(|| self.err("expected a number"))
    }

    fn amount(&self) -> Result<f64, DataError> {
        let x = self.num()?;
        if x < 0.0 {
            return Err(self.err("expected a non-negative amount"));
        }
        Ok(x)
    }

    fn rate(&self) -> Result<f64, DataError> {
        let x = self.num()?;
        if !(0.0..=1.0).contains(&x) {
            return Err(self.err("expected a rate between 0 and 1"));
        }
        Ok(x)
    }

    fn boolean(&self) -> Result<bool, DataError> {
        self.value
            .as_bool()
            .ok_or_else(|| self.err("expected true or false"))
    }

    fn text(&self) -> Result<&'a str, DataError> {
        self.value
            .as_str()
            .ok_or_else(|| self.err("expected a string"))
    }

    fn items(&self) -> Result<Vec<Node<'a>>, DataError> {
        let list = self
            .value
            .as_array()
            .ok_or_else(|| self.err("expected a list"))?;
        Ok(list
            .iter()
            .enumerate()
            .map(|(i, v)| Node {
                value: v,
                file: self.file,
                path: format!("{}[{i}]", self.path),
            })
            .collect())
    }

    fn get_amount(&self, key: &str) -> Result<f64, DataError> {
        self.get(key)?.amount()
    }

    fn get_rate(&self, key: &str) -> Result<f64, DataError> {
        self.get(key)?.rate()
    }

    fn get_year(&self, key: &str) -> Result<i32, DataError> {
        let node = self.get(key)?;
        let x = node.num()?;
        if x.fract() != 0.0 || !(1900.0..=2200.0).contains(&x) {
            return Err(node.err("expected a calendar year"));
        }
        Ok(x as i32)
    }

    /// `{single, married_joint, head_of_household}` amounts. When the block has no
    /// head-of-household entry and `hoh_as_single` is set, single is used.
    fn by_status(&self, hoh_as_single: bool) -> Result<[f64; FilingStatus::COUNT], DataError> {
        let mut out = [0.0; FilingStatus::COUNT];
        for status in FilingStatus::ALL {
            out[status.index()] = if status == FilingStatus::HeadOfHousehold && hoh_as_single {
                match self.opt(status.key()) {
                    Some(node) => node.amount()?,
                    None => self.get_amount(FilingStatus::Single.key())?,
                }
            } else {
                self.get_amount(status.key())?
            };
        }
        Ok(out)
    }
}

/// Reads `[{up_to, rate}]` (federal convention: inclusive upper bounds, null for the
/// last) into a schedule of lower bounds.
fn brackets_up_to(node: &Node) -> Result<Brackets, DataError> {
    let rows = node.items()?;
    if rows.is_empty() || rows.len() > MAX_BRACKETS {
        return Err(node.err("expected 1 to 12 brackets"));
    }
    let mut pairs = [(0.0, 0.0); MAX_BRACKETS];
    let mut lower = 0.0;
    for (i, row) in rows.iter().enumerate() {
        pairs[i] = (lower, row.get_rate("rate")?);
        match row.opt("up_to") {
            Some(up_to) => {
                let upper = up_to.amount()?;
                if upper <= lower {
                    return Err(up_to.err("bracket bounds must ascend"));
                }
                lower = upper;
            }
            None if i + 1 == rows.len() => {}
            None => return Err(row.err("only the last bracket may be open-ended")),
        }
    }
    if rows.last().is_some_and(|r| r.opt("up_to").is_some()) {
        return Err(node.err("the last bracket must be open-ended (up_to null)"));
    }
    Ok(Brackets::from_pairs(&pairs[..rows.len()]))
}

/// Reads `[{over, rate}]` (state convention: lower bounds). An empty list is a
/// schedule with no brackets.
fn brackets_over(node: &Node) -> Result<Brackets, DataError> {
    let rows = node.items()?;
    if rows.len() > MAX_BRACKETS {
        return Err(node.err("more than 12 brackets"));
    }
    let mut pairs = [(0.0, 0.0); MAX_BRACKETS];
    for (i, row) in rows.iter().enumerate() {
        let over = row.get_amount("over")?;
        if (i == 0 && over != 0.0) || (i > 0 && over <= pairs[i - 1].0) {
            return Err(row.err("brackets must start at 0 and ascend"));
        }
        pairs[i] = (over, row.get_rate("rate")?);
    }
    Ok(Brackets::from_pairs(&pairs[..rows.len()]))
}

fn brackets_by_status(node: &Node) -> Result<[Brackets; FilingStatus::COUNT], DataError> {
    let mut out = [Brackets::default(); FilingStatus::COUNT];
    for status in FilingStatus::ALL {
        out[status.index()] = brackets_up_to(&node.get(status.key())?)?;
    }
    Ok(out)
}

/// One year of the SALT cap schedule.
#[derive(Clone, Copy, Debug)]
struct SaltRow {
    year: i32,
    cap: f64,
    /// Infinite when the cap has no phase-down that year.
    threshold: f64,
}

/// Federal law in base-year dollars plus the rules that depend on the calendar.
#[derive(Clone, Debug)]
pub(super) struct FederalBase {
    /// Base-year values of every parameter.
    year: FederalYear,
    salt_schedule: Vec<SaltRow>,
    senior_first_year: i32,
    senior_last_year: i32,
    /// `(first person, each additional)` poverty guidelines for Alaska and Hawaii.
    fpl_alaska: (f64, f64),
    fpl_hawaii: (f64, f64),
    /// Whether the 400% cliff applies, by coverage year; the last entry persists.
    aca_cliff: Vec<(i32, bool)>,
}

/// One state in base-year dollars.
#[derive(Clone, Debug)]
pub(super) struct StateBase {
    year: StateYear,
    brackets_indexed: bool,
    standard_deduction_indexed: bool,
    standard_deduction_follows_federal: bool,
    major_city: Option<String>,
    major_city_rate: f64,
}

/// Reads the Uniform Lifetime Table and RMD starting ages.
pub(super) fn parse_rmd(federal: &Value) -> Result<RmdTable, DataError> {
    let root = Node::root(federal, FEDERAL_FILE);
    let block = root.get("required_minimum_distributions")?;
    let table = block.get("uniform_lifetime_table")?;
    let rows = table.items()?;
    let mut divisors = Vec::with_capacity(rows.len());
    let mut first_age = 0;
    for (i, row) in rows.iter().enumerate() {
        let age = row.get_amount("age")? as u32;
        if i == 0 {
            first_age = age;
        } else if age != first_age + i as u32 {
            return Err(row.err("ages must be consecutive"));
        }
        let divisor = row.get_amount("divisor")?;
        if divisor <= 0.0 {
            return Err(row.err("divisor must be positive"));
        }
        divisors.push(divisor);
    }
    if divisors.is_empty() {
        return Err(table.err("empty table"));
    }
    let mut start_ages = Vec::new();
    for row in block.get("start_age_by_birth_year")?.items()? {
        let bound = |key: &str| -> Result<Option<i32>, DataError> {
            row.opt(key).map(|n| n.num().map(|x| x as i32)).transpose()
        };
        start_ages.push((
            bound("birth_year_from")?,
            bound("birth_year_to")?,
            row.get_amount("age")?,
        ));
    }
    if start_ages.is_empty() {
        return Err(block.err("start_age_by_birth_year is empty"));
    }
    Ok(RmdTable {
        first_age,
        divisors,
        start_ages,
    })
}

fn parse_irmaa(node: &Node) -> Result<IrmaaTable, DataError> {
    let rows = node.items()?;
    if rows.is_empty() || rows.len() > MAX_IRMAA_TIERS {
        return Err(node.err("expected 1 to 6 IRMAA tiers"));
    }
    let mut table = IrmaaTable {
        len: rows.len(),
        ..IrmaaTable::default()
    };
    for (i, row) in rows.iter().enumerate() {
        table.over[i] = row.get_amount("magi_over")?;
        if (i == 0 && table.over[i] != 0.0) || (i > 0 && table.over[i] <= table.over[i - 1]) {
            return Err(row.err("tiers must start at 0 and ascend"));
        }
        table.monthly[i] = row.get_amount("part_b_surcharge_monthly")?
            + row.get_amount("part_d_surcharge_monthly")?;
    }
    Ok(table)
}

fn parse_aca_table(node: &Node) -> Result<AcaYear, DataError> {
    let rows = node.items()?;
    if rows.is_empty() || rows.len() > MAX_ACA_BANDS {
        return Err(node.err("expected 1 to 8 bands"));
    }
    let mut aca = AcaYear {
        len: rows.len(),
        ..AcaYear::default()
    };
    for (i, row) in rows.iter().enumerate() {
        aca.from[i] = row.get_amount("fpl_from")?;
        aca.to[i] = row.get_amount("fpl_to")?;
        aca.initial[i] = row.get_rate("initial")?;
        aca.final_[i] = row.get_rate("final")?;
        if aca.to[i] <= aca.from[i] || (i > 0 && aca.from[i] != aca.to[i - 1]) {
            return Err(row.err("bands must be contiguous and ascending"));
        }
    }
    Ok(aca)
}

fn parse_fraction(node: &Node) -> Result<f64, DataError> {
    if let Some(x) = node.value.as_f64() {
        return Ok(x);
    }
    let text = node.text()?;
    let parsed = text.split_once('/').and_then(|(a, b)| {
        let (a, b) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?);
        (b != 0.0).then_some(a / b)
    });
    parsed.ok_or_else(|| node.err("expected a fraction such as \"2/37\""))
}

fn parse_limits(root: &Node) -> Result<ContributionLimits, DataError> {
    let accounts = root.get("retirement_accounts")?;
    let plans = accounts.get("employer_plans_401k_403b_457b_tsp")?;
    let ira = accounts.get("ira")?;
    let hsa = accounts.get("hsa")?;
    let roth = accounts.get("roth_ira_contribution_phase_out_magi")?;
    let mut limits = ContributionLimits {
        k401_employee: plans.get_amount("elective_deferral_limit")?,
        k401_catch_up: plans.get_amount("catch_up_age_50_plus")?,
        k401_catch_up_60_63: plans.get_amount("super_catch_up_ages_60_to_63")?,
        k401_total: plans.get_amount("total_annual_additions_415c")?,
        ira: ira.get_amount("contribution_limit")?,
        ira_catch_up: ira.get_amount("catch_up_age_50_plus")?,
        hsa_self: hsa.get_amount("contribution_limit_self_only")?,
        hsa_family: hsa.get_amount("contribution_limit_family")?,
        hsa_catch_up: hsa.get_amount("catch_up_age_55_plus")?,
        ..ContributionLimits::default()
    };
    for status in FilingStatus::ALL {
        let range = roth.get(status.key())?;
        limits.roth_ira_phaseout_start[status.index()] = range.get_amount("start")?;
        limits.roth_ira_phaseout_end[status.index()] = range.get_amount("end")?;
    }
    limits.compensation_limit = plans.get_amount("compensation_limit_401a17")?;
    let deduction = accounts.get("traditional_ira_deduction_phase_out_magi")?;
    let covered = deduction.get("covered_by_workplace_plan")?;
    for status in FilingStatus::ALL {
        let range = covered.get(status.key())?;
        limits.ira_deduction_phaseout_start[status.index()] = range.get_amount("start")?;
        limits.ira_deduction_phaseout_end[status.index()] = range.get_amount("end")?;
    }
    let spouse = deduction
        .get("not_covered_but_spouse_covered")?
        .get(FilingStatus::MarriedJoint.key())?;
    limits.ira_deduction_phaseout_spouse = [spouse.get_amount("start")?, spouse.get_amount("end")?];
    Ok(limits)
}

fn parse_federal(root: &Node, base_year: i32) -> Result<FederalBase, DataError> {
    let income = root.get("income_tax")?;
    let niit = root.get("net_investment_income_tax")?;
    let additional = root.get("additional_medicare_tax")?;
    let fica = root.get("fica")?;
    let se = fica.get("self_employment")?;
    let ctc = root.get("credits")?.get("child_tax_credit")?;
    let senior = root.get("senior_deduction")?;
    let itemized = root.get("itemized_deductions")?;
    let salt = itemized.get("salt_cap")?;
    let charitable = itemized.get("charitable")?;
    let early = root.get("early_withdrawal")?;
    let benefit_tax = root.get("social_security")?.get("benefit_taxation")?;
    let qbi = root.get("qualified_business_income_deduction")?;
    let aca = root.get("aca")?;
    let credit = aca.get("premium_tax_credit")?;
    let poverty = aca.get("federal_poverty_guidelines")?;
    let irmaa = root.get("medicare")?.get("irmaa")?;

    // SALT cap schedule.
    let mut salt_schedule = Vec::new();
    for row in salt.get("schedule")?.items()? {
        salt_schedule.push(SaltRow {
            year: row.get_year("year")?,
            cap: row.get_amount("cap")?,
            threshold: match row.opt("phase_down_magi_threshold") {
                Some(t) => t.amount()?,
                None => f64::INFINITY,
            },
        });
    }
    if salt_schedule.is_empty() || salt_schedule.windows(2).any(|w| w[0].year >= w[1].year) {
        return Err(salt.err("schedule must be non-empty and ascending by year"));
    }

    // Poverty guideline: coverage year Y uses the guideline published in Y - 1.
    let guideline_year = base_year - 1;
    let guideline = poverty.get(&guideline_year.to_string())?;
    if guideline.get_year("used_for_coverage_year")? != base_year {
        return Err(guideline.err("guideline is not the one used for the base coverage year"));
    }
    let regional = |name: &str| -> Result<(f64, f64), DataError> {
        let block = poverty.get(&format!("{name}_{guideline_year}"))?;
        Ok((
            block.get_amount("first_person")?,
            block.get_amount("each_additional")?,
        ))
    };

    // ACA table and the cliff flags.
    let mut aca_year =
        parse_aca_table(&credit.get(&format!("applicable_percentage_table_{base_year}"))?)?;
    aca_year.min_ratio = credit.get_amount("eligibility_min_pct_fpl")?;
    aca_year.max_ratio = credit.get_amount("eligibility_max_pct_fpl")?;
    let status = aca.get("enhanced_subsidies_status")?;
    let mut aca_cliff = vec![(
        base_year,
        status
            .get(&format!("cliff_at_400_pct_fpl_applies_{base_year}"))?
            .boolean()?,
    )];
    let mut year = base_year + 1;
    while let Some(flag) = status.opt(&format!("cliff_at_400_pct_fpl_applies_{year}")) {
        aca_cliff.push((year, flag.boolean()?));
        year += 1;
    }
    aca_year.cliff = aca_cliff[0].1;

    // IRMAA: head of household pays as single.
    let irmaa_single = parse_irmaa(&irmaa.get(FilingStatus::Single.key())?)?;
    let irmaa_joint = parse_irmaa(&irmaa.get(FilingStatus::MarriedJoint.key())?)?;

    let ordinary = brackets_by_status(&income.get("ordinary_brackets")?)?;
    let capital_gains = brackets_by_status(&income.get("long_term_capital_gains_brackets")?)?;
    for b in &capital_gains {
        if b.rate[0] != 0.0 {
            return Err(income.err("long_term_capital_gains_brackets must start with a 0% band"));
        }
    }

    let base_salt = salt_row(&salt_schedule, base_year);
    let year = FederalYear {
        ordinary,
        capital_gains,
        standard_deduction: income.get("standard_deduction")?.by_status(false)?,
        additional_deduction_65: income
            .get("additional_standard_deduction_65_or_blind")?
            .get("by_status")?
            .by_status(false)?,
        senior_age: senior.get_amount("qualifying_age")?,
        senior_deduction: senior.get_amount("amount_per_qualifying_person")?,
        senior_phaseout: senior.get("phase_out_magi_threshold")?.by_status(false)?,
        senior_phaseout_rate: senior.get_rate("phase_out_rate")?,
        salt_cap: base_salt.cap,
        salt_phase_down_threshold: base_salt.threshold,
        salt_phase_down_rate: salt.get_rate("phase_down_rate")?,
        salt_floor: salt.get_amount("floor")?,
        charitable_floor_rate: charitable.get_rate("itemizer_floor_pct_of_agi")?,
        charitable_non_itemizer: charitable.get("non_itemizer_deduction")?.by_status(true)?,
        itemized_limit_fraction: parse_fraction(
            &income
                .get("itemized_deduction_limitation")?
                .get("reduction_fraction")?,
        )?,
        capital_loss_limit: root
            .get("capital_gains_and_property")?
            .get("capital_loss_deduction_limit")?
            .get_amount("annual_limit_against_ordinary_income")?,
        niit_threshold: niit.get("magi_threshold")?.by_status(false)?,
        niit_rate: niit.get_rate("rate")?,
        oasdi_wage_base: fica.get_amount("oasdi_wage_base")?,
        oasdi_rate: fica.get_rate("oasdi_rate_employee")?,
        medicare_rate: fica.get_rate("medicare_rate_employee")?,
        additional_medicare_rate: additional.get_rate("rate")?,
        additional_medicare_threshold: additional.get("threshold")?.by_status(false)?,
        se_net_earnings_factor: se.get_rate("net_earnings_factor")?,
        se_oasdi_rate: se.get_rate("oasdi_rate")?,
        se_medicare_rate: se.get_rate("medicare_rate")?,
        se_minimum_earnings: se.get_amount("minimum_net_earnings")?,
        se_deduction_fraction: se.get_rate("deduction_fraction_of_se_tax")?,
        qbi_rate: qbi.get_rate("rate")?,
        qbi_threshold: qbi.get("taxable_income_threshold")?.by_status(false)?,
        qbi_phase_in_width: qbi.get("phase_in_width")?.by_status(true)?,
        child_tax_credit: ctc.get_amount("amount_per_child")?,
        child_credit_refundable_max: ctc.get_amount("refundable_max_per_child")?,
        child_credit_earned_floor: ctc.get_amount("earned_income_floor")?,
        child_credit_refundable_rate: CHILD_CREDIT_REFUNDABLE_RATE,
        other_dependent_credit: ctc.get_amount("other_dependent_credit")?,
        child_credit_phaseout: ctc.get("phase_out_magi_threshold")?.by_status(false)?,
        child_credit_phaseout_per_1000: ctc.get_amount("phase_out_reduction_per_1000")?,
        ss_taxation_base: benefit_tax.get("base_amount")?.by_status(false)?,
        ss_taxation_upper: benefit_tax.get("adjusted_base_amount")?.by_status(false)?,
        ss_max_taxable_fraction: benefit_tax.get_rate("max_taxable_fraction")?,
        early_withdrawal_penalty: early.get_rate("penalty_rate")?,
        hsa_penalty: early.get("hsa_non_medical_penalty")?.get_rate("rate")?,
        fpl_base: guideline.get_amount("first_person")?,
        fpl_per_person: guideline.get_amount("each_additional")?,
        aca: aca_year,
        irmaa: [irmaa_single, irmaa_joint, irmaa_single],
        limits: parse_limits(root)?,
    };
    for status in FilingStatus::ALL {
        let s = status.index();
        if year.ss_taxation_upper[s] < year.ss_taxation_base[s] {
            return Err(benefit_tax.err("adjusted base amount below base amount"));
        }
    }

    Ok(FederalBase {
        year,
        salt_schedule,
        senior_first_year: senior.get_year("first_tax_year")?,
        senior_last_year: senior.get_year("last_tax_year")?,
        fpl_alaska: regional("alaska")?,
        fpl_hawaii: regional("hawaii")?,
        aca_cliff,
    })
}

/// The schedule row in force in `year`: the latest row not after it (the first row
/// for earlier years).
fn salt_row(schedule: &[SaltRow], year: i32) -> SaltRow {
    let mut row = schedule[0];
    for r in schedule {
        if r.year <= year {
            row = *r;
        }
    }
    row
}

/// `{single, married_joint}` amounts with head of household as single.
fn state_pair(node: &Node) -> Result<[f64; FilingStatus::COUNT], DataError> {
    let single = node.get_amount("single")?;
    Ok([single, node.get_amount("married_joint")?, single])
}

fn parse_state(
    code: StateCode,
    node: &Node,
    full_retirement_age: f64,
    retirement_age: f64,
) -> Result<StateBase, DataError> {
    let mut y = StateYear {
        code,
        capital_gains_rate: -1.0,
        ss_max_taxable_fraction_of_benefits: 1.0,
        full_retirement_age,
        property_tax_rate: node.get_rate("property_tax_effective_rate")?,
        ..StateYear::default()
    };
    let has_income_tax = node.get("has_income_tax")?.boolean()?;
    let taxes_wages = node.get("taxes_wages")?.boolean()?;

    // Brackets.
    let brackets = node.get("brackets")?;
    let single = brackets_over(&brackets.get("single")?)?;
    let joint = brackets_over(&brackets.get("married_joint")?)?;
    if taxes_wages != (single.len > 0) || taxes_wages != (joint.len > 0) {
        return Err(brackets.err("brackets must be present exactly when taxes_wages is true"));
    }
    if taxes_wages && !has_income_tax {
        return Err(node.err("taxes_wages without has_income_tax"));
    }
    y.brackets = [single, joint, single];

    // Standard deduction or credit.
    let sd = node.get("standard_deduction")?;
    let follows_federal = sd.get("follows_federal")?.boolean()?;
    match sd.get("type")?.text()? {
        "deduction" => {
            y.standard_deduction = state_pair(&sd)?;
            y.allows_itemizing = true;
            if let Some(phase_out) = sd.opt("phase_out") {
                for (status, key) in [(0, "single"), (1, "married_joint"), (2, "single")] {
                    let range = phase_out.get(key)?.items()?;
                    if range.len() != 2 {
                        return Err(phase_out.err("expected [start, end]"));
                    }
                    let (start, end) = (range[0].amount()?, range[1].amount()?);
                    if end <= start {
                        return Err(phase_out.err("phase-out end must exceed start"));
                    }
                    y.standard_deduction_phaseout_start[status] = start;
                    y.standard_deduction_phaseout_end[status] = end;
                }
            }
        }
        "credit" => {
            y.standard_credit = state_pair(&sd.get("credit_amount")?)?;
            y.standard_credit_phaseout_start = STANDARD_CREDIT_PHASEOUT_START;
            y.standard_credit_phaseout_rate = STANDARD_CREDIT_PHASEOUT_RATE;
        }
        "none" => {}
        _ => return Err(sd.err("type must be deduction, credit or none")),
    }

    // Personal and dependent exemptions.
    let pe = node.get("personal_exemption")?;
    match pe.get("type")?.text()? {
        "deduction" => y.personal_exemption = state_pair(&pe)?,
        "credit" => y.personal_credit = state_pair(&pe)?,
        "none" => {}
        _ => return Err(pe.err("type must be deduction, credit or none")),
    }
    match pe.get("dependent_type")?.text()? {
        "deduction" => y.dependent_exemption = pe.get_amount("dependent")?,
        "credit" => y.dependent_credit = pe.get_amount("dependent")?,
        "none" => {}
        _ => return Err(pe.err("dependent_type must be deduction, credit or none")),
    }

    // Social Security.
    let ss = node.get("social_security")?;
    match ss.get("status")?.text()? {
        "none" => {}
        "follows_federal" => y.social_security_taxed = 1.0,
        "partial" => {
            y.social_security_taxed = 1.0;
            if let Some(exempt) = ss.opt("exempt_below_agi") {
                y.ss_exempt_below_agi = state_pair(&exempt)?;
            }
            if let Some(end) = ss.opt("phase_out_end_agi") {
                y.ss_phase_out_end_agi = state_pair(&end)?;
            }
            if let Some(step) = ss.opt("phase_out_per_step") {
                y.ss_phase_out_step = step.get_amount("step")?;
                y.ss_phase_out_fraction_per_step = step.get_rate("fraction")?;
            }
            if let Some(cap) = ss.opt("max_taxable_fraction_of_benefits") {
                y.ss_max_taxable_fraction_of_benefits = cap.rate()?;
            }
            if let Some(age) = ss.opt("full_exemption_age") {
                y.ss_full_exemption_age = age.amount()?;
            }
            if let Some(flag) = ss.opt("requires_full_retirement_age") {
                y.ss_exemption_requires_fra = flag.boolean()?;
            }
        }
        _ => return Err(ss.err("status must be none, partial or follows_federal")),
    }

    // Retirement distributions.
    let retirement = node.get("retirement_distributions")?;
    match retirement.get("taxed")?.text()? {
        "full" => {}
        "none" => {
            y.retirement_exclusion = f64::INFINITY;
            y.retirement_exclusion_age = retirement_age;
        }
        "partial" => {
            if let Some(exclusion) = retirement.opt("exclusion") {
                y.retirement_exclusion = exclusion.get_amount("amount")?;
                y.retirement_exclusion_age = match exclusion.opt("min_age") {
                    Some(age) => age.amount()?,
                    None => 0.0,
                };
            }
        }
        _ => return Err(retirement.err("taxed must be none, partial or full")),
    }

    // Long-term capital gains.
    let gains = node.get("long_term_capital_gains")?;
    match gains.get("treatment")?.text()? {
        "ordinary" | "not_taxed" => {}
        "exempt" => y.capital_gains_exclusion_fraction = 1.0,
        "exclusion" => {
            if let Some(fraction) = gains.opt("exclusion_fraction") {
                y.capital_gains_exclusion_fraction = fraction.rate()?;
            }
            if let Some(amount) = gains.opt("exclusion_amount") {
                y.capital_gains_exclusion_amount = amount.amount()?;
            }
            if y.capital_gains_exclusion_fraction == 0.0 && y.capital_gains_exclusion_amount == 0.0
            {
                return Err(gains.err("exclusion needs exclusion_fraction or exclusion_amount"));
            }
        }
        "rate_cap" | "lower_rates" => y.capital_gains_rate = gains.get_rate("max_rate")?,
        "surtax" => {
            let rate = gains.get_rate("surtax_rate")?;
            if let Some(threshold) = gains.opt("surtax_agi_threshold") {
                y.capital_gains_surtax_rate = rate;
                y.capital_gains_surtax_agi_threshold = threshold.amount()?;
            } else {
                y.investment_surtax_rate = rate;
                y.investment_surtax_threshold =
                    gains.get_amount("surtax_net_investment_income_threshold")?;
            }
        }
        "excise" => {
            y.capital_gains_excise = brackets_over(&gains.get("brackets")?)?;
            y.capital_gains_excise_deduction = gains.get_amount("standard_deduction")?;
        }
        _ => return Err(gains.err("unknown treatment")),
    }

    // Local income tax.
    let local = node.get("local_income_tax")?;
    let mut major_city = None;
    let mut major_city_rate = 0.0;
    if local.get("has_local_income_tax")?.boolean()? {
        if let Some(rate) = local.opt("statewide_avg_effective_rate") {
            y.local_rate = rate.rate()?;
        }
        if let (Some(city), Some(rate)) = (local.opt("major_city"), local.opt("major_city_rate")) {
            major_city = Some(city.text()?.to_ascii_lowercase());
            major_city_rate = rate.rate()?;
        }
    }

    Ok(StateBase {
        year: y,
        brackets_indexed: node.get("inflation_indexed")?.boolean()?,
        standard_deduction_indexed: sd.get("inflation_indexed")?.boolean()?,
        standard_deduction_follows_federal: follows_federal,
        major_city,
        major_city_rate,
    })
}

fn parse_states(
    states: &Value,
    base_year: i32,
    full_retirement_age: f64,
    retirement_age: f64,
) -> Result<Vec<StateBase>, DataError> {
    let root = Node::root(states, STATES_FILE);
    let map = states
        .as_object()
        .ok_or_else(|| root.err("expected an object keyed by state code"))?;
    if root.get("meta")?.get_year("year")? != base_year {
        return Err(root.err("meta.year differs from the federal tax_year"));
    }
    let mut out = Vec::with_capacity(map.len());
    for (key, value) in map {
        if key == "meta" {
            continue;
        }
        let node = root.child(key, value);
        let code = StateCode::parse(key).ok_or_else(|| node.err("not a two-letter state code"))?;
        out.push(parse_state(
            code,
            &node,
            full_retirement_age,
            retirement_age,
        )?);
    }
    if out.is_empty() {
        return Err(root.err("no states"));
    }
    out.sort_by_key(|s| s.year.code);
    Ok(out)
}

/// The factors implied by the published figures for the year after the base year.
fn parse_published_next_year(root: &Node, base_year: i32) -> Result<Indexation, DataError> {
    let next = base_year + 1;
    let published = root.get(&format!("published_{next}"))?;
    // The top single bracket threshold is the largest indexed amount, so its ratio
    // is the least affected by rounding to $50.
    let top_threshold = |block: &Node| -> Result<f64, DataError> {
        let schedule = brackets_up_to(
            &block
                .get("income_tax")?
                .get("ordinary_brackets")?
                .get(FilingStatus::Single.key())?,
        )?;
        Ok(schedule.lower[schedule.len - 1])
    };
    let poverty = root.get("aca")?.get("federal_poverty_guidelines")?;
    let guideline = |year: i32| -> Result<f64, DataError> {
        poverty.get(&year.to_string())?.get_amount("first_person")
    };
    let part_b = |block: &Node| -> Result<f64, DataError> {
        block
            .get("medicare")?
            .get("part_b")?
            .get_amount("standard_premium_monthly")
    };
    let ratio = |node: &Node, new: f64, old: f64| -> Result<f64, DataError> {
        if old > 0.0 && new > 0.0 {
            Ok(new / old)
        } else {
            Err(node.err("published next-year value must be positive"))
        }
    };
    Ok(Indexation {
        year: next,
        cpi_factor: ratio(&published, top_threshold(&published)?, top_threshold(root)?)?,
        wage_factor: ratio(
            &published,
            published.get("fica")?.get_amount("oasdi_wage_base")?,
            root.get("fica")?.get_amount("oasdi_wage_base")?,
        )?,
        fpl_factor: ratio(&published, guideline(base_year)?, guideline(base_year - 1)?)?,
        health_factor: ratio(&published, part_b(&published)?, part_b(root)?)?,
    })
}

impl TaxLaw {
    /// Builds the typed law from the embedded JSON (`data.tax_federal`,
    /// `data.tax_states`) and the profile's tax assumptions. Fails on the first
    /// missing or malformed key, naming the file and JSON path.
    pub fn from_data(data: &Data, model: &TaxModel) -> Result<TaxLaw, DataError> {
        let root = Node::root(&data.tax_federal, FEDERAL_FILE);
        let base_year = root.get_year("tax_year")?;
        let federal = parse_federal(&root, base_year)?;

        // Full retirement age for people now approaching it (the last row of the
        // SSA table), used by states that tie an exemption to it.
        let fra_table = root
            .get("social_security")?
            .get("full_retirement_age")?
            .get("table")?;
        let fra_rows = fra_table.items()?;
        let full_retirement_age = fra_rows
            .last()
            .ok_or_else(|| fra_table.err("empty table"))?
            .get_amount("fra_months")?
            / 12.0;
        // States that exempt retirement income do so for retirement-age
        // distributions; the federal penalty-free age marks that line.
        let retirement_age = root
            .get("early_withdrawal")?
            .get_amount("penalty_free_age")?;

        let states = parse_states(
            &data.tax_states,
            base_year,
            full_retirement_age,
            retirement_age,
        )?;
        let national_property_tax_rate =
            states.iter().map(|s| s.year.property_tax_rate).sum::<f64>() / states.len() as f64;

        Ok(TaxLaw {
            base_year,
            kind: model.kind,
            index_brackets: model.index_brackets,
            published_next_year: parse_published_next_year(&root, base_year)?,
            rmd: parse_rmd(&data.tax_federal)?,
            federal,
            states,
            national_property_tax_rate,
        })
    }

    /// [`Indexation::from_cpi`] with this law's base year.
    pub fn index_from_cpi(&self, year: i32, cpi_factor: f64) -> Indexation {
        Indexation::from_cpi(self.base_year, year, cpi_factor)
    }

    /// The law for a calendar year in which prices and wages stand at `index`
    /// relative to the base year, for a household resident in `state`.
    ///
    /// * Chained-CPI and CPI items (brackets, standard and additional deductions,
    ///   capital-gain thresholds, QBI thresholds, child credit amounts, contribution
    ///   limits, IRMAA tiers 1 to 4) scale with `index.cpi_factor` and are rounded to
    ///   the statute's increment (see [`index_rounded`]).
    /// * The OASDI wage base scales with `index.wage_factor`.
    /// * Frozen items stay nominal: NIIT and additional Medicare thresholds, Social
    ///   Security taxation thresholds, child credit phase-out and other-dependant
    ///   credit, the capital loss limit, the senior deduction, penalties.
    /// * Calendar rules use `index.year`: the SALT cap schedule, the senior deduction
    ///   sunset, the IRMAA top-tier freeze, the ACA cliff flag.
    /// * The poverty guideline scales with `index.fpl_factor` and IRMAA surcharge
    ///   amounts with `index.health_factor`, also when `index_brackets` is off.
    /// * State brackets, deductions and exemptions scale with the CPI factor only
    ///   where the state indexes them.
    ///
    /// An unknown state code gives [`StateCode::NONE`] parameters (no state tax).
    /// Cheap: copies and scales a few hundred numbers, no allocation.
    pub fn for_year(&self, index: Indexation, state: StateCode) -> TaxYearParams {
        let (cpi, wage) = if self.index_brackets {
            (index.cpi_factor, index.wage_factor)
        } else {
            (1.0, 1.0)
        };
        let base = &self.federal;
        let mut f = base.year;
        let bracket = |x: f64| index_rounded(x, cpi, BRACKET_INCREMENT);

        for s in 0..FilingStatus::COUNT {
            f.ordinary[s] = base.year.ordinary[s].indexed_rounded(cpi, BRACKET_INCREMENT);
            f.capital_gains[s] = base.year.capital_gains[s].indexed_rounded(cpi, BRACKET_INCREMENT);
            f.standard_deduction[s] = bracket(f.standard_deduction[s]);
            f.additional_deduction_65[s] = bracket(f.additional_deduction_65[s]);
            f.qbi_threshold[s] = bracket(f.qbi_threshold[s]);
            f.limits.roth_ira_phaseout_start[s] = index_rounded(
                f.limits.roth_ira_phaseout_start[s],
                cpi,
                ROTH_PHASEOUT_INCREMENT,
            );
            f.limits.roth_ira_phaseout_end[s] = index_rounded(
                f.limits.roth_ira_phaseout_end[s],
                cpi,
                ROTH_PHASEOUT_INCREMENT,
            );
            f.limits.ira_deduction_phaseout_start[s] = index_rounded(
                f.limits.ira_deduction_phaseout_start[s],
                cpi,
                ROTH_PHASEOUT_INCREMENT,
            );
            f.limits.ira_deduction_phaseout_end[s] = index_rounded(
                f.limits.ira_deduction_phaseout_end[s],
                cpi,
                ROTH_PHASEOUT_INCREMENT,
            );
            let table = &mut f.irmaa[s];
            for tier in 1..table.len {
                let top_frozen =
                    tier + 1 == table.len && index.year <= IRMAA_TOP_TIER_FROZEN_THROUGH;
                if !top_frozen {
                    table.over[tier] = index_rounded(table.over[tier], cpi, IRMAA_INCREMENT);
                }
                table.monthly[tier] *= index.health_factor;
            }
            // A frozen top tier must never sit below the indexed tier beneath it.
            if table.len >= 2 {
                let top = table.len - 1;
                table.over[top] = table.over[top].max(table.over[top - 1]);
            }
        }

        f.child_tax_credit = index_rounded(f.child_tax_credit, cpi, CHILD_CREDIT_INCREMENT);
        f.child_credit_refundable_max =
            index_rounded(f.child_credit_refundable_max, cpi, CHILD_CREDIT_INCREMENT);
        f.oasdi_wage_base = index_rounded(f.oasdi_wage_base, wage, WAGE_BASE_INCREMENT);

        f.limits.k401_employee = index_rounded(f.limits.k401_employee, cpi, K401_INCREMENT);
        f.limits.k401_catch_up = index_rounded(f.limits.k401_catch_up, cpi, K401_INCREMENT);
        f.limits.k401_catch_up_60_63 =
            index_rounded(f.limits.k401_catch_up_60_63, cpi, K401_INCREMENT);
        f.limits.k401_total = index_rounded(f.limits.k401_total, cpi, K401_TOTAL_INCREMENT);
        f.limits.compensation_limit =
            index_rounded(f.limits.compensation_limit, cpi, COMPENSATION_INCREMENT);
        for bound in &mut f.limits.ira_deduction_phaseout_spouse {
            *bound = index_rounded(*bound, cpi, ROTH_PHASEOUT_INCREMENT);
        }
        f.limits.ira = index_rounded(f.limits.ira, cpi, IRA_INCREMENT);
        f.limits.ira_catch_up = index_rounded(f.limits.ira_catch_up, cpi, IRA_CATCH_UP_INCREMENT);
        f.limits.hsa_self = index_rounded(f.limits.hsa_self, cpi, HSA_INCREMENT);
        f.limits.hsa_family = index_rounded(f.limits.hsa_family, cpi, HSA_INCREMENT);

        // Calendar rules.
        let salt = salt_row(&base.salt_schedule, index.year);
        f.salt_cap = salt.cap;
        f.salt_phase_down_threshold = salt.threshold;
        if index.year < base.senior_first_year || index.year > base.senior_last_year {
            f.senior_deduction = 0.0;
        }
        for &(year, cliff) in &base.aca_cliff {
            if year <= index.year {
                f.aca.cliff = cliff;
            }
        }

        // Poverty guideline, by region.
        let (first, each) = match &state.0 {
            b"AK" => base.fpl_alaska,
            b"HI" => base.fpl_hawaii,
            _ => (base.year.fpl_base, base.year.fpl_per_person),
        };
        f.fpl_base = first * index.fpl_factor;
        f.fpl_per_person = each * index.fpl_factor;

        TaxYearParams {
            kind: self.kind,
            year: index.year,
            cpi_factor: cpi,
            federal: f,
            state: self.state_year(state, cpi, &f),
            foreign_tax_rate: None,
        }
    }

    fn state_base(&self, state: StateCode) -> Option<&StateBase> {
        self.states
            .binary_search_by_key(&state, |s| s.year.code)
            .ok()
            .map(|i| &self.states[i])
    }

    fn state_year(&self, state: StateCode, cpi: f64, federal: &FederalYear) -> StateYear {
        let Some(base) = self.state_base(state) else {
            return StateYear {
                code: StateCode::NONE,
                capital_gains_rate: -1.0,
                ss_max_taxable_fraction_of_benefits: 1.0,
                property_tax_rate: self.national_property_tax_rate,
                ..StateYear::default()
            };
        };
        let mut y = base.year;
        let bracket_factor = if base.brackets_indexed { cpi } else { 1.0 };
        let deduction_factor = if base.standard_deduction_indexed {
            cpi
        } else {
            1.0
        };
        for s in 0..FilingStatus::COUNT {
            y.brackets[s] = y.brackets[s].indexed(bracket_factor);
            y.personal_exemption[s] *= bracket_factor;
            y.personal_credit[s] *= bracket_factor;
            y.ss_exempt_below_agi[s] *= bracket_factor;
            y.ss_phase_out_end_agi[s] *= bracket_factor;
            y.standard_deduction[s] *= deduction_factor;
            y.standard_credit[s] *= deduction_factor;
            y.standard_credit_phaseout_start[s] *= deduction_factor;
            if base.standard_deduction_follows_federal && y.allows_itemizing {
                y.standard_deduction[s] = federal.standard_deduction[s];
            }
        }
        y.ss_phase_out_step *= bracket_factor;
        y.dependent_exemption *= bracket_factor;
        y.dependent_credit *= bracket_factor;
        // Washington indexes the excise deduction (the state's own note in the data);
        // the $1,000,000 rate break is not indexed.
        y.capital_gains_excise_deduction *= cpi;
        y
    }

    /// True if `state` is in the data (a state or DC).
    pub fn knows_state(&self, state: StateCode) -> bool {
        self.state_base(state).is_some()
    }

    /// Average effective property tax rate for a state (the unweighted average of
    /// all states for [`StateCode::NONE`] or an unknown code).
    pub fn property_tax_rate(&self, state: StateCode) -> f64 {
        self.state_base(state)
            .map_or(self.national_property_tax_rate, |s| {
                s.year.property_tax_rate
            })
    }

    /// Local income tax rate for a household in `city`: the rate of the state's major
    /// taxing city when `city` names it, otherwise the statewide average share of
    /// income. Assign it to `params.state.local_rate` to override the default
    /// (the statewide average).
    pub fn local_rate(&self, state: StateCode, city: Option<&str>) -> f64 {
        let Some(base) = self.state_base(state) else {
            return 0.0;
        };
        if let (Some(city), Some(major)) = (city, base.major_city.as_deref()) {
            let city = city.trim().to_ascii_lowercase();
            // "indianapolis (marion county)" should match "Indianapolis".
            if !city.is_empty() && (major == city || major.starts_with(&format!("{city} ("))) {
                return base.major_city_rate;
            }
        }
        base.year.local_rate
    }
}
