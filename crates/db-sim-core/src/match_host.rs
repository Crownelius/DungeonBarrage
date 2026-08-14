//! The match orchestrator — the layer that turns a pile of correct systems into a game.
//!
//! Every subsystem below this one was, at some point, correct, tested, and called by
//! nothing. `todolist.md` records four occurrences. This module exists so there is exactly
//! one place that drives them, and so "can a match be played end to end?" has an answer that
//! is a test rather than an opinion.
//!
//! # What it owns
//!
//! ```text
//! MatchHost
//!   ├─ scheduler   phase cycle, turn order, sudden death, hard turn limit
//!   ├─ movement    walking, jumping, falling
//!   ├─ command     ability validation and application (the security boundary)
//!   ├─ victory     elimination and outcome
//!   └─ block_ops   terrain operations, routed through block health
//! ```
//!
//! # What it deliberately does not own
//!
//! Any game rule. This module sequences calls; it never decides damage, legality, or
//! outcome. A rule implemented here instead of in the subsystem that owns it would be a
//! second resolution path, which is the bug shape this codebase keeps producing.
//!
//! # The passive-selection interrupt
//!
//! `MatchPhase::PassiveSelection` had no entry point before this module: `command.rs` could
//! *read* the phase and `scheduler.rs` could resume from it, but nothing ever *set* it
//! outside tests, so the one-time passive choice could never actually be triggered in a real
//! match. [`MatchHost::submit_ability`] raises it the first time an actor's gauge fills.

use crate::error::SimResult;
use crate::types::{
    AbilityCommand, CommandResult, GAUGE_FULL, MatchOutcome, MatchPhase, PassiveChoiceCommand,
    SimulationState, TurnEndReason,
};
use crate::{command, movement, scheduler, victory};

/// Drives one match.
///
/// Owns the authoritative state. The client never holds a `SimulationState` directly — it
/// submits intents and reads results, which is the trust boundary `SECURITY_BASELINE.md` §2
/// requires.
#[derive(Debug)]
pub struct MatchHost {
    state: SimulationState,
}

impl MatchHost {
    /// Wraps an already-constructed state and begins the match.
    ///
    /// # Errors
    ///
    /// Propagates any failure from [`scheduler::begin_match`].
    pub fn start(mut state: SimulationState) -> SimResult<Self> {
        scheduler::begin_match(&mut state)?;
        // Settle immediately so nobody begins the match hanging in the air. A character
        // spawned above their platform would otherwise take their first turn mid-fall.
        movement::settle(&mut state)?;
        let mut host = Self { state };
        host.open_turn()?;
        Ok(host)
    }

    /// Advances a freshly-opened turn from `TurnStart` into `Movement`, where commands are
    /// accepted.
    ///
    /// `begin_match` and the scheduler's rotation both leave the phase at `TurnStart`, which
    /// `scheduler::is_accepting_commands` deliberately refuses — a turn is not ready for
    /// input until any scheduled start-of-turn effects have run. Opening it is the host's
    /// job, not the scheduler's, because only the host knows there is a client waiting.
    fn open_turn(&mut self) -> SimResult<()> {
        if self.state.phase == MatchPhase::TurnStart {
            scheduler::advance_phase(&mut self.state)?;
        }
        // `todolist.md` P12: a gauge can fill from damage *taken* during someone else's turn,
        // or from being healed. Raising the prompt only for whoever just acted meant such a
        // player reached a full gauge and was never offered their one-time choice until they
        // next attacked — `CHARACTERS.md` §2 says it happens the first time the gauge fills.
        //
        // Prompted at the start of the owed player's own turn rather than the instant the
        // gauge fills, because `MatchPhase` is global: only one player can be choosing at a
        // time, and interrupting someone else's turn to ask a third party would need a
        // per-player phase this design does not have.
        let actor = self.state.active_player_id.clone();
        self.raise_passive_selection_if_due(&actor);
        Ok(())
    }

    /// Read-only view of the authoritative state.
    #[must_use]
    pub const fn state(&self) -> &SimulationState {
        &self.state
    }

    /// The current phase.
    #[must_use]
    pub const fn phase(&self) -> MatchPhase {
        self.state.phase
    }

    /// Whose turn it is.
    #[must_use]
    pub fn active_player(&self) -> &str {
        &self.state.active_player_id
    }

    /// Whether the match has reached a terminal state.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self.state.phase, MatchPhase::MatchComplete)
    }

    /// The current outcome without mutating anything.
    #[must_use]
    pub fn outcome(&self) -> MatchOutcome {
        victory::evaluate(&self.state)
    }

    /// Moves the active player horizontally, returning the distance actually travelled.
    ///
    /// Terrain, the movement allowance, and `Lockdown` are all enforced by
    /// [`movement::walk`]; this only checks that it is a legal moment to move.
    ///
    /// # Errors
    ///
    /// Propagates failures from [`movement::walk`] and [`movement::settle`].
    pub fn submit_move(&mut self, player_id: &str, dx: i32) -> SimResult<i32> {
        if !scheduler::is_accepting_commands(&self.state)
            || player_id != self.state.active_player_id
        {
            return Ok(0);
        }
        let travelled = movement::walk(&mut self.state, player_id, dx)?;
        // Walking off a ledge must drop the character in the same action, not next turn.
        movement::settle(&mut self.state)?;
        Ok(travelled)
    }

    /// Submits an ability command and advances the match.
    ///
    /// Validation, damage, effects, and terrain all resolve inside [`command::apply_ability`]
    /// — this adds only the sequencing around it:
    ///
    /// 1. Apply the command. A rejection leaves state untouched and does **not** end the turn;
    ///    a client sending a malformed command must not lose its turn to it.
    /// 2. Settle anything the action left airborne.
    /// 3. Raise the passive-selection interrupt if the actor's gauge just filled for the
    ///    first time, so the choice cannot be skipped.
    /// 4. Otherwise run the turn to completion and hand over.
    ///
    /// # Errors
    ///
    /// Propagates failures from settling and from the scheduler.
    pub fn submit_ability(&mut self, ability: &AbilityCommand) -> SimResult<CommandResult> {
        if !scheduler::is_accepting_commands(&self.state) {
            return Ok(CommandResult::Rejected(
                crate::types::CommandRejection::WrongPhase,
            ));
        }

        let result = command::apply_ability(&mut self.state, ability);
        if matches!(result, CommandResult::Rejected(_)) {
            // A rejected command costs nothing. Ending the turn here would let a client
            // grief itself into a skipped turn, and would let a *replayed* command end a
            // turn that the original already ended.
            return Ok(result);
        }

        movement::settle(&mut self.state)?;

        if self.raise_passive_selection_if_due(&ability.player_id) {
            // Hold the turn open. The scheduler resumes the cycle once the choice lands.
            return Ok(result);
        }

        self.finish_turn(TurnEndReason::Attacked)?;
        Ok(result)
    }

    /// Submits the one-time passive choice and resumes the interrupted turn.
    ///
    /// # Errors
    ///
    /// Propagates scheduler failures once the choice is accepted.
    pub fn submit_passive_choice(
        &mut self,
        choice: &PassiveChoiceCommand,
    ) -> SimResult<CommandResult> {
        let result = command::apply_passive_choice(&mut self.state, choice);
        if matches!(result, CommandResult::Rejected(_)) {
            return Ok(result);
        }
        self.finish_turn(TurnEndReason::Attacked)?;
        Ok(result)
    }

    /// Ends the active player's turn without acting.
    ///
    /// # Errors
    ///
    /// Propagates scheduler failures.
    pub fn pass_turn(&mut self) -> SimResult<()> {
        self.finish_turn(TurnEndReason::Passed)
    }

    /// Applies the deterministic timeout action for an expired planning deadline.
    ///
    /// The server owns the clock (`SECURITY_BASELINE.md` §2), so a client can never trigger
    /// this — it exists for the room process to call.
    ///
    /// # Errors
    ///
    /// Propagates scheduler failures.
    pub fn time_out_turn(&mut self) -> SimResult<()> {
        self.finish_turn(TurnEndReason::TimedOut)
    }

    /// Raises [`MatchPhase::PassiveSelection`] if `player_id` just earned their first choice.
    ///
    /// Returns whether the interrupt was raised. This is the entry point that did not exist
    /// before: without it, a full gauge never prompts and the passive is never chosen.
    fn raise_passive_selection_if_due(&mut self, player_id: &str) -> bool {
        let due = self
            .state
            .player(player_id)
            .is_some_and(|player| !player.has_chosen_passive && player.special_gauge >= GAUGE_FULL);
        if due {
            self.state.phase = MatchPhase::PassiveSelection;
        }
        due
    }

    /// Runs the remainder of the turn cycle and hands over to the next player.
    ///
    /// **The scheduler owns the whole cycle, including `end_turn`.** Leaving
    /// `MatchPhase::VictoryCheck` is what runs the victory check, forces the draw at the
    /// hard turn limit, rotates to the next player, and lands on `TurnStart`. An earlier
    /// version of this function stopped *at* `VictoryCheck` and called `end_turn` itself,
    /// which rotated twice and skipped the victory check entirely — matches never ended and
    /// an eliminated team never produced a winner. Do not reintroduce that: drive the
    /// scheduler, never duplicate it.
    ///
    /// Bounded rather than looped-until-done: a phase machine that cannot progress must
    /// surface as a stuck match, not a hung process.
    fn finish_turn(&mut self, reason: TurnEndReason) -> SimResult<()> {
        const MAX_PHASE_STEPS: u32 = 32;

        // Declare why this turn is ending before driving the cycle. The scheduler commits it
        // when it reaches `end_turn`; this layer is the only one that knows the difference
        // between an attack, a pass, and a timeout (`todolist.md` P11).
        self.state.pending_turn_end_reason = reason;

        let mut steps = 0u32;
        loop {
            if self.is_complete() {
                return Ok(());
            }
            let phase = scheduler::advance_phase(&mut self.state)?;
            steps = steps.saturating_add(1);
            if matches!(phase, MatchPhase::TurnStart | MatchPhase::MatchComplete) {
                break;
            }
            if steps >= MAX_PHASE_STEPS {
                break;
            }
        }
        if self.is_complete() {
            return Ok(());
        }
        // A new actor may be standing on terrain the last action removed.
        movement::settle(&mut self.state)?;
        self.open_turn()
    }
}

#[cfg(test)]
// Tests may panic on a fixture invariant; production paths above may not.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::fixed::FixedPoint;
    use crate::map;
    use crate::types::{Appearance, PlayerState};

    fn player(id: &str, team: u8, character_id: &str, position: FixedPoint) -> PlayerState {
        PlayerState {
            id: id.to_owned(),
            team,
            health: 300,
            max_health: 300,
            position,
            character_id: character_id.to_owned(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    /// A real map with real characters — not a hand-built stub.
    fn duel() -> SimulationState {
        let definition = map::horizontal_test_array();
        let Ok(terrain) = map::build_mask(&definition) else {
            panic!("fixture invariant: the test map must build");
        };
        let Some(first) = definition.spawn_points.first().copied() else {
            panic!("fixture invariant: the test map must have spawn points");
        };
        let Some(second) = definition.spawn_points.get(4).copied() else {
            panic!("fixture invariant: the test map must have several spawn points");
        };
        let mut players = vec![
            player("a_arzum", 0, "arzum", first),
            player("b_emi", 1, "emi", second),
        ];
        players.sort_by(|left, right| left.id.cmp(&right.id));

        SimulationState {
            pending_turn_end_reason: TurnEndReason::Passed,
            last_turn_end_reason: TurnEndReason::Passed,
            simulation_version: crate::SIMULATION_VERSION,
            content_version: crate::CONTENT_VERSION,
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
            rng_state: 20_260_807,
        }
    }

    #[test]
    fn a_match_starts_with_an_active_player_and_accepts_commands() {
        let Ok(host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        assert!(
            !host.active_player().is_empty(),
            "somebody must be on turn once the match begins",
        );
        assert!(!host.is_complete());
        assert_eq!(host.outcome(), MatchOutcome::InProgress);
    }

    #[test]
    fn passing_hands_the_turn_to_the_other_player() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let first = host.active_player().to_owned();
        let Ok(()) = host.pass_turn() else {
            panic!("passing must succeed");
        };
        assert_ne!(
            host.active_player(),
            first,
            "the turn must move to the other player",
        );
    }

    #[test]
    fn a_full_round_returns_to_the_first_player_and_advances_the_turn_counter() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let first = host.active_player().to_owned();
        let before = host.state().turn_number;

        let Ok(()) = host.pass_turn() else {
            panic!("pass must succeed");
        };
        let Ok(()) = host.pass_turn() else {
            panic!("pass must succeed");
        };

        assert_eq!(host.active_player(), first, "two passes complete a round");
        assert!(
            host.state().turn_number > before,
            "the turn counter must advance",
        );
    }

    #[test]
    fn the_active_player_can_actually_walk() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let actor = host.active_player().to_owned();
        let Some(before) = host.state().player(&actor).map(|p| p.position) else {
            panic!("the active player must exist");
        };

        let Ok(travelled) = host.submit_move(&actor, 2 * crate::POSITION_SCALE) else {
            panic!("walking must succeed");
        };

        let Some(after) = host.state().player(&actor).map(|p| p.position) else {
            panic!("the active player must still exist");
        };
        assert!(travelled > 0, "some distance must be covered");
        assert_ne!(before, after, "the character must actually move");
    }

    #[test]
    fn a_player_who_is_not_on_turn_cannot_move() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let actor = host.active_player().to_owned();
        let Some(idle) = host
            .state()
            .players
            .iter()
            .map(|p| p.id.clone())
            .find(|id| *id != actor)
        else {
            panic!("a duel has two players");
        };
        let Some(before) = host.state().player(&idle).map(|p| p.position) else {
            panic!("the idle player must exist");
        };

        let Ok(travelled) = host.submit_move(&idle, 4 * crate::POSITION_SCALE) else {
            panic!("an out-of-turn move is refused, not an error");
        };

        assert_eq!(travelled, 0);
        assert_eq!(
            host.state().player(&idle).map(|p| p.position),
            Some(before),
            "an out-of-turn move must not shift anybody",
        );
    }

    #[test]
    fn a_match_always_terminates() {
        // The load-bearing property: `PRODUCT_SPEC.md` §2 requires every match to reach a
        // terminal state. Passing forever must still end, via the hard turn limit — a match
        // that can run indefinitely is a hung room, not a long game.
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let mut guard = 0u32;
        while !host.is_complete() {
            let Ok(()) = host.pass_turn() else {
                panic!("passing must keep succeeding");
            };
            guard = guard.saturating_add(1);
            assert!(
                guard < scheduler::HARD_TURN_LIMIT.saturating_mul(4),
                "a match must terminate; ran {guard} passes without completing",
            );
        }
        assert!(matches!(
            host.outcome(),
            MatchOutcome::Draw | MatchOutcome::Victory { .. }
        ));
    }

    #[test]
    fn eliminating_a_team_completes_the_match_with_the_right_winner() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let Some(loser) = host
            .state()
            .players
            .iter()
            .find(|p| p.team == 1)
            .map(|p| p.id.clone())
        else {
            panic!("team 1 must exist");
        };

        // Reach in to eliminate: this test is about the host observing a terminal state, not
        // about how the damage arrived.
        let Ok(()) = victory::eliminate(&mut host.state, &loser) else {
            panic!("elimination must succeed");
        };
        let Ok(()) = host.pass_turn() else {
            panic!("the turn must still resolve");
        };

        assert_eq!(host.outcome(), MatchOutcome::Victory { team: 0 });
        assert!(host.is_complete(), "a decided match must be over");
    }

    #[test]
    fn the_same_inputs_produce_the_same_match() {
        // Determinism at the level that matters: the whole orchestrated loop, not one helper.
        let run = || {
            let Ok(mut host) = MatchHost::start(duel()) else {
                panic!("a match must be startable");
            };
            for _ in 0..6 {
                let actor = host.active_player().to_owned();
                let _ = host.submit_move(&actor, crate::POSITION_SCALE);
                let Ok(()) = host.pass_turn() else {
                    panic!("pass must succeed");
                };
            }
            crate::hash::hash_state(host.state())
        };
        assert_eq!(run(), run());
    }
}
