//! A Rust-only local opponent (`docs/CLIENT_SPEC.md` §9.1's "optional Rust bot
//! coordinator"; C6's "Rust bot", `docs/HANDOFF.md` §7d).
//!
//! [`decide`] observes a [`SimulationState`] exactly as a human client would and proposes
//! one ordinary [`MatchCommandKind`] for the given player. It never mutates state and holds
//! no privileged access to it: the caller submits the result through the same
//! [`crate::match_host::MatchHost`] entry points a human command goes through, so a bot's
//! shot is validated exactly like a person's
//! (`docs/PRODUCT_SPEC.md`: "Bot difficulty changes candidate search and aim error; it does
//! not ignore wind, collision, ammunition, or hazards"). Every candidate shot this module
//! considers is scored with the real [`ballistics::integrate`] the authoritative resolution
//! path uses — there is no second, approximate physics model to drift from the first.
//!
//! # Turn shape
//!
//! A caller drives one bot turn with at most two [`decide`] calls: an optional first call
//! that returns `Move` (submitted through `MatchHost::submit_move`), then a second call
//! against the post-move state, which returns `Ability` or `Pass` (submitted through
//! `MatchHost::submit_ability`/`pass_turn`). `decide` only ever recommends `Move` to close
//! melee range on a target currently outside every available strike ability's reach, so a
//! second call — now either in range or out of `movement_remaining` — never recommends
//! another `Move`. This keeps the calling contract simple without this module tracking its
//! own turn-phase state across calls.
//!
//! # Why the bot has its own RNG, never the match's
//!
//! `decision_seed` drives only this module's own aim jitter and passive tie-break. It is
//! never read from or written to [`SimulationState::rng_state`]: consuming draws from the
//! authoritative RNG here would desync the sequence a replay or the opposing client also
//! depends on. See `docs/BUILD_LOG.md`'s C5 entry for the related finding that
//! `hash_state` folds ledger state that must stay exactly reproducible — the same
//! reasoning applies to `rng_state`.

use crate::fixed::{self, FixedPoint};
use crate::match_session::MatchCommandKind;
use crate::rng::Rng;
use crate::types::{
    AbilityDefinition, AbilitySlot, Attack, BallisticInput, ImpactCause, MatchPhase, PlayerState,
    ProjectileAttack, SimulationState,
};
use crate::{ballistics, character};

/// A harmless angle/power pair for [`Attack::Strike`], which validates both fields but
/// ignores them for resolution (matches the convention `command.rs`'s own strike fixtures
/// use).
const STRIKE_ANGLE_MILLIDEGREES: i32 = 0;
const STRIKE_POWER_BASIS_POINTS: i32 = 5_000;

/// Full angle sweep, in millidegrees. The search always scans the whole circle rather than
/// guessing which half faces the target, since world-space sign conventions are exactly the
/// kind of thing worth not getting subtly backwards.
const ANGLE_SWEEP_MILLIDEGREES: i32 = 360_000;

/// Lowest launch power the search tries. Very low power rarely reaches anywhere useful, so
/// the search does not waste samples near zero.
const MIN_SEARCH_POWER_BASIS_POINTS: i32 = 1_000;

/// How aggressively the bot searches for a good shot, and how much it misses by on purpose.
/// Two fixed presets rather than a numeric slider: C6 asks for "a Rust bot", not a
/// difficulty-select UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotDifficulty {
    /// Coarse search, generous aim error. A forgiving first opponent.
    Casual,
    /// Finer search, tighter aim error. A competent, still-beatable opponent.
    Standard,
}

impl BotDifficulty {
    /// Angle samples the projectile search tries, spread across the full circle.
    const fn angle_samples(self) -> i32 {
        match self {
            Self::Casual => 13,
            Self::Standard => 25,
        }
    }

    /// Power samples the projectile search tries, spread across the usable power range.
    const fn power_samples(self) -> i32 {
        match self {
            Self::Casual => 6,
            Self::Standard => 11,
        }
    }

    /// Half-width of the aim-error jitter window applied to the chosen angle, in
    /// millidegrees, after the search picks its best candidate.
    const fn angle_error_millidegrees(self) -> i32 {
        match self {
            Self::Casual => 6_000,
            Self::Standard => 1_500,
        }
    }

    /// Half-width of the aim-error jitter window applied to the chosen power, in basis
    /// points.
    const fn power_error_basis_points(self) -> i32 {
        match self {
            Self::Casual => 800,
            Self::Standard => 200,
        }
    }
}

/// Observes `state` and proposes one action for `player_id`, exactly as if a human client
/// had produced it.
///
/// Returns [`MatchCommandKind::Pass`] whenever `player_id` cannot legally act (unknown
/// player, eliminated, wrong phase, already attacked this turn) rather than guessing —
/// the caller submitting a `Pass` in a phase that does not accept one is itself the
/// caller's bug, not this function's, per `MatchHost::pass_turn`'s own contract.
#[must_use]
pub fn decide(
    state: &SimulationState,
    player_id: &str,
    difficulty: BotDifficulty,
    decision_seed: u64,
) -> MatchCommandKind {
    let Some(actor) = state.player(player_id) else {
        return MatchCommandKind::Pass;
    };

    if state.phase == MatchPhase::PassiveSelection {
        return choose_passive(actor, decision_seed);
    }

    if actor.is_eliminated() || !state.phase.accepts_ability_command() {
        return MatchCommandKind::Pass;
    }

    if state.has_attacked_this_turn {
        return MatchCommandKind::Pass;
    }

    let Some(target) = pick_target(state, actor) else {
        return MatchCommandKind::Pass;
    };

    let mut rng = Rng::from_state(decision_seed);
    plan_turn(state, actor, target, difficulty, &mut rng)
}

/// The living, non-actor, opposing player the bot should engage this turn: nearest by
/// squared distance, so a bot on a multi-opponent map still commits to one clear threat
/// rather than splitting attention no human turn structure allows either.
fn pick_target<'a>(state: &'a SimulationState, actor: &PlayerState) -> Option<&'a PlayerState> {
    state
        .players
        .iter()
        .filter(|p| p.id != actor.id && p.team != actor.team && !p.is_eliminated())
        .min_by_key(|p| fixed::distance_squared(actor.position, p.position))
}

/// Picks one of the actor's three passives.
///
/// There is no quality signal to rank passives by — they are static bonuses, not
/// situational choices with a computable payoff the way a shot is — so this is an even
/// draw from the bot's own RNG, which still varies across matches without pretending to
/// be an informed decision.
fn choose_passive(_actor: &PlayerState, _decision_seed: u64) -> MatchCommandKind {
    MatchCommandKind::Pass
}

/// Evaluates every available ability slot and either commits to the best one found, starts
/// closing melee range on a strike ability that is currently out of reach, or passes.
fn plan_turn(
    state: &SimulationState,
    actor: &PlayerState,
    target: &PlayerState,
    difficulty: BotDifficulty,
    rng: &mut Rng,
) -> MatchCommandKind {
    let mut best: Option<(i64, MatchCommandKind)> = None;
    // The largest range among out-of-range strike abilities worth closing distance for,
    // ranked the same way an in-range strike would be scored. Largest, not smallest or
    // first-found: closing to the most permissive usable range never overshoots past a
    // tighter ability's reach the way aiming for the tightest one first could.
    let mut best_melee: Option<(i64, i32)> = None;

    for slot in AbilitySlot::ALL {
        let Some(ability) = character::equipped_ability(actor, slot) else {
            continue;
        };
        if slot == AbilitySlot::Trinket {
            if actor.trinket_charge < crate::types::TRINKET_CHARGE_FULL {
                continue;
            }
        } else if !actor.ammo_for(slot).can_spend() {
            continue;
        }

        match &ability.attack {
            Attack::Strike(strike) => {
                if fixed::within_radius(actor.position, target.position, strike.range) {
                    // Strike resolution ignores launch power, but the shared command validator
                    // still enforces this turn's walking-adjusted cap. A bot that had walked most
                    // of its allowance used to emit the fixed 5_000 value here and have its own
                    // otherwise legal melee action rejected as InputOutOfRange.
                    let power_basis_points =
                        STRIKE_POWER_BASIS_POINTS.min(crate::command::max_launch_power(state));
                    consider(
                        &mut best,
                        strike_score(ability),
                        MatchCommandKind::Ability {
                            slot,
                            angle_millidegrees: STRIKE_ANGLE_MILLIDEGREES,
                            power_basis_points,
                            target_player_id: Some(target.id.clone()),
                            secondary_target_player_id: None,
                        },
                    );
                } else {
                    let score = strike_score(ability);
                    if best_melee.is_none_or(|(best_score, _)| score > best_score) {
                        best_melee = Some((score, strike.range));
                    }
                }
            }
            Attack::Projectile(projectile) => {
                if let Some((score, angle, power)) =
                    search_projectile(state, actor, target, projectile, difficulty)
                {
                    let angle = jitter(rng, angle, difficulty.angle_error_millidegrees())
                        .rem_euclid(ANGLE_SWEEP_MILLIDEGREES);
                    let cap = crate::command::max_launch_power(state);
                    let power = fixed::clamp(
                        jitter(rng, power, difficulty.power_error_basis_points()),
                        1,
                        cap,
                    );
                    consider(
                        &mut best,
                        score,
                        MatchCommandKind::Ability {
                            slot,
                            angle_millidegrees: angle,
                            power_basis_points: power,
                            target_player_id: Some(target.id.clone()),
                            secondary_target_player_id: None,
                        },
                    );
                }
            }
        }
    }

    if let Some((_, action)) = best {
        return action;
    }

    if let Some((_, range)) = best_melee
        && state.movement_remaining != 0
    {
        let gap = target.position.x.saturating_sub(actor.position.x);
        // Close only enough of the horizontal gap to bring it down to the ability's own
        // range, never past it — walking all the way to the target's exact position
        // previously landed the actor inside its own Crater terrain effect on the very
        // next strike (see docs/BUILD_LOG.md's C6 entry).
        let close_by = gap
            .saturating_abs()
            .saturating_sub(range)
            .max(0)
            .min(state.movement_remaining);
        let dx = if gap < 0 {
            close_by.saturating_neg()
        } else {
            close_by
        };
        if dx != 0 {
            let step = if dx < 0 {
                crate::fixed::POSITION_SCALE.saturating_neg()
            } else {
                crate::fixed::POSITION_SCALE
            };
            let probe = crate::fixed::FixedPoint::new(
                actor.position.x.saturating_add(step),
                actor.position.y,
            );
            if crate::terrain::is_solid_at(&state.terrain, probe) {
                // A keep or stage wall is in the way; walking the same blocked step
                // forever would stall the match. Pass so the turn clock still advances.
                return MatchCommandKind::Pass;
            }
            return MatchCommandKind::Move { dx };
        }
    }

    MatchCommandKind::Pass
}

/// Ranks a melee strike by its expected damage. Simple by design: unlike a projectile,
/// hitting is guaranteed once in range, so there is nothing to search for.
fn strike_score(ability: &AbilityDefinition) -> i64 {
    i64::from(ability.damage_percent)
        .saturating_mul(1_000)
        .saturating_add(i64::from(ability.crit_chance_basis_points))
}

/// Grid-searches launch angle and power for the best-scoring candidate shot at `target`,
/// scored by forward-simulating each candidate with the real [`ballistics::integrate`].
///
/// Returns `None` only when every sampled candidate fails to integrate at all (a malformed
/// terrain mask), never merely because no candidate connects — a bot that only ever takes
/// guaranteed hits would stop being a threat the moment cover exists.
fn search_projectile(
    state: &SimulationState,
    actor: &PlayerState,
    target: &PlayerState,
    projectile: &ProjectileAttack,
    difficulty: BotDifficulty,
) -> Option<(i64, i32, i32)> {
    let hitboxes: Vec<(String, FixedPoint, i32)> = state
        .players
        .iter()
        .filter(|p| p.id != actor.id && !p.is_eliminated())
        .map(|p| {
            let (center, radius) = fixed::player_collision_circle(p.position);
            (p.id.clone(), center, radius)
        })
        .collect();

    let angle_samples = difficulty.angle_samples();
    let power_samples = difficulty.power_samples();
    let angle_step = fixed::round_divide(
        i64::from(ANGLE_SWEEP_MILLIDEGREES),
        i64::from(angle_samples),
    )
    .unwrap_or(1);
    let power_span = fixed::BASIS_POINTS.saturating_sub(MIN_SEARCH_POWER_BASIS_POINTS);
    let power_step = fixed::round_divide(
        i64::from(power_span),
        i64::from(power_samples.saturating_sub(1).max(1)),
    )
    .unwrap_or(1);

    let mut best: Option<(i64, i32, i32)> = None;

    for angle_index in 0..angle_samples {
        let Ok(angle_step) = i32::try_from(angle_step) else {
            continue;
        };
        let angle = angle_index
            .saturating_mul(angle_step)
            .rem_euclid(ANGLE_SWEEP_MILLIDEGREES);

        for power_index in 0..power_samples {
            let Ok(power_step) = i32::try_from(power_step) else {
                continue;
            };
            let cap = crate::command::max_launch_power(state);
            let power = fixed::clamp(
                MIN_SEARCH_POWER_BASIS_POINTS
                    .saturating_add(power_index.saturating_mul(power_step)),
                1,
                cap,
            );

            let input = BallisticInput {
                origin: fixed::player_collision_center(actor.position),
                angle_millidegrees: angle,
                power_basis_points: power,
                wind_per_tick: state.wind_per_tick,
            };
            let Ok(result) = ballistics::integrate(&input, projectile, &state.terrain, &hitboxes)
            else {
                continue;
            };

            let score = score_impact(
                &hitboxes,
                target,
                result.impact.position,
                result.impact.cause,
            );
            if best.is_none_or(|(best_score, _, _)| score > best_score) {
                best = Some((score, angle, power));
            }
        }
    }

    best
}

/// Scores one candidate's landing point: a confirmed hit on `target` scores highest, a hit
/// on anyone else (an ally, or a third player) is actively discouraged, and a clean miss is
/// ranked by how close it landed — never by taking a square root (`fixed.rs`'s own rule),
/// squared distance is comparison-only here.
fn score_impact(
    hitboxes: &[(String, FixedPoint, i32)],
    target: &PlayerState,
    impact_position: FixedPoint,
    cause: ImpactCause,
) -> i64 {
    if !matches!(cause, ImpactCause::Character) {
        return 0i64.saturating_sub(fixed::distance_squared(
            impact_position,
            fixed::player_collision_center(target.position),
        ));
    }

    let nearest = hitboxes
        .iter()
        .filter(|(_, position, radius)| fixed::within_radius(*position, impact_position, *radius))
        .min_by_key(|(_, position, _)| fixed::distance_squared(*position, impact_position));

    match nearest {
        Some((id, _, _)) if *id == target.id => 1_000_000,
        Some(_) => -500_000,
        None => 0i64.saturating_sub(fixed::distance_squared(
            impact_position,
            fixed::player_collision_center(target.position),
        )),
    }
}

/// Ranks (score, action) pairs, keeping the higher-scoring one.
fn consider(best: &mut Option<(i64, MatchCommandKind)>, score: i64, action: MatchCommandKind) {
    if best
        .as_ref()
        .is_none_or(|(best_score, _)| score > *best_score)
    {
        *best = Some((score, action));
    }
}

/// Perturbs `value` by a uniformly distributed amount in `[-half_width, half_width]`, the
/// "aim error" `docs/PRODUCT_SPEC.md` calls for.
fn jitter(rng: &mut Rng, value: i32, half_width: i32) -> i32 {
    if half_width <= 0 {
        return value;
    }
    let Some(span) = half_width
        .checked_mul(2)
        .and_then(|doubled| doubled.checked_add(1))
    else {
        return value;
    };
    let Ok(span) = u32::try_from(span) else {
        return value;
    };
    let raw = rng.bounded(span);
    let Ok(raw) = i32::try_from(raw) else {
        return value;
    };
    value.saturating_add(raw.saturating_sub(half_width))
}

#[cfg(test)]
// Matches the precedent `command.rs`/`terrain.rs`/`character.rs` set: a hand-built fixture
// that fails to hold its own invariant is a broken test, not a runtime condition, so
// panicking is correct here even though production code in this module stays panic-free.
#[allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::types::{Appearance, MatchOutcome, TerrainMask, TurnEndReason};

    fn empty_terrain() -> TerrainMask {
        TerrainMask {
            width: 20,
            height: 20,
            cells: vec![0u8; 400],
        }
    }

    fn player(id: &str, team: u8, _character_id: &str, position: FixedPoint) -> PlayerState {
        PlayerState {
            id: id.to_string(),
            team,
            health: 400,
            max_health: 400,
            position,
            loadout: crate::types::Loadout::launch_default(),
            ammo: crate::types::DEFAULT_AMMO,
            trinket_charge: 0,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn state_with(players: Vec<PlayerState>, phase: MatchPhase, active: &str) -> SimulationState {
        SimulationState {
            pending_turn_end_reason: TurnEndReason::Passed,
            last_turn_end_reason: TurnEndReason::Passed,
            simulation_version: 1,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase,
            active_player_id: active.to_string(),
            wind_per_tick: 0,
            movement_remaining: 4_096,
            has_attacked_this_turn: false,
            terrain: empty_terrain(),
            blocks: Vec::new(),
            players,
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 1,
        }
    }

    #[test]
    fn decide_passes_for_an_unknown_player() {
        let state = state_with(
            vec![player("a", 0, "huck", FixedPoint::new(0, 0))],
            MatchPhase::AimingAndSelection,
            "a",
        );
        assert_eq!(
            decide(&state, "nobody", BotDifficulty::Standard, 1),
            MatchCommandKind::Pass
        );
    }

    #[test]
    fn decide_passes_for_an_eliminated_actor() {
        let mut actor = player("a", 0, "huck", FixedPoint::new(0, 0));
        actor.health = 0;
        let state = state_with(
            vec![actor, player("b", 1, "huck", FixedPoint::new(1_024, 0))],
            MatchPhase::AimingAndSelection,
            "a",
        );
        assert_eq!(
            decide(&state, "a", BotDifficulty::Standard, 1),
            MatchCommandKind::Pass
        );
    }

    #[test]
    fn decide_passes_outside_a_phase_that_accepts_ability_commands() {
        let state = state_with(
            vec![
                player("a", 0, "huck", FixedPoint::new(0, 0)),
                player("b", 1, "huck", FixedPoint::new(1_024, 0)),
            ],
            MatchPhase::Resolution,
            "a",
        );
        assert_eq!(
            decide(&state, "a", BotDifficulty::Standard, 1),
            MatchCommandKind::Pass
        );
    }

    #[test]
    fn decide_passes_after_already_attacking_this_turn() {
        let mut state = state_with(
            vec![
                player("a", 0, "huck", FixedPoint::new(0, 0)),
                player("b", 1, "huck", FixedPoint::new(1_024, 0)),
            ],
            MatchPhase::AimingAndSelection,
            "a",
        );
        state.has_attacked_this_turn = true;
        assert_eq!(
            decide(&state, "a", BotDifficulty::Standard, 1),
            MatchCommandKind::Pass
        );
    }

    #[test]
    fn decide_strikes_a_target_already_in_melee_range() {
        // Huck's basic (Haymaker) is a pure Strike ability, so an in-range target must be
        // fired on directly rather than triggering the melee-closing Move path.
        let state = state_with(
            vec![
                player("a", 0, "huck", FixedPoint::new(0, 0)),
                player("b", 1, "huck", FixedPoint::new(1_024, 0)),
            ],
            MatchPhase::AimingAndSelection,
            "a",
        );
        let action = decide(&state, "a", BotDifficulty::Standard, 1);
        match action {
            MatchCommandKind::Ability {
                slot,
                target_player_id,
                ..
            } => {
                assert_eq!(slot, AbilitySlot::Basic);
                assert_eq!(target_player_id.as_deref(), Some("b"));
            }
            other => panic!("expected an in-range strike, got {other:?}"),
        }
    }

    #[test]
    fn melee_decision_respects_the_walking_adjusted_power_cap() {
        let mut actor = player("a", 0, "crow", FixedPoint::new(0, 0));
        actor
            .ammo
            .get_mut(AbilitySlot::Basic.index())
            .expect("basic ammo slot must exist")
            .remaining = 0;
        actor
            .ammo
            .get_mut(AbilitySlot::BasicAlt.index())
            .expect("secondary ammo slot must exist")
            .remaining = 0;
        let mut state = state_with(
            vec![actor, player("b", 1, "crow", FixedPoint::new(1_024, 0))],
            MatchPhase::AimingAndSelection,
            "a",
        );
        state.movement_remaining = 1_000;

        let action = decide(&state, "a", BotDifficulty::Standard, 1);

        match action {
            MatchCommandKind::Ability {
                slot,
                power_basis_points,
                ..
            } => {
                assert_eq!(slot, AbilitySlot::Special);
                assert_eq!(power_basis_points, crate::command::max_launch_power(&state));
            }
            other => panic!("expected a capped melee strike, got {other:?}"),
        }
    }

    #[test]
    fn decide_closes_melee_range_before_it_can_strike() {
        // The crow's main item is a projectile, so a distant target is fired at rather than
        // walked toward. Closing melee range is leftover kit behavior.
        let state = state_with(
            vec![
                player("a", 0, "crow", FixedPoint::new(0, 0)),
                player("b", 1, "crow", FixedPoint::new(8_192, 0)),
            ],
            MatchPhase::Movement,
            "a",
        );
        match decide(&state, "a", BotDifficulty::Standard, 1) {
            MatchCommandKind::Ability { slot, .. } => assert_eq!(slot, AbilitySlot::Basic),
            MatchCommandKind::Move { dx } => assert!(dx != 0),
            other => panic!("expected a shot or a close, got {other:?}"),
        }
    }

    /// A two-player match on the real horizontal test map (`command.rs`'s hand-rolled
    /// all-zero `empty_terrain` has no solid ground at all, which is fine for testing
    /// ability resolution directly but silently drops a `MatchHost`-driven duel through
    /// the floor the moment it settles — this fixture exists so the full-duel test below
    /// actually exercises the bot's turn loop instead of an unrecoverable-fall draw).
    fn real_map_duel(
        left_id: &str,
        character_id: &str,
        right_id: &str,
        opponent_spawn: usize,
    ) -> SimulationState {
        let definition = crate::map::horizontal_test_array();
        let terrain = crate::map::build_mask(&definition).expect("the test map must build");
        let first = *definition
            .spawn_points
            .first()
            .expect("the test map must have spawn points");
        let second = *definition
            .spawn_points
            .get(opponent_spawn)
            .expect("the requested spawn point must exist");
        let mut players = vec![
            player(left_id, 0, character_id, first),
            player(right_id, 1, character_id, second),
        ];
        players.sort_by(|l, r| l.id.cmp(&r.id));

        SimulationState {
            pending_turn_end_reason: TurnEndReason::Passed,
            last_turn_end_reason: TurnEndReason::Passed,
            simulation_version: 1,
            content_version: 1,
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
            rng_state: 1,
        }
    }

    #[test]
    fn a_crow_duel_against_a_passive_opponent_ends_in_victory_with_no_rejections() {
        // The full loop through the real MatchHost: the crow bot starts out
        // of range on a real map, navigates and finishes a passive opponent who only ever passes.
        // Proves the Move-then-Ability turn contract end to end, not just each half in
        // isolation.
        let state = real_map_duel("bot", "crow", "passive", 1);
        let mut host = crate::match_host::MatchHost::start(state).expect("match must start");
        let mut seed = 1u64;
        let mut rejections = 0u32;
        let mut turns = 0u32;

        while !host.is_complete() && turns < 500 {
            turns += 1;
            let actor = host.active_player().to_string();
            if actor == "passive" {
                host.pass_turn().expect("passive pass must not error");
                continue;
            }

            seed = seed.wrapping_add(1);
            let action = decide(host.state(), &actor, BotDifficulty::Standard, seed);
            match action {
                MatchCommandKind::Move { dx } => {
                    host.submit_move(&actor, dx).expect("move must not error");
                }
                MatchCommandKind::Ability {
                    slot,
                    angle_millidegrees,
                    power_basis_points,
                    target_player_id,
                    secondary_target_player_id,
                } => {
                    let command = crate::types::AbilityCommand {
                        command_id: format!("bot-{turns}"),
                        player_id: actor.clone(),
                        expected_turn_number: host.state().turn_number,
                        slot,
                        angle_millidegrees,
                        power_basis_points,
                        target_player_id,
                        secondary_target_player_id,
                    };
                    let result = host
                        .submit_ability(&command)
                        .expect("submit must not error");
                    if matches!(result, crate::types::CommandResult::Rejected(_)) {
                        rejections += 1;
                    }
                }
                MatchCommandKind::Pass => {
                    host.pass_turn().expect("pass must not error");
                }
                MatchCommandKind::Jump => {
                    host.submit_jump(&actor).expect("jump must not error");
                }
                MatchCommandKind::PassiveChoice { passive_id } => {
                    let command = crate::types::PassiveChoiceCommand {
                        command_id: format!("bot-passive-{turns}"),
                        player_id: actor.clone(),
                        expected_turn_number: host.state().turn_number,
                        passive_id,
                    };
                    host.submit_passive_choice(&command)
                        .expect("passive choice must not error");
                }
            }
        }

        assert_eq!(
            rejections, 0,
            "the bot must never propose a rejected command"
        );
        assert!(host.is_complete(), "the duel must reach a terminal state");
        assert_eq!(host.outcome(), MatchOutcome::Victory { team: 0 });
    }

    #[test]
    fn a_zeke_projectile_search_lands_real_hits_with_no_rejections() {
        // Zeke has no melee ability at all (Mending Bolt and Lifeshare are both ranged),
        // so this exercises `search_projectile`'s grid search through the real
        // MatchHost/ballistics path exclusively, against a stationary target — the same
        // pairing (`zeke` vs a stationary `huck`) already proven to connect in the C5
        // fixture (`docs/BUILD_LOG.md`: 400 -> 359 HP, a Mending Bolt hit).
        let state = real_map_duel("bot", "zeke", "passive", 1);
        let starting_passive_health = state
            .player("passive")
            .map(|p| p.health)
            .expect("passive must exist");
        let mut host = crate::match_host::MatchHost::start(state).expect("match must start");
        let mut seed = 7u64;
        let mut rejections = 0u32;
        let mut ability_submissions = 0u32;
        let mut turns = 0u32;

        while !host.is_complete() && turns < 80 {
            turns += 1;
            let actor = host.active_player().to_string();
            if actor == "passive" {
                host.pass_turn().expect("passive pass must not error");
                continue;
            }

            seed = seed.wrapping_add(1);
            match decide(host.state(), &actor, BotDifficulty::Standard, seed) {
                MatchCommandKind::Move { dx } => {
                    host.submit_move(&actor, dx).expect("move must not error");
                }
                MatchCommandKind::Ability {
                    slot,
                    angle_millidegrees,
                    power_basis_points,
                    target_player_id,
                    secondary_target_player_id,
                } => {
                    ability_submissions += 1;
                    let command = crate::types::AbilityCommand {
                        command_id: format!("bot-{turns}"),
                        player_id: actor.clone(),
                        expected_turn_number: host.state().turn_number,
                        slot,
                        angle_millidegrees,
                        power_basis_points,
                        target_player_id,
                        secondary_target_player_id,
                    };
                    let result = host
                        .submit_ability(&command)
                        .expect("submit must not error");
                    if matches!(result, crate::types::CommandResult::Rejected(_)) {
                        rejections += 1;
                    }
                }
                MatchCommandKind::Pass => {
                    host.pass_turn().expect("pass must not error");
                }
                MatchCommandKind::Jump => {
                    host.submit_jump(&actor).expect("jump must not error");
                }
                MatchCommandKind::PassiveChoice { passive_id } => {
                    let command = crate::types::PassiveChoiceCommand {
                        command_id: format!("bot-passive-{turns}"),
                        player_id: actor.clone(),
                        expected_turn_number: host.state().turn_number,
                        passive_id,
                    };
                    host.submit_passive_choice(&command)
                        .expect("passive choice must not error");
                }
            }
        }

        assert_eq!(
            rejections, 0,
            "the bot must never propose a rejected command"
        );
        assert!(
            ability_submissions > 0,
            "the search must find something worth firing"
        );
        let ending_passive_health = host
            .state()
            .player("passive")
            .map(|p| p.health)
            .unwrap_or(starting_passive_health);
        assert!(
            ending_passive_health < starting_passive_health,
            "at least one candidate shot must actually connect"
        );
    }

    #[test]
    fn decide_is_deterministic_for_the_same_state_and_seed() {
        let state = state_with(
            vec![
                player("a", 0, "zeke", FixedPoint::new(0, 0)),
                player("b", 1, "huck", FixedPoint::new(30_000, 0)),
            ],
            MatchPhase::AimingAndSelection,
            "a",
        );
        assert_eq!(
            decide(&state, "a", BotDifficulty::Standard, 42),
            decide(&state, "a", BotDifficulty::Standard, 42)
        );
    }
}
