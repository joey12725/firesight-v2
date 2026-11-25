//! The fat-tailed parametric model (`MarketKind::FatTailed`): the lognormal model's
//! means, volatilities and recession chain, with three things the lognormal model
//! lacks.
//!
//! * **Fat tails.** Each month's shocks to stocks (US and international), bonds and
//!   gold share one Student-t mixing variable `w = sqrt((nu - 2) / chi2_nu)` with
//!   [`FAT_TAIL_DOF`] degrees of freedom (a multivariate t: a bad month tends to be
//!   bad for all of them at once). Bills, home prices and inflation keep normal
//!   shocks, as in the lognormal model.
//! * **Volatility clustering.** The variance of both stock markets follows a
//!   GARCH(1,1) recursion on the US shock (Bollerslev 1986; with t innovations,
//!   Bollerslev 1987): `v' = (1 - a - b) + a v eta^2 + b v`, where `eta` is the
//!   month's standardised US shock. `v` is capped at [`FAT_TAIL_VARIANCE_CAP`] and
//!   divided by its own long-run mean, so the unconditional volatility is the
//!   lognormal model's.
//! * **Correlations that change in turbulent markets.** Months whose conditional
//!   variance is in its top quarter use the correlation matrix of the record's
//!   turbulent months (trailing 12-month volatility above its 75th percentile, the
//!   split `data/history/README.md` describes); the others use the calm months'
//!   matrix. In the record, US and international stocks move together more in
//!   turbulent months (0.77 against 0.60) while stocks and 10-year Treasuries move
//!   together less (0.00 against 0.13): the flight to quality. Longin and Solnik
//!   (2001, Journal of Finance 56(2)) find the same rise of stock-stock correlation
//!   in bear markets.
//!
//! The mean of every monthly log return is the lognormal model's exactly (the shocks
//! are symmetric with mean zero), so the two models have the same geometric means.
//!
//! The parameters were fitted to the record by simulated moments: `a`, `b` and `nu`
//! were chosen on a grid so that 100-year simulated samples (1,203 months, the
//! record's length) have, at the median, the record's excess kurtosis of monthly US
//! stock returns (7.4), autocorrelation of absolute returns at lags 1 and 12 (0.22
//! and 0.15) (all from `data/history/calibration.json`). Maximum likelihood on the
//! same record gives a = 0.13, b = 0.83, nu = 7.3, whose simulated samples have a
//! kurtosis of 3.5 and lag-12 volatility autocorrelation of 0.11: too thin-tailed
//! and too short-memoried for a planning model whose point is the tails.
//!
//! Draws: from `Market` exactly the lognormal model's (6 normals and 1 uniform a
//! month, 1 uniform at the start of a path), so the two models see the same normal
//! shocks on the same path; from `Inflation` 1 normal a month; from `MarketTails`
//! 3 a month (two uniforms and one normal for the chi-square).

use std::sync::OnceLock;

use super::fit;
use super::regime::{forward_fill, LognormalParams, LN_DIM};
use super::sampler::{self, ShockTable};
use super::Core;
use crate::data::History;
use crate::math;
use crate::rng::Rng;

/// ARCH coefficient `a` of the variance recursion. Fitted to the record by
/// simulated moments (module documentation).
pub const FAT_TAIL_ARCH: f64 = 0.15;
/// GARCH coefficient `b`. Fitted with `a`; the persistence `a + b = 0.98` sets how
/// slowly a volatile spell fades (half-life about 3 years).
pub const FAT_TAIL_GARCH: f64 = 0.83;
/// Degrees of freedom of the Student-t shocks. Fitted with `a` and `b`.
pub const FAT_TAIL_DOF: f64 = 5.0;
/// Highest raw variance multiplier: the record's highest trailing 12-month variance
/// of US stocks (71% annualised volatility, in the 12 months to April 1933) is about 15 times its
/// full-sample variance (18.3%). Rounded to 16.
pub const FAT_TAIL_VARIANCE_CAP: f64 = 16.0;
/// Cap on the Student-t mixing variable (the regime model's cap): no single month's
/// shock is more than 8 times its normal-model size. Judgement.
pub const FAT_TAIL_MIXING_CAP: f64 = 8.0;

const TURBULENT_QUANTILE: f64 = 0.75;
const THRESHOLD_WINDOW: usize = 12;
/// Length and seed of the construction-time run of the variance chain that gives
/// its long-run mean and its 75th percentile.
const CHAIN_MONTHS: usize = 1 << 21;
const CHAIN_SEED: u64 = 0x1800_FA77;

/// Rows of the shock vector, as in [`LognormalParams`].
const EQ: usize = 0;

/// The long-run statistics of the raw variance chain under the constants above.
#[derive(Clone, Copy, Debug)]
struct ChainStats {
    mean: f64,
    turbulent: f64,
}

/// The Student-t mixing variable: 2 uniforms and 1 normal from `rng`.
#[inline(always)]
fn mixing(rng: &mut Rng, normal: &ShockTable) -> f64 {
    let u = rng.uniform_open0() * rng.uniform_open0();
    let n = normal.sample(rng.next_u64());
    // chi2 with 5 degrees of freedom: chi2(4) = -2 ln(u1 u2), plus one squared normal.
    let chi2 = -2.0 * math::ln(u) + n * n;
    ((FAT_TAIL_DOF - 2.0) / chi2)
        .sqrt()
        .min(FAT_TAIL_MIXING_CAP)
}

/// One step of the raw variance recursion.
#[inline(always)]
fn next_variance(v: f64, eta: f64) -> f64 {
    ((1.0 - FAT_TAIL_ARCH - FAT_TAIL_GARCH) + FAT_TAIL_ARCH * v * eta * eta + FAT_TAIL_GARCH * v)
        .min(FAT_TAIL_VARIANCE_CAP)
}

fn chain_stats() -> ChainStats {
    static S: OnceLock<ChainStats> = OnceLock::new();
    *S.get_or_init(|| {
        let normal = &sampler::normal().table;
        let mut rng = Rng::from_seed(CHAIN_SEED);
        let mut v = 1.0;
        let mut values = Vec::with_capacity(CHAIN_MONTHS);
        for _ in 0..CHAIN_MONTHS {
            values.push(v);
            let w = mixing(&mut rng, normal);
            let z = normal.sample(rng.next_u64());
            v = next_variance(v, w * z);
        }
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        values.sort_by(f64::total_cmp);
        let turbulent = values[(TURBULENT_QUANTILE * (values.len() - 1) as f64) as usize];
        ChainStats { mean, turbulent }
    })
}

/// Per-path state.
#[derive(Clone, Copy, Debug)]
pub(super) struct FatState {
    /// Raw variance multiplier.
    v: f64,
    recession: u8,
}

#[derive(Clone, Debug)]
pub(super) struct FatTailedParams {
    pub(super) ln: LognormalParams,
    /// Calm and turbulent Cholesky factors, rows scaled by the full-sample
    /// standard deviations.
    chol: [[[f64; LN_DIM]; LN_DIM]; 2],
    /// `1 / E[v]`: the variance multiplier applied is `v * var_scale`.
    var_scale: f64,
    /// Raw variance above which a month uses the turbulent correlations.
    v_turbulent: f64,
}

impl FatTailedParams {
    pub(super) fn calibrate(ln: LognormalParams, h: &History) -> FatTailedParams {
        let home = forward_fill(&h.home);
        let log = |v: &[f64]| -> Vec<f64> { v.iter().map(|&r| math::ln_1p(r)).collect() };
        let series: [Vec<f64>; LN_DIM] = [
            log(&h.equity_us),
            log(&h.equity_intl),
            log(&h.bond10),
            log(&h.tbill),
            log(&home),
            log(&h.gold),
            log(&h.cpi),
        ];
        let turbulent =
            fit::threshold_classification(&h.equity_us, THRESHOLD_WINDOW, TURBULENT_QUANTILE);
        // Standard deviations from the lognormal factor: the norm of each row.
        let mut sd = [0.0; LN_DIM];
        for i in 0..LN_DIM {
            sd[i] = ln.chol[i].iter().map(|x| x * x).sum::<f64>().sqrt();
        }
        let mut chol = [[[0.0; LN_DIM]; LN_DIM]; 2];
        for (k, out) in chol.iter_mut().enumerate() {
            let state = k == 1;
            let mut c = [[0.0; LN_DIM]; LN_DIM];
            for i in 0..LN_DIM {
                c[i][i] = 1.0;
                for j in 0..i {
                    let r = fit::corr_where(&series[i], &series[j], |t| turbulent[t] == state);
                    c[i][j] = r;
                    c[j][i] = r;
                }
            }
            let l = fit::correlation_cholesky(&c);
            for i in 0..LN_DIM {
                for j in 0..LN_DIM {
                    out[i][j] = sd[i] * l[i][j];
                }
            }
        }
        let stats = chain_stats();
        FatTailedParams {
            ln,
            chol,
            var_scale: 1.0 / stats.mean,
            v_turbulent: stats.turbulent,
        }
    }

    /// The state at the start of a path: the long-run mean variance, and the
    /// recession indicator from 1 uniform on `market` (as the lognormal model).
    pub(super) fn start(&self, market: &mut Rng) -> FatState {
        FatState {
            v: 1.0 / self.var_scale,
            recession: u8::from(market.uniform() < self.ln.recession_share()),
        }
    }

    /// One month. Draws from `market`: 6 normals and 1 uniform; from `inflation`:
    /// 1 normal; from `tails`: 3.
    #[inline]
    pub(super) fn step(
        &self,
        s: &mut FatState,
        market: &mut Rng,
        inflation: &mut Rng,
        tails: &mut Rng,
        normal: &ShockTable,
    ) -> Core {
        let mut z = [0.0; LN_DIM];
        for zi in z.iter_mut().take(LN_DIM - 1) {
            *zi = normal.sample(market.next_u64());
        }
        z[LN_DIM - 1] = normal.sample(inflation.next_u64());
        let w = mixing(tails, normal);
        let l = &self.chol[usize::from(s.v > self.v_turbulent)];
        let sv = w * (self.var_scale * s.v).sqrt();
        // US and international stocks, bonds, bills, home prices, gold, CPI.
        let scale = [sv, sv, w, 1.0, 1.0, w, 1.0];
        let mut r = [0.0; LN_DIM];
        for i in 0..LN_DIM {
            let mut acc = 0.0;
            for j in 0..=i {
                acc += l[i][j] * z[j];
            }
            r[i] = math::exp_m1(self.ln.mu[i] + scale[i] * acc);
        }
        // The calm and turbulent factors share the first row (sd, 0, ...), so the
        // standardised US shock is w z0 in both.
        s.v = next_variance(s.v, w * z[EQ]);
        s.recession = u8::from(market.uniform() < self.ln.recession_next[usize::from(s.recession)]);
        Core {
            eq: r[0],
            intl: r[1],
            bond: r[2],
            cash: r[3].max(0.0),
            cpi: r[6],
            home: r[4],
            gold: r[5],
            long_yield: self.ln.long_yield,
            mortgage: self.ln.mortgage,
            recession: s.recession,
        }
    }

    #[cfg(test)]
    pub(super) fn correlation(&self, turbulent: bool, i: usize, j: usize) -> f64 {
        let l = &self.chol[usize::from(turbulent)];
        let dot = |a: usize, b: usize| (0..LN_DIM).map(|k| l[a][k] * l[b][k]).sum::<f64>();
        dot(i, j) / (dot(i, i) * dot(j, j)).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variance_recursion_by_hand() {
        // a = 0.15, b = 0.83: v' = 0.02 + 0.15 v eta^2 + 0.83 v.
        assert!((next_variance(1.0, 0.0) - 0.85).abs() < 1e-15);
        assert!((next_variance(1.0, 1.0) - 1.0).abs() < 1e-15);
        assert!((next_variance(2.0, 2.0) - (0.02 + 1.2 + 1.66)).abs() < 1e-15);
        assert_eq!(next_variance(10.0, 5.0), FAT_TAIL_VARIANCE_CAP);
    }

    #[test]
    fn the_mixing_variable_has_unit_variance_and_fat_tails() {
        let normal = &sampler::normal().table;
        let mut rng = Rng::from_seed(7);
        let n = 400_000;
        let (mut m2, mut m4) = (0.0, 0.0);
        for _ in 0..n {
            let x = mixing(&mut rng, normal) * normal.sample(rng.next_u64());
            m2 += x * x;
            m4 += x * x * x * x;
        }
        let (m2, m4) = (m2 / n as f64, m4 / n as f64);
        // Unit variance (E[3 / chi2_5] = 1), and a t5 has infinite kurtosis: far
        // above the normal's 3 in any sample.
        assert!((m2 - 1.0).abs() < 0.02, "{m2}");
        assert!(m4 / (m2 * m2) > 6.0, "{}", m4 / (m2 * m2));
    }

    #[test]
    fn the_chain_statistics_are_plausible() {
        let s = chain_stats();
        // Without the cap the long-run mean is 1; the cap removes the extreme
        // spells, and the paths spend most months below the mean.
        assert!(s.mean > 0.6 && s.mean < 1.0, "{}", s.mean);
        assert!(s.turbulent > 0.4 && s.turbulent < 1.5, "{}", s.turbulent);
    }
}
