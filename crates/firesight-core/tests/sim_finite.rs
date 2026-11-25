//! Every demo household under every market model: nothing a recorder is handed is
//! ever NaN or infinite, in the aggregate or in a trace.

mod common;

use common::*;
use firesight_core::model::Profile;
use firesight_core::sim::{self, Workspace};
use firesight_core::stats::{
    Accumulator, MonthRecord, PathEnd, PathScalar, Recorder, Status, TraceEvent, YearRecord,
    YearSeries,
};

/// Forwards to an accumulator after checking every number.
struct Checked {
    inner: Accumulator,
    path: u64,
    months: u64,
    /// No amount may be this large (a sanity bound, well above any demo).
    limit: f64,
    /// Extra dollars the year's cash-flow residual may be off by: zero for the
    /// demos, something for amounts so large that a double cannot hold their cents.
    slack: f64,
}

impl Recorder for Checked {
    fn begin_path(&mut self, path_index: u64) {
        self.path = path_index;
        self.inner.begin_path(path_index);
    }

    fn record_year(&mut self, year_index: usize, record: &YearRecord) {
        assert!((1..=12).contains(&record.months));
        assert_ne!(record.status, Status::Dead);
        for (s, v) in YearSeries::ALL.iter().zip(record.values) {
            assert!(
                v.is_finite(),
                "path {} year {year_index} {}: {v}",
                self.path,
                s.key()
            );
            assert!(
                v >= -1.0 || *s == YearSeries::InvestmentReturn,
                "path {} year {year_index} {} is negative: {v}",
                self.path,
                s.key()
            );
            assert!(v.abs() < self.limit, "path {} {} = {v}", self.path, s.key());
        }
        assert!(
            record.cash_flow_residual().abs()
                < 1.0 + 1e-9 * record.gross_income().abs() + self.slack
        );
        self.inner.record_year(year_index, record);
    }

    fn end_path(&mut self, end: &PathEnd) {
        for s in PathScalar::ALL {
            let v = end.get(*s);
            assert!(
                !v.is_infinite(),
                "path {} scalar {}: {v}",
                self.path,
                s.key()
            );
        }
        // These always exist.
        for s in [
            PathScalar::DeathAge0,
            PathScalar::WorstDrawdown,
            PathScalar::MonthsUnemployed,
            PathScalar::LongTermCareCost,
            PathScalar::Inflation,
            PathScalar::Success,
        ] {
            assert!(
                end.get(s).is_finite(),
                "path {} scalar {}",
                self.path,
                s.key()
            );
        }
        let success = end.get(PathScalar::Success);
        assert!(success == 0.0 || success == 1.0);
        assert_eq!(success == 1.0, end.get(PathScalar::FailureCause).is_nan());
        let dd = end.get(PathScalar::WorstDrawdown);
        assert!((0.0..=1.0).contains(&dd));
        self.inner.end_path(end);
    }

    fn wants_detail(&self) -> bool {
        self.months > 0
    }

    fn record_month(&mut self, record: &MonthRecord) {
        self.months += 1;
        assert!(record.price_level.is_finite() && record.price_level > 0.0);
        assert!(record.portfolio_return.is_finite() && record.portfolio_return >= -1.0);
        for v in record.values.iter().chain(&record.asset_returns) {
            assert!(v.is_finite());
        }
    }

    fn record_event(&mut self, event: TraceEvent) {
        assert!(event.amount.map_or(true, f64::is_finite), "{event:?}");
        assert!(!event.label.is_empty());
    }
}

#[test]
fn two_thousand_paths_of_every_demo_under_every_market_model_are_finite() {
    const LIMIT: f64 = 1e13;
    const SLACK: f64 = 0.0;
    for example in Profile::examples(START) {
        for kind in MARKETS {
            let plan = compile(&with_market(example.profile.clone(), kind));
            let mut ws = Workspace::new(&plan);
            let mut rec = Checked {
                inner: plan.new_accumulator(),
                path: 0,
                months: 0,
                limit: LIMIT,
                slack: SLACK,
            };
            for i in 0..2000 {
                sim::run_path(&plan, 17, i, &mut ws, &mut rec);
            }
            let summary = plan.summarize(&rec.inner);
            assert_eq!(summary.paths, 2000);
            let h = &summary.headline;
            assert!((0.0..=1.0).contains(&h.success_rate.value));
            assert!(h.median_terminal_net_worth.is_finite());
            // The JSON never carries a non-number.
            let json = serde_json::to_string(&summary).unwrap();
            assert!(!json.contains("NaN") && !json.contains("Infinity") && !json.contains(":inf"));
            // Not degenerate: some paths succeed and at least a few retire.
            assert!(
                h.success_rate.value > 0.05,
                "{} {kind:?}: {}",
                example.slug,
                h.success_rate.value
            );
            assert!(h.retired_rate.value > 0.5, "{} {kind:?}", example.slug);

            // The same paths in detail mode.
            rec.months = 1;
            let mut detail = Checked {
                inner: plan.new_accumulator(),
                path: 0,
                months: 1,
                limit: LIMIT,
                slack: SLACK,
            };
            for i in 0..25 {
                sim::run_path(&plan, 17, i, &mut ws, &mut detail);
            }
            assert!(detail.months > 25 * 12);
            let trace = sim::trace_path(&plan, 17, 3);
            let json = serde_json::to_string(&trace).unwrap();
            assert!(!json.contains("NaN") && !json.contains("Infinity") && !json.contains(":inf"));
            assert!(trace.tax_returns.len() >= 2);
        }
    }
}

/// Private-company equity at the edge of what validation allows: a holding of ten
/// billion shares at $100 and an outcome table that spans a millionfold range of
/// prices. Every number handed to a recorder is still finite.
#[test]
fn an_extreme_equity_position_stays_finite() {
    use firesight_core::model::*;
    // A trillion dollars at the last round, times up to a thousand, then held as
    // single stock for decades: large, and finite.
    const LIMIT: f64 = 1e24;
    // A double carries sixteen digits: at these sizes a year's flows cannot add up
    // to the dollar. A billionth of the bound is allowed.
    const SLACK: f64 = 1e-9 * LIMIT;
    let mut extreme = startup_employee();
    {
        let co = &mut extreme.private_equity[0];
        co.preferred_price = 100.0;
        co.fmv_409a = Some(40.0);
        co.holdings = vec![EquityHolding {
            id: "huge".into(),
            kind: HoldingKind::Nso {
                strike: 1.0,
                granted_months_ago: 12,
            },
            shares: 1e10,
            vested: 1e10,
            ..EquityHolding::default()
        }];
        co.outcomes = vec![
            OutcomeRow {
                id: "sale".into(),
                kind: OutcomeKind::Acquired,
                probability: 0.5,
                years_from: 0.5,
                years_to: 6.0,
                multiple_low: 0.001,
                multiple_high: 1000.0,
            },
            OutcomeRow {
                id: "listing".into(),
                kind: OutcomeKind::Ipo,
                probability: 0.5,
                years_from: 0.5,
                years_to: 6.0,
                multiple_low: 0.001,
                multiple_high: 1000.0,
            },
        ];
        co.tenders.annual_probability = Some(1.0);
        co.policy.sell_down = SellDown::Hold;
    }
    for profile in [startup_employee(), extreme] {
        for kind in MARKETS {
            let plan = compile(&with_market(profile.clone(), kind));
            let mut ws = Workspace::new(&plan);
            let mut rec = Checked {
                inner: plan.new_accumulator(),
                path: 0,
                months: 0,
                limit: LIMIT,
                slack: SLACK,
            };
            for i in 0..600 {
                sim::run_path(&plan, 17, i, &mut ws, &mut rec);
            }
            let summary = plan.summarize(&rec.inner);
            assert_eq!(summary.paths, 600);
            let json = serde_json::to_string(&summary).unwrap();
            assert!(!json.contains("NaN") && !json.contains("Infinity") && !json.contains(":inf"));
            let mut detail = Checked {
                inner: plan.new_accumulator(),
                path: 0,
                months: 1,
                limit: LIMIT,
                slack: SLACK,
            };
            for i in 0..25 {
                sim::run_path(&plan, 17, i, &mut ws, &mut detail);
            }
            assert!(detail.months > 25 * 12);
            let trace = sim::trace_path(&plan, 17, 3);
            let json = serde_json::to_string(&trace).unwrap();
            assert!(!json.contains("NaN") && !json.contains("Infinity") && !json.contains(":inf"));
        }
    }
}
