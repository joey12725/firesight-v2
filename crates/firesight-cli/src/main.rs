//! `firesight`: the native command-line runner.
//!
//! ```text
//! firesight run <profile> [--paths N] [--seed S] [--threads T] [--json] [--summary-out FILE]
//! firesight compare <profile> <patch.json>... [--paths N] [--seed S] [--json]
//! firesight solve <profile> --lever <patch path> --target-success 0.9 --lo A --hi B
//! firesight sweep <profile> --x-path <patch path> --x-values a,b,c [--y-path .. --y-values ..]
//! firesight trace <profile> (--path I | --quantile Q [--by SCALAR]) [--json]
//! firesight preview <profile>
//! firesight bench [profile] [--paths N]
//! firesight schema | validate <profile> | migrate <v1> | examples <dir>
//! ```
//!
//! Simulation commands spread chunks of paths over rayon workers, each with its own
//! `Workspace`, and merge the accumulators; the result does not depend on the number
//! of threads. A profile without a `start` month starts in the current month.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use firesight_core::data::Data;
use firesight_core::legacy;
use firesight_core::model::{Profile, YearMonth, EXAMPLE_START};
use firesight_core::patch::{self, PatchOp};
use firesight_core::scenario::{GridAxis, GridSweep, Metric, SolveOutcome, Solver};
use firesight_core::sim::{self, Plan, Workspace};
use firesight_core::stats::{compare, Accumulator, PathScalar, PathTrace, Summary};
use firesight_core::validate::{validate, Issue};
use rayon::prelude::*;

/// Paths per work item. Small enough to balance threads, large enough that the
/// per-chunk accumulator is a small part of the work.
const CHUNK: u64 = 250;

#[derive(Parser)]
#[command(
    name = "firesight",
    version,
    about = "Monte Carlo early-retirement simulator"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the JSON Schema of the profile format.
    Schema,
    /// Check a profile and list errors and warnings. Exits 1 if there are errors.
    Validate {
        /// Profile JSON file.
        file: PathBuf,
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Convert a v1 profile to the v2 format. Assumptions are listed on stderr.
    Migrate {
        /// v1 profile JSON file.
        file: PathBuf,
        /// Write the v2 profile here instead of stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Write the demo profiles to a directory as `<slug>.json`.
    Examples {
        /// Target directory (created if missing).
        dir: PathBuf,
        /// Start month of the demos, YYYY-MM. Defaults to the month the checked-in
        /// examples use, so that regenerating them is reproducible.
        #[arg(long)]
        start: Option<YearMonth>,
    },
    /// Run a simulation and print the headline results.
    Run {
        /// Profile JSON file.
        file: PathBuf,
        /// Number of paths (default: the profile's setting).
        #[arg(long)]
        paths: Option<u32>,
        /// Random seed (default: the profile's setting).
        #[arg(long)]
        seed: Option<u64>,
        /// Worker threads (default: all cores).
        #[arg(long)]
        threads: Option<usize>,
        /// Print the whole `Summary` as JSON instead of the headline.
        #[arg(long)]
        json: bool,
        /// Also write the `Summary` JSON to this file.
        #[arg(long)]
        summary_out: Option<PathBuf>,
    },
    /// Compare scenarios (patch files) against a base profile on the same paths.
    Compare {
        /// Base profile JSON file.
        file: PathBuf,
        /// Patch files: a JSON array of operations, or `{ "name": ..., "patch": [...] }`.
        #[arg(required = true)]
        scenarios: Vec<PathBuf>,
        /// Number of paths (default: the profile's setting).
        #[arg(long)]
        paths: Option<u32>,
        /// Random seed (default: the profile's setting).
        #[arg(long)]
        seed: Option<u64>,
        /// Worker threads (default: all cores).
        #[arg(long)]
        threads: Option<usize>,
        /// Print each `Comparison` as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Find the value of a numeric field at which a metric hits a target.
    Solve {
        /// Profile JSON file.
        file: PathBuf,
        /// Patch path of the numeric field to vary, for example
        /// /retirement/trigger/withdrawal_rate.
        #[arg(long, alias = "path")]
        lever: String,
        /// Metric to drive.
        #[arg(long, default_value = "success_rate")]
        metric: String,
        /// Target value of the metric.
        #[arg(long, alias = "target-success", default_value_t = 0.9)]
        target: f64,
        /// Lower bound of the search.
        #[arg(long)]
        lo: f64,
        /// Upper bound of the search.
        #[arg(long)]
        hi: f64,
        /// Stop when the interval is narrower than this (default: 0.5% of its width).
        #[arg(long)]
        tolerance: Option<f64>,
        /// Paths per evaluation.
        #[arg(long, default_value_t = 2_000)]
        paths: u32,
        /// Worker threads (default: all cores).
        #[arg(long)]
        threads: Option<usize>,
    },
    /// Evaluate a metric over a grid of one or two numeric fields.
    Sweep {
        /// Profile JSON file.
        file: PathBuf,
        /// Patch path of the horizontal axis.
        #[arg(long)]
        x_path: String,
        /// Values along the horizontal axis, comma separated.
        #[arg(long, value_delimiter = ',')]
        x_values: Vec<f64>,
        /// Patch path of the vertical axis.
        #[arg(long)]
        y_path: Option<String>,
        /// Values along the vertical axis, comma separated.
        #[arg(long, value_delimiter = ',')]
        y_values: Vec<f64>,
        /// Metric shown in each cell.
        #[arg(long, default_value = "success_rate")]
        metric: String,
        /// Paths per cell.
        #[arg(long, default_value_t = 2_000)]
        paths: u32,
        /// Worker threads (default: all cores).
        #[arg(long)]
        threads: Option<usize>,
    },
    /// Print one path year by year with its events.
    Trace {
        /// Profile JSON file.
        file: PathBuf,
        /// Path index.
        #[arg(long)]
        path: Option<u64>,
        /// Pick the path at this quantile (0..1) of `--by` in a run of `--paths`.
        #[arg(long)]
        quantile: Option<f64>,
        /// Scalar the quantile refers to.
        #[arg(long, default_value = "terminal_net_worth")]
        by: String,
        /// Paths simulated to find the quantile.
        #[arg(long, default_value_t = 2_000)]
        paths: u32,
        /// Random seed (default: the profile's setting).
        #[arg(long)]
        seed: Option<u64>,
        /// Print the whole `PathTrace` as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print the deterministic facts of a plan (no simulation).
    Preview {
        /// Profile JSON file.
        file: PathBuf,
    },
    /// Measure paths per second, single- and multi-threaded.
    Bench {
        /// Profile JSON file (default: the built-in dual-income demo).
        file: Option<PathBuf>,
        /// Number of paths to time.
        #[arg(long, default_value_t = 20_000)]
        paths: u32,
    },
}

/// The current month in UTC, from the system clock (civil-from-days, Howard Hinnant).
fn today() -> YearMonth {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    YearMonth::new(year as i32, month as u32).unwrap_or_default()
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn print_issues(kind: &str, issues: &[Issue]) {
    for issue in issues {
        println!(
            "{kind} [{}] {}: {}",
            issue.code,
            if issue.path.is_empty() {
                "/"
            } else {
                &issue.path
            },
            issue.message
        );
    }
}

/// Reads a profile and fills in the start month.
fn load(file: &Path) -> Result<Profile, String> {
    Profile::from_json(&read(file)?)
        .map(|p| p.with_start(today()))
        .map_err(|e| format!("{}: {e}", file.display()))
}

fn compile(profile: &Profile) -> Result<Plan, String> {
    Plan::compile(profile, Data::embedded()).map_err(|report| {
        let lines: Vec<String> = report
            .errors
            .iter()
            .map(|i| format!("[{}] {}: {}", i.code, i.path, i.message))
            .collect();
        format!("the profile cannot be simulated:\n  {}", lines.join("\n  "))
    })
}

fn pool(threads: Option<usize>) -> Result<rayon::ThreadPool, String> {
    let mut builder = rayon::ThreadPoolBuilder::new();
    if let Some(n) = threads {
        builder = builder.num_threads(n.max(1));
    }
    builder.build().map_err(|e| e.to_string())
}

/// Simulates paths `0..paths` on the pool. Chunks merge in any order to the same
/// bytes, so the result does not depend on scheduling.
fn simulate(plan: &Plan, seed: u64, paths: u64, pool: &rayon::ThreadPool) -> Accumulator {
    let chunks: Vec<u64> = (0..paths).step_by(CHUNK as usize).collect();
    pool.install(|| {
        chunks
            .into_par_iter()
            .map_init(
                || Workspace::new(plan),
                |ws, first| {
                    let mut acc = plan.new_accumulator();
                    for i in first..(first + CHUNK).min(paths) {
                        sim::run_path(plan, seed, i, ws, &mut acc);
                    }
                    acc
                },
            )
            .reduce(
                || plan.new_accumulator(),
                |mut a, b| {
                    a.merge(&b).expect("chunks of one plan always merge");
                    a
                },
            )
    })
}

fn dollars(v: f64) -> String {
    if !v.is_finite() {
        return "n/a".into();
    }
    let n = v.round() as i64;
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{}${out}", if n < 0 { "-" } else { "" })
}

fn age(v: f64) -> String {
    if v.is_finite() {
        format!("{v:.1}")
    } else {
        "never".into()
    }
}

fn pct(v: f64) -> String {
    if v.is_finite() {
        format!("{:.1}%", v * 100.0)
    } else {
        "n/a".into()
    }
}

fn print_headline(summary: &Summary) {
    let h = &summary.headline;
    println!("paths                  {}", summary.paths);
    println!(
        "success rate           {}  (95% interval {} to {})",
        pct(h.success_rate.value),
        pct(h.success_rate.lo),
        pct(h.success_rate.hi)
    );
    println!(
        "depletion risk         {}  (median age when it happens {})",
        pct(h.depletion_rate.value),
        age(h.depletion_age.p50)
    );
    println!(
        "financially independent {} of paths; age p10 {} / p50 {} / p90 {}",
        pct(h.fi_rate.value),
        age(h.fi_age.p10),
        age(h.fi_age.p50),
        age(h.fi_age.p90)
    );
    println!(
        "retirement age         p10 {} / p50 {} / p90 {}",
        age(h.retirement_age.p10),
        age(h.retirement_age.p50),
        age(h.retirement_age.p90)
    );
    println!(
        "spending delivered     {} of plan on average; {} of retired paths had a year below 90%",
        pct(h.delivered_spending_ratio.value),
        pct(summary.spending.paths_below_90.value)
    );
    println!(
        "terminal net worth     p10 {} / p50 {} / p90 {}",
        dollars(h.terminal_net_worth.p10),
        dollars(h.terminal_net_worth.p50),
        dollars(h.terminal_net_worth.p90)
    );
    println!("lifetime tax (median)  {}", dollars(h.median_lifetime_tax));
    if summary.failure.failed > 0 {
        let mut causes: Vec<_> = summary
            .failure
            .causes
            .iter()
            .filter(|c| c.count > 0)
            .collect();
        causes.sort_by_key(|c| std::cmp::Reverse(c.count));
        let text: Vec<String> = causes
            .iter()
            .take(4)
            .map(|c| format!("{:?} {}", c.cause, pct(c.share_of_failed)))
            .collect();
        println!("failed paths, first cause: {}", text.join(", "));
    }
}

fn parse_metric(name: &str) -> Result<Metric, String> {
    serde_json::from_value(serde_json::Value::String(name.to_string())).map_err(|_| {
        format!(
            "unknown metric {name:?}: use success_rate, median_fi_age, median_retirement_age, \
             median_terminal_net_worth, p10_terminal_net_worth, delivered_spending_ratio or \
             median_lifetime_tax"
        )
    })
}

/// A patch file: a bare array of operations or `{ "name": ..., "patch": [...] }`.
fn load_patch(file: &Path) -> Result<(String, Vec<PatchOp>), String> {
    let value: serde_json::Value =
        serde_json::from_str(&read(file)?).map_err(|e| format!("{}: {e}", file.display()))?;
    let default_name = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (name, ops) = match &value {
        serde_json::Value::Object(map) => (
            map.get("name")
                .and_then(|n| n.as_str())
                .map_or(default_name, str::to_string),
            map.get("patch").cloned().unwrap_or(serde_json::json!([])),
        ),
        _ => (default_name, value),
    };
    let ops = serde_json::from_value(ops).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok((name, ops))
}

fn print_trace(trace: &PathTrace) {
    let annual = &trace.annual;
    let column = |key: &str| -> Vec<f64> {
        annual
            .columns
            .iter()
            .find(|c| c.key == key)
            .map_or_else(|| vec![0.0; annual.years.len()], |c| c.values.clone())
    };
    let shown = [
        ("income", "gross_income"),
        ("tax", "total_tax"),
        ("spending", "spending"),
        ("planned", "spend_planned"),
        ("cash", "balance_cash"),
        ("taxable", "balance_taxable"),
        ("traditional", "balance_traditional"),
        ("roth", "balance_roth"),
        ("hsa", "balance_hsa"),
        ("property", "property_value"),
        ("mortgage", "mortgage"),
        ("debt", "debt"),
    ];
    let columns: Vec<Vec<f64>> = shown.iter().map(|(_, key)| column(key)).collect();
    println!(
        "path {} (seed {}), {} months, today's dollars",
        trace.path_index, trace.seed, trace.months
    );
    print!("year  status          ");
    for (label, _) in shown {
        print!("{label:>12}");
    }
    println!();
    for (i, year) in annual.years.iter().enumerate() {
        let status = trace
            .statuses
            .get(annual.status[i] as usize)
            .map_or("?", |s| s.key());
        print!("{year}  {status:<16}");
        for c in &columns {
            print!("{:>12.0}", c[i]);
        }
        println!();
    }
    println!("\nevents");
    for e in &trace.events {
        match e.amount {
            Some(a) => println!("  {}  {:<24} {} ({a:.0})", e.month, e.kind.key(), e.label),
            None => println!("  {}  {:<24} {}", e.month, e.kind.key(), e.label),
        }
    }
    println!("\nscalars");
    for s in &trace.scalars {
        if s.value.is_finite() {
            println!("  {:<28} {:.4}", s.scalar.key(), s.value);
        }
    }
}

fn bench(profile: &Profile, paths: u32) -> Result<(), String> {
    let plan = compile(profile)?;
    let paths = u64::from(paths.max(100));
    let seed = plan.seed();
    // Warm up caches and the lazily built tables.
    let mut warm = plan.new_accumulator();
    sim::run_range(&plan, seed, 0..50, &mut warm);

    let single = (paths / 4).max(100);
    let started = Instant::now();
    let mut acc = plan.new_accumulator();
    sim::run_range(&plan, seed, 0..single, &mut acc);
    let elapsed = started.elapsed().as_secs_f64();
    let months: f64 = acc
        .scalar_column(PathScalar::YearsSimulated)
        .iter()
        .map(|y| f64::from(*y) * 12.0)
        .sum();
    println!(
        "horizon                {} years ({} months); mean path about {:.0} months",
        plan.horizon_years(),
        plan.horizon_months(),
        months / single as f64
    );
    println!(
        "1 thread               {:>9.0} paths/s  {:>6.1} M month-steps/s  ({} paths in {:.2} s, {:.0} us/path)",
        single as f64 / elapsed,
        months / elapsed / 1e6,
        single,
        elapsed,
        elapsed / single as f64 * 1e6
    );
    let all = pool(None)?;
    let started = Instant::now();
    let acc = simulate(&plan, seed, paths, &all);
    let elapsed = started.elapsed().as_secs_f64();
    println!(
        "{} threads{}{:>9.0} paths/s  ({} paths in {:.2} s)",
        all.current_num_threads(),
        " ".repeat(13usize.saturating_sub(all.current_num_threads().to_string().len())),
        paths as f64 / elapsed,
        paths,
        elapsed
    );
    let started = Instant::now();
    let bytes = acc.to_bytes();
    let summary = plan.summarize(&acc);
    println!(
        "summary                {:.1} ms; accumulator {} KB",
        started.elapsed().as_secs_f64() * 1e3,
        bytes.len() / 1024
    );
    println!(
        "success rate           {}",
        pct(summary.headline.success_rate.value)
    );
    Ok(())
}

fn run(cli: Cli) -> Result<ExitCode, String> {
    match cli.command {
        Command::Schema => {
            println!(
                "{}",
                serde_json::to_string_pretty(&Profile::json_schema()).map_err(|e| e.to_string())?
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Validate { file, json } => {
            let profile = Profile::from_json(&read(&file)?)
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let report = validate(&profile.with_start(today()));
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
                );
            } else {
                print_issues("error  ", &report.errors);
                print_issues("warning", &report.warnings);
                println!(
                    "{} error(s), {} warning(s)",
                    report.errors.len(),
                    report.warnings.len()
                );
            }
            Ok(if report.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Migrate { file, output } => {
            let v1 = serde_json::from_str(&read(&file)?)
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let (profile, notes) = legacy::migrate_v1_at(&v1, today())?;
            for note in &notes {
                eprintln!("note: {note}");
            }
            let json = profile.to_json_pretty();
            match output {
                Some(path) => std::fs::write(&path, json + "\n")
                    .map_err(|e| format!("{}: {e}", path.display()))?,
                None => println!("{json}"),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Examples { dir, start } => {
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for example in Profile::examples(start.unwrap_or(EXAMPLE_START)) {
                let path = dir.join(format!("{}.json", example.slug));
                std::fs::write(&path, example.profile.to_json_pretty() + "\n")
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                println!("wrote {}", path.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            file,
            paths,
            seed,
            threads,
            json,
            summary_out,
        } => {
            let profile = load(&file)?;
            let plan = compile(&profile)?;
            let paths = u64::from(paths.unwrap_or(profile.simulation.paths));
            let seed = seed.unwrap_or(plan.seed());
            let pool = pool(threads)?;
            let started = Instant::now();
            let acc = simulate(&plan, seed, paths, &pool);
            let elapsed = started.elapsed().as_secs_f64();
            let summary = plan.summarize(&acc);
            let text = serde_json::to_string(&summary).map_err(|e| e.to_string())?;
            if let Some(path) = summary_out {
                std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            if json {
                println!("{text}");
            } else {
                println!("{} (start {}, seed {seed})", profile.name, plan.start());
                print_headline(&summary);
                println!(
                    "{:.0} paths/s on {} thread(s) ({:.2} s)",
                    paths as f64 / elapsed,
                    pool.current_num_threads(),
                    elapsed
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Compare {
            file,
            scenarios,
            paths,
            seed,
            threads,
            json,
        } => {
            let profile = load(&file)?;
            let plan = compile(&profile)?;
            let paths = u64::from(paths.unwrap_or(profile.simulation.paths));
            let seed = seed.unwrap_or(plan.seed());
            let pool = pool(threads)?;
            let base = simulate(&plan, seed, paths, &pool);
            for scenario in scenarios {
                let (name, ops) = load_patch(&scenario)?;
                let changed = patch::apply_to_profile(&profile, &ops)
                    .map_err(|e| format!("{}: {e}", scenario.display()))?;
                // Same seed, same path indices: the same lives with one decision changed.
                let other = simulate(&compile(&changed)?, seed, paths, &pool);
                let c = compare(&base, &other);
                if json {
                    println!("{}", serde_json::to_string(&c).map_err(|e| e.to_string())?);
                    continue;
                }
                println!("\n{name}  ({} paired paths; B minus A)", c.paths);
                for o in &c.outcomes {
                    println!(
                        "  {:<10} {} -> {}   change {:+.1} points (95% {:+.1} to {:+.1}){}   rescued {} / harmed {}",
                        o.outcome,
                        pct(o.a.value),
                        pct(o.b.value),
                        o.difference.value * 100.0,
                        o.difference.lo * 100.0,
                        o.difference.hi * 100.0,
                        if o.within_noise { " within noise" } else { "" },
                        o.b_only,
                        o.a_only
                    );
                }
                for m in &c.headline {
                    println!(
                        "  {:<28} {:>14.3} -> {:>14.3}   {:+.3}",
                        m.metric, m.a, m.b, m.delta
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Solve {
            file,
            lever,
            metric,
            target,
            lo,
            hi,
            tolerance,
            paths,
            threads,
        } => {
            let profile = load(&file)?;
            let metric = parse_metric(&metric)?;
            let pool = pool(threads)?;
            let tolerance = tolerance.unwrap_or((hi - lo).abs() * 0.005);
            let mut solver = Solver::new(lo, hi, target, tolerance);
            while let Some(x) = solver.next() {
                let changed = patch::apply_to_profile(&profile, &[PatchOp::set(lever.clone(), x)])
                    .map_err(|e| e.to_string())?;
                let plan = compile(&changed)?;
                let acc = simulate(&plan, plan.seed(), u64::from(paths), &pool);
                let value = metric.read(&plan.summarize(&acc));
                println!("  {lever} = {x:<12.6} -> {value:.4}");
                solver.report(x, value);
            }
            match solver.outcome() {
                SolveOutcome::Converged { value, metric } => {
                    println!("solved: {lever} = {value:.6} gives {metric:.4} (target {target})");
                    Ok(ExitCode::SUCCESS)
                }
                SolveOutcome::OutOfRange { value, metric } => {
                    println!(
                        "the target {target} is not reached between {lo} and {hi}; closest is \
                         {lever} = {value} with {metric:.4}"
                    );
                    Ok(ExitCode::from(1))
                }
                SolveOutcome::Pending => Err("the solver did not finish".into()),
            }
        }
        Command::Sweep {
            file,
            x_path,
            x_values,
            y_path,
            y_values,
            metric,
            paths,
            threads,
        } => {
            let profile = load(&file)?;
            let metric = parse_metric(&metric)?;
            let pool = pool(threads)?;
            let sweep = GridSweep {
                x: GridAxis {
                    label: x_path.clone(),
                    path: x_path,
                    values: x_values,
                },
                y: y_path.map(|path| GridAxis {
                    label: path.clone(),
                    path,
                    values: y_values,
                }),
                metric,
            };
            let width = sweep.x.values.len();
            let mut row = Vec::new();
            print!("{:>14}", "");
            for x in &sweep.x.values {
                print!("{x:>12}");
            }
            println!();
            for cell in sweep.cells() {
                let changed =
                    patch::apply_to_profile(&profile, &cell.patch).map_err(|e| e.to_string())?;
                let plan = compile(&changed)?;
                let acc = simulate(&plan, plan.seed(), u64::from(paths), &pool);
                row.push(metric.read(&plan.summarize(&acc)));
                if row.len() == width {
                    let label = sweep
                        .y
                        .as_ref()
                        .map_or(String::new(), |y| y.values[cell.iy].to_string());
                    print!("{label:>14}");
                    for v in row.drain(..) {
                        print!("{v:>12.4}");
                    }
                    println!();
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Trace {
            file,
            path,
            quantile,
            by,
            paths,
            seed,
            json,
        } => {
            let profile = load(&file)?;
            let plan = compile(&profile)?;
            let seed = seed.unwrap_or(plan.seed());
            let index = match (path, quantile) {
                (Some(i), _) => i,
                (None, Some(q)) => {
                    let scalar = PathScalar::from_key(&by)
                        .ok_or_else(|| format!("unknown scalar {by:?}"))?;
                    let acc = simulate(&plan, seed, u64::from(paths), &pool(None)?);
                    Summary::path_at_quantile(&acc, scalar, q)
                        .ok_or_else(|| format!("no path has a value for {by}"))?
                }
                (None, None) => 0,
            };
            let trace = sim::trace_path(&plan, seed, index);
            if json {
                println!(
                    "{}",
                    serde_json::to_string(&trace).map_err(|e| e.to_string())?
                );
            } else {
                print_trace(&trace);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Preview { file } => {
            let profile = load(&file)?;
            let plan = compile(&profile)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&plan.preview()).map_err(|e| e.to_string())?
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Bench { file, paths } => {
            let profile = match file {
                Some(f) => load(&f)?,
                None => Profile::example(),
            };
            bench(&profile, paths)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn today_is_a_plausible_month() {
        let now = today();
        assert!(now >= YearMonth::ym(2025, 1) && now < YearMonth::ym(2200, 1));
    }

    #[test]
    fn parallel_runs_do_not_depend_on_the_thread_count() {
        let plan = compile(&Profile::example()).unwrap();
        let one = simulate(&plan, 7, 600, &pool(Some(1)).unwrap());
        let four = simulate(&plan, 7, 600, &pool(Some(4)).unwrap());
        assert_eq!(one.to_bytes(), four.to_bytes());
        assert_eq!(one.paths, 600);
    }

    #[test]
    fn formatting() {
        assert_eq!(dollars(1_234_567.4), "$1,234,567");
        assert_eq!(dollars(-950.0), "-$950");
        assert_eq!(age(f64::NAN), "never");
        assert!(parse_metric("success_rate").is_ok());
        assert!(parse_metric("nope").is_err());
    }
}
