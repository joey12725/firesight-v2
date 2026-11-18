//! The full ledger of one path: monthly columns, annual columns, tax returns, events
//! and scalars, in a struct-of-arrays layout that serialises compactly to JSON.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::accumulator::PathTally;
use super::num::{nan, nan_vec, round_dp};
use super::summary::unit_decimals;
use super::{
    HistSeries, MonthRecord, PathEnd, PathScalar, Recorder, Status, TraceEvent, YearRecord,
    YearSeries,
};
use crate::model::{AssetClass, YearMonth};
use crate::N_ASSETS;

/// Decimal places kept for rates in a trace (monthly returns).
const RATE_DECIMALS: u32 = 6;
/// Decimal places kept for the price level.
const PRICE_DECIMALS: u32 = 5;
/// Decimal places kept for tax-return amounts (cents).
const TAX_DECIMALS: u32 = 2;

/// One column of a traced path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceColumn {
    /// Column key: `net_worth`, `investable`, a `YearSeries::key()`, `price_level`,
    /// `portfolio_return` or `return_<asset class key>`; in the annual table also
    /// `spending`, `gross_income` and `total_tax`.
    pub key: String,
    /// `dollars` (whole today's dollars), `rate` (nominal return for the month as a
    /// decimal fraction) or `index` (price level, 1.0 at the start).
    pub unit: String,
    /// One value per month (or per year in the annual table).
    #[serde(with = "nan_vec")]
    #[schemars(with = "Vec<Option<f64>>")]
    pub values: Vec<f64>,
}

/// The annual view of a traced path: exactly the records the aggregate run saw.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct TraceAnnual {
    /// Calendar year of each record.
    pub years: Vec<i32>,
    /// Months each record covers.
    pub months: Vec<u8>,
    /// Status at each year end, as an index into `PathTrace::statuses`.
    pub status: Vec<u8>,
    /// Annual columns (flows are totals for the months covered, not annualised).
    /// Series that are zero throughout are omitted.
    pub columns: Vec<TraceColumn>,
}

/// One line of a tax return in a trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceTaxLine {
    /// Stable key, for example `taxable_income`.
    pub key: String,
    /// Human label.
    pub label: String,
    /// Amount in nominal dollars of the tax year.
    pub amount: f64,
    /// How it was derived.
    pub note: String,
}

/// One year's tax return in a trace, in nominal dollars of that year.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceTaxReturn {
    /// Tax year.
    pub year: i32,
    /// Price level in the last traced month of the tax year: divide the amounts by it
    /// for today's dollars.
    pub price_level: f64,
    /// The computed return (`tax::TaxResult`) as an object.
    pub result: serde_json::Value,
    /// Form-like lines in order.
    pub lines: Vec<TraceTaxLine>,
}

/// One per-path scalar of a traced path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraceScalar {
    /// Which scalar.
    pub scalar: PathScalar,
    /// Its value; null where it did not happen.
    #[serde(with = "nan")]
    #[schemars(with = "Option<f64>")]
    pub value: f64,
}

/// The complete ledger of one path. Money is in whole today's dollars except in
/// `tax_returns`; multiply by the `price_level` column for nominal dollars.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PathTrace {
    /// Which path.
    pub path_index: u64,
    /// The run's seed.
    pub seed: u64,
    /// First month; month `i` of every column is `start + i`.
    pub start: YearMonth,
    /// Number of months simulated.
    pub months: u32,
    /// The status vocabulary that `status` indexes.
    pub statuses: Vec<Status>,
    /// Status at each month end, as an index into `statuses`.
    pub status: Vec<u8>,
    /// Monthly columns. Balances are month-end, flows are the month's total. Series
    /// that are zero throughout are omitted; `net_worth`, `investable`,
    /// `price_level` and `portfolio_return` are always present.
    pub columns: Vec<TraceColumn>,
    /// The annual records.
    pub annual: TraceAnnual,
    /// The tax return of each calendar year.
    pub tax_returns: Vec<TraceTaxReturn>,
    /// What happened, in order.
    pub events: Vec<TraceEvent>,
    /// The per-path scalars, every `PathScalar` in order, identical to the row the
    /// aggregate run stores for this path.
    pub scalars: Vec<TraceScalar>,
}

impl PathTrace {
    /// The monthly column with this key, if present (absent means zero throughout).
    pub fn column(&self, key: &str) -> Option<&[f64]> {
        self.columns
            .iter()
            .find(|c| c.key == key)
            .map(|c| &c.values[..])
    }
}

/// The recorder used by `sim::trace_path`.
#[derive(Clone, Debug)]
pub struct TraceRecorder {
    seed: u64,
    start: YearMonth,
    path_index: u64,
    status: Vec<u8>,
    series: Vec<Vec<f64>>,
    price_level: Vec<f64>,
    portfolio_return: Vec<f64>,
    asset_returns: Vec<Vec<f64>>,
    annual_months: Vec<u8>,
    annual_status: Vec<u8>,
    annual_series: Vec<Vec<f64>>,
    annual_hist: Vec<Vec<f64>>,
    tax_returns: Vec<TraceTaxReturn>,
    events: Vec<TraceEvent>,
    tally: PathTally,
    scalars: [f32; PathScalar::COUNT],
}

fn rounded(values: &[f64], decimals: u32) -> Vec<f64> {
    values.iter().map(|v| round_dp(*v, decimals)).collect()
}

/// A dollar column, or `None` if it is zero in every period after rounding.
fn dollar_column(key: &str, values: &[f64], always: bool) -> Option<TraceColumn> {
    let values = rounded(values, 0);
    (always || values.iter().any(|v| *v != 0.0)).then(|| TraceColumn {
        key: key.to_string(),
        unit: "dollars".to_string(),
        values,
    })
}

impl TraceRecorder {
    /// A recorder for one path of the run with this seed, starting in `start`.
    pub fn new(seed: u64, start: YearMonth) -> TraceRecorder {
        TraceRecorder {
            seed,
            start,
            path_index: 0,
            status: Vec::new(),
            series: vec![Vec::new(); YearSeries::COUNT],
            price_level: Vec::new(),
            portfolio_return: Vec::new(),
            asset_returns: vec![Vec::new(); N_ASSETS],
            annual_months: Vec::new(),
            annual_status: Vec::new(),
            annual_series: vec![Vec::new(); YearSeries::COUNT],
            annual_hist: vec![Vec::new(); HistSeries::COUNT],
            tax_returns: Vec::new(),
            events: Vec::new(),
            tally: PathTally::default(),
            scalars: [f32::NAN; PathScalar::COUNT],
        }
    }

    /// The finished trace.
    pub fn finish(self) -> PathTrace {
        let months = self.status.len();
        let net_worth: Vec<f64> = (0..months).map(|m| self.month_values(m, true)).collect();
        let investable: Vec<f64> = (0..months).map(|m| self.month_values(m, false)).collect();
        let mut columns = Vec::new();
        columns.extend(dollar_column("net_worth", &net_worth, true));
        columns.extend(dollar_column("investable", &investable, true));
        for s in YearSeries::ALL {
            columns.extend(dollar_column(s.key(), &self.series[s.index()], false));
        }
        columns.push(TraceColumn {
            key: "price_level".to_string(),
            unit: "index".to_string(),
            values: rounded(&self.price_level, PRICE_DECIMALS),
        });
        columns.push(TraceColumn {
            key: "portfolio_return".to_string(),
            unit: "rate".to_string(),
            values: rounded(&self.portfolio_return, RATE_DECIMALS),
        });
        for asset in AssetClass::ALL {
            let values = rounded(&self.asset_returns[asset.index()], RATE_DECIMALS);
            if values.iter().any(|v| *v != 0.0) {
                columns.push(TraceColumn {
                    key: format!("return_{}", asset.key()),
                    unit: "rate".to_string(),
                    values,
                });
            }
        }

        let mut annual_columns = Vec::new();
        for h in HistSeries::ALL {
            annual_columns.extend(dollar_column(h.key(), &self.annual_hist[h.index()], true));
        }
        for s in YearSeries::ALL {
            annual_columns.extend(dollar_column(
                s.key(),
                &self.annual_series[s.index()],
                false,
            ));
        }

        PathTrace {
            path_index: self.path_index,
            seed: self.seed,
            start: self.start,
            months: months as u32,
            statuses: Status::ALL.to_vec(),
            status: self.status,
            columns,
            annual: TraceAnnual {
                years: (0..self.annual_months.len() as i32)
                    .map(|y| self.start.year() + y)
                    .collect(),
                months: self.annual_months,
                status: self.annual_status,
                columns: annual_columns,
            },
            tax_returns: self.tax_returns,
            events: self.events,
            scalars: PathScalar::ALL
                .iter()
                .map(|s| TraceScalar {
                    scalar: *s,
                    value: round_dp(f64::from(self.scalars[s.index()]), unit_decimals(s.unit())),
                })
                .collect(),
        }
    }

    /// Net worth (or investable assets) of month `m` from the stored series.
    fn month_values(&self, m: usize, net_worth: bool) -> f64 {
        let mut values = [0.0; YearSeries::COUNT];
        for (v, column) in values.iter_mut().zip(&self.series) {
            *v = column[m];
        }
        if net_worth {
            super::net_worth_of(&values)
        } else {
            super::investable_of(&values)
        }
    }
}

impl Recorder for TraceRecorder {
    fn begin_path(&mut self, path_index: u64) {
        *self = TraceRecorder::new(self.seed, self.start);
        self.path_index = path_index;
    }

    fn record_year(&mut self, year_index: usize, record: &YearRecord) {
        debug_assert_eq!(year_index, self.annual_months.len(), "years in order");
        let hist = record.hist_values();
        self.tally.record(record, &hist);
        self.annual_months.push(record.months);
        self.annual_status.push(record.status.index() as u8);
        for (column, v) in self.annual_series.iter_mut().zip(record.values) {
            column.push(v);
        }
        for (column, v) in self.annual_hist.iter_mut().zip(hist) {
            column.push(v);
        }
    }

    fn end_path(&mut self, end: &PathEnd) {
        self.scalars = end.scalars;
        self.tally.finish(&mut self.scalars);
    }

    fn wants_detail(&self) -> bool {
        true
    }

    fn record_month(&mut self, record: &MonthRecord) {
        debug_assert_eq!(
            record.month,
            self.start + self.status.len() as i32,
            "months in order from the start"
        );
        self.status.push(record.status.index() as u8);
        for (column, v) in self.series.iter_mut().zip(record.values) {
            column.push(v);
        }
        self.price_level.push(record.price_level);
        self.portfolio_return.push(record.portfolio_return);
        for (column, v) in self.asset_returns.iter_mut().zip(record.asset_returns) {
            column.push(v);
        }
    }

    fn record_event(&mut self, event: TraceEvent) {
        self.events.push(TraceEvent {
            amount: event.amount.map(|a| round_dp(a, 0)),
            ..event
        });
    }

    fn record_tax_return(&mut self, year: i32, detail: &crate::tax::TaxDetail) {
        // The price level of the last month traced in the tax year (its December,
        // unless the path ended sooner); 1.0 if the year precedes the trace.
        let december = YearMonth::ym(year.clamp(0, 9999), 12) - self.start;
        let price_level = usize::try_from(december)
            .ok()
            .and_then(|m| {
                let last = self.price_level.len().checked_sub(1)?;
                Some(self.price_level[m.min(last)])
            })
            .unwrap_or(1.0);
        self.tax_returns.push(TraceTaxReturn {
            year,
            price_level: round_dp(price_level, PRICE_DECIMALS),
            result: serde_json::to_value(detail.result).unwrap_or(serde_json::Value::Null),
            lines: detail
                .lines
                .iter()
                .map(|l| TraceTaxLine {
                    key: l.key.clone(),
                    label: l.label.clone(),
                    amount: round_dp(l.amount, TAX_DECIMALS),
                    note: l.note.clone(),
                })
                .collect(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::testkit::Synthetic;
    use crate::stats::{Accumulator, TraceEventKind};
    use crate::tax::{TaxDetail, TaxLine, TaxResult};

    /// Fourteen months from November 2026: two in 2026, twelve in 2027.
    fn traced() -> PathTrace {
        let start = YearMonth::ym(2026, 11);
        let mut rec = TraceRecorder::new(42, start);
        assert!(rec.wants_detail());
        rec.begin_path(7);
        let mut year = YearRecord::default();
        for m in 0..14 {
            let mut r = MonthRecord::new(start + m);
            r.price_level = 1.0 + 0.002 * f64::from(m);
            r.values[YearSeries::BalanceTaxable.index()] = 100_000.4 + 1_000.0 * f64::from(m);
            r.values[YearSeries::Mortgage.index()] = 40_000.0;
            r.values[YearSeries::IncomeWages.index()] = 8_000.0;
            r.values[YearSeries::SpendEssential.index()] = 3_000.49;
            r.values[YearSeries::SpendPlanned.index()] = 3_000.49;
            r.values[YearSeries::TaxFederal.index()] = 0.3;
            r.portfolio_return = 0.012_345_678;
            r.asset_returns[AssetClass::UsStocks.index()] = -0.05;
            r.status = if m < 12 {
                Status::Working
            } else {
                Status::RetiredOnPlan
            };
            r.retired = m >= 12;
            rec.record_month(&r);
            assert!((r.net_worth() - (60_000.4 + 1_000.0 * f64::from(m))).abs() < 1e-6);
            assert!((r.investable() - (100_000.4 + 1_000.0 * f64::from(m))).abs() < 1e-6);

            // Build the annual record the way the loop does: balances from the last
            // month, flows summed.
            for s in YearSeries::ALL {
                let i = s.index();
                if s.is_stock() {
                    year.values[i] = r.values[i];
                } else {
                    year.values[i] += r.values[i];
                }
            }
            year.status = r.status;
            year.retired = r.retired;
            if (start + m).month() == 12 || m == 13 {
                year.months = if m == 1 { 2 } else { 12 };
                rec.record_year(if m == 1 { 0 } else { 1 }, &year);
                year = YearRecord::default();
            }
            if m == 12 {
                rec.record_event(TraceEvent {
                    month: start + m,
                    kind: TraceEventKind::Retired,
                    label: "Retired at 61".to_string(),
                    amount: Some(1_234.56),
                });
            }
        }
        let detail = TaxDetail {
            result: TaxResult {
                total: 1_500.0,
                ..TaxResult::default()
            },
            lines: vec![TaxLine {
                key: "taxable_income".into(),
                label: "Taxable income".into(),
                amount: 16_000.004,
                note: "wages less standard deduction".into(),
            }],
        };
        rec.record_tax_return(2026, &detail);
        rec.record_tax_return(2027, &detail);
        rec.record_tax_return(2020, &detail);
        let mut end = PathEnd::default();
        end.set(PathScalar::RetirementAge, 61.0);
        end.set_outcome(None);
        rec.end_path(&end);
        rec.finish()
    }

    #[test]
    fn trace_has_columns_events_returns_and_scalars() {
        let t = traced();
        assert_eq!((t.path_index, t.seed, t.months), (7, 42, 14));
        assert_eq!(t.start, YearMonth::ym(2026, 11));
        assert_eq!(t.statuses, Status::ALL);
        assert_eq!(t.status[11], Status::Working.index() as u8);
        assert_eq!(t.status[12], Status::RetiredOnPlan.index() as u8);

        // Present: the four fixed columns, the non-zero series and one asset return.
        let keys: Vec<&str> = t.columns.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "net_worth",
                "investable",
                "balance_taxable",
                "mortgage",
                "income_wages",
                "spend_essential",
                "spend_planned",
                "price_level",
                "portfolio_return",
                "return_us_stocks"
            ]
        );
        assert!(t.columns.iter().all(|c| c.values.len() == 14));
        // Whole dollars; a column that rounds to zero throughout (30 cents of tax a
        // month) is dropped.
        assert_eq!(t.column("net_worth").unwrap()[0], 60_000.0);
        assert_eq!(t.column("net_worth").unwrap()[13], 73_000.0);
        assert_eq!(t.column("investable").unwrap()[13], 113_000.0);
        assert_eq!(t.column("spend_essential").unwrap()[3], 3_000.0);
        assert!(t.column("tax_federal").is_none());
        assert_eq!(t.column("price_level").unwrap()[13], 1.026);
        assert_eq!(t.column("portfolio_return").unwrap()[0], 0.012_346);
        assert_eq!(t.column("return_us_stocks").unwrap()[5], -0.05);
        assert_eq!(t.columns[0].unit, "dollars");
        assert_eq!(t.columns[7].unit, "index");
        assert_eq!(t.columns[8].unit, "rate");

        // Annual table: what the aggregate recorder was given.
        assert_eq!(t.annual.years, [2026, 2027]);
        assert_eq!(t.annual.months, [2, 12]);
        assert_eq!(
            t.annual.status,
            [
                Status::Working.index() as u8,
                Status::RetiredOnPlan.index() as u8
            ]
        );
        let annual = |key: &str| {
            &t.annual
                .columns
                .iter()
                .find(|c| c.key == key)
                .unwrap()
                .values
        };
        assert_eq!(annual("net_worth"), &[61_000.0, 73_000.0]);
        assert_eq!(annual("gross_income"), &[16_000.0, 96_000.0]);
        assert_eq!(annual("spending"), &[6_001.0, 36_006.0]);
        assert_eq!(annual("total_tax"), &[1.0, 4.0]);
        assert_eq!(annual("income_wages"), &[16_000.0, 96_000.0]);
        assert_eq!(annual("tax_federal"), &[1.0, 4.0]);
        assert_eq!(t.annual.columns[0].key, "net_worth");
        assert_eq!(t.annual.columns[4].key, "total_tax");

        assert_eq!(t.events.len(), 1);
        assert_eq!(t.events[0].kind, TraceEventKind::Retired);
        assert_eq!(t.events[0].month, YearMonth::ym(2027, 11));
        assert_eq!(t.events[0].amount, Some(1_235.0));

        // Tax returns carry the December price level of their year.
        assert_eq!(t.tax_returns.len(), 3);
        assert_eq!(t.tax_returns[0].year, 2026);
        assert_eq!(t.tax_returns[0].price_level, 1.002);
        assert_eq!(t.tax_returns[1].price_level, 1.026);
        assert_eq!(t.tax_returns[2].price_level, 1.0);
        assert_eq!(t.tax_returns[0].lines[0].amount, 16_000.0);
        assert_eq!(t.tax_returns[0].lines[0].key, "taxable_income");
        assert_eq!(t.tax_returns[0].result["total"], 1_500.0);

        // Scalars: supplied and derived.
        let scalar = |s: PathScalar| t.scalars[s.index()].value;
        assert_eq!(t.scalars.len(), PathScalar::COUNT);
        assert_eq!(scalar(PathScalar::RetirementAge), 61.0);
        assert_eq!(scalar(PathScalar::Success), 1.0);
        assert!(scalar(PathScalar::FiAge).is_nan());
        assert_eq!(scalar(PathScalar::YearsSimulated), 2.0);
        assert_eq!(scalar(PathScalar::TerminalNetWorth), 73_000.0);
        assert_eq!(scalar(PathScalar::YearsRetired), 1.0);
        assert_eq!(scalar(PathScalar::DeliveredSpendingRatio), 1.0);
    }

    #[test]
    fn trace_json_is_compact_and_round_trips() {
        let t = traced();
        let json = serde_json::to_string(&t).unwrap();
        // Struct of arrays: keys once, integers without a fraction, nulls for NaN.
        assert!(json.contains(r#"{"key":"mortgage","unit":"dollars","values":[40000,40000,"#));
        assert!(json.contains(r#""start":"2026-11""#));
        assert!(json.contains(r#"{"scalar":"fi_age","value":null}"#));
        assert!(json.contains(r#""kind":"retired""#));
        assert!(!json.contains("NaN"));
        let back: PathTrace = serde_json::from_str(&json).unwrap();
        assert_eq!(back.columns, t.columns);
        assert_eq!(back.events, t.events);
        assert_eq!(back.tax_returns, t.tax_returns);
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    #[test]
    fn trace_json_size_for_a_sixty_year_life() {
        // 720 months with thirty active series and four asset classes.
        let start = YearMonth::ym(2026, 1);
        let mut rec = TraceRecorder::new(1, start);
        rec.begin_path(0);
        for m in 0..720 {
            let mut r = MonthRecord::new(start + m);
            r.price_level = 1.0 + 0.002_13 * f64::from(m);
            for (i, v) in r.values.iter_mut().take(30).enumerate() {
                *v = 1_234.56 * (i as f64 + 1.0) * (1.0 + 0.004 * f64::from(m));
            }
            r.portfolio_return = 0.004_321 + 0.000_01 * f64::from(m % 17);
            for (i, a) in r.asset_returns.iter_mut().take(4).enumerate() {
                *a = 0.003_217 * (i as f64 + 1.0) - 0.000_1 * f64::from(m % 13);
            }
            rec.record_month(&r);
        }
        rec.end_path(&PathEnd::default());
        let json = serde_json::to_string(&rec.finish()).unwrap();
        println!("60-year trace: {} bytes of JSON", json.len());
        assert!(json.len() < 250_000, "{}", json.len());
    }

    /// A recorder that feeds two recorders at once.
    struct Both<'a>(&'a mut Accumulator, &'a mut TraceRecorder);

    impl Recorder for Both<'_> {
        fn begin_path(&mut self, path_index: u64) {
            self.0.begin_path(path_index);
            self.1.begin_path(path_index);
        }
        fn record_year(&mut self, year_index: usize, record: &YearRecord) {
            self.0.record_year(year_index, record);
            self.1.record_year(year_index, record);
        }
        fn end_path(&mut self, end: &PathEnd) {
            self.0.end_path(end);
            self.1.end_path(end);
        }
    }

    #[test]
    fn traced_scalars_equal_the_aggregate_row() {
        let kit = Synthetic::default();
        let mut acc = Accumulator::new(kit.years);
        for path in [3u64, 11, 40] {
            let mut rec = TraceRecorder::new(kit.seed, crate::stats::testkit::START);
            kit.run_path(path, &mut Both(&mut acc, &mut rec));
            let trace = rec.finish();
            assert_eq!(trace.path_index, path);
            assert_eq!(trace.months, 0, "the synthetic kit records no months");
            let row = acc.path_scalars(path).unwrap();
            for s in PathScalar::ALL {
                let want = round_dp(f64::from(row[s.index()]), unit_decimals(s.unit()));
                let got = trace.scalars[s.index()].value;
                assert!(want == got || want.is_nan() && got.is_nan(), "{s:?}");
            }
            // The annual table matches the sample path the aggregate kept.
            let sample = acc.samples.iter().find(|s| s.path_index == path).unwrap();
            let net_worth: Vec<i64> = trace.annual.columns[0]
                .values
                .iter()
                .map(|v| *v as i64)
                .collect();
            assert_eq!(net_worth, sample.net_worth);
            assert_eq!(trace.annual.years.len(), sample.net_worth.len());
        }
        // A recorder is reusable: begin_path resets it.
        let mut rec = TraceRecorder::new(kit.seed, crate::stats::testkit::START);
        kit.run_path(1, &mut rec);
        kit.run_path(2, &mut rec);
        let trace = rec.finish();
        assert_eq!(trace.path_index, 2);
        assert_eq!(
            trace.annual.years.len() as f64,
            trace.scalars[PathScalar::YearsSimulated.index()].value
        );
    }
}
