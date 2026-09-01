//! Validated construction of a playable match from lobby-owned configuration.
//!
//! Tests historically built [`SimulationState`] with struct literals. That is useful for
//! isolated scenarios, but it is not a client or server boundary: a real host needs one
//! production path that resolves authored map and character identifiers, assigns spawn
//! points deterministically, rejects malformed rosters, and only then starts [`MatchHost`].
//!
//! This module is deliberately free of transport concerns. JSON, a C ABI, and a network
//! protocol may all decode into [`MatchConfig`], but none of them gets a second way to
//! construct authoritative state.

use std::collections::BTreeSet;

use crate::character;
use crate::error::{SimError, SimResult};
use crate::map::{self, MapDefinition};
use crate::match_host::MatchHost;
use crate::types::{
    Appearance, CROW_MAX_HEALTH, Loadout, MatchPhase, PlayerState, SimulationState, TurnEndReason,
};
use crate::{CONTENT_VERSION, SIMULATION_VERSION};

/// Minimum supported roster size for the turn-based mode.
pub const MIN_MATCH_PLAYERS: usize = 2;
/// Maximum supported roster size for the first playable mode (`PRODUCT_SPEC.md` §2).
pub const MAX_MATCH_PLAYERS: usize = 4;
/// Maximum byte length of an opaque match-local ASCII player identifier.
pub const MAX_PLAYER_ID_BYTES: usize = 64;

/// The scheduler model selected for a match.
///
/// Only the turn-based implementation exists. Naming it in configuration prevents a later
/// real-time mode from silently reinterpreting an old match envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// One active player plans and commits an action at a time.
    TurnBased,
}

/// One lobby participant as needed to create authoritative state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchPlayerConfig {
    /// Opaque identifier unique within this match. Accepted bytes are ASCII alphanumeric,
    /// hyphen, underscore, period, and colon.
    pub player_id: String,
    /// Players with the same team value are allies.
    pub team: u8,
    /// Equipped items; each slot is ammunition for this match.
    pub loadout: Loadout,
    /// Cosmetic-only appearance selected before the match.
    pub appearance: Appearance,
}

/// Complete, version-independent input for constructing a local or server-hosted match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchConfig {
    /// Explicit entropy for the deterministic match PRNG.
    pub seed: u64,
    /// Stable authored map identifier.
    pub map_id: String,
    /// Scheduler model. Unsupported modes fail closed.
    pub mode: MatchMode,
    /// Lobby order. Spawn points are assigned in this order before players are sorted by id.
    pub players: Vec<MatchPlayerConfig>,
}

/// Validates `config`, constructs authoritative state, and starts its scheduler.
///
/// # Errors
///
/// Returns [`SimError::OutOfRange`] for malformed roster shape or player identifiers and
/// [`SimError::UnknownDefinition`] for unknown maps or characters. Map construction and
/// scheduler errors are propagated unchanged.
pub fn create_match(config: &MatchConfig) -> SimResult<MatchHost> {
    let state = build_initial_state(config)?;
    MatchHost::start(state)
}

/// Builds the pre-scheduler state represented by `config`.
///
/// Kept public for protocol and golden-fixture tests that need to compare the exact starting
/// state separately from scheduler initialization. Runtime callers normally want
/// [`create_match`].
///
/// # Errors
///
/// Uses the same validation and error contract as [`create_match`].
pub fn build_initial_state(config: &MatchConfig) -> SimResult<SimulationState> {
    if config.mode != MatchMode::TurnBased {
        return Err(SimError::OutOfRange {
            field: "match mode",
        });
    }
    if !(MIN_MATCH_PLAYERS..=MAX_MATCH_PLAYERS).contains(&config.players.len()) {
        return Err(SimError::OutOfRange {
            field: "player count",
        });
    }

    let definition = find_map(&config.map_id).ok_or(SimError::UnknownDefinition)?;
    if definition.spawn_points.len() < config.players.len() {
        return Err(SimError::OutOfRange {
            field: "map spawn count",
        });
    }

    validate_roster(&config.players)?;
    let terrain = map::build_mask(&definition)?;
    let mut players = Vec::with_capacity(config.players.len());

    for (player_config, spawn) in config.players.iter().zip(definition.spawn_points.iter()) {
        let ammo = character::ammo_for_loadout(&player_config.loadout)?;
        players.push(PlayerState {
            id: player_config.player_id.clone(),
            team: player_config.team,
            health: CROW_MAX_HEALTH,
            max_health: CROW_MAX_HEALTH,
            position: *spawn,
            loadout: player_config.loadout.clone(),
            ammo,
            statuses: Vec::new(),
            appearance: player_config.appearance.clone(),
        });
    }
    players.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(SimulationState {
        simulation_version: SIMULATION_VERSION,
        content_version: CONTENT_VERSION,
        tick: 0,
        turn_number: 0,
        phase: MatchPhase::MatchIntro,
        active_player_id: String::new(),
        wind_per_tick: 0,
        movement_remaining: 0,
        has_attacked_this_turn: false,
        terrain,
        blocks: definition.blocks,
        players,
        objects: Vec::new(),
        processed_command_ids: Vec::new(),
        next_terrain_sequence: 0,
        next_object_sequence: 0,
        rng_state: config.seed,
        pending_turn_end_reason: TurnEndReason::Passed,
        last_turn_end_reason: TurnEndReason::Passed,
    })
}

fn validate_roster(players: &[MatchPlayerConfig]) -> SimResult<()> {
    let mut player_ids = BTreeSet::new();
    let mut teams = BTreeSet::new();

    for player in players {
        if !is_valid_match_local_id(&player.player_id)
            || !player_ids.insert(player.player_id.as_str())
        {
            return Err(SimError::OutOfRange { field: "player id" });
        }
        if !loadout_ids_are_legal(&player.loadout) {
            return Err(SimError::OutOfRange { field: "loadout" });
        }
        if character::ammo_for_loadout(&player.loadout).is_err() {
            return Err(SimError::UnknownDefinition);
        }
        teams.insert(player.team);
    }

    if teams.len() < 2 {
        return Err(SimError::OutOfRange {
            field: "team count",
        });
    }
    Ok(())
}

/// Whether `id` satisfies the shared match-local opaque identifier contract.
///
/// Player IDs and normalized command IDs deliberately use the same bounded alphabet so
/// adapters cannot disagree about which identifier bytes are safe to retain and echo.
#[must_use]
pub(crate) fn is_valid_match_local_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_PLAYER_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn loadout_ids_are_legal(loadout: &Loadout) -> bool {
    [&loadout.main, &loadout.secondary, &loadout.melee_tool]
        .into_iter()
        .all(|id| !id.is_empty() && !id.contains('\0'))
}

fn find_map(id: &str) -> Option<MapDefinition> {
    map::find(id)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::fixed::POSITION_SCALE;

    fn player(id: &str, team: u8) -> MatchPlayerConfig {
        MatchPlayerConfig {
            player_id: id.to_owned(),
            team,
            loadout: Loadout::launch_default(),
            appearance: Appearance::default(),
        }
    }

    fn duel() -> MatchConfig {
        MatchConfig {
            seed: 12_345,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![player("z_right", 1), player("a_left", 0)],
        }
    }

    fn configured_player_mut(config: &mut MatchConfig, index: usize) -> &mut MatchPlayerConfig {
        let Some(player) = config.players.get_mut(index) else {
            panic!("fixture player index must exist");
        };
        player
    }

    #[test]
    fn a_real_match_is_created_from_roster_and_map_definitions() {
        let config = duel();
        let Ok(host) = create_match(&config) else {
            panic!("a valid duel must start");
        };

        assert_eq!(host.active_player(), "a_left");
        assert_eq!(host.state().players.len(), 2);
        assert_eq!(host.state().blocks.len(), 8);
        assert_eq!(host.state().terrain.width, 50);
        assert_eq!(host.state().terrain.height, 20);
        assert_eq!(host.state().rng_state, config.seed);

        let Some(crow) = host.state().player("a_left") else {
            panic!("configured player must exist");
        };
        assert_eq!(crow.health, CROW_MAX_HEALTH);
        assert_eq!(crow.max_health, CROW_MAX_HEALTH);
        assert_eq!(crow.loadout, Loadout::launch_default());
        assert_eq!(crow.ammo_for(crate::types::AbilitySlot::Basic).remaining, 3);
    }

    #[test]
    fn lobby_order_assigns_spawns_before_canonical_player_sorting() {
        let config = duel();
        let Ok(state) = build_initial_state(&config) else {
            panic!("a valid duel must build");
        };

        let Some(first_lobby_player) = state.player("z_right") else {
            panic!("first lobby player must exist");
        };
        let Some(second_lobby_player) = state.player("a_left") else {
            panic!("second lobby player must exist");
        };
        assert_eq!(first_lobby_player.position.x, 2 * POSITION_SCALE);
        assert_eq!(second_lobby_player.position.x, 8 * POSITION_SCALE);
        assert!(
            state
                .players
                .windows(2)
                .all(|pair| matches!(pair, [left, right] if left.id < right.id))
        );
    }

    #[test]
    fn identical_config_builds_identical_started_matches() {
        let config = duel();
        let Ok(first) = create_match(&config) else {
            panic!("first match must start");
        };
        let Ok(second) = create_match(&config) else {
            panic!("second match must start");
        };

        assert_eq!(first.state(), second.state());
    }

    #[test]
    fn invalid_player_counts_fail_closed() {
        let mut too_few = duel();
        too_few.players.pop();
        assert_eq!(
            create_match(&too_few).err(),
            Some(SimError::OutOfRange {
                field: "player count"
            })
        );

        let mut too_many = duel();
        too_many
            .players
            .extend([player("c", 2), player("d", 3), player("e", 4)]);
        assert_eq!(
            create_match(&too_many).err(),
            Some(SimError::OutOfRange {
                field: "player count"
            })
        );
    }

    #[test]
    fn duplicate_or_malformed_player_ids_fail_closed() {
        let mut duplicate = duel();
        let Some(first_id) = duplicate
            .players
            .first()
            .map(|player| player.player_id.clone())
        else {
            panic!("fixture first player must exist");
        };
        configured_player_mut(&mut duplicate, 1).player_id = first_id;
        assert_eq!(
            create_match(&duplicate).err(),
            Some(SimError::OutOfRange { field: "player id" })
        );

        let mut empty = duel();
        configured_player_mut(&mut empty, 0).player_id.clear();
        assert_eq!(
            create_match(&empty).err(),
            Some(SimError::OutOfRange { field: "player id" })
        );

        let mut nul = duel();
        configured_player_mut(&mut nul, 0).player_id = "bad\0id".to_owned();
        assert_eq!(
            create_match(&nul).err(),
            Some(SimError::OutOfRange { field: "player id" })
        );

        let mut whitespace = duel();
        configured_player_mut(&mut whitespace, 0).player_id = "bad id".to_owned();
        assert_eq!(
            create_match(&whitespace).err(),
            Some(SimError::OutOfRange { field: "player id" })
        );

        let mut non_ascii = duel();
        configured_player_mut(&mut non_ascii, 0).player_id = "joueur-é".to_owned();
        assert_eq!(
            create_match(&non_ascii).err(),
            Some(SimError::OutOfRange { field: "player id" })
        );
    }

    #[test]
    fn player_id_alphabet_and_byte_limit_are_exact() {
        assert!(is_valid_match_local_id("Player_9.local:blue-team"));
        assert!(!is_valid_match_local_id("contains space"));
        assert!(!is_valid_match_local_id("é"));
        assert!(is_valid_match_local_id(&"a".repeat(MAX_PLAYER_ID_BYTES)));
        assert!(!is_valid_match_local_id(
            &"a".repeat(MAX_PLAYER_ID_BYTES.saturating_add(1))
        ));
    }

    #[test]
    fn a_roster_must_contain_opponents() {
        let mut config = duel();
        let Some(first_team) = config.players.first().map(|player| player.team) else {
            panic!("fixture first player must exist");
        };
        configured_player_mut(&mut config, 1).team = first_team;
        assert_eq!(
            create_match(&config).err(),
            Some(SimError::OutOfRange {
                field: "team count"
            })
        );
    }

    #[test]
    fn unknown_map_and_item_ids_fail_closed() {
        let mut bad_map = duel();
        bad_map.map_id = "missing-map".to_owned();
        assert_eq!(
            create_match(&bad_map).err(),
            Some(SimError::UnknownDefinition)
        );

        let mut bad_item = duel();
        configured_player_mut(&mut bad_item, 0).loadout.main = "missing-item".to_owned();
        assert_eq!(
            create_match(&bad_item).err(),
            Some(SimError::UnknownDefinition)
        );
    }
}
