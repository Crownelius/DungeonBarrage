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
pub mod bot;
pub mod canonical;
pub mod character;
pub mod character_roster;
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
pub mod projectile_mechanics;
pub mod resolve;
pub mod rng;
pub mod scheduler;
pub mod terrain;
pub mod types;
pub mod victory;

pub use error::{CharacterRejection, SimError, SimResult};
pub use fixed::{
    BASE_MELEE_RANGE, BODY_WIDTH, FIXED_TICK_RATE, FixedPoint, PLAYER_COLLISION_RADIUS,
    POSITION_SCALE, player_collision_center, player_collision_circle,
};

/// Version of the simulation rules.
///
/// Incremented whenever a change could alter the outcome of a replayed match — including
/// a change to the canonical encoding. Every match records the version it ran under so
/// old replays stay interpretable (`PLATFORM_STRATEGY.md` §6).
///
/// Version 10 restores character-owned fixed kits, derives compatibility loadouts from
/// `characterId`, and makes SS a charge-gated free action around one normal attack.
/// Version 9 makes `BODY_WIDTH` the player-collider diameter, centres that collider above
/// the standing pivot, and uses the same centre for projectile launch, preview, and bot aim.
/// Version 8 adds a crown/anklet trinket to the loadout and a charge-gated special.
/// Version 7 dropped kits: every fighter is the crow; items are ammunition; stacked
/// structures collapse when their support is destroyed.
pub const SIMULATION_VERSION: u32 = 10;

/// Version of the gameplay content tables (items, maps, modes).
///
/// Version 3 scopes the Ramshot Cannon's knockback to its own crater radius. At version 2
/// that effect carried `magnitude_secondary: 0`, which `displacement.rs` treats as "no
/// radius test at all" rather than its documented "primary target only", so every shot
/// shoved every opponent a flat 8 cells regardless of where the shell landed.
/// Version 5 is eight ranged, eight one-shot secondaries, eight melee skins, and eight
/// charge-gated crowns/anklets.
/// Version 6 is Melee-style stages (durable main platform plus stacked perches), crow
/// health 280, and smaller craters so one throw cannot delete a tower and the void in
/// the same shot.
/// Version 7 restores character-owned fixed kits with Leslie, Crow, Erus, and Kreena and
/// removes ammunition from the playable decision surface.
pub const CONTENT_VERSION: u32 = 7;

/// Version of the wire protocol.
pub const PROTOCOL_VERSION: u32 = 1;
