//! WebAssembly boundary for the Dungeon Barrage simulation core.
//!
//! This crate is deliberately thin. It holds only marshalling between JavaScript and
//! [`db_sim_core`] — no game rules. Anything resembling a decision about damage,
//! ammunition, terrain, or turn order that appears here is a bug, because the browser is
//! on the untrusted side of the boundary (`SECURITY_BASELINE.md` §2) and this code runs
//! there.
//!
//! The client runs the core to *predict* and to play the training mode locally. Online,
//! the server's identically-compiled core is authoritative and its result overrides any
//! local prediction.
//!
//! `wasm-bindgen` is not yet wired up; the exported surface arrives with the client
//! integration milestone. The crate exists now so the workspace, build, and size budget
//! are established before there is anything to marshal.

/// Simulation version this WASM module was built against.
///
/// The client compares this with the server's advertised version and refuses to connect
/// on a mismatch rather than desynchronizing mid-match
/// (`PLATFORM_STRATEGY.md` §15, release compatibility).
#[must_use]
pub const fn simulation_version() -> u32 {
    db_sim_core::SIMULATION_VERSION
}

/// Protocol version this WASM module speaks.
#[must_use]
pub const fn protocol_version() -> u32 {
    db_sim_core::PROTOCOL_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_reexported_from_the_core() {
        assert_eq!(simulation_version(), db_sim_core::SIMULATION_VERSION);
        assert_eq!(protocol_version(), db_sim_core::PROTOCOL_VERSION);
    }
}
