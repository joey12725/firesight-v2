//! Lumpy home maintenance: routine upkeep plus random major repairs.
//!
//! A home's `maintenance_rate` is its expected annual cost as a share of value. In
//! the lumpy model [`CAPITAL_SHARE`] of that expectation is spent on major
//! components that wear out and are replaced; the rest stays a smooth routine charge.
//! Each component has a Weibull lifetime (hazard rising with the years since it was
//! last replaced). Its replacement cost is sized so that
//!
//! ```text
//! cost_j = w_j * CAPITAL_SHARE * maintenance_rate * V * L_j
//! ```
//!
//! where `V` is the home's value when the repair falls due (the structure is taken to
//! be worth a fixed share of the home, so a repair costs a fixed share of its value,
//! which is also what the smooth rate assumes), `w_j` the component's share of the
//! capital budget and `L_j` its mean life. Replacing component `j` once every `L_j`
//! years on average then costs `w_j * CAPITAL_SHARE * maintenance_rate * V` a year, and
//! the sum over components is `CAPITAL_SHARE * maintenance_rate * V`: the same expected
//! cost as the smooth rate. (Repairs therefore get cheaper in a price crash, which real
//! repairs do not; the alternative, tying them to CPI alone, makes the lumpy home
//! cost about 8% less than the smooth one over 30 years because real house prices
//! rise.)
//!
//! Randomness: [`DRAWS_PER_MONTH`] 64-bit words a month from `Stream::Maintenance`,
//! whether the property is owned or not.

use crate::math;

/// Number of major components.
pub const N_COMPONENTS: usize = 5;
/// 64-bit words drawn each month: one failure draw and one initial-age draw per
/// component.
pub const DRAWS_PER_MONTH: usize = 2 * N_COMPONENTS;

/// `(name, mean life in years, share of the capital budget)`.
///
/// Lives: roof about 20 to 30 years for asphalt shingle, furnace 15 to 20, tank
/// water heater 8 to 12, per the NAHB and Bank of America Home Equity "Study of Life
/// Expectancy of Home Components" (2007) as reported by secondary sources (the
/// original table was not re-read; see params.json `housing_realism.maintenance`).
/// The budget shares are a JUDGEMENT.
pub const COMPONENTS: [(&str, f64, f64); N_COMPONENTS] = [
    ("roof", 25.0, 0.30),
    ("heating and cooling", 17.0, 0.25),
    ("water heater", 11.0, 0.05),
    ("exterior paint and siding", 12.0, 0.20),
    ("appliances and flooring", 13.0, 0.20),
];

/// Share of `maintenance_rate` that is capital replacement. The research file's
/// `maintenance_pct_of_value` gives 1% routine plus 0.5% capital replacement (one
/// third); half is used here so that the lumps are visible at the default 1% rate.
/// JUDGEMENT.
pub const CAPITAL_SHARE: f64 = 0.5;
/// The Weibull shape of every component's lifetime (3 gives a coefficient of
/// variation of about 0.36: failures cluster around the mean life). JUDGEMENT.
pub const WEIBULL_SHAPE: i32 = 3;
/// Gamma(1 + 1/3), so that a Weibull of shape 3 and scale `L / GAMMA` has mean `L`.
const GAMMA_1_PLUS_INV_SHAPE: f64 = 0.892_979_511_569_249_2;
/// A home owned at the start has components of unknown age: each is drawn uniform on
/// `[0, INITIAL_AGE_SPAN * L]`. The span of 1.13 gives the mean age of a stationary
/// renewal process with this shape (about 0.565 L).
pub const INITIAL_AGE_SPAN: f64 = 1.13;

const F64_UNIT: f64 = 1.0 / (1u64 << 53) as f64;

/// A property's component ages and repair sizing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MaintState {
    /// True once the ages have been set (the first month the property is owned).
    pub init: bool,
    /// Years since each component was last replaced.
    pub age: [f64; N_COMPONENTS],
}

/// Probability that a component of mean life `life` that is `age` years old fails
/// within the next month.
pub fn monthly_failure_probability(life: f64, age: f64) -> f64 {
    let scale = life / GAMMA_1_PLUS_INV_SHAPE;
    let a0 = age / scale;
    let a1 = (age + 1.0 / 12.0) / scale;
    1.0 - math::exp(math::powi(a0, WEIBULL_SHAPE) - math::powi(a1, WEIBULL_SHAPE))
}

/// Replacement cost of component `i` as a share of the home's value, for an annual
/// `maintenance_rate`.
pub fn repair_share(i: usize, maintenance_rate: f64) -> f64 {
    let (_, life, weight) = COMPONENTS[i];
    weight * CAPITAL_SHARE * maintenance_rate * life
}

#[inline]
fn unit(bits: u64) -> f64 {
    (bits >> 11) as f64 * F64_UNIT
}

/// Advances one month and returns the nominal cost of the repairs that fall in it.
/// `words` are this month's [`DRAWS_PER_MONTH`] draws (the first `N_COMPONENTS`
/// decide failures, the rest set the initial ages). Call only while the property is
/// owned; the caller must still have drawn the words otherwise.
pub fn advance(
    st: &mut MaintState,
    maintenance_rate: f64,
    value: f64,
    words: &[u64; DRAWS_PER_MONTH],
) -> f64 {
    if !st.init {
        st.init = true;
        for (i, (_, life, _)) in COMPONENTS.iter().enumerate() {
            st.age[i] = INITIAL_AGE_SPAN * life * unit(words[N_COMPONENTS + i]);
        }
    }
    let mut cost = 0.0;
    for (i, (_, life, _)) in COMPONENTS.iter().enumerate() {
        let p = monthly_failure_probability(*life, st.age[i]);
        if unit(words[i]) < p {
            cost += repair_share(i, maintenance_rate) * value;
            st.age[i] = 0.0;
        } else {
            st.age[i] += 1.0 / 12.0;
        }
    }
    cost
}
