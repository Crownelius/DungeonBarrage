//! Stacked-map and bot-outcome gates for the playable cut.
//!
//! Loads each stacked map, asserts living blocks are stacked (not a single row), then
//! drives a bot through the same [`MatchHost`] apply path a human uses until the match
//! is terminal.

#![allow(clippy::expect_used, clippy::panic)]

use db_sim_core::block_ops;
use db_sim_core::blocks;
use db_sim_core::bot::{self, BotDifficulty};
use db_sim_core::character;
use db_sim_core::client_contract::CLIENT_CONTRACT_VERSION;
use db_sim_core::map;
use db_sim_core::match_host::MatchHost;
use db_sim_core::match_session::{
    AbilityPreviewRequest, AuthorityTimeout, MatchCommandKind, MatchSessionHost,
    TransitionDisposition,
};
use db_sim_core::match_setup::{MatchConfig, MatchMode, MatchPlayerConfig, create_match};
use db_sim_core::types::{
    AbilityCommand, AbilitySlot, Appearance, CommandResult, Loadout, MatchOutcome, MatchPhase,
};

fn crow_player(id: &str, team: u8) -> MatchPlayerConfig {
    MatchPlayerConfig {
        player_id: id.to_owned(),
        team,
        loadout: Loadout::launch_default(),
        appearance: Appearance::default(),
    }
}

fn stacked_duel(map_id: &str, seed: u64) -> MatchConfig {
    MatchConfig {
        seed,
        map_id: map_id.to_owned(),
        mode: MatchMode::TurnBased,
        players: vec![crow_player("human", 0), crow_player("bot", 1)],
    }
}

fn submit_bot_kind(host: &mut MatchHost, kind: MatchCommandKind, command_id: &str) {
    let player_id = host.active_player().to_owned();
    let turn = host.state().turn_number;
    match kind {
        MatchCommandKind::Move { dx } => {
            host.submit_move(&player_id, dx)
                .expect("bot move must be a legal host submission");
        }
        MatchCommandKind::Ability {
            slot,
            angle_millidegrees,
            power_basis_points,
            target_player_id,
            secondary_target_player_id,
        } => {
            let command = AbilityCommand {
                command_id: command_id.to_owned(),
                player_id,
                expected_turn_number: turn,
                slot,
                angle_millidegrees,
                power_basis_points,
                target_player_id,
                secondary_target_player_id,
            };
            let result = host
                .submit_ability(&command)
                .expect("bot ability submission must not fault the host");
            if matches!(result, db_sim_core::types::CommandResult::Rejected(_)) {
                host.pass_turn()
                    .expect("a rejected bot shot must still yield the turn");
            }
        }
        MatchCommandKind::Pass | MatchCommandKind::PassiveChoice { .. } => {
            host.pass_turn().expect("bot pass must be legal");
        }
    }
}

#[test]
fn stacked_maps_are_not_a_single_unstacked_row() {
    for definition in map::stacked_catalog() {
        assert!(
            map::has_stacked_destructible_structure(&definition.blocks),
            "{} must contain stacked destructible structures",
            definition.id
        );
        let ys: std::collections::BTreeSet<_> = definition
            .blocks
            .iter()
            .map(|block| block.origin_cell_y)
            .collect();
        assert!(
            ys.len() >= 2,
            "{} must stack on more than one row",
            definition.id
        );
        assert!(definition.spawn_points.len() >= 2);
        create_match(&stacked_duel(definition.id, 7)).expect("stacked map must create a match");
    }
}

fn run_bot_until_terminal(host: &mut MatchHost, map_id: &str) {
    let mut steps = 0u32;
    while matches!(host.outcome(), MatchOutcome::InProgress) {
        if host.state().phase == MatchPhase::MatchComplete {
            break;
        }
        let active = host.active_player().to_owned();
        let kind = bot::decide(
            host.state(),
            &active,
            BotDifficulty::Standard,
            10_000u64.saturating_add(u64::from(steps)),
        );
        let command_id = format!("bot-step-{steps}");
        submit_bot_kind(host, kind, &command_id);
        steps = steps.saturating_add(1);
        assert!(
            steps < 120,
            "{map_id}: a bot duel must reach a terminal outcome; still {:?}",
            host.outcome()
        );
    }

    match host.outcome() {
        MatchOutcome::Victory { .. } | MatchOutcome::Draw => {}
        MatchOutcome::InProgress => panic!("{map_id}: bot match must not stay in progress"),
    }
}

#[test]
fn a_bot_opponent_on_the_ordinary_apply_path_reaches_win_or_lose() {
    for (index, definition) in map::stacked_catalog().iter().enumerate() {
        let seed = 42u64.saturating_add(u64::try_from(index).unwrap_or(u64::MAX));
        let config = stacked_duel(definition.id, seed);
        let mut host =
            create_match(&config).unwrap_or_else(|_| panic!("{} duel must start", definition.id));
        assert!(
            map::has_stacked_destructible_structure(&host.state().blocks),
            "{} must start stacked",
            definition.id
        );
        run_bot_until_terminal(&mut host, definition.id);
    }
}

#[test]
fn destroying_support_on_each_stacked_map_drops_the_crown() {
    for definition in map::stacked_catalog() {
        let host = create_match(&stacked_duel(definition.id, 7))
            .unwrap_or_else(|_| panic!("{} must create", definition.id));
        let mut state = host.state().clone();
        let Some((upper_id, lower_id, upper_before)) = stacked_pair(&state.blocks) else {
            panic!("{} must contain a living stacked pair", definition.id);
        };
        for block in &mut state.blocks {
            if block.id == lower_id {
                block.health = 0;
                blocks::apply_to_mask(&mut state.terrain, block, None);
            }
        }
        block_ops::settle_unsupported_blocks(&mut state);
        let Some(upper) = state.blocks.iter().find(|block| block.id == upper_id) else {
            panic!(
                "{}: crown block {upper_id} must remain addressable",
                definition.id
            );
        };
        assert!(
            upper.health > 0,
            "{}: crown must survive the support collapse",
            definition.id
        );
        assert!(
            upper.origin_cell_y > upper_before,
            "{}: crown y {} must increase after support {lower_id} is destroyed (was {upper_before})",
            definition.id,
            upper.origin_cell_y
        );
    }
}

fn stacked_pair(blocks: &[db_sim_core::blocks::TerrainBlock]) -> Option<(u32, u32, i32)> {
    for upper in blocks {
        if upper.health == 0 {
            continue;
        }
        let upper_bottom = upper
            .origin_cell_y
            .saturating_add(i32::from(upper.height_cells));
        for lower in blocks {
            if lower.id == upper.id || lower.health == 0 {
                continue;
            }
            if lower.origin_cell_y == upper_bottom {
                return Some((upper.id, lower.id, upper.origin_cell_y));
            }
        }
    }
    None
}

fn loadout_equipping(item: &db_sim_core::types::ItemDefinition) -> Loadout {
    let mut loadout = Loadout::launch_default();
    match item.slot {
        AbilitySlot::Basic => loadout.main = item.id.to_owned(),
        AbilitySlot::BasicAlt => loadout.secondary = item.id.to_owned(),
        AbilitySlot::Special => loadout.melee_tool = item.id.to_owned(),
    }
    loadout
}

fn crow_player_with(id: &str, team: u8, loadout: Loadout) -> MatchPlayerConfig {
    MatchPlayerConfig {
        player_id: id.to_owned(),
        team,
        loadout,
        appearance: Appearance::default(),
    }
}

#[test]
fn every_catalog_item_fires_with_no_named_target_on_the_aim_path() {
    for item in character::LAUNCH_ITEMS {
        let loadout = loadout_equipping(item);
        let config = MatchConfig {
            seed: 12345,
            map_id: "crow-perch".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                crow_player_with("human", 0, loadout.clone()),
                crow_player_with("bot", 1, loadout),
            ],
        };
        let mut host = create_match(&config)
            .unwrap_or_else(|_| panic!("crow-perch with {} must start", item.id));
        let player_id = host.active_player().to_owned();
        let command = AbilityCommand {
            command_id: format!("aim-{}", item.id),
            player_id,
            expected_turn_number: host.state().turn_number,
            slot: item.slot,
            angle_millidegrees: 45_000,
            power_basis_points: 1_500,
            target_player_id: None,
            secondary_target_player_id: None,
        };
        let result = host
            .submit_ability(&command)
            .unwrap_or_else(|_| panic!("{} submit must not fault the host", item.id));
        assert!(
            matches!(result, CommandResult::Accepted(_)),
            "{} with target_player_id=None was {result:?}; Godot aim always sends a null target",
            item.id
        );
    }
}

#[test]
fn timeout_and_preview_work_on_crow_perch() {
    let mut session = MatchSessionHost::create(&stacked_duel("crow-perch", 7))
        .expect("crow-perch session must start");
    let preview = AbilityPreviewRequest {
        schema_version: CLIENT_CONTRACT_VERSION,
        expected_snapshot_generation: session.generation(),
        player_id: session.host().active_player().to_owned(),
        slot: AbilitySlot::Basic,
        angle_millidegrees: 45_000,
        power_basis_points: 1_500,
        target_player_id: None,
        secondary_target_player_id: None,
    };
    let before = session.host().state().clone();
    let guide = session.preview(&preview).expect("preview must resolve");
    assert!(
        guide.legal,
        "default ramshot on crow-perch must preview as legal"
    );
    assert_eq!(session.host().state(), &before, "preview must not mutate");

    let first = session.host().active_player().to_owned();
    let timeout = AuthorityTimeout {
        schema_version: CLIENT_CONTRACT_VERSION,
        action_id: "crow-perch-timeout".to_owned(),
        player_id: first.clone(),
        expected_turn_number: session.host().state().turn_number,
        expected_snapshot_generation: session.generation(),
    };
    let transition = session
        .apply_authority_timeout(timeout)
        .expect("timeout must be accepted");
    assert_eq!(transition.disposition, TransitionDisposition::Accepted);
    assert_ne!(
        session.host().active_player(),
        first,
        "timeout must hand the crow-perch turn over"
    );
}
