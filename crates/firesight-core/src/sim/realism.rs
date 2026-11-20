//! Per-path state and hooks of the tax and policy realism options: policy risk
//! (`assumptions.taxes.policy_risk`), tax-loss harvesting
//! (`portfolio.tax_loss_harvesting`) and qualified charitable distributions
//! (`retirement.qcd`). The month loop and the annual plan call the small `State`
//! methods below; each does nothing (one predictable branch) when its option is off.

use crate::accounts::TaxableBucket;
use crate::accounts::{Bucket, WithdrawContext};
use crate::benefits::MonthlyBenefits;
use crate::math;
use crate::model::{AssetClass, Category, YearMonth, N_ASSETS};
use crate::policy::PolicyDraw;
use crate::stats::{TraceEventKind, YearSeries};
use crate::tax::index_rounded;
use crate::MAX_PEOPLE;

use super::plan::Plan;
use super::state::State;

/// A holding is harvested when it stands this fraction below its cost basis: a
/// judgement (automated services act on losses of a few percent). Registered in
/// `data/actuarial/params.json` under `unverified_from_memory.tax_loss_harvesting`.
pub const TLH_LOSS_THRESHOLD: f64 = 0.05;

/// ... and the loss is at least this many dollars of today's money, so the household
/// does not trade for trivial amounts: a judgement, registered with the threshold.
pub const TLH_MIN_LOSS_REAL: f64 = 1_000.0;

/// The qualified charitable distribution limit per person for 2026: $111,000, up
/// from $108,000 in 2025 (IRS Notice 2025-67; IRC 408(d)(8)(A) as indexed by the
/// SECURE 2.0 Act section 307). It is indexed with prices from then on, rounded to
/// $1,000 (the rounding is an assumption: the statute indexes it "in the same manner"
/// as other limits).
pub const QCD_LIMIT_2026: f64 = 111_000.0;
const QCD_ROUNDING: f64 = 1_000.0;

/// Age in months from which a QCD is allowed: 70 and a half (IRC 408(d)(8)(A)).
pub const QCD_MIN_AGE_MONTHS: i32 = 70 * 12 + 6;

/// What the realism options keep for one path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RealismState {
    /// This path's policy outcome.
    pub policy: PolicyDraw,
    /// Qualified charitable distributions made so far this calendar year, nominal.
    pub qcd_ytd: [f64; MAX_PEOPLE],
    /// QCDs the annual plan expects for the rest of this year, nominal.
    pub qcd_expected: f64,
}

impl Default for RealismState {
    fn default() -> Self {
        Self {
            policy: PolicyDraw::NONE,
            qcd_ytd: [0.0; MAX_PEOPLE],
            qcd_expected: 0.0,
        }
    }
}

impl State {
    /// Resets the realism state for a new path and draws its policy outcome.
    pub(crate) fn realism_begin(&mut self, plan: &Plan, seed: u64, path: u64) {
        self.rz = RealismState::default();
        if let Some(risk) = plan.profile.assumptions.taxes.policy_risk {
            self.rz.policy = PolicyDraw::draw(&risk, seed, path);
        }
    }

    // ------------------------------------------------------------ policy risk

    /// The month's Social Security benefits after this path's policy cut.
    #[inline]
    pub(crate) fn policy_benefits(&self, now: YearMonth, b: MonthlyBenefits) -> MonthlyBenefits {
        self.rz.policy.scale_benefits(now.year(), b)
    }

    /// Adds this path's tax-rate increase to the freshly indexed law of the year, for
    /// the filing statuses in the bit mask.
    #[inline]
    pub(crate) fn policy_rates(&mut self, statuses: u8) {
        self.rz
            .policy
            .raise_rates(self.index.year, &mut self.params, statuses);
    }

    /// Start of a calendar year: logs a policy change taking effect and restarts the
    /// year's QCD tally.
    pub(crate) fn realism_year_start(&mut self, now: YearMonth) {
        self.rz.qcd_ytd = [0.0; MAX_PEOPLE];
        let p = self.rz.policy;
        if !p.any() {
            return;
        }
        let year = now.year();
        if year == p.ss_from_year {
            self.event(now, TraceEventKind::PolicyChange, Some(p.ss_factor), || {
                format!(
                    "Policy risk: Social Security benefits cut to {:.0}% of scheduled",
                    p.ss_factor * 100.0
                )
            });
        }
        if year == p.tax_from_year {
            self.event(
                now,
                TraceEventKind::PolicyChange,
                Some(p.tax_points),
                || {
                    format!(
                        "Policy risk: ordinary income tax rates rise {:.1} points",
                        p.tax_points * 100.0
                    )
                },
            );
        }
    }

    // --------------------------------------------------- tax-loss harvesting

    /// Realises the losses of taxable holdings that stand `TLH_LOSS_THRESHOLD` or
    /// more below their basis and stays invested in the same asset classes (a
    /// similar fund is bought, so the wash-sale rule is avoided and holdings are
    /// unchanged). Each harvested holding's basis is reset to its value; the loss
    /// goes into the year's long-term gains, where the return nets it against gains
    /// and takes up to $3,000 of the rest against other income, carrying the
    /// remainder forward. Called once a month after returns.
    pub(crate) fn harvest_losses(&mut self, now: YearMonth, price: f64) {
        let skip = if self.acc.keep_single_stock {
            Some(AssetClass::SingleStock.index())
        } else {
            None
        };
        let harvested =
            harvest_taxable_losses(&mut self.acc.taxable, TLH_MIN_LOSS_REAL * price, skip);
        if harvested > 0.0 {
            self.ty.long_term_gains -= harvested;
            self.event(
                now,
                TraceEventKind::TaxLossHarvest,
                Some(harvested / price),
                || "Harvested a capital loss and stayed invested (basis reset)".to_string(),
            );
        }
    }

    // ------------------------------------- qualified charitable distributions

    /// This month's giving in nominal dollars: the budget lines of category
    /// `Giving` as the budget would charge them (same growth, retirement phase,
    /// survivor factor and spending cuts as [`State::budget_as`]).
    fn giving_now(&self, plan: &Plan, t: usize, price: f64) -> f64 {
        let ti = t as i32;
        let years = (t / 12) as i32;
        let survivor_factor = if self.survivor {
            plan.profile.spending.survivor_factor
        } else {
            1.0
        };
        let mut total = 0.0;
        for (i, item) in plan.items.iter().enumerate() {
            if item.category != Category::Giving {
                continue;
            }
            let start = item.start.month(self.retire_month);
            let end = item.end.month(self.retire_month);
            if ti < start || ti >= end {
                continue;
            }
            if let Some(owner) = item.owner {
                if owner >= MAX_PEOPLE || !self.present[owner] {
                    continue;
                }
            }
            let mut amount = if self.retired {
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
            if self.retired {
                amount *= self.smile;
            }
            if !item.essential {
                amount *= self.creep * self.disc_mult;
            }
            total += amount;
        }
        total * price
    }

    /// The annual QCD limit per person this year, nominal.
    fn qcd_limit(&self) -> f64 {
        index_rounded(QCD_LIMIT_2026, self.params.cpi_factor, QCD_ROUNDING)
    }

    /// Whether person `i` is old enough for a QCD in month `t`.
    fn qcd_eligible(&self, plan: &Plan, i: usize, t: usize) -> bool {
        self.present[i] && self.age_months(plan, i, t) >= QCD_MIN_AGE_MONTHS
    }

    /// Annual plan: what the QCDs of the rest of the year are expected to be, so the
    /// required distribution (`rmd`, for the months to come) and the cash the plan has
    /// to raise both fall by that much. Months before 70 and a half count nothing.
    pub(crate) fn qcd_plan(
        &mut self,
        plan: &Plan,
        t: usize,
        price: f64,
        m_left: f64,
        rmd: &mut [f64; MAX_PEOPLE],
    ) {
        self.rz.qcd_expected = 0.0;
        let giving = self.giving_now(plan, t, price);
        if giving <= 0.0 {
            return;
        }
        let limit = self.qcd_limit();
        let mut pool = giving * m_left;
        for i in 0..self.n {
            if !self.present[i] {
                continue;
            }
            let wait = (QCD_MIN_AGE_MONTHS - self.age_months(plan, i, t)).max(0);
            let months = (m_left - f64::from(wait)).max(0.0);
            if months <= 0.0 {
                continue;
            }
            let room = (limit - self.rz.qcd_ytd[i]).max(0.0);
            let balance = self.acc.total(Bucket::Traditional(i));
            let x = (giving * months).min(pool).min(room).min(balance);
            if x > 0.0 {
                // The gift counts toward the required distribution, up to its size.
                rmd[i] = (rmd[i] - x).max(0.0);
                pool -= x;
                self.rz.qcd_expected += x;
            }
        }
    }

    /// Month: pays this month's giving from the IRAs of people 70 and a half or
    /// older, up to each person's annual limit. The money moves from the
    /// traditional account to cash (and out again as the gift) without passing
    /// through taxable income, so it is not in the return's distributions.
    pub(crate) fn qcd_month(&mut self, plan: &Plan, t: usize, price: f64, now: YearMonth) {
        let mut pool = self.giving_now(plan, t, price);
        if pool <= 0.0 {
            return;
        }
        let limit = self.qcd_limit();
        for i in 0..self.n {
            if pool <= 0.0 {
                break;
            }
            if !self.qcd_eligible(plan, i, t) {
                continue;
            }
            let room = (limit - self.rz.qcd_ytd[i]).max(0.0);
            let want = pool.min(room);
            if want <= 0.0 {
                continue;
            }
            let ctx = WithdrawContext {
                owner_age: self.age(plan, i, t),
                ..WithdrawContext::default()
            };
            let got = self.acc.withdraw(Bucket::Traditional(i), want, &ctx).amount;
            if got <= 0.0 {
                continue;
            }
            self.acc.cash += got;
            self.led[YearSeries::WithdrawTraditional.index()] += got;
            pool -= got;
            let first = self.rz.qcd_ytd[i] == 0.0;
            self.rz.qcd_ytd[i] += got;
            if first {
                self.event(
                    now,
                    TraceEventKind::QualifiedCharitableDistribution,
                    Some(got / price),
                    || {
                        format!(
                            "Person {} gives from the IRA as a qualified charitable distribution",
                            i + 1
                        )
                    },
                );
            }
        }
    }
}

/// Resets the basis of every holding that stands at least [`TLH_LOSS_THRESHOLD`]
/// below it (and by at least `min_loss` dollars) to its value, except asset class
/// `skip`; returns the loss realised. Holdings are unchanged.
pub(crate) fn harvest_taxable_losses(
    taxable: &mut TaxableBucket,
    min_loss: f64,
    skip: Option<usize>,
) -> f64 {
    let mut harvested = 0.0;
    for i in 0..N_ASSETS {
        if Some(i) == skip {
            continue;
        }
        let loss = taxable.basis[i] - taxable.value[i];
        if loss >= min_loss && loss >= TLH_LOSS_THRESHOLD * taxable.basis[i] {
            taxable.basis[i] = taxable.value[i];
            harvested += loss;
        }
    }
    harvested
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(values: &[(AssetClass, f64, f64)]) -> TaxableBucket {
        let mut b = TaxableBucket::default();
        for &(class, value, basis) in values {
            b.value[class.index()] = value;
            b.basis[class.index()] = basis;
        }
        b
    }

    #[test]
    fn harvests_only_holdings_below_the_threshold_and_resets_their_basis() {
        // Stocks 80,000 on a basis of 100,000: a loss of 20,000 (20%). Bonds 50,000 on
        // 51,000: 1,000 is 1.96% of basis, under 5%. Intl 94,000 on 100,000: 6%, and
        // 6,000 is above the 1,000 minimum, so it is harvested too.
        let mut b = bucket(&[
            (AssetClass::UsStocks, 80_000.0, 100_000.0),
            (AssetClass::Bonds, 50_000.0, 51_000.0),
            (AssetClass::IntlStocks, 94_000.0, 100_000.0),
        ]);
        let value_before = b.value;
        let loss = harvest_taxable_losses(&mut b, 1_000.0, None);
        assert_eq!(loss, 26_000.0);
        assert_eq!(b.value, value_before, "holdings do not change");
        assert_eq!(b.basis[AssetClass::UsStocks.index()], 80_000.0);
        assert_eq!(b.basis[AssetClass::IntlStocks.index()], 94_000.0);
        assert_eq!(b.basis[AssetClass::Bonds.index()], 51_000.0);
        // Nothing left to harvest.
        assert_eq!(harvest_taxable_losses(&mut b, 1_000.0, None), 0.0);
    }

    #[test]
    fn gains_small_losses_and_skipped_classes_are_left_alone() {
        let mut b = bucket(&[
            (AssetClass::UsStocks, 120_000.0, 100_000.0),
            (AssetClass::Gold, 9_000.0, 10_000.0),
            (AssetClass::SingleStock, 50_000.0, 100_000.0),
        ]);
        // Gold's loss is 10% but only 1,000: below a 1,500 minimum. The single stock
        // is skipped (kept employer stock).
        let loss = harvest_taxable_losses(&mut b, 1_500.0, Some(AssetClass::SingleStock.index()));
        assert_eq!(loss, 0.0);
        assert_eq!(b.basis[AssetClass::UsStocks.index()], 100_000.0);
        assert_eq!(b.basis[AssetClass::SingleStock.index()], 100_000.0);
        let loss = harvest_taxable_losses(&mut b, 500.0, None);
        assert_eq!(loss, 1_000.0 + 50_000.0);
    }

    #[test]
    fn qcd_limit_indexes_with_prices_and_rounds_to_a_thousand() {
        // 2026 is the base year: unchanged. A 10% price rise gives 122,100, which
        // rounds to 122,000; 4% gives 115,440, which rounds to 115,000.
        assert_eq!(index_rounded(QCD_LIMIT_2026, 1.0, QCD_ROUNDING), 111_000.0);
        assert_eq!(index_rounded(QCD_LIMIT_2026, 1.10, QCD_ROUNDING), 122_000.0);
        assert_eq!(index_rounded(QCD_LIMIT_2026, 1.04, QCD_ROUNDING), 115_000.0);
        // 70 and a half is 846 months.
        assert_eq!(QCD_MIN_AGE_MONTHS, 846);
    }

    #[test]
    fn judgement_constants_are_registered_as_unverified() {
        let section = &crate::data::Data::embedded().actuarial["unverified_from_memory"]
            ["tax_loss_harvesting"];
        assert_eq!(
            TLH_LOSS_THRESHOLD,
            section["loss_threshold"].as_f64().unwrap()
        );
        assert_eq!(
            TLH_MIN_LOSS_REAL,
            section["min_loss_real"].as_f64().unwrap()
        );
    }
}
