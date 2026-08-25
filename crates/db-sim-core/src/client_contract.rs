//! Engine-neutral, read-only projection of authoritative match state for clients.
//!
//! This module deliberately does not serialize, marshal, or expose [`SimulationState`].
//! A local C ABI and a future remote protocol may both project a [`MatchSnapshot`], but
//! transport concerns and engine types do not belong in the simulation core. Dedicated
//! snapshot DTOs also keep an internal state refactor from silently becoming a protocol
//! change.
//!
//! [`MatchSnapshot`] is the core-owned projection, not the complete session or transport
//! envelope described by `CLIENT_SPEC.md` section 7.3. Match/session identity, planning
//! timestamps and server time, map asset metadata, ABI/envelope versions, and transport
//! sequencing are adapter-owned metadata because [`SimulationState`] does not own them.
//! Adapters wrap this projection with those values. Terrain cell bytes likewise travel via
//! the coarse terrain export; the projection exposes dimensions and a generation that tells
//! consumers when to fetch that payload.

use crate::blocks::TerrainBlock;
use crate::fixed::FixedPoint;
use crate::match_host::MatchHost;
use crate::types::{
    Appearance, EffectKind, ErosionAxis, MatchOutcome, MatchPhase, Material, PersistentObject,
    PersistentObjectKind, PlayerState, SimulationState, StatusEffect,
};

/// Version of the semantic client snapshot contract defined by this module.
///
/// Increment this when a field, enum meaning, ordering rule, or normalization rule changes.
/// It is independent of simulation, content, protocol, and any future C ABI versions.
pub const CLIENT_CONTRACT_VERSION: u32 = 1;

/// An integer position in authoritative fixed-point simulation units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionSnapshot {
    /// Horizontal component in simulation units.
    pub x: i32,
    /// Vertical component in simulation units; positive is downward.
    pub y: i32,
}

/// Client-facing match phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientMatchPhase {
    /// Pre-match presentation.
    MatchIntro,
    /// Start-of-turn processing.
    TurnStart,
    /// The active player may move.
    Movement,
    /// The active player may aim and select an ability.
    AimingAndSelection,
    /// A one-time passive choice is required.
    PassiveSelection,
    /// A command has been accepted and input is locked.
    CommandLocked,
    /// The committed action is resolving.
    Resolution,
    /// Bodies and objects are settling.
    Settling,
    /// Status and lifetime effects are resolving.
    StatusResolution,
    /// Victory conditions are being evaluated.
    VictoryCheck,
    /// The match is terminal.
    MatchComplete,
}

/// Client-facing reason for a future turn-ended presentation event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientTurnEndReason {
    /// The active player committed an attack.
    Attacked,
    /// The active player explicitly passed.
    Passed,
    /// The authoritative planning deadline expired.
    TimedOut,
    /// The active player was eliminated before completing the turn.
    Eliminated,
}

/// Client-facing match result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientMatchOutcome {
    /// The match is still in progress.
    InProgress,
    /// Exactly one team remains alive.
    Victory {
        /// The winning team index.
        team: u8,
    },
    /// No team won.
    Draw,
}

/// Client-facing terrain material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClientMaterial {
    /// Open space.
    Empty,
    /// Broadly destructible soil.
    Soil,
    /// Broadly destructible wood.
    Wood,
    /// Reinforced stone, removable only by permitted attacks.
    ReinforcedStone,
}

/// Client-facing block erosion direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClientErosionAxis {
    /// Whole columns erode and the block narrows.
    Columns,
    /// Whole rows erode and the block thins.
    Rows,
}

/// Client-facing identifier for an active status effect.
///
/// This is intentionally closed. Adding an authoritative effect kind must cause an
/// exhaustive-match compile error here and a reviewed client-contract version decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClientStatusKind {
    /// Displacement along an impact normal.
    Knockback,
    /// Reduced next-turn movement.
    Chill,
    /// Projectile submunition behavior.
    Cluster,
    /// Persistent marked contact zones.
    Embers,
    /// Terrain-boring behavior.
    Tunnel,
    /// Outbound-and-return behavior.
    Return,
    /// Actor displacement opposite a shot.
    Recoil,
    /// Damage dealt to the actor, presented as Backlash.
    SelfDamage,
    /// Actor relocation.
    Teleport,
    /// Pull toward a source.
    Pull,
    /// Push away from a source.
    Push,
    /// Extra damage caused by striking terrain.
    WallImpact,
    /// Movement and displacement prevention.
    Lockdown,
    /// Persistent turret creation.
    SpawnTurret,
    /// Health restoration.
    Heal,
    /// Health transfer between players.
    HealthTransfer,
    /// Repeated attack resolution.
    MultiStrike,
    /// Guaranteed critical-hit status.
    GuaranteeCrit,
    /// Persistent embedded-projectile creation.
    EmbedProjectile,
    /// Deterministic embedded-object detonation.
    ChainDetonate,
    /// Relocation of a target to another position.
    Relocate,
    /// Line-of-sight obstruction.
    Obscure,
}

/// Client-facing persistent-object kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClientObjectKind {
    /// Emi's cube turret.
    Turret,
    /// One of Aleph's embedded knives.
    EmbeddedKnife,
    /// Aleph's line-of-sight-blocking gas cloud.
    GasCloud,
}

/// One addressable destructible terrain block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSnapshot {
    /// Stable per-match block identifier.
    pub id: u32,
    /// Inclusive leftmost cell.
    pub origin_cell_x: i32,
    /// Inclusive topmost cell.
    pub origin_cell_y: i32,
    /// Width in cells.
    pub width_cells: u16,
    /// Height in cells.
    pub height_cells: u16,
    /// Material written to surviving block cells.
    pub material: ClientMaterial,
    /// Current block health.
    pub health: u16,
    /// Maximum block health.
    pub max_health: u16,
    /// Direction in which lost health erodes cells.
    pub erosion_axis: ClientErosionAxis,
}

/// One active status attached to a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSnapshot {
    /// Closed status/effect identifier.
    pub kind: ClientStatusKind,
    /// Authoritative integer magnitude.
    pub magnitude: i32,
    /// Turns remaining before removal.
    pub turns_remaining: u8,
}

/// Cosmetic appearance selected for one player.
///
/// These fields are present for rendering but intentionally excluded from the
/// authoritative state hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppearanceSnapshot {
    /// Character skin identifier.
    pub skin_id: String,
    /// Ability effect skin identifiers in slot order.
    pub ability_skin_ids: [String; 3],
    /// Victory pose identifier.
    pub victory_pose_id: String,
}

/// Complete client-visible authoritative and cosmetic state for one player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSnapshot {
    /// Opaque match-local player identifier.
    pub id: String,
    /// Team index.
    pub team: u8,
    /// Current health; zero means eliminated.
    pub health: u16,
    /// Explicit authoritative elimination state.
    ///
    /// This intentionally accompanies `health` so presentation code does not duplicate
    /// the core's elimination rule.
    pub is_eliminated: bool,
    /// Maximum health for this match.
    pub max_health: u16,
    /// Authoritative fixed-point position.
    pub position: PositionSnapshot,
    /// Stable character definition identifier.
    pub character_id: String,
    /// Chosen passive identifier, if the one-time choice has occurred.
    pub passive_id: Option<String>,
    /// Special gauge in hundredths, from zero through ten thousand.
    pub special_gauge: u16,
    /// Whether the one-time passive-selection gate has been consumed.
    pub has_chosen_passive: bool,
    /// Active statuses in deterministic kind/magnitude/duration order.
    pub statuses: Vec<StatusSnapshot>,
    /// Cosmetic appearance used only by presentation.
    pub appearance: AppearanceSnapshot,
}

/// One persistent, destructible object in the match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentObjectSnapshot {
    /// Monotonic per-match creation sequence.
    pub sequence: u32,
    /// Opaque owner player identifier.
    pub owner_id: String,
    /// Closed object-kind identifier.
    pub kind: ClientObjectKind,
    /// Authoritative fixed-point position.
    pub position: PositionSnapshot,
    /// Remaining health.
    pub health: u16,
    /// Turns until expiry; `u8::MAX` means no automatic expiry.
    pub turns_remaining: u8,
}

/// An immutable core-state projection for one authoritative match generation.
///
/// The caller supplies `generation`. It is monotonically increased by the host or adapter
/// publishing snapshots and is not part of authoritative state or its hash. Consumers use it
/// to discard stale asynchronous deliveries without interpreting simulation ticks as network
/// ordering.
///
/// Session and transport metadata are intentionally outside this DTO; see the module-level
/// boundary documentation. Terrain cells are also separate and must be fetched through the
/// coarse terrain export when [`Self::terrain_generation`] changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchSnapshot {
    /// Version of this projection's shape and semantics.
    pub client_contract_version: u32,
    /// Version of the authoritative simulation rules.
    pub simulation_version: u32,
    /// Version of gameplay content definitions.
    pub content_version: u32,
    /// Caller-supplied monotonic publication generation.
    pub generation: u64,
    /// Authoritative simulation tick.
    pub tick: u64,
    /// Monotonic turn number.
    pub turn_number: u32,
    /// Current authoritative match phase.
    pub phase: ClientMatchPhase,
    /// Active player, or `None` while the authoritative id is empty.
    pub active_player_id: Option<String>,
    /// Living players in current-then-upcoming turn order, exactly once each.
    ///
    /// The active player is first when still alive. If the active player is eliminated (or
    /// absent before the first turn), the first entry is the next eligible player according
    /// to the scheduler's sorted-ID rotation. Eliminated players are omitted.
    pub current_and_upcoming_player_ids: Vec<String>,
    /// Horizontal wind acceleration per simulation tick.
    pub wind_per_tick: i32,
    /// Active player's remaining fixed-point movement allowance.
    pub movement_remaining: i32,
    /// Whether the active player has committed an attack this turn.
    pub has_attacked_this_turn: bool,
    /// Current authoritative match outcome.
    pub outcome: ClientMatchOutcome,
    /// Width of the separately exported terrain mask, in cells.
    pub terrain_width: u32,
    /// Height of the separately exported terrain mask, in cells.
    pub terrain_height: u32,
    /// Deterministic terrain mutation generation for the coarse terrain export.
    ///
    /// This is the core's next terrain-operation sequence: it begins at zero and changes
    /// after every authoritative terrain mutation. Consumers compare it for equality and
    /// refetch the separate row-major cell payload whenever it differs.
    pub terrain_generation: u32,
    /// Destructible blocks, sorted by stable block id.
    pub blocks: Vec<BlockSnapshot>,
    /// Players, sorted by opaque player id.
    pub players: Vec<PlayerSnapshot>,
    /// Persistent objects, sorted by creation sequence with deterministic tie-breakers.
    pub persistent_objects: Vec<PersistentObjectSnapshot>,
    /// Canonical hash of the complete authoritative state, including internal fields not
    /// exposed to the client.
    pub authoritative_state_hash: String,
}

impl MatchSnapshot {
    /// Projects a detached client snapshot from `host` without mutating it.
    ///
    /// `generation` belongs to the publishing adapter rather than the simulation. Callers
    /// must increase it monotonically for each published snapshot.
    #[must_use]
    pub fn from_host(host: &MatchHost, generation: u64) -> Self {
        let state = host.state();
        let active_player_id = if state.active_player_id.is_empty() {
            None
        } else {
            Some(state.active_player_id.clone())
        };

        Self {
            client_contract_version: CLIENT_CONTRACT_VERSION,
            simulation_version: state.simulation_version,
            content_version: state.content_version,
            generation,
            tick: state.tick,
            turn_number: state.turn_number,
            phase: snapshot_phase(state.phase),
            active_player_id,
            current_and_upcoming_player_ids: snapshot_turn_order(state),
            wind_per_tick: state.wind_per_tick,
            movement_remaining: state.movement_remaining,
            has_attacked_this_turn: state.has_attacked_this_turn,
            outcome: snapshot_outcome(host.outcome()),
            terrain_width: state.terrain.width,
            terrain_height: state.terrain.height,
            terrain_generation: state.next_terrain_sequence,
            blocks: snapshot_blocks(&state.blocks),
            players: snapshot_players(&state.players),
            persistent_objects: snapshot_objects(&state.objects),
            authoritative_state_hash: crate::hash::hash_state(state),
        }
    }
}

const fn snapshot_position(position: FixedPoint) -> PositionSnapshot {
    PositionSnapshot {
        x: position.x,
        y: position.y,
    }
}

const fn snapshot_phase(phase: MatchPhase) -> ClientMatchPhase {
    match phase {
        MatchPhase::MatchIntro => ClientMatchPhase::MatchIntro,
        MatchPhase::TurnStart => ClientMatchPhase::TurnStart,
        MatchPhase::Movement => ClientMatchPhase::Movement,
        MatchPhase::AimingAndSelection => ClientMatchPhase::AimingAndSelection,
        MatchPhase::PassiveSelection => ClientMatchPhase::PassiveSelection,
        MatchPhase::CommandLocked => ClientMatchPhase::CommandLocked,
        MatchPhase::Resolution => ClientMatchPhase::Resolution,
        MatchPhase::Settling => ClientMatchPhase::Settling,
        MatchPhase::StatusResolution => ClientMatchPhase::StatusResolution,
        MatchPhase::VictoryCheck => ClientMatchPhase::VictoryCheck,
        MatchPhase::MatchComplete => ClientMatchPhase::MatchComplete,
    }
}

const fn snapshot_outcome(outcome: MatchOutcome) -> ClientMatchOutcome {
    match outcome {
        MatchOutcome::InProgress => ClientMatchOutcome::InProgress,
        MatchOutcome::Victory { team } => ClientMatchOutcome::Victory { team },
        MatchOutcome::Draw => ClientMatchOutcome::Draw,
    }
}

const fn snapshot_material(material: Material) -> ClientMaterial {
    match material {
        Material::Empty => ClientMaterial::Empty,
        Material::Soil => ClientMaterial::Soil,
        Material::Wood => ClientMaterial::Wood,
        Material::ReinforcedStone => ClientMaterial::ReinforcedStone,
    }
}

const fn snapshot_erosion_axis(axis: ErosionAxis) -> ClientErosionAxis {
    match axis {
        ErosionAxis::Columns => ClientErosionAxis::Columns,
        ErosionAxis::Rows => ClientErosionAxis::Rows,
    }
}

const fn snapshot_status_kind(kind: EffectKind) -> ClientStatusKind {
    match kind {
        EffectKind::Knockback => ClientStatusKind::Knockback,
        EffectKind::Chill => ClientStatusKind::Chill,
        EffectKind::Cluster => ClientStatusKind::Cluster,
        EffectKind::Embers => ClientStatusKind::Embers,
        EffectKind::Tunnel => ClientStatusKind::Tunnel,
        EffectKind::Return => ClientStatusKind::Return,
        EffectKind::Recoil => ClientStatusKind::Recoil,
        EffectKind::SelfDamage => ClientStatusKind::SelfDamage,
        EffectKind::Teleport => ClientStatusKind::Teleport,
        EffectKind::Pull => ClientStatusKind::Pull,
        EffectKind::Push => ClientStatusKind::Push,
        EffectKind::WallImpact => ClientStatusKind::WallImpact,
        EffectKind::Lockdown => ClientStatusKind::Lockdown,
        EffectKind::SpawnTurret => ClientStatusKind::SpawnTurret,
        EffectKind::Heal => ClientStatusKind::Heal,
        EffectKind::HealthTransfer => ClientStatusKind::HealthTransfer,
        EffectKind::MultiStrike => ClientStatusKind::MultiStrike,
        EffectKind::GuaranteeCrit => ClientStatusKind::GuaranteeCrit,
        EffectKind::EmbedProjectile => ClientStatusKind::EmbedProjectile,
        EffectKind::ChainDetonate => ClientStatusKind::ChainDetonate,
        EffectKind::Relocate => ClientStatusKind::Relocate,
        EffectKind::Obscure => ClientStatusKind::Obscure,
    }
}

const fn snapshot_object_kind(kind: PersistentObjectKind) -> ClientObjectKind {
    match kind {
        PersistentObjectKind::Turret => ClientObjectKind::Turret,
        PersistentObjectKind::EmbeddedKnife => ClientObjectKind::EmbeddedKnife,
        PersistentObjectKind::GasCloud => ClientObjectKind::GasCloud,
    }
}

fn snapshot_turn_order(state: &SimulationState) -> Vec<String> {
    let mut living_ids: Vec<String> = state
        .players
        .iter()
        .filter(|player| !player.is_eliminated())
        .map(|player| player.id.clone())
        .collect();
    living_ids.sort();

    let len = living_ids.len();
    if len == 0 {
        return living_ids;
    }

    // A found active id is the current player and therefore leads the sequence. If the
    // active id is absent (pre-match or eliminated), its insertion point is the next living
    // player in scheduler order. A past-the-end insertion point wraps to the lowest id.
    let anchor = match living_ids.binary_search(&state.active_player_id) {
        Ok(index) | Err(index) => index,
    };
    let start = anchor.checked_rem(len).unwrap_or(0);
    living_ids.rotate_left(start);
    living_ids
}

fn snapshot_blocks(source: &[TerrainBlock]) -> Vec<BlockSnapshot> {
    let mut blocks: Vec<BlockSnapshot> = source
        .iter()
        .map(|block| BlockSnapshot {
            id: block.id,
            origin_cell_x: block.origin_cell_x,
            origin_cell_y: block.origin_cell_y,
            width_cells: block.width_cells,
            height_cells: block.height_cells,
            material: snapshot_material(block.material),
            health: block.health,
            max_health: block.max_health,
            erosion_axis: snapshot_erosion_axis(block.erosion_axis),
        })
        .collect();
    blocks.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.origin_cell_x.cmp(&right.origin_cell_x))
            .then_with(|| left.origin_cell_y.cmp(&right.origin_cell_y))
    });
    blocks
}

fn snapshot_statuses(source: &[StatusEffect]) -> Vec<StatusSnapshot> {
    let mut statuses: Vec<StatusSnapshot> = source
        .iter()
        .map(|status| StatusSnapshot {
            kind: snapshot_status_kind(status.kind),
            magnitude: status.magnitude,
            turns_remaining: status.turns_remaining,
        })
        .collect();
    statuses.sort_by_key(|status| (status.kind, status.magnitude, status.turns_remaining));
    statuses
}

fn snapshot_appearance(source: &Appearance) -> AppearanceSnapshot {
    AppearanceSnapshot {
        skin_id: source.skin_id.clone(),
        ability_skin_ids: source.ability_skin_ids.clone(),
        victory_pose_id: source.victory_pose_id.clone(),
    }
}

fn snapshot_players(source: &[PlayerState]) -> Vec<PlayerSnapshot> {
    let mut players: Vec<PlayerSnapshot> = source
        .iter()
        .map(|player| PlayerSnapshot {
            id: player.id.clone(),
            team: player.team,
            health: player.health,
            is_eliminated: player.is_eliminated(),
            max_health: player.max_health,
            position: snapshot_position(player.position),
            character_id: player.character_id.clone(),
            passive_id: player.passive_id.clone(),
            special_gauge: player.special_gauge,
            has_chosen_passive: player.has_chosen_passive,
            statuses: snapshot_statuses(&player.statuses),
            appearance: snapshot_appearance(&player.appearance),
        })
        .collect();
    players.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.team.cmp(&right.team))
    });
    players
}

fn snapshot_objects(source: &[PersistentObject]) -> Vec<PersistentObjectSnapshot> {
    let mut objects: Vec<PersistentObjectSnapshot> = source
        .iter()
        .map(|object| PersistentObjectSnapshot {
            sequence: object.sequence,
            owner_id: object.owner_id.clone(),
            kind: snapshot_object_kind(object.kind),
            position: snapshot_position(object.position),
            health: object.health,
            turns_remaining: object.turns_remaining,
        })
        .collect();
    objects.sort_by(|left, right| {
        left.sequence
            .cmp(&right.sequence)
            .then_with(|| left.owner_id.cmp(&right.owner_id))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.position.x.cmp(&right.position.x))
            .then_with(|| left.position.y.cmp(&right.position.y))
    });
    objects
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::match_setup::{MatchConfig, MatchMode, MatchPlayerConfig, build_initial_state};

    fn player_config(
        player_id: &str,
        team: u8,
        character_id: &str,
        skin_id: &str,
    ) -> MatchPlayerConfig {
        MatchPlayerConfig {
            player_id: player_id.to_owned(),
            team,
            character_id: character_id.to_owned(),
            appearance: Appearance {
                skin_id: skin_id.to_owned(),
                ability_skin_ids: [
                    format!("{skin_id}-main"),
                    format!("{skin_id}-secondary"),
                    format!("{skin_id}-melee"),
                ],
                victory_pose_id: format!("{skin_id}-victory"),
            },
        }
    }

    fn populated_host() -> MatchHost {
        let config = MatchConfig {
            seed: 0x0A11_CE55,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                player_config("zeta", 2, "zeke", "skin-zeta"),
                player_config("alpha", 1, "huck", "skin-alpha"),
                player_config("beta", 3, "huck", "skin-beta"),
            ],
        };
        let Ok(mut state) = build_initial_state(&config) else {
            panic!("complete client-contract fixture must build");
        };

        state.tick = 4_242;
        state.wind_per_tick = -73;
        state.next_terrain_sequence = 23;

        let Some(alpha) = state.player_mut("alpha") else {
            panic!("alpha fixture player must exist");
        };
        alpha.health = 777;
        alpha.passive_id = Some("huck-unyielding".to_owned());
        alpha.special_gauge = 8_765;
        alpha.has_chosen_passive = true;
        alpha.statuses = vec![
            StatusEffect {
                kind: EffectKind::GuaranteeCrit,
                magnitude: 3,
                turns_remaining: 2,
            },
            StatusEffect {
                kind: EffectKind::Chill,
                magnitude: -512,
                turns_remaining: 1,
            },
        ];

        let Some(beta) = state.player_mut("beta") else {
            panic!("beta fixture player must exist");
        };
        beta.health = 0;

        let Some(block) = state.blocks.iter_mut().find(|block| block.id == 2) else {
            panic!("fixture block must exist");
        };
        block.health = 321;
        block.erosion_axis = ErosionAxis::Rows;

        state.objects = vec![
            PersistentObject {
                sequence: 91,
                owner_id: "zeta".to_owned(),
                kind: PersistentObjectKind::GasCloud,
                position: FixedPoint::new(9_100, 8_200),
                health: 12,
                turns_remaining: 4,
            },
            PersistentObject {
                sequence: 7,
                owner_id: "alpha".to_owned(),
                kind: PersistentObjectKind::Turret,
                position: FixedPoint::new(700, 800),
                health: 44,
                turns_remaining: u8::MAX,
            },
        ];

        // Projection order is contractual even if a future state-building caller hands the
        // host vectors in a noncanonical order. Gameplay currently documents these vectors as
        // sorted, but the read boundary should not make that an unchecked transport premise.
        state.players.reverse();
        state.blocks.reverse();

        let Ok(host) = MatchHost::start(state) else {
            panic!("complete client-contract fixture must start");
        };
        host
    }

    #[test]
    fn snapshot_projects_every_required_field_and_authoritative_hash() {
        let host = populated_host();
        let authoritative = host.state();
        let snapshot = MatchSnapshot::from_host(&host, 55);

        let MatchSnapshot {
            client_contract_version,
            simulation_version,
            content_version,
            generation,
            tick,
            turn_number,
            phase,
            active_player_id,
            current_and_upcoming_player_ids,
            wind_per_tick,
            movement_remaining,
            has_attacked_this_turn,
            outcome,
            terrain_width,
            terrain_height,
            terrain_generation,
            blocks,
            players,
            persistent_objects,
            authoritative_state_hash,
        } = snapshot;

        assert_eq!(client_contract_version, CLIENT_CONTRACT_VERSION);
        assert_eq!(simulation_version, authoritative.simulation_version);
        assert_eq!(content_version, authoritative.content_version);
        assert_eq!(generation, 55);
        assert_eq!(tick, 4_242);
        assert_eq!(turn_number, authoritative.turn_number);
        assert_eq!(phase, ClientMatchPhase::Movement);
        assert_eq!(active_player_id.as_deref(), Some("alpha"));
        assert_eq!(
            current_and_upcoming_player_ids,
            ["alpha".to_owned(), "zeta".to_owned()]
        );
        assert_eq!(wind_per_tick, -73);
        assert_eq!(movement_remaining, authoritative.movement_remaining);
        assert_eq!(has_attacked_this_turn, authoritative.has_attacked_this_turn);
        assert_eq!(outcome, ClientMatchOutcome::InProgress);

        assert_eq!(terrain_width, authoritative.terrain.width);
        assert_eq!(terrain_height, authoritative.terrain.height);
        assert_eq!(terrain_generation, 23);

        assert_eq!(blocks.len(), authoritative.blocks.len());
        assert!(
            blocks
                .windows(2)
                .all(|pair| matches!(pair, [left, right] if left.id < right.id))
        );
        let Some(block) = blocks.iter().find(|block| block.id == 2) else {
            panic!("projected block must exist");
        };
        let Some(authoritative_block) = authoritative.blocks.iter().find(|block| block.id == 2)
        else {
            panic!("authoritative block must exist");
        };
        assert_eq!(block.origin_cell_x, authoritative_block.origin_cell_x);
        assert_eq!(block.origin_cell_y, authoritative_block.origin_cell_y);
        assert_eq!(block.width_cells, authoritative_block.width_cells);
        assert_eq!(block.height_cells, authoritative_block.height_cells);
        assert_eq!(
            block.material,
            snapshot_material(authoritative_block.material)
        );
        assert_eq!(block.health, authoritative_block.health);
        assert_eq!(block.max_health, authoritative_block.max_health);
        assert_eq!(block.erosion_axis, ClientErosionAxis::Rows);

        assert_eq!(players.len(), authoritative.players.len());
        assert!(
            players
                .windows(2)
                .all(|pair| matches!(pair, [left, right] if left.id < right.id))
        );
        let Some(alpha) = players.iter().find(|player| player.id == "alpha") else {
            panic!("projected alpha player must exist");
        };
        let Some(authoritative_alpha) = authoritative.player("alpha") else {
            panic!("authoritative alpha player must exist");
        };
        assert_eq!(alpha.team, 1);
        assert_eq!(alpha.health, 777);
        assert!(!alpha.is_eliminated);
        assert_eq!(alpha.max_health, authoritative_alpha.max_health);
        assert_eq!(
            alpha.position,
            snapshot_position(authoritative_alpha.position)
        );
        assert_eq!(alpha.character_id, "huck");
        assert_eq!(alpha.passive_id.as_deref(), Some("huck-unyielding"));
        assert_eq!(alpha.special_gauge, 8_765);
        assert!(alpha.has_chosen_passive);
        assert_eq!(alpha.statuses.len(), 2);
        assert!(
            alpha
                .statuses
                .windows(2)
                .all(|pair| matches!(pair, [left, right] if left.kind < right.kind))
        );
        let Some(chill) = alpha
            .statuses
            .iter()
            .find(|status| status.kind == ClientStatusKind::Chill)
        else {
            panic!("projected Chill status must exist");
        };
        assert_eq!(chill.magnitude, -512);
        assert_eq!(chill.turns_remaining, 1);
        assert_eq!(alpha.appearance.skin_id, "skin-alpha");
        assert_eq!(
            alpha.appearance.ability_skin_ids,
            [
                "skin-alpha-main".to_owned(),
                "skin-alpha-secondary".to_owned(),
                "skin-alpha-melee".to_owned(),
            ]
        );
        assert_eq!(alpha.appearance.victory_pose_id, "skin-alpha-victory");

        let Some(beta) = players.iter().find(|player| player.id == "beta") else {
            panic!("projected beta player must exist");
        };
        assert_eq!(beta.health, 0);
        assert!(beta.is_eliminated);

        assert_eq!(persistent_objects.len(), 2);
        assert!(
            persistent_objects
                .windows(2)
                .all(|pair| matches!(pair, [left, right] if left.sequence < right.sequence))
        );
        let Some(turret) = persistent_objects
            .iter()
            .find(|object| object.sequence == 7)
        else {
            panic!("projected turret must exist");
        };
        assert_eq!(turret.owner_id, "alpha");
        assert_eq!(turret.kind, ClientObjectKind::Turret);
        assert_eq!(turret.position, PositionSnapshot { x: 700, y: 800 });
        assert_eq!(turret.health, 44);
        assert_eq!(turret.turns_remaining, u8::MAX);

        assert_eq!(
            authoritative_state_hash,
            crate::hash::hash_state(authoritative)
        );
    }

    #[test]
    fn caller_generation_changes_no_authoritative_data_or_hash() {
        let host = populated_host();
        let first = MatchSnapshot::from_host(&host, 100);
        let second = MatchSnapshot::from_host(&host, 101);

        assert_eq!(first.generation, 100);
        assert_eq!(second.generation, 101);
        assert_eq!(
            first.authoritative_state_hash,
            second.authoritative_state_hash
        );
        assert_eq!(first.tick, second.tick);
        assert_eq!(first.turn_number, second.turn_number);
        assert_eq!(first.players, second.players);
        assert_eq!(first.terrain_width, second.terrain_width);
        assert_eq!(first.terrain_height, second.terrain_height);
        assert_eq!(first.terrain_generation, second.terrain_generation);
    }

    #[test]
    fn turn_order_matches_sorted_rotation_and_omits_eliminated_players() {
        let config = MatchConfig {
            seed: 17,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                player_config("delta", 4, "huck", "skin-delta"),
                player_config("bravo", 2, "huck", "skin-bravo"),
                player_config("alpha", 1, "huck", "skin-alpha"),
                player_config("charlie", 3, "huck", "skin-charlie"),
            ],
        };
        let Ok(mut state) = build_initial_state(&config) else {
            panic!("turn-order fixture must build");
        };

        state.active_player_id = "bravo".to_owned();
        assert_eq!(
            snapshot_turn_order(&state),
            [
                "bravo".to_owned(),
                "charlie".to_owned(),
                "delta".to_owned(),
                "alpha".to_owned(),
            ]
        );

        let Some(bravo) = state.player_mut("bravo") else {
            panic!("bravo fixture player must exist");
        };
        bravo.health = 0;
        assert_eq!(
            snapshot_turn_order(&state),
            ["charlie".to_owned(), "delta".to_owned(), "alpha".to_owned(),]
        );

        state.active_player_id = "zulu-eliminated-anchor".to_owned();
        assert_eq!(
            snapshot_turn_order(&state),
            ["alpha".to_owned(), "charlie".to_owned(), "delta".to_owned(),]
        );

        for player in &mut state.players {
            player.health = 0;
        }
        assert!(snapshot_turn_order(&state).is_empty());
    }

    #[test]
    fn closed_enum_projections_are_exhaustive_and_semantically_stable() {
        let phases = [
            (MatchPhase::MatchIntro, ClientMatchPhase::MatchIntro),
            (MatchPhase::TurnStart, ClientMatchPhase::TurnStart),
            (MatchPhase::Movement, ClientMatchPhase::Movement),
            (
                MatchPhase::AimingAndSelection,
                ClientMatchPhase::AimingAndSelection,
            ),
            (
                MatchPhase::PassiveSelection,
                ClientMatchPhase::PassiveSelection,
            ),
            (MatchPhase::CommandLocked, ClientMatchPhase::CommandLocked),
            (MatchPhase::Resolution, ClientMatchPhase::Resolution),
            (MatchPhase::Settling, ClientMatchPhase::Settling),
            (
                MatchPhase::StatusResolution,
                ClientMatchPhase::StatusResolution,
            ),
            (MatchPhase::VictoryCheck, ClientMatchPhase::VictoryCheck),
            (MatchPhase::MatchComplete, ClientMatchPhase::MatchComplete),
        ];
        for (core, client) in phases {
            assert_eq!(snapshot_phase(core), client);
        }

        assert_eq!(
            snapshot_outcome(MatchOutcome::InProgress),
            ClientMatchOutcome::InProgress
        );
        assert_eq!(
            snapshot_outcome(MatchOutcome::Victory { team: 9 }),
            ClientMatchOutcome::Victory { team: 9 }
        );
        assert_eq!(
            snapshot_outcome(MatchOutcome::Draw),
            ClientMatchOutcome::Draw
        );

        let materials = [
            (Material::Empty, ClientMaterial::Empty),
            (Material::Soil, ClientMaterial::Soil),
            (Material::Wood, ClientMaterial::Wood),
            (Material::ReinforcedStone, ClientMaterial::ReinforcedStone),
        ];
        for (core, client) in materials {
            assert_eq!(snapshot_material(core), client);
        }

        assert_eq!(
            snapshot_erosion_axis(ErosionAxis::Columns),
            ClientErosionAxis::Columns
        );
        assert_eq!(
            snapshot_erosion_axis(ErosionAxis::Rows),
            ClientErosionAxis::Rows
        );

        let object_kinds = [
            (PersistentObjectKind::Turret, ClientObjectKind::Turret),
            (
                PersistentObjectKind::EmbeddedKnife,
                ClientObjectKind::EmbeddedKnife,
            ),
            (PersistentObjectKind::GasCloud, ClientObjectKind::GasCloud),
        ];
        for (core, client) in object_kinds {
            assert_eq!(snapshot_object_kind(core), client);
        }

        let status_kinds = [
            (EffectKind::Knockback, ClientStatusKind::Knockback),
            (EffectKind::Chill, ClientStatusKind::Chill),
            (EffectKind::Cluster, ClientStatusKind::Cluster),
            (EffectKind::Embers, ClientStatusKind::Embers),
            (EffectKind::Tunnel, ClientStatusKind::Tunnel),
            (EffectKind::Return, ClientStatusKind::Return),
            (EffectKind::Recoil, ClientStatusKind::Recoil),
            (EffectKind::SelfDamage, ClientStatusKind::SelfDamage),
            (EffectKind::Teleport, ClientStatusKind::Teleport),
            (EffectKind::Pull, ClientStatusKind::Pull),
            (EffectKind::Push, ClientStatusKind::Push),
            (EffectKind::WallImpact, ClientStatusKind::WallImpact),
            (EffectKind::Lockdown, ClientStatusKind::Lockdown),
            (EffectKind::SpawnTurret, ClientStatusKind::SpawnTurret),
            (EffectKind::Heal, ClientStatusKind::Heal),
            (EffectKind::HealthTransfer, ClientStatusKind::HealthTransfer),
            (EffectKind::MultiStrike, ClientStatusKind::MultiStrike),
            (EffectKind::GuaranteeCrit, ClientStatusKind::GuaranteeCrit),
            (
                EffectKind::EmbedProjectile,
                ClientStatusKind::EmbedProjectile,
            ),
            (EffectKind::ChainDetonate, ClientStatusKind::ChainDetonate),
            (EffectKind::Relocate, ClientStatusKind::Relocate),
            (EffectKind::Obscure, ClientStatusKind::Obscure),
        ];
        for (core, client) in status_kinds {
            assert_eq!(snapshot_status_kind(core), client);
        }
    }
}
