//! Turn scheduler — advances [`MatchPhase`], turn order, and per-turn state.
//!
//! `MatchPhase` existed as a hashed enum with nothing that ever moved it (`todolist.md`
//! P4): a match could be constructed, but nothing chose who acted first, nothing walked the
//! phase cycle `PRODUCT_SPEC.md` §2 defines, and nothing ever refreshed a player's movement
//! allowance or ticked a status. This module is that missing driver.
//!
//! # The phase cycle
//!
//! `PRODUCT_SPEC.md` §2:
//!
//! ```text
//! MatchIntro -> TurnStart -> Movement -> AimingAndSelection -> CommandLocked -> Resolution
//! -> Settling -> StatusResolution -> VictoryCheck -> (TurnStart | MatchComplete)
//! ```
//!
//! [`begin_match`] performs the one-time `MatchIntro -> TurnStart` step and opens the first
//! turn. [`advance_phase`] performs every subsequent step, one call per transition — a full
//! lap from `TurnStart` back to `TurnStart` is exactly eight calls. Two steps in that lap do
//! real work rather than just relabelling the phase:
//!
//! - Leaving `StatusResolution` ticks every status exactly once
//!   ([`resolve::status::tick_statuses`]) — never from anywhere else in this file, so a lap
//!   around the cycle can never tick a status twice and silently halve every duration.
//! - Leaving `VictoryCheck` calls [`victory::check_and_finalize`]. On a terminal outcome the
//!   phase is left at [`MatchPhase::MatchComplete`] (set by that function) and the lap stops
//!   there for good; otherwise this module rotates to the next living player via
//!   [`end_turn`] and returns to `TurnStart`.
//!
//! [`MatchPhase::PassiveSelection`] is not a step in that lap. It is a one-time interrupt
//! raised elsewhere (when a player's gauge fills for the first time) that this module did
//! not create and does not enter on its own; [`advance_phase`] only knows how to resume the
//! normal cycle once the pending choice lands — see its own doc comment for exactly where.
//!
//! # Sudden death and a bounded match
//!
//! `PRODUCT_SPEC.md` §2 triggers sudden death at turn 12 in a duel and requires a match to
//! always reach a terminal state. This file owns the trigger and the termination guarantee;
//! it does not own the hazard itself (`todolist.md` P4 explicitly scopes that out — no
//! hazard system exists yet). [`is_sudden_death_active`] is the hook a future hazard system
//! calls; [`HARD_TURN_LIMIT`] is the fallback that keeps "a match always terminates" true
//! even before that hazard exists, rather than a guarantee that only holds once every other
//! system has been built (`todolist.md`'s recurring lesson about code that is correct,
//! tested, and never actually reached).

use crate::character;
use crate::error::{SimError, SimResult};
use crate::resolve::status;
use crate::types::{
    MatchOutcome, MatchPhase, PersistentObjectChange, SimulationState, StatusChange, TurnEndReason,
};
use crate::victory;

/// The turn at which sudden death triggers in a duel (`PRODUCT_SPEC.md` §2: "Sudden-death
/// trigger: Turn 12 in a duel").
pub const SUDDEN_DEATH_TURN: u32 = 12;

/// Hard turn ceiling past which [`advance_phase`] forces a match still `InProgress` to end
/// in a draw rather than let the phase cycle run forever.
///
/// The *intended* bound on a stalled match is sudden death's own escalating hazard actually
/// killing someone, but that hazard is not this file's to build. Generous relative to
/// [`SUDDEN_DEATH_TURN`] so it only ever fires once escalation has plainly failed to end the
/// match on its own — this is a backstop, not the normal ending.
pub const HARD_TURN_LIMIT: u32 = 200;

/// Starts a match: selects the opening active player and enters the first turn.
///
/// Call exactly once, while `state.phase` is still [`MatchPhase::MatchIntro`]. The opener is
/// the lowest-sorted-`id` living player, matching [`next_active_player`]'s own rotation order
/// so turn order is a pure function of the roster rather than of submission order
/// (`PRODUCT_SPEC.md` §2: "fixed alternating or clockwise turns ship first").
///
/// On success, `state.phase` is [`MatchPhase::TurnStart`], `state.turn_number` is `1`,
/// `state.active_player_id` names the opener, `state.movement_remaining` is refreshed from
/// their character's [`crate::types::MovementClass`], and `state.has_attacked_this_turn` is
/// `false`.
///
/// # Errors
///
/// Returns [`SimError::OutOfRange`] if `state.phase` is not [`MatchPhase::MatchIntro`]
/// (calling this against a match already underway would silently rewind it rather than
/// refuse the out-of-order call) or if `state.players` has no living entry to open with.
/// Returns [`SimError::UnknownDefinition`] if the opener's `character_id` is not in the
/// roster.
pub fn begin_match(state: &mut SimulationState) -> SimResult<()> {
    if state.phase != MatchPhase::MatchIntro {
        return Err(SimError::OutOfRange { field: "phase" });
    }

    let Some(first_id) = lowest_living_id(state) else {
        return Err(SimError::OutOfRange { field: "players" });
    };
    let movement = movement_allowance_for(state, &first_id)?;

    state.active_player_id = first_id;
    state.movement_remaining = movement;
    state.has_attacked_this_turn = false;
    state.turn_number = 1;
    state.phase = MatchPhase::TurnStart;
    Ok(())
}

/// Advances `state.phase` by exactly one step and returns the phase reached.
///
/// See the module documentation for the full cycle and what happens on the two steps that
/// do real work (`StatusResolution` and `VictoryCheck`). [`MatchPhase::MatchComplete`] is
/// absorbing: once reached, further calls return it unchanged rather than erroring, since a
/// terminal match being polled again is normal, not a caller bug.
///
/// [`MatchPhase::PassiveSelection`] resumes into [`MatchPhase::Settling`] — the pending
/// choice always arises after an ability has already resolved (a gauge only fills as a
/// result of dealing or receiving damage), so the normal cycle's next real step is exactly
/// where it left off: settling, then status ticking, then the victory check. This is a
/// judgement call rather than a value read from `types.rs`, since nothing records which
/// phase `PassiveSelection` interrupted; it is documented here rather than silently assumed.
///
/// # Errors
///
/// Propagates any error from [`victory::check_and_finalize`] or [`end_turn`], both of which
/// this function calls while leaving [`MatchPhase::VictoryCheck`].
pub fn advance_phase(
    state: &mut SimulationState,
    status_changes: &mut Vec<StatusChange>,
    object_changes: &mut Vec<PersistentObjectChange>,
) -> SimResult<MatchPhase> {
    let next = match state.phase {
        MatchPhase::MatchIntro => MatchPhase::TurnStart,
        MatchPhase::TurnStart => MatchPhase::Movement,
        MatchPhase::Movement => MatchPhase::AimingAndSelection,
        MatchPhase::AimingAndSelection => MatchPhase::CommandLocked,
        MatchPhase::PassiveSelection => MatchPhase::Settling,
        MatchPhase::CommandLocked => MatchPhase::Resolution,
        MatchPhase::Resolution => MatchPhase::Settling,
        MatchPhase::Settling => MatchPhase::StatusResolution,
        MatchPhase::StatusResolution => {
            // The only call site in the crate (see module docs): exactly one lap of
            // `advance_phase` leaves `StatusResolution` exactly once, so the active
            // player's statuses tick by exactly one turn per lap — never zero, never two.
            //
            // Scoped to the active player because a status lasts for *their* turns. Ticking
            // everyone here would make a two-turn Chill expire after two commands submitted
            // by anyone, which in a duel is one of the victim's own turns.
            let active = state.active_player_id.clone();
            status_changes.extend(status::tick_statuses(state, &active));
            MatchPhase::VictoryCheck
        }
        MatchPhase::VictoryCheck => return leave_victory_check(state, object_changes),
        MatchPhase::MatchComplete => MatchPhase::MatchComplete,
    };
    state.phase = next;
    Ok(next)
}

/// The half of [`advance_phase`] that runs when leaving [`MatchPhase::VictoryCheck`].
///
/// Split out because it has two exit paths — terminal, or rotate and continue — each of
/// which already sets `state.phase` itself, unlike every other arm in [`advance_phase`]'s
/// match, which share one assignment after the match.
fn leave_victory_check(
    state: &mut SimulationState,
    object_changes: &mut Vec<PersistentObjectChange>,
) -> SimResult<MatchPhase> {
    let mut outcome = victory::check_and_finalize(state)?;

    // The real bound on a stalled match is sudden death's hazard actually eliminating
    // someone; absent that hazard (out of this file's scope), this is what keeps "a match
    // always terminates" true rather than aspirational. Forcing every survivor's health to
    // zero routes through the same `evaluate` logic that already turns "no team left" into
    // `MatchOutcome::Draw` for a simultaneous elimination, so there is no second notion of
    // "the match is over" for this path to get out of sync with.
    if matches!(outcome, MatchOutcome::InProgress) && state.turn_number >= HARD_TURN_LIMIT {
        force_draw(state, object_changes)?;
        outcome = victory::check_and_finalize(state)?;
    }

    if matches!(outcome, MatchOutcome::InProgress) {
        // `todolist.md` P11: this used to hardcode `Attacked`, which made a pass, a
        // timeout, and a committed attack indistinguishable downstream. The orchestrator
        // declares the real reason on the state before driving the cycle.
        end_turn(state, state.pending_turn_end_reason)?;
        // `end_turn` deliberately leaves `state.phase` alone (see its own doc comment) —
        // this is the one call site responsible for choosing what phase the new turn
        // starts in.
        state.phase = MatchPhase::TurnStart;
        Ok(state.phase)
    } else {
        // `check_and_finalize` already committed `MatchPhase::MatchComplete`.
        // There is no next player, so `end_turn` is intentionally not called; nevertheless
        // the terminal action did complete the final turn and its reason must not remain the
        // previous turn's value. Transition/replay consumers read this field to distinguish
        // a final attack, timeout, pass, or movement fall.
        state.last_turn_end_reason = state.pending_turn_end_reason;
        Ok(state.phase)
    }
}

/// Eliminates every living player, so the next [`victory::evaluate`] call reports
/// [`MatchOutcome::Draw`].
///
/// The only caller is [`leave_victory_check`]'s [`HARD_TURN_LIMIT`] fallback; see its doc
/// comment for why this exists.
fn force_draw(
    state: &mut SimulationState,
    object_changes: &mut Vec<PersistentObjectChange>,
) -> SimResult<()> {
    let living_ids: Vec<String> = state
        .players
        .iter()
        .filter(|player| !player.is_eliminated())
        .map(|player| player.id.clone())
        .collect();
    for id in living_ids {
        // Eliminating an owner removes their persistent objects; those removals are
        // recorded rather than left for a client to infer from a shrunken object list.
        victory::eliminate(state, &id, object_changes)?;
    }
    Ok(())
}

/// Ends the active player's turn and opens the next one.
///
/// Selects the next living player via [`next_active_player`], refreshes
/// `state.movement_remaining` from *their* character's movement class (not the player who
/// just finished), clears `state.has_attacked_this_turn`, and advances `state.turn_number`.
/// Does **not** touch `state.phase` — callers decide what phase the new turn starts in;
/// [`advance_phase`] is the only caller in this crate today, and it sets
/// [`MatchPhase::TurnStart`] itself immediately afterward.
///
/// Does **not** tick statuses — that happens exactly once per lap, on leaving
/// [`MatchPhase::StatusResolution`] (see module docs), never here, so a caller cannot
/// accidentally double-tick by calling both in the same turn.
///
/// # Errors
///
/// Returns [`SimError::OutOfRange`] if [`next_active_player`] finds nobody living to hand
/// the turn to — reachable only if a caller invokes this while the match should already be
/// over, since [`advance_phase`] never reaches this call when fewer than two teams remain.
/// Returns [`SimError::UnknownDefinition`] if the next player's `character_id` is not in the
/// roster.
pub fn end_turn(state: &mut SimulationState, reason: TurnEndReason) -> SimResult<()> {
    // Recorded for replay, telemetry, and presentation consumers (`todolist.md` P11).
    // Match exhaustively before assignment so a newly added variant fails this build instead
    // of being silently folded into "just another turn end"; every current variant otherwise
    // shares the bookkeeping below.
    match reason {
        TurnEndReason::Attacked
        | TurnEndReason::Passed
        | TurnEndReason::TimedOut
        | TurnEndReason::Eliminated => {}
    }
    state.last_turn_end_reason = reason;

    let Some(next_id) = next_active_player(state) else {
        return Err(SimError::OutOfRange {
            field: "active_player_id",
        });
    };
    let movement = movement_allowance_for(state, &next_id)?;

    state.active_player_id = next_id;
    state.movement_remaining = movement;
    state.has_attacked_this_turn = false;
    // Monotonic turn counter; saturating so an extreme replay caps rather than wraps back to
    // a small number and corrupts every subsequent `expected_turn_number` check.
    state.turn_number = state.turn_number.saturating_add(1);

    Ok(())
}

/// The living player who should act after `state.active_player_id`, in sorted-`id` order,
/// wrapping from the last id back to the first.
///
/// An eliminated player is never returned — a dead player is removed from the rotation
/// entirely, not merely skipped for one turn. Also handles `state.active_player_id` itself
/// no longer being alive (or being the empty placeholder before the first turn, per its own
/// doc comment on [`SimulationState`]): in both cases the id is not present in the living
/// set, and the search naturally lands on the next living id in sort order with no special
/// case needed.
///
/// Returns [`None`] only when `state.players` has no living entry at all.
#[must_use]
pub fn next_active_player(state: &SimulationState) -> Option<String> {
    let mut living_ids: Vec<&str> = state
        .players
        .iter()
        .filter(|player| !player.is_eliminated())
        .map(|player| player.id.as_str())
        .collect();
    // `SimulationState::players` is documented as kept sorted by `id`, but the rotation
    // below depends entirely on that order, so it is asserted here rather than borrowed on
    // trust from a comment on a struct this module does not own.
    living_ids.sort();

    let len = living_ids.len();
    if len == 0 {
        return None;
    }

    // Binary search for the current active id among the living: found means "skip past
    // them" (one past their index); not found (dead, or the empty pre-match placeholder)
    // means the search already landed on the insertion point, which is exactly the next
    // living id in sort order with no further adjustment.
    let anchor = match living_ids.binary_search(&state.active_player_id.as_str()) {
        Ok(idx) => idx.saturating_add(1),
        Err(idx) => idx,
    };
    // `len` is nonzero (checked above), so this remainder is always defined; the fallback
    // is unreachable defensively rather than assumed, matching this crate's no-panic rule.
    let next_idx = anchor.checked_rem(len).unwrap_or(0);
    living_ids.get(next_idx).map(|id| (*id).to_string())
}

/// Whether the match, in its current phase, accepts a player command right now.
///
/// Delegates to [`MatchPhase::accepts_ability_command`] so there is exactly one place that
/// decides this. In particular, [`MatchPhase::PassiveSelection`] already reads as "not
/// accepting" there — that is what stops a player from skipping the one-time passive prompt
/// by firing another ability while the choice is still pending.
#[must_use]
pub fn is_accepting_commands(state: &SimulationState) -> bool {
    state.phase.accepts_ability_command()
}

/// Whether sudden death should be in effect for `state` right now.
///
/// `PRODUCT_SPEC.md` §2 defines the trigger as turn 12 in a duel — exactly two players still
/// standing, evaluated by living player count rather than living team count so a 1v1 with
/// each side down to their last character reads the same as a free-for-all narrowed to its
/// final two. Its *behavior* ("rising hazard plus increasing damage pressure") is out of
/// scope for this file (see the module documentation); this is only the hook a future hazard
/// system calls once per turn to decide whether to escalate.
#[must_use]
pub fn is_sudden_death_active(state: &SimulationState) -> bool {
    let living = state
        .players
        .iter()
        .filter(|player| !player.is_eliminated())
        .count();
    living == 2 && state.turn_number >= SUDDEN_DEATH_TURN
}

/// The lowest-sorted-`id` living player, or [`None`] if nobody is living.
///
/// Shared by [`begin_match`], which needs an opener chosen the same way
/// [`next_active_player`] would choose one from a not-yet-started match (empty
/// `active_player_id`).
fn lowest_living_id(state: &SimulationState) -> Option<String> {
    state
        .players
        .iter()
        .filter(|player| !player.is_eliminated())
        .map(|player| player.id.clone())
        .min()
}

/// Movement allowance for `player_id`'s character, in fixed-point units.
///
/// Looks the class up in the frozen roster via `player_id`'s `character_id` rather than
/// caching it on [`crate::types::PlayerState`] — `types.rs` is the single source for
/// character stats, and re-deriving it here keeps this module from holding a second copy
/// that a definition update could leave stale.
///
/// # Errors
///
/// Returns [`SimError::UnknownDefinition`] if `player_id` does not name a player in
/// `state.players`, or if that player's `character_id` is not in the roster.
fn movement_allowance_for(state: &SimulationState, player_id: &str) -> SimResult<i32> {
    let Some(player) = state.player(player_id) else {
        return Err(SimError::UnknownDefinition);
    };
    let Some(character_def) = character::find(&player.character_id) else {
        return Err(SimError::UnknownDefinition);
    };
    Ok(character_def.movement.per_turn())
}

#[cfg(test)]
// A fixture invariant that must hold is stated with `let ... else { panic!() }` because it
// documents the expectation better than threading a `Result` through every test. Matches the
// precedent in `command.rs::tests`, `victory.rs::tests`, and `resolve::status::tests`.
// Production code in this module remains panic-free.
#[allow(clippy::panic)]
mod tests {
    use super::*;

    /// Advances one phase, discarding any status expiries.
    ///
    /// These tests assert phase transitions, not status lifecycle; the tests that do assert
    /// on expiries call [`advance_phase`] directly and inspect what it collected.
    fn step(state: &mut SimulationState) -> SimResult<MatchPhase> {
        advance_phase(state, &mut Vec::new(), &mut Vec::new())
    }
    use crate::fixed::{BODY_WIDTH, FixedPoint, POSITION_SCALE};
    use crate::types::{Appearance, EffectKind, Material, PlayerState, StatusEffect, TerrainMask};

    // -----------------------------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------------------------

    /// `character_id` defaults to "arzum" (Fast) unless overridden with [`player_as`].
    fn player(id: &str, team: u8) -> PlayerState {
        player_as(id, team, "arzum")
    }

    fn player_as(id: &str, team: u8, character_id: &str) -> PlayerState {
        PlayerState {
            id: id.to_string(),
            team,
            health: 200,
            max_health: 200,
            position: FixedPoint::ZERO,
            character_id: character_id.to_string(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn base_state(players: Vec<PlayerState>) -> SimulationState {
        SimulationState {
            pending_turn_end_reason: TurnEndReason::Passed,
            last_turn_end_reason: TurnEndReason::Passed,
            simulation_version: 2,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::MatchIntro,
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

    /// Drives a full lap of the phase cycle (`TurnStart` back to `TurnStart`) — see the
    /// module doc comment for why that is exactly eight [`advance_phase`] calls.
    fn run_full_lap(state: &mut SimulationState) {
        for _ in 0..8 {
            let Ok(_) = step(state) else {
                panic!("advance_phase must not error mid-lap in these fixtures");
            };
        }
    }

    // -----------------------------------------------------------------------------------
    // begin_match
    // -----------------------------------------------------------------------------------

    #[test]
    fn begin_match_opens_turn_one_with_the_lowest_sorted_id() {
        let mut state = base_state(vec![player("zeta", 0), player("alpha", 1)]);

        assert_eq!(begin_match(&mut state), Ok(()));

        assert_eq!(state.phase, MatchPhase::TurnStart);
        assert_eq!(state.turn_number, 1);
        assert_eq!(state.active_player_id, "alpha");
        assert!(!state.has_attacked_this_turn);
    }

    #[test]
    fn begin_match_refreshes_movement_from_the_openers_class() {
        // "karl" is Slow (2.5 BW); confirms begin_match reads the real roster, not a
        // hardcoded default.
        let mut state = base_state(vec![player_as("only", 0, "karl")]);

        assert_eq!(begin_match(&mut state), Ok(()));

        assert_eq!(state.movement_remaining, 10 * POSITION_SCALE);
    }

    #[test]
    fn begin_match_rejects_a_match_already_underway() {
        let mut state = base_state(vec![player("a", 0)]);
        state.phase = MatchPhase::Movement;

        assert_eq!(
            begin_match(&mut state),
            Err(SimError::OutOfRange { field: "phase" })
        );
    }

    #[test]
    fn begin_match_rejects_a_roster_with_nobody_living() {
        let mut dead = player("a", 0);
        dead.health = 0;
        let mut state = base_state(vec![dead]);

        assert_eq!(
            begin_match(&mut state),
            Err(SimError::OutOfRange { field: "players" })
        );
    }

    // -----------------------------------------------------------------------------------
    // advance_phase — the cycle itself
    // -----------------------------------------------------------------------------------

    #[test]
    fn advance_phase_steps_through_every_named_phase_in_order() {
        let mut state = base_state(vec![player("a", 0), player("b", 1)]);
        assert_eq!(begin_match(&mut state), Ok(()));
        assert_eq!(state.phase, MatchPhase::TurnStart);

        let expected = [
            MatchPhase::Movement,
            MatchPhase::AimingAndSelection,
            MatchPhase::CommandLocked,
            MatchPhase::Resolution,
            MatchPhase::Settling,
            MatchPhase::StatusResolution,
            MatchPhase::VictoryCheck,
            MatchPhase::TurnStart,
        ];
        for phase in expected {
            let reached = step(&mut state);
            assert_eq!(
                reached,
                Ok(phase),
                "phase must actually change to {phase:?}"
            );
            assert_eq!(state.phase, phase);
        }
    }

    #[test]
    fn a_full_cycle_returns_to_turn_start_with_turn_number_incremented_once() {
        let mut state = base_state(vec![player("a", 0), player("b", 1)]);
        assert_eq!(begin_match(&mut state), Ok(()));
        let turn_before = state.turn_number;

        run_full_lap(&mut state);

        assert_eq!(state.phase, MatchPhase::TurnStart);
        assert_eq!(state.turn_number, turn_before.saturating_add(1));
    }

    #[test]
    fn victory_check_commits_the_terminal_turn_reason_when_a_team_is_already_wiped() {
        let mut a = player("a", 0);
        a.health = 0;
        let mut state = base_state(vec![a, player("b", 1)]);
        state.phase = MatchPhase::VictoryCheck;
        state.pending_turn_end_reason = TurnEndReason::Eliminated;

        assert_eq!(step(&mut state), Ok(MatchPhase::MatchComplete));
        assert_eq!(state.phase, MatchPhase::MatchComplete);
        assert_eq!(state.last_turn_end_reason, TurnEndReason::Eliminated);
    }

    #[test]
    fn match_complete_is_absorbing() {
        let mut state = base_state(vec![player("a", 0)]);
        state.phase = MatchPhase::MatchComplete;

        assert_eq!(step(&mut state), Ok(MatchPhase::MatchComplete));
        assert_eq!(step(&mut state), Ok(MatchPhase::MatchComplete));
    }

    #[test]
    fn passive_selection_resumes_into_settling() {
        let mut state = base_state(vec![player("a", 0), player("b", 1)]);
        state.active_player_id = "a".to_string();
        state.phase = MatchPhase::PassiveSelection;

        assert_eq!(step(&mut state), Ok(MatchPhase::Settling));
    }

    #[test]
    fn hard_turn_limit_forces_a_draw_when_the_match_would_otherwise_run_forever() {
        let mut state = base_state(vec![player("a", 0), player("b", 1)]);
        state.active_player_id = "a".to_string();
        state.phase = MatchPhase::VictoryCheck;
        state.turn_number = HARD_TURN_LIMIT;

        assert_eq!(step(&mut state), Ok(MatchPhase::MatchComplete));
        let Some(a) = state.player("a") else {
            panic!("player a must still be present");
        };
        let Some(b) = state.player("b") else {
            panic!("player b must still be present");
        };
        assert!(a.is_eliminated());
        assert!(b.is_eliminated());
    }

    #[test]
    fn below_the_hard_turn_limit_the_match_keeps_going() {
        let mut state = base_state(vec![player("a", 0), player("b", 1)]);
        state.active_player_id = "a".to_string();
        state.phase = MatchPhase::VictoryCheck;
        state.turn_number = HARD_TURN_LIMIT.saturating_sub(1);

        assert_eq!(step(&mut state), Ok(MatchPhase::TurnStart));
        let Some(a) = state.player("a") else {
            panic!("player a must still be present");
        };
        assert!(!a.is_eliminated(), "must not force-draw before the ceiling");
    }

    // -----------------------------------------------------------------------------------
    // next_active_player
    // -----------------------------------------------------------------------------------

    #[test]
    fn next_active_player_wraps_around_in_sorted_id_order() {
        let mut state = base_state(vec![player("a", 0), player("b", 0), player("c", 0)]);

        state.active_player_id = "a".to_string();
        assert_eq!(next_active_player(&state).as_deref(), Some("b"));

        state.active_player_id = "b".to_string();
        assert_eq!(next_active_player(&state).as_deref(), Some("c"));

        state.active_player_id = "c".to_string();
        assert_eq!(
            next_active_player(&state).as_deref(),
            Some("a"),
            "must wrap from the last id back to the first"
        );
    }

    #[test]
    fn next_active_player_never_selects_an_eliminated_player() {
        let mut b = player("b", 0);
        b.health = 0;
        let mut state = base_state(vec![player("a", 0), b, player("c", 0)]);
        state.active_player_id = "a".to_string();

        assert_eq!(
            next_active_player(&state).as_deref(),
            Some("c"),
            "eliminated `b` must be skipped entirely, not merely deferred"
        );
    }

    #[test]
    fn next_active_player_handles_the_active_player_dying_mid_turn() {
        // The active player is eliminated (e.g. by their own backlash) before the turn
        // ends; they are no longer in the living set at all, and rotation must still find
        // the next living id after their old sorted position.
        let mut a = player("a", 0);
        a.health = 0;
        let mut state = base_state(vec![a, player("b", 0), player("c", 0)]);
        state.active_player_id = "a".to_string();

        assert_eq!(next_active_player(&state).as_deref(), Some("b"));
    }

    #[test]
    fn next_active_player_returns_none_when_nobody_is_living() {
        let mut a = player("a", 0);
        a.health = 0;
        let state = base_state(vec![a]);

        assert_eq!(next_active_player(&state), None);
    }

    #[test]
    fn next_active_player_before_the_match_starts_picks_the_first_sorted_id() {
        let state = base_state(vec![player("zeta", 0), player("alpha", 1)]);
        assert_eq!(state.active_player_id, "");

        assert_eq!(next_active_player(&state).as_deref(), Some("alpha"));
    }

    // -----------------------------------------------------------------------------------
    // end_turn
    // -----------------------------------------------------------------------------------

    #[test]
    fn end_turn_refreshes_movement_for_the_incoming_players_own_class() {
        // "arzum" is Fast (8 BW); "karl" is Slow (2.5 BW). Each incoming player must get
        // their own allowance, not the outgoing player's.
        let mut state = base_state(vec![
            player_as("arzum-p", 0, "arzum"),
            player_as("karl-p", 1, "karl"),
        ]);
        state.active_player_id = "arzum-p".to_string();
        state.movement_remaining = 999;
        state.has_attacked_this_turn = true;
        let turn_before = state.turn_number;

        assert_eq!(end_turn(&mut state, TurnEndReason::Attacked), Ok(()));

        assert_eq!(state.active_player_id, "karl-p");
        assert_eq!(
            state.movement_remaining,
            10 * POSITION_SCALE,
            "2.5 BW for Slow"
        );
        assert!(!state.has_attacked_this_turn);
        assert_eq!(state.turn_number, turn_before.saturating_add(1));

        // Rotate back the other way to confirm Fast is read correctly too, not just Slow.
        assert_eq!(end_turn(&mut state, TurnEndReason::Passed), Ok(()));
        assert_eq!(state.active_player_id, "arzum-p");
        assert_eq!(state.movement_remaining, 8 * BODY_WIDTH, "8 BW for Fast");
    }

    #[test]
    fn end_turn_errors_when_nobody_is_left_to_activate() {
        let mut a = player("a", 0);
        a.health = 0;
        let mut state = base_state(vec![a]);
        state.active_player_id = "a".to_string();

        assert_eq!(
            end_turn(&mut state, TurnEndReason::Eliminated),
            Err(SimError::OutOfRange {
                field: "active_player_id"
            })
        );
    }

    #[test]
    fn end_turn_accepts_every_reason_variant() {
        for reason in [
            TurnEndReason::Attacked,
            TurnEndReason::Passed,
            TurnEndReason::TimedOut,
            TurnEndReason::Eliminated,
        ] {
            let mut state = base_state(vec![player("a", 0), player("b", 1)]);
            state.active_player_id = "a".to_string();
            assert_eq!(end_turn(&mut state, reason), Ok(()));
        }
    }

    // -----------------------------------------------------------------------------------
    // Status ticking — exactly once per turn cycle
    // -----------------------------------------------------------------------------------

    #[test]
    fn a_two_turn_status_lasts_two_of_the_affected_players_own_turns() {
        let mut target = player("target", 1);
        target.statuses.push(StatusEffect {
            kind: EffectKind::Lockdown,
            magnitude: 0,
            turns_remaining: 2,
        });
        let mut state = base_state(vec![player("actor", 0), target]);
        assert_eq!(begin_match(&mut state), Ok(()));

        fn remaining(state: &SimulationState) -> Option<u8> {
            let Some(target) = state.player("target") else {
                panic!("target must still be present")
            };
            target.statuses.first().map(|status| status.turns_remaining)
        }

        // Lap 1 is the *actor's* turn. A status measured in the victim's turns must not
        // erode while somebody else acts — otherwise a two-turn Lockdown in a four-player
        // match would be spent before its victim ever got to move, and the same status
        // would mean something different at every table size.
        run_full_lap(&mut state);
        assert_eq!(
            remaining(&state),
            Some(2),
            "another player's turn must not tick it"
        );

        // Lap 2 is the target's own turn, and it ends with exactly one decrement.
        run_full_lap(&mut state);
        assert_eq!(
            remaining(&state),
            Some(1),
            "the victim's own turn ticks it once"
        );

        // Lap 3 is the actor again: still untouched.
        run_full_lap(&mut state);
        assert_eq!(
            remaining(&state),
            Some(1),
            "and still not on the actor's turn"
        );

        // Lap 4 is the target's second turn, which spends the last one.
        run_full_lap(&mut state);
        assert_eq!(
            remaining(&state),
            None,
            "gone after exactly two of the victim's own turns, not two laps",
        );
    }

    #[test]
    fn a_count_based_guarantee_crit_is_not_eroded_by_the_turn_tick() {
        let mut target = player("target", 1);
        // `GuaranteeCrit` stores remaining charges in `magnitude` and parks
        // `turns_remaining` at the never-expires sentinel. A turn tick that decremented it
        // would quietly convert "the next three attacks crit" into a countdown clock.
        target.statuses.push(StatusEffect {
            kind: EffectKind::GuaranteeCrit,
            magnitude: 3,
            turns_remaining: u8::MAX,
        });
        let mut state = base_state(vec![player("actor", 0), target]);
        assert_eq!(begin_match(&mut state), Ok(()));

        for _ in 0..4 {
            run_full_lap(&mut state);
        }

        let Some(target) = state.player("target") else {
            panic!("target must still be present")
        };
        let Some(status) = target.statuses.first() else {
            panic!("a count-based mark must survive turn ticking")
        };
        assert_eq!(
            status.magnitude, 3,
            "charges are spent by use, never by time"
        );
        assert_eq!(
            status.turns_remaining,
            u8::MAX,
            "the sentinel must not erode"
        );
    }

    // -----------------------------------------------------------------------------------
    // is_accepting_commands
    // -----------------------------------------------------------------------------------

    #[test]
    fn is_accepting_commands_matches_the_movement_and_aiming_phases_only() {
        let mut state = base_state(vec![player("a", 0)]);

        for phase in [
            MatchPhase::MatchIntro,
            MatchPhase::TurnStart,
            MatchPhase::CommandLocked,
            MatchPhase::Resolution,
            MatchPhase::Settling,
            MatchPhase::StatusResolution,
            MatchPhase::VictoryCheck,
            MatchPhase::MatchComplete,
        ] {
            state.phase = phase;
            assert!(
                !is_accepting_commands(&state),
                "{phase:?} must reject commands"
            );
        }

        for phase in [MatchPhase::Movement, MatchPhase::AimingAndSelection] {
            state.phase = phase;
            assert!(
                is_accepting_commands(&state),
                "{phase:?} must accept commands"
            );
        }
    }

    #[test]
    fn passive_selection_blocks_commands_so_the_choice_cannot_be_skipped() {
        let mut state = base_state(vec![player("a", 0)]);
        state.phase = MatchPhase::PassiveSelection;

        assert!(!is_accepting_commands(&state));
    }

    // -----------------------------------------------------------------------------------
    // Sudden death trigger
    // -----------------------------------------------------------------------------------

    #[test]
    fn sudden_death_triggers_at_turn_twelve_in_a_duel() {
        let mut state = base_state(vec![player("a", 0), player("b", 1)]);

        state.turn_number = 11;
        assert!(!is_sudden_death_active(&state), "not yet at turn 12");

        state.turn_number = 12;
        assert!(is_sudden_death_active(&state));

        state.turn_number = 20;
        assert!(
            is_sudden_death_active(&state),
            "stays active past the trigger turn"
        );
    }

    #[test]
    fn sudden_death_does_not_trigger_outside_a_duel() {
        let mut state = base_state(vec![player("a", 0), player("b", 1), player("c", 2)]);
        state.turn_number = 12;

        assert!(
            !is_sudden_death_active(&state),
            "three living players is not a duel, regardless of turn number"
        );
    }
}
