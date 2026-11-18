//! Results. **Partly implemented**: the [`Accumulator`] container, its exact merge and
//! its binary codec work and are tested; recording into it (`impl Recorder`),
//! [`Summary`] and the trace recorder are `todo!()` for the stats owner.
//!
//! Responsibility: everything that leaves the engine.
//!
//! * [`Recorder`] is what the simulation loop writes into: one [`YearRecord`] each
//!   December and one [`PathEnd`] per path, plus monthly detail when tracing.
//! * [`Accumulator`] is the aggregate recorder: fixed-size histograms and sums, so
//!   memory does not grow with the number of paths except for the per-path scalar
//!   columns. It is what a worker returns for a chunk of paths.
//! * [`Summary`] is the JSON the UI and the AI read, derived from an accumulator.
//! * [`PathTrace`] is the full ledger of one path.
//!
//! Contract:
//!
//! * Every money value handed to a recorder is in **today's dollars**.
//! * `merge` is exact, commutative and associative: integer counts add, money sums are
//!   kept as whole-dollar `i64` (rounded per path-year, so no floating-point
//!   reassociation), and per-path scalar chunks are kept sorted by first path index.
//!   Any chunking and any merge order therefore gives identical bytes.
//! * Series and scalar identities are the enums below. Their `key()` strings are the
//!   names the UI and stored results use: never rename one, only add at the end.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::YearMonth;

/// Defines a fieldless enum with a dense index, a stable snake_case key and `ALL`.
macro_rules! keyed_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$vmeta:meta])* $variant:ident => $key:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($(#[$vmeta])* $variant,)+ }

        impl $name {
            /// Every variant in index order.
            pub const ALL: &'static [$name] = &[$($name::$variant,)+];
            /// Number of variants.
            pub const COUNT: usize = Self::ALL.len();

            /// Dense index into per-series arrays.
            pub const fn index(self) -> usize { self as usize }

            /// Stable name used in JSON output and stored results.
            pub const fn key(self) -> &'static str {
                match self { $($name::$variant => $key,)+ }
            }
        }
    };
}

keyed_enum! {
    /// Quantities whose full distribution is kept for every year (histograms).
    HistSeries {
        /// Net worth: investable assets plus home equity less debts.
        NetWorth => "net_worth",
        /// Investable assets: cash, taxable, traditional, Roth, HSA.
        Investable => "investable",
        /// Total household spending in the year.
        Spending => "spending",
        /// Gross income from all sources before tax.
        GrossIncome => "gross_income",
        /// Total tax paid in the year.
        TotalTax => "total_tax",
    }
}

keyed_enum! {
    /// Where a household stands at the end of a year (the "rich, broke or dead" view).
    Status {
        /// Not yet retired.
        Working => "working",
        /// Retired and spending the full planned budget.
        RetiredOnPlan => "retired_on_plan",
        /// Retired with spending cut below plan by a spending rule.
        RetiredCutting => "retired_cutting",
        /// Liquid assets exhausted while someone is alive.
        Depleted => "depleted",
        /// Everyone has died.
        Dead => "dead",
    }
}

keyed_enum! {
    /// Per-year balances and flows that are summed across paths for composition
    /// charts. Balances are year-end; flows are totals for the calendar year.
    YearSeries {
        /// Balance: cash accounts.
        BalanceCash => "balance_cash",
        /// Balance: taxable brokerage.
        BalanceTaxable => "balance_taxable",
        /// Balance: traditional retirement accounts.
        BalanceTraditional => "balance_traditional",
        /// Balance: Roth accounts.
        BalanceRoth => "balance_roth",
        /// Balance: HSAs.
        BalanceHsa => "balance_hsa",
        /// Balance: equity in all properties.
        HomeEquity => "home_equity",
        /// Balance: non-mortgage debt outstanding (positive number).
        Debt => "debt",
        /// Income: wages, bonuses, commission and vested equity.
        IncomeWages => "income_wages",
        /// Income: self-employment net profit.
        IncomeSelfEmployment => "income_self_employment",
        /// Income: Social Security benefits.
        IncomeSocialSecurity => "income_social_security",
        /// Income: pensions and annuities.
        IncomePensions => "income_pensions",
        /// Income: net rental cash flow.
        IncomeRental => "income_rental",
        /// Income: interest and dividends in taxable accounts.
        IncomeInvestment => "income_investment",
        /// Income: everything else, including windfalls.
        IncomeOther => "income_other",
        /// Withdrawals from cash.
        WithdrawCash => "withdraw_cash",
        /// Withdrawals from taxable accounts.
        WithdrawTaxable => "withdraw_taxable",
        /// Withdrawals from traditional accounts, including RMDs.
        WithdrawTraditional => "withdraw_traditional",
        /// Withdrawals from Roth accounts.
        WithdrawRoth => "withdraw_roth",
        /// Withdrawals from HSAs.
        WithdrawHsa => "withdraw_hsa",
        /// Roth conversions (a transfer, not spending).
        RothConversion => "roth_conversion",
        /// Tax: federal income tax including capital gains and NIIT.
        TaxFederal => "tax_federal",
        /// Tax: state income tax.
        TaxState => "tax_state",
        /// Tax: Social Security, Medicare and self-employment tax.
        TaxPayroll => "tax_payroll",
        /// Tax: early-withdrawal and other penalties.
        TaxPenalties => "tax_penalties",
        /// Spending: essential budget lines.
        SpendEssential => "spend_essential",
        /// Spending: discretionary budget lines.
        SpendDiscretionary => "spend_discretionary",
        /// Spending: housing costs of owned homes (mortgage, tax, insurance, upkeep).
        SpendHousing => "spend_housing",
        /// Spending: modelled healthcare premiums and out-of-pocket costs.
        SpendHealthcare => "spend_healthcare",
        /// Spending: long-term care.
        SpendLongTermCare => "spend_long_term_care",
        /// Spending: modelled child costs and college.
        SpendChildren => "spend_children",
        /// Spending: debt payments.
        SpendDebt => "spend_debt",
        /// Spending: irregular shocks and one-off event expenses.
        SpendOneOff => "spend_one_off",
        /// Saving: into cash.
        SaveCash => "save_cash",
        /// Saving: into taxable accounts.
        SaveTaxable => "save_taxable",
        /// Saving: employee contributions to traditional accounts.
        SaveTraditional => "save_traditional",
        /// Saving: contributions to Roth accounts.
        SaveRoth => "save_roth",
        /// Saving: HSA contributions (employee and employer).
        SaveHsa => "save_hsa",
        /// Saving: employer match.
        SaveEmployerMatch => "save_employer_match",
        /// Planned retirement spending before any rule adjustment (for the
        /// delivered-versus-planned comparison).
        SpendPlanned => "spend_planned",
    }
}

keyed_enum! {
    /// One number per path, stored for every path as `f32`. `NaN` means "did not
    /// happen" (for example no depletion).
    PathScalar {
        /// Age of the first person when the retirement trigger was first satisfied.
        FiAge => "fi_age",
        /// Age of the first person at retirement.
        RetirementAge => "retirement_age",
        /// Age of the first person when liquid assets ran out; NaN if never.
        DepletionAge => "depletion_age",
        /// Age at death of the first person (or age at the horizon if still alive).
        DeathAge0 => "death_age_0",
        /// Age at death of the second person; NaN in a one-person household.
        DeathAge1 => "death_age_1",
        /// Net worth at the end of the path.
        TerminalNetWorth => "terminal_net_worth",
        /// Lowest investable balance after retirement.
        MinInvestable => "min_investable",
        /// Total tax paid over the path.
        LifetimeTax => "lifetime_tax",
        /// Total spending over the path.
        LifetimeSpending => "lifetime_spending",
        /// Retirement spending delivered divided by spending planned (1.0 = on plan).
        DeliveredSpendingRatio => "delivered_spending_ratio",
        /// Annualised real portfolio return over the first ten years of retirement.
        EarlyRetirementReturn => "early_retirement_return",
        /// Worst peak-to-trough fall in the investable balance, as a positive fraction.
        WorstDrawdown => "worst_drawdown",
        /// Total months any person spent unemployed.
        MonthsUnemployed => "months_unemployed",
        /// Total long-term care cost.
        LongTermCareCost => "long_term_care_cost",
        /// Highest net worth reached.
        PeakNetWorth => "peak_net_worth",
        /// First cause of failure as a `FailureCause` index; NaN if the path succeeded.
        FailureCause => "failure_cause",
        /// 1.0 if the path met the success definition, else 0.0.
        Success => "success",
    }
}

keyed_enum! {
    /// Attribution of a failed path to its first cause.
    FailureCause {
        /// Poor market returns early in retirement.
        MarketSequence => "market_sequence",
        /// Unemployment before retirement.
        JobLoss => "job_loss",
        /// Medical or long-term care costs.
        Health => "health",
        /// Outliving the money despite average returns.
        Longevity => "longevity",
        /// Planned spending too high for the savings even on a median path.
        Overspending => "overspending",
    }
}

/// Number of bins in each annual histogram.
pub const HIST_BINS: usize = 512;

/// Number of paths at the start of a run whose annual series are kept verbatim.
pub const SAMPLE_PATHS: u64 = 96;

/// One household-year, in today's dollars, as recorded each December.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct YearRecord {
    /// Status at year end.
    pub status: Status,
    /// Values for the histogrammed series, indexed by `HistSeries::index()`.
    pub hist: [f64; HistSeries::COUNT],
    /// Values for the summed series, indexed by `YearSeries::index()`.
    pub values: [f64; YearSeries::COUNT],
}

impl Default for YearRecord {
    fn default() -> Self {
        Self {
            status: Status::Working,
            hist: [0.0; HistSeries::COUNT],
            values: [0.0; YearSeries::COUNT],
        }
    }
}

/// The per-path scalars, indexed by `PathScalar::index()`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathEnd {
    /// One value per [`PathScalar`]; `NaN` where not applicable.
    pub scalars: [f32; PathScalar::COUNT],
}

impl Default for PathEnd {
    fn default() -> Self {
        Self {
            scalars: [f32::NAN; PathScalar::COUNT],
        }
    }
}

/// One month of one path for tracing: nominal and real views of every balance and
/// flow. The trace owner may add columns; consumers address them by key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonthRecord {
    /// Calendar month.
    pub month: YearMonth,
    /// Price level relative to the start (to convert between real and nominal).
    pub price_level: f64,
    /// Month-to-date flows and month-end balances in today's dollars, indexed by
    /// `YearSeries::index()`.
    pub values: [f64; YearSeries::COUNT],
    /// Net worth at month end, today's dollars.
    pub net_worth: f64,
}

/// An entry in a traced path's event log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceEvent {
    /// When it happened.
    pub month: YearMonth,
    /// Stable kind, for example "laid_off", "bought_home", "roth_conversion",
    /// "guardrail_cut", "retired", "depleted", "died".
    pub kind: String,
    /// Human-readable sentence, for example "Roth conversion of $38,000".
    pub message: String,
    /// The amount involved in today's dollars, when there is one.
    pub amount: Option<f64>,
}

/// What the simulation loop writes into. The loop is generic over the recorder, so
/// the aggregate run pays nothing for tracing.
pub trait Recorder {
    /// Called once before a path's first month.
    fn begin_path(&mut self, path_index: u64);

    /// Called each December (and at the final month if the path ends mid-year) with
    /// the year's record. `year_index` is 0 for the calendar year of `profile.start`.
    fn record_year(&mut self, year_index: usize, record: &YearRecord);

    /// Called once after a path's last month.
    fn end_path(&mut self, end: &PathEnd);

    /// Whether the loop should build and pass monthly records and events. The
    /// aggregate recorder says no, and the loop skips the work.
    fn wants_detail(&self) -> bool {
        false
    }

    /// Monthly detail; only called when [`Recorder::wants_detail`] is true.
    fn record_month(&mut self, record: &MonthRecord) {}

    /// An event-log entry; only called when [`Recorder::wants_detail`] is true.
    fn record_event(&mut self, event: TraceEvent) {}

    /// The finished tax return for a year; only called when detail is wanted.
    fn record_tax_return(&mut self, year: i32, detail: &crate::tax::TaxDetail) {}
}

/// The per-path scalars of a contiguous run of paths, row-major: path `first_path + r`
/// occupies `values[r * PathScalar::COUNT ..][.. PathScalar::COUNT]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScalarChunk {
    /// Index of the first path in the chunk.
    pub first_path: u64,
    /// Row-major scalar values.
    pub values: Vec<f32>,
}

impl ScalarChunk {
    /// Number of paths in the chunk.
    pub fn paths(&self) -> u64 {
        (self.values.len() / PathScalar::COUNT) as u64
    }
}

/// Annual series of one of the first [`SAMPLE_PATHS`] paths, for spaghetti plots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SamplePath {
    /// Which path.
    pub path_index: u64,
    /// Net worth at each year end, today's dollars.
    pub net_worth: Vec<f32>,
    /// Investable assets at each year end.
    pub investable: Vec<f32>,
    /// Spending in each year.
    pub spending: Vec<f32>,
}

/// Mergeable aggregate of any number of paths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Accumulator {
    /// Layout version of this struct, checked on merge and decode.
    pub version: u32,
    /// Number of calendar years covered (horizon).
    pub years: u32,
    /// Number of paths recorded.
    pub paths: u64,
    /// Histogram counts, `[series][year][bin]` flattened:
    /// `HistSeries::COUNT * years * HIST_BINS` entries.
    pub hist: Vec<u32>,
    /// Status counts, `[year][status]` flattened.
    pub status: Vec<u32>,
    /// Sums in whole today's dollars, `[series][year]` flattened. Integer so that
    /// merging is exact in any order.
    pub sums: Vec<i64>,
    /// Per-path scalars, as chunks sorted by `first_path` and never overlapping.
    pub scalars: Vec<ScalarChunk>,
    /// Sample paths sorted by `path_index`.
    pub samples: Vec<SamplePath>,
}

/// Why two accumulators cannot be merged or bytes cannot be decoded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AccumulatorError {
    /// The accumulators come from runs with different horizons or layouts.
    #[error("accumulators are incompatible: {0}")]
    Incompatible(String),
    /// The same path appears in both.
    #[error("paths {0}..{1} are present in both accumulators")]
    Overlap(u64, u64),
    /// The bytes are not a serialised accumulator of this version.
    #[error("cannot decode accumulator: {0}")]
    Decode(String),
}

impl Accumulator {
    /// Layout version written by this build.
    pub const VERSION: u32 = 1;

    /// An empty accumulator for a horizon of `years` calendar years.
    pub fn new(years: u32) -> Accumulator {
        let y = years as usize;
        Accumulator {
            version: Self::VERSION,
            years,
            paths: 0,
            hist: vec![0; HistSeries::COUNT * y * HIST_BINS],
            status: vec![0; y * Status::COUNT],
            sums: vec![0; YearSeries::COUNT * y],
            scalars: Vec::new(),
            samples: Vec::new(),
        }
    }

    /// Adds another accumulator's paths to this one. Exact and order-independent.
    /// Fails without modifying `self` if the two are incompatible or share paths.
    pub fn merge(&mut self, other: &Accumulator) -> Result<(), AccumulatorError> {
        if self.version != other.version || self.years != other.years {
            return Err(AccumulatorError::Incompatible(format!(
                "version {} / {} years versus version {} / {} years",
                self.version, self.years, other.version, other.years
            )));
        }
        if self.hist.len() != other.hist.len()
            || self.status.len() != other.status.len()
            || self.sums.len() != other.sums.len()
        {
            return Err(AccumulatorError::Incompatible("array sizes differ".into()));
        }
        let mut chunks: Vec<&ScalarChunk> = self.scalars.iter().chain(&other.scalars).collect();
        chunks.sort_by_key(|c| c.first_path);
        for pair in chunks.windows(2) {
            let end = pair[0].first_path + pair[0].paths();
            if pair[1].first_path < end {
                return Err(AccumulatorError::Overlap(
                    pair[1].first_path,
                    end.min(pair[1].first_path + pair[1].paths()),
                ));
            }
        }
        // Coalesce adjacent chunks so the representation does not depend on how the
        // run was split.
        let mut merged: Vec<ScalarChunk> = Vec::with_capacity(chunks.len());
        for c in chunks {
            match merged.last_mut() {
                Some(last) if last.first_path + last.paths() == c.first_path => {
                    last.values.extend_from_slice(&c.values);
                }
                _ => merged.push(c.clone()),
            }
        }
        self.scalars = merged;

        for (a, b) in self.hist.iter_mut().zip(&other.hist) {
            *a += b;
        }
        for (a, b) in self.status.iter_mut().zip(&other.status) {
            *a += b;
        }
        for (a, b) in self.sums.iter_mut().zip(&other.sums) {
            *a += b;
        }
        self.paths += other.paths;
        self.samples.extend(other.samples.iter().cloned());
        self.samples.sort_by_key(|s| s.path_index);
        Ok(())
    }

    /// Serialises to the compact binary form passed between workers (postcard).
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("Accumulator serialisation cannot fail")
    }

    /// Decodes [`Accumulator::to_bytes`] output.
    pub fn from_bytes(bytes: &[u8]) -> Result<Accumulator, AccumulatorError> {
        let acc: Accumulator =
            postcard::from_bytes(bytes).map_err(|e| AccumulatorError::Decode(e.to_string()))?;
        if acc.version != Self::VERSION {
            return Err(AccumulatorError::Decode(format!(
                "version {} is not supported (expected {})",
                acc.version,
                Self::VERSION
            )));
        }
        Ok(acc)
    }

    /// The scalar column for one quantity across all recorded paths, in path order.
    pub fn scalar_column(&self, scalar: PathScalar) -> Vec<f32> {
        self.scalars
            .iter()
            .flat_map(|c| {
                c.values
                    .chunks_exact(PathScalar::COUNT)
                    .map(move |row| row[scalar.index()])
            })
            .collect()
    }

    /// The path index of each row of [`Accumulator::scalar_column`].
    pub fn path_indices(&self) -> Vec<u64> {
        self.scalars
            .iter()
            .flat_map(|c| c.first_path..c.first_path + c.paths())
            .collect()
    }
}

/// Maps a today's-dollar value to its histogram bin on the signed-log grid, and back.
/// The architecture asks for 512 bins covering -$1B..$1B with the region between
/// -$100 and $100 collapsed; with both signs in 512 bins that is about 6.5% per bin
/// (not 1%), which is still finer than a chart pixel. The stats owner fixes the exact
/// grid here; everything else treats bins as opaque.
pub fn hist_bin(value: f64) -> usize {
    todo!("stats owner")
}

/// Representative value (geometric centre) of a histogram bin.
pub fn hist_bin_value(bin: usize) -> f64 {
    todo!("stats owner")
}

impl Recorder for Accumulator {
    fn begin_path(&mut self, path_index: u64) {
        todo!("stats owner")
    }

    fn record_year(&mut self, year_index: usize, record: &YearRecord) {
        todo!("stats owner")
    }

    fn end_path(&mut self, end: &PathEnd) {
        todo!("stats owner")
    }
}

/// A proportion or mean with its Monte Carlo standard error.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Estimate {
    /// Point estimate.
    pub value: f64,
    /// Standard error from the finite number of paths.
    pub std_error: f64,
}

/// Quantiles of one quantity for each year of the plan.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct QuantileCurves {
    /// Series key (`HistSeries::key()`).
    pub series: String,
    /// The quantile levels, for example [0.05, 0.1, 0.25, 0.5, 0.75, 0.9, 0.95].
    pub levels: Vec<f64>,
    /// `values[i][y]` is quantile `levels[i]` in year `y`, today's dollars.
    pub values: Vec<Vec<f64>>,
}

/// Headline numbers.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Headline {
    /// Share of paths that retire before the maximum age and never run out of money.
    pub success_rate: Estimate,
    /// Share of paths that reach financial independence at all.
    pub fi_rate: Estimate,
    /// Median age of the first person at financial independence (NaN if under half
    /// of paths reach it).
    pub median_fi_age: f64,
    /// 10th and 90th percentile FI age.
    pub fi_age_range: [f64; 2],
    /// Median age of the first person at retirement.
    pub median_retirement_age: f64,
    /// Share of paths on which money runs out while someone is alive.
    pub depletion_rate: Estimate,
    /// Median net worth at the end of the plan, today's dollars.
    pub median_terminal_net_worth: f64,
    /// 10th percentile net worth at the end of the plan.
    pub p10_terminal_net_worth: f64,
    /// Mean ratio of retirement spending delivered to spending planned.
    pub delivered_spending_ratio: Estimate,
    /// Share of retired path-years in which a spending rule had cut spending.
    pub years_cutting_share: f64,
    /// Median lifetime tax, today's dollars.
    pub median_lifetime_tax: f64,
}

/// Share of failures attributed to one first cause.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FailureShare {
    /// The cause.
    pub cause: FailureCause,
    /// Share of all paths (not of failures) that failed for this reason first.
    pub share: f64,
}

/// One summed series as per-path means by year.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct MeanSeries {
    /// Series key (`YearSeries::key()`).
    pub series: String,
    /// Mean across surviving paths in each year, today's dollars.
    pub values: Vec<f64>,
}

/// The JSON document the UI and the AI consume.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Summary {
    /// Number of paths summarised.
    pub paths: u64,
    /// Calendar year of the first entry of every per-year array.
    pub start_year: i32,
    /// Age of the first person in each year (x axis for age-based charts).
    pub ages: Vec<f64>,
    /// Headline metrics.
    pub headline: Headline,
    /// Quantile bands for each histogrammed series.
    pub quantiles: Vec<QuantileCurves>,
    /// Share of paths in each status by year: `status[y][Status::index()]`.
    pub status: Vec<Vec<f64>>,
    /// Cumulative share of paths financially independent by each year.
    pub fi_by_year: Vec<f64>,
    /// Share of paths with at least one person alive in each year.
    pub survival: Vec<f64>,
    /// Mean composition series for stacked charts.
    pub means: Vec<MeanSeries>,
    /// Why paths fail.
    pub failure_attribution: Vec<FailureShare>,
    /// Annual series of the sample paths.
    pub samples: Vec<SamplePath>,
}

/// Run facts the summary needs that the accumulator does not hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SummaryContext {
    /// `profile.start`.
    pub start: YearMonth,
    /// Birth month of the first person.
    pub first_birth: YearMonth,
}

impl Summary {
    /// Derives the summary. Deterministic for a given accumulator.
    pub fn from_accumulator(acc: &Accumulator, ctx: &SummaryContext) -> Summary {
        todo!("stats owner")
    }

    /// The path whose value of `scalar` sits at quantile `q` (0..1) among recorded
    /// paths, so the UI can ask for "the 10th percentile path" and trace it.
    pub fn path_at_quantile(acc: &Accumulator, scalar: PathScalar, q: f64) -> Option<u64> {
        todo!("stats owner")
    }
}

/// One column of a traced path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceColumn {
    /// Column key: a `YearSeries::key()`, "net_worth", "price_level", or an
    /// asset-level key such as "return_us_stocks".
    pub key: String,
    /// One value per month.
    pub values: Vec<f64>,
}

/// The complete ledger of one path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathTrace {
    /// Which path.
    pub path_index: u64,
    /// The run's seed.
    pub seed: u64,
    /// First month.
    pub start: YearMonth,
    /// Number of months simulated.
    pub months: u32,
    /// Monthly columns, today's dollars unless the key ends in `_nominal`.
    pub columns: Vec<TraceColumn>,
    /// The tax return of each calendar year.
    pub tax_returns: Vec<TraceTaxReturn>,
    /// What happened, in order.
    pub events: Vec<TraceEvent>,
    /// The per-path scalars by key.
    pub scalars: Vec<(String, f64)>,
}

/// One year's tax return in a trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceTaxReturn {
    /// Tax year.
    pub year: i32,
    /// Lines as `(key, label, amount, note)`, nominal dollars of that year.
    pub lines: Vec<(String, String, f64, String)>,
}

/// The recorder used by `sim::trace_path`.
#[derive(Clone, Debug)]
pub struct TraceRecorder {
    trace: PathTrace,
}

impl TraceRecorder {
    /// A recorder for one path.
    pub fn new(seed: u64, start: YearMonth) -> TraceRecorder {
        TraceRecorder {
            trace: PathTrace {
                path_index: 0,
                seed,
                start,
                months: 0,
                columns: Vec::new(),
                tax_returns: Vec::new(),
                events: Vec::new(),
                scalars: Vec::new(),
            },
        }
    }

    /// The finished trace.
    pub fn finish(self) -> PathTrace {
        self.trace
    }
}

impl Recorder for TraceRecorder {
    fn begin_path(&mut self, path_index: u64) {
        todo!("stats owner")
    }

    fn record_year(&mut self, year_index: usize, record: &YearRecord) {
        todo!("stats owner")
    }

    fn end_path(&mut self, end: &PathEnd) {
        todo!("stats owner")
    }

    fn wants_detail(&self) -> bool {
        true
    }

    fn record_month(&mut self, record: &MonthRecord) {
        todo!("stats owner")
    }

    fn record_event(&mut self, event: TraceEvent) {
        todo!("stats owner")
    }

    fn record_tax_return(&mut self, year: i32, detail: &crate::tax::TaxDetail) {
        todo!("stats owner")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic chunk covering `first..first + n` with recognisable contents.
    fn chunk(first: u64, n: u64) -> Accumulator {
        let mut a = Accumulator::new(3);
        a.paths = n;
        let mut values = Vec::new();
        for p in first..first + n {
            for s in 0..PathScalar::COUNT {
                values.push((p * 100 + s as u64) as f32);
            }
            a.hist[(p as usize * 7) % (HistSeries::COUNT * 3 * HIST_BINS)] += 1;
            a.status[(p as usize) % (3 * Status::COUNT)] += 1;
            a.sums[(p as usize) % (YearSeries::COUNT * 3)] += p as i64 * 1_000 - 500;
            if p < SAMPLE_PATHS {
                a.samples.push(SamplePath {
                    path_index: p,
                    net_worth: vec![p as f32; 3],
                    investable: vec![0.0; 3],
                    spending: vec![1.0; 3],
                });
            }
        }
        a.scalars.push(ScalarChunk {
            first_path: first,
            values,
        });
        a
    }

    #[test]
    fn enum_keys_are_unique_and_dense() {
        fn check(keys: Vec<&'static str>) {
            let mut sorted = keys.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), keys.len());
        }
        check(YearSeries::ALL.iter().map(|s| s.key()).collect());
        check(PathScalar::ALL.iter().map(|s| s.key()).collect());
        check(HistSeries::ALL.iter().map(|s| s.key()).collect());
        for (i, s) in YearSeries::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
            assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!(s.key()));
        }
        assert_eq!(Status::COUNT, 5);
    }

    #[test]
    fn merge_is_independent_of_chunking_and_order() {
        let whole = chunk(0, 300);
        let parts = [chunk(0, 100), chunk(100, 50), chunk(150, 150)];
        let orders: [[usize; 3]; 4] = [[0, 1, 2], [2, 1, 0], [1, 2, 0], [2, 0, 1]];
        for order in orders {
            let mut acc = Accumulator::new(3);
            for i in order {
                acc.merge(&parts[i]).unwrap();
            }
            assert_eq!(acc, whole, "order {order:?}");
            assert_eq!(acc.to_bytes(), whole.to_bytes());
        }
        assert_eq!(whole.scalar_column(PathScalar::RetirementAge)[5], 501.0);
        assert_eq!(whole.path_indices().len(), 300);
    }

    #[test]
    fn merge_rejects_overlap_and_mismatch() {
        let mut a = chunk(0, 10);
        let before = a.clone();
        assert!(matches!(
            a.merge(&chunk(5, 10)),
            Err(AccumulatorError::Overlap(5, 10))
        ));
        assert_eq!(a, before, "a failed merge must not modify the target");
        assert!(matches!(
            a.merge(&Accumulator::new(4)),
            Err(AccumulatorError::Incompatible(_))
        ));
    }

    #[test]
    fn bytes_round_trip_and_are_compact() {
        let a = chunk(0, 500);
        let bytes = a.to_bytes();
        assert_eq!(Accumulator::from_bytes(&bytes).unwrap(), a);
        // Mostly-zero histograms encode as one byte per bin.
        assert!(bytes.len() < 60_000, "{} bytes", bytes.len());
        assert!(Accumulator::from_bytes(&bytes[..10]).is_err());
    }
}
