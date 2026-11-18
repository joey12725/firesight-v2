//! Construction-time statistics: moments, small dense linear algebra, least squares,
//! ARMA(1,1) by moments and a two-state Gaussian hidden Markov model. Nothing here
//! runs per month.

#![allow(clippy::needless_range_loop)]

use crate::math;

/// Mean, sample standard deviation and count of the finite values.
pub(super) fn mean_sd(xs: impl Iterator<Item = f64> + Clone) -> (f64, f64, usize) {
    let (mut sum, mut n) = (0.0, 0usize);
    for x in xs.clone().filter(|x| x.is_finite()) {
        sum += x;
        n += 1;
    }
    if n == 0 {
        return (0.0, 0.0, 0);
    }
    let mean = sum / n as f64;
    let ss: f64 = xs
        .filter(|x| x.is_finite())
        .map(|x| (x - mean) * (x - mean))
        .sum();
    let sd = if n > 1 {
        (ss / (n - 1) as f64).sqrt()
    } else {
        0.0
    };
    (mean, sd, n)
}

/// Pearson correlation over the positions where both values are finite and `keep`
/// holds. Returns 0 when fewer than three pairs or a series is constant.
pub(super) fn corr_where(a: &[f64], b: &[f64], keep: impl Fn(usize) -> bool) -> f64 {
    let pairs = || {
        a.iter()
            .zip(b)
            .enumerate()
            .filter(|(i, (x, y))| x.is_finite() && y.is_finite() && keep(*i))
            .map(|(_, (x, y))| (*x, *y))
    };
    let n = pairs().count();
    if n < 3 {
        return 0.0;
    }
    let (mut sa, mut sb) = (0.0, 0.0);
    for (x, y) in pairs() {
        sa += x;
        sb += y;
    }
    let (ma, mb) = (sa / n as f64, sb / n as f64);
    let (mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0);
    for (x, y) in pairs() {
        saa += (x - ma) * (x - ma);
        sbb += (y - mb) * (y - mb);
        sab += (x - ma) * (y - mb);
    }
    if saa <= 0.0 || sbb <= 0.0 {
        0.0
    } else {
        sab / (saa * sbb).sqrt()
    }
}

/// Pearson correlation over the positions where both values are finite.
pub(super) fn corr(a: &[f64], b: &[f64]) -> f64 {
    corr_where(a, b, |_| true)
}

/// Sample autocorrelation at `lag` of a gap-free series.
pub(super) fn autocorr(x: &[f64], lag: usize) -> f64 {
    if x.len() <= lag + 1 {
        return 0.0;
    }
    let mean = x.iter().sum::<f64>() / x.len() as f64;
    let denom: f64 = x.iter().map(|v| (v - mean) * (v - mean)).sum();
    if denom <= 0.0 {
        return 0.0;
    }
    let num: f64 = x[lag..]
        .iter()
        .zip(x)
        .map(|(a, b)| (a - mean) * (b - mean))
        .sum();
    num / denom
}

/// Lower Cholesky factor of a symmetric positive definite matrix, or `None`.
pub(super) fn cholesky<const N: usize>(a: &[[f64; N]; N]) -> Option<[[f64; N]; N]> {
    let mut l = [[0.0; N]; N];
    for i in 0..N {
        for j in 0..=i {
            let mut s = a[i][j];
            for k in 0..j {
                s -= l[i][k] * l[j][k];
            }
            if i == j {
                if s <= 1e-12 {
                    return None;
                }
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    Some(l)
}

/// Shrink factor applied to off-diagonal correlations when a pairwise-complete matrix
/// is not positive definite (an engineering choice: the smallest uniform shrink in
/// steps of 5% that makes the factorisation succeed).
const PSD_SHRINK_STEP: f64 = 0.95;

/// Cholesky factor of a correlation matrix estimated pairwise (so possibly not
/// positive definite): off-diagonals are shrunk towards zero until it factors.
pub(super) fn correlation_cholesky<const N: usize>(corr: &[[f64; N]; N]) -> [[f64; N]; N] {
    let mut c = *corr;
    for i in 0..N {
        c[i][i] = 1.0;
    }
    for _ in 0..200 {
        if let Some(l) = cholesky(&c) {
            return l;
        }
        for i in 0..N {
            for j in 0..N {
                if i != j {
                    c[i][j] *= PSD_SHRINK_STEP;
                }
            }
        }
    }
    let mut identity = [[0.0; N]; N];
    for i in 0..N {
        identity[i][i] = 1.0;
    }
    identity
}

/// Solves `a x = b` by Gaussian elimination with partial pivoting. Returns zeros for a
/// singular system.
pub(super) fn solve<const K: usize>(mut a: [[f64; K]; K], mut b: [f64; K]) -> [f64; K] {
    for col in 0..K {
        let mut pivot = col;
        for r in col + 1..K {
            if a[r][col].abs() > a[pivot][col].abs() {
                pivot = r;
            }
        }
        if a[pivot][col].abs() < 1e-300 {
            return [0.0; K];
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        for r in col + 1..K {
            let f = a[r][col] / a[col][col];
            for c in col..K {
                a[r][c] -= f * a[col][c];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = [0.0; K];
    for r in (0..K).rev() {
        let mut s = b[r];
        for c in r + 1..K {
            s -= a[r][c] * x[c];
        }
        x[r] = s / a[r][r];
    }
    x
}

/// Ordinary least squares of `y` on the regressors `x` (include a column of ones for
/// an intercept). Returns the coefficients and the residual standard deviation.
pub(super) fn ols<const K: usize>(x: &[[f64; K]], y: &[f64]) -> ([f64; K], f64) {
    let mut xtx = [[0.0; K]; K];
    let mut xty = [0.0; K];
    for (row, &yv) in x.iter().zip(y) {
        for i in 0..K {
            xty[i] += row[i] * yv;
            for j in 0..K {
                xtx[i][j] += row[i] * row[j];
            }
        }
    }
    let beta = solve(xtx, xty);
    let mut ss = 0.0;
    for (row, &yv) in x.iter().zip(y) {
        let fit: f64 = row.iter().zip(&beta).map(|(a, b)| a * b).sum();
        ss += (yv - fit) * (yv - fit);
    }
    let dof = x.len().saturating_sub(K).max(1);
    (beta, (ss / dof as f64).sqrt())
}

/// Largest AR coefficient accepted from a moment fit; keeps the half-life finite
/// (about 11 years at 0.995 monthly).
const MAX_AR: f64 = 0.995;

/// An ARMA(1,1) process in innovations (steady-state Kalman) form:
///
/// ```text
/// x_t     = m_t + v_t                         v_t = one-step-ahead surprise
/// m_{t+1} = mu + phi * (m_t - mu) + gain * v_t
/// ```
///
/// This is the observable form of "persistent AR(1) component plus noise": `m_t` is
/// the expected value of `x_t` given the past.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Arma11 {
    pub(super) phi: f64,
    pub(super) gain: f64,
}

impl Arma11 {
    /// Method of moments from the autocorrelations at lag 1 and lag `k`: for an
    /// ARMA(1,1) `rho(k) = phi^(k-1) * rho(1)`, which gives `phi`; the moving-average
    /// coefficient `theta` then solves
    /// `rho(1) = (1 - phi*theta)(phi - theta) / (1 + theta^2 - 2*phi*theta)` and
    /// `gain = phi - theta`. Falls back to a plain AR(1) when the two autocorrelations
    /// are not consistent with that shape.
    pub(super) fn from_autocorrelations(rho1: f64, rho_k: f64, k: usize) -> Arma11 {
        if !(rho1 > 0.02 && rho_k > 0.0 && rho_k < rho1 && k >= 2) {
            let phi = rho1.clamp(0.0, MAX_AR);
            return Arma11 { phi, gain: phi };
        }
        let phi = math::pow(rho_k / rho1, 1.0 / (k as f64 - 1.0)).min(MAX_AR);
        if rho1 >= phi {
            return Arma11 { phi, gain: phi };
        }
        let rho = |theta: f64| {
            (1.0 - phi * theta) * (phi - theta) / (1.0 + theta * theta - 2.0 * phi * theta)
        };
        // rho(theta) falls from phi at theta = 0 to 0 at theta = phi.
        let (mut lo, mut hi) = (0.0, phi);
        for _ in 0..80 {
            let mid = 0.5 * (lo + hi);
            if rho(mid) > rho1 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Arma11 {
            phi,
            gain: phi - 0.5 * (lo + hi),
        }
    }

    /// Runs the filter over `x` from `m = mu`: returns the expectations `m_t` (one
    /// longer than `x`: the last entry is the expectation for the month after the
    /// sample) and the surprises `v_t`.
    pub(super) fn filter(&self, x: &[f64], mu: f64) -> (Vec<f64>, Vec<f64>) {
        let mut m = Vec::with_capacity(x.len() + 1);
        let mut v = Vec::with_capacity(x.len());
        let mut cur = mu;
        for &xv in x {
            m.push(cur);
            let surprise = xv - cur;
            v.push(surprise);
            cur = mu + self.phi * (cur - mu) + self.gain * surprise;
        }
        m.push(cur);
        (m, v)
    }
}

/// A fitted two-state Gaussian hidden Markov model. State 0 is the low-volatility
/// ("calm") state.
#[derive(Clone, Debug)]
pub(super) struct Hmm {
    pub(super) mu: [f64; 2],
    pub(super) sigma: [f64; 2],
    /// `trans[i][j]` = P(next state j | state i).
    pub(super) trans: [[f64; 2]; 2],
    /// Log-likelihood at convergence (diagnostic).
    #[allow(dead_code)]
    pub(super) loglik: f64,
    /// EM iterations used (diagnostic).
    #[allow(dead_code)]
    pub(super) iterations: usize,
    /// Smoothed probability of state 1 (turbulent) for every observation.
    pub(super) p_turbulent: Vec<f64>,
}

/// Baum-Welch stops when the log-likelihood improves by less than this.
const HMM_TOLERANCE: f64 = 1e-7;
const HMM_MAX_ITERATIONS: usize = 500;
/// Floor on a state's standard deviation, so a state cannot collapse onto one point.
const HMM_MIN_SIGMA: f64 = 1e-4;

impl Hmm {
    /// Stationary probability of the turbulent state.
    pub(super) fn stationary_turbulent(&self) -> f64 {
        let (a, b) = (self.trans[0][1], self.trans[1][0]);
        if a + b <= 0.0 {
            0.0
        } else {
            a / (a + b)
        }
    }

    /// Fits by Baum-Welch (EM) with scaled forward-backward recursions, started from
    /// the hard classification `init` (true = turbulent).
    pub(super) fn fit(x: &[f64], init: &[bool]) -> Hmm {
        const INV_SQRT_2PI: f64 = 0.398_942_280_401_432_7;
        let n = x.len();
        let mut mu = [0.0; 2];
        let mut sigma = [1.0; 2];
        for k in 0..2 {
            let (m, s, _) = mean_sd(
                x.iter()
                    .zip(init)
                    .filter(|(_, &c)| usize::from(c) == k)
                    .map(|(v, _)| *v),
            );
            mu[k] = m;
            sigma[k] = s.max(HMM_MIN_SIGMA);
        }
        let mut counts = [[1.0; 2]; 2];
        for w in init.windows(2) {
            counts[usize::from(w[0])][usize::from(w[1])] += 1.0;
        }
        let mut trans = [[0.0; 2]; 2];
        for i in 0..2 {
            let total = counts[i][0] + counts[i][1];
            trans[i] = [counts[i][0] / total, counts[i][1] / total];
        }
        let mut start = [0.5, 0.5];

        let mut b = vec![[0.0f64; 2]; n];
        let mut alpha = vec![[0.0f64; 2]; n];
        let mut beta = vec![[0.0f64; 2]; n];
        let mut scale = vec![0.0f64; n];
        let mut gamma1 = vec![0.0f64; n];
        let mut loglik = f64::NEG_INFINITY;
        let mut iterations = 0;
        while iterations < HMM_MAX_ITERATIONS && n > 2 {
            iterations += 1;
            for (bt, &xv) in b.iter_mut().zip(x) {
                for k in 0..2 {
                    let z = (xv - mu[k]) / sigma[k];
                    bt[k] = (INV_SQRT_2PI / sigma[k] * math::exp(-0.5 * z * z)).max(1e-300);
                }
            }
            // Forward.
            let mut ll = 0.0;
            for t in 0..n {
                let mut a = [0.0; 2];
                for j in 0..2 {
                    let prior = if t == 0 {
                        start[j]
                    } else {
                        alpha[t - 1][0] * trans[0][j] + alpha[t - 1][1] * trans[1][j]
                    };
                    a[j] = prior * b[t][j];
                }
                let c = a[0] + a[1];
                scale[t] = c;
                alpha[t] = [a[0] / c, a[1] / c];
                ll += math::ln(c);
            }
            // Backward, with the same scaling.
            beta[n - 1] = [1.0, 1.0];
            for t in (0..n - 1).rev() {
                for i in 0..2 {
                    beta[t][i] = (trans[i][0] * b[t + 1][0] * beta[t + 1][0]
                        + trans[i][1] * b[t + 1][1] * beta[t + 1][1])
                        / scale[t + 1];
                }
            }
            // Expected counts.
            let mut xi = [[0.0; 2]; 2];
            let mut w = [0.0; 2];
            let mut wx = [0.0; 2];
            for t in 0..n {
                let g = [alpha[t][0] * beta[t][0], alpha[t][1] * beta[t][1]];
                let total = g[0] + g[1];
                let g = [g[0] / total, g[1] / total];
                gamma1[t] = g[1];
                for k in 0..2 {
                    w[k] += g[k];
                    wx[k] += g[k] * x[t];
                }
                if t + 1 < n {
                    for i in 0..2 {
                        for j in 0..2 {
                            xi[i][j] += alpha[t][i] * trans[i][j] * b[t + 1][j] * beta[t + 1][j]
                                / scale[t + 1];
                        }
                    }
                }
            }
            let converged = (ll - loglik).abs() < HMM_TOLERANCE;
            loglik = ll;
            if converged {
                break;
            }
            // Maximisation.
            for k in 0..2 {
                mu[k] = wx[k] / w[k];
            }
            let mut wss = [0.0; 2];
            for t in 0..n {
                let g1 = gamma1[t];
                wss[0] += (1.0 - g1) * (x[t] - mu[0]) * (x[t] - mu[0]);
                wss[1] += g1 * (x[t] - mu[1]) * (x[t] - mu[1]);
            }
            for k in 0..2 {
                sigma[k] = (wss[k] / w[k]).sqrt().max(HMM_MIN_SIGMA);
                let total = xi[k][0] + xi[k][1];
                trans[k] = [xi[k][0] / total, xi[k][1] / total];
            }
            start = [1.0 - gamma1[0], gamma1[0]];
        }
        let mut hmm = Hmm {
            mu,
            sigma,
            trans,
            loglik,
            iterations,
            p_turbulent: gamma1,
        };
        if hmm.sigma[0] > hmm.sigma[1] {
            hmm.mu.swap(0, 1);
            hmm.sigma.swap(0, 1);
            hmm.trans = [
                [hmm.trans[1][1], hmm.trans[1][0]],
                [hmm.trans[0][1], hmm.trans[0][0]],
            ];
            for p in &mut hmm.p_turbulent {
                *p = 1.0 - *p;
            }
        }
        hmm
    }
}

/// The transparent threshold classification from `data/history/README.md`: month `t`
/// is turbulent when the standard deviation of the `window` returns ending at `t` is
/// above the `quantile` of that statistic over the sample. Months before the first
/// full window are calm.
pub(super) fn threshold_classification(returns: &[f64], window: usize, quantile: f64) -> Vec<bool> {
    let n = returns.len();
    if n < window + 1 {
        return vec![false; n];
    }
    let trailing: Vec<f64> = (window - 1..n)
        .map(|t| mean_sd(returns[t + 1 - window..=t].iter().copied()).1)
        .collect();
    let mut sorted = trailing.clone();
    sorted.sort_by(f64::total_cmp);
    let pos = quantile * (sorted.len() - 1) as f64;
    let lo = pos.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    let threshold = sorted[lo] + (pos - lo as f64) * (sorted[hi] - sorted[lo]);
    let mut out = vec![false; n];
    for (k, &sd) in trailing.iter().enumerate() {
        out[window - 1 + k] = sd > threshold;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    #[test]
    fn moments_and_correlation_by_hand() {
        let a = [1.0, 2.0, 3.0, 4.0, f64::NAN];
        let (m, sd, n) = mean_sd(a.iter().copied());
        assert_eq!((m, n), (2.5, 4));
        assert!((sd - (5.0f64 / 3.0).sqrt()).abs() < 1e-15);
        let b = [2.0, 4.0, 6.0, 8.0, 1.0];
        assert!((corr(&a, &b) - 1.0).abs() < 1e-12);
        let c = [4.0, 3.0, 2.0, 1.0, 0.0];
        assert!((corr(&a, &c) + 1.0).abs() < 1e-12);
        assert_eq!(mean_sd([].iter().copied()), (0.0, 0.0, 0));
        // x = 1,2,3,4: lag-1 autocorrelation = (-1.5*-0.5 + -0.5*0.5 + 0.5*1.5) / 5 = 0.25.
        assert!((autocorr(&a[..4], 1) - 0.25).abs() < 1e-15);
    }

    #[test]
    fn cholesky_and_solver_by_hand() {
        let a = [[4.0, 2.0], [2.0, 5.0]];
        let l = cholesky(&a).unwrap();
        assert_eq!(l, [[2.0, 0.0], [1.0, 2.0]]);
        assert!(cholesky(&[[1.0, 2.0], [2.0, 1.0]]).is_none());
        // An impossible "correlation" matrix is shrunk until it factors.
        let bad = [[1.0, 0.9, -0.9], [0.9, 1.0, 0.9], [-0.9, 0.9, 1.0]];
        let l = correlation_cholesky(&bad);
        assert!((l[0][0] - 1.0).abs() < 1e-12 && l[2][2] > 0.0);
        let x = solve([[2.0, 1.0], [1.0, 3.0]], [5.0, 10.0]);
        assert!((x[0] - 1.0).abs() < 1e-12 && (x[1] - 3.0).abs() < 1e-12);
        assert_eq!(solve([[1.0, 2.0], [2.0, 4.0]], [1.0, 1.0]), [0.0, 0.0]);
    }

    #[test]
    fn ols_recovers_an_exact_line() {
        let x: Vec<[f64; 2]> = (0..10).map(|i| [1.0, f64::from(i)]).collect();
        let y: Vec<f64> = (0..10).map(|i| 3.0 - 0.5 * f64::from(i)).collect();
        let (beta, sd) = ols(&x, &y);
        assert!((beta[0] - 3.0).abs() < 1e-10 && (beta[1] + 0.5).abs() < 1e-10);
        assert!(sd < 1e-9);
    }

    #[test]
    fn arma_moment_fit_recovers_a_simulated_process() {
        // Persistent component plus noise in innovations form.
        let truth = Arma11 {
            phi: 0.95,
            gain: 0.30,
        };
        let mut rng = Rng::from_seed(3);
        let mut m = 0.0;
        let x: Vec<f64> = (0..400_000)
            .map(|_| {
                let v = rng.normal();
                let x = m + v;
                m = truth.phi * m + truth.gain * v;
                x
            })
            .collect();
        let fit = Arma11::from_autocorrelations(autocorr(&x, 1), autocorr(&x, 12), 12);
        assert!((fit.phi - truth.phi).abs() < 0.01, "{fit:?}");
        assert!((fit.gain - truth.gain).abs() < 0.03, "{fit:?}");
        // The filter with the true parameters recovers the surprises exactly.
        let (ms, vs) = truth.filter(&x[..100], 0.0);
        assert_eq!(ms.len(), 101);
        assert!((vs[50] - (x[50] - ms[50])).abs() < 1e-15);
        // Degenerate inputs fall back to AR(1).
        let ar = Arma11::from_autocorrelations(0.4, -0.1, 12);
        assert_eq!((ar.phi, ar.gain), (0.4, 0.4));
        assert_eq!(Arma11::from_autocorrelations(-0.3, 0.1, 12).phi, 0.0);
    }

    #[test]
    fn hmm_recovers_two_simulated_regimes() {
        let mut rng = Rng::from_seed(9);
        let (mu, sigma) = ([0.01, -0.02], [0.03, 0.10]);
        let p = [0.03, 0.12];
        let mut state = 0usize;
        let mut truth = Vec::new();
        let x: Vec<f64> = (0..20_000)
            .map(|_| {
                if rng.bernoulli(p[state]) {
                    state = 1 - state;
                }
                truth.push(state == 1);
                rng.normal_with(mu[state], sigma[state])
            })
            .collect();
        let init = threshold_classification(&x, 12, 0.75);
        let hmm = Hmm::fit(&x, &init);
        assert!((hmm.mu[0] - mu[0]).abs() < 0.002, "{hmm:?}");
        assert!((hmm.mu[1] - mu[1]).abs() < 0.006);
        assert!((hmm.sigma[0] - sigma[0]).abs() < 0.002);
        assert!((hmm.sigma[1] - sigma[1]).abs() < 0.005);
        assert!((hmm.trans[0][1] - p[0]).abs() < 0.008);
        assert!((hmm.trans[1][0] - p[1]).abs() < 0.03);
        assert!((hmm.stationary_turbulent() - 0.2).abs() < 0.03);
        let agree = hmm
            .p_turbulent
            .iter()
            .zip(&truth)
            .filter(|(p, t)| (**p > 0.5) == **t)
            .count();
        assert!(agree as f64 / x.len() as f64 > 0.9);
    }

    #[test]
    fn threshold_classification_flags_the_volatile_stretch() {
        let mut x = vec![0.01; 60];
        for (i, v) in x.iter_mut().enumerate().skip(40).take(12) {
            *v = if i % 2 == 0 { 0.1 } else { -0.1 };
        }
        let c = threshold_classification(&x, 12, 0.75);
        assert!(!c[20] && c[51]);
        assert!(c.iter().filter(|b| **b).count() < 30);
        assert_eq!(threshold_classification(&x[..5], 12, 0.75), vec![false; 5]);
    }
}
