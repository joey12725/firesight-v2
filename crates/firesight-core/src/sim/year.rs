//! The annual cycle: closing a tax year, and the plan made each January (and again
//! whenever the household's situation changes mid-year: the first month of the plan,
//! retirement, a death, a life event).
//!
//! # Tax timing
//!
//! Tax is computed once per calendar year on what actually happened
//! ([`State::close_tax_year`], run in January for the year just ended) and paid on a
//! cash basis:
//!
//! * employee FICA is withheld from each pay cheque, exactly, against the year's
//!   wage base;
//! * income tax (and self-employment tax) is prepaid during the year: a withholding
//!   rate on wage-like income plus a level monthly instalment, both set by the plan
//!   from the estimated return;
//! * the balance of the actual return, by category, is paid or refunded in April;
//!   the last year of a life is settled in its last month, before the estate is
//!   counted.
//!
//! The first calendar year usually starts part-way through. Its return is computed
//! on the simulated months scaled to a full year and the result scaled back, so a
//! household that starts in November is not taxed as if two months of pay were its
//! whole year's income.

use crate::accounts::{self, Bucket, PersonSavings, SavingsContext};
use crate::career::Employment;
use crate::decumulation::{plan_year_from, Funding, PlanInputs, SpendingInputs};
use crate::economy::EconomyPath;
use crate::health::{CoveragePhase, HealthContext};
use crate::math;
use crate::model::{
    AssetLocation, GlidePath, PropertyUse, SpendingRule, TaxKind, WealthBasis, WithdrawalOrder,
};
use crate::stats::TraceEventKind;
use crate::tax::{self, FilingStatus, Indexation, TaxInputs};
use crate::MAX_PEOPLE;

use super::family::AdultPhases;
use super::plan::{Plan, StreamKind, TriggerPlan};
use super::state::{State, TAX_FED, TAX_PAYROLL, TAX_PENALTY, TAX_STATE};

/// Inflation assumed between the tax law's base year and a later start year when
/// bridging the published thresholds to the start of the plan.
const BRIDGE_INFLATION: f64 = 0.025;

/// A mortgage is refinanced in January when the market rate is at least this far
/// below the loan's rate (the usual rule of thumb) and the closing costs pay for
/// themselves.
const REFINANCE_THRESHOLD: f64 = 0.01;
/// Closing costs of a refinance: this share of the balance plus a fixed amount in
/// today's dollars (lender, title and appraisal fees; a judgement in the usual
/// 2 to 5% range, at the low end because nothing is paid for points).
const REFINANCE_COST_FRACTION: f64 = 0.02;
const REFINANCE_COST_FIXED: f64 = 2_000.0;
/// The interest saved over this many years must exceed the closing costs.
const REFINANCE_PAYBACK_YEARS: f64 = 4.0;

/// The year is planned a second time when the premium credit the first plan earns
/// differs from the one it assumed by more than this share of a month's spending
/// (what is left is settled with the return, like any estimate).
const ACA_ADVANCE_TOLERANCE: f64 = 0.03;

/// Highest share of wage-like income withheld for income tax.
const MAX_WITHHOLDING_RATE: f64 = 0.6;

/// Months of spending a retired household keeps in cash before reinvesting surplus
/// (on top of its retirement cash bucket).
const RETIRED_CASH_MONTHS: f64 = 1.0;

/// Blanchett (2014) retirement spending curve: annual real change at an age for a
/// household with real spending target `spend`
/// (`retirement_spending.blanchett_smile.formula`).
fn smile_delta(age: f64, spend: f64) -> f64 {
    0.00008 * age * age - 0.0125 * age - 0.0066 * math::ln(spend.max(1_000.0)) + 0.546
}

/// Multiplies every dollar amount of a return by `k` (counts and flags unchanged).
fn scale_inputs(x: &mut TaxInputs, k: f64) {
    for p in 0..MAX_PEOPLE {
        x.wages[p] *= k;
        x.self_employment[p] *= k;
        x.pretax_deferrals[p] *= k;
        x.hsa_contributions[p] *= k;
        x.social_security[p] *= k;
    }
    x.interest *= k;
    x.ordinary_dividends *= k;
    x.qualified_dividends *= k;
    x.short_term_gains *= k;
    x.long_term_gains *= k;
    x.traditional_distributions *= k;
    x.roth_conversions *= k;
    x.pensions *= k;
    x.rental_net *= k;
    x.other_ordinary *= k;
    x.mortgage_interest *= k;
    x.property_tax *= k;
    x.state_tax_paid *= k;
    x.charitable *= k;
    x.early_distributions *= k;
    x.hsa_nonqualified *= k;
    x.adjustments *= k;
    x.depreciation_recapture *= k;
    x.passive_loss_released *= k;
}

/// Expected spending outside the budget lines for one month, nominal dollars.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RunRate {
    /// Owned-home costs including mortgage payments, and rental mortgage payments.
    pub housing: f64,
    /// Expected rental income net of operating costs.
    pub rental_noi: f64,
    /// Modelled healthcare before any premium credit, including long-term care
    /// already under way.
    pub health: f64,
    /// The part of `health` an HSA may pay tax-free: out-of-pocket costs, Medicare
    /// premiums and long-term care, but not marketplace or employer premiums.
    pub medical: f64,
    /// Premiums of the people expected on a marketplace plan.
    pub aca_premium: f64,
    /// True while someone is in a long-term care episode.
    pub care: bool,
    /// Benchmark ACA premium for the people expected on the marketplace.
    pub aca_benchmark: f64,
    /// People expected on a marketplace plan.
    pub aca_people: u32,
    pub children: f64,
    pub debt: f64,
}

impl RunRate {
    pub(crate) fn total(&self) -> f64 {
        self.housing + self.health + self.children + self.debt
    }
}

impl State {
    /// Filing status and dependants as they stand in calendar `year`.
    fn household_for_tax(&self, plan: &Plan, year: i32) -> (FilingStatus, u32, u32, u32) {
        let mut adults = 0;
        for i in 0..self.n {
            if self.present[i] {
                adults += 1;
            }
        }
        let (mut young, mut other) = (0, 0);
        for (k, c) in plan.children.iter().enumerate() {
            let birth = self.child_birth(plan, k);
            if birth >= super::family::NEVER {
                continue;
            }
            let born = (plan.start + birth).year();
            let age = year - born;
            let n = self.child_count(k);
            if (0..17).contains(&age) {
                young += n;
            } else if (17..19).contains(&age)
                || ((19..19 + self.college_years(k) as i32).contains(&age) && c.college > 0.0)
            {
                other += n;
            }
        }
        // A surviving spouse files jointly for the year of the death.
        let status = if adults >= 2 || (self.death_year == year && adults == 1) {
            FilingStatus::MarriedJoint
        } else if young + other > 0 {
            FilingStatus::HeadOfHousehold
        } else {
            FilingStatus::Single
        };
        (status, adults.max(1), young, other)
    }

    /// The indexation of the tax law for `year`, chained from simulated CPI.
    fn indexation(&mut self, plan: &Plan, year: i32, price: f64) -> Indexation {
        let law = &plan.law;
        let base = law.base_year;
        let start_year = plan.start.year();
        if year == start_year {
            if start_year <= base {
                self.anchor_cpi = 1.0;
                self.anchor_price = 1.0;
                self.prev_cpi_factor = 1.0;
                return Indexation::base(year);
            }
            // The plan starts after the law's base year: bridge from the officially
            // published next-year factors at an assumed inflation rate.
            let k = math::powi(1.0 + BRIDGE_INFLATION, start_year - base - 1);
            let published = law.published_next_year;
            self.anchor_cpi = published.cpi_factor * k;
            self.anchor_price = 1.0;
            self.prev_cpi_factor = self.anchor_cpi / (1.0 + BRIDGE_INFLATION);
            return law
                .index_from_cpi(year, self.anchor_cpi)
                .with_fpl_factor(published.fpl_factor * k);
        }
        if start_year <= base && year == base + 1 && year == start_year + 1 {
            // The first simulated January: the real figures for this year are
            // already published.
            let published = law.published_next_year;
            self.anchor_cpi = published.cpi_factor;
            self.anchor_price = price;
            self.prev_cpi_factor = published.cpi_factor;
            return Indexation { year, ..published };
        }
        let cpi = self.anchor_cpi * price / self.anchor_price;
        // Coverage in year Y uses the poverty guideline published in Y - 1, which
        // carries the prices of Y - 2: chain it from the latest published guideline
        // (the one for the year after the law's base year) with the inflation since.
        let index = law
            .index_from_cpi(year, cpi)
            .with_fpl_factor(law.published_next_year.fpl_factor * self.prev_cpi_factor);
        self.prev_cpi_factor = cpi;
        index
    }

    /// Sets up the tax law, filing status and year-end ages of the first calendar
    /// year. Called at the end of [`State::begin`] so that nothing in month 0 can read
    /// values left over from the previous path in the workspace.
    pub(crate) fn init_first_year(&mut self, plan: &Plan) {
        let year = plan.start.year();
        self.index = self.indexation(plan, year, 1.0);
        let (status, _, _, _) = self.household_for_tax(plan, year);
        self.build_law(plan, status, true);
        if self.params.state.code == self.tax_state && self.tax_state != tax::StateCode::NONE {
            self.local.apply(&mut self.params.state);
        }
        self.fica_on = self.params.kind == TaxKind::UsCurrentLaw && self.foreign_rate.is_none();
        self.filing = status;
        for i in 0..MAX_PEOPLE {
            self.age_ye[i] = if i < self.n && self.present[i] {
                f64::from(year - plan.people[i].birth.year())
            } else {
                0.0
            };
        }
    }

    /// Builds the law of the year `self.index` stands for, for the household's
    /// state, with the per-status tables indexed for `status` only (the status the
    /// household files under; [`State::ensure_law`] adds another when one is
    /// needed).
    ///
    /// `first` is true for the first law of a path. Later ones for the same state
    /// only rewrite what depends on the year.
    fn build_law(&mut self, plan: &Plan, status: FilingStatus, first: bool) {
        let refresh = !first && self.law_state == self.tax_state;
        plan.law.for_year_statuses_into(
            self.index,
            self.tax_state,
            &mut self.params,
            1 << status.index(),
            refresh,
        );
        self.params.foreign_tax_rate = self.foreign_rate;
        self.law_ready = 1 << status.index();
        self.law_state = self.tax_state;
    }

    /// Makes sure the per-status tables of `self.params` are indexed for `status`.
    /// Every use of the parameters with a filing status goes through here first.
    fn ensure_law(&mut self, plan: &Plan, status: FilingStatus) {
        let bit = 1 << status.index();
        if self.law_ready & bit == 0 {
            plan.law.index_status_into(
                self.index,
                self.law_state,
                &mut self.params,
                status.index(),
            );
            self.law_ready |= bit;
        }
    }

    /// Expected monthly spending outside the budget lines, and the coverage the
    /// household can expect, as things stand in month `t`.
    pub(crate) fn run_rate(&self, plan: &Plan, t: usize, price: f64, retired: bool) -> RunRate {
        let mut r = RunRate::default();
        for (st, pp) in self.props.iter().zip(&plan.props) {
            if !st.owned {
                continue;
            }
            let (operating, mortgage, rent) = pp.model.expected_month(st, price);
            if matches!(st.usage, PropertyUse::Rental(_)) {
                r.rental_noi += rent - operating;
                r.housing += mortgage;
            } else {
                r.housing += operating + mortgage;
            }
        }
        let mut employer = false;
        if !retired {
            for (rt, jp) in self.jobs.iter().zip(&plan.jobs) {
                if self.present[jp.owner]
                    && matches!(
                        rt.st.employment,
                        Employment::Employed | Employment::OnBreak { .. }
                    )
                {
                    employer = true;
                }
            }
        } else {
            for (rt, jp) in self.jobs.iter().zip(&plan.jobs) {
                if self.present[jp.owner]
                    && !jp.ends_at_retirement
                    && matches!(rt.st.employment, Employment::Employed)
                {
                    employer = true;
                }
            }
        }
        let mut adults_on = AdultPhases {
            employer,
            ..AdultPhases::default()
        };
        for i in 0..self.n {
            if !self.present[i] {
                continue;
            }
            let age = self.age(plan, i, t);
            let phase = plan.health.phase(&HealthContext {
                age,
                employer_coverage: employer,
                months_since_coverage_ended: if employer { 0 } else { self.coverage_gap },
                in_us: self.in_us,
                months_since_start: t as u32,
                price_level: price,
            });
            adults_on.note(phase);
            let (premium, pocket) = plan.health.expected_annual_parts_at(age, phase, t as u32);
            let (premium, pocket) = (premium / 12.0 * price, pocket / 12.0 * price);
            r.health += premium + pocket + self.irmaa_m[i];
            r.medical += pocket;
            if phase == CoveragePhase::Medicare {
                r.medical += premium + self.irmaa_m[i];
            }
            // A care episode that has begun is expected to go on: the household
            // does not know when it ends.
            let h = &self.health[i];
            if (t as u32) >= h.ltc_start_month && (t as u32) < h.ltc_end_month {
                let care = h.ltc_monthly_cost * plan.health.excess_growth(t as u32) * price;
                r.health += care;
                r.medical += care;
                r.care = true;
            }
            if phase == CoveragePhase::Aca {
                r.aca_premium += premium;
                r.aca_people += 1;
                r.aca_benchmark += plan.health.aca_benchmark_monthly(age)
                    * plan.health.excess_growth(t as u32)
                    * price;
            }
        }
        if plan.family.cover_children {
            let kids = self.child_health(plan, t, price, adults_on);
            r.health += kids.total;
            r.aca_premium += kids.aca_premium;
            r.aca_benchmark += kids.aca_benchmark;
        }
        if (t as i32) < self.kids_until {
            r.children = self.children_cost(plan, t) * price;
        }
        for d in &self.debts {
            if d.balance > 0.0 {
                r.debt += if d.revolving {
                    (0.02 * d.balance).max(35.0)
                } else {
                    d.payment.min(d.balance)
                };
            }
        }
        r
    }

    /// Closes the books on calendar `year`: computes the actual return and what is
    /// still owed (or refundable) by category, and rolls the carryforwards.
    /// `last` marks the final month of the path.
    pub(crate) fn close_tax_year(&mut self, plan: &Plan, year: i32, last: bool) {
        let (status, adults, young, other) = self.household_for_tax(plan, year);
        self.ensure_law(plan, status);
        let mut x = self.ty;
        x.filing_status = status;
        x.adults = adults;
        x.children_under_17 = young;
        x.other_dependents = other;
        x.state = self.params.state.code;
        for i in 0..MAX_PEOPLE {
            x.ages[i] = if i < self.n && (self.present[i] || (last && self.alive[i])) {
                f64::from(year - plan.people[i].birth.year())
            } else {
                0.0
            };
        }
        if x.aca_months > 0 {
            x.aca_household_size = adults + young + other;
            x.aca_benchmark_premium *= 12.0 / f64::from(x.aca_months);
        } else {
            x.aca_household_size = 0;
        }
        x.short_term_gains -= self.loss_carry;
        x.rental_net -= self.rental_carry;
        // A year that starts part-way through is taxed at its annual rate.
        let months = self.year_months.clamp(1, 12);
        let annualise = 12.0 / f64::from(months);
        if months < 12 {
            scale_inputs(&mut x, annualise);
        }
        let r = tax::compute(&self.params, &x);
        let back = 1.0 / annualise;
        let mut payroll = 0.0;
        for i in 0..MAX_PEOPLE {
            payroll += r.fica[i] + r.se_tax[i];
        }
        let state = r.state + r.foreign;
        let federal = r.total - payroll - state - r.penalties;
        // The premium credit actually earned, against what was advanced.
        let credit = (r.aca_premium_credit * back).min(self.aca_premium_ytd);
        self.due[TAX_FED] += federal * back - self.paid[TAX_FED] + self.aca_advance_ytd - credit;
        self.due[TAX_STATE] += state * back - self.paid[TAX_STATE];
        self.due[TAX_PAYROLL] += payroll * back - self.paid[TAX_PAYROLL];
        self.due[TAX_PENALTY] += r.penalties * back - self.paid[TAX_PENALTY];

        self.loss_carry = r.capital_loss_carryforward * back;
        self.rental_carry = r.rental_loss_suspended * back;
        self.magi_hist = [self.magi_hist[1], r.magi_irmaa];
        self.magi_known = (self.magi_known + 1).min(2);
        if plan.family.any_college {
            self.note_aid_income(r.magi_irmaa);
        }
        self.prev_interest = self.ty.interest * annualise;
        self.prev_ordinary_div = self.ty.ordinary_dividends * annualise;
        self.prev_qualified_div = self.ty.qualified_dividends * annualise;
        self.prev_rental_net = self.ty.rental_net * annualise;
        self.prev_mortgage_interest = self.ty.mortgage_interest * annualise;
        self.prev_property_tax = self.ty.property_tax * annualise;
        if self.detail {
            self.tax_details
                .push((year, tax::explain(&self.params, &x)));
        }
        self.ty = TaxInputs::default();
        self.paid = [0.0; 4];
        self.ytd_fica_wages = [0.0; MAX_PEOPLE];
        self.aca_advance_ytd = 0.0;
        self.aca_premium_ytd = 0.0;
        self.aca_people = 0;
        self.year_months = 12;
    }

    /// The turn of the year, in January before anything else happens: Social
    /// Security records, last year's tax return, the Roth clocks, refinancing,
    /// lifestyle creep and the retirement spending smile. Events dated in January
    /// fire after this, so what they add to the tax year lands on the new year's
    /// return.
    pub(crate) fn close_year(&mut self, plan: &Plan, econ: &EconomyPath, t: usize) {
        let now = plan.start + t as i32;
        let year = now.year();
        let price = econ.price_level[t];
        // Social Security records and benefits for the year just ended.
        for i in 0..self.n {
            if !self.alive[i] {
                continue;
            }
            if matches!(
                plan.people[i].basis,
                crate::benefits::BenefitBasis::FromEarnings
            ) {
                plan.ss.record_earnings(
                    &mut self.records[i],
                    plan.people[i].birth,
                    year - 1,
                    self.year_earned[i],
                    price,
                );
                let pia = plan
                    .ss
                    .pia_real(&self.records[i], plan.people[i].birth, now, price);
                if let Some(c) = &mut self.claimants[i] {
                    c.pia_real = Some(pia);
                }
            }
        }
        self.year_earned = [0.0; MAX_PEOPLE];
        self.close_tax_year(plan, year - 1, false);
        self.acc.roll_year();
        self.prior_return_negative = self.year_port < 1.0;
        self.year_port = 1.0;
        self.prior_inflation = price / self.price_jan - 1.0;
        // Refinance any mortgage the market now beats by a point or more, when the
        // household does that and the costs pay for themselves.
        let market = econ.mortgage_rate[t];
        for i in 0..self.props.len() {
            if !plan.profile.debt_policy.refinance_mortgages || !self.props[i].owned {
                continue;
            }
            let costs = REFINANCE_COST_FRACTION * self.props[i].mortgage_balance()
                + REFINANCE_COST_FIXED * price;
            if self.props[i].refinance(market, REFINANCE_THRESHOLD, costs, REFINANCE_PAYBACK_YEARS)
            {
                self.spend(crate::stats::YearSeries::SpendOneOff, costs);
                self.req_dirty = true;
                self.event(now, TraceEventKind::LifeEvent, None, || {
                    format!(
                        "Refinanced {} at {:.2}%",
                        plan.props[i].name,
                        market * 100.0
                    )
                });
            }
        }
        // Lifestyle creep: a share of real income growth beyond its previous
        // high becomes discretionary spending, while the household works.
        if !self.retired && plan.income_rate0 > 0.0 {
            let rate = self.income_rate(plan, t);
            if rate > self.income_hwm {
                self.income_hwm = rate;
                self.creep = 1.0
                    + plan.profile.spending.lifestyle_creep.clamp(0.0, 1.0)
                        * (self.income_hwm / plan.income_rate0 - 1.0);
                self.budget_dirty = true;
            }
        }
        // The retirement spending smile: real spending drifts down with age.
        if self.retired && plan.profile.spending.retirement_smile {
            let mut age: f64 = 0.0;
            for i in 0..self.n {
                if self.present[i] {
                    age = age.max(self.age(plan, i, t));
                }
            }
            if age >= 60.0 && self.smile_target > 0.0 {
                self.smile *= 1.0 + smile_delta(age, self.smile_target).min(0.0);
                self.budget_dirty = true;
            }
        }
    }

    /// The plan for the rest of the calendar year, made in month `t`. `new_year` is
    /// true in January (not in the first month of the path), after
    /// [`State::close_year`] has closed the previous year.
    pub(crate) fn annual_plan(
        &mut self,
        plan: &Plan,
        econ: &EconomyPath,
        t: usize,
        new_year: bool,
    ) {
        let now = plan.start + t as i32;
        let year = now.year();
        let month = now.month();
        let price = econ.price_level[t];
        let m_left = f64::from(13 - month);
        let first = t == 0;

        if new_year || first {
            self.price_jan = price;
            self.index = self.indexation(plan, year, price);
        }
        let (status, adults, young, other) = self.household_for_tax(plan, year);
        // The law is re-indexed each January and rebuilt when the residence changes.
        if new_year
            || first
            || self.params.state.code != self.tax_state && plan.law.knows_state(self.tax_state)
            || self.params.foreign_tax_rate != self.foreign_rate
            || (self.tax_state == tax::StateCode::NONE
                && self.params.state.code != tax::StateCode::NONE)
        {
            self.build_law(plan, status, false);
        } else {
            self.ensure_law(plan, status);
        }
        if self.params.state.code == self.tax_state && self.tax_state != tax::StateCode::NONE {
            self.local.apply(&mut self.params.state);
        }
        self.fica_on = self.params.kind == TaxKind::UsCurrentLaw && self.foreign_rate.is_none();
        self.filing = status;
        let mut ages_now = [0.0; MAX_PEOPLE];
        for i in 0..MAX_PEOPLE {
            if i < self.n && self.present[i] {
                self.age_ye[i] = f64::from(year - plan.people[i].birth.year());
                ages_now[i] = self.age(plan, i, t);
            } else {
                self.age_ye[i] = 0.0;
            }
        }

        // Glide path and the annual rebalance.
        if !matches!(plan.profile.portfolio.glide, GlidePath::None) {
            let years = self.years_to_retirement(plan, t, price);
            self.target = accounts::glide_target(&plan.profile.portfolio, years);
        }
        if new_year && plan.rebalance_annual {
            self.rebalance(plan);
        }

        if self.budget_dirty || new_year || first {
            self.rebudget(plan, t);
        }
        if self.retired && self.smile_target <= 0.0 {
            self.smile_target = (self.ess_real + self.disc_real) * 12.0;
        }
        let rate = self.run_rate(plan, t, price, self.retired);
        let budget = (self.ess_real + self.disc_real) * price;
        // The premium credit the household can expect to be advanced each month:
        // last plan's figure until this plan has been costed.
        let mut advance = if rate.aca_people > 0 {
            (self.aca_guess_real * price).min(rate.aca_premium)
        } else {
            0.0
        };
        let planned_month = budget + rate.total() - advance;
        self.efund_target = plan.profile.portfolio.emergency_months.max(0.0) * planned_month;

        // What the rest of the year is expected to bring in.
        let mut wages = [0.0; MAX_PEOPLE];
        let mut business = [0.0; MAX_PEOPLE];
        for (rt, jp) in self.jobs.iter().zip(&plan.jobs) {
            if self.present[jp.owner] && matches!(rt.st.employment, Employment::Employed) {
                wages[jp.owner] += (rt.st.real_salary * (1.0 + jp.bonus_target)
                    + jp.commission_real)
                    * price
                    * m_left
                    / 12.0;
            }
        }
        for (k, (rt, sp)) in self.selfemp.iter().zip(&plan.selfemp).enumerate() {
            let p = sp.owner;
            let ti = t as i32;
            if self.present[p]
                && !rt.ended
                && ti >= sp.start.month(self.retire_month)
                && ti < sp.end.month(self.retire_month)
                && !(sp.ends_at_retirement && self.person_retired[p])
            {
                business[p] += sp.model.expected_real_annual(self.se_years(plan, k, t))
                    * rt.st.factor
                    * rt.scale
                    * price
                    * m_left
                    / 12.0;
            }
        }
        let mut pensions = 0.0;
        let mut other_taxable = 0.0;
        let mut other_untaxed = 0.0;
        for (rt, sp) in self.streams.iter().zip(&plan.streams) {
            if rt.ended || rt.last <= 0.0 {
                continue;
            }
            match sp.kind {
                StreamKind::Other { taxable: false, .. } => other_untaxed += rt.last * m_left,
                StreamKind::Other { .. } => other_taxable += rt.last * m_left,
                _ => pensions += rt.last * m_left,
            }
        }
        let mut benefits = [0.0; MAX_PEOPLE];
        // Earnings the earnings test will see this year.
        for i in 0..self.n {
            if let Some(c) = &mut self.claimants[i] {
                c.annual_earnings = (wages[i] + business[i]) * 12.0 / m_left;
            }
        }
        self.ss_dirty = true;
        if t >= self.ss_from {
            let b = plan.ss.monthly(now, &self.claimants, price);
            for i in 0..self.n {
                if self.present[i] {
                    benefits[i] = b.paid[i] * m_left;
                }
            }
        }
        let income_rest: f64 = wages.iter().sum::<f64>()
            + business.iter().sum::<f64>()
            + pensions
            + other_taxable
            + other_untaxed
            + benefits.iter().sum::<f64>()
            + rate.rental_noi * m_left;
        let wage_like_rest: f64 = wages.iter().sum::<f64>()
            + business.iter().sum::<f64>()
            + pensions
            + other_taxable
            + benefits.iter().sum::<f64>();

        // The return as it would stand with no withdrawals.
        let share = m_left / 12.0;
        let mut base = self.ty;
        base.filing_status = status;
        base.adults = adults;
        base.children_under_17 = young;
        base.other_dependents = other;
        base.state = self.params.state.code;
        base.ages = self.age_ye;
        for p in 0..MAX_PEOPLE {
            base.wages[p] += wages[p];
            base.self_employment[p] += business[p];
            base.social_security[p] += benefits[p];
            base.on_medicare[p] |= self.present[p] && self.in_us && self.age_ye[p] >= 65.0;
            // Deferrals and payroll HSA money on the pay still to come.
            if wages[p] > 0.0 {
                for (rt, jp) in self.jobs.iter().zip(&plan.jobs) {
                    if jp.owner == p && matches!(rt.st.employment, Employment::Employed) {
                        if let Some(ep) = rt.plan {
                            let fraction = if ep.employee_max {
                                (self.params.federal.limits.k401_employee
                                    / (rt.st.real_salary * price).max(1.0))
                                .min(1.0)
                            } else {
                                ep.employee_fraction
                            };
                            base.pretax_deferrals[p] += rt.st.real_salary
                                * price
                                * share
                                * fraction
                                * (1.0 - ep.roth_fraction.clamp(0.0, 1.0));
                        }
                        base.hsa_contributions[p] += jp.hsa_real * price * share;
                    }
                }
            }
        }
        base.pensions += pensions;
        base.other_ordinary += other_taxable;
        if self.prev_interest.is_nan() {
            // No history yet: a rough yield on what is held.
            base.interest += self.acc.cash.max(0.0) * econ.tbill[t].max(0.0) * share;
            base.qualified_dividends += self.acc.total(Bucket::Taxable) * 0.015 * share;
            base.rental_net += 0.0;
        } else {
            base.interest += self.prev_interest * share;
            base.ordinary_dividends += self.prev_ordinary_div * share;
            base.qualified_dividends += self.prev_qualified_div * share;
            base.rental_net += self.prev_rental_net * share;
            base.mortgage_interest += self.prev_mortgage_interest * share;
            base.property_tax += self.prev_property_tax * share;
        }
        base.short_term_gains -= self.loss_carry;
        base.rental_net -= self.rental_carry;
        if rate.aca_people > 0 || base.aca_months > 0 {
            let months_rest = if rate.aca_people > 0 { 13 - month } else { 0 };
            base.aca_benchmark_premium += rate.aca_benchmark * f64::from(months_rest);
            base.aca_months = (base.aca_months + months_rest).min(12);
            if base.aca_months > 0 {
                base.aca_household_size = adults + young + other;
                base.aca_benchmark_premium *= 12.0 / f64::from(base.aca_months);
            }
        }
        let year_months = f64::from(self.year_months.clamp(1, 12));
        let annualise = 12.0 / year_months;
        if annualise > 1.0 {
            scale_inputs(&mut base, annualise);
        }
        let base_result = tax::compute(&self.params, &base);
        let mut fica_est = 0.0;
        let mut se_est = 0.0;
        for p in 0..MAX_PEOPLE {
            fica_est += base_result.fica[p];
            se_est += base_result.se_tax[p];
        }
        let paid_nonfica = self.paid[TAX_FED]
            + self.paid[TAX_STATE]
            + self.paid[TAX_PENALTY]
            + (self.paid[TAX_PAYROLL] - self.fica_paid_estimate()).max(0.0);
        let base_nonfica = (base_result.total - fica_est) / annualise;
        let tax_rest_base = (base_nonfica - paid_nonfica).max(0.0)
            + (fica_est / annualise)
                * (wages.iter().sum::<f64>() * annualise
                    / (base.wages.iter().sum::<f64>()).max(1.0))
                .min(1.0);

        // Retirement: this year's spending under the rule.
        let investable = self.acc.investable();
        let ess_annual = (self.ess_real * price + rate.total() - advance) * 12.0;
        let disc_annual = self.disc_real * price * 12.0;
        if self.retired && (new_year || !self.planner_started) {
            let other_income = ((income_rest - tax_rest_base) * 12.0 / m_left).max(0.0) / price;
            // The rules that follow a withdrawal rate measure it on the funded
            // test's quantities, future income always counted.
            let mut rule = self.funding_rule(plan);
            let funding = if matches!(
                plan.profile.retirement.spending_rule,
                SpendingRule::Guardrails { .. }
                    | SpendingRule::VanguardDynamic { .. }
                    | SpendingRule::FloorCeiling { .. }
            ) {
                rule.count_future_income = true;
                rule.basis = WealthBasis::Investable;
                let req = self.requirement(plan, t, price, &rule);
                Some(Funding {
                    base_real: (self.wealth(WealthBasis::Investable, price) - req.lumps
                        + req.pv_income)
                        .max(0.0),
                    need_real: (req.need + req.tax).max(0.0),
                })
            } else {
                None
            };
            let inputs = SpendingInputs {
                budget_real: (ess_annual + disc_annual) / price,
                essential_real: ess_annual / price,
                portfolio_real: investable.max(0.0) / price,
                other_income_real: other_income,
                years_remaining: ((plan.horizon_months as usize).saturating_sub(t)) as f64 / 12.0,
                expected_real_return: plan.planner.params.vpw_real_return(&self.target)
                    - plan.fee_monthly * 12.0,
                prior_return_negative: self.prior_return_negative,
                prior_inflation: self.prior_inflation,
                funding,
                reference_floor: rule.withdrawal_rate,
                hold_raises: rate.care,
            };
            if !self.planner_started {
                self.sp_state = plan.planner.start(&inputs);
                self.planner_started = true;
                // The first year's actual draw on the portfolio.
                self.initial_wr = if inputs.portfolio_real > 0.0 {
                    (inputs.budget_real - other_income).max(0.0) / inputs.portfolio_real
                } else {
                    f64::NAN
                };
            }
            let decision = plan.planner.decide(&mut self.sp_state, &inputs);
            let wanted = decision.spending_real * price - ess_annual;
            self.disc_mult = if disc_annual > 0.0 {
                (wanted / disc_annual).max(0.0)
            } else {
                1.0
            };
            if decision.adjustment.abs() > 1e-9 {
                let kind = if decision.adjustment < 0.0 {
                    TraceEventKind::SpendingCut
                } else {
                    TraceEventKind::SpendingRaise
                };
                let adjustment = decision.adjustment;
                self.event(now, kind, Some(decision.spending_real), || {
                    format!("Spending rule: {:+.1}%", adjustment * 100.0)
                });
            }
        } else if !self.retired {
            self.disc_mult = 1.0;
        }

        // Required minimum distributions on the balances at the start of the year.
        let mut rmd = [0.0; MAX_PEOPLE];
        for i in 0..self.n {
            if !self.present[i] {
                continue;
            }
            let birth_year = plan.people[i].birth.year();
            if let Some(divisor) = plan
                .law
                .rmd
                .divisor((year - birth_year).max(0) as u32, birth_year)
            {
                let balance = self.acc.total(Bucket::Traditional(i));
                rmd[i] = if divisor > 0.0 {
                    balance / divisor
                } else {
                    0.0
                } * share;
                if !self.rmd_started[i] && rmd[i] > 0.0 {
                    self.rmd_started[i] = true;
                    let amount = rmd[i] / share / price;
                    self.event(now, TraceEventKind::RmdStarted, Some(amount), || {
                        format!("Person {} begins required distributions", i + 1)
                    });
                }
            }
            if self.sepp[i] > 0.0 && (t as i32) >= self.sepp_until[i] {
                self.sepp[i] = 0.0;
            }
        }
        self.pinputs = PlanInputs {
            ages: ages_now,
            ..PlanInputs::default()
        };
        let mut est_total = base_result.total / annualise;
        self.has_plan = self.retired || rmd.iter().any(|r| *r > 0.0);
        self.aca_advance_m = 0.0;
        // The plan depends on the premium credit (it lowers what must be withdrawn)
        // and the credit on the plan (withdrawals are income): cost the plan, and
        // once more if the credit it earns is not the one assumed.
        for pass in 0..2 {
            let spend_month =
                budget - self.disc_real * price * (1.0 - self.disc_mult) + rate.total() - advance;
            self.retire_cash_cap =
                plan.profile.portfolio.retirement_cash_years.max(0.0) * 12.0 * spend_month
                    + RETIRED_CASH_MONTHS * spend_month;
            est_total = base_result.total / annualise;
            let mut est_inputs = base;
            if self.has_plan {
                let due: f64 = if month <= 4 {
                    self.due.iter().sum()
                } else {
                    0.0
                };
                let net_need = spend_month * m_left + tax_rest_base - income_rest + due;
                self.pinputs = PlanInputs {
                    // The plan works at the tax year's scale: amounts for the months
                    // still to come, annualised when the year started part-way.
                    net_need: net_need * annualise,
                    ages: ages_now,
                    rmd: [rmd[0] * annualise, rmd[1] * annualise],
                    base_tax_inputs: base,
                    on_aca: rate.aca_people > 0,
                    sepp_available: [
                        self.sepp[0] * share * annualise,
                        self.sepp[1] * share * annualise,
                    ],
                    penalty_exempt: [false; MAX_PEOPLE],
                    // What an HSA may pay tax-free: not marketplace premiums.
                    medical_need: rate.medical * m_left * annualise,
                    cash_target: plan.profile.portfolio.retirement_cash_years.max(0.0)
                        * 12.0
                        * spend_month,
                    cash_floor: 0.0,
                    in_drawdown: self.in_drawdown(),
                };
                self.wplan = plan_year_from(
                    &plan.profile.retirement.withdrawal_order,
                    plan.profile.retirement.early_access,
                    &self.acc,
                    &self.pinputs,
                    &self.params,
                    &base_result,
                );
                self.exec_frac = 1.0 / (m_left * annualise);
                self.plan_fresh = true;
                est_total = self.wplan.estimated_tax / annualise;
                let gain_share = {
                    let value = self.acc.total(Bucket::Taxable);
                    if value > 0.0 {
                        (self.acc.unrealized_gain() / value).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                };
                for p in 0..MAX_PEOPLE {
                    est_inputs.traditional_distributions += self.wplan.from_traditional[p];
                    est_inputs.roth_conversions += self.wplan.roth_conversion[p];
                }
                est_inputs.long_term_gains +=
                    self.wplan.from_taxable * gain_share + self.wplan.gain_harvest;
            }
            // Premium credit advanced through the rest of the year.
            if rate.aca_people == 0 {
                break;
            }
            let credit = tax::compute(&self.params, &est_inputs).aca_premium_credit / annualise;
            self.aca_advance_m = (credit - self.aca_advance_ytd).max(0.0) / m_left;
            let expected = self.aca_advance_m.min(rate.aca_premium);
            if pass > 0
                || !self.has_plan
                || (expected - advance).abs() <= ACA_ADVANCE_TOLERANCE * spend_month
            {
                break;
            }
            advance = expected;
        }
        if rate.aca_people > 0 {
            self.aca_guess_real = self.aca_advance_m.min(rate.aca_premium) / price;
        }
        if self.has_plan {
            for p in 0..MAX_PEOPLE {
                if self.wplan.sepp[p] > 0.0 && self.sepp[p] <= 0.0 && p < self.n {
                    // A 72(t) schedule, once started, runs for five years or until
                    // 59 and a half, whichever is later, at the amount it began with.
                    self.sepp[p] = self.wplan.sepp[p] / (share * annualise);
                    let to_60 = (59 * 12 + 6 - self.age_months(plan, p, t)).max(0);
                    self.sepp_until[p] = t as i32 + to_60.max(60);
                }
            }
            if self.detail {
                let converted: f64 = self.wplan.roth_conversion.iter().sum::<f64>() / annualise;
                if converted > 1.0 {
                    self.event(
                        now,
                        TraceEventKind::RothConversion,
                        Some(converted / price),
                        || "Roth conversion planned for the year".to_string(),
                    );
                }
            }
        }

        // Withholding and estimated payments for the months left.
        let nonfica_total = (est_total - fica_est / annualise).max(0.0);
        let rest = (nonfica_total - paid_nonfica).max(0.0);
        let base_rest = (base_nonfica - paid_nonfica).clamp(0.0, rest);
        self.wh_rate = if wage_like_rest > 0.0 {
            (base_rest / wage_like_rest).min(MAX_WITHHOLDING_RATE)
        } else {
            0.0
        };
        self.installment = (rest - self.wh_rate * wage_like_rest).max(0.0) / m_left;
        let nonfica_est = (base_result.total - fica_est).max(1e-9);
        self.share_state =
            ((base_result.state + base_result.foreign) / nonfica_est).clamp(0.0, 1.0);
        self.share_se = (se_est / nonfica_est).clamp(0.0, 1.0 - self.share_state);

        // Medicare surcharges.
        if new_year || first {
            let magi = if self.magi_known >= 2 {
                self.magi_hist[0]
            } else {
                base_result.magi_irmaa
            };
            for i in 0..MAX_PEOPLE {
                self.irmaa_m[i] = if i < self.n && self.present[i] && self.age_ye[i] >= 65.0 {
                    tax::irmaa_monthly_surcharge(&self.params, status, magi)
                } else {
                    0.0
                };
            }
        }

        // The savings waterfall's view of the year.
        let earned = (wages.iter().sum::<f64>() + business.iter().sum::<f64>()) * annualise
            + (self.ty.wages.iter().sum::<f64>() + self.ty.self_employment.iter().sum::<f64>())
                * annualise;
        let mut person = [PersonSavings::default(); MAX_PEOPLE];
        let mut covered = [false; MAX_PEOPLE];
        let mut hsa_plan = [false; MAX_PEOPLE];
        let mut mega = [false; MAX_PEOPLE];
        for (rt, jp) in self.jobs.iter().zip(&plan.jobs) {
            if matches!(rt.st.employment, Employment::Employed) {
                covered[jp.owner] |= rt.plan.is_some();
                hsa_plan[jp.owner] |= jp.hsa_real > 0.0 || jp.hsa_employer_real > 0.0;
                mega[jp.owner] |= rt.plan.is_some_and(|p| p.after_tax_mega);
            }
        }
        for p in 0..self.n {
            let partner_covered = covered[MAX_PEOPLE - 1 - p];
            person[p] = PersonSavings {
                present: self.present[p],
                age: self.age_ye[p],
                hsa_eligible: hsa_plan[p] && self.age_ye[p] < 65.0,
                traditional_deductible: tax::ira_deductible_fraction(
                    &self.params,
                    status,
                    base_result.agi,
                    covered[p],
                    partner_covered,
                ) >= 0.5,
                after_tax_401k: mega[p],
            };
        }
        self.sctx = SavingsContext {
            limits: self.params.federal.limits,
            filing_status: status,
            magi: base_result.agi,
            earned_income: earned,
            marginal_rate: base_result.marginal_ordinary_rate,
            retirement_rate: plan.retirement_rate,
            hsa_family: adults + young + other > 1,
            person,
        };
        self.req_dirty = true;
    }

    /// Rebalances to the current target.
    ///
    /// On a marketplace plan a taxable sale made only to rebalance can cost the
    /// whole premium credit, so a household that protects the credit trades inside
    /// its tax-advantaged accounts and leaves the taxable account alone. Once it
    /// has done that, the taxable account is off target on purpose; from then on it
    /// is rebalanced tax-efficiently (sold only where the household as a whole is
    /// overweight) rather than snapped back to the target in one taxable sale.
    pub(crate) fn rebalance(&mut self, plan: &Plan) {
        if self.guards_aca_credit(plan) {
            self.acc.rebalance_sheltered(&self.target);
            self.sheltered_rebalancing = true;
            return;
        }
        let location = if self.sheltered_rebalancing {
            AssetLocation::TaxEfficient
        } else {
            plan.profile.portfolio.location
        };
        let r = self.acc.rebalance_to(&self.target, location);
        self.ty.long_term_gains += r.long_term_gain;
    }

    /// True when the plan protects the marketplace premium credit and someone in
    /// the household is on a marketplace plan.
    pub(crate) fn guards_aca_credit(&self, plan: &Plan) -> bool {
        matches!(
            plan.profile.retirement.withdrawal_order,
            WithdrawalOrder::TaxAware { mind_aca: true, .. }
        ) && self.params.kind == TaxKind::UsCurrentLaw
            && self.foreign_rate.is_none()
            && (0..self.n).any(|i| self.present[i] && self.last_phase[i] == CoveragePhase::Aca)
    }

    /// Employee FICA withheld so far this year (the exact amounts paid through
    /// payroll), reconstructed from the wage tally.
    fn fica_paid_estimate(&self) -> f64 {
        if !self.fica_on {
            return 0.0;
        }
        let f = &self.params.federal;
        let mut paid = 0.0;
        for p in 0..MAX_PEOPLE {
            let wages = (self.ty.wages[p] - self.ty.hsa_contributions[p]).max(0.0);
            paid += f.oasdi_rate * wages.min(f.oasdi_wage_base) + f.medicare_rate * wages;
        }
        paid
    }

    /// The household's earned-income rate in today's dollars per year: the pay of
    /// everyone employed right now and expected business profit.
    fn income_rate(&self, plan: &Plan, t: usize) -> f64 {
        let mut rate = 0.0;
        for (rt, jp) in self.jobs.iter().zip(&plan.jobs) {
            if self.present[jp.owner] && matches!(rt.st.employment, Employment::Employed) {
                rate += rt.st.real_salary * (1.0 + jp.bonus_target) + jp.commission_real;
            }
        }
        for (k, (rt, sp)) in self.selfemp.iter().zip(&plan.selfemp).enumerate() {
            let ti = t as i32;
            if self.present[sp.owner]
                && !rt.ended
                && ti >= sp.start.month(self.retire_month)
                && ti < sp.end.month(self.retire_month)
            {
                rate += sp.model.expected_real_annual(self.se_years(plan, k, t))
                    * rt.st.factor
                    * rt.scale;
            }
        }
        rate
    }

    /// Years until retirement for the glide path: negative once retired, the known
    /// date when the trigger fixes one, otherwise an estimate from how far the
    /// household is from its number (growth at 5% real, capped by the latest age).
    fn years_to_retirement(&mut self, plan: &Plan, t: usize, price: f64) -> Option<f64> {
        let ti = t as i32;
        if self.retire_month != i32::MAX {
            return Some(f64::from(self.retire_month - ti) / 12.0);
        }
        match plan.trigger {
            TriggerPlan::Funded {
                rule, max_month, ..
            } => {
                let cap = f64::from((max_month - ti).max(0)) / 12.0;
                if self.req_dirty {
                    self.req = self.requirement(plan, t, price, &rule);
                    self.req_dirty = false;
                }
                let wealth = self.wealth(rule.basis, price);
                let years = if wealth <= 0.0 {
                    cap
                } else if wealth >= self.req.total {
                    0.0
                } else {
                    math::ln(self.req.total / wealth) / math::ln(1.05)
                };
                Some(years.min(cap))
            }
            _ => None,
        }
    }
}
