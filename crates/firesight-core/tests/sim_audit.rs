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
