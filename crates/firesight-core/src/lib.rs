//! FIRESight core: the whole retirement model as a pure function.
//!
//! `f(profile, data, seed, path_index)` must give the same bits on native and wasm.
//! That rules out, everywhere in this crate: I/O, threads, the wall clock, global RNG,
//! `HashMap` iteration in anything that reaches a result, and `std` transcendental
//! functions (use [`math`]). See `docs/architecture.md` for the contract.
//!
//! Module map (owners fill the stub modules in parallel; the public signatures in each
//! stub are the agreed interface, change them only with the other owners):
//!
//! | Module | State |
//! |---|---|
//! | [`model`], [`rng`], [`patch`], [`validate`], [`legacy`], [`math`] | implemented and tested |
//! | [`data`] | history and life tables typed; tax and actuarial JSON embedded, typed views pending |
//! | [`stats`] | `Accumulator` container, merge and codec implemented; recording and `Summary` pending |
//! | [`scenario`] | `Solver`, `GridSweep`, first `levers` implemented |
//! | [`sim`] | `Plan::compile` validates and sizes the horizon; the loop is pending |
//! | [`economy`], [`tax`], [`benefits`], [`career`], [`health`], [`housing`], [`accounts`], [`decumulation`] | interface stubs (`todo!()`) |
//!
//! Each stub module starts with `#![allow(dead_code, unused_variables)]`; delete that
//! line when the module is implemented.

pub mod accounts;
pub mod benefits;
pub mod career;
pub mod data;
pub mod decumulation;
pub mod economy;
pub mod health;
pub mod housing;
pub mod legacy;
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
