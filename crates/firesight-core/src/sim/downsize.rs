//! Planned downsizing (`retirement.downsize`): once, when its condition is met, the
//! household sells its main home and buys a cheaper one, or rents.
//!
//! ```text
//! sale          as any sale: selling costs, mortgage and home-equity lines repaid,
//!               the gain above the section 121 exclusion taxed
//! new home      price   = to_value_fraction x the old home's market value
//!               closing = buying_cost_fraction x price, paid from the proceeds
//!               down    = min(price, proceeds - closing); the rest is a 30-year
//!                         mortgage at the month's market rate
//!               property tax, insurance and maintenance follow the new value
//! renting       (rent_instead, or a down payment below 20% of the price)
//!               rent = 4% x price a year, growing like other rents
//! ```
//!
//! Triggers, checked every month in step 7 after the properties have stepped:
//!
//! * `at_age`: the first month the person is alive and at least that old;
//! * `when_children_leave`: the month the youngest child turns 18, or 22 if a
//!   college is planned for that child (never, without children);
//! * `when_widowed`: twelve months after the first death in a couple;
//! * `when_funded_ratio_below`: in a January of retirement, when investable savings
//!   less debts fall below `ratio` times the funded test's requirement (the same
//!   requirement the retirement trigger uses, recomputed then).
//!
//! The move draws nothing at random (the sale price uses the slot's transaction
//! shock of the month, drawn whether or not it sells), so paths stay paired.
//!
//! Simplifications: the new home sits in the old one's property slot, so it keeps
//! its tax, insurance and maintenance rates, its HOA fee and its appreciation
//! relative to the national index; budget lines (utilities, furnishing) are not
//! rescaled; there is no separate moving cost beyond `buying_cost_fraction`; the
//! lender asks for nothing but the down payment (no income test).

use crate::model::{Category, College, Downsize, DownsizeWhen, PropertyUse, WealthBasis};
use crate::stats::{TraceEventKind, YearSeries};

use super::plan::{Plan, LAST_RESORT_RENT_YIELD};
use super::state::State;

/// A compiled downsizing plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DownsizePlan {
    pub trigger: Trigger,
    pub fraction: f64,
    pub rent_instead: bool,
    pub selling_cost: f64,
    pub buying_cost: f64,
}

/// When a downsizing fires.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Trigger {
    /// Person `person` at least `age` years old.
    AtAge { person: usize, age: f64 },
    /// From this month index (the youngest child leaves home).
    FromMonth(i32),
    /// Never (children were asked for but there are none).
    Never,
    /// Twelve months after the first death in a couple.
    Widowed,
    /// In a January of retirement, wealth below `ratio` x the requirement.
    FundedBelow(f64),
}

/// Months a survivor stays before a `when_widowed` move (a judgement,
/// `data/actuarial/params.json`, `housing_decisions.downsize`).
pub const WIDOWED_DELAY_MONTHS: u32 = 12;
/// Age at which a child leaves home without college, and with it (the schema's
/// definition; a convention, not a statistic).
pub const LEAVE_AGE: i32 = 18;
pub const LEAVE_AGE_COLLEGE: i32 = 22;
/// Smallest down payment, as a share of the new home's price, with which the
/// household buys rather than rents. A judgement: 20% is the conventional
/// threshold below which lenders require mortgage insurance (Fannie Mae's 80%
/// loan-to-value line), and a household moving down is taken to buy within it.
pub const DOWNSIZE_MIN_DOWN: f64 = 0.20;
/// Term of the mortgage on the new home, months (a 30-year fixed loan, the usual
/// US mortgage).
pub const DOWNSIZE_LOAN_MONTHS: u32 = 360;

impl DownsizePlan {
    pub(crate) fn compile(profile: &crate::model::Profile, d: &Downsize) -> DownsizePlan {
        // Compiled only from `Plan::compile`, which requires a start month.
        let start = profile.start.unwrap_or_default();
        let trigger = match d.when {
            DownsizeWhen::AtAge { person, age } => Trigger::AtAge { person, age },
            DownsizeWhen::WhenChildrenLeave => profile
                .household
                .children
                .iter()
                .map(|c| {
                    let leave = if matches!(c.college, College::None) {
                        LEAVE_AGE
                    } else {
                        LEAVE_AGE_COLLEGE
                    };
                    (c.birth - start) + 12 * leave
                })
                .max()
                .map_or(Trigger::Never, Trigger::FromMonth),
            DownsizeWhen::WhenWidowed => Trigger::Widowed,
            DownsizeWhen::WhenFundedRatioBelow { ratio } => Trigger::FundedBelow(ratio),
        };
        DownsizePlan {
            trigger,
            fraction: d.to_value_fraction.max(0.0),
            rent_instead: d.rent_instead,
            selling_cost: d.selling_cost_fraction.clamp(0.0, 1.0),
            buying_cost: d.buying_cost_fraction.max(0.0),
        }
    }
}

impl State {
    /// Step 7 hook: moves if the plan's condition is met this month and the
    /// household still owns its main home. Runs at most once per path.
    pub(crate) fn hx_downsize(
        &mut self,
        plan: &Plan,
        d: &DownsizePlan,
        t: usize,
        price: f64,
        mortgage_rate: f64,
    ) {
        if self.hx.downsized {
            return;
        }
        let due = match d.trigger {
            Trigger::AtAge { person, age } => {
                person < self.n && self.alive[person] && self.age(plan, person, t) >= age
            }
            Trigger::FromMonth(m) => t as i32 >= m,
            Trigger::Never => false,
            Trigger::Widowed => {
                self.survivor && {
                    let mut first = u32::MAX;
                    for i in 0..self.n {
                        if !self.alive[i] {
                            first = first.min(self.death_month[i]);
                        }
                    }
                    first != u32::MAX && t as u32 >= first.saturating_add(WIDOWED_DELAY_MONTHS)
                }
            }
            Trigger::FundedBelow(ratio) => {
                self.retired && (plan.start + t as i32).month() == 1 && {
                    let rule = self.funding_rule(plan);
                    let req = self.requirement(plan, t, price, &rule);
                    let wealth = self.wealth(WealthBasis::Investable, price);
                    wealth < ratio * req.total
                }
            }
        };
        if !due {
            return;
        }
        let Some(slot) = (0..self.props.len())
            .find(|&s| self.props[s].owned && self.props[s].usage == PropertyUse::Primary)
        else {
            // Nothing to sell (already sold, or never owned): the plan lapses.
            self.hx.downsized = true;
            return;
        };
        self.hx.downsized = true;
        self.downsize_slot(plan, d, t, price, slot, mortgage_rate);
    }

    /// Sells the home in `slot` and buys (or rents) the smaller one.
    fn downsize_slot(
        &mut self,
        plan: &Plan,
        d: &DownsizePlan,
        t: usize,
        price: f64,
        slot: usize,
        mortgage_rate: f64,
    ) {
        let now = plan.start + t as i32;
        let new_price = d.fraction * self.props[slot].value;
        let closing = new_price * d.buying_cost;
        let buy = !d.rent_instead && new_price > 0.0;
        // Rent in today's dollars at the level the rent line has grown to, so that
        // it is 4% of the new home's price a year now (as for a last-resort sale).
        let years = (t / 12) as i32;
        let rent_today = LAST_RESORT_RENT_YIELD * new_price
            / price
            / 12.0
            / crate::math::powi(1.0 + Category::Rent.default_real_growth(), years);
        let rent = if buy { 0.0 } else { rent_today };
        let net = self.sell_slot(plan, t, slot, d.selling_cost, rent);
        // The proceeds pay the closing costs and as much of the price as they can;
        // a mortgage at the market rate finances the rest, if the proceeds make at
        // least the minimum down payment.
        let down = (net - closing).min(new_price);
        if buy && down >= DOWNSIZE_MIN_DOWN * new_price {
            let loan = new_price - down;
            let quote = plan.props[slot].model.buy_at(
                &mut self.props[slot],
                t,
                new_price,
                closing,
                loan,
                mortgage_rate,
                DOWNSIZE_LOAN_MONTHS,
            );
            self.pay(YearSeries::PropertyPurchase, quote.down_payment);
            self.spend(YearSeries::SpendOneOff, quote.closing_costs);
            if self.check {
                self.chk_prop_flow += quote.price;
                self.chk_mort_flow += quote.loan;
            }
            self.event(
                now,
                TraceEventKind::BoughtHome,
                Some(new_price / price),
                || {
                    if loan > 0.0 {
                        format!(
                            "Downsized: sold {} and bought a smaller home with a {:.2}% mortgage",
                            plan.props[slot].name,
                            mortgage_rate * 100.0
                        )
                    } else {
                        format!(
                            "Downsized: sold {} and bought a smaller home",
                            plan.props[slot].name
                        )
                    }
                },
            );
        } else {
            if buy {
                // The proceeds do not make the down payment: rent such a home.
                self.extra_rent += rent_today;
            }
            self.event(now, TraceEventKind::SoldHome, Some(net / price), || {
                format!("Downsized: sold {} and rented", plan.props[slot].name)
            });
        }
        self.budget_dirty = true;
        self.req_dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Child, Profile};

    #[test]
    fn children_leave_at_18_or_22_with_college() {
        let mut p = Profile::example();
        let start = p.start.unwrap_or(crate::model::EXAMPLE_START);
        p.household.children = vec![
            Child {
                id: "a".into(),
                birth: start - 10 * 12,
                college: College::None,
                ..Child::default()
            },
            Child {
                id: "b".into(),
                birth: start - 5 * 12,
                college: College::PublicInState,
                ..Child::default()
            },
        ];
        let d = DownsizePlan::compile(&p, &Downsize::default());
        // The first leaves at 18, in 8 years; the second at 22, in 17: the later.
        assert_eq!(d.trigger, Trigger::FromMonth(17 * 12));
        p.household.children.clear();
        assert_eq!(
            DownsizePlan::compile(&p, &Downsize::default()).trigger,
            Trigger::Never
        );
    }
}
