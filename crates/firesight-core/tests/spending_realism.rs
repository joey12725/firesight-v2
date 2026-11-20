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

// ----------------------------------------------------------------------------------
// Monthly capture, shared by the features below
// ----------------------------------------------------------------------------------

/// Every month's value of chosen series and the event log of one or more paths.
#[derive(Default)]
struct Monthly {
    series: Vec<YearSeries>,
    /// `(calendar month 1..=12, values)` for every month of every path.
    months: Vec<(u32, Vec<f64>)>,
    events: Vec<firesight_core::stats::TraceEvent>,
}

impl Monthly {
    fn new(series: &[YearSeries]) -> Monthly {
        Monthly {
            series: series.to_vec(),
            ..Monthly::default()
        }
    }
}

impl Recorder for Monthly {
    fn begin_path(&mut self, _: u64) {}
    fn record_year(&mut self, _: usize, _: &YearRecord) {}
    fn end_path(&mut self, _: &PathEnd) {}
    fn wants_detail(&self) -> bool {
        true
    }
    fn record_month(&mut self, m: &firesight_core::stats::MonthRecord) {
        let v = self.series.iter().map(|s| m.values[s.index()]).collect();
        self.months.push((m.month.month(), v));
    }
    fn record_event(&mut self, e: firesight_core::stats::TraceEvent) {
        self.events.push(e);
    }
}

fn monthly(plan: &Plan, seed: u64, path: u64, series: &[YearSeries]) -> Monthly {
    let mut ws = Workspace::new(plan);
    let mut rec = Monthly::new(series);
    sim::run_path(plan, seed, path, &mut ws, &mut rec);
    rec
}

// ----------------------------------------------------------------------------------
// F02 seasonality
// ----------------------------------------------------------------------------------

fn seasonal_base() -> Profile {
    let mut p = retiree_base();
    p.spending.items = vec![
        item("utilities", Category::Utilities, 500.0, true),
        item("giving", Category::Giving, 200.0, false),
        item("rent_like", Category::Insurance, 1_000.0, true),
    ];
    p
}

#[test]
fn typical_seasonality_follows_the_weights_and_keeps_the_annual_total() {
    let mut on = seasonal_base();
    on.spending.seasonality = Seasonality::Typical;
    let off = seasonal_base();
    let series = [YearSeries::SpendEssential, YearSeries::SpendDiscretionary];
    let a = monthly(&compile(&off), 4, 0, &series);
    let b = monthly(&compile(&on), 4, 0, &series);
    assert_eq!(a.months.len(), b.months.len());

    // Off: flat. Essential = utilities 500 + insurance 1,000; discretionary = giving 200.
    for (_, v) in &a.months {
        assert!((v[0] - 1_500.0).abs() < 1e-6 && (v[1] - 200.0).abs() < 1e-6);
    }
    // On, by hand. The utilities weights sum to 11.85 (mean 0.9875), so January is
    // 500 x 1.25 / 0.9875 = 632.911; insurance is flat at 1,000. Giving: eleven months at
    // 0.76 and December at 3.6 (mean 0.996667): January 200 x 0.76 / 0.996667 = 152.508,
    // December 200 x 3.6 / 0.996667 = 722.408.
    let jan = b.months.iter().find(|(m, _)| *m == 1).unwrap();
    assert!(
        (jan.1[0] - (500.0 * 1.25 / 0.9875 + 1_000.0)).abs() < 1e-6,
        "{:?}",
        jan
    );
    assert!(
        (jan.1[1] - 200.0 * 0.76 / (11.96 / 12.0)).abs() < 1e-6,
        "{:?}",
        jan
    );
    let dec = b.months.iter().find(|(m, _)| *m == 12).unwrap();
    assert!(
        (dec.1[1] - 200.0 * 3.6 / (11.96 / 12.0)).abs() < 1e-6,
        "{:?}",
        dec
    );
    // A full calendar year (the plan starts in November, so 2027) adds to the same total.
    let year = |m: &Monthly, k: usize| -> f64 {
        m.months.iter().skip(2).take(12).map(|(_, v)| v[k]).sum()
    };
    for k in 0..2 {
        assert!((year(&a, k) - year(&b, k)).abs() < 1e-6, "series {k}");
    }
    // And the shape is really seasonal: December discretionary is 4.7 times January's.
    assert!(dec.1[1] / jan.1[1] > 4.5);
}

#[test]
fn seasonality_changes_timing_not_wealth_much() {
    let off = seasonal_base();
    let mut on = off.clone();
    on.spending.seasonality = Seasonality::Typical;
    let a = capture(&compile(&off), 8, 200, &[], false);
    let b = capture(&compile(&on), 8, 200, &[], false);
    assert_ne!(a.terminal, b.terminal, "seasonality had no effect at all");
    // The year's spending is the same, so terminal wealth moves only through the
    // timing of withdrawals: well under 1% of a roughly multi-million terminal sum.
    let rel = mean(
        a.terminal
            .iter()
            .zip(&b.terminal)
            .map(|(x, y)| ((x - y) / x).abs()),
    );
    assert!(rel < 0.01, "mean relative difference {rel}");
}

// ----------------------------------------------------------------------------------
// F03 category inflation
// ----------------------------------------------------------------------------------

fn utilities_base() -> Profile {
    let mut p = retiree_base();
    p.spending.items = vec![
        item("utilities", Category::Utilities, 2_000.0, true),
        item("other", Category::Other, 1_000.0, true),
    ];
    p
}

#[test]
fn the_wedge_starts_at_one_changes_each_january_and_only_touches_its_categories() {
    let mut on = utilities_base();
    on.spending.category_inflation = CategoryInflation::Stochastic;
    let plan = compile(&on);
    let m = monthly(&plan, 6, 0, &[YearSeries::SpendEssential]);
    // The plan starts in November 2026: the first two months are priced at today's
    // prices exactly.
    assert!((m.months[0].1[0] - 3_000.0).abs() < 1e-9);
    assert!((m.months[1].1[0] - 3_000.0).abs() < 1e-9);
    // Each January the multiplier changes and then holds for the year.
    let jan27 = m.months[2].1[0];
    assert!(
        (jan27 - 3_000.0).abs() > 1e-3,
        "no wedge in January: {jan27}"
    );
    for (k, month) in m.months.iter().enumerate().take(14).skip(2) {
        assert!((month.1[0] - jan27).abs() < 1e-9, "month {k}");
    }
    assert!((m.months[14].1[0] - jan27).abs() > 1e-3);
    // Only the utilities line carries it: "other" is always 1,000 of the total. The
    // wedge of January 2027 is therefore (total - 1000) / 2000, a small deviation.
    let w = (jan27 - 1_000.0) / 2_000.0;
    assert!((0.8..1.25).contains(&w), "{w}");
}

#[test]
fn the_wedge_adds_dispersion_not_drift() {
    let base = utilities_base();
    let mut on = base.clone();
    on.spending.category_inflation = CategoryInflation::Stochastic;
    let series = [YearSeries::SpendEssential];
    let a = capture(&compile(&base), 12, 3_000, &series, false);
    let b = capture(&compile(&on), 12, 3_000, &series, false);
    // Year 10 (2036), a full calendar year of 12 months at the budget's 36,000 real.
    let year = 10;
    let off: Vec<f64> = a.years.iter().map(|p| p[year][0]).collect();
    let wedge: Vec<f64> = b.years.iter().map(|p| p[year][0]).collect();
    assert!(off.iter().all(|x| (x - 36_000.0).abs() < 1e-6));
    let m = mean(wedge.iter().copied());
    let sd = (mean(wedge.iter().map(|x| (x - m) * (x - m)))).sqrt();
    // Mean-preserving: the wedge multiplies the 24,000 of utilities only, so the
    // average stays 36,000 to within sampling error (sd about 1,700 / sqrt(3000)).
    assert!((m - 36_000.0).abs() < 200.0, "mean {m}");
    // Dispersion: utilities' log-wedge after ten years has a standard deviation of
    // about 7% (phi 0.923, sigma 0.0305), so the total's is about 24,000 x 0.07 = 1,700.
    assert!((1_100.0..2_400.0).contains(&sd), "sd {sd}");
}

// ----------------------------------------------------------------------------------
// F04 detailed shocks
// ----------------------------------------------------------------------------------

fn homeowner(value: f64, transport_monthly: f64) -> Profile {
    let mut p = retiree_base();
    p.spending.items = vec![
        item("living", Category::Other, 3_500.0, true),
        item("car", Category::Transport, transport_monthly, true),
    ];
    p.properties = vec![Property {
        id: "home".into(),
        name: "Home".into(),
        usage: PropertyUse::Primary,
        value,
        ..Property::default()
    }];
    // So rich that no path is ever forced to sell the home, which would end its repairs.
    p.accounts[0].balance = 40_000_000.0;
    p.accounts[0].cost_basis = Some(40_000_000.0);
    p.spending.shocks = ShockLevel::Detailed;
    p
}

/// Every unplanned-expense event of `paths` paths: `(path, month index, label, amount)`.
fn shock_events(plan: &Plan, paths: u64) -> Vec<(u64, i32, String, f64)> {
    let start = plan.start();
    let mut out = Vec::new();
    for i in 0..paths {
        let m = monthly(plan, 21, i, &[]);
        for e in m.events {
            if e.kind == firesight_core::stats::TraceEventKind::ExpenseShock {
                out.push((i, e.month - start, e.label, e.amount.unwrap()));
            }
        }
    }
    out
}

#[test]
fn home_repair_scales_with_the_home_and_car_repair_with_transport_spending() {
    let small = shock_events(&compile(&homeowner(300_000.0, 400.0)), 120);
    let big = shock_events(&compile(&homeowner(600_000.0, 800.0)), 120);
    // Same random numbers, same hazards: the same shocks at the same times, only larger.
    assert_eq!(small.len(), big.len());
    let mut homes = 0;
    let mut cars = 0;
    for (a, b) in small.iter().zip(&big) {
        assert_eq!((a.0, a.1, &a.2), (b.0, b.1, &b.2));
        match a.2.as_str() {
            // Twice the home, twice the repair: exactly (the home's real value path is
            // the same shape).
            "Home repair" => {
                homes += 1;
                assert!((b.3 / a.3 - 2.0).abs() < 1e-9, "{a:?} {b:?}");
            }
            // The transport line doubled; the car repair doubles with it.
            "Car repair" => {
                cars += 1;
                assert!((b.3 / a.3 - 2.0).abs() < 1e-9, "{a:?} {b:?}");
            }
            _ => assert!((b.3 / a.3 - 1.0).abs() < 1e-6 || (b.3 / a.3 - 2.0).abs() > 1e-3),
        }
    }
    assert!(homes > 20 && cars > 20, "{homes} {cars}");
}

#[test]
fn a_renter_has_no_home_repairs_and_shocks_are_typed() {
    let mut p = homeowner(300_000.0, 400.0);
    p.properties.clear();
    let events = shock_events(&compile(&p), 120);
    assert!(!events.is_empty());
    assert!(events.iter().all(|e| e.2 != "Home repair"));
    let labels: std::collections::BTreeSet<&str> = events.iter().map(|e| e.2.as_str()).collect();
    assert!(labels.contains("Car repair"));
    assert!(labels.contains("Medical-adjacent expense"));
    assert!(labels.contains("Family emergency"));
}

#[test]
fn shocks_arrive_at_the_calibrated_rate_and_cluster() {
    let plan = compile(&homeowner(400_000.0, 600.0));
    let paths = 300;
    let events = shock_events(&plan, paths);
    let years = plan.horizon_years() as usize;
    // Count per path and calendar year, using the years every path covers in full.
    let mut counts = vec![vec![0.0f64; years]; paths as usize];
    for e in &events {
        counts[e.0 as usize][((e.1 + 10) / 12) as usize] += 1.0;
    }
    let sample: Vec<f64> = counts
        .iter()
        .flat_map(|c| c[1..20].iter().copied())
        .collect();
    let m = mean(sample.iter().copied());
    let var = mean(sample.iter().map(|x| (x - m) * (x - m)));
    // 0.4 + 0.4 + 0.16 + 0.12 = 1.08 shocks a year in total (the clustering keeps the
    // long-run rate: the base hazard is scaled by 1 - n).
    assert!((m - 1.08).abs() < 0.12, "rate {m}");
    // Poisson counts would have variance equal to the mean; clustering makes them
    // overdispersed. Measured 1.19 here (Hawkes branching 0.3, one-year half-life,
    // counted over calendar years: a window shorter than the memory shows only part
    // of the long-run factor 1 / (1 - 0.3)^2 = 2.04); the threshold is ten standard
    // errors above Poisson.
    assert!(var / m > 1.1, "variance/mean {}", var / m);
}

#[test]
fn detailed_shocks_are_mean_neutral_but_lumpy() {
    let series = [
        YearSeries::SpendEssential,
        YearSeries::SpendDiscretionary,
        YearSeries::SpendOneOff,
    ];
    // A renter: home repairs are deliberately not netted out of the budget.
    let mut on = homeowner(400_000.0, 600.0);
    on.properties.clear();
    let mut off = on.clone();
    off.spending.shocks = ShockLevel::Off;
    let total = |c: &Capture| -> f64 {
        mean(
            c.years
                .iter()
                .map(|p| p.iter().map(|y| y.iter().sum::<f64>()).sum::<f64>()),
        )
    };
    let a = capture(&compile(&off), 30, 400, &series, false);
    let b = capture(&compile(&on), 30, 400, &series, false);
    // The expected cost is netted out of the budget: lifetime spending is the same on
    // average (measured +0.1%; allow one percent for sampling and for the offset being
    // taken at the starting budget).
    let (ta, tb) = (total(&a), total(&b));
    assert!(((tb - ta) / ta).abs() < 0.01, "off {ta}, detailed {tb}");
    // But the spending is lumpier: the worst path's one-off spending is several times
    // the typical path's.
    let one_off = |c: &Capture| -> Vec<f64> {
        c.years
            .iter()
            .map(|p| p.iter().map(|y| y[2]).sum::<f64>())
            .collect()
    };
    assert!(one_off(&a).iter().all(|x| *x == 0.0));
    let ob = one_off(&b);
    let max = ob.iter().cloned().fold(0.0, f64::max);
    assert!(max > 3.0 * mean(ob.iter().copied()), "max {max}");
}

#[test]
fn detailed_shocks_barely_move_success_because_they_are_mean_neutral() {
    // A retiree spending 5% of a $1m portfolio for 30 years. The shocks add variance
    // but, being netted out of the budget, no level, and when savings run out the
    // engine cuts one-off spending before essentials: so success moves by well under
    // a point either way (measured +0.2 points, inside the noise of 4,000 paths).
    let mut off = retiree(1_000_000.0, 0.6, 50_000.0, 30.0);
    off.simulation.seed = 5;
    let mut on = off.clone();
    on.spending.shocks = ShockLevel::Detailed;
    let s_off = summary(&off, 5, 4_000).headline.success_rate.value;
    let s_on = summary(&on, 5, 4_000).headline.success_rate.value;
    assert!((s_on - s_off).abs() < 0.01, "off {s_off}, detailed {s_on}");
    assert_ne!(s_on, s_off, "detailed shocks had no effect at all");
}

#[test]
fn home_repairs_cost_about_a_third_of_a_percent_of_the_home_a_year() {
    // lambda 0.4, median 0.5% of value, sigma 1.1: mean 0.5% x exp(0.605) = 0.915% per
    // repair, so 0.366% of value a year: about 1,465 on a 400,000 home (in today's
    // dollars; the home's real value drifts, hence the range).
    let plan = compile(&homeowner(400_000.0, 600.0));
    let paths = 150;
    let total: f64 = shock_events(&plan, paths)
        .iter()
        .filter(|e| e.2 == "Home repair")
        .map(|e| e.3)
        .sum();
    let per_year = total / (paths as f64 * 25.0);
    assert!((1_000.0..2_200.0).contains(&per_year), "{per_year}");
}

#[test]
fn detailed_shocks_conserve_money() {
    let plan = compile(&homeowner(400_000.0, 600.0));
    capture(&plan, 31, 100, &[], true);
}

// ----------------------------------------------------------------------------------
// F05 unemployment belt-tightening
// ----------------------------------------------------------------------------------

/// A 40-year-old earner in a layoff-prone job with a flat budget, half of it
/// discretionary, and a portfolio so that spending is never constrained.
fn worker_base() -> Profile {
    let mut p = quiet(vec![person("w", 40, Sex::Unspecified)], 80.0);
    p.simulation.seed = 17;
    p.assumptions.career_risk = true;
    let mut j = job("job", 0, 100_000.0);
    if let IncomeKind::Job(job) = &mut j.kind {
        job.risk = CareerRisk {
            level: RiskLevel::High,
        };
    }
    p.incomes = vec![j];
    // A fixed retirement age, so that being richer does not move the date.
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 65.0,
    };
    p.spending.items = vec![
        item("needs", Category::Insurance, 2_000.0, true),
        item("wants", Category::Dining, 2_000.0, false),
    ];
    p.accounts = vec![Account {
        cost_basis: Some(300_000.0),
        ..account("portfolio", 0, AccountKind::Taxable, 300_000.0)
    }];
    p
}

#[test]
fn an_unemployment_cut_of_zero_is_bit_identical() {
    let a = worker_base();
    let mut b = a.clone();
    b.spending.unemployment_cut = 0.0;
    let x = capture(
        &compile(&a),
        3,
        100,
        &[YearSeries::SpendDiscretionary],
        false,
    );
    let y = capture(
        &compile(&b),
        3,
        100,
        &[YearSeries::SpendDiscretionary],
        false,
    );
    assert_eq!(x.years, y.years);
    assert_eq!(x.terminal, y.terminal);
}

#[test]
fn the_cut_phases_in_over_three_months_and_is_restored() {
    let mut on = worker_base();
    on.spending.unemployment_cut = 0.3;
    let plan = compile(&on);
    let series = [YearSeries::SpendDiscretionary];
    let mut checked = 0;
    for path in 0..300 {
        let m = monthly(&plan, 5, path, &series);
        let cuts: Vec<_> = m
            .events
            .iter()
            .filter(|e| e.kind == firesight_core::stats::TraceEventKind::SpendingCut)
            .collect();
        if cuts.is_empty() {
            continue;
        }
        let first = cuts[0].month - plan.start();
        // The event is logged in the first month of the cut, at a third of its size
        // (0.1), so that month's discretionary spending is 2,000 x 0.9 in real terms
        // times the price level; relative to the month before the layoff the drop is
        // 10%, then 20%, then 30%. Compare against the same path without the cut.
        let mut off = on.clone();
        off.spending.unemployment_cut = 0.0;
        let base = monthly(&compile(&off), 5, path, &series);
        let ratio = |i: usize| m.months[i].1[0] / base.months[i].1[0];
        let f = first as usize;
        assert!((ratio(f) - 0.9).abs() < 1e-9, "month 1 of cut {}", ratio(f));
        // Still out of work the next month: 20%; back at work: the cut unwinds by a
        // third of its size, which from 10% is all the way to 0.
        let next = ratio(f + 1);
        assert!(
            (next - 0.8).abs() < 1e-9 || (next - 1.0).abs() < 1e-9,
            "second month {next}"
        );
        let floor = (f..f + 60)
            .filter(|&i| i < m.months.len())
            .map(ratio)
            .fold(1.0f64, f64::min);
        assert!(floor >= 0.7 - 1e-9, "never deeper than 30%: {floor}");
        let restored = m.events.iter().any(|e| {
            e.kind == firesight_core::stats::TraceEventKind::SpendingRaise
                && e.label.starts_with("Back to work")
        });
        assert!(restored, "path {path} never restored spending");
        checked += 1;
        if checked >= 5 {
            break;
        }
    }
    assert!(
        checked >= 3,
        "too few layoffs in 300 paths of a high-risk job: {checked}"
    );
}

#[test]
fn the_cut_only_touches_discretionary_spending_and_raises_terminal_wealth() {
    let off = worker_base();
    let mut on = off.clone();
    on.spending.unemployment_cut = 0.3;
    let series = [YearSeries::SpendEssential, YearSeries::SpendDiscretionary];
    let a = capture(&compile(&off), 9, 300, &series, false);
    let b = capture(&compile(&on), 9, 300, &series, false);
    // Essential spending is untouched except where the baseline ran out of money (a
    // handful of path-years): there the cut leaves more to pay for it, never less.
    let (mut same, mut all) = (0usize, 0usize);
    for (pa, pb) in a.years.iter().zip(&b.years) {
        for (ya, yb) in pa.iter().zip(pb) {
            all += 1;
            assert!(
                yb[0] >= ya[0] - 1e-6,
                "essential spending fell with the cut"
            );
            if (ya[0] - yb[0]).abs() <= 1e-6 {
                same += 1;
            }
        }
    }
    assert!(same as f64 > 0.99 * all as f64, "{same} of {all} unchanged");
    let total = |c: &Capture, k: usize| -> f64 {
        c.years
            .iter()
            .flat_map(|p| p.iter().map(|y| y[k]))
            .sum::<f64>()
    };
    let drop = 1.0 - total(&b, 1) / total(&a, 1);
    // A month-long spell costs a few percent of one year of discretionary spending; over
    // 40 years at a high layoff rate the total falls by a small but visible amount.
    assert!(
        (0.002..0.1).contains(&drop),
        "discretionary spending fell {drop}"
    );
    // Spending less keeps more wealth on every path, paired.
    let better = a
        .terminal
        .iter()
        .zip(&b.terminal)
        .filter(|(x, y)| y >= x)
        .count();
    assert_eq!(better, a.terminal.len(), "some path ended poorer");
    assert!(mean(b.terminal.iter().copied()) > mean(a.terminal.iter().copied()));
}

#[test]
fn the_unemployment_cut_conserves_money() {
    let mut on = worker_base();
    on.spending.unemployment_cut = 0.3;
    capture(&compile(&on), 12, 100, &[], true);
}
