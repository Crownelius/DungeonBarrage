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
    TurnEndReason,
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
    let profile = db_sim_core::character_roster::find(character_id)
        .unwrap_or_else(|| panic!("unknown golden character {character_id}"));
    PlayerState {
        id: id.to_owned(),
        team,
        health,
        max_health: health,
        position,
        loadout: profile.derived_loadout(),
        ammo: db_sim_core::types::CHARACTER_KIT_AMMO,
        trinket_charge: 0,
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
        pending_turn_end_reason: TurnEndReason::Passed,
        last_turn_end_reason: TurnEndReason::Passed,
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
// REGENERATED 2026-08-25 at SIMULATION_VERSION 6. Status durations now count the affected
// player's own turns, Feeding Frenzy forces and consumes its three live Carrion Call crits, and
// ordinary health-zero/fall elimination removes the defeated owner's persistent objects. The
// version bump moves every hash even where a script does not exercise those mechanics. Previous
// values are recorded per-vector below.
// ---------------------------------------------------------------------------

#[test]
fn golden_all_passes_terminates_identically() {
    // The longest-running vector: nothing but passes, so it exercises the hard turn limit
    // and the forced draw. This is the one that catches a change to match termination.
    let script = vec![Step::Pass; 64];
    let actual = run(duel(1, "leslie", "erus", 300, 5), &script);
    // Was "b75ec70f007a7a7b" at SIMULATION_VERSION 5,
    // "876de8693b5b75a8" at SIMULATION_VERSION 4, and
    // "e828d490e955f3d7" at SIMULATION_VERSION 3.
    // REGENERATED 2026-08-31 at SIMULATION_VERSION 7.
    // Was "ecff79397aa402de" at SIMULATION_VERSION 6.
    // REGENERATED 2026-08-31 at CONTENT_VERSION 3. Every vector moved because
    // `content_version` is part of the hashed state (`hash.rs`), which is what stops a
    // new content table from silently replaying against an old one.
    // Previous value was "5fe56c374f884cf6".
    // REGENERATED 2026-08-31 at CONTENT_VERSION 4: the Ramshot Cannon's knockback drops
    // from eight cells to two, so a direct hit no longer clears a four-cell perch and the
    // damage race decides the match. `content_version` is hashed, so every vector moves.
    // Previous value was "f66401193708e515".
    // REGENERATED 2026-09-01 at SIMULATION_VERSION 8 / CONTENT_VERSION 5: loadout.trinket,
    // trinket_charge, and the eight-per-slot catalog. Previous value was "fc8cf0f4ba111b74".
    // REGENERATED 2026-09-01 at CONTENT_VERSION 6: Melee-style stages, crow 280 HP, smaller
    // craters. Previous value was "894d5b42f7ec2cc6".
    // REGENERATED 2026-09-03 at SIMULATION_VERSION 9: BODY_WIDTH is now the player
    // collider diameter, and projectile origins/collision use the visible body's centre.
    // Previous value was "1d9f133f0916b2fb".
    // REGENERATED 2026-09-04 at SIMULATION_VERSION 10 / CONTENT_VERSION 7 for the
    // four-character fixed-kit migration. Previous value was "3c375eea936aa24c".
    // 2026-09-04: 370c9b00a275a5b2 -> 0909d60a903b242a when turn movement
    // became authoritative per selected launch character instead of the legacy shared class.
    assert_vector("all_passes", &actual, "0909d60a903b242a");
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
    let actual = run(duel(7, "erus", "kreena", 300, 5), &script);
    // Was "0038e5ddfabfec81" at SIMULATION_VERSION 5,
    // "b28768a38619df88" at SIMULATION_VERSION 4, and
    // "35636b623102bbed" at SIMULATION_VERSION 3.
    // REGENERATED 2026-08-31 at SIMULATION_VERSION 7.
    // Was "af6978b06c1f9772" at SIMULATION_VERSION 6.
    // REGENERATED 2026-08-31 at CONTENT_VERSION 3. Every vector moved because
    // `content_version` is part of the hashed state (`hash.rs`), which is what stops a
    // new content table from silently replaying against an old one.
    // Previous value was "0b914d175ade7d6e".
    // REGENERATED 2026-08-31 at CONTENT_VERSION 4: the Ramshot Cannon's knockback drops
    // from eight cells to two, so a direct hit no longer clears a four-cell perch and the
    // damage race decides the match. `content_version` is hashed, so every vector moves.
    // Previous value was "ddb193455a403319".
    // REGENERATED 2026-09-01 at SIMULATION_VERSION 8 / CONTENT_VERSION 5: loadout.trinket,
    // trinket_charge, and the eight-per-slot catalog. Previous value was "8831e091d2b5e054".
    // REGENERATED 2026-09-01 at CONTENT_VERSION 6: Melee-style stages, crow 280 HP, smaller
    // craters. Previous value was "c3727217929d598e".
    // REGENERATED 2026-09-03 at SIMULATION_VERSION 9 for the authoritative visible-body
    // collider correction. Previous value was "f687dc12c3e0e33f".
    // REGENERATED 2026-09-04 for fixed Erus/Kreena kits and schema 2. Previous value was
    // "50e91a7905ebcdf4".
    assert_vector("walking_duel", &actual, "7a002a57d45da611");
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
    let (actual, outcome) = run_detailed(duel(99, "crow", "erus", 300, 1), &script);
    assert!(
        outcome.total_health_lost > 0,
        "a firing vector must actually deal damage, or it covers nothing: {outcome:?}",
    );
    // Was "9c53418575ea824d" at SIMULATION_VERSION 5,
    // "2fbdca99f94c944c" at SIMULATION_VERSION 4, and
    // "7b49a0275beafc1f" at SIMULATION_VERSION 3.
    // REGENERATED 2026-08-31 at SIMULATION_VERSION 7.
    // Was "a009c290a796d1ba" at SIMULATION_VERSION 6.
    // REGENERATED 2026-08-31 at CONTENT_VERSION 3. Every vector moved because
    // `content_version` is part of the hashed state (`hash.rs`), which is what stops a
    // new content table from silently replaying against an old one. The Ramshot Cannon's
    // knockback also stopped shoving every opponent on the map: its
    // `magnitude_secondary` was 0, which `displacement.rs` reads as "no radius test"
    // rather than its documented "primary target only", so this vector's shots now
    // only shove what they land near.
    // Previous value was "7b07ba4dfa57d5f6".
    // REGENERATED 2026-08-31 at CONTENT_VERSION 4: the Ramshot Cannon's knockback drops
    // from eight cells to two, so a direct hit no longer clears a four-cell perch and the
    // damage race decides the match. `content_version` is hashed, so every vector moves.
    // Previous value was "9ea57823cddeb8ae".
    // REGENERATED 2026-09-01 at SIMULATION_VERSION 8 / CONTENT_VERSION 5: loadout.trinket,
    // trinket_charge, and the eight-per-slot catalog. Previous value was "13df21bcfee32f12".
    // REGENERATED 2026-09-01 at CONTENT_VERSION 6: Melee-style stages, crow 280 HP, smaller
    // craters. Previous value was "6f5278d82e4f0cff".
    // REGENERATED 2026-09-03 at SIMULATION_VERSION 9 for the authoritative visible-body
    // collider correction. Previous value was "ee07dc04a8821e68".
    // REGENERATED 2026-09-04 for Crow/Erus fixed actions with no ammunition. Previous value
    // was "fe4243a898724148".
    assert_vector("firing_duel", &actual, "3e88c0765412f549");
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
    let (actual, outcome) = run_detailed(duel(4_242, "leslie", "kreena", 300, 1), &script);
    assert!(
        outcome.turns_elapsed >= 1,
        "a mixed script must apply: {outcome:?}",
    );
    // REGENERATED 2026-08-31 at SIMULATION_VERSION 7 (crow + item ammo).
    // Was "c29e2d75ceba7f33" at SIMULATION_VERSION 6.
    // REGENERATED 2026-08-31 at CONTENT_VERSION 3. Every vector moved because
    // `content_version` is part of the hashed state (`hash.rs`), which is what stops a
    // new content table from silently replaying against an old one. The Ramshot Cannon's
    // knockback also stopped shoving every opponent on the map: its
    // `magnitude_secondary` was 0, which `displacement.rs` reads as "no radius test"
    // rather than its documented "primary target only", so this vector's shots now
    // only shove what they land near.
    // Previous value was "6faaa414ab6cff84".
    // REGENERATED 2026-08-31 at CONTENT_VERSION 4: the Ramshot Cannon's knockback drops
    // from eight cells to two, so a direct hit no longer clears a four-cell perch and the
    // damage race decides the match. `content_version` is hashed, so every vector moves.
    // Previous value was "c4911a24c765b3d0".
    // REGENERATED 2026-09-01 at SIMULATION_VERSION 8 / CONTENT_VERSION 5: loadout.trinket,
    // trinket_charge, and the eight-per-slot catalog. Previous value was "0aec335ec568cf3e".
    // REGENERATED 2026-09-01 at CONTENT_VERSION 6: Melee-style stages, crow 280 HP, smaller
    // craters. Previous value was "f9b14740c92e84a7".
    // REGENERATED 2026-09-03 at SIMULATION_VERSION 9 for the authoritative visible-body
    // collider correction. Previous value was "701f6a98d4adda2d".
    // REGENERATED 2026-09-04 for Leslie/Kreena fixed actions and schema 2. Previous value was
    // "201d41976cf6dd4e".
    // 2026-09-04: 4b276d55b2318613 -> f621281066a412d3 for character-specific
    // movement refresh and the resulting launch-power budget.
    assert_vector("mixed_actions", &actual, "f621281066a412d3");
}

#[test]
fn golden_low_health_duel_reaches_a_decision() {
    // Low health so the match actually resolves rather than timing out, exercising the
    // elimination and victory path end to end.
    let script = vec![
        Step::Fire {
            slot: AbilitySlot::Basic,
            angle_millidegrees: 0,
            power_basis_points: 2_500,
        };
        24
    ];
    let (actual, outcome) = run_detailed(duel(11, "crow", "crow", 60, 1), &script);
    assert!(
        outcome.total_health_lost > 0,
        "the decision vector must actually damage somebody: {outcome:?}",
    );
    // Was "323672057a1d53af" at SIMULATION_VERSION 5,
    // "06db50b907568060" at SIMULATION_VERSION 4, and
    // "b88af74446995c79" at SIMULATION_VERSION 3.
    // REGENERATED 2026-08-31 at SIMULATION_VERSION 7.
    // Was "0c908bfce4b927d6" at SIMULATION_VERSION 6.
    // REGENERATED 2026-08-31 at CONTENT_VERSION 3. Every vector moved because
    // `content_version` is part of the hashed state (`hash.rs`), which is what stops a
    // new content table from silently replaying against an old one.
    // Previous value was "dc6477a177b3e1f9".
    // REGENERATED 2026-08-31 at CONTENT_VERSION 4: the Ramshot Cannon's knockback drops
    // from eight cells to two, so a direct hit no longer clears a four-cell perch and the
    // damage race decides the match. `content_version` is hashed, so every vector moves.
    // Previous value was "f1f76f9d8e9c1252".
    // REGENERATED 2026-09-01 at SIMULATION_VERSION 8 / CONTENT_VERSION 5: loadout.trinket,
    // trinket_charge, and the eight-per-slot catalog. Previous value was "3e201b123e110a0b".
    // REGENERATED 2026-09-01 at CONTENT_VERSION 6: Melee-style stages, crow 280 HP, smaller
    // craters. Previous value was "127a3ee345fbe462".
    // REGENERATED 2026-09-03 at SIMULATION_VERSION 9 for the authoritative visible-body
    // collider correction. Previous value was "6a673f45206d2d38".
    // REGENERATED 2026-09-04 for Crow's straight precision shot and unlimited normal actions.
    // Previous value was "3e1c744e7a62a0be".
    // 2026-09-04: d9a65942e63d3726 -> 53a733d88e1b3be6 for character-specific
    // movement refresh and the resulting launch-power budget.
    assert_vector("low_health_duel", &actual, "53a733d88e1b3be6");
}

#[test]
fn a_vector_is_stable_across_repeated_runs_in_one_process() {
    // Guards against hidden per-run state: a vector that only holds on a cold process would
    // pass CI and fail a server that plays two matches.
    let script = vec![Step::Walk(POSITION_SCALE), Step::Pass, Step::Pass];
    let first = run(duel(5, "leslie", "erus", 300, 2), &script);
    let second = run(duel(5, "leslie", "erus", 300, 2), &script);
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
    let a = run(duel(1, "crow", "crow", 300, 1), &script);
    let b = run(duel(2, "crow", "crow", 300, 1), &script);
    assert_ne!(a, b, "the seed must affect the match");
}
