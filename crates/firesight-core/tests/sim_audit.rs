//! Regression tests for the findings of the engine audit. Each test states the
//! defect it guards against and fails on the code as it was before the fix.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim;
use firesight_core::stats::{FailureCause, PathTrace, Summary};
use serde_json::{json, Value};

const SEED: u64 = 11;

/// One of the demo households as JSON, to be edited by pointer.
fn example(slug: &str) -> Value {
    let found = Profile::examples(START)
        .into_iter()
        .find(|e| e.slug == slug)
        .unwrap_or_else(|| panic!("no example {slug}"));
    serde_json::to_value(&found.profile).unwrap()
}

fn set(doc: &mut Value, pointer: &str, value: Value) {
    *doc.pointer_mut(pointer)
        .unwrap_or_else(|| panic!("no {pointer}")) = value;
}

fn profile(doc: &Value) -> Profile {
    serde_json::from_value(doc.clone()).expect("a valid profile")
}

fn summarize(doc: &Value, paths: u64) -> Summary {
    summary(&profile(doc), SEED, paths)
}

fn trace(doc: &Value, path: u64) -> PathTrace {
    sim::trace_path(&compile(&profile(doc)), SEED, path)
}

/// Annual column of a trace (zero when the series never moved).
fn annual(trace: &PathTrace, key: &str) -> Vec<f64> {
    trace
        .annual
        .columns
        .iter()
        .find(|c| c.key == key)
        .map_or_else(|| vec![0.0; trace.annual.years.len()], |c| c.values.clone())
}

/// A couple of 60, already retired in Texas, with ample taxable basis: 900k
/// taxable (500k basis), 1.2M traditional, 150k Roth.
fn retired_couple_on_the_marketplace() -> Value {
    let mut d = example("dual-income-family");
    set(&mut d, "/household/people/0/birth", json!("1966-09"));
    set(&mut d, "/household/people/1/birth", json!("1966-04"));
    for i in 0..2 {
        set(
            &mut d,
            &format!("/household/people/{i}/social_security"),
            json!({"claim_age": 67.0, "basis": {"type": "known_benefit", "monthly_at_fra": 2600.0}}),
        );
    }
    set(&mut d, "/household/children", json!([]));
    set(&mut d, "/incomes", json!([]));
    set(&mut d, "/events", json!([]));
    set(&mut d, "/debts", json!([]));
    let account = |id: &str, kind: &str, owner: u32, balance: f64, basis: Value, roth: Value| {
        json!({"id": id, "name": id, "owner": owner, "kind": kind, "balance": balance,
               "cost_basis": basis, "roth_basis": roth})
    };
    set(
        &mut d,
        "/accounts",
        json!([
            account("cash", "cash", 0, 14_000.0, Value::Null, Value::Null),
            account(
                "taxable",
                "taxable",
                0,
                900_000.0,
                json!(500_000.0),
                Value::Null
            ),
            account(
                "trad0",
                "traditional",
                0,
                700_000.0,
                Value::Null,
                Value::Null
            ),
            account(
                "trad1",
                "traditional",
                1,
                500_000.0,
                Value::Null,
                Value::Null
            ),
            account("roth", "roth", 0, 150_000.0, Value::Null, json!(100_000.0)),
        ]),
    );
    set(
        &mut d,
        "/retirement/trigger",
        json!({"type": "already_retired"}),
    );
    set(&mut d, "/portfolio/location", json!("mirror"));
    d
}

/// Critical: with the default tax-aware order, a couple who retire at 60 on a
/// marketplace plan lost the premium credit in 88% of the years before Medicare
/// (a 2% cushion under the cliff against January estimates that were off by more,
/// January rebalancing gains, and bracket filling up to the cliff). They must
/// receive it in at least 90% of those years.
#[test]
fn early_retirees_keep_the_premium_credit() {
    let d = retired_couple_on_the_marketplace();
    let plan = compile(&profile(&d));
    let (mut years, mut with_credit) = (0, 0);
    for path in 0..24 {
        let t = sim::trace_path(&plan, SEED, path);
        for r in &t.tax_returns {
            // Both are under 65 for the whole of 2027 to 2030.
            if (2027..=2030).contains(&r.year) {
                years += 1;
                let credit = r.result["aca_premium_credit"].as_f64().unwrap();
                with_credit += i32::from(credit > 0.0);
            }
        }
    }
    assert!(years >= 90, "{years} household-years");
    assert!(
        with_credit * 10 >= years * 9,
        "premium credit in {with_credit} of {years} pre-Medicare years"
    );
}

/// Major: the plan budgeted pre-65 health insurance at full price, so retirees
/// withdrew from retirement accounts and re-saved a median $210k in taxable in the
/// same years, and the HSA paid "medical" bills several times the health spending.
#[test]
fn retirees_do_not_withdraw_in_order_to_save() {
    let d = example("dual-income-family");
    let plan = compile(&profile(&d));
    let (mut withdrawn, mut resaved, mut hsa_excess) = (0.0, 0.0, 0.0);
    for path in 0..10 {
        let t = sim::trace_path(&plan, SEED, path);
        let cols = [
            "withdraw_traditional",
            "withdraw_roth",
            "withdraw_hsa",
            "save_taxable",
            "spend_healthcare",
            "income_wages",
        ]
        .map(|k| annual(&t, k));
        for (i, year) in t.annual.years.iter().enumerate() {
            // Retired (no wages) and both under 65 (born 1989 and 1990).
            if cols[5][i] > 0.0 || *year >= 2054 || *year < 2040 {
                continue;
            }
            withdrawn += cols[0][i] + cols[1][i] + cols[2][i];
            resaved += cols[3][i];
            hsa_excess += (cols[2][i] - cols[4][i]).max(0.0);
        }
    }
    assert!(withdrawn > 0.0);
    assert!(
        resaved < 0.08 * withdrawn,
        "re-saved {resaved:.0} of {withdrawn:.0} withdrawn"
    );
    assert!(hsa_excess < 0.01 * withdrawn, "HSA excess {hsa_excess:.0}");
}

/// Critical: guardrails, Vanguard-dynamic and floor-and-ceiling were anchored on
/// the first retired year's cash withdrawal rate (a bridge rate with a mortgage,
/// children and pre-Medicare insurance in it), cut households that were on plan,
/// and lowered success well below constant spending (83% against 88% on the family
/// example; 70% against 88% for Vanguard). A flexible rule may spend more in good
/// times, so it need not beat constant spending, but it must not fail materially
/// more often, and the delivered share of the plan is never above 1.
#[test]
fn flexible_spending_rules_do_not_fail_more_than_constant_spending() {
    for (slug, rules) in [
        (
            "dual-income-family",
            vec![
                json!({"type": "guardrails", "upper": 0.2, "lower": 0.2, "adjust": 0.1}),
                json!({"type": "vanguard_dynamic", "ceiling": 0.05, "floor": 0.025, "rate": null}),
            ],
        ),
        (
            "side-hustle-landlord",
            vec![
                json!({"type": "vanguard_dynamic", "ceiling": 0.05, "floor": 0.025, "rate": null}),
            ],
        ),
    ] {
        let mut d = example(slug);
        set(
            &mut d,
            "/retirement/spending_rule",
            json!({"type": "constant_real"}),
        );
        let constant = summarize(&d, 1200);
        for rule in rules {
            set(&mut d, "/retirement/spending_rule", rule.clone());
            let s = summarize(&d, 1200);
            let (a, b) = (
                s.headline.success_rate.value,
                constant.headline.success_rate.value,
            );
            assert!(a >= b - 0.025, "{slug} {rule}: {a} against constant {b}");
            let delivered = s.headline.delivered_spending_ratio.value;
            assert!(delivered <= 1.0 && delivered > 0.9, "{slug}: {delivered}");
            assert!(s.headline.extra_spending_ratio.value >= 0.0);
        }
    }
}

/// A couple of 50 with about 2M and an 80k budget; one retires now, the partner
/// earns 95k for `offset` more years.
fn couple_with_a_working_partner(offset: f64, rule: Value) -> Value {
    let mut d = example("early-career-saver");
    let person = |id: &str, sex: &str| {
        json!({"id": id, "name": id, "birth": "1976-11", "sex": sex,
               "social_security": {"claim_age": 67.0, "basis": {"type": "known_benefit", "monthly_at_fra": 2400.0}}})
    };
    set(
        &mut d,
        "/household/people",
        json!([person("a", "female"), person("b", "male")]),
    );
    set(
        &mut d,
        "/household/location",
        json!({"country": "US", "state": "TX"}),
    );
    set(&mut d, "/events", json!([]));
    set(&mut d, "/debts", json!([]));
    set(
        &mut d,
        "/accounts",
        json!([
            {"id": "cash", "name": "cash", "owner": 0, "kind": "cash", "balance": 10_000.0},
            {"id": "tax", "name": "tax", "owner": 0, "kind": "taxable", "balance": 800_000.0, "cost_basis": 600_000.0},
            {"id": "trad", "name": "trad", "owner": 0, "kind": "traditional", "balance": 900_000.0},
            {"id": "roth", "name": "roth", "owner": 0, "kind": "roth", "balance": 300_000.0, "roth_basis": 200_000.0},
        ]),
    );
    set(
        &mut d,
        "/spending/items",
        json!([
            {"id": "ess", "name": "ess", "category": "other", "amount": 4000.0, "essential": true},
            {"id": "disc", "name": "disc", "category": "other", "amount": 2667.0, "essential": false},
        ]),
    );
    set(&mut d, "/spending/healthcare/mode", json!("in_budget"));
    set(&mut d, "/spending/healthcare/long_term_care", json!(false));
    if offset > 0.0 {
        set(&mut d, "/incomes/0/owner", json!(1));
        set(&mut d, "/incomes/0/kind/salary", json!(95_000.0));
        set(&mut d, "/incomes/0/kind/equity", json!([]));
        set(&mut d, "/incomes/0/kind/bonus", Value::Null);
    } else {
        set(&mut d, "/incomes", json!([]));
    }
    set(
        &mut d,
        "/retirement/trigger",
        json!({"type": "already_retired"}),
    );
    set(&mut d, "/retirement/partner_offset_years", json!(offset));
    set(&mut d, "/retirement/spending_rule", rule);
    d
}

/// Major: a partner who kept working made the reference withdrawal rate nearly
/// zero, so when that income stopped as planned the rules cut spending every year:
/// guardrails delivered 68% of plan with a four-year offset against 158% with
/// none. Working longer must not lower what is delivered.
#[test]
fn a_partner_who_works_longer_does_not_trigger_cuts() {
    for rule in [
        json!({"type": "guardrails", "upper": 0.2, "lower": 0.2, "adjust": 0.1}),
        json!({"type": "vanguard_dynamic", "ceiling": 0.05, "floor": 0.025, "rate": null}),
        json!({"type": "floor_ceiling", "floor": 0.85, "ceiling": 1.2, "rate": null}),
    ] {
        let with = summarize(&couple_with_a_working_partner(4.0, rule.clone()), 800);
        let without = summarize(&couple_with_a_working_partner(0.0, rule.clone()), 800);
        let (a, b) = (
            with.headline.delivered_spending_ratio.value,
            without.headline.delivered_spending_ratio.value,
        );
        assert!(
            a >= b - 0.005,
            "{rule}: delivered {a} with the offset, {b} without"
        );
        assert!(a > 0.95, "{rule}: {a}");
        assert!(
            with.spending.paths_below_90.value <= without.spending.paths_below_90.value + 0.02,
            "{rule}"
        );
    }
}

/// Critical: home equity was never touched. Homeowners were scored as depleted
/// while holding a house worth $0.9M at the median, so owning cut the success rate
/// by nine points against renting. By default the home is now sold as a last
/// resort, the sale is reported on its own, and `never` restores the old rule.
#[test]
fn home_equity_is_released_before_essentials_are_cut() {
    let own = example("dual-income-family");
    let mut rent = own.clone();
    let events: Vec<Value> = own["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["id"] != "buy_home")
        .cloned()
        .collect();
    set(&mut rent, "/events", Value::Array(events));
    let mut never = own.clone();
    set(&mut never, "/retirement/home_equity", json!("never"));
    let (own, rent, never) = (
        summarize(&own, 1200),
        summarize(&rent, 1200),
        summarize(&never, 1200),
    );
    let rate = |s: &Summary| s.headline.success_rate.value;
    assert!(
        rate(&own) >= rate(&rent) - 0.02,
        "owning {} against renting {}",
        rate(&own),
        rate(&rent)
    );
    assert!(rate(&never) < rate(&own) - 0.03);
    assert!(own.headline.home_sale_rate.value > 0.02);
    assert_eq!(never.headline.home_sale_rate.value, 0.0);
    assert_eq!(rent.headline.home_sale_rate.value, 0.0);

    // On a path where the home is sold, the household is not left without
    // essentials while it still owns property.
    let d = example("dual-income-family");
    let plan = compile(&profile(&d));
    let mut checked = 0;
    for path in 0..60 {
        let t = sim::trace_path(&plan, SEED, path);
        let Some(sale) = t
            .events
            .iter()
            .find(|e| e.label.contains("because savings ran out"))
        else {
            continue;
        };
        checked += 1;
        let year = sale.month.year();
        let i = t.annual.years.iter().position(|y| *y == year).unwrap();
        let essential = annual(&t, "spend_essential");
        assert!(essential[i] > 0.0, "path {path}: no essentials in {year}");
        if checked >= 3 {
            break;
        }
    }
    assert!(checked > 0, "no path sold a home");
}

/// Major: a taxable windfall was added to the pay that withholding applies to and
/// also taxed through the re-planned instalments: 66% of $5M was taken in the
/// month it arrived and $1.23M came back as "income" the next April.
#[test]
fn a_taxable_windfall_is_taxed_once() {
    let mut d = example("early-career-saver");
    set(
        &mut d,
        "/events",
        json!([{"id": "w", "name": "Windfall", "when": {"type": "date", "at": "2028-06"},
                "kind": {"type": "windfall", "amount": 5_000_000.0, "taxable": true}}]),
    );
    let t = trace(&d, 0);
    let month = 19; // 2028-06 from a 2026-11 start
    let col = |k: &str| t.column(k).map_or(0.0, |c| c[month]);
    assert!((col("income_other") - 5_000_000.0).abs() < 1.0);
    let paid = col("tax_federal") + col("tax_state");
    let liability = t
        .tax_returns
        .iter()
        .find(|r| r.year == 2028)
        .map(|r| r.result["total"].as_f64().unwrap() / r.price_level)
        .unwrap();
    // Seven months of the year remain: about a seventh of the year's tax each.
    assert!(
        paid < 0.25 * liability,
        "{paid:.0} paid in the event month against {liability:.0} for the year"
    );
    // No refund the next spring.
    let other = t.column("income_other").unwrap();
    let refund: f64 = other[26..38].iter().sum();
    assert!(refund < 50_000.0, "refund booked as income: {refund:.0}");
}

/// Major: every job's wage curve ran from the plan's start, so a $50,000 job that
/// began eight years later paid $79,600 in its first full year.
#[test]
fn a_job_that_starts_later_starts_at_its_salary() {
    let mut base = example("early-career-saver");
    set(&mut base, "/assumptions/career_risk", json!(false));
    set(&mut base, "/incomes/0/kind/equity", json!([]));
    set(&mut base, "/incomes/0/kind/bonus", Value::Null);
    set(
        &mut base,
        "/retirement/trigger",
        json!({"type": "at_age", "person": 0, "age": 60.0}),
    );
    let mut with = base.clone();
    let mut second = base["incomes"][0].clone();
    set(&mut second, "/id", json!("second"));
    set(&mut second, "/kind/salary", json!(50_000.0));
    set(&mut second, "/kind/plan", Value::Null);
    set(&mut second, "/kind/hsa_contribution", json!(0.0));
    set(&mut second, "/kind/hsa_employer", json!(0.0));
    set(
        &mut second,
        "/start",
        json!({"type": "date", "at": "2034-11"}),
    );
    with["incomes"].as_array_mut().unwrap().push(second);
    let (a, b) = (trace(&with, 0), trace(&base, 0));
    let wages = |t: &PathTrace, year: i32| {
        let i = t.annual.years.iter().position(|y| *y == year).unwrap();
        annual(t, "income_wages")[i]
    };
    assert!((wages(&a, 2033) - wages(&b, 2033)).abs() < 1.0);
    // 2035 is its first full year: the salary entered plus at most a year of growth.
    let first_year = wages(&a, 2035) - wages(&b, 2035);
    assert!(
        (49_500.0..54_000.0).contains(&first_year),
        "first full year paid {first_year:.0}"
    );
}

/// Major: with `sell_on_vest: false` the default annual rebalance sold the
/// employer shares every January, so holding them changed the 10th-percentile
/// outcome by 2% when really holding changes it by half.
#[test]
fn kept_employer_shares_stay_concentrated() {
    let mut d = example("early-career-saver");
    set(
        &mut d,
        "/incomes/0/kind/equity/0/unvested_value",
        json!(400_000.0),
    );
    set(
        &mut d,
        "/incomes/0/kind/equity/0/vest_months_remaining",
        json!(48),
    );
    set(
        &mut d,
        "/incomes/0/kind/equity/0/refresh_annual_value",
        json!(150_000.0),
    );
    set(
        &mut d,
        "/portfolio/single_stock",
        json!({"volatility": 0.8, "beta": 1.3}),
    );
    set(
        &mut d,
        "/retirement/trigger",
        json!({"type": "at_age", "person": 0, "age": 45.0}),
    );
    let sold = summarize(&d, 1000);
    set(
        &mut d,
        "/incomes/0/kind/equity/0/sell_on_vest",
        json!(false),
    );
    let kept = summarize(&d, 1000);
    let p10 = |s: &Summary| s.headline.investable_at_retirement.p10;
    assert!(
        p10(&kept) < 0.75 * p10(&sold),
        "kept {} against sold {}",
        p10(&kept),
        p10(&sold)
    );
}

/// A single 40-year-old in Texas on 150k with 1M saved (400k taxable at 250k basis,
/// 500k traditional, 100k Roth), a 60k budget and the default 4% funded test.
fn saver_with_appreciated_assets() -> Value {
    let mut d = example("early-career-saver");
    set(&mut d, "/household/people/0/birth", json!("1986-11"));
    set(
        &mut d,
        "/household/location",
        json!({"country": "US", "state": "TX"}),
    );
    set(&mut d, "/incomes/0/kind/salary", json!(150_000.0));
    set(&mut d, "/incomes/0/kind/equity", json!([]));
    set(&mut d, "/incomes/0/kind/bonus", Value::Null);
    set(&mut d, "/events", json!([]));
    set(&mut d, "/debts", json!([]));
    set(
        &mut d,
        "/accounts",
        json!([
            {"id": "cash", "name": "cash", "owner": 0, "kind": "cash", "balance": 10_000.0},
            {"id": "tax", "name": "tax", "owner": 0, "kind": "taxable", "balance": 400_000.0, "cost_basis": 250_000.0},
            {"id": "trad", "name": "trad", "owner": 0, "kind": "traditional", "balance": 500_000.0},
            {"id": "roth", "name": "roth", "owner": 0, "kind": "roth", "balance": 100_000.0, "roth_basis": 60_000.0},
        ]),
    );
    set(
        &mut d,
        "/spending/items",
        json!([
            {"id": "ess", "name": "ess", "category": "other", "amount": 3500.0, "essential": true},
            {"id": "disc", "name": "disc", "category": "other", "amount": 1500.0, "essential": false},
        ]),
    );
    set(&mut d, "/spending/healthcare/mode", json!("in_budget"));
    set(&mut d, "/spending/healthcare/long_term_care", json!(false));
    set(
        &mut d,
        "/retirement/trigger",
        json!({"type": "when_funded", "withdrawal_rate": 0.04, "basis": "investable",
               "count_future_income": true, "max_age": 70.0}),
    );
    set(
        &mut d,
        "/retirement/spending_rule",
        json!({"type": "constant_real"}),
    );
    d
}

/// Major: the conversion planner sized Roth conversions on the ordinary bracket
/// and ignored that each converted dollar pushed a dollar of the year's realised
/// gains from 0% to 15%; the default tax-aware order then succeeded less often
/// than the conventional one (90.7% against 91.6%, 0 rescued and 44 harmed) on a
/// household with appreciated taxable assets. Same paths, same seed.
#[test]
fn tax_aware_is_not_worse_than_conventional_with_appreciated_assets() {
    let mut d = saver_with_appreciated_assets();
    let aware = summarize(&d, 1500);
    set(
        &mut d,
        "/retirement/withdrawal_order",
        json!({"type": "conventional"}),
    );
    let conventional = summarize(&d, 1500);
    assert!(
        aware.headline.success_rate.value >= conventional.headline.success_rate.value - 0.002,
        "tax-aware {} against conventional {}",
        aware.headline.success_rate.value,
        conventional.headline.success_rate.value
    );
}

/// Major: with `count_future_income` the funded test subtracted the present value
/// of a Social Security benefit 25 years away from the liquid target, so a "4%"
/// household retired on a 4.55% draw and ran out mostly before the benefit
/// started. Savings must carry the bridge on their own: the realised first-year
/// rate stays near the rule's.
#[test]
fn the_funded_test_does_not_spend_social_security_decades_early() {
    let d = saver_with_appreciated_assets();
    let s = summarize(&d, 1000);
    let rate = s.headline.initial_withdrawal_rate.p50;
    assert!(rate > 0.03 && rate < 0.042, "median initial rate {rate}");
    let mut off = d.clone();
    set(
        &mut off,
        "/retirement/trigger/count_future_income",
        json!(false),
    );
    let t = summarize(&off, 1000);
    // Counting income that is decades away buys little.
    assert!(
        s.headline.median_retirement_age > t.headline.median_retirement_age - 0.75,
        "{} against {}",
        s.headline.median_retirement_age,
        t.headline.median_retirement_age
    );
}

/// Major: the bootstrap started every path at a random historical mortgage rate
/// and refinancing was free, so 54% of households with a mortgage at today's rate
/// refinanced a point or more lower two months into the plan.
#[test]
fn refinancing_waits_for_rates_to_fall() {
    let mut d = example("side-hustle-landlord");
    set(&mut d, "/properties/0/mortgage/rate", json!(0.0686));
    let plan = compile(&profile(&d));
    let (mut immediate, mut ever) = (0, 0);
    let paths = 60;
    for path in 0..paths as u64 {
        let t = sim::trace_path(&plan, SEED, path);
        let mut refis = t
            .events
            .iter()
            .filter(|e| e.label.starts_with("Refinanced"));
        if let Some(first) = refis.next() {
            ever += 1;
            immediate += i32::from(first.month == YearMonth::ym(2027, 1));
        }
    }
    assert!(immediate * 10 <= paths, "{immediate} of {paths} in 2027-01");
    assert!(ever > 0, "nobody ever refinanced");
    // And the household can switch it off.
    set(&mut d, "/debt_policy/refinance_mortgages", json!(false));
    let plan = compile(&profile(&d));
    for path in 0..10 {
        let t = sim::trace_path(&plan, SEED, path);
        assert!(!t.events.iter().any(|e| e.label.starts_with("Refinanced")));
    }
}

/// Minor: an event dated in January ran before the year was closed, so a home
/// sold in January 2030 put its gain on the 2029 return.
#[test]
fn a_january_sale_is_taxed_in_its_own_year() {
    let mut d = example("side-hustle-landlord");
    set(
        &mut d,
        "/events",
        json!([{"id": "sell", "name": "Sell the duplex", "when": {"type": "date", "at": "2030-01"},
                "kind": {"type": "sell_property", "property": "duplex"}}]),
    );
    let t = trace(&d, 0);
    let gains = |year: i32| {
        t.tax_returns
            .iter()
            .find(|r| r.year == year)
            .and_then(|r| r.lines.iter().find(|l| l.key.as_str() == "capital_gains"))
            .map_or(0.0, |l| l.amount)
    };
    assert!(gains(2030) > 20_000.0, "2030 gains {}", gains(2030));
    assert!(
        gains(2029) < 0.2 * gains(2030),
        "2029 gains {}",
        gains(2029)
    );
}

/// Minor: a household with no assets that spends more than it earns ran out at 27
/// on every path and was told the cause was the market (45%) or a job loss (43%).
#[test]
fn running_out_while_working_is_not_blamed_on_the_market() {
    let mut d = example("early-career-saver");
    set(&mut d, "/incomes/0/kind/salary", json!(50_000.0));
    set(&mut d, "/incomes/0/kind/equity", json!([]));
    set(&mut d, "/incomes/0/kind/bonus", Value::Null);
    set(&mut d, "/events", json!([]));
    set(
        &mut d,
        "/accounts",
        json!([{"id": "cash", "name": "cash", "owner": 0, "kind": "cash", "balance": 0.0}]),
    );
    set(
        &mut d,
        "/spending/items",
        json!([{"id": "all", "name": "all", "category": "other", "amount": 6500.0, "essential": true}]),
    );
    let s = summarize(&d, 400);
    assert!(s.headline.success_rate.value < 0.05);
    let share = |cause: FailureCause| s.failure.causes[cause.index()].share_of_failed;
    assert_eq!(share(FailureCause::MarketSequence), 0.0);
    assert_eq!(share(FailureCause::Longevity), 0.0);
    assert!(
        share(FailureCause::Overspending) > 0.7,
        "{}",
        share(FailureCause::Overspending)
    );
}

/// Minor: the child-cost tier was chosen from earned income at the start, so a
/// retired couple got the lowest USDA tier whatever it spent (about 16k a year
/// for two children).
#[test]
fn a_retired_household_gets_the_child_cost_tier_of_its_spending() {
    let mut d = retired_couple_on_the_marketplace();
    set(&mut d, "/household/people/0/birth", json!("1981-09"));
    set(&mut d, "/household/people/1/birth", json!("1981-04"));
    set(
        &mut d,
        "/household/children",
        json!([
            {"id": "a", "name": "a", "birth": "2018-06", "costs": "modeled"},
            {"id": "b", "name": "b", "birth": "2016-06", "costs": "modeled"},
        ]),
    );
    let t = trace(&d, 1);
    let children = annual(&t, "spend_children");
    let i = t.annual.years.iter().position(|y| *y == 2027).unwrap();
    // The lowest tier gives about 16k for these two; this household's budget puts
    // it in the middle tier.
    assert!(
        children[i] > 19_000.0,
        "child costs in 2027: {:.0}",
        children[i]
    );
}

/// The additive outputs asked for by the result views: annual series of the picked
/// lives, planned spending by year as quantiles, the capped share of plan
/// delivered, depletion by year with its depth, the plan's moments, and the
/// quantile levels of a comparison.
#[test]
fn the_summary_carries_the_additive_outputs() {
    let d = example("dual-income-family");
    let plan = compile(&profile(&d));
    let acc = run(&plan, SEED, 0..600);
    let s = plan.summarize_seeded(&acc, SEED);
    let years = s.axis.years.len();

    // (a) every pick has its annual series, and they are that path's own.
    assert!(!s.picks.is_empty());
    for pick in &s.picks {
        let path = s
            .pick_paths
            .iter()
            .find(|p| p.path_index == pick.path_index)
            .unwrap_or_else(|| panic!("no series for pick {}", pick.label));
        assert_eq!(path.net_worth.len(), path.spending.len());
        assert_eq!(path.net_worth.len(), path.investable.len());
        assert!(!path.net_worth.is_empty() && path.net_worth.len() <= years);
        if pick.label == "median" {
            let last = *path.net_worth.last().unwrap() as f64;
            assert!((last - pick.value).abs() <= 1.0 + 1e-6 * pick.value.abs());
        }
    }
    // With the wrong seed the paths do not reproduce and are left out.
    assert!(plan.summarize_seeded(&acc, SEED + 1).pick_paths.is_empty());
    // A sample path and a pick of the same index agree.
    let sample = &s.samples[0];
    let mut rec = firesight_core::stats::SampleRecorder::default();
    sim::run_path(
        &plan,
        SEED,
        sample.path_index,
        &mut sim::Workspace::new(&plan),
        &mut rec,
    );
    assert_eq!(&rec.sample, sample);

    // (b) planned spending by year, as quantiles and as a density, like spending.
    let planned = s
        .quantiles
        .iter()
        .find(|q| q.series.key() == "spend_planned")
        .expect("spend_planned quantiles");
    let spending = s
        .quantiles
        .iter()
        .find(|q| q.series.key() == "spending")
        .unwrap();
    assert_eq!(planned.values.len(), spending.values.len());
    // While everyone works, planned and actual spending are the same thing.
    assert_eq!(planned.values[4][1], spending.values[4][1]);
    assert!(s.density.iter().any(|g| g.series.key() == "spend_planned"));

    // (c) the capped share of the plan delivered.
    let share = s.headline.plan_delivered_share.value;
    assert!(share > 0.8 && share <= 1.0, "{share}");
    assert!(s.headline.delivered_spending_ratio.value <= 1.0);

    // (d) depletion by year: a share of the living, and how deep it is.
    let by_year = &s.spending.by_year;
    assert_eq!(by_year.depleted.len(), years);
    assert_eq!(by_year.depleted_shortfall.len(), years);
    assert_eq!(by_year.depleted[0], 0.0);
    assert!(by_year.depleted_shortfall[0].is_nan());
    let mut seen = false;
    for y in 0..years {
        let (p, depth) = (by_year.depleted[y], by_year.depleted_shortfall[y]);
        if p > 0.0 {
            seen = true;
            assert!(depth >= 0.0, "year {y}: {depth}");
        }
    }
    assert!(seen, "no depleted year in 600 paths");

    // (e) the plan's moments.
    let m = &s.plan_moments;
    assert_eq!(m.social_security_claim_age.len(), 2);
    for age in &m.social_security_claim_age {
        assert!((66.9..67.3).contains(age), "claim age {age}");
    }
    assert!(m.social_security_claim_share.iter().all(|p| *p > 0.8));
    assert_eq!(m.person_retirement_age.len(), 2);
    assert!((m.person_retirement_age[0] - s.headline.median_retirement_age).abs() < 1.5);
    // The home is bought in 2028 on a 30-year loan.
    assert!(
        (2040.0..=2062.0).contains(&m.mortgage_payoff_year),
        "{}",
        m.mortgage_payoff_year
    );
    assert!(m.mortgage_payoff_share > 0.5);
    assert!(
        (m.mortgage_payoff_age - s.axis.start_age - (m.mortgage_payoff_year - 2026.0)).abs() < 1.5
    );

    // (f) a comparison names its quantile levels.
    let c = firesight_core::stats::compare(&acc, &acc);
    assert_eq!(c.quantile_levels.len(), 9);
    assert_eq!(c.quantile_levels.len(), c.scalars[0].quantiles.len());
    assert_eq!(c.quantile_levels[4], 0.5);
}

// ---------------------------------------------------------------------------
// Private-company equity: tax at the event, the instalments, the linked job
// ---------------------------------------------------------------------------

/// The startup employee with one fully vested RSU holding of `units` at a $10
/// preferred price and the company acquired at three times that in month 6 (May
/// 2027): a wage-type payout of `30 * units`, with everything else quiet.
fn payout_in_month_six(units: f64) -> Profile {
    let mut p = startup_with(certain(OutcomeKind::Acquired, 0.5, 0.5, 3.0));
    let co = &mut p.private_equity[0];
    co.preferred_price = 10.0;
    co.fmv_409a = Some(4.0);
    co.holdings = vec![EquityHolding {
        id: "rsu".into(),
        kind: HoldingKind::Rsu {
            must_be_employed_at_event: false,
        },
        shares: units,
        vested: units,
        ..EquityHolding::default()
    }];
    p
}

/// The same household with the company counted at zero: the same job, the same
/// fate, no money.
fn without_equity(p: &Profile) -> Profile {
    let mut zero = p.clone();
    for co in &mut zero.private_equity {
        co.included = false;
    }
    zero
}

/// A monthly column in nominal dollars (flows are deflated by the price level at
/// the start of their month).
fn nominal(t: &PathTrace, key: &str) -> Vec<f64> {
    let price = t.column("price_level").unwrap();
    column(t, key)
        .iter()
        .enumerate()
        .map(|(i, v)| v * if i == 0 { 1.0 } else { price[i - 1] })
        .collect()
}

/// All tax paid each month, nominal.
fn tax_paid(t: &PathTrace) -> Vec<f64> {
    let mut total = vec![0.0; t.months as usize];
    for key in ["tax_federal", "tax_state", "tax_payroll", "tax_penalties"] {
        for (sum, v) in total.iter_mut().zip(nominal(t, key)) {
            *sum += v;
        }
    }
    total
}

fn return_of(t: &PathTrace, year: i32) -> &serde_json::Value {
    &t.tax_returns
        .iter()
        .find(|r| r.year == year)
        .unwrap_or_else(|| panic!("no return for {year}"))
        .result
}

fn amount(result: &Value, key: &str) -> f64 {
    result[key]
        .as_f64()
        .unwrap_or_else(|| panic!("no {key} in the return"))
}

/// Supplemental wages are withheld at 22% up to $1,000,000 in the year and 37%
/// above it, with payroll tax on top, in the month they are paid.
#[test]
fn supplemental_withholding_is_22_then_37_above_a_million() {
    let p = payout_in_month_six(100_000.0);
    let with = sim::trace_path(&compile(&p), SEED, 0);
    let zero = sim::trace_path(&compile(&without_equity(&p)), SEED, 0);
    let acquired: Vec<_> = with
        .events
        .iter()
        .filter(|e| e.kind.key() == "company_acquired")
        .collect();
    assert_eq!(acquired.len(), 1);
    assert_eq!(acquired[0].month - START, 6);
    assert!(
        acquired[0].label.contains("acquired at $30.00 a share"),
        "{}",
        acquired[0].label
    );
    let payout = nominal(&with, "income_equity")[6];
    assert!((payout - 3_000_000.0).abs() < 40.0, "payout {payout}");
    let federal = nominal(&with, "tax_federal")[6] - nominal(&zero, "tax_federal")[6];
    let expected = 0.22 * 1_000_000.0 + 0.37 * 2_000_000.0;
    assert!(
        (federal - expected).abs() < 40.0,
        "withheld {federal:.0}, expected {expected:.0}"
    );
    // Medicare on all of it, Social Security up to what is left of the wage base,
    // and the 0.9% Additional Medicare tax on the pay that takes the year's wages
    // over $200,000 (the salary so far has used part of that).
    let payroll = nominal(&with, "tax_payroll")[6] - nominal(&zero, "tax_payroll")[6];
    let additional = 0.009 * 2_800_000.0..=0.009 * 3_000_000.0;
    assert!(
        (43_500.0 + additional.start() - 40.0..43_500.0 + additional.end() + 0.062 * 200_000.0)
            .contains(&payroll),
        "payroll tax at the event {payroll:.0}"
    );
    // Nothing reaches the company counted at zero.
    assert!(zero.column("income_equity").is_none());
}

/// A payout is taxed at the event and through level instalments over the rest of
/// the year; the cash for them is neither invested nor sold back, and little is
/// left for April. Two sizes: one whose remaining tax is small beside the pay still
/// to come that year, and one many times larger.
#[test]
fn an_equity_payout_is_taxed_before_it_is_invested() {
    for units in [5_000.0, 100_000.0] {
        let p = payout_in_month_six(units);
        let (plan, plan_zero) = (compile(&p), compile(&without_equity(&p)));
        for path in 0..3 {
            let with = sim::trace_path(&plan, SEED, path);
            let zero = sim::trace_path(&plan_zero, SEED, path);
            let wages = 30.0 * units;
            let at_event = nominal(&with, "tax_federal")[6] - nominal(&zero, "tax_federal")[6];
            assert!(
                at_event >= 0.22 * wages - 40.0,
                "{units}: {at_event:.0} withheld on {wages:.0}"
            );
            // The year's tax the equity caused, and what the event month paid of it.
            let (ra, rz) = (return_of(&with, 2027), return_of(&zero, 2027));
            let equity_tax = amount(ra, "total") - amount(rz, "total");
            let (paid, paid_zero) = (tax_paid(&with), tax_paid(&zero));
            let event_month = paid[6] - paid_zero[6];
            // June to December: seven level instalments carry the rest.
            let extra: Vec<f64> = (7..=13).map(|t| paid[t] - paid_zero[t]).collect();
            let rest: f64 = extra.iter().sum();
            assert!(
                (rest - (equity_tax - event_month)).abs() <= 0.05 * equity_tax.abs() + 200.0,
                "{units} path {path}: instalments {rest:.0}, equity tax {equity_tax:.0} less {event_month:.0} at the event"
            );
            let mean = rest / 7.0;
            for (i, x) in extra.iter().enumerate() {
                // (The $1,100 is the Social Security tax on a month's pay, which
                // one of the two households stops paying once past the wage base.)
                assert!(
                    (x - mean).abs() <= 0.05 * mean.abs() + 1_100.0,
                    "{units} path {path}: instalment {i} is {x:.0} against a mean of {mean:.0}"
                );
            }
            // The money for them stays in cash: none of it is invested and sold
            // back, and nothing is withdrawn before the next April.
            let price = with.column("price_level").unwrap();
            let cash = column(&with, "balance_cash");
            for t in 7..=13 {
                let still_owed: f64 = (t + 1..=13).map(|u| paid[u] - paid_zero[u]).sum();
                assert!(
                    cash[t] * price[t] >= 0.97 * still_owed - 200.0,
                    "{units} path {path} month {t}: cash {:.0}, instalments to come {still_owed:.0}",
                    cash[t] * price[t]
                );
            }
            for key in ["withdraw_taxable", "withdraw_traditional", "withdraw_roth"] {
                let drawn: f64 = column(&with, key)[6..=17].iter().sum();
                assert!(drawn < 1.0, "{units} path {path}: {key} {drawn:.0}");
            }
            // The balance settled the next April is small beside the year's tax,
            // once the 0.9% Additional Medicare Tax is set aside: the engine settles
            // that with the return for every kind of pay.
            let settled = |t: &PathTrace, paid: &[f64]| paid[17] - nominal(t, "income_other")[17];
            let april = settled(&with, &paid) - settled(&zero, &paid_zero);
            let total = amount(ra, "total");
            let additional_medicare = 0.009 * (wages + 190_000.0 - 200_000.0).max(0.0);
            assert!(
                (april - additional_medicare).abs() <= 0.02 * total,
                "{units} path {path}: {april:.0} settled in April ({additional_medicare:.0} of it Additional Medicare Tax) against a return of {total:.0}"
            );
        }
    }
}

/// A one-off amount in a year that starts part-way through is taxed once, at the
/// year's annual rate, not multiplied up with the regular income. Incentive options
/// exercised in the first month of a plan that starts in November owe about the
/// minimum tax they would owe in a plan that starts in January.
#[test]
fn a_partial_first_year_does_not_multiply_a_one_off() {
    let mut p = startup_with(certain(OutcomeKind::StillPrivate, 0.0, 0.0, 1.0));
    p.accounts
        .push(account("more", 0, AccountKind::Cash, 1_500_000.0));
    {
        let co = &mut p.private_equity[0];
        co.holdings.truncate(1);
        co.holdings[0].vested = co.holdings[0].shares;
        co.holdings[0].vest_months_left = 0;
        co.policy.exercise = ExerciseTiming::VestedNow;
    }
    let mut january = p.clone();
    january.start = Some(YearMonth::ym(2027, 1));
    let november = sim::trace_path(&compile(&p), SEED, 0);
    let january = sim::trace_path(&compile(&january), SEED, 0);
    for t in [&november, &january] {
        let exercised: Vec<_> = t
            .events
            .iter()
            .filter(|e| e.kind.key() == "options_exercised")
            .collect();
        assert_eq!(exercised.len(), 1);
        assert_eq!(exercised[0].month, t.start);
        assert!(
            exercised[0]
                .label
                .starts_with("Exercised 60,000 Northwind Robotics options for $90,000"),
            "{}",
            exercised[0].label
        );
    }
    // 60,000 options, strike $1.50, 409A value $5: an adjustment of $210,000.
    let (first, full) = (
        amount(&november.tax_returns[0].result, "amt"),
        amount(&january.tax_returns[0].result, "amt"),
    );
    assert_eq!(
        (november.tax_returns[0].year, january.tax_returns[0].year),
        (2026, 2027)
    );
    assert!(full > 20_000.0, "minimum tax in a full year: {full:.0}");
    assert!(
        (first / full - 1.0).abs() < 0.05,
        "minimum tax {first:.0} in the two-month year against {full:.0} in a full one"
    );
    // And it is charged once and in full: against the same household without the
    // options, the tax paid for 2026 (two months, then the balance in April) is
    // higher by the minimum tax.
    let zero = sim::trace_path(&compile(&without_equity(&p)), SEED, 0);
    let net_paid = |t: &PathTrace| -> f64 {
        tax_paid(t)[..=5].iter().sum::<f64>() - nominal(t, "income_other")[..=5].iter().sum::<f64>()
    };
    let charged = net_paid(&november) - net_paid(&zero);
    assert!(
        (charged / first - 1.0).abs() < 0.12,
        "{charged:.0} more tax paid through April against a minimum tax of {first:.0}"
    );
}

/// A company's failure lays its employee off in the same month (the trace shows
/// the failure first), and the usual spell and re-hire follow. With
/// `company_failure_ends_job` off the job carries on.
#[test]
fn a_company_failure_lays_off_the_linked_job() {
    let p = startup_with(certain(OutcomeKind::Fails, 1.5, 1.5, 1.0));
    let t = sim::trace_path(&compile(&p), SEED, 2);
    assert_eq!(event_months(&t, "company_failed"), vec![18]);
    assert_eq!(event_months(&t, "laid_off"), vec![18]);
    let position = |kind: &str| t.events.iter().position(|e| e.kind.key() == kind).unwrap();
    assert!(position("company_failed") < position("laid_off"));
    let back = event_months(&t, "rehired");
    assert!(back.first().is_some_and(|m| *m >= 18), "{back:?}");
    // What was unvested is gone with the company, and no tax or cash moved.
    assert!(t.events.iter().any(|e| e.label.contains("shuts down")));
    assert_eq!(column(&t, "income_equity").iter().sum::<f64>(), 0.0);
    let wages = column(&t, "income_wages");
    assert!(wages[17] > 10_000.0 && wages[18] < wages[17]);

    let mut stays = p.clone();
    stays.private_equity[0].company_failure_ends_job = false;
    let t = sim::trace_path(&compile(&stays), SEED, 2);
    assert_eq!(event_months(&t, "company_failed"), vec![18]);
    assert!(event_months(&t, "laid_off").is_empty());
    let wages = column(&t, "income_wages");
    assert!((wages[18] - wages[17]).abs() < 200.0);
}

/// The "AMT trap": incentive options exercised and held in a company that then
/// fails. The minimum tax is paid in the year of exercise; the credit comes back
/// only as fast as ordinary years allow, and most of it is still unused ten years
/// on. The shares' cost is a capital loss.
#[test]
fn exercising_and_failing_leaves_the_amt_credit_stranded() {
    let mut p = startup_with(certain(OutcomeKind::Fails, 3.0, 3.0, 1.0));
    p.accounts
        .push(account("more", 0, AccountKind::Cash, 2_000_000.0));
    {
        let co = &mut p.private_equity[0];
        co.fmv_409a = Some(12.0);
        co.holdings.truncate(1);
        co.holdings[0].shares = 200_000.0;
        co.holdings[0].vested = 200_000.0;
        co.holdings[0].vest_months_left = 0;
        co.policy.exercise = ExerciseTiming::VestedNow;
    }
    let t = sim::trace_path(&compile(&p), SEED, 1);
    let first = &t.tax_returns[0].result;
    let amt = amount(first, "amt");
    // A spread of $2,100,000: roughly 28% of it.
    assert!(
        (480_000.0..700_000.0).contains(&amt),
        "minimum tax in the year of exercise: {amt:.0}"
    );
    assert!((amount(first, "amt_credit_carryforward") - amt).abs() < 1.0);
    let failed = t
        .events
        .iter()
        .find(|e| e.kind.key() == "company_failed")
        .expect("the company fails");
    assert_eq!(failed.month - START, 36);
    assert!(
        failed.label.contains("paid for its shares is lost"),
        "{}",
        failed.label
    );
    let (mut used_total, mut years_used) = (0.0, 0);
    for r in &t.tax_returns[1..11] {
        let used = amount(&r.result, "amt_credit_used");
        assert!(used >= 0.0 && used < 0.06 * amt, "{}: {used:.0}", r.year);
        used_total += used;
        years_used += u32::from(used > 0.0);
    }
    assert!(years_used >= 5, "credit used in {years_used} of ten years");
    assert!(
        used_total < 0.3 * amt,
        "{used_total:.0} of {amt:.0} came back in ten years"
    );
    let left = amount(&t.tax_returns[10].result, "amt_credit_carryforward");
    assert!(left > 0.7 * amt, "{left:.0} of {amt:.0} still unused");
    // The strike paid is a loss in the year of the failure, used $3,000 a year.
    let loss = amount(return_of(&t, 2029), "capital_loss_carryforward");
    assert!(
        (250_000.0..300_000.0).contains(&loss),
        "loss carried out of 2029: {loss:.0}"
    );
}

/// Exercising incentive options early and holding the shares past both holding
/// periods turns ordinary income into long-term gain, and the minimum tax paid at
/// exercise comes back when the shares are sold. On the same fate, less tax is
/// paid over the life than by waiting and selling at the listing.
#[test]
fn a_qualifying_iso_sale_beats_a_disqualifying_one() {
    let mut early = startup_with(certain(OutcomeKind::Ipo, 2.5, 2.5, 3.0));
    early
        .accounts
        .push(account("more", 0, AccountKind::Cash, 600_000.0));
    {
        let co = &mut early.private_equity[0];
        co.holdings.truncate(1);
        co.holdings[0].vested = co.holdings[0].shares;
        co.holdings[0].vest_months_left = 0;
        co.policy.sell_down = SellDown::AllAtLockupEnd;
        co.policy.exercise = ExerciseTiming::VestedNow;
    }
    let mut late = early.clone();
    late.private_equity[0].policy.exercise = ExerciseTiming::AtLiquidity;
    let (plan_early, plan_late) = (compile(&early), compile(&late));
    for path in 0..5 {
        let a = sim::trace_path(&plan_early, SEED, path);
        let b = sim::trace_path(&plan_late, SEED, path);
        // The same listing and the same price at the end of the lockup.
        let sale = |t: &PathTrace| {
            let e = t
                .events
                .iter()
                .find(|e| e.kind.key() == "lockup_sale")
                .expect("a sale when the lockup ends");
            (e.month, e.label.split(" for ").next().unwrap().to_string())
        };
        assert_eq!(
            event_months(&a, "company_ipo"),
            event_months(&b, "company_ipo")
        );
        assert_eq!(sale(&a), sale(&b), "path {path}");
        let year = sale(&a).0.year();
        // Sold after more than a year: all long-term gain, no ordinary income, and
        // the minimum tax of the exercise year is credited back.
        let (ra, rb) = (return_of(&a, year), return_of(&b, year));
        assert!(amount(&a.tax_returns[0].result, "amt") > 20_000.0);
        assert!(amount(ra, "amt_credit_used") > 0.0, "path {path}");
        assert_eq!(amount(rb, "amt_credit_used"), 0.0);
        let window = |t: &PathTrace| -> f64 {
            // Tax paid from the start to the April after the sale, today's dollars.
            let end = ((sale(t).0 - START) as usize + 18).min(t.months as usize);
            ["tax_federal", "tax_state", "tax_payroll"]
                .iter()
                .map(|k| column(t, k)[..end].iter().sum::<f64>())
                .sum()
        };
        assert!(
            window(&a) < window(&b),
            "path {path}: early exercise paid {:.0}, waiting paid {:.0}",
            window(&a),
            window(&b)
        );
    }
}

/// Small exercises month after month are not re-plan events, yet their tax must
/// not wait for April: the year is planned again each quarter, most of the year's
/// minimum tax is paid in instalments, and the cash for them is kept.
#[test]
fn exercises_as_they_vest_reach_the_instalments() {
    let mut p = startup_with(certain(OutcomeKind::StillPrivate, 0.0, 0.0, 1.0));
    p.start = Some(YearMonth::ym(2027, 1));
    p.accounts.push(Account {
        cost_basis: Some(1_500_000.0),
        ..account("more", 0, AccountKind::Taxable, 1_500_000.0)
    });
    {
        let co = &mut p.private_equity[0];
        co.holdings.truncate(1);
        let h = &mut co.holdings[0];
        h.kind = HoldingKind::Iso {
            strike: 1.0,
            granted_months_ago: 12,
        };
        h.shares = 120_000.0;
        h.vested = 0.0;
        h.vest_months_left = 12;
        co.policy.exercise = ExerciseTiming::AsTheyVest;
    }
    let with = sim::trace_path(&compile(&p), SEED, 0);
    let exercised = with
        .events
        .iter()
        .filter(|e| e.kind.key() == "options_exercised")
        .count();
    assert_eq!(exercised, 12, "one exercise a month for a year");
    // 10,000 options a month at a $4 spread: $480,000 of adjustment in the year.
    let result = &with.tax_returns[0].result;
    let amt = amount(result, "amt");
    assert!(amt > 60_000.0, "minimum tax for the year: {amt:.0}");
    let federal = nominal(&with, "tax_federal");
    // Each quarter's plan sees more of the year's exercises and raises the
    // instalment: April over March, July over June, October over September.
    for month in [3, 6, 9] {
        assert!(
            federal[month] > federal[month - 1] + 500.0,
            "no re-plan in month {month}: {:?}",
            &federal[..12]
        );
    }
    // What is left for April is the tax on the exercises after the last plan (two
    // months of twelve, at the top of the exemption phase-out, about 28% of the
    // year's minimum tax) and the usual error of a first year's estimate of
    // investment income, not the year's minimum tax.
    let during: f64 = tax_paid(&with)[..12].iter().sum();
    let balance = amount(result, "total") - during;
    assert!(
        balance < 0.40 * amt,
        "{balance:.0} left for April of a minimum tax of {amt:.0}"
    );
    // The cash for the instalments still to come under the plan in force (this
    // month's instalment for each month left in the year) is there at each month
    // end, and the strike was paid from taxable savings, never from a retirement
    // account.
    let price = with.column("price_level").unwrap();
    let cash = column(&with, "balance_cash");
    let state = nominal(&with, "tax_state");
    let regular = federal[1] + state[1];
    for t in 3..11 {
        let instalment = (federal[t] + state[t] - regular).max(0.0);
        let still_owed = instalment * (11 - t) as f64;
        assert!(
            cash[t] * price[t] >= 0.9 * still_owed - 500.0,
            "month {t}: cash {:.0}, instalments to come {still_owed:.0}",
            cash[t] * price[t]
        );
    }
    assert!(column(&with, "withdraw_taxable")[..12].iter().sum::<f64>() > 100_000.0);
    for key in ["withdraw_traditional", "withdraw_roth", "withdraw_hsa"] {
        assert!(column(&with, key)[..16].iter().sum::<f64>() < 1.0, "{key}");
    }
}
