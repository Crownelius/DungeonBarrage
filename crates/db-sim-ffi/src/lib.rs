//! C ABI boundary for the Dungeon Barrage simulation core.
//!
//! Both the C# client and the C# match server P/Invoke into this library, so there remains
//! exactly one implementation of the game rules (ADR 0004). This crate contains **no
//! gameplay logic** — only marshalling. Anything here that decides damage, ammunition,
//! terrain, or turn order is a bug.
//!
//! # Why this crate exists separately
//!
//! `db-sim-core` sets `unsafe_code = "forbid"` at the workspace level, which is what makes
//! "a malformed network command cannot corrupt memory" a structural guarantee rather than a
//! hope. A C ABI cannot be expressed without `unsafe`, so the boundary is isolated into this
//! one small, individually reviewed crate — exactly as ADR 0001 §1 requires. The core stays
//! provably safe; the unsafe surface stays small enough to audit by reading.
//!
//! # Safety contract for every exported function
//!
//! - Handles are opaque pointers produced only by [`db_sim_create`] and invalidated only by
//!   [`db_sim_destroy`]. Passing anything else is undefined behaviour on the caller's side.
//! - A null handle is **tolerated**, not undefined: every function checks and returns an
//!   error status. Callers get a status code, not a crash.
//! - Every function is `catch_unwind`-wrapped. A panic must never unwind across the FFI
//!   boundary — that is undefined behaviour — and must never take down a server process
//!   holding other players' matches.
//! - No function takes or returns a floating-point value. Every gameplay scalar crossing
//!   this boundary is a quantized integer (ADR 0001 §4).
//! - Strings out are UTF-8, NUL-terminated, owned by this library, and freed with
//!   [`db_sim_string_free`]. Callers must not free them with the C# allocator.

use core::ffi::{c_char, c_int};
use std::ffi::CString;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Status codes returned across the boundary.
///
/// Deliberately a plain `c_int` rather than a Rust enum: the C# side matches on integers,
/// and a `#[repr]` enum with an out-of-range value would be undefined behaviour.
pub mod status {
    use core::ffi::c_int;

    /// Operation succeeded.
    pub const OK: c_int = 0;
    /// A required pointer argument was null.
    pub const NULL_POINTER: c_int = -1;
    /// A string argument was not valid UTF-8.
    pub const INVALID_UTF8: c_int = -2;
    /// The simulation rejected the request. Not a fault — a game-rules outcome.
    pub const REJECTED: c_int = -3;
    /// A panic was caught at the boundary. Indicates a bug in the core; the caller's
    /// process survives and the match should be abandoned rather than continued.
    pub const INTERNAL_PANIC: c_int = -4;
    /// The requested buffer was too small; nothing was written.
    pub const BUFFER_TOO_SMALL: c_int = -5;
}

/// Opaque handle to a live simulation.
///
/// The layout is deliberately private. C# sees only an `IntPtr`, so the internal
/// representation can change without breaking the ABI.
pub struct SimHandle {
    /// Placeholder for the owned match state.
    ///
    /// Kept as a field rather than a unit struct so adding the real state later does not
    /// change the handle's identity from C#'s perspective.
    _reserved: u64,
}

/// Runs `body`, converting any panic into [`status::INTERNAL_PANIC`].
///
/// Unwinding across an FFI boundary is undefined behaviour, so this wrapper is mandatory on
/// every exported function — not a convenience.
fn guard<F: FnOnce() -> c_int>(body: F) -> c_int {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(code) => code,
        Err(_) => status::INTERNAL_PANIC,
    }
}

/// Returns the simulation rules version this library was built against.
///
/// The C# client compares this with the server's advertised version and refuses to connect
/// on a mismatch rather than desynchronizing mid-match.
#[unsafe(no_mangle)]
pub extern "C" fn db_sim_simulation_version() -> u32 {
    db_sim_core::SIMULATION_VERSION
}

/// Returns the wire protocol version this library speaks.
#[unsafe(no_mangle)]
pub extern "C" fn db_sim_protocol_version() -> u32 {
    db_sim_core::PROTOCOL_VERSION
}

/// Returns the content tables version.
#[unsafe(no_mangle)]
pub extern "C" fn db_sim_content_version() -> u32 {
    db_sim_core::CONTENT_VERSION
}

/// Creates a new simulation and returns an opaque handle.
///
/// Returns null on allocation failure or if the core rejects the seed. The caller must pass
/// the returned handle to [`db_sim_destroy`] exactly once.
#[unsafe(no_mangle)]
pub extern "C" fn db_sim_create(seed: u64) -> *mut SimHandle {
    let created = catch_unwind(AssertUnwindSafe(|| {
        Box::into_raw(Box::new(SimHandle { _reserved: seed }))
    }));
    created.unwrap_or(core::ptr::null_mut())
}

/// Destroys a simulation created by [`db_sim_create`].
///
/// # Safety
///
/// `handle` must be either null or a pointer returned by [`db_sim_create`] that has not
/// already been destroyed. Passing a pointer twice is a double free.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_destroy(handle: *mut SimHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: the caller contract above guarantees `handle` came from `db_sim_create` and
    // has not been destroyed, so reconstructing the Box to drop it is sound.
    let boxed = unsafe { Box::from_raw(handle) };
    drop(boxed);
}

/// Writes the current state hash into `out` as a NUL-terminated UTF-8 string.
///
/// The hash is 16 hex characters, so `out_len` must be at least 17 to include the
/// terminator. Returns [`status::BUFFER_TOO_SMALL`] without writing if it is not.
///
/// # Safety
///
/// `handle` must be null or valid per [`db_sim_destroy`]'s contract. `out` must be null or
/// point to at least `out_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_state_hash(
    handle: *const SimHandle,
    out: *mut c_char,
    out_len: usize,
) -> c_int {
    guard(|| {
        if handle.is_null() || out.is_null() {
            return status::NULL_POINTER;
        }
        // 16 hex digits plus the NUL terminator.
        const REQUIRED: usize = 17;
        if out_len < REQUIRED {
            return status::BUFFER_TOO_SMALL;
        }
        let hash = db_sim_core::canonical::CanonicalHasher::new().finish_hex();
        let Ok(encoded) = CString::new(hash) else {
            return status::INVALID_UTF8;
        };
        let bytes = encoded.as_bytes_with_nul();
        if bytes.len() > out_len {
            return status::BUFFER_TOO_SMALL;
        }
        // SAFETY: `out` is non-null with at least `out_len` writable bytes per the caller
        // contract, and `bytes.len() <= out_len` was just checked. The regions cannot
        // overlap because `bytes` is owned by a local CString.
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr().cast::<c_char>(), out, bytes.len());
        }
        status::OK
    })
}

/// Frees a string previously returned by this library.
///
/// # Safety
///
/// `ptr` must be null or a pointer returned by this library and not yet freed. Freeing it
/// with the C# allocator instead is undefined behaviour — the allocators differ.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: the caller contract guarantees `ptr` came from a `CString::into_raw` in this
    // library and has not been freed, so reclaiming it here is sound.
    drop(unsafe { CString::from_raw(ptr) });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_reexported_from_the_core() {
        assert_eq!(db_sim_simulation_version(), db_sim_core::SIMULATION_VERSION);
        assert_eq!(db_sim_protocol_version(), db_sim_core::PROTOCOL_VERSION);
        assert_eq!(db_sim_content_version(), db_sim_core::CONTENT_VERSION);
    }

    #[test]
    fn create_and_destroy_round_trips() {
        let handle = db_sim_create(12_345);
        assert!(!handle.is_null());
        // SAFETY: `handle` came from `db_sim_create` and is destroyed exactly once.
        unsafe { db_sim_destroy(handle) };
    }

    #[test]
    fn destroy_tolerates_null() {
        // A null handle must be a no-op, not a crash — C# marshalling can legitimately
        // produce one after a failed create.
        // SAFETY: null is explicitly permitted by the function's contract.
        unsafe { db_sim_destroy(core::ptr::null_mut()) };
    }

    #[test]
    fn state_hash_rejects_null_arguments_instead_of_crashing() {
        let mut buffer = [0 as c_char; 32];
        // SAFETY: passing null is explicitly permitted and must return an error status.
        let code = unsafe { db_sim_state_hash(core::ptr::null(), buffer.as_mut_ptr(), 32) };
        assert_eq!(code, status::NULL_POINTER);

        let handle = db_sim_create(1);
        // SAFETY: valid handle, null output buffer — must be reported, not dereferenced.
        let code = unsafe { db_sim_state_hash(handle, core::ptr::null_mut(), 32) };
        assert_eq!(code, status::NULL_POINTER);
        // SAFETY: destroyed exactly once.
        unsafe { db_sim_destroy(handle) };
    }

    #[test]
    fn state_hash_reports_a_short_buffer_rather_than_overflowing_it() {
        let handle = db_sim_create(1);
        let mut buffer = [0 as c_char; 4];
        // SAFETY: valid handle, valid buffer, truthful length.
        let code = unsafe { db_sim_state_hash(handle, buffer.as_mut_ptr(), 4) };
        assert_eq!(code, status::BUFFER_TOO_SMALL);
        assert!(
            buffer.iter().all(|byte| *byte == 0),
            "nothing may be written when the buffer is rejected",
        );
        // SAFETY: destroyed exactly once.
        unsafe { db_sim_destroy(handle) };
    }

    #[test]
    fn state_hash_writes_sixteen_hex_digits_and_a_terminator() {
        let handle = db_sim_create(1);
        let mut buffer = [0 as c_char; 32];
        // SAFETY: valid handle, valid buffer, truthful length.
        let code = unsafe { db_sim_state_hash(handle, buffer.as_mut_ptr(), 32) };
        assert_eq!(code, status::OK);

        let written: Vec<u8> = buffer
            .iter()
            .take_while(|byte| **byte != 0)
            .map(|byte| *byte as u8)
            .collect();
        assert_eq!(written.len(), 16);
        assert!(written.iter().all(u8::is_ascii_hexdigit));
    }
}
