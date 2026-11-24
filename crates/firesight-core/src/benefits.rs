//! Social Security.
//!
//! Responsibility: turn an earnings history into a retirement benefit the way SSA
//! does (wage-indexed earnings, best 35 years, AIME, bend-point PIA formula, early or
//! delayed claiming adjustment), plus spousal and survivor benefits, the earnings test
//! before full retirement age, COLA, and the optional trust-fund haircut.
//!
//! Contract:
//!
//! * [`EarningsRecord`] is fixed-size and lives in the path state: one entry per year
//!   of covered earnings, updated each December from simulated wages
//!   ([`SocialSecurityModel::record_earnings`]), so a layoff or early retirement
//!   lowers the benefit on that path.
//! * Past earnings (before `profile.start`) are reconstructed once at compile time
//!   from today's wage and a lifecycle curve ([`EarningsRecord::backfill`]), or
//!   loaded from an actual history ([`EarningsRecord::from_history`]).
//! * The national average wage index (AWI) is the published series up to the start
//!   year and, after it, the simulated CPI level times real wage growth of
//!   [`REAL_WAGE_GROWTH`] a year. The wage base, the earnings-test limits and the
//!   bend points of future cohorts follow it.
//! * A primary insurance amount (PIA) is carried in **today's dollars**
//!   ([`SocialSecurityModel::pia_real`]); [`SocialSecurityModel::monthly`] returns
//!   **nominal** dollars per month.
//!
//! Simplifications (each is a deliberate, documented departure from the statute):
//!
//! * COLA is continuous: a benefit is its real amount times the price level passed
//!   in, instead of stepping each December. The real value of a PIA is fixed when
//!   the December before the eligibility year is recorded.
//! * The wage base and the earnings-test limits track the same year's average wage
//!   (the statute lags them two years). Bend points and the AIME indexing year keep
//!   their statutory lag (the wage index of the year the worker turns 60).
//! * Claiming happens in whole months at the planned age; delayed credits apply at
//!   once (SSA applies those earned mid-year the following January). No rounding of
//!   monthly benefits to the dollar, no family maximum, no child benefits, no
//!   divorced-spouse benefits, no 40-credit eligibility check.
//! * Earnings test: the annual excess is withheld evenly over the year (SSA withholds
//!   whole months from January). The months withheld are given back at full
//!   retirement age by moving the claim age later by that many months, which is
//!   SSA's adjustment of the reduction factor; the caller accumulates
//!   [`MonthlyBenefits::withheld_months`] into [`Claimant::withheld_months`].
//! * Survivors take the survivor benefit as soon as they can (age 60) and switch to
//!   their own benefit at their claim age if it is larger. A widow(er)'s benefit is
//!   reduced only for the survivor's own age.

use serde_json::Value;

use crate::data::Data;
use crate::math;
use crate::model::{LifecycleProfile, SocialSecurityBasis, SocialSecurityHaircut, YearMonth};
use crate::MAX_PEOPLE;

/// Maximum years of covered earnings tracked per person (ages 16 to 75).
pub const MAX_EARNING_YEARS: usize = 60;

/// Real growth of the national average wage, per year: the long-range intermediate
/// assumption of the Social Security Trustees ("about 1.1% real per year",
/// `data/actuarial/params.json` `social_security_notes.points[0]`).
pub const REAL_WAGE_GROWTH: f64 = 0.011;

/// Inflation assumed only when a PIA has to be *projected* for someone who has not
/// yet reached 60 (the formula fixes wages at 60 and prices at 62, so the gap costs
/// about a year of inflation in real terms). 2.4% is the Trustees' long-range
/// intermediate CPI assumption. Paths replace it with simulated inflation as the
/// person passes 60.
pub const PROJECTION_INFLATION: f64 = 0.024;

/// Months in a year, for the ages the data gives in years (earliest claim 62,
/// latest delayed credit 70, earliest survivor benefit 60).
const MONTHS_PER_YEAR: u32 = 12;

/// Lowest and highest ages of the Cocco-Gomes-Maenhout wage profile
/// (`earnings.model.f`: "ages 20-65").
const PROFILE_AGE_RANGE: (f64, f64) = (20.0, 65.0);

/// Age at which the PIA formula fixes the wage index (`aime.rule`: "the year the
/// worker turns 60"); eligibility is two years later
/// (`aime.indexing_year_offset_from_eligibility` = -2).
const INDEXING_AGE: i32 = 60;

fn num(v: &Value, path: &[&str]) -> f64 {
    let mut cur = v;
    for key in path {
        cur = &cur[*key];
    }
    cur.as_f64()
        .unwrap_or_else(|| panic!("embedded data: missing number at {}", path.join(".")))
}

/// Reads a JSON object keyed by year into a dense vector starting at the first year.
fn year_series<T>(v: &Value, what: &str, read: impl Fn(&Value) -> Option<T>) -> (i32, Vec<T>) {
    let object = v
        .as_object()
        .unwrap_or_else(|| panic!("embedded data: {what} is not an object"));
    let mut rows: Vec<(i32, T)> = object
        .iter()
        .map(|(k, x)| {
            let year = k
                .parse::<i32>()
                .unwrap_or_else(|_| panic!("embedded data: {what}: bad year {k:?}"));
            let value = read(x).unwrap_or_else(|| panic!("embedded data: {what}: bad value"));
            (year, value)
        })
        .collect();
    rows.sort_by_key(|(year, _)| *year);
    let first = rows
        .first()
        .unwrap_or_else(|| panic!("embedded data: {what} is empty"))
        .0;
    for (i, (year, _)) in rows.iter().enumerate() {
        assert_eq!(*year, first + i as i32, "embedded data: {what} has a gap");
    }
    (first, rows.into_iter().map(|(_, value)| value).collect())
}

/// One person's covered earnings by year, already capped at each year's wage base and
/// expressed in wage-indexed dollars of the model's index year (the last year of the
/// published AWI series).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EarningsRecord {
    /// Indexed earnings per year of work; only the first `years` entries are used.
    /// Years with no earnings are not stored (they count as zeros in the average).
    pub indexed: [f64; MAX_EARNING_YEARS],
    /// Number of years recorded.
    pub years: usize,
    /// National average wage (nominal dollars) of the year the person turned 60,
    /// which fixes the scale of their AIME and bend points. 0 until that year has
    /// been recorded.
    pub awi_at_60: f64,
    /// Price level (1.0 at the plan's start month) in which the PIA at eligibility
    /// is expressed: the December before the year the person turns 62. 0 until
    /// recorded; below 1 for someone already past 62 at the start (past COLAs).
    pub price_at_62: f64,
    /// Earnings of the start year before the start month (nominal), added to the
    /// first year recorded by the simulation so that year is not understated.
    pub carry_in: f64,
}

impl Default for EarningsRecord {
    fn default() -> Self {
        Self {
            indexed: [0.0; MAX_EARNING_YEARS],
            years: 0,
            awi_at_60: 0.0,
            price_at_62: 0.0,
            carry_in: 0.0,
        }
    }
}

impl EarningsRecord {
    /// Reconstructs earnings from `career_start_age` up to `current_age` given today's
    /// annual earnings, walking the typical lifecycle wage curve backwards. See
    /// [`EarningsRecord::backfill_with`].
    pub fn backfill(
        current_earnings: f64,
        current_age: f64,
        career_start_age: f64,
        model: &SocialSecurityModel,
    ) -> EarningsRecord {
        Self::backfill_profile(
            current_earnings,
            current_age,
            career_start_age,
            LifecycleProfile::Typical,
            model,
        )
    }

    /// [`EarningsRecord::backfill`] with the job's lifecycle profile: `Flat` holds
    /// real pay constant, `Typical` uses the Cocco-Gomes-Maenhout high-school
    /// (including some college) age profile and `Steep` the college profile, both
    /// from `data/actuarial/params.json` `earnings.age_profile`.
    pub fn backfill_profile(
        current_earnings: f64,
        current_age: f64,
        career_start_age: f64,
        profile: LifecycleProfile,
        model: &SocialSecurityModel,
    ) -> EarningsRecord {
        Self::backfill_with(
            current_earnings,
            current_age,
            career_start_age,
            model,
            |age| model.lifecycle_wage(profile, age),
        )
    }

    /// Reconstructs a plausible past: for every calendar year before the start year
    /// in which the person was at least `career_start_age` at mid-year,
    ///
    /// ```text
    /// earnings(year) = current_earnings * curve(age) / curve(current_age) * CPI(year) / CPI(today)
    /// ```
    ///
    /// capped at that year's wage base and indexed by the published AWI. `curve` is
    /// the real wage level by age in any units (pass the career module's lifecycle
    /// factor to keep the past consistent with the simulated future).
    /// `current_earnings` is today's annual rate in today's dollars. The part of the
    /// start year before the start month goes to [`EarningsRecord::carry_in`].
    pub fn backfill_with(
        current_earnings: f64,
        current_age: f64,
        career_start_age: f64,
        model: &SocialSecurityModel,
        curve: impl Fn(f64) -> f64,
    ) -> EarningsRecord {
        let start = model.start;
        let birth = start.add_months(-((current_age * 12.0).round() as i32));
        let mut record = EarningsRecord::default();
        let anchor = curve(current_age);
        if current_earnings > 0.0 && current_earnings.is_finite() && anchor > 0.0 {
            let born = f64::from(birth.year()) + f64::from(birth.month() - 1) / 12.0;
            let first_year = (born + career_start_age).floor() as i32;
            for year in first_year.max(model.awi_first_year)..start.year() {
                let age = f64::from(year) + 0.5 - born;
                if age < career_start_age {
                    continue;
                }
                let nominal = current_earnings * curve(age) / anchor * model.cpi_relative(year);
                if nominal.is_finite() {
                    model.record_historical(&mut record, birth, year, nominal);
                }
            }
            record.carry_in = current_earnings * f64::from(start.month() - 1) / 12.0;
        }
        model.seed_history(&mut record, birth);
        record
    }

    /// Builds a record from actual past earnings, `(calendar year, nominal earnings)`
    /// as on an SSA statement. Years from the plan's start year on are ignored (the
    /// simulation records them).
    pub fn from_history(
        birth: YearMonth,
        earnings: &[(i32, f64)],
        model: &SocialSecurityModel,
    ) -> EarningsRecord {
        let mut record = EarningsRecord::default();
        for &(year, nominal) in earnings {
            if year < model.start.year() {
                model.record_historical(&mut record, birth, year, nominal);
            }
        }
        model.seed_history(&mut record, birth);
        record
    }

    /// Adds one year of covered earnings. `wage_index` is the ratio of that year's
    /// average wage to the index year's; earnings above `wage_base` are ignored.
    /// Zero or negative earnings add nothing. When the record is full the smallest
    /// year is replaced if the new one is larger (only the best 35 count).
    ///
    /// Prefer [`SocialSecurityModel::record_earnings`], which supplies the index and
    /// the base and keeps the age-60 and age-62 snapshots.
    pub fn record_year(&mut self, nominal_earnings: f64, wage_index: f64, wage_base: f64) {
        // Written so that NaN inputs are rejected too.
        if !(nominal_earnings > 0.0 && wage_base > 0.0 && wage_index > 0.0) {
            return;
        }
        let covered = nominal_earnings.min(wage_base);
        let indexed = covered / wage_index;
        if self.years < MAX_EARNING_YEARS {
            self.indexed[self.years] = indexed;
            self.years += 1;
            return;
        }
        let mut lowest = 0;
        for i in 1..MAX_EARNING_YEARS {
            if self.indexed[i] < self.indexed[lowest] {
                lowest = i;
            }
        }
        if indexed > self.indexed[lowest] {
            self.indexed[lowest] = indexed;
        }
    }

    /// Average indexed monthly earnings in dollars of the model's index year: the
    /// best 35 years, divided by 420. Fewer than 35 years leave zeros in the average.
    pub fn aime(&self) -> f64 {
        const BEST: usize = 35;
        let n = self.years.min(MAX_EARNING_YEARS);
        let mut sum = 0.0;
        if n > BEST {
            let mut top = self.indexed;
            let used = &mut top[..n];
            used.select_nth_unstable_by(BEST - 1, |a, b| b.total_cmp(a));
            for value in &used[..BEST] {
                sum += value;
            }
        } else {
            // Every year counts, in the recorded order: no copy needed.
            for value in &self.indexed[..n] {
                sum += value;
            }
        }
        sum / (BEST as f64 * 12.0)
    }
}

/// How a person's own benefit is determined, compiled from `SocialSecurityBasis`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BenefitBasis {
    /// Computed from the path's [`EarningsRecord`].
    FromEarnings,
    /// Fixed primary insurance amount in today's dollars per month.
    KnownPia(f64),
    /// Not eligible.
    None,
}

impl From<&SocialSecurityBasis> for BenefitBasis {
    fn from(basis: &SocialSecurityBasis) -> Self {
        match *basis {
            SocialSecurityBasis::FromEarnings { .. } => BenefitBasis::FromEarnings,
            SocialSecurityBasis::KnownBenefit { monthly_at_fra } => {
                BenefitBasis::KnownPia(monthly_at_fra.max(0.0))
            }
            SocialSecurityBasis::None => BenefitBasis::None,
        }
    }
}

/// Compiled Social Security rules. Built once in `Plan::compile`.
#[derive(Clone, Debug)]
pub struct SocialSecurityModel {
    start: YearMonth,
    haircut: Option<SocialSecurityHaircut>,
    /// Published average wage index, `awi[0]` for `awi_first_year`.
    awi: Vec<f64>,
    awi_first_year: i32,
    /// Growth of the last published year, used to bridge to the start year.
    awi_latest_growth: f64,
    /// AWI of the index year (the last published year).
    index_awi: f64,
    /// Average wage of the plan's start year (published or bridged).
    anchor_awi: f64,
    /// Contribution and benefit base by year.
    wage_base: Vec<f64>,
    wage_base_first_year: i32,
    /// Last published wage base as a fraction of that year's average wage.
    wage_base_per_awi: f64,
    /// PIA formula factors (0.90, 0.32, 0.15).
    pia_factors: [f64; 3],
    /// Bend points by eligibility year.
    bend_points: Vec<[f64; 2]>,
    bend_first_year: i32,
    /// Statutory bend points of the first table year (1979) and the average wage
    /// two years before, from which later bend points derive.
    bend_base: [f64; 2],
    bend_base_awi: f64,
    /// COLA by determination year (effective that December), decimals.
    cola: Vec<f64>,
    cola_first_year: i32,
    /// Full retirement age rows: (last birth year of the row, months).
    fra_table: Vec<(i32, u32)>,
    /// Birth years after this one all take the last row of `fra_table` (they are past
    /// every other row): `(last birth year of the second-to-last row, months of the
    /// last row)`. Nearly everyone is in the last row.
    fra_tail: (i32, u32),
    earliest_claim_months: u32,
    latest_claim_months: u32,
    early_first_36: f64,
    early_beyond_36: f64,
    delayed_per_month: f64,
    spousal_fraction: f64,
    spousal_first_36: f64,
    spousal_beyond_36: f64,
    survivor_earliest_months: u32,
    survivor_fraction_at_earliest: f64,
    widow_floor: f64,
    /// Earnings-test exempt amounts as fractions of the average wage (under full
    /// retirement age, in the year it is reached), from the last published year.
    test_limit_per_awi: [f64; 2],
    /// Published exempt amounts by year.
    test_limits: Vec<[f64; 2]>,
    test_first_year: i32,
    test_withhold: [f64; 2],
    /// Cocco-Gomes-Maenhout cubic coefficients (const, age, age^2/10, age^3/100)
    /// for the typical and steep profiles.
    profile_typical: [f64; 4],
    profile_steep: [f64; 4],
    /// Annual average CPI level relative to the last month of the history.
    cpi: Vec<f64>,
    cpi_first_year: i32,
}

/// A household's benefits for one month, nominal dollars.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MonthlyBenefits {
    /// Benefit paid to each person: own retirement benefit, topped up to the spousal
    /// or survivor benefit where that is higher, after the earnings test and haircut.
    pub paid: [f64; MAX_PEOPLE],
    /// Amount withheld from each person by the earnings test this month.
    pub withheld: [f64; MAX_PEOPLE],
    /// Share of this month's benefit withheld (0 to 1). Add it to
    /// [`Claimant::withheld_months`] so the reduction for early claiming is undone
    /// for those months at full retirement age.
    pub withheld_months: [f64; MAX_PEOPLE],
}

impl MonthlyBenefits {
    /// Total paid to the household this month.
    pub fn total(&self) -> f64 {
        self.paid.iter().sum()
    }
}

/// The state of one person that benefits depend on, supplied by `sim` each month.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Claimant {
    /// Birth month.
    pub birth: YearMonth,
    /// Planned claim age in years (62 to 70; clamped).
    pub claim_age: f64,
    /// Primary insurance amount in today's dollars per month. `None` means no Social
    /// Security at all for this person (basis `none`): no own, spousal or survivor
    /// benefit, and no survivor benefit for their widow(er). `Some(0.0)` is someone
    /// with no record of their own who can still receive spousal and survivor
    /// benefits. Before 62 this may be a projection; payments only start at the
    /// claim age.
    pub pia_real: Option<f64>,
    /// False once the person has died.
    pub alive: bool,
    /// Death month, for survivor benefits. Keep the deceased in the array.
    pub died: Option<YearMonth>,
    /// Earned income (wages and self-employment) expected for the current calendar
    /// year, nominal and annualised, for the earnings test. 0 when not working.
    pub annual_earnings: f64,
    /// Months of benefits withheld by the earnings test so far (the running sum of
    /// [`MonthlyBenefits::withheld_months`]).
    pub withheld_months: f64,
}

impl Claimant {
    /// A living person with no earnings and nothing withheld.
    pub fn new(birth: YearMonth, claim_age: f64, pia_real: Option<f64>) -> Claimant {
        Claimant {
            birth,
            claim_age,
            pia_real,
            alive: true,
            died: None,
            annual_earnings: 0.0,
            withheld_months: 0.0,
        }
    }
}

/// Present value factors of one person's own retirement benefit, one per month of
/// the plan, so the funded-ratio test costs one lookup.
/// Built by [`SocialSecurityModel::annuity`].
#[derive(Clone, Debug, PartialEq)]
pub struct BenefitAnnuity {
    factors: Vec<f64>,
}

impl BenefitAnnuity {
    /// Present value in today's dollars, at `months_since_start` and conditional on
    /// being alive then, of the remaining own benefits of someone with this PIA
    /// (today's dollars per month). 0 beyond the survival table.
    #[inline]
    pub fn value(&self, months_since_start: u32, pia_real: f64) -> f64 {
        self.factors
            .get(months_since_start as usize)
            .map_or(0.0, |f| f * pia_real)
    }
}

impl SocialSecurityModel {
    /// Loads bend points, the wage index and wage base series, the
    /// full-retirement-age table, claiming adjustment factors, the earnings-test
    /// thresholds and the lifecycle wage profiles.
    ///
    /// `start` is the plan's start month (`profile.start`): published figures are
    /// used up to that year and the simulated price level after it.
    pub fn new(
        data: &Data,
        haircut: Option<SocialSecurityHaircut>,
        start: YearMonth,
    ) -> SocialSecurityModel {
        let ss = &data.tax_federal["social_security"];
        let number = |x: &Value| x.as_f64();
        let pair = |x: &Value| Some([x.get(0)?.as_f64()?, x.get(1)?.as_f64()?]);

        let (awi_first_year, awi) = year_series(
            &ss["average_wage_index"]["series"],
            "average_wage_index.series",
            number,
        );
        let awi_latest_growth = num(
            ss,
            &["average_wage_index", "latest", "growth_over_prior_year"],
        );
        let index_awi = *awi.last().expect("embedded data: empty AWI series");
        let (wage_base_first_year, wage_base) = year_series(
            &ss["maximum_taxable_earnings"]["series"],
            "maximum_taxable_earnings.series",
            number,
        );
        let (bend_first_year, bend_points) = year_series(
            &ss["pia_formula"]["bend_points_by_eligibility_year"],
            "pia_formula.bend_points_by_eligibility_year",
            pair,
        );
        let (cola_first_year, cola) = year_series(
            &ss["cola"]["history_by_effective_december_year_pct"],
            "cola.history",
            |x| Some(x.as_f64()? / 100.0),
        );
        let (test_first_year, test_limits) = year_series(
            &ss["earnings_test"]["history"],
            "earnings_test.history",
            |x| Some([x["under_fra"].as_f64()?, x["year_of_fra"].as_f64()?]),
        );
        let factors = ss["pia_formula"]["factors"]
            .as_array()
            .expect("embedded data: pia_formula.factors");
        let pia_factors = [
            factors[0].as_f64().expect("embedded data: PIA factor"),
            factors[1].as_f64().expect("embedded data: PIA factor"),
            factors[2].as_f64().expect("embedded data: PIA factor"),
        ];
        let mut fra_table: Vec<(i32, u32)> = ss["full_retirement_age"]["table"]
            .as_array()
            .expect("embedded data: full_retirement_age.table")
            .iter()
            .map(|row| {
                (
                    row["birth_year_to"]
                        .as_i64()
                        .map_or(i32::MAX, |year| year as i32),
                    row["fra_months"]
                        .as_u64()
                        .expect("embedded data: fra_months") as u32,
                )
            })
            .collect();
        fra_table.sort_by_key(|row| row.0);
        let fra_tail = match fra_table.len() {
            0 => (i32::MAX, 0),
            1 => (i32::MIN, fra_table[0].1),
            n => (fra_table[n - 2].0, fra_table[n - 1].1),
        };

        let cgm = &data.actuarial["earnings"]["age_profile"]["cgm_coefficients"];
        let coefficients = |group: &str| {
            [
                num(cgm, &[group, "const"]),
                num(cgm, &[group, "age"]),
                num(cgm, &[group, "age2_over_10"]),
                num(cgm, &[group, "age3_over_100"]),
            ]
        };

        // Annual average CPI level from the monthly history, relative to its last
        // month (taken as today).
        let history = &data.history;
        let mut level = 1.0;
        let mut sums: Vec<(f64, u32)> = Vec::new();
        let cpi_first_year = history.start.year();
        for (i, change) in history.cpi.iter().enumerate() {
            if change.is_finite() {
                level *= 1.0 + change;
            }
            let slot = (history.month(i).year() - cpi_first_year) as usize;
            if sums.len() <= slot {
                sums.resize(slot + 1, (0.0, 0));
            }
            sums[slot].0 += level;
            sums[slot].1 += 1;
        }
        let cpi: Vec<f64> = sums
            .iter()
            .map(|(sum, n)| sum / f64::from((*n).max(1)) / level)
            .collect();

        let mut model = SocialSecurityModel {
            start,
            haircut,
            awi,
            awi_first_year,
            awi_latest_growth,
            index_awi,
            anchor_awi: 0.0,
            wage_base,
            wage_base_first_year,
            wage_base_per_awi: 0.0,
            pia_factors,
            bend_base: bend_points[0],
            bend_base_awi: 0.0,
            bend_points,
            bend_first_year,
            cola,
            cola_first_year,
            fra_table,
            fra_tail,
            earliest_claim_months: num(ss, &["early_retirement_reduction", "earliest_age"]) as u32
                * MONTHS_PER_YEAR,
            latest_claim_months: num(ss, &["delayed_retirement_credit", "max_age"]) as u32
                * MONTHS_PER_YEAR,
            early_first_36: num(ss, &["early_retirement_reduction", "per_month_first_36"]),
            early_beyond_36: num(ss, &["early_retirement_reduction", "per_month_beyond_36"]),
            delayed_per_month: num(ss, &["delayed_retirement_credit", "per_month"]),
            spousal_fraction: num(ss, &["spousal_benefit", "max_fraction_of_worker_pia"]),
            spousal_first_36: num(ss, &["spousal_benefit", "reduction_per_month_first_36"]),
            spousal_beyond_36: num(ss, &["spousal_benefit", "reduction_per_month_beyond_36"]),
            survivor_earliest_months: num(ss, &["survivor_benefit", "earliest_age"]) as u32
                * MONTHS_PER_YEAR,
            survivor_fraction_at_earliest: num(ss, &["survivor_benefit", "fraction_at_60"]),
            widow_floor: num(
                ss,
                &["survivor_benefit", "widows_limit_floor_fraction_of_pia"],
            ),
            test_limit_per_awi: [0.0; 2],
            test_limits,
            test_first_year,
            test_withhold: [
                num(ss, &["earnings_test", "withhold_per_dollar_over_under_fra"]),
                num(
                    ss,
                    &["earnings_test", "withhold_per_dollar_over_year_of_fra"],
                ),
            ],
            profile_typical: coefficients("high_school"),
            profile_steep: coefficients("college"),
            cpi,
            cpi_first_year,
        };
        model.anchor_awi = model.published_awi(start.year());
        model.bend_base_awi = model.published_awi(model.bend_first_year - 2);
        let last_base_year = model.wage_base_first_year + model.wage_base.len() as i32 - 1;
        model.wage_base_per_awi =
            model.wage_base[model.wage_base.len() - 1] / model.published_awi(last_base_year);
        let last_test_year = model.test_first_year + model.test_limits.len() as i32 - 1;
        let last_test = model.test_limits[model.test_limits.len() - 1];
        let test_awi = model.published_awi(last_test_year);
        model.test_limit_per_awi = [last_test[0] / test_awi, last_test[1] / test_awi];
        model
    }

    /// The plan's start month given at construction.
    pub fn start(&self) -> YearMonth {
        self.start
    }

    /// The calendar year whose average wage is the unit of
    /// [`EarningsRecord::indexed`] (the last published AWI year).
    pub fn index_year(&self) -> i32 {
        self.awi_first_year + self.awi.len() as i32 - 1
    }

    /// Published AWI; before the series its first value; after it the last value
    /// grown at the last published rate.
    fn published_awi(&self, year: i32) -> f64 {
        let i = year - self.awi_first_year;
        if i < 0 {
            self.awi[0]
        } else if (i as usize) < self.awi.len() {
            self.awi[i as usize]
        } else {
            self.index_awi
                * math::powi(
                    1.0 + self.awi_latest_growth,
                    i - (self.awi.len() as i32 - 1),
                )
        }
    }

    /// National average wage of a calendar year in nominal dollars. Up to the start
    /// year it is the published series (bridged from its last year at the last
    /// published growth rate); after the start year it is the start year's wage
    /// times `price_level` (the simulated CPI level in that year, 1.0 at start)
    /// times [`REAL_WAGE_GROWTH`] compounded.
    pub fn average_wage(&self, year: i32, price_level: f64) -> f64 {
        let ahead = year - self.start.year();
        if ahead <= 0 {
            self.published_awi(year)
        } else {
            self.anchor_awi * price_level * math::powi(1.0 + REAL_WAGE_GROWTH, ahead)
        }
    }

    /// [`SocialSecurityModel::average_wage`] relative to the index year: the
    /// `wage_index` argument of [`EarningsRecord::record_year`].
    pub fn wage_index(&self, year: i32, price_level: f64) -> f64 {
        self.average_wage(year, price_level) / self.index_awi
    }

    /// Maximum taxable earnings (the contribution and benefit base) of a year,
    /// nominal. Published years come from the table; later years keep the last
    /// published ratio to the average wage.
    pub fn wage_base(&self, year: i32, price_level: f64) -> f64 {
        let i = year - self.wage_base_first_year;
        if i < 0 {
            self.wage_base[0]
        } else if (i as usize) < self.wage_base.len() {
            self.wage_base[i as usize]
        } else {
            self.wage_base_per_awi * self.average_wage(year, price_level)
        }
    }

    /// Earnings-test exempt amounts of a year, nominal: `[under full retirement age,
    /// in the year it is reached]`.
    pub fn earnings_test_limits(&self, year: i32, price_level: f64) -> [f64; 2] {
        let i = year - self.test_first_year;
        if i >= 0 && (i as usize) < self.test_limits.len() {
            self.test_limits[i as usize]
        } else if i < 0 {
            self.test_limits[0]
        } else {
            let wage = self.average_wage(year, price_level);
            [
                self.test_limit_per_awi[0] * wage,
                self.test_limit_per_awi[1] * wage,
            ]
        }
    }

    /// Real wage level at an age on a lifecycle profile, in arbitrary units (only
    /// ratios matter). Ages are clamped to the profile's range of 20 to 65.
    pub fn lifecycle_wage(&self, profile: LifecycleProfile, age: f64) -> f64 {
        let c = match profile {
            LifecycleProfile::Flat => return 1.0,
            LifecycleProfile::Typical => &self.profile_typical,
            LifecycleProfile::Steep => &self.profile_steep,
        };
        let a = age.clamp(PROFILE_AGE_RANGE.0, PROFILE_AGE_RANGE.1);
        math::exp(c[0] + c[1] * a + c[2] * a * a / 10.0 + c[3] * a * a * a / 100.0)
    }

    /// Annual average CPI level of a past year relative to today (the end of the
    /// embedded history); 1.0 for years after it.
    fn cpi_relative(&self, year: i32) -> f64 {
        let i = year - self.cpi_first_year;
        if i < 0 {
            self.cpi[0]
        } else {
            self.cpi.get(i as usize).copied().unwrap_or(1.0)
        }
    }

    /// Product of the COLAs determined in `from_year..to_year` (each effective that
    /// December). Years after the published history use [`PROJECTION_INFLATION`].
    fn cola_factor(&self, from_year: i32, to_year: i32) -> f64 {
        let mut factor = 1.0;
        for year in from_year..to_year {
            let i = year - self.cola_first_year;
            let cola = if i >= 0 && (i as usize) < self.cola.len() {
                self.cola[i as usize]
            } else {
                PROJECTION_INFLATION
            };
            factor *= 1.0 + cola;
        }
        factor
    }

    /// Records a year before the plan's start with published wage bases and AWI.
    fn record_historical(
        &self,
        record: &mut EarningsRecord,
        birth: YearMonth,
        year: i32,
        nominal: f64,
    ) {
        // Earnings from 60 on count at face value: index them like the year of 60.
        let index_year = year.min(birth.year() + INDEXING_AGE);
        record.record_year(
            nominal,
            self.published_awi(index_year) / self.index_awi,
            self.wage_base(year, 1.0),
        );
    }

    /// Fills the age-60 wage and age-62 price snapshots for someone who passed
    /// those ages before the plan's start.
    fn seed_history(&self, record: &mut EarningsRecord, birth: YearMonth) {
        let year_60 = birth.year() + INDEXING_AGE;
        let eligibility = year_60 + 2;
        if year_60 < self.start.year() {
            record.awi_at_60 = self.published_awi(year_60);
        }
        if eligibility <= self.start.year() {
            record.price_at_62 = 1.0 / self.cola_factor(eligibility, self.start.year());
        }
    }

    /// Adds one simulated calendar year to a person's record. Call it every
    /// December for every living person with a `FromEarnings` basis, **including
    /// years with no earnings**: the call also snapshots the wage level in the year
    /// the person turns 60 and the price level before the year they turn 62, which
    /// fix the real value of their benefit on this path.
    ///
    /// `nominal_earnings` is the year's covered earnings (wages plus net
    /// self-employment income) in nominal dollars, before the wage-base cap;
    /// `price_level` is the simulated CPI level in that December (1.0 at start).
    pub fn record_earnings(
        &self,
        record: &mut EarningsRecord,
        birth: YearMonth,
        year: i32,
        nominal_earnings: f64,
        price_level: f64,
    ) {
        let year_60 = birth.year() + INDEXING_AGE;
        let wage = self.average_wage(year, price_level);
        if year == year_60 && record.awi_at_60 <= 0.0 {
            record.awi_at_60 = wage;
        }
        if year == year_60 + 1 && record.price_at_62 <= 0.0 {
            record.price_at_62 = price_level;
        }
        let earned = nominal_earnings.max(0.0) + record.carry_in;
        record.carry_in = 0.0;
        // Earnings after 60 count at face value in age-60 wage dollars.
        let index_wage = if year > year_60 && record.awi_at_60 > 0.0 {
            record.awi_at_60
        } else {
            wage
        };
        record.record_year(
            earned,
            index_wage / self.index_awi,
            self.wage_base(year, price_level),
        );
    }

    /// Full retirement age in months for a birth year (67 years for 1960 and later).
    #[inline]
    pub fn full_retirement_age_months(&self, birth_year: i32) -> u32 {
        if birth_year > self.fra_tail.0 {
            return self.fra_tail.1;
        }
        self.fra_table
            .iter()
            .find(|(last_year, _)| birth_year <= *last_year)
            .or(self.fra_table.last())
            .map_or(67 * MONTHS_PER_YEAR, |(_, months)| *months)
    }

    /// Full retirement age for survivor benefits: the same table two birth years
    /// later (`full_retirement_age.note`).
    fn survivor_fra_months(&self, birth_year: i32) -> u32 {
        self.full_retirement_age_months(birth_year - 2)
    }

    /// Bend points of the PIA formula for an eligibility year: the published table,
    /// or for later years the statutory derivation from the average wage two years
    /// earlier (`awi_at_60`), rounded to the dollar.
    pub fn bend_points(&self, eligibility_year: i32, awi_at_60: f64) -> [f64; 2] {
        let i = eligibility_year - self.bend_first_year;
        if i >= 0 && (i as usize) < self.bend_points.len() {
            return self.bend_points[i as usize];
        }
        if i < 0 {
            return self.bend_base;
        }
        let scale = awi_at_60 / self.bend_base_awi;
        [
            (self.bend_base[0] * scale).round(),
            (self.bend_base[1] * scale).round(),
        ]
    }

    /// The PIA formula: 90% of AIME up to the first bend point, 32% up to the
    /// second, 15% above, rounded down to the dime.
    pub fn pia_from_bend_points(&self, aime: f64, bends: [f64; 2]) -> f64 {
        let aime = aime.max(0.0);
        let pia = self.pia_factors[0] * aime.min(bends[0])
            + self.pia_factors[1] * (aime.min(bends[1]) - bends[0]).max(0.0)
            + self.pia_factors[2] * (aime - bends[1]).max(0.0);
        // The small offset keeps exact dimes from falling a cent under themselves.
        (pia * 10.0 + 1e-6).floor() / 10.0
    }

    /// Primary insurance amount per month for an AIME, in dollars of the year the
    /// person turns 62. `wage_index_at_62` scales the bend points of the latest
    /// published eligibility year (1.0 gives that year's formula exactly).
    pub fn pia(&self, aime: f64, wage_index_at_62: f64) -> f64 {
        let latest = self.bend_points[self.bend_points.len() - 1];
        self.pia_from_bend_points(
            aime,
            [latest[0] * wage_index_at_62, latest[1] * wage_index_at_62],
        )
    }

    /// A person's PIA at eligibility (age 62) in nominal dollars of that year, with
    /// the AIME it came from in the same dollars: `(aime, pia)`. `awi_at_60` is the
    /// average wage of the year they turned 60.
    pub fn pia_at_eligibility(
        &self,
        record: &EarningsRecord,
        birth: YearMonth,
        awi_at_60: f64,
    ) -> (f64, f64) {
        let aime = (record.aime() * awi_at_60 / self.index_awi + 1e-6).floor();
        let bends = self.bend_points(birth.year() + INDEXING_AGE + 2, awi_at_60);
        (aime, self.pia_from_bend_points(aime, bends))
    }

    /// The person's PIA in today's dollars per month from their record as it stands
    /// at `now` (no further earnings assumed): the value for
    /// [`Claimant::pia_real`].
    ///
    /// Once the record holds the age-60 wage and age-62 price snapshots this is
    /// exact for the path. Before that it is a projection: the average wage grows
    /// from its level at `now` (`price_level` is the CPI level at `now`) at
    /// [`REAL_WAGE_GROWTH`] plus [`PROJECTION_INFLATION`] until the person turns 60,
    /// and prices at [`PROJECTION_INFLATION`] until the eligibility year.
    ///
    /// Costs a partial sort of the record (a few hundred nanoseconds): call it once
    /// a year after [`SocialSecurityModel::record_earnings`], not every month.
    pub fn pia_real(
        &self,
        record: &EarningsRecord,
        birth: YearMonth,
        now: YearMonth,
        price_level: f64,
    ) -> f64 {
        let year_60 = birth.year() + INDEXING_AGE;
        let awi_at_60 = if record.awi_at_60 > 0.0 {
            record.awi_at_60
        } else if year_60 <= now.year() {
            self.average_wage(year_60, price_level)
        } else {
            self.average_wage(now.year(), price_level)
                * math::powi(
                    (1.0 + REAL_WAGE_GROWTH) * (1.0 + PROJECTION_INFLATION),
                    year_60 - now.year(),
                )
        };
        let price_at_62 = if record.price_at_62 > 0.0 {
            record.price_at_62
        } else {
            price_level
                * math::powi(
                    1.0 + PROJECTION_INFLATION,
                    (year_60 + 1 - now.year()).max(0),
                )
        };
        let (_, pia) = self.pia_at_eligibility(record, birth, awi_at_60);
        pia / price_at_62
    }

    fn claim_months(&self, claim_age: f64) -> u32 {
        let months = (claim_age * 12.0).round();
        if months.is_nan() {
            return self.latest_claim_months;
        }
        (months.max(0.0) as u32).clamp(self.earliest_claim_months, self.latest_claim_months)
    }

    fn claim_factor_at(&self, claim_age_months: f64, fra_months: f64) -> f64 {
        if claim_age_months < fra_months {
            let early = fra_months - claim_age_months;
            (1.0 - self.early_first_36 * early.min(36.0)
                - self.early_beyond_36 * (early - 36.0).max(0.0))
            .max(0.0)
        } else {
            let delayed = claim_age_months.min(f64::from(self.latest_claim_months)) - fra_months;
            1.0 + self.delayed_per_month * delayed.max(0.0)
        }
    }

    /// Multiplier on PIA for claiming at `claim_age_months` given the full retirement
    /// age: below 1 for early claiming (5/9 of 1% a month for 36 months, then 5/12:
    /// 0.70 at 62 when the full age is 67), 2/3 of 1% a month above it up to 70
    /// (1.24).
    pub fn claim_factor(&self, claim_age_months: u32, fra_months: u32) -> f64 {
        self.claim_factor_at(f64::from(claim_age_months), f64::from(fra_months))
    }

    /// Share of the full spousal benefit paid when it starts at `age_months`: 25/36
    /// of 1% a month for 36 months before full retirement age, then 5/12; no credit
    /// for waiting past it.
    pub fn spousal_factor(&self, age_months: u32, fra_months: u32) -> f64 {
        let early = f64::from(fra_months.saturating_sub(age_months));
        (1.0 - self.spousal_first_36 * early.min(36.0)
            - self.spousal_beyond_36 * (early - 36.0).max(0.0))
        .max(0.0)
    }

    /// Fraction of scheduled benefits payable in a calendar year (1.0, or the haircut
    /// factor from its start year).
    pub fn payable_fraction(&self, year: i32) -> f64 {
        match self.haircut {
            Some(haircut) if year >= haircut.from_year => haircut.factor.clamp(0.0, 1.0),
            _ => 1.0,
        }
    }

    /// Benefits paid in month `now` to up to two people, including spousal top-up,
    /// survivor step-up and the earnings test. `price_level` converts today's-dollar
    /// PIAs to nominal (benefits get full CPI COLA).
    ///
    /// * Own benefit from the claim age: `PIA x claim factor`.
    /// * Spousal top-up while both are alive and both have claimed (deemed filing):
    ///   `max(0, 50% of the partner's PIA - own PIA)`, reduced for the age at which
    ///   it began (the later of the two claims).
    /// * After the partner's death, from age 60: the survivor benefit, paid instead
    ///   of the own benefit when larger. It is what the deceased was receiving (at
    ///   least 82.5% of their PIA; their PIA with any delayed credits if they died
    ///   before claiming), reduced linearly from 100% at the survivor's full
    ///   retirement age to 71.5% at 60 for the age at which it began.
    /// * Earnings test before full retirement age: one dollar withheld for every two
    ///   of [`Claimant::annual_earnings`] above the year's limit (one for three above
    ///   the higher limit in the year full retirement age is reached), spread over
    ///   twelve months and capped at the month's benefit.
    /// * Every amount is scaled by [`SocialSecurityModel::payable_fraction`].
    ///
    /// Stateless and allocation-free; a few dozen nanoseconds.
    pub fn monthly(
        &self,
        now: YearMonth,
        people: &[Option<Claimant>; MAX_PEOPLE],
        price_level: f64,
    ) -> MonthlyBenefits {
        let mut out = MonthlyBenefits::default();
        let scale = price_level * self.payable_fraction(now.year());
        for i in 0..MAX_PEOPLE {
            let Some(me) = people[i] else { continue };
            let Some(pia) = me.pia_real else { continue };
            let age = me.birth.age_months_at(now);
            if !me.alive || age < 0 {
                continue;
            }
            let age = age as u32;
            let fra = self.full_retirement_age_months(me.birth.year());
            let claim = self.claim_months(me.claim_age);
            let claimed = age >= claim;

            let own = if claimed {
                let effective = if age >= fra && claim < fra {
                    (f64::from(claim) + me.withheld_months.max(0.0)).min(f64::from(fra))
                } else {
                    f64::from(claim)
                };
                pia.max(0.0) * self.claim_factor_at(effective, f64::from(fra))
            } else {
                0.0
            };

            let mut total = own;
            let partner = people[MAX_PEOPLE - 1 - i]
                .and_then(|partner| partner.pia_real.map(|partner_pia| (partner, partner_pia)));
            if let Some((partner, partner_pia)) = partner {
                let partner_pia = partner_pia.max(0.0);
                let partner_fra = self.full_retirement_age_months(partner.birth.year());
                let partner_claim = self.claim_months(partner.claim_age);
                if partner.alive {
                    let partner_age = partner.birth.age_months_at(now);
                    let excess = self.spousal_fraction * partner_pia - pia.max(0.0);
                    if claimed && excess > 0.0 && partner_age >= partner_claim as i32 {
                        // My age in the month my partner claimed.
                        let then =
                            partner_claim as i32 + partner.birth.ordinal() - me.birth.ordinal();
                        let began = (claim as i32).max(then).max(0) as u32;
                        total = own + excess * self.spousal_factor(began, fra);
                    }
                } else if let Some(died) = partner.died {
                    let partner_age = partner.birth.age_months_at(died).max(0) as u32;
                    let base = if partner_age >= partner_claim {
                        partner_pia
                            * self
                                .claim_factor(partner_claim, partner_fra)
                                .max(self.widow_floor)
                    } else if partner_age > partner_fra {
                        partner_pia * self.claim_factor(partner_age, partner_fra)
                    } else {
                        partner_pia
                    };
                    let earliest = self.survivor_earliest_months;
                    let began = (me.birth.age_months_at(died).max(0) as u32).max(earliest);
                    if age >= began {
                        let survivor_fra = self.survivor_fra_months(me.birth.year());
                        let factor = if began >= survivor_fra {
                            1.0
                        } else {
                            let floor = self.survivor_fraction_at_earliest;
                            floor
                                + (1.0 - floor) * f64::from(began - earliest)
                                    / f64::from(survivor_fra - earliest)
                        };
                        total = own.max(base * factor);
                    }
                }
            }

            let mut paid = total * scale;
            if paid > 0.0 && age < fra && me.annual_earnings > 0.0 {
                let limits = self.earnings_test_limits(now.year(), price_level);
                let fra_year = me.birth.add_months(fra as i32).year();
                let k = usize::from(now.year() == fra_year);
                let excess = me.annual_earnings - limits[k];
                if excess > 0.0 {
                    let withheld = (excess * self.test_withhold[k] / 12.0).min(paid);
                    out.withheld[i] = withheld;
                    out.withheld_months[i] = withheld / paid;
                    paid -= withheld;
                }
            }
            out.paid[i] = paid;
        }
        out
    }

    /// Present value in today's dollars of a person's own expected retirement
    /// benefits if claimed as planned (no spousal or survivor amounts, no earnings
    /// test), including the payable fraction of each year. `discount_rate` is real
    /// and annual. `survival[k]` is the probability of being alive `k` months after
    /// `now`, up to any common factor (pass `&mortality.survival_table()[t..]`);
    /// months beyond the slice count as dead.
    ///
    /// This walks every remaining month. Inside the monthly loop use
    /// [`SocialSecurityModel::annuity`], which gives the same number from a table.
    pub fn present_value(
        &self,
        claimant: &Claimant,
        now: YearMonth,
        discount_rate: f64,
        survival: &[f64],
    ) -> f64 {
        let Some(pia) = claimant.pia_real else {
            return 0.0;
        };
        let alive_now = survival.first().copied().unwrap_or(0.0);
        if alive_now.is_nan() || alive_now <= 0.0 {
            return 0.0;
        }
        let fra = self.full_retirement_age_months(claimant.birth.year());
        let claim = self.claim_months(claimant.claim_age);
        let benefit = pia.max(0.0) * self.claim_factor(claim, fra);
        let monthly_discount = 1.0 / (1.0 + math::annual_to_monthly(discount_rate));
        let age_now = claimant.birth.age_months_at(now);
        let first = (claim as i32 - age_now).max(0) as usize;
        let mut discount = math::powi(monthly_discount, first as i32);
        let mut value = 0.0;
        for (k, alive) in survival.iter().enumerate().skip(first) {
            value += discount * alive * self.payable_fraction((now + k as i32).year());
            discount *= monthly_discount;
        }
        benefit * value / alive_now
    }

    /// Precomputes [`SocialSecurityModel::present_value`] for every month of the
    /// plan. `survival_from_start[k]` is the probability of being alive `k` months
    /// after the plan's start (`MortalityModel::survival_table()`); `discount_rate`
    /// is real and annual. Build once per person in `Plan::compile`.
    pub fn annuity(
        &self,
        birth: YearMonth,
        claim_age: f64,
        discount_rate: f64,
        survival_from_start: &[f64],
    ) -> BenefitAnnuity {
        let n = survival_from_start.len();
        let fra = self.full_retirement_age_months(birth.year());
        let claim = self.claim_months(claim_age);
        let claim_factor = self.claim_factor(claim, fra);
        let monthly_discount = 1.0 / (1.0 + math::annual_to_monthly(discount_rate));
        let age_at_start = birth.age_months_at(self.start);
        let mut factors = vec![0.0; n];
        let mut next = 0.0;
        for t in (0..n).rev() {
            let alive = survival_from_start[t];
            let value = if alive > 0.0 {
                let pay = if age_at_start + t as i32 >= claim as i32 {
                    claim_factor * self.payable_fraction((self.start + t as i32).year())
                } else {
                    0.0
                };
                let survive = survival_from_start.get(t + 1).copied().unwrap_or(0.0) / alive;
                pay + monthly_discount * survive * next
            } else {
                0.0
            };
            factors[t] = value;
            next = value;
        }
        BenefitAnnuity { factors }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> &'static Data {
        Data::embedded()
    }

    fn model_at(start: YearMonth) -> SocialSecurityModel {
        SocialSecurityModel::new(data(), None, start)
    }

    fn model() -> SocialSecurityModel {
        model_at(YearMonth::ym(2026, 1))
    }

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    /// Someone who earned the taxable maximum every year from `from` to `to`.
    fn max_earner(birth: YearMonth, from: i32, to: i32, m: &SocialSecurityModel) -> EarningsRecord {
        let years: Vec<(i32, f64)> = (from..=to).map(|y| (y, m.wage_base(y, 1.0))).collect();
        EarningsRecord::from_history(birth, &years, m)
    }

    #[test]
    fn full_retirement_age_table() {
        let m = model();
        assert_eq!(m.full_retirement_age_months(1930), 65 * 12);
        assert_eq!(m.full_retirement_age_months(1937), 65 * 12);
        assert_eq!(m.full_retirement_age_months(1940), 65 * 12 + 6);
        assert_eq!(m.full_retirement_age_months(1950), 66 * 12);
        assert_eq!(m.full_retirement_age_months(1959), 66 * 12 + 10);
        assert_eq!(m.full_retirement_age_months(1960), 67 * 12);
        assert_eq!(m.full_retirement_age_months(2005), 67 * 12);
        // Survivor table is shifted two birth years.
        assert_eq!(m.survivor_fra_months(1961), 66 * 12 + 10);
        assert_eq!(m.survivor_fra_months(1962), 67 * 12);
    }

    #[test]
    fn claim_factors() {
        let m = model();
        let fra = 67 * 12;
        // SSA: 70% at 62, 75% at 63, 80% at 64, 86.67% at 65, 93.33% at 66, 100% at
        // 67, 108% at 68, 116% at 69, 124% at 70.
        let expected = [
            (62, 0.70),
            (63, 0.75),
            (64, 0.80),
            (65, 0.866_666_7),
            (66, 0.933_333_3),
            (67, 1.00),
            (68, 1.08),
            (69, 1.16),
            (70, 1.24),
        ];
        for (age, factor) in expected {
            assert!(
                close(m.claim_factor(age * 12, fra), factor, 1e-6),
                "age {age}: {}",
                m.claim_factor(age * 12, fra)
            );
        }
        // The published examples in the data agree.
        let examples = &data().tax_federal["social_security"]["claiming_factor_examples_fra_67"];
        for age in 62..=70u32 {
            let published = examples[age.to_string()].as_f64().unwrap();
            assert!(close(m.claim_factor(age * 12, fra), published, 5e-5));
        }
        // Monthly steps: one month early costs 5/9 of 1%; one month late earns 2/3.
        assert!(close(m.claim_factor(fra - 1, fra), 1.0 - 5.0 / 900.0, 1e-9));
        assert!(close(m.claim_factor(fra + 1, fra), 1.0 + 2.0 / 300.0, 1e-9));
        // No credit after 70; full retirement age 66: 75% at 62 and 132% at 70.
        assert_eq!(m.claim_factor(75 * 12, fra), m.claim_factor(70 * 12, fra));
        assert!(close(m.claim_factor(62 * 12, 66 * 12), 0.75, 1e-6));
        assert!(close(m.claim_factor(70 * 12, 66 * 12), 1.32, 1e-6));
        // Spousal: 32.5% of the worker's PIA at 62 is 65% of the full 50%.
        assert!(close(m.spousal_factor(62 * 12, fra), 0.65, 1e-6));
        assert!(close(m.spousal_factor(64 * 12, fra), 0.75, 1e-6));
        assert_eq!(m.spousal_factor(fra, fra), 1.0);
        assert_eq!(m.spousal_factor(70 * 12, fra), 1.0);
        // Claim ages are clamped to 62..70.
        assert_eq!(m.claim_months(50.0), 62 * 12);
        assert_eq!(m.claim_months(99.0), 70 * 12);
        assert_eq!(m.claim_months(f64::NAN), 70 * 12);
        assert_eq!(m.claim_months(66.5), 798);
    }

    #[test]
    fn pia_formula_for_the_2026_bend_points() {
        let m = model();
        let bends = m.bend_points(2026, 0.0);
        assert_eq!(bends, [1_286.0, 7_749.0]);
        // SSA's worked example (ssa.gov/oact/cola/piaformula.html): a worker with
        // maximum-taxable earnings in every year since age 22 who retires at 62 in
        // 2026. Recomputed from the published series: AIME $14,358; PIA
        // 0.9 x 1,286 + 0.32 x (7,749 - 1,286) + 0.15 x (14,358 - 7,749)
        // = 1,157.40 + 2,068.16 + 991.35 = 4,216.91 -> $4,216.90.
        assert!(close(
            m.pia_from_bend_points(14_358.0, bends),
            4_216.9,
            1e-9
        ));
        // Hand-computed points on each segment.
        assert!(close(m.pia_from_bend_points(1_000.0, bends), 900.0, 1e-9));
        assert!(close(m.pia_from_bend_points(1_286.0, bends), 1_157.4, 1e-9));
        // 1,157.40 + 0.32 x (5,000 - 1,286) = 2,345.88 -> 2,345.80.
        assert!(close(m.pia_from_bend_points(5_000.0, bends), 2_345.8, 1e-9));
        assert_eq!(m.pia_from_bend_points(0.0, bends), 0.0);
        assert_eq!(m.pia_from_bend_points(-500.0, bends), 0.0);
        // 3,225.56 + 0.15 x (1e9 - 7,749) = 150,002,063.21
        assert!(close(
            m.pia_from_bend_points(1e9, bends),
            150_002_063.2,
            1e-3
        ));
        // `pia` scales the latest published bend points (2027: 1,340 and 8,077).
        assert!(close(m.pia(1_000.0, 1.0), 900.0, 1e-9));
        assert!(close(
            m.pia(2_000.0, 1.0),
            0.9 * 1_340.0 + 0.32 * 660.0,
            0.1
        ));
        assert!(close(m.pia(4_000.0, 2.0), 2.0 * m.pia(2_000.0, 1.0), 0.11));
        // Bend points of future cohorts follow the statutory derivation: the 2027
        // pair comes out of the 2025 average wage.
        let derived = [
            (180.0 * 72_800.33f64 / 9_779.44).round(),
            (1_085.0 * 72_800.33f64 / 9_779.44).round(),
        ];
        assert_eq!(derived, [1_340.0, 8_077.0]);
        assert_eq!(m.bend_points(2027, 0.0), derived);
        assert_eq!(m.bend_points(2040, 72_800.33), derived);
        assert_eq!(m.bend_points(1960, 0.0), [180.0, 1_085.0]);
    }

    #[test]
    fn maximum_earner_retiring_at_62_in_2026() {
        // Born 1964: indexed to the 2024 average wage, eligible in 2026.
        let m = model();
        let birth = YearMonth::ym(1964, 6);
        let record = max_earner(birth, 1986, 2025, &m);
        assert_eq!(record.years, 40);
        assert_eq!(record.awi_at_60, 69_846.57);
        assert_eq!(record.price_at_62, 1.0);
        let (aime, pia) = m.pia_at_eligibility(&record, birth, record.awi_at_60);
        assert_eq!(aime, 14_358.0);
        assert!(close(pia, 4_216.9, 1e-9));
        assert!(close(
            m.pia_real(&record, birth, m.start(), 1.0),
            4_216.9,
            1e-9
        ));
        // Claiming at 62 and 1 month (the earliest month for most birthdays) pays
        // 4,216.90 x (1 - 0.20 - 23 x 5/1200) = $2,969, SSA's published maximum for
        // retirement at 62 in 2026.
        let benefit = pia * m.claim_factor(62 * 12 + 1, 67 * 12);
        assert_eq!(benefit.floor(), 2_969.0);
    }

    #[test]
    fn maximum_earner_at_full_retirement_age_matches_ssa() {
        // SSA: the maximum benefit for a worker retiring at full retirement age in
        // 2026 is $4,152 a month. That worker was born in 1959 (full age 66 and 10
        // months), became eligible in 2021 and has had the COLAs of 2021 to 2025.
        let published = num(
            &data().tax_federal["social_security"],
            &["max_benefit_at_fra_monthly", "2026"],
        );
        assert_eq!(published, 4_152.0);
        let m = model();
        let birth = YearMonth::ym(1959, 3);
        let record = max_earner(birth, 1981, 2025, &m);
        let (aime, pia) = m.pia_at_eligibility(&record, birth, record.awi_at_60);
        assert_eq!(aime, 11_463.0);
        assert!(close(pia, 3_317.4, 1e-9));
        // 1.059 x 1.087 x 1.032 x 1.025 x 1.028 of COLA since eligibility.
        let colas = 1.059 * 1.087 * 1.032 * 1.025 * 1.028;
        assert!(close(record.price_at_62, 1.0 / colas, 1e-12));
        let pia_today = m.pia_real(&record, birth, m.start(), 1.0);
        assert!(close(pia_today, published, 2.0), "{pia_today}");
        // Paid from the month of full retirement age (January 2026).
        let person = Claimant::new(birth, 66.0 + 10.0 / 12.0, Some(pia_today));
        let paid = m.monthly(YearMonth::ym(2026, 1), &[Some(person), None], 1.0);
        assert!(close(paid.paid[0], published, 2.0), "{}", paid.paid[0]);
        assert_eq!(paid.paid[1], 0.0);
        let before = m.monthly(YearMonth::ym(2025, 12), &[Some(person), None], 1.0);
        assert_eq!(before.total(), 0.0);
    }

    #[test]
    fn aime_uses_the_best_35_years() {
        let mut record = EarningsRecord::default();
        assert_eq!(record.aime(), 0.0);
        // Ten years of 42,000 leave 25 zeros: 420,000 / 420 = 1,000.
        for _ in 0..10 {
            record.record_year(42_000.0, 1.0, 100_000.0);
        }
        assert_eq!(record.years, 10);
        assert!(close(record.aime(), 1_000.0, 1e-9));
        // Capped at the wage base; indexed by the wage index.
        let mut capped = EarningsRecord::default();
        capped.record_year(500_000.0, 0.5, 84_000.0);
        assert!(close(capped.indexed[0], 168_000.0, 1e-9));
        assert!(close(capped.aime(), 400.0, 1e-9));
        // Zero, negative and NaN years are not recorded.
        capped.record_year(0.0, 1.0, 1e6);
        capped.record_year(-5.0, 1.0, 1e6);
        capped.record_year(f64::NAN, 1.0, 1e6);
        capped.record_year(10.0, 0.0, 1e6);
        assert_eq!(capped.years, 1);
        // 40 years of 1..=40 thousand: the best 35 are 6..=40, sum 805 thousand.
        let mut long = EarningsRecord::default();
        for k in 1..=40 {
            long.record_year(f64::from(k) * 1_000.0, 1.0, 1e9);
        }
        assert!(close(long.aime(), 805_000.0 / 420.0, 1e-9));
        // A full record replaces its lowest year with a better one.
        let mut full = EarningsRecord::default();
        for k in 1..=60 {
            full.record_year(f64::from(k), 1.0, 1e9);
        }
        assert_eq!(full.years, MAX_EARNING_YEARS);
        full.record_year(100.0, 1.0, 1e9);
        full.record_year(0.5, 1.0, 1e9);
        assert_eq!(full.years, MAX_EARNING_YEARS);
        let sum: f64 = full.indexed.iter().sum();
        assert!(close(sum, 1_830.0 - 1.0 + 100.0, 1e-9));
        // Best 35 of {2..=60, 100}: 100 + (27..=60).
        let best: f64 = 100.0 + (27..=60).map(f64::from).sum::<f64>();
        assert!(close(full.aime(), best / 420.0, 1e-9));
    }

    #[test]
    fn wage_index_wage_base_and_test_limits() {
        let m = model_at(YearMonth::ym(2026, 11));
        assert_eq!(m.index_year(), 2025);
        assert_eq!(m.start(), YearMonth::ym(2026, 11));
        assert_eq!(m.average_wage(2025, 1.0), 72_800.33);
        assert_eq!(m.average_wage(1977, 5.0), 9_779.44);
        assert_eq!(m.average_wage(1900, 1.0), 2_799.16);
        // The start year is bridged from the last published year at its growth rate.
        let anchor = 72_800.33 * 1.0423;
        assert!(close(m.average_wage(2026, 1.0), anchor, 1e-6));
        // Later years: price level times 1.1% real growth a year.
        assert!(close(
            m.average_wage(2036, 1.3),
            anchor * 1.3 * math::powi(1.011, 10),
            1e-6
        ));
        assert!(close(m.wage_index(2025, 1.0), 1.0, 1e-12));
        // Published wage bases, then growth with the average wage.
        assert_eq!(m.wage_base(2026, 1.0), 184_500.0);
        assert_eq!(m.wage_base(2027, 1.2), 192_300.0);
        assert_eq!(m.wage_base(1990, 1.0), 51_300.0);
        assert_eq!(m.wage_base(1900, 1.0), 3_000.0);
        let base_2028 = m.wage_base(2028, 1.05);
        assert!(
            base_2028 > 192_300.0 && base_2028 < 215_000.0,
            "{base_2028}"
        );
        assert!(close(
            m.wage_base(2038, 1.05) / base_2028,
            math::powi(1.011, 10),
            1e-9
        ));
        assert!(close(
            m.wage_base(2040, 2.0),
            2.0 * m.wage_base(2040, 1.0),
            1e-6
        ));
        // Earnings test: $24,480 and $65,160 in 2026.
        assert_eq!(m.earnings_test_limits(2026, 1.0), [24_480.0, 65_160.0]);
        assert_eq!(m.earnings_test_limits(2027, 1.0), [25_560.0, 67_920.0]);
        let later = m.earnings_test_limits(2030, 1.1);
        assert!(later[0] > 25_560.0 && later[0] < 32_000.0, "{later:?}");
        assert!(close(later[1] / later[0], 67_920.0 / 25_560.0, 1e-9));
    }

    #[test]
    fn backfill_builds_a_plausible_past() {
        let m = model_at(YearMonth::ym(2026, 7));
        // A 40-year-old earning $100,000 who started at 22: mid-year age reaches 22
        // in 2008 (born July 1986), so 2008..=2025 are recorded.
        let record = EarningsRecord::backfill(100_000.0, 40.0, 22.0, &m);
        assert_eq!(record.years, 18);
        assert_eq!(record.awi_at_60, 0.0);
        assert_eq!(record.price_at_62, 0.0);
        assert!(close(record.carry_in, 50_000.0, 1e-9));
        // Last year: one year of the age profile and one year of inflation below
        // today's pay, indexed by exactly the 2025 average wage.
        let last = record.indexed[record.years - 1];
        assert!(last > 90_000.0 && last < 100_000.0, "{last}");
        // Earlier years are lower in indexed terms (steep early career).
        let first = record.indexed[0];
        assert!(first > 40_000.0 && first < 0.85 * last, "{first}");
        // Projected benefit if they stopped now: between a quarter and a half of
        // the maximum.
        let birth = YearMonth::ym(1986, 7);
        let pia = m.pia_real(&record, birth, m.start(), 1.0);
        assert!(pia > 1_500.0 && pia < 2_600.0, "{pia}");

        // Flat profile: real pay was constant, so indexed pay falls with the gap
        // between wage and price growth only.
        let flat =
            EarningsRecord::backfill_profile(100_000.0, 40.0, 22.0, LifecycleProfile::Flat, &m);
        assert!(flat.indexed[0] > first);
        assert!(flat.indexed[0] > 85_000.0 && flat.indexed[0] < 125_000.0);
        // Steep profile: lower early pay than typical.
        let steep =
            EarningsRecord::backfill_profile(100_000.0, 40.0, 22.0, LifecycleProfile::Steep, &m);
        assert!(steep.indexed[0] < first);
        // A custom curve is honoured.
        let doubled = EarningsRecord::backfill_with(100_000.0, 40.0, 22.0, &m, |age| {
            if age < 30.0 {
                1.0
            } else {
                2.0
            }
        });
        assert!(doubled.indexed[0] < 0.6 * flat.indexed[0]);

        // More pay, more benefit, up to the wage base: $1M and $10M earn the same.
        let low = EarningsRecord::backfill(50_000.0, 40.0, 22.0, &m);
        assert!(m.pia_real(&low, birth, m.start(), 1.0) < pia);
        let rich = EarningsRecord::backfill(1e6, 40.0, 22.0, &m);
        let richer = EarningsRecord::backfill(1e7, 40.0, 22.0, &m);
        assert_eq!(rich.indexed[..18], richer.indexed[..18]);
        // Nothing to backfill.
        for record in [
            EarningsRecord::backfill(0.0, 40.0, 22.0, &m),
            EarningsRecord::backfill(-10.0, 40.0, 22.0, &m),
            EarningsRecord::backfill(f64::NAN, 40.0, 22.0, &m),
            EarningsRecord::backfill(80_000.0, 21.0, 22.0, &m),
        ] {
            assert_eq!(record.years, 0);
            assert_eq!(m.pia_real(&record, birth, m.start(), 1.0), 0.0);
        }
        // A 64-year-old: snapshots come from history (60 in 2022, eligible 2024).
        let old = EarningsRecord::backfill(120_000.0, 64.0, 22.0, &m);
        assert_eq!(old.years, 42);
        assert_eq!(old.awi_at_60, 63_795.13);
        assert!(close(old.price_at_62, 1.0 / (1.025 * 1.028), 1e-12));
        let old_pia = m.pia_real(&old, YearMonth::ym(1962, 7), m.start(), 1.0);
        assert!(old_pia > 2_500.0 && old_pia < 4_300.0, "{old_pia}");
        // A very long career fits the record.
        let ancient = EarningsRecord::backfill(60_000.0, 95.0, 16.0, &m);
        assert_eq!(ancient.years, MAX_EARNING_YEARS);
    }

    #[test]
    fn lifecycle_profiles_come_from_the_data() {
        let m = model();
        assert_eq!(m.lifecycle_wage(LifecycleProfile::Flat, 30.0), 1.0);
        // CGM high school: f(45) - f(22) = 0.1682 x 23 - 0.0323 x 154.1 + 0.002 x
        // 804.77 = 0.5007.
        let typical = m.lifecycle_wage(LifecycleProfile::Typical, 45.0)
            / m.lifecycle_wage(LifecycleProfile::Typical, 22.0);
        assert!(close(math::ln(typical), 0.5007, 1e-3), "{typical}");
        let steep = m.lifecycle_wage(LifecycleProfile::Steep, 45.0)
            / m.lifecycle_wage(LifecycleProfile::Steep, 22.0);
        assert!(steep > typical);
        // Clamped outside 20..65.
        assert_eq!(
            m.lifecycle_wage(LifecycleProfile::Typical, 10.0),
            m.lifecycle_wage(LifecycleProfile::Typical, 20.0)
        );
        assert_eq!(
            m.lifecycle_wage(LifecycleProfile::Steep, 80.0),
            m.lifecycle_wage(LifecycleProfile::Steep, 65.0)
        );
    }

    /// Simulates a career from the start month with constant real pay and a given
    /// inflation rate, stopping work after `last_work_age`, and returns the record
    /// at the end of the year the person turns 62 with the PIA in today's dollars.
    fn simulate_career(
        m: &SocialSecurityModel,
        birth: YearMonth,
        real_pay: f64,
        inflation: f64,
        last_work_age: i32,
    ) -> (EarningsRecord, f64) {
        let mut record = EarningsRecord::default();
        let start_year = m.start().year();
        let mut price = 1.0;
        let mut year = start_year;
        loop {
            // December price level of this year.
            price *= 1.0 + inflation;
            let age = year - birth.year();
            let earned = if age <= last_work_age {
                real_pay * price
            } else {
                0.0
            };
            m.record_earnings(&mut record, birth, year, earned, price);
            if age == 62 {
                let now = YearMonth::ym(year, 12);
                return (record, m.pia_real(&record, birth, now, price));
            }
            year += 1;
        }
    }

    #[test]
    fn simulated_years_are_recorded_and_indexed() {
        let m = model_at(YearMonth::ym(2027, 1));
        let birth = YearMonth::ym(1997, 1);
        // 30 years of $80,000 real from 30 to 59, no inflation.
        let (record, pia) = simulate_career(&m, birth, 80_000.0, 0.0, 59);
        assert_eq!(record.years, 30);
        // Year 60 is 2057: 30 years of 1.1% real wage growth from the start year.
        let wage_2027 = 72_800.33 * 1.0423 * 1.0423;
        assert!(close(
            record.awi_at_60,
            wage_2027 * math::powi(1.011, 30),
            1e-6
        ));
        assert_eq!(record.price_at_62, 1.0);
        // The first year is indexed to 2025 wages: 80,000 / (1.0423^2).
        assert!(close(record.indexed[0], 80_000.0 / (1.0423 * 1.0423), 1e-6));
        // Constant real pay loses ground to wages by 1.1% a year.
        assert!(close(
            record.indexed[1] / record.indexed[0],
            1.0 / 1.011,
            1e-9
        ));
        // Hand check of the PIA: AIME in age-60 dollars, bend points scaled by the
        // same wage.
        let aime = (record.aime() * record.awi_at_60 / 72_800.33 + 1e-6).floor();
        let scale = record.awi_at_60 / 9_779.44;
        let (b1, b2) = ((180.0 * scale).round(), (1_085.0 * scale).round());
        let expected = 0.9 * b1 + 0.32 * (aime.min(b2) - b1);
        assert!(aime > b1 && aime < b2);
        assert!(close(pia, expected, 0.11), "{pia} vs {expected}");
        // Today's formula would give about $2,570 for 30 years at $80,000 (AIME
        // 5,714). Thirty years of 1.1% real wage growth lift both the indexed
        // earnings and the bend points of this later cohort.
        assert!(pia > 3_000.0 && pia < 3_700.0, "{pia}");

        // Inflation alone changes the real benefit only through the one-year gap
        // between the wage snapshot (60) and the price snapshot (before 62).
        let (inflated, pia_inflated) = simulate_career(&m, birth, 80_000.0, 0.03, 59);
        assert!(close(
            inflated.price_at_62 / math::powi(1.03, 32),
            1.0,
            1e-12
        ));
        assert!(
            close(pia_inflated / pia, 1.0 / 1.03, 2e-3),
            "{}",
            pia_inflated / pia
        );

        // Working past 60 adds years at face value (in age-60 wage dollars).
        let (longer, pia_longer) = simulate_career(&m, birth, 80_000.0, 0.0, 62);
        assert_eq!(longer.years, 33);
        assert!(close(
            longer.indexed[32],
            80_000.0 / (longer.awi_at_60 / 72_800.33),
            1e-6
        ));
        assert!(pia_longer > pia);
        // A layoff year lowers the benefit.
        let (shorter, pia_shorter) = simulate_career(&m, birth, 80_000.0, 0.0, 50);
        assert_eq!(shorter.years, 21);
        assert!(pia_shorter < pia);
        // Pay above the wage base is ignored.
        let (capped, _) = simulate_career(&m, birth, 5e6, 0.0, 59);
        assert!(close(
            capped.indexed[0],
            192_300.0 / (1.0423 * 1.0423),
            1e-6
        ));

        // The carried-in part of the start year is added once.
        let mid = model_at(YearMonth::ym(2027, 7));
        let mut record = EarningsRecord::backfill(60_000.0, 30.0, 22.0, &mid);
        let before = record.years;
        assert!(close(record.carry_in, 30_000.0, 1e-9));
        mid.record_earnings(&mut record, YearMonth::ym(1997, 7), 2027, 30_000.0, 1.0);
        assert_eq!(record.carry_in, 0.0);
        assert_eq!(record.years, before + 1);
        assert!(close(
            record.indexed[before],
            60_000.0 / mid.wage_index(2027, 1.0),
            1e-6
        ));
    }

    #[test]
    fn projection_before_sixty_is_close_to_the_realised_benefit() {
        let m = model_at(YearMonth::ym(2027, 1));
        let birth = YearMonth::ym(1987, 1);
        // Works to 49 (through 2036), then stops. Project at the end of 2036.
        let mut record = EarningsRecord::backfill(90_000.0, 40.0, 22.0, &m);
        let inflation = PROJECTION_INFLATION;
        let mut price = 1.0;
        let mut projected = 0.0;
        for year in 2027..=2049 {
            price *= 1.0 + inflation;
            let earned = if year <= 2036 { 90_000.0 * price } else { 0.0 };
            m.record_earnings(&mut record, birth, year, earned, price);
            if year == 2036 {
                projected = m.pia_real(&record, birth, YearMonth::ym(2036, 12), price);
            }
        }
        let realised = m.pia_real(&record, birth, YearMonth::ym(2049, 12), price);
        assert!(record.awi_at_60 > 0.0 && record.price_at_62 > 0.0);
        // With inflation at the projection assumption the two agree.
        assert!(
            close(projected, realised, 0.002 * realised),
            "{projected} {realised}"
        );
        assert!(realised > 2_500.0 && realised < 3_800.0, "{realised}");
    }

    fn couple(
        pia_a: Option<f64>,
        claim_a: f64,
        pia_b: Option<f64>,
        claim_b: f64,
    ) -> [Option<Claimant>; MAX_PEOPLE] {
        [
            Some(Claimant::new(YearMonth::ym(1970, 1), claim_a, pia_a)),
            Some(Claimant::new(YearMonth::ym(1970, 1), claim_b, pia_b)),
        ]
    }

    #[test]
    fn own_benefit_by_claim_age_with_cola() {
        let m = model();
        for (claim, factor) in [(62.0, 0.70), (67.0, 1.00), (70.0, 1.24)] {
            let people = [
                Some(Claimant::new(YearMonth::ym(1970, 1), claim, Some(2_000.0))),
                None,
            ];
            let claim_month = YearMonth::ym(1970 + claim as i32, 1);
            assert_eq!(m.monthly(claim_month + -1, &people, 1.0).total(), 0.0);
            let first = m.monthly(claim_month, &people, 1.0);
            assert!(close(first.paid[0], 2_000.0 * factor, 1e-5));
            assert_eq!(first.withheld, [0.0, 0.0]);
            // Benefits follow the price level one for one.
            let later = m.monthly(YearMonth::ym(2050, 6), &people, 1.75);
            assert!(close(later.paid[0], 2_000.0 * factor * 1.75, 1e-4));
        }
        // No basis, dead, not yet born, or nobody at all: nothing is paid.
        let none = [
            Some(Claimant::new(YearMonth::ym(1950, 1), 67.0, None)),
            None,
        ];
        assert_eq!(m.monthly(YearMonth::ym(2030, 1), &none, 1.0).total(), 0.0);
        let mut dead = Claimant::new(YearMonth::ym(1950, 1), 67.0, Some(2_000.0));
        dead.alive = false;
        dead.died = Some(YearMonth::ym(2028, 1));
        assert_eq!(
            m.monthly(YearMonth::ym(2030, 1), &[Some(dead), None], 1.0)
                .total(),
            0.0
        );
        let unborn = Claimant::new(YearMonth::ym(2040, 1), 67.0, Some(2_000.0));
        assert_eq!(
            m.monthly(YearMonth::ym(2030, 1), &[None, Some(unborn)], 1.0)
                .total(),
            0.0
        );
        assert_eq!(
            m.monthly(YearMonth::ym(2030, 1), &[None, None], 1.0)
                .total(),
            0.0
        );
    }

    #[test]
    fn spousal_top_up() {
        let m = model();
        let at = YearMonth::ym(2040, 1); // both 70
                                         // Worker PIA 3,000; spouse PIA 500; both claim at 67: the spouse is topped
                                         // up to half the worker's PIA.
        let paid = m.monthly(at, &couple(Some(3_000.0), 67.0, Some(500.0), 67.0), 1.0);
        assert!(close(paid.paid[0], 3_000.0, 1e-6));
        assert!(close(paid.paid[1], 1_500.0, 1e-6));
        // A spouse with no record of their own: 50% at full retirement age, 32.5%
        // when both claim at 62.
        let paid = m.monthly(at, &couple(Some(3_000.0), 67.0, Some(0.0), 67.0), 1.0);
        assert!(close(paid.paid[1], 1_500.0, 1e-6));
        let paid = m.monthly(at, &couple(Some(3_000.0), 62.0, Some(0.0), 62.0), 1.0);
        assert!(close(paid.paid[0], 2_100.0, 1e-4));
        assert!(close(paid.paid[1], 975.0, 1e-4));
        // Own 500 at 62 (70% = 350) plus the excess 1,000 at 65%: 1,000.
        let paid = m.monthly(at, &couple(Some(3_000.0), 62.0, Some(500.0), 62.0), 1.0);
        assert!(close(paid.paid[1], 350.0 + 650.0, 1e-4));
        // No delayed credits on the spousal part: own 500 x 1.24 + 1,000.
        let paid = m.monthly(at, &couple(Some(3_000.0), 67.0, Some(500.0), 70.0), 1.0);
        assert!(close(paid.paid[1], 620.0 + 1_000.0, 1e-4));
        // Nothing spousal until the worker has filed: at 68 the worker (claiming at
        // 70) has not, so the spouse who claimed at 62 gets only their own.
        let waiting = couple(Some(3_000.0), 70.0, Some(500.0), 62.0);
        let paid = m.monthly(YearMonth::ym(2038, 1), &waiting, 1.0);
        assert_eq!(paid.paid[0], 0.0);
        assert!(close(paid.paid[1], 350.0, 1e-4));
        // Once the worker files at 70 the top-up starts unreduced (the spouse is
        // past full retirement age by then).
        let paid = m.monthly(YearMonth::ym(2040, 1), &waiting, 1.0);
        assert!(close(paid.paid[0], 3_720.0, 1e-4));
        assert!(close(paid.paid[1], 350.0 + 1_000.0, 1e-4));
        // Similar earners: no top-up either way.
        let paid = m.monthly(at, &couple(Some(2_000.0), 67.0, Some(1_800.0), 67.0), 1.0);
        assert!(close(paid.paid[0], 2_000.0, 1e-6));
        assert!(close(paid.paid[1], 1_800.0, 1e-6));
        // A partner excluded from Social Security gets and gives nothing.
        let paid = m.monthly(at, &couple(Some(3_000.0), 67.0, None, 67.0), 1.0);
        assert!(close(paid.paid[0], 3_000.0, 1e-6));
        assert_eq!(paid.paid[1], 0.0);
        // A younger spouse: born 1975, claims at 62 in 2037 when the worker (who
        // claimed at 67 in 2037) has already filed.
        let mut pair = couple(Some(3_000.0), 67.0, Some(0.0), 62.0);
        pair[1] = Some(Claimant::new(YearMonth::ym(1975, 1), 62.0, Some(0.0)));
        assert_eq!(m.monthly(YearMonth::ym(2036, 12), &pair, 1.0).paid[1], 0.0);
        assert!(close(
            m.monthly(YearMonth::ym(2037, 1), &pair, 1.0).paid[1],
            975.0,
            1e-4
        ));
    }

    #[test]
    fn survivor_keeps_the_larger_benefit() {
        let m = model();
        let widow = |own: f64, claim: f64| Claimant::new(YearMonth::ym(1970, 1), claim, Some(own));
        let deceased = |pia: f64, claim: f64, died: YearMonth| {
            let mut c = Claimant::new(YearMonth::ym(1970, 1), claim, Some(pia));
            c.alive = false;
            c.died = Some(died);
            c
        };
        let at = YearMonth::ym(2045, 1); // survivor is 75
                                         // The worker claimed at 70 (3,000 x 1.24 = 3,720) and died at 73; the
                                         // survivor (own 1,500 at 67) steps up to 3,720.
        let people = [
            Some(deceased(3_000.0, 70.0, YearMonth::ym(2043, 1))),
            Some(widow(1_500.0, 67.0)),
        ];
        let paid = m.monthly(at, &people, 1.0);
        assert_eq!(paid.paid[0], 0.0);
        assert!(close(paid.paid[1], 3_720.0, 1e-4));
        // The survivor's own benefit is larger: they keep it.
        let people = [
            Some(deceased(1_000.0, 67.0, YearMonth::ym(2043, 1))),
            Some(widow(2_500.0, 67.0)),
        ];
        assert!(close(m.monthly(at, &people, 1.0).paid[1], 2_500.0, 1e-6));
        // The worker claimed at 62 (70%): the widow's limit lifts it to 82.5% of
        // the PIA.
        let people = [
            Some(deceased(3_000.0, 62.0, YearMonth::ym(2043, 1))),
            Some(widow(0.0, 67.0)),
        ];
        assert!(close(m.monthly(at, &people, 1.0).paid[1], 2_475.0, 1e-6));
        // The worker died at 55 before claiming: the survivor gets the PIA, from
        // age 60 at 71.5%.
        let people = [
            Some(deceased(3_000.0, 67.0, YearMonth::ym(2025, 1))),
            Some(widow(1_000.0, 67.0)),
        ];
        assert_eq!(
            m.monthly(YearMonth::ym(2029, 12), &people, 1.0).paid[1],
            0.0
        );
        assert!(close(
            m.monthly(YearMonth::ym(2030, 1), &people, 1.0).paid[1],
            0.715 * 3_000.0,
            1e-6
        ));
        // ... and the reduction stays for life; their own 1,000 never exceeds it.
        assert!(close(
            m.monthly(at, &people, 1.0).paid[1],
            0.715 * 3_000.0,
            1e-6
        ));
        // Widowed at 63.5, halfway from 60 to the survivor full age of 67:
        // 71.5% + 28.5% x 42/84 = 85.75%.
        let people = [
            Some(deceased(3_000.0, 67.0, YearMonth::ym(2033, 7))),
            Some(widow(0.0, 67.0)),
        ];
        assert!(close(
            m.monthly(YearMonth::ym(2033, 7), &people, 1.0).paid[1],
            0.8575 * 3_000.0,
            1e-6
        ));
        // The worker died at 69 without claiming (planned 70): delayed credits to
        // the month of death, 3,000 x 1.16.
        let people = [
            Some(deceased(3_000.0, 70.0, YearMonth::ym(2039, 1))),
            Some(widow(0.0, 67.0)),
        ];
        assert!(close(m.monthly(at, &people, 1.0).paid[1], 3_480.0, 1e-4));
        // A survivor who has not reached their own claim age gets the survivor
        // benefit alone, then the larger of the two.
        let people = [
            Some(deceased(1_000.0, 67.0, YearMonth::ym(2037, 6))),
            Some(widow(2_000.0, 70.0)),
        ];
        assert!(close(
            m.monthly(YearMonth::ym(2038, 1), &people, 1.0).paid[1],
            1_000.0,
            1e-6
        ));
        assert!(close(
            m.monthly(YearMonth::ym(2040, 1), &people, 1.0).paid[1],
            2_480.0,
            1e-4
        ));
        // No death month recorded, or the deceased had no Social Security: own only.
        let mut unknown = deceased(3_000.0, 67.0, YearMonth::ym(2043, 1));
        unknown.died = None;
        let people = [Some(unknown), Some(widow(1_500.0, 67.0))];
        assert!(close(m.monthly(at, &people, 1.0).paid[1], 1_500.0, 1e-6));
        let mut excluded = deceased(3_000.0, 67.0, YearMonth::ym(2043, 1));
        excluded.pia_real = None;
        let people = [Some(excluded), Some(widow(1_500.0, 67.0))];
        assert!(close(m.monthly(at, &people, 1.0).paid[1], 1_500.0, 1e-6));
    }

    #[test]
    fn earnings_test_withholds_and_gives_back() {
        let m = model();
        // Born January 1964: 62 in January 2026, full retirement age January 2031.
        let mut person = Claimant::new(YearMonth::ym(1964, 1), 62.0, Some(3_000.0));
        let benefit = 3_000.0 * 0.70;
        // Earning $50,000 in 2026: (50,000 - 24,480) / 2 = 12,760 a year, 1,063.33
        // a month.
        person.annual_earnings = 50_000.0;
        let paid = m.monthly(YearMonth::ym(2026, 3), &[Some(person), None], 1.0);
        assert!(close(paid.withheld[0], 12_760.0 / 12.0, 1e-6));
        assert!(close(paid.paid[0], benefit - 12_760.0 / 12.0, 1e-4));
        assert!(close(
            paid.withheld_months[0],
            12_760.0 / 12.0 / benefit,
            1e-6
        ));
        // Below the limit nothing is withheld.
        person.annual_earnings = 24_000.0;
        let paid = m.monthly(YearMonth::ym(2026, 3), &[Some(person), None], 1.0);
        assert_eq!(paid.withheld[0], 0.0);
        assert!(close(paid.paid[0], benefit, 1e-4));
        // High pay withholds the whole benefit, never more.
        person.annual_earnings = 400_000.0;
        let paid = m.monthly(YearMonth::ym(2026, 3), &[Some(person), None], 1.0);
        assert!(close(paid.withheld[0], benefit, 1e-4));
        assert_eq!(paid.paid[0], 0.0);
        assert_eq!(paid.withheld_months[0], 1.0);
        // In the year full retirement age is reached the limit is higher and the
        // rate one in three. Born June 1964: full age in June 2031; in March 2031
        // the limits are projected from the wage index.
        let mut person = Claimant::new(YearMonth::ym(1964, 6), 62.0, Some(3_000.0));
        person.annual_earnings = 110_000.0;
        let limits = m.earnings_test_limits(2031, 1.1);
        assert!(limits[1] > 70_000.0 && limits[1] < 95_000.0);
        let paid = m.monthly(YearMonth::ym(2031, 3), &[Some(person), None], 1.1);
        let expected = (110_000.0 - limits[1]) / 3.0 / 12.0;
        assert!(expected < benefit);
        assert!(close(paid.withheld[0], expected, 1e-6));
        // From full retirement age there is no test, and months withheld are
        // credited: 12 months withheld turn a claim at 62 into one at 63 (75%).
        person.withheld_months = 12.0;
        let paid = m.monthly(YearMonth::ym(2031, 6), &[Some(person), None], 1.0);
        assert_eq!(paid.withheld[0], 0.0);
        assert!(close(paid.paid[0], 3_000.0 * 0.75, 1e-4));
        // The credit cannot go beyond full retirement age.
        person.withheld_months = 500.0;
        let paid = m.monthly(YearMonth::ym(2031, 6), &[Some(person), None], 1.0);
        assert!(close(paid.paid[0], 3_000.0, 1e-4));
        // Before full retirement age the credit is not applied yet.
        person.annual_earnings = 0.0;
        let paid = m.monthly(YearMonth::ym(2030, 6), &[Some(person), None], 1.0);
        assert!(close(paid.paid[0], benefit, 1e-4));
    }

    #[test]
    fn trust_fund_haircut() {
        let cut = SocialSecurityHaircut {
            from_year: 2033,
            factor: 0.79,
        };
        let m = SocialSecurityModel::new(data(), Some(cut), YearMonth::ym(2026, 1));
        assert_eq!(m.payable_fraction(2032), 1.0);
        assert_eq!(m.payable_fraction(2033), 0.79);
        assert_eq!(m.payable_fraction(2090), 0.79);
        assert_eq!(model().payable_fraction(2090), 1.0);
        // Born 1960, claiming at 67 (2027).
        let people = [
            Some(Claimant::new(YearMonth::ym(1960, 1), 67.0, Some(2_500.0))),
            Some(Claimant::new(YearMonth::ym(1960, 1), 67.0, Some(0.0))),
        ];
        let before = m.monthly(YearMonth::ym(2032, 12), &people, 1.2);
        assert!(close(before.paid[0], 3_000.0, 1e-6));
        assert!(close(before.paid[1], 1_500.0, 1e-6));
        let after = m.monthly(YearMonth::ym(2033, 1), &people, 1.2);
        assert!(close(after.paid[0], 3_000.0 * 0.79, 1e-6));
        assert!(close(after.paid[1], 1_500.0 * 0.79, 1e-6));
        // Out-of-range factors are clamped.
        let odd = SocialSecurityModel::new(
            data(),
            Some(SocialSecurityHaircut {
                from_year: 2030,
                factor: 1.7,
            }),
            YearMonth::ym(2026, 1),
        );
        assert_eq!(odd.payable_fraction(2031), 1.0);
    }

    #[test]
    fn known_benefit_basis() {
        assert_eq!(
            BenefitBasis::from(&SocialSecurityBasis::KnownBenefit {
                monthly_at_fra: 2_800.0
            }),
            BenefitBasis::KnownPia(2_800.0)
        );
        assert_eq!(
            BenefitBasis::from(&SocialSecurityBasis::KnownBenefit {
                monthly_at_fra: -5.0
            }),
            BenefitBasis::KnownPia(0.0)
        );
        assert_eq!(
            BenefitBasis::from(&SocialSecurityBasis::default()),
            BenefitBasis::FromEarnings
        );
        assert_eq!(
            BenefitBasis::from(&SocialSecurityBasis::None),
            BenefitBasis::None
        );
        // The statement figure is the PIA in today's dollars: paid in full at 67,
        // and 30% less at 62, scaled by the price level.
        let m = model();
        let person = Claimant::new(YearMonth::ym(1975, 4), 62.0, Some(2_800.0));
        let paid = m.monthly(YearMonth::ym(2040, 1), &[Some(person), None], 1.4);
        assert!(close(paid.paid[0], 2_800.0 * 0.7 * 1.4, 1e-4));
    }

    #[test]
    fn present_value_of_benefits() {
        let m = model();
        // Born January 1966: 60 at the start (January 2026), claims at 67.
        let birth = YearMonth::ym(1966, 1);
        let person = Claimant::new(birth, 67.0, Some(2_000.0));
        // Certain survival for 30 years, no discounting: 23 years of benefits.
        let mut certain = vec![1.0; 360];
        certain.push(0.0);
        let value = m.present_value(&person, m.start(), 0.0, &certain);
        assert!(close(value, 2_000.0 * 12.0 * 23.0, 1e-6));
        // Scaling the survival slice changes nothing.
        let scaled: Vec<f64> = certain.iter().map(|s| 0.3 * s).collect();
        assert!(close(
            m.present_value(&person, m.start(), 0.0, &scaled),
            value,
            1e-6
        ));
        // 2% real discounting: a 23-year monthly annuity-due deferred 7 years.
        let i = math::annual_to_monthly(0.02);
        let v = 1.0 / (1.0 + i);
        let annuity = (1.0 - math::powi(v, 276)) / (1.0 - v) * math::powi(v, 84);
        let discounted = m.present_value(&person, m.start(), 0.02, &certain);
        assert!(close(discounted, 2_000.0 * annuity, 1e-6));
        // Claiming at 70 pays 24% more for 3 fewer years.
        let late = Claimant::new(birth, 70.0, Some(2_000.0));
        assert!(close(
            m.present_value(&late, m.start(), 0.0, &certain),
            2_480.0 * 12.0 * 20.0,
            1e-2
        ));
        // Later in life: 10 years in (age 70), 20 years remain.
        let later = m.present_value(&person, YearMonth::ym(2036, 1), 0.0, &certain[120..]);
        assert!(close(later, 2_000.0 * 12.0 * 20.0, 1e-6));
        // The table gives the same numbers.
        let table = m.annuity(birth, 67.0, 0.02, &certain);
        assert!(close(table.value(0, 2_000.0), discounted, 1e-6));
        let at_120 = m.present_value(&person, YearMonth::ym(2036, 1), 0.02, &certain[120..]);
        assert!(close(table.value(120, 2_000.0), at_120, 1e-6));
        assert_eq!(table.value(360, 2_000.0), 0.0);
        assert_eq!(table.value(100_000, 2_000.0), 0.0);

        // With real mortality and the haircut, table and direct sum still agree and
        // the haircut lowers the value.
        let mortality = crate::health::MortalityModel::new(
            birth,
            crate::model::Sex::Female,
            crate::model::HealthStatus::Average,
            m.start(),
            &crate::model::Longevity::default(),
            data(),
        );
        let survival = mortality.survival_table();
        let cut =
            SocialSecurityModel::new(data(), Some(SocialSecurityHaircut::default()), m.start());
        let table = m.annuity(birth, 67.0, 0.02, survival);
        let table_cut = cut.annuity(birth, 67.0, 0.02, survival);
        for t in [0usize, 50, 84, 200, 400] {
            let now = m.start() + t as i32;
            let direct = m.present_value(&person, now, 0.02, &survival[t..]);
            assert!(close(
                table.value(t as u32, 2_000.0),
                direct,
                1e-6 * direct.max(1.0)
            ));
            let direct_cut = cut.present_value(&person, now, 0.02, &survival[t..]);
            assert!(close(
                table_cut.value(t as u32, 2_000.0),
                direct_cut,
                1e-6 * direct_cut.max(1.0)
            ));
            assert!(direct_cut < direct);
            assert!(direct_cut > 0.78 * direct);
        }
        // A 60-year-old woman's benefit of $2,000 at 67 is worth roughly 17 years
        // of payments discounted: between $250k and $350k.
        let value = table.value(0, 2_000.0);
        assert!(value > 250_000.0 && value < 350_000.0, "{value}");
        // Degenerate inputs.
        assert_eq!(m.present_value(&person, m.start(), 0.02, &[]), 0.0);
        assert_eq!(m.present_value(&person, m.start(), 0.02, &[0.0, 0.0]), 0.0);
        let excluded = Claimant::new(birth, 67.0, None);
        assert_eq!(m.present_value(&excluded, m.start(), 0.02, &certain), 0.0);
        assert!(m.annuity(birth, 67.0, 0.02, &[]).value(0, 1.0) == 0.0);
    }

    #[test]
    fn monthly_is_deterministic_and_finite_for_odd_inputs() {
        let m = model();
        let mut a = Claimant::new(YearMonth::ym(1968, 9), 64.5, Some(1e9));
        a.annual_earnings = f64::MAX;
        let mut b = Claimant::new(YearMonth::ym(1972, 2), 80.0, Some(-100.0));
        b.withheld_months = -3.0;
        let people = [Some(a), Some(b)];
        for month in 0..600 {
            let now = YearMonth::ym(2026, 1) + month;
            let x = m.monthly(now, &people, 1.0 + f64::from(month) * 0.01);
            let y = m.monthly(now, &people, 1.0 + f64::from(month) * 0.01);
            assert_eq!(x, y);
            for i in 0..MAX_PEOPLE {
                assert!(x.paid[i].is_finite() && x.paid[i] >= 0.0);
                assert!(x.withheld[i].is_finite() && x.withheld[i] >= 0.0);
                assert!((0.0..=1.0).contains(&x.withheld_months[i]));
            }
        }
    }
}
