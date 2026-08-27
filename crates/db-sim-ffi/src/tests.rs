use super::*;
use serde_json::Value;

const CREATE_REQUEST: &[u8] =
    include_bytes!("../../../tests/fixtures/matches/horizontal-test-duel-v1/create-request.json");
const MOVE_REQUEST: &[u8] = include_bytes!(
    "../../../tests/fixtures/matches/horizontal-test-duel-v1/commands/001-move.json"
);
const ABILITY_REQUEST: &[u8] = include_bytes!(
    "../../../tests/fixtures/matches/horizontal-test-duel-v1/commands/002-ability.json"
);
const PREVIEW_REQUEST: &[u8] = include_bytes!(
    "../../../tests/fixtures/matches/horizontal-test-duel-v1/previews/001-basic.json"
);
const CREATE_RESPONSE: &[u8] =
    include_bytes!("../../../tests/fixtures/matches/horizontal-test-duel-v1/responses/create.json");
const INITIAL_SNAPSHOT_RESPONSE: &[u8] = include_bytes!(
    "../../../tests/fixtures/matches/horizontal-test-duel-v1/responses/snapshot-initial.json"
);
const PREVIEW_RESPONSE: &[u8] = include_bytes!(
    "../../../tests/fixtures/matches/horizontal-test-duel-v1/responses/preview-basic.json"
);
const MOVE_RESPONSE: &[u8] = include_bytes!(
    "../../../tests/fixtures/matches/horizontal-test-duel-v1/responses/001-move.json"
);
const ABILITY_RESPONSE: &[u8] = include_bytes!(
    "../../../tests/fixtures/matches/horizontal-test-duel-v1/responses/002-ability.json"
);

unsafe fn create(bytes: &[u8]) -> (c_int, *mut SimHandle, DbOwnedBuffer) {
    let mut handle = core::ptr::null_mut();
    let mut output = DbOwnedBuffer::empty();
    // SAFETY: the byte slice and output pointers remain valid for the call.
    let code =
        unsafe { db_sim_match_create(bytes.as_ptr(), bytes.len(), &mut handle, &mut output) };
    (code, handle, output)
}

unsafe fn apply(handle: *mut SimHandle, bytes: &[u8]) -> (c_int, DbOwnedBuffer) {
    let mut output = DbOwnedBuffer::empty();
    // SAFETY: tests pass a live handle and a valid byte slice.
    let code = unsafe { db_sim_match_apply(handle, bytes.as_ptr(), bytes.len(), &mut output) };
    (code, output)
}

unsafe fn json_and_free(buffer: &mut DbOwnedBuffer) -> Value {
    assert!(!buffer.ptr.is_null());
    // SAFETY: `buffer` is a live allocation returned by this library.
    let bytes = unsafe { core::slice::from_raw_parts(buffer.ptr, buffer.len) };
    let value = serde_json::from_slice(bytes).expect("ABI output must be valid JSON");
    // SAFETY: freed exactly once; the function clears the value.
    unsafe { db_sim_buffer_free(buffer) };
    assert!(buffer.ptr.is_null());
    assert_eq!(buffer.len, 0);
    value
}

unsafe fn fixture_json_and_free(buffer: &mut DbOwnedBuffer, expected: &[u8]) -> Value {
    assert!(!buffer.ptr.is_null());
    // SAFETY: `buffer` is a live allocation returned by this library.
    let actual = unsafe { core::slice::from_raw_parts(buffer.ptr, buffer.len) };
    assert_eq!(actual, expected, "production wire bytes changed");
    // SAFETY: ownership remains with `buffer`; the delegated helper frees it exactly once.
    unsafe { json_and_free(buffer) }
}

unsafe fn destroy(handle: *mut SimHandle) {
    // SAFETY: every helper caller passes a live handle exactly once.
    unsafe { db_sim_match_destroy(handle) };
}

#[test]
fn versions_are_exactly_reexported() {
    assert_eq!(db_sim_abi_version(), ABI_VERSION);
    assert_eq!(db_sim_simulation_version(), db_sim_core::SIMULATION_VERSION);
    assert_eq!(db_sim_content_version(), db_sim_core::CONTENT_VERSION);
}

#[test]
fn shared_fixture_runs_through_the_real_c_abi_with_the_direct_hashes() {
    // SAFETY: every pointer used below comes from a live Rust value and every allocation is freed.
    unsafe {
        let (code, handle, mut created_buffer) = create(CREATE_REQUEST);
        assert_eq!(code, status::OK);
        assert!(!handle.is_null());
        let created = fixture_json_and_free(&mut created_buffer, CREATE_RESPONSE);
        assert_eq!(created["created"], true);
        assert_eq!(created["snapshot"]["abiVersion"], ABI_VERSION);
        assert_eq!(created["snapshot"]["matchId"], "fixture-horizontal-duel-v1");
        assert_eq!(created["snapshot"]["mapId"], "horizontal-test-array");
        assert_eq!(created["snapshot"]["stateHash"], "f67c5371bcddbdf5");

        let mut snapshot_buffer = DbOwnedBuffer::empty();
        assert_eq!(
            db_sim_match_snapshot(handle, &mut snapshot_buffer),
            status::OK
        );
        let snapshot = fixture_json_and_free(&mut snapshot_buffer, INITIAL_SNAPSHOT_RESPONSE);
        assert_eq!(snapshot["snapshotGeneration"], 0);
        assert_eq!(snapshot["players"].as_array().map(Vec::len), Some(2));
        assert_eq!(snapshot["blocks"].as_array().map(Vec::len), Some(8));

        let mut preview_buffer = DbOwnedBuffer::empty();
        assert_eq!(
            db_sim_match_preview(
                handle,
                PREVIEW_REQUEST.as_ptr(),
                PREVIEW_REQUEST.len(),
                &mut preview_buffer,
            ),
            status::OK
        );
        let preview = fixture_json_and_free(&mut preview_buffer, PREVIEW_RESPONSE);
        assert_eq!(preview["legal"], true);
        assert_eq!(preview["snapshotGeneration"], 0);
        assert!(
            preview["projectileTraces"]
                .as_array()
                .is_some_and(|traces| !traces.is_empty())
        );

        let mut width = 0;
        let mut height = 0;
        let mut generation = 0;
        let mut terrain = DbOwnedBuffer::empty();
        assert_eq!(
            db_sim_match_terrain(
                handle,
                u64::MAX,
                &mut width,
                &mut height,
                &mut generation,
                &mut terrain,
            ),
            status::OK
        );
        assert_eq!(
            terrain.len,
            usize::try_from(width)
                .expect("fixture width")
                .checked_mul(usize::try_from(height).expect("fixture height"))
                .expect("fixture dimensions")
        );
        db_sim_buffer_free(&mut terrain);
        assert_eq!(
            db_sim_match_terrain(
                handle,
                generation,
                &mut width,
                &mut height,
                &mut generation,
                &mut terrain,
            ),
            status::OK
        );
        assert!(terrain.ptr.is_null());
        assert_eq!(terrain.len, 0);

        let (move_code, mut move_buffer) = apply(handle, MOVE_REQUEST);
        assert_eq!(move_code, status::OK);
        let moved = fixture_json_and_free(&mut move_buffer, MOVE_RESPONSE);
        assert_eq!(moved["disposition"], "accepted");
        assert_eq!(moved["postSnapshotGeneration"], 1);
        assert_eq!(moved["postStateHash"], "378081bb2e830a5d");

        let (ability_code, mut ability_buffer) = apply(handle, ABILITY_REQUEST);
        assert_eq!(ability_code, status::OK);
        let ability = fixture_json_and_free(&mut ability_buffer, ABILITY_RESPONSE);
        assert_eq!(ability["disposition"], "accepted");
        assert_eq!(ability["postSnapshotGeneration"], 2);
        assert_eq!(ability["postStateHash"], "d8686762470c0c36");
        assert_eq!(ability["postSnapshot"]["stateHash"], "d8686762470c0c36");
        assert!(ability["events"].as_array().is_some_and(|events| {
            events
                .iter()
                .any(|event| event["kind"] == "projectileTrace")
        }));

        destroy(handle);
    }
}

#[test]
fn malformed_unknown_duplicate_and_unsupported_create_inputs_fail_closed() {
    let malformed_cases: &[&[u8]] = &[
        b"not json",
        &[0xff, 0xfe],
        br#"{"schemaVersion":1,"schemaVersion":1,"matchId":"x","simulationVersion":6,"contentVersion":1,"match":{}}"#,
        br#"{"schemaVersion":1,"matchId":"x","simulationVersion":6,"contentVersion":1,"unknown":0,"match":{}}"#,
        br#"{"schemaVersion":1,"matchId":"x","simulationVersion":6,"contentVersion":1,"match":{"seed":1.5,"mapId":"horizontal-test-array","mode":"turnBased","players":[]}}"#,
        br#"{"schemaVersion":1,"matchId":"x","simulationVersion":6,"contentVersion":1,"match":{"seed":1,"mapId":"horizontal-test-array","mode":"realtime","players":[]}}"#,
        b"{} trailing",
    ];
    for bytes in malformed_cases {
        // SAFETY: helper owns valid byte/output storage.
        let (code, handle, output) = unsafe { create(bytes) };
        assert_eq!(code, status::MALFORMED_ENVELOPE, "bytes: {bytes:?}");
        assert!(handle.is_null());
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);
    }

    let unsupported = CREATE_REQUEST.to_vec();
    let unsupported_text = String::from_utf8(unsupported)
        .expect("fixture UTF-8")
        .replace("\"schemaVersion\":1", "\"schemaVersion\":2");
    // SAFETY: helper owns valid byte/output storage.
    let (code, handle, output) = unsafe { create(unsupported_text.as_bytes()) };
    assert_eq!(code, status::UNSUPPORTED_VERSION);
    assert!(handle.is_null());
    assert!(output.ptr.is_null());
}

#[test]
fn create_rejects_oversized_player_collections_at_the_wire_boundary() {
    let mut request: Value = serde_json::from_slice(CREATE_REQUEST).expect("fixture JSON");
    let first_player = request["match"]["players"][0].clone();
    request["match"]["players"] = Value::Array(vec![first_player; 5]);
    let bytes = serde_json::to_vec(&request).expect("negative-case JSON");

    // SAFETY: helper owns valid request and output storage.
    let (code, handle, output) = unsafe { create(&bytes) };
    assert_eq!(code, status::MALFORMED_ENVELOPE);
    assert!(handle.is_null());
    assert!(output.ptr.is_null());
    assert_eq!(output.len, 0);
}

#[test]
fn oversized_and_overdeep_inputs_are_rejected_before_dereferencing_the_claimed_length() {
    let byte = 0u8;
    let mut handle = core::ptr::null_mut();
    let mut output = DbOwnedBuffer {
        ptr: core::ptr::dangling_mut::<u8>(),
        len: 99,
    };
    // SAFETY: the oversized length is rejected before the one-byte pointer is made into a slice.
    let code = unsafe { db_sim_match_create(&byte, MAX_INPUT_BYTES + 1, &mut handle, &mut output) };
    assert_eq!(code, status::MALFORMED_ENVELOPE);
    assert!(handle.is_null());
    assert!(output.ptr.is_null());
    assert_eq!(output.len, 0);

    let deep = format!("{}0{}", "[".repeat(13), "]".repeat(13));
    // SAFETY: helper owns valid bytes/output pointers.
    let (code, handle, output) = unsafe { create(deep.as_bytes()) };
    assert_eq!(code, status::MALFORMED_ENVELOPE);
    assert!(handle.is_null());
    assert!(output.ptr.is_null());
}

#[test]
fn invalid_domain_config_is_an_ok_response_with_no_handle() {
    let invalid = String::from_utf8(CREATE_REQUEST.to_vec())
        .expect("fixture UTF-8")
        .replace("horizontal-test-array", "missing-map");
    // SAFETY: helper owns valid bytes/output pointers.
    let (code, handle, mut output) = unsafe { create(invalid.as_bytes()) };
    assert_eq!(code, status::OK);
    assert!(handle.is_null());
    // SAFETY: returned output is a live library allocation.
    let response = unsafe { json_and_free(&mut output) };
    assert_eq!(response["created"], false);
    assert_eq!(response["diagnostic"]["code"], "invalidConfig");
    assert!(response["snapshot"].is_null());
}

#[test]
fn command_parser_requires_nullable_fields_and_rejects_unknowns_without_mutation() {
    // SAFETY: helper owns all pointers and destroys the handle once.
    unsafe {
        let (code, handle, mut create_output) = create(CREATE_REQUEST);
        assert_eq!(code, status::OK);
        let _created = json_and_free(&mut create_output);

        let missing_nullable = br#"{"schemaVersion":1,"commandId":"missing-null","playerId":"a-local-player","expectedTurnNumber":1,"expectedSnapshotGeneration":0,"kind":"ability","slot":"basic","angleMillidegrees":45000,"powerBasisPoints":1500}"#;
        let unknown = br#"{"schemaVersion":1,"commandId":"unknown-field","playerId":"a-local-player","expectedTurnNumber":1,"expectedSnapshotGeneration":0,"kind":"pass","dx":1}"#;
        let duplicate = br#"{"schemaVersion":1,"commandId":"duplicate","commandId":"duplicate","playerId":"a-local-player","expectedTurnNumber":1,"expectedSnapshotGeneration":0,"kind":"pass"}"#;
        let unknown_kind = br#"{"schemaVersion":1,"commandId":"unknown-kind","playerId":"a-local-player","expectedTurnNumber":1,"expectedSnapshotGeneration":0,"kind":"jump"}"#;
        let unknown_slot = br#"{"schemaVersion":1,"commandId":"unknown-slot","playerId":"a-local-player","expectedTurnNumber":1,"expectedSnapshotGeneration":0,"kind":"ability","slot":"ultimate","angleMillidegrees":45000,"powerBasisPoints":1500,"targetPlayerId":null,"secondaryTargetPlayerId":null}"#;
        let float_move = br#"{"schemaVersion":1,"commandId":"float-move","playerId":"a-local-player","expectedTurnNumber":1,"expectedSnapshotGeneration":0,"kind":"move","dx":1.5}"#;
        let trailing = br#"{"schemaVersion":1,"commandId":"trailing","playerId":"a-local-player","expectedTurnNumber":1,"expectedSnapshotGeneration":0,"kind":"pass"} trailing"#;
        for bytes in [
            missing_nullable.as_slice(),
            unknown.as_slice(),
            duplicate.as_slice(),
            unknown_kind.as_slice(),
            unknown_slot.as_slice(),
            float_move.as_slice(),
            trailing.as_slice(),
        ] {
            let (apply_code, output) = apply(handle, bytes);
            assert_eq!(
                apply_code,
                status::MALFORMED_ENVELOPE,
                "input unexpectedly accepted: {}",
                String::from_utf8_lossy(bytes)
            );
            assert!(output.ptr.is_null());
            assert_eq!(output.len, 0);
        }

        let preview_missing_nullable = br#"{"schemaVersion":1,"expectedSnapshotGeneration":0,"playerId":"a-local-player","kind":"ability","slot":"basic","angleMillidegrees":45000,"powerBasisPoints":1500}"#;
        let preview_unknown = br#"{"schemaVersion":1,"expectedSnapshotGeneration":0,"playerId":"a-local-player","kind":"ability","slot":"basic","angleMillidegrees":45000,"powerBasisPoints":1500,"targetPlayerId":null,"secondaryTargetPlayerId":null,"extra":0}"#;
        let preview_kind = br#"{"schemaVersion":1,"expectedSnapshotGeneration":0,"playerId":"a-local-player","kind":"trajectory","slot":"basic","angleMillidegrees":45000,"powerBasisPoints":1500,"targetPlayerId":null,"secondaryTargetPlayerId":null}"#;
        for bytes in [
            preview_missing_nullable.as_slice(),
            preview_unknown.as_slice(),
            preview_kind.as_slice(),
        ] {
            let mut output = DbOwnedBuffer {
                ptr: core::ptr::dangling_mut::<u8>(),
                len: 77,
            };
            assert_eq!(
                db_sim_match_preview(handle, bytes.as_ptr(), bytes.len(), &mut output),
                status::MALFORMED_ENVELOPE,
                "preview input unexpectedly accepted: {}",
                String::from_utf8_lossy(bytes)
            );
            assert!(output.ptr.is_null());
            assert_eq!(output.len, 0);
        }

        let mut snapshot_output = DbOwnedBuffer::empty();
        assert_eq!(
            db_sim_match_snapshot(handle, &mut snapshot_output),
            status::OK
        );
        let snapshot = json_and_free(&mut snapshot_output);
        assert_eq!(snapshot["snapshotGeneration"], 0);
        assert_eq!(snapshot["stateHash"], "f67c5371bcddbdf5");
        destroy(handle);
    }
}

#[test]
fn output_cap_failure_does_not_commit_the_working_session() {
    // SAFETY: helper owns all pointers and destroys the handle once.
    unsafe {
        let (code, handle, mut output) = create(CREATE_REQUEST);
        assert_eq!(code, status::OK);
        let _created = json_and_free(&mut output);
        let handle_ref = &*handle;
        let request: wire::MatchCommandDto = decode_json(MOVE_REQUEST).expect("fixture command");
        let command = request.into_core();
        let mut inner = lock_handle(handle_ref).expect("live handle lock");
        let before_snapshot = inner.session.snapshot();
        let before_ledger_len = inner.session.ledger_len();
        let before_ledger_bytes = inner.session.ledger_bytes();

        assert!(matches!(
            apply_serialized(handle_ref, &mut inner, command, 0),
            Err(status::RESPONSE_TOO_LARGE)
        ));
        assert_eq!(inner.session.snapshot(), before_snapshot);
        assert_eq!(inner.session.ledger_len(), before_ledger_len);
        assert_eq!(inner.session.ledger_bytes(), before_ledger_bytes);
        drop(inner);

        let mut snapshot_output = DbOwnedBuffer::empty();
        assert_eq!(
            db_sim_match_snapshot(handle, &mut snapshot_output),
            status::OK
        );
        let snapshot = json_and_free(&mut snapshot_output);
        assert_eq!(snapshot["snapshotGeneration"], 0);
        destroy(handle);
    }
}

#[test]
fn controlled_panic_poisoning_is_caught_in_the_release_profile_path() {
    // SAFETY: helper owns the handle and output values.
    unsafe {
        let (code, handle, mut output) = create(CREATE_REQUEST);
        assert_eq!(code, status::OK);
        let _created = json_and_free(&mut output);
        let handle_ref = &*handle;

        assert_eq!(
            guard(Some(handle_ref), || panic!("controlled guard test")),
            status::INTERNAL_PANIC
        );
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 123,
        };
        assert_eq!(
            db_sim_match_snapshot(handle, &mut output),
            status::INTERNAL_PANIC
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 123,
        };
        assert_eq!(
            db_sim_match_apply(
                handle,
                MOVE_REQUEST.as_ptr(),
                MOVE_REQUEST.len(),
                &mut output,
            ),
            status::INTERNAL_PANIC
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        let oversized = vec![b' '; MAX_INPUT_BYTES + 1];
        let unsupported_move = std::str::from_utf8(MOVE_REQUEST)
            .expect("move fixture is UTF-8")
            .replacen("\"schemaVersion\":1", "\"schemaVersion\":2", 1);
        for bytes in [
            b"{".as_slice(),
            unsupported_move.as_bytes(),
            oversized.as_slice(),
        ] {
            output = DbOwnedBuffer {
                ptr: core::ptr::dangling_mut::<u8>(),
                len: 123,
            };
            assert_eq!(
                db_sim_match_apply(handle, bytes.as_ptr(), bytes.len(), &mut output),
                status::INTERNAL_PANIC,
                "poison must precede apply request validation"
            );
            assert!(output.ptr.is_null());
            assert_eq!(output.len, 0);
        }
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 123,
        };
        assert_eq!(
            db_sim_match_apply(handle, core::ptr::null(), 0, &mut output),
            status::INTERNAL_PANIC
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        let mut width = 99;
        let mut height = 99;
        let mut generation = 99;
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 123,
        };
        assert_eq!(
            db_sim_match_terrain(
                handle,
                u64::MAX,
                &mut width,
                &mut height,
                &mut generation,
                &mut output,
            ),
            status::INTERNAL_PANIC
        );
        assert_eq!((width, height, generation), (0, 0, 0));
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 123,
        };
        assert_eq!(
            db_sim_match_preview(
                handle,
                PREVIEW_REQUEST.as_ptr(),
                PREVIEW_REQUEST.len(),
                &mut output,
            ),
            status::INTERNAL_PANIC
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        let unsupported_preview = std::str::from_utf8(PREVIEW_REQUEST)
            .expect("preview fixture is UTF-8")
            .replacen("\"schemaVersion\":1", "\"schemaVersion\":2", 1);
        for bytes in [
            b"{".as_slice(),
            unsupported_preview.as_bytes(),
            oversized.as_slice(),
        ] {
            output = DbOwnedBuffer {
                ptr: core::ptr::dangling_mut::<u8>(),
                len: 123,
            };
            assert_eq!(
                db_sim_match_preview(handle, bytes.as_ptr(), bytes.len(), &mut output),
                status::INTERNAL_PANIC,
                "poison must precede preview request validation"
            );
            assert!(output.ptr.is_null());
            assert_eq!(output.len, 0);
        }
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 123,
        };
        assert_eq!(
            db_sim_match_preview(handle, core::ptr::null(), 0, &mut output),
            status::INTERNAL_PANIC
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);
        destroy(handle);
    }
}

#[test]
fn every_negative_status_initializes_each_non_null_output() {
    // SAFETY: every non-null pointer below names live writable storage; oversized claimed input
    // lengths are rejected before their one-byte backing pointer can be dereferenced.
    unsafe {
        let mut handle = core::ptr::dangling_mut::<SimHandle>();
        let mut output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 41,
        };
        assert_eq!(
            db_sim_match_create(
                CREATE_REQUEST.as_ptr(),
                CREATE_REQUEST.len(),
                &mut handle,
                core::ptr::null_mut(),
            ),
            status::NULL_POINTER
        );
        assert!(handle.is_null());

        assert_eq!(
            db_sim_match_create(
                CREATE_REQUEST.as_ptr(),
                CREATE_REQUEST.len(),
                core::ptr::null_mut(),
                &mut output,
            ),
            status::NULL_POINTER
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        let (code, live_handle, mut created) = create(CREATE_REQUEST);
        assert_eq!(code, status::OK);
        let _created = json_and_free(&mut created);

        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 41,
        };
        assert_eq!(
            db_sim_match_apply(
                core::ptr::null_mut(),
                MOVE_REQUEST.as_ptr(),
                MOVE_REQUEST.len(),
                &mut output,
            ),
            status::NULL_POINTER
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        let one_byte = 0u8;
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 41,
        };
        assert_eq!(
            db_sim_match_apply(live_handle, &one_byte, MAX_INPUT_BYTES + 1, &mut output,),
            status::MALFORMED_ENVELOPE
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        let unsupported_command = String::from_utf8(MOVE_REQUEST.to_vec())
            .expect("fixture UTF-8")
            .replace("\"schemaVersion\":1", "\"schemaVersion\":2");
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 41,
        };
        assert_eq!(
            db_sim_match_apply(
                live_handle,
                unsupported_command.as_ptr(),
                unsupported_command.len(),
                &mut output,
            ),
            status::UNSUPPORTED_VERSION
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        let mut width = 41;
        let mut height = 42;
        let mut generation = 43;
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 44,
        };
        assert_eq!(
            db_sim_match_terrain(
                live_handle,
                0,
                core::ptr::null_mut(),
                &mut height,
                &mut generation,
                &mut output,
            ),
            status::NULL_POINTER
        );
        assert_eq!(width, 41);
        assert_eq!((height, generation), (0, 0));
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        width = 41;
        height = 42;
        generation = 43;
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 44,
        };
        assert_eq!(
            db_sim_match_terrain(
                core::ptr::null(),
                0,
                &mut width,
                &mut height,
                &mut generation,
                &mut output,
            ),
            status::NULL_POINTER
        );
        assert_eq!((width, height, generation), (0, 0, 0));
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 41,
        };
        assert_eq!(
            db_sim_match_preview(live_handle, &one_byte, MAX_INPUT_BYTES + 1, &mut output,),
            status::MALFORMED_ENVELOPE
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        let unsupported_preview = String::from_utf8(PREVIEW_REQUEST.to_vec())
            .expect("fixture UTF-8")
            .replace("\"schemaVersion\":1", "\"schemaVersion\":2");
        output = DbOwnedBuffer {
            ptr: core::ptr::dangling_mut::<u8>(),
            len: 41,
        };
        assert_eq!(
            db_sim_match_preview(
                live_handle,
                unsupported_preview.as_ptr(),
                unsupported_preview.len(),
                &mut output,
            ),
            status::UNSUPPORTED_VERSION
        );
        assert!(output.ptr.is_null());
        assert_eq!(output.len, 0);

        destroy(live_handle);
    }
}

#[test]
fn owned_buffer_limit_is_exact_and_failure_allocates_nothing() {
    assert!(matches!(
        boxed_buffer_with_limit(vec![1, 2, 3], 2),
        Err(status::RESPONSE_TOO_LARGE)
    ));
    let mut exact = boxed_buffer_with_limit(vec![1, 2, 3], 3).expect("exact cap is allowed");
    // SAFETY: exact is one live library-owned allocation.
    unsafe { db_sim_buffer_free(&mut exact) };
    assert!(exact.ptr.is_null());
    assert_eq!(exact.len, 0);
}

#[test]
fn nulls_and_buffer_disposal_are_total_for_documented_forms() {
    // SAFETY: null is explicitly accepted by both disposal functions.
    unsafe {
        db_sim_match_destroy(core::ptr::null_mut());
        db_sim_buffer_free(core::ptr::null_mut());
    }
    let mut output = boxed_buffer(b"owned".to_vec()).expect("small output");
    assert!(!output.ptr.is_null());
    // SAFETY: exact live allocation, then documented empty repeat.
    unsafe {
        db_sim_buffer_free(&mut output);
        db_sim_buffer_free(&mut output);
    }
    assert!(output.ptr.is_null());
    assert_eq!(output.len, 0);

    let mut snapshot = DbOwnedBuffer {
        ptr: core::ptr::dangling_mut::<u8>(),
        len: 12,
    };
    // SAFETY: null handle is a documented negative-path input; output is writable.
    let status_code = unsafe { db_sim_match_snapshot(core::ptr::null(), &mut snapshot) };
    assert_eq!(status_code, status::NULL_POINTER);
    assert!(snapshot.ptr.is_null());
    assert_eq!(snapshot.len, 0);
}

#[test]
fn repeated_create_apply_preview_snapshot_terrain_and_destroy_cycles_complete_cleanly() {
    for _ in 0..64 {
        // SAFETY: each iteration owns one handle and every buffer is freed before destruction.
        unsafe {
            let (code, handle, mut created) = create(CREATE_REQUEST);
            assert_eq!(code, status::OK);
            let _value = json_and_free(&mut created);

            let mut preview = DbOwnedBuffer::empty();
            assert_eq!(
                db_sim_match_preview(
                    handle,
                    PREVIEW_REQUEST.as_ptr(),
                    PREVIEW_REQUEST.len(),
                    &mut preview,
                ),
                status::OK
            );
            let _value = json_and_free(&mut preview);

            let (code, mut transition) = apply(handle, MOVE_REQUEST);
            assert_eq!(code, status::OK);
            let _value = json_and_free(&mut transition);

            let mut snapshot = DbOwnedBuffer::empty();
            assert_eq!(db_sim_match_snapshot(handle, &mut snapshot), status::OK);
            let _value = json_and_free(&mut snapshot);

            let mut width = 0;
            let mut height = 0;
            let mut generation = 0;
            let mut cells = DbOwnedBuffer::empty();
            assert_eq!(
                db_sim_match_terrain(
                    handle,
                    u64::MAX,
                    &mut width,
                    &mut height,
                    &mut generation,
                    &mut cells,
                ),
                status::OK
            );
            db_sim_buffer_free(&mut cells);
            destroy(handle);
        }
    }
}
