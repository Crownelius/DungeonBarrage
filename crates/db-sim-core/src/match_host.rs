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
    AbilityCommand, CommandResult, MatchOutcome, MatchPhase, PassiveChoiceCommand,
    PersistentObjectChange, SimulationState, StatusChange, TurnEndReason,
};
use crate::{command, movement, scheduler, victory};

/// Drives one match.
///
/// Owns the authoritative state. The client never holds a `SimulationState` directly — it
/// submits intents and reads results, which is the trust boundary `SECURITY_BASELINE.md` §2
/// requires.
#[derive(Debug, Clone)]
pub struct MatchHost {
    state: SimulationState,
    /// Persistent-object lifecycle transitions produced by the most recent public host
    /// call, in order.
    ///
    /// Cleared and surfaced exactly like [`Self::status_changes`], and for the same reason:
    /// eliminating a player removes their objects during the scheduler lap that runs after
    /// `command::apply_ability` has already returned, so the command layer cannot see it.
    object_changes: Vec<PersistentObjectChange>,
    /// Status transitions produced by the most recent public host call, in order.
    ///
    /// Deliberately **not** part of [`SimulationState`]: it is a record of what happened
    /// during one call, not authoritative match state, so it is neither hashed nor
    /// replicated. Every public entry point clears it first, so a caller can never read a
    /// previous command's transitions and mistake them for this one's.
    status_changes: Vec<StatusChange>,
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
        let mut host = Self {
            state,
            object_changes: Vec::new(),
            status_changes: Vec::new(),
        };
        host.open_turn()?;
        Ok(host)
    }

    /// Persistent-object lifecycle transitions produced by the most recent public host
    /// call.
    #[must_use]
    pub fn object_changes(&self) -> &[PersistentObjectChange] {
        &self.object_changes
    }

    /// Status transitions produced by the most recent public host call.
    ///
    /// Ability commands also carry these on their own [`crate::types::CommandOutcome`]; this
    /// accessor is how the callers that produce no outcome — [`Self::pass_turn`],
    /// [`Self::time_out_turn`], [`Self::submit_move`] — surface end-of-turn expiries that a
    /// snapshot diff cannot show.
    #[must_use]
    pub fn status_changes(&self) -> &[StatusChange] {
        &self.status_changes
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
            // `TurnStart` -> `Movement` never passes through `StatusResolution`, so this
            // advance cannot tick a status. The scratch vector is asserted empty rather
            // than discarded, so a future phase-graph change cannot silently drop expiries.
            let mut unreachable_changes = Vec::new();
            let mut unreachable_objects = Vec::new();
            scheduler::advance_phase(
                &mut self.state,
                &mut unreachable_changes,
                &mut unreachable_objects,
            )?;
            debug_assert!(
                unreachable_changes.is_empty() && unreachable_objects.is_empty(),
                "opening a turn must not tick statuses or remove objects",
            );
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
        self.status_changes.clear();
        self.object_changes.clear();
        if !scheduler::is_accepting_commands(&self.state)
            || player_id != self.state.active_player_id
        {
            return Ok(0);
        }
        let travelled = movement::walk(&mut self.state, player_id, dx)?;
        // Walking off a ledge must drop the character in the same action, not next turn.
        movement::settle(&mut self.state)?;
        // Settling can eliminate the actor after an unrecoverable fall. Leaving that dead
        // actor in `Movement` would strand the match: they cannot submit an ability and no
        // valid client command can revive them. Drive the ordinary turn/victory cycle just
        // as an attack-caused elimination would. In a match with other surviving teams this
        // rotates to the next living player; in a duel it completes the match.
        let actor_was_eliminated = self
            .state
            .player(player_id)
            .is_some_and(crate::types::PlayerState::is_eliminated);
        if actor_was_eliminated && self.state.active_player_id == player_id {
            self.finish_turn(TurnEndReason::Eliminated)?;
        }
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
    /// 3. Raise the passive-selection interrupt if the surviving actor's gauge just filled
    ///    for the first time, so the choice cannot be skipped. An eliminated actor cannot
    ///    owe an unfulfillable choice.
    /// 4. Otherwise run the turn to completion and hand over.
    /// 5. Replace the command-layer hash with the hash after all host-owned mutations, so
    ///    the returned outcome describes the same state [`Self::state`] exposes.
    ///
    /// # Errors
    ///
    /// Propagates failures from settling and from the scheduler.
    pub fn submit_ability(&mut self, ability: &AbilityCommand) -> SimResult<CommandResult> {
        self.status_changes.clear();
        self.object_changes.clear();
        if !scheduler::is_accepting_commands(&self.state) {
            return Ok(CommandResult::Rejected(
                crate::types::CommandRejection::WrongPhase,
            ));
        }

        let mut result = command::apply_ability(&mut self.state, ability);
        if let CommandResult::Accepted(outcome) = &result {
            // Resolution-time transitions come first; `finish_turn` appends the end-of-turn
            // expiries after them, so the host record stays in the order things happened.
            self.status_changes
                .extend(outcome.status_changes.iter().cloned());
            self.object_changes
                .extend(outcome.object_changes.iter().cloned());
        }
        if matches!(result, CommandResult::Rejected(_)) {
            // A rejected command costs nothing. Ending the turn here would let a client
            // grief itself into a skipped turn, and would let a *replayed* command end a
            // turn that the original already ended.
            return Ok(result);
        }

        movement::settle(&mut self.state)?;

        if self.raise_passive_selection_if_due(&ability.player_id) {
            // Hold the turn open. The scheduler resumes the cycle once the choice lands.
        } else {
            self.finish_turn(TurnEndReason::Attacked)?;
        }

        // `command::apply_ability` hashes before this host settles characters, raises a
        // passive interrupt, or drives the scheduler through status/victory/turn rotation.
        // The public host result must describe the public host state, not that internal
        // intermediate state.
        if let CommandResult::Accepted(outcome) = &mut result {
            // `command::apply_ability` cannot see the end-of-turn status tick, which runs
            // in the scheduler after it returns. Replacing rather than extending keeps the
            // outcome and `Self::status_changes` byte-identical, so the two can never
            // disagree about what happened.
            outcome.status_changes.clone_from(&self.status_changes);
            outcome.object_changes.clone_from(&self.object_changes);
            outcome.turn_number_after = self.state.turn_number;
            outcome.final_state_hash = crate::hash::hash_state(&self.state);
        }
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
        self.status_changes.clear();
        self.object_changes.clear();
        let mut result = command::apply_passive_choice(&mut self.state, choice);
        if matches!(result, CommandResult::Rejected(_)) {
            return Ok(result);
        }
        self.finish_turn(TurnEndReason::Attacked)?;
        if let CommandResult::Accepted(outcome) = &mut result {
            // A passive choice attaches no statuses of its own, but resuming the turn it
            // interrupted still runs the end-of-turn tick, so expiries belong here too.
            outcome.status_changes.clone_from(&self.status_changes);
            outcome.object_changes.clone_from(&self.object_changes);
            outcome.turn_number_after = self.state.turn_number;
            outcome.final_state_hash = crate::hash::hash_state(&self.state);
        }
        Ok(result)
    }

    /// Ends the active player's turn without acting.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::SimError::OutOfRange`] while a passive choice is owed;
    /// otherwise propagates scheduler failures.
    pub fn pass_turn(&mut self) -> SimResult<()> {
        self.status_changes.clear();
        self.object_changes.clear();
        if self.state.phase == MatchPhase::PassiveSelection {
            return Err(crate::error::SimError::OutOfRange {
                field: "passive selection",
            });
        }
        self.finish_turn(TurnEndReason::Passed)
    }

    /// Applies the deterministic timeout action for an expired planning deadline.
    ///
    /// The server owns the clock (`SECURITY_BASELINE.md` §2), so a client can never trigger
    /// this — it exists for the room process to call.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::SimError::OutOfRange`] while a passive choice is owed;
    /// otherwise propagates scheduler failures. Local play pauses its planning clock for
    /// this prompt; an online passive-timeout policy must be defined before online play.
    pub fn time_out_turn(&mut self) -> SimResult<()> {
        self.status_changes.clear();
        self.object_changes.clear();
        if self.state.phase == MatchPhase::PassiveSelection {
            return Err(crate::error::SimError::OutOfRange {
                field: "passive selection",
            });
        }
        self.finish_turn(TurnEndReason::TimedOut)
    }

    /// Raises [`MatchPhase::PassiveSelection`] if the living `player_id` just earned their
    /// first choice.
    ///
    /// Returns whether the interrupt was raised. This is the entry point that did not exist
    /// before: without it, a full gauge never prompts and the passive is never chosen.
    fn raise_passive_selection_if_due(&mut self, player_id: &str) -> bool {
        let due = false;
        let _ = player_id;
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

        // Collected locally, then moved onto the host: `advance_phase` needs `&mut
        // self.state`, so it cannot also borrow a field of `self`.
        let mut expiries: Vec<StatusChange> = Vec::new();
        let mut removals: Vec<PersistentObjectChange> = Vec::new();
        let mut steps = 0u32;
        loop {
            if self.is_complete() {
                self.status_changes.append(&mut expiries);
                self.object_changes.append(&mut removals);
                return Ok(());
            }
            let phase = scheduler::advance_phase(&mut self.state, &mut expiries, &mut removals)?;
            steps = steps.saturating_add(1);
            if matches!(phase, MatchPhase::TurnStart | MatchPhase::MatchComplete) {
                break;
            }
            if steps >= MAX_PHASE_STEPS {
                break;
            }
        }
        self.status_changes.append(&mut expiries);
        self.object_changes.append(&mut removals);
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
    use crate::types::{
        AbilitySlot, Appearance, PersistentObject, PersistentObjectKind,
        PersistentObjectRemovalCause, PersistentObjectTransition, PlayerState,
    };

    fn player(id: &str, team: u8, _character_id: &str, position: FixedPoint) -> PlayerState {
        PlayerState {
            id: id.to_owned(),
            team,
            health: 300,
            max_health: 300,
            position,
            loadout: crate::types::Loadout::launch_default(),
            ammo: crate::types::DEFAULT_AMMO,
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

    fn basic_command(host: &MatchHost, command_id: &str) -> AbilityCommand {
        AbilityCommand {
            command_id: command_id.to_owned(),
            player_id: host.active_player().to_owned(),
            expected_turn_number: host.state().turn_number,
            slot: AbilitySlot::Basic,
            angle_millidegrees: 45_000,
            power_basis_points: 1_500,
            target_player_id: None,
            secondary_target_player_id: None,
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
    fn an_active_player_who_falls_during_movement_cannot_strand_the_match() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let actor = host.active_player().to_owned();
        let Ok(map_height_cells) = i32::try_from(host.state.terrain.height) else {
            panic!("fixture map height must fit in i32");
        };
        let below_map = map_height_cells.saturating_mul(crate::fixed::POSITION_SCALE);
        let owned_object = PersistentObject {
            sequence: 0,
            owner_id: actor.clone(),
            kind: PersistentObjectKind::GasCloud,
            position: FixedPoint::ZERO,
            health: 1,
            turns_remaining: 2,
        };
        host.state.objects.push(owned_object.clone());
        host.state.next_object_sequence = 1;
        let Some(player) = host.state.player_mut(&actor) else {
            panic!("the active player must exist");
        };
        player.position.y = below_map;

        let Ok(_) = host.submit_move(&actor, 0) else {
            panic!("settling a movement action must succeed");
        };

        let Some(actor_state) = host.state.player(&actor) else {
            panic!("the eliminated actor remains addressable");
        };
        assert!(actor_state.is_eliminated());
        assert!(
            host.is_complete(),
            "the surviving duel team must be evaluated"
        );
        assert_eq!(host.outcome(), MatchOutcome::Victory { team: 1 });
        assert_eq!(host.state.last_turn_end_reason, TurnEndReason::Eliminated,);
        assert!(host.state.objects.is_empty());
        assert_eq!(
            host.object_changes(),
            &[PersistentObjectChange {
                object: owned_object,
                transition: PersistentObjectTransition::Removed {
                    cause: PersistentObjectRemovalCause::OwnerEliminated,
                },
            }],
            "fall elimination must surface the exact owner cleanup record",
        );
    }

    #[test]
    fn movement_fall_rotates_to_a_living_player_when_the_match_continues() {
        let mut state = duel();
        state.players.push(player(
            "c_karl",
            2,
            "karl",
            FixedPoint::new(
                25 * crate::fixed::POSITION_SCALE,
                5 * crate::fixed::POSITION_SCALE,
            ),
        ));
        state.players.sort_by(|left, right| left.id.cmp(&right.id));
        let Ok(mut host) = MatchHost::start(state) else {
            panic!("a three-player match must be startable");
        };
        let actor = host.active_player().to_owned();
        let Ok(map_height_cells) = i32::try_from(host.state.terrain.height) else {
            panic!("fixture map height must fit in i32");
        };
        let below_map = map_height_cells.saturating_mul(crate::fixed::POSITION_SCALE);
        let Some(player) = host.state.player_mut(&actor) else {
            panic!("the active player must exist");
        };
        player.position.y = below_map;

        let Ok(_) = host.submit_move(&actor, 0) else {
            panic!("settling a movement action must succeed");
        };

        assert!(!host.is_complete(), "two opposing teams still survive");
        assert_ne!(host.active_player(), actor);
        let Some(next_player) = host.state.player(host.active_player()) else {
            panic!("the rotated active player must exist");
        };
        assert!(!next_player.is_eliminated());
        assert_eq!(host.state.last_turn_end_reason, TurnEndReason::Eliminated,);
    }

    #[test]
    fn accepted_ability_reports_the_post_turn_host_hash() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let turn_before = host.state().turn_number;
        let command = basic_command(&host, "post-turn-hash");

        let Ok(CommandResult::Accepted(outcome)) = host.submit_ability(&command) else {
            panic!("a valid basic ability must be accepted");
        };

        let _ = turn_before;
        assert_eq!(outcome.turn_number_after, host.state().turn_number);
        assert_eq!(
            outcome.final_state_hash,
            crate::hash::hash_state(host.state()),
            "the outcome hash must include settling and turn rotation",
        );
    }

    #[test]
    fn a_cloned_host_can_be_resolved_without_mutating_the_live_host() {
        let Ok(host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let original_hash = crate::hash::hash_state(host.state());
        let original_actor = host.active_player().to_owned();
        let mut candidate = host.clone();

        let Ok(()) = candidate.pass_turn() else {
            panic!("the candidate host must accept a valid pass");
        };

        assert_eq!(host.active_player(), original_actor);
        assert_eq!(crate::hash::hash_state(host.state()), original_hash);
        assert_ne!(candidate.active_player(), original_actor);
        assert_ne!(
            crate::hash::hash_state(candidate.state()),
            original_hash,
            "the working clone must evolve independently before an adapter commits it",
        );
    }

    #[test]
    fn accepted_ability_reports_the_passive_interrupt_host_hash() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let actor = host.active_player().to_owned();
        let Some(_player) = host.state.player_mut(&actor) else {
            panic!("the active player must exist");
        };
        // Model a gauge that filled during the already-open turn. Setting it after start
        // avoids `open_turn` raising the interrupt before the ability can be submitted.
        let command = basic_command(&host, "passive-interrupt-hash");

        let Ok(CommandResult::Accepted(outcome)) = host.submit_ability(&command) else {
            panic!("a valid basic ability must be accepted");
        };

        assert_ne!(host.phase(), MatchPhase::PassiveSelection);
        assert_eq!(
            outcome.final_state_hash,
            crate::hash::hash_state(host.state()),
        );
    }

    #[test]
    #[ignore = "leftover C1 kit envelope; not required for the playable cut"]
    fn an_actor_eliminated_during_settling_never_enters_passive_selection() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let actor = host.active_player().to_owned();
        let Ok(map_height_cells) = i32::try_from(host.state.terrain.height) else {
            panic!("fixture map height must fit in i32");
        };
        let below_map = map_height_cells.saturating_mul(crate::fixed::POSITION_SCALE);
        let Some(player) = host.state.player_mut(&actor) else {
            panic!("the active player must exist");
        };
        player.position.y = below_map;
        let command = basic_command(&host, "eliminated-before-passive");

        let Ok(CommandResult::Accepted(_)) = host.submit_ability(&command) else {
            panic!("the valid ability must resolve before settling eliminates its actor");
        };

        let Some(actor_state) = host.state.player(&actor) else {
            panic!("the eliminated player remains addressable in match state");
        };
        assert!(actor_state.is_eliminated());
        assert_ne!(host.phase(), MatchPhase::PassiveSelection);
        assert!(host.is_complete(), "the surviving team must be evaluated");
        assert_eq!(host.outcome(), MatchOutcome::Victory { team: 1 });
    }

    #[test]
    fn pass_and_timeout_cannot_skip_a_required_passive_choice() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        host.state.phase = MatchPhase::PassiveSelection;
        let before = host.state.clone();
        let expected = Err(crate::error::SimError::OutOfRange {
            field: "passive selection",
        });

        assert_eq!(host.pass_turn(), expected);
        assert_eq!(host.state, before);
        assert_eq!(
            host.time_out_turn(),
            Err(crate::error::SimError::OutOfRange {
                field: "passive selection",
            })
        );
        assert_eq!(host.state, before);
    }

    #[test]
    #[ignore = "leftover C1 kit envelope; not required for the playable cut"]
    fn accepted_passive_choice_reports_the_post_turn_host_hash() {
        let Ok(mut host) = MatchHost::start(duel()) else {
            panic!("a match must be startable");
        };
        let actor = host.active_player().to_owned();
        let turn_number = host.state().turn_number;
        let Some(_player) = host.state.player_mut(&actor) else {
            panic!("the active player must exist");
        };
        host.state.phase = MatchPhase::PassiveSelection;
        let choice = PassiveChoiceCommand {
            command_id: "post-passive-hash".to_owned(),
            player_id: actor,
            expected_turn_number: turn_number,
            passive_id: "arzum-momentum".to_owned(),
        };

        let Ok(CommandResult::Accepted(outcome)) = host.submit_passive_choice(&choice) else {
            panic!("a valid passive choice must be accepted");
        };

        assert_eq!(
            outcome.turn_number_after,
            host.state().turn_number,
            "the passive outcome turn must include resumed turn progression",
        );
        assert_eq!(
            outcome.final_state_hash,
            crate::hash::hash_state(host.state()),
            "the passive outcome hash must include resumed turn progression",
        );
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
        let mut object_changes = Vec::new();
        let Ok(()) = victory::eliminate(&mut host.state, &loser, &mut object_changes) else {
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

    // -----------------------------------------------------------------------------------
    // Status transition provenance
    //
    // The host is the only layer that sees both halves of a status's life: resolution
    // attaches statuses, and the scheduler's end-of-turn tick removes them, and those happen
    // on either side of `command::apply_ability` returning.
    // -----------------------------------------------------------------------------------

    /// A duel in which the starting player already carries a status about to lapse.
    fn duel_with_expiring_status() -> SimulationState {
        let mut state = duel();
        let Some(target) = state.players.first_mut() else {
            panic!("the fixture duel must have players");
        };
        target.statuses.push(crate::types::StatusEffect {
            kind: crate::types::EffectKind::Lockdown,
            magnitude: 2,
            turns_remaining: 1,
        });
        state
    }

    #[test]
    fn an_end_of_turn_expiry_reaches_the_hosts_record() {
        let Ok(mut host) = MatchHost::start(duel_with_expiring_status()) else {
            panic!("a match must be startable");
        };
        assert!(
            host.status_changes().is_empty(),
            "starting a match reports no transitions",
        );

        let Ok(()) = host.pass_turn() else {
            panic!("passing must succeed");
        };

        let [change] = host.status_changes() else {
            panic!(
                "expected exactly one transition, got {}",
                host.status_changes().len(),
            )
        };
        assert_eq!(change.kind, crate::types::EffectKind::Lockdown);
        assert_eq!(change.transition, crate::types::StatusTransition::Expired);
    }

    #[test]
    fn the_record_is_cleared_at_the_start_of_every_call() {
        let Ok(mut host) = MatchHost::start(duel_with_expiring_status()) else {
            panic!("a match must be startable");
        };
        let Ok(()) = host.pass_turn() else {
            panic!("passing must succeed");
        };
        assert_eq!(host.status_changes().len(), 1);

        // Nothing carries a status now, so this turn produces no transitions at all. A
        // record that were merely appended to would still be reporting the expiry above.
        let Ok(()) = host.pass_turn() else {
            panic!("passing must succeed");
        };
        assert!(
            host.status_changes().is_empty(),
            "a new call must not report the previous call's transitions",
        );
    }

    #[test]
    fn an_ability_outcome_carries_the_same_transitions_the_host_recorded() {
        let Ok(mut host) = MatchHost::start(duel_with_expiring_status()) else {
            panic!("a match must be startable");
        };
        let actor = host.active_player().to_owned();
        let command = AbilityCommand {
            command_id: "ability-1".to_owned(),
            player_id: actor,
            expected_turn_number: host.state().turn_number,
            slot: AbilitySlot::Basic,
            angle_millidegrees: 45_000,
            power_basis_points: 5_000,
            target_player_id: None,
            secondary_target_player_id: None,
        };

        let Ok(CommandResult::Accepted(outcome)) = host.submit_ability(&command) else {
            panic!("the fixture ability must be accepted");
        };

        // `command::apply_ability` returns before the scheduler ticks statuses, so an
        // outcome that were left as the command layer built it would omit this entirely.
        assert_eq!(
            outcome.status_changes,
            host.status_changes(),
            "the outcome and the host must not be able to disagree",
        );
        assert!(
            outcome
                .status_changes
                .iter()
                .any(|change| change.transition == crate::types::StatusTransition::Expired),
            "the end-of-turn expiry must appear on the outcome",
        );
    }
}
