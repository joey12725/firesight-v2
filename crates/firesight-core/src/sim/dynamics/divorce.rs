//! Divorce: the second person leaves the household.
//!
//! What happens in the month of the divorce, in order:
//!
//! 1. Unless the person who stays keeps the home, every primary residence is sold
//!    (6% selling costs, the gain on the year's return) and the household rents.
//! 2. The departing partner takes `asset_split` of the household's net worth
//!    (liquid accounts plus property equity less mortgages and debts), paid out of
//!    cash, taxable and retirement accounts in proportion to their balances. A
//!    transfer incident to divorce is not taxable (IRC 1041; a qualified domestic
//!    relations order for workplace plans), so basis is carried over and no tax is
//!    due. Retirement money left in the departing partner's accounts is rolled into
//!    the first person's (the household's share under the order). When the equity
//!    owed is more than the liquid accounts hold, the partner leaves with all of
//!    them and nothing more is owed (no equalisation loan is modelled).
//! 3. The partner leaves: their jobs, self-employment, pensions and other income,
//!    their own budget lines and their health costs are no longer the household's.
//!    They are not dead: they still die on their own drawn date, which is when a
//!    divorced spouse's survivor benefit can start.
//! 4. Shared budget lines are multiplied by `spending_factor`. The event is the
//!    separation: the couple files one last joint return for that calendar year
//!    (a divorce is seldom final in the year a couple separates, and a couple
//!    still married on 31 December may file jointly), which keeps the partner's
//!    wages of the months before, and the tax withheld on them, on the same
//!    return. The partner's income after the separation is not on it (a small
//!    understatement of that year's tax). From the next year the household files
//!    single, or head of household while it has dependent children.
//! 5. Legal costs are paid; support is paid or received monthly for its term.
//! 6. Social Security: after a marriage of ten years or more the person who stays
//!    keeps the spousal benefit on the former partner's record and, once the former
//!    partner dies, the survivor benefit (42 U.S.C. 402(b), (e); SSA "If You Are
//!    Divorced"). After a shorter marriage both are lost. The former partner's
//!    record is frozen at the divorce date.
//!
//! The departing partner's share leaves the household's books as a change in who
//! the household is, like the accounts a joining partner brings: it is not
//! spending, not income and not a withdrawal, and it is reported as the amount of
//! the trace event. The path ends when the person who stays dies.

use crate::math;
use crate::model::{
    Category, DivorceTerms, PropertyUse, SupportDirection, YearMonth, DISSOLUTION_BY_DURATION,
    LATE_DIVORCE_RATE_65_PLUS, LATE_DIVORCE_RATE_UNDER_65,
};
use crate::stats::{TraceEventKind, YearSeries};
use crate::{model::Profile, MAX_PEOPLE, N_ASSETS};

use super::super::plan::{Plan, LAST_RESORT_RENT_YIELD, LAST_RESORT_SELLING_COST};
use super::super::state::State;

/// Months of marriage after which a divorced spouse keeps spousal and survivor
/// benefits on the former partner's record (ten years, 42 U.S.C. 416(d)).
pub const SOCIAL_SECURITY_MARRIAGE_MONTHS: i32 = 120;

/// Annual divorce hazard for a marriage of `years` when the first person is `age`.
pub(crate) fn annual_divorce_hazard(years: f64, age: f64) -> f64 {
    let mut below = 0.0;
    let mut from = 0.0;
    for (until, cumulative) in DISSOLUTION_BY_DURATION {
        if years < until {
            // Constant hazard within each band, reproducing the cumulative figures.
            let survive = (1.0 - cumulative) / (1.0 - below);
            return 1.0 - math::pow(survive, 1.0 / (until - from));
        }
        below = cumulative;
        from = until;
    }
    if age < 65.0 {
        LATE_DIVORCE_RATE_UNDER_65
    } else {
        LATE_DIVORCE_RATE_65_PLUS
    }
}

/// The divorce hazard of one profile: the terms that apply and the probability
/// that the marriage is intact at the start of each month.
#[derive(Clone, Debug)]
pub(crate) struct MarriageHazard {
    pub terms: DivorceTerms,
    /// `survival[t]`: probability that the couple is still married at the start of
    /// month `t`, given that it is at the start (`survival[0] = 1`).
    pub survival: Vec<f64>,
}

impl MarriageHazard {
    /// The hazard of a two-person profile with `divorce_risk: typical`, or `None`.
    pub(crate) fn compile(
        profile: &Profile,
        start: YearMonth,
        married_month: i32,
        joins: i32,
        horizon_months: usize,
    ) -> Option<MarriageHazard> {
        let crate::model::DivorceRisk::Typical(terms) = profile.household.divorce_risk else {
            return None;
        };
        let age0 = profile.household.people[0].birth.age_months_at(start);
        let from = married_month.max(joins).max(0);
        let mut survival = Vec::with_capacity(horizon_months + 1);
        survival.push(1.0);
        let mut s = 1.0;
        let mut cached = (-1, -1.0);
        let mut monthly = 0.0;
        for t in 0..horizon_months as i32 {
            if t >= from {
                let years = f64::from((t - married_month).max(0) / 12);
                let age = f64::from(age0 + t) / 12.0;
                let key = (years as i32, if age < 65.0 { 0.0 } else { 1.0 });
                if key != cached {
                    cached = key;
                    let h = annual_divorce_hazard(years, age);
                    monthly = -math::exp_m1(math::ln_1p(-h) / 12.0);
                }
                s *= 1.0 - monthly;
            }
            survival.push(s);
        }
        Some(MarriageHazard { terms, survival })
    }

    /// The month in which a marriage ends for the uniform draw `u` (`i32::MAX` for
    /// none within the horizon): the first month whose end finds the marriage
    /// over, `P(divorce in month t) = survival[t] - survival[t + 1]`.
    pub(crate) fn draw_month(&self, u: f64) -> i32 {
        let k = self.survival.partition_point(|&s| s > u);
        if k == 0 || k >= self.survival.len() {
            i32::MAX
        } else {
            (k - 1) as i32
        }
    }
}

impl State {
    /// The couple divorces at the start of month `t`. Returns true if it did (both
    /// people were in the household).
    pub(crate) fn divorce(
        &mut self,
        plan: &Plan,
        terms: &DivorceTerms,
        name: &str,
        price: f64,
        t: usize,
    ) -> bool {
        if self.n < 2 || !self.present[0] || !self.present[1] || self.dynamics.divorced {
            return false;
        }
        let now = plan.start + t as i32;
        let ti = t as i32;
        let factor = terms.spending_factor.clamp(0.0, 2.0);

        // 1. The home is sold unless the person who stays keeps it.
        if !terms.keeps_home {
            for slot in 0..self.props.len() {
                let st = self.props[slot];
                if !st.owned || !matches!(st.usage, PropertyUse::Primary) {
                    continue;
                }
                // Rent lines grow from the plan's start like every budget line and
                // are rescaled with the shared lines: set their level so that what
                // is paid after the divorce is the rent stated (or 4% of the price).
                let paid = match terms.then_rent_monthly {
                    Some(r) => r.max(0.0),
                    None => {
                        let years = (t / 12) as i32;
                        LAST_RESORT_RENT_YIELD * st.value
                            / price
                            / 12.0
                            / math::powi(1.0 + Category::Rent.default_real_growth(), years)
                    }
                };
                let rent = if factor > 0.0 { paid / factor } else { 0.0 };
                let net = self.sell_slot(plan, t, slot, LAST_RESORT_SELLING_COST, rent);
                self.event(now, TraceEventKind::SoldHome, Some(net / price), || {
                    format!("{name}: sold {}", plan.props[slot].name)
                });
            }
        }

        // 2. The departing partner's share of the net worth.
        let kinds = self.acc.by_kind();
        let invested: f64 = kinds[1] + kinds[2] + kinds[3] + kinds[4];
        let cash = self.acc.cash.max(0.0);
        let (value, mortgage) = self.property_totals();
        let net = self.acc.cash + invested + value - mortgage - self.debt_total();
        let take = (terms.asset_split.clamp(0.0, 1.0) * net).max(0.0);
        let pool = cash + invested;
        let f = if pool > 0.0 {
            (take / pool).min(1.0)
        } else {
            0.0
        };
        let cash_out = cash * f;
        let invested_out = self.acc.split_off(f);
        self.acc.cash -= cash_out;
        // The share leaves the books as a change of household, not as a flow: the
        // month's cash and bucket identities start from the balances without it.
        self.cash_start -= cash_out;
        if self.check {
            self.chk_other_start -= invested_out;
        }
        self.merge_person_keeping_inherited(1, 0);

        // 3. The partner leaves the household.
        self.present[1] = false;
        self.in_facility[1] = false;
        self.dynamics.left[1] = true;
        self.dynamics.divorced = true;
        for (rt, sp) in self.streams.iter_mut().zip(&plan.streams) {
            if sp.owner == 1 {
                rt.ended = true;
                rt.last = 0.0;
            }
        }
        let married = ti - plan.dynamics.married_month;
        if married < SOCIAL_SECURITY_MARRIAGE_MONTHS {
            self.claimants[1] = None;
        } else if let Some(c) = &mut self.claimants[1] {
            c.annual_earnings = 0.0;
        }
        // The path ends when the person who stays dies.
        self.months = self.months.min(self.death_month[0] as usize).max(t + 1);

        // 4-5. Spending, legal costs and support. The joint return of the year of
        // separation reuses the rule for the year of a death (`household_for_tax`):
        // only one person is present, the status is joint for this calendar year.
        self.death_year = now.year();
        self.dynamics.shared_factor = factor;
        if terms.legal_costs > 0.0 {
            self.spend(YearSeries::SpendOneOff, terms.legal_costs * price);
        }
        let support = terms.support;
        if support.annual > 0.0 && support.years > 0.0 {
            let monthly = support.annual / 12.0 * price;
            self.dynamics.support_monthly = match support.direction {
                SupportDirection::Pays => monthly,
                SupportDirection::Receives => -monthly,
            };
            self.dynamics.support_until = ti + 1 + (support.years * 12.0).round() as i32;
        }
        self.budget_dirty = true;
        self.ss_dirty = true;
        self.req_dirty = true;
        let left = (cash_out + invested_out) / price;
        let years = f64::from(married.max(0)) / 12.0;
        self.event(now, TraceEventKind::LifeEvent, Some(left), || {
            format!(
                "{name} after {years:.0} years of marriage: partner leaves with ${:.0}k",
                left / 1000.0
            )
        });
        true
    }
}

impl crate::accounts::Accounts {
    /// Removes the fraction `f` of every invested bucket (taxable with its basis,
    /// each person's traditional, Roth with its basis records, and HSA) and
    /// returns the value removed. Cash is left to the caller.
    pub(crate) fn split_off(&mut self, f: f64) -> f64 {
        if f <= 0.0 {
            return 0.0;
        }
        let keep = 1.0 - f.min(1.0);
        let invested = |a: &Self| {
            let k = a.by_kind();
            k[1] + k[2] + k[3] + k[4]
        };
        self.holdings_changed();
        let before = invested(self);
        for a in 0..N_ASSETS {
            self.taxable.value[a] *= keep;
            self.taxable.basis[a] *= keep;
            for p in 0..MAX_PEOPLE {
                let pa = &mut self.person[p];
                pa.traditional[a] *= keep;
                pa.roth.value[a] *= keep;
                pa.hsa[a] *= keep;
            }
        }
        for p in 0..MAX_PEOPLE {
            let roth = &mut self.person[p].roth;
            roth.contribution_basis *= keep;
            roth.seasoned_conversions *= keep;
            for u in roth.unseasoned.iter_mut() {
                *u *= keep;
            }
        }
        before - invested(self)
    }

    /// Moves person `from`'s retirement and health accounts into person `to`'s,
    /// keeping their tax character and basis records (a rollover, not a sale).
    pub(crate) fn merge_person(&mut self, from: usize, to: usize) {
        if from >= MAX_PEOPLE || to >= MAX_PEOPLE || from == to {
            return;
        }
        let d = std::mem::take(&mut self.person[from]);
        let p = &mut self.person[to];
        for i in 0..N_ASSETS {
            p.traditional[i] += d.traditional[i];
            p.roth.value[i] += d.roth.value[i];
            p.hsa[i] += d.hsa[i];
        }
        p.roth.contribution_basis += d.roth.contribution_basis;
        p.roth.seasoned_conversions += d.roth.seasoned_conversions;
        for k in 0..p.roth.unseasoned.len() {
            p.roth.unseasoned[k] += d.roth.unseasoned[k];
        }
        p.roth.opened |= d.roth.opened;
        p.roth.years_open = p.roth.years_open.max(d.roth.years_open);
        self.holdings_changed();
    }
}
