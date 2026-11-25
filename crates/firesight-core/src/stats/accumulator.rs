//! The mergeable aggregate recorder and its binary codec.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::hist::{self, hist_bin, HIST_BINS};
use super::{
    HistSeries, PathEnd, PathScalar, Recorder, Status, YearRecord, YearSeries, DEEP_CUT_RATIO,
    MILD_CUT_RATIO, SAMPLE_PATHS,
};

/// The per-path scalars of a contiguous run of paths, row-major: path `first_path + r`
/// occupies `values[r * PathScalar::COUNT ..][.. PathScalar::COUNT]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScalarChunk {
    /// Index of the first path in the chunk.
    pub first_path: u64,
    /// Row-major scalar values.
    pub values: Vec<f32>,
}

/// Bitwise on the values, so "not applicable" (NaN) equals itself and two
/// accumulators holding the same paths compare equal.
impl PartialEq for ScalarChunk {
    fn eq(&self, other: &Self) -> bool {
        self.first_path == other.first_path
            && self.values.len() == other.values.len()
            && self
                .values
                .iter()
                .zip(&other.values)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

impl ScalarChunk {
    /// Number of paths in the chunk.
    pub fn paths(&self) -> u64 {
        (self.values.len() / PathScalar::COUNT) as u64
    }

    fn end(&self) -> u64 {
        self.first_path + self.paths()
    }
}

/// Annual series of one of the first [`SAMPLE_PATHS`] paths, for spaghetti plots.
/// Whole today's dollars; one entry per recorded year (shorter than the horizon when
/// the household dies earlier).
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct SamplePath {
    /// Which path.
    pub path_index: u64,
    /// Net worth at each year end.
    pub net_worth: Vec<i64>,
    /// Investable assets at each year end.
    pub investable: Vec<i64>,
    /// Spending in each year (as recorded, not annualised).
    pub spending: Vec<i64>,
}

/// A recorder that keeps the annual net worth, investable assets and spending of
/// one path and nothing else: what [`Accumulator`] keeps for its sample paths, for
/// any path that is simulated again.
#[derive(Clone, Debug, Default)]
pub struct SampleRecorder {
    /// The path recorded so far.
    pub sample: SamplePath,
}

impl Recorder for SampleRecorder {
    fn begin_path(&mut self, path_index: u64) {
        self.sample = SamplePath {
            path_index,
            ..SamplePath::default()
        };
    }

    fn record_year(&mut self, _year_index: usize, record: &YearRecord) {
        let s = &mut self.sample;
        s.net_worth.push(record.net_worth().round() as i64);
        s.investable.push(record.investable().round() as i64);
        s.spending.push(record.spending().round() as i64);
    }

    fn end_path(&mut self, _end: &PathEnd) {}
}

/// Running per-path state from which the derived [`PathScalar`]s are computed. Shared
/// by the aggregate and the trace recorder so both report identical scalars.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PathTally {
    years: u32,
    terminal_net_worth: f64,
    peak_net_worth: f64,
    min_investable: f64,
    tax: f64,
    spending: f64,
    delivered: f64,
    extra: f64,
    planned: f64,
    share_sum: f64,
    share_years: u32,
    worst_ratio: f64,
    years_retired: u32,
    years_below_90: u32,
    years_below_75: u32,
    years_depleted: u32,
}

impl Default for PathTally {
    fn default() -> Self {
        PathTally {
            years: 0,
            terminal_net_worth: 0.0,
            peak_net_worth: f64::NEG_INFINITY,
            min_investable: f64::INFINITY,
            tax: 0.0,
            spending: 0.0,
            delivered: 0.0,
            extra: 0.0,
            planned: 0.0,
            share_sum: 0.0,
            share_years: 0,
            worst_ratio: f64::INFINITY,
            years_retired: 0,
            years_below_90: 0,
            years_below_75: 0,
            years_depleted: 0,
        }
    }
}

/// What one year contributes to the per-year spending-delivery counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct CutFlags {
    pub retired: bool,
    pub below_90: bool,
    pub below_75: bool,
}

impl PathTally {
    /// Folds one year in. `hist` is `record.hist_values()`.
    pub(crate) fn record(
        &mut self,
        record: &YearRecord,
        hist: &[f64; HistSeries::COUNT],
    ) -> CutFlags {
        let net_worth = hist[HistSeries::NetWorth.index()];
        let investable = hist[HistSeries::Investable.index()];
        let spending = hist[HistSeries::Spending.index()];
        self.years += 1;
        self.terminal_net_worth = net_worth;
        self.peak_net_worth = self.peak_net_worth.max(net_worth);
        self.tax += hist[HistSeries::TotalTax.index()];
        self.spending += spending;
        if record.status == Status::Depleted {
            self.years_depleted += 1;
        }
        let mut flags = CutFlags::default();
        if record.retired {
            flags.retired = true;
            self.years_retired += 1;
            self.min_investable = self.min_investable.min(investable);
            let planned = record.get(YearSeries::SpendPlanned);
            if planned > 0.0 {
                self.delivered += spending.min(planned);
                self.extra += (spending - planned).max(0.0);
                self.planned += planned;
                let ratio = spending / planned;
                self.share_sum += ratio.min(1.0);
                self.share_years += 1;
                self.worst_ratio = self.worst_ratio.min(ratio);
                if ratio < MILD_CUT_RATIO {
                    flags.below_90 = true;
                    self.years_below_90 += 1;
                }
                if ratio < DEEP_CUT_RATIO {
                    flags.below_75 = true;
                    self.years_below_75 += 1;
                }
            }
        }
        flags
    }

    /// Writes the derived scalars into a row.
    pub(crate) fn finish(&self, row: &mut [f32; PathScalar::COUNT]) {
        let nan_unless = |cond: bool, v: f64| if cond { v as f32 } else { f32::NAN };
        let mut set = |s: PathScalar, v: f32| row[s.index()] = v;
        let any = self.years > 0;
        let retired = self.years_retired > 0;
        set(PathScalar::YearsSimulated, self.years as f32);
        set(
            PathScalar::TerminalNetWorth,
            nan_unless(any, self.terminal_net_worth),
        );
        set(
            PathScalar::PeakNetWorth,
            nan_unless(any, self.peak_net_worth),
        );
        set(
            PathScalar::MinInvestable,
            nan_unless(retired, self.min_investable),
        );
        set(PathScalar::LifetimeTax, self.tax as f32);
        set(PathScalar::LifetimeSpending, self.spending as f32);
        set(
            PathScalar::DeliveredSpendingRatio,
            nan_unless(self.planned > 0.0, self.delivered / self.planned),
        );
        set(
            PathScalar::ExtraSpendingRatio,
            nan_unless(self.planned > 0.0, self.extra / self.planned),
        );
        set(
            PathScalar::PlanDeliveredShare,
            nan_unless(
                self.share_years > 0,
                self.share_sum / f64::from(self.share_years.max(1)),
            ),
        );
        set(
            PathScalar::WorstSpendingRatio,
            nan_unless(self.worst_ratio.is_finite(), self.worst_ratio),
        );
        set(PathScalar::YearsRetired, self.years_retired as f32);
        set(PathScalar::YearsBelow90, self.years_below_90 as f32);
        set(PathScalar::YearsBelow75, self.years_below_75 as f32);
        set(PathScalar::YearsDepleted, self.years_depleted as f32);
    }
}

/// The path being recorded. Not part of the serialised state.
#[derive(Clone, Debug, PartialEq, Default)]
struct Current {
    active: bool,
    path_index: u64,
    tally: PathTally,
    sample: Option<SamplePath>,
}

/// Mergeable aggregate of any number of paths.
#[derive(Clone, PartialEq)]
pub struct Accumulator {
    /// Layout version of this struct, checked on merge and decode.
    pub version: u32,
    /// Number of calendar years covered (horizon).
    pub years: u32,
    /// Number of paths recorded.
    pub paths: u64,
    /// Histogram counts, `[series][year][bin]` flattened:
    /// `HistSeries::COUNT * years * HIST_BINS` entries. Only living households are
    /// counted, so each year's total is the number of paths alive that year.
    pub hist: Vec<u32>,
    /// Status counts, `[year][status]` flattened (`Dead` stays zero; it is derived).
    pub status: Vec<u32>,
    /// Months recorded per year, summed over paths (annualises flows).
    pub months: Vec<u64>,
    /// Spending-delivery counts, `[year][3]` flattened: retired path-years, those
    /// delivering under 90% of plan, those under 75%.
    pub cuts: Vec<u32>,
    /// Sums in whole today's dollars, `[series][year]` flattened. Integer so that
    /// merging is exact in any order.
    pub sums: Vec<i64>,
    /// Spending shortfall of depleted household-years, `[year]`: the sum over
    /// households whose year ended `depleted` of planned less delivered spending
    /// (not below zero, annualised), in whole today's dollars. Divide by that
    /// year's `depleted` status count for the mean depth of the shortfall.
    pub shortfall: Vec<i64>,
    /// Per-path scalars, as chunks sorted by `first_path`, never overlapping and
    /// never adjacent (adjacent chunks are joined).
    pub scalars: Vec<ScalarChunk>,
    /// Sample paths sorted by `path_index`.
    pub samples: Vec<SamplePath>,
    current: Current,
}

/// A one-line description: the arrays are far too large to print.
impl std::fmt::Debug for Accumulator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accumulator")
            .field("version", &self.version)
            .field("years", &self.years)
            .field("paths", &self.paths)
            .field("scalar_chunks", &self.scalars.len())
            .field("samples", &self.samples.len())
            .finish_non_exhaustive()
    }
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

/// Number of per-year spending-delivery counters.
const CUT_COUNTS: usize = 3;

/// Longest horizon accepted when decoding, in years: a guard against allocating from
/// corrupt input (no plan runs longer than a human life from birth).
const MAX_YEARS: u32 = 150;

#[derive(Serialize)]
struct WireRef<'a> {
    version: u32,
    years: u32,
    paths: u64,
    hist: &'a [u8],
    status: &'a [u32],
    months: &'a [u64],
    cuts: &'a [u32],
    sums: &'a [i64],
    shortfall: &'a [i64],
    scalars: &'a [ScalarChunk],
    samples: &'a [SamplePath],
}

#[derive(Deserialize)]
struct WireOwned {
    version: u32,
    years: u32,
    paths: u64,
    hist: Vec<u8>,
    status: Vec<u32>,
    months: Vec<u64>,
    cuts: Vec<u32>,
    sums: Vec<i64>,
    shortfall: Vec<i64>,
    scalars: Vec<ScalarChunk>,
    samples: Vec<SamplePath>,
}

impl Accumulator {
    /// Layout version written by this build. Bump whenever a field, the histogram
    /// grid or one of the series / scalar enums changes.
    pub const VERSION: u32 = 6;

    /// An empty accumulator for a horizon of `years` calendar years.
    pub fn new(years: u32) -> Accumulator {
        let y = years as usize;
        Accumulator {
            version: Self::VERSION,
            years,
            paths: 0,
            hist: vec![0; HistSeries::COUNT * y * HIST_BINS],
            status: vec![0; y * Status::COUNT],
            months: vec![0; y],
            cuts: vec![0; y * CUT_COUNTS],
            sums: vec![0; YearSeries::COUNT * y],
            shortfall: vec![0; y],
            scalars: Vec::new(),
            samples: Vec::new(),
            current: Current::default(),
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
            || self.months.len() != other.months.len()
            || self.cuts.len() != other.cuts.len()
            || self.sums.len() != other.sums.len()
        {
            return Err(AccumulatorError::Incompatible("array sizes differ".into()));
        }
        let mut chunks: Vec<&ScalarChunk> = self.scalars.iter().chain(&other.scalars).collect();
        chunks.sort_by_key(|c| c.first_path);
        for pair in chunks.windows(2) {
            if pair[1].first_path < pair[0].end() {
                return Err(AccumulatorError::Overlap(
                    pair[1].first_path,
                    pair[0].end().min(pair[1].end()),
                ));
            }
        }
        // Coalesce adjacent chunks so the representation does not depend on how the
        // run was split.
        let mut merged: Vec<ScalarChunk> = Vec::with_capacity(chunks.len());
        for c in chunks {
            match merged.last_mut() {
                Some(last) if last.end() == c.first_path => {
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
        for (a, b) in self.months.iter_mut().zip(&other.months) {
            *a += b;
        }
        for (a, b) in self.cuts.iter_mut().zip(&other.cuts) {
            *a += b;
        }
        for (a, b) in self.sums.iter_mut().zip(&other.sums) {
            *a += b;
        }
        for (a, b) in self.shortfall.iter_mut().zip(&other.shortfall) {
            *a += b;
        }
        self.paths += other.paths;
        self.samples.extend(other.samples.iter().cloned());
        self.samples.sort_by_key(|s| s.path_index);
        Ok(())
    }

    /// Serialises to the compact binary form passed between workers: postcard, with
    /// the histograms run-length packed (see `hist::pack`). Equal accumulators give
    /// equal bytes. A path in progress (between `begin_path` and `end_path`) is not
    /// included.
    pub fn to_bytes(&self) -> Vec<u8> {
        let packed = hist::pack(&self.hist);
        postcard::to_allocvec(&WireRef {
            version: self.version,
            years: self.years,
            paths: self.paths,
            hist: &packed,
            status: &self.status,
            months: &self.months,
            cuts: &self.cuts,
            sums: &self.sums,
            shortfall: &self.shortfall,
            scalars: &self.scalars,
            samples: &self.samples,
        })
        .expect("Accumulator serialisation cannot fail")
    }

    /// Decodes [`Accumulator::to_bytes`] output, checking the version first and then
    /// every structural invariant, so corrupt input is an error and never a panic.
    pub fn from_bytes(bytes: &[u8]) -> Result<Accumulator, AccumulatorError> {
        let decode = |e: postcard::Error| AccumulatorError::Decode(e.to_string());
        let bad = |what: &str| AccumulatorError::Decode(what.to_string());
        let (version, _) = postcard::take_from_bytes::<u32>(bytes).map_err(decode)?;
        if version != Self::VERSION {
            return Err(AccumulatorError::Decode(format!(
                "version {} is not supported (expected {})",
                version,
                Self::VERSION
            )));
        }
        let wire: WireOwned = postcard::from_bytes(bytes).map_err(decode)?;
        if wire.years > MAX_YEARS {
            return Err(bad("horizon is implausibly long"));
        }
        let y = wire.years as usize;
        if wire.status.len() != y * Status::COUNT
            || wire.months.len() != y
            || wire.cuts.len() != y * CUT_COUNTS
            || wire.sums.len() != y * YearSeries::COUNT
            || wire.shortfall.len() != y
        {
            return Err(bad("array sizes do not match the horizon"));
        }
        let hist = hist::unpack(&wire.hist, HistSeries::COUNT * y)
            .ok_or_else(|| bad("histograms are malformed"))?;
        let mut rows = 0u64;
        let mut prev_end = None;
        for c in &wire.scalars {
            if c.values.is_empty() || c.values.len() % PathScalar::COUNT != 0 {
                return Err(bad("scalar chunk has a partial row"));
            }
            let end = c
                .first_path
                .checked_add(c.paths())
                .ok_or_else(|| bad("scalar chunk overflows"))?;
            if prev_end.is_some_and(|p| c.first_path <= p) {
                return Err(bad("scalar chunks are not sorted and separated"));
            }
            prev_end = Some(end);
            rows += c.paths();
        }
        if rows != wire.paths {
            return Err(bad("path count does not match the scalar rows"));
        }
        if wire
            .samples
            .windows(2)
            .any(|w| w[0].path_index >= w[1].path_index)
        {
            return Err(bad("sample paths are not sorted"));
        }
        Ok(Accumulator {
            version: wire.version,
            years: wire.years,
            paths: wire.paths,
            hist,
            status: wire.status,
            months: wire.months,
            cuts: wire.cuts,
            sums: wire.sums,
            shortfall: wire.shortfall,
            scalars: wire.scalars,
            samples: wire.samples,
            current: Current::default(),
        })
    }

    /// The scalar column for one quantity across all recorded paths, in path order.
    pub fn scalar_column(&self, scalar: PathScalar) -> Vec<f32> {
        self.rows().map(|(_, row)| row[scalar.index()]).collect()
    }

    /// The path index of each row of [`Accumulator::scalar_column`].
    pub fn path_indices(&self) -> Vec<u64> {
        self.rows().map(|(p, _)| p).collect()
    }

    /// Every recorded path's index and scalar row, in ascending path order.
    pub fn rows(&self) -> impl Iterator<Item = (u64, &[f32])> + Clone {
        self.scalars
            .iter()
            .flat_map(|c| (c.first_path..).zip(c.values.chunks_exact(PathScalar::COUNT)))
    }

    /// The scalar row of one path, if it has been recorded.
    pub fn path_scalars(&self, path_index: u64) -> Option<&[f32]> {
        let i = self
            .scalars
            .partition_point(|c| c.first_path <= path_index)
            .checked_sub(1)?;
        let c = &self.scalars[i];
        let r = (path_index - c.first_path) as usize * PathScalar::COUNT;
        c.values.get(r..r + PathScalar::COUNT)
    }

    /// One year's histogram of one series ([`HIST_BINS`] counts).
    pub fn histogram(&self, series: HistSeries, year: usize) -> &[u32] {
        let at = (series.index() * self.years as usize + year) * HIST_BINS;
        &self.hist[at..at + HIST_BINS]
    }

    /// Number of paths with a record in `year` (households with someone alive).
    pub fn alive(&self, year: usize) -> u64 {
        self.status[year * Status::COUNT..(year + 1) * Status::COUNT]
            .iter()
            .map(|&c| u64::from(c))
            .sum()
    }

    /// Inserts one path's scalar row, keeping chunks sorted and coalesced.
    fn insert_row(&mut self, path_index: u64, row: &[f32; PathScalar::COUNT]) {
        if let Some(last) = self.scalars.last_mut() {
            if last.end() == path_index {
                last.values.extend_from_slice(row);
                return;
            }
        }
        let i = self.scalars.partition_point(|c| c.first_path < path_index);
        let joins_prev = i > 0 && {
            let prev = &self.scalars[i - 1];
            assert!(
                prev.end() <= path_index,
                "path {path_index} was recorded twice"
            );
            prev.end() == path_index
        };
        let joins_next = self.scalars.get(i).is_some_and(|next| {
            assert!(
                next.first_path != path_index,
                "path {path_index} was recorded twice"
            );
            next.first_path == path_index + 1
        });
        match (joins_prev, joins_next) {
            (true, true) => {
                let next = self.scalars.remove(i);
                let prev = &mut self.scalars[i - 1];
                prev.values.extend_from_slice(row);
                prev.values.extend_from_slice(&next.values);
            }
            (true, false) => self.scalars[i - 1].values.extend_from_slice(row),
            (false, true) => {
                let next = &mut self.scalars[i];
                next.first_path = path_index;
                next.values.splice(0..0, row.iter().copied());
            }
            (false, false) => self.scalars.insert(
                i,
                ScalarChunk {
                    first_path: path_index,
                    values: row.to_vec(),
                },
            ),
        }
    }
}

impl Recorder for Accumulator {
    fn begin_path(&mut self, path_index: u64) {
        debug_assert!(!self.current.active, "begin_path without end_path");
        self.current = Current {
            active: true,
            path_index,
            tally: PathTally::default(),
            sample: (path_index < SAMPLE_PATHS).then(|| SamplePath {
                path_index,
                ..SamplePath::default()
            }),
        };
    }

    fn record_year(&mut self, year_index: usize, record: &YearRecord) {
        debug_assert!(self.current.active, "record_year outside a path");
        debug_assert!(year_index < self.years as usize, "year beyond the horizon");
        debug_assert!(record.status != Status::Dead, "dead years are not recorded");
        debug_assert!((1..=12).contains(&record.months));
        if year_index >= self.years as usize || record.status == Status::Dead {
            return;
        }
        let years = self.years as usize;
        let values = record.hist_values();
        let months = record.months.clamp(1, 12);
        // Flows of a partial year are scaled to a full year for the distribution, so
        // the first and the last year of a life do not read as a collapse.
        let annualise = 12.0 / f64::from(months);
        for series in HistSeries::ALL {
            let mut v = values[series.index()];
            if months < 12 && series.is_flow() {
                v *= annualise;
            }
            self.hist[(series.index() * years + year_index) * HIST_BINS + hist_bin(v)] += 1;
        }
        self.status[year_index * Status::COUNT + record.status.index()] += 1;
        self.months[year_index] += u64::from(months);
        for (s, v) in record.values.iter().enumerate() {
            // `as` saturates and maps NaN to 0, identically on every platform.
            // Saturating: inputs are bounded by validation, but a total must never
            // wrap (or panic in a debug build) whatever reaches it.
            let slot = &mut self.sums[s * years + year_index];
            *slot = slot.saturating_add(v.round() as i64);
        }
        if record.status == Status::Depleted {
            let short = (record.get(YearSeries::SpendPlanned)
                - values[HistSeries::Spending.index()])
            .max(0.0)
                * annualise;
            let slot = &mut self.shortfall[year_index];
            *slot = slot.saturating_add(short.round() as i64);
        }
        let flags = self.current.tally.record(record, &values);
        let cuts = &mut self.cuts[year_index * CUT_COUNTS..(year_index + 1) * CUT_COUNTS];
        cuts[0] += u32::from(flags.retired);
        cuts[1] += u32::from(flags.below_90);
        cuts[2] += u32::from(flags.below_75);
        if let Some(sample) = &mut self.current.sample {
            sample
                .net_worth
                .push(values[HistSeries::NetWorth.index()].round() as i64);
            sample
                .investable
                .push(values[HistSeries::Investable.index()].round() as i64);
            sample
                .spending
                .push(values[HistSeries::Spending.index()].round() as i64);
        }
    }

    fn end_path(&mut self, end: &PathEnd) {
        debug_assert!(self.current.active, "end_path without begin_path");
        let current = std::mem::take(&mut self.current);
        let mut row = end.scalars;
        current.tally.finish(&mut row);
        self.insert_row(current.path_index, &row);
        self.paths += 1;
        if let Some(sample) = current.sample {
            let i = self
                .samples
                .partition_point(|s| s.path_index < sample.path_index);
            self.samples.insert(i, sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::testkit::Synthetic;
    use crate::stats::FailureCause;

    fn year(status: Status, retired: bool, months: u8, set: &[(YearSeries, f64)]) -> YearRecord {
        let mut r = YearRecord {
            status,
            retired,
            months,
            ..YearRecord::default()
        };
        for &(s, v) in set {
            r.set(s, v);
        }
        r
    }

    /// A three-year life worked by hand.
    fn hand_path(acc: &mut Accumulator, path: u64) {
        use YearSeries::*;
        acc.begin_path(path);
        // Year 0 covers 3 months, working.
        acc.record_year(
            0,
            &year(
                Status::Working,
                false,
                3,
                &[
                    (BalanceTaxable, 100_000.4),
                    (Debt, 20_000.0),
                    (IncomeWages, 30_000.0),
                    (TaxFederal, 6_000.0),
                    (SpendEssential, 12_000.0),
                    (SpendPlanned, 12_000.0),
                    (SaveTaxable, 12_000.0),
                ],
            ),
        );
        // Year 1: retired, delivering 80% of plan.
        acc.record_year(
            1,
            &year(
                Status::RetiredCutting,
                true,
                12,
                &[
                    (BalanceTaxable, 60_000.0),
                    (WithdrawTaxable, 44_000.0),
                    (TaxFederal, 4_000.0),
                    (SpendEssential, 30_000.0),
                    (SpendDiscretionary, 10_000.0),
                    (SpendPlanned, 50_000.0),
                ],
            ),
        );
        // Year 2: depleted, delivering 50% of plan, dies after 6 months.
        acc.record_year(
            2,
            &year(
                Status::Depleted,
                true,
                6,
                &[
                    (BalanceCash, 0.0),
                    (IncomeSocialSecurity, 12_500.0),
                    (SpendEssential, 12_500.0),
                    (SpendPlanned, 25_000.0),
                ],
            ),
        );
        let mut end = PathEnd::default();
        end.set(PathScalar::FiAge, 60.25);
        end.set(PathScalar::TerminalNetWorth, 123.0); // derived: must be overwritten
        end.set_outcome(Some(FailureCause::Overspending));
        acc.end_path(&end);
    }

    #[test]
    fn recording_fills_histograms_counts_sums_and_derived_scalars() {
        let mut acc = Accumulator::new(4);
        hand_path(&mut acc, 7);
        assert_eq!(acc.paths, 1);
        assert_eq!(
            (0..4).map(|y| acc.alive(y)).collect::<Vec<_>>(),
            [1, 1, 1, 0]
        );

        let count = |s: HistSeries, y: usize, v: f64| acc.histogram(s, y)[hist_bin(v)];
        // Balances are recorded as they are; flows of a partial year are annualised.
        assert_eq!(count(HistSeries::NetWorth, 0, 80_000.4), 1);
        assert_eq!(count(HistSeries::Investable, 0, 100_000.4), 1);
        assert_eq!(count(HistSeries::Spending, 0, 48_000.0), 1);
        assert_eq!(count(HistSeries::GrossIncome, 0, 120_000.0), 1);
        assert_eq!(count(HistSeries::TotalTax, 0, 24_000.0), 1);
        assert_eq!(count(HistSeries::Spending, 1, 40_000.0), 1);
        assert_eq!(count(HistSeries::GrossIncome, 1, 0.0), 1);
        assert_eq!(count(HistSeries::Investable, 2, 0.0), 1);
        assert_eq!(count(HistSeries::Spending, 2, 25_000.0), 1);
        for s in HistSeries::ALL {
            for y in 0..4 {
                let total: u32 = acc.histogram(*s, y).iter().sum();
                assert_eq!(u64::from(total), acc.alive(y));
            }
        }

        let status = |y: usize, s: Status| acc.status[y * Status::COUNT + s.index()];
        assert_eq!(status(0, Status::Working), 1);
        assert_eq!(status(1, Status::RetiredCutting), 1);
        assert_eq!(status(2, Status::Depleted), 1);
        assert_eq!(acc.months, [3, 12, 6, 0]);
        // [retired, below 90%, below 75%] per year: 80% then 50% of plan.
        assert_eq!(acc.cuts, [0, 0, 0, 1, 1, 0, 1, 1, 1, 0, 0, 0]);

        let sum = |s: YearSeries, y: usize| acc.sums[s.index() * 4 + y];
        assert_eq!(
            sum(YearSeries::BalanceTaxable, 0),
            100_000,
            "rounded to dollars"
        );
        assert_eq!(
            sum(YearSeries::IncomeWages, 0),
            30_000,
            "sums are not annualised"
        );
        assert_eq!(sum(YearSeries::SpendPlanned, 1), 50_000);
        assert_eq!(sum(YearSeries::IncomeSocialSecurity, 2), 12_500);

        let get = |s: PathScalar| acc.path_scalars(7).unwrap()[s.index()];
        assert_eq!(get(PathScalar::FiAge), 60.25);
        assert_eq!(get(PathScalar::Success), 0.0);
        assert_eq!(get(PathScalar::FailureCause), 4.0);
        assert!(get(PathScalar::DepletionAge).is_nan());
        assert_eq!(get(PathScalar::YearsSimulated), 3.0);
        assert_eq!(get(PathScalar::TerminalNetWorth), 0.0);
        assert_eq!(get(PathScalar::PeakNetWorth), 80_000.4);
        assert_eq!(get(PathScalar::MinInvestable), 0.0);
        assert_eq!(get(PathScalar::LifetimeTax), 10_000.0);
        assert_eq!(get(PathScalar::LifetimeSpending), 64_500.0);
        // Retired years: delivered 40,000 + 12,500 of planned 50,000 + 25,000.
        assert_eq!(get(PathScalar::DeliveredSpendingRatio), 0.7);
        assert_eq!(get(PathScalar::WorstSpendingRatio), 0.5);
        assert_eq!(get(PathScalar::YearsRetired), 2.0);
        assert_eq!(get(PathScalar::YearsBelow90), 2.0);
        assert_eq!(get(PathScalar::YearsBelow75), 1.0);
        assert_eq!(get(PathScalar::YearsDepleted), 1.0);
        assert!(acc.path_scalars(6).is_none());
        assert!(acc.path_scalars(8).is_none());

        assert_eq!(
            acc.samples,
            [SamplePath {
                path_index: 7,
                net_worth: vec![80_000, 60_000, 0],
                investable: vec![100_000, 60_000, 0],
                spending: vec![12_000, 40_000, 12_500],
            }]
        );
    }

    #[test]
    fn a_path_that_never_retires_has_nan_retirement_scalars() {
        let mut acc = Accumulator::new(2);
        acc.begin_path(SAMPLE_PATHS);
        acc.record_year(
            0,
            &year(Status::Working, false, 12, &[(YearSeries::Debt, 5e3)]),
        );
        acc.end_path(&PathEnd::default());
        let row = acc.path_scalars(SAMPLE_PATHS).unwrap();
        assert!(row[PathScalar::MinInvestable.index()].is_nan());
        assert!(row[PathScalar::DeliveredSpendingRatio.index()].is_nan());
        assert!(row[PathScalar::WorstSpendingRatio.index()].is_nan());
        assert_eq!(row[PathScalar::TerminalNetWorth.index()], -5_000.0);
        assert_eq!(row[PathScalar::YearsRetired.index()], 0.0);
        assert!(
            acc.samples.is_empty(),
            "only the first SAMPLE_PATHS are kept"
        );
        assert_eq!(acc.histogram(HistSeries::NetWorth, 0)[hist_bin(-5e3)], 1);

        // A path with no years at all (cannot happen, but must not poison anything).
        acc.begin_path(0);
        acc.end_path(&PathEnd::default());
        let row = acc.path_scalars(0).unwrap();
        assert!(row[PathScalar::TerminalNetWorth.index()].is_nan());
        assert!(row[PathScalar::PeakNetWorth.index()].is_nan());
        assert_eq!(row[PathScalar::YearsSimulated.index()], 0.0);
    }

    #[test]
    fn rows_stay_sorted_and_coalesced_in_any_recording_order() {
        let mut acc = Accumulator::new(4);
        for p in [5, 3, 4, 9, 0, 1, 2, 8, 6, 7] {
            hand_path(&mut acc, p);
        }
        assert_eq!(acc.scalars.len(), 1);
        assert_eq!(acc.path_indices(), (0..10).collect::<Vec<_>>());
        assert_eq!(
            acc.samples.iter().map(|s| s.path_index).collect::<Vec<_>>(),
            (0..10).collect::<Vec<_>>()
        );
        let mut ordered = Accumulator::new(4);
        for p in 0..10 {
            hand_path(&mut ordered, p);
        }
        assert_eq!(acc, ordered);
        assert_eq!(acc.to_bytes(), ordered.to_bytes());

        let mut gaps = Accumulator::new(4);
        for p in [10, 2, 30] {
            hand_path(&mut gaps, p);
        }
        assert_eq!(
            gaps.scalars
                .iter()
                .map(|c| c.first_path)
                .collect::<Vec<_>>(),
            [2, 10, 30]
        );
        assert_eq!(
            gaps.path_scalars(10).unwrap()[PathScalar::FiAge.index()],
            60.25
        );
        assert!(gaps.path_scalars(11).is_none());
    }

    #[test]
    #[should_panic(expected = "recorded twice")]
    fn recording_a_path_twice_panics() {
        let mut acc = Accumulator::new(4);
        hand_path(&mut acc, 3);
        hand_path(&mut acc, 5);
        hand_path(&mut acc, 3);
    }

    fn run(kit: &Synthetic, paths: std::ops::Range<u64>) -> Accumulator {
        let mut acc = Accumulator::new(kit.years);
        for p in paths {
            kit.run_path(p, &mut acc);
        }
        acc
    }

    #[test]
    fn one_chunk_of_1000_equals_ten_merged_chunks_of_100_in_any_order() {
        let kit = Synthetic::default();
        let whole = run(&kit, 0..1_000);
        let whole_bytes = whole.to_bytes();
        let chunks: Vec<Vec<u8>> = (0..10)
            .map(|i| run(&kit, i * 100..(i + 1) * 100).to_bytes())
            .collect();
        let orders: [[usize; 10]; 4] = [
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
            [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
            [3, 7, 0, 9, 1, 5, 8, 2, 6, 4],
            [5, 0, 6, 1, 7, 2, 8, 3, 9, 4],
        ];
        for order in orders {
            let mut acc = Accumulator::new(kit.years);
            for i in order {
                acc.merge(&Accumulator::from_bytes(&chunks[i]).unwrap())
                    .unwrap();
            }
            assert_eq!(acc.to_bytes(), whole_bytes, "order {order:?}");
            assert_eq!(acc, whole);
        }
        // A tree of merges and uneven chunks give the same bytes too.
        let mut left = run(&kit, 0..37);
        left.merge(&run(&kit, 600..1_000)).unwrap();
        let mut right = run(&kit, 250..600);
        right.merge(&run(&kit, 37..250)).unwrap();
        right.merge(&left).unwrap();
        assert_eq!(right.to_bytes(), whole_bytes);
        assert_eq!(whole.samples.len(), SAMPLE_PATHS as usize);
        assert_eq!(whole.scalars.len(), 1);
    }

    #[test]
    fn merged_chunks_carry_the_equity_scalars_exactly() {
        // The same with private-company equity on every path: the six scalars
        // ride in the rows, so the bytes and the equity summary are those of one
        // run whatever the chunking.
        use crate::stats::Summary;
        let kit = Synthetic {
            equity: true,
            ..Synthetic::default()
        };
        let whole = run(&kit, 0..1_000);
        let whole_bytes = whole.to_bytes();
        let chunks: Vec<Vec<u8>> = (0..10)
            .map(|i| run(&kit, i * 100..(i + 1) * 100).to_bytes())
            .collect();
        let expected = Summary::from_accumulator(&whole, &kit.context());
        assert_eq!(expected.equity.as_ref().unwrap().paths, 1_000);
        for order in [
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
            [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
            [3, 7, 0, 9, 1, 5, 8, 2, 6, 4],
        ] {
            let mut acc = Accumulator::new(kit.years);
            for i in order {
                acc.merge(&Accumulator::from_bytes(&chunks[i]).unwrap())
                    .unwrap();
            }
            assert_eq!(acc.to_bytes(), whole_bytes, "order {order:?}");
            let merged = Summary::from_accumulator(&acc, &kit.context());
            // Compared as JSON: a missing number is NaN, which does not equal itself.
            assert_eq!(
                serde_json::to_string(&merged.equity).unwrap(),
                serde_json::to_string(&expected.equity).unwrap()
            );
            assert_eq!(
                serde_json::to_string(&merged).unwrap(),
                serde_json::to_string(&expected).unwrap()
            );
        }
        // The equity scalars are the only difference from the plain kit.
        let plain = run(&Synthetic::default(), 0..1_000);
        for ((_, with), (_, without)) in whole.rows().zip(plain.rows()) {
            let first = PathScalar::EquityOutcome.index();
            assert!(with[..first]
                .iter()
                .zip(&without[..first])
                .all(|(a, b)| a.to_bits() == b.to_bits()));
            assert!(with[first].is_finite() && without[first..].iter().all(|v| v.is_nan()));
        }
    }

    #[test]
    fn merge_rejects_overlap_and_mismatch() {
        let kit = Synthetic::default();
        let mut a = run(&kit, 0..10);
        let before = a.clone();
        assert_eq!(
            a.merge(&run(&kit, 5..15)),
            Err(AccumulatorError::Overlap(5, 10))
        );
        assert_eq!(a, before, "a failed merge must not modify the target");
        assert!(matches!(
            a.merge(&Accumulator::new(kit.years + 1)),
            Err(AccumulatorError::Incompatible(_))
        ));
        let mut old = Accumulator::new(kit.years);
        old.version = 1;
        assert!(matches!(
            a.merge(&old),
            Err(AccumulatorError::Incompatible(_))
        ));
        assert_eq!(a, before);
        a.merge(&Accumulator::new(kit.years)).unwrap();
        assert_eq!(a, before, "merging an empty accumulator changes nothing");
    }

    #[test]
    fn bytes_round_trip_and_reject_corruption() {
        let kit = Synthetic::default();
        let a = run(&kit, 40..140);
        let bytes = a.to_bytes();
        assert_eq!(Accumulator::from_bytes(&bytes).unwrap(), a);

        let empty = Accumulator::new(0);
        assert_eq!(Accumulator::from_bytes(&empty.to_bytes()).unwrap(), empty);
        let empty = Accumulator::new(85);
        let empty_bytes = empty.to_bytes();
        assert_eq!(Accumulator::from_bytes(&empty_bytes).unwrap(), empty);
        assert!(empty_bytes.len() < 6_000, "{} bytes", empty_bytes.len());

        // Version is checked before anything else is interpreted.
        let mut old = bytes.clone();
        old[0] = 1;
        let err = Accumulator::from_bytes(&old).unwrap_err();
        assert!(
            err.to_string().contains("version 1 is not supported"),
            "{err}"
        );
        // The layout before the six equity scalars and three series were appended.
        let mut old = bytes.clone();
        old[0] = 5;
        let err = Accumulator::from_bytes(&old).unwrap_err();
        assert!(
            err.to_string()
                .contains("version 5 is not supported (expected 6)"),
            "{err}"
        );
        assert!(Accumulator::from_bytes(&[]).is_err());
        assert!(Accumulator::from_bytes(&bytes[..10]).is_err());
        assert!(Accumulator::from_bytes(&bytes[..bytes.len() - 1]).is_err());
        // Truncations and single-byte corruptions never panic.
        for cut in (0..bytes.len()).step_by(997) {
            let _ = Accumulator::from_bytes(&bytes[..cut]);
        }
        for at in (1..bytes.len()).step_by(211) {
            let mut b = bytes.clone();
            b[at] ^= 0x55;
            if let Ok(decoded) = Accumulator::from_bytes(&b) {
                assert_eq!(decoded.years, a.years);
            }
        }
        // An absurd horizon is refused before allocating for it.
        let mut huge = Accumulator::new(0);
        huge.years = 1_000_000;
        assert!(Accumulator::from_bytes(&huge.to_bytes()).is_err());
    }

    #[test]
    fn serialised_size_of_a_realistic_500_path_chunk() {
        // 60-year horizon, the first chunk of a run (it carries all 96 sample paths)
        // and a later one. Target: well under 300 KB.
        let kit = Synthetic::default();
        assert_eq!(kit.years, 60);
        let first = run(&kit, 0..500).to_bytes().len();
        let later = run(&kit, 5_000..5_500).to_bytes().len();
        println!("500-path chunk: first {first} bytes, later {later} bytes");
        let scalars = 500 * PathScalar::COUNT * 4;
        println!("  of which scalars {scalars} bytes");
        assert!(first < 215_000, "{first}");
        assert!(later < 175_000, "{later}");
        assert!(later < first);
    }

    #[test]
    fn recording_and_codec_speed() {
        // Reports the cost of the hot call and of the chunk codec (run with --release
        // for the figures quoted in docs/engine-output.md).
        let kit = Synthetic::default();
        let mut template = None;
        struct Capture<'a>(&'a mut Option<YearRecord>);
        impl Recorder for Capture<'_> {
            fn begin_path(&mut self, _: u64) {}
            fn record_year(&mut self, year: usize, record: &YearRecord) {
                if year == 20 {
                    *self.0 = Some(*record);
                }
            }
            fn end_path(&mut self, _: &PathEnd) {}
        }
        kit.run_path(1, &mut Capture(&mut template));
        let template = template.expect("the synthetic life reaches year 20");

        const PATHS: u64 = 2_000;
        let mut acc = Accumulator::new(60);
        let end = PathEnd::default();
        let started = std::time::Instant::now();
        for p in 0..PATHS {
            acc.begin_path(SAMPLE_PATHS + p);
            let mut record = template;
            for y in 0..60 {
                // Drift the balances so successive years land in different bins.
                record.values[YearSeries::BalanceTaxable.index()] *= 1.03;
                acc.record_year(y, &record);
            }
            acc.end_path(&end);
        }
        let per_year = started.elapsed().as_nanos() as f64 / (PATHS * 60) as f64;

        let chunk = run(&kit, 0..500);
        let started = std::time::Instant::now();
        let bytes = chunk.to_bytes();
        let encode = started.elapsed();
        let started = std::time::Instant::now();
        let decoded = Accumulator::from_bytes(&bytes).unwrap();
        let decode = started.elapsed();
        let mut total = Accumulator::new(kit.years);
        let started = std::time::Instant::now();
        total.merge(&decoded).unwrap();
        let merge = started.elapsed();
        println!(
            "record_year {per_year:.0} ns; 500-path chunk: encode {encode:?}, decode {decode:?}, merge {merge:?}"
        );
        if !cfg!(debug_assertions) {
            assert!(per_year < 500.0, "{per_year} ns per record_year");
            assert!(encode.as_millis() < 20 && decode.as_millis() < 20 && merge.as_millis() < 20);
        }
    }
}
