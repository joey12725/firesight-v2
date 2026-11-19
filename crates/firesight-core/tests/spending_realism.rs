//! The opt-in spending realism features (`spending.vehicles`, `seasonality`,
//! `category_inflation`, `shocks = "detailed"`, `unemployment_cut`): each is off by
//! default and identical to the baseline when off, conserves money when on, and moves
//! outcomes in the direction and by the amount its mechanism implies.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim::{self, Plan, Workspace};
use firesight_core::stats::{PathEnd, Recorder, YearRecord, YearSeries};

/// Keeps the yearly values of chosen series and the terminal scalars of every path.
#[derive(Default)]
struct Capture {
    series: Vec<YearSeries>,
    /// `years[path][year][series]`
    years: Vec<Vec<Vec<f64>>>,
    terminal: Vec<f64>,
    last_net_worth: f64,
}

impl Capture {
    fn new(series: &[YearSeries]) -> Capture {
        Capture {
            series: series.to_vec(),
            ..Capture::default()
        }
    }
}

impl Recorder for Capture {
    fn begin_path(&mut self, _: u64) {
        self.years.push(Vec::new());
    }
    fn record_year(&mut self, _: usize, r: &YearRecord) {
        self.last_net_worth = r.net_worth();
        let row = self.series.iter().map(|s| r.get(*s)).collect();
        self.years.last_mut().unwrap().push(row);
    }
    fn end_path(&mut self, _: &PathEnd) {
        self.terminal.push(self.last_net_worth);
    }
}

fn capture(plan: &Plan, seed: u64, paths: u64, series: &[YearSeries], check: bool) -> Capture {
    let mut ws = Workspace::new(plan);
    ws.set_check(check);
    let mut cap = Capture::new(series);
    for i in 0..paths {
        sim::run_path(plan, seed, i, &mut ws, &mut cap);
    }
    cap
}

/// A 65-year-old retiree with a large portfolio and a flat budget, so that the
/// feature under test is the only thing that moves.
fn retiree_base() -> Profile {
    let mut p = retiree(2_000_000.0, 0.6, 60_000.0, 25.0);
    p.simulation.seed = 11;
    p
}

fn mean(v: impl Iterator<Item = f64>) -> f64 {
    let (n, s) = v.fold((0.0, 0.0), |(n, s), x| (n + 1.0, s + x));
    s / n
}

// ----------------------------------------------------------------------------------
// F01 vehicles
// ----------------------------------------------------------------------------------

fn car(price: f64, cycle: f64) -> Vehicle {
    Vehicle {
        id: "car".into(),
        name: "Car".into(),
        price,
        replace_every_years: cycle,
        age_years: 0.0,
        trade_in_fraction: 0.25,
        ..Vehicle::default()
    }
}

#[test]
fn a_free_vehicle_leaves_every_path_bit_identical() {
    let base = retiree_base();
    let mut with = base.clone();
    with.spending.vehicles = vec![car(0.0, 10.0)];
    let a = capture(&compile(&base), 3, 60, &[YearSeries::SpendOneOff], false);
    let b = capture(&compile(&with), 3, 60, &[YearSeries::SpendOneOff], false);
    assert_eq!(a.terminal, b.terminal);
    assert_eq!(a.years, b.years);
}

#[test]
fn replacement_is_lumpy_priced_in_todays_dollars_and_jittered() {
    let mut p = retiree_base();
    p.spending.vehicles = vec![car(40_000.0, 10.0)];
    let plan = compile(&p);
    let cap = capture(&plan, 5, 300, &[YearSeries::SpendOneOff], true);
    let mut first_years = std::collections::BTreeSet::new();
    for path in &cap.years {
        let spikes: Vec<(usize, f64)> = path
            .iter()
            .enumerate()
            .filter(|(_, y)| y[0] > 1.0)
            .map(|(i, y)| (i, y[0]))
            .collect();
        // 40,000 less a 25% trade-in, in today's dollars, whatever the price level was
        // when it was paid. Over a 25-year life there are two or three replacements.
        assert!((2..=3).contains(&spikes.len()), "{spikes:?}");
        for (_, amount) in &spikes {
            assert!((amount - 30_000.0).abs() < 1e-6, "{amount}");
        }
        // The first is ten years out, give or take the jitter (at most two sd = 12
        // months); the plan starts in November 2026.
        assert!((9..=11).contains(&spikes[0].0), "{spikes:?}");
        first_years.insert(spikes[0].0);
    }
    assert!(first_years.len() >= 2, "no jitter: {first_years:?}");
}

#[test]
fn an_old_vehicle_is_replaced_early_and_an_end_date_stops_replacement() {
    let mut p = retiree_base();
    let mut v = car(40_000.0, 10.0);
    v.age_years = 9.5;
    p.spending.vehicles = vec![v.clone()];
    let cap = capture(&compile(&p), 5, 50, &[YearSeries::SpendOneOff], false);
    for path in &cap.years {
        // Due in six months, plus or minus the jitter of up to a year.
        let first = path.iter().position(|y| y[0] > 1.0).unwrap();
        assert!(first <= 2, "{first}");
    }
    v.end = Some(When::Date { at: START + 5 * 12 });
    p.spending.vehicles = vec![v];
    let cap = capture(&compile(&p), 5, 50, &[YearSeries::SpendOneOff], false);
    for path in &cap.years {
        // The first replacement (month ~6) happens; the next would be ten years on, after
        // the end date.
        assert_eq!(path.iter().filter(|y| y[0] > 1.0).count(), 1);
    }
}

#[test]
fn a_financed_vehicle_becomes_an_auto_debt_and_money_is_conserved() {
    let mut p = retiree_base();
    let mut v = car(40_000.0, 10.0);
    v.financing = Some(VehicleFinancing {
        down_fraction: 0.1,
        rate: 0.06,
        term_months: 60,
    });
    p.spending.vehicles = vec![v];
    let plan = compile(&p);
    let cap = capture(
        &plan,
        5,
        100,
        &[YearSeries::SpendOneOff, YearSeries::SpendDebt],
        true,
    );
    for path in &cap.years {
        let spike = path.iter().find(|y| y[0] > 1.0).unwrap();
        // 10% of the 30,000 net price is paid at once; the rest is borrowed.
        assert!((spike[0] - 3_000.0).abs() < 1e-6, "{}", spike[0]);
        // The loan is repaid over the next five years: 27,000 at 6% for 60 months is a
        // payment of 522.01, so about 6,264 a year in nominal dollars.
        let paid: f64 = path.iter().map(|y| y[1]).sum();
        assert!(paid > 27_000.0 * 0.85, "{paid}");
    }
}

#[test]
fn vehicles_lower_terminal_wealth_by_about_what_they_cost() {
    let base = retiree_base();
    let mut with = base.clone();
    with.spending.vehicles = vec![car(40_000.0, 10.0)];
    let a = capture(&compile(&base), 9, 300, &[], false);
    let b = capture(&compile(&with), 9, 300, &[], false);
    // Paired paths: same markets, same lifespans, only the vehicle differs.
    let diffs: Vec<f64> = a
        .terminal
        .iter()
        .zip(&b.terminal)
        .map(|(x, y)| x - y)
        .collect();
    assert!(diffs.iter().all(|d| *d > 0.0), "some path is no worse off");
    // Two or three replacements of 30,000 (today's dollars) with the investment growth
    // forgone: roughly 60,000 to 200,000 in today's dollars by the end.
    let m = mean(diffs.iter().copied());
    assert!((55_000.0..250_000.0).contains(&m), "mean cost {m}");
}
