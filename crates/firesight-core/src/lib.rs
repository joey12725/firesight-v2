//! FIRESight core: the whole retirement model as a pure function.
//!
//! `f(profile, data, seed, path_index)` must give the same bits on native and wasm.
//! That rules out, everywhere in this crate: I/O, threads, the wall clock, global RNG,
//! `HashMap` iteration in anything that reaches a result, and `std` transcendental
//! functions (use [`math`]). See `docs/architecture.md` for the contract.
//!
//! Module map (all implemented; each module's own documentation lists what it
//! simplifies):
//!
//! | Module | Responsibility |
//! |---|---|
//! | [`model`], [`validate`], [`patch`], [`legacy`] | the profile schema, its checks, patches and v1 migration |
//! | [`rng`], [`math`], [`data`] | deterministic random streams and elementary functions, embedded datasets |
//! | [`economy`] | joint market, rate, inflation and home-price paths; stress overlays |
//! | [`tax`], [`benefits`], [`health`] | the annual return; Social Security; mortality, health and care costs |
//! | [`career`], [`housing`] | jobs and equity compensation; properties and mortgages |
//! | [`accounts`], [`decumulation`] | tax buckets and their mechanics; spending rules and the withdrawal plan |
//! | [`sim`] | `Plan::compile` and the monthly loop that ties the modules together |
//! | [`stats`] | recorders, the mergeable `Accumulator`, `Summary`, `Comparison`, `PathTrace` |
//! | [`scenario`] | sensitivity levers, the goal-seek solver, grid sweeps |

pub mod accounts;
pub mod benefits;
pub mod career;
pub mod data;
pub mod decumulation;
pub mod economy;
pub mod health;
pub mod health_states;
pub mod housing;
pub mod legacy;
pub mod longevity_links;
pub mod math;
pub mod model;
pub mod patch;
pub mod rng;
pub mod scenario;
pub mod sim;
pub mod stats;
pub mod tax;
pub mod validate;

pub use model::{AssetClass, Profile, YearMonth, N_ASSETS};

/// Maximum number of adults in a household. Per-person state is stored in fixed
/// `[T; MAX_PEOPLE]` arrays so the monthly loop never allocates.
pub const MAX_PEOPLE: usize = 2;
