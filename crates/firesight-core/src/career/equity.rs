//! Opt-in shapes of employer stock (`incomes[].kind.equity[].company`): fat tails, a
//! ruin hazard and a link to the job for a listed employer; the timing and the top
//! of the size of a private company's exit.
//!
//! Responsibility: the pure mechanisms behind `tails: fat`, `ruin`, `layoff_link`,
//! `exit_timing: lognormal` and `exit_size: pareto_tail`. Each is compiled once per
//! job in [`super::JobModel::with_params`] and only read on the path. All are off
//! by default, and with them off [`super::JobModel::step`] does exactly the
//! arithmetic it did before they existed.
//!
//! Contract:
//!
//! * No new random numbers. [`super::JobModel::step`] draws four words a month from
//!   `Stream::EquityComp` for every job whatever its company: stock, liquidity,
//!   class, multiple. The default reads the liquidity, class and multiple words
//!   only for a private company, so for a listed one they are drawn and discarded.
//!   - `tails: fat` turns the stock word into a fat-tailed factor in place of the
//!     lognormal one;
//!   - `ruin` compares the liquidity word of a listed company with the monthly
//!     chance of a total loss;
//!   - `exit_timing: lognormal` compares the liquidity word of a private company
//!     with a hazard that depends on how long it has waited, in place of a
//!     constant;
//!   - `exit_size: pareto_tail` turns the multiple word into a truncated Pareto
//!     multiple when the outcome class is the large one.
//! * `layoff_link` draws nothing either. It changes the threshold the job's own
//!   layoff word (`Stream::Career`) is compared with, using the stock's history up
//!   to the month before, and it reads this month's liquidity word, which decides
//!   this month's ruin. No stream is shared: the equity words are drawn by the job
//!   itself and read twice.
//! * Nothing here allocates after compilation or calls a rejection sampler.
//!
//! # Fat tails (`tails: fat`)
//!
//! The default monthly company-specific factor is `exp(s z - s^2 / 2)`, `z ~ N(0,
//! 1)`. Here it is the single-stock sleeve's fat factor
//! ([`crate::economy::fat_stock_shock`]): `exp` of a scaled unit-variance
//! Student-t(4), limited to eight standard deviations and to a factor of five,
//! rescaled to mean exactly 1 and solved to the standard deviation of the lognormal
//! factor it replaces. Mean and volatility of the price are unchanged.
//!
//! # Ruin (`ruin`)
//!
//! With `h` the yearly chance (stated, or read from the three presets of
//! `equity_compensation.single_public_stock.defaults` by the stock's own
//! volatility, as the single-stock sleeve does), each month the price goes to zero
//! for good with probability `q = 1 - (1 - h)^(1/12)`, and otherwise the month's
//! return is multiplied by `1 / (1 - q)`. The expected price is unchanged; the
//! typical price is higher in the paths that survive. After a ruin unvested grants
//! are worth nothing and refresh grants stop. When the person is next rehired
//! after a layoff the stock starts again at one real dollar a share (a new
//! employer), and refresh grants resume.
//!
//! # The link to the job (`layoff_link`)
//!
//! The job's monthly layoff chance is multiplied by `hazard_multiplier` (3) while
//! the sum of the last twelve months of the stock's company-specific log returns,
//! up to last month, is below `ln(1 + crash_return)` (a fall of more than 40%). A
//! ruin lays the person off in the same month. This is deliberately not
//! mean-preserving: it adds layoffs. It needs the job's layoffs to be simulated
//! (`risk.level` other than `none`, `assumptions.career_risk` on).
//!
//! # Exit timing (`exit_timing: lognormal`)
//!
//! The wait `T` in years is lognormal with the entered median and log standard
//! deviation `sigma`. With `S(t)` its survival function, the chance of the outcome
//! in the `m`-th month of waiting is the discrete hazard
//!
//! ```text
//! h(m) = 1 - S((m + 1) / 12) / S(m / 12)
//! ```
//!
//! tabulated at compilation. The clock counts months that are not recession
//! months (the default also blocks the outcome in a recession month), so the
//! number of such months waited is exactly the rounded-up lognormal.
//!
//! # Exit size (`exit_size: pareto_tail`)
//!
//! The default large class pays a multiple log-uniform on `[low, high]` (4 to 20).
//! Here it pays a Pareto multiple from `low`, truncated at `cap` (200):
//!
//! ```text
//! M = low * (1 - u D)^(-1 / alpha),   D = 1 - (low / cap)^alpha,   u uniform
//! ```
//!
//! `alpha` is solved by bisection so that `E[M]` equals the log-uniform mean
//! `(high - low) / ln(high / low)`: the expected payout of the class, and so of the
//! company, is unchanged. The class probabilities and the other classes are
//! untouched.

use serde_json::Value;

use super::{CareerClimate, JobState, StockModel};
use crate::data::Data;
use crate::economy;
use crate::math;
use crate::model::{ExitSize, ExitTiming, StockRuin, TailShape};
use crate::rng::normal_from_bits;
use crate::sampler::{self, ShockTable};

/// Months of company-specific returns the link looks back over.
pub const LINK_MONTHS: usize = 12;
/// Months covered by the exit-hazard table (100 years of waiting); beyond it the
/// last value is used.
const EXIT_TABLE_MONTHS: usize = 1200;
/// A survival probability below this is treated as zero: the outcome happens.
const SURVIVAL_FLOOR: f64 = 1e-300;
/// Largest yearly ruin chance accepted (validation allows less).
const MAX_ANNUAL_RUIN: f64 = 0.9;

/// The employer-stock part of `business_and_equity` in
/// `data/actuarial/params.json`, plus the single-stock presets the ruin hazard is
/// read from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EquityParams {
    /// Layoff-hazard multiplier while the employer's stock has crashed.
    pub link_hazard_multiplier: f64,
    /// The company-specific twelve-month return below which the stock counts as
    /// crashed (-0.4 = a fall of more than 40%).
    pub link_crash_return: f64,
    /// Largest multiple of paper value a large private outcome can pay under
    /// `exit_size: pareto_tail`.
    pub pareto_cap_multiple: f64,
    /// `(idiosyncratic annual volatility, annual ruin hazard)` of the three
    /// single-stock presets, sorted by volatility.
    pub ruin_presets: [(f64, f64); 3],
}

fn num(root: &Value, path: &[&str]) -> f64 {
    let mut v = root;
    for key in path {
        v = &v[*key];
    }
    v.as_f64().unwrap_or_else(|| {
        panic!(
            "data/actuarial/params.json: expected a number at {}",
            path.join(".")
        )
    })
}

impl EquityParams {
    /// Reads the parameters from the embedded data.
    ///
    /// # Panics
    /// If `params.json` lacks one of the expected numbers (a build error: the file
    /// is embedded and covered by this module's tests).
    pub fn from_data(data: &Data) -> EquityParams {
        let a = &data.actuarial;
        let link = |k: &str| num(a, &["business_and_equity", "employer_stock_layoff_link", k]);
        EquityParams {
            link_hazard_multiplier: link("hazard_multiplier"),
            link_crash_return: link("crash_return_12m"),
            pareto_cap_multiple: num(
                a,
                &[
                    "business_and_equity",
                    "private_exit_pareto",
                    "cap_multiple_of_paper_value",
                ],
            ),
            ruin_presets: economy::stock_presets_from_json(a),
        }
    }
}

/// `tails: fat` and `ruin` for a listed employer.
#[derive(Clone, Debug)]
pub(super) struct PublicShape {
    /// The fat company-specific factor, mean exactly 1; `None` keeps the lognormal.
    pub(super) fat: Option<ShockTable>,
    /// Monthly chance of a total loss; 0 for none.
    pub(super) ruin_monthly: f64,
    /// `1 / (1 - ruin_monthly)`.
    pub(super) survivor_boost: f64,
    /// The trailing company-specific return is tracked (`layoff_link`).
    track: bool,
}

/// `layoff_link` for a listed employer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Link {
    pub(super) multiplier: f64,
    /// `ln(1 + crash_return)`.
    pub(super) log_threshold: f64,
    /// As [`PublicShape::ruin_monthly`].
    ruin_monthly: f64,
}

impl Link {
    /// Whether an employed person is laid off this month. `hazard` is the job's
    /// ordinary monthly chance, `u_layoff` its layoff word and `u_ruin` this
    /// month's liquidity word, which also decides the ruin in
    /// [`PublicShape::step`].
    #[inline]
    pub(super) fn laid_off(&self, s: &JobState, hazard: f64, u_layoff: f64, u_ruin: f64) -> bool {
        if s.stock_ruined || u_ruin < self.ruin_monthly {
            return true;
        }
        let h = if s.idio_12m < self.log_threshold {
            (hazard * self.multiplier).min(1.0)
        } else {
            hazard
        };
        u_layoff < h
    }
}

impl PublicShape {
    /// One month of a listed employer's price with at least one option on.
    #[inline]
    pub(super) fn step(
        &self,
        stock: &StockModel,
        s: &mut JobState,
        c: &CareerClimate,
        rehired: bool,
        r_stock: u64,
        u_ruin: f64,
    ) {
        if s.stock_ruined {
            if !rehired {
                return;
            }
            // A new employer: its stock starts at one real dollar a share.
            s.stock_ruined = false;
            s.stock_price = c.price_level;
            s.idio_log = [0.0; LINK_MONTHS];
            s.idio_12m = 0.0;
        }
        if u_ruin < self.ruin_monthly {
            s.stock_ruined = true;
            s.stock_price = 0.0;
            return;
        }
        let market = (1.0 + stock.beta * c.equity_return).max(0.0);
        let (idio, log_idio) = match &self.fat {
            Some(table) => {
                let f = table.sample(r_stock);
                (f, if self.track { math::ln(f) } else { 0.0 })
            }
            None if stock.idio_log_sd > 0.0 => {
                let sd = stock.idio_log_sd;
                let e = sd * normal_from_bits(r_stock) - 0.5 * sd * sd;
                (math::exp(e), e)
            }
            None => (1.0, 0.0),
        };
        // The same product as the default; the boost only when ruin is possible.
        let mut gross = market * idio;
        if self.ruin_monthly > 0.0 {
            gross *= self.survivor_boost;
        }
        s.stock_price *= gross;
        if self.track {
            s.idio_log[s.steps as usize % LINK_MONTHS] = log_idio;
            let mut total = 0.0;
            for x in &s.idio_log {
                total += x;
            }
            s.idio_12m = total;
        }
    }
}

/// Annual hazard of a total loss for a stock with idiosyncratic annual volatility
/// `idio_vol`.
pub(super) fn ruin_annual(ruin: StockRuin, idio_vol: f64, params: &EquityParams) -> f64 {
    let h = match ruin {
        StockRuin::Never => 0.0,
        StockRuin::ByVolatility => economy::stock_ruin_hazard(&params.ruin_presets, idio_vol),
        StockRuin::Stated { annual_probability } => annual_probability,
    };
    if h.is_finite() {
        h.clamp(0.0, MAX_ANNUAL_RUIN)
    } else {
        0.0
    }
}

/// A truncated Pareto multiple for the large outcome class.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Pareto {
    pub(super) low: f64,
    pub(super) alpha: f64,
    /// `1 - (low / cap)^alpha`.
    span: f64,
}

impl Pareto {
    /// Mean of a Pareto with index `alpha` from `low`, truncated at `cap`.
    pub(super) fn mean_of(low: f64, cap: f64, alpha: f64) -> f64 {
        let span = 1.0 - math::pow(low / cap, alpha);
        if (alpha - 1.0).abs() < 1e-9 {
            return low * math::ln(cap / low) / span;
        }
        alpha * math::pow(low, alpha) / span
            * (math::pow(cap, 1.0 - alpha) - math::pow(low, 1.0 - alpha))
            / (1.0 - alpha)
    }

    /// The Pareto from `low` to `cap` whose mean is the mean of a log-uniform
    /// multiple on `[low, high]`. `None` when the ranges leave no room for a tail.
    pub(super) fn matching(low: f64, high: f64, cap: f64) -> Option<Pareto> {
        let ok = low.is_finite() && high.is_finite() && cap.is_finite();
        if !ok || low <= 0.0 || high <= low || cap <= high {
            return None;
        }
        let target = (high - low) / math::ln(high / low);
        // The mean falls from the log-uniform mean on [low, cap] (alpha near 0) to
        // `low` (alpha large) as alpha rises: bisect.
        let (mut lo, mut hi) = (1e-3, 50.0);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if Self::mean_of(low, cap, mid) > target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let alpha = 0.5 * (lo + hi);
        Some(Pareto {
            low,
            alpha,
            span: 1.0 - math::pow(low / cap, alpha),
        })
    }

    /// The multiple at `u` in `[0, 1)`.
    #[inline]
    pub(super) fn quantile(&self, u: f64) -> f64 {
        self.low * math::pow(1.0 - u * self.span, -1.0 / self.alpha)
    }

    /// The largest multiple (the cap).
    #[cfg(test)]
    pub(super) fn cap(&self) -> f64 {
        self.quantile(1.0)
    }
}

/// Monthly discrete hazard of a lognormal wait with the given median (years) and
/// log standard deviation, for months 0, 1, ... of waiting.
pub(super) fn exit_hazard_table(median_years: f64, sigma: f64) -> Vec<f64> {
    let finite = median_years.is_finite() && sigma.is_finite();
    let median = if finite { median_years.max(1e-3) } else { 5.0 };
    let sigma = if finite { sigma.max(1e-3) } else { 0.5 };
    let log_median = math::ln(median);
    // P(T > years).
    let survival = |years: f64| {
        if years <= 0.0 {
            1.0
        } else {
            math::normal_cdf(-(math::ln(years) - log_median) / sigma)
        }
    };
    let mut table = Vec::with_capacity(EXIT_TABLE_MONTHS);
    let mut before = 1.0;
    for m in 0..EXIT_TABLE_MONTHS {
        let after = survival((m + 1) as f64 / 12.0);
        table.push(if before > SURVIVAL_FLOOR {
            (1.0 - after / before).clamp(0.0, 1.0)
        } else {
            1.0
        });
        before = after;
    }
    table
}

/// The opt-in equity shapes one job has switched on. All `None` is the default
/// model.
#[derive(Clone, Debug, Default)]
pub(super) struct EquityShapes {
    pub(super) public: Option<PublicShape>,
    pub(super) link: Option<Link>,
    /// Monthly exit hazard by months waited (`exit_timing: lognormal`).
    pub(super) exit_hazard: Option<Vec<f64>>,
    pub(super) pareto: Option<Pareto>,
}

impl EquityShapes {
    /// Shapes of a listed employer. `layoffs` says whether the job's layoffs are
    /// simulated.
    pub(super) fn public(
        stock: &StockModel,
        tails: TailShape,
        ruin: StockRuin,
        layoff_link: bool,
        layoffs: bool,
        params: &EquityParams,
    ) -> EquityShapes {
        let sigma = stock.idio_log_sd;
        let fat = (tails == TailShape::Fat && sigma > 0.0).then(|| {
            let mut lognormal = sampler::normal().transformed(|x| math::exp(sigma * x));
            let k = 1.0 / lognormal.mean();
            lognormal.scale(k);
            economy::fat_stock_shock(sigma, &lognormal)
        });
        let ruin_monthly =
            super::business::monthly_probability(ruin_annual(ruin, sigma * 12f64.sqrt(), params));
        let link = (layoff_link && layoffs).then(|| Link {
            multiplier: if params.link_hazard_multiplier.is_finite() {
                params.link_hazard_multiplier.max(1.0)
            } else {
                1.0
            },
            log_threshold: math::ln_1p(params.link_crash_return.clamp(-0.99, 0.0)),
            ruin_monthly,
        });
        let on = fat.is_some() || ruin_monthly > 0.0 || link.is_some();
        EquityShapes {
            public: on.then(|| PublicShape {
                fat,
                ruin_monthly,
                survivor_boost: 1.0 / (1.0 - ruin_monthly),
                track: link.is_some(),
            }),
            link,
            ..EquityShapes::default()
        }
    }

    /// Shapes of a private employer. `large_range` is the `[low, high]` multiple
    /// range of the large outcome class.
    pub(super) fn private(
        timing: ExitTiming,
        size: ExitSize,
        large_range: [f64; 2],
        params: &EquityParams,
    ) -> EquityShapes {
        EquityShapes {
            exit_hazard: match timing {
                ExitTiming::ConstantHazard => None,
                ExitTiming::Lognormal {
                    median_years,
                    sigma,
                } => Some(exit_hazard_table(median_years, sigma)),
            },
            pareto: match size {
                ExitSize::Classes => None,
                ExitSize::ParetoTail => {
                    Pareto::matching(large_range[0], large_range[1], params.pareto_cap_multiple)
                }
            },
            ..EquityShapes::default()
        }
    }
}

#[cfg(test)]
#[path = "equity_tests.rs"]
mod tests;
