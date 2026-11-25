//! A number the household is unsure about, written as a range of possible values.
//!
//! This is the profile-facing half; `crate::dist` turns it into a sampler
//! (`CompiledDist`) and holds the validation helper (`check_uncertain`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A number you are unsure about, given as a range of possible values instead of a
/// single figure. Each simulated future draws its own value from the range. Pick the
/// shape that matches what you know: `pert` for a best guess with a low and a high,
/// `quantiles` or `lognormal` for amounts that could run well above the typical
/// figure, `normal` for a plus-or-minus around an average. Every field of the chosen
/// shape is required, in the same units as the number it describes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Uncertain {
    /// Any value between `low` and `high` is equally likely. Use it when you only
    /// know the limits.
    Uniform {
        /// The lowest possible value.
        low: f64,
        /// The highest possible value. Must not be below `low`.
        high: f64,
    },
    /// A simple three-point estimate: values near `mode` are most likely and the
    /// chance falls in a straight line to zero at `low` and `high`. The average is
    /// (low + mode + high) / 3.
    Triangular {
        /// The lowest possible value.
        low: f64,
        /// The single most likely value, between `low` and `high`.
        mode: f64,
        /// The highest possible value.
        high: f64,
    },
    /// The usual "optimistic, most likely, pessimistic" estimate. Like `triangular`
    /// but smoother, with more weight near the most likely value: the average is
    /// (low + 4 x mode + high) / 6. A good default when you have a best guess and a
    /// range, for example a renovation you expect to cost 40,000 but which could be
    /// anywhere from 30,000 to 80,000.
    Pert {
        /// The lowest possible value.
        low: f64,
        /// The single most likely value, between `low` and `high`.
        mode: f64,
        /// The highest possible value.
        high: f64,
    },
    /// A bell curve: values are symmetric around `mean`, about two thirds of them
    /// within one `sd` of it. Values never fall more than 4 `sd` from the mean. It
    /// can go negative, so prefer `lognormal` or `pert` for amounts that cannot.
    Normal {
        /// The average value, which is also the middle value.
        mean: f64,
        /// The standard deviation: the typical distance from the mean. Zero means
        /// the value is always exactly `mean`.
        sd: f64,
    },
    /// A positive amount that is usually near `median` but occasionally far above
    /// it, such as a medical bill or a sale price. Values are never zero or
    /// negative, and the upper tail is capped at about the 1-in-30,000 level.
    Lognormal {
        /// The middle value: half of outcomes are below it. Must be above zero.
        median: f64,
        /// The value that outcomes stay below 9 times in 10. Must be above
        /// `median`; the further above, the longer the upper tail.
        p90: f64,
    },
    /// Three percentiles you supply: the value is below `p10` 1 time in 10, below
    /// `p50` half the time and below `p90` 9 times in 10. The two sides may differ,
    /// which gives a skewed range. Values can be negative. Outcomes beyond `p10` and
    /// `p90` follow a smooth curve through the three points, with limited tails.
    Quantiles {
        /// The 10th percentile: outcomes fall below this 1 time in 10.
        p10: f64,
        /// The 50th percentile (the median): outcomes fall below this half the time.
        p50: f64,
        /// The 90th percentile: outcomes fall below this 9 times in 10.
        p90: f64,
    },
}

impl Uncertain {
    /// A "distribution" that is always exactly `x`, for code that wants to treat a
    /// plain number and a range the same way. It serialises as a uniform range of
    /// zero width.
    pub const fn point(x: f64) -> Uncertain {
        Uncertain::Uniform { low: x, high: x }
    }

    /// The JSON tag of the variant: "uniform", "triangular", "pert", "normal",
    /// "lognormal" or "quantiles".
    pub const fn type_name(&self) -> &'static str {
        match self {
            Uncertain::Uniform { .. } => "uniform",
            Uncertain::Triangular { .. } => "triangular",
            Uncertain::Pert { .. } => "pert",
            Uncertain::Normal { .. } => "normal",
            Uncertain::Lognormal { .. } => "lognormal",
            Uncertain::Quantiles { .. } => "quantiles",
        }
    }
}
