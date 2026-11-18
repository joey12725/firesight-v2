//! A small synthetic life generator for the stats tests. It is not a model of
//! anything: it only has to produce records with realistic magnitudes, spreads,
//! statuses and missing values so sizes, timings and summaries are representative.

use super::{
    FailureCause, PathEnd, PathScalar, Recorder, Status, SummaryContext, YearRecord, YearSeries,
};
use crate::model::YearMonth;
use crate::rng::{Rng, Stream};

/// Seed of the synthetic runs.
pub(crate) const SEED: u64 = 0x5EED_F1EE;

/// Parameters of the synthetic household.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Synthetic {
    /// Horizon in calendar years.
    pub years: u32,
    /// Seed (same seed and path index give the same world).
    pub seed: u64,
    /// Annual retirement spending planned, today's dollars.
    pub spending: f64,
    /// Annual saving while working.
    pub saving: f64,
}

impl Default for Synthetic {
    fn default() -> Self {
        Synthetic {
            years: 60,
            seed: SEED,
            spending: 60_000.0,
            saving: 30_000.0,
        }
    }
}

/// Age of the first person at the start.
pub(crate) const START_AGE: f64 = 35.0;
/// The plan starts in March, so year 0 covers ten months.
pub(crate) const START: YearMonth = YearMonth::ym(2026, 3);

impl Synthetic {
    /// The summary context matching the synthetic household.
    pub(crate) fn context(&self) -> SummaryContext {
        SummaryContext {
            start: START,
            first_birth: YearMonth::ym(1991, 3),
            second_birth: Some(YearMonth::ym(1993, 6)),
        }
    }

    /// Simulates one synthetic life into `rec`.
    pub(crate) fn run_path<R: Recorder>(&self, path: u64, rec: &mut R) {
        let mut market = Rng::stream(self.seed, path, Stream::Market);
        let mut life = Rng::stream(self.seed, path, Stream::Longevity(0));
        let mut career = Rng::stream(self.seed, path, Stream::Career(0));
        let mut health = Rng::stream(self.seed, path, Stream::Health(0));

        let death_age_0 = (80.0 + 9.0 * life.normal()).clamp(START_AGE + 0.5, 95.0);
        let death_age_1 = (81.0 + 9.0 * life.normal()).clamp(33.0, 92.5);
        // The second person is 2.25 years younger: their death in the first
        // person's age terms.
        let last_death = death_age_0.max(death_age_1 + 2.25);
        let months_total =
            (((last_death - START_AGE) * 12.0).ceil() as u32).clamp(1, self.years * 12 - 2);
        let renter = career.uniform() < 0.12;
        let months_unemployed = career.poisson(4.0);
        let ltc = if health.uniform() < 0.2 {
            health.lognormal(11.5, 0.6)
        } else {
            0.0
        };
        let inflation = 0.025 + 0.008 * market.normal();

        let mut investable = if renter { 15_000.0 } else { 150_000.0 } * market.lognormal(0.0, 0.5);
        let mut property = if renter { 0.0 } else { 400_000.0 };
        let mut mortgage = if renter { 0.0 } else { 250_000.0 };
        let mut debt = if renter { 80_000.0 } else { 20_000.0 };
        let wages = 110_000.0 * career.lognormal(0.0, 0.25);
        let saving = self.saving - 1_500.0 * f64::from(months_unemployed);

        let mut end = PathEnd::default();
        let mut retired_at: Option<(u32, f64)> = None;
        let mut depleted = false;
        let mut growth_since_retirement = 1.0;
        let mut peak = investable;
        let mut worst_drawdown: f64 = 0.0;

        rec.begin_path(path);
        let mut month = 0u32;
        let mut year_index = 0usize;
        while month < months_total {
            let months = (if year_index == 0 { 10 } else { 12 }).min(months_total - month);
            let share = f64::from(months) / 12.0;
            month += months;
            let age = START_AGE + f64::from(month) / 12.0;
            let r = (0.045 + 0.16 * market.normal()).max(-0.6);
            let mut rec_year = YearRecord {
                months: months as u8,
                ..YearRecord::default()
            };
            let mut set = |s: YearSeries, v: f64| rec_year.values[s.index()] = v;

            let planned = (self.spending + if age > 65.0 { 8_000.0 } else { 0.0 }) * share;
            let in_ltc = ltc > 0.0 && month + 36 > months_total;
            let ltc_cost = if in_ltc { ltc / 3.0 * share } else { 0.0 };
            let growth = investable * (math_pow(1.0 + r, share) - 1.0);
            let delivered;
            if retired_at.is_none() {
                let w = wages * share;
                let tax = 0.24 * w;
                let save = saving.max(0.0) * share;
                delivered = w - tax - save;
                set(YearSeries::IncomeWages, w);
                set(YearSeries::TaxFederal, 0.6 * tax);
                set(YearSeries::TaxState, 0.1 * tax);
                set(YearSeries::TaxPayroll, 0.3 * tax);
                set(YearSeries::SaveTraditional, 0.6 * save);
                set(YearSeries::SaveTaxable, 0.4 * save);
                set(YearSeries::SaveEmployer, 0.04 * w);
                set(YearSeries::SpendPlanned, delivered);
                investable += growth + save + 0.04 * w;
                if investable >= 25.0 * self.spending && age < 70.0 {
                    retired_at = Some((month, investable));
                    end.set(PathScalar::FiAge, age);
                    end.set(PathScalar::RetirementAge, age);
                    end.set(PathScalar::InvestableAtRetirement, investable);
                    end.set(PathScalar::FundedRatio, investable / (25.0 * self.spending));
                }
            } else {
                let social = if age > 67.0 { 32_000.0 * share } else { 0.0 };
                let cut = if investable < 8.0 * self.spending {
                    0.70
                } else if investable < 15.0 * self.spending {
                    0.85
                } else {
                    1.0
                };
                let mut spend = planned * cut + ltc_cost;
                let tax = 0.12 * (spend - social).max(0.0);
                let mut need = spend + tax - social;
                if need > investable + growth {
                    // Out of money: live on Social Security.
                    if !depleted {
                        depleted = true;
                        end.set(PathScalar::DepletionAge, age);
                    }
                    need = (investable + growth).max(0.0);
                    spend = (social + need) / 1.12;
                }
                delivered = spend;
                investable = (investable + growth - need).max(0.0);
                set(YearSeries::IncomeSocialSecurity, social);
                set(YearSeries::WithdrawTaxable, 0.4 * need.max(0.0));
                set(YearSeries::WithdrawTraditional, 0.6 * need.max(0.0));
                set(YearSeries::TaxFederal, 0.12 * (spend - social).max(0.0));
                set(YearSeries::SpendPlanned, planned + ltc_cost);
                set(YearSeries::SpendLongTermCare, ltc_cost.min(spend));
                if month - retired_at.map_or(0, |r| r.0) <= 120 {
                    growth_since_retirement *= math_pow(1.0 + r, share);
                }
            }
            let ordinary = delivered - rec_year.values[YearSeries::SpendLongTermCare.index()];
            let mut set = |s: YearSeries, v: f64| rec_year.values[s.index()] = v;
            set(YearSeries::SpendEssential, 0.5 * ordinary);
            set(YearSeries::SpendDiscretionary, 0.3 * ordinary);
            set(YearSeries::SpendHousing, 0.12 * ordinary);
            set(YearSeries::SpendHealthcare, 0.08 * ordinary);
            set(YearSeries::InvestmentReturn, growth);
            set(YearSeries::BalanceCash, 0.05 * investable);
            set(YearSeries::BalanceTaxable, 0.35 * investable);
            set(YearSeries::BalanceTraditional, 0.40 * investable);
            set(YearSeries::BalanceRoth, 0.15 * investable);
            set(YearSeries::BalanceHsa, 0.05 * investable);
            property *= 1.0 + 0.01 * share;
            mortgage = (mortgage - 10_000.0 * share).max(0.0);
            debt = (debt - 4_000.0 * share).max(0.0);
            set(YearSeries::PropertyValue, property);
            set(YearSeries::Mortgage, mortgage);
            set(YearSeries::Debt, debt);

            peak = peak.max(investable);
            if peak > 0.0 {
                worst_drawdown = worst_drawdown.max(1.0 - investable / peak);
            }
            rec_year.retired = retired_at.is_some();
            rec_year.status = Status::classify(
                rec_year.retired,
                depleted,
                delivered,
                rec_year.values[YearSeries::SpendPlanned.index()],
                investable,
                retired_at.map_or(0.0, |r| r.1),
            );
            rec.record_year(year_index, &rec_year);
            year_index += 1;
        }

        end.set(PathScalar::DeathAge0, death_age_0);
        end.set(PathScalar::DeathAge1, death_age_1);
        end.set(PathScalar::WorstDrawdown, worst_drawdown);
        end.set(PathScalar::MonthsUnemployed, f64::from(months_unemployed));
        end.set(PathScalar::LongTermCareCost, ltc);
        end.set(PathScalar::Inflation, inflation);
        if let Some((at, _)) = retired_at {
            let years = (f64::from(month - at) / 12.0).clamp(1.0 / 12.0, 10.0);
            end.set(
                PathScalar::EarlyRetirementReturn,
                math_pow(growth_since_retirement, 1.0 / years) - 1.0,
            );
        }
        end.set_outcome(match (retired_at, depleted) {
            (Some(_), false) => None,
            (None, _) if months_unemployed > 8 => Some(FailureCause::JobLoss),
            (None, _) => Some(FailureCause::Overspending),
            (Some(_), true) if ltc > 0.0 => Some(FailureCause::Health),
            (Some(_), true) if last_death > 90.0 => Some(FailureCause::Longevity),
            (Some(_), true) => Some(FailureCause::MarketSequence),
        });
        rec.end_path(&end);
    }
}

fn math_pow(x: f64, y: f64) -> f64 {
    crate::math::pow(x, y)
}
