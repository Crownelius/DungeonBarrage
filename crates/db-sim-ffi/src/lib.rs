//! Coarse, client-only C ABI for the authoritative Dungeon Barrage simulation.
//!
//! The future Rust server links `db-sim-core` directly. This crate exists solely for the local
//! Godot/C# client and contains no gameplay rules. JSON inputs are bounded and decoded into closed
//! DTOs; outputs are exact Rust-owned boxed byte slices; mutating calls resolve and serialize a
//! cloned session before atomically replacing the live one.
//!
//! # Pointer and output-slot contract
//!
//! Every non-null pointer must be correctly aligned and valid for its documented reads or writes
//! for the full call. Output slots must be pairwise non-overlapping and must not overlap a handle
//! allocation, an input byte range, or a returned buffer allocation. A `DbOwnedBuffer` output slot
//! must own no live allocation when passed: exports initialize it by assignment, so reusing an
//! unfreed slot would leak the old allocation. A handle may be shared between serialized live calls,
//! but it must not be destroyed while any call is in progress. These are caller obligations; a raw C
//! ABI cannot prove them at runtime. The C3 wrapper satisfies them with distinct zeroed locals and
//! `SafeHandle`.

use core::ffi::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use db_sim_core::client_contract::CLIENT_CONTRACT_VERSION;
use db_sim_core::match_session::{MatchSessionHost, SessionFault};
use serde::de::DeserializeOwned;

mod wire;

/// Native calling convention and buffer-ownership version.
///
/// Version 4 adds the thirteenth export, [`db_sim_match_timeout`] — another function-set
/// addition, per `docs/CLIENT_SPEC.md` §6's versioning rule. Version 3 added the twelfth
/// export, [`db_sim_roster`]. Version 2 added the eleventh export,
/// [`db_sim_match_bot_decide`]. Version 1 exposed exactly the ten
/// version/create/apply/snapshot/terrain/preview/disposal symbols.
pub const ABI_VERSION: u32 = 4;
/// Maximum accepted JSON request size: 256 KiB.
pub const MAX_INPUT_BYTES: usize = 256 * 1024;
/// Maximum serialized transition/snapshot/preview size: 8 MiB.
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
/// Maximum JSON object/array nesting accepted at the boundary.
const MAX_JSON_DEPTH: usize = 12;

/// ABI status codes. Gameplay rejection is carried inside an `OK` response envelope.
pub mod status {
    use core::ffi::c_int;

    /// ABI call completed; inspect the output envelope.
    pub const OK: c_int = 0;
    /// A required pointer was null.
    pub const NULL_POINTER: c_int = -1;
    /// Invalid UTF-8, malformed JSON/envelope, unknown field/enum, or invalid normalized field.
    pub const MALFORMED_ENVELOPE: c_int = -2;
    /// Unsupported envelope schema, simulation version, or content version.
    pub const UNSUPPORTED_VERSION: c_int = -3;
    /// A panic or terminal internal/session invariant was contained. The handle is poisoned.
    pub const INTERNAL_PANIC: c_int = -4;
    /// A response would exceed the documented 8 MiB cap.
    pub const RESPONSE_TOO_LARGE: c_int = -5;
}

/// The only allocation returned through the ABI.
///
/// A non-empty value owns an exact `Box<[u8]>`; [`db_sim_buffer_free`] reconstructs that same
/// allocation from `ptr` and `len`, then writes the zero representation back.
#[repr(C)]
#[derive(Debug)]
pub struct DbOwnedBuffer {
    /// First byte of the Rust-owned allocation, or null for an empty buffer.
    pub ptr: *mut u8,
    /// Exact allocation length.
    pub len: usize,
}

impl DbOwnedBuffer {
    const fn empty() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            len: 0,
        }
    }
}

struct HandleInner {
    session: MatchSessionHost,
    match_id: String,
    map_id: String,
}

/// Opaque live local-match handle.
///
/// C# sees only a pointer. A mutex serializes calls even if a broken caller bypasses the intended
/// one-call-at-a-time executor; the atomic poison bit remains readable if a panic poisons the mutex.
pub struct SimHandle {
    poisoned: AtomicBool,
    inner: Mutex<HandleInner>,
}

fn guard(handle: Option<&SimHandle>, body: impl FnOnce() -> c_int) -> c_int {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(code) => code,
        Err(_) => {
            if let Some(handle) = handle {
                handle.poisoned.store(true, Ordering::Release);
            }
            status::INTERNAL_PANIC
        }
    }
}

fn lock_handle(handle: &SimHandle) -> Result<MutexGuard<'_, HandleInner>, c_int> {
    if handle.poisoned.load(Ordering::Acquire) {
        return Err(status::INTERNAL_PANIC);
    }
    match handle.inner.lock() {
        Ok(inner) => Ok(inner),
        Err(_) => {
            handle.poisoned.store(true, Ordering::Release);
            Err(status::INTERNAL_PANIC)
        }
    }
}

fn session_fault_status(handle: &SimHandle, fault: &SessionFault) -> c_int {
    match fault {
        SessionFault::UnsupportedSchema { .. } => status::UNSUPPORTED_VERSION,
        SessionFault::InvalidCommand { .. } => status::MALFORMED_ENVELOPE,
        SessionFault::ResourceLimit
        | SessionFault::Simulation(_)
        | SessionFault::GenerationExhausted
        | SessionFault::ContractInvariant
        | SessionFault::Closed => {
            handle.poisoned.store(true, Ordering::Release);
            status::INTERNAL_PANIC
        }
    }
}

fn json_depth_is_bounded(bytes: &[u8]) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                let Some(next) = depth.checked_add(1) else {
                    return false;
                };
                depth = next;
                if depth > MAX_JSON_DEPTH {
                    return false;
                }
            }
            b'}' | b']' => {
                let Some(next) = depth.checked_sub(1) else {
                    return false;
                };
                depth = next;
            }
            _ => {}
        }
    }
    !in_string && depth == 0
}

fn decode_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, c_int> {
    if bytes.len() > MAX_INPUT_BYTES || !json_depth_is_bounded(bytes) {
        return Err(status::MALFORMED_ENVELOPE);
    }
    serde_json::from_slice(bytes).map_err(|_| status::MALFORMED_ENVELOPE)
}

fn is_definition_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_match_local_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn is_appearance_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn validate_config_adapter_fields(config: &db_sim_core::match_setup::MatchConfig) -> bool {
    is_definition_id(&config.map_id)
        && config.players.iter().all(|player| {
            is_definition_id(&player.character_id)
                && is_appearance_id(&player.appearance.skin_id)
                && player
                    .appearance
                    .ability_skin_ids
                    .iter()
                    .all(|id| is_appearance_id(id))
                && is_appearance_id(&player.appearance.victory_pose_id)
        })
}

fn boxed_buffer(bytes: Vec<u8>) -> Result<DbOwnedBuffer, c_int> {
    boxed_buffer_with_limit(bytes, MAX_OUTPUT_BYTES)
}

fn boxed_buffer_with_limit(bytes: Vec<u8>, limit: usize) -> Result<DbOwnedBuffer, c_int> {
    if bytes.len() > limit {
        return Err(status::RESPONSE_TOO_LARGE);
    }
    if bytes.is_empty() {
        return Ok(DbOwnedBuffer::empty());
    }
    let mut boxed = bytes.into_boxed_slice();
    let buffer = DbOwnedBuffer {
        ptr: boxed.as_mut_ptr(),
        len: boxed.len(),
    };
    core::mem::forget(boxed);
    Ok(buffer)
}

fn serialize_status<T>(
    handle: Option<&SimHandle>,
    result: Result<Vec<u8>, T>,
) -> Result<DbOwnedBuffer, c_int> {
    match result {
        Ok(bytes) => boxed_buffer(bytes),
        Err(_) => {
            if let Some(handle) = handle {
                handle.poisoned.store(true, Ordering::Release);
            }
            Err(status::INTERNAL_PANIC)
        }
    }
}

fn apply_serialized(
    handle: &SimHandle,
    inner: &mut HandleInner,
    command: db_sim_core::match_session::MatchCommand,
    output_limit: usize,
) -> Result<DbOwnedBuffer, c_int> {
    let mut working = inner.session.clone();
    let transition = working
        .apply(command)
        .map_err(|fault| session_fault_status(handle, &fault))?;
    let bytes =
        wire::serialize_transition(&transition, &inner.match_id, &inner.map_id).map_err(|_| {
            handle.poisoned.store(true, Ordering::Release);
            status::INTERNAL_PANIC
        })?;
    let output = boxed_buffer_with_limit(bytes, output_limit)?;
    // No fallible work follows this point: session replacement and buffer publication are the
    // atomic success boundary owned by the caller.
    inner.session = working;
    Ok(output)
}

/// Mirrors [`apply_serialized`], but ends the turn via [`db_sim_core::match_session::MatchSessionHost::apply_authority_timeout`]
/// instead of an ordinary command — see [`db_sim_match_timeout`].
fn apply_timeout_serialized(
    handle: &SimHandle,
    inner: &mut HandleInner,
    timeout: db_sim_core::match_session::AuthorityTimeout,
    output_limit: usize,
) -> Result<DbOwnedBuffer, c_int> {
    let mut working = inner.session.clone();
    let transition = working
        .apply_authority_timeout(timeout)
        .map_err(|fault| session_fault_status(handle, &fault))?;
    let bytes =
        wire::serialize_transition(&transition, &inner.match_id, &inner.map_id).map_err(|_| {
            handle.poisoned.store(true, Ordering::Release);
            status::INTERNAL_PANIC
        })?;
    let output = boxed_buffer_with_limit(bytes, output_limit)?;
    // No fallible work follows this point: session replacement and buffer publication are the
    // atomic success boundary owned by the caller, matching apply_serialized.
    inner.session = working;
    Ok(output)
}

/// Returns the exact native ABI version.
#[unsafe(no_mangle)]
pub extern "C" fn db_sim_abi_version() -> u32 {
    ABI_VERSION
}

/// Returns the authoritative simulation version linked into this library.
#[unsafe(no_mangle)]
pub extern "C" fn db_sim_simulation_version() -> u32 {
    db_sim_core::SIMULATION_VERSION
}

/// Returns the authoritative content version linked into this library.
#[unsafe(no_mangle)]
pub extern "C" fn db_sim_content_version() -> u32 {
    db_sim_core::CONTENT_VERSION
}

/// Serializes the full launch roster.
///
/// Static content, not match state: takes no handle, and never fails except the two ways any
/// output pointer can (`NULL_POINTER`, or `INTERNAL_PANIC` on a serialization failure this
/// crate would treat as a defect rather than a caller error, exactly like every other export's
/// serialization step).
///
/// # Safety
///
/// `roster_out` may be null and then follows the documented status precedence. A non-null
/// `roster_out` must be a writable, allocation-free slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_roster(roster_out: *mut DbOwnedBuffer) -> c_int {
    if roster_out.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: checked non-null and writable.
    unsafe { *roster_out = DbOwnedBuffer::empty() };
    guard(None, || {
        let output = match serialize_status(None, wire::serialize_roster()) {
            Ok(output) => output,
            Err(code) => return code,
        };
        // SAFETY: checked non-null and writable; serialization and allocation already succeeded.
        unsafe { *roster_out = output };
        status::OK
    })
}

/// Creates a real match session from a strict UTF-8 JSON request.
///
/// Domain-invalid configs return `OK`, a null handle, and `{created:false,...}`. Malformed bytes or
/// unsupported versions use negative ABI statuses.
///
/// # Safety
///
/// Every pointer may be null, which produces `NULL_POINTER` after any non-null output is initialized.
/// A non-null `config_json` must point to `config_len` readable bytes. Non-null `handle_out` and
/// `response_out` values must be valid writable, pairwise non-overlapping output slots and must not
/// overlap the input range. Neither slot may own a live handle/allocation when passed. The
/// module-level pointer contract also applies; arbitrary non-null pointers are outside the C
/// contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_match_create(
    config_json: *const u8,
    config_len: usize,
    handle_out: *mut *mut SimHandle,
    response_out: *mut DbOwnedBuffer,
) -> c_int {
    if !handle_out.is_null() {
        // SAFETY: a non-null output is writable by the caller contract.
        unsafe { *handle_out = core::ptr::null_mut() };
    }
    if !response_out.is_null() {
        // SAFETY: a non-null output is writable by the caller contract.
        unsafe { *response_out = DbOwnedBuffer::empty() };
    }
    if handle_out.is_null() || response_out.is_null() {
        return status::NULL_POINTER;
    }
    if config_json.is_null() {
        return status::NULL_POINTER;
    }
    if config_len > MAX_INPUT_BYTES {
        return status::MALFORMED_ENVELOPE;
    }
    guard(None, || {
        // SAFETY: the caller contract promises exactly `config_len` readable bytes.
        let bytes = unsafe { core::slice::from_raw_parts(config_json, config_len) };
        let request: wire::MatchCreateRequestDto = match decode_json(bytes) {
            Ok(request) => request,
            Err(code) => return code,
        };
        if request.schema_version != CLIENT_CONTRACT_VERSION
            || request.simulation_version != db_sim_core::SIMULATION_VERSION
            || request.content_version != db_sim_core::CONTENT_VERSION
        {
            return status::UNSUPPORTED_VERSION;
        }
        let match_id = request.match_id.clone();
        let config = request.into_core();
        let map_id = config.map_id.clone();
        if !is_match_local_id(&match_id) || !validate_config_adapter_fields(&config) {
            let output = match serialize_status(
                None,
                wire::serialize_create_failure(
                    "invalidConfig",
                    "adapter-owned identifier validation failed".to_owned(),
                ),
            ) {
                Ok(output) => output,
                Err(code) => return code,
            };
            // SAFETY: initialized non-null output pointer remains valid for this call.
            unsafe { *response_out = output };
            return status::OK;
        }
        let session = match MatchSessionHost::create(&config) {
            Ok(session) => session,
            Err(error) => {
                let output = match serialize_status(
                    None,
                    wire::serialize_create_failure("invalidConfig", error.to_string()),
                ) {
                    Ok(output) => output,
                    Err(code) => return code,
                };
                // SAFETY: initialized non-null output pointer remains valid for this call.
                unsafe { *response_out = output };
                return status::OK;
            }
        };
        let output = match serialize_status(
            None,
            wire::serialize_create_success(&session.snapshot(), &match_id, &map_id),
        ) {
            Ok(output) => output,
            Err(code) => return code,
        };
        let handle = Box::into_raw(Box::new(SimHandle {
            poisoned: AtomicBool::new(false),
            inner: Mutex::new(HandleInner {
                session,
                match_id,
                map_id,
            }),
        }));
        // SAFETY: initialized non-null output pointers remain valid for this call. No fallible work
        // follows publication of the handle and its response.
        unsafe {
            *response_out = output;
            *handle_out = handle;
        }
        status::OK
    })
}

/// Applies one strict command to a cloned session, serializes its complete transition, then commits.
///
/// # Safety
///
/// `handle`, `command_json`, and `transition_out` may be null and then follow the documented status
/// precedence. A non-null `handle` must be live and returned by [`db_sim_match_create`]. A non-null
/// `command_json` must point to `command_len` readable bytes. A non-null `transition_out` must be a
/// writable, allocation-free slot that does not overlap the handle or input range. The handle must
/// not be destroyed concurrently. A poisoned live handle is checked before the request pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_match_apply(
    handle: *mut SimHandle,
    command_json: *const u8,
    command_len: usize,
    transition_out: *mut DbOwnedBuffer,
) -> c_int {
    if transition_out.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: checked non-null and writable by contract.
    unsafe { *transition_out = DbOwnedBuffer::empty() };
    if handle.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: caller guarantees this is a live handle for the duration of the call.
    let handle_ref = unsafe { &*handle };
    guard(Some(handle_ref), || {
        // A poisoned live handle is terminal. Check it before inspecting request bytes so malformed,
        // unsupported, and oversized follow-up calls cannot obscure that terminal state.
        let mut inner = match lock_handle(handle_ref) {
            Ok(inner) => inner,
            Err(code) => return code,
        };
        if command_json.is_null() {
            return status::NULL_POINTER;
        }
        if command_len > MAX_INPUT_BYTES {
            return status::MALFORMED_ENVELOPE;
        }
        // SAFETY: caller promises exactly `command_len` readable bytes.
        let bytes = unsafe { core::slice::from_raw_parts(command_json, command_len) };
        let request: wire::MatchCommandDto = match decode_json(bytes) {
            Ok(request) => request,
            Err(code) => return code,
        };
        if request.schema_version() != CLIENT_CONTRACT_VERSION {
            return status::UNSUPPORTED_VERSION;
        }
        let command = request.into_core();
        let output = match apply_serialized(handle_ref, &mut inner, command, MAX_OUTPUT_BYTES) {
            Ok(output) => output,
            Err(code) => return code,
        };
        // SAFETY: checked non-null and writable; serialization and allocation already succeeded.
        unsafe { *transition_out = output };
        status::OK
    })
}

/// Ends the active player's turn because their own local planning deadline expired.
///
/// This is the local-play counterpart to [`db_sim_match_apply`], not an alternate route to the
/// same effect: `db_sim_core::match_session::AuthorityTimeout` is deliberately not part of the
/// `MatchCommandDto` union a command JSON payload decodes into, so no client command can reach
/// this behavior through [`db_sim_match_apply`]. Calling this export at all is the caller
/// (`LocalMatchSession`) claiming authority over its own clock — legitimate only because local
/// play has no separate untrusted-client/trusted-server split; a future networked session must
/// never expose this to a remote peer (`docs/SECURITY_BASELINE.md` §2: the server owns the
/// clock).
///
/// # Safety
///
/// Every pointer may be null, then follows the documented status precedence. A non-null `handle`
/// must be live. A non-null `timeout_json` must name `timeout_len` readable bytes. A non-null
/// `transition_out` must be a writable, allocation-free slot that does not overlap the handle or
/// input range. A live handle must not be destroyed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_match_timeout(
    handle: *mut SimHandle,
    timeout_json: *const u8,
    timeout_len: usize,
    transition_out: *mut DbOwnedBuffer,
) -> c_int {
    if transition_out.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: checked non-null and writable by contract.
    unsafe { *transition_out = DbOwnedBuffer::empty() };
    if handle.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: caller guarantees this is a live handle for the duration of the call.
    let handle_ref = unsafe { &*handle };
    guard(Some(handle_ref), || {
        // A poisoned live handle is terminal. Check it before inspecting request bytes so
        // malformed, unsupported, and oversized follow-up calls cannot obscure that terminal
        // state.
        let mut inner = match lock_handle(handle_ref) {
            Ok(inner) => inner,
            Err(code) => return code,
        };
        if timeout_json.is_null() {
            return status::NULL_POINTER;
        }
        if timeout_len > MAX_INPUT_BYTES {
            return status::MALFORMED_ENVELOPE;
        }
        // SAFETY: caller promises exactly `timeout_len` readable bytes.
        let bytes = unsafe { core::slice::from_raw_parts(timeout_json, timeout_len) };
        let request: wire::AuthorityTimeoutDto = match decode_json(bytes) {
            Ok(request) => request,
            Err(code) => return code,
        };
        if request.schema_version() != CLIENT_CONTRACT_VERSION {
            return status::UNSUPPORTED_VERSION;
        }
        let timeout = request.into_core();
        let output =
            match apply_timeout_serialized(handle_ref, &mut inner, timeout, MAX_OUTPUT_BYTES) {
                Ok(output) => output,
                Err(code) => return code,
            };
        // SAFETY: checked non-null and writable; serialization and allocation already succeeded.
        unsafe { *transition_out = output };
        status::OK
    })
}

/// Serializes one atomic composite snapshot.
///
/// # Safety
///
/// `handle` must be live or null. A non-null `snapshot_out` must be a writable, allocation-free slot
/// that does not overlap the handle. A live handle must not be destroyed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_match_snapshot(
    handle: *const SimHandle,
    snapshot_out: *mut DbOwnedBuffer,
) -> c_int {
    if snapshot_out.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: checked non-null and writable by contract.
    unsafe { *snapshot_out = DbOwnedBuffer::empty() };
    if handle.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: caller guarantees a live handle for the call.
    let handle_ref = unsafe { &*handle };
    guard(Some(handle_ref), || {
        let inner = match lock_handle(handle_ref) {
            Ok(inner) => inner,
            Err(code) => return code,
        };
        let output = match serialize_status(
            Some(handle_ref),
            wire::serialize_snapshot(&inner.session.snapshot(), &inner.match_id, &inner.map_id),
        ) {
            Ok(output) => output,
            Err(code) => return code,
        };
        // SAFETY: checked non-null and writable.
        unsafe { *snapshot_out = output };
        status::OK
    })
}

/// Reads raw row-major terrain bytes only when its generation differs from `known_generation`.
///
/// # Safety
///
/// `handle` must be live or null. All four output pointers must be null or writable; when non-null
/// they are initialized on every returned status. Non-null output slots must be pairwise
/// non-overlapping and must not overlap the handle; `cells_out` must own no live allocation. A live
/// handle must not be destroyed concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_match_terrain(
    handle: *const SimHandle,
    known_generation: u64,
    width_out: *mut u32,
    height_out: *mut u32,
    generation_out: *mut u64,
    cells_out: *mut DbOwnedBuffer,
) -> c_int {
    if !width_out.is_null() {
        // SAFETY: a non-null output is writable by the caller contract.
        unsafe { *width_out = 0 };
    }
    if !height_out.is_null() {
        // SAFETY: a non-null output is writable by the caller contract.
        unsafe { *height_out = 0 };
    }
    if !generation_out.is_null() {
        // SAFETY: a non-null output is writable by the caller contract.
        unsafe { *generation_out = 0 };
    }
    if !cells_out.is_null() {
        // SAFETY: a non-null output is writable by the caller contract.
        unsafe { *cells_out = DbOwnedBuffer::empty() };
    }
    if width_out.is_null()
        || height_out.is_null()
        || generation_out.is_null()
        || cells_out.is_null()
    {
        return status::NULL_POINTER;
    }
    if handle.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: caller guarantees a live handle for the call.
    let handle_ref = unsafe { &*handle };
    guard(Some(handle_ref), || {
        let inner = match lock_handle(handle_ref) {
            Ok(inner) => inner,
            Err(code) => return code,
        };
        let state = inner.session.host().state();
        let generation = u64::from(state.next_terrain_sequence);
        // SAFETY: initialized non-null scalar outputs remain writable.
        unsafe {
            *width_out = state.terrain.width;
            *height_out = state.terrain.height;
            *generation_out = generation;
        }
        if known_generation == generation {
            return status::OK;
        }
        let expected = match usize::try_from(state.terrain.width).ok().and_then(|width| {
            usize::try_from(state.terrain.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        }) {
            Some(expected) => expected,
            None => {
                handle_ref.poisoned.store(true, Ordering::Release);
                return status::INTERNAL_PANIC;
            }
        };
        if state.terrain.cells.len() != expected {
            handle_ref.poisoned.store(true, Ordering::Release);
            return status::INTERNAL_PANIC;
        }
        let bytes = state.terrain.cells.clone();
        let output = match boxed_buffer(bytes) {
            Ok(output) => output,
            Err(code) => return code,
        };
        // SAFETY: checked non-null and writable.
        unsafe { *cells_out = output };
        status::OK
    })
}

/// Computes one read-only ability preview.
///
/// # Safety
///
/// `handle`, `request_json`, and `preview_out` may be null and then follow the documented status
/// precedence. A non-null `handle` must be live. A non-null `request_json` must name `request_len`
/// readable bytes. A non-null `preview_out` must be a writable, allocation-free slot that does not
/// overlap the handle or input range. A live handle must not be destroyed concurrently. A poisoned
/// live handle is checked before the request pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_match_preview(
    handle: *const SimHandle,
    request_json: *const u8,
    request_len: usize,
    preview_out: *mut DbOwnedBuffer,
) -> c_int {
    if preview_out.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: checked non-null and writable.
    unsafe { *preview_out = DbOwnedBuffer::empty() };
    if handle.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: caller guarantees a live handle for the call.
    let handle_ref = unsafe { &*handle };
    guard(Some(handle_ref), || {
        // Poison is a terminal session state and takes precedence over request decoding.
        let inner = match lock_handle(handle_ref) {
            Ok(inner) => inner,
            Err(code) => return code,
        };
        if request_json.is_null() {
            return status::NULL_POINTER;
        }
        if request_len > MAX_INPUT_BYTES {
            return status::MALFORMED_ENVELOPE;
        }
        // SAFETY: caller promises exactly `request_len` readable bytes.
        let bytes = unsafe { core::slice::from_raw_parts(request_json, request_len) };
        let request: wire::AbilityPreviewRequestDto = match decode_json(bytes) {
            Ok(request) => request,
            Err(code) => return code,
        };
        if request.schema_version() != CLIENT_CONTRACT_VERSION {
            return status::UNSUPPORTED_VERSION;
        }
        let request = request.into_core();
        let response = match inner.session.preview(&request) {
            Ok(response) => response,
            Err(fault) => return session_fault_status(handle_ref, &fault),
        };
        let output = match serialize_status(Some(handle_ref), wire::serialize_preview(&response)) {
            Ok(output) => output,
            Err(code) => return code,
        };
        // SAFETY: checked non-null and writable.
        unsafe { *preview_out = output };
        status::OK
    })
}

/// Proposes one action for a bot-controlled player, without submitting or mutating anything.
///
/// The caller is responsible for turning the returned decision into an ordinary command
/// (adding a fresh `commandId` and reading `expectedTurnNumber`/`expectedSnapshotGeneration`
/// off its own current snapshot) and submitting it through [`db_sim_match_apply`] exactly as
/// a human command would be. This call never mutates the session: a bot's move gets no
/// special access, and only ever takes effect via the same validated path a person's does
/// (`docs/PRODUCT_SPEC.md`: "Bot difficulty changes candidate search and aim error; it does
/// not ignore wind, collision, ammunition, or hazards").
///
/// # Safety
///
/// `handle`, `request_json`, and `decision_out` may be null and then follow the documented
/// status precedence. A non-null `handle` must be live. A non-null `request_json` must name
/// `request_len` readable bytes. A non-null `decision_out` must be a writable, allocation-free
/// slot that does not overlap the handle or input range. A live handle must not be destroyed
/// concurrently. A poisoned live handle is checked before the request pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_match_bot_decide(
    handle: *const SimHandle,
    request_json: *const u8,
    request_len: usize,
    decision_out: *mut DbOwnedBuffer,
) -> c_int {
    if decision_out.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: checked non-null and writable.
    unsafe { *decision_out = DbOwnedBuffer::empty() };
    if handle.is_null() {
        return status::NULL_POINTER;
    }
    // SAFETY: caller guarantees a live handle for the call.
    let handle_ref = unsafe { &*handle };
    guard(Some(handle_ref), || {
        // Poison is a terminal session state and takes precedence over request decoding.
        let inner = match lock_handle(handle_ref) {
            Ok(inner) => inner,
            Err(code) => return code,
        };
        if request_json.is_null() {
            return status::NULL_POINTER;
        }
        if request_len > MAX_INPUT_BYTES {
            return status::MALFORMED_ENVELOPE;
        }
        // SAFETY: caller promises exactly `request_len` readable bytes.
        let bytes = unsafe { core::slice::from_raw_parts(request_json, request_len) };
        let request: wire::BotDecisionRequestDto = match decode_json(bytes) {
            Ok(request) => request,
            Err(code) => return code,
        };
        if request.schema_version() != CLIENT_CONTRACT_VERSION {
            return status::UNSUPPORTED_VERSION;
        }
        let (player_id, difficulty, decision_seed) = request.into_core();
        let decision = db_sim_core::bot::decide(
            inner.session.host().state(),
            &player_id,
            difficulty,
            decision_seed,
        );
        let output =
            match serialize_status(Some(handle_ref), wire::serialize_bot_decision(decision)) {
                Ok(output) => output,
                Err(code) => return code,
            };
        // SAFETY: checked non-null and writable; serialization and allocation already succeeded.
        unsafe { *decision_out = output };
        status::OK
    })
}

/// Destroys a live handle. Null is a no-op.
///
/// # Safety
///
/// `handle` must be null or a pointer returned by [`db_sim_match_create`] that has not already been
/// destroyed. No other call or reference may use it concurrently. Double destroy remains a caller
/// bug prevented by C# `SafeHandle`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_match_destroy(handle: *mut SimHandle) {
    if handle.is_null() {
        return;
    }
    // SAFETY: the caller contract guarantees unique ownership of this live allocation.
    drop(unsafe { Box::from_raw(handle) });
}

/// Frees one exact Rust-owned byte buffer and writes `{NULL, 0}` back. Null/empty are no-ops.
///
/// # Safety
///
/// `buffer` must be null or writable and must not lie within the allocation it describes. A
/// non-empty `ptr,len` pair must be exactly one returned by this library, not yet freed, and not in
/// concurrent use.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn db_sim_buffer_free(buffer: *mut DbOwnedBuffer) {
    if buffer.is_null() {
        return;
    }
    // SAFETY: `buffer` is writable by contract. Copy the allocation identity, clear the public
    // value first so wrapper-level repeat disposal is harmless, then reclaim the exact boxed slice.
    let (ptr, len) = unsafe {
        let ptr = (*buffer).ptr;
        let len = (*buffer).len;
        *buffer = DbOwnedBuffer::empty();
        (ptr, len)
    };
    if ptr.is_null() || len == 0 {
        return;
    }
    let slice = core::ptr::slice_from_raw_parts_mut(ptr, len);
    // SAFETY: the caller contract guarantees this exact pointer/length came from `boxed_buffer`.
    drop(unsafe { Box::from_raw(slice) });
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod tests;
