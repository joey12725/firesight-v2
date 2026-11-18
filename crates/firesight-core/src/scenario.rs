//! Scenarios, sensitivity levers, goal-seeking and sweeps. **Partly implemented**:
//! [`Scenario`], [`Solver`] and [`GridSweep::cells`] work and are tested; [`levers`]
//! returns a first set that the scenario owner should extend.
//!
//! Responsibility: describe *what to run*, never run it. This module produces patches
//! and consumes metric values; the caller (worker pool or CLI) applies each patch with
//! `patch::apply_to_profile`, runs the paths, summarises, and reports back. That keeps
//! core free of threads and lets the browser spread the work over its workers.
//!
//! Contract:
//!
//! * A scenario is `{ name, patch }` against a base profile, never a full copy.
//! * Every evaluation of a solver or sweep uses the same seed and path indices as the
//!   base run, so differences between cells are paired, not noise.
//! * [`Solver`] is a plain state machine: `next()` yields the next value to evaluate,
//!   `report()` feeds back the metric, until `next()` returns `None`.

// STUB: remove this allow when the module is implemented.
#![allow(dead_code, unused_variables)]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::model::{IncomeKind, Profile, RetirementTrigger};
use crate::patch::PatchOp;
use crate::stats::Summary;

/// A named variation of a base profile.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Scenario {
    /// Stable identifier, unique among a profile's scenarios.
    pub id: String,
    /// Display name, for example "Retire at 55".
    pub name: String,
    /// The edits that turn the base profile into this scenario.
    pub patch: Vec<PatchOp>,
}

/// A sensitivity lever: one assumption or decision with a low and a high setting,
/// each expressed as a patch. Running both gives one bar of a tornado chart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Lever {
    /// Stable identifier, for example "spending".
    pub id: String,
    /// Display label, for example "Spending".
    pub label: String,
    /// Description of the low setting, for example "10% lower".
    pub low_label: String,
    /// Description of the high setting, for example "10% higher".
    pub high_label: String,
    /// Patch producing the low setting.
    pub low: Vec<PatchOp>,
    /// Patch producing the high setting.
    pub high: Vec<PatchOp>,
}

/// The sensitivity levers that apply to a profile. Levers that would be no-ops (no
/// job to change, no withdrawal rate in the trigger) are omitted.
pub fn levers(profile: &Profile) -> Vec<Lever> {
    let mut out = Vec::new();
    let lever = |id: &str,
                 label: &str,
                 low_label: &str,
                 high_label: &str,
                 low: Vec<PatchOp>,
                 high: Vec<PatchOp>| Lever {
        id: id.into(),
        label: label.into(),
        low_label: low_label.into(),
        high_label: high_label.into(),
        low,
        high,
    };

    if !profile.spending.items.is_empty() {
        let scale = |f: f64| -> Vec<PatchOp> {
            profile
                .spending
                .items
                .iter()
                .enumerate()
                .map(|(i, it)| PatchOp::set(format!("/spending/items/{i}/amount"), it.amount * f))
                .collect()
        };
        out.push(lever(
            "spending",
            "Spending",
            "10% lower",
            "10% higher",
            scale(0.9),
            scale(1.1),
        ));
    }

    let salary_ops = |f: f64| -> Vec<PatchOp> {
        profile
            .incomes
            .iter()
            .enumerate()
            .filter_map(|(i, inc)| match &inc.kind {
                IncomeKind::Job(job) => Some(PatchOp::set(
                    format!("/incomes/{i}/kind/salary"),
                    job.salary * f,
                )),
                _ => None,
            })
            .collect()
    };
    if !salary_ops(1.0).is_empty() {
        out.push(lever(
            "salary",
            "Salaries",
            "10% lower",
            "10% higher",
            salary_ops(0.9),
            salary_ops(1.1),
        ));
    }

    if let RetirementTrigger::WhenFunded(w) = profile.retirement.trigger {
        let set = |r: f64| vec![PatchOp::set("/retirement/trigger/withdrawal_rate", r)];
        out.push(lever(
            "withdrawal_rate",
            "Withdrawal rate target",
            "0.5 points lower (retire later, safer)",
            "0.5 points higher (retire sooner, riskier)",
            set((w.withdrawal_rate - 0.005).max(0.005)),
            set(w.withdrawal_rate + 0.005),
        ));
    }

    let m = &profile.assumptions.market;
    let premium = |d: f64| {
        vec![PatchOp::set(
            "/assumptions/market/equity_premium_adjustment",
            m.equity_premium_adjustment + d,
        )]
    };
    out.push(lever(
        "stock_returns",
        "Stock returns",
        "1 point a year lower",
        "1 point a year higher",
        premium(-0.01),
        premium(0.01),
    ));
    let inflation = |d: f64| {
        vec![PatchOp::set(
            "/assumptions/market/inflation_adjustment",
            m.inflation_adjustment + d,
        )]
    };
    out.push(lever(
        "inflation",
        "Inflation",
        "1 point a year lower",
        "1 point a year higher",
        inflation(-0.01),
        inflation(0.01),
    ));

    let haircut = |factor: f64| {
        vec![PatchOp::set(
            "/assumptions/taxes/social_security_haircut",
            json!({"from_year": 2033, "factor": factor}),
        )]
    };
    out.push(lever(
        "social_security",
        "Social Security",
        "benefits cut 21% from 2033",
        "paid in full",
        haircut(0.79),
        vec![PatchOp::set(
            "/assumptions/taxes/social_security_haircut",
            json!(null),
        )],
    ));

    let fees = |r: f64| vec![PatchOp::set("/portfolio/expense_ratio", r)];
    out.push(lever(
        "fees",
        "Investment fees",
        "0.05% a year",
        "1% a year",
        fees(0.0005),
        fees(0.01),
    ));
    out
}

/// A scalar read from a [`Summary`] that a solver or sweep optimises or displays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Share of paths that succeed (0..1).
    SuccessRate,
    /// Median age at financial independence.
    MedianFiAge,
    /// Median retirement age.
    MedianRetirementAge,
    /// Median net worth at the end of the plan.
    MedianTerminalNetWorth,
    /// 10th percentile net worth at the end of the plan.
    P10TerminalNetWorth,
    /// Mean delivered-to-planned retirement spending ratio.
    DeliveredSpendingRatio,
    /// Median lifetime tax.
    MedianLifetimeTax,
    /// Share of paths that reach financial independence (0..1).
    FiRate,
    /// Share of paths that run out of money (0..1).
    DepletionRate,
    /// Share of retired paths with at least one year below 90% of planned spending.
    PathsBelow90,
}

impl Metric {
    /// Reads the metric from a summary.
    pub fn read(self, summary: &Summary) -> f64 {
        match self {
            Metric::SuccessRate => summary.headline.success_rate.value,
            Metric::MedianFiAge => summary.headline.median_fi_age,
            Metric::MedianRetirementAge => summary.headline.median_retirement_age,
            Metric::MedianTerminalNetWorth => summary.headline.median_terminal_net_worth,
            Metric::P10TerminalNetWorth => summary.headline.p10_terminal_net_worth,
            Metric::DeliveredSpendingRatio => summary.headline.delivered_spending_ratio.value,
            Metric::MedianLifetimeTax => summary.headline.median_lifetime_tax,
            Metric::FiRate => summary.headline.fi_rate.value,
            Metric::DepletionRate => summary.headline.depletion_rate.value,
            Metric::PathsBelow90 => summary.spending.paths_below_90.value,
        }
    }
}

/// A goal-seek request: find the value of one numeric profile field at which a metric
/// reaches a target ("how much can we spend and still succeed 90% of the time?").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SolveSpec {
    /// Patch path of the numeric field to vary, for example
    /// "/retirement/trigger/age". Use a [`Lever`]-style multi-op patch through
    /// `scale_paths` instead when several fields move together.
    pub path: String,
    /// Additional paths whose current values are multiplied by `value / base` when
    /// the solved quantity is a common scale factor (for example every budget amount).
    #[serde(default)]
    pub scale_paths: Vec<String>,
    /// Lower end of the search interval.
    pub lo: f64,
    /// Upper end of the search interval.
    pub hi: f64,
    /// The metric to drive.
    pub metric: Metric,
    /// The value the metric should reach.
    pub target: f64,
    /// Stop when the interval is narrower than this.
    pub tolerance: f64,
}

/// Why a solver stopped.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SolveOutcome {
    /// Still running.
    Pending,
    /// The interval narrowed to the tolerance; `value` is its midpoint.
    Converged { value: f64, metric: f64 },
    /// The target is not reached anywhere in `[lo, hi]`; `value` is the end that
    /// comes closest.
    OutOfRange { value: f64, metric: f64 },
}

/// Bisection as a state machine over a monotone (possibly noisy) metric.
///
/// It first evaluates both ends to learn the direction and check the target is
/// bracketed, then halves the interval until it is narrower than the tolerance or the
/// iteration cap is reached. With common random numbers the metric is a step function
/// of the input, so bisection is robust where derivative-based methods are not.
#[derive(Clone, Debug, PartialEq)]
pub struct Solver {
    lo: f64,
    hi: f64,
    target: f64,
    tolerance: f64,
    max_evaluations: u32,
    evaluations: u32,
    f_lo: Option<f64>,
    f_hi: Option<f64>,
    pending: Option<f64>,
    outcome: SolveOutcome,
}

impl Solver {
    /// Default cap on evaluations (two ends plus up to 30 halvings).
    pub const DEFAULT_MAX_EVALUATIONS: u32 = 32;

    /// A solver over `[lo, hi]` for `metric(x) == target`.
    pub fn new(lo: f64, hi: f64, target: f64, tolerance: f64) -> Solver {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        Solver {
            lo,
            hi,
            target,
            tolerance: tolerance.max(f64::EPSILON * hi.abs().max(1.0)),
            max_evaluations: Self::DEFAULT_MAX_EVALUATIONS,
            evaluations: 0,
            f_lo: None,
            f_hi: None,
            pending: None,
            outcome: SolveOutcome::Pending,
        }
    }

    /// A solver for a [`SolveSpec`].
    pub fn from_spec(spec: &SolveSpec) -> Solver {
        Solver::new(spec.lo, spec.hi, spec.target, spec.tolerance)
    }

    /// The next input to evaluate, or `None` when finished. Calling `next` again
    /// before `report` returns the same value.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<f64> {
        if self.outcome != SolveOutcome::Pending {
            return None;
        }
        if self.pending.is_none() {
            self.pending = Some(if self.f_lo.is_none() {
                self.lo
            } else if self.f_hi.is_none() {
                self.hi
            } else {
                0.5 * (self.lo + self.hi)
            });
        }
        self.pending
    }

    /// Feeds back the metric observed at `value` (the value last returned by `next`).
    pub fn report(&mut self, value: f64, metric: f64) {
        if self.outcome != SolveOutcome::Pending || self.pending != Some(value) {
            return;
        }
        self.pending = None;
        self.evaluations += 1;
        let (f_lo, f_hi) = match (self.f_lo, self.f_hi) {
            (None, _) => {
                self.f_lo = Some(metric);
                return;
            }
            (Some(f_lo), None) => {
                self.f_hi = Some(metric);
                // Not bracketed: report the end that comes closest.
                if (f_lo - self.target) * (metric - self.target) > 0.0 {
                    self.outcome = if (f_lo - self.target).abs() <= (metric - self.target).abs() {
                        SolveOutcome::OutOfRange {
                            value: self.lo,
                            metric: f_lo,
                        }
                    } else {
                        SolveOutcome::OutOfRange {
                            value: self.hi,
                            metric,
                        }
                    };
                }
                return;
            }
            (Some(f_lo), Some(f_hi)) => (f_lo, f_hi),
        };
        // Keep the half whose ends still straddle the target.
        if (f_lo - self.target) * (metric - self.target) <= 0.0 {
            self.hi = value;
            self.f_hi = Some(metric);
        } else {
            self.lo = value;
            self.f_lo = Some(metric);
        }
        if self.hi - self.lo <= self.tolerance || self.evaluations >= self.max_evaluations {
            let mid = 0.5 * (self.lo + self.hi);
            let f_mid = 0.5 * (self.f_lo.unwrap_or(metric) + self.f_hi.unwrap_or(metric));
            self.outcome = SolveOutcome::Converged {
                value: mid,
                metric: f_mid,
            };
        }
    }

    /// The current state: pending, converged or out of range.
    pub fn outcome(&self) -> SolveOutcome {
        self.outcome
    }

    /// Number of evaluations reported so far.
    pub fn evaluations(&self) -> u32 {
        self.evaluations
    }
}

/// One axis of a sweep: a numeric field and the values to try.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GridAxis {
    /// Axis label for the chart.
    pub label: String,
    /// Patch path of the numeric field, for example "/retirement/trigger/age".
    pub path: String,
    /// Values to set, in order.
    pub values: Vec<f64>,
}

/// A one- or two-dimensional sweep for heatmaps ("retirement age by spending level").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GridSweep {
    /// Horizontal axis.
    pub x: GridAxis,
    /// Vertical axis; omit for a one-dimensional sweep.
    pub y: Option<GridAxis>,
    /// The metric shown in each cell.
    pub metric: Metric,
}

/// One cell of a sweep: its position and the patch to run.
#[derive(Clone, Debug, PartialEq)]
pub struct GridCell {
    /// Index along `x`.
    pub ix: usize,
    /// Index along `y` (0 for a one-dimensional sweep).
    pub iy: usize,
    /// The patch for this cell.
    pub patch: Vec<PatchOp>,
}

impl GridSweep {
    /// Every cell, row by row (`y` outer, `x` inner).
    pub fn cells(&self) -> Vec<GridCell> {
        let ys: Vec<Option<f64>> = match &self.y {
            Some(axis) => axis.values.iter().copied().map(Some).collect(),
            None => vec![None],
        };
        let mut out = Vec::with_capacity(ys.len() * self.x.values.len());
        for (iy, y) in ys.iter().enumerate() {
            for (ix, x) in self.x.values.iter().enumerate() {
                let mut patch = vec![PatchOp::set(self.x.path.clone(), *x)];
                if let (Some(axis), Some(y)) = (&self.y, y) {
                    patch.push(PatchOp::set(axis.path.clone(), *y));
                }
                out.push(GridCell { ix, iy, patch });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::apply_to_profile;
    use crate::validate::validate;

    fn solve(f: impl Fn(f64) -> f64, lo: f64, hi: f64, target: f64) -> (SolveOutcome, u32) {
        let mut s = Solver::new(lo, hi, target, 1e-6);
        while let Some(x) = s.next() {
            s.report(x, f(x));
        }
        (s.outcome(), s.evaluations())
    }

    #[test]
    fn solver_finds_increasing_and_decreasing_roots() {
        let (out, n) = solve(|x| x * x, 0.0, 10.0, 2.0);
        let SolveOutcome::Converged { value, .. } = out else {
            panic!("{out:?}")
        };
        assert!((value - std::f64::consts::SQRT_2).abs() < 1e-5);
        assert!(n <= Solver::DEFAULT_MAX_EVALUATIONS);
        // Success rate falls as spending rises.
        let (out, _) = solve(|x| 1.0 - x / 100_000.0, 0.0, 100_000.0, 0.9);
        let SolveOutcome::Converged { value, .. } = out else {
            panic!("{out:?}")
        };
        assert!((value - 10_000.0).abs() < 1e-3);
    }

    #[test]
    fn solver_handles_step_functions_and_unbracketed_targets() {
        // A metric from a finite number of paths is a staircase.
        let (out, _) = solve(|x| (x * 10.0).floor() / 10.0, 0.0, 1.0, 0.45);
        let SolveOutcome::Converged { value, .. } = out else {
            panic!("{out:?}")
        };
        assert!((value - 0.5).abs() < 1e-5);
        let (out, n) = solve(|x| x, 0.0, 1.0, 5.0);
        assert_eq!(
            out,
            SolveOutcome::OutOfRange {
                value: 1.0,
                metric: 1.0
            }
        );
        assert_eq!(n, 2);
    }

    #[test]
    fn solver_repeats_pending_value_and_ignores_stale_reports() {
        let mut s = Solver::new(0.0, 8.0, 3.0, 0.5);
        assert_eq!(s.next(), Some(0.0));
        assert_eq!(s.next(), Some(0.0));
        s.report(123.0, 9.0);
        assert_eq!(s.evaluations(), 0);
        s.report(0.0, 0.0);
        assert_eq!(s.next(), Some(8.0));
    }

    #[test]
    fn levers_produce_valid_profiles() {
        let base = Profile::example();
        let all = levers(&base);
        assert!(all.len() >= 6);
        for l in &all {
            let apply = |patch: &Vec<PatchOp>| {
                let p = apply_to_profile(&base, patch).unwrap_or_else(|e| panic!("{}: {e}", l.id));
                assert!(validate(&p).is_ok(), "{}", l.id);
                p
            };
            // One side may coincide with the base (for example "paid in full").
            assert!(apply(&l.low) != apply(&l.high), "{} is a no-op", l.id);
        }
    }

    #[test]
    fn grid_cells_enumerate_row_major() {
        let sweep = GridSweep {
            x: GridAxis {
                label: "Age".into(),
                path: "/retirement/trigger/max_age".into(),
                values: vec![60.0, 65.0],
            },
            y: Some(GridAxis {
                label: "Rate".into(),
                path: "/retirement/trigger/withdrawal_rate".into(),
                values: vec![0.03, 0.04, 0.05],
            }),
            metric: Metric::SuccessRate,
        };
        let cells = sweep.cells();
        assert_eq!(cells.len(), 6);
        assert_eq!((cells[3].ix, cells[3].iy), (1, 1));
        assert_eq!(cells[3].patch.len(), 2);
        let p = apply_to_profile(&Profile::example(), &cells[5].patch).unwrap();
        assert!(validate(&p).is_ok());
    }
}
