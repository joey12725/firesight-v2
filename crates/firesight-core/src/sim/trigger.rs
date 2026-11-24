//! Financial independence, Coast-FI and the retirement trigger.
//!
//! # The funded test (`when_funded`)
//!
//! ```text
//! wealth      = investable assets - non-mortgage debt        (basis: investable)
//!             = that + property value - mortgages             (basis: net_worth)
//! need        = retirement budget + home running costs + modelled healthcare
//!               + the average yearly cost of replacing `spending.vehicles`
//!               - income already being received that continues
//!               + tax on the withdrawals that pay for it
//! gross       = need / withdrawal_rate
//!               + mortgages on homes (investable basis) + child costs still due
//! requirement = gross                                   (count_future_income off)
//!             = max(gross - PV(income not yet started),
//!                   min(gross, bridge))                 (count_future_income on)
//! funded when wealth >= requirement
//! ```
//!
//! `bridge` is what liquid savings must be able to do on their own, because
//! income that starts in twenty years cannot be spent today: pay the whole need
//! until each income starts, and the remaining need after it,
//!
//! ```text
//! bridge = sum over the years before each income starts of the need still unmet,
//!          discounted at rho
//!        + (need - all future income) / withdrawal_rate, discounted at rho
//!        + the same lumps
//! rho    = the real return at which the withdrawal rate exhausts a portfolio in
//!          30 years (1.2% for a 4% rule): the horizon the rule was designed for
//! ```
//!
//! For a household close to its Social Security claim the present-value form
//! binds; for one decades away the bridge binds and future income counts for
//! little or nothing, which is what the simulated depletion rates say it is worth.
//!
//! * The retirement budget is the budget as it would be if the household retired
//!   this month: each line's `in_retirement` treatment, the lifestyle creep so far,
//!   today's cost-of-living factors.
//! * Healthcare is the expected cost in the coverage phase the household would be in
//!   (marketplace, COBRA or a custom plan before 65, Medicare after), averaged over
//!   the next thirty years, less the premium credit the retirement income would earn.
//! * Tax is `tax::compute` on a year of retirement income: the withdrawals split in
//!   proportion to the accounts held (traditional money as ordinary income, the gain
//!   share of taxable money as long-term gain), grossed up at its marginal rate (the
//!   tax has to be withdrawn, and taxed, too).
//! * Income that has started (a pension in payment, Social Security already claimed,
//!   rent) reduces the annual need. Income still to come is worth its present value
//!   with the person's survival curve, discounted at the rule's own withdrawal rate
//!   (never below 2% real: `INCOME_DISCOUNT_RATE`).
//!
//! The requirement is recomputed when something material changes (each January, a
//! life event, a death, a job lost or found) and compared with wealth every month.
//!
//! # FI age and retirement age
//!
//! The FI age is the first month the funded test is met. For `when_funded` the
//! household retires the month after (once `min_age` is reached) or at `max_age`
//! regardless. For `at_age`, `at_date` and `already_retired` the test is still run,
//! with the default rule (4%, investable, counting future income), until the
//! retirement date: it answers "was the plan funded by the time they stopped?".
//! `fixed_number` is met when wealth reaches the amount.
//!
//! # Coast-FI
//!
//! The first month in which today's wealth, with no further saving, grown at the
//! plan's expected real return to the planned (or latest) retirement age, would meet
//! the requirement as it would stand then (future income valued at that age).

use crate::accounts::Bucket;
use crate::health::{CoveragePhase, HealthContext};
use crate::model::{PropertyUse, WealthBasis, WhenFunded};
use crate::stats::TraceEventKind;
use crate::tax::{self, TaxInputs};
use crate::MAX_PEOPLE;

use super::plan::{Plan, StreamKind, TriggerPlan};
use super::state::State;

/// Years over which pre-Medicare and Medicare health costs are averaged in the
/// funded test.
const HEALTH_AVERAGING_YEARS: f64 = 30.0;

/// The funded test's working, today's dollars.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Requirement {
    /// Annual spending to be funded from savings, before tax.
    pub need: f64,
    /// Tax on the withdrawals, per year.
    pub tax: f64,
    /// Sums set aside rather than funded for ever: mortgage balances (investable
    /// basis) and child costs still due.
    pub lumps: f64,
    /// `(need + tax) / withdrawal_rate + lumps`.
    pub gross: f64,
    /// Present value now of income not yet started.
    pub pv_income: f64,
    /// Present value of that income at the Coast-FI target month.
    pub pv_income_at_coast: f64,
    /// What savings must cover on their own until future income starts (see the
    /// module documentation); equals `gross` when no income is still to come.
    pub bridge: f64,
    /// The requirement: `gross`, or with future income counted the larger of
    /// `gross - pv_income` and `min(gross, bridge)`; not below zero.
    pub total: f64,
}

/// Most future incomes the bridge calculation follows individually.
const MAX_BRIDGE_INCOMES: usize = 8;

/// `(1 + rho)^-years` with `rho` the plan's bridge discount rate: from the plan's
/// table when `years` is a whole number of months within it (it always is in the
/// simulation), computed otherwise. The table holds the same `pow` results.
#[inline]
fn bridge_power(plan: &Plan, years: f64) -> f64 {
    let months = years * 12.0 + 0.5;
    if months >= 0.0 && months < plan.bridge_pow.len() as f64 {
        let m = months as usize;
        if m as f64 / 12.0 == years {
            return plan.bridge_pow[m];
        }
    }
    crate::math::pow(1.0 + plan.bridge_discount, -years)
}

/// Present value of one real dollar a year for `years` at the plan's bridge
/// discount rate.
#[inline]
fn annuity(plan: &Plan, years: f64) -> f64 {
    let rho = plan.bridge_discount;
    if rho.abs() < 1e-9 {
        years
    } else {
        (1.0 - bridge_power(plan, years)) / rho
    }
}

impl State {
    /// The rule the funded test uses: the trigger's own, or the default.
    pub(crate) fn funding_rule(&self, plan: &Plan) -> WhenFunded {
        match plan.trigger {
            TriggerPlan::Funded { rule, .. } => rule,
            TriggerPlan::Fixed { basis, .. } => WhenFunded {
                basis,
                ..WhenFunded::default()
            },
            _ => WhenFunded::default(),
        }
    }

    /// Wealth that counts toward the target, today's dollars.
    pub(crate) fn wealth(&self, basis: WealthBasis, price: f64) -> f64 {
        let mut w = self.acc.investable() - self.debt_total();
        if basis == WealthBasis::NetWorth {
            let (value, mortgage) = self.property_totals();
            w += value - mortgage;
        }
        w / price
    }

    /// The funded test's requirement as things stand in month `t`.
    pub(crate) fn requirement(
        &self,
        plan: &Plan,
        t: usize,
        price: f64,
        rule: &WhenFunded,
    ) -> Requirement {
        let ti = t as i32;
        let retire_month = if self.retire_month == i32::MAX {
            ti
        } else {
            self.retire_month.min(ti)
        };
        let (ess, disc, _) = self.budget_as(plan, t, true, retire_month);
        let mut need = (ess + disc) * 12.0;
        if !plan.sx.vehicles.is_empty() {
            need += self.sx_vehicle_need(plan, ti, retire_month);
        }
        let mut lumps = 0.0;

        // Homes cost money to run for ever; their mortgages are a debt to clear.
        // A rental is income (possibly negative) that continues.
        let mut rental = 0.0;
        for (st, pp) in self.props.iter().zip(&plan.props) {
            if !st.owned {
                continue;
            }
            let (operating, mortgage, rent) = pp.model.expected_month(st, price);
            if matches!(st.usage, PropertyUse::Rental(_)) {
                rental += (rent - operating - mortgage) * 12.0 / price;
            } else {
                need += operating * 12.0 / price;
                if rule.basis == WealthBasis::Investable {
                    lumps += st.mortgage_balance() / price;
                } else {
                    need += mortgage * 12.0 / price;
                }
            }
        }
        if self.hd.any_line && rule.basis == WealthBasis::Investable {
            // Borrowing against the home is a debt to clear, like a mortgage.
            lumps += self.hd.liability() / price;
        }

        // Healthcare once retired.
        let mut health = 0.0;
        let mut aca_benchmark = 0.0;
        let mut aca_people = 0u32;
        let mut pre65_weight: f64 = 0.0;
        let mut adults = 0u32;
        let mut ages = [0.0; MAX_PEOPLE];
        for i in 0..self.n {
            if !self.present[i] {
                continue;
            }
            adults += 1;
            let age = self.age(plan, i, t);
            ages[i] = age.floor();
            let phase = plan.health.phase(&HealthContext {
                age,
                employer_coverage: false,
                months_since_coverage_ended: u32::MAX,
                in_us: self.in_us,
                months_since_start: t as u32,
                price_level: price,
            });
            let now_cost = plan.health.expected_annual_cost_at(age, phase, t as u32);
            if phase == CoveragePhase::Medicare || phase == CoveragePhase::None {
                health += now_cost;
            } else {
                let w = ((65.0 - age) / HEALTH_AVERAGING_YEARS).clamp(0.0, 1.0);
                let later =
                    plan.health
                        .expected_annual_cost_at(65.0, CoveragePhase::Medicare, t as u32);
                health += w * now_cost + (1.0 - w) * later;
                if phase == CoveragePhase::Aca {
                    aca_people += 1;
                    aca_benchmark += plan.health.aca_benchmark_monthly(age)
                        * plan.health.excess_growth(t as u32)
                        * 12.0;
                    pre65_weight = pre65_weight.max(w);
                }
            }
        }
        need += health;

        // Child costs still to come, as a sum to set aside.
        if ti < self.kids_until {
            for (k, c) in plan.children.iter().enumerate() {
                let Some((birth, n)) = self.child_for_funding(plan, k, ti) else {
                    continue;
                };
                let age = (ti - birth).max(0);
                let mut m = age;
                // Whole years from the next birthday on, the part-year first.
                if m < 22 * 12 && m % 12 != 0 {
                    let years = (m / 12) as usize;
                    let annual = if years < 18 {
                        c.core[years] + c.care[years]
                    } else {
                        c.college
                    };
                    let months = 12 - m % 12;
                    lumps += annual * f64::from(months) / 12.0 * n;
                    m += months;
                }
                if m < 22 * 12 {
                    for lump in &c.lump_year[(m / 12) as usize..] {
                        lumps += lump * n;
                    }
                }
            }
        }

        // Income that continues, and income still to come.
        let mut continuing = rental;
        let mut pensions = 0.0;
        let mut other_taxable = 0.0;
        let mut pv = 0.0;
        let mut pv_coast = 0.0;
        // Income still to come: (years until it starts, annual amount).
        let mut future = [(0.0f64, 0.0f64); MAX_BRIDGE_INCOMES];
        let mut n_future = 0;
        let coast = plan.coast_month.max(ti) as usize;
        for (rt, sp) in self.streams.iter().zip(&plan.streams) {
            if rt.ended {
                continue;
            }
            let owner_alive = self.alive[sp.owner];
            let annual = sp.annual * rt.scale;
            if rt.started {
                let real = rt.last * 12.0 / price;
                match sp.kind {
                    StreamKind::Other {
                        ends_at_retirement: true,
                        ..
                    } => {}
                    StreamKind::Other { taxable, .. } => {
                        continuing += real;
                        if taxable {
                            other_taxable += real;
                        }
                    }
                    _ => {
                        continuing += real;
                        pensions += real;
                    }
                }
            } else if rule.count_future_income && owner_alive && !sp.pv.is_empty() {
                pv += annual / 12.0 * sp.pv.get(t).copied().unwrap_or(0.0);
                pv_coast += annual / 12.0 * sp.pv.get(coast).copied().unwrap_or(0.0);
                if n_future < MAX_BRIDGE_INCOMES {
                    let wait = (sp.start.month(self.retire_month) - ti).max(0);
                    future[n_future] = (f64::from(wait) / 12.0, annual);
                    n_future += 1;
                }
            }
        }
        let mut benefits = [0.0; MAX_PEOPLE];
        // A policy-risk cut that has already happened is known to the household
        // (exactly 1 before it, and without `policy_risk`).
        let ss_cut = self.trz.policy.ss_scale((plan.start + ti).year());
        for i in 0..self.n {
            if !self.present[i] {
                continue;
            }
            let Some(c) = &self.claimants[i] else {
                continue;
            };
            let Some(pia) = c.pia_real else { continue };
            let pia = pia * ss_cut;
            if self.ss_claimed[i] {
                // Paid already: the benefit at its claimed level.
                let fra = plan.ss.full_retirement_age_months(c.birth.year());
                let claim = (c.claim_age.clamp(62.0, 70.0) * 12.0).round() as u32;
                let annual = pia * plan.ss.claim_factor(claim, fra) * 12.0;
                benefits[i] = annual;
                continuing += annual;
            } else if rule.count_future_income {
                pv += plan.people[i].ss_annuity.value(t as u32, pia);
                pv_coast += plan.people[i].ss_annuity.value(coast as u32, pia);
                if n_future < MAX_BRIDGE_INCOMES {
                    let fra = plan.ss.full_retirement_age_months(c.birth.year());
                    let claim = (c.claim_age.clamp(62.0, 70.0) * 12.0).round() as u32;
                    let wait = (claim as i32 - self.age_months(plan, i, t)).max(0);
                    future[n_future] = (
                        f64::from(wait) / 12.0,
                        pia * plan.ss.claim_factor(claim, fra) * 12.0,
                    );
                    n_future += 1;
                }
            }
        }
        need -= continuing;

        // Tax on a year of retirement income, at today's law.
        let investable = self.acc.investable();
        let (trad_share, gain_share) = if investable > 0.0 {
            let kinds = self.acc.by_kind();
            let taxable = self.acc.total(Bucket::Taxable);
            let gain = if taxable > 0.0 {
                (self.acc.unrealized_gain() / taxable).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (kinds[2] / investable, kinds[1] / investable * gain)
        } else {
            (1.0, 0.0)
        };
        let mut x = TaxInputs {
            filing_status: self.filing,
            ages,
            adults: adults.max(1),
            state: self.params.state.code,
            pensions: pensions * price,
            other_ordinary: other_taxable * price,
            rental_net: rental.max(0.0) * price,
            social_security: [benefits[0] * price, benefits[1] * price],
            ..TaxInputs::default()
        };
        if aca_people > 0 {
            x.aca_household_size = adults;
            x.aca_months = 12;
            x.aca_benchmark_premium = aca_benchmark * price;
        }
        let withdrawals = need.max(0.0) * price;
        // One return on the net withdrawals. The tax has to be withdrawn, and taxed,
        // too: G = W + tax(G) gives, to first order, tax(G) = tax(W) / (1 - m) with
        // m the marginal rate on one more dollar withdrawn in the same proportions.
        x.traditional_distributions = withdrawals * trad_share;
        x.long_term_gains = withdrawals * gain_share;
        let r = tax::compute(&self.params, &x);
        let credit = r.aca_premium_credit;
        let mut tax_total = r.total.max(0.0);
        if withdrawals > 0.0 && tax_total > 0.0 {
            let average = tax_total / (withdrawals + tax_total);
            let marginal = (trad_share * r.marginal_ordinary_rate).max(average);
            tax_total /= 1.0 - marginal.min(0.9);
        }
        let tax_real = tax_total / price;
        need -= credit / price * pre65_weight;

        let rate = rule.withdrawal_rate.max(1e-4);
        let annual = (need + tax_real).max(0.0);
        let gross = annual / rate + lumps;

        // The bridge: the need until each income starts, then what is left of it.
        future[..n_future].sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut bridge = lumps;
        let mut level = annual;
        let mut from = 0.0;
        for &(wait, income) in &future[..n_future] {
            bridge += level * (annuity(plan, wait) - annuity(plan, from));
            level = (level - income.max(0.0)).max(0.0);
            from = wait;
        }
        bridge += level / rate * bridge_power(plan, from);
        let total = if rule.count_future_income {
            (gross - pv).max(bridge.min(gross))
        } else {
            gross
        };
        Requirement {
            need,
            tax: tax_real,
            lumps,
            gross,
            pv_income: pv,
            pv_income_at_coast: pv_coast,
            bridge,
            total: total.max(0.0),
        }
    }

    /// Step 11 for a household still working: the funded test, Coast-FI, and the
    /// decision to retire next month.
    pub(crate) fn check_trigger(&mut self, plan: &Plan, t: usize, price: f64, investable: f64) {
        let ti = t as i32;
        let rule = self.funding_rule(plan);
        if self.req_dirty {
            self.req = self.requirement(plan, t, price, &rule);
            self.req_dirty = false;
        }
        let mut wealth = investable - self.debt_total();
        if rule.basis == WealthBasis::NetWorth {
            let (value, mortgage) = self.property_totals();
            wealth += value - mortgage;
        }
        let wealth = wealth / price;
        let funded = match plan.trigger {
            TriggerPlan::Fixed { amount, .. } => wealth >= amount,
            _ => wealth >= self.req.total,
        };
        if funded && self.fi_month == i32::MAX {
            self.fi_month = ti + 1;
            let now = plan.start + ti + 1;
            self.event(
                now,
                TraceEventKind::FinanciallyIndependent,
                Some(wealth),
                || "Financially independent: savings cover the retirement budget".to_string(),
            );
        }
        if self.coast_month == i32::MAX && ti < plan.coast_month && wealth > 0.0 {
            let target = (self.req.gross - self.req.pv_income_at_coast).max(0.0);
            if wealth >= target * plan.coast_discount[t] {
                self.coast_month = ti + 1;
            }
        }
        match plan.trigger {
            TriggerPlan::Funded {
                min_month,
                max_month,
                ..
            } => {
                let mut go = funded && ti + 1 >= min_month;
                if plan.beh.any {
                    go = self.confirm_retirement(plan, t, go);
                }
                if go || ti + 1 >= max_month {
                    self.retire_month = ti + 1;
                }
            }
            TriggerPlan::Fixed { .. } => {
                if funded {
                    self.retire_month = ti + 1;
                }
            }
            TriggerPlan::Already | TriggerPlan::AtMonth(_) => {}
        }
    }
}
