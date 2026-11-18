//! Paired comparison of two runs made with the same seed and path indices.
//!
//! Under common random numbers path `i` lives through the same markets, career and
//! health shocks and lifetimes in both runs, so the per-path difference removes the
//! luck and what remains is the effect of the change. For a yes/no outcome the
//! difference in rates then has variance `(d - delta^2) / n`, with `d` the share of
//! paths that flip and `delta` the net change (Glasserman 2003, ch. 4;
//! `docs/research/landscape.md` 2.9): far smaller than for independent runs.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::accumulator::Accumulator;
use super::num::{
    censored_quantile, moments, nan, nan_vec, quantile_sorted, round_dp, sorted_finite, Estimate,
};
use super::summary::{unit_decimals, DOTS, QUANTILE_LEVELS};
use super::{PathScalar, ScalarUnit};

/// A yes/no outcome compared path by path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PairedOutcome {
    /// `success`, `fi` (ever financially independent), `retired` or `depleted`.
    pub outcome: String,
    /// Rate in run A.
    pub a: Estimate,
    /// Rate in run B.
    pub b: Estimate,
    /// Paths where it holds in both runs.
    pub both: u64,
    /// Paths where it holds only in A (for `success`: harmed by the change).
    pub a_only: u64,
    /// Paths where it holds only in B (for `success`: rescued by the change).
    pub b_only: u64,
    /// Paths where it holds in neither.
    pub neither: u64,
    /// Rate in B minus rate in A, with the paired standard error
    /// `sqrt((d - delta^2) / n)` and its 95% interval.
    pub difference: Estimate,
    /// McNemar statistic `(b_only - a_only) / sqrt(a_only + b_only)`; null when no
    /// path flips.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub z: f64,
    /// True if the interval of the difference contains zero.
    pub within_noise: bool,
}

/// A per-path quantity compared path by path (B minus A).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PairedScalar {
    /// Which scalar.
    pub scalar: PathScalar,
    /// Its unit.
    pub unit: ScalarUnit,
    /// Paths with a value in both runs; every statistic below is over these.
    pub both: u64,
    /// Paths with a value only in A.
    pub a_only: u64,
    /// Paths with a value only in B.
    pub b_only: u64,
    /// Paths with a value in neither.
    pub neither: u64,
    /// Mean in A.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub mean_a: f64,
    /// Mean in B.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub mean_b: f64,
    /// Mean of B minus A with its paired standard error and 95% interval.
    pub mean_delta: Estimate,
    /// Quantiles of B minus A at [`QUANTILE_LEVELS`].
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub quantiles: Vec<f64>,
    /// [`DOTS`] equally likely per-path differences for a quantile dot plot.
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub dots: Vec<f64>,
    /// Share of paths where B is higher.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub b_higher: f64,
    /// Share of paths where A is higher.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub a_higher: f64,
    /// Share of paths where the two are equal.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub equal: f64,
    /// True if the interval of the mean difference contains zero.
    pub within_noise: bool,
}

/// A headline metric in both runs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MetricDelta {
    /// `success_rate`, `fi_rate`, `depletion_rate`, `median_fi_age`,
    /// `median_retirement_age` (both censored over all paths),
    /// `median_terminal_net_worth`, `p10_terminal_net_worth`,
    /// `delivered_spending_ratio` (mean), `median_lifetime_tax` or
    /// `median_lifetime_spending`.
    pub metric: String,
    /// Value in run A.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub a: f64,
    /// Value in run B.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub b: f64,
    /// B minus A; null when either is missing.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub delta: f64,
}

/// Run B against run A on the paths both contain.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Comparison {
    /// Paths present in both runs; everything is computed over these.
    pub paths: u64,
    /// Paths only in A (ignored).
    pub unpaired_a: u64,
    /// Paths only in B (ignored).
    pub unpaired_b: u64,
    /// Yes/no outcomes with flip counts.
    pub outcomes: Vec<PairedOutcome>,
    /// Per-path differences of each scalar.
    pub scalars: Vec<PairedScalar>,
    /// Headline metrics side by side.
    pub headline: Vec<MetricDelta>,
}

fn paired_outcome(
    name: &str,
    pairs: &[(&[f32], &[f32])],
    holds: impl Fn(&[f32]) -> bool,
) -> PairedOutcome {
    let (mut both, mut a_only, mut b_only, mut neither) = (0u64, 0u64, 0u64, 0u64);
    for (ra, rb) in pairs {
        match (holds(ra), holds(rb)) {
            (true, true) => both += 1,
            (true, false) => a_only += 1,
            (false, true) => b_only += 1,
            (false, false) => neither += 1,
        }
    }
    let n = pairs.len() as u64;
    let difference = if n == 0 {
        Estimate::NONE
    } else {
        let nf = n as f64;
        let delta = (b_only as f64 - a_only as f64) / nf;
        let flips = (a_only + b_only) as f64 / nf;
        Estimate::normal(delta, ((flips - delta * delta).max(0.0) / nf).sqrt())
    };
    let flips = a_only + b_only;
    PairedOutcome {
        outcome: name.to_string(),
        a: Estimate::proportion(both + a_only, n).rounded(5),
        b: Estimate::proportion(both + b_only, n).rounded(5),
        both,
        a_only,
        b_only,
        neither,
        z: if flips == 0 {
            f64::NAN
        } else {
            round_dp((b_only as f64 - a_only as f64) / (flips as f64).sqrt(), 3)
        },
        within_noise: difference.within_noise(),
        difference: difference.rounded(5),
    }
}

fn paired_scalar(scalar: PathScalar, pairs: &[(&[f32], &[f32])]) -> PairedScalar {
    let i = scalar.index();
    let dp = unit_decimals(scalar.unit());
    let (mut a_only, mut b_only, mut neither) = (0u64, 0u64, 0u64);
    let (mut higher, mut lower, mut equal) = (0u64, 0u64, 0u64);
    let mut deltas: Vec<f32> = Vec::with_capacity(pairs.len());
    let (mut sum_a, mut sum_b) = (0.0f64, 0.0f64);
    for (ra, rb) in pairs {
        let (va, vb) = (ra[i], rb[i]);
        match (va.is_finite(), vb.is_finite()) {
            (true, true) => {
                let (va, vb) = (f64::from(va), f64::from(vb));
                sum_a += va;
                sum_b += vb;
                let d = vb - va;
                if d > 0.0 {
                    higher += 1;
                } else if d < 0.0 {
                    lower += 1;
                } else {
                    equal += 1;
                }
                deltas.push(d as f32);
            }
            (true, false) => a_only += 1,
            (false, true) => b_only += 1,
            (false, false) => neither += 1,
        }
    }
    let both = deltas.len() as u64;
    let mean_delta = Estimate::mean(deltas.iter().map(|d| f64::from(*d)));
    let sorted = sorted_finite(deltas.into_iter());
    let q = |q: f64| round_dp(quantile_sorted(&sorted, q), dp);
    let share = |k: u64| {
        if both == 0 {
            f64::NAN
        } else {
            round_dp(k as f64 / both as f64, 5)
        }
    };
    let mean = |sum: f64| {
        if both == 0 {
            f64::NAN
        } else {
            round_dp(sum / both as f64, (dp + 1).min(6))
        }
    };
    PairedScalar {
        scalar,
        unit: scalar.unit(),
        both,
        a_only,
        b_only,
        neither,
        mean_a: mean(sum_a),
        mean_b: mean(sum_b),
        within_noise: mean_delta.within_noise(),
        mean_delta: mean_delta.rounded((dp + 1).min(6)),
        quantiles: QUANTILE_LEVELS.iter().map(|l| q(*l)).collect(),
        dots: if sorted.is_empty() {
            Vec::new()
        } else {
            (0..DOTS)
                .map(|k| q((k as f64 + 0.5) / DOTS as f64))
                .collect()
        },
        b_higher: share(higher),
        a_higher: share(lower),
        equal: share(equal),
    }
}

/// "This scalar has a value on the path", as a yes/no outcome.
fn finite(s: PathScalar) -> impl Fn(&[f32]) -> bool {
    move |row| row[s.index()].is_finite()
}

/// Compares two runs made with the same seed and path indices. Paths are matched by
/// index; paths present in only one run are counted and left out. `b` is the variant
/// and `a` the baseline: every difference is B minus A.
pub fn compare(a: &Accumulator, b: &Accumulator) -> Comparison {
    let mut pairs: Vec<(&[f32], &[f32])> = Vec::with_capacity(a.paths.min(b.paths) as usize);
    let (mut unpaired_a, mut unpaired_b) = (0u64, 0u64);
    let (mut rows_a, mut rows_b) = (a.rows().peekable(), b.rows().peekable());
    loop {
        match (rows_a.peek().copied(), rows_b.peek().copied()) {
            (Some((pa, ra)), Some((pb, rb))) => match pa.cmp(&pb) {
                std::cmp::Ordering::Equal => {
                    pairs.push((ra, rb));
                    rows_a.next();
                    rows_b.next();
                }
                std::cmp::Ordering::Less => {
                    unpaired_a += 1;
                    rows_a.next();
                }
                std::cmp::Ordering::Greater => {
                    unpaired_b += 1;
                    rows_b.next();
                }
            },
            (Some(_), None) => {
                unpaired_a += 1;
                rows_a.next();
            }
            (None, Some(_)) => {
                unpaired_b += 1;
                rows_b.next();
            }
            (None, None) => break,
        }
    }
    let n = pairs.len() as u64;
    let outcomes = vec![
        paired_outcome("success", &pairs, |row| {
            row[PathScalar::Success.index()] == 1.0
        }),
        paired_outcome("fi", &pairs, finite(PathScalar::FiAge)),
        paired_outcome("retired", &pairs, finite(PathScalar::RetirementAge)),
        paired_outcome("depleted", &pairs, finite(PathScalar::DepletionAge)),
    ];
    let scalars = PathScalar::ALL
        .iter()
        .filter(|s| !matches!(s.unit(), ScalarUnit::Index | ScalarUnit::Flag))
        .map(|s| paired_scalar(*s, &pairs))
        .collect();

    let sorted = |s: PathScalar, second: bool| {
        sorted_finite(
            pairs
                .iter()
                .map(move |(ra, rb)| if second { rb[s.index()] } else { ra[s.index()] }),
        )
    };
    let mut headline = Vec::new();
    let mut metric = |name: &str, va: f64, vb: f64, dp: u32| {
        let (va, vb) = (round_dp(va, dp), round_dp(vb, dp));
        headline.push(MetricDelta {
            metric: name.to_string(),
            a: va,
            b: vb,
            delta: round_dp(vb - va, dp),
        });
    };
    for o in &outcomes[..] {
        let name = match o.outcome.as_str() {
            "success" => "success_rate",
            "fi" => "fi_rate",
            "depleted" => "depletion_rate",
            _ => continue,
        };
        metric(name, o.a.value, o.b.value, 5);
    }
    for (name, s) in [
        ("median_fi_age", PathScalar::FiAge),
        ("median_retirement_age", PathScalar::RetirementAge),
    ] {
        let q = |second| censored_quantile(&sorted(s, second), n, 0.5);
        metric(name, q(false), q(true), 2);
    }
    for (name, s, level) in [
        (
            "median_terminal_net_worth",
            PathScalar::TerminalNetWorth,
            0.5,
        ),
        ("p10_terminal_net_worth", PathScalar::TerminalNetWorth, 0.1),
        ("median_lifetime_tax", PathScalar::LifetimeTax, 0.5),
        (
            "median_lifetime_spending",
            PathScalar::LifetimeSpending,
            0.5,
        ),
    ] {
        let q = |second| quantile_sorted(&sorted(s, second), level);
        metric(name, q(false), q(true), 0);
    }
    let mean = |second| {
        let (count, mean, _) = moments(
            sorted(PathScalar::DeliveredSpendingRatio, second)
                .iter()
                .map(|v| f64::from(*v)),
        );
        if count == 0 {
            f64::NAN
        } else {
            mean
        }
    };
    metric("delivered_spending_ratio", mean(false), mean(true), 4);

    Comparison {
        paths: n,
        unpaired_a,
        unpaired_b,
        outcomes,
        scalars,
        headline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::testkit::Synthetic;
    use crate::stats::{PathEnd, Recorder, YearRecord, YearSeries};

    /// Ten hand-built paths per run.
    fn hand(run_b: bool) -> Accumulator {
        let success_a = [1, 1, 1, 1, 1, 1, 0, 0, 0, 0];
        let success_b = [1, 1, 1, 1, 1, 0, 1, 1, 1, 0];
        let mut acc = Accumulator::new(1);
        for i in 0..10usize {
            acc.begin_path(i as u64);
            let mut r = YearRecord::default();
            // Terminal net worth: A = 100k * (i + 1); B = A + 10k * (i - 4).
            let mut worth = 100_000.0 * (i as f64 + 1.0);
            if run_b {
                worth += 10_000.0 * (i as f64 - 4.0);
            }
            r.set(YearSeries::BalanceTaxable, worth);
            acc.record_year(0, &r);
            let mut end = PathEnd::default();
            // FI: A reaches it on paths 0..8 at 50 + i; B two years sooner, and also
            // on path 8 at 60.
            if i < 8 {
                end.set(
                    PathScalar::FiAge,
                    50.0 + i as f64 - if run_b { 2.0 } else { 0.0 },
                );
            } else if i == 8 && run_b {
                end.set(PathScalar::FiAge, 60.0);
            }
            let ok = if run_b { success_b[i] } else { success_a[i] };
            end.set(PathScalar::Success, f64::from(ok));
            acc.end_path(&end);
        }
        acc
    }

    #[test]
    fn hand_built_paired_comparison() {
        let c = compare(&hand(false), &hand(true));
        assert_eq!((c.paths, c.unpaired_a, c.unpaired_b), (10, 0, 0));

        let s = &c.outcomes[0];
        assert_eq!(s.outcome, "success");
        assert_eq!((s.both, s.a_only, s.b_only, s.neither), (5, 1, 3, 1));
        assert_eq!((s.a.value, s.b.value), (0.6, 0.8));
        assert_eq!(s.difference.value, 0.2);
        // d = 4/10 flip, delta = 0.2: SE = sqrt((0.4 - 0.04) / 10) = 0.18974.
        assert_eq!(s.difference.std_error, 0.189_737);
        assert_eq!(s.z, 1.0);
        assert!(s.within_noise);
        // Independent runs would have SE sqrt(0.6*0.4/10 + 0.8*0.2/10) = 0.2.
        assert!(s.difference.std_error < 0.2);

        let fi = &c.outcomes[1];
        assert_eq!((fi.both, fi.a_only, fi.b_only, fi.neither), (8, 0, 1, 1));
        assert_eq!(fi.difference.value, 0.1);
        // One flip of ten: sqrt((0.1 - 0.01) / 10) = 0.094868.
        assert_eq!(fi.difference.std_error, 0.094_868);
        assert_eq!(fi.z, 1.0);
        let dep = &c.outcomes[3];
        assert_eq!(dep.outcome, "depleted");
        assert_eq!(dep.neither, 10);
        assert!(dep.z.is_nan());
        assert_eq!(dep.difference.value, 0.0);
        assert_eq!(dep.difference.std_error, 0.0);
        assert!(dep.within_noise);

        let scalar = |s: PathScalar| c.scalars.iter().find(|x| x.scalar == s).unwrap();
        let age = scalar(PathScalar::FiAge);
        assert_eq!(
            (age.both, age.a_only, age.b_only, age.neither),
            (8, 0, 1, 1)
        );
        assert_eq!((age.mean_a, age.mean_b), (53.5, 51.5));
        assert_eq!(age.mean_delta.value, -2.0);
        assert_eq!(age.mean_delta.std_error, 0.0);
        assert!(!age.within_noise);
        assert!(age.quantiles.iter().all(|q| *q == -2.0));
        assert_eq!((age.b_higher, age.a_higher, age.equal), (0.0, 1.0, 0.0));
        assert_eq!(age.dots.len(), DOTS);

        let worth = scalar(PathScalar::TerminalNetWorth);
        assert_eq!(worth.both, 10);
        // Deltas -40k, -30k, ..., +50k: mean 5k, sample sd 30,277, SE 9,574.3.
        assert_eq!(worth.mean_delta.value, 5_000.0);
        assert_eq!(worth.mean_delta.std_error, 9_574.271);
        assert!(worth.within_noise);
        assert_eq!(
            (worth.b_higher, worth.a_higher, worth.equal),
            (0.5, 0.4, 0.1)
        );
        assert_eq!(worth.quantiles[4], 5_000.0);
        assert_eq!((worth.dots[0], worth.dots[99]), (-39_550.0, 49_550.0));
        assert_eq!((worth.mean_a, worth.mean_b), (550_000.0, 555_000.0));
        // Index and flag scalars are not differenced.
        assert!(c.scalars.iter().all(|x| x.scalar != PathScalar::Success));
        assert_eq!(c.scalars.len(), PathScalar::COUNT - 2);

        let metric = |name: &str| c.headline.iter().find(|m| m.metric == name).unwrap();
        assert_eq!(metric("success_rate").delta, 0.2);
        assert_eq!(metric("fi_rate").delta, 0.1);
        // Censored medians over ten paths: the 5th FI age, 54 and 52.
        let fi_age = metric("median_fi_age");
        assert_eq!((fi_age.a, fi_age.b, fi_age.delta), (54.0, 52.0, -2.0));
        assert_eq!(metric("median_terminal_net_worth").delta, 5_000.0);
        assert!(metric("delivered_spending_ratio").delta.is_nan());

        let json = serde_json::to_string(&c).unwrap();
        let back: Comparison = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    #[test]
    fn identical_runs_differ_by_exactly_nothing() {
        let kit = Synthetic::default();
        let mut acc = Accumulator::new(kit.years);
        for p in 0..500 {
            kit.run_path(p, &mut acc);
        }
        let c = compare(&acc, &acc);
        assert_eq!(c.paths, 500);
        for o in &c.outcomes {
            assert_eq!((o.a_only, o.b_only), (0, 0));
            assert_eq!(o.difference.value, 0.0);
            assert_eq!(o.difference.std_error, 0.0);
        }
        for s in &c.scalars {
            if s.both > 0 {
                assert_eq!(s.equal, 1.0);
                assert_eq!(s.mean_delta.value, 0.0);
            } else {
                assert!(s.equal.is_nan() && s.mean_delta.value.is_nan());
            }
        }
        assert!(c
            .headline
            .iter()
            .all(|m| m.delta == 0.0 || m.delta.is_nan()));
    }

    #[test]
    fn common_random_numbers_make_the_difference_sharp() {
        // B saves $3,000 a year more than A in the same simulated worlds.
        let a_kit = Synthetic::default();
        let b_kit = Synthetic {
            saving: a_kit.saving + 3_000.0,
            ..a_kit
        };
        let run = |kit: &Synthetic, paths: std::ops::Range<u64>| {
            let mut acc = Accumulator::new(kit.years);
            for p in paths {
                kit.run_path(p, &mut acc);
            }
            acc
        };
        let (a, b) = (run(&a_kit, 0..4_000), run(&b_kit, 0..4_000));
        let c = compare(&a, &b);
        let s = &c.outcomes[0];
        assert_eq!(s.both + s.a_only + s.b_only + s.neither, 4_000);
        assert!((s.difference.value - (s.b.value - s.a.value)).abs() < 2e-5);
        assert!(s.b_only > s.a_only, "saving more rescues paths");
        assert!(!s.within_noise);
        // The paired standard error is far below that of two independent runs.
        let independent = (s.a.std_error * s.a.std_error + s.b.std_error * s.b.std_error).sqrt();
        assert!(s.difference.std_error < 0.5 * independent);
        // Most paths are unchanged.
        assert!(s.both + s.neither > 3_600);
        let fi = c
            .scalars
            .iter()
            .find(|x| x.scalar == PathScalar::FiAge)
            .unwrap();
        assert!(
            fi.mean_delta.value < 0.0 && !fi.within_noise,
            "FI comes sooner"
        );
        assert!(fi.a_higher > fi.b_higher);
        assert!(fi.b_only >= fi.a_only);

        // Runs covering different paths pair on the overlap only.
        let c = compare(&run(&a_kit, 0..300), &run(&b_kit, 200..450));
        assert_eq!((c.paths, c.unpaired_a, c.unpaired_b), (100, 200, 150));
        let c = compare(&Accumulator::new(3), &run(&b_kit, 0..5));
        assert_eq!((c.paths, c.unpaired_b), (0, 5));
        assert!(c.outcomes[0].difference.value.is_nan());
        serde_json::to_string(&c).unwrap();
    }
}
