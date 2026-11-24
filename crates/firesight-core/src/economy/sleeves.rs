//! Opt-in shapes for the derived asset sleeves: fat tails for the single stock and
//! the custom asset, crypto outlooks, token types and volatility cycles, and loss
//! events on the custom asset.
//!
//! Responsibility: everything `portfolio.single_stock.tails`,
//! `portfolio.single_stock.ruin_hazard`, `portfolio.crypto` and
//! `portfolio.custom_asset.{tails, loss_event}` add to the generator. All are off by
//! default, and then nothing here changes a number: the tables and parameters built
//! are the ones the generator always built.
//!
//! Contract:
//!
//! * Fat tails, the stock's own ruin hazard, the crypto outlook and the token type
//!   draw nothing new. They change the table or the threshold a word that is drawn
//!   anyway is read with.
//! * Crypto cycles and custom loss events draw from the `AssetExt` stream:
//!   [`WORDS_PER_MONTH`] = 3 words every month when either is on (the cycle word,
//!   whether a loss happens, its size; all three always drawn), none when both are
//!   off. The count depends only on the plan. Switching one of the two on does not
//!   move the other's words, and neither moves a word of `Market` or `Inflation`.
//!
//! # Single stock (`tails: fat`)
//!
//! The stock's own monthly factor is `L = exp(s x) / E[exp(s x)]` with `x` a
//! unit-variance Student-t(4) and `s x` limited to `+/- min(8 s, ln 5)`: a company's
//! own news moves it by at most a factor of five in a month. `E[L] = 1` exactly, as
//! for the lognormal default, so the expected return is unchanged. `s` is chosen so
//! that the standard deviation of `L` equals that of the lognormal factor it
//! replaces: same mean, same volatility, more weight in the middle and in the far
//! tails. The same word of the `Market` stream is used.
//!
//! # Crypto
//!
//! The outlook picks the expected real return and the token type the collapse
//! hazard from `crypto.model` in `data/actuarial/params.json`; both feed the same
//! formula as before.
//!
//! With `dynamics: cycles` the sleeve has two volatility states, calm and wild, and
//! moves between them as a Markov chain: a calm month is followed by a wild one
//! with probability `1 / calm_months`, a wild month by a calm one with probability
//! `1 / wild_months`, and the first month is drawn from the chain's long-run mix
//! (`wild_months / (calm_months + wild_months)` wild). In state `k` the shock is
//! `exp(s_k x) / E[exp(s_k x)]` with the same `x` (the same `Market` word) as the
//! steady model and `s_k x` limited to the steady model's cap, eight steady
//! standard deviations. `s_wild = ratio x s_calm`, and `s_calm` is solved so that
//! the long-run variance of the log shock equals the steady model's. Each state's
//! shock has mean exactly 1, so the expected return is the outlook's in every month
//! of either state: the cycle moves risk around in time and says nothing about
//! direction.
//!
//! # Custom asset
//!
//! `tails: fat` replaces the normal residual with a unit-variance Student-t(6) (the
//! table listed real estate uses), read from the same word: same mean, same
//! volatility.
//!
//! With a `loss_event`, each month has probability `q = 1 - (1 - p)^(1/12)` of a
//! loss; the month's gross return is then multiplied by `1 - L`, `L` drawn from the
//! user's distribution (limited to 0..1). When `mean_includes_losses` is true every
//! month's gross return is also multiplied by `1 / (1 - q E[L])`, so the expected
//! gross return is exactly what it was without the option; when false nothing is
//! added back and the expected return is lower by about `p E[L]` a year. The stated
//! volatility describes months without a loss; losses add variance on top.
//!
//! Simplifications: cycle states and loss events are independent of the stock
//! market and of each other; the two volatility states share one tail shape; a loss
//! event is a single month's write-down with no recovery.

use std::sync::Arc;

use crate::dist::CompiledDist;
use crate::math;
use crate::model::{CryptoAssumptions, CryptoDynamics, CryptoOutlook, CryptoToken, LossEvent};
use crate::rng::Rng;

use super::sampler::{self, ShockTable};
use super::{
    fit, monthly_hazard, probability_bits, CryptoSpec, HistBase, CRYPTO_MIN_IDIO_SHARE,
    CRYPTO_SHOCK_CAP_SD, SQRT_12, TWO_POW_32,
};

/// Words drawn from the `AssetExt` stream each month when crypto cycles or custom
/// loss events are on.
pub const WORDS_PER_MONTH: usize = 3;
/// The fat-tailed single-stock shock is limited to this many standard deviations of
/// the underlying Student-t draw, as the crypto shock is.
pub const STOCK_SHOCK_CAP_SD: f64 = CRYPTO_SHOCK_CAP_SD;
/// The fat-tailed single-stock factor is also limited to this multiple (and its
/// reciprocal) in one month, before normalisation. A judgement: a company's own
/// news rarely moves it by more than a factor of five in a month, and without the
/// limit a very volatile stock could gain a hundredfold in one.
pub const STOCK_SHOCK_MAX_FACTOR: f64 = 5.0;
/// Largest annual hazard of a total loss applied to the single stock or of a loss
/// event on the custom asset (validation allows up to 0.5).
pub const MAX_ANNUAL_HAZARD: f64 = 0.9;
/// Limits on the cycle settings, as validation applies them.
pub const CYCLE_MONTHS_RANGE: (f64, f64) = (1.0, 600.0);
/// Limits on the ratio of wild to calm volatility.
pub const CYCLE_RATIO_RANGE: (f64, f64) = (1.0, 5.0);

/// Fixed-point steps used to match a variance. The maps are close to linear, so a
/// handful is enough; the loops stop early once the match is exact to rounding.
const MATCH_STEPS: usize = 40;

/// The crypto sleeve's compiled parameters.
pub(super) struct CryptoSetup {
    pub(super) beta: f64,
    pub(super) beta_extra: f64,
    /// `exp(mu / 12)` over the means that are divided out.
    pub(super) norm: f64,
    /// Monthly collapse probability times 2^32.
    pub(super) ruin: u64,
    /// The steady shock table (in cycles mode: the steady shape with mean 1, used
    /// only when no `AssetExt` stream is supplied).
    pub(super) shock: ShockTable,
    pub(super) cycles: Option<Cycles>,
}

/// The two-state volatility chain of the crypto sleeve.
#[derive(Debug)]
pub(super) struct Cycles {
    /// Mean-one shock tables: calm, wild.
    tables: [ShockTable; 2],
    /// Probabilities times 2^32: calm to wild, wild to calm, wild in month 0.
    enter_wild: u64,
    leave_wild: u64,
    start_wild: u64,
    /// Monthly standard deviation scale of the log shock: calm, wild.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) sigma: [f64; 2],
}

/// Loss events on the custom asset.
#[derive(Debug)]
pub(super) struct Loss {
    /// Monthly probability times 2^32.
    hazard: u64,
    size: CompiledDist,
    /// Factor on every month's gross return (1 when the stated mean is before
    /// losses).
    boost: f64,
}

/// The parts of the sleeves that need the `AssetExt` stream.
#[derive(Clone, Debug, Default)]
pub(super) struct AssetExt {
    pub(super) cycles: Option<Arc<Cycles>>,
    pub(super) loss: Option<Arc<Loss>>,
}

impl AssetExt {
    pub(super) fn is_on(&self) -> bool {
        self.cycles.is_some() || self.loss.is_some()
    }
}

/// Per-path state of the asset extension while a path is generated.
pub(super) struct ExtRt<'a> {
    rng: &'a mut Rng,
    cycles: Option<&'a Cycles>,
    loss: Option<&'a Loss>,
    wild: bool,
}

impl<'a> ExtRt<'a> {
    pub(super) fn new(ext: &'a AssetExt, rng: &'a mut Rng) -> ExtRt<'a> {
        ExtRt {
            rng,
            cycles: ext.cycles.as_deref(),
            loss: ext.loss.as_deref(),
            wild: false,
        }
    }

    /// The volatility state of the month last stepped.
    #[cfg(test)]
    pub(super) fn is_wild(&self) -> bool {
        self.wild
    }

    /// One month: draws the three words, then rewrites the crypto return (cycles)
    /// and the custom return (loss events). `crypto_base` is the sleeve's gross
    /// return before its shock and `crypto_word` the `Market` word of that shock.
    #[inline]
    pub(super) fn step(
        &mut self,
        t: usize,
        crypto_base: f64,
        crypto_word: u64,
        crypto: &mut f64,
        custom: &mut f64,
    ) {
        let cycle_word = self.rng.next_u64();
        let loss_word = self.rng.next_u64();
        let size_word = self.rng.next_u64();
        if let Some(c) = self.cycles {
            let u = cycle_word >> 32;
            self.wild = if t == 0 {
                u < c.start_wild
            } else if self.wild {
                u >= c.leave_wild
            } else {
                u < c.enter_wild
            };
            *crypto = crypto_base * c.tables[usize::from(self.wild)].sample(crypto_word) - 1.0;
        }
        if let Some(l) = self.loss {
            let keep = if (loss_word >> 32) < l.hazard {
                1.0 - l.size.sample(size_word).clamp(0.0, 1.0)
            } else {
                1.0
            };
            *custom = (1.0 + *custom) * (l.boost * keep) - 1.0;
        }
    }
}

/// The crypto presets of `crypto.model` in `data/actuarial/params.json` that the
/// default does not use.
#[derive(Clone, Copy, Debug)]
pub(super) struct CryptoPresets {
    skeptic: f64,
    believer: f64,
    other_tokens_ruin: f64,
}

impl CryptoPresets {
    pub(super) fn from_json(actuarial: &serde_json::Value) -> CryptoPresets {
        let preset = |name: &str, fallback: f64| {
            super::json_f64(
                actuarial,
                &format!("/crypto/model/expected_real_return_presets/{name}"),
                fallback,
            )
            .max(-0.99)
        };
        CryptoPresets {
            skeptic: preset("skeptic", -0.05),
            believer: preset("believer", 0.06),
            other_tokens_ruin: super::json_f64(
                actuarial,
                "/crypto/model/ruin_hazard_annual/other_tokens",
                0.08,
            )
            .clamp(0.0, 1.0),
        }
    }
}

/// The crypto assumptions with the profile's outlook and token type applied.
/// `base` holds the file's default preset (neutral) and the bitcoin-or-ether hazard.
pub(super) fn crypto_spec(
    base: CryptoSpec,
    presets: &CryptoPresets,
    crypto: &CryptoAssumptions,
) -> CryptoSpec {
    let mut spec = base;
    match crypto.outlook {
        CryptoOutlook::Skeptic => spec.expected_real_return = presets.skeptic,
        CryptoOutlook::Neutral => {}
        CryptoOutlook::Believer => spec.expected_real_return = presets.believer,
    }
    if crypto.token == CryptoToken::OtherTokens {
        spec.ruin_annual = presets.other_tokens_ruin;
    }
    spec
}

/// Builds the crypto sleeve: the equity link sized for the two correlations,
/// idiosyncratic volatility for the rest of the total, and the normalisation that
/// makes the expected real return the stated one.
pub(super) fn crypto_setup(
    base: &HistBase,
    spec: &CryptoSpec,
    dynamics: CryptoDynamics,
) -> CryptoSetup {
    let sigma_c = spec.sigma_annual;
    let eq_vol = (base.eq_sd * SQRT_12).max(1e-6);
    let crypto_beta = spec.equity_corr * sigma_c / eq_vol;
    let crypto_beta_extra = (spec.drawdown_corr - spec.equity_corr).max(0.0) * sigma_c / eq_vol;
    let (_, link_sd, _) = fit::mean_sd(
        base.rows
            .iter()
            .map(|r| math::ln(base.crypto_link(crypto_beta, crypto_beta_extra, r.eq))),
    );
    let total_var = sigma_c * sigma_c / 12.0;
    let idio_var = (total_var - link_sd * link_sd).max(CRYPTO_MIN_IDIO_SHARE * total_var);
    let idio_sd = idio_var.sqrt();
    let unit = sampler::student_t(spec.dof);
    let mut shock = unit
        .transformed(|x| math::exp(idio_sd * x.clamp(-CRYPTO_SHOCK_CAP_SD, CRYPTO_SHOCK_CAP_SD)));
    // The preset fixes the expected return, not the median: the mean of the
    // equity link, the mean of the fat-tailed shock and the chance of a collapse
    // are all divided out, so that the average real return of the sleeve is the
    // stated one. With 70% volatility the typical path then loses most of its
    // value; a few gain enormously; on average the holding keeps what the
    // preset says.
    let link_arithmetic = fit::mean_sd(
        base.rows
            .iter()
            .map(|r| base.crypto_link(crypto_beta, crypto_beta_extra, r.eq)),
    )
    .0;
    let ruin = probability_bits(monthly_hazard(spec.ruin_annual));
    let survive = 1.0 - ruin as f64 / TWO_POW_32;
    let growth = math::pow(1.0 + spec.expected_real_return, 1.0 / 12.0);
    let cycles = match dynamics {
        CryptoDynamics::Steady => None,
        CryptoDynamics::Cycles {
            calm_months,
            wild_months,
            volatility_ratio,
        } => Some(Cycles::new(
            unit,
            idio_sd,
            calm_months,
            wild_months,
            volatility_ratio,
        )),
    };
    let norm = if cycles.is_some() {
        // Every table has mean exactly 1.
        let k = 1.0 / shock.mean();
        shock.scale(k);
        growth / (link_arithmetic * survive).max(1e-9)
    } else {
        growth / (link_arithmetic * shock.mean() * survive).max(1e-9)
    };
    CryptoSetup {
        beta: crypto_beta,
        beta_extra: crypto_beta_extra,
        norm,
        ruin,
        shock,
        cycles,
    }
}

fn in_range(x: f64, range: (f64, f64), fallback: f64) -> f64 {
    if x.is_finite() {
        x.clamp(range.0, range.1)
    } else {
        fallback
    }
}

impl Cycles {
    /// `idio_sd` is the steady model's monthly scale of the log shock.
    fn new(
        unit: &sampler::UnitShock,
        idio_sd: f64,
        calm_months: f64,
        wild_months: f64,
        ratio: f64,
    ) -> Cycles {
        let calm = in_range(
            calm_months,
            CYCLE_MONTHS_RANGE,
            CryptoDynamics::DEFAULT_CALM_MONTHS,
        );
        let wild = in_range(
            wild_months,
            CYCLE_MONTHS_RANGE,
            CryptoDynamics::DEFAULT_WILD_MONTHS,
        );
        let ratio = in_range(
            ratio,
            CYCLE_RATIO_RANGE,
            CryptoDynamics::DEFAULT_VOLATILITY_RATIO,
        );
        let enter_wild = probability_bits(1.0 / calm);
        let leave_wild = probability_bits(1.0 / wild);
        // The chain's long-run share of wild months, from the probabilities applied.
        let (pe, pl) = (
            enter_wild as f64 / TWO_POW_32,
            leave_wild as f64 / TWO_POW_32,
        );
        let share = if pe + pl > 0.0 { pe / (pe + pl) } else { 0.0 };
        // Variance of the log shock at scale `s`, with the steady model's cap.
        let cap = CRYPTO_SHOCK_CAP_SD * idio_sd;
        let log_var = |s: f64| {
            unit.transformed(|x| (s * x).clamp(-cap, cap))
                .second_moment()
        };
        let target = log_var(idio_sd);
        let mixed = |s: f64| (1.0 - share) * log_var(s) + share * log_var(ratio * s);
        // Without the cap the answer is exact; the cap trims the wild state a
        // little, so step up to the scale that restores the long-run variance.
        let mut s_calm = idio_sd / ((1.0 - share) + share * ratio * ratio).sqrt();
        if target > 0.0 {
            for _ in 0..MATCH_STEPS {
                let now = mixed(s_calm);
                if now <= 0.0 || !now.is_finite() {
                    break;
                }
                let k = (target / now).sqrt();
                if (k - 1.0).abs() < 1e-13 {
                    break;
                }
                s_calm = (s_calm * k).min(idio_sd);
            }
        }
        let sigma = [s_calm, ratio * s_calm];
        let table = |s: f64| {
            let mut t = unit.transformed(|x| math::exp((s * x).clamp(-cap, cap)));
            let k = 1.0 / t.mean();
            t.scale(k);
            t
        };
        Cycles {
            tables: [table(sigma[0]), table(sigma[1])],
            enter_wild,
            leave_wild,
            start_wild: probability_bits(share),
            sigma,
        }
    }
}

/// The fat-tailed single-stock factor for a monthly idiosyncratic volatility
/// `sigma > 0`: mean exactly 1 and the standard deviation of the lognormal factor
/// `lognormal` (already normalised) that it replaces.
pub(super) fn fat_stock_shock(sigma: f64, lognormal: &ShockTable) -> ShockTable {
    let unit = sampler::student_t4();
    let max_log = math::ln(STOCK_SHOCK_MAX_FACTOR);
    let build = |s: f64| {
        let cap = (STOCK_SHOCK_CAP_SD * s).min(max_log);
        let mut t = unit.transformed(|x| math::exp((s * x).clamp(-cap, cap)));
        let k = 1.0 / t.mean();
        t.scale(k);
        t
    };
    let sd = |t: &ShockTable| (t.second_moment() - 1.0).max(0.0).sqrt();
    let target = sd(lognormal);
    let mut s = sigma;
    let mut table = build(s);
    if target > 0.0 {
        for _ in 0..MATCH_STEPS {
            let now = sd(&table);
            if now <= 0.0 || !now.is_finite() {
                break;
            }
            let k = target / now;
            if (k - 1.0).abs() < 1e-13 {
                break;
            }
            // Far beyond the caps a larger scale no longer adds volatility: stop
            // within a fixed multiple of the lognormal's scale.
            s = (s * k).clamp(0.05 * sigma, 20.0 * sigma);
            table = build(s);
        }
    }
    table
}

/// Annual hazard of a total loss of the single stock: the user's when given, else
/// the preset interpolation.
pub(super) fn stock_hazard(user: Option<f64>, preset: f64) -> f64 {
    match user {
        Some(h) if h.is_finite() => h.clamp(0.0, MAX_ANNUAL_HAZARD),
        _ => preset,
    }
}

impl Loss {
    /// `None` when the event can never happen.
    pub(super) fn new(event: &LossEvent) -> Option<Loss> {
        let p = if event.annual_probability.is_finite() {
            event.annual_probability.clamp(0.0, MAX_ANNUAL_HAZARD)
        } else {
            0.0
        };
        let hazard = probability_bits(monthly_hazard(p));
        if hazard == 0 {
            return None;
        }
        let size = event.loss.compile();
        // Validation keeps the distribution inside 0..1, where limiting it changes
        // nothing and `mean()` is the mean of what is applied.
        let mean_loss = size.mean().clamp(0.0, 1.0);
        let q = hazard as f64 / TWO_POW_32;
        let boost = if event.mean_includes_losses {
            1.0 / (1.0 - q * mean_loss).max(1e-9)
        } else {
            1.0
        };
        Some(Loss {
            hazard,
            size,
            boost,
        })
    }

    /// Expected factor on a month's gross return (1 when the mean is compensated).
    #[cfg(test)]
    pub(super) fn expected_factor(&self) -> f64 {
        let q = self.hazard as f64 / TWO_POW_32;
        self.boost * (1.0 - q * self.size.mean().clamp(0.0, 1.0))
    }
}
