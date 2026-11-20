//! The parametric generators: the two-regime model and the i.i.d. lognormal baseline.
//! Both are calibrated at construction from the embedded history itself, so they
//! follow the data file whenever it is rebuilt.
//!
//! # Regime model
//!
//! * **Regimes.** A two-state Gaussian hidden Markov model is fitted by Baum-Welch to
//!   monthly log excess returns of US equities, `ln(1 + equity_us) - ln(1 + tbill)`,
//!   started from the threshold classification (trailing 12-month volatility above
//!   its 75th percentile), exactly as `data/history/README.md` describes. Months are
//!   assigned to the state with smoothed probability above one half. State 0 is calm,
//!   state 1 turbulent. The fitted transition matrix drives the simulated regime.
//! * **Joint innovations.** Per regime, the mean, standard deviation and correlation
//!   of six series over the months classified to that regime: US and international
//!   equity log excess returns, the log real return of gold, and the surprises of the
//!   long yield, home prices and inflation defined below. Correlations are pairwise
//!   complete (international from 1975, gold from 1968, home surprises from 1953) and
//!   shrunk if needed to be positive definite. Each month the six innovations are a
//!   multivariate Student-t draw (a Cholesky-correlated normal vector times one
//!   chi-square mixing variable, scaled to unit variance).
//! * **Inflation.** `cpi_t = m_t + v_t`, `m_{t+1} = mu + phi (m_t - mu) + K v_t`: an
//!   AR(1) expected-inflation state driven by the monthly surprises (the innovations
//!   form of "persistent AR(1) plus noise"), fitted by moments to the lag-1 and
//!   lag-12 autocorrelations of monthly CPI. The surprise's own innovation is the one
//!   draw taken from the `Inflation` stream.
//! * **Yields and bonds.** The long yield reverts to its historical mean and follows
//!   expected inflation: `dy = b (y - ybar) + c * 12 (m - mu) + shock`, `b` and `c`
//!   by least squares on the history (clamped to a stable, sensible range). The bond
//!   return is carry + duration x yield change + convexity for a 10-year par bond,
//!   the same construction as the `bond10` column.
//! * **Bills.** Bill rate = long yield minus a term spread that follows an AR(1)
//!   fitted to the historical spread; floored at zero.
//! * **Home prices.** `home_t = m_t + rbar + g_t + u_t`, with the real momentum state
//!   `g_{t+1} = phi_h g_t + K_h u_t` fitted by moments to home growth in excess of
//!   expected inflation from 1953 (earlier rows are interpolated annual data).
//! * **Recessions.** A recession indicator with probabilities
//!   `P(recession | previous indicator, current regime)` counted from the NBER flag
//!   and the regime classification.
//! * **Start.** Each path starts from the latest state in the data (regime, yield,
//!   expected inflation, home momentum, spread, recession flag), so the first years
//!   are conditioned on today rather than on an average.

#![allow(clippy::needless_range_loop)]

use crate::data::History;
use crate::math;
use crate::model::YearMonth;
use crate::rng::Rng;

use super::fit::{self, Arma11, Hmm};
use super::sampler::ShockTable;
use super::Core;

/// Number of jointly drawn innovations in the regime model.
pub(super) const DIM: usize = 6;
const EQ: usize = 0;
const INTL: usize = 1;
const GOLD: usize = 2;
const DY: usize = 3;
const HOME: usize = 4;
/// Inflation is last so that its own innovation is the last normal, taken from the
/// `Inflation` stream.
const CPI: usize = 5;

/// Degrees of freedom of the multivariate Student-t innovations. docs/research/
/// landscape.md 2.1 recommends 5 to 8. The regime mixture already supplies most of the
/// monthly kurtosis, so the thin end of that range is used (with 6 the simulated excess
/// kurtosis of US equities is about 15 against 7.4 in the data). 8 is even, so the
/// chi-square mixing variable is `-2 ln(u1 u2 u3 u4)` (four uniforms, one logarithm).
const T_DOF: f64 = 8.0;
/// Cap on the Student-t mixing scale, so that one month cannot be more than this many
/// times the regime's normal scale (it binds less than once in a million months).
const MIXING_CAP: f64 = 8.0;
/// Window and quantile of the threshold classification that starts the HMM fit
/// (data/history/README.md, "Regime methods").
const THRESHOLD_WINDOW: usize = 12;
const THRESHOLD_QUANTILE: f64 = 0.75;
/// Lag used with lag 1 to identify the persistent component of inflation and of home
/// price growth.
const ARMA_LONG_LAG: usize = 12;
/// First month of genuinely monthly home prices (data/history/README.md: Shiller's
/// monthly index starts 1953-02; earlier rows are interpolated annual values).
const HOME_MONTHLY_FROM: YearMonth = YearMonth::ym(1953, 2);
/// Bounds on the monthly speed of mean reversion of the long yield (half-lives of
/// about 19 years and 1 year). The least-squares estimate is used inside this range.
const YIELD_REVERSION_RANGE: (f64, f64) = (0.003, 0.05);
/// The simulated long yield is kept inside this range. The floor is half the lowest
/// 10-year yield in the data (0.55% in 2020); the cap is above the 1981 peak of 15.8%.
const YIELD_RANGE: (f64, f64) = (0.0025, 0.25);
/// Grid step of the duration and convexity tables.
const YIELD_GRID: f64 = 0.001;
/// Lowest monthly inflation the parametric models may produce (keeps `1 + cpi`
/// positive; the historical minimum is about -3%).
pub(super) const MIN_MONTHLY_INFLATION: f64 = -0.2;
/// Minimum observations in a regime before its own moments are used for a series.
const MIN_REGIME_OBS: usize = 24;

/// Long-run return assumptions for the asset classes whose history is too short to
/// estimate one (`data/actuarial/params.json`, `long_run_assumptions`). The same
/// two numbers apply in every market model; volatility and co-movement still come
/// from the data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LongRun {
    /// Annual geometric return of international equities less that of US equities.
    pub(super) intl_spread: f64,
    /// Annual geometric real return of gold.
    pub(super) gold_real: f64,
}

/// Horizon at which the regime model's home-price dispersion is matched to history.
const HOME_SPREAD_MONTHS: usize = 120;
/// Largest monthly pull of the real home-price level toward its trend considered.
const HOME_PULL_MAX: f64 = 0.2;

/// The state carried from month to month.
#[derive(Clone, Copy, Debug)]
pub(super) struct RegimeState {
    /// 0 = calm, 1 = turbulent.
    pub(super) regime: usize,
    /// Long yield at the start of the month.
    pub(super) long_yield: f64,
    /// Expected monthly inflation for the coming month.
    pub(super) expected_inflation: f64,
    /// Home-price momentum: expected real monthly growth in excess of its mean.
    pub(super) home_momentum: f64,
    /// Log of the real home-price level relative to its trend (0 at the start).
    pub(super) home_gap: f64,
    /// Long yield minus the annualised bill rate.
    pub(super) spread: f64,
    pub(super) recession: u8,
}

/// Calibrated parameters of the regime model.
#[derive(Clone, Debug)]
pub(super) struct RegimeParams {
    /// The fitted hidden Markov model (diagnostics and tests).
    #[allow(dead_code)]
    pub(super) hmm: Hmm,
    /// Number of months classified calm.
    #[allow(dead_code)]
    pub(super) calm_months: usize,
    /// P(next regime is turbulent | current regime).
    p_turbulent: [f64; 2],
    /// Per-regime means of the six series.
    pub(super) mean: [[f64; DIM]; 2],
    /// Per-regime standard deviations of the six series (already folded into
    /// `chol`; kept for diagnostics).
    #[allow(dead_code)]
    pub(super) sd: [[f64; DIM]; 2],
    /// Per-regime Cholesky factors, pre-multiplied by the standard deviations.
    chol: [[[f64; DIM]; DIM]; 2],
    pub(super) inflation_mean: f64,
    pub(super) inflation: Arma11,
    pub(super) yield_mean: f64,
    /// Monthly reversion coefficient (negative).
    pub(super) yield_reversion: f64,
    /// Monthly response of the yield to annualised expected inflation above its mean.
    pub(super) yield_inflation: f64,
    spread_mean: f64,
    spread_phi: f64,
    spread_sd: f64,
    pub(super) home_real_mean: f64,
    pub(super) home: Arma11,
    /// Monthly pull of the real home-price level back toward its trend, sized so
    /// that the spread of ten-year real price changes matches the history's.
    pub(super) home_pull: f64,
    mortgage_spread: f64,
    /// P(recession this month | recession last month, current regime).
    pub(super) recession_prob: [[f64; 2]; 2],
    pub(super) start: RegimeState,
    duration: Vec<f64>,
    convexity: Vec<f64>,
}

/// `values` with every non-finite entry replaced by the previous finite one (or 0).
pub(super) fn forward_fill(values: &[f64]) -> Vec<f64> {
    let mut last = 0.0;
    values
        .iter()
        .map(|&v| {
            if v.is_finite() {
                last = v;
            }
            last
        })
        .collect()
}

/// Mean over all rows of `mortgage30 - long_yield` (1.76 points in the data).
pub(super) fn mortgage_spread(h: &History) -> f64 {
    fit::mean_sd(h.mortgage30.iter().zip(&h.long_yield).map(|(m, y)| m - y)).0
}

/// Modified duration (years) of a 10-year par bond with semi-annual coupons at yield
/// `y`: `(1 - (1 + y/2)^-20) / y` (data/history/README.md).
pub(super) fn par_bond_duration(y: f64) -> f64 {
    if y.abs() < 1e-9 {
        return 10.0;
    }
    (1.0 - math::powi(1.0 + 0.5 * y, -20)) / y
}

/// Convexity (years squared) of the same bond:
/// `sum_k CF_k * k (k + 1) * (1 + y/2)^-(k + 2) / 4`.
pub(super) fn par_bond_convexity(y: f64) -> f64 {
    let v = 1.0 / (1.0 + 0.5 * y);
    let mut discount = v * v;
    let mut sum = 0.0;
    for k in 1..=20 {
        discount *= v;
        let cash_flow = 0.5 * y + if k == 20 { 1.0 } else { 0.0 };
        sum += cash_flow * f64::from(k * (k + 1)) * discount;
    }
    0.25 * sum
}

impl RegimeParams {
    /// Fits everything from the history. See the module documentation.
    pub(super) fn calibrate(h: &History, long_run: &LongRun) -> RegimeParams {
        let n = h.len();
        let home = forward_fill(&h.home);
        let log_bill: Vec<f64> = h.tbill.iter().map(|&r| math::ln_1p(r)).collect();
        let excess: Vec<f64> = h
            .equity_us
            .iter()
            .zip(&log_bill)
            .map(|(&r, lb)| math::ln_1p(r) - lb)
            .collect();
        let init =
            fit::threshold_classification(&h.equity_us, THRESHOLD_WINDOW, THRESHOLD_QUANTILE);
        let hmm = Hmm::fit(&excess, &init);
        let regime: Vec<usize> = hmm
            .p_turbulent
            .iter()
            .map(|&p| usize::from(p > 0.5))
            .collect();
        let calm_months = regime.iter().filter(|&&r| r == 0).count();
        let p_stationary = hmm.stationary_turbulent();

        // Inflation: expected-inflation state and surprises.
        let inflation_mean = fit::mean_sd(h.cpi.iter().copied()).0;
        let inflation = Arma11::from_autocorrelations(
            fit::autocorr(&h.cpi, 1),
            fit::autocorr(&h.cpi, ARMA_LONG_LAG),
            ARMA_LONG_LAG,
        );
        let (expected, surprise) = inflation.filter(&h.cpi, inflation_mean);

        // Long yield: reversion to the mean plus response to expected inflation.
        let yield_mean = fit::mean_sd(h.long_yield.iter().copied()).0;
        let mut x = Vec::with_capacity(n);
        let mut dy = Vec::with_capacity(n);
        for t in 1..n {
            x.push([
                1.0,
                h.long_yield[t - 1] - yield_mean,
                12.0 * (expected[t] - inflation_mean),
            ]);
            dy.push(h.long_yield[t] - h.long_yield[t - 1]);
        }
        let (coef, _) = fit::ols(&x, &dy);
        let yield_reversion = -(-coef[1]).clamp(YIELD_REVERSION_RANGE.0, YIELD_REVERSION_RANGE.1);
        // Long-run pass-through of expected inflation into the yield is c / -b; keep
        // it between 0 and 1 (the Fisher relation).
        let yield_inflation = coef[2].clamp(0.0, -yield_reversion);
        let mut yield_shock = vec![f64::NAN; n];
        for t in 1..n {
            yield_shock[t] =
                dy[t - 1] - yield_reversion * x[t - 1][1] - yield_inflation * x[t - 1][2];
        }

        // Term spread over bills.
        let spread: Vec<f64> = h
            .long_yield
            .iter()
            .zip(&h.tbill)
            .map(|(y, b)| y - 12.0 * b)
            .collect();
        let (spread_mean, spread_total_sd, _) = fit::mean_sd(spread.iter().copied());
        let spread_phi = fit::autocorr(&spread, 1).clamp(0.0, 0.99);
        let spread_sd = spread_total_sd * (1.0 - spread_phi * spread_phi).sqrt();

        // Home prices: momentum in growth above expected inflation.
        let first_home = h.index_of(HOME_MONTHLY_FROM).unwrap_or(0);
        let real_growth: Vec<f64> = (first_home..n).map(|t| home[t] - expected[t]).collect();
        let momentum_mean = fit::mean_sd(real_growth.iter().copied()).0;
        let home_arma = Arma11::from_autocorrelations(
            fit::autocorr(&real_growth, 1),
            fit::autocorr(&real_growth, ARMA_LONG_LAG),
            ARMA_LONG_LAG,
        );
        let (momentum, home_surprise_tail) = home_arma.filter(&real_growth, momentum_mean);
        let mut home_surprise = vec![f64::NAN; n];
        home_surprise[first_home..].copy_from_slice(&home_surprise_tail);
        let home_real_mean =
            fit::mean_sd(home.iter().copied()).0 - fit::mean_sd(h.cpi.iter().copied()).0;

        // Per-regime joint moments of the six series.
        let mut series: [Vec<f64>; DIM] = Default::default();
        series[EQ] = excess;
        series[INTL] = h
            .equity_intl
            .iter()
            .zip(&log_bill)
            .map(|(&r, lb)| math::ln_1p(r) - lb)
            .collect();
        series[GOLD] = h
            .gold
            .iter()
            .zip(&h.cpi)
            .map(|(&g, &c)| math::ln_1p(g) - math::ln_1p(c))
            .collect();
        series[DY] = yield_shock;
        series[HOME] = home_surprise;
        series[CPI] = surprise;

        let mut mean = [[0.0; DIM]; 2];
        let mut sd = [[0.0; DIM]; 2];
        let mut chol = [[[0.0; DIM]; DIM]; 2];
        for i in 0..DIM {
            let (all_mean, all_sd, _) = fit::mean_sd(series[i].iter().copied());
            for k in 0..2 {
                let (m, s, count) = fit::mean_sd(
                    series[i]
                        .iter()
                        .zip(&regime)
                        .filter(|(_, &r)| r == k)
                        .map(|(v, _)| *v),
                );
                (mean[k][i], sd[k][i]) = if count >= MIN_REGIME_OBS {
                    (m, s)
                } else {
                    (all_mean, all_sd)
                };
            }
            if i == EQ {
                // The HMM's own (probability-weighted) estimates.
                for k in 0..2 {
                    mean[k][i] = hmm.mu[k];
                    sd[k][i] = hmm.sigma[k];
                }
                continue;
            }
            // Shift both regime means so the long-run average equals the sample
            // average (returns) or zero (surprises): the regime shares of a short
            // sample differ from the stationary shares.
            // International equities and gold are short series whose own-sample
            // means are not comparable with the full-sample means of the rest: both
            // follow the stated long-run assumptions instead (`LongRun`).
            let target = match i {
                INTL => {
                    (1.0 - p_stationary) * mean[0][EQ]
                        + p_stationary * mean[1][EQ]
                        + math::ln_1p(long_run.intl_spread) / 12.0
                }
                GOLD => math::ln_1p(long_run.gold_real) / 12.0,
                _ => 0.0,
            };
            let _ = all_mean;
            let long_run = (1.0 - p_stationary) * mean[0][i] + p_stationary * mean[1][i];
            for k in 0..2 {
                mean[k][i] += target - long_run;
            }
        }
        // The history is not exactly an ARMA(1,1), so the filtered surprises are a
        // little larger than the fitted process needs to reproduce the sample
        // variance. Rescale them so the model's unconditional monthly variance,
        // `var(v) * (1 + K^2 / (1 - phi^2))`, equals the sample's.
        let mut rescale = |i: usize, arma: &Arma11, sample_sd: f64| {
            let var_v = (1.0 - p_stationary) * (sd[0][i] * sd[0][i] + mean[0][i] * mean[0][i])
                + p_stationary * (sd[1][i] * sd[1][i] + mean[1][i] * mean[1][i]);
            let implied = var_v * (1.0 + arma.gain * arma.gain / (1.0 - arma.phi * arma.phi));
            if implied > 0.0 && sample_sd > 0.0 {
                let k = sample_sd / implied.sqrt();
                for r in 0..2 {
                    sd[r][i] *= k;
                    mean[r][i] *= k;
                }
            }
        };
        rescale(CPI, &inflation, fit::mean_sd(h.cpi.iter().copied()).1);
        rescale(
            HOME,
            &home_arma,
            fit::mean_sd(real_growth.iter().copied()).1,
        );
        for k in 0..2 {
            let mut c = [[0.0; DIM]; DIM];
            for i in 0..DIM {
                for j in 0..i {
                    let r = fit::corr_where(&series[i], &series[j], |t| regime[t] == k);
                    c[i][j] = r;
                    c[j][i] = r;
                }
            }
            let l = fit::correlation_cholesky(&c);
            for i in 0..DIM {
                for j in 0..DIM {
                    chol[k][i][j] = sd[k][i] * l[i][j];
                }
            }
        }

        // Momentum alone lets the real price level wander without limit: the spread
        // of ten-year real changes comes out three to four times the history's.
        // Add a pull of the level toward its trend, sized to match that spread.
        let home_pull = {
            let real_log: Vec<f64> = (first_home..n)
                .map(|t| math::ln_1p(home[t]) - math::ln_1p(h.cpi[t]))
                .collect();
            let mut changes = Vec::new();
            if real_log.len() > HOME_SPREAD_MONTHS {
                let mut window: f64 = real_log[..HOME_SPREAD_MONTHS].iter().sum();
                changes.push(window);
                for t in HOME_SPREAD_MONTHS..real_log.len() {
                    window += real_log[t] - real_log[t - HOME_SPREAD_MONTHS];
                    changes.push(window);
                }
            }
            let target = fit::mean_sd(changes.iter().copied()).1;
            let var_v = (1.0 - p_stationary)
                * (sd[0][HOME] * sd[0][HOME] + mean[0][HOME] * mean[0][HOME])
                + p_stationary * (sd[1][HOME] * sd[1][HOME] + mean[1][HOME] * mean[1][HOME]);
            // Variance of the level after HOME_SPREAD_MONTHS from a known start, for
            // the linear system (momentum, gap) driven by one shock.
            let spread_at = |pull: f64| -> f64 {
                let (phi, gain) = (home_arma.phi, home_arma.gain);
                // Covariance entries: mm, mg, gg.
                let (mut mm, mut mg, mut gg) = (0.0f64, 0.0f64, 0.0f64);
                let a = 1.0 - pull;
                for _ in 0..HOME_SPREAD_MONTHS {
                    let n_mm = phi * phi * mm + gain * gain * var_v;
                    let n_mg = phi * (mm + a * mg) + gain * var_v;
                    let n_gg = mm + 2.0 * a * mg + a * a * gg + var_v;
                    (mm, mg, gg) = (n_mm, n_mg, n_gg);
                }
                gg.max(0.0).sqrt()
            };
            if changes.len() < 24 || target <= 0.0 || spread_at(0.0) <= target {
                0.0
            } else {
                let (mut lo, mut hi) = (0.0, HOME_PULL_MAX);
                for _ in 0..40 {
                    let mid = 0.5 * (lo + hi);
                    if spread_at(mid) > target {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                0.5 * (lo + hi)
            }
        };

        // Recession indicator given the previous indicator and the current regime.
        let mut counts = [[[1.0f64; 2]; 2]; 2];
        for t in 1..n {
            counts[usize::from(h.recession[t - 1])][regime[t]][usize::from(h.recession[t])] += 1.0;
        }
        let mut recession_prob = [[0.0; 2]; 2];
        for prev in 0..2 {
            for k in 0..2 {
                recession_prob[prev][k] =
                    counts[prev][k][1] / (counts[prev][k][0] + counts[prev][k][1]);
            }
        }

        let grid = (YIELD_RANGE.1 / YIELD_GRID).ceil() as usize + 2;
        let duration = (0..grid)
            .map(|i| par_bond_duration(i as f64 * YIELD_GRID))
            .collect();
        let convexity = (0..grid)
            .map(|i| par_bond_convexity(i as f64 * YIELD_GRID))
            .collect();

        let start = RegimeState {
            regime: regime.last().copied().unwrap_or(0),
            long_yield: h
                .long_yield
                .last()
                .copied()
                .unwrap_or(yield_mean)
                .clamp(YIELD_RANGE.0, YIELD_RANGE.1),
            expected_inflation: expected.last().copied().unwrap_or(inflation_mean),
            // The index is not seasonally adjusted: the last month alone carries the
            // season. Start from the average momentum of the last twelve months.
            home_momentum: {
                let tail = &momentum[momentum.len().saturating_sub(12)..];
                if tail.is_empty() {
                    0.0
                } else {
                    tail.iter().sum::<f64>() / tail.len() as f64 - momentum_mean
                }
            },
            home_gap: 0.0,
            spread: spread.last().copied().unwrap_or(spread_mean),
            recession: h.recession.last().copied().unwrap_or(0),
        };
        RegimeParams {
            p_turbulent: [hmm.trans[0][1], hmm.trans[1][1]],
            hmm,
            calm_months,
            mean,
            sd,
            chol,
            inflation_mean,
            inflation,
            yield_mean,
            yield_reversion,
            yield_inflation,
            spread_mean,
            spread_phi,
            spread_sd,
            home_real_mean,
            home: home_arma,
            home_pull,
            mortgage_spread: mortgage_spread(h),
            recession_prob,
            start,
            duration,
            convexity,
        }
    }

    /// Return of the 10-year par bond over a month in which its yield moves from
    /// `y0` by `dy`: carry + duration x yield change + convexity.
    #[inline]
    pub(super) fn bond_return(&self, y0: f64, dy: f64) -> f64 {
        let pos = (y0 / YIELD_GRID).clamp(0.0, (self.duration.len() - 2) as f64);
        let i = pos as usize;
        let f = pos - i as f64;
        let d = self.duration[i] + f * (self.duration[i + 1] - self.duration[i]);
        let c = self.convexity[i] + f * (self.convexity[i + 1] - self.convexity[i]);
        y0 / 12.0 - d * dy + 0.5 * c * dy * dy
    }

    /// Mean monthly real log return of US equities over the first `months` of a
    /// path from today's state, estimated on `paths` paths of a fixed private seed
    /// (deterministic). The regime model's mean has no closed form: the bill rate
    /// floor, the start from today's state and the logarithms all move it.
    pub(super) fn real_equity_log_mean(&self, paths: u64, months: usize) -> f64 {
        let normal = &super::sampler::normal().table;
        let (mut sum, mut n) = (0.0, 0.0);
        for path in 0..paths {
            let mut market = Rng::from_seed(0x1800_0000_0000_0001 ^ path);
            let mut inflation = Rng::from_seed(0x1800_0000_0000_0002 ^ path);
            let mut state = self.start;
            for _ in 0..months {
                let c = self.step(&mut state, &mut market, &mut inflation, normal);
                sum += math::ln_1p(c.eq) - math::ln_1p(c.cpi);
                n += 1.0;
            }
        }
        sum / n
    }

    /// Advances one month. Draws from `market`: 1 uniform (regime), 4 uniforms
    /// (mixing), 5 normals, 1 normal (spread), 1 uniform (recession); from
    /// `inflation`: 1 normal.
    #[inline]
    pub(super) fn step(
        &self,
        s: &mut RegimeState,
        market: &mut Rng,
        inflation: &mut Rng,
        normal: &ShockTable,
    ) -> Core {
        let k = usize::from(market.uniform() < self.p_turbulent[s.regime]);
        s.regime = k;
        let chi2 = -2.0
            * math::ln(
                market.uniform_open0()
                    * market.uniform_open0()
                    * market.uniform_open0()
                    * market.uniform_open0(),
            );
        let w = ((T_DOF - 2.0) / chi2).sqrt().min(MIXING_CAP);
        let mut z = [0.0; DIM];
        for zi in z.iter_mut().take(DIM - 1) {
            *zi = normal.sample(market.next_u64());
        }
        z[DIM - 1] = normal.sample(inflation.next_u64());
        let l = &self.chol[k];
        let mut e = self.mean[k];
        for i in 0..DIM {
            let mut acc = 0.0;
            for j in 0..=i {
                acc += l[i][j] * z[j];
            }
            e[i] += w * acc;
        }
        let spread_shock = normal.sample(market.next_u64());
        let recession_draw = market.uniform();

        let y0 = s.long_yield;
        let cash = (y0 - s.spread).max(0.0) / 12.0;
        let log_cash = math::ln_1p(cash);
        let m = s.expected_inflation;
        let cpi = (m + e[CPI]).max(MIN_MONTHLY_INFLATION);
        let y1 = (y0
            + self.yield_reversion * (y0 - self.yield_mean)
            + self.yield_inflation * 12.0 * (m - self.inflation_mean)
            + e[DY])
            .clamp(YIELD_RANGE.0, YIELD_RANGE.1);
        let recession = u8::from(recession_draw < self.recession_prob[usize::from(s.recession)][k]);
        let core = Core {
            eq: math::exp_m1(log_cash + e[EQ]),
            intl: math::exp_m1(log_cash + e[INTL]),
            bond: self.bond_return(y0, y1 - y0),
            cash,
            cpi,
            home: m + self.home_real_mean + s.home_momentum - self.home_pull * s.home_gap + e[HOME],
            gold: (1.0 + cpi) * math::exp(e[GOLD]) - 1.0,
            long_yield: y1,
            mortgage: y1 + self.mortgage_spread,
            recession,
        };
        s.long_yield = y1;
        s.expected_inflation = self.inflation_mean
            + self.inflation.phi * (m - self.inflation_mean)
            + self.inflation.gain * (cpi - m);
        // The gap is measured against expected inflation, as the calibration of the
        // pull assumes: measured against realised inflation it would tie nominal home
        // prices to every inflation surprise.
        s.home_gap += core.home - m - self.home_real_mean;
        s.home_momentum = self.home.phi * s.home_momentum + self.home.gain * e[HOME];
        s.spread = self.spread_mean
            + self.spread_phi * (s.spread - self.spread_mean)
            + self.spread_sd * spread_shock;
        s.recession = recession;
        core
    }
}

/// Number of series in the lognormal model.
pub(super) const LN_DIM: usize = 7;

/// The naive baseline: every month an independent draw of jointly normal log returns
/// `ln(1 + r)` of US equities, international equities, bonds, bills, home prices,
/// gold and CPI (in that order; CPI last so its own innovation comes from the
/// `Inflation` stream) with the full-sample means, standard deviations and
/// pairwise-complete correlations of the history. No serial dependence of any kind,
/// except in the recession indicator, which is an independent two-state Markov chain
/// with the NBER transition frequencies so that job-loss risk keeps its clustering.
/// The long yield and the mortgage rate are constant at their historical means.
#[derive(Clone, Debug)]
pub(super) struct LognormalParams {
    mu: [f64; LN_DIM],
    chol: [[f64; LN_DIM]; LN_DIM],
    long_yield: f64,
    mortgage: f64,
    /// P(recession next month | indicator this month).
    pub(super) recession_next: [f64; 2],
}

impl LognormalParams {
    pub(super) fn calibrate(h: &History, long_run: &LongRun) -> LognormalParams {
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
        let mut mu = [0.0; LN_DIM];
        let mut sd = [0.0; LN_DIM];
        let mut c = [[0.0; LN_DIM]; LN_DIM];
        for i in 0..LN_DIM {
            (mu[i], sd[i], _) = fit::mean_sd(series[i].iter().copied());
            for j in 0..i {
                let r = fit::corr(&series[i], &series[j]);
                c[i][j] = r;
                c[j][i] = r;
            }
        }
        // International equities and gold are short series: their own-sample means
        // would be set against full-sample inflation. Both follow the long-run
        // assumptions instead (index 0 US equities, 1 international, 5 gold, 6 CPI).
        mu[1] = mu[0] + math::ln_1p(long_run.intl_spread) / 12.0;
        mu[5] = mu[6] + math::ln_1p(long_run.gold_real) / 12.0;
        // Bills are floored at zero in `step` (a bill cannot pay less than nothing,
        // and independent normal draws around a 0.27% monthly mean go negative one
        // month in seven). The floor alone would raise the average bill return:
        // lower the centre so that the floored draw keeps the sample mean,
        // E max(0, N(m, s)) = m Phi(m / s) + s phi(m / s).
        if sd[3] > 0.0 && mu[3] > 0.0 {
            let (target, s) = (mu[3], sd[3]);
            let floored = |m: f64| {
                let x = m / s;
                m * 0.5 * math::erfc(-x / std::f64::consts::SQRT_2)
                    + s * math::exp(-0.5 * x * x) / (2.0 * std::f64::consts::PI).sqrt()
            };
            let (mut lo, mut hi) = (target - 4.0 * s, target);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if floored(mid) < target {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            mu[3] = 0.5 * (lo + hi);
        }
        let l = fit::correlation_cholesky(&c);
        let mut chol = [[0.0; LN_DIM]; LN_DIM];
        for i in 0..LN_DIM {
            for j in 0..LN_DIM {
                chol[i][j] = sd[i] * l[i][j];
            }
        }
        let mut counts = [[1.0f64; 2]; 2];
        for w in h.recession.windows(2) {
            counts[usize::from(w[0])][usize::from(w[1])] += 1.0;
        }
        let long_yield = fit::mean_sd(h.long_yield.iter().copied()).0;
        LognormalParams {
            mu,
            chol,
            long_yield,
            mortgage: long_yield + mortgage_spread(h),
            recession_next: [
                counts[0][1] / (counts[0][0] + counts[0][1]),
                counts[1][1] / (counts[1][0] + counts[1][1]),
            ],
        }
    }

    /// Mean of the monthly real log return of US equities: exactly `mu_eq - mu_cpi`
    /// (log returns are normal with these means).
    pub(super) fn real_equity_log_mean(&self) -> f64 {
        self.mu[0] - self.mu[LN_DIM - 1]
    }

    /// Long-run share of recession months implied by the chain.
    pub(super) fn recession_share(&self) -> f64 {
        let (a, b) = (self.recession_next[0], 1.0 - self.recession_next[1]);
        a / (a + b)
    }

    /// One month. Draws from `market`: 6 normals and 1 uniform; from `inflation`:
    /// 1 normal.
    #[inline]
    pub(super) fn step(
        &self,
        recession: &mut u8,
        market: &mut Rng,
        inflation: &mut Rng,
        normal: &ShockTable,
    ) -> Core {
        let mut z = [0.0; LN_DIM];
        for zi in z.iter_mut().take(LN_DIM - 1) {
            *zi = normal.sample(market.next_u64());
        }
        z[LN_DIM - 1] = normal.sample(inflation.next_u64());
        let mut r = [0.0; LN_DIM];
        for i in 0..LN_DIM {
            let mut acc = self.mu[i];
            for j in 0..=i {
                acc += self.chol[i][j] * z[j];
            }
            r[i] = math::exp_m1(acc);
        }
        *recession = u8::from(market.uniform() < self.recession_next[usize::from(*recession)]);
        Core {
            eq: r[0],
            intl: r[1],
            bond: r[2],
            // A bill cannot pay less than nothing.
            cash: r[3].max(0.0),
            cpi: r[6],
            home: r[4],
            gold: r[5],
            long_yield: self.long_yield,
            mortgage: self.mortgage,
            recession: *recession,
        }
    }
}
