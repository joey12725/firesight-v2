//! The opt-in housing realism options (lumpy maintenance, assessment caps and
//! insurance drift, market rents, slow sales, adjustable-rate mortgages) on paired
//! paths: with an option off a household simulates exactly as before; with it on
//! the outcome moves in the expected direction by a plausible amount; and money is
//! conserved and every number finite in every option at once.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{
    PathEnd, PathScalar, Recorder, TraceEventKind, YearRecord, YearSeries,
};

const SEED: u64 = 77;

/// Collects one series per path per year (today's dollars) and the path scalars.
struct Collect {
    series: YearSeries,
    rows: Vec<Vec<f64>>,
}

impl Collect {
    fn new(series: YearSeries) -> Self {
        Collect {
            series,
            rows: Vec::new(),
        }
    }
}

impl Recorder for Collect {
    fn begin_path(&mut self, _path_index: u64) {
        self.rows.push(Vec::new());
    }
    fn record_year(&mut self, _year_index: usize, record: &YearRecord) {
        let v = record.get(self.series);
        self.rows.last_mut().unwrap().push(v);
    }
    fn end_path(&mut self, _end: &PathEnd) {}
}

fn collect(profile: &Profile, series: YearSeries, paths: u64) -> Vec<Vec<f64>> {
    let plan = compile(profile);
    let mut ws = Workspace::new(&plan);
    let mut rec = Collect::new(series);
    for i in 0..paths {
        sim::run_path(&plan, SEED, i, &mut ws, &mut rec);
    }
    rec.rows
}

fn mean(xs: impl Iterator<Item = f64>) -> f64 {
    let (s, n) = xs.fold((0.0, 0.0), |(s, n), x| (s + x, n + 1.0));
    s / n
}

fn sd(xs: &[f64]) -> f64 {
    let m = mean(xs.iter().copied());
    (xs.iter()
        .map(|x| firesight_core::math::powi(x - m, 2))
        .sum::<f64>()
        / xs.len() as f64)
        .sqrt()
}

/// A rich retiree (never short of money) over 30 years with `properties`.
fn household(properties: Vec<Property>) -> Profile {
    let mut p = retiree(8_000_000.0, 0.6, 36_000.0, 30.0);
    p.properties = properties;
    p
}

/// A debt-free home whose only cost is the one the test switches on.
fn bare_home(value: f64) -> Property {
    Property {
        id: "home".into(),
        value,
        property_tax_rate: Some(0.0),
        insurance_rate: 0.0,
        maintenance_rate: 0.0,
        hoa_monthly: 0.0,
        idiosyncratic_volatility: 0.0,
        ..Property::default()
    }
}

// ---- Off is identical -------------------------------------------------------------

#[test]
fn a_profile_without_the_new_fields_simulates_like_one_with_their_defaults() {
    let mut p = household(vec![Property {
        mortgage: Some(Mortgage {
            balance: 200_000.0,
            rate: 0.05,
            remaining_months: 240,
            ..Mortgage::default()
        }),
        ..bare_home(600_000.0)
    }]);
    p.spending
        .items
        .push(item("rent", Category::Rent, 500.0, true));
    let json = serde_json::to_value(&p).unwrap();
    // Remove every new key: an older profile file.
    fn strip(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(map) => {
                for k in [
                    "maintenance",
                    "assessment",
                    "insurance_real_growth",
                    "mortgage_kind",
                    "rent_growth",
                    "housing_liquidity",
                ] {
                    map.remove(k);
                }
                if map.contains_key("remaining_months") && map.contains_key("balance") {
                    map.remove("kind");
                }
                for x in map.values_mut() {
                    strip(x);
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut old = json.clone();
    strip(&mut old);
    assert_eq!(
        old, json,
        "default values of the new fields are omitted from the serialised profile"
    );
    let old: Profile = serde_json::from_value(old).unwrap();
    assert_eq!(old, p);
    let a = summary(&p, SEED, 100);
    let b = summary(&old, SEED, 100);
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

// ---- F11 lumpy maintenance --------------------------------------------------------

#[test]
fn lumpy_maintenance_has_the_same_mean_cost_and_far_more_spread() {
    let mut smooth = bare_home(500_000.0);
    smooth.maintenance_rate = 0.01;
    let mut lumpy = smooth.clone();
    lumpy.maintenance = MaintenanceModel::Lumpy;
    let paths = 300;
    let a = collect(&household(vec![smooth]), YearSeries::SpendHousing, paths);
    let b = collect(&household(vec![lumpy]), YearSeries::SpendHousing, paths);
    let total = |rows: &Vec<Vec<f64>>| mean(rows.iter().map(|r| r.iter().sum::<f64>()));
    let ratio = total(&b) / total(&a);
    // Both are 1% of value in expectation (repairs are sized on the value of the
    // day). Over 3,000 paths the ratio is 1.001 (1.02 in the first ten years, 0.993
    // in the last twenty: the start-up ages are not quite the stationary mix); 300
    // paths leave sampling error of about 1.2%.
    assert!(
        (0.95..=1.05).contains(&ratio),
        "lumpy / smooth total cost {ratio}"
    );
    // The most expensive full year against the median full year (year 0 is a two-month
    // stub). A smooth charge follows the home's value, which never triples; a lumpy
    // path has a routine median (0.5% of value) and nearly always a repair year of
    // at least 4 times that (a roof alone is 3.75% of value, 7.5 times the routine).
    let spike = |r: &Vec<f64>| {
        let mut years: Vec<f64> = r[1..].to_vec();
        years.sort_by(|x, y| x.partial_cmp(y).unwrap());
        years[years.len() - 1] / years[years.len() / 2]
    };
    assert!(a.iter().all(|r| spike(r) < 3.0));
    let spiky = b.iter().filter(|r| spike(r) > 4.0).count();
    assert!(
        spiky as f64 > 0.6 * paths as f64,
        "{spiky} of {paths} paths had a major repair year"
    );
}

// ---- F12 assessment and insurance ------------------------------------------------

#[test]
fn a_two_percent_assessment_cap_lowers_property_tax_a_lot_in_a_rising_market() {
    let mut market = bare_home(800_000.0);
    market.property_tax_rate = Some(0.011);
    let mut capped = market.clone();
    capped.assessment = Assessment::Capped {
        annual_cap: 0.02,
        assessed_value: None,
    };
    let paths = 200;
    let a = collect(&household(vec![market]), YearSeries::SpendHousing, paths);
    let b = collect(&household(vec![capped]), YearSeries::SpendHousing, paths);
    let years = 25;
    let cum = |r: &Vec<f64>| r.iter().take(years).sum::<f64>();
    // Cheaper on nearly every path (the cap can only bind when the market has risen
    // by more than 2% a year plus inflation: in a real-terms sense it binds only when
    // nominal prices rise faster than 2%).
    let cheaper = a.iter().zip(&b).filter(|(x, y)| cum(y) < cum(x)).count();
    assert!(cheaper as f64 > 0.9 * paths as f64, "{cheaper} of {paths}");
    // The saving is the 2.2%-a-year assessment drift against nominal prices of 4 to
    // 5%: of the order of a fifth to a half of the tax over 25 years.
    let saving = 1.0 - mean(b.iter().map(cum)) / mean(a.iter().map(cum));
    assert!((0.10..=0.55).contains(&saving), "saving {saving}");
}

#[test]
fn insurance_drift_raises_premiums_on_average_with_a_long_tail() {
    let mut fixed = bare_home(500_000.0);
    fixed.insurance_rate = 0.005;
    let mut drift = fixed.clone();
    drift.insurance_real_growth = InsuranceGrowth::Stochastic;
    let paths = 400;
    let a = collect(&household(vec![fixed]), YearSeries::SpendHousing, paths);
    let b = collect(&household(vec![drift]), YearSeries::SpendHousing, paths);
    let years = 20;
    let cum = |r: &Vec<f64>| r.iter().take(years).sum::<f64>();
    // Paired: the home's value path is the same, so the ratio is the premium index
    // averaged over 20 years. Its drift is about 1.5% a year: about +16% on average.
    let ratio = mean(b.iter().map(cum)) / mean(a.iter().map(cum));
    assert!((1.08..=1.35).contains(&ratio), "ratio {ratio}");
    let per_path: Vec<f64> = a.iter().zip(&b).map(|(x, y)| cum(y) / cum(x)).collect();
    // Never below half, and some paths see premiums well above the mean.
    assert!(per_path.iter().all(|r| *r > 0.5));
    assert!(per_path.iter().any(|r| *r > 1.6), "no heavy-tail path");
    assert!(sd(&per_path) > 0.1);
}

#[test]
fn every_property_of_a_path_follows_the_same_insurance_index() {
    // Two identical homes (no local price noise) pay exactly twice what one pays,
    // year by year and path by path: one index, not one per property.
    let mut home = bare_home(500_000.0);
    home.insurance_rate = 0.005;
    home.insurance_real_growth = InsuranceGrowth::Stochastic;
    let mut second = home.clone();
    second.id = "second".into();
    second.usage = PropertyUse::Vacation;
    let one = collect(&household(vec![home.clone()]), YearSeries::SpendHousing, 40);
    let two = collect(&household(vec![home, second]), YearSeries::SpendHousing, 40);
    for (a, b) in one.iter().zip(&two) {
        for (x, y) in a.iter().zip(b) {
            assert!((2.0 * x - y).abs() <= 1e-9 * y.abs(), "{x} and {y}");
        }
    }
}

// ---- F13 market rents --------------------------------------------------------------

fn renter(growth: RentGrowth) -> Profile {
    let mut p = retiree(8_000_000.0, 0.6, 36_000.0, 30.0);
    let mut rent = item("rent", Category::Rent, 2_500.0, true);
    rent.rent_growth = growth;
    p.spending.items = vec![rent];
    p
}

#[test]
fn market_rents_have_the_same_trend_and_much_more_variation() {
    let paths = 300;
    let fixed = collect(
        &renter(RentGrowth::Fixed),
        YearSeries::SpendEssential,
        paths,
    );
    let market = collect(
        &renter(RentGrowth::Market),
        YearSeries::SpendEssential,
        paths,
    );
    // Year 0 is a partial year (the plan starts in November); both pay the same.
    assert_eq!(fixed[0][0], market[0][0]);
    for year in [10usize, 20] {
        let ratios: Vec<f64> = fixed
            .iter()
            .zip(&market)
            .map(|(f, m)| m[year] / f[year])
            .collect();
        let m = mean(ratios.iter().copied());
        // Over 3,000 paths the mean ratio is 1.003 at year 10 and 1.004 at year 20;
        // 300 paths leave sampling error of about 0.35%.
        assert!((0.98..=1.03).contains(&m), "year {year}: mean ratio {m}");
        // Fixed rent has no variation in real terms: all the spread is the market's.
        let s = sd(&ratios);
        // The local deviation has a stationary sd of 5% and home prices add a little:
        // 5.3% at year 10 and 6.0% at year 20 over 3,000 paths.
        assert!((0.035..=0.09).contains(&s), "year {year}: sd of ratio {s}");
    }
}

#[test]
fn leases_are_sometimes_not_renewed_and_only_notable_renewals_are_logged() {
    let plan = compile(&renter(RentGrowth::Market));
    let mut forced = 0;
    let mut renewals = 0;
    let paths = 60;
    for path in 0..paths {
        let trace = sim::trace_path(&plan, SEED, path);
        for e in &trace.events {
            if e.label.starts_with("Rent: lease not renewed") {
                forced += 1;
            } else if let Some(rest) = e.label.strip_prefix("Rent: lease renewed (") {
                renewals += 1;
                let pct: f64 = rest[..rest.find('%').unwrap()].parse().unwrap();
                assert!(pct.abs() >= 2.95, "a small renewal was logged: {}", e.label);
            }
        }
    }
    // 29 renewals a path at 6% a year: 104 forced moves expected in 1,740 lease
    // years, with a binomial sd of 10.
    let rate = f64::from(forced) / (paths as f64 * 29.0);
    assert!((0.04..=0.08).contains(&rate), "forced share {rate}");
    // Ordinary renewals of 3% or more happen, but not every year.
    assert!(renewals > 0 && renewals < paths * 20, "{renewals} logged");
}

#[test]
fn rent_renewals_are_not_logged_while_the_household_does_not_pay_market_rent() {
    // The rent line ends after five years; the market goes on, the log does not.
    let mut p = renter(RentGrowth::Market);
    p.spending.items[0].end = Some(When::date(START + 60));
    let plan = compile(&p);
    for path in 0..40 {
        let trace = sim::trace_path(&plan, SEED, path);
        for e in &trace.events {
            if e.label.starts_with("Rent:") {
                assert!(e.month < START + 60, "{} at {:?}", e.label, e.month);
            }
        }
    }
}

// ---- F14 slow home sales ----------------------------------------------------------

fn seller(liquidity: HousingLiquidity) -> Profile {
    let mut p = household(vec![Property {
        mortgage: Some(Mortgage {
            balance: 150_000.0,
            rate: 0.04,
            remaining_months: 240,
            ..Mortgage::default()
        }),
        ..bare_home(600_000.0)
    }]);
    p.assumptions.housing_liquidity = liquidity;
    p.events = vec![LifeEvent {
        id: "sell".into(),
        name: "sell".into(),
        when: When::date(START + 60),
        kind: EventKind::SellProperty(SellProperty {
            property: "home".into(),
            selling_cost_fraction: 0.06,
            then_rent_monthly: Some(2_500.0),
        }),
        ..LifeEvent::default()
    }];
    p
}

/// `(months from the decision to closing, discount in percent)` of each traced path.
fn sale_delays(profile: &Profile, paths: u64) -> Vec<(i32, f64)> {
    let plan = compile(profile);
    let mut out = Vec::new();
    for path in 0..paths {
        let trace = sim::trace_path(&plan, SEED, path);
        let sold = trace
            .events
            .iter()
            .find(|e| e.kind == TraceEventKind::SoldHome)
            .expect("the home is sold");
        let discount = trace
            .events
            .iter()
            .find(|e| e.label.starts_with("Listed"))
            .map(|e| {
                let at = e.label.find(" at ").unwrap();
                let end = e.label.find("% below").unwrap();
                e.label[at + 4..end].parse::<f64>().unwrap()
            })
            .unwrap_or(0.0);
        out.push(((sold.month - (START + 60)), discount));
    }
    out
}

#[test]
fn instant_sales_close_the_month_they_are_decided() {
    for (months, discount) in sale_delays(&seller(HousingLiquidity::Instant), 20) {
        assert_eq!((months, discount), (0, 0.0));
    }
}

#[test]
fn realistic_sales_take_months_and_longer_when_the_market_is_weak() {
    let d = sale_delays(&seller(HousingLiquidity::Realistic), 250);
    assert!(
        d.iter().all(|(m, _)| *m >= 2),
        "a sale closes in under 2 months"
    );
    let mean_months = mean(d.iter().map(|(m, _)| f64::from(*m)));
    // 2 months plus half a month on average in a normal market, up to 5 in a
    // 2009-style one; most paths are not in one.
    assert!(
        (2.2..=4.0).contains(&mean_months),
        "mean months to close {mean_months}"
    );
    let weak: Vec<_> = d.iter().filter(|(_, disc)| *disc > 0.0).collect();
    let calm: Vec<_> = d.iter().filter(|(_, disc)| *disc == 0.0).collect();
    assert!(!weak.is_empty() && !calm.is_empty(), "{} weak", weak.len());
    let months = |v: &Vec<&(i32, f64)>| mean(v.iter().map(|(m, _)| f64::from(*m)));
    assert!(
        months(&weak) > months(&calm) + 0.5,
        "weak {} vs calm {}",
        months(&weak),
        months(&calm)
    );
    // No discount exceeds 25%, and the weakest markets are around the forced-sale
    // discounts of the literature (3 to 7%, with a spread around the 6% mean).
    assert!(d.iter().all(|(_, disc)| *disc <= 25.0));
    assert!(weak.iter().any(|(_, disc)| *disc >= 3.0));
}

#[test]
fn a_sale_into_a_weak_market_nets_less_than_the_same_sale_made_at_once() {
    let paths = 400;
    let sale = |l| {
        collect(&seller(l), YearSeries::PropertySale, paths)
            .iter()
            .map(|r| r.iter().sum::<f64>())
            .collect::<Vec<_>>()
    };
    let instant = sale(HousingLiquidity::Instant);
    let slow = sale(HousingLiquidity::Realistic);
    let terms = sale_delays(&seller(HousingLiquidity::Realistic), paths);
    // Net proceeds (today's dollars) of the slow sale over the instant one, by
    // whether the market was weak when the home was listed.
    let ratio = |weak: bool| {
        let r: Vec<f64> = (0..paths as usize)
            .filter(|i| (terms[*i].1 > 0.0) == weak)
            .map(|i| slow[i] / instant[i])
            .collect();
        assert!(r.len() > 20, "{} paths with weak = {weak}", r.len());
        mean(r.into_iter())
    };
    // In a normal market the slow sale closes two or three months later at the
    // home's value of that month: the same money, give or take a quarter's real
    // price change (leveraged 1.35 times by the mortgage) and principal repaid.
    let calm = ratio(false);
    assert!((0.98..=1.03).contains(&calm), "calm ratio {calm}");
    // In a weak market the home sells below its value (3% of the price on average
    // at half stress, 6% at full) after a longer wait in a market that is often
    // still falling: several percent of the net proceeds, not tens.
    let weak = ratio(true);
    assert!(weak < calm - 0.02, "weak {weak} against calm {calm}");
    assert!(weak > 0.80, "weak ratio {weak}");
}

// ---- F15 adjustable-rate mortgages -------------------------------------------------

fn arm_kind() -> MortgageKind {
    MortgageKind::Arm {
        fixed_months: 24,
        adjust_every_months: 12,
        margin: 0.0275,
        periodic_cap: 0.02,
        lifetime_cap: 0.05,
    }
}

fn loan_home(kind: MortgageKind) -> Profile {
    household(vec![Property {
        mortgage: Some(Mortgage {
            balance: 400_000.0,
            rate: 0.04,
            remaining_months: 360,
            kind,
            ..Mortgage::default()
        }),
        ..bare_home(800_000.0)
    }])
}

#[test]
fn an_arm_costs_the_same_until_it_resets_then_more_on_average_with_a_tail() {
    let paths = 300;
    let fixed = collect(
        &loan_home(MortgageKind::Fixed),
        YearSeries::SpendHousing,
        paths,
    );
    let arm = collect(&loan_home(arm_kind()), YearSeries::SpendHousing, paths);
    // Years 0 (a two-month stub) and 1: before the reset at month 24 the two loans
    // are identical.
    for p in 0..paths as usize {
        assert_eq!(fixed[p][0], arm[p][0]);
        assert_eq!(fixed[p][1], arm[p][1]);
    }
    // Eight years in: the ARM pays the history's short rates plus 2.75 points, capped
    // at 9%, on a loan that started at 4%.
    let diffs: Vec<f64> = fixed.iter().zip(&arm).map(|(f, a)| a[8] - f[8]).collect();
    let m = mean(diffs.iter().copied());
    // A 4% loan of 400,000 pays 22,900 a year; at 6% re-amortised over 28 years about
    // 29,000. The bounds are +10% to +60% of the original payment.
    assert!((2_300.0..=14_000.0).contains(&m), "mean extra cost {m}");
    // Some paths see rates fall (a lower payment) and some hit the 9% lifetime cap
    // (about 30,500 on 400,000 over 28 years, 7,600 a year more).
    assert!(diffs.iter().any(|d| *d < -500.0), "no path got cheaper");
    assert!(diffs.iter().any(|d| *d > 6_000.0), "no path got dearer");
    assert!(
        diffs.iter().all(|d| *d < 14_000.0),
        "the lifetime cap binds"
    );
}

#[test]
fn an_arm_with_no_room_to_move_is_a_fixed_loan() {
    let rigid = MortgageKind::Arm {
        fixed_months: 12,
        adjust_every_months: 12,
        margin: 0.0275,
        periodic_cap: 0.0,
        lifetime_cap: 0.0,
    };
    // Without refinancing: a refinance re-derives the months left from the payment
    // and rounds up, an ARM knows them exactly, which can differ by a month.
    let no_refi = |kind| {
        let mut p = loan_home(kind);
        p.debt_policy.refinance_mortgages = false;
        p
    };
    let a = collect(&no_refi(MortgageKind::Fixed), YearSeries::SpendHousing, 40);
    let b = collect(&no_refi(rigid), YearSeries::SpendHousing, 40);
    // Re-amortising the same balance at the same rate over the months left gives the
    // same payment, up to rounding.
    for (x, y) in a.iter().zip(&b) {
        for (u, v) in x.iter().zip(y) {
            assert!((u - v).abs() <= 1e-6 * u.abs().max(1.0), "{u} vs {v}");
        }
    }
}

// ---- Conservation and finiteness with everything on --------------------------------

/// A renter who buys a house on an adjustable loan, owns a rental, and later sells
/// (and, in the poorer variant, is forced to).
fn everything(poor: bool) -> Profile {
    let mut p = retiree(
        if poor { 250_000.0 } else { 1_500_000.0 },
        0.6,
        60_000.0,
        30.0,
    );
    let mut rent = item("rent", Category::Rent, 2_000.0, true);
    rent.rent_growth = RentGrowth::Market;
    p.spending.items.push(rent);
    p.assumptions.housing_liquidity = HousingLiquidity::Realistic;
    p.retirement.home_equity = HomeEquity::LastResort;
    p.properties = vec![Property {
        id: "rental".into(),
        usage: PropertyUse::Rental(RentalTerms {
            monthly_rent: 2_200.0,
            vacancy: 0.06,
            management_fee: 0.08,
            ..RentalTerms::default()
        }),
        value: 350_000.0,
        basis: Some(280_000.0),
        mortgage: Some(Mortgage {
            balance: 180_000.0,
            rate: 0.05,
            remaining_months: 300,
            kind: arm_kind(),
            ..Mortgage::default()
        }),
        maintenance: MaintenanceModel::Lumpy,
        insurance_real_growth: InsuranceGrowth::Stochastic,
        ..Property::default()
    }];
    p.events = vec![
        LifeEvent {
            id: "buy".into(),
            name: "buy".into(),
            when: When::date(START + 30),
            kind: EventKind::BuyHome(BuyHome {
                property_id: "house".into(),
                price: 500_000.0,
                down_payment_fraction: if poor { 0.05 } else { 0.2 },
                maintenance: MaintenanceModel::Lumpy,
                assessment: Assessment::Capped {
                    annual_cap: 0.02,
                    assessed_value: None,
                },
                insurance_real_growth: InsuranceGrowth::Stochastic,
                mortgage_kind: arm_kind(),
                ..BuyHome::default()
            }),
            ..LifeEvent::default()
        },
        LifeEvent {
            id: "sell".into(),
            name: "sell".into(),
            when: When::date(START + 150),
            kind: EventKind::SellProperty(SellProperty {
                property: "house".into(),
                selling_cost_fraction: 0.06,
                then_rent_monthly: Some(2_800.0),
            }),
            ..LifeEvent::default()
        },
    ];
    p
}

#[test]
fn money_is_conserved_with_every_option_on() {
    for poor in [false, true] {
        let profile = everything(poor);
        let report = firesight_core::validate::validate(&profile);
        assert!(report.is_ok(), "{:#?}", report.errors);
        for market in MARKETS {
            let plan = compile(&with_market(profile.clone(), market));
            let acc = run_checked(&plan, 9, 0..60);
            assert_eq!(acc.paths, 60);
        }
    }
}

#[test]
fn housing_realism_and_housing_decisions_together_conserve_money() {
    // Slow sales, lumpy maintenance, an adjustable loan and market rents together with
    // every housing decision (a home-equity line or a reverse mortgage, downsizing,
    // cash-out refinancing and prepayment, realistic rental dynamics), with savings
    // running out so that forced sales, bridges and borrowing all happen.
    for poor in [false, true] {
        for profile in [
            with_housing_decisions(overspending(everything(poor), 1.8)),
            with_housing_decisions_hecm(overspending(everything(poor), 1.8)),
        ] {
            let report = firesight_core::validate::validate(&profile);
            assert!(report.is_ok(), "{:#?}", report.errors);
            for market in MARKETS {
                let plan = compile(&with_market(profile.clone(), market));
                let acc = run_checked(&plan, 9, 0..60);
                assert_eq!(acc.paths, 60);
            }
        }
    }
}

#[test]
fn every_number_is_finite_with_every_option_on() {
    struct Finite;
    impl Recorder for Finite {
        fn begin_path(&mut self, _: u64) {}
        fn record_year(&mut self, y: usize, r: &YearRecord) {
            for (s, v) in YearSeries::ALL.iter().zip(r.values) {
                assert!(v.is_finite(), "year {y} {}: {v}", s.key());
                assert!(v.abs() < 1e13, "year {y} {} = {v}", s.key());
            }
        }
        fn end_path(&mut self, end: &PathEnd) {
            for s in PathScalar::ALL {
                assert!(!end.get(*s).is_infinite(), "{}", s.key());
            }
        }
    }
    for poor in [false, true] {
        let plan = compile(&everything(poor));
        let mut ws = Workspace::new(&plan);
        for path in 0..60 {
            sim::run_path(&plan, 4, path, &mut ws, &mut Finite);
        }
    }
}

#[test]
fn a_forced_sale_waits_for_closing_and_is_logged() {
    let plan = compile(&everything(true));
    let mut ws = Workspace::new(&plan);
    ws.set_check(true);
    let mut listed = 0;
    let mut closed = 0;
    for path in 0..80 {
        let trace = sim::trace_path_in(&plan, 3, path, &mut ws);
        let end = trace.start + trace.months as i32;
        for (i, e) in trace.events.iter().enumerate() {
            if !e.label.starts_with("Listed") {
                continue;
            }
            listed += 1;
            let name = &e.label["Listed ".len()..e.label.find(" for sale").unwrap()];
            // No instant sale in this mode: the next thing that happens to the
            // property is its closing, at least two months later, unless the plan
            // ends first.
            assert!(!trace
                .events
                .iter()
                .any(|x| x.label.contains("because savings ran out")));
            let closing = trace.events[i..]
                .iter()
                .find(|x| x.kind == TraceEventKind::SoldHome && x.label.contains(name));
            match closing {
                Some(c) => {
                    closed += 1;
                    assert!(c.month >= e.month + 2, "{:?} then {:?}", e.month, c.month);
                    assert!(c.month <= e.month + 24);
                }
                None => assert!(end <= e.month + 24, "a listing never closed"),
            }
        }
    }
    assert!(listed > 20, "{listed} sales listed");
    assert!(closed * 10 >= listed * 9, "{closed} of {listed} closed");
}

// ---- Validation ---------------------------------------------------------------------

#[test]
fn impossible_settings_are_errors_and_odd_ones_warnings() {
    use firesight_core::validate::validate;
    let codes = |p: &Profile| {
        let r = validate(p);
        (
            r.errors.iter().map(|i| i.code.clone()).collect::<Vec<_>>(),
            r.warnings
                .iter()
                .map(|i| i.code.clone())
                .collect::<Vec<_>>(),
        )
    };
    // Market rent on a grocery line.
    let mut p = renter(RentGrowth::Fixed);
    let mut food = item("food", Category::Groceries, 800.0, true);
    food.rent_growth = RentGrowth::Market;
    p.spending.items.push(food);
    assert!(codes(&p).0.contains(&"rent_growth_category".to_string()));
    // Market rent with an explicit real growth: ignored, so a warning.
    let mut p = renter(RentGrowth::Market);
    p.spending.items[0].real_growth = Some(0.02);
    assert!(codes(&p).1.contains(&"rent_growth_ignored".to_string()));
    // ARM terms.
    let bad = MortgageKind::Arm {
        fixed_months: 60,
        adjust_every_months: 0,
        margin: 0.03,
        periodic_cap: 0.05,
        lifetime_cap: 0.02,
    };
    assert!(codes(&loan_home(bad))
        .0
        .contains(&"out_of_range".to_string()));
    let odd = MortgageKind::Arm {
        fixed_months: 60,
        adjust_every_months: 12,
        margin: 0.03,
        periodic_cap: 0.05,
        lifetime_cap: 0.02,
    };
    assert!(codes(&loan_home(odd))
        .1
        .contains(&"arm_caps_inverted".to_string()));
    // Assessment cap.
    let mut home = bare_home(500_000.0);
    home.assessment = Assessment::Capped {
        annual_cap: 0.4,
        assessed_value: None,
    };
    assert!(codes(&household(vec![home.clone()]))
        .0
        .contains(&"out_of_range".to_string()));
    home.assessment = Assessment::Capped {
        annual_cap: 0.1,
        assessed_value: None,
    };
    assert!(codes(&household(vec![home]))
        .1
        .contains(&"assessment_cap_high".to_string()));
    // Slow sales with no property.
    let mut p = household(vec![]);
    p.assumptions.housing_liquidity = HousingLiquidity::Realistic;
    assert!(codes(&p)
        .1
        .contains(&"housing_liquidity_unused".to_string()));
}

// ---- Random streams ------------------------------------------------------------------

/// Switches every option on wherever the profile has something for it to act on.
fn all_options_on(mut p: Profile) -> Profile {
    let cap = Assessment::Capped {
        annual_cap: 0.02,
        assessed_value: None,
    };
    for pr in &mut p.properties {
        pr.maintenance = MaintenanceModel::Lumpy;
        pr.assessment = cap;
        pr.insurance_real_growth = InsuranceGrowth::Stochastic;
        if let Some(m) = &mut pr.mortgage {
            m.kind = arm_kind();
        }
    }
    for ev in &mut p.events {
        if let EventKind::BuyHome(b) = &mut ev.kind {
            b.maintenance = MaintenanceModel::Lumpy;
            b.assessment = cap;
            b.insurance_real_growth = InsuranceGrowth::Stochastic;
            b.mortgage_kind = arm_kind();
        }
    }
    for item in &mut p.spending.items {
        if item.category == Category::Rent {
            item.rent_growth = RentGrowth::Market;
        }
    }
    p.assumptions.housing_liquidity = HousingLiquidity::Realistic;
    p
}

fn scalar(t: &firesight_core::stats::PathTrace, s: PathScalar) -> u64 {
    t.scalars
        .iter()
        .find(|x| x.scalar == s)
        .unwrap()
        .value
        .to_bits()
}

fn months_of(t: &firesight_core::stats::PathTrace, kind: TraceEventKind) -> Vec<YearMonth> {
    t.events
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| e.month)
        .collect()
}

#[test]
fn switching_the_options_on_leaves_markets_careers_health_and_lifespans_untouched() {
    for example in Profile::examples(START) {
        let on = all_options_on(example.profile.clone());
        assert!(firesight_core::validate::validate(&on).is_ok());
        let (plan_a, plan_b) = (compile(&example.profile), compile(&on));
        let mut changed = 0;
        for path in 0..20 {
            let a = sim::trace_path(&plan_a, 21, path);
            let b = sim::trace_path(&plan_b, 21, path);
            let at = format!("{} path {path}", example.slug);
            // Lifespans and inflation.
            assert_eq!(a.months, b.months, "{at}");
            for s in [
                PathScalar::DeathAge0,
                PathScalar::DeathAge1,
                PathScalar::Inflation,
            ] {
                assert_eq!(scalar(&a, s), scalar(&b, s), "{at} {s:?}");
            }
            // Markets, month by month.
            for key in [
                "price_level",
                "return_us_stocks",
                "return_intl_stocks",
                "return_bonds",
                "return_cash",
            ] {
                assert_eq!(a.column(key), b.column(key), "{at} {key}");
            }
            // Careers: the same layoffs up to the earlier retirement (the options
            // may move the date the household can afford to stop).
            let retired = |t: &firesight_core::stats::PathTrace| {
                months_of(t, TraceEventKind::Retired)
                    .first()
                    .copied()
                    .unwrap_or(YearMonth::ym(9999, 1))
            };
            let cutoff = retired(&a).min(retired(&b));
            let layoffs = |t: &firesight_core::stats::PathTrace| -> Vec<YearMonth> {
                months_of(t, TraceEventKind::LaidOff)
                    .into_iter()
                    .filter(|m| *m < cutoff)
                    .collect()
            };
            assert_eq!(layoffs(&a), layoffs(&b), "{at}");
            // Health: long-term care begins in the same months.
            assert_eq!(
                months_of(&a, TraceEventKind::LongTermCare),
                months_of(&b, TraceEventKind::LongTermCare),
                "{at}"
            );
            if a.column("net_worth") != b.column("net_worth") {
                changed += 1;
            }
        }
        assert_eq!(changed, 20, "{}: the options did something", example.slug);
    }
}

#[test]
fn an_unrelated_decision_does_not_move_the_options_draws() {
    // The same household spending 30% less, and one that sells its house three
    // years earlier: the rent market, the insurance index, the rental's repairs and
    // (for the first) the terms of the listed sale are the same, event for event.
    let base = everything(false);
    let mut frugal = base.clone();
    for item in &mut frugal.spending.items {
        if item.category != Category::Rent {
            item.amount *= 0.7;
        }
    }
    frugal.spending.shocks = ShockLevel::Off;
    let mut early = base.clone();
    early.events[1].when = When::date(START + 114);
    let plans = [compile(&base), compile(&frugal), compile(&early)];
    let picked = |t: &firesight_core::stats::PathTrace, prefixes: &[&str], until: YearMonth| {
        t.events
            .iter()
            .filter(|e| e.month < until && prefixes.iter().any(|p| e.label.starts_with(p)))
            .map(|e| (e.month, e.label.clone()))
            .collect::<Vec<_>>()
    };
    let mut seen = [0usize; 3];
    for path in 0..25 {
        let [a, b, c] = [0, 1, 2].map(|i| sim::trace_path(&plans[i], 8, path));
        // The rental is listed only if a household runs out of money, which is its
        // own doing: compare up to the first time any of them does.
        let until = [&a, &b, &c]
            .iter()
            .flat_map(|t| t.events.iter())
            .filter(|e| e.label.starts_with("Listed rental"))
            .map(|e| e.month)
            .min()
            .unwrap_or(YearMonth::ym(9999, 1));
        // Spending less changes nothing the options draw.
        let all = [
            "Rent:",
            "Insurance repriced",
            "Major repair",
            "Listed",
            "Sale of",
        ];
        assert_eq!(
            picked(&a, &all, until),
            picked(&b, &all, until),
            "path {path}"
        );
        // The ARM resets too (same index, same loans).
        assert_eq!(
            picked(&a, &["rental: adjustable", "buy: adjustable"], until),
            picked(&b, &["rental: adjustable", "buy: adjustable"], until),
            "path {path}"
        );
        // Selling earlier moves the listing, but not the insurance index, nor the
        // rental's repairs, nor the rent market while both still rent or both own.
        let own = ["Insurance repriced", "Major repair on rental"];
        assert_eq!(
            picked(&a, &own, until),
            picked(&c, &own, until),
            "path {path}"
        );
        assert_eq!(
            picked(&a, &["Rent:"], START + 30),
            picked(&c, &["Rent:"], START + 30),
            "path {path}"
        );
        seen[0] += picked(&a, &["Insurance repriced"], until).len();
        seen[1] += picked(&a, &["Major repair"], until).len();
        seen[2] += picked(&a, &["Listed"], until).len();
    }
    assert!(seen.iter().all(|n| *n >= 20), "{seen:?}");
}
