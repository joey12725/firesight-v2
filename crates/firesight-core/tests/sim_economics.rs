//! The whole stack against things that can be checked from outside it.

mod common;

use common::*;
use firesight_core::model::*;
use firesight_core::sim;
use firesight_core::stats::{PathScalar, TraceEventKind};

/// A salary-and-savings household with no tax, no risk and a single asset (Treasury
/// bills), against the spreadsheet recurrence
///
/// ```text
/// balance(t) = (balance(t-1) + (salary - spending) / 12 * price(t)) * (1 + bill return(t))
/// ```
///
/// using the path's own price level and bill returns.
#[test]
fn a_riskless_saver_matches_the_spreadsheet() {
    let mut p = quiet(vec![person("saver", 30, Sex::Female)], 90.0);
    p.incomes = vec![job("job", 0, 96_000.0)];
    p.spending.items = vec![
        item("rent", Category::Rent, 2_000.0, true),
        item("fun", Category::Entertainment, 1_000.0, false),
    ];
    // Everything in Treasury bills; rent is given no real growth.
    p.spending.items[0].real_growth = Some(0.0);
    p.portfolio.target = Allocation::single(AssetClass::Cash);
    p.accounts = vec![Account {
        cost_basis: Some(50_000.0),
        ..account("bills", 0, AccountKind::Taxable, 50_000.0)
    }];
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 65.0,
    };
    let plan = compile(&p);
    for path in 0..5 {
        let trace = sim::trace_path(&plan, 21, path);
        let price = trace.column("price_level").unwrap();
        let bills = trace.column("return_cash").unwrap();
        let investable = trace.column("investable").unwrap();
        let wages = trace.column("income_wages").unwrap();
        let mut balance = 50_000.0;
        let mut start_price = 1.0;
        for t in 0..120 {
            let net = (96_000.0 - 36_000.0) / 12.0 * start_price;
            balance = (balance + net) * (1.0 + bills[t]);
            let expected = balance / price[t];
            // Whole-dollar rounding of the trace, six-decimal rounding of returns.
            let tolerance = 2.0 + expected * 1e-5 * (t + 1) as f64 / 12.0;
            assert!(
                (investable[t] - expected).abs() <= tolerance,
                "path {path} month {t}: engine {} spreadsheet {expected:.2}",
                investable[t]
            );
            assert!((wages[t] - 8_000.0).abs() <= 1.0, "wages {}", wages[t]);
            start_price = price[t];
        }
        // No tax at all in this configuration.
        assert!(trace.column("tax_federal").is_none());
        assert!(trace.column("tax_payroll").is_none());
    }
}

/// Bengen (1994) and the Trinity study: a 4% initial withdrawal, kept constant in
/// real terms for thirty years, from a balanced portfolio of US stocks and
/// intermediate Treasuries survived almost every start date in US history; the
/// retirees of the mid and late 1960s are the ones who ran out. Here: 60/40,
/// rebalanced annually, no tax or fees, each start month from 1926 for which thirty
/// years of data exist.
#[test]
fn the_four_percent_rule_holds_as_it_did_in_history() {
    let profile = with_market(
        retiree(1_000_000.0, 0.6, 40_000.0, 30.0),
        MarketKind::Historical,
    );
    let plan = compile(&profile);
    let cohorts = plan.cohort_count() as u64;
    assert!(cohorts > 1100);
    let complete = cohorts - 360;
    let acc = run_checked(&plan, 1, 0..complete);
    let success = acc.scalar_column(PathScalar::Success);
    let first = firesight_core::data::Data::embedded().history.start;
    let mut failed = Vec::new();
    for (i, s) in success.iter().enumerate() {
        if *s < 0.5 {
            failed.push(first + i as i32);
        }
    }
    let rate = 1.0 - failed.len() as f64 / complete as f64;
    let years: std::collections::BTreeSet<i32> = failed.iter().map(|m| m.year()).collect();
    println!(
        "4% rule, 60/40, 30 years: {:.1}% of {complete} start months succeed; failing start years: {years:?}",
        rate * 100.0
    );
    assert!((0.92..=0.975).contains(&rate), "success rate {rate}");
    assert!(!failed.is_empty());
    for m in &failed {
        let y = m.year();
        assert!(
            (1964..=1969).contains(&y)
                || (1928..=1930).contains(&y)
                || (1936..=1937).contains(&y)
                || (1972..=1973).contains(&y),
            "unexpected failing cohort {m}"
        );
    }
    assert!(years.contains(&1966) && years.contains(&1968));

    // 3% survived everything; 6% failed much more often.
    let run_rate = |annual: f64| {
        let p = with_market(
            retiree(1_000_000.0, 0.6, annual, 30.0),
            MarketKind::Historical,
        );
        let acc = run(&compile(&p), 1, 0..complete);
        let ok = acc
            .scalar_column(PathScalar::Success)
            .iter()
            .filter(|s| **s > 0.5)
            .count();
        ok as f64 / complete as f64
    };
    assert_eq!(run_rate(30_000.0), 1.0);
    let six = run_rate(60_000.0);
    assert!((0.25..0.75).contains(&six), "6% rule: {six}");
}

fn success_and_median(profile: &Profile, paths: u64) -> (f64, f64, f64) {
    let s = summary(profile, 31, paths);
    (
        s.headline.success_rate.value,
        s.headline.median_terminal_net_worth,
        s.headline.median_retirement_age,
    )
}

#[test]
fn spending_more_never_helps_and_retiring_later_never_hurts() {
    // Same seed, same paths: common random numbers make these comparisons exact
    // path by path for the retiree, and nearly so for the working household.
    let mut last = 1.0;
    for annual in [30_000.0, 36_000.0, 42_000.0, 48_000.0, 56_000.0, 70_000.0] {
        let p = retiree(1_000_000.0, 0.6, annual, 35.0);
        let plan = compile(&p);
        let acc = run(&plan, 31, 0..1500);
        let rate = plan.summarize(&acc).headline.success_rate.value;
        assert!(rate <= last + 1e-12, "spending {annual}: {rate} > {last}");
        last = rate;
    }
    assert!(last < 0.5);

    // Path by path: a path that survives at a higher spending level survives at
    // every lower one.
    let lean = run(
        &compile(&retiree(1_000_000.0, 0.6, 40_000.0, 35.0)),
        31,
        0..1500,
    );
    let rich = run(
        &compile(&retiree(1_000_000.0, 0.6, 46_000.0, 35.0)),
        31,
        0..1500,
    );
    let (a, b) = (
        lean.scalar_column(PathScalar::Success),
        rich.scalar_column(PathScalar::Success),
    );
    assert!(a.iter().zip(&b).all(|(lean, rich)| lean >= rich));

    // A working couple: each extra year of work must not lower success materially.
    let mut base = Profile::example();
    let mut previous = 0.0;
    for age in [50.0, 54.0, 58.0, 62.0, 66.0] {
        base.retirement.trigger = RetirementTrigger::AtAge { person: 0, age };
        let (success, _, _) = success_and_median(&base, 1500);
        assert!(
            success >= previous - 0.02,
            "retiring at {age}: success {success} after {previous}"
        );
        previous = success;
    }
    assert!(previous > 0.6);

    // More spending in a working household: success does not rise, FI comes later.
    let base = Profile::example();
    let mut spendy = base.clone();
    for item in &mut spendy.spending.items {
        item.amount *= 1.25;
    }
    let (a, b) = (summary(&base, 31, 1500), summary(&spendy, 31, 1500));
    assert!(b.headline.fi_rate.value <= a.headline.fi_rate.value + 0.01);
    assert!(b.headline.fi_age.p50.is_nan() || b.headline.fi_age.p50 >= a.headline.fi_age.p50);
}

#[test]
fn a_higher_equity_premium_raises_terminal_wealth() {
    let mut last = f64::NEG_INFINITY;
    for adjustment in [-0.03, -0.01, 0.0, 0.02] {
        let mut p = retiree(1_000_000.0, 0.6, 35_000.0, 30.0);
        p.assumptions.market.equity_premium_adjustment = adjustment;
        let (_, median, _) = success_and_median(&p, 1500);
        assert!(median > last, "adjustment {adjustment}: {median} <= {last}");
        last = median;
    }
    // And higher inflation, with real returns preserved, leaves a real plan with no
    // nominal commitments unchanged within noise.
    let base = retiree(1_000_000.0, 0.6, 40_000.0, 30.0);
    let mut hot = base.clone();
    hot.assumptions.market.inflation_adjustment = 0.02;
    let (a, _, _) = success_and_median(&base, 1500);
    let (b, _, _) = success_and_median(&hot, 1500);
    assert!((a - b).abs() < 0.03, "{a} vs {b}");
}

/// Tax is computed once a year on what happened and every dollar of it is paid:
/// over a whole life, the tax series (less refunds booked as income) add up to the
/// sum of the annual returns.
#[test]
fn every_return_is_paid_in_full_by_the_end() {
    let mut p = quiet(vec![person("worker", 55, Sex::Male)], 67.0);
    p.assumptions.taxes = TaxModel::default();
    p.household.location.state = Some("NY".into());
    p.incomes = vec![job("job", 0, 140_000.0)];
    if let IncomeKind::Job(j) = &mut p.incomes[0].kind {
        j.plan = Some(EmployerPlan::default());
        j.bonus = Some(Bonus {
            target_fraction: 0.15,
            volatility: 0.0,
            month: 3,
        });
    }
    p.spending.items = vec![item("living", Category::Other, 5_000.0, true)];
    p.accounts = vec![
        account("cash", 0, AccountKind::Cash, 20_000.0),
        account("taxable", 0, AccountKind::Taxable, 150_000.0),
    ];
    p.portfolio.emergency_months = 3.0;
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 75.0,
    };
    let plan = compile(&p);
    for path in 0..4 {
        let trace = sim::trace_path(&plan, 2, path);
        let price = trace.column("price_level").unwrap();
        let start_price = |t: usize| if t == 0 { 1.0 } else { price[t - 1] };
        let nominal = |key: &str| -> f64 {
            trace.column(key).map_or(0.0, |c| {
                c.iter().enumerate().map(|(t, v)| v * start_price(t)).sum()
            })
        };
        let paid = nominal("tax_federal")
            + nominal("tax_state")
            + nominal("tax_payroll")
            + nominal("tax_penalties")
            - nominal("income_other");
        let mut owed = 0.0;
        assert_eq!(trace.tax_returns.len(), 13, "one return per calendar year");
        for (i, r) in trace.tax_returns.iter().enumerate() {
            let total = r.result["total"].as_f64().unwrap();
            // The first year covers two months and is taxed at its annual rate.
            owed += if i == 0 { total * 2.0 / 12.0 } else { total };
            assert!(total > 0.0);
        }
        // Monthly amounts are rounded to whole dollars in the trace.
        let tolerance = 0.5 * (trace.months as f64) * 4.0 * 0.2 + owed * 1e-4;
        assert!(
            (paid - owed).abs() <= tolerance,
            "path {path}: paid {paid:.0}, returns {owed:.0}"
        );
        // A typical full year: the return is within a few percent of what was
        // prepaid, so April settlements are small.
        assert!(trace
            .events
            .iter()
            .all(|e| e.kind != TraceEventKind::Depleted));
    }
}

/// The same accounting with private-company equity in the plan: incentive options
/// exercised in the first month (minimum tax), a listing, shares sold over two
/// years (the credit comes back), non-qualified options and RSUs settled as wages.
/// Whatever the returns say is owed, with the AMT and the credit in it, is what is
/// paid over the life: at the event, in instalments, or the next April.
#[test]
fn every_equity_return_is_paid_in_full_by_the_end() {
    let mut p = quiet(vec![person("worker", 40, Sex::Female)], 60.0);
    p.start = Some(YearMonth::ym(2027, 1));
    p.assumptions.taxes = TaxModel::default();
    p.household.location.state = Some("CA".into());
    p.incomes = vec![job("job", 0, 180_000.0)];
    p.spending.items = vec![item("living", Category::Other, 6_000.0, true)];
    p.accounts = vec![
        account("cash", 0, AccountKind::Cash, 80_000.0),
        Account {
            cost_basis: Some(300_000.0),
            ..account("taxable", 0, AccountKind::Taxable, 400_000.0)
        },
    ];
    p.portfolio.emergency_months = 3.0;
    p.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 75.0,
    };
    let holding = |id: &str, kind: HoldingKind, shares: f64| EquityHolding {
        id: id.into(),
        kind,
        shares,
        vested: shares,
        ..EquityHolding::default()
    };
    p.private_equity = vec![PrivateCompany {
        id: "co".into(),
        name: "Co".into(),
        job: Some("job".into()),
        preferred_price: 10.0,
        fmv_409a: Some(4.0),
        outcomes: certain(OutcomeKind::Ipo, 3.0, 4.0, 2.0),
        tenders: TenderOffers {
            annual_probability: Some(0.0),
            ..TenderOffers::default()
        },
        holdings: vec![
            holding(
                "iso",
                HoldingKind::Iso {
                    strike: 1.0,
                    granted_months_ago: 30,
                },
                30_000.0,
            ),
            holding(
                "nso",
                HoldingKind::Nso {
                    strike: 2.0,
                    granted_months_ago: 30,
                },
                10_000.0,
            ),
            holding(
                "rsu",
                HoldingKind::Rsu {
                    must_be_employed_at_event: false,
                },
                5_000.0,
            ),
        ],
        policy: EquityPolicy {
            exercise: ExerciseTiming::VestedNow,
            sell_down: SellDown::OverYears { years: 2 },
            ..EquityPolicy::default()
        },
        ..PrivateCompany::default()
    }];
    let plan = compile(&p);
    for path in 0..4 {
        let trace = sim::trace_path(&plan, 2, path);
        let price = trace.column("price_level").unwrap();
        let start_price = |t: usize| if t == 0 { 1.0 } else { price[t - 1] };
        let nominal = |key: &str| -> f64 {
            trace.column(key).map_or(0.0, |c| {
                c.iter().enumerate().map(|(t, v)| v * start_price(t)).sum()
            })
        };
        let paid = nominal("tax_federal")
            + nominal("tax_state")
            + nominal("tax_payroll")
            + nominal("tax_penalties")
            - nominal("income_other");
        // Every year here is a whole calendar year from January on.
        let mut owed = 0.0;
        let (mut amt, mut credit_used) = (0.0, 0.0);
        for r in &trace.tax_returns {
            owed += r.result["total"].as_f64().unwrap();
            amt += r.result["amt"].as_f64().unwrap();
            credit_used += r.result["amt_credit_used"].as_f64().unwrap();
        }
        let tolerance = 0.5 * (trace.months as f64) * 4.0 * 0.2 + owed * 1e-4;
        assert!(
            (paid - owed).abs() <= tolerance,
            "path {path}: paid {paid:.0}, returns {owed:.0}"
        );
        // The equity really went through the returns and the ledger.
        assert!(amt > 5_000.0, "path {path}: minimum tax {amt:.0}");
        assert!(
            credit_used > 0.5 * amt,
            "path {path}: {credit_used:.0} of {amt:.0} credited back"
        );
        let received = nominal("income_equity");
        let strike = nominal("equity_exercise");
        assert!((strike - 50_000.0).abs() < 5.0, "strike paid {strike:.0}");
        assert!(received > 500_000.0, "received {received:.0}");
        let kinds: Vec<&str> = trace.events.iter().map(|e| e.kind.key()).collect();
        for kind in ["options_exercised", "company_ipo", "lockup_sale"] {
            assert!(kinds.contains(&kind), "path {path}: no {kind}");
        }
        assert!(trace
            .events
            .iter()
            .all(|e| e.kind != TraceEventKind::Depleted));
    }
}

/// The four equity amounts of a life are the ledger's own: gross proceeds are the
/// `income_equity` series, exercise cost the `equity_exercise` series, and what is
/// left after both is the tax the equity caused.
#[test]
fn equity_scalars_reconcile_with_the_ledger() {
    let scalar = |trace: &firesight_core::stats::PathTrace, s: PathScalar| -> f64 {
        trace
            .scalars
            .iter()
            .find(|x| x.scalar == s)
            .map_or(f64::NAN, |x| x.value)
    };
    // Scalars are stored in single precision and trace columns in whole dollars
    // a month.
    let close = |a: f64, b: f64, months: u32| {
        (a - b).abs() <= 5.0 + 1e-6 * a.abs() + 0.5 * f64::from(months) * 0.02
    };

    // 1. The demo as shipped, 50 lives.
    let plan = compile(&startup_employee());
    let (mut paid, mut exercised, mut tendered) = (0, 0, 0);
    for path in 0..50 {
        let trace = sim::trace_path(&plan, 5, path);
        let gross = scalar(&trace, PathScalar::EquityGrossProceeds);
        let cost = scalar(&trace, PathScalar::EquityExerciseCost);
        let tender = scalar(&trace, PathScalar::EquityTenderProceeds);
        let net = scalar(&trace, PathScalar::EquityNetProceeds);
        let income = column_sum(&trace, "income_equity", 0..trace.months as usize);
        let strike = column_sum(&trace, "equity_exercise", 0..trace.months as usize);
        assert!(
            close(gross, income, trace.months),
            "path {path}: gross {gross:.0}, ledger {income:.0}"
        );
        assert!(
            close(cost, strike, trace.months),
            "path {path}: cost {cost:.0}, ledger {strike:.0}"
        );
        assert!(tender >= 0.0 && tender <= gross + 1.0, "path {path}");
        // Tender proceeds are the amounts on the tender lines of the life story.
        let tender_lines: f64 = trace
            .events
            .iter()
            .filter(|e| e.kind == TraceEventKind::TenderOffer)
            .filter_map(|e| e.amount)
            .sum();
        assert!(
            (tender - tender_lines).abs() <= 2.0 + 1e-6 * tender,
            "path {path}: tender {tender:.0}, events {tender_lines:.0}"
        );
        // The tax the equity caused. With nothing received and nothing paid it
        // can only be a saving (the basis of the shares held is a capital loss
        // when the company fails), never a bill.
        let tax = gross - cost - net;
        if gross == 0.0 && cost == 0.0 {
            assert!(
                (-15_000.0..=1.0).contains(&tax),
                "path {path}: tax {tax:.0}"
            );
        }
        // With no exercise there is no minimum tax to strand: the tax is a share
        // of what came in, under the top combined rate.
        if cost == 0.0 {
            assert!(
                tax <= 0.62 * gross + 1.0,
                "path {path}: tax {tax:.0} on {gross:.0}"
            );
        }
        assert!(
            tax >= -0.5 * (cost + 15_000.0) - 5.0,
            "path {path}: {tax:.0}"
        );
        paid += i32::from(gross > 0.0);
        exercised += i32::from(cost > 0.0);
        tendered += i32::from(tender > 0.0);
    }
    // About 6% of demo lives sell in a tender offer (most companies that run one
    // are heading up, and a former employee cannot take part), so 50 lives hold a
    // few at most.
    assert!(
        paid >= 10 && exercised >= 3 && tendered >= 1,
        "{paid} {exercised} {tendered}"
    );

    // 2. The tax leg exactly, where it can be recomputed from outside: a flat tax
    // of 30% and a position that is all pay (RSUs settled in cash by an
    // acquisition). Each year's tax with the equity less the tax without it is 30%
    // of that year's payout, in the dollars of the month the year is closed.
    let mut p = startup_with(certain(OutcomeKind::Acquired, 1.5, 2.5, 2.0));
    p.assumptions.taxes.kind = TaxKind::Flat { rate: 0.30 };
    p.private_equity[0]
        .holdings
        .retain(|h| matches!(h.kind, HoldingKind::Rsu { .. }));
    let plan = compile(&p);
    for path in 0..12 {
        let trace = sim::trace_path(&plan, 5, path);
        let gross = scalar(&trace, PathScalar::EquityGrossProceeds);
        let net = scalar(&trace, PathScalar::EquityNetProceeds);
        assert_eq!(scalar(&trace, PathScalar::EquityExerciseCost), 0.0);
        assert!(gross > 100_000.0, "path {path}: {gross:.0}");
        let income = column(&trace, "income_equity");
        let price = trace.column("price_level").unwrap();
        // The price level at the start of month `t` (the one flows are deflated by).
        let start_price = |t: usize| if t == 0 { 1.0 } else { price[t - 1] };
        let months = trace.months as usize;
        let mut tax_real = 0.0;
        let mut t = 0;
        while t < months {
            // One calendar year: its months, and the month it is closed in (the
            // January after it, or the last month of the life).
            let year = (trace.start + t as i32).year();
            let mut nominal = 0.0;
            let mut end = t;
            while end < months && (trace.start + end as i32).year() == year {
                nominal += income[end] * start_price(end);
                end += 1;
            }
            let closed_in = end.min(months - 1);
            tax_real += 0.30 * nominal / start_price(closed_in);
            t = end;
        }
        let caused = gross - net;
        assert!(
            (caused - tax_real).abs() <= 5.0 + 2e-4 * tax_real,
            "path {path}: gross less net {caused:.0}, 30% of each year's payout {tax_real:.0}"
        );
    }
}
