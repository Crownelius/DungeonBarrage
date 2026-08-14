//! Frozen golden vectors — the regression gate (`todolist.md` P6).
//!
//! Each vector is a scripted match: a seed, a fixed sequence of actions, and the state hash
//! the engine must produce. If a refactor changes any of these hashes, it changed observable
//! behaviour, and the diff has to justify that.
//!
//! # What this proves, and what it does not
//!
//! It proves **self-consistency**: the engine behaves identically across builds, machines,
//! and refactors. It does **not** prove correctness — the TypeScript reference oracle was
//! retired with the web surface (ADR 0004), so there is no independent implementation left to
//! check against. The corpus freezes whatever it is given, bugs included.
//!
//! That limitation is the reason for the rule below, not a footnote to it.
//!
//! # Regenerating a vector
//!
//! `docs/MODULE_OWNERSHIP.md` forbids changing a committed vector *silently*, not changing
//! one. When a behaviour change is genuinely intended:
//!
//! 1. Run `cargo test --test golden_vectors -- --nocapture`. A failure prints the old and new
//!    hashes side by side.
//! 2. Update the constant, and add a comment giving the date, the old value, and why.
//! 3. Bump `SIMULATION_VERSION` if the change affects replay compatibility.
//! 4. Do it in a commit whose message says a vector was regenerated. Never fold it into a
//!    feature commit — that is indistinguishable from breaking determinism by accident.
//!
//! These matches are deliberately scripted from `MatchHost`, the top of the engine, rather
//! than from individual subsystems. A vector over a helper only proves that helper still
//! behaves; a vector over an orchestrated match proves the whole loop still composes.

// A fixture that cannot be built is a broken test, not a runtime condition to handle, so
// panicking here is correct — the same allowance every unit-test module in this crate takes.
// Production code remains panic-free; that is what the workspace lint protects.
#![allow(clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use db_sim_core::fixed::{FixedPoint, POSITION_SCALE};
use db_sim_core::hash::hash_state;
use db_sim_core::match_host::MatchHost;
use db_sim_core::types::{
    AbilityCommand, AbilitySlot, Appearance, MatchPhase, PlayerState, SimulationState,
};
use db_sim_core::{CONTENT_VERSION, SIMULATION_VERSION, map};

/// One scripted action.
///
/// Deliberately a small closed vocabulary: a script that could express anything would drift
/// from what a real client can actually send.
#[derive(Debug, Clone, Copy)]
enum Step {
    /// Walk the active player by a fixed-point distance.
    Walk(i32),
    /// End the active player's turn without acting.
    Pass,
    /// Fire the given slot at a fixed angle and power.
    Fire {
        slot: AbilitySlot,
        angle_millidegrees: i32,
        power_basis_points: i32,
    },
}

fn player(
    id: &str,
    team: u8,
    character_id: &str,
    position: FixedPoint,
    health: u16,
) -> PlayerState {
    PlayerState {
        id: id.to_owned(),
        team,
        health,
        max_health: health,
        position,
        character_id: character_id.to_owned(),
        passive_id: None,
        special_gauge: 0,
        has_chosen_passive: false,
        statuses: Vec::new(),
        appearance: Appearance::default(),
    }
}

/// Builds a two-player duel on the horizontal test array.
///
/// Uses the real map and real roster characters. A vector over a synthetic fixture would
/// freeze the fixture, not the game.
fn duel(seed: u64, left: &str, right: &str, health: u16, opponent_spawn: usize) -> SimulationState {
    let definition = map::horizontal_test_array();
    let terrain = map::build_mask(&definition).expect("the test map must build");
    let first = *definition
        .spawn_points
        .first()
        .expect("the test map must have spawn points");
    let second = *definition
        .spawn_points
        .get(opponent_spawn)
        .expect("the requested spawn point must exist");

    let mut players = vec![
        player("a_left", 0, left, first, health),
        player("b_right", 1, right, second, health),
    ];
    players.sort_by(|l, r| l.id.cmp(&r.id));

    SimulationState {
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
        rng_state: seed,
    }
}

/// What a scripted match actually did, beyond producing a hash.
///
/// Exists so a vector cannot silently freeze a no-op. A script whose every command was
/// rejected still produces a perfectly stable hash, and without these counters that vector
/// would pass forever while testing nothing -- the exact failure mode this project has hit
/// four times in other guises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Outcome {
    hash_matches_start: bool,
    turns_elapsed: u32,
    total_health_lost: u32,
    blocks_damaged: usize,
}

/// Runs a script to completion and returns the final state hash plus what it did.
fn run_detailed(state: SimulationState, script: &[Step]) -> (String, Outcome) {
    let starting_hash = hash_state(&state);
    let starting_health: u32 = state.players.iter().map(|p| u32::from(p.health)).sum();
    let starting_blocks: Vec<u16> = state.blocks.iter().map(|b| b.health).collect();
    let mut host = MatchHost::start(state).expect("a match must be startable");

    for (index, step) in script.iter().enumerate() {
        if host.is_complete() {
            break;
        }
        let actor = host.active_player().to_owned();
        match *step {
            Step::Walk(dx) => {
                host.submit_move(&actor, dx)
                    .expect("walking must not error");
            }
            Step::Pass => host.pass_turn().expect("passing must not error"),
            Step::Fire {
                slot,
                angle_millidegrees,
                power_basis_points,
            } => {
                let command = AbilityCommand {
                    // Index-derived so a replay of the same script produces the same ids —
                    // a random id would make every run hash differently and the vector
                    // meaningless.
                    command_id: format!("golden-{index}"),
                    player_id: actor.clone(),
                    expected_turn_number: host.state().turn_number,
                    slot,
                    angle_millidegrees,
                    power_basis_points,
                    target_player_id: None,
                    secondary_target_player_id: None,
                };
                host.submit_ability(&command)
                    .expect("submitting must not error");
            }
        }
    }
    let final_state = host.state();
    let ending_health: u32 = final_state
        .players
        .iter()
        .map(|p| u32::from(p.health))
        .sum();
    let blocks_damaged = final_state
        .blocks
        .iter()
        .zip(starting_blocks.iter())
        .filter(|(now, before)| now.health < **before)
        .count();
    let hash = hash_state(final_state);

    let outcome = Outcome {
        hash_matches_start: hash == starting_hash,
        turns_elapsed: final_state.turn_number,
        total_health_lost: starting_health.saturating_sub(ending_health),
        blocks_damaged,
    };
    (hash, outcome)
}

/// Runs a script and returns only the hash, asserting the script did something first.
fn run(state: SimulationState, script: &[Step]) -> String {
    let (hash, outcome) = run_detailed(state, script);
    assert!(
        !outcome.hash_matches_start,
        "this script left the world untouched -- freezing its hash would test nothing",
    );
    assert!(
        outcome.turns_elapsed > 0,
        "this script never completed a turn: {outcome:?}",
    );
    hash
}

/// Asserts a vector, printing both hashes on failure so regeneration is mechanical.
fn assert_vector(name: &str, actual: &str, expected: &str) {
    assert_eq!(
        actual, expected,
        "\ngolden vector `{name}` changed.\n  expected: {expected}\n  actual:   {actual}\n\
         \nIf this change is intended, see this file's module docs before updating the constant.\n",
    );
}

// ---------------------------------------------------------------------------
// The corpus.
//
// Generated 2026-08-07 from the engine at SIMULATION_VERSION 3. Every hash below was
// produced by reviewed code; see the module docs for the regeneration rule.
// ---------------------------------------------------------------------------

#[test]
fn golden_all_passes_terminates_identically() {
    // The longest-running vector: nothing but passes, so it exercises the hard turn limit
    // and the forced draw. This is the one that catches a change to match termination.
    let script = vec![Step::Pass; 64];
    let actual = run(duel(1, "arzum", "emi", 300, 5), &script);
    assert_vector("all_passes", &actual, "e828d490e955f3d7");
}

#[test]
fn golden_walking_duel() {
    // Movement, settling, and turn rotation, with no combat.
    let script = vec![
        Step::Walk(2 * POSITION_SCALE),
        Step::Pass,
        Step::Walk(-3 * POSITION_SCALE),
        Step::Pass,
        Step::Walk(POSITION_SCALE),
        Step::Pass,
    ];
    let actual = run(duel(7, "arzum", "natomica", 300, 5), &script);
    assert_vector("walking_duel", &actual, "35636b623102bbed");
}

#[test]
fn golden_firing_duel() {
    // Abilities, damage, terrain destruction, and block erosion through a real command path.
    let script = vec![
        Step::Fire {
            slot: AbilitySlot::Basic,
            angle_millidegrees: 45_000,
            power_basis_points: 1_500,
        },
        Step::Fire {
            slot: AbilitySlot::Basic,
            angle_millidegrees: 60_000,
            power_basis_points: 2_500,
        },
        Step::Fire {
            slot: AbilitySlot::Basic,
            angle_millidegrees: 10_000,
            power_basis_points: 1_500,
        },
    ];
    let (actual, outcome) = run_detailed(duel(99, "roberto", "emi", 300, 1), &script);
    assert!(
        outcome.total_health_lost > 0,
        "a firing vector must actually deal damage, or it covers nothing: {outcome:?}",
    );
    assert_vector("firing_duel", &actual, "7b49a0275beafc1f");
}

#[test]
fn golden_mixed_actions() {
    // Movement interleaved with fire, which is the shape of a real turn.
    let script = vec![
        Step::Walk(POSITION_SCALE),
        Step::Fire {
            slot: AbilitySlot::Basic,
            angle_millidegrees: 45_000,
            power_basis_points: 1_500,
        },
        Step::Walk(-2 * POSITION_SCALE),
        Step::Fire {
            slot: AbilitySlot::Basic,
            angle_millidegrees: 60_000,
            power_basis_points: 2_500,
        },
        Step::Pass,
    ];
    let (actual, outcome) = run_detailed(duel(4_242, "karl", "numa", 300, 1), &script);
    assert!(
        outcome.turns_elapsed > 1,
        "a mixed script must advance several turns: {outcome:?}",
    );
    assert_vector("mixed_actions", &actual, "112717b8831056f8");
}

#[test]
fn golden_low_health_duel_reaches_a_decision() {
    // Low health so the match actually resolves rather than timing out, exercising the
    // elimination and victory path end to end.
    let script = vec![
        Step::Fire {
            slot: AbilitySlot::Basic,
            angle_millidegrees: 45_000,
            power_basis_points: 1_500,
        };
        24
    ];
    let (actual, outcome) = run_detailed(duel(11, "roberto", "roberto", 60, 1), &script);
    assert!(
        outcome.total_health_lost > 0,
        "the decision vector must actually damage somebody: {outcome:?}",
    );
    assert_vector("low_health_duel", &actual, "b88af74446995c79");
}

#[test]
fn a_vector_is_stable_across_repeated_runs_in_one_process() {
    // Guards against hidden per-run state: a vector that only holds on a cold process would
    // pass CI and fail a server that plays two matches.
    let script = vec![Step::Walk(POSITION_SCALE), Step::Pass, Step::Pass];
    let first = run(duel(5, "arzum", "emi", 300, 2), &script);
    let second = run(duel(5, "arzum", "emi", 300, 2), &script);
    assert_eq!(first, second, "the same script must hash identically twice");
}

#[test]
fn different_seeds_produce_different_matches() {
    // Proves the vectors are actually sensitive to the seed. Without this, a corpus of
    // identical hashes would look green while testing nothing.
    let script = vec![
        Step::Fire {
            slot: AbilitySlot::Basic,
            angle_millidegrees: 45_000,
            power_basis_points: 1_500,
        };
        6
    ];
    let a = run(duel(1, "karl", "karl", 300, 1), &script);
    let b = run(duel(2, "karl", "karl", 300, 1), &script);
    assert_ne!(a, b, "the seed must affect the match");
}
