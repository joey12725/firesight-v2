//! One-draw shock samplers for the whole crate.
//!
//! The tables are built and owned by `crate::economy::sampler` (see its module
//! documentation for the layout and the truncation); this module only re-exports them
//! from a neutral place so that careers, health, events and the rest can draw
//! fat-tailed shocks without depending on the economy's internals. Nothing here
//! changes a number the economy produces.
//!
//! One 64-bit word in, one variate out, no rejection:
//!
//! ```ignore
//! let z = crate::sampler::normal().table.sample(rng.next_u64());      // N(0, 1)
//! let t = crate::sampler::student_t4().table.sample(rng.next_u64());  // t(4), variance 1
//! // A transformed table, built once in `Plan::compile`, never per path:
//! let jump: ShockTable = crate::sampler::student_t6().transformed(|x| math::exp(0.2 * x));
//! let x = jump.sample(rng.next_u64());
//! ```
//!
//! * [`normal`], [`student_t4`], [`student_t6`], [`student_t`] return `&'static`
//!   [`UnitShock`]s: symmetric, mean 0, variance exactly 1 as sampled, truncated at a
//!   tail probability of about 2.4e-7 per side.
//! * [`UnitShock::transformed`] derives the table of `f(x)`; [`ShockTable::mean`],
//!   [`ShockTable::second_moment`] and [`ShockTable::scale`] let a caller normalise it
//!   (for example to mean exactly 1).
//! * [`ShockTable::build`] makes a table from any symmetric distribution's
//!   half-quantile function; [`ShockTable::constant`] is the degenerate table.
//! * [`normal_quantile`] is the normal inverse CDF polished to near machine precision
//!   (slower than `rng::inverse_normal_cdf`; meant for building tables).
//!
//! For a distribution a user supplies in the profile, use `crate::dist` instead.

// Re-exported for the modules that will draw from these tables; not every item has a
// caller outside `economy` yet.
#[allow(unused_imports)]
pub(crate) use crate::economy::sampler::{
    normal, normal_quantile, student_t, student_t4, student_t6, student_t_cdf_even, ShockTable,
    UnitShock,
};
