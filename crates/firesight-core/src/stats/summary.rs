//! The `Summary`: the single JSON payload the charts and the AI tools consume.
//!
//! Conventions: money is in whole today's dollars; ages are the first person's, in
//! years; shares and probabilities are fractions of 1; a number that does not exist
//! is `null`; per-year arrays are indexed by year (`axis.years`). Distributions by
//! year cover the households with someone alive that year (`axis.alive`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::accumulator::{Accumulator, SamplePath};
use super::hist::{self, mag_edge, MAG_BINS, ZERO_BIN};
use super::num::{
    censored_quantile, moments, nan, nan_vec, nan_vec2, quantile_sorted, round_dp, sorted_finite,
    Estimate, Spread,
};
use super::{FailureCause, HistSeries, PathScalar, ScalarUnit, SeriesKind, Status, YearSeries};
use crate::model::YearMonth;

/// The quantile levels of every curve and distribution.
pub const QUANTILE_LEVELS: [f64; 9] = [0.01, 0.05, 0.10, 0.25, 0.50, 0.75, 0.90, 0.95, 0.99];
/// Fine histogram bins merged into one density-grid cell (8 cells per doubling of
/// value, about 9% each: finer than a chart pixel row on a log axis).
pub const DENSITY_GROUP: usize = 8;
/// Number of equally likely outcomes in a quantile dot plot (Kay et al. 2016 use 20
/// to 100; 100 reads as "of 100 lives").
pub const DOTS: usize = 100;
/// Bins in the histogram of a per-path scalar.
pub const DISTRIBUTION_BINS: usize = 40;
/// Largest number of paths in the scatter sample.
pub const SCATTER_MAX: usize = 5_000;
/// Scalars carried by the scatter sample.
pub const SCATTER_COLUMNS: [PathScalar; 8] = [
    PathScalar::EarlyRetirementReturn,
    PathScalar::TerminalNetWorth,
    PathScalar::FiAge,
    PathScalar::RetirementAge,
    PathScalar::DepletionAge,
    PathScalar::DeathAge0,
    PathScalar::InvestableAtRetirement,
    PathScalar::Success,
];
/// Version of the summary layout.
pub const SUMMARY_VERSION: u32 = 1;
/// A factor is "present" on a path whose value lies in the adverse quarter of all
/// paths (below the 25th or above the 75th percentile).
const ADVERSE_TAIL: f64 = 0.25;
/// The histogram of a scalar spans its 0.5th to 99.5th percentile; outliers fall in
/// the end bins.
const DISTRIBUTION_TRIM: f64 = 0.005;
/// Percentiles of the worst single-year spending cut that are reported.
const WORST_CUT_LEVELS: [f64; 5] = [0.50, 0.75, 0.90, 0.95, 0.99];

/// Decimal places kept in output for a unit.
pub(crate) fn unit_decimals(unit: ScalarUnit) -> u32 {
    match unit {
        ScalarUnit::Dollars | ScalarUnit::Index => 0,
        ScalarUnit::Age | ScalarUnit::Years | ScalarUnit::Months => 2,
        ScalarUnit::Ratio | ScalarUnit::Rate | ScalarUnit::Flag => 4,
    }
}

/// Run facts the summary needs that the accumulator does not hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SummaryContext {
    /// `profile.start`.
    pub start: YearMonth,
    /// Birth month of the first person.
    pub first_birth: YearMonth,
    /// Birth month of the second person, if there is one.
    pub second_birth: Option<YearMonth>,
}

/// The x axis shared by every per-year array.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Axis {
    /// First simulated month.
    pub start: YearMonth,
    /// Age of the first person in the first simulated month.
    pub start_age: f64,
    /// Months covered by year 0 (12 unless the plan starts mid-year). Flow
    /// distributions and mean flows are annualised, so year 0 is comparable.
    pub first_year_months: u32,
    /// Calendar year of each index.
    pub years: Vec<i32>,
    /// Age of the first person in December of each year.
    pub ages: Vec<f64>,
    /// Number of paths with someone alive in each year: the sample behind that
    /// year's quantiles, densities and means.
    pub alive: Vec<u64>,
}

/// Headline numbers. Probabilities are Wilson 95% intervals over all paths.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Headline {
    /// Share of paths that retire by the plan's latest age and never run out of
    /// liquid assets while anyone is alive.
    pub success_rate: Estimate,
    /// Share of paths that ever satisfy the retirement trigger.
    pub fi_rate: Estimate,
    /// Share of paths that retire.
    pub retired_rate: Estimate,
    /// Share of paths on which liquid assets run out while someone is alive.
    pub depletion_rate: Estimate,
    /// Median age at financial independence over ALL paths, counting paths that
    /// never get there as later than any age; null if fewer than half get there.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub median_fi_age: f64,
    /// FI age quantiles over all paths (censored the same way).
    pub fi_age: Spread,
    /// Median retirement age over all paths (censored like `median_fi_age`).
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub median_retirement_age: f64,
    /// Retirement age quantiles over all paths (censored).
    pub retirement_age: Spread,
    /// Age at depletion among the paths that deplete.
    pub depletion_age: Spread,
    /// Years spent depleted among the paths that deplete (duration of shortfall).
    pub years_depleted: Spread,
    /// Median net worth at the end of the plan.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub median_terminal_net_worth: f64,
    /// 10th percentile net worth at the end of the plan.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p10_terminal_net_worth: f64,
    /// Net worth at the end of the plan (the bequest).
    pub terminal_net_worth: Spread,
    /// Investable assets on the retirement date among paths that retire.
    pub investable_at_retirement: Spread,
    /// Mean over retired paths of the share of planned retirement spending that
    /// was delivered (`sum of min(delivered, planned) / sum of planned`): never
    /// above 1.
    pub delivered_spending_ratio: Estimate,
    /// Mean over retired paths of retirement spending above the plan as a fraction
    /// of planned spending (a spending rule raising spending in good times).
    pub extra_spending_ratio: Estimate,
    /// First-year portfolio draw as a fraction of investable assets on the
    /// retirement date, among paths that retire.
    pub initial_withdrawal_rate: Spread,
    /// Share of all paths on which a home was sold to fund retirement after
    /// savings ran out.
    pub home_sale_rate: Estimate,
    /// Share of retired path-years in the `retired_cutting` or `depleted` status.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub years_cutting_share: f64,
    /// Median lifetime tax.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub median_lifetime_tax: f64,
    /// Lifetime tax.
    pub lifetime_tax: Spread,
    /// Lifetime spending.
    pub lifetime_spending: Spread,
    /// Funded ratio at retirement, when the simulation records it.
    pub funded_ratio: Option<Spread>,
}

/// Spending delivered versus planned in retirement ("probability of adjustment" and
/// its size). "Retired paths" are paths with at least one retired year.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct SpendingDelivery {
    /// Number of paths with at least one retired year.
    pub retired_paths: u64,
    /// Share of retired paths with at least one year below 90% of plan.
    pub paths_below_90: Estimate,
    /// Share of retired paths with at least one year below 75% of plan.
    pub paths_below_75: Estimate,
    /// Share of retired path-years below 90% of plan (standard error clustered by
    /// path).
    pub years_below_90: Estimate,
    /// Share of retired path-years below 75% of plan.
    pub years_below_75: Estimate,
    /// Among retired paths with a cut year, the number of years below 90% of plan.
    pub years_cut: Spread,
    /// Levels of `worst_cut`.
    pub worst_cut_levels: Vec<f64>,
    /// Percentiles across retired paths of the worst single-year cut
    /// (1 - delivered / planned, at least 0): `worst_cut[i]` is the cut that
    /// `worst_cut_levels[i]` of retired paths never exceed.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub worst_cut: Vec<f64>,
    /// By year.
    pub by_year: SpendingByYear,
}

/// Spending delivery by year.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct SpendingByYear {
    /// Share of living paths that are retired.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub retired: Vec<f64>,
    /// Share of retired paths delivering under 90% of plan that year.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub below_90: Vec<f64>,
    /// Share of retired paths delivering under 75% of plan that year.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub below_75: Vec<f64>,
}

/// Quantiles of one quantity for each year of the plan, among living paths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QuantileCurves {
    /// Which series.
    pub series: HistSeries,
    /// The quantile levels ([`QUANTILE_LEVELS`]).
    pub levels: Vec<f64>,
    /// `values[i][y]` is quantile `levels[i]` in year `y`; null when nobody is alive.
    #[serde(with = "nan_vec2")]
    #[schemars(with = "Vec<Vec<Option<f64>>>")]
    pub values: Vec<Vec<f64>>,
}

/// The distribution of one series in every year on a coarse value grid, for a
/// density fan. Cell `k` of year `y` covers `edges[starts[y] + k] ..
/// edges[starts[y] + k + 1]` and holds `counts[y][k]` paths; divide by
/// `axis.alive[y]` for a share. Paths at exactly zero are in `zeros`, not in a cell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DensityGrid {
    /// Which series.
    pub series: HistSeries,
    /// Ascending cell edges in dollars covering every occupied cell of every year.
    /// Logarithmic beyond $1,024 in magnitude (8 cells per doubling), $128 steps
    /// inside; includes 0 when the range spans it.
    pub edges: Vec<f64>,
    /// Index into `edges` of each year's first cell.
    pub starts: Vec<u32>,
    /// Path counts of each year's occupied cell range.
    pub counts: Vec<Vec<u32>>,
    /// Paths at exactly zero in each year.
    pub zeros: Vec<u32>,
}

/// Share of all paths in each status by year (the "rich, broke or dead" stack).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct StatusStack {
    /// Status of each row of `shares`, in `Status` order.
    pub statuses: Vec<Status>,
    /// `shares[s][y]`: share of all paths in status `statuses[s]` at the end of year
    /// `y`. The rows sum to 1 in every year.
    pub shares: Vec<Vec<f64>>,
}

/// Survival curves by year.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Survival {
    /// Share of paths with at least one person alive at some point in each year
    /// (`axis.alive / paths`).
    pub household: Vec<f64>,
    /// Share of paths on which each person is alive at some point in each year (one
    /// row per person; a death counts as alive through the year it happens in).
    pub people: Vec<Vec<f64>>,
}

/// One composition series as a mean by year among living households.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MeanSeries {
    /// Which series.
    pub series: YearSeries,
    /// Its kind (how to stack it).
    pub kind: SeriesKind,
    /// Mean per living household: year-end level for balances and liabilities,
    /// annualised flow for everything else. Null when nobody is alive.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub values: Vec<f64>,
}

/// When an event (FI, retirement, depletion) happens, by age of the first person,
/// over ALL paths: paths on which it never happens stay in the denominator.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct AgeCurve {
    /// Whole ages; entry `i` covers ages in `[ages[i], ages[i] + 1)`.
    pub ages: Vec<u32>,
    /// Number of paths on which the event happens at each age (the histogram).
    pub counts: Vec<u64>,
    /// Share of all paths on which it has happened by the end of each age.
    pub cumulative: Vec<f64>,
    /// Lower end of the Wilson 95% interval of `cumulative`.
    pub lo: Vec<f64>,
    /// Upper end of the Wilson 95% interval of `cumulative`.
    pub hi: Vec<f64>,
    /// Share of all paths on which it never happens.
    pub never: f64,
    /// [`DOTS`] equally likely outcomes for a quantile dot plot: the age at
    /// quantile `(i + 0.5) / DOTS` over all paths, null for "never".
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub dots: Vec<f64>,
}

/// The distribution of one per-path scalar across paths that have a value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScalarDistribution {
    /// Which scalar.
    pub scalar: PathScalar,
    /// Its unit.
    pub unit: ScalarUnit,
    /// Paths with a value.
    pub count: u64,
    /// Paths without one (the event did not happen or does not apply).
    pub missing: u64,
    /// Mean.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub mean: f64,
    /// Standard deviation.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub std_dev: f64,
    /// Smallest value.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub min: f64,
    /// Largest value.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub max: f64,
    /// Quantiles at [`QUANTILE_LEVELS`].
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub quantiles: Vec<f64>,
    /// [`DOTS`] equally likely outcomes: quantiles at `(i + 0.5) / DOTS`.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub dots: Vec<f64>,
    /// Histogram bin edges (ascending, `counts.len() + 1` of them), spanning the
    /// 0.5th to 99.5th percentile. Empty when there are no values.
    pub edges: Vec<f64>,
    /// Paths per bin; values beyond the outer edges are in the end bins.
    pub counts: Vec<u64>,
}

/// One column of the scatter sample.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScatterColumn {
    /// Which scalar.
    pub scalar: PathScalar,
    /// One value per sampled path; null where the path has none.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub values: Vec<f64>,
}

/// A deterministic, evenly spaced sample of paths with a few scalars each, for the
/// sequence-risk scatterplot and for picking a path to trace.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Scatter {
    /// Path index of each sampled path.
    pub paths: Vec<u64>,
    /// The columns ([`SCATTER_COLUMNS`]), aligned with `paths`.
    pub columns: Vec<ScatterColumn>,
}

/// Failures attributed to one first cause.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FailureShare {
    /// The cause.
    pub cause: FailureCause,
    /// Number of failed paths with this first cause.
    pub count: u64,
    /// Share of failed paths.
    pub share_of_failed: f64,
    /// Share of all paths.
    pub share_of_all: f64,
}

/// How often an adverse factor is present among failed and successful paths. Factors
/// overlap: a path can carry several.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FactorShare {
    /// Factor key: `poor_early_returns`, `deep_drawdown`, `job_loss`,
    /// `long_term_care`, `long_life` or `high_inflation`.
    pub factor: String,
    /// The scalar it is read from.
    pub scalar: PathScalar,
    /// True if the factor is "value above the threshold", false if below.
    pub above: bool,
    /// The threshold: the 75th (or 25th) percentile of the scalar over all paths.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub threshold: f64,
    /// Share of failed paths with the factor.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub failed: f64,
    /// Share of successful paths with the factor.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub succeeded: f64,
}

/// One scalar compared between failed and successful paths.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScalarContrast {
    /// Which scalar.
    pub scalar: PathScalar,
    /// Failed paths with a value.
    pub failed_count: u64,
    /// Successful paths with a value.
    pub succeeded_count: u64,
    /// Median among failed paths.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub failed_median: f64,
    /// Median among successful paths.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub succeeded_median: f64,
    /// Mean among failed paths.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub failed_mean: f64,
    /// Mean among successful paths.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub succeeded_mean: f64,
}

/// Why paths fail.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct FailureAttribution {
    /// Number of failed paths (not successful).
    pub failed: u64,
    /// Failed paths that never retired.
    pub never_retired: u64,
    /// Failed paths that ran out of liquid assets.
    pub depleted: u64,
    /// First cause chosen by the simulation, every cause listed.
    pub causes: Vec<FailureShare>,
    /// Overlapping adverse factors, failed versus successful paths.
    pub factors: Vec<FactorShare>,
    /// Every scalar compared between failed and successful paths.
    pub contrasts: Vec<ScalarContrast>,
}

/// A path chosen by a stated rule, to trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathPick {
    /// `p10`, `median`, `p90` (by terminal net worth), `median_fi` (median FI age
    /// among paths that reach it) or `failing` (the median depletion age among
    /// failed paths, or the first failed path if none depletes).
    pub label: String,
    /// The scalar the path was ranked by.
    pub scalar: PathScalar,
    /// The path to pass to `trace`.
    pub path_index: u64,
    /// Its value of `scalar`.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub value: f64,
}

/// The JSON document the UI and the AI consume.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Summary {
    /// Layout version ([`SUMMARY_VERSION`]).
    pub version: u32,
    /// Number of paths summarised.
    pub paths: u64,
    /// The x axis of every per-year array.
    pub axis: Axis,
    /// Headline metrics.
    pub headline: Headline,
    /// Spending delivered versus planned.
    pub spending: SpendingDelivery,
    /// Quantile bands for each histogrammed series.
    pub quantiles: Vec<QuantileCurves>,
    /// Density grid for each histogrammed series.
    pub density: Vec<DensityGrid>,
    /// Status stack by year.
    pub status: StatusStack,
    /// Survival by year.
    pub survival: Survival,
    /// Mean composition series for stacked charts, every `YearSeries` in order.
    pub composition: Vec<MeanSeries>,
    /// Financial independence by age.
    pub fi: AgeCurve,
    /// Retirement by age.
    pub retirement: AgeCurve,
    /// Depletion by age.
    pub depletion: AgeCurve,
    /// Distribution of every per-path scalar, in `PathScalar` order.
    pub distributions: Vec<ScalarDistribution>,
    /// Scatter sample.
    pub scatter: Scatter,
    /// Why paths fail.
    pub failure: FailureAttribution,
    /// Representative paths to trace.
    pub picks: Vec<PathPick>,
    /// Annual series of the first sample paths.
    pub samples: Vec<SamplePath>,
}

/// Row access to the scalar table.
struct Table<'a> {
    paths: Vec<u64>,
    rows: Vec<&'a [f32]>,
}

impl<'a> Table<'a> {
    fn new(acc: &'a Accumulator) -> Table<'a> {
        let (paths, rows) = acc.rows().unzip();
        Table { paths, rows }
    }

    fn n(&self) -> u64 {
        self.rows.len() as u64
    }

    fn col(&self, s: PathScalar) -> impl Iterator<Item = f32> + Clone + '_ {
        let i = s.index();
        self.rows.iter().map(move |r| r[i])
    }

    fn sorted(&self, s: PathScalar) -> Vec<f32> {
        sorted_finite(self.col(s))
    }

    fn failed(&self) -> impl Iterator<Item = bool> + Clone + '_ {
        self.col(PathScalar::Success).map(|v| v != 1.0)
    }
}

/// The median of a slice, reordering it. NaN for an empty slice.
fn median_in_place(values: &mut [f32]) -> f64 {
    let n = values.len();
    if n == 0 {
        return f64::NAN;
    }
    let mid = n / 2;
    let (below, at, _) = values.select_nth_unstable_by(mid, f32::total_cmp);
    let hi = f64::from(*at);
    if n % 2 == 1 {
        hi
    } else {
        let lo = below.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        0.5 * (f64::from(lo) + hi)
    }
}

/// The (value, path) pair at quantile `q` by nearest rank, ties broken by path index.
fn pick_at(pairs: &mut [(f32, u64)], q: f64) -> Option<(f32, u64)> {
    if pairs.is_empty() {
        return None;
    }
    let idx = (q.clamp(0.0, 1.0) * (pairs.len() - 1) as f64).round() as usize;
    let (_, at, _) =
        pairs.select_nth_unstable_by(idx, |a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    Some(*at)
}

fn share(k: u64, n: u64) -> f64 {
    if n == 0 {
        f64::NAN
    } else {
        round_dp(k as f64 / n as f64, 5)
    }
}

impl Summary {
    /// Derives the summary. Deterministic for a given accumulator.
    pub fn from_accumulator(acc: &Accumulator, ctx: &SummaryContext) -> Summary {
        let table = Table::new(acc);
        let n = table.n();
        let sorted: Vec<Vec<f32>> = PathScalar::ALL.iter().map(|s| table.sorted(*s)).collect();
        let col = |s: PathScalar| &sorted[s.index()][..];
        let axis = axis(acc, ctx);
        let first_age = axis.start_age.floor() as i32;
        let last_age = axis.ages.last().map_or(first_age, |a| a.floor() as i32);
        let age_curve = |s: PathScalar| age_curve(col(s), n, first_age, last_age);

        Summary {
            version: SUMMARY_VERSION,
            paths: n,
            headline: headline(acc, &table, &sorted),
            spending: spending(acc, &table, &axis),
            quantiles: HistSeries::ALL
                .iter()
                .map(|s| quantile_curves(acc, *s))
                .collect(),
            density: HistSeries::ALL
                .iter()
                .map(|s| density_grid(acc, *s))
                .collect(),
            status: status_stack(acc, &axis, n),
            survival: survival(&table, &axis, ctx, n),
            composition: composition(acc, &axis),
            fi: age_curve(PathScalar::FiAge),
            retirement: age_curve(PathScalar::RetirementAge),
            depletion: age_curve(PathScalar::DepletionAge),
            distributions: PathScalar::ALL
                .iter()
                .map(|s| distribution(*s, col(*s), n))
                .collect(),
            scatter: scatter(&table),
            failure: failure(&table, &sorted),
            picks: picks(&table),
            samples: acc.samples.clone(),
            axis,
        }
    }

    /// The path whose value of `scalar` sits at quantile `q` (0..1) among the paths
    /// that have one (nearest rank, ties broken by path index), so the UI can ask for
    /// "the 10th percentile life" and trace it. `None` if no path has a value.
    pub fn path_at_quantile(acc: &Accumulator, scalar: PathScalar, q: f64) -> Option<u64> {
        let mut pairs: Vec<(f32, u64)> = acc
            .rows()
            .map(|(p, row)| (row[scalar.index()], p))
            .filter(|(v, _)| v.is_finite())
            .collect();
        pick_at(&mut pairs, q).map(|(_, p)| p)
    }
}

fn axis(acc: &Accumulator, ctx: &SummaryContext) -> Axis {
    let start_year = ctx.start.year();
    let years: Vec<i32> = (0..acc.years as i32).map(|y| start_year + y).collect();
    Axis {
        start: ctx.start,
        start_age: round_dp(ctx.first_birth.age_years_at(ctx.start), 2),
        first_year_months: 13 - ctx.start.month(),
        ages: years
            .iter()
            .map(|y| round_dp(ctx.first_birth.age_years_at(YearMonth::ym(*y, 12)), 2))
            .collect(),
        alive: (0..acc.years as usize).map(|y| acc.alive(y)).collect(),
        years,
    }
}

fn headline(acc: &Accumulator, table: &Table, sorted: &[Vec<f32>]) -> Headline {
    let n = table.n();
    let col = |s: PathScalar| &sorted[s.index()][..];
    let finite = |s: PathScalar| col(s).len() as u64;
    let dollars = |s: PathScalar| Spread::of(col(s), 0);
    let successes = table.col(PathScalar::Success).filter(|v| *v == 1.0).count() as u64;
    let fi_age = Spread::censored(col(PathScalar::FiAge), n, 2);
    let retirement_age = Spread::censored(col(PathScalar::RetirementAge), n, 2);
    let terminal = dollars(PathScalar::TerminalNetWorth);
    let lifetime_tax = dollars(PathScalar::LifetimeTax);
    let depleted_years: Vec<f32> = col(PathScalar::YearsDepleted)
        .iter()
        .copied()
        .filter(|v| *v > 0.0)
        .collect();

    let status_total = |s: Status| -> u64 {
        (0..acc.years as usize)
            .map(|y| u64::from(acc.status[y * Status::COUNT + s.index()]))
            .sum()
    };
    let cutting = status_total(Status::RetiredCutting) + status_total(Status::Depleted);
    let retired_years =
        cutting + status_total(Status::RetiredOnPlan) + status_total(Status::RetiredSurplus);

    Headline {
        success_rate: Estimate::proportion(successes, n).rounded(5),
        fi_rate: Estimate::proportion(finite(PathScalar::FiAge), n).rounded(5),
        retired_rate: Estimate::proportion(finite(PathScalar::RetirementAge), n).rounded(5),
        depletion_rate: Estimate::proportion(finite(PathScalar::DepletionAge), n).rounded(5),
        median_fi_age: fi_age.p50,
        fi_age,
        median_retirement_age: retirement_age.p50,
        retirement_age,
        depletion_age: Spread::of(col(PathScalar::DepletionAge), 2),
        years_depleted: Spread::of(&depleted_years, 1),
        median_terminal_net_worth: terminal.p50,
        p10_terminal_net_worth: terminal.p10,
        terminal_net_worth: terminal,
        investable_at_retirement: dollars(PathScalar::InvestableAtRetirement),
        delivered_spending_ratio: Estimate::mean(
            col(PathScalar::DeliveredSpendingRatio)
                .iter()
                .map(|v| f64::from(*v)),
        )
        .rounded(4),
        extra_spending_ratio: Estimate::mean(
            col(PathScalar::ExtraSpendingRatio)
                .iter()
                .map(|v| f64::from(*v)),
        )
        .rounded(4),
        initial_withdrawal_rate: Spread::of(col(PathScalar::InitialWithdrawalRate), 4),
        home_sale_rate: Estimate::proportion(finite(PathScalar::HomeSaleAge), n).rounded(5),
        years_cutting_share: share(cutting, retired_years),
        median_lifetime_tax: lifetime_tax.p50,
        lifetime_tax,
        lifetime_spending: dollars(PathScalar::LifetimeSpending),
        funded_ratio: (finite(PathScalar::FundedRatio) > 0)
            .then(|| Spread::of(col(PathScalar::FundedRatio), 4)),
    }
}

fn spending(acc: &Accumulator, table: &Table, axis: &Axis) -> SpendingDelivery {
    let years_retired = table.col(PathScalar::YearsRetired);
    let retired_paths = years_retired.clone().filter(|v| *v > 0.0).count() as u64;
    let among_retired = |s: PathScalar| {
        table
            .col(s)
            .zip(years_retired.clone())
            .filter(|(_, r)| *r > 0.0)
            .map(|(v, r)| (f64::from(v), f64::from(r)))
    };
    let paths_with = |s: PathScalar| {
        Estimate::proportion(
            among_retired(s).filter(|(v, _)| *v > 0.0).count() as u64,
            retired_paths,
        )
        .rounded(5)
    };
    let cut_years = sorted_finite(
        among_retired(PathScalar::YearsBelow90)
            .filter(|(v, _)| *v > 0.0)
            .map(|(v, _)| v as f32),
    );
    // Worst single-year cut per retired path, as a fraction of plan.
    let worst = sorted_finite(
        table
            .col(PathScalar::WorstSpendingRatio)
            .filter(|r| r.is_finite())
            .map(|r| (1.0 - r).max(0.0)),
    );
    let per_year = |k: usize, of: &dyn Fn(usize) -> u64| -> Vec<f64> {
        (0..acc.years as usize)
            .map(|y| share(u64::from(acc.cuts[y * 3 + k]), of(y)))
            .collect()
    };
    let retired_in = |y: usize| u64::from(acc.cuts[y * 3]);
    SpendingDelivery {
        retired_paths,
        paths_below_90: paths_with(PathScalar::YearsBelow90),
        paths_below_75: paths_with(PathScalar::YearsBelow75),
        years_below_90: Estimate::ratio(among_retired(PathScalar::YearsBelow90)).rounded(5),
        years_below_75: Estimate::ratio(among_retired(PathScalar::YearsBelow75)).rounded(5),
        years_cut: Spread::of(&cut_years, 1),
        worst_cut_levels: WORST_CUT_LEVELS.to_vec(),
        worst_cut: WORST_CUT_LEVELS
            .iter()
            .map(|q| round_dp(quantile_sorted(&worst, *q), 4))
            .collect(),
        by_year: SpendingByYear {
            retired: per_year(0, &|y| axis.alive[y]),
            below_90: per_year(1, &retired_in),
            below_75: per_year(2, &retired_in),
        },
    }
}

fn quantile_curves(acc: &Accumulator, series: HistSeries) -> QuantileCurves {
    let years = acc.years as usize;
    let mut values = vec![vec![0.0; years]; QUANTILE_LEVELS.len()];
    let mut out = [0.0; QUANTILE_LEVELS.len()];
    for y in 0..years {
        hist::quantiles(acc.histogram(series, y), &QUANTILE_LEVELS, &mut out);
        for (row, v) in values.iter_mut().zip(out) {
            row[y] = round_dp(v, 0);
        }
    }
    QuantileCurves {
        series,
        levels: QUANTILE_LEVELS.to_vec(),
        values,
    }
}

/// Coarse cells on each side of zero.
const DENSITY_CELLS: usize = MAG_BINS / DENSITY_GROUP;

/// Edge `j` of the coarse grid, `j` in `0..=2 * DENSITY_CELLS`, ascending; the middle
/// edge is 0.
fn density_edge(j: usize) -> f64 {
    if j >= DENSITY_CELLS {
        mag_edge((j - DENSITY_CELLS) * DENSITY_GROUP)
    } else {
        -mag_edge((DENSITY_CELLS - j) * DENSITY_GROUP)
    }
}

fn density_grid(acc: &Accumulator, series: HistSeries) -> DensityGrid {
    let years = acc.years as usize;
    let cells = 2 * DENSITY_CELLS;
    let mut coarse = vec![0u32; years * cells];
    let mut zeros = vec![0u32; years];
    for y in 0..years {
        let fine = acc.histogram(series, y);
        let row = &mut coarse[y * cells..(y + 1) * cells];
        zeros[y] = fine[ZERO_BIN];
        // Negative bins ascend towards zero; group them from zero outwards so cell
        // boundaries mirror the positive side.
        for (i, chunk) in fine[..ZERO_BIN].rchunks(DENSITY_GROUP).enumerate() {
            row[DENSITY_CELLS - 1 - i] = chunk.iter().sum();
        }
        for (i, chunk) in fine[ZERO_BIN + 1..].chunks(DENSITY_GROUP).enumerate() {
            row[DENSITY_CELLS + i] = chunk.iter().sum();
        }
    }
    let occupied = |y: usize| {
        let row = &coarse[y * cells..(y + 1) * cells];
        let first = row.iter().position(|c| *c != 0)?;
        let last = row.iter().rposition(|c| *c != 0)?;
        Some((first, last))
    };
    let ranges: Vec<Option<(usize, usize)>> = (0..years).map(occupied).collect();
    let lo = ranges.iter().flatten().map(|r| r.0).min();
    let hi = ranges.iter().flatten().map(|r| r.1).max();
    let (Some(lo), Some(hi)) = (lo, hi) else {
        return DensityGrid {
            series,
            edges: Vec::new(),
            starts: vec![0; years],
            counts: vec![Vec::new(); years],
            zeros,
        };
    };
    DensityGrid {
        series,
        edges: (lo..=hi + 1).map(density_edge).collect(),
        starts: ranges
            .iter()
            .map(|r| r.map_or(0, |r| (r.0 - lo) as u32))
            .collect(),
        counts: ranges
            .iter()
            .enumerate()
            .map(|(y, r)| {
                r.map_or(Vec::new(), |r| {
                    coarse[y * cells + r.0..=y * cells + r.1].to_vec()
                })
            })
            .collect(),
        zeros,
    }
}

fn status_stack(acc: &Accumulator, axis: &Axis, n: u64) -> StatusStack {
    let years = acc.years as usize;
    let shares = Status::ALL
        .iter()
        .map(|s| {
            (0..years)
                .map(|y| {
                    let count = if *s == Status::Dead {
                        n.saturating_sub(axis.alive[y])
                    } else {
                        u64::from(acc.status[y * Status::COUNT + s.index()])
                    };
                    if n == 0 {
                        0.0
                    } else {
                        round_dp(count as f64 / n as f64, 5)
                    }
                })
                .collect()
        })
        .collect();
    StatusStack {
        statuses: Status::ALL.to_vec(),
        shares,
    }
}

/// Share of people alive at some point in each year, from their death ages.
fn person_survival(
    death_ages: impl Iterator<Item = f32>,
    birth: YearMonth,
    axis: &Axis,
) -> Option<Vec<f64>> {
    let years = axis.years.len();
    let first_december = f64::from(YearMonth::ym(axis.start.year(), 12).ordinal());
    let mut dead_by = vec![0u64; years + 1];
    let mut people = 0u64;
    for age in death_ages.filter(|a| a.is_finite()) {
        people += 1;
        // Ages are whole months; rounding removes the f32 noise. A person counts as
        // alive in every year up to and including the year of their death, which is
        // how the household's records end too.
        let death = f64::from(birth.ordinal()) + (f64::from(age) * 12.0).round();
        let y = (((death - first_december) / 12.0).ceil() + 1.0).max(0.0) as usize;
        dead_by[y.min(years)] += 1;
    }
    if people == 0 {
        return None;
    }
    let mut dead = 0u64;
    Some(
        dead_by[..years]
            .iter()
            .map(|d| {
                dead += d;
                round_dp(1.0 - dead as f64 / people as f64, 5)
            })
            .collect(),
    )
}

fn survival(table: &Table, axis: &Axis, ctx: &SummaryContext, n: u64) -> Survival {
    let mut people = Vec::new();
    people.extend(person_survival(
        table.col(PathScalar::DeathAge0),
        ctx.first_birth,
        axis,
    ));
    if let Some(birth) = ctx.second_birth {
        people.extend(person_survival(
            table.col(PathScalar::DeathAge1),
            birth,
            axis,
        ));
    }
    Survival {
        household: axis
            .alive
            .iter()
            .map(|a| if n == 0 { 0.0 } else { share(*a, n) })
            .collect(),
        people,
    }
}

fn composition(acc: &Accumulator, axis: &Axis) -> Vec<MeanSeries> {
    let years = acc.years as usize;
    YearSeries::ALL
        .iter()
        .map(|s| MeanSeries {
            series: *s,
            kind: s.kind(),
            values: (0..years)
                .map(|y| {
                    let sum = acc.sums[s.index() * years + y] as f64;
                    // Stocks: per living household. Flows: per household-year, so a
                    // partial first or last year does not dilute the mean.
                    let per = if s.is_stock() {
                        axis.alive[y] as f64
                    } else {
                        acc.months[y] as f64 / 12.0
                    };
                    if per > 0.0 {
                        round_dp(sum / per, 0)
                    } else {
                        f64::NAN
                    }
                })
                .collect(),
        })
        .collect()
}

fn age_curve(sorted: &[f32], total: u64, first_age: i32, last_age: i32) -> AgeCurve {
    let bins = (last_age - first_age + 1).max(1) as usize;
    let mut counts = vec![0u64; bins];
    for v in sorted {
        let i = (v.floor() as i32 - first_age).clamp(0, bins as i32 - 1);
        counts[i as usize] += 1;
    }
    let mut cum = 0u64;
    let estimates: Vec<Estimate> = counts
        .iter()
        .map(|c| {
            cum += c;
            Estimate::proportion(cum, total).rounded(5)
        })
        .collect();
    let zero_if_nan = |v: f64| if v.is_nan() { 0.0 } else { v };
    AgeCurve {
        ages: (0..bins as i32)
            .map(|i| (first_age + i).max(0) as u32)
            .collect(),
        counts,
        cumulative: estimates.iter().map(|e| zero_if_nan(e.value)).collect(),
        lo: estimates.iter().map(|e| zero_if_nan(e.lo)).collect(),
        hi: estimates.iter().map(|e| zero_if_nan(e.hi)).collect(),
        never: if total == 0 {
            0.0
        } else {
            share(total - (sorted.len() as u64).min(total), total)
        },
        dots: (0..DOTS)
            .map(|i| {
                let q = (i as f64 + 0.5) / DOTS as f64;
                round_dp(censored_quantile(sorted, total, q), 2)
            })
            .collect(),
    }
}

fn distribution(scalar: PathScalar, sorted: &[f32], n: u64) -> ScalarDistribution {
    let unit = scalar.unit();
    let dp = unit_decimals(unit);
    let (count, mean, sd) = moments(sorted.iter().map(|v| f64::from(*v)));
    let q = |q: f64| round_dp(quantile_sorted(sorted, q), dp);
    let (edges, counts) = if sorted.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let lo = quantile_sorted(sorted, DISTRIBUTION_TRIM);
        let hi = quantile_sorted(sorted, 1.0 - DISTRIBUTION_TRIM);
        if hi > lo {
            let edge_dp = (dp + 2).min(6);
            let edges: Vec<f64> = (0..=DISTRIBUTION_BINS)
                .map(|i| {
                    round_dp(
                        lo + (hi - lo) * i as f64 / DISTRIBUTION_BINS as f64,
                        edge_dp,
                    )
                })
                .collect();
            let below = |e: f64| sorted.partition_point(|v| f64::from(*v) < e) as u64;
            let counts = (0..DISTRIBUTION_BINS)
                .map(|i| {
                    let from = if i == 0 { 0 } else { below(edges[i]) };
                    let to = if i + 1 == DISTRIBUTION_BINS {
                        sorted.len() as u64
                    } else {
                        below(edges[i + 1])
                    };
                    to.saturating_sub(from)
                })
                .collect();
            (edges, counts)
        } else {
            // All but the trimmed tails share one value.
            (vec![q(0.0), q(1.0)], vec![sorted.len() as u64])
        }
    };
    ScalarDistribution {
        scalar,
        unit,
        count,
        missing: n.saturating_sub(count),
        mean: if count == 0 {
            f64::NAN
        } else {
            round_dp(mean, (dp + 2).min(6))
        },
        std_dev: if count == 0 {
            f64::NAN
        } else {
            round_dp(sd, (dp + 2).min(6))
        },
        min: q(0.0),
        max: q(1.0),
        quantiles: QUANTILE_LEVELS.iter().map(|l| q(*l)).collect(),
        dots: if sorted.is_empty() {
            Vec::new()
        } else {
            (0..DOTS)
                .map(|i| q((i as f64 + 0.5) / DOTS as f64))
                .collect()
        },
        edges,
        counts,
    }
}

fn scatter(table: &Table) -> Scatter {
    let n = table.rows.len();
    let take = n.min(SCATTER_MAX);
    // Evenly spaced rows: path indices carry no information, so a systematic sample
    // is as good as a random one and is stable from run to run.
    let picks: Vec<usize> = (0..take).map(|i| i * n / take.max(1)).collect();
    Scatter {
        paths: picks.iter().map(|r| table.paths[*r]).collect(),
        columns: SCATTER_COLUMNS
            .iter()
            .map(|s| ScatterColumn {
                scalar: *s,
                values: picks
                    .iter()
                    .map(|r| {
                        round_dp(
                            f64::from(table.rows[*r][s.index()]),
                            unit_decimals(s.unit()),
                        )
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// The overlapping adverse factors: key, scalar, and whether high values are adverse.
const FACTORS: [(&str, PathScalar, bool); 6] = [
    (
        "poor_early_returns",
        PathScalar::EarlyRetirementReturn,
        false,
    ),
    ("deep_drawdown", PathScalar::WorstDrawdown, true),
    ("job_loss", PathScalar::MonthsUnemployed, true),
    ("long_term_care", PathScalar::LongTermCareCost, true),
    ("long_life", PathScalar::YearsSimulated, true),
    ("high_inflation", PathScalar::Inflation, true),
];

fn failure(table: &Table, sorted: &[Vec<f32>]) -> FailureAttribution {
    let n = table.n();
    let failed = table.failed().filter(|f| *f).count() as u64;
    let succeeded = n - failed;
    let mut cause_counts = [0u64; FailureCause::COUNT];
    let mut never_retired = 0;
    let mut depleted = 0;
    for row in table
        .rows
        .iter()
        .filter(|r| r[PathScalar::Success.index()] != 1.0)
    {
        let cause = row[PathScalar::FailureCause.index()];
        let cause = if cause >= 0.0 {
            FailureCause::from_index(cause as usize).unwrap_or(FailureCause::Other)
        } else {
            FailureCause::Other
        };
        cause_counts[cause.index()] += 1;
        never_retired += u64::from(row[PathScalar::RetirementAge.index()].is_nan());
        depleted += u64::from(row[PathScalar::DepletionAge.index()].is_finite());
    }
    let zero_if_nan = |v: f64| if v.is_nan() { 0.0 } else { v };

    let factors = FACTORS
        .iter()
        .map(|&(key, scalar, above)| {
            let level = if above {
                1.0 - ADVERSE_TAIL
            } else {
                ADVERSE_TAIL
            };
            let threshold = quantile_sorted(&sorted[scalar.index()], level);
            let (mut in_failed, mut in_succeeded) = (0u64, 0u64);
            for (v, is_failed) in table.col(scalar).zip(table.failed()) {
                let v = f64::from(v);
                let present = if above { v > threshold } else { v < threshold };
                if present {
                    if is_failed {
                        in_failed += 1;
                    } else {
                        in_succeeded += 1;
                    }
                }
            }
            FactorShare {
                factor: key.to_string(),
                scalar,
                above,
                threshold: round_dp(threshold, unit_decimals(scalar.unit())),
                failed: share(in_failed, failed),
                succeeded: share(in_succeeded, succeeded),
            }
        })
        .collect();

    let contrasts = PathScalar::ALL
        .iter()
        .filter(|s| !matches!(s, PathScalar::Success | PathScalar::FailureCause))
        .map(|s| {
            let dp = unit_decimals(s.unit());
            let (mut f, mut ok): (Vec<f32>, Vec<f32>) = (Vec::new(), Vec::new());
            for (v, is_failed) in table.col(*s).zip(table.failed()) {
                if v.is_finite() {
                    if is_failed {
                        f.push(v);
                    } else {
                        ok.push(v);
                    }
                }
            }
            let mean = |v: &[f32]| {
                let (count, mean, _) = moments(v.iter().map(|x| f64::from(*x)));
                if count == 0 {
                    f64::NAN
                } else {
                    round_dp(mean, (dp + 1).min(6))
                }
            };
            ScalarContrast {
                scalar: *s,
                failed_count: f.len() as u64,
                succeeded_count: ok.len() as u64,
                failed_mean: mean(&f),
                succeeded_mean: mean(&ok),
                failed_median: round_dp(median_in_place(&mut f), dp),
                succeeded_median: round_dp(median_in_place(&mut ok), dp),
            }
        })
        .collect();

    FailureAttribution {
        failed,
        never_retired,
        depleted,
        causes: FailureCause::ALL
            .iter()
            .map(|c| FailureShare {
                cause: *c,
                count: cause_counts[c.index()],
                share_of_failed: zero_if_nan(share(cause_counts[c.index()], failed)),
                share_of_all: zero_if_nan(share(cause_counts[c.index()], n)),
            })
            .collect(),
        factors,
        contrasts,
    }
}

fn picks(table: &Table) -> Vec<PathPick> {
    let pairs = |s: PathScalar, only_failed: bool| -> Vec<(f32, u64)> {
        table
            .rows
            .iter()
            .zip(&table.paths)
            .filter(|(r, _)| !only_failed || r[PathScalar::Success.index()] != 1.0)
            .map(|(r, p)| (r[s.index()], *p))
            .filter(|(v, _)| v.is_finite())
            .collect()
    };
    let mut out = Vec::new();
    let mut push = |label: &str, scalar: PathScalar, pick: Option<(f32, u64)>| {
        if let Some((value, path_index)) = pick {
            out.push(PathPick {
                label: label.to_string(),
                scalar,
                path_index,
                value: round_dp(f64::from(value), unit_decimals(scalar.unit())),
            });
        }
    };
    let mut terminal = pairs(PathScalar::TerminalNetWorth, false);
    for (label, q) in [("p10", 0.10), ("median", 0.50), ("p90", 0.90)] {
        push(
            label,
            PathScalar::TerminalNetWorth,
            pick_at(&mut terminal, q),
        );
    }
    push(
        "median_fi",
        PathScalar::FiAge,
        pick_at(&mut pairs(PathScalar::FiAge, false), 0.5),
    );
    let failing = pick_at(&mut pairs(PathScalar::DepletionAge, true), 0.5).or_else(|| {
        table
            .rows
            .iter()
            .zip(&table.paths)
            .find(|(r, _)| r[PathScalar::Success.index()] != 1.0)
            .map(|(_, p)| (f32::NAN, *p))
    });
    push("failing", PathScalar::DepletionAge, failing);
    out
}

/// A probability with its 95% interval, for the digest.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct DigestRate {
    /// Probability (share of paths).
    pub p: f64,
    /// Lower end of the 95% sampling interval.
    pub lo: f64,
    /// Upper end of the 95% sampling interval.
    pub hi: f64,
}

/// Three quantiles, for the digest; null where fewer paths than that share have a
/// value.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DigestSpread {
    /// 10th percentile.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p10: f64,
    /// Median.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p50: f64,
    /// 90th percentile.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub p90: f64,
}

impl Default for DigestSpread {
    fn default() -> Self {
        DigestSpread {
            p10: f64::NAN,
            p50: f64::NAN,
            p90: f64::NAN,
        }
    }
}

/// A cumulative probability at a few ages.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct DigestCurve {
    /// Ages of the first person.
    pub ages: Vec<u32>,
    /// Share of all paths on which the event has happened by the end of that age.
    pub p: Vec<f64>,
}

/// Net worth and annual spending at a few ages, among households still alive.
/// Dollars are rounded to $1,000; null where nobody is alive.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct DigestBands {
    /// Age of the first person.
    pub ages: Vec<u32>,
    /// Share of paths with someone alive at that age.
    pub alive: Vec<f64>,
    /// Net worth, 10th percentile.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub net_worth_p10: Vec<f64>,
    /// Net worth, median.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub net_worth_p50: Vec<f64>,
    /// Net worth, 90th percentile.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub net_worth_p90: Vec<f64>,
    /// Annual spending, 10th percentile.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub spending_p10: Vec<f64>,
    /// Annual spending, median.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub spending_p50: Vec<f64>,
    /// Annual spending, 90th percentile.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub spending_p90: Vec<f64>,
}

/// Spending delivery in the digest.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DigestSpending {
    /// Mean share of planned retirement spending delivered (never above 1).
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub delivered_ratio: f64,
    /// Mean retirement spending above the plan, as a fraction of the plan.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub extra_ratio: f64,
    /// Share of retired paths with a year below 90% of plan.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub paths_below_90: f64,
    /// Share of retired paths with a year below 75% of plan.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub paths_below_75: f64,
    /// Share of retired years below 90% of plan.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub years_below_90: f64,
    /// Worst single-year cut that 90% of retired paths never exceed.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub worst_cut_p90: f64,
}

impl Default for DigestSpending {
    fn default() -> Self {
        DigestSpending {
            delivered_ratio: f64::NAN,
            extra_ratio: f64::NAN,
            paths_below_90: f64::NAN,
            paths_below_75: f64::NAN,
            years_below_90: f64::NAN,
            worst_cut_p90: f64::NAN,
        }
    }
}

/// A failure cause in the digest.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DigestCause {
    /// The cause.
    pub cause: FailureCause,
    /// Share of failed paths.
    pub share: f64,
}

/// Path indices worth tracing.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct DigestPicks {
    /// Path at the 10th percentile of terminal net worth.
    pub p10: Option<u64>,
    /// Path at the median of terminal net worth.
    pub median: Option<u64>,
    /// Path at the 90th percentile of terminal net worth.
    pub p90: Option<u64>,
    /// A representative failed path.
    pub failing: Option<u64>,
}

/// The headline numbers and a handful of curve points, small enough (under 2 KB of
/// JSON) to return to a language model as a tool result.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct SummaryDigest {
    /// What the numbers mean; read this before quoting any of them.
    pub notes: String,
    /// Number of simulated lives.
    pub paths: u64,
    /// First calendar year.
    pub start_year: i32,
    /// Last calendar year of the horizon.
    pub end_year: i32,
    /// Age of the first person at the start.
    pub start_age: f64,
    /// Probability of success.
    pub success: DigestRate,
    /// Probability of ever reaching financial independence.
    pub fi: DigestRate,
    /// Probability of running out of liquid assets while alive.
    pub depletion: DigestRate,
    /// Age at financial independence over all paths.
    pub fi_age: DigestSpread,
    /// Retirement age over all paths.
    pub retirement_age: DigestSpread,
    /// Net worth at the end of the plan.
    pub terminal_net_worth: DigestSpread,
    /// Median lifetime tax.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub lifetime_tax_p50: f64,
    /// Probability of being financially independent by age.
    pub fi_by_age: DigestCurve,
    /// Probability of having run out by age.
    pub depletion_by_age: DigestCurve,
    /// Net worth and annual spending by age.
    pub by_age: DigestBands,
    /// Spending delivered versus planned in retirement.
    pub spending_delivery: DigestSpending,
    /// The most common first causes of failure, at most [`DIGEST_CAUSES`], most
    /// common first.
    pub failure_causes: Vec<DigestCause>,
    /// Paths to trace for a life story.
    pub trace_paths: DigestPicks,
}

/// Explains the digest to a reader who has nothing else.
const DIGEST_NOTES: &str = "Simulated lives; today's dollars; ages are the first person's. \
success = retired by the plan's latest age and never ran out of liquid assets while alive. \
lo/hi = 95% sampling interval (model uncertainty is larger). Age quantiles cover all lives: \
null = fewer than that share ever got there. by_age covers lives still alive at that age.";

/// Most points kept from a curve in the digest.
const DIGEST_POINTS: usize = 6;
/// Most failure causes listed in the digest.
pub const DIGEST_CAUSES: usize = 3;

impl Summary {
    /// The compact form for an LLM tool result.
    pub fn digest(&self) -> SummaryDigest {
        let rate = |e: &Estimate| {
            let z = |v: f64| if v.is_nan() { 0.0 } else { round_dp(v, 3) };
            DigestRate {
                p: z(e.value),
                lo: z(e.lo),
                hi: z(e.hi),
            }
        };
        let spread = |s: &Spread, dp: u32| DigestSpread {
            p10: round_dp(s.p10, dp),
            p50: round_dp(s.p50, dp),
            p90: round_dp(s.p90, dp),
        };
        let thousands = |v: f64| round_dp(v / 1_000.0, 0) * 1_000.0;
        let curve = |c: &AgeCurve| {
            // Every fifth birthday, thinned to every tenth if that is too many.
            let pick = |step: u32| -> Vec<usize> {
                (0..c.ages.len())
                    .filter(|i| c.ages[*i] % step == 0)
                    .collect()
            };
            let mut idx = pick(5);
            if idx.len() > DIGEST_POINTS {
                idx = pick(10);
            }
            DigestCurve {
                ages: idx.iter().map(|i| c.ages[*i]).collect(),
                p: idx.iter().map(|i| round_dp(c.cumulative[*i], 3)).collect(),
            }
        };
        let years = self.axis.years.len();
        let points: Vec<usize> = if years <= DIGEST_POINTS {
            (0..years).collect()
        } else {
            (0..DIGEST_POINTS)
                .map(|i| i * (years - 1) / (DIGEST_POINTS - 1))
                .collect()
        };
        let row = |series: HistSeries, q: f64| -> Vec<f64> {
            let curves = self.quantiles.iter().find(|c| c.series == series);
            let level = QUANTILE_LEVELS.iter().position(|l| *l == q);
            match (curves, level) {
                (Some(c), Some(l)) => points.iter().map(|y| thousands(c.values[l][*y])).collect(),
                _ => Vec::new(),
            }
        };
        let pick = |label: &str| {
            self.picks
                .iter()
                .find(|p| p.label == label)
                .map(|p| p.path_index)
        };
        let mut causes: Vec<DigestCause> = self
            .failure
            .causes
            .iter()
            .filter(|c| c.count > 0)
            .map(|c| DigestCause {
                cause: c.cause,
                share: round_dp(c.share_of_failed, 3),
            })
            .collect();
        causes.sort_by(|a, b| b.share.total_cmp(&a.share));
        causes.truncate(DIGEST_CAUSES);
        let worst_cut_p90 = self
            .spending
            .worst_cut_levels
            .iter()
            .position(|l| *l == 0.90)
            .and_then(|i| self.spending.worst_cut.get(i).copied())
            .unwrap_or(f64::NAN);
        SummaryDigest {
            notes: DIGEST_NOTES.to_string(),
            paths: self.paths,
            start_year: self.axis.years.first().copied().unwrap_or(0),
            end_year: self.axis.years.last().copied().unwrap_or(0),
            start_age: round_dp(self.axis.start_age, 1),
            success: rate(&self.headline.success_rate),
            fi: rate(&self.headline.fi_rate),
            depletion: rate(&self.headline.depletion_rate),
            fi_age: spread(&self.headline.fi_age, 1),
            retirement_age: spread(&self.headline.retirement_age, 1),
            terminal_net_worth: DigestSpread {
                p10: thousands(self.headline.terminal_net_worth.p10),
                p50: thousands(self.headline.terminal_net_worth.p50),
                p90: thousands(self.headline.terminal_net_worth.p90),
            },
            lifetime_tax_p50: thousands(self.headline.median_lifetime_tax),
            fi_by_age: curve(&self.fi),
            depletion_by_age: curve(&self.depletion),
            by_age: DigestBands {
                ages: points
                    .iter()
                    .map(|y| self.axis.ages[*y].round().max(0.0) as u32)
                    .collect(),
                alive: points
                    .iter()
                    .map(|y| round_dp(self.survival.household[*y], 3))
                    .collect(),
                net_worth_p10: row(HistSeries::NetWorth, 0.10),
                net_worth_p50: row(HistSeries::NetWorth, 0.50),
                net_worth_p90: row(HistSeries::NetWorth, 0.90),
                spending_p10: row(HistSeries::Spending, 0.10),
                spending_p50: row(HistSeries::Spending, 0.50),
                spending_p90: row(HistSeries::Spending, 0.90),
            },
            spending_delivery: DigestSpending {
                delivered_ratio: round_dp(self.headline.delivered_spending_ratio.value, 3),
                extra_ratio: round_dp(self.headline.extra_spending_ratio.value, 3),
                paths_below_90: round_dp(self.spending.paths_below_90.value, 3),
                paths_below_75: round_dp(self.spending.paths_below_75.value, 3),
                years_below_90: round_dp(self.spending.years_below_90.value, 3),
                worst_cut_p90: round_dp(worst_cut_p90, 3),
            },
            failure_causes: causes,
            trace_paths: DigestPicks {
                p10: pick("p10"),
                median: pick("median"),
                p90: pick("p90"),
                failing: pick("failing"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::testkit::{Synthetic, START_AGE};
    use crate::stats::{PathEnd, Recorder, YearRecord};

    fn run(kit: &Synthetic, paths: u64) -> Accumulator {
        let mut acc = Accumulator::new(kit.years);
        for p in 0..paths {
            kit.run_path(p, &mut acc);
        }
        acc
    }

    /// Ten hand-built paths over three years (plan starts January 2030, first person
    /// born January 1990, so aged 40 at the start and 40.92, 41.92, 42.92 each
    /// December).
    fn tiny() -> (Accumulator, SummaryContext) {
        let ctx = SummaryContext {
            start: YearMonth::ym(2030, 1),
            first_birth: YearMonth::ym(1990, 1),
            second_birth: None,
        };
        let mut acc = Accumulator::new(3);
        // (FI age, death age, depletion age, terminal net worth in $k)
        let lives: [(f32, f32, f32, f64); 10] = [
            (40.5, 43.0, f32::NAN, 900.0),
            (40.9, 43.0, f32::NAN, 800.0),
            (41.2, 43.0, f32::NAN, 700.0),
            (41.5, 43.0, f32::NAN, 600.0),
            (42.1, 43.0, f32::NAN, 500.0),
            (42.6, 43.0, 42.8, 0.0),
            (f32::NAN, 43.0, f32::NAN, 300.0),
            (f32::NAN, 43.0, f32::NAN, 200.0),
            (f32::NAN, 41.5, f32::NAN, 100.0),
            (f32::NAN, 40.4, f32::NAN, -50.0),
        ];
        for (p, (fi, death, depletion, terminal)) in lives.iter().enumerate() {
            acc.begin_path(p as u64);
            let years = ((death - 40.0).ceil() as usize).min(3);
            for y in 0..years {
                let retired = fi.is_finite() && f64::from(*fi) < 41.0 + y as f64;
                let depleted = depletion.is_finite() && y == 2;
                let mut r = YearRecord {
                    retired,
                    ..YearRecord::default()
                };
                if *terminal >= 0.0 {
                    r.set(YearSeries::BalanceTaxable, terminal * 1_000.0);
                } else {
                    r.set(YearSeries::Debt, -terminal * 1_000.0);
                }
                r.set(
                    YearSeries::SpendEssential,
                    if depleted { 20_000.0 } else { 40_000.0 },
                );
                r.set(YearSeries::SpendPlanned, 40_000.0);
                r.set(YearSeries::TaxFederal, 5_000.0);
                r.status = Status::classify(retired, depleted, r.spending(), 40_000.0, 1.0, 1.0);
                acc.record_year(y, &r);
            }
            let mut end = PathEnd::default();
            end.set(PathScalar::FiAge, f64::from(*fi));
            end.set(PathScalar::RetirementAge, f64::from(*fi));
            end.set(PathScalar::DepletionAge, f64::from(*depletion));
            end.set(PathScalar::DeathAge0, f64::from(*death));
            end.set(
                PathScalar::MonthsUnemployed,
                if p >= 6 { 12.0 } else { 0.0 },
            );
            end.set_outcome(if p < 5 {
                None
            } else if p == 5 {
                Some(FailureCause::MarketSequence)
            } else {
                Some(FailureCause::JobLoss)
            });
            acc.end_path(&end);
        }
        (acc, ctx)
    }

    #[test]
    fn hand_built_summary() {
        let (acc, ctx) = tiny();
        let s = Summary::from_accumulator(&acc, &ctx);
        assert_eq!(s.paths, 10);
        assert_eq!(s.axis.years, [2030, 2031, 2032]);
        assert_eq!(s.axis.ages, [40.92, 41.92, 42.92]);
        assert_eq!(s.axis.start_age, 40.0);
        assert_eq!(s.axis.first_year_months, 12);
        // Path 9 dies at 40.4 (one record), path 8 at 41.5 (two records).
        assert_eq!(s.axis.alive, [10, 9, 8]);

        let h = &s.headline;
        assert_eq!(h.success_rate.value, 0.5);
        assert_eq!((h.success_rate.lo, h.success_rate.hi), (0.23659, 0.76341));
        assert_eq!(h.fi_rate.value, 0.6);
        assert_eq!(h.depletion_rate.value, 0.1);
        // Six of ten reach FI: the median over all ten is the 5th, 42.1; v1 would
        // have reported the median of the six (41.35). The 75th percentile is never
        // reached.
        assert_eq!(h.median_fi_age, 42.1);
        assert_eq!(h.fi_age.p10, 40.5);
        assert_eq!(h.fi_age.p25, 41.2);
        assert!(h.fi_age.p75.is_nan());
        assert_eq!(h.fi_age.count, 6);
        assert_eq!(h.depletion_age.p50, 42.8);
        assert_eq!(h.years_depleted.p50, 1.0);
        // Terminal net worth sorted: -50, 0, 100, 200, 300, 500, ..., 900 ($k).
        assert_eq!(h.median_terminal_net_worth, 400_000.0);
        assert_eq!(h.p10_terminal_net_worth, -5_000.0);
        // Lifetime tax: 5,000 a year for 3, 3, ..., 2, 1 years.
        assert_eq!(h.median_lifetime_tax, 15_000.0);
        assert!(h.funded_ratio.is_none());

        // FI by age, over all ten paths: 2 at 40, 2 at 41, 2 at 42.
        assert_eq!(s.fi.ages, [40, 41, 42]);
        assert_eq!(s.fi.counts, [2, 2, 2]);
        assert_eq!(s.fi.cumulative, [0.2, 0.4, 0.6]);
        assert_eq!(s.fi.never, 0.4);
        assert_eq!(s.fi.dots.len(), DOTS);
        assert_eq!(s.fi.dots[0], 40.5);
        assert_eq!(s.fi.dots[59], 42.6);
        assert!(s.fi.dots[60].is_nan());
        assert_eq!(s.depletion.counts, [0, 0, 1]);
        assert_eq!(s.depletion.cumulative, [0.0, 0.0, 0.1]);
        assert!((s.fi.lo[2] - 0.3127).abs() < 1e-3 && (s.fi.hi[2] - 0.8318).abs() < 1e-3);

        // Status stack: rows sum to one, dead derived.
        let row = |st: Status| &s.status.shares[st.index()];
        assert_eq!(row(Status::Dead), &[0.0, 0.1, 0.2]);
        assert_eq!(row(Status::Depleted), &[0.0, 0.0, 0.1]);
        // Retired by the end of year 0: FI ages 40.5 and 40.9.
        assert_eq!(row(Status::RetiredOnPlan), &[0.2, 0.4, 0.5]);
        assert_eq!(row(Status::Working), &[0.8, 0.5, 0.2]);
        for y in 0..3 {
            let total: f64 = s.status.shares.iter().map(|r| r[y]).sum();
            assert!((total - 1.0).abs() < 1e-9);
        }
        assert_eq!(s.survival.household, [1.0, 0.9, 0.8]);
        // Person 0 dies in 2030 on one path and in 2031 on another: alive in the year
        // of death, gone from the next.
        assert_eq!(s.survival.people, [vec![1.0, 0.9, 0.8]]);

        // Composition: spending per living household.
        let mean = |ys: YearSeries| &s.composition[ys.index()].values;
        assert_eq!(
            mean(YearSeries::SpendPlanned),
            &[40_000.0, 40_000.0, 40_000.0]
        );
        assert_eq!(
            mean(YearSeries::SpendEssential),
            &[40_000.0, 40_000.0, 37_500.0]
        );
        assert_eq!(mean(YearSeries::Debt), &[5_000.0, 0.0, 0.0]);
        assert_eq!(s.composition.len(), YearSeries::COUNT);
        assert_eq!(
            s.composition[YearSeries::Debt.index()].kind,
            SeriesKind::Liability
        );

        // Spending delivery: six paths retire; one of them has one year at 50%.
        let sp = &s.spending;
        assert_eq!(sp.retired_paths, 6);
        assert_eq!(sp.paths_below_90.value, round_dp(1.0 / 6.0, 5));
        assert_eq!(sp.paths_below_75.value, round_dp(1.0 / 6.0, 5));
        // Retired years: 3 + 3 + 2 + 2 + 1 + 1 = 12, one of them cut.
        assert_eq!(sp.years_below_90.value, round_dp(1.0 / 12.0, 5));
        assert_eq!(sp.by_year.retired, [0.2, round_dp(4.0 / 9.0, 5), 0.75]);
        assert_eq!(sp.by_year.below_90, [0.0, 0.0, round_dp(1.0 / 6.0, 5)]);
        assert_eq!(sp.worst_cut[0], 0.0);
        assert_eq!(*sp.worst_cut.last().unwrap(), 0.475);
        assert_eq!(h.years_cutting_share, round_dp(1.0 / 12.0, 5));
        assert_eq!(sp.years_cut.p50, 1.0);

        // Failure attribution.
        let f = &s.failure;
        assert_eq!((f.failed, f.never_retired, f.depleted), (5, 4, 1));
        let cause = |c: FailureCause| f.causes[c.index()];
        assert_eq!(cause(FailureCause::JobLoss).count, 4);
        assert_eq!(cause(FailureCause::JobLoss).share_of_failed, 0.8);
        assert_eq!(cause(FailureCause::JobLoss).share_of_all, 0.4);
        assert_eq!(cause(FailureCause::MarketSequence).share_of_failed, 0.2);
        assert_eq!(cause(FailureCause::Health).count, 0);
        let job = f.factors.iter().find(|x| x.factor == "job_loss").unwrap();
        // 75th percentile of months unemployed over all paths is 12; nobody exceeds it.
        assert_eq!((job.threshold, job.failed, job.succeeded), (12.0, 0.0, 0.0));
        let contrast = f
            .contrasts
            .iter()
            .find(|c| c.scalar == PathScalar::MonthsUnemployed)
            .unwrap();
        assert_eq!(
            (contrast.failed_median, contrast.succeeded_median),
            (12.0, 0.0)
        );
        assert_eq!((contrast.failed_mean, contrast.succeeded_mean), (9.6, 0.0));
        assert_eq!((contrast.failed_count, contrast.succeeded_count), (5, 5));
        let contrast = f
            .contrasts
            .iter()
            .find(|c| c.scalar == PathScalar::TerminalNetWorth)
            .unwrap();
        assert_eq!(contrast.succeeded_median, 700_000.0);
        assert_eq!(contrast.failed_median, 100_000.0);
        assert_eq!(f.contrasts.len(), PathScalar::COUNT - 2);

        // Picks and path lookup.
        assert_eq!(
            Summary::path_at_quantile(&acc, PathScalar::TerminalNetWorth, 0.0),
            Some(9)
        );
        assert_eq!(
            Summary::path_at_quantile(&acc, PathScalar::TerminalNetWorth, 1.0),
            Some(0)
        );
        assert_eq!(
            Summary::path_at_quantile(&acc, PathScalar::FiAge, 0.5),
            Some(3)
        );
        assert_eq!(
            Summary::path_at_quantile(&acc, PathScalar::FundedRatio, 0.5),
            None
        );
        let pick = |label: &str| s.picks.iter().find(|p| p.label == label).unwrap();
        assert_eq!(pick("p10").path_index, 5);
        assert_eq!(pick("p90").path_index, 1);
        assert_eq!(pick("failing").path_index, 5);
        assert_eq!(pick("failing").value, 42.8);
        assert_eq!(pick("median_fi").value, 41.5);

        // Distributions.
        let d = &s.distributions[PathScalar::FiAge.index()];
        assert_eq!((d.count, d.missing), (6, 4));
        assert_eq!((d.min, d.max), (40.5, 42.6));
        assert_eq!(d.counts.iter().sum::<u64>(), 6);
        assert_eq!(d.edges.len(), d.counts.len() + 1);
        let d = &s.distributions[PathScalar::DeathAge1.index()];
        assert_eq!((d.count, d.missing), (0, 10));
        assert!(d.mean.is_nan() && d.edges.is_empty() && d.dots.is_empty());
        let d = &s.distributions[PathScalar::LongTermCareCost.index()];
        assert_eq!(d.count, 0);

        // Scatter carries every path when there are few.
        assert_eq!(s.scatter.paths, (0..10).collect::<Vec<u64>>());
        assert_eq!(s.scatter.columns.len(), SCATTER_COLUMNS.len());
        assert_eq!(s.scatter.columns[2].scalar, PathScalar::FiAge);
        assert_eq!(s.scatter.columns[2].values[0], 40.5);
        assert!(s.scatter.columns[2].values[9].is_nan());
        assert_eq!(s.samples.len(), 10);

        // Quantile curves: year 2 net worth among the eight alive, median between
        // $300k and $500k bins.
        let nw = &s.quantiles[HistSeries::NetWorth.index()];
        assert_eq!(nw.levels, QUANTILE_LEVELS);
        let median = nw.values[4][2];
        assert!((300_000.0..=505_000.0).contains(&median), "{median}");
        let spend = &s.quantiles[HistSeries::Spending.index()];
        assert!((spend.values[4][0] - 40_000.0).abs() < 400.0);

        // JSON round trip (NaN travels as null).
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("NaN"));
        let back: Summary = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    #[test]
    fn density_grid_matches_the_histograms() {
        let (acc, ctx) = tiny();
        let s = Summary::from_accumulator(&acc, &ctx);
        for d in &s.density {
            assert!(d.edges.windows(2).all(|w| w[0] < w[1]));
            for y in 0..3 {
                let cells: u32 = d.counts[y].iter().sum();
                assert_eq!(u64::from(cells + d.zeros[y]), s.axis.alive[y]);
                assert!(d.starts[y] as usize + d.counts[y].len() < d.edges.len() + 1);
                if let (Some(first), Some(last)) = (d.counts[y].first(), d.counts[y].last()) {
                    assert!(*first > 0 && *last > 0);
                }
            }
        }
        // Net worth year 0: one path at -$50k, one at exactly 0, eight positive.
        let nw = &s.density[HistSeries::NetWorth.index()];
        assert_eq!(nw.zeros, [1, 1, 1]);
        assert!(nw.edges[0] <= -50_000.0 && nw.edges[0] > -55_000.0);
        assert!(nw.edges.contains(&0.0));
        assert_eq!(nw.counts[0][0], 1);
        assert_eq!(nw.starts[0], 0);
        // The cell holding $900k: edges 8 per octave.
        let top = *nw.edges.last().unwrap();
        assert!((900_000.0..=1_000_000.0).contains(&top), "{top}");
        // Every edge is a fine-grid edge, so cells nest exactly.
        assert_eq!(density_edge(DENSITY_CELLS), 0.0);
        assert_eq!(density_edge(DENSITY_CELLS + 8), 1_024.0);
        assert_eq!(density_edge(DENSITY_CELLS + 16), 2_048.0);
        assert_eq!(density_edge(DENSITY_CELLS + 9), 1_152.0);
        assert_eq!(density_edge(DENSITY_CELLS - 8), -1_024.0);
        assert_eq!(density_edge(0), -hist::HIST_MAX);
        assert_eq!(density_edge(2 * DENSITY_CELLS), hist::HIST_MAX);
    }

    #[test]
    fn empty_accumulator_summarises_without_panicking() {
        let ctx = Synthetic::default().context();
        for years in [0, 5] {
            let s = Summary::from_accumulator(&Accumulator::new(years), &ctx);
            assert_eq!(s.paths, 0);
            assert!(s.headline.success_rate.value.is_nan());
            assert!(s.picks.is_empty());
            let json = serde_json::to_string(&s).unwrap();
            let _: Summary = serde_json::from_str(&json).unwrap();
            let digest = s.digest();
            assert_eq!(digest.success.p, 0.0);
            serde_json::to_string(&digest).unwrap();
        }
    }

    #[test]
    fn synthetic_summary_is_coherent() {
        let kit = Synthetic::default();
        let acc = run(&kit, 4_000);
        let s = Summary::from_accumulator(&acc, &kit.context());
        assert_eq!(s.axis.first_year_months, 10);
        assert_eq!(s.axis.start_age, START_AGE);
        assert_eq!(s.axis.alive[0], 4_000);
        assert!(s.axis.alive.windows(2).all(|w| w[0] >= w[1]));

        // Quantile curves are ordered and agree with exact quantiles of the scalar.
        for curves in &s.quantiles {
            for y in 0..kit.years as usize {
                if s.axis.alive[y] == 0 {
                    continue;
                }
                assert!(curves.values.windows(2).all(|w| w[0][y] <= w[1][y]));
            }
        }
        // Rates are consistent with the columns.
        let h = &s.headline;
        let fi = acc
            .scalar_column(PathScalar::FiAge)
            .iter()
            .filter(|v| v.is_finite())
            .count();
        assert_eq!(h.fi_rate.value, round_dp(fi as f64 / 4_000.0, 5));
        assert!(
            h.success_rate.lo < h.success_rate.value && h.success_rate.value < h.success_rate.hi
        );
        assert!(h.success_rate.value <= h.fi_rate.value);
        assert!(h.fi_age.p10 <= h.fi_age.p50 || h.fi_age.p50.is_nan());
        assert!(h.fi_age.p50_lo <= h.fi_age.p50 && h.fi_age.p50 <= h.fi_age.p50_hi);
        assert!((s.fi.cumulative.last().unwrap() - h.fi_rate.value).abs() < 1e-9);
        assert!((s.fi.never - (1.0 - h.fi_rate.value)).abs() < 2e-5);
        assert_eq!(s.fi.counts.iter().sum::<u64>(), fi as u64);
        assert!(h.funded_ratio.is_some());
        assert!(h.delivered_spending_ratio.value > 0.5 && h.delivered_spending_ratio.value <= 1.01);
        assert!(s.spending.paths_below_90.value >= s.spending.paths_below_75.value);
        assert!(s.spending.years_below_90.std_error > 0.0);
        assert!(s.spending.worst_cut.windows(2).all(|w| w[0] <= w[1]));

        // Survival: person curves decline and the household outlives each person.
        assert_eq!(s.survival.people.len(), 2);
        for person in &s.survival.people {
            assert!(person.windows(2).all(|w| w[0] >= w[1]));
            assert!(person
                .iter()
                .zip(&s.survival.household)
                .all(|(p, h)| *p <= *h + 0.002));
        }
        // Failure shares add up.
        let f = &s.failure;
        assert_eq!(f.causes.iter().map(|c| c.count).sum::<u64>(), f.failed);
        assert_eq!(
            f.failed,
            4_000 - (h.success_rate.value * 4_000.0).round() as u64
        );
        let early = f
            .factors
            .iter()
            .find(|x| x.factor == "poor_early_returns")
            .unwrap();
        assert!(!early.above);
        let ltc = f
            .factors
            .iter()
            .find(|x| x.factor == "long_term_care")
            .unwrap();
        assert!(
            ltc.failed > ltc.succeeded,
            "LTC is over-represented among failures"
        );
        // Picks point at real paths with the stated values.
        for p in &s.picks {
            let row = acc.path_scalars(p.path_index).unwrap();
            let v = f64::from(row[p.scalar.index()]);
            assert!(v.is_nan() && p.value.is_nan() || (v - p.value).abs() <= 0.5);
        }
        let p10 = s.picks.iter().find(|p| p.label == "p10").unwrap();
        let d = &s.distributions[PathScalar::TerminalNetWorth.index()];
        assert!((p10.value - d.quantiles[2]).abs() <= 0.02 * d.quantiles[2].abs() + 2_000.0);
        assert_eq!(s.scatter.paths.len(), 4_000);
        assert_eq!(d.counts.iter().sum::<u64>(), 4_000);
        assert_eq!(d.dots.len(), DOTS);
        assert!(d.dots.windows(2).all(|w| w[0] <= w[1]));

        // Deterministic (compared as JSON: NaN is not equal to itself).
        let again = Summary::from_accumulator(&acc, &kit.context());
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            serde_json::to_string(&again).unwrap()
        );
    }

    #[test]
    fn scatter_is_an_even_deterministic_subsample() {
        let kit = Synthetic::default();
        let acc = run(&kit, 12_500);
        let s = Summary::from_accumulator(&acc, &kit.context());
        assert_eq!(s.scatter.paths.len(), SCATTER_MAX);
        // 12,500 / 5,000 = 2.5: paths 0, 2, 5, 7, 10, ...
        assert_eq!(&s.scatter.paths[..5], &[0, 2, 5, 7, 10]);
        assert_eq!(*s.scatter.paths.last().unwrap(), 12_497);
        let success = s.scatter.columns.last().unwrap();
        assert_eq!(success.scalar, PathScalar::Success);
        let rate = success.values.iter().sum::<f64>() / SCATTER_MAX as f64;
        assert!((rate - s.headline.success_rate.value).abs() < 0.02);
    }

    #[test]
    fn digest_is_small_and_self_explanatory() {
        let kit = Synthetic::default();
        let acc = run(&kit, 3_000);
        let s = Summary::from_accumulator(&acc, &kit.context());
        let d = s.digest();
        let json = serde_json::to_string(&d).unwrap();
        println!("digest: {} bytes\n{json}", json.len());
        assert!(json.len() < 1_800, "{} bytes", json.len());
        assert_eq!(d.paths, 3_000);
        assert_eq!((d.start_year, d.end_year), (2026, 2085));
        assert_eq!(d.success.p, round_dp(s.headline.success_rate.value, 3));
        assert_eq!(d.by_age.ages.len(), DIGEST_POINTS);
        assert_eq!(d.by_age.ages[0], 36);
        assert_eq!(d.by_age.alive[0], 1.0);
        assert!(d.by_age.net_worth_p10[2] <= d.by_age.net_worth_p50[2]);
        assert!(d.by_age.net_worth_p50[2] <= d.by_age.net_worth_p90[2]);
        assert!(d.by_age.spending_p10[3] <= d.by_age.spending_p90[3]);
        assert_eq!(d.by_age.net_worth_p50[0] % 1_000.0, 0.0);
        assert!(d.failure_causes.len() <= DIGEST_CAUSES);
        assert!(d.fi_by_age.ages.len() <= DIGEST_POINTS);
        assert!(d.fi_by_age.ages.iter().all(|a| a % 10 == 0));
        assert!(d.fi_by_age.p.windows(2).all(|w| w[0] <= w[1]));
        assert!(d
            .failure_causes
            .windows(2)
            .all(|w| w[0].share >= w[1].share));
        assert!(d.trace_paths.median.is_some() && d.trace_paths.failing.is_some());
        assert!(d.notes.contains("today's dollars"));
        let back: SummaryDigest = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    #[test]
    fn json_schemas_generate_for_every_output_type() {
        let schemas = [
            ("Summary", schemars::schema_for!(Summary)),
            ("SummaryDigest", schemars::schema_for!(SummaryDigest)),
            (
                "Comparison",
                schemars::schema_for!(crate::stats::Comparison),
            ),
            ("PathTrace", schemars::schema_for!(crate::stats::PathTrace)),
        ];
        for (name, schema) in schemas {
            let json = serde_json::to_string(&schema).unwrap();
            assert!(json.contains("\"properties\""), "{name}");
            assert!(json.len() > 1_000, "{name}");
        }
        let summary = serde_json::to_string(&schemars::schema_for!(Summary)).unwrap();
        // Field docs become descriptions; nullable numbers are declared as such.
        assert!(summary.contains("median_fi_age"));
        assert!(summary.contains("counting paths that"));
        assert!(summary.contains("\"null\""));
        assert!(summary.contains("retired_surplus"));
    }

    #[test]
    fn sizes_and_timing_at_100k_paths() {
        // Realistic 60-year horizon. Reports the serialised accumulator size, the
        // Summary JSON size and the time to compute it (run with --release for the
        // figure quoted in docs/engine-output.md).
        let kit = Synthetic::default();
        let acc = run(&kit, 100_000);
        let bytes = acc.to_bytes();
        let packed_hist = hist::pack(&acc.hist).len();
        println!(
            "100k paths: accumulator {} bytes (histograms {}, scalars {})",
            bytes.len(),
            packed_hist,
            100_000 * PathScalar::COUNT * 4
        );
        assert_eq!(Accumulator::from_bytes(&bytes).unwrap(), acc);
        assert!(packed_hist < 600_000, "{packed_hist}");

        let ctx = kit.context();
        let started = std::time::Instant::now();
        let s = Summary::from_accumulator(&acc, &ctx);
        let elapsed = started.elapsed();
        let json = serde_json::to_string(&s).unwrap();
        let digest = serde_json::to_string(&s.digest()).unwrap();
        println!(
            "100k paths: Summary computed in {elapsed:?}, JSON {} bytes, digest {} bytes",
            json.len(),
            digest.len()
        );
        assert!(json.len() < 1_500_000, "{}", json.len());
        assert!(digest.len() < 1_800, "{}", digest.len());
        if !cfg!(debug_assertions) {
            assert!(elapsed.as_millis() < 150, "{elapsed:?}");
        }

        // Sampling precision at 100k paths: at most about +/- 0.3 points.
        let h = &s.headline;
        let p = h.success_rate.value;
        let se = (p * (1.0 - p) / 100_000.0).sqrt();
        assert!((h.success_rate.std_error - se).abs() < 1e-6);
        assert!(h.success_rate.hi - h.success_rate.lo < 0.0065);
    }
}
