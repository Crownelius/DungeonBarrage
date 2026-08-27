//! Direct-Rust consumer for client/FFI match fixtures.
//!
//! The same request bytes consumed here are passed verbatim through the native ABI by the
//! later C# interop gate. Keeping this test at the session boundary proves that a fixture is
//! meaningful before marshalling is involved, while the frozen hashes catch simulation drift.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

use std::fs;
use std::path::{Component, Path, PathBuf};

use db_sim_core::client_contract::{
    CLIENT_CONTRACT_VERSION, ClientMatchPhase, MatchSnapshot, PlayerSnapshot,
};
use db_sim_core::hash::hash_state;
use db_sim_core::match_session::{
    AbilityPreviewRequest, MatchCommand, MatchCommandKind, MatchSessionHost, MatchTransition,
    PresentationEventKind, TransitionDisposition,
};
use db_sim_core::match_setup::{MatchConfig, MatchMode, MatchPlayerConfig};
use db_sim_core::types::{AbilitySlot, Appearance};
use db_sim_core::{CONTENT_VERSION, SIMULATION_VERSION};
use serde::Deserialize;

const FIXTURE_SCHEMA_VERSION: u32 = 1;
const INITIAL_ABI_VERSION: u32 = 1;
const PENDING_HASH: &str = "PENDING";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureManifest {
    fixture_schema_version: u32,
    id: String,
    purpose: String,
    versions: FixtureVersions,
    create_request_file: String,
    create_response_file: String,
    initial_snapshot_response_file: String,
    preview: PreviewExpectation,
    initial: InitialExpectation,
    steps: Vec<FixtureStep>,
    final_: FinalExpectation,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureVersions {
    abi_version: u32,
    client_contract_version: u32,
    simulation_version: u32,
    content_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitialExpectation {
    snapshot_generation: u64,
    turn_number: u32,
    phase: FixturePhase,
    active_player_id: String,
    player_count: usize,
    block_count: usize,
    state_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureStep {
    request_file: String,
    response_file: String,
    expect: StepExpectation,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviewExpectation {
    request_file: String,
    response_file: String,
    legal: bool,
    snapshot_generation: u64,
    minimum_projectile_traces: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StepExpectation {
    disposition: FixtureDisposition,
    pre_generation: u64,
    post_generation: u64,
    post_turn_number: u32,
    post_active_player_id: String,
    post_state_hash: String,
    minimum_actor_distance_moved: u32,
    minimum_projectile_traces: usize,
    minimum_projectile_samples: usize,
    minimum_damage_events: usize,
    minimum_terrain_cells_removed: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FinalExpectation {
    snapshot_generation: u64,
    turn_number: u32,
    active_player_id: String,
    state_hash: String,
    state_changed_from_initial: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum FixturePhase {
    MatchIntro,
    TurnStart,
    Movement,
    AimingAndSelection,
    PassiveSelection,
    CommandLocked,
    Resolution,
    Settling,
    StatusResolution,
    VictoryCheck,
    MatchComplete,
}

impl From<FixturePhase> for ClientMatchPhase {
    fn from(value: FixturePhase) -> Self {
        match value {
            FixturePhase::MatchIntro => Self::MatchIntro,
            FixturePhase::TurnStart => Self::TurnStart,
            FixturePhase::Movement => Self::Movement,
            FixturePhase::AimingAndSelection => Self::AimingAndSelection,
            FixturePhase::PassiveSelection => Self::PassiveSelection,
            FixturePhase::CommandLocked => Self::CommandLocked,
            FixturePhase::Resolution => Self::Resolution,
            FixturePhase::Settling => Self::Settling,
            FixturePhase::StatusResolution => Self::StatusResolution,
            FixturePhase::VictoryCheck => Self::VictoryCheck,
            FixturePhase::MatchComplete => Self::MatchComplete,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum FixtureDisposition {
    Accepted,
    Rejected,
    DuplicateReplay,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AbilityPreviewRequestDto {
    schema_version: u32,
    expected_snapshot_generation: u64,
    player_id: String,
    kind: PreviewKindDto,
    slot: AbilitySlotDto,
    angle_millidegrees: i32,
    power_basis_points: i32,
    target_player_id: Option<String>,
    secondary_target_player_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum PreviewKindDto {
    #[serde(rename = "ability")]
    Ability,
}

impl From<AbilityPreviewRequestDto> for AbilityPreviewRequest {
    fn from(value: AbilityPreviewRequestDto) -> Self {
        let AbilityPreviewRequestDto {
            schema_version,
            expected_snapshot_generation,
            player_id,
            kind: PreviewKindDto::Ability,
            slot,
            angle_millidegrees,
            power_basis_points,
            target_player_id,
            secondary_target_player_id,
        } = value;
        Self {
            schema_version,
            expected_snapshot_generation,
            player_id,
            slot: match slot {
                AbilitySlotDto::Basic => AbilitySlot::Basic,
                AbilitySlotDto::BasicAlt => AbilitySlot::BasicAlt,
                AbilitySlotDto::Special => AbilitySlot::Special,
            },
            angle_millidegrees,
            power_basis_points,
            target_player_id,
            secondary_target_player_id,
        }
    }
}

impl From<FixtureDisposition> for TransitionDisposition {
    fn from(value: FixtureDisposition) -> Self {
        match value {
            FixtureDisposition::Accepted => Self::Accepted,
            FixtureDisposition::Rejected => Self::Rejected,
            FixtureDisposition::DuplicateReplay => Self::DuplicateReplay,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MatchCreateRequestDto {
    schema_version: u32,
    match_id: String,
    simulation_version: u32,
    content_version: u32,
    #[serde(rename = "match")]
    match_config: MatchConfigDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MatchConfigDto {
    seed: u64,
    map_id: String,
    mode: MatchModeDto,
    players: Vec<MatchPlayerConfigDto>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MatchModeDto {
    TurnBased,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MatchPlayerConfigDto {
    player_id: String,
    team: u8,
    character_id: String,
    appearance: AppearanceDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AppearanceDto {
    skin_id: String,
    ability_skin_ids: [String; 3],
    victory_pose_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum MatchCommandDto {
    #[serde(rename = "move")]
    Move {
        #[serde(rename = "schemaVersion")]
        schema_version: u32,
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "playerId")]
        player_id: String,
        #[serde(rename = "expectedTurnNumber")]
        expected_turn_number: u32,
        #[serde(rename = "expectedSnapshotGeneration")]
        expected_snapshot_generation: u64,
        dx: i32,
    },
    #[serde(rename = "ability")]
    Ability {
        #[serde(rename = "schemaVersion")]
        schema_version: u32,
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "playerId")]
        player_id: String,
        #[serde(rename = "expectedTurnNumber")]
        expected_turn_number: u32,
        #[serde(rename = "expectedSnapshotGeneration")]
        expected_snapshot_generation: u64,
        slot: AbilitySlotDto,
        #[serde(rename = "angleMillidegrees")]
        angle_millidegrees: i32,
        #[serde(rename = "powerBasisPoints")]
        power_basis_points: i32,
        #[serde(
            rename = "targetPlayerId",
            deserialize_with = "deserialize_required_nullable_string"
        )]
        target_player_id: Option<String>,
        #[serde(
            rename = "secondaryTargetPlayerId",
            deserialize_with = "deserialize_required_nullable_string"
        )]
        secondary_target_player_id: Option<String>,
    },
    #[serde(rename = "passiveChoice")]
    PassiveChoice {
        #[serde(rename = "schemaVersion")]
        schema_version: u32,
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "playerId")]
        player_id: String,
        #[serde(rename = "expectedTurnNumber")]
        expected_turn_number: u32,
        #[serde(rename = "expectedSnapshotGeneration")]
        expected_snapshot_generation: u64,
        #[serde(rename = "passiveId")]
        passive_id: String,
    },
    #[serde(rename = "pass")]
    Pass {
        #[serde(rename = "schemaVersion")]
        schema_version: u32,
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "playerId")]
        player_id: String,
        #[serde(rename = "expectedTurnNumber")]
        expected_turn_number: u32,
        #[serde(rename = "expectedSnapshotGeneration")]
        expected_snapshot_generation: u64,
    },
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum AbilitySlotDto {
    #[serde(rename = "basic")]
    Basic,
    #[serde(rename = "basicAlt")]
    BasicAlt,
    #[serde(rename = "special")]
    Special,
}

fn deserialize_required_nullable_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

impl From<MatchConfigDto> for MatchConfig {
    fn from(value: MatchConfigDto) -> Self {
        Self {
            seed: value.seed,
            map_id: value.map_id,
            mode: match value.mode {
                MatchModeDto::TurnBased => MatchMode::TurnBased,
            },
            players: value.players.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<MatchPlayerConfigDto> for MatchPlayerConfig {
    fn from(value: MatchPlayerConfigDto) -> Self {
        Self {
            player_id: value.player_id,
            team: value.team,
            character_id: value.character_id,
            appearance: Appearance {
                skin_id: value.appearance.skin_id,
                ability_skin_ids: value.appearance.ability_skin_ids,
                victory_pose_id: value.appearance.victory_pose_id,
            },
        }
    }
}

impl From<MatchCommandDto> for MatchCommand {
    fn from(value: MatchCommandDto) -> Self {
        match value {
            MatchCommandDto::Move {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                dx,
            } => Self {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                kind: MatchCommandKind::Move { dx },
            },
            MatchCommandDto::Ability {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                slot,
                angle_millidegrees,
                power_basis_points,
                target_player_id,
                secondary_target_player_id,
            } => Self {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                kind: MatchCommandKind::Ability {
                    slot: match slot {
                        AbilitySlotDto::Basic => AbilitySlot::Basic,
                        AbilitySlotDto::BasicAlt => AbilitySlot::BasicAlt,
                        AbilitySlotDto::Special => AbilitySlot::Special,
                    },
                    angle_millidegrees,
                    power_basis_points,
                    target_player_id,
                    secondary_target_player_id,
                },
            },
            MatchCommandDto::PassiveChoice {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                passive_id,
            } => Self {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                kind: MatchCommandKind::PassiveChoice { passive_id },
            },
            MatchCommandDto::Pass {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
            } => Self {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                kind: MatchCommandKind::Pass,
            },
        }
    }
}

#[derive(Debug, Default)]
struct EventMetrics {
    projectile_traces: usize,
    projectile_samples: usize,
    damage_events: usize,
    terrain_cells_removed: u64,
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/matches/horizontal-test-duel-v1")
}

fn read_manifest(root: &Path) -> FixtureManifest {
    let path = root.join("fixture.json");
    let bytes = fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read fixture manifest `{}`: {error}",
            path.display()
        )
    });
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "fixture manifest `{}` does not match its closed schema: {error}",
            path.display()
        )
    })
}

fn resolve_fixture_file(root: &Path, relative: &str) -> PathBuf {
    let relative_path = Path::new(relative);
    assert!(
        !relative_path.as_os_str().is_empty()
            && relative_path
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "fixture file path must be non-empty and bundle-relative: `{relative}`"
    );
    root.join(relative_path)
}

fn read_wire_request<T: for<'de> Deserialize<'de>>(root: &Path, relative: &str) -> T {
    let path = resolve_fixture_file(root, relative);
    let bytes = fs::read(&path).unwrap_or_else(|error| {
        panic!("failed to read wire fixture `{}`: {error}", path.display())
    });

    assert!(
        !bytes.starts_with(&[0xef, 0xbb, 0xbf]),
        "wire fixture `{}` must not contain a UTF-8 BOM",
        path.display()
    );
    assert!(
        !bytes.contains(&b'\r'),
        "wire fixture `{}` must use LF, never CR or CRLF",
        path.display()
    );
    assert_eq!(
        bytes.last().copied(),
        Some(b'\n'),
        "wire fixture `{}` must end in exactly one LF",
        path.display()
    );
    assert_eq!(
        bytes.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "wire fixture `{}` must be one compact JSON line plus one terminal LF",
        path.display()
    );

    let Some(payload) = bytes.strip_suffix(b"\n") else {
        panic!(
            "terminal LF check and stripping disagreed for `{}`",
            path.display()
        );
    };
    let text = std::str::from_utf8(payload)
        .unwrap_or_else(|error| panic!("wire fixture `{}` is not UTF-8: {error}", path.display()));
    assert_compact_json(text, &path);

    serde_json::from_slice(payload).unwrap_or_else(|error| {
        panic!(
            "wire fixture `{}` does not match its closed DTO: {error}",
            path.display()
        )
    })
}

fn assert_compact_json(text: &str, path: &Path) {
    let mut inside_string = false;
    let mut escaped = false;

    for byte in text.bytes() {
        if escaped {
            escaped = false;
            continue;
        }
        if inside_string && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            inside_string = !inside_string;
        } else if !inside_string && byte.is_ascii_whitespace() {
            panic!(
                "wire fixture `{}` contains whitespace outside a JSON string; requests must be compact",
                path.display()
            );
        }
    }
}

fn find_player<'a>(snapshot: &'a MatchSnapshot, player_id: &str) -> &'a PlayerSnapshot {
    snapshot
        .players
        .iter()
        .find(|player| player.id == player_id)
        .unwrap_or_else(|| panic!("snapshot is missing fixture player `{player_id}`"))
}

fn actor_horizontal_distance(
    before: &MatchSnapshot,
    after: &MatchSnapshot,
    player_id: &str,
) -> u32 {
    find_player(before, player_id)
        .position
        .x
        .abs_diff(find_player(after, player_id).position.x)
}

fn event_metrics(transition: &MatchTransition) -> EventMetrics {
    let mut metrics = EventMetrics::default();
    let mut previous_tick = None;

    for (index, event) in transition.events.iter().enumerate() {
        let expected_sequence = u32::try_from(index).expect("fixture transition is bounded");
        assert_eq!(
            event.sequence, expected_sequence,
            "presentation event sequences must be contiguous and zero-based"
        );
        if let Some(tick) = previous_tick {
            assert!(
                event.presentation_tick >= tick,
                "presentation ticks must be nondecreasing"
            );
        }
        previous_tick = Some(event.presentation_tick);

        match &event.kind {
            PresentationEventKind::ProjectileTrace(trace) => {
                metrics.projectile_traces = metrics.projectile_traces.saturating_add(1);
                metrics.projectile_samples = metrics
                    .projectile_samples
                    .saturating_add(trace.samples.len());
            }
            PresentationEventKind::HealthChanged {
                previous_health,
                new_health,
                ..
            } if new_health < previous_health => {
                metrics.damage_events = metrics.damage_events.saturating_add(1);
            }
            PresentationEventKind::TerrainChanged {
                dirty_rectangles, ..
            } => {
                for rectangle in dirty_rectangles {
                    let cells =
                        u64::from(rectangle.width).saturating_mul(u64::from(rectangle.height));
                    metrics.terrain_cells_removed =
                        metrics.terrain_cells_removed.saturating_add(cells);
                }
            }
            _ => {}
        }
    }
    metrics
}

fn assert_hash_or_collect_pending(
    label: &str,
    expected: &str,
    actual: &str,
    pending: &mut Vec<String>,
) {
    assert!(
        actual.len() == 16
            && actual
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "actual hash for `{label}` is not sixteen lowercase hex digits: `{actual}`"
    );

    if expected == PENDING_HASH {
        pending.push(format!("{label} = \"{actual}\""));
        return;
    }

    assert!(
        expected.len() == 16
            && expected
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "frozen hash for `{label}` is malformed: `{expected}`"
    );
    assert_eq!(actual, expected, "shared fixture hash `{label}` changed");
}

fn assert_initial_snapshot(snapshot: &MatchSnapshot, expected: &InitialExpectation) {
    assert_eq!(snapshot.generation, expected.snapshot_generation);
    assert_eq!(snapshot.turn_number, expected.turn_number);
    assert_eq!(snapshot.phase, expected.phase.into());
    assert_eq!(
        snapshot.active_player_id.as_deref(),
        Some(expected.active_player_id.as_str())
    );
    assert_eq!(snapshot.players.len(), expected.player_count);
    assert_eq!(snapshot.blocks.len(), expected.block_count);
}

fn assert_step(
    session: &MatchSessionHost,
    before: &MatchSnapshot,
    actor_id: &str,
    transition: &MatchTransition,
    expected: &StepExpectation,
) {
    assert_eq!(transition.disposition, expected.disposition.into());
    assert_eq!(transition.pre_snapshot_generation, expected.pre_generation);
    assert_eq!(
        transition.post_snapshot_generation,
        expected.post_generation
    );
    assert_eq!(
        transition.post_snapshot.generation,
        expected.post_generation
    );
    assert_eq!(session.generation(), expected.post_generation);
    assert_eq!(
        transition.post_snapshot.turn_number,
        expected.post_turn_number
    );
    assert_eq!(
        transition.post_snapshot.active_player_id.as_deref(),
        Some(expected.post_active_player_id.as_str())
    );
    assert_eq!(
        transition.post_state_hash,
        transition.post_snapshot.authoritative_state_hash
    );
    assert_eq!(
        transition.post_state_hash,
        hash_state(session.host().state()),
        "transition, snapshot, and live host must describe one atomic state"
    );

    let moved = actor_horizontal_distance(before, &transition.post_snapshot, actor_id);
    assert!(
        moved >= expected.minimum_actor_distance_moved,
        "actor `{actor_id}` moved {moved} fixed-point units; fixture requires at least {}",
        expected.minimum_actor_distance_moved
    );

    let metrics = event_metrics(transition);
    assert!(
        metrics.projectile_traces >= expected.minimum_projectile_traces,
        "transition produced {} projectile traces; fixture requires at least {}",
        metrics.projectile_traces,
        expected.minimum_projectile_traces
    );
    assert!(
        metrics.projectile_samples >= expected.minimum_projectile_samples,
        "transition produced {} projectile samples; fixture requires at least {}",
        metrics.projectile_samples,
        expected.minimum_projectile_samples
    );
    assert!(
        metrics.damage_events >= expected.minimum_damage_events,
        "transition produced {} damage events; fixture requires at least {}",
        metrics.damage_events,
        expected.minimum_damage_events
    );
    assert!(
        metrics.terrain_cells_removed >= expected.minimum_terrain_cells_removed,
        "transition changed {} terrain cells; fixture requires at least {}",
        metrics.terrain_cells_removed,
        expected.minimum_terrain_cells_removed
    );
}

#[test]
fn horizontal_test_duel_replays_through_the_direct_session_contract() {
    let root = fixture_root();
    let manifest = read_manifest(&root);

    assert_eq!(manifest.fixture_schema_version, FIXTURE_SCHEMA_VERSION);
    assert_eq!(manifest.versions.abi_version, INITIAL_ABI_VERSION);
    assert_eq!(
        manifest.versions.client_contract_version,
        CLIENT_CONTRACT_VERSION
    );
    assert_eq!(manifest.versions.simulation_version, SIMULATION_VERSION);
    assert_eq!(manifest.versions.content_version, CONTENT_VERSION);
    assert!(
        !manifest.id.is_empty(),
        "fixture id must explain its identity"
    );
    assert!(
        !manifest.purpose.is_empty(),
        "fixture purpose must explain the behavior it freezes"
    );

    let create: MatchCreateRequestDto = read_wire_request(&root, &manifest.create_request_file);
    let _create_response: serde_json::Value =
        read_wire_request(&root, &manifest.create_response_file);
    let _initial_snapshot_response: serde_json::Value =
        read_wire_request(&root, &manifest.initial_snapshot_response_file);
    assert_eq!(create.schema_version, CLIENT_CONTRACT_VERSION);
    assert_eq!(create.simulation_version, SIMULATION_VERSION);
    assert_eq!(create.content_version, CONTENT_VERSION);
    assert!(
        !create.match_id.is_empty(),
        "the adapter-owned fixture match id must be present"
    );

    let mut session = MatchSessionHost::create(&create.match_config.into())
        .expect("the reviewed shared fixture must create a real match");
    let initial = session.snapshot();
    assert_initial_snapshot(&initial, &manifest.initial);

    let mut pending_hashes = Vec::new();
    assert_hash_or_collect_pending(
        "initial.stateHash",
        &manifest.initial.state_hash,
        &initial.authoritative_state_hash,
        &mut pending_hashes,
    );

    let preview_request: AbilityPreviewRequestDto =
        read_wire_request(&root, &manifest.preview.request_file);
    let _preview_response: serde_json::Value =
        read_wire_request(&root, &manifest.preview.response_file);
    let preview = session
        .preview(&preview_request.into())
        .expect("the shared preview fixture must not fault");
    assert_eq!(preview.legal, manifest.preview.legal);
    assert_eq!(
        preview.snapshot_generation,
        manifest.preview.snapshot_generation
    );
    assert!(
        preview.projectile_traces.len() >= manifest.preview.minimum_projectile_traces,
        "preview produced {} traces; fixture requires at least {}",
        preview.projectile_traces.len(),
        manifest.preview.minimum_projectile_traces
    );

    let mut last_transition_hash = initial.authoritative_state_hash.clone();
    for (index, step) in manifest.steps.iter().enumerate() {
        let wire: MatchCommandDto = read_wire_request(&root, &step.request_file);
        let _response: serde_json::Value = read_wire_request(&root, &step.response_file);
        let command: MatchCommand = wire.into();
        let actor_id = command.player_id.clone();
        let before = session.snapshot();
        let transition = session.apply(command).unwrap_or_else(|error| {
            panic!(
                "fixture step {} (`{}`) faulted: {error}",
                index + 1,
                step.request_file
            )
        });

        assert_step(&session, &before, &actor_id, &transition, &step.expect);
        let label = format!("steps[{index}].expect.postStateHash");
        assert_hash_or_collect_pending(
            &label,
            &step.expect.post_state_hash,
            &transition.post_state_hash,
            &mut pending_hashes,
        );
        last_transition_hash = transition.post_state_hash;
    }

    let final_snapshot = session.snapshot();
    assert_eq!(
        final_snapshot.generation,
        manifest.final_.snapshot_generation
    );
    assert_eq!(final_snapshot.turn_number, manifest.final_.turn_number);
    assert_eq!(
        final_snapshot.active_player_id.as_deref(),
        Some(manifest.final_.active_player_id.as_str())
    );
    assert_eq!(
        final_snapshot.authoritative_state_hash, last_transition_hash,
        "the final snapshot must equal the last accepted transition"
    );
    assert_eq!(
        final_snapshot.authoritative_state_hash != initial.authoritative_state_hash,
        manifest.final_.state_changed_from_initial
    );
    assert_hash_or_collect_pending(
        "final.stateHash",
        &manifest.final_.state_hash,
        &final_snapshot.authoritative_state_hash,
        &mut pending_hashes,
    );

    assert!(
        pending_hashes.is_empty(),
        "shared fixture still contains PENDING hashes; replace them with these reviewed values:\n{}",
        pending_hashes.join("\n")
    );
}
