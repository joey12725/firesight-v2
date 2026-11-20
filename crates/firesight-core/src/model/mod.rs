//! The profile schema: everything a household tells FIRESight about itself.
//!
//! These types are the single source of truth. `schemars` derives the JSON Schema the
//! web UI and the AI tools validate against, so every field's doc comment is user-facing
//! help text: keep it to one or two plain sentences with units and a typical value.
//!
//! Conventions (see `docs/architecture.md` section 2):
//!
//! * Money is in today's dollars (dollars at `profile.start`).
//! * Rates are annual decimal fractions (`0.04`), never percents.
//! * Structs are `#[serde(default, deny_unknown_fields)]`: any field may be omitted,
//!   misspelt fields are rejected. The only required fields are birth months.
//! * Enums that carry data are internally tagged with `"type"`; enums without data are
//!   plain snake_case strings (`"female"`, not `{"type": "female"}`).
//! * Patchable list elements carry a stable string `id`.

mod accounts;
mod assumptions;
mod debt;
mod events;
mod examples;
mod household;
mod income;
mod portfolio;
mod profile;
mod property;
mod retirement;
mod spending;
mod time;
mod uncertain;

pub use accounts::*;
pub use assumptions::*;
pub use debt::*;
pub use events::*;
pub use examples::{Example, EXAMPLE_START};
pub use household::*;
pub use income::*;
pub use portfolio::*;
pub use profile::*;
pub use property::*;
pub use retirement::*;
pub use spending::*;
pub use time::*;
pub use uncertain::*;
