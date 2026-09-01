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
use crate::types::{
    MatchOutcome, MatchPhase, PersistentObject, PersistentObjectChange,
    PersistentObjectRemovalCause, PersistentObjectTransition, SimulationState,
};

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
/// Every owned-object removal is appended to `object_changes` at this producer, in the
/// objects' existing sequence order, with the complete last authoritative object snapshot.
/// A consumer must never reconstruct these removals from final state.
///
/// Idempotent. Eliminating an already-dead player changes nothing and is not an error — a
/// simultaneous-elimination effect (backlash plus a lethal counter-hit) may call this twice
/// for the same player in one resolution, and the second call must be a harmless no-op that
/// emits no duplicate lifecycle records rather than a double-cleanup or an error the caller
/// has to special-case.
///
/// The player entry itself is never removed from `state.players` (see module docs); only
/// `health` changes.
///
/// # Errors
///
/// Returns [`crate::error::SimError::UnknownDefinition`] when `player_id` does not name a
/// player in `state.players` — eliminating a player who does not exist is a caller bug, not
/// a normal game event, so it is reported rather than silently ignored.
pub fn eliminate(
    state: &mut SimulationState,
    player_id: &str,
    object_changes: &mut Vec<PersistentObjectChange>,
) -> SimResult<()> {
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
    remove_owned_objects(&mut state.objects, player_id, object_changes);

    Ok(())
}

/// Removes every object owned by `player_id` from `objects`, in place, and appends one
/// producer-owned lifecycle record per removal.
///
/// Split out from [`eliminate`] so the ordering guarantee — sequence order preserved,
/// because both collection and `retain` walk the already-sorted vector in order — is
/// documented and tested on its own. Existing records in `object_changes` are preserved;
/// this producer only appends what it did.
fn remove_owned_objects(
    objects: &mut Vec<PersistentObject>,
    player_id: &str,
    object_changes: &mut Vec<PersistentObjectChange>,
) {
    let removed: Vec<PersistentObject> = objects
        .iter()
        .filter(|object| object.owner_id == player_id)
        .cloned()
        .collect();
    objects.retain(|object| object.owner_id != player_id);
    object_changes.extend(removed.into_iter().map(|object| PersistentObjectChange {
        object,
        transition: PersistentObjectTransition::Removed {
            cause: PersistentObjectRemovalCause::OwnerEliminated,
        },
    }));
}

/// Canonicalizes cleanup for every health-zero owner, then evaluates the match and, on a
/// terminal outcome, sets `state.phase` to [`MatchPhase::MatchComplete`].
///
/// The scheduler calls this at [`MatchPhase::VictoryCheck`]. Cleanup records are appended in
/// player and object sequence order and are idempotent. On [`MatchOutcome::InProgress`] the
/// phase is left untouched — advancing past `VictoryCheck` into the next turn is the scheduler's
/// job, not this function's.
///
/// # Errors
///
/// Never fails on its own; the `SimResult` return matches this crate's convention that
/// state-mutating entry points report failure through `SimResult` rather than by panicking,
/// so a future fallible precondition can be added here without changing the signature.
pub fn check_and_finalize(
    state: &mut SimulationState,
    object_changes: &mut Vec<PersistentObjectChange>,
) -> SimResult<MatchOutcome> {
    // Damage and falling set health to zero at their authoritative producers. Canonicalize
    // the ownership side effect here, before evaluating victory, so ordinary elimination
    // and explicit elimination cannot diverge about whether dead-owned objects survive.
    let eliminated_ids: Vec<String> = state
        .players
        .iter()
        .filter(|player| player.is_eliminated())
        .map(|player| player.id.clone())
        .collect();
    for player_id in eliminated_ids {
        remove_owned_objects(&mut state.objects, &player_id, object_changes);
    }

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
            loadout: crate::types::Loadout::launch_default(),
            ammo: crate::types::DEFAULT_AMMO,
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
        let mut object_changes = Vec::new();
        assert!(eliminate(&mut state, "a1", &mut object_changes).is_ok());
        assert!(eliminate(&mut state, "a2", &mut object_changes).is_ok());
        assert!(object_changes.is_empty());

        assert_eq!(evaluate(&state), MatchOutcome::Victory { team: 1 });
    }

    #[test]
    fn wiping_every_team_yields_draw_not_in_progress() {
        let mut state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        let mut object_changes = Vec::new();
        assert!(eliminate(&mut state, "a", &mut object_changes).is_ok());
        assert!(eliminate(&mut state, "b", &mut object_changes).is_ok());
        assert!(object_changes.is_empty());

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
        let mut object_changes = Vec::new();
        assert!(eliminate(&mut state, "c", &mut object_changes).is_ok());
        assert!(object_changes.is_empty());
        assert_eq!(evaluate(&state), MatchOutcome::InProgress);
        assert_eq!(living_teams(&state), vec![0, 1]);
    }

    // -----------------------------------------------------------------------------------
    // eliminate
    // -----------------------------------------------------------------------------------

    #[test]
    fn eliminate_zeroes_health_but_keeps_the_player_in_state() {
        let mut state = base_state(vec![player("a", 0, 200)]);
        let mut object_changes = Vec::new();
        assert!(eliminate(&mut state, "a", &mut object_changes).is_ok());
        assert!(object_changes.is_empty());

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
        let owned = object(0, "a", PersistentObjectKind::Turret);
        state.objects.push(owned.clone());
        let mut object_changes = Vec::new();

        assert!(eliminate(&mut state, "a", &mut object_changes).is_ok());
        let after_first = object_changes.clone();
        assert!(eliminate(&mut state, "a", &mut object_changes).is_ok());

        let Some(found) = state.player("a") else {
            panic!("player must still be present")
        };
        assert_eq!(found.health, 0);
        assert_eq!(state.players.len(), 1);
        assert!(state.objects.is_empty());
        assert_eq!(
            after_first,
            vec![PersistentObjectChange {
                object: owned,
                transition: PersistentObjectTransition::Removed {
                    cause: PersistentObjectRemovalCause::OwnerEliminated,
                },
            }]
        );
        assert_eq!(
            object_changes, after_first,
            "the second elimination must not emit a duplicate removal"
        );
    }

    #[test]
    fn eliminate_unknown_player_is_an_error() {
        let mut state = base_state(vec![player("a", 0, 200)]);
        let mut object_changes = Vec::new();
        assert!(eliminate(&mut state, "nobody", &mut object_changes).is_err());
        assert!(object_changes.is_empty());
    }

    #[test]
    fn eliminate_removes_the_dead_players_turret() {
        let mut state = base_state(vec![player("emi", 0, 200), player("foe", 1, 200)]);
        let owned = object(0, "emi", PersistentObjectKind::Turret);
        let other_owner = object(1, "foe", PersistentObjectKind::Turret);
        state.objects.extend([owned.clone(), other_owner.clone()]);
        let mut object_changes = Vec::new();

        assert!(eliminate(&mut state, "emi", &mut object_changes).is_ok());

        assert_eq!(
            state.objects,
            vec![other_owner],
            "another player's exact object must be untouched"
        );
        assert_eq!(
            object_changes,
            vec![PersistentObjectChange {
                object: owned,
                transition: PersistentObjectTransition::Removed {
                    cause: PersistentObjectRemovalCause::OwnerEliminated,
                },
            }],
            "the producer must retain the complete removed-object snapshot"
        );
    }

    #[test]
    fn eliminate_removes_multiple_owned_objects_preserving_sequence_order() {
        let mut state = base_state(vec![player("aleph", 0, 200)]);
        let first_owned = object(0, "aleph", PersistentObjectKind::EmbeddedKnife);
        let other_owner = object(1, "other", PersistentObjectKind::Turret);
        let second_owned = object(2, "aleph", PersistentObjectKind::EmbeddedKnife);
        state.objects.extend([
            first_owned.clone(),
            other_owner.clone(),
            second_owned.clone(),
        ]);
        let mut object_changes = Vec::new();

        assert!(eliminate(&mut state, "aleph", &mut object_changes).is_ok());

        assert_eq!(
            state.objects,
            vec![other_owner],
            "only the surviving owner's exact object remains"
        );
        assert_eq!(
            object_changes,
            vec![
                PersistentObjectChange {
                    object: first_owned,
                    transition: PersistentObjectTransition::Removed {
                        cause: PersistentObjectRemovalCause::OwnerEliminated,
                    },
                },
                PersistentObjectChange {
                    object: second_owned,
                    transition: PersistentObjectTransition::Removed {
                        cause: PersistentObjectRemovalCause::OwnerEliminated,
                    },
                },
            ],
            "removal snapshots must retain authoritative sequence order"
        );
    }

    // -----------------------------------------------------------------------------------
    // check_and_finalize
    // -----------------------------------------------------------------------------------

    #[test]
    fn check_and_finalize_sets_match_complete_on_victory() {
        let mut state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        let mut object_changes = Vec::new();
        assert!(eliminate(&mut state, "b", &mut object_changes).is_ok());
        assert!(object_changes.is_empty());
        state.phase = MatchPhase::VictoryCheck;

        let outcome = check_and_finalize(&mut state, &mut Vec::new());
        assert_eq!(outcome, Ok(MatchOutcome::Victory { team: 0 }));
        assert_eq!(state.phase, MatchPhase::MatchComplete);
    }

    #[test]
    fn check_and_finalize_cleans_objects_for_an_owner_already_reduced_to_zero() {
        let mut state = base_state(vec![player("dead", 0, 0), player("living", 1, 200)]);
        let owned = object(0, "dead", PersistentObjectKind::EmbeddedKnife);
        let survivor = object(1, "living", PersistentObjectKind::Turret);
        state.objects.extend([owned.clone(), survivor.clone()]);
        let mut object_changes = Vec::new();

        let outcome = check_and_finalize(&mut state, &mut object_changes);

        assert_eq!(outcome, Ok(MatchOutcome::Victory { team: 1 }));
        assert_eq!(state.objects, vec![survivor]);
        assert_eq!(
            object_changes,
            vec![PersistentObjectChange {
                object: owned,
                transition: PersistentObjectTransition::Removed {
                    cause: PersistentObjectRemovalCause::OwnerEliminated,
                },
            }],
            "ordinary damage/fall elimination must use the canonical cleanup producer",
        );

        let recorded_once = object_changes.clone();
        assert_eq!(
            check_and_finalize(&mut state, &mut object_changes),
            Ok(MatchOutcome::Victory { team: 1 }),
        );
        assert_eq!(
            object_changes, recorded_once,
            "rechecking terminal state must not duplicate lifecycle records",
        );
    }

    #[test]
    fn check_and_finalize_sets_match_complete_on_draw() {
        let mut state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        let mut object_changes = Vec::new();
        assert!(eliminate(&mut state, "a", &mut object_changes).is_ok());
        assert!(eliminate(&mut state, "b", &mut object_changes).is_ok());
        assert!(object_changes.is_empty());
        state.phase = MatchPhase::VictoryCheck;

        let outcome = check_and_finalize(&mut state, &mut Vec::new());
        assert_eq!(outcome, Ok(MatchOutcome::Draw));
        assert_eq!(state.phase, MatchPhase::MatchComplete);
    }

    #[test]
    fn check_and_finalize_leaves_phase_alone_while_in_progress() {
        let mut state = base_state(vec![player("a", 0, 200), player("b", 1, 200)]);
        state.phase = MatchPhase::VictoryCheck;

        let outcome = check_and_finalize(&mut state, &mut Vec::new());
        assert_eq!(outcome, Ok(MatchOutcome::InProgress));
        assert_eq!(
            state.phase,
            MatchPhase::VictoryCheck,
            "advancing past VictoryCheck when the match continues is the scheduler's job"
        );
    }
}
