//! One month of one life, in the order of `docs/architecture.md` section 3.
//!
//! Every dollar goes through the cash bucket: income is received into it
//! ([`State::recv`]), everything paid leaves it ([`State::pay`]), and the settlement
//! step then either invests what is above the cash target or raises what is missing
//! from the other buckets. A month therefore cannot create or lose money without the
//! ledger showing it; `record.rs` asserts this in check mode.

use crate::accounts::{Bucket, PayPeriod, Weights};
use crate::career::{CareerClimate, Employment};
use crate::decumulation::{cover_shortfall, execute_plan, Executed};
use crate::economy::EconomyPath;
use crate::health::{CoveragePhase, HealthContext, LTC_FACILITY_SPENDING_OFFSET};
use crate::math;
use crate::model::{
    AssetClass, Category, Cola, DebtStrategy, EmployerPlan, HomeEquity, PropertyUse, N_ASSETS,
};
use crate::stats::{Recorder, TraceEventKind, YearSeries};
use crate::MAX_PEOPLE;

use super::plan::{Plan, StreamKind, CHILD_SERVICE_REAL_GROWTH};
use super::state::{State, TAX_FED, TAX_PAYROLL, TAX_SERIES, TAX_STATE};

/// Vested employer shares that are kept are held as the single-stock asset class.
const SINGLE_STOCK_MIX: Weights = {
    let mut w = [0.0; N_ASSETS];
    w[AssetClass::SingleStock as usize] = 1.0;
    w
};

/// Minimum payment on revolving debt: 2% of the balance, at least $35
/// (`model::Debt::payment`).
const REVOLVING_MIN_FRACTION: f64 = 0.02;
const REVOLVING_MIN_PAYMENT: f64 = 35.0;

/// Equities count as "in a drawdown" for the retirement cash bucket when the US
/// stock index is more than 10% below its previous peak (a correction).
const DRAWDOWN_THRESHOLD: f64 = 0.90;

/// Amounts below half a cent are rounding, not money.
const DUST: f64 = 0.005;

/// The October re-plan of a household on a marketplace plan is made only when the
/// year's income so far, annualised, is above this share of the premium-credit
/// cliff (the January plan aims 8% below the cliff).
const OCTOBER_REPLAN_SHARE: f64 = 0.95;

impl State {
    /// Simulates month `t`.
    pub(crate) fn month<R: Recorder>(
        &mut self,
        plan: &Plan,
        econ: &mut EconomyPath,
        rec: &mut R,
        t: usize,
    ) {
        let now = plan.start + t as i32;
        let month = now.month();
        let ti = t as i32;
        self.led = [0.0; super::state::N_SERIES];
        self.planned = 0.0;
        self.wage_like = 0.0;
        self.cash_start = self.acc.cash;
        if self.check {
            self.check_begin();
        }

        // 1. Calendar: deaths, a partner joining, retirement dates, the annual plan.
        let mut replan = t == 0 || month == 1;
        for i in 0..self.n {
            if self.alive[i] && t as u32 >= self.death_month[i] {
                self.die(plan, i, t);
                replan = true;
            }
        }
        if self.n == 2 && self.alive[1] && !self.present[1] && ti >= plan.people[1].joins {
            self.join(plan, econ.price_level[t], t);
            replan = true;
        }
        if !self.retired && ti >= self.retire_month {
            self.begin_retirement(plan, econ, t);
            replan = true;
        }
        for p in 0..self.n {
            if !self.person_retired[p] && ti >= self.person_retire_month(plan, p) {
                self.retire_person(plan, p, t);
                replan = true;
            }
        }
        for j in 0..self.jobs.len() {
            let jp = &plan.jobs[j];
            let age = self.age(plan, jp.owner, t);
            let rt = &mut self.jobs[j];
            if !rt.begun && !rt.ended && ti >= jp.start.month(self.retire_month) {
                if self.present[jp.owner]
                    && !(jp.ends_at_retirement && self.person_retired[jp.owner])
                {
                    jp.model.begin(&mut rt.st, age);
                    rt.begun = true;
                    self.req_dirty = true;
                } else if !self.alive[jp.owner] {
                    rt.ended = true;
                }
            }
            if !rt.ended && ti >= jp.end.month(self.retire_month) {
                jp.model.end(&mut rt.st);
                rt.ended = true;
                self.req_dirty = true;
            }
        }
        // The turn of the year comes before January's events, so that what they add
        // to the tax year (a sale's gain, a taxable windfall) is on this year's
        // return and not on the one being closed.
        let new_year = month == 1 && t > 0;
        if new_year {
            self.close_year(plan, econ, t);
        }
        // On a marketplace plan the year is re-planned in October on what has
        // actually been earned and realised, so that the last quarter's money comes
        // from sources that keep income under the premium-credit cliff.
        if month == 10
            && self.has_plan
            && self.pinputs.on_aca
            && self.guards_aca_credit(plan)
            && self.aca_cliff_in_reach(t)
        {
            replan = true;
        }
        // 2. Events due this month (before planning, so the plan sees them).
        if ti >= self.next_event {
            self.run_events(plan, econ, t);
            replan = true;
        }
        let price = econ.price_level[t];
        if replan {
            self.annual_plan(plan, econ, t, new_year);
        }
        if self.budget_dirty || ti >= self.next_item_change {
            self.rebudget(plan, t);
        }

        // 3-5. Careers, pay, payroll deductions.
        let eq_return = econ.returns[AssetClass::UsStocks.index()][t];
        self.track_equity(eq_return, t, plan.eq12_months & (1 << month) != 0);
        let recession = econ.recession[t];
        let mut employer_coverage = false;
        for j in 0..self.jobs.len() {
            let jp = &plan.jobs[j];
            let p = jp.owner;
            if !self.alive[p] {
                continue;
            }
            let climate = CareerClimate {
                now,
                age: self.age(plan, p, t),
                price_level: price,
                recession,
                equity_return: eq_return,
                equity_return_12m: self.eq12,
            };
            if jp.model.is_inert(&self.jobs[j].st) {
                jp.model
                    .skip(month == 1, &mut self.rng_career[p], &mut self.rng_equity);
                continue;
            }
            let jm = jp.model.step(
                &mut self.jobs[j].st,
                &climate,
                &mut self.rng_career[p],
                &mut self.rng_equity,
            );
            if !self.present[p] {
                continue;
            }
            if matches!(
                self.jobs[j].st.employment,
                Employment::Employed | Employment::OnBreak { .. }
            ) {
                employer_coverage = true;
            }
            let gross = jm.gross();
            if gross > 0.0 {
                self.recv(YearSeries::IncomeWages, gross);
                self.ty.wages[p] += gross;
                self.year_earned[p] += gross;
                self.wage_like += gross;
                let pay = jm.wages + jm.bonus + jm.commission;
                if let (Some(ep), true) = (self.jobs[j].plan, pay > 0.0) {
                    let d = self.acc.payroll_deferral(
                        p,
                        &ep,
                        &PayPeriod {
                            gross_pay: pay,
                            month,
                            age: self.age_ye[p],
                        },
                        &self.params.federal.limits,
                        &self.target,
                    );
                    self.acc.cash -= d.employee_pretax + d.employee_roth;
                    self.led[YearSeries::SaveTraditional.index()] += d.employee_pretax;
                    self.led[YearSeries::SaveRoth.index()] += d.employee_roth;
                    self.led[YearSeries::SaveEmployer.index()] += d.employer;
                    self.ty.pretax_deferrals[p] += d.employee_pretax;
                }
                let mut hsa_payroll = 0.0;
                if jm.wages > 0.0 && (jp.hsa_real > 0.0 || jp.hsa_employer_real > 0.0) {
                    let other_age = self.age_ye[MAX_PEOPLE - 1 - p];
                    let family = self.sctx.hsa_family;
                    let room = self.acc.hsa_room(
                        p,
                        self.age_ye[p],
                        other_age,
                        family,
                        &self.params.federal.limits,
                    );
                    if room > 0.0 {
                        let want = jp.hsa_real / 12.0 * price;
                        let mix = self.acc.buy_mix(want, &self.target);
                        hsa_payroll = self.acc.contribute_hsa(p, want.min(pay), room, &mix);
                        self.acc.cash -= hsa_payroll;
                        self.led[YearSeries::SaveHsa.index()] += hsa_payroll;
                        self.ty.hsa_contributions[p] += hsa_payroll;
                        let employer = self.acc.contribute_hsa(
                            p,
                            jp.hsa_employer_real / 12.0 * price,
                            room - hsa_payroll,
                            &mix,
                        );
                        self.led[YearSeries::SaveEmployer.index()] += employer;
                    }
                }
                if self.fica_on {
                    let f = &self.params.federal;
                    let fica_wages = (gross - hsa_payroll).max(0.0);
                    let before = self.ytd_fica_wages[p];
                    let after = before + fica_wages;
                    let fica = f.oasdi_rate
                        * (after.min(f.oasdi_wage_base) - before.min(f.oasdi_wage_base))
                        + f.medicare_rate * fica_wages;
                    self.ytd_fica_wages[p] = after;
                    self.pay(YearSeries::TaxPayroll, fica);
                    self.paid[TAX_PAYROLL] += fica;
                }
                if jm.equity_retained > 0.0 {
                    // Shares kept after vesting: the tax on them is withheld from
                    // other pay, the net value moves into the single-stock sleeve.
                    let payroll = if self.fica_on {
                        self.params.federal.oasdi_rate + self.params.federal.medicare_rate
                    } else {
                        0.0
                    };
                    let net = jm.equity_retained * (1.0 - self.wh_rate - payroll).max(0.0);
                    self.acc.cash -= net;
                    self.acc.contribute(Bucket::Taxable, net, &SINGLE_STOCK_MIX);
                    self.acc.keep_single_stock = true;
                    self.led[YearSeries::SaveTaxable.index()] += net;
                }
            }
            if jm.unemployment_benefit > 0.0 {
                self.recv(YearSeries::IncomeOther, jm.unemployment_benefit);
                self.ty.other_ordinary += jm.unemployment_benefit;
                self.wage_like += jm.unemployment_benefit;
            }
            if jm.laid_off {
                self.req_dirty = true;
                self.event(now, TraceEventKind::LaidOff, None, || {
                    format!("Laid off from {}", jp.name)
                });
            }
            if jm.rehired {
                self.req_dirty = true;
                let salary = self.jobs[j].st.real_salary;
                self.event(now, TraceEventKind::Rehired, Some(salary), || {
                    format!("Back at work ({})", jp.name)
                });
            }
            if self.detail && jm.equity_income > 0.0 && jm.wages <= 0.0 && jm.bonus <= 0.0 {
                self.event(
                    now,
                    TraceEventKind::EquityEvent,
                    Some(jm.equity_income / price),
                    || format!("Equity payout from {}", jp.name),
                );
            }
        }
        for k in 0..self.selfemp.len() {
            let sp = &plan.selfemp[k];
            let p = sp.owner;
            if !self.alive[p] {
                continue;
            }
            let climate = CareerClimate {
                now,
                age: self.age(plan, p, t),
                price_level: price,
                recession,
                equity_return: eq_return,
                equity_return_12m: self.eq12,
            };
            let years = self.se_years(plan, k, t);
            let rt = &mut self.selfemp[k];
            let profit = sp
                .model
                .step(&mut rt.st, years, &climate, &mut self.rng_career[p])
                * rt.scale;
            let rm = self.retire_month;
            let active = self.present[p]
                && !rt.ended
                && ti >= sp.start.month(rm)
                && ti < sp.end.month(rm)
                && !(sp.ends_at_retirement && self.person_retired[p]);
            if active && profit > 0.0 {
                self.recv(YearSeries::IncomeSelfEmployment, profit);
                self.ty.self_employment[p] += profit;
                self.year_earned[p] += profit;
                self.wage_like += profit;
                if sp.solo_fraction > 0.0 {
                    let solo = EmployerPlan {
                        employee_fraction: sp.solo_fraction,
                        employee_max: false,
                        roth_fraction: 0.0,
                        match_rate: 0.0,
                        match_up_to: 0.0,
                        after_tax_mega: false,
                    };
                    let d = self.acc.payroll_deferral(
                        p,
                        &solo,
                        &PayPeriod {
                            gross_pay: profit,
                            month,
                            age: self.age_ye[p],
                        },
                        &self.params.federal.limits,
                        &self.target,
                    );
                    self.acc.cash -= d.employee_pretax;
                    self.led[YearSeries::SaveTraditional.index()] += d.employee_pretax;
                    self.ty.pretax_deferrals[p] += d.employee_pretax;
                }
            }
        }

        // 4. Other income: pensions, annuities, other streams, Social Security.
        self.pay_streams(plan, t, price);
        if t >= self.ss_from {
            // Benefits in real terms change only in January (earnings test, payable
            // fraction), at an age threshold, or when someone dies or joins; in
            // between they rise with prices, so the last result is rescaled.
            if month == 1 || self.ss_dirty || ti >= self.ss_next_change {
                self.ss_cache = plan.ss.monthly(now, &self.claimants, price);
                self.ss_cache_price = price;
                self.ss_dirty = false;
                self.ss_next_change = self.next_ss_threshold(plan, t);
            }
            let scale = price / self.ss_cache_price;
            let benefits = self.ss_cache;
            for i in 0..self.n {
                let paid = benefits.paid[i] * scale;
                if paid > 0.0 && self.present[i] {
                    self.recv(YearSeries::IncomeSocialSecurity, paid);
                    self.ty.social_security[i] += paid;
                    self.wage_like += paid;
                    if !self.ss_claimed[i] {
                        self.ss_claimed[i] = true;
                        self.ss_first_paid[i] = ti;
                        self.req_dirty = true;
                        self.event(
                            now,
                            TraceEventKind::ClaimedSocialSecurity,
                            Some(paid * 12.0 / price),
                            || format!("Person {} starts Social Security", i + 1),
                        );
                    }
                }
                if benefits.withheld_months[i] > 0.0 {
                    if let Some(c) = &mut self.claimants[i] {
                        c.withheld_months += benefits.withheld_months[i];
                    }
                }
            }
        }

        // 6. Spending: the budget, children, healthcare, shocks.
        let mut facility = 0.0;
        let mut adults = 0.0;
        for i in 0..self.n {
            if self.present[i] {
                adults += 1.0;
                if self.in_facility[i] {
                    facility += 1.0;
                }
            }
        }
        // Someone living in a care facility no longer spends their share of the
        // household's ordinary budget in full.
        let living = if facility > 0.0 && adults > 0.0 {
            1.0 - LTC_FACILITY_SPENDING_OFFSET * facility / adults
        } else {
            1.0
        };
        let essential = self.ess_real * price * living;
        let discretionary = self.disc_real * price * living;
        self.spend(YearSeries::SpendEssential, essential);
        self.pay(
            YearSeries::SpendDiscretionary,
            discretionary * self.disc_mult,
        );
        self.planned += discretionary;
        if ti < self.kids_until {
            // A child's cost changes on a birthday and each January.
            if t % 12 == 0 || plan.birthday_months & (1 << (t % 12)) != 0 || self.kids_real < 0.0 {
                self.kids_real = self.children_cost(plan, t);
            }
            let kids = self.kids_real * price;
            if kids > 0.0 {
                self.spend(YearSeries::SpendChildren, kids);
            }
        }
        self.coverage_gap = if employer_coverage {
            0
        } else {
            self.coverage_gap.saturating_add(1)
        };
        let mut aca_premiums = 0.0;
        let mut aca_people = 0u32;
        let mut health_cost = 0.0;
        for i in 0..self.n {
            if !self.alive[i] {
                continue;
            }
            let ctx = HealthContext {
                age: self.age(plan, i, t),
                employer_coverage,
                months_since_coverage_ended: self.coverage_gap,
                in_us: self.in_us,
                months_since_start: t as u32,
                price_level: price,
            };
            let hm = plan
                .health
                .step(&mut self.health[i], &ctx, &mut self.rng_health[i]);
            if !self.present[i] {
                continue;
            }
            health_cost += hm.premium + hm.out_of_pocket;
            match hm.phase {
                CoveragePhase::Aca => {
                    aca_premiums += hm.premium;
                    aca_people += 1;
                    self.ty.aca_benchmark_premium += hm.aca_benchmark;
                }
                CoveragePhase::Medicare => {
                    health_cost += self.irmaa_m[i];
                    self.ty.on_medicare[i] = true;
                }
                _ => {}
            }
            if hm.long_term_care > 0.0 {
                self.spend(YearSeries::SpendLongTermCare, hm.long_term_care);
                self.ltc_real += hm.long_term_care / price;
            }
            self.in_facility[i] = hm.ltc_in_facility;
            if hm.ltc_started {
                self.event(now, TraceEventKind::LongTermCare, None, || {
                    format!("Person {} begins long-term care", i + 1)
                });
            }
            if self.detail && hm.phase != self.last_phase[i] {
                let phase = hm.phase;
                self.event(now, TraceEventKind::HealthCoverage, None, || {
                    format!("Person {} health coverage: {:?}", i + 1, phase)
                });
            }
            self.last_phase[i] = hm.phase;
        }
        if aca_people > 0 {
            self.ty.aca_months += 1;
            self.aca_people = self.aca_people.max(aca_people);
            self.aca_premium_ytd += aca_premiums;
            // The advance premium credit lowers what the household pays now; the
            // return reconciles it against the credit actually earned.
            let advance = self.aca_advance_m.min(aca_premiums);
            self.aca_advance_ytd += advance;
            self.planned += health_cost;
            self.pay(YearSeries::SpendHealthcare, health_cost - advance);
            self.planned -= advance;
        } else if health_cost > 0.0 {
            self.spend(YearSeries::SpendHealthcare, health_cost);
        }
        {
            let u = self.rng_shocks.uniform();
            let bits = self.rng_shocks.next_u64();
            if u < plan.shocks.monthly_probability {
                let z = crate::rng::normal_from_bits(bits);
                let annual = (self.ess_real + self.disc_real) * 12.0 * price;
                let shock = annual * math::exp(plan.shocks.log_median + plan.shocks.sigma * z);
                if shock > 0.0 {
                    self.spend(YearSeries::SpendOneOff, shock);
                    self.event(
                        now,
                        TraceEventKind::ExpenseShock,
                        Some(shock / price),
                        || "Unplanned expense".to_string(),
                    );
                }
            }
        }

        // 7. Housing.
        let home_national = econ.home_national[t];
        for i in 0..self.props.len() {
            let st = &self.props[i];
            let owned = st.owned;
            let rental = matches!(st.usage, PropertyUse::Rental(_));
            let value_before = st.value;
            let balance = st.mortgage_balance();
            let hm = plan.props[i].model.step(
                &mut self.props[i],
                home_national,
                price,
                &mut self.rng_housing[i],
            );
            if !owned {
                continue;
            }
            if self.check {
                self.chk_prop_flow += self.props[i].value - value_before;
                self.chk_mort_flow -= hm.principal;
            }
            if rental {
                let noi = hm.net_operating_income();
                if noi >= 0.0 {
                    self.recv(YearSeries::IncomeRental, noi);
                } else {
                    self.spend(YearSeries::SpendHousing, -noi);
                }
                self.spend(
                    YearSeries::SpendHousing,
                    hm.interest + hm.principal + hm.pmi,
                );
                self.ty.rental_net += hm.rental_taxable_income;
            } else {
                self.spend(YearSeries::SpendHousing, hm.cash_out());
                let deductible = if balance > plan.acquisition_debt_limit {
                    plan.acquisition_debt_limit / balance
                } else {
                    1.0
                };
                self.ty.mortgage_interest += hm.deductible_mortgage_interest * deductible;
                self.ty.property_tax += hm.property_tax;
            }
        }
        self.home_index *= 1.0 + home_national;

        // 8. Debts: required payments, then the extra payment by strategy.
        self.pay_debts(plan, t, price);

        // Tax withheld and estimated payments, then the April settlement.
        let prepay = self.wh_rate * self.wage_like + self.installment;
        if prepay > 0.0 {
            let state = prepay * self.share_state;
            let se = prepay * self.share_se;
            let federal = prepay - state - se;
            self.pay(YearSeries::TaxFederal, federal);
            self.pay(YearSeries::TaxState, state);
            self.pay(YearSeries::TaxPayroll, se);
            self.paid[TAX_FED] += federal;
            self.paid[TAX_STATE] += state;
            self.paid[TAX_PAYROLL] += se;
        }
        if month == 4 {
            self.pay_tax_due();
        }
        // The last month of a life: the final return is settled before the estate
        // is counted.
        if t + 1 == self.months {
            self.close_tax_year(plan, now.year(), true);
            self.pay_tax_due();
        }

        // 9. Cash settlement.
        self.settle(plan, t, price);

        // 10. Returns.
        let returns = econ.returns_at(t);
        let mut yields = econ.income_yields_at(t);
        yields[AssetClass::Custom.index()] = plan.custom_yield;
        let cash_before = self.acc.cash;
        let inc = self.acc.apply_returns(
            &returns,
            plan.cash_terms
                .monthly_rate_from_bill_return(returns[AssetClass::Cash.index()]),
            plan.fee_monthly,
            &yields,
        );
        self.ty.interest += inc.interest;
        self.ty.ordinary_dividends += inc.ordinary_dividends;
        self.ty.qualified_dividends += inc.qualified_dividends;
        self.led[YearSeries::InvestmentIncome.index()] =
            inc.interest + inc.ordinary_dividends + inc.qualified_dividends;
        self.led[YearSeries::InvestmentReturn.index()] = inc.net_change;
        if self.check {
            self.chk_ret_other = inc.net_change - (self.acc.cash - cash_before);
        }
        let investable = self.acc.investable();
        let before = investable - inc.net_change;
        self.port_return = if before > 0.0 {
            inc.net_change / before
        } else {
            0.0
        };
        self.track_portfolio(self.port_return, econ.inflation[t]);
        if let Some(width) = plan.rebalance_band {
            if self.acc.max_drift(&self.target) > width {
                self.rebalance(plan);
            }
        }

        // 11. Financial independence and the retirement trigger.
        if !self.retired {
            self.check_trigger(plan, t, econ.price_level[t + 1], investable);
        }

        // 12. Record.
        self.record(plan, econ, rec, t);
    }

    /// The next month after `t` at which someone's Social Security status can
    /// change with age: 60 (survivor benefits), 62, the planned claim age, full
    /// retirement age and 70.
    fn next_ss_threshold(&self, plan: &Plan, t: usize) -> i32 {
        let mut next = i32::MAX;
        for (i, person) in plan.people.iter().enumerate() {
            if i >= self.n || !self.alive[i] {
                continue;
            }
            let fra = plan.ss.full_retirement_age_months(person.birth.year()) as i32;
            let claim = (person.claim_age.clamp(62.0, 70.0) * 12.0).round() as i32;
            let withheld = self.claimants[i].map_or(0.0, |c| c.withheld_months).ceil() as i32;
            for age in [
                60 * 12,
                62 * 12,
                claim,
                claim + withheld,
                fra,
                fra + withheld,
                70 * 12,
            ] {
                let month = age - person.age0;
                if month > t as i32 {
                    next = next.min(month);
                }
            }
        }
        next
    }

    /// True when the income of the tax year so far, kept up at the same pace, would
    /// end the year within [`OCTOBER_REPLAN_SHARE`] of the premium-credit cliff or
    /// beyond it: the case in which the October re-plan can change anything. (A year
    /// comfortably under the cliff keeps January's plan.)
    fn aca_cliff_in_reach(&self, t: usize) -> bool {
        let elapsed = t.min(9);
        if elapsed == 0 {
            return true;
        }
        let y = &self.ty;
        let mut so_far = y.interest
            + y.ordinary_dividends
            + y.qualified_dividends
            + y.short_term_gains
            + y.long_term_gains
            + y.traditional_distributions
            + y.roth_conversions
            + y.pensions
            + y.rental_net.max(0.0)
            + y.other_ordinary;
        for p in 0..MAX_PEOPLE {
            so_far += y.wages[p] + y.self_employment[p] + y.social_security[p];
        }
        let people = f64::from(self.pinputs.base_tax_inputs.aca_household_size.max(1));
        let f = &self.params.federal;
        let cliff = crate::decumulation::ACA_CLIFF_FPL_MULTIPLE
            * (f.fpl_base + f.fpl_per_person * (people - 1.0));
        so_far * 12.0 / elapsed as f64 > OCTOBER_REPLAN_SHARE * cliff
    }

    /// Years since self-employment income `k` began (since the plan's start for
    /// one already running): the clock its real growth runs on, so that an income
    /// that starts later starts at the amount entered.
    pub(crate) fn se_years(&self, plan: &Plan, k: usize, t: usize) -> f64 {
        let start = plan.selfemp[k].start.month(self.retire_month);
        if start == i32::MAX {
            return 0.0;
        }
        f64::from((t as i32 - start.max(0)).max(0)) / 12.0
    }

    /// Month in which person `p` stops working.
    fn person_retire_month(&self, plan: &Plan, p: usize) -> i32 {
        if self.retire_month == i32::MAX {
            return i32::MAX;
        }
        if p == 0 {
            self.retire_month
        } else {
            self.retire_month.saturating_add(plan.partner_offset).max(0)
        }
    }

    /// Person `p` stops working: their jobs that end at retirement end now.
    fn retire_person(&mut self, plan: &Plan, p: usize, t: usize) {
        self.person_retired[p] = true;
        if self.stopped_work[p] == i32::MAX {
            self.stopped_work[p] = t as i32;
        }
        for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
            if jp.owner == p && jp.ends_at_retirement && !rt.ended {
                jp.model.end(&mut rt.st);
                rt.ended = true;
            }
        }
        if let Some(c) = &mut self.claimants[p] {
            c.annual_earnings = 0.0;
        }
        self.ss_dirty = true;
        self.req_dirty = true;
    }

    /// The household retires in month `t`.
    fn begin_retirement(&mut self, plan: &Plan, econ: &mut EconomyPath, t: usize) {
        let now = plan.start + t as i32;
        let price = econ.price_level[t];
        // Judge the plan as it stood on the day: how well funded was it?
        let rule = self.funding_rule(plan);
        self.req = self.requirement(plan, t, price, &rule);
        let wealth = self.wealth(rule.basis, price);
        if self.req.total > 0.0 {
            self.funded_ratio = wealth / self.req.total;
        }
        if self.fi_month == i32::MAX && wealth >= self.req.total {
            self.fi_month = t as i32;
        }
        self.retired = true;
        self.retire_month = t as i32;
        self.inv_at_ret = self.acc.investable() / price;
        self.ret_index = 1.0;
        self.ret_months = 0;
        self.req_dirty = true;
        self.budget_dirty = true;
        // Dates that were waiting for the retirement month.
        for (rt, ev) in self.events.iter_mut().zip(&plan.events) {
            if !rt.done && rt.month == i32::MAX {
                if let super::plan::Sched::Retire(off) = ev.when {
                    rt.month = (t as i32 + off + rt.shift).max(t as i32);
                    self.next_event = self.next_event.min(rt.month);
                }
            }
        }
        for stress in &plan.stresses {
            self.apply_stress(plan, econ, stress, t, true);
        }
        let amount = self.inv_at_ret;
        self.event(now, TraceEventKind::Retired, Some(amount), || {
            "Retired".to_string()
        });
    }

    /// Person `i` dies at the start of month `t`.
    fn die(&mut self, plan: &Plan, i: usize, t: usize) {
        let now = plan.start + t as i32;
        let was_present = self.present[i];
        self.alive[i] = false;
        self.present[i] = false;
        let survivor = (0..self.n).find(|&o| o != i && self.present[o]);
        if was_present {
            self.acc.inherit(i, survivor);
            if survivor.is_some() {
                self.survivor = true;
                self.death_year = now.year();
            }
        }
        for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
            if jp.owner == i && !rt.ended {
                jp.model.end(&mut rt.st);
                rt.ended = true;
            }
        }
        if let Some(c) = &mut self.claimants[i] {
            c.alive = false;
            c.died = Some(now);
            c.annual_earnings = 0.0;
        }
        self.in_facility[i] = false;
        self.ss_dirty = true;
        self.budget_dirty = true;
        self.req_dirty = true;
        let age = self.age(plan, i, t);
        self.event(now, TraceEventKind::Died, Some(age), || {
            format!("Person {} dies at {:.0}", i + 1, age.floor())
        });
    }

    /// The second person joins the household in month `t` with their accounts, which
    /// are taken at their stated value in today's dollars.
    fn join(&mut self, plan: &Plan, price: f64, t: usize) {
        let now = plan.start + t as i32;
        self.present[1] = true;
        let theirs = &plan.joiner_accounts;
        let kinds = theirs.by_kind();
        self.acc.holdings_changed();
        self.acc.cash += theirs.cash * price;
        for a in 0..N_ASSETS {
            self.acc.taxable.value[a] += theirs.taxable.value[a] * price;
            self.acc.taxable.basis[a] += theirs.taxable.basis[a] * price;
            for p in 0..MAX_PEOPLE {
                self.acc.person[p].traditional[a] += theirs.person[p].traditional[a] * price;
                self.acc.person[p].roth.value[a] += theirs.person[p].roth.value[a] * price;
                self.acc.person[p].hsa[a] += theirs.person[p].hsa[a] * price;
            }
        }
        for p in 0..MAX_PEOPLE {
            let from = &theirs.person[p].roth;
            let to = &mut self.acc.person[p].roth;
            to.contribution_basis += from.contribution_basis * price;
            to.opened |= from.opened;
            to.years_open = to.years_open.max(from.years_open);
        }
        let total: f64 = kinds.iter().sum::<f64>() * price;
        self.led[YearSeries::IncomeOther.index()] += total;
        self.led[YearSeries::SaveTaxable.index()] += kinds[1] * price;
        self.led[YearSeries::SaveTraditional.index()] += kinds[2] * price;
        self.led[YearSeries::SaveRoth.index()] += kinds[3] * price;
        self.led[YearSeries::SaveHsa.index()] += kinds[4] * price;
        let person = &plan.people[1];
        let pia = match person.basis {
            crate::benefits::BenefitBasis::KnownPia(x) => Some(x),
            crate::benefits::BenefitBasis::FromEarnings => {
                Some(plan.ss.pia_real(&self.records[1], person.birth, now, price))
            }
            crate::benefits::BenefitBasis::None => None,
        };
        self.claimants[1] = Some(crate::benefits::Claimant::new(
            person.birth,
            person.claim_age,
            pia,
        ));
        self.budget_dirty = true;
        self.ss_dirty = true;
        self.req_dirty = true;
        self.event(
            now,
            TraceEventKind::PartnerJoined,
            Some(total / price),
            || "Partner joins the household".to_string(),
        );
    }

    /// Trailing 12-month US stock return (computed only in the months that use it)
    /// and the stock index for the drawdown flag.
    fn track_equity(&mut self, eq_return: f64, t: usize, need_trailing: bool) {
        self.eq_ring[t % 12] = eq_return;
        if need_trailing {
            let mut growth = 1.0;
            for r in &self.eq_ring {
                growth *= 1.0 + r;
            }
            self.eq12 = growth - 1.0;
        }
        self.eq_index *= 1.0 + eq_return;
        // A select, not a branch: whether a month sets a new high is close to a
        // coin toss, which a branch predictor cannot learn.
        self.eq_peak = if self.eq_index > self.eq_peak {
            self.eq_index
        } else {
            self.eq_peak
        };
    }

    /// True while US stocks are in a correction.
    pub(crate) fn in_drawdown(&self) -> bool {
        self.eq_index < DRAWDOWN_THRESHOLD * self.eq_peak
    }

    /// Real portfolio return index, its worst drawdown and the retirement decade.
    fn track_portfolio(&mut self, r: f64, inflation: f64) {
        let real = (1.0 + r) / (1.0 + inflation);
        self.year_port *= 1.0 + r;
        self.port_index *= real;
        // Written as selects rather than branches. The index is the end of the
        // month's longest chain of arithmetic (every holding's return, their sum,
        // two divisions), and whether it sets a new high is close to a coin toss:
        // as a branch it is mispredicted often and each miss waits for the whole
        // chain. The values stored are the same.
        let index = self.port_index;
        let peak = self.port_peak;
        let new_high = index > peak;
        let dd = 1.0 - index / peak;
        let deeper = !new_high && peak > 0.0 && dd > self.worst_dd;
        self.port_peak = if new_high { index } else { peak };
        self.worst_dd = if deeper { dd } else { self.worst_dd };
        if self.retired && self.ret_months < 120 {
            self.ret_index *= real;
            self.ret_months += 1;
        }
    }

    /// Pensions, annuities and other income streams.
    fn pay_streams(&mut self, plan: &Plan, t: usize, price: f64) {
        let ti = t as i32;
        let rm = self.retire_month;
        for k in 0..self.streams.len() {
            let sp = &plan.streams[k];
            let p = sp.owner;
            let mut rt = self.streams[k];
            if let StreamKind::Other { growth, .. } = sp.kind {
                if growth != 0.0 && t > 0 && t % 12 == 0 {
                    rt.level *= 1.0 + growth;
                }
            }
            rt.last = 0.0;
            if rt.ended || ti < sp.start.month(rm) || ti < plan.people[p].joins {
                self.streams[k] = rt;
                continue;
            }
            if ti >= sp.end.month(rm) {
                rt.ended = true;
                self.streams[k] = rt;
                self.req_dirty = true;
                continue;
            }
            let mut share = 1.0;
            if !self.alive[p] {
                // A joint-and-survivor pension keeps paying the surviving partner.
                let partner = (0..self.n).any(|o| o != p && self.present[o]);
                match sp.kind {
                    StreamKind::Pension {
                        survivor_fraction, ..
                    } if partner && survivor_fraction > 0.0 => share = survivor_fraction,
                    _ => {
                        rt.ended = true;
                        self.streams[k] = rt;
                        continue;
                    }
                }
            }
            if let StreamKind::Other {
                ends_at_retirement: true,
                ..
            } = sp.kind
            {
                if self.person_retired[p] {
                    rt.ended = true;
                    self.streams[k] = rt;
                    continue;
                }
            }
            if !rt.started {
                rt.started = true;
                rt.start_price = price;
                self.req_dirty = true;
            }
            let base = sp.annual / 12.0 * rt.scale * share;
            let (amount, taxable, series) = match sp.kind {
                StreamKind::Pension { cola, .. } => {
                    let amount = match cola {
                        Cola::Full => base * price,
                        Cola::None => base * rt.start_price,
                        Cola::Partial { rate } => {
                            rt.level *= 1.0 + math::annual_to_monthly(rate);
                            base * rt.start_price * rt.level
                        }
                    };
                    (amount, true, YearSeries::IncomePensions)
                }
                StreamKind::Annuity { real } => (
                    base * if real { price } else { rt.start_price },
                    true,
                    YearSeries::IncomePensions,
                ),
                StreamKind::Other { taxable, .. } => {
                    (base * price * rt.level, taxable, YearSeries::IncomeOther)
                }
            };
            rt.last = amount;
            self.streams[k] = rt;
            if amount > 0.0 {
                self.recv(series, amount);
                if taxable {
                    if series == YearSeries::IncomePensions {
                        self.ty.pensions += amount;
                    } else {
                        self.ty.other_ordinary += amount;
                    }
                    self.wage_like += amount;
                }
            }
        }
    }

    /// Modelled cost of the children this month, today's dollars.
    pub(crate) fn children_cost(&self, plan: &Plan, t: usize) -> f64 {
        let ti = t as i32;
        let mut minors = 0;
        for c in &plan.children {
            let age = ti - c.birth;
            if (0..18 * 12).contains(&age) {
                minors += 1;
            }
        }
        // USDA family-size adjustment (`children.usda_2015.family_size_adjustment`).
        let family = match minors {
            0 | 2 => 1.0,
            1 => 1.27,
            _ => 0.76,
        };
        let mut total = 0.0;
        for c in &plan.children {
            let age = ti - c.birth;
            if age < 0 {
                continue;
            }
            let years = (age / 12) as usize;
            if years < 18 {
                total += c.core[years] * family + c.care[years] * self.service_growth;
            } else if years < 22 {
                total += c.college * self.service_growth;
            }
        }
        total / 12.0
    }

    /// The budget in today's dollars per month as it stands in month `t`, for a
    /// household that is (`retired`) or is not retired, with retirement-relative
    /// dates read against `retire_month`. Returns `(essential, discretionary, next
    /// month at which a line starts or stops)`.
    pub(crate) fn budget_as(
        &self,
        plan: &Plan,
        t: usize,
        retired: bool,
        retire_month: i32,
    ) -> (f64, f64, i32) {
        let ti = t as i32;
        let years = (t / 12) as i32;
        let mut ess = 0.0;
        let mut disc = 0.0;
        let mut next = i32::MAX;
        let survivor_factor = if self.survivor {
            plan.profile.spending.survivor_factor
        } else {
            1.0
        };
        for (i, item) in plan.items.iter().enumerate() {
            let start = item.start.month(retire_month);
            let end = item.end.month(retire_month);
            if start > ti {
                next = next.min(start);
            }
            if end > ti {
                next = next.min(end);
            }
            if ti < start || ti >= end {
                continue;
            }
            if let Some(owner) = item.owner {
                if owner >= MAX_PEOPLE || !self.present[owner] {
                    continue;
                }
            }
            let mut amount = if retired {
                item.monthly_retired
            } else {
                item.monthly
            } * self.item_scale[i];
            if amount == 0.0 {
                continue;
            }
            if item.growth != 0.0 {
                amount *= if (years as usize) < plan.growth_years {
                    plan.item_growth[i * plan.growth_years + years as usize]
                } else {
                    math::powi(1.0 + item.growth, years)
                };
            }
            if item.owner.is_none() {
                amount *= survivor_factor;
            }
            if retired && item.category != Category::Healthcare {
                amount *= self.smile;
            }
            if item.essential {
                ess += amount;
            } else {
                disc += amount * self.creep;
            }
        }
        if self.extra_rent > 0.0 {
            ess += self.extra_rent
                * plan
                    .rent_growth
                    .get(years as usize)
                    .copied()
                    .unwrap_or_else(|| {
                        math::powi(1.0 + Category::Rent.default_real_growth(), years)
                    })
                * survivor_factor;
        }
        // Shocks add variance, not level: their expected cost comes out of the
        // baseline budget, discretionary lines first.
        let offset = plan.shocks.mean_fraction * (ess + disc);
        if disc >= offset {
            disc -= offset;
        } else {
            ess = (ess - (offset - disc)).max(0.0);
            disc = 0.0;
        }
        (ess, disc, next)
    }

    /// Recomputes the monthly budget.
    pub(crate) fn rebudget(&mut self, plan: &Plan, t: usize) {
        let (ess, disc, next) = self.budget_as(plan, t, self.retired, self.retire_month);
        self.ess_real = ess;
        self.disc_real = disc;
        // Real growth steps once a year, so look again at the next anniversary.
        let anniversary = ((t / 12 + 1) * 12) as i32;
        self.next_item_change = next.min(anniversary);
        self.budget_dirty = false;
        self.service_growth = plan
            .service_growth
            .get(t / 12)
            .copied()
            .unwrap_or_else(|| math::powi(1.0 + CHILD_SERVICE_REAL_GROWTH, (t / 12) as i32));
    }

    /// Required debt payments and the extra payment.
    fn pay_debts(&mut self, plan: &Plan, t: usize, price: f64) {
        if !self.debts_active {
            return;
        }
        let now = plan.start + t as i32;
        let policy = &plan.profile.debt_policy;
        let mut total = 0.0;
        let mut any = false;
        for i in 0..self.debts.len() {
            let mut d = self.debts[i];
            if d.balance <= 0.0 {
                continue;
            }
            any = true;
            let interest = d.balance * d.rate / 12.0;
            d.balance += interest;
            let due = if d.revolving {
                (REVOLVING_MIN_FRACTION * d.balance).max(REVOLVING_MIN_PAYMENT)
            } else {
                d.payment
            };
            let paid = due.min(d.balance);
            d.balance -= paid;
            total += paid;
            if self.check {
                self.chk_debt_flow += interest - paid;
            }
            if d.balance <= DUST {
                d.balance = 0.0;
                self.freed_payments += d.payment;
                self.req_dirty = true;
                self.event(now, TraceEventKind::DebtPaidOff, None, || {
                    format!("Paid off {}", plan.debts[i].name)
                });
            }
            self.debts[i] = d;
        }
        if any && policy.strategy != DebtStrategy::Minimum {
            // Payments freed by debts already cleared roll into the extra amount.
            let mut extra = policy.extra_monthly.max(0.0) * price + self.freed_payments;
            while extra > DUST {
                let mut pick: Option<usize> = None;
                for (i, d) in self.debts.iter().enumerate() {
                    if d.balance <= 0.0 {
                        continue;
                    }
                    let better = match pick {
                        None => true,
                        Some(b) => match policy.strategy {
                            DebtStrategy::Avalanche => d.rate > self.debts[b].rate,
                            _ => d.balance < self.debts[b].balance,
                        },
                    };
                    if better {
                        pick = Some(i);
                    }
                }
                let Some(i) = pick else { break };
                let paid = extra.min(self.debts[i].balance);
                self.debts[i].balance -= paid;
                extra -= paid;
                total += paid;
                if self.check {
                    self.chk_debt_flow -= paid;
                }
                if self.debts[i].balance <= DUST {
                    self.debts[i].balance = 0.0;
                    self.freed_payments += self.debts[i].payment;
                    self.req_dirty = true;
                    self.event(now, TraceEventKind::DebtPaidOff, None, || {
                        format!("Paid off {}", plan.debts[i].name)
                    });
                }
            }
        }
        if total > 0.0 {
            self.spend(YearSeries::SpendDebt, total);
        }
        self.debts_active = any;
    }

    /// Adds what a plan execution or shortfall cover did to the ledger.
    fn ledger_withdrawals(&mut self, ex: &Executed) {
        self.led[YearSeries::WithdrawTaxable.index()] += ex.from_taxable;
        self.led[YearSeries::WithdrawTraditional.index()] += ex.from_traditional;
        self.led[YearSeries::WithdrawRoth.index()] += ex.from_roth;
        self.led[YearSeries::WithdrawHsa.index()] += ex.from_hsa;
        self.led[YearSeries::RothConversion.index()] += ex.converted;
    }

    /// Step 9: fund the month or invest its surplus.
    pub(crate) fn settle(&mut self, plan: &Plan, t: usize, price: f64) {
        let now = plan.start + t as i32;
        let target = self.target;
        // Sales steer the mix toward target only when nothing else rebalances it.
        let sell_target = if plan.plain_mix { None } else { Some(&target) };
        // The plan is carried out a quarter at a time (and for the months to the
        // next quarter when it is made mid-quarter).
        let month = now.month();
        if self.has_plan && (self.plan_fresh || (month - 1) % 3 == 0) {
            let months = f64::from(3 - (month - 1) % 3);
            self.plan_fresh = false;
            let ex = execute_plan(
                &self.wplan,
                self.exec_frac * months,
                &mut self.acc,
                &self.pinputs,
                &mut self.ty,
                sell_target,
            );
            self.ledger_withdrawals(&ex);
        }
        if self.acc.cash < -DUST {
            // Cash has run out: sell investments, penalty-free sources first, then
            // with the penalty.
            let need = -self.acc.cash;
            let ex = cover_shortfall(
                need,
                plan.profile.retirement.early_access,
                &mut self.acc,
                &self.pinputs,
                &mut self.ty,
                sell_target,
            );
            self.ledger_withdrawals(&ex);
        }
        // An overdraft left from earlier months is not this month's shortfall: what
        // has to be found now is only what would make it grow.
        let overdraft = (-self.cash_start).max(0.0);
        let mut essentials_cut = false;
        if -self.acc.cash - overdraft > DUST {
            // Nothing liquid is left to sell. Spending is cut to what there is: what
            // can be postponed first; then property is sold if the plan allows it;
            // then the essentials. Obligations that cannot be cut
            // (housing costs, debt service, tax) stay unpaid as an overdraft, which
            // is reported as debt and repaid from later surpluses.
            let mut release = plan.profile.retirement.home_equity == HomeEquity::LastResort
                && self.props.iter().any(|p| p.owned);
            const CUTS: [(YearSeries, bool); 6] = [
                (YearSeries::SpendDiscretionary, false),
                (YearSeries::SpendOneOff, true),
                (YearSeries::SpendEssential, true),
                (YearSeries::SpendChildren, true),
                (YearSeries::SpendHealthcare, true),
                (YearSeries::SpendLongTermCare, true),
            ];
            for (series, essential) in CUTS {
                let mut missing = -self.acc.cash - overdraft;
                if missing <= DUST {
                    break;
                }
                if essential && release {
                    // Before an essential is cut, property is sold.
                    release = false;
                    self.release_home_equity(plan, t, price, missing);
                    missing = -self.acc.cash - overdraft;
                    if missing <= DUST {
                        break;
                    }
                }
                let cut = missing.min(self.led[series.index()]);
                if cut > 0.0 {
                    self.led[series.index()] -= cut;
                    self.acc.cash += cut;
                    essentials_cut |= essential && cut > DUST;
                }
            }
        }
        // Housing costs, debt service and tax cannot be cut: if they alone are more
        // than there is, property goes too.
        if -self.acc.cash - overdraft > DUST
            && plan.profile.retirement.home_equity == HomeEquity::LastResort
            && self.props.iter().any(|p| p.owned)
        {
            self.release_home_equity(plan, t, price, -self.acc.cash - overdraft);
        }
        let depleted = essentials_cut || -self.acc.cash - overdraft > DUST;
        if depleted {
            self.depleted_year = true;
            if !self.ever_depleted {
                self.ever_depleted = true;
                self.depletion_month = t as i32;
                self.ltc_at_depletion = self.ltc_real;
                self.unemployed_at_depletion = self
                    .jobs
                    .iter()
                    .map(|rt| rt.st.months_unemployed_total)
                    .sum();
            }
            if !self.depleted_now {
                self.event(now, TraceEventKind::Depleted, None, || {
                    "Savings exhausted: spending cut to income".to_string()
                });
            }
        } else if self.depleted_now {
            self.event(now, TraceEventKind::Recovered, None, || {
                "Income covers essential spending again".to_string()
            });
        }
        self.depleted_now = depleted;

        // Surplus above the cash target is invested through the savings waterfall.
        // A working household invests every month; a retired one, which draws its
        // spending money a quarter ahead, sweeps what is left over each December.
        let cap = if self.retired {
            self.retire_cash_cap
        } else {
            self.efund_target
        } + self.tax_reserve();
        if self.acc.cash > cap + 1.0 && (!self.retired || month == 12) {
            let surplus = self.acc.cash - cap;
            self.acc.cash -= surplus;
            let flows = self.acc.save_surplus(surplus, &self.sctx, &target);
            for p in 0..MAX_PEOPLE {
                self.led[YearSeries::SaveHsa.index()] += flows.hsa[p];
                self.led[YearSeries::SaveTraditional.index()] += flows.traditional_ira[p];
                self.led[YearSeries::SaveRoth.index()] +=
                    flows.roth_ira[p] + flows.after_tax_401k[p];
                self.ty.adjustments += flows.hsa[p] + flows.traditional_ira[p];
            }
            self.led[YearSeries::SaveTaxable.index()] += flows.taxable;
        }
        // The cash bucket's own line: its change this month before interest.
        // (Selects: the sign depends on the whole month's cash flow.)
        let delta = self.acc.cash - self.cash_start;
        let up = delta >= 0.0;
        let saved = self.led[YearSeries::SaveCash.index()];
        let withdrawn = self.led[YearSeries::WithdrawCash.index()];
        self.led[YearSeries::SaveCash.index()] = if up { delta } else { saved };
        self.led[YearSeries::WithdrawCash.index()] = if up { withdrawn } else { -delta };
    }

    /// Cash held back for tax already assessed and due next April.
    fn tax_reserve(&self) -> f64 {
        let mut due = 0.0;
        for d in &self.due {
            due += d;
        }
        due.max(0.0)
    }

    /// Pays (or receives) the balance of the last return.
    pub(crate) fn pay_tax_due(&mut self) {
        for c in 0..4 {
            let due = self.due[c];
            self.due[c] = 0.0;
            let series = TAX_SERIES[c];
            if due > 0.0 {
                self.pay(series, due);
            } else if due < 0.0 {
                // A refund first nets against tax paid this month in the same
                // category; the rest is income.
                let refund = -due;
                self.acc.cash += refund;
                let net = refund.min(self.led[series.index()]);
                self.led[series.index()] -= net;
                self.led[YearSeries::IncomeOther.index()] += refund - net;
            }
        }
    }
}
