//! Strict JSON wire DTOs for the coarse client ABI.
//!
//! The authoritative core intentionally has no serialization dependency.  This adapter owns the
//! camel-case envelope, closed string enums, and the composite session metadata described by
//! `CLIENT_SPEC.md` sections 7 and 8.  Every input struct denies unknown fields; serde's derived
//! struct visitors also reject duplicate known fields.

use core::fmt;
use db_sim_core::bot::BotDifficulty;
use db_sim_core::character;
use db_sim_core::client_contract::{
    AppearanceSnapshot, BlockSnapshot, ClientErosionAxis, ClientMatchOutcome, ClientMatchPhase,
    ClientMaterial, ClientObjectKind, ClientStatusKind, ClientTurnEndReason, MatchSnapshot,
    PersistentObjectSnapshot, PlayerSnapshot, PositionSnapshot, StatusSnapshot,
};
use db_sim_core::match_session::{
    AbilityPreviewRequest, AbilityPreviewResponse, AuthorityTimeout, CellRectangle,
    ChangeProvenance, ClientImpactCause, DamageBreakdown, EntityMovementCause, ImpactSnapshot,
    MatchCommand, MatchCommandKind, MatchTransition, PresentationEvent, PresentationEventKind,
    PreviewRejection, ProjectileSampleSnapshot, ProjectileTraceEvent, TransitionDisposition,
    TransitionRejection,
};
use db_sim_core::match_setup::{MatchConfig, MatchMode, MatchPlayerConfig};
use db_sim_core::types::{
    AbilityDefinition, AbilitySlot, Appearance, Attack, CommandRejection, CritRoll, EffectKind,
    PersistentObjectRemovalCause, RandomOutcome, StatusTransition, StrikeDelivery,
    StrikeResolution,
};

use serde::de::{Error as _, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::ABI_VERSION;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MatchCreateRequestDto {
    pub(crate) schema_version: u32,
    pub(crate) match_id: String,
    pub(crate) simulation_version: u32,
    pub(crate) content_version: u32,
    #[serde(rename = "match")]
    match_config: MatchConfigDto,
}

impl MatchCreateRequestDto {
    pub(crate) fn into_core(self) -> MatchConfig {
        self.match_config.into_core()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MatchConfigDto {
    seed: u64,
    map_id: String,
    mode: MatchModeDto,
    #[serde(deserialize_with = "deserialize_players")]
    players: Vec<MatchPlayerDto>,
}

fn deserialize_players<'de, D>(deserializer: D) -> Result<Vec<MatchPlayerDto>, D::Error>
where
    D: Deserializer<'de>,
{
    struct PlayersVisitor;

    impl<'de> Visitor<'de> for PlayersVisitor {
        type Value = Vec<MatchPlayerDto>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an array containing at most four match players")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if sequence
                .size_hint()
                .is_some_and(|hint| hint > db_sim_core::match_setup::MAX_MATCH_PLAYERS)
            {
                return Err(A::Error::custom("match player count exceeds four"));
            }

            let mut players = Vec::with_capacity(db_sim_core::match_setup::MAX_MATCH_PLAYERS);
            while let Some(player) = sequence.next_element()? {
                if players.len() == db_sim_core::match_setup::MAX_MATCH_PLAYERS {
                    return Err(A::Error::custom("match player count exceeds four"));
                }
                players.push(player);
            }
            Ok(players)
        }
    }

    deserializer.deserialize_seq(PlayersVisitor)
}

impl MatchConfigDto {
    fn into_core(self) -> MatchConfig {
        MatchConfig {
            seed: self.seed,
            map_id: self.map_id,
            mode: match self.mode {
                MatchModeDto::TurnBased => MatchMode::TurnBased,
            },
            players: self
                .players
                .into_iter()
                .map(MatchPlayerDto::into_core)
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
enum MatchModeDto {
    TurnBased,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MatchPlayerDto {
    player_id: String,
    team: u8,
    loadout: LoadoutDto,
    appearance: AppearanceDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LoadoutDto {
    main: String,
    secondary: String,
    melee_tool: String,
}

impl MatchPlayerDto {
    fn into_core(self) -> MatchPlayerConfig {
        MatchPlayerConfig {
            player_id: self.player_id,
            team: self.team,
            loadout: db_sim_core::types::Loadout {
                main: self.loadout.main,
                secondary: self.loadout.secondary,
                melee_tool: self.loadout.melee_tool,
            },
            appearance: self.appearance.into_core(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AppearanceDto {
    skin_id: String,
    ability_skin_ids: [String; 3],
    victory_pose_id: String,
}

impl AppearanceDto {
    fn into_core(self) -> Appearance {
        Appearance {
            skin_id: self.skin_id,
            ability_skin_ids: self.ability_skin_ids,
            victory_pose_id: self.victory_pose_id,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum MatchCommandDto {
    Move(MoveCommandDto),
    Ability(AbilityCommandDto),
    PassiveChoice(PassiveChoiceCommandDto),
    Pass(PassCommandDto),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MoveCommandDto {
    schema_version: u32,
    command_id: String,
    player_id: String,
    expected_turn_number: u32,
    expected_snapshot_generation: u64,
    #[serde(rename = "kind")]
    _kind: MoveKindDto,
    dx: i32,
}

#[derive(Debug, Deserialize)]
enum MoveKindDto {
    #[serde(rename = "move")]
    Move,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AbilityCommandDto {
    schema_version: u32,
    command_id: String,
    player_id: String,
    expected_turn_number: u32,
    expected_snapshot_generation: u64,
    #[serde(rename = "kind")]
    _kind: AbilityKindDto,
    slot: AbilitySlotDto,
    angle_millidegrees: i32,
    power_basis_points: i32,
    target_player_id: RequiredNullableString,
    secondary_target_player_id: RequiredNullableString,
}

#[derive(Debug, Deserialize)]
enum AbilityKindDto {
    #[serde(rename = "ability")]
    Ability,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PassiveChoiceCommandDto {
    schema_version: u32,
    command_id: String,
    player_id: String,
    expected_turn_number: u32,
    expected_snapshot_generation: u64,
    #[serde(rename = "kind")]
    _kind: PassiveChoiceKindDto,
    passive_id: String,
}

#[derive(Debug, Deserialize)]
enum PassiveChoiceKindDto {
    #[serde(rename = "passiveChoice")]
    PassiveChoice,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PassCommandDto {
    schema_version: u32,
    command_id: String,
    player_id: String,
    expected_turn_number: u32,
    expected_snapshot_generation: u64,
    #[serde(rename = "kind")]
    _kind: PassKindDto,
}

#[derive(Debug, Deserialize)]
enum PassKindDto {
    #[serde(rename = "pass")]
    Pass,
}

impl MatchCommandDto {
    pub(crate) const fn schema_version(&self) -> u32 {
        match self {
            Self::Move(command) => command.schema_version,
            Self::Ability(command) => command.schema_version,
            Self::PassiveChoice(command) => command.schema_version,
            Self::Pass(command) => command.schema_version,
        }
    }

    pub(crate) fn into_core(self) -> MatchCommand {
        match self {
            Self::Move(MoveCommandDto {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                _kind: _,
                dx,
            }) => MatchCommand {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                kind: MatchCommandKind::Move { dx },
            },
            Self::Ability(AbilityCommandDto {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                _kind: _,
                slot,
                angle_millidegrees,
                power_basis_points,
                target_player_id,
                secondary_target_player_id,
            }) => MatchCommand {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                kind: MatchCommandKind::Ability {
                    slot: slot.into_core(),
                    angle_millidegrees,
                    power_basis_points,
                    target_player_id: target_player_id.0,
                    secondary_target_player_id: secondary_target_player_id.0,
                },
            },
            Self::PassiveChoice(PassiveChoiceCommandDto {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                _kind: _,
                passive_id,
            }) => MatchCommand {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                kind: MatchCommandKind::PassiveChoice { passive_id },
            },
            Self::Pass(PassCommandDto {
                schema_version,
                command_id,
                player_id,
                expected_turn_number,
                expected_snapshot_generation,
                _kind: _,
            }) => MatchCommand {
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

/// Wire shape for [`AuthorityTimeout`] — deliberately its own DTO rather than a
/// `MatchCommandDto` variant, mirroring the core type's own separation: a client submits this
/// to end its *own local* planning deadline, but no byte sequence for it can reach a remote
/// authority's command-decode path, because that path only ever decodes `MatchCommandDto`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AuthorityTimeoutDto {
    schema_version: u32,
    action_id: String,
    player_id: String,
    expected_turn_number: u32,
    expected_snapshot_generation: u64,
}

impl AuthorityTimeoutDto {
    pub(crate) const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub(crate) fn into_core(self) -> AuthorityTimeout {
        AuthorityTimeout {
            schema_version: self.schema_version,
            action_id: self.action_id,
            player_id: self.player_id,
            expected_turn_number: self.expected_turn_number,
            expected_snapshot_generation: self.expected_snapshot_generation,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AbilitySlotDto {
    Main,
    Secondary,
    MeleeTool,
}

impl AbilitySlotDto {
    const fn into_core(self) -> AbilitySlot {
        match self {
            Self::Main => AbilitySlot::Basic,
            Self::Secondary => AbilitySlot::BasicAlt,
            Self::MeleeTool => AbilitySlot::Special,
        }
    }
}

#[derive(Debug)]
pub(crate) struct RequiredNullableString(Option<String>);

impl<'de> Deserialize<'de> for RequiredNullableString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct NullableStringVisitor;

        impl Visitor<'_> for NullableStringVisitor {
            type Value = RequiredNullableString;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a string or explicit null")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(RequiredNullableString(None))
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(RequiredNullableString(None))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(RequiredNullableString(Some(value.to_owned())))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
                Ok(RequiredNullableString(Some(value)))
            }
        }

        // `deserialize_any` is intentional. Serde's missing-field deserializer only grants the
        // Option path an implicit `None`, so omitted fields fail while an explicit JSON null calls
        // `visit_unit` and remains valid.
        deserializer.deserialize_any(NullableStringVisitor)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AbilityPreviewRequestDto {
    schema_version: u32,
    expected_snapshot_generation: u64,
    player_id: String,
    #[serde(rename = "kind")]
    _kind: AbilityKindDto,
    slot: AbilitySlotDto,
    angle_millidegrees: i32,
    power_basis_points: i32,
    target_player_id: RequiredNullableString,
    secondary_target_player_id: RequiredNullableString,
}

impl AbilityPreviewRequestDto {
    pub(crate) const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub(crate) fn into_core(self) -> AbilityPreviewRequest {
        let Self {
            schema_version,
            expected_snapshot_generation,
            player_id,
            _kind: _,
            slot,
            angle_millidegrees,
            power_basis_points,
            target_player_id,
            secondary_target_player_id,
        } = self;
        AbilityPreviewRequest {
            schema_version,
            expected_snapshot_generation,
            player_id,
            slot: slot.into_core(),
            angle_millidegrees,
            power_basis_points,
            target_player_id: target_player_id.0,
            secondary_target_player_id: secondary_target_player_id.0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BotDecisionRequestDto {
    schema_version: u32,
    player_id: String,
    difficulty: BotDifficultyDto,
    decision_seed: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum BotDifficultyDto {
    Casual,
    Standard,
}

impl BotDifficultyDto {
    const fn into_core(self) -> BotDifficulty {
        match self {
            Self::Casual => BotDifficulty::Casual,
            Self::Standard => BotDifficulty::Standard,
        }
    }
}

impl BotDecisionRequestDto {
    pub(crate) const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// Consumes the request into `(player_id, difficulty, decision_seed)`, the exact
    /// argument shape `db_sim_core::bot::decide` takes.
    pub(crate) fn into_core(self) -> (String, BotDifficulty, u64) {
        (
            self.player_id,
            self.difficulty.into_core(),
            self.decision_seed,
        )
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireSnapshot {
    schema_version: u32,
    abi_version: u32,
    simulation_version: u32,
    content_version: u32,
    position_scale: i32,
    fixed_tick_rate: u32,
    match_id: String,
    map_id: String,
    snapshot_generation: u64,
    tick: u64,
    turn_number: u32,
    phase: &'static str,
    active_player_id: Option<String>,
    current_and_upcoming_player_ids: Vec<String>,
    wind_per_tick: i32,
    movement_remaining: i32,
    has_attacked_this_turn: bool,
    input_opens_at: Option<u64>,
    deadline_at: Option<u64>,
    outcome: WireMatchOutcome,
    terrain_width: u32,
    terrain_height: u32,
    terrain_generation: u32,
    blocks: Vec<WireBlock>,
    players: Vec<WirePlayer>,
    persistent_objects: Vec<WireObject>,
    state_hash: String,
}

impl WireSnapshot {
    fn from_core(snapshot: &MatchSnapshot, match_id: &str, map_id: &str) -> Self {
        Self {
            schema_version: snapshot.client_contract_version,
            abi_version: ABI_VERSION,
            simulation_version: snapshot.simulation_version,
            content_version: snapshot.content_version,
            position_scale: snapshot.position_scale,
            fixed_tick_rate: snapshot.fixed_tick_rate,
            match_id: match_id.to_owned(),
            map_id: map_id.to_owned(),
            snapshot_generation: snapshot.generation,
            tick: snapshot.tick,
            turn_number: snapshot.turn_number,
            phase: phase_name(snapshot.phase),
            active_player_id: snapshot.active_player_id.clone(),
            current_and_upcoming_player_ids: snapshot.current_and_upcoming_player_ids.clone(),
            wind_per_tick: snapshot.wind_per_tick,
            movement_remaining: snapshot.movement_remaining,
            has_attacked_this_turn: snapshot.has_attacked_this_turn,
            input_opens_at: None,
            deadline_at: None,
            outcome: WireMatchOutcome::from_core(snapshot.outcome),
            terrain_width: snapshot.terrain_width,
            terrain_height: snapshot.terrain_height,
            terrain_generation: snapshot.terrain_generation,
            blocks: snapshot.blocks.iter().map(WireBlock::from_core).collect(),
            players: snapshot.players.iter().map(WirePlayer::from_core).collect(),
            persistent_objects: snapshot
                .persistent_objects
                .iter()
                .map(WireObject::from_core)
                .collect(),
            state_hash: snapshot.authoritative_state_hash.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireMatchOutcome {
    InProgress,
    Victory { team: u8 },
    Draw,
}

impl WireMatchOutcome {
    const fn from_core(outcome: ClientMatchOutcome) -> Self {
        match outcome {
            ClientMatchOutcome::InProgress => Self::InProgress,
            ClientMatchOutcome::Victory { team } => Self::Victory { team },
            ClientMatchOutcome::Draw => Self::Draw,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePosition {
    x: i32,
    y: i32,
}

impl WirePosition {
    const fn from_core(position: PositionSnapshot) -> Self {
        Self {
            x: position.x,
            y: position.y,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireBlock {
    id: u32,
    origin_cell_x: i32,
    origin_cell_y: i32,
    width_cells: u16,
    height_cells: u16,
    material: &'static str,
    health: u16,
    max_health: u16,
    erosion_axis: &'static str,
}

impl WireBlock {
    fn from_core(block: &BlockSnapshot) -> Self {
        Self {
            id: block.id,
            origin_cell_x: block.origin_cell_x,
            origin_cell_y: block.origin_cell_y,
            width_cells: block.width_cells,
            height_cells: block.height_cells,
            material: material_name(block.material),
            health: block.health,
            max_health: block.max_health,
            erosion_axis: erosion_axis_name(block.erosion_axis),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireStatus {
    kind: &'static str,
    magnitude: i32,
    turns_remaining: u8,
}

impl WireStatus {
    fn from_core(status: &StatusSnapshot) -> Self {
        Self {
            kind: status_kind_name(status.kind),
            magnitude: status.magnitude,
            turns_remaining: status.turns_remaining,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireAppearance {
    skin_id: String,
    ability_skin_ids: [String; 3],
    victory_pose_id: String,
}

impl WireAppearance {
    fn from_core(appearance: &AppearanceSnapshot) -> Self {
        Self {
            skin_id: appearance.skin_id.clone(),
            ability_skin_ids: appearance.ability_skin_ids.clone(),
            victory_pose_id: appearance.victory_pose_id.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePlayer {
    player_id: String,
    team: u8,
    health: u16,
    is_eliminated: bool,
    max_health: u16,
    position: WirePosition,
    loadout: WireLoadout,
    ammo: [WireAmmo; 3],
    statuses: Vec<WireStatus>,
    appearance: WireAppearance,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireLoadout {
    main: String,
    secondary: String,
    melee_tool: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireAmmo {
    remaining: u16,
    maximum: u16,
    policy: &'static str,
}

impl WirePlayer {
    fn from_core(player: &PlayerSnapshot) -> Self {
        Self {
            player_id: player.id.clone(),
            team: player.team,
            health: player.health,
            is_eliminated: player.is_eliminated,
            max_health: player.max_health,
            position: WirePosition::from_core(player.position),
            loadout: WireLoadout {
                main: player.loadout.main.clone(),
                secondary: player.loadout.secondary.clone(),
                melee_tool: player.loadout.melee_tool.clone(),
            },
            ammo: [
                WireAmmo::from_core(snapshot_ammo(player, 0)),
                WireAmmo::from_core(snapshot_ammo(player, 1)),
                WireAmmo::from_core(snapshot_ammo(player, 2)),
            ],
            statuses: player.statuses.iter().map(WireStatus::from_core).collect(),
            appearance: WireAppearance::from_core(&player.appearance),
        }
    }
}

impl WireAmmo {
    fn from_core(ammo: db_sim_core::types::AmmoCounter) -> Self {
        Self {
            remaining: ammo.remaining,
            maximum: ammo.maximum,
            policy: ammo.policy.wire_name(),
        }
    }
}

fn snapshot_ammo(player: &PlayerSnapshot, index: usize) -> db_sim_core::types::AmmoCounter {
    player
        .ammo
        .get(index)
        .copied()
        .unwrap_or(db_sim_core::types::AmmoCounter {
            remaining: 0,
            maximum: 0,
            policy: db_sim_core::types::AmmoPolicy::Finite,
        })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireObject {
    sequence: u32,
    owner_id: String,
    kind: &'static str,
    position: WirePosition,
    health: u16,
    turns_remaining: u8,
}

impl WireObject {
    fn from_core(object: &PersistentObjectSnapshot) -> Self {
        Self {
            sequence: object.sequence,
            owner_id: object.owner_id.clone(),
            kind: object_kind_name(object.kind),
            position: WirePosition::from_core(object.position),
            health: object.health,
            turns_remaining: object.turns_remaining,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireProjectileSample {
    tick: u32,
    position: WirePosition,
}

impl WireProjectileSample {
    const fn from_core(sample: ProjectileSampleSnapshot) -> Self {
        Self {
            tick: sample.tick,
            position: WirePosition::from_core(sample.position),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireImpact {
    position: WirePosition,
    tick: u32,
    cause: &'static str,
}

impl WireImpact {
    const fn from_core(impact: ImpactSnapshot) -> Self {
        Self {
            position: WirePosition::from_core(impact.position),
            tick: impact.tick,
            cause: impact_cause_name(impact.cause),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireProjectileTrace {
    trace_id: u32,
    owner_id: String,
    ability_id: String,
    samples: Vec<WireProjectileSample>,
    terminal_impact: WireImpact,
}

impl WireProjectileTrace {
    fn from_core(trace: &ProjectileTraceEvent) -> Self {
        Self {
            trace_id: trace.trace_id,
            owner_id: trace.owner_id.clone(),
            ability_id: trace.ability_id.clone(),
            samples: trace
                .samples
                .iter()
                .copied()
                .map(WireProjectileSample::from_core)
                .collect(),
            terminal_impact: WireImpact::from_core(trace.terminal_impact),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireStrikeDelivery {
    Projectile { trace_sequence: u32 },
    Melee,
    Effect { effect_kind: &'static str },
}

impl WireStrikeDelivery {
    const fn from_core(delivery: StrikeDelivery) -> Self {
        match delivery {
            StrikeDelivery::Projectile { trace_sequence } => Self::Projectile { trace_sequence },
            StrikeDelivery::Melee => Self::Melee,
            StrikeDelivery::Effect { kind } => Self::Effect {
                effect_kind: effect_kind_name(kind),
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireStrike {
    strike_index: u16,
    target_player_id: String,
    impact_point: WirePosition,
    delivery: WireStrikeDelivery,
    crit: &'static str,
    damage_applied: u16,
    eliminated_target: bool,
}

impl WireStrike {
    fn from_core(strike: &StrikeResolution) -> Self {
        Self {
            strike_index: strike.strike_index,
            target_player_id: strike.target_player_id.clone(),
            impact_point: WirePosition {
                x: strike.impact_point.x,
                y: strike.impact_point.y,
            },
            delivery: WireStrikeDelivery::from_core(strike.delivery),
            crit: crit_name(strike.crit),
            damage_applied: strike.damage_applied,
            eliminated_target: strike.eliminated_target,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireRectangle {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl WireRectangle {
    const fn from_core(rectangle: CellRectangle) -> Self {
        Self {
            x: rectangle.x,
            y: rectangle.y,
            width: rectangle.width,
            height: rectangle.height,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireDamageBreakdown {
    direct: u16,
    splash: u16,
    backlash: u16,
    hazard: u16,
    wall_impact: u16,
    healed: u16,
    was_critical: bool,
    knockback: WirePosition,
    eliminated: bool,
}

impl WireDamageBreakdown {
    const fn from_core(damage: &DamageBreakdown) -> Self {
        Self {
            direct: damage.direct,
            splash: damage.splash,
            backlash: damage.backlash,
            hazard: damage.hazard,
            wall_impact: damage.wall_impact,
            healed: damage.healed,
            was_critical: damage.was_critical,
            knockback: WirePosition::from_core(damage.knockback),
            eliminated: damage.eliminated,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "purpose",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireRandomOutcome {
    ArzumChainStrikeTeleportTarget {
        candidate_count: u32,
        selected_index: u32,
        target_player_id: String,
        destination: WirePosition,
    },
    AlephVeilstepTeleportPoint {
        axis_bound: u32,
        x_result: u32,
        y_result: u32,
        fallback_used: bool,
        drawn_point: WirePosition,
        destination: WirePosition,
    },
}

impl WireRandomOutcome {
    fn from_core(outcome: &RandomOutcome) -> Self {
        match outcome {
            RandomOutcome::ArzumChainStrikeTeleportTarget {
                candidate_count,
                selected_index,
                target_player_id,
                destination,
            } => Self::ArzumChainStrikeTeleportTarget {
                candidate_count: *candidate_count,
                selected_index: *selected_index,
                target_player_id: target_player_id.clone(),
                destination: WirePosition {
                    x: destination.x,
                    y: destination.y,
                },
            },
            RandomOutcome::AlephVeilstepTeleportPoint {
                axis_bound,
                x_result,
                y_result,
                fallback_used,
                drawn_point,
                destination,
            } => Self::AlephVeilstepTeleportPoint {
                axis_bound: *axis_bound,
                x_result: *x_result,
                y_result: *y_result,
                fallback_used: *fallback_used,
                drawn_point: WirePosition {
                    x: drawn_point.x,
                    y: drawn_point.y,
                },
                destination: WirePosition {
                    x: destination.x,
                    y: destination.y,
                },
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireStatusTransition {
    Applied {
        magnitude: i32,
        turns_remaining: u8,
    },
    Refreshed {
        magnitude: i32,
        turns_remaining: u8,
        replaced_magnitude: i32,
        replaced_turns_remaining: u8,
    },
    ChargeConsumed {
        remaining: i32,
    },
    Ticked {
        turns_remaining: u8,
    },
    Exhausted,
    Expired,
}

impl WireStatusTransition {
    const fn from_core(transition: &StatusTransition) -> Self {
        match transition {
            StatusTransition::Applied {
                magnitude,
                turns_remaining,
            } => Self::Applied {
                magnitude: *magnitude,
                turns_remaining: *turns_remaining,
            },
            StatusTransition::Refreshed {
                magnitude,
                turns_remaining,
                replaced_magnitude,
                replaced_turns_remaining,
            } => Self::Refreshed {
                magnitude: *magnitude,
                turns_remaining: *turns_remaining,
                replaced_magnitude: *replaced_magnitude,
                replaced_turns_remaining: *replaced_turns_remaining,
            },
            StatusTransition::ChargeConsumed { remaining } => Self::ChargeConsumed {
                remaining: *remaining,
            },
            StatusTransition::Ticked { turns_remaining } => Self::Ticked {
                turns_remaining: *turns_remaining,
            },
            StatusTransition::Exhausted => Self::Exhausted,
            StatusTransition::Expired => Self::Expired,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireEliminationCause {
    Strike {
        owner_id: String,
        ability_id: String,
        strike_index: u16,
    },
    Backlash {
        owner_id: String,
        ability_id: String,
    },
    Splash {
        owner_id: String,
        ability_id: String,
    },
    WallImpact {
        owner_id: String,
        ability_id: String,
    },
    AbilityEffect {
        owner_id: String,
        ability_id: String,
    },
    Hazard,
    AuthoritativeResolution,
}

impl WireEliminationCause {
    fn from_core(cause: &ChangeProvenance) -> Self {
        match cause {
            ChangeProvenance::Strike {
                owner_id,
                ability_id,
                strike_index,
            } => Self::Strike {
                owner_id: owner_id.clone(),
                ability_id: ability_id.clone(),
                strike_index: *strike_index,
            },
            ChangeProvenance::Backlash {
                owner_id,
                ability_id,
            } => Self::Backlash {
                owner_id: owner_id.clone(),
                ability_id: ability_id.clone(),
            },
            ChangeProvenance::Splash {
                owner_id,
                ability_id,
            } => Self::Splash {
                owner_id: owner_id.clone(),
                ability_id: ability_id.clone(),
            },
            ChangeProvenance::WallImpact {
                owner_id,
                ability_id,
            } => Self::WallImpact {
                owner_id: owner_id.clone(),
                ability_id: ability_id.clone(),
            },
            ChangeProvenance::AbilityEffect {
                owner_id,
                ability_id,
            } => Self::AbilityEffect {
                owner_id: owner_id.clone(),
                ability_id: ability_id.clone(),
            },
            ChangeProvenance::Hazard => Self::Hazard,
            ChangeProvenance::AuthoritativeResolution => Self::AuthoritativeResolution,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireEventKind {
    ProjectileTrace {
        trace: WireProjectileTrace,
    },
    Impact {
        trace_id: u32,
        impact: WireImpact,
    },
    StrikeResolved {
        owner_id: String,
        ability_id: String,
        strike: WireStrike,
    },
    TerrainChanged {
        terrain_generation: u32,
        dirty_rectangles: Vec<WireRectangle>,
    },
    BlockChanged {
        block_id: u32,
        previous_health: Option<u16>,
        new_health: Option<u16>,
        previous_surviving_bounds: Option<WireRectangle>,
        new_surviving_bounds: Option<WireRectangle>,
    },
    HealthChanged {
        player_id: String,
        previous_health: u16,
        new_health: u16,
        breakdown: Option<WireDamageBreakdown>,
    },
    GaugeChanged {
        player_id: String,
        previous_gauge: u16,
        new_gauge: u16,
        delta: i32,
    },
    RandomOutcome {
        owner_id: String,
        ability_id: String,
        outcome: WireRandomOutcome,
    },
    StatusChanged {
        player_id: String,
        status_kind: &'static str,
        transition: WireStatusTransition,
    },
    EntityMoved {
        player_id: String,
        start: WirePosition,
        end: WirePosition,
        cause: &'static str,
    },
    ObjectSpawned {
        object: WireObject,
    },
    ObjectChanged {
        previous: WireObject,
        current: WireObject,
    },
    ObjectRemoved {
        previous: WireObject,
        cause: &'static str,
    },
    PlayerEliminated {
        player_id: String,
        cause: WireEliminationCause,
    },
    PassiveChoiceRequired {
        player_id: String,
        passive_ids: Vec<String>,
    },
    PassiveChosen {
        player_id: String,
        passive_id: String,
    },
    TurnEnded {
        player_id: String,
        reason: &'static str,
    },
    TurnOpened {
        player_id: String,
        turn_number: u32,
        input_opens_at: Option<u64>,
        deadline_at: Option<u64>,
    },
    MatchCompleted {
        outcome: WireMatchOutcome,
    },
}

impl WireEventKind {
    fn from_core(kind: &PresentationEventKind) -> Self {
        match kind {
            PresentationEventKind::ProjectileTrace(trace) => Self::ProjectileTrace {
                trace: WireProjectileTrace::from_core(trace),
            },
            PresentationEventKind::Impact { trace_id, impact } => Self::Impact {
                trace_id: *trace_id,
                impact: WireImpact::from_core(*impact),
            },
            PresentationEventKind::StrikeResolved {
                owner_id,
                ability_id,
                strike,
            } => Self::StrikeResolved {
                owner_id: owner_id.clone(),
                ability_id: ability_id.clone(),
                strike: WireStrike::from_core(strike),
            },
            PresentationEventKind::TerrainChanged {
                terrain_generation,
                dirty_rectangles,
            } => Self::TerrainChanged {
                terrain_generation: *terrain_generation,
                dirty_rectangles: dirty_rectangles
                    .iter()
                    .copied()
                    .map(WireRectangle::from_core)
                    .collect(),
            },
            PresentationEventKind::BlockChanged {
                block_id,
                previous_health,
                new_health,
                previous_surviving_bounds,
                new_surviving_bounds,
            } => Self::BlockChanged {
                block_id: *block_id,
                previous_health: *previous_health,
                new_health: *new_health,
                previous_surviving_bounds: previous_surviving_bounds.map(WireRectangle::from_core),
                new_surviving_bounds: new_surviving_bounds.map(WireRectangle::from_core),
            },
            PresentationEventKind::HealthChanged {
                player_id,
                previous_health,
                new_health,
                breakdown,
            } => Self::HealthChanged {
                player_id: player_id.clone(),
                previous_health: *previous_health,
                new_health: *new_health,
                breakdown: breakdown.as_ref().map(WireDamageBreakdown::from_core),
            },
            PresentationEventKind::GaugeChanged {
                player_id,
                previous_gauge,
                new_gauge,
                delta,
            } => Self::GaugeChanged {
                player_id: player_id.clone(),
                previous_gauge: *previous_gauge,
                new_gauge: *new_gauge,
                delta: *delta,
            },
            PresentationEventKind::RandomOutcome {
                owner_id,
                ability_id,
                outcome,
            } => Self::RandomOutcome {
                owner_id: owner_id.clone(),
                ability_id: ability_id.clone(),
                outcome: WireRandomOutcome::from_core(outcome),
            },
            PresentationEventKind::StatusChanged {
                player_id,
                kind,
                transition,
            } => Self::StatusChanged {
                player_id: player_id.clone(),
                status_kind: status_kind_name(*kind),
                transition: WireStatusTransition::from_core(transition),
            },
            PresentationEventKind::EntityMoved {
                player_id,
                start,
                end,
                cause,
            } => Self::EntityMoved {
                player_id: player_id.clone(),
                start: WirePosition::from_core(*start),
                end: WirePosition::from_core(*end),
                cause: movement_cause_name(*cause),
            },
            PresentationEventKind::ObjectSpawned { object } => Self::ObjectSpawned {
                object: WireObject::from_core(object),
            },
            PresentationEventKind::ObjectChanged { previous, current } => Self::ObjectChanged {
                previous: WireObject::from_core(previous),
                current: WireObject::from_core(current),
            },
            PresentationEventKind::ObjectRemoved { previous, cause } => Self::ObjectRemoved {
                previous: WireObject::from_core(previous),
                cause: removal_cause_name(*cause),
            },
            PresentationEventKind::PlayerEliminated { player_id, cause } => {
                Self::PlayerEliminated {
                    player_id: player_id.clone(),
                    cause: WireEliminationCause::from_core(cause),
                }
            }
            PresentationEventKind::PassiveChoiceRequired {
                player_id,
                passive_ids,
            } => Self::PassiveChoiceRequired {
                player_id: player_id.clone(),
                passive_ids: passive_ids.clone(),
            },
            PresentationEventKind::PassiveChosen {
                player_id,
                passive_id,
            } => Self::PassiveChosen {
                player_id: player_id.clone(),
                passive_id: passive_id.clone(),
            },
            PresentationEventKind::TurnEnded { player_id, reason } => Self::TurnEnded {
                player_id: player_id.clone(),
                reason: turn_end_reason_name(*reason),
            },
            PresentationEventKind::TurnOpened {
                player_id,
                turn_number,
            } => Self::TurnOpened {
                player_id: player_id.clone(),
                turn_number: *turn_number,
                // The transport-free core cannot manufacture wall-clock values. C3's local
                // session decorates these after its monotonic input lock, and a future Rust
                // server supplies server-time values. They remain required-nullable on the
                // version-1 wire shape so strict managed DTOs never need two schemas.
                input_opens_at: None,
                deadline_at: None,
            },
            PresentationEventKind::MatchCompleted { outcome } => Self::MatchCompleted {
                outcome: WireMatchOutcome::from_core(*outcome),
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireEvent {
    presentation_tick: u32,
    sequence: u32,
    #[serde(flatten)]
    kind: WireEventKind,
}

impl WireEvent {
    fn from_core(event: &PresentationEvent) -> Self {
        Self {
            presentation_tick: event.presentation_tick,
            sequence: event.sequence,
            kind: WireEventKind::from_core(&event.kind),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireTransitionRejection {
    SnapshotGenerationMismatch { expected: u64, actual: u64 },
    CommandIdConflict,
    Core { reason: &'static str },
}

impl WireTransitionRejection {
    fn from_core(rejection: &TransitionRejection) -> Self {
        match rejection {
            TransitionRejection::SnapshotGenerationMismatch { expected, actual } => {
                Self::SnapshotGenerationMismatch {
                    expected: *expected,
                    actual: *actual,
                }
            }
            TransitionRejection::CommandIdConflict => Self::CommandIdConflict,
            TransitionRejection::Core(reason) => Self::Core {
                reason: command_rejection_name(*reason),
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireTransition {
    schema_version: u32,
    command_id: String,
    disposition: &'static str,
    rejection_reason: Option<WireTransitionRejection>,
    pre_snapshot_generation: u64,
    post_snapshot_generation: u64,
    presentation_tick_rate: u32,
    input_lock_ticks: u32,
    events: Vec<WireEvent>,
    post_snapshot: WireSnapshot,
    post_state_hash: String,
}

impl WireTransition {
    fn from_core(transition: &MatchTransition, match_id: &str, map_id: &str) -> Self {
        Self {
            schema_version: transition.schema_version,
            command_id: transition.command_id.clone(),
            disposition: disposition_name(transition.disposition),
            rejection_reason: transition
                .rejection_reason
                .as_ref()
                .map(WireTransitionRejection::from_core),
            pre_snapshot_generation: transition.pre_snapshot_generation,
            post_snapshot_generation: transition.post_snapshot_generation,
            presentation_tick_rate: transition.presentation_tick_rate,
            input_lock_ticks: transition.input_lock_ticks,
            events: transition.events.iter().map(WireEvent::from_core).collect(),
            post_snapshot: WireSnapshot::from_core(&transition.post_snapshot, match_id, map_id),
            post_state_hash: transition.post_state_hash.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WirePreviewRejection {
    SnapshotGenerationMismatch { expected: u64, actual: u64 },
    Core { reason: &'static str },
}

impl WirePreviewRejection {
    fn from_core(rejection: &PreviewRejection) -> Self {
        match rejection {
            PreviewRejection::SnapshotGenerationMismatch { expected, actual } => {
                Self::SnapshotGenerationMismatch {
                    expected: *expected,
                    actual: *actual,
                }
            }
            PreviewRejection::Core(reason) => Self::Core {
                reason: command_rejection_name(*reason),
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WirePreview {
    schema_version: u32,
    snapshot_generation: u64,
    legal: bool,
    rejection_reason: Option<WirePreviewRejection>,
    gauge_cost: u16,
    legal_target_player_ids: Vec<String>,
    projectile_traces: Vec<WireProjectileTrace>,
}

impl WirePreview {
    fn from_core(response: &AbilityPreviewResponse) -> Self {
        Self {
            schema_version: response.schema_version,
            snapshot_generation: response.snapshot_generation,
            legal: response.legal,
            rejection_reason: response
                .rejection_reason
                .as_ref()
                .map(WirePreviewRejection::from_core),
            gauge_cost: response.gauge_cost,
            legal_target_player_ids: response.legal_target_player_ids.clone(),
            projectile_traces: response
                .projectile_traces
                .iter()
                .map(WireProjectileTrace::from_core)
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireBotDecision {
    schema_version: u32,
    #[serde(flatten)]
    action: WireBotAction,
}

/// The bot's proposed action, shaped like `MatchCommandDto`'s own `kind` variants but with
/// none of that type's session-bookkeeping fields (`commandId`, `expectedTurnNumber`,
/// `expectedSnapshotGeneration`): those belong to the caller that submits this decision
/// through the ordinary apply path, not to the decision itself.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum WireBotAction {
    Move {
        dx: i32,
    },
    Ability {
        slot: &'static str,
        angle_millidegrees: i32,
        power_basis_points: i32,
        target_player_id: Option<String>,
        secondary_target_player_id: Option<String>,
    },
    PassiveChoice {
        passive_id: String,
    },
    Pass,
}

impl WireBotDecision {
    fn from_core(decision: MatchCommandKind) -> Self {
        Self {
            schema_version: db_sim_core::client_contract::CLIENT_CONTRACT_VERSION,
            action: WireBotAction::from_core(decision),
        }
    }
}

impl WireBotAction {
    fn from_core(decision: MatchCommandKind) -> Self {
        match decision {
            MatchCommandKind::Move { dx } => Self::Move { dx },
            MatchCommandKind::Ability {
                slot,
                angle_millidegrees,
                power_basis_points,
                target_player_id,
                secondary_target_player_id,
            } => Self::Ability {
                slot: slot.wire_name(),
                angle_millidegrees,
                power_basis_points,
                target_player_id,
                secondary_target_player_id,
            },
            MatchCommandKind::PassiveChoice { passive_id } => Self::PassiveChoice { passive_id },
            MatchCommandKind::Pass => Self::Pass,
        }
    }
}

/// The one fighter plus the item catalog, for the loadout picker. Static content, not
/// match state — callable without a live handle.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireRoster {
    schema_version: u32,
    fighter: WireFighter,
    items: Vec<WireItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireFighter {
    id: &'static str,
    display_name: &'static str,
    max_health: u16,
    range_tier: &'static str,
    movement_class: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireItem {
    id: &'static str,
    display_name: &'static str,
    slot: &'static str,
    ammo_policy: &'static str,
    starting_ammo: u16,
    ability: WireAbility,
}

/// An ability's selection-relevant shape. Deliberately excludes resolution internals
/// (projectile speed/gravity/wind, terrain effects, strikes-per-turn breakdown beyond the
/// count) that mean nothing to a player picking a character — those live in match transitions,
/// not this roster listing.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireAbility {
    id: &'static str,
    display_name: &'static str,
    slot: &'static str,
    damage_percent: u16,
    crit_damage_percent: u16,
    crit_chance_basis_points: u16,
    strikes_per_turn: u8,
    attack_shape: &'static str,
    /// Reach, fixed-point. Present only for a [`Attack::Strike`] — a projectile's effective
    /// range depends on the player's own aim and power, not a fixed number worth showing here.
    range: Option<i32>,
}

impl WireAbility {
    fn from_core(ability: &AbilityDefinition) -> Self {
        let (attack_shape, range) = match &ability.attack {
            Attack::Projectile(_) => ("projectile", None),
            Attack::Strike(strike) => ("strike", Some(strike.range)),
        };
        Self {
            id: ability.id,
            display_name: ability.display_name,
            slot: ability.slot.wire_name(),
            damage_percent: ability.damage_percent,
            crit_damage_percent: ability.crit_damage_percent,
            crit_chance_basis_points: ability.crit_chance_basis_points,
            strikes_per_turn: ability.strikes_per_turn,
            attack_shape,
            range,
        }
    }
}

pub(crate) fn serialize_roster() -> Result<Vec<u8>, serde_json::Error> {
    let fighter = character::fighter();
    let roster = WireRoster {
        schema_version: db_sim_core::client_contract::CLIENT_CONTRACT_VERSION,
        fighter: WireFighter {
            id: fighter.id,
            display_name: fighter.display_name,
            max_health: fighter.max_health,
            range_tier: fighter.range_tier.wire_name(),
            movement_class: fighter.movement.wire_name(),
        },
        items: character::LAUNCH_ITEMS
            .iter()
            .map(|item| WireItem {
                id: item.id,
                display_name: item.display_name,
                slot: item.slot.wire_name(),
                ammo_policy: item.ammo_policy.wire_name(),
                starting_ammo: item.starting_ammo,
                ability: WireAbility::from_core(&item.ability),
            })
            .collect(),
    };
    serialize_line(&roster)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireCreateDiagnostic {
    code: &'static str,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WireCreateResponse {
    schema_version: u32,
    created: bool,
    diagnostic: Option<WireCreateDiagnostic>,
    snapshot: Option<WireSnapshot>,
}

pub(crate) fn serialize_create_success(
    snapshot: &MatchSnapshot,
    match_id: &str,
    map_id: &str,
) -> Result<Vec<u8>, serde_json::Error> {
    serialize_line(&WireCreateResponse {
        schema_version: db_sim_core::client_contract::CLIENT_CONTRACT_VERSION,
        created: true,
        diagnostic: None,
        snapshot: Some(WireSnapshot::from_core(snapshot, match_id, map_id)),
    })
}

pub(crate) fn serialize_create_failure(
    code: &'static str,
    message: String,
) -> Result<Vec<u8>, serde_json::Error> {
    serialize_line(&WireCreateResponse {
        schema_version: db_sim_core::client_contract::CLIENT_CONTRACT_VERSION,
        created: false,
        diagnostic: Some(WireCreateDiagnostic { code, message }),
        snapshot: None,
    })
}

pub(crate) fn serialize_snapshot(
    snapshot: &MatchSnapshot,
    match_id: &str,
    map_id: &str,
) -> Result<Vec<u8>, serde_json::Error> {
    serialize_line(&WireSnapshot::from_core(snapshot, match_id, map_id))
}

pub(crate) fn serialize_transition(
    transition: &MatchTransition,
    match_id: &str,
    map_id: &str,
) -> Result<Vec<u8>, serde_json::Error> {
    serialize_line(&WireTransition::from_core(transition, match_id, map_id))
}

pub(crate) fn serialize_preview(
    response: &AbilityPreviewResponse,
) -> Result<Vec<u8>, serde_json::Error> {
    serialize_line(&WirePreview::from_core(response))
}

pub(crate) fn serialize_bot_decision(
    decision: MatchCommandKind,
) -> Result<Vec<u8>, serde_json::Error> {
    serialize_line(&WireBotDecision::from_core(decision))
}

fn serialize_line<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

const fn phase_name(value: ClientMatchPhase) -> &'static str {
    match value {
        ClientMatchPhase::MatchIntro => "matchIntro",
        ClientMatchPhase::TurnStart => "turnStart",
        ClientMatchPhase::Movement => "movement",
        ClientMatchPhase::AimingAndSelection => "aimingAndSelection",
        ClientMatchPhase::PassiveSelection => "passiveSelection",
        ClientMatchPhase::CommandLocked => "commandLocked",
        ClientMatchPhase::Resolution => "resolution",
        ClientMatchPhase::Settling => "settling",
        ClientMatchPhase::StatusResolution => "statusResolution",
        ClientMatchPhase::VictoryCheck => "victoryCheck",
        ClientMatchPhase::MatchComplete => "matchComplete",
    }
}

const fn material_name(value: ClientMaterial) -> &'static str {
    match value {
        ClientMaterial::Empty => "empty",
        ClientMaterial::Soil => "soil",
        ClientMaterial::Wood => "wood",
        ClientMaterial::ReinforcedStone => "reinforcedStone",
    }
}

const fn erosion_axis_name(value: ClientErosionAxis) -> &'static str {
    match value {
        ClientErosionAxis::Columns => "columns",
        ClientErosionAxis::Rows => "rows",
    }
}

const fn status_kind_name(value: ClientStatusKind) -> &'static str {
    match value {
        ClientStatusKind::Knockback => "knockback",
        ClientStatusKind::Chill => "chill",
        ClientStatusKind::Cluster => "cluster",
        ClientStatusKind::Embers => "embers",
        ClientStatusKind::Tunnel => "tunnel",
        ClientStatusKind::Return => "return",
        ClientStatusKind::Recoil => "recoil",
        ClientStatusKind::SelfDamage => "selfDamage",
        ClientStatusKind::Teleport => "teleport",
        ClientStatusKind::Pull => "pull",
        ClientStatusKind::Push => "push",
        ClientStatusKind::WallImpact => "wallImpact",
        ClientStatusKind::Lockdown => "lockdown",
        ClientStatusKind::SpawnTurret => "spawnTurret",
        ClientStatusKind::Heal => "heal",
        ClientStatusKind::HealthTransfer => "healthTransfer",
        ClientStatusKind::MultiStrike => "multiStrike",
        ClientStatusKind::GuaranteeCrit => "guaranteeCrit",
        ClientStatusKind::EmbedProjectile => "embedProjectile",
        ClientStatusKind::ChainDetonate => "chainDetonate",
        ClientStatusKind::Relocate => "relocate",
        ClientStatusKind::Obscure => "obscure",
    }
}

const fn effect_kind_name(value: EffectKind) -> &'static str {
    match value {
        EffectKind::Knockback => "knockback",
        EffectKind::Chill => "chill",
        EffectKind::Cluster => "cluster",
        EffectKind::Embers => "embers",
        EffectKind::Tunnel => "tunnel",
        EffectKind::Return => "return",
        EffectKind::Recoil => "recoil",
        EffectKind::SelfDamage => "selfDamage",
        EffectKind::Teleport => "teleport",
        EffectKind::Pull => "pull",
        EffectKind::Push => "push",
        EffectKind::WallImpact => "wallImpact",
        EffectKind::Lockdown => "lockdown",
        EffectKind::SpawnTurret => "spawnTurret",
        EffectKind::Heal => "heal",
        EffectKind::HealthTransfer => "healthTransfer",
        EffectKind::MultiStrike => "multiStrike",
        EffectKind::GuaranteeCrit => "guaranteeCrit",
        EffectKind::EmbedProjectile => "embedProjectile",
        EffectKind::ChainDetonate => "chainDetonate",
        EffectKind::Relocate => "relocate",
        EffectKind::Obscure => "obscure",
    }
}

const fn object_kind_name(value: ClientObjectKind) -> &'static str {
    match value {
        ClientObjectKind::Turret => "turret",
        ClientObjectKind::EmbeddedKnife => "embeddedKnife",
        ClientObjectKind::GasCloud => "gasCloud",
    }
}

const fn impact_cause_name(value: ClientImpactCause) -> &'static str {
    match value {
        ClientImpactCause::Terrain => "terrain",
        ClientImpactCause::Character => "character",
        ClientImpactCause::OutOfBounds => "outOfBounds",
        ClientImpactCause::Expired => "expired",
    }
}

const fn crit_name(value: CritRoll) -> &'static str {
    match value {
        CritRoll::NotEligible => "notEligible",
        CritRoll::Missed => "missed",
        CritRoll::Landed => "landed",
        CritRoll::Forced => "forced",
    }
}

const fn movement_cause_name(value: EntityMovementCause) -> &'static str {
    match value {
        EntityMovementCause::RequestedMove => "requestedMove",
        EntityMovementCause::AuthoritativeResolution => "authoritativeResolution",
    }
}

const fn removal_cause_name(value: PersistentObjectRemovalCause) -> &'static str {
    match value {
        PersistentObjectRemovalCause::Replaced => "replaced",
        PersistentObjectRemovalCause::CapacityEvicted => "capacityEvicted",
        PersistentObjectRemovalCause::Detonated => "detonated",
        PersistentObjectRemovalCause::Expired => "expired",
        PersistentObjectRemovalCause::Destroyed => "destroyed",
        PersistentObjectRemovalCause::OwnerEliminated => "ownerEliminated",
    }
}

const fn turn_end_reason_name(value: ClientTurnEndReason) -> &'static str {
    match value {
        ClientTurnEndReason::Attacked => "attacked",
        ClientTurnEndReason::Passed => "passed",
        ClientTurnEndReason::TimedOut => "timedOut",
        ClientTurnEndReason::Eliminated => "eliminated",
    }
}

const fn disposition_name(value: TransitionDisposition) -> &'static str {
    match value {
        TransitionDisposition::Accepted => "accepted",
        TransitionDisposition::Rejected => "rejected",
        TransitionDisposition::DuplicateReplay => "duplicateReplay",
    }
}

const fn command_rejection_name(value: CommandRejection) -> &'static str {
    match value {
        CommandRejection::DuplicateCommand => "duplicateCommand",
        CommandRejection::PlayerEliminated => "playerEliminated",
        CommandRejection::NotActivePlayer => "notActivePlayer",
        CommandRejection::WrongPhase => "wrongPhase",
        CommandRejection::TurnVersionMismatch => "turnVersionMismatch",
        CommandRejection::UnknownCharacter => "unknownCharacter",
        CommandRejection::AbilityNotAvailable => "abilityNotAvailable",
        CommandRejection::GaugeNotReady => "gaugeNotReady",
        CommandRejection::OutOfAmmo => "outOfAmmo",
        CommandRejection::AlreadyAttacked => "alreadyAttacked",
        CommandRejection::InputOutOfRange => "inputOutOfRange",
        CommandRejection::InvalidTarget => "invalidTarget",
        CommandRejection::InvalidPassive => "invalidPassive",
        CommandRejection::PassiveAlreadyChosen => "passiveAlreadyChosen",
    }
}
