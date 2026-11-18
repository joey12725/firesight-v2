//! `firesight`: the native command-line runner.
//!
//! Implemented: `schema`, `validate`, `migrate`, `examples`. The simulation commands
//! (`run`, `compare`, `solve`, `trace`, `bench`) parse their arguments and exit with
//! status 2 until `firesight_core::sim` is implemented; they will fan paths out over
//! rayon in chunks and merge the accumulators.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use firesight_core::legacy;
use firesight_core::model::{Profile, YearMonth, EXAMPLE_START};
use firesight_core::validate::{validate, Issue};
// The simulation commands will spread path chunks over rayon; referenced here so the
// dependency is part of the build graph before they are implemented.
use rayon as _;

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
    /// Run a simulation and print the summary. (Not implemented yet.)
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
    },
    /// Compare scenarios (patch files) against a base profile. (Not implemented yet.)
    Compare {
        /// Base profile JSON file.
        file: PathBuf,
        /// Scenario files, each `{ "name": ..., "patch": [...] }`.
        scenarios: Vec<PathBuf>,
    },
    /// Find the value of a field at which a metric hits a target. (Not implemented yet.)
    Solve {
        /// Profile JSON file.
        file: PathBuf,
        /// Patch path of the numeric field to vary.
        #[arg(long)]
        path: String,
        /// Metric to drive, for example success_rate.
        #[arg(long, default_value = "success_rate")]
        metric: String,
        /// Target value of the metric.
        #[arg(long)]
        target: f64,
        /// Lower bound of the search.
        #[arg(long)]
        lo: f64,
        /// Upper bound of the search.
        #[arg(long)]
        hi: f64,
    },
    /// Print the full monthly ledger of one path. (Not implemented yet.)
    Trace {
        /// Profile JSON file.
        file: PathBuf,
        /// Path index.
        #[arg(long, default_value_t = 0)]
        path: u64,
    },
    /// Measure paths per second. (Not implemented yet.)
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

fn not_implemented(command: &str) -> Result<ExitCode, String> {
    eprintln!(
        "firesight {command}: not implemented yet (the simulation core is a stub in this build)"
    );
    Ok(ExitCode::from(2))
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
        Command::Run { .. } => not_implemented("run"),
        Command::Compare { .. } => not_implemented("compare"),
        Command::Solve { .. } => not_implemented("solve"),
        Command::Trace { .. } => not_implemented("trace"),
        Command::Bench { .. } => not_implemented("bench"),
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
}
