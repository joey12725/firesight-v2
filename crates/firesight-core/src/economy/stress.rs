//! Stress overlays: deterministic rewrites of a generated path from a start month on.

use crate::math;
use crate::model::{AssetClass, Episode, N_ASSETS};

use super::{EconomyModel, EconomyPath};

/// Months over which a crash unfolds. The 2020 crash took one month, 1987 two, the
/// 2008 leg from September to the March 2009 low six; six keeps the fall fast enough
/// to be a sequence-risk shock without being a single-month jump.
pub const CRASH_MONTHS: usize = 6;
/// Length of the lost decade.
pub const LOST_DECADE_MONTHS: usize = 120;
/// Deepest drawdown a crash overlay will apply.
const MAX_DRAWDOWN: f64 = 0.99;

/// Asset classes that respond to an imposed equity move through their equity beta.
/// (US equities are rewritten directly; bonds and cash do not respond.)
const BETA_ASSETS: [AssetClass; 6] = [
    AssetClass::IntlStocks,
    AssetClass::RealEstate,
    AssetClass::Gold,
    AssetClass::Crypto,
    AssetClass::SingleStock,
    AssetClass::Custom,
];
/// The same without international equities, for overlays that rewrite those directly.
const DERIVED_BETA_ASSETS: [AssetClass; 5] = [
    AssetClass::RealEstate,
    AssetClass::Gold,
    AssetClass::Crypto,
    AssetClass::SingleStock,
    AssetClass::Custom,
];

/// A stress overlay with its dates resolved to month indices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResolvedStress {
    /// US equities fall by exactly `drawdown` over the [`CRASH_MONTHS`] months from
    /// `at_month` (the sampled returns of those months are replaced by an even
    /// geometric decline). Over the `recovery_months` that follow, each month's
    /// sampled gross return is scaled by one constant so that at the end of the
    /// recovery the index is back at its pre-crash level in nominal terms, and not
    /// before: the shape of the sampled path is kept, its level is not. After that
    /// the sampled returns resume, so the stressed index stays below the unstressed
    /// one for good (by whatever the unstressed path earned over crash and
    /// recovery). Every other risky asset moves by its equity beta times the change
    /// imposed on US equities. The crash months are flagged as a recession.
    Crash {
        at_month: usize,
        drawdown: f64,
        recovery_months: usize,
    },
    /// [`LOST_DECADE_MONTHS`] months from `from_month` over which US and
    /// international stocks each return exactly zero after inflation in total. Each
    /// month's gross return is scaled by one constant, so monthly volatility and the
    /// shape of the path are kept. Other risky assets move by their equity beta.
    LostDecade { from_month: usize },
    /// `months` of inflation averaging `inflation` a year (geometric; the path's own
    /// monthly variation is kept), with the real returns of equities, bonds and bills
    /// over the window set to what US equities, 10-year bonds and bills earned after
    /// inflation in 1973-01..1981-12 in the embedded history (about -2.8%, -5.2% and
    /// -0.9% a year). Home prices and gold keep their real returns; the long yield
    /// and the mortgage rate rise by the change in inflation.
    Stagflation {
        from_month: usize,
        months: usize,
        inflation: f64,
    },
    /// The record's months of `episode` from `from_month` (`economy::episodes`).
    HistoricalEpisode { from_month: usize, episode: Episode },
    /// `months` of Japan-style stagnation from `from_month` (`economy::episodes`).
    LostGeneration { from_month: usize, months: usize },
}

/// `r + beta * delta`, never below a total loss. A return of exactly -1.0 marks a
/// collapse (a failed token or company) and is left alone: an imposed market move
/// does not bring it back.
#[inline]
pub(super) fn shifted(r: f64, beta: f64, delta: f64) -> f64 {
    if r <= -1.0 {
        return -1.0;
    }
    (r + beta * delta).max(-1.0)
}

impl EconomyModel {
    /// Rewrites `out` from the overlay's start month onward; months before it are
    /// never touched. Deterministic: draws no random numbers. Safe to call mid-path
    /// for months not yet simulated, and overlays compose (apply them in order).
    /// An overlay that runs past the end of the path is truncated.
    pub fn apply_stress(&self, stress: &ResolvedStress, out: &mut EconomyPath) {
        match *stress {
            ResolvedStress::Crash {
                at_month,
                drawdown,
                recovery_months,
            } => self.apply_crash(out, at_month, drawdown, recovery_months),
            ResolvedStress::LostDecade { from_month } => self.apply_lost_decade(out, from_month),
            ResolvedStress::Stagflation {
                from_month,
                months,
                inflation,
            } => self.apply_stagflation(out, from_month, months, inflation),
            ResolvedStress::HistoricalEpisode {
                from_month,
                episode,
            } => self.apply_episode(out, from_month, episode),
            ResolvedStress::LostGeneration { from_month, months } => {
                self.apply_lost_generation(out, from_month, months)
            }
        }
        if let Some(v) = &self.valuation {
            if let Some((from, end)) = super::valuation::stress_window(stress) {
                v.after_stress(out, from, end, &self.stress_beta);
            }
        }
        if self.dividends.is_some() {
            self.refresh_dividends(out);
        }
    }

    /// Sets the US equity return of month `t` and moves the beta assets with it.
    #[inline]
    fn set_equity(&self, out: &mut EconomyPath, t: usize, new: f64, assets: &[AssetClass]) {
        let us = AssetClass::UsStocks.index();
        let delta = new - out.returns[us][t];
        out.returns[us][t] = new;
        for a in assets {
            let i = a.index();
            out.returns[i][t] = shifted(out.returns[i][t], self.stress_beta[i], delta);
        }
    }

    fn apply_crash(&self, out: &mut EconomyPath, at: usize, drawdown: f64, recovery_months: usize) {
        if at >= out.months || !drawdown.is_finite() {
            return;
        }
        let us = AssetClass::UsStocks.index();
        let depth = drawdown.clamp(0.0, MAX_DRAWDOWN);
        if depth <= 0.0 {
            return;
        }
        let crash_end = at.saturating_add(CRASH_MONTHS).min(out.months);
        let monthly = math::pow(1.0 - depth, 1.0 / CRASH_MONTHS as f64);
        let mut stressed = 1.0;
        for t in at..crash_end {
            stressed *= monthly;
            self.set_equity(out, t, monthly - 1.0, &BETA_ASSETS);
            out.recession[t] = 1;
        }
        // The recovery: the sampled returns of the window, scaled so that together
        // they just make good the fall.
        let recovery = recovery_months.max(1);
        let recovery_end = crash_end.saturating_add(recovery).min(out.months);
        if recovery_end <= crash_end {
            return;
        }
        let mut sampled = 1.0;
        for r in &out.returns[us][crash_end..recovery_end] {
            sampled *= 1.0 + r;
        }
        // A path that ends inside the recovery gets the same monthly scaling as if
        // the window were complete only when the whole window is there; a truncated
        // window recovers the share of the fall its length is of the whole.
        let share = (recovery_end - crash_end) as f64 / recovery as f64;
        let target = math::pow(1.0 / stressed, share);
        let n = (recovery_end - crash_end) as f64;
        let drift = if sampled > 0.0 {
            math::pow(target / sampled, 1.0 / n)
        } else {
            1.0
        };
        for t in crash_end..recovery_end {
            let new = (1.0 + out.returns[us][t]) * drift - 1.0;
            self.set_equity(out, t, new, &BETA_ASSETS);
        }
    }

    fn apply_lost_decade(&self, out: &mut EconomyPath, from: usize) {
        if from >= out.months {
            return;
        }
        let end = from.saturating_add(LOST_DECADE_MONTHS).min(out.months);
        let n = (end - from) as f64;
        let (us, intl) = (AssetClass::UsStocks.index(), AssetClass::IntlStocks.index());
        let (mut real_us, mut real_intl) = (1.0, 1.0);
        for t in from..end {
            let deflator = 1.0 + out.inflation[t];
            real_us *= (1.0 + out.returns[us][t]) / deflator;
            real_intl *= (1.0 + out.returns[intl][t]) / deflator;
        }
        let k_us = math::pow(real_us, -1.0 / n);
        let k_intl = math::pow(real_intl, -1.0 / n);
        for t in from..end {
            let new = (1.0 + out.returns[us][t]) * k_us - 1.0;
            self.set_equity(out, t, new, &DERIVED_BETA_ASSETS);
            out.returns[intl][t] = (1.0 + out.returns[intl][t]) * k_intl - 1.0;
        }
    }

    fn apply_stagflation(&self, out: &mut EconomyPath, from: usize, months: usize, inflation: f64) {
        if from >= out.months || months == 0 || !inflation.is_finite() {
            return;
        }
        let end = from.saturating_add(months).min(out.months);
        let n = (end - from) as f64;
        const DIRECT: [AssetClass; 4] = [
            AssetClass::UsStocks,
            AssetClass::IntlStocks,
            AssetClass::Bonds,
            AssetClass::Cash,
        ];
        // Real monthly targets: international equities share the US equity target.
        let [eq, bond, cash] = self.base.stagflation_real;
        let targets = [eq, eq, bond, cash];
        let mut price = 1.0;
        let mut real = [1.0; 4];
        for t in from..end {
            let deflator = 1.0 + out.inflation[t];
            price *= deflator;
            for (acc, a) in real.iter_mut().zip(DIRECT) {
                *acc *= (1.0 + out.returns[a.index()][t]) / deflator;
            }
        }
        let target_monthly = 1.0 + math::annual_to_monthly(inflation.max(-0.5));
        let k_inflation = target_monthly / math::pow(price, 1.0 / n);
        let own_annual = math::pow(price, 12.0 / n) - 1.0;
        let rate_shift = inflation - own_annual;
        let mut k = [1.0; 4];
        for i in 0..4 {
            k[i] = k_inflation * targets[i] / math::pow(real[i], 1.0 / n);
        }
        let us = AssetClass::UsStocks.index();
        let cash_index = AssetClass::Cash.index();
        for t in from..end {
            out.inflation[t] = (1.0 + out.inflation[t]) * k_inflation - 1.0;
            out.home_national[t] = (1.0 + out.home_national[t]) * k_inflation - 1.0;
            // Equity move beyond the pure inflation shift, for the beta assets.
            let eq_inflated = (1.0 + out.returns[us][t]) * k_inflation - 1.0;
            for (a, ki) in DIRECT.iter().zip(k) {
                let r = &mut out.returns[a.index()][t];
                *r = (1.0 + *r) * ki - 1.0;
            }
            let delta = out.returns[us][t] - eq_inflated;
            for a in DERIVED_BETA_ASSETS {
                let i = a.index();
                let inflated = (1.0 + out.returns[i][t]) * k_inflation - 1.0;
                out.returns[i][t] = shifted(inflated, self.stress_beta[i], delta);
            }
            out.tbill[t] = math::powi(1.0 + out.returns[cash_index][t], 12) - 1.0;
            out.long_yield[t] += rate_shift;
            out.mortgage_rate[t] += rate_shift;
        }
        for t in from..out.months {
            out.price_level[t + 1] = out.price_level[t] * (1.0 + out.inflation[t]);
        }
    }

    /// The equity beta each asset class is moved by under a stress overlay.
    pub fn stress_betas(&self) -> [f64; N_ASSETS] {
        self.stress_beta
    }
}
