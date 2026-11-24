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

#[test]
fn the_funded_test_counts_the_average_cost_of_replacing_vehicles() {
    // A vehicle is a cost the retirement has to carry, like a home's running costs:
    // 40,000 less a 25% trade-in every ten years is 3,000 a year of need, which at
    // the 4% rule is 75,000 of savings before the tax on withdrawing it.
    let base = retiree_base();
    let mut with = base.clone();
    with.spending.vehicles = vec![car(40_000.0, 10.0)];
    let (a, b) = (compile(&base).preview(), compile(&with).preview());
    assert!(
        (b.retirement_need - a.retirement_need - 3_000.0).abs() < 1e-6,
        "{} {}",
        a.retirement_need,
        b.retirement_need
    );
    let extra = b.funded_requirement - a.funded_requirement;
    assert!((75_000.0 - 1e-6..100_000.0).contains(&extra), "{extra}");
    // A vehicle that is no longer replaced costs the retirement nothing.
    with.spending.vehicles[0].end = Some(When::Date { at: START });
    let c = compile(&with).preview();
    assert_eq!(c.retirement_need, a.retirement_need);
    assert_eq!(c.funded_requirement, a.funded_requirement);
}

#[test]
fn a_household_that_retires_when_funded_works_longer_to_pay_for_its_cars() {
    // The shipped family retires when the funded test is met. Two cars costing
    // 38,000 x 0.75 / 10 + 32,000 x 0.75 / 9 = 5,517 a year raise the requirement by
    // about 138,000 plus tax, which this household saves in roughly a year, and the
    // purchases made before retiring delay it further: 1.75 years measured. Before
    // the funded test counted the cars the delay was 0.9 years (the purchases alone)
    // and the household retired unable to afford the next ones.
    let family = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == "dual-income-family")
        .unwrap()
        .profile;
    let mut with = family.clone();
    let mut second = car(32_000.0, 9.0);
    second.id = "second".into();
    second.age_years = 6.0;
    let mut first = car(38_000.0, 10.0);
    first.age_years = 4.0;
    with.spending.vehicles = vec![first, second];
    let a = summary(&family, 1, 1_000).headline.median_retirement_age;
    let b = summary(&with, 1, 1_000).headline.median_retirement_age;
    assert!((1.25..2.5).contains(&(b - a)), "without {a}, with {b}");
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
    /// Price level of every month.
    prices: Vec<f64>,
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
        self.prices.push(m.price_level);
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
    // On, by hand. The utilities weights (Federal Reserve G.17, January 1.16) sum to
    // 11.999 (mean 0.999917), so January is 500 x 1.16 / 0.999917 = 580.048; insurance
    // is flat at 1,000. Giving: eleven months at 0.76 and December at 3.6 (mean
    // 0.996667): January 200 x 0.76 / 0.996667 = 152.508, December
    // 200 x 3.6 / 0.996667 = 722.408.
    let jan = b.months.iter().find(|(m, _)| *m == 1).unwrap();
    assert!(
        (jan.1[0] - (500.0 * 1.16 / (11.999 / 12.0) + 1_000.0)).abs() < 1e-6,
        "{:?}",
        jan
    );
    assert!((jan.1[0] - 1_580.048).abs() < 1e-3, "{:?}", jan);
    // April is the trough of the energy year: 500 x 0.866 / 0.999917 = 433.036.
    let apr = b.months.iter().find(|(m, _)| *m == 4).unwrap();
    assert!((apr.1[0] - 1_433.036).abs() < 1e-3, "{:?}", apr);
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
    let a = capture(&compile(&off), 30, 1_500, &series, false);
    let b = capture(&compile(&on), 30, 1_500, &series, false);
    // The expected cost is netted out of the budget. By hand, a year: car repairs
    // 0.4 x 8% x exp(0.405) = 4.8% of the 7,200 transport line = 346; medical
    // 0.16 x 2% x exp(0.5) = 0.53% and family 0.12 x 3% x exp(0.72) = 0.74% of the
    // 49,200 budget = 624. So the budget falls by 970 a year, 24,250 over 25 years.
    let lifetime = |c: &Capture, k: usize| -> f64 {
        mean(c.years.iter().map(|p| p.iter().map(|y| y[k]).sum::<f64>()))
    };
    let netted = lifetime(&a, 0) + lifetime(&a, 1) - lifetime(&b, 0) - lifetime(&b, 1);
    assert!((netted - 24_250.0).abs() < 250.0, "netted {netted}");
    // And the shocks that are drawn cost that much on average: the ratio is 0.97 by
    // construction (medical and family shocks are sized from the budget after
    // netting, 2% lower, and the cap on the draw removes about 1%), with a sampling
    // error of about 2% here. A renter's shocks used to come out 14% short because
    // the missing home repairs did not feed the clustering.
    let drawn = lifetime(&b, 2);
    let ratio = drawn / netted;
    assert!(
        (0.91..1.04).contains(&ratio),
        "drawn {drawn}, netted {netted}"
    );
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
    // a point either way (measured 0.0 points on 4,000 paths: 22 paths change sides,
    // eleven each way).
    let mut off = retiree(1_000_000.0, 0.6, 50_000.0, 30.0);
    off.simulation.seed = 5;
    let mut on = off.clone();
    on.spending.shocks = ShockLevel::Detailed;
    let s_off = summary(&off, 5, 4_000).headline.success_rate.value;
    let s_on = summary(&on, 5, 4_000).headline.success_rate.value;
    assert!((s_on - s_off).abs() < 0.01, "off {s_off}, detailed {s_on}");
    // The rate can come out equal while the paths behind it do not: on paired paths
    // the shocks must have moved the wealth left at the end nearly everywhere.
    let a = capture(&compile(&off), 5, 500, &[], false);
    let b = capture(&compile(&on), 5, 500, &[], false);
    let moved = a
        .terminal
        .iter()
        .zip(&b.terminal)
        .filter(|(x, y)| x != y)
        .count();
    assert!(
        moved > 350,
        "detailed shocks moved only {moved} of 500 paths"
    );
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
fn a_homeowners_repairs_come_out_of_the_maintenance_rate() {
    // Home repairs are part of what the 1% maintenance rule of thumb pays for, so
    // their expected cost (0.4 x 0.5% x exp(0.605) = 0.366% of value a year) is taken
    // out of the home's maintenance_rate: the owner pays the same on average, in lumps.
    let series = [
        YearSeries::SpendEssential,
        YearSeries::SpendDiscretionary,
        YearSeries::SpendHousing,
        YearSeries::SpendOneOff,
    ];
    let on = homeowner(400_000.0, 600.0);
    let mut off = on.clone();
    off.spending.shocks = ShockLevel::Off;
    let a = capture(&compile(&off), 30, 1_500, &series, false);
    let b = capture(&compile(&on), 30, 1_500, &series, false);
    let lifetime = |c: &Capture, k: usize| -> f64 {
        mean(c.years.iter().map(|p| p.iter().map(|y| y[k]).sum::<f64>()))
    };
    // Housing costs fall by 0.366% of the home's value a year: about 1,465 on 400,000,
    // 36,600 over 25 years (the home's real value drifts, hence the range).
    let housing = lifetime(&a, 2) - lifetime(&b, 2);
    assert!((30_000.0..45_000.0).contains(&housing), "{housing}");
    // Everything planned that was given up, against the shocks drawn instead.
    let netted = housing + lifetime(&a, 0) + lifetime(&a, 1) - lifetime(&b, 0) - lifetime(&b, 1);
    let drawn = lifetime(&b, 3);
    let ratio = drawn / netted;
    assert!(
        (0.91..1.05).contains(&ratio),
        "drawn {drawn}, netted {netted}"
    );
    // A maintenance rate too low to absorb the repairs is floored at zero, not negative.
    let mut low = on.clone();
    low.properties[0].maintenance_rate = 0.001;
    low.properties[0].property_tax_rate = Some(0.0);
    low.properties[0].insurance_rate = 0.0;
    let c = capture(&compile(&low), 30, 50, &[YearSeries::SpendHousing], false);
    assert!(c.years.iter().flatten().all(|y| y[0].abs() < 1e-9));
}

#[test]
fn the_retirement_date_still_counts_the_repairs_carved_out_of_maintenance() {
    // The shipped family buys a home and retires when it can afford to. Detailed
    // shocks move 0.366% of the home's value a year from the maintenance paid every
    // month to repairs drawn at random; the funded test must go on counting that as a
    // cost of the home. When it did not, the household looked richer than it was and
    // the median retirement came two months earlier (53.25 against 53.42), which cost
    // 1.8 points of success. The typical level plans with the same home costs and
    // nets about as much out of the budget, so the two median dates must agree to
    // within a month.
    let family = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == "dual-income-family")
        .unwrap()
        .profile;
    let median_age = |level: ShockLevel| -> f64 {
        let mut p = family.clone();
        p.spending.shocks = level;
        summary(&p, 1, 1_000).headline.median_retirement_age
    };
    let (typical, detailed) = (
        median_age(ShockLevel::Typical),
        median_age(ShockLevel::Detailed),
    );
    assert!(
        (detailed - typical).abs() < 0.1,
        "typical {typical}, detailed {detailed}"
    );
}

#[test]
fn shocks_keep_their_dates_and_frequency_whatever_the_household_owns() {
    let owner = shock_events(&compile(&homeowner(400_000.0, 600.0)), 300);
    let mut p = homeowner(400_000.0, 600.0);
    p.properties.clear();
    let renter = shock_events(&compile(&p), 300);
    // The renter has exactly the owner's shocks less the home repairs, to the month
    // and (for car repairs, which do not depend on the budget level) to the dollar.
    let rest: Vec<_> = owner.iter().filter(|e| e.2 != "Home repair").collect();
    assert_eq!(rest.len(), renter.len());
    for (a, b) in rest.iter().zip(&renter) {
        assert_eq!((a.0, a.1, &a.2), (b.0, b.1, &b.2));
        if a.2 == "Car repair" {
            assert!((a.3 - b.3).abs() < 1e-6, "{a:?} {b:?}");
        }
    }
    // 0.4 + 0.16 + 0.12 = 0.68 a year over 300 paths of 25 years: 5,100 expected,
    // with a standard error of about 100 (clustered counts).
    let rate = renter.len() as f64 / (300.0 * 25.0);
    assert!((rate - 0.68).abs() < 0.06, "rate {rate}");
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

// ----------------------------------------------------------------------------------
// All five together: random streams, conservation, finite values
// ----------------------------------------------------------------------------------

/// Switches every spending-realism feature on.
fn all_on(mut p: Profile) -> Profile {
    let mut financed = car(32_000.0, 9.0);
    financed.id = "second".into();
    financed.age_years = 6.0;
    financed.financing = Some(VehicleFinancing {
        down_fraction: 0.1,
        rate: 0.07,
        term_months: 60,
    });
    let mut cash = car(38_000.0, 10.0);
    cash.age_years = 4.0;
    p.spending.vehicles = vec![cash, financed];
    p.spending.seasonality = Seasonality::Typical;
    p.spending.category_inflation = CategoryInflation::Stochastic;
    p.spending.shocks = ShockLevel::Detailed;
    p.spending.unemployment_cut = 0.15;
    p
}

/// The shipped two-earner family with a fixed retirement age, so that nothing the
/// household can afford moves the date it stops earning.
fn family() -> Profile {
    let mut p = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == "dual-income-family")
        .unwrap()
        .profile;
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 62.0,
    };
    p
}

/// What the other random streams produce on a path: the price level of every month
/// (market), wages (career), the months long-term care begins (health; the amount paid
/// for care is not exogenous, it is cut on a path that has run out of money) and the
/// months lived (longevity).
#[derive(Default)]
struct Exogenous {
    prices: Vec<f64>,
    wages: Vec<f64>,
    care: Vec<i32>,
    events: Vec<(i32, String)>,
}

impl Recorder for Exogenous {
    fn begin_path(&mut self, _: u64) {}
    fn record_year(&mut self, _: usize, _: &YearRecord) {}
    fn end_path(&mut self, _: &PathEnd) {}
    fn wants_detail(&self) -> bool {
        true
    }
    fn record_month(&mut self, m: &firesight_core::stats::MonthRecord) {
        self.prices.push(m.price_level);
        self.wages.push(m.values[YearSeries::IncomeWages.index()]);
    }
    fn record_event(&mut self, e: firesight_core::stats::TraceEvent) {
        use firesight_core::stats::TraceEventKind as K;
        if e.kind == K::LongTermCare {
            self.care.push(e.month - START);
        }
        let vehicle = e.kind == K::MajorExpense && e.label.starts_with("Replace vehicle");
        if vehicle || e.kind == K::ExpenseShock {
            self.events.push((e.month - START, e.label));
        }
    }
}

fn exogenous(plan: &Plan, seed: u64, path: u64) -> Exogenous {
    let mut ws = Workspace::new(plan);
    let mut rec = Exogenous::default();
    sim::run_path(plan, seed, path, &mut ws, &mut rec);
    rec
}

#[test]
fn switching_the_features_on_leaves_every_other_random_stream_alone() {
    let off = compile(&family());
    let on = compile(&all_on(family()));
    let (mut wages, mut care) = (0.0, 0usize);
    for path in 0..60 {
        let a = exogenous(&off, 7, path);
        let b = exogenous(&on, 7, path);
        // Same lifetimes, same inflation, same pay (layoffs included), same care needs.
        assert_eq!(a.prices, b.prices, "path {path}: market or longevity");
        assert_eq!(a.wages, b.wages, "path {path}: career");
        assert_eq!(a.care, b.care, "path {path}: health");
        wages += a.wages.iter().sum::<f64>();
        care += a.care.len();
        // The baseline's own shocks (stream Shocks is still drawn, never used).
        assert!(a.events.iter().all(|e| e.1 == "Unplanned expense"));
        assert!(b.events.iter().all(|e| e.1 != "Unplanned expense"));
    }
    assert!(wages > 0.0 && care > 10, "nothing was compared");
}

#[test]
fn a_household_decision_does_not_move_the_features_random_draws() {
    let base = all_on(family());
    // Decisions that change wealth and the budget, but not what the household owns:
    // retire five years earlier with a different portfolio and a larger cushion.
    let mut other = base.clone();
    other.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 57.0,
    };
    for a in &mut other.accounts {
        a.balance *= 3.0;
        a.cost_basis = a.cost_basis.map(|b| b * 3.0);
    }
    let (pa, pb) = (compile(&base), compile(&other));
    let mut seen = 0;
    for path in 0..60 {
        let a = exogenous(&pa, 7, path);
        let b = exogenous(&pb, 7, path);
        // Every vehicle replacement and every shock, of the same kind in the same month.
        // Home repairs are left out: the draws behind them are the same, but they cost
        // money (and are logged) only while the household owns a home, and whether it
        // still does late in life depends on what it could afford.
        let keep = |e: &Exogenous| -> Vec<(i32, String)> {
            e.events
                .iter()
                .filter(|x| x.1 != "Home repair")
                .cloned()
                .collect()
        };
        let (ea, eb) = (keep(&a), keep(&b));
        assert_eq!(ea, eb, "path {path}");
        seen += ea.len();
    }
    assert!(seen > 60 * 20, "too few events to compare: {seen}");

    // The price wedge: with a flat all-essential budget the essential spending of each
    // month is the wedge itself, so it must not change with the size of the portfolio.
    let mut small = utilities_base();
    small.spending.category_inflation = CategoryInflation::Stochastic;
    let mut large = small.clone();
    large.accounts[0].balance *= 2.0;
    large.accounts[0].cost_basis = large.accounts[0].cost_basis.map(|b| b * 2.0);
    let series = [YearSeries::SpendEssential, YearSeries::BalanceTaxable];
    for path in 0..20 {
        let a = monthly(&compile(&small), 6, path, &series);
        let b = monthly(&compile(&large), 6, path, &series);
        assert_eq!(a.months.len(), b.months.len());
        for (x, y) in a.months.iter().zip(&b.months) {
            assert_eq!(x.1[0], y.1[0], "path {path}");
        }
        assert!(b.months[12].1[1] > 1.5 * a.months[12].1[1]);
    }
}

#[test]
fn every_feature_at_once_conserves_money_and_stays_finite_in_every_market_model() {
    let series: Vec<YearSeries> = YearSeries::ALL.to_vec();
    for example in Profile::examples(START) {
        for kind in MARKETS {
            let mut p = all_on(with_market(example.profile.clone(), kind));
            p.assumptions.career_risk = true;
            let plan = compile(&p);
            // `true`: the engine's own monthly conservation checks panic on a leak.
            let cap = capture(&plan, 13, 150, &series, true);
            assert_eq!(cap.terminal.len(), 150);
            assert!(cap.terminal.iter().all(|x| x.is_finite()));
            for y in cap.years.iter().flatten() {
                assert!(y.iter().all(|v| v.is_finite()), "{} {kind:?}", example.slug);
            }
            let one_off = series
                .iter()
                .position(|s| *s == YearSeries::SpendOneOff)
                .unwrap();
            let lumps: f64 = cap.years.iter().flatten().map(|y| y[one_off]).sum();
            assert!(
                lumps > 0.0,
                "{}: no vehicle or shock was paid",
                example.slug
            );
        }
    }
}

#[test]
fn a_second_car_loan_takes_back_the_payment_the_first_one_freed() {
    // A car financed over two years and replaced every four, with another debt and
    // the avalanche strategy: when the first loan is paid off its payment rolls into
    // the extra payment, and the next loan must take it back, or every cycle would add
    // another car payment to the extra debt payments for good.
    let mut p = retiree_base();
    let mut v = car(40_000.0, 4.0);
    v.age_years = 3.5;
    v.financing = Some(VehicleFinancing {
        down_fraction: 0.0,
        rate: 0.05,
        term_months: 24,
    });
    p.spending.vehicles = vec![v];
    p.debts = vec![Debt {
        id: "loan".into(),
        name: "Long loan".into(),
        balance: 600_000.0,
        rate: 0.03,
        payment: Some(2_000.0),
        ..Debt::default()
    }];
    p.accounts[0].balance = 10_000_000.0;
    p.accounts[0].cost_basis = Some(10_000_000.0);
    p.debt_policy.strategy = DebtStrategy::Avalanche;
    let plan = compile(&p);
    // The engine's own conservation checks, on every path.
    capture(&plan, 5, 20, &[YearSeries::SpendDebt], true);
    for path in 0..20 {
        let m = monthly(&plan, 5, path, &[YearSeries::SpendDebt]);
        // Loan payments are fixed in nominal dollars. A car loan is 30,000 of today's
        // dollars at 5% over 24 months: 1,316.14 a month times the price level of the
        // month it was taken out. So in a typical month debt service is at most the
        // long loan's 2,000 plus one car payment (while no car loan runs, the freed
        // payment goes to the long loan instead: the same sum). Months 120 to 180 are
        // past two or three payoffs: without the fix every one of them left its
        // payment in the extra amount for good, and the median month would be 2,000
        // plus two or three car payments (measured without it, first path: 6,331). The median ignores the month of a payoff,
        // which pays the last instalment and the freed amount together.
        let peak = m.prices[..180].iter().cloned().fold(0.0, f64::max);
        let mut nominal: Vec<f64> = (120..180).map(|i| m.months[i].1[0] * m.prices[i]).collect();
        nominal.sort_by(f64::total_cmp);
        let median = nominal[30];
        assert!(median >= 2_000.0, "path {path}: the long loan is gone");
        assert!(
            median <= 2_000.0 + 1_316.14 * peak * 1.001,
            "path {path}: median monthly debt service {median}, peak price level {peak}"
        );
    }
}
