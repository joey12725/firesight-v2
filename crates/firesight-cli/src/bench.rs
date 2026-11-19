//! `firesight bench`: throughput measurement and the regression check.
//!
//! # What is measured
//!
//! * **One thread**: paths per second and month-steps per second of `sim::run_range`
//!   over a quarter of `--paths`, best of `--repeat` runs.
//! * **All threads**: paths per second of the chunked rayon run over `--paths`, best
//!   of `--repeat` runs.
//!
//! Best-of is used because other load on the machine can only make a run slower,
//! never faster: the fastest of a few runs is the least disturbed one.
//!
//! # The machine-readable line
//!
//! The last line printed is one JSON object (see [`Measurement::to_json`]), for
//! scripts: `firesight bench examples/x.json | tail -1`.
//!
//! # The regression check
//!
//! Paths per second depends on the machine, so a committed number cannot be
//! compared with a run on another machine directly. The check compares a
//! *score*: single-thread paths per second divided by the speed of a fixed
//! calibration kernel ([`calibrate`]) measured in the same process just before.
//! The kernel has the engine's instruction mix (64-bit mixing, floating-point
//! multiply, add and divide, a table lookup, one `exp` call), so a machine that is
//! uniformly faster or slower, or throttled, moves both numbers together and the
//! score stays put. It is not a perfect normalisation across CPU families; record
//! the baseline on the kind of machine that runs the check
//! (`--write-baseline`), and treat the 25% tolerance as the room that leaves.
//!
//! `--check FILE` exits with status 1 when the score is more than the baseline's
//! tolerance below the baseline score for the same profile. `tools/bench-check.sh`
//! runs it for the three shipped examples.

use std::path::Path;
use std::time::Instant;

use serde_json::{json, Map, Value};

/// Regression tolerance written to a new baseline file: the check fails when the
/// score has dropped by more than this share (architecture.md section 7).
pub const DEFAULT_TOLERANCE: f64 = 0.25;

/// Iterations of the calibration kernel per timed run (about 15 ms on a 2020s
/// laptop core).
const CALIBRATION_ITERATIONS: u64 = 8_000_000;
/// Timed runs of the kernel; the fastest counts.
const CALIBRATION_RUNS: u32 = 7;

/// One run of the calibration kernel. Returns a value that depends on every
/// iteration so the work cannot be optimised away.
fn kernel(iterations: u64) -> f64 {
    let table: [f64; 64] = std::array::from_fn(|i| 1.0 + i as f64 / 64.0);
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut level = 1.0f64;
    let mut total = 0.0f64;
    for i in 0..iterations {
        // xorshift64: the same kind of work as the engine's generator.
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let u = (x >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
        let r = table[(x >> 58) as usize] * (u - 0.5) * 0.01;
        level *= 1.0 + r;
        total += level / (1.0 + u);
        if i % 64 == 0 {
            total += firesight_core::math::exp(-u);
            if level > 1e6 || level < 1e-6 {
                level = 1.0;
            }
        }
    }
    total + level
}

/// Speed of the calibration kernel on this machine right now, in millions of
/// iterations per second (best of several runs).
pub fn calibrate() -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..CALIBRATION_RUNS {
        let started = Instant::now();
        std::hint::black_box(kernel(std::hint::black_box(CALIBRATION_ITERATIONS)));
        best = best.min(started.elapsed().as_secs_f64());
    }
    CALIBRATION_ITERATIONS as f64 / best / 1e6
}

/// What one `bench` invocation measured.
#[derive(Clone, Debug, PartialEq)]
pub struct Measurement {
    /// Name of the profile: the file stem, or `builtin` for the built-in demo.
    pub profile: String,
    /// Paths timed on all threads (a quarter of it on one thread).
    pub paths: u64,
    /// Timed repetitions of each measurement.
    pub repeat: u32,
    /// Mean months simulated per path.
    pub mean_months: f64,
    /// Single-thread paths per second, best run.
    pub paths_per_sec_1t: f64,
    /// Single-thread month-steps per second, best run.
    pub month_steps_per_sec_1t: f64,
    /// Worker threads of the multi-thread run.
    pub threads: usize,
    /// Paths per second on all threads, best run.
    pub paths_per_sec_mt: f64,
    /// Calibration kernel speed, millions of iterations per second.
    pub calibration: f64,
}

impl Measurement {
    /// Single-thread paths per second per unit of calibration speed: the number the
    /// regression check compares.
    pub fn score(&self) -> f64 {
        self.paths_per_sec_1t / self.calibration
    }

    /// The one-line summary.
    pub fn to_json(&self) -> Value {
        json!({
            "bench": "firesight",
            "version": env!("CARGO_PKG_VERSION"),
            "profile": self.profile,
            "paths": self.paths,
            "repeat": self.repeat,
            "mean_months": round(self.mean_months, 1),
            "paths_per_sec_1t": round(self.paths_per_sec_1t, 0),
            "us_per_path_1t": round(1e6 / self.paths_per_sec_1t, 1),
            "month_steps_per_sec_1t": round(self.month_steps_per_sec_1t, 0),
            "threads": self.threads,
            "paths_per_sec_mt": round(self.paths_per_sec_mt, 0),
            "calibration": round(self.calibration, 2),
            "score": round(self.score(), 3),
        })
    }
}

fn round(v: f64, digits: i32) -> f64 {
    let k = firesight_core::math::powi(10.0, digits);
    (v * k).round() / k
}

/// The profile name used in baselines: the file stem.
pub fn profile_name(file: Option<&Path>) -> String {
    file.and_then(|f| f.file_stem()).map_or_else(
        || "builtin".to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// Outcome of comparing a measurement with a baseline.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// Within tolerance. `change` is the relative change in score (negative: slower).
    Ok { baseline: f64, change: f64 },
    /// Slower than the baseline by more than the tolerance.
    Regressed {
        baseline: f64,
        change: f64,
        tolerance: f64,
    },
    /// The baseline has no entry for this profile.
    Unknown,
}

/// Compares `m` with the baseline document (the format [`update_baseline`] writes).
pub fn check(m: &Measurement, baseline: &Value) -> Result<Verdict, String> {
    let tolerance = baseline
        .get("tolerance")
        .and_then(Value::as_f64)
        .unwrap_or(DEFAULT_TOLERANCE);
    if !(0.0..1.0).contains(&tolerance) {
        return Err(format!("baseline tolerance {tolerance} is not in 0..1"));
    }
    let Some(entry) = baseline.get("profiles").and_then(|p| p.get(&m.profile)) else {
        return Ok(Verdict::Unknown);
    };
    let base = entry
        .get("score")
        .and_then(Value::as_f64)
        .filter(|s| s.is_finite() && *s > 0.0)
        .ok_or_else(|| format!("baseline entry for {} has no positive score", m.profile))?;
    let change = m.score() / base - 1.0;
    Ok(if change < -tolerance {
        Verdict::Regressed {
            baseline: base,
            change,
            tolerance,
        }
    } else {
        Verdict::Ok {
            baseline: base,
            change,
        }
    })
}

/// Adds or replaces the entry for `m` in a baseline document (pass `Value::Null`
/// to start a new one) and returns it.
pub fn update_baseline(existing: Value, m: &Measurement, machine: &str) -> Value {
    let mut root = match existing {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    root.entry("note").or_insert_with(|| {
        json!(
            "Throughput baseline for `firesight bench --check` (tools/bench-check.sh). \
             score = single-thread paths per second / calibration kernel speed, both \
             measured in the same process; the check fails when a score falls more \
             than `tolerance` below the one recorded here. Re-record with \
             `firesight bench <profile> --write-baseline <this file>` on the machine \
             class that runs the check."
        )
    });
    root.entry("tolerance")
        .or_insert_with(|| json!(DEFAULT_TOLERANCE));
    let profiles = root.entry("profiles").or_insert_with(|| json!({}));
    if !profiles.is_object() {
        *profiles = json!({});
    }
    profiles[&m.profile] = json!({
        "score": round(m.score(), 3),
        "paths_per_sec_1t": round(m.paths_per_sec_1t, 0),
        "paths_per_sec_mt": round(m.paths_per_sec_mt, 0),
        "threads": m.threads,
        "calibration": round(m.calibration, 2),
        "machine": machine,
    });
    Value::Object(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measurement(paths_per_sec: f64, calibration: f64) -> Measurement {
        Measurement {
            profile: "demo".into(),
            paths: 20_000,
            repeat: 3,
            mean_months: 680.0,
            paths_per_sec_1t: paths_per_sec,
            month_steps_per_sec_1t: paths_per_sec * 680.0,
            threads: 10,
            paths_per_sec_mt: paths_per_sec * 6.0,
            calibration,
        }
    }

    #[test]
    fn the_check_follows_the_score_not_the_raw_speed() {
        let recorded = measurement(6_000.0, 300.0);
        let baseline = update_baseline(Value::Null, &recorded, "test machine");
        assert_eq!(baseline["tolerance"], json!(DEFAULT_TOLERANCE));
        assert_eq!(baseline["profiles"]["demo"]["score"], json!(20.0));

        // Same code on a machine half as fast: both numbers halve, the score holds.
        let slow_machine = measurement(3_000.0, 150.0);
        assert!(matches!(
            check(&slow_machine, &baseline).unwrap(),
            Verdict::Ok { change, .. } if change.abs() < 1e-9
        ));
        // 20% slower code on the same machine: inside the tolerance.
        assert!(matches!(
            check(&measurement(4_800.0, 300.0), &baseline).unwrap(),
            Verdict::Ok { change, .. } if (change + 0.2).abs() < 1e-9
        ));
        // 30% slower: a regression.
        assert!(matches!(
            check(&measurement(4_200.0, 300.0), &baseline).unwrap(),
            Verdict::Regressed { tolerance, .. } if tolerance == DEFAULT_TOLERANCE
        ));
        // Faster is never a failure.
        assert!(matches!(
            check(&measurement(9_000.0, 300.0), &baseline).unwrap(),
            Verdict::Ok { .. }
        ));
    }

    #[test]
    fn baselines_keep_other_profiles_and_report_missing_ones() {
        let mut other = measurement(5_000.0, 250.0);
        other.profile = "other".into();
        let baseline = update_baseline(Value::Null, &other, "m");
        let baseline = update_baseline(baseline, &measurement(6_000.0, 300.0), "m");
        assert!(baseline["profiles"]["other"].is_object());
        assert!(baseline["profiles"]["demo"].is_object());

        let mut unknown = measurement(1.0, 1.0);
        unknown.profile = "missing".into();
        assert_eq!(check(&unknown, &baseline).unwrap(), Verdict::Unknown);
        assert!(check(&other, &json!({"tolerance": 2.0})).is_err());
    }

    #[test]
    fn the_summary_line_is_one_json_object_with_the_headline_numbers() {
        let line = measurement(6_000.0, 300.0).to_json().to_string();
        assert!(!line.contains('\n'));
        let back: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(back["paths_per_sec_1t"], json!(6000.0));
        assert_eq!(back["score"], json!(20.0));
        assert_eq!(back["profile"], json!("demo"));
    }

    #[test]
    fn calibration_is_positive_and_names_come_from_the_file_stem() {
        assert!(kernel(10_000).is_finite());
        assert_eq!(
            profile_name(Some(Path::new("examples/dual-income-family.json"))),
            "dual-income-family"
        );
        assert_eq!(profile_name(None), "builtin");
    }
}
