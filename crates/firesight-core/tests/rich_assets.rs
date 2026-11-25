//! The opt-in shapes of the single stock, crypto and the custom asset inside the
//! whole simulation: money is conserved, nothing is NaN, the other subsystems'
//! random numbers do not move, validation catches bad settings, and the effects go
//! the right way. The distributions themselves are tested in
//! `src/economy/shape_tests.rs`.

mod common;

use common::*;
use firesight_core::data::Data;
use firesight_core::economy;
use firesight_core::model::*;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{
    Accumulator, PathEnd, PathScalar, PathTrace, Recorder, TraceEventKind, YearRecord, YearSeries,
};

/// A target mix that holds all three sleeves, and a custom asset to go with it.
fn holding_all_three(mut p: Profile) -> Profile {
    p.portfolio.target = Allocation::from_pairs(&[
        (AssetClass::UsStocks, 35.0),
        (AssetClass::Bonds, 20.0),
        (AssetClass::Crypto, 15.0),
        (AssetClass::SingleStock, 15.0),
        (AssetClass::Custom, 15.0),
    ]);
    p.portfolio.custom_asset = Some(CustomAsset {
        name: "Credit fund".into(),
        mean: 0.08,
        volatility: 0.08,
        equity_corr: 0.3,
        income_yield: 0.06,
        ..CustomAsset::default()
    });
    p
}

/// Every option on at once.
fn all_on(mut p: Profile) -> Profile {
    let pf = &mut p.portfolio;
    pf.single_stock.tails = TailShape::Fat;
    pf.single_stock.ruin_hazard = Some(0.05);
    pf.crypto = CryptoAssumptions {
        outlook: CryptoOutlook::Believer,
        token: CryptoToken::OtherTokens,
        dynamics: CryptoDynamics::cycles(),
    };
    let custom = pf.custom_asset.as_mut().expect("the fixture has one");
    custom.tails = TailShape::Fat;
    custom.loss_event = Some(LossEvent {
        annual_probability: 0.1,
        ..LossEvent::default()
    });
    p
}

/// Forwards to an accumulator after checking that every number is finite and of a
/// sane size.
struct Finite {
    inner: Accumulator,
}

impl Recorder for Finite {
    fn begin_path(&mut self, path_index: u64) {
        self.inner.begin_path(path_index);
    }
    fn record_year(&mut self, year_index: usize, record: &YearRecord) {
        for (s, v) in YearSeries::ALL.iter().zip(record.values) {
            assert!(v.is_finite(), "year {year_index} {}: {v}", s.key());
            assert!(v.abs() < 1e13, "year {year_index} {} = {v}", s.key());
            assert!(
                v >= -1.0 || *s == YearSeries::InvestmentReturn,
                "year {year_index} {} is negative: {v}",
                s.key()
            );
        }
        self.inner.record_year(year_index, record);
    }
    fn end_path(&mut self, end: &PathEnd) {
        for s in PathScalar::ALL {
            assert!(!end.get(*s).is_infinite(), "scalar {}", s.key());
        }
        let dd = end.get(PathScalar::WorstDrawdown);
        assert!((0.0..=1.0).contains(&dd));
        self.inner.end_path(end);
    }
}

fn valid(profile: &Profile) {
    let report = firesight_core::validate::validate(profile);
    assert!(report.errors.is_empty(), "{:#?}", report.errors);
}

#[test]
fn every_option_on_conserves_money_and_stays_finite_under_every_market_kind() {
    for example in Profile::examples(START) {
        for kind in MARKETS {
            let profile = all_on(holding_all_three(with_market(
                example.profile.clone(),
                kind,
            )));
            valid(&profile);
            let plan = compile(&profile);
            // Check mode: the conservation identities are asserted inside the loop.
            let acc = run_checked(&plan, 31, 0..150);
            assert_eq!(plan.summarize(&acc).paths, 150);
            let mut ws = Workspace::new(&plan);
            let mut rec = Finite {
                inner: plan.new_accumulator(),
            };
            for i in 0..400 {
                sim::run_path(&plan, 17, i, &mut ws, &mut rec);
            }
            let summary = plan.summarize(&rec.inner);
            let rate = summary.headline.success_rate.value;
            assert!((0.0..=1.0).contains(&rate), "{} {kind:?}", example.slug);
            assert!(summary.headline.median_terminal_net_worth.is_finite());
        }
    }
}

/// The hardest settings the validator accepts, with the whole portfolio in the
/// three sleeves: the engine still neither panics nor produces a non-finite number.
#[test]
fn the_most_extreme_valid_settings_stay_finite() {
    for kind in MARKETS {
        let mut profile = holding_all_three(with_market(Profile::example(), kind));
        let pf = &mut profile.portfolio;
        pf.target = Allocation::from_pairs(&[
            (AssetClass::Crypto, 1.0),
            (AssetClass::SingleStock, 1.0),
            (AssetClass::Custom, 1.0),
        ]);
        pf.single_stock = SingleStock {
            volatility: 2.0,
            beta: 4.0,
            tails: TailShape::Fat,
            ruin_hazard: Some(0.5),
        };
        pf.crypto = CryptoAssumptions {
            outlook: CryptoOutlook::Believer,
            token: CryptoToken::OtherTokens,
            dynamics: CryptoDynamics::Cycles {
                calm_months: 600.0,
                wild_months: 1.0,
                volatility_ratio: 5.0,
            },
        };
        pf.custom_asset = Some(CustomAsset {
            mean: 0.5,
            volatility: 2.0,
            equity_corr: 0.5,
            tails: TailShape::Fat,
            loss_event: Some(LossEvent {
                annual_probability: 0.5,
                loss: Uncertain::Uniform {
                    low: 0.0,
                    high: 1.0,
                },
                mean_includes_losses: true,
            }),
            ..CustomAsset::default()
        });
        valid(&profile);
        let plan = compile(&profile);
        run_checked(&plan, 5, 0..60);
        let mut ws = Workspace::new(&plan);
        let mut rec = Finite {
            inner: plan.new_accumulator(),
        };
        for i in 0..300 {
            sim::run_path(&plan, 6, i, &mut ws, &mut rec);
        }
    }
}

fn scalar(t: &PathTrace, s: PathScalar) -> f64 {
    t.scalars.iter().find(|x| x.scalar == s).unwrap().value
}

fn career_events(t: &PathTrace) -> Vec<(YearMonth, TraceEventKind)> {
    t.events
        .iter()
        .filter(|e| matches!(e.kind, TraceEventKind::LaidOff | TraceEventKind::Rehired))
        .map(|e| (e.month, e.kind))
        .collect()
}

const SLEEVES: [&str; 3] = ["return_crypto", "return_single_stock", "return_custom"];

/// Switching an option on changes its own sleeve and nothing else random:
/// lifespans, long-term care, layoffs, prices, and every other asset's returns
/// (the other two sleeves included) are the same bits.
#[test]
fn each_option_moves_only_its_own_sleeve() {
    // A fixed retirement date, so that nothing depends on when the household can
    // afford to stop working.
    let mut base = holding_all_three(Profile::example());
    base.retirement.trigger = RetirementTrigger::AtAge {
        person: 0,
        age: 58.0,
    };
    type Change = (&'static str, usize, fn(&mut PortfolioPolicy));
    let changes: [Change; 7] = [
        ("stock tails", 1, |p| p.single_stock.tails = TailShape::Fat),
        ("stock hazard", 1, |p| {
            p.single_stock.ruin_hazard = Some(0.2)
        }),
        ("crypto outlook", 0, |p| {
            p.crypto.outlook = CryptoOutlook::Skeptic
        }),
        ("crypto token", 0, |p| {
            p.crypto.token = CryptoToken::OtherTokens
        }),
        ("crypto cycles", 0, |p| {
            p.crypto.dynamics = CryptoDynamics::cycles()
        }),
        ("custom tails", 2, |p| {
            p.custom_asset.as_mut().unwrap().tails = TailShape::Fat
        }),
        ("custom loss event", 2, |p| {
            p.custom_asset.as_mut().unwrap().loss_event = Some(LossEvent {
                annual_probability: 0.2,
                ..LossEvent::default()
            })
        }),
    ];
    let plan_base = compile(&base);
    for (name, own, change) in changes {
        let mut p = base.clone();
        change(&mut p.portfolio);
        valid(&p);
        let plan = compile(&p);
        let mut changed = 0;
        for path in 0..12 {
            let a = sim::trace_path(&plan_base, 77, path);
            let b = sim::trace_path(&plan, 77, path);
            assert_eq!(a.months, b.months, "{name} path {path}");
            for s in [
                PathScalar::DeathAge0,
                PathScalar::DeathAge1,
                PathScalar::LongTermCareCost,
            ] {
                assert_eq!(
                    scalar(&a, s).to_bits(),
                    scalar(&b, s).to_bits(),
                    "{name} path {path} {s:?}"
                );
            }
            assert_eq!(career_events(&a), career_events(&b), "{name} path {path}");
            for key in [
                "price_level",
                "return_us_stocks",
                "return_intl_stocks",
                "return_bonds",
                "return_cash",
                "return_real_estate",
                "return_gold",
            ] {
                assert!(a.column(key) == b.column(key), "{name} path {path} {key}");
            }
            for (i, key) in SLEEVES.iter().enumerate() {
                let same = a.column(key) == b.column(key);
                if i == own {
                    changed += usize::from(!same);
                } else {
                    assert!(same, "{name} path {path} {key}");
                }
            }
        }
        // Outlooks, tails and cycles change every path; hazards change most.
        assert!(changed >= 8, "{name}: changed on {changed} of 12 paths");
    }
}

fn check_error(change: impl Fn(&mut PortfolioPolicy), code: &str, path: &str) {
    let mut p = holding_all_three(Profile::example());
    change(&mut p.portfolio);
    let report = firesight_core::validate::validate(&p);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.code == code && e.path == path),
        "expected {code} at {path}, got {:#?}",
        report.errors
    );
    assert!(sim::Plan::compile(&p, Data::embedded()).is_err());
}

#[test]
fn bad_settings_are_reported_at_their_paths() {
    valid(&all_on(holding_all_three(Profile::example())));
    for bad in [-0.01, 0.51, f64::NAN, f64::INFINITY] {
        check_error(
            |p| p.single_stock.ruin_hazard = Some(bad),
            "out_of_range",
            "/portfolio/single_stock/ruin_hazard",
        );
        check_error(
            |p| {
                p.custom_asset.as_mut().unwrap().loss_event = Some(LossEvent {
                    annual_probability: bad,
                    ..LossEvent::default()
                })
            },
            "out_of_range",
            "/portfolio/custom_asset/loss_event/annual_probability",
        );
    }
    let cycles = |calm_months, wild_months, volatility_ratio| CryptoDynamics::Cycles {
        calm_months,
        wild_months,
        volatility_ratio,
    };
    for bad in [0.5, 601.0, f64::NAN] {
        check_error(
            |p| p.crypto.dynamics = cycles(bad, 12.0, 2.5),
            "out_of_range",
            "/portfolio/crypto/dynamics/calm_months",
        );
        check_error(
            |p| p.crypto.dynamics = cycles(30.0, bad, 2.5),
            "out_of_range",
            "/portfolio/crypto/dynamics/wild_months",
        );
    }
    for bad in [0.99, 5.01, f64::NAN] {
        check_error(
            |p| p.crypto.dynamics = cycles(30.0, 12.0, bad),
            "out_of_range",
            "/portfolio/crypto/dynamics/volatility_ratio",
        );
    }
    let loss = |loss| {
        move |p: &mut PortfolioPolicy| {
            p.custom_asset.as_mut().unwrap().loss_event = Some(LossEvent {
                loss,
                ..LossEvent::default()
            })
        }
    };
    // A loss above 100%, below zero, or with its limits out of order.
    check_error(
        loss(Uncertain::Uniform {
            low: 0.5,
            high: 1.2,
        }),
        "out_of_range",
        "/portfolio/custom_asset/loss_event/loss/high",
    );
    check_error(
        loss(Uncertain::Normal { mean: 0.1, sd: 0.1 }),
        "out_of_range",
        "/portfolio/custom_asset/loss_event/loss/sd",
    );
    check_error(
        loss(Uncertain::Triangular {
            low: 0.5,
            mode: 0.2,
            high: 0.8,
        }),
        "dist_order",
        "/portfolio/custom_asset/loss_event/loss/mode",
    );
    // The edges are accepted.
    let mut p = holding_all_three(Profile::example());
    p.portfolio.single_stock.ruin_hazard = Some(0.0);
    p.portfolio.crypto.dynamics = cycles(1.0, 600.0, 1.0);
    p.portfolio.custom_asset.as_mut().unwrap().loss_event = Some(LossEvent {
        annual_probability: 0.5,
        loss: Uncertain::point(1.0),
        mean_includes_losses: false,
    });
    valid(&p);
    compile(&p);
}

/// The new fields read and write as the schema says, and an empty object is the
/// default.
#[test]
fn new_fields_round_trip_through_json() {
    let p = all_on(holding_all_three(Profile::example()));
    let back = Profile::from_json(&p.to_json_pretty()).unwrap();
    assert_eq!(p, back);
    let text = serde_json::json!({
        "portfolio": {
            "single_stock": { "tails": "fat", "ruin_hazard": 0.02 },
            "crypto": { "outlook": "skeptic", "token": "other_tokens", "dynamics": { "type": "cycles" } },
            "custom_asset": { "tails": "fat", "loss_event": {} }
        }
    })
    .to_string();
    let p = Profile::from_json(&text).unwrap();
    assert_eq!(p.portfolio.single_stock.tails, TailShape::Fat);
    assert_eq!(p.portfolio.single_stock.ruin_hazard, Some(0.02));
    assert_eq!(p.portfolio.crypto.outlook, CryptoOutlook::Skeptic);
    assert_eq!(p.portfolio.crypto.token, CryptoToken::OtherTokens);
    assert_eq!(p.portfolio.crypto.dynamics, CryptoDynamics::cycles());
    let custom = p.portfolio.custom_asset.unwrap();
    assert_eq!(custom.tails, TailShape::Fat);
    assert_eq!(custom.loss_event, Some(LossEvent::default()));
    // Unknown keys are refused.
    for bad in [
        r#"{"portfolio": {"crypto": {"mood": "bullish"}}}"#,
        r#"{"portfolio": {"crypto": {"dynamics": {"type": "cycles", "months": 3}}}}"#,
        r#"{"portfolio": {"custom_asset": {"loss_event": {"size": 0.3}}}}"#,
        r#"{"portfolio": {"single_stock": {"tails": "thick"}}}"#,
    ] {
        assert!(Profile::from_json(bad).is_err(), "{bad}");
    }
    // The defaults are all "off".
    let d = PortfolioPolicy::default();
    assert_eq!(d.single_stock.tails, TailShape::Normal);
    assert_eq!(d.single_stock.ruin_hazard, None);
    assert_eq!(d.crypto, CryptoAssumptions::default());
    assert_eq!(d.crypto.outlook, CryptoOutlook::Neutral);
    assert_eq!(d.crypto.token, CryptoToken::BitcoinOrEther);
    assert_eq!(d.crypto.dynamics, CryptoDynamics::Steady);
    let c = CustomAsset::default();
    assert!(c.tails == TailShape::Normal && c.loss_event.is_none());
}

/// A retiree drawing 4% of a portfolio with `share` of it in one sleeve and the
/// rest 60/40: `(success rate, sorted terminal net worth)`.
fn retiree_stats(
    asset: AssetClass,
    share: f64,
    change: &dyn Fn(&mut PortfolioPolicy),
    paths: u64,
) -> (f64, Vec<f64>) {
    let mut p = retiree(1_000_000.0, 0.6, 40_000.0, 30.0);
    // The default market model, cautious equity adjustment included.
    p.assumptions.market = MarketModel::default();
    p.portfolio.target = Allocation::from_pairs(&[
        (AssetClass::UsStocks, 0.6 * (1.0 - share)),
        (AssetClass::Bonds, 0.4 * (1.0 - share)),
        (asset, share),
    ]);
    p.portfolio.custom_asset = Some(CustomAsset {
        mean: 0.08,
        volatility: 0.06,
        equity_corr: 0.3,
        ..CustomAsset::default()
    });
    change(&mut p.portfolio);
    valid(&p);
    let plan = compile(&p);
    let acc = run(&plan, 2026, 0..paths);
    let summary = plan.summarize(&acc);
    let mut worth: Vec<f64> = acc
        .scalar_column(PathScalar::TerminalNetWorth)
        .into_iter()
        .map(f64::from)
        .collect();
    worth.sort_by(f64::total_cmp);
    (summary.headline.success_rate.value, worth)
}

fn q(v: &[f64], p: f64) -> f64 {
    v[((v.len() - 1) as f64 * p) as usize]
}

#[test]
fn effects_go_the_right_way_for_a_retiree() {
    let paths = 3000;
    // Crypto, a fifth of the portfolio: a better outlook means more money at
    // every level and more plans surviving; riskier tokens mean fewer.
    let crypto = |f: fn(&mut PortfolioPolicy)| retiree_stats(AssetClass::Crypto, 0.2, &f, paths);
    let (neutral_rate, neutral) = crypto(|_| {});
    let (skeptic_rate, skeptic) = crypto(|p| p.crypto.outlook = CryptoOutlook::Skeptic);
    let (believer_rate, believer) = crypto(|p| p.crypto.outlook = CryptoOutlook::Believer);
    assert!(
        skeptic_rate < neutral_rate && neutral_rate < believer_rate,
        "{skeptic_rate} {neutral_rate} {believer_rate}"
    );
    for p in [0.25, 0.5, 0.75] {
        assert!(q(&skeptic, p) <= q(&neutral, p) && q(&neutral, p) <= q(&believer, p));
    }
    // The same average return with eight times the chance of collapse: the
    // survivors are paid more, so the top does better and the typical plan worse.
    let (other_rate, other) = crypto(|p| p.crypto.token = CryptoToken::OtherTokens);
    assert!(other_rate < neutral_rate, "{other_rate} vs {neutral_rate}");
    assert!(q(&other, 0.5) < q(&neutral, 0.5));

    // A credit fund, 30% of the portfolio, with a 5% a year chance of losing
    // between a tenth and three fifths of it.
    let fund = |f: fn(&mut PortfolioPolicy)| retiree_stats(AssetClass::Custom, 0.3, &f, paths);
    fn event(mean_includes_losses: bool) -> LossEvent {
        LossEvent {
            annual_probability: 0.05,
            mean_includes_losses,
            ..LossEvent::default()
        }
    }
    let (base_rate, base) = fund(|_| {});
    // Losses on top of the stated return: lower everywhere.
    let (rate, worth) = fund(|p| p.custom_asset.as_mut().unwrap().loss_event = Some(event(false)));
    assert!(rate < base_rate - 0.01, "{rate} vs {base_rate}");
    for p in [0.1, 0.5, 0.9] {
        assert!(q(&worth, p) < q(&base, p), "quantile {p}");
    }
    // Losses inside the stated return: the same average, a worse bad case, and
    // the lucky paths (no loss, a higher ordinary return) end ahead.
    let (rate_in, worth_in) =
        fund(|p| p.custom_asset.as_mut().unwrap().loss_event = Some(event(true)));
    assert!(rate_in > rate, "{rate_in} vs {rate}");
    assert!(q(&worth_in, 0.2) < q(&base, 0.2));
    assert!(q(&worth_in, 0.9) > q(&base, 0.9));

    // A single stock, 40% of the portfolio: a higher chance of total loss at the
    // same average return costs success.
    let stock =
        |f: fn(&mut PortfolioPolicy)| retiree_stats(AssetClass::SingleStock, 0.4, &f, paths);
    let (safe_rate, _) = stock(|p| p.single_stock.ruin_hazard = Some(0.0));
    let (risky_rate, risky) = stock(|p| p.single_stock.ruin_hazard = Some(0.1));
    let (_, safe) = stock(|p| p.single_stock.ruin_hazard = Some(0.0));
    assert!(risky_rate < safe_rate - 0.01, "{risky_rate} vs {safe_rate}");
    assert!(q(&risky, 0.25) < q(&safe, 0.25));
    // Fat tails keep the mean and the volatility, so over 30 years the plan is
    // about where it was: the change is in how the risk arrives month to month
    // (tested in src/economy/shape_tests.rs).
    let (thin_rate, _) = stock(|_| {});
    let (fat_rate, _) = stock(|p| p.single_stock.tails = TailShape::Fat);
    assert!(
        (fat_rate - thin_rate).abs() < 0.03,
        "{fat_rate} vs {thin_rate}"
    );
}

/// Cycles make one year's crypto volatility differ more from another's, and fat
/// tails make extreme single-stock months more common, as seen in traced paths.
#[test]
fn shapes_show_in_the_traced_returns() {
    let mut base = holding_all_three(Profile::example());
    base.portfolio.single_stock.volatility = 0.5;
    let mut shaped = base.clone();
    shaped.portfolio.crypto.dynamics = CryptoDynamics::cycles();
    shaped.portfolio.single_stock.tails = TailShape::Fat;
    let stats = |p: &Profile| {
        let plan = compile(p);
        let (mut yearly, mut extreme, mut months) = (Vec::new(), 0usize, 0usize);
        for path in 0..150 {
            let t = sim::trace_path(&plan, 5, path);
            for year in t.column("return_crypto").unwrap().chunks_exact(12) {
                if year.iter().all(|r| *r > -1.0) {
                    let mean = year.iter().map(|r| r.abs()).sum::<f64>() / 12.0;
                    yearly.push(mean);
                }
            }
            let stock = t.column("return_single_stock").unwrap();
            extreme += stock.iter().filter(|r| r.abs() > 0.4 && **r > -1.0).count();
            months += stock.len();
        }
        yearly.sort_by(f64::total_cmp);
        (
            q(&yearly, 0.9) / q(&yearly, 0.1),
            extreme as f64 / months as f64,
        )
    };
    let (spread, extreme) = stats(&base);
    let (spread_cycles, extreme_fat) = stats(&shaped);
    // Average absolute monthly move per calendar year: its 90th percentile over
    // its 10th.
    assert!(spread_cycles > 1.25 * spread, "{spread_cycles} vs {spread}");
    assert!(extreme_fat > 1.5 * extreme, "{extreme_fat} vs {extreme}");
}

/// The defaults and constants written in `data/actuarial/params.json`
/// (`asset_shapes`) are the ones the code uses.
#[test]
fn defaults_match_the_data_file() {
    let section = &Data::embedded().actuarial["asset_shapes"];
    let num = |v: &serde_json::Value| v.as_f64().unwrap();
    let c = &section["crypto_cycles"];
    assert_eq!(
        CryptoDynamics::cycles(),
        CryptoDynamics::Cycles {
            calm_months: num(&c["calm_months"]),
            wild_months: num(&c["wild_months"]),
            volatility_ratio: num(&c["volatility_ratio"]),
        }
    );
    assert_eq!(
        economy::CYCLE_MONTHS_RANGE,
        (num(&c["months_min"]), num(&c["months_max"]))
    );
    assert_eq!(
        economy::CYCLE_RATIO_RANGE,
        (1.0, num(&c["volatility_ratio_max"]))
    );
    let s = &section["single_stock_fat_tails"];
    assert_eq!(economy::STOCK_SHOCK_CAP_SD, num(&s["cap_sd"]));
    assert_eq!(
        economy::STOCK_SHOCK_MAX_FACTOR,
        num(&s["max_monthly_factor"])
    );
    let l = &section["custom_loss_event"];
    let d = LossEvent::default();
    assert_eq!(d.annual_probability, num(&l["annual_probability"]));
    assert_eq!(
        d.loss,
        Uncertain::Pert {
            low: num(&l["loss_pert"]["low"]),
            mode: num(&l["loss_pert"]["mode"]),
            high: num(&l["loss_pert"]["high"]),
        }
    );
    assert!(d.mean_includes_losses);
    assert_eq!(
        economy::ASSET_EXT_WORDS_PER_MONTH as f64,
        num(&section["asset_ext_words_per_month"])
    );
    // The profile's default outlook is the file's default preset, and its default
    // token the one the engine has always used.
    let model = &Data::embedded().actuarial["crypto"]["model"];
    assert_eq!(model["default_preset"], "neutral");
    assert_eq!(num(&model["expected_real_return_presets"]["neutral"]), 0.0);
    assert_eq!(
        num(&model["expected_real_return_presets"]["skeptic"]),
        -0.05
    );
    assert_eq!(
        num(&model["expected_real_return_presets"]["believer"]),
        0.06
    );
    assert_eq!(num(&model["ruin_hazard_annual"]["other_tokens"]), 0.08);
}

/// Prints the figures quoted in docs/methodology.md:
/// `cargo test -p firesight-core --release --test rich_assets report -- --ignored --nocapture`.
#[test]
#[ignore = "report: prints retiree success rates with each sleeve option on"]
fn report_success_rates() {
    type Change = (&'static str, AssetClass, f64, fn(&mut PortfolioPolicy));
    let changes: [Change; 14] = [
        ("stock 20%: default", AssetClass::SingleStock, 0.2, |_| {}),
        ("stock 20%: fat tails", AssetClass::SingleStock, 0.2, |p| {
            p.single_stock.tails = TailShape::Fat
        }),
        ("stock 20%: hazard 0", AssetClass::SingleStock, 0.2, |p| {
            p.single_stock.ruin_hazard = Some(0.0)
        }),
        ("stock 20%: hazard 3%", AssetClass::SingleStock, 0.2, |p| {
            p.single_stock.ruin_hazard = Some(0.03)
        }),
        ("crypto 10%: neutral", AssetClass::Crypto, 0.1, |_| {}),
        ("crypto 10%: skeptic", AssetClass::Crypto, 0.1, |p| {
            p.crypto.outlook = CryptoOutlook::Skeptic
        }),
        ("crypto 10%: believer", AssetClass::Crypto, 0.1, |p| {
            p.crypto.outlook = CryptoOutlook::Believer
        }),
        ("crypto 10%: other tokens", AssetClass::Crypto, 0.1, |p| {
            p.crypto.token = CryptoToken::OtherTokens
        }),
        ("crypto 10%: cycles", AssetClass::Crypto, 0.1, |p| {
            p.crypto.dynamics = CryptoDynamics::cycles()
        }),
        ("no sleeve (60/40)", AssetClass::Custom, 0.0, |_| {}),
        ("fund 30%: default", AssetClass::Custom, 0.3, |_| {}),
        ("fund 30%: fat tails", AssetClass::Custom, 0.3, |p| {
            p.custom_asset.as_mut().unwrap().tails = TailShape::Fat
        }),
        ("fund 30%: loss in mean", AssetClass::Custom, 0.3, |p| {
            p.custom_asset.as_mut().unwrap().loss_event = Some(LossEvent::default())
        }),
        ("fund 30%: loss on top", AssetClass::Custom, 0.3, |p| {
            p.custom_asset.as_mut().unwrap().loss_event = Some(LossEvent {
                mean_includes_losses: false,
                ..LossEvent::default()
            })
        }),
    ];
    let paths = 10_000;
    for (label, asset, share, change) in changes {
        let (rate, worth) = retiree_stats(asset, share, &change, paths);
        println!(
            "{label:28} success {:5.1}%  terminal net worth p10 {:>10.0} p50 {:>10.0} p90 {:>10.0}",
            rate * 100.0,
            q(&worth, 0.1),
            q(&worth, 0.5),
            q(&worth, 0.9),
        );
    }
}
