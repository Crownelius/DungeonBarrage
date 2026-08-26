//! # Dungeon Barrage authoritative simulation core
//!
//! This crate is the single, engine-independent implementation of the game rules. It imports
//! no renderer, game engine, network transport, database, or platform SDK types. The native
//! presentation client reaches it through the separate `db-sim-ffi` crate, while the future
//! Rust-native match server will depend on it directly (ADR 0006). The `db-sim-wasm` boundary
//! is dormant and preserves a web-revisit path; there is no active browser client. Every
//! target-specific artifact is built from this same deterministic source rather than a second
//! implementation that can drift.
//!
//! ## Invariants
//!
//! These are enforced by lints in `Cargo.toml` and by CI gates
//! (`docs/SECURITY_BASELINE.md` §10). They are not stylistic preferences.
//!
//! - **No `unsafe`.** `unsafe_code` is `forbid`den at the workspace level. A malformed
//!   network command must not be able to corrupt memory.
//! - **No floating point.** `clippy::float_arithmetic` is `deny`. Every spatial value is
//!   fixed-point ([`fixed`]), so results are bit-identical across `wasm32`, `x86_64`, and
//!   `aarch64`.
//! - **No ambient nondeterminism.** No wall clock, no thread scheduling, no OS entropy.
//!   The only randomness is the match seed, threaded explicitly.
//! - **No unordered iteration in hashed output.** Collections are sorted by a stated key
//!   before encoding ([`canonical`]).
//! - **No panics on untrusted input.** Fallible operations return [`error::SimResult`].
//!
//! ## Regression protection
//!
//! The TypeScript reference oracle was retired with the web surface (ADR 0004), so there is
//! no second implementation to check against. Cross-implementation parity is replaced by
//! **frozen golden vectors**: seeded command sequences and their resulting state hashes,
//! committed and asserted in CI.
//!
//! This proves self-consistency — a refactor cannot silently change behavior — but it does
//! **not** prove correctness against an independent implementation. The corpus freezes
//! whatever it is given, bugs included, so it may only be generated from reviewed code.

pub mod ballistics;
pub mod block_ops;
pub mod blocks;
pub mod canonical;
pub mod character;
pub mod client_contract;
pub mod command;
pub mod error;
pub mod fixed;
pub mod hash;
pub mod map;
pub mod match_host;
pub mod match_session;
pub mod match_setup;
pub mod movement;
pub mod resolve;
pub mod rng;
pub mod scheduler;
pub mod terrain;
pub mod types;
pub mod victory;

pub use error::{CharacterRejection, SimError, SimResult};
pub use fixed::{BASE_MELEE_RANGE, BODY_WIDTH, FIXED_TICK_RATE, FixedPoint, POSITION_SCALE};

/// Version of the simulation rules.
///
/// Incremented whenever a change could alter the outcome of a replayed match — including
/// a change to the canonical encoding. Every match records the version it ran under so
/// old replays stay interpretable (`PLATFORM_STRATEGY.md` §6).
///
/// Version 6 corrects three live lifecycle rules: statuses tick on the affected player's
/// own turns, Feeding Frenzy's count-based mark forces and consumes Carrion Call crits, and
/// ordinary health-zero/fall elimination removes the defeated owner's persistent objects.
/// Version 5 remains the terminal-turn-reason compatibility boundary.
pub const SIMULATION_VERSION: u32 = 6;

/// Version of the gameplay content tables (weapons, maps, modes).
pub const CONTENT_VERSION: u32 = 1;

/// Version of the wire protocol.
pub const PROTOCOL_VERSION: u32 = 1;
