//! Victory and elimination — the last thing standing between the engine and a
//! completable match.
//!
//! `PRODUCT_SPEC.md` §2 states the rule plainly: "the standard victory condition is last
//! player or team standing" and "a character is eliminated at zero health". This module is
//! the single place that turns "every player on team N is dead" into a terminal
//! [`MatchOutcome`], and the single place that turns "this player's health hit zero" into
//! elimination bookkeeping (health, ownership cleanup) that the rest of the engine can
//! trust.
//!
//! # Draw is not an edge case
//!
//! Blood Maul's backlash and Roberto's own ultimate can kill the acting player at the same
//! instant as the target (`CHARACTERS.md`). When that leaves zero teams with a living
//! member, [`evaluate`] returns [`MatchOutcome::Draw`] rather than staying
//! [`MatchOutcome::InProgress`] — a match must always reach a terminal state
//! (`PRODUCT_SPEC.md` §2), and looping forever because "the winner" was never decided would
//! violate that.
//!
//! # Elimination never removes a player from state
//!
//! [`eliminate`] sets health to zero and leaves the [`PlayerState`] entry in
//! `state.players`. Removing it would renumber nothing (players are keyed by `id`, not
//! index) but would silently change iteration order and length — and the result panel,
//! replay, and the state hash all need the eliminated player's final stats to still be
//! there.

use crate::error::SimResult;
use crate::types::{MatchOutcome, MatchPhase, PersistentObject, SimulationState};

/// Evaluates the current match state and reports whether it has ended.
///
/// - Exactly one team with a living member: [`MatchOutcome::Victory`] for that team.
/// - Two or more teams with a living member: [`MatchOutcome::InProgress`].
/// - Zero teams with a living member (including zero players at all): [`MatchOutcome::Draw`].
///
/// Reads `state.players` only; callers wanting "and finalize the phase" should use
/// [`check_and_finalize`] instead.
#[must_use]
pub fn evaluate(state: &SimulationState) -> MatchOutcome {
    let teams = living_teams(state);
    match teams.as_slice() {
        [] => MatchOutcome::Draw,
        // A single surviving team, whatever its index, is the winner. Slice patterns read
        // the sole element without indexing into the `Vec`.
        [team] => MatchOutcome::Victory { team: *team },
        _ => MatchOutcome::InProgress,
    }
}

/// The distinct team indices that still have at least one living member.
///
/// Sorted and de-duplicated: `living_teams` is walked by [`evaluate`] to decide a match
/// outcome, and a stable order keeps that decision independent of `state.players`'
/// iteration order (already sorted by `id` per `types.rs`, but this does not rely on that).
#[must_use]
pub fn living_teams(state: &SimulationState) -> Vec<u8> {
    // BTreeSet rather than a HashSet: this crate never lets hashed-collection iteration
    // order feed ordered output (`lib.rs` invariant), and a BTreeSet gives the ascending
    // order this function promises for free.
    let mut teams = std::collections::BTreeSet::new();
    for player in &state.players {
        if !player.is_eliminated() {
            teams.insert(player.team);
        }
    }
    teams.into_iter().collect()
}

/// Eliminates `player_id`: sets health to zero and removes every persistent object they
/// own.
///
/// Idempotent. Eliminating an already-dead player changes nothing and is not an error — a
/// simultaneous-elimination effect (backlash plus a lethal counter-hit) may call this twice
/// for the same player in one resolution, and the second call must be a harmless no-op
/// rather than a double-cleanup or an error the caller has to special-case.
///
/// The player entry itself is never removed from `state.players` (see module docs); only
/// `health` changes.
///
/// # Errors
///
/// Returns [`crate::error::SimError::UnknownDefinition`] when `player_id` does not name a
/// player in `state.players` — eliminating a player who does not exist is a caller bug, not
/// a normal game event, so it is reported rather than silently ignored.
pub fn eliminate(state: &mut SimulationState, player_id: &str) -> SimResult<()> {
    let Some(player) = state.player_mut(player_id) else {
        return Err(crate::error::SimError::UnknownDefinition);
    };

    // Idempotent by construction: setting an already-zero health to zero is a no-op, and
    // the ownership cleanup below is driven by a `retain` that is equally harmless to run
    // twice — the second call finds nothing left to remove.
    player.health = 0;

    // A dead player's persistent objects (Emi's turret, Aleph's embedded knives) must stop
    // acting. Iterate in `sequence` order — `state.objects` is documented as kept sorted by
    // it — and `retain` preserves that order in the survivors, so this never needs to
    // re-sort afterward.
    remove_owned_objects(&mut state.objects, player_id);

    Ok(())
}

/// Removes every object owned by `player_id` from `objects`, in place.
///
/// Split out from [`eliminate`] so the ordering guarantee — sequence order preserved,
/// because `retain` never reorders survivors — is documented and tested on its own.
fn remove_owned_objects(objects: &mut Vec<PersistentObject>, player_id: &str) {
    objects.retain(|object| object.owner_id != player_id);
}

/// Evaluates the match and, on a terminal outcome, sets `state.phase` to
/// [`MatchPhase::MatchComplete`].
///
/// The scheduler calls this at [`MatchPhase::VictoryCheck`]. On [`MatchOutcome::InProgress`]
/// the phase is left untouched — advancing past `VictoryCheck` into the next turn is the
/// scheduler's job, not this function's.
///
/// # Errors
///
/// Never fails on its own; the `SimResult` return matches this crate's convention that
/// state-mutating entry points report failure through `SimResult` rather than by panicking,
/// so a future fallible precondition can be added here without changing the signature.
pub fn check_and_finalize(state: &mut SimulationState) -> SimResult<MatchOutcome> {
    let outcome = evaluate(state);
    if !matches!(outcome, MatchOutcome::InProgress) {
        state.phase = MatchPhase::MatchComplete;
    }
    Ok(outcome)
}

#[cfg(test)]
// `let ... else { panic!() }` states a fixture invariant far more clearly than threading a
// `Result` through every test case. Matches the precedent set by `terrain::tests` and
// `resolve::objects::tests`; production paths in this file remain panic-free.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::fixed::FixedPoint;
    use crate::types::TurnEndReason;
    use crate::types::{Appearance, Material, PersistentObjectKind, PlayerState, TerrainMask};

    // -----------------------------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------------------------

    fn player(id: &str, team: u8, health: u16) -> PlayerState {
        PlayerState {
            id: id.to_string(),
            team,
            health,
            max_health: 300,
            position: FixedPoint::ZERO,
            character_id: "test-character".to_string(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn object(sequence: u32, owner_id: &str, kind: PersistentObjectKind) -> PersistentObject {
        PersistentObject {
            sequence,
            owner_id: owner_id.to_string(),
            kind,
            position: FixedPoint::ZERO,
            health: 80,
            turns_remaining: 5,
        }
    }

    fn base_state(players: Vec<PlayerState>) -> SimulationState {
        SimulationState {
            pending_turn_end_reason: TurnEndReason::Passed,
            last_turn_end_reason: TurnEndReason::Passed,
            simulation_version: 1,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::VictoryCheck,
            active_player_id: String::new(),
            wind_per_tick: 0,
            movement_remaining: 0,
            has_attacked_this_turn: false,
            terrain: TerrainMask {
                width: 4,
                height: 4,
                cells: vec![Material::Empty as u8; 16],
            },
            blocks: Vec::new(),
            players,
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 1,
        }
    }

    // -----------------------------------------------------------------------------------
    // evaluate / living_teams
    // -----------------------------------------------------------------------------------

    #[test]
    fn two_teams_both_alive_is_in_progress() {
        let state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        assert_eq!(evaluate(&state), MatchOutcome::InProgress);
        assert_eq!(living_teams(&state), vec![0, 1]);
    }

    #[test]
    fn wiping_one_team_yields_victory_for_the_right_team() {
        let mut state = base_state(vec![
            player("a1", 0, 200),
            player("a2", 0, 150),
            player("b1", 1, 200),
        ]);
        assert!(eliminate(&mut state, "a1").is_ok());
        assert!(eliminate(&mut state, "a2").is_ok());

        assert_eq!(evaluate(&state), MatchOutcome::Victory { team: 1 });
    }

    #[test]
    fn wiping_every_team_yields_draw_not_in_progress() {
        let mut state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        assert!(eliminate(&mut state, "a").is_ok());
        assert!(eliminate(&mut state, "b").is_ok());

        assert_eq!(
            evaluate(&state),
            MatchOutcome::Draw,
            "simultaneous elimination must resolve to Draw, not loop as InProgress"
        );
    }

    #[test]
    fn empty_player_list_is_a_draw() {
        let state = base_state(vec![]);
        assert_eq!(evaluate(&state), MatchOutcome::Draw);
        assert!(living_teams(&state).is_empty());
    }

    #[test]
    fn three_teams_two_alive_is_still_in_progress() {
        let mut state = base_state(vec![
            player("a", 0, 200),
            player("b", 1, 200),
            player("c", 2, 200),
        ]);
        assert!(eliminate(&mut state, "c").is_ok());
        assert_eq!(evaluate(&state), MatchOutcome::InProgress);
        assert_eq!(living_teams(&state), vec![0, 1]);
    }

    // -----------------------------------------------------------------------------------
    // eliminate
    // -----------------------------------------------------------------------------------

    #[test]
    fn eliminate_zeroes_health_but_keeps_the_player_in_state() {
        let mut state = base_state(vec![player("a", 0, 200)]);
        assert!(eliminate(&mut state, "a").is_ok());

        let Some(found) = state.player("a") else {
            panic!("eliminated player must remain in state.players")
        };
        assert_eq!(found.health, 0);
        assert_eq!(
            state.players.len(),
            1,
            "elimination must not remove the player entry"
        );
    }

    #[test]
    fn eliminate_twice_is_a_no_op_the_second_time() {
        let mut state = base_state(vec![player("a", 0, 200)]);
        assert!(eliminate(&mut state, "a").is_ok());
        assert!(eliminate(&mut state, "a").is_ok());

        let Some(found) = state.player("a") else {
            panic!("player must still be present")
        };
        assert_eq!(found.health, 0);
        assert_eq!(state.players.len(), 1);
    }

    #[test]
    fn eliminate_unknown_player_is_an_error() {
        let mut state = base_state(vec![player("a", 0, 200)]);
        assert!(eliminate(&mut state, "nobody").is_err());
    }

    #[test]
    fn eliminate_removes_the_dead_players_turret() {
        let mut state = base_state(vec![player("emi", 0, 200), player("foe", 1, 200)]);
        state
            .objects
            .push(object(0, "emi", PersistentObjectKind::Turret));
        state
            .objects
            .push(object(1, "foe", PersistentObjectKind::Turret));

        assert!(eliminate(&mut state, "emi").is_ok());

        assert!(
            !state.objects.iter().any(|object| object.owner_id == "emi"),
            "a dead player's turret must not remain to keep shooting"
        );
        assert!(
            state.objects.iter().any(|object| object.owner_id == "foe"),
            "another player's objects are untouched"
        );
    }

    #[test]
    fn eliminate_removes_multiple_owned_objects_preserving_sequence_order() {
        let mut state = base_state(vec![player("aleph", 0, 200)]);
        state
            .objects
            .push(object(0, "aleph", PersistentObjectKind::EmbeddedKnife));
        state
            .objects
            .push(object(1, "other", PersistentObjectKind::Turret));
        state
            .objects
            .push(object(2, "aleph", PersistentObjectKind::EmbeddedKnife));

        assert!(eliminate(&mut state, "aleph").is_ok());

        let remaining: Vec<u32> = state.objects.iter().map(|object| object.sequence).collect();
        assert_eq!(
            remaining,
            vec![1],
            "only the surviving owner's object remains, sequence order intact"
        );
    }

    // -----------------------------------------------------------------------------------
    // check_and_finalize
    // -----------------------------------------------------------------------------------

    #[test]
    fn check_and_finalize_sets_match_complete_on_victory() {
        let mut state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        assert!(eliminate(&mut state, "b").is_ok());
        state.phase = MatchPhase::VictoryCheck;

        let outcome = check_and_finalize(&mut state);
        assert_eq!(outcome, Ok(MatchOutcome::Victory { team: 0 }));
        assert_eq!(state.phase, MatchPhase::MatchComplete);
    }

    #[test]
    fn check_and_finalize_sets_match_complete_on_draw() {
        let mut state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        assert!(eliminate(&mut state, "a").is_ok());
        assert!(eliminate(&mut state, "b").is_ok());
        state.phase = MatchPhase::VictoryCheck;

        let outcome = check_and_finalize(&mut state);
        assert_eq!(outcome, Ok(MatchOutcome::Draw));
        assert_eq!(state.phase, MatchPhase::MatchComplete);
    }

    #[test]
    fn check_and_finalize_leaves_phase_alone_while_in_progress() {
        let mut state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        state.phase = MatchPhase::VictoryCheck;

        let outcome = check_and_finalize(&mut state);
        assert_eq!(outcome, Ok(MatchOutcome::InProgress));
        assert_eq!(
            state.phase,
            MatchPhase::VictoryCheck,
            "advancing past VictoryCheck when the match continues is the scheduler's job"
        );
    }
}
