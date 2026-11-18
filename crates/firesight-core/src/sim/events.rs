//! Life events: what each kind does to the path in the month it fires.
//!
//! Whether and when an event happens is drawn once per path on the `Events` stream
//! (two draws per event, in profile order, whatever its settings). Everything an
//! event costs is paid out of cash like any other expense, so a purchase the
//! household cannot fund sells investments, and one it cannot fund at all leaves an
//! overdraft: nothing is free.
//!
//! * `buy_home`: the down payment and closing costs leave cash; the property and its
//!   mortgage appear. If cash plus taxable investments cannot cover the cash needed,
//!   the purchase waits, month by month, for up to five years and is then dropped.
//! * `sell_property`: proceeds arrive in cash; the taxable gain, recaptured
//!   depreciation and any suspended rental losses go on the year's return.
//! * `relocate`: the moving cost is paid, budget lines are rescaled by the
//!   cost-of-living factors of their category, and the tax residence changes. The
//!   whole calendar year of the move is taxed by the new residence (the return uses
//!   the residence at year end).
//! * `expense`: paid in cash, or the down payment in cash and the rest as a new
//!   amortising debt.
//! * `income_change`, `career_break`, `spending_change`, `contribution_change`:
//!   permanent changes to the stream, job or budget lines they name.

use crate::accounts::Bucket;
use crate::career::Employment;
use crate::economy::EconomyPath;
use crate::housing::amortized_payment;
use crate::model::{Category, IncomeChangeKind, PropertyUse, SpendingChangeKind};
use crate::stats::{TraceEventKind, YearSeries};
use crate::tax::StateCode;

use super::plan::{EvKind, IncomeRef, Plan, SpendRef, PURCHASE_RETRY_MONTHS};
use super::state::State;

impl State {
    /// Fires every event due in month `t` and finds the next one.
    pub(crate) fn run_events(&mut self, plan: &Plan, econ: &EconomyPath, t: usize) {
        let ti = t as i32;
        let mut next = i32::MAX;
        for e in 0..self.events.len() {
            let rt = self.events[e];
            if rt.done {
                continue;
            }
            if rt.month <= ti {
                if self.fire(plan, econ, e, t) {
                    self.events[e].done = true;
                } else {
                    // Postponed: try again next month.
                    self.events[e].month = ti + 1;
                    self.events[e].retries += 1;
                    next = next.min(ti + 1);
                }
            } else {
                next = next.min(rt.month);
            }
        }
        self.next_event = next;
        self.req_dirty = true;
    }

    /// Applies event `e`. Returns false if it must wait.
    fn fire(&mut self, plan: &Plan, econ: &EconomyPath, e: usize, t: usize) -> bool {
        let now = plan.start + t as i32;
        let price = econ.price_level[t];
        let ev = &plan.events[e];
        match &ev.kind {
            EvKind::BuyHome {
                slot,
                replaces_rent,
            } => {
                let slot = *slot;
                if self.props[slot].owned {
                    return true;
                }
                // Dry run on a copy to learn the cash needed.
                let mut probe = self.props[slot];
                let quote = plan.props[slot].model.buy(
                    &mut probe,
                    t,
                    self.home_index,
                    econ.mortgage_rate[t],
                );
                if quote.price <= 0.0 {
                    return true;
                }
                let available = self.acc.cash.max(0.0) + self.acc.total(Bucket::Taxable);
                if available < quote.cash_needed {
                    if self.events[e].retries >= PURCHASE_RETRY_MONTHS {
                        self.event(now, TraceEventKind::LifeEvent, None, || {
                            format!("{}: abandoned, never affordable", ev.name)
                        });
                        return true;
                    }
                    return false;
                }
                self.props[slot] = probe;
                self.pay(YearSeries::PropertyPurchase, quote.down_payment);
                self.spend(YearSeries::SpendOneOff, quote.closing_costs);
                if self.check {
                    self.chk_prop_flow += quote.price;
                    self.chk_mort_flow += quote.loan;
                }
                if *replaces_rent {
                    for (scale, item) in self.item_scale.iter_mut().zip(&plan.items) {
                        if item.category == Category::Rent {
                            *scale = 0.0;
                        }
                    }
                    self.extra_rent = 0.0;
                }
                self.budget_dirty = true;
                self.event(
                    now,
                    TraceEventKind::BoughtHome,
                    Some(quote.price / price),
                    || format!("{} ({:.2}% mortgage)", ev.name, quote.rate * 100.0),
                );
            }
            EvKind::Sell {
                slot,
                cost_fraction,
                then_rent,
            } => {
                let Some(slot) = *slot else { return true };
                if !self.props[slot].owned {
                    // Not bought yet (the purchase was postponed): wait for it.
                    return self.events[e].retries >= PURCHASE_RETRY_MONTHS;
                }
                let before = self.props[slot];
                let exclusion = plan.home_exclusion[self.filing.index()];
                let sale = plan.props[slot].model.sell(
                    &mut self.props[slot],
                    t,
                    *cost_fraction,
                    exclusion,
                );
                if sale.net_proceeds >= 0.0 {
                    self.recv(YearSeries::PropertySale, sale.net_proceeds);
                } else {
                    self.spend(YearSeries::SpendOneOff, -sale.net_proceeds);
                }
                if self.check {
                    self.chk_prop_flow -= before.value;
                    self.chk_mort_flow -= sale.mortgage_payoff;
                }
                self.ty.long_term_gains += sale.taxable_gain;
                self.ty.depreciation_recapture += sale.depreciation_recapture;
                if matches!(before.usage, PropertyUse::Rental(_)) {
                    // The household-level suspended losses are released with the
                    // property (the return tracks them for all rentals together).
                    self.ty.passive_loss_released += self.rental_carry;
                    self.rental_carry = 0.0;
                }
                if *then_rent > 0.0 {
                    self.extra_rent += *then_rent;
                }
                self.budget_dirty = true;
                self.event(
                    now,
                    TraceEventKind::SoldHome,
                    Some(sale.net_proceeds / price),
                    || format!("{}: sold {}", ev.name, plan.props[slot].name),
                );
            }
            EvKind::Relocate {
                state,
                in_us,
                foreign_rate,
                local_rate,
                moving_cost,
                factors,
            } => {
                if *moving_cost > 0.0 {
                    self.spend(YearSeries::SpendOneOff, moving_cost * price);
                }
                for (scale, item) in self.item_scale.iter_mut().zip(&plan.items) {
                    *scale *= factors[item.category.index()];
                }
                self.extra_rent *= factors[Category::Rent.index()];
                self.tax_state = *state;
                self.in_us = *in_us;
                self.local_rate = *local_rate;
                self.foreign_rate = if *in_us {
                    None
                } else {
                    Some(foreign_rate.unwrap_or(0.0))
                };
                if !matches!(
                    plan.profile.assumptions.taxes.kind,
                    crate::model::TaxKind::UsCurrentLaw
                ) {
                    self.foreign_rate = None;
                }
                self.budget_dirty = true;
                let code = *state;
                self.event(now, TraceEventKind::Relocated, None, || {
                    if code == StateCode::NONE {
                        ev.name.clone()
                    } else {
                        format!("{} (to {})", ev.name, code.as_str())
                    }
                });
            }
            EvKind::Windfall { amount, taxable } => {
                let nominal = amount * price;
                self.recv(YearSeries::IncomeOther, nominal);
                if *taxable {
                    self.ty.other_ordinary += nominal;
                    self.wage_like += nominal;
                }
                self.event(now, TraceEventKind::Windfall, Some(*amount), || {
                    ev.name.clone()
                });
            }
            EvKind::Expense { amount, financed } => {
                let nominal = amount * price;
                match financed {
                    Some((down, slot, rate, term)) => {
                        let cash = nominal * down;
                        let loan = nominal - cash;
                        self.spend(YearSeries::SpendOneOff, cash);
                        let d = &mut self.debts[*slot];
                        d.balance += loan;
                        d.rate = *rate;
                        d.payment = amortized_payment(d.balance, *rate, *term);
                        self.debts_active = true;
                        if self.check {
                            self.chk_debt_flow += loan;
                        }
                    }
                    None => self.spend(YearSeries::SpendOneOff, nominal),
                }
                self.event(now, TraceEventKind::MajorExpense, Some(*amount), || {
                    ev.name.clone()
                });
            }
            EvKind::IncomeChange { target, change } => {
                match *target {
                    Some(IncomeRef::Job(j)) => {
                        let model = &plan.jobs[j].model;
                        let rt = &mut self.jobs[j];
                        match *change {
                            IncomeChangeKind::SetSalary { amount } => {
                                model.set_real_salary(&mut rt.st, amount.max(0.0));
                            }
                            IncomeChangeKind::Scale { factor } => {
                                model.scale_real_salary(&mut rt.st, factor.max(0.0));
                            }
                            IncomeChangeKind::End => {
                                model.end(&mut rt.st);
                                rt.ended = true;
                            }
                        }
                    }
                    Some(IncomeRef::Se(k)) => {
                        let rt = &mut self.selfemp[k];
                        match *change {
                            IncomeChangeKind::SetSalary { amount } => {
                                let expected =
                                    plan.selfemp[k].model.expected_real_annual(t as f64 / 12.0);
                                rt.scale = if expected > 0.0 {
                                    amount.max(0.0) / (expected * rt.st.factor.max(1e-9))
                                } else {
                                    0.0
                                };
                            }
                            IncomeChangeKind::Scale { factor } => rt.scale *= factor.max(0.0),
                            IncomeChangeKind::End => rt.ended = true,
                        }
                    }
                    Some(IncomeRef::Stream(k)) => {
                        let rt = &mut self.streams[k];
                        match *change {
                            IncomeChangeKind::SetSalary { amount } => {
                                let annual = plan.streams[k].annual;
                                rt.scale = if annual > 0.0 {
                                    amount.max(0.0) / annual
                                } else {
                                    0.0
                                };
                            }
                            IncomeChangeKind::Scale { factor } => rt.scale *= factor.max(0.0),
                            IncomeChangeKind::End => rt.ended = true,
                        }
                    }
                    None => {}
                }
                self.event(now, TraceEventKind::LifeEvent, None, || ev.name.clone());
            }
            EvKind::CareerBreak {
                person,
                months,
                fraction,
            } => {
                for (rt, jp) in self.jobs.iter_mut().zip(&plan.jobs) {
                    if jp.owner == *person && rt.st.employment == Employment::Employed {
                        jp.model.start_break(&mut rt.st, *months, *fraction);
                    }
                }
                self.event(now, TraceEventKind::CareerBreak, None, || {
                    format!("{} ({} months)", ev.name, months)
                });
            }
            EvKind::SpendingChange { target, change } => {
                let selected = |i: usize| match *target {
                    SpendRef::Item(k) => i == k,
                    SpendRef::Category(c) => plan.items[i].category == c,
                    SpendRef::All => true,
                };
                match *change {
                    SpendingChangeKind::Scale { factor } => {
                        for i in 0..self.item_scale.len() {
                            if selected(i) {
                                self.item_scale[i] *= factor.max(0.0);
                            }
                        }
                    }
                    SpendingChangeKind::Set { annual } => {
                        // Scale the selected lines so that together they come to the
                        // new annual amount (in the household's current mode).
                        let mut current = 0.0;
                        for (i, item) in plan.items.iter().enumerate() {
                            if selected(i) {
                                let monthly = if self.retired {
                                    item.monthly_retired
                                } else {
                                    item.monthly
                                };
                                current += monthly * self.item_scale[i] * 12.0;
                            }
                        }
                        if current > 0.0 {
                            let k = annual.max(0.0) / current;
                            for i in 0..self.item_scale.len() {
                                if selected(i) {
                                    self.item_scale[i] *= k;
                                }
                            }
                        }
                    }
                }
                self.budget_dirty = true;
                self.event(now, TraceEventKind::LifeEvent, None, || ev.name.clone());
            }
            EvKind::ContributionChange { job, fraction } => {
                if let Some(j) = *job {
                    if let Some(p) = &mut self.jobs[j].plan {
                        p.employee_fraction = *fraction;
                        p.employee_max = false;
                    }
                }
                self.event(now, TraceEventKind::LifeEvent, None, || ev.name.clone());
            }
        }
        true
    }
}
