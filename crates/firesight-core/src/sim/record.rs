//! Turning the month's ledger into records, the conservation checks, and the
//! end-of-path scalars.
//!
//! # Units
//!
//! The ledger is nominal. Flows of month `t` are deflated by the price level at the
//! start of the month and balances by the level at its end, so everything a recorder
//! sees is in today's dollars.
//!
//! # Conservation (check mode)
//!
//! Four identities are asserted every month, each to a tolerance of a cent plus
//! one part in 10^9 of the amounts involved:
//!
//! 1. **Cash**: sources equal uses (`YearRecord::cash_flow_residual`), with the cash
//!    bucket's own line taken from its actual change in balance. Any code that moved
//!    cash without booking a flow, or booked a flow without moving cash, breaks it.
//!    Private-company equity is inside it: proceeds are a source (`income_equity`),
//!    the strike paid on an exercise a use (`equity_exercise`).
//! 2. **Invested buckets**: their change in value equals contributions less
//!    withdrawals plus the market return reported by `Accounts::apply_returns`.
//! 3. **Property, mortgages and debts**: each changes by exactly the flows the
//!    housing and debt steps reported (appreciation, purchases and sales, principal,
//!    interest, new loans).
//!
//! 4. **Private-company share lots**: the regular-tax basis held in lots changes by
//!    exactly the basis the equity steps reported coming in (an exercise, a step-up
//!    at death) and going out (a sale, a move to the taxable account, a failure, a
//!    write-off, holdings dropped when nobody inherits). Every lot has non-negative
//!    shares, cost and AMT basis, and the paper value is finite and not negative.
//!
//! The first three make the change in net worth equal to the sum of the recorded
//! flows. Private-company holdings are outside net worth until they turn into cash
//! or into shares in the taxable account; their paper value is the memo series
//! `equity_paper_value`.
//!
//! # Success and failure attribution
//!
//! A path **succeeds** if it was never depleted, ends with at least the bequest
//! target, and either retired or ended (by death) before the plan's latest
//! retirement age. A failed path gets one first cause, by the first rule that holds:
//!
//! 1. `health`: long-term care paid before the money ran out came to at least a
//!    quarter of the savings held at retirement (or, without a retirement, at least
//!    two years of the budget);
//! 2. the money ran out before retirement: `job_loss` if at least 6 months of
//!    involuntary unemployment came before that, otherwise `overspending` (the
//!    household spent more than it earned); never a market or longevity cause;
//!    otherwise `job_loss` if at least 18 months of unemployment came before the
//!    money ran out;
//! 3. `inflation`: inflation over the path averaged 4.5% a year or more;
//! 4. `market_sequence`: the real portfolio return over the first ten retirement
//!    years was more than two points a year below the plan's expected real return;
//! 5. `longevity`: the money ran out at 90 or later;
//! 6. `overspending`: the first-year withdrawal rate was above 5%, or spending
//!    delivered exceeded the plan;
//! 7. `other`.

use crate::economy::EconomyPath;
use crate::math;
use crate::stats::{
    FailureCause, MonthRecord, PathEnd, PathScalar, Recorder, Status, YearRecord, YearSeries,
};

use super::plan::Plan;
use super::state::State;

/// Absolute tolerance of the conservation checks, dollars.
const CHECK_ABSOLUTE: f64 = 0.02;
/// Relative tolerance of the conservation checks.
const CHECK_RELATIVE: f64 = 1e-9;

/// Period-end stocks: zeroed and rewritten at each record.
const BALANCES: [YearSeries; 9] = [
    YearSeries::BalanceCash,
    YearSeries::BalanceTaxable,
    YearSeries::BalanceTraditional,
    YearSeries::BalanceRoth,
    YearSeries::BalanceHsa,
    YearSeries::PropertyValue,
    YearSeries::Mortgage,
    YearSeries::Debt,
    YearSeries::EquityPaperValue,
];

impl State {
    /// Snapshot for the conservation checks, taken before the month begins.
    #[cold]
    #[inline(never)]
    pub(crate) fn check_begin(&mut self) {
        let kinds = self.acc.by_kind();
        self.chk_other_start = kinds[1] + kinds[2] + kinds[3] + kinds[4];
        let (value, mortgage) = self.property_totals();
        self.chk_prop_start = value;
        self.chk_mort_start = mortgage;
        self.chk_debt_start = self.debt_total();
        self.chk_ret_other = 0.0;
        self.chk_prop_flow = 0.0;
        self.chk_mort_flow = 0.0;
        self.chk_debt_flow = 0.0;
        self.chk_eq_start = self.lot_basis_total();
        self.chk_eq_flow = 0.0;
    }

    /// Regular-tax basis of every private-company share lot held.
    fn lot_basis_total(&self) -> f64 {
        let mut total = 0.0;
        for rt in &self.companies {
            total += rt.st.lot_basis();
        }
        total
    }

    /// Asserts the conservation identities for the month just simulated.
    #[cold]
    #[inline(never)]
    fn check_end(&self, t: usize) {
        let led = &self.led;
        let get = |s: YearSeries| led[s.index()];
        let mut gross = 0.0;
        for v in led {
            gross += v.abs();
        }
        let tol = |scale: f64| CHECK_ABSOLUTE + CHECK_RELATIVE * (scale.abs() + gross);

        let residual = crate::stats::cash_flow_residual_of(led);
        assert!(
            residual.abs() <= tol(self.acc.cash),
            "month {t}: cash sources and uses differ by {residual:.4} (ledger {led:?})"
        );

        let kinds = self.acc.by_kind();
        let other = kinds[1] + kinds[2] + kinds[3] + kinds[4];
        let expected = self.chk_other_start
            + get(YearSeries::SaveTaxable)
            + get(YearSeries::SaveTraditional)
            + get(YearSeries::SaveRoth)
            + get(YearSeries::SaveHsa)
            + get(YearSeries::SaveEmployer)
            - get(YearSeries::WithdrawTaxable)
            - get(YearSeries::WithdrawTraditional)
            - get(YearSeries::WithdrawRoth)
            - get(YearSeries::WithdrawHsa)
            + self.chk_ret_other;
        assert!(
            (other - expected).abs() <= tol(other),
            "month {t}: invested buckets hold {other:.4}, flows explain {expected:.4}"
        );

        let (value, mortgage) = self.property_totals();
        let debt = self.debt_total();
        assert!(
            (value - self.chk_prop_start - self.chk_prop_flow).abs() <= tol(value),
            "month {t}: property value changed by {:.4}, flows explain {:.4}",
            value - self.chk_prop_start,
            self.chk_prop_flow
        );
        assert!(
            (mortgage - self.chk_mort_start - self.chk_mort_flow).abs() <= tol(mortgage),
            "month {t}: mortgages changed by {:.4}, flows explain {:.4}",
            mortgage - self.chk_mort_start,
            self.chk_mort_flow
        );
        assert!(
            (debt - self.chk_debt_start - self.chk_debt_flow).abs() <= tol(debt),
            "month {t}: debts changed by {:.4}, flows explain {:.4}",
            debt - self.chk_debt_start,
            self.chk_debt_flow
        );
        let basis = self.lot_basis_total();
        assert!(
            (basis - self.chk_eq_start - self.chk_eq_flow).abs() <= tol(basis),
            "month {t}: private-company lot basis changed by {:.4}, flows explain {:.4}",
            basis - self.chk_eq_start,
            self.chk_eq_flow
        );
        for (c, rt) in self.companies.iter().enumerate() {
            for lot in rt.st.lots() {
                assert!(
                    lot.shares >= 0.0 && lot.cost >= 0.0 && lot.amt_extra >= 0.0,
                    "month {t}: company {c} holds a negative lot ({lot:?})"
                );
            }
        }
        assert!(
            self.eq_paper.is_finite() && self.eq_paper >= 0.0,
            "month {t}: private-company paper value is {}",
            self.eq_paper
        );
        for (i, v) in led.iter().enumerate() {
            assert!(v.is_finite(), "month {t}: series {i} is {v}");
            assert!(
                *v >= -CHECK_ABSOLUTE || i == YearSeries::InvestmentReturn.index(),
                "month {t}: series {} is negative ({v})",
                YearSeries::ALL[i].key()
            );
        }
    }

    /// Year-end (or month-end) balances in today's dollars, in the order of
    /// [`BALANCES`]. An overdrawn cash bucket is reported as debt.
    fn balances(&self, end_price: f64) -> [f64; BALANCES.len()] {
        let kinds = self.acc.by_kind();
        let (value, mortgage) = self.property_totals();
        let k = 1.0 / end_price;
        [
            kinds[0].max(0.0) * k,
            kinds[1] * k,
            kinds[2] * k,
            kinds[3] * k,
            kinds[4] * k,
            value * k,
            mortgage * k,
            (self.debt_total() + (-kinds[0]).max(0.0)) * k,
            // Not a balance: vested private-company holdings at the company's price.
            self.eq_paper * k,
        ]
    }

    /// Step 12: adds the month to the year, and the year to the recorder in December
    /// or in the last month of the path.
    pub(crate) fn record<R: Recorder>(
        &mut self,
        plan: &Plan,
        econ: &EconomyPath,
        rec: &mut R,
        t: usize,
    ) {
        let now = plan.start + t as i32;
        self.led[YearSeries::SpendPlanned.index()] = self.planned;
        // The month the last mortgage is paid off (cleared if another is taken out).
        if self
            .props
            .iter()
            .any(|p| p.owned && p.mortgage_balance() > 0.0)
        {
            self.had_mortgage = true;
            self.mortgage_payoff_month = i32::MAX;
        } else if self.had_mortgage && self.mortgage_payoff_month == i32::MAX {
            self.mortgage_payoff_month = t as i32;
        }
        if self.check {
            self.check_end(t);
        }
        let deflate = 1.0 / econ.price_level[t];
        let end_price = econ.price_level[t + 1];
        for (acc, v) in self.yr.values.iter_mut().zip(&self.led) {
            *acc += v * deflate;
        }
        self.yr_months += 1;
        let spending = crate::stats::spending_of(&self.led);
        self.last_spend = spending;

        if self.detail {
            self.record_detail(econ, rec, t, now, spending);
        }

        if now.month() == 12 || t + 1 == self.months {
            // Every balance series is overwritten (the monthly sums of the
            // ledger's zero balance lines are discarded).
            let balances = self.balances(end_price);
            for (s, v) in BALANCES.into_iter().zip(balances) {
                self.yr.values[s.index()] = v;
            }
            self.yr.months = self.yr_months;
            self.yr.retired = self.retired;
            self.yr.status = Status::classify(
                self.retired,
                self.depleted_year,
                self.yr.spending(),
                self.yr.get(YearSeries::SpendPlanned),
                self.yr.investable(),
                self.inv_at_ret,
            );
            rec.record_year((now.year() - plan.start.year()) as usize, &self.yr);
            self.yr = YearRecord::default();
            self.yr_months = 0;
            self.depleted_year = self.depleted_now;
        }
    }

    /// The month's full ledger, tax returns and events, for a trace.
    #[cold]
    #[inline(never)]
    fn record_detail<R: Recorder>(
        &mut self,
        econ: &EconomyPath,
        rec: &mut R,
        t: usize,
        now: crate::model::YearMonth,
        spending: f64,
    ) {
        let now_year = now.year();
        let deflate = 1.0 / econ.price_level[t];
        let end_price = econ.price_level[t + 1];
        // Returns of earlier years go in before this month; the final return
        // (closed in the last month) after it.
        let pending = std::mem::take(&mut self.tax_details);
        for (year, detail) in &pending {
            if *year < now_year {
                rec.record_tax_return(*year, detail);
            }
        }
        let mut m = MonthRecord::new(now);
        for (out, v) in m.values.iter_mut().zip(&self.led) {
            *out = v * deflate;
        }
        for (s, v) in BALANCES.into_iter().zip(self.balances(end_price)) {
            m.values[s.index()] = v;
        }
        m.price_level = end_price;
        m.retired = self.retired;
        m.status = Status::classify(
            self.retired,
            self.depleted_now,
            spending,
            self.planned,
            crate::stats::investable_of(&m.values),
            self.inv_at_ret,
        );
        m.portfolio_return = self.port_return;
        m.asset_returns = econ.returns_at(t);
        rec.record_month(&m);
        for (year, detail) in &pending {
            if *year >= now_year {
                rec.record_tax_return(*year, detail);
            }
        }
        self.emit_market_notes(t, now);
        for event in self.trace_events.drain(..) {
            rec.record_event(event);
        }
    }

    /// Ends the path: the scalars the simulation supplies, and the outcome.
    pub(crate) fn finish<R: Recorder>(&mut self, plan: &Plan, econ: &EconomyPath, rec: &mut R) {
        let months = self.months;
        let end_price = econ.price_level[months];
        let age0 = f64::from(plan.people[0].age0);
        let age_at = |m: i32| (age0 + f64::from(m)) / 12.0;
        let mut end = PathEnd::default();
        if self.fi_month != i32::MAX {
            end.set(PathScalar::FiAge, age_at(self.fi_month));
        }
        if self.retired {
            end.set(PathScalar::RetirementAge, age_at(self.retire_month));
            end.set(PathScalar::InvestableAtRetirement, self.inv_at_ret);
            if self.funded_ratio.is_finite() {
                end.set(PathScalar::FundedRatio, self.funded_ratio);
            }
            if self.ret_months >= 12 {
                let years = f64::from(self.ret_months) / 12.0;
                end.set(
                    PathScalar::EarlyRetirementReturn,
                    math::pow(self.ret_index.max(1e-12), 1.0 / years) - 1.0,
                );
            }
        }
        if self.coast_month != i32::MAX {
            end.set(PathScalar::CoastFiAge, age_at(self.coast_month));
        }
        if self.ever_depleted {
            end.set(PathScalar::DepletionAge, age_at(self.depletion_month));
        }
        if self.home_sale_month != i32::MAX {
            end.set(PathScalar::HomeSaleAge, age_at(self.home_sale_month));
        }
        if self.retired && self.initial_wr.is_finite() {
            end.set(PathScalar::InitialWithdrawalRate, self.initial_wr);
        }
        if self.mortgage_payoff_month != i32::MAX {
            end.set(
                PathScalar::MortgagePayoffAge,
                age_at(self.mortgage_payoff_month),
            );
        }
        if plan.has_equity {
            // The outcome and its age describe the primary company; the four
            // amounts total every included company.
            end.set_equity_outcome(self.companies[plan.primary_company].st.outcome);
            if self.eq_event_month != i32::MAX {
                end.set(PathScalar::EquityEventAge, age_at(self.eq_event_month));
            }
            end.set(PathScalar::EquityGrossProceeds, self.eq_gross_real);
            end.set(PathScalar::EquityExerciseCost, self.eq_cost_real);
            end.set(PathScalar::EquityTenderProceeds, self.eq_tender_real);
            end.set(
                PathScalar::EquityNetProceeds,
                self.eq_gross_real - self.eq_cost_real - self.eq_tax_real,
            );
        }
        let own_age = |i: usize, m: i32| (f64::from(plan.people[i].age0) + f64::from(m)) / 12.0;
        for (i, (claim, stop)) in [
            (
                PathScalar::SocialSecurityClaimAge0,
                PathScalar::PersonRetirementAge0,
            ),
            (
                PathScalar::SocialSecurityClaimAge1,
                PathScalar::PersonRetirementAge1,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if i >= self.n {
                break;
            }
            if self.ss_first_paid[i] != i32::MAX {
                // A benefit already in payment when the plan starts was claimed
                // at the age the profile gives, not today.
                let age = own_age(i, self.ss_first_paid[i]);
                let age = if self.ss_first_paid[i] == 0 {
                    age.min(plan.people[i].claim_age.max(62.0))
                } else {
                    age
                };
                end.set(claim, age);
            }
            if self.stopped_work[i] != i32::MAX {
                end.set(stop, own_age(i, self.stopped_work[i]));
            }
        }
        for (i, scalar) in [PathScalar::DeathAge0, PathScalar::DeathAge1]
            .into_iter()
            .enumerate()
        {
            if i < self.n {
                let lived = self.death_month[i].min(months as u32);
                end.set(
                    scalar,
                    (f64::from(plan.people[i].age0) + f64::from(lived)) / 12.0,
                );
            }
        }
        end.set(PathScalar::WorstDrawdown, self.worst_dd);
        let mut unemployed = 0.0;
        for rt in &self.jobs {
            unemployed += rt.st.months_unemployed_total;
        }
        end.set(PathScalar::MonthsUnemployed, unemployed);
        end.set(PathScalar::LongTermCareCost, self.ltc_real);
        let inflation = math::pow(end_price, 12.0 / months as f64) - 1.0;
        end.set(PathScalar::Inflation, inflation);

        // Outcome.
        let kinds = self.acc.by_kind();
        let (value, mortgage) = self.property_totals();
        let mut estate = kinds.iter().sum::<f64>() + value - mortgage - self.debt_total();
        let bequest = plan.profile.retirement.bequest_target;
        let age_end = age_at(months as i32);
        let everyone_died = (0..self.n).all(|i| self.death_month[i] as usize <= months);
        if plan.care.active {
            if self.care.medicaid_month != i32::MAX {
                end.set(PathScalar::MedicaidAge, age_at(self.care.medicaid_month));
            }
            estate -= self.care_estate_recovery(plan, months, estate, everyone_died, end_price);
            for event in self.trace_events.drain(..) {
                rec.record_event(event);
            }
        }
        let estate = estate / end_price;
        let retired_in_time =
            self.retired || (everyone_died && age_end < plan.latest_retirement_age);
        let success =
            !self.ever_depleted && retired_in_time && (bequest <= 0.0 || estate >= bequest);
        let cause = if success {
            None
        } else {
            Some(self.failure_cause(plan, unemployed, inflation))
        };
        end.set_outcome(cause);
        rec.end_path(&end);
    }

    /// The first cause of a failed path (see the module documentation).
    fn failure_cause(&self, plan: &Plan, unemployed: f64, inflation: f64) -> FailureCause {
        let budget = (self.ess_real + self.disc_real) * 12.0;
        // Care paid for before the money ran out (all of it on a path that never
        // ran out but fell short of its bequest).
        let care = if self.ever_depleted {
            self.ltc_at_depletion
        } else {
            self.ltc_real
        };
        let care_threshold = if self.inv_at_ret.is_finite() && self.inv_at_ret > 0.0 {
            0.25 * self.inv_at_ret
        } else {
            2.0 * budget
        };
        if care > 0.0 && care >= care_threshold {
            return FailureCause::Health;
        }
        let depleted_working =
            self.ever_depleted && (!self.retired || self.depletion_month < self.retire_month);
        // Unemployment that came before the money ran out is what can have caused it.
        let unemployed = if self.ever_depleted {
            self.unemployed_at_depletion
        } else {
            unemployed
        };
        if depleted_working {
            // Running out while still working is not a market or a longevity
            // story: either pay stopped for a long time or the household spent
            // more than it earned.
            return if unemployed >= 6.0 {
                FailureCause::JobLoss
            } else {
                FailureCause::Overspending
            };
        }
        if unemployed >= 18.0 {
            return FailureCause::JobLoss;
        }
        if inflation >= 0.045 {
            return FailureCause::Inflation;
        }
        if self.retired && self.ret_months >= 12 {
            let years = f64::from(self.ret_months) / 12.0;
            let real = math::pow(self.ret_index.max(1e-12), 1.0 / years) - 1.0;
            if real < plan.expected_real_return - 0.02 {
                return FailureCause::MarketSequence;
            }
        }
        if self.ever_depleted {
            let age = (f64::from(plan.people[0].age0) + f64::from(self.depletion_month)) / 12.0;
            if age >= 90.0 {
                return FailureCause::Longevity;
            }
        }
        if (self.initial_wr.is_finite() && self.initial_wr > 0.05) || self.disc_mult > 1.0 {
            return FailureCause::Overspending;
        }
        FailureCause::Other
    }
}
