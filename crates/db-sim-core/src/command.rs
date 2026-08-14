//! Command validation and application.
//!
//! This is the security boundary described by `SECURITY_BASELINE.md` §2, §5, and §6: the
//! one place untrusted client input meets authoritative state. Every field on every
//! command is treated as adversarial input until the checks below have passed. Nothing
//! here trusts a client-declared identity, range, or gauge state — every fact is
//! re-derived from `SimulationState` and the frozen `character` roster.
//!
//! # Validation is total before any mutation
//!
//! [`apply_ability`] and [`apply_passive_choice`] both run every check in
//! [`validate_ability`] (or the equivalent passive-choice checks) against the
//! *pre-mutation* state, then perform all mutation on a cloned working copy that is only
//! written back to `*state` once every fallible step inside has succeeded. This is what
//! makes "either the command fully applies, or state is byte-identical to before" true
//! without hand-rolled rollback for each field touched during resolution.
//!
//! # Scope of ability resolution (read before extending)
//!
//! `types.rs` defines a closed vocabulary of 21 [`EffectKind`] variants driving 24
//! characters' kits, and per `docs/MODULE_OWNERSHIP.md` this module may not invent new
//! type-level contract beyond what `types.rs` already states. Implementing full,
//! character-faithful resolution for every effect (embedded-knife chain detonation in
//! deterministic sequence order, Arzum's random second target, Numa's HP-threshold pull
//! direction, persistent turret spawning, and so on) would mean inventing gameplay
//! behavior that `character.rs`'s own authors flag throughout their file as unresolved
//! ("NOT specified", "Discrepancy noted", "Confirm") — precisely the kind of guess
//! `docs/MODULE_OWNERSHIP.md` rule 5 says costs more than stopping to ask. Blocking on all
//! of that would leave the security-critical validation and replay-protection machinery
//! unimplemented, which is the part every other module and the room process actually
//! depends on.
//!
//! So this module implements, generically and from the type contract alone (never from a
//! character-specific special case):
//!
//! - The full 11-step [`validate_ability`] order, exactly as specified, short-circuiting.
//! - Atomic apply-or-reject for both ability commands and passive choices.
//! - Direct-hit damage resolution: [`Attack::Projectile`] via `ballistics::integrate`
//!   (single nearest living, non-actor character within one body width of the ballistic
//!   impact point takes the hit); [`Attack::Strike`] via a direct reach check against an
//!   explicit target, or an area effect centred on the actor when no target is given
//!   (covers both "hit the one thing I aimed at" and "push everyone within N BW" kits).
//! - Crit rolls, using the ability's own `crit_chance_basis_points` against the seeded
//!   match RNG, independently per `strikes_per_turn` iteration.
//! - Backlash from [`StrikeAttack::self_damage`]: applied unconditionally, computed from
//!   pre-mutation health so it lands the same turn as (and independently of) target
//!   damage, and never special-cased to spare the actor from elimination.
//! - Terrain reshaping via `terrain::apply_operation` for any [`TerrainProfile::Crater`]
//!   or [`TerrainProfile::Dig`] on the resolved attack, using [`MaterialMask::SOFT`] —
//!   nothing in the launch roster asks for [`MaterialMask::ALL`] (Breach Pick only).
//! - **Every effect on the resolved ability**, in declaration order, via
//!   [`resolve::resolve_effect`] (`todolist.md` P1): after the direct attack and its own
//!   terrain reshaping resolve, this module builds a [`resolve::ResolveContext`] once and
//!   feeds it `ability.effects` in the order the character definition declares them —
//!   order is load-bearing, since a later effect (Natomica's `WallImpact`) must observe
//!   where an earlier one (`Push`) actually left its target. This is the **only** place
//!   any effect resolves; there is no second, inline copy of `Heal`, `HealthTransfer`, or
//!   `SelfDamage` here any more (they moved to `resolve::support` in the same change that
//!   wired this loop in, so there is exactly one resolution path for every `EffectKind`).
//! - Gauge award per `CHARACTERS.md` §2's per-action rates, through
//!   [`PlayerState::add_gauge`] so the per-action and full-gauge caps are enforced by the
//!   single already-reviewed implementation of that cap rather than a second copy of it.
//!   The "dealt" rate is still computed only from the direct-hit resolution above (no
//!   launch ability's effects add further direct damage, so this is unobserved today —
//!   flagged as a judgement call rather than silently assumed correct for a future
//!   ability that does); the "taken" and "healed" rates are read back from the itemized
//!   damage map after every effect has resolved, since `Heal`/`HealthTransfer`/
//!   `SelfDamage` no longer return their totals directly to this module.
//! - Sorted, idempotent `processed_command_ids` insertion.
//!
//! **Deliberately out of scope**, and left for a follow-up once the corresponding data
//! exists: splash-damage falloff from the *direct* ballistic/strike hit
//! (`DamageEvent::splash` from that hit is always `0` here — `fixed.rs` forbids `sqrt`, so
//! a distance-based falloff needs a formula that isn't specified anywhere in
//! `CHARACTERS.md`, and inventing one is exactly the guess this module's rules warn
//! against; note that `EffectKind::ChainDetonate`'s own splash damage *does* now apply,
//! since it resolves through `resolve::resolve_effect` like every other effect). None of
//! this affects the security-critical surface: replay protection,
//! turn/phase/gauge/authorization checks, and atomicity are complete and covered by the
//! tests below.
//!
//! # Cross-module dependencies still in flight
//!
//! At the time this file was written, `hash.rs` and `ballistics.rs` are placeholder
//! stubs (`docs/MODULE_OWNERSHIP.md` lists both as in-flight implementation tasks). Every
//! call below to `hash::hash_state` and `ballistics::integrate` is written against the
//! fixed cross-module signature given to every concurrent task; this file cannot compile
//! end-to-end until those land, which is expected, not a defect here
//! (`docs/MODULE_OWNERSHIP.md` rule 4).

use crate::character;
use crate::fixed::{self, BODY_WIDTH, FixedPoint};
use crate::hash;
use crate::resolve;
use crate::rng::Rng;
use crate::types::*;
use std::collections::BTreeMap;

/// Gauge gained per point of damage the actor deals to another character this action, in
/// hundredths (`CHARACTERS.md` §2: "+0.40 per point of damage dealt").
const GAUGE_PER_DAMAGE_DEALT: u16 = 40;

/// Gauge gained per point of backlash damage the actor takes this action, in hundredths
/// (`CHARACTERS.md` §2: "+0.25 per point of damage taken").
///
/// Scoped to *this action's* backlash, not damage taken on other players' turns — gauge
/// gained from being attacked is a turn-scheduler concern (it happens on someone else's
/// command), not something `apply_ability` can observe or award.
const GAUGE_PER_DAMAGE_TAKEN: u16 = 25;

/// Gauge gained per point healed by the actor this action, in hundredths
/// (`CHARACTERS.md` §2: "+0.30 per point, ally healed").
const GAUGE_PER_HEALED: u16 = 30;

/// Rejection-sampling bound for a crit roll, one basis point per unit
/// (`fixed::BASIS_POINTS`, restated as a `u32` literal because [`Rng::bounded`] takes
/// `u32` and `BASIS_POINTS` is `i32`).
const CRIT_ROLL_BOUND: u32 = 10_000;

/// Terrain materials an ordinary ability detonation is permitted to remove.
///
/// [`MaterialMask::ALL`] (which also removes `ReinforcedStone`) is reserved for the
/// Breach Pick per `CHARACTERS.md` §6; no launch-roster ability asks for it, so every
/// terrain op this module builds uses the ordinary [`MaterialMask::SOFT`].
const DESTRUCTIBLE_TERRAIN: MaterialMask = MaterialMask::SOFT;

/// Validates an [`AbilityCommand`] against `state`, short-circuiting on the first failure
/// in the exact order `SECURITY_BASELINE.md` §5 and this module's task brief specify.
///
/// The order matters and is load-bearing, not cosmetic: [`CommandRejection::DuplicateCommand`]
/// is checked before anything else touches player or gauge state, so a replayed command can
/// never re-apply damage or re-consume gauge even in a build where every later check would
/// otherwise pass. Returns the resolved character and ability on success so callers do not
/// have to repeat the roster lookup.
///
/// # Errors
///
/// Returns the first [`CommandRejection`] encountered, in validation order.
pub fn validate_ability(
    state: &SimulationState,
    command: &AbilityCommand,
) -> Result<(&'static CharacterDefinition, &'static AbilityDefinition), CommandRejection> {
    // 1. Replay protection, unconditionally first (see doc comment above).
    if state.has_processed(&command.command_id) {
        return Err(CommandRejection::DuplicateCommand);
    }

    // 2. Player must exist and be alive. A claimed id that matches no player is treated
    // the same as an eliminated one: neither can legally act, and this is the rejection
    // the task brief names for both cases.
    let Some(player) = state.player(&command.player_id) else {
        return Err(CommandRejection::PlayerEliminated);
    };
    if player.is_eliminated() {
        return Err(CommandRejection::PlayerEliminated);
    }

    // 3. Turn ownership. A mismatch here is a client attempting to act out of turn —
    // a security event (`CommandRejection::is_security_event`).
    if command.player_id != state.active_player_id {
        return Err(CommandRejection::NotActivePlayer);
    }

    // 4. Phase gate. `PassiveSelection` deliberately excludes ability commands so the
    // one-time passive choice cannot be skipped by firing instead.
    if !state.phase.accepts_ability_command() {
        return Err(CommandRejection::WrongPhase);
    }

    // 5. Turn version. Catches a client acting on stale or speculative state.
    if command.expected_turn_number != state.turn_number {
        return Err(CommandRejection::TurnVersionMismatch);
    }

    // 6. Character lookup against the frozen roster. An id with no match is a forged or
    // corrupted claim — a security event.
    let Some(character) = character::find(&player.character_id) else {
        return Err(CommandRejection::UnknownCharacter);
    };

    // 7. The character must actually have an ability in the claimed slot (e.g. only
    // Aleph has `BasicAlt`). A security event: a legitimate client never sends this.
    let Some(ability) = character.ability(command.slot) else {
        return Err(CommandRejection::AbilityNotAvailable);
    };

    // 8. Special requires (and will consume) a full gauge.
    if command.slot.consumes_gauge() && !player.special_ready() {
        return Err(CommandRejection::GaugeNotReady);
    }

    // 9. One attack per turn.
    if state.has_attacked_this_turn {
        return Err(CommandRejection::AlreadyAttacked);
    }

    // 10. Input ranges, rejected rather than clamped: a client sending an out-of-range
    // value is cheating or broken, and clamping would hide the difference between the
    // two. Bounds per the task brief: angle in [0, 360_000), power in [0, 10_000].
    if command.angle_millidegrees < 0 || command.angle_millidegrees >= 360_000 {
        return Err(CommandRejection::InputOutOfRange);
    }
    if command.power_basis_points < 0 || command.power_basis_points > 10_000 {
        return Err(CommandRejection::InputOutOfRange);
    }

    // 11. Any target the command names must exist, be alive, and (for two-target
    // abilities) be distinct from the other target.
    validate_targets(state, command)?;

    Ok((character, ability))
}

/// Validates the targets named on an [`AbilityCommand`], if any.
///
/// `types.rs` gives `AbilityCommand` two optional target fields without a flag saying
/// which abilities require one — that per-ability requirement lives in the not-yet-fully
/// specified effect vocabulary (see the module doc comment's scope note). What this
/// function *can* enforce generically, and does: any target the client actually named
/// must be a real, living player, and a second target must not simply repeat the first
/// (a two-target ability naming the same player twice, e.g. "throw Huck's target onto
/// themselves", is not a meaningful action).
fn validate_targets(
    state: &SimulationState,
    command: &AbilityCommand,
) -> Result<(), CommandRejection> {
    if let Some(target_id) = &command.target_player_id {
        let Some(target) = state.player(target_id) else {
            return Err(CommandRejection::InvalidTarget);
        };
        if target.is_eliminated() {
            return Err(CommandRejection::InvalidTarget);
        }
    }

    if let Some(secondary_id) = &command.secondary_target_player_id {
        let Some(secondary) = state.player(secondary_id) else {
            return Err(CommandRejection::InvalidTarget);
        };
        if secondary.is_eliminated() {
            return Err(CommandRejection::InvalidTarget);
        }
        if command.target_player_id.as_deref() == Some(secondary_id.as_str()) {
            return Err(CommandRejection::InvalidTarget);
        }
    }

    Ok(())
}

/// Validates and applies an [`AbilityCommand`].
///
/// Runs every check in [`validate_ability`] first; a rejection there leaves `state`
/// untouched. On acceptance, resolution happens on a cloned working copy and `*state` is
/// only overwritten once every fallible step has succeeded — see the module doc comment
/// for why this is what makes application atomic in effect without per-field rollback.
pub fn apply_ability(state: &mut SimulationState, command: &AbilityCommand) -> CommandResult {
    let (_character, ability) = match validate_ability(state, command) {
        Ok(pair) => pair,
        Err(rejection) => return CommandResult::Rejected(rejection),
    };

    let mut working = state.clone();
    match resolve_ability(&mut working, command, ability) {
        Ok(outcome) => {
            *state = working;
            CommandResult::Accepted(Box::new(outcome))
        }
        // A failure inside resolution (currently only a ballistics integration error —
        // see the call site below) means the command cannot be legally resolved against
        // this board state. There is no dedicated `CommandRejection` variant for an
        // internal resolution fault, and this module may not add one (`types.rs` is
        // frozen), so it is reported as `InvalidTarget`: the closest existing meaning
        // ("this attack, against this target/configuration, cannot be resolved") without
        // inventing new contract. `working` is discarded, so `state` is untouched.
        Err(rejection) => CommandResult::Rejected(rejection),
    }
}

/// Performs the mutating half of [`apply_ability`] on `state`, which the caller has
/// already cloned so a failure here can be discarded without touching the real state.
fn resolve_ability(
    state: &mut SimulationState,
    command: &AbilityCommand,
    ability: &'static AbilityDefinition,
) -> Result<CommandOutcome, CommandRejection> {
    let turn_before = state.turn_number;

    // Consuming the gauge and marking the attack are unconditional consequences of a
    // validated command being committed, independent of what the attack goes on to hit.
    if command.slot.consumes_gauge()
        && let Some(actor) = state.player_mut(&command.player_id)
    {
        actor.special_gauge = 0;
    }
    state.has_attacked_this_turn = true;

    let actor_position = state
        .player(&command.player_id)
        // Re-validated defensively: `validate_ability` already proved this player exists
        // in the pre-clone state, and cloning preserves it. This can only fail if that
        // invariant is somehow broken, in which case failing the command is correct.
        .ok_or(CommandRejection::PlayerEliminated)?
        .position;

    let mut rng = Rng::from_state(state.rng_state);
    let mut samples: Vec<BallisticSample> = Vec::new();
    let mut impact: Option<BallisticImpact> = None;
    let mut terrain_ops: Vec<TerrainOperation> = Vec::new();
    let mut terrain_cells_removed: u32 = 0;
    let mut damage_by_player: BTreeMap<String, DamageEvent> = BTreeMap::new();
    let mut objects_created: Vec<PersistentObject> = Vec::new();
    let mut dealt_to_others: u32 = 0;
    // Where the attack itself landed — the origin `ResolveContext::impact_point` gives
    // every effect below. Defaults to the actor's own position so a targetless area
    // ability (Natomica's Repulse with no target named) centres its effects on the caster,
    // matching `resolve_strike_point`'s own no-target reading.
    let mut impact_point = actor_position;

    match ability.attack {
        Attack::Projectile(projectile) => {
            let strikes = ability.strikes_per_turn.max(1);
            for _ in 0..strikes {
                let input = BallisticInput {
                    origin: actor_position,
                    angle_millidegrees: command.angle_millidegrees,
                    power_basis_points: command.power_basis_points,
                    wind_per_tick: state.wind_per_tick,
                };
                // Snapshot every living character as a ballistics hitbox. Recomputed each
                // iteration of a multi-strike ability (Karl's three-hit basic) so a target
                // eliminated by an earlier strike this turn is correctly excluded from the
                // next one.
                //
                // THE SHOOTER IS EXCLUDED. `ballistics::integrate` checks every character it
                // is given uniformly, with no notion of who fired — its own docs say so. A
                // projectile launched from inside the shooter's own hitbox therefore
                // terminated on tick 1 with `ImpactCause::Character`, every single shot, so
                // nothing in the game could ever hit anything. Neither module was wrong on
                // its own; the contract between them was, which is why only an end-to-end
                // match surfaced it.
                let character_hitboxes: Vec<(String, FixedPoint, i32)> = state
                    .players
                    .iter()
                    .filter(|p| !p.is_eliminated() && p.id != command.player_id)
                    .map(|p| (p.id.clone(), p.position, BODY_WIDTH))
                    .collect();

                let result = crate::ballistics::integrate(
                    &input,
                    &projectile,
                    &state.terrain,
                    &character_hitboxes,
                )
                .map_err(|_| CommandRejection::InvalidTarget)?;
                let impact_position = result.impact.position;
                let hit_character = matches!(result.impact.cause, ImpactCause::Character);
                samples.extend(result.samples);
                impact = Some(result.impact);
                // Overwritten on every iteration of a multi-strike projectile, same as
                // `impact` above — the effects loop below sees only the final strike's
                // landing point, matching how `impact`/`terrain_ops` already behave.
                impact_point = impact_position;

                if hit_character {
                    // Direct hit: nearest living, non-actor character within one body
                    // width of the ballistic impact point. Splash to *other* nearby
                    // characters is intentionally not modelled here — see the module doc
                    // comment's scope note on splash falloff.
                    let direct_target = state
                        .players
                        .iter()
                        .filter(|p| p.id != command.player_id && !p.is_eliminated())
                        .filter(|p| fixed::within_radius(p.position, impact_position, BODY_WIDTH))
                        .min_by_key(|p| fixed::distance_squared(p.position, impact_position))
                        .map(|p| p.id.clone());

                    if let Some(target_id) = direct_target {
                        let is_crit = roll_crit(&mut rng, ability.crit_chance_basis_points);
                        let hp = resolved_damage_hp(ability, is_crit);
                        let actual = deal_damage(
                            state,
                            &mut damage_by_player,
                            &target_id,
                            hp,
                            DamageField::Direct,
                            is_crit,
                        );
                        dealt_to_others = dealt_to_others.saturating_add(u32::from(actual));
                    }
                }

                apply_terrain_profile(
                    state,
                    projectile.terrain,
                    actor_position,
                    impact_position,
                    &mut terrain_ops,
                    &mut terrain_cells_removed,
                );
            }
        }
        Attack::Strike(strike) => {
            let strike_point = resolve_strike_point(state, command, strike, actor_position)?;
            impact_point = strike_point;

            let targets: Vec<String> = if let Some(target_id) = &command.target_player_id {
                vec![target_id.clone()]
            } else {
                // No explicit target: an area effect centred on `strike_point` (which
                // `resolve_strike_point` sets to the actor's own position in this case),
                // using the ability's own reach as the area radius — this is exactly how
                // Natomica's Repulse ("push all enemies within 10 BW") is encoded: a
                // `StrikeAttack` whose `range` field *is* the 10 BW push radius.
                state
                    .players
                    .iter()
                    .filter(|p| p.id != command.player_id && !p.is_eliminated())
                    .filter(|p| fixed::within_radius(p.position, strike_point, strike.range))
                    .map(|p| p.id.clone())
                    .collect()
            };

            let strikes = ability.strikes_per_turn.max(1);
            for _ in 0..strikes {
                for target_id in &targets {
                    let is_crit = roll_crit(&mut rng, ability.crit_chance_basis_points);
                    let hp = resolved_damage_hp(ability, is_crit);
                    let actual = deal_damage(
                        state,
                        &mut damage_by_player,
                        target_id,
                        hp,
                        DamageField::Direct,
                        is_crit,
                    );
                    dealt_to_others = dealt_to_others.saturating_add(u32::from(actual));
                }
            }

            apply_terrain_profile(
                state,
                strike.terrain,
                actor_position,
                strike_point,
                &mut terrain_ops,
                &mut terrain_cells_removed,
            );
        }
    }

    // Backlash from the attack's own `StrikeAttack::self_damage` field (not an
    // `EffectKind`, so it is not part of the effects loop below) resolves simultaneously
    // with target damage and is never gated on whether the actor survived hitting anyone
    // else — both are computed from independent pre-mutation reads and applied
    // unconditionally, so there is no ordering in which one could suppress the other.
    apply_backlash(state, ability, &command.player_id, &mut damage_by_player);

    // Every effect on the resolved ability, in declaration order — the P1 wiring. Order is
    // load-bearing: a later effect must observe what an earlier one already did (Natomica's
    // `WallImpact` reading where `Push` left the target is the canonical example), which is
    // exactly why one `ResolveContext` is built once and threaded through every iteration
    // rather than rebuilt per effect. A resolver failure here means this action cannot be
    // legally resolved against the current board state; mapped to `InvalidTarget` for the
    // same reason the ballistics failure above is (no dedicated `CommandRejection` variant
    // exists for an internal resolution fault, and `types.rs` is frozen). `working` (this
    // whole `state`) is discarded by the caller on any `Err`, so a failure partway through
    // this loop — even after damage has already landed in this clone — leaves the real
    // state untouched; see `effect_resolution_failure_leaves_state_untouched` below.
    {
        let mut ctx = resolve::ResolveContext {
            state: &mut *state,
            rng: &mut rng,
            actor_id: command.player_id.as_str(),
            primary_target_id: command.target_player_id.as_deref(),
            secondary_target_id: command.secondary_target_player_id.as_deref(),
            impact_point,
            damage: &mut damage_by_player,
            terrain_ops: &mut terrain_ops,
            objects_created: &mut objects_created,
            terrain_cells_removed: &mut terrain_cells_removed,
        };
        for effect in ability.effects {
            resolve::resolve_effect(&mut ctx, effect)
                .map_err(|_| CommandRejection::InvalidTarget)?;
        }
    }

    // `Heal`, `HealthTransfer`, and `SelfDamage` no longer return their totals directly to
    // this module (they resolve inside the loop above, alongside every other effect), so
    // the itemized damage map — the single source of truth every resolver in this action
    // wrote into — is summed instead. `healed`/`backlash` are written only by those three
    // effects and by `apply_backlash` just above, so this sum is exact, not an
    // approximation.
    let healed_total: u32 = damage_by_player
        .values()
        .map(|event| u32::from(event.healed))
        .sum();
    let backlash_total: u32 = damage_by_player
        .values()
        .map(|event| u32::from(event.backlash))
        .sum();

    award_gauge(
        state,
        &command.player_id,
        dealt_to_others,
        backlash_total,
        healed_total,
    );

    // The RNG state advances exactly as far as the rolls actually made this action, and
    // only the seeded generator ever moves — no wall clock, no ambient entropy.
    state.rng_state = rng.state();

    // Idempotency ledger: insert maintaining sorted order, since `has_processed` binary
    // searches it. `validate_ability` already proved this id is absent, so only the `Err`
    // arm (the "not found, insert here" case) is ever reachable; the `Ok` arm is kept as a
    // defensive no-op rather than an unreachable!()/panic, per this crate's no-panic rule.
    if let Err(idx) = state
        .processed_command_ids
        .binary_search_by(|existing| existing.as_str().cmp(command.command_id.as_str()))
    {
        state
            .processed_command_ids
            .insert(idx, command.command_id.clone());
    }

    // `turn_number` is deliberately NOT advanced here. `scheduler::end_turn` owns turn
    // progression, and when this module advanced it too, every action counted twice — turns
    // went 1, 3, 5, 7. That desynchronises `expected_turn_number` validation from the turn a
    // client can actually observe, so a well-behaved client's next command would be rejected
    // as stale. Reported below as before/after so the outcome still describes the turn it
    // resolved in; committing the increment is the scheduler's job.

    Ok(CommandOutcome {
        command_id: command.command_id.clone(),
        turn_number_before: turn_before,
        turn_number_after: state.turn_number,
        samples,
        impact,
        terrain_ops,
        damage: damage_by_player.into_values().collect(),
        objects_created,
        gauge_gained: gauge_gained(state, &command.player_id),
        terrain_cells_removed,
        final_state_hash: hash::hash_state(state),
    })
}

/// Tracks the pre-award gauge value so [`resolve_ability`] can report the actual delta
/// after [`PlayerState::add_gauge`]'s caps are applied, without a second mutable pass.
///
/// Kept as a tiny free function (rather than inlined) because it is called from two
/// distinct points in [`resolve_ability`]'s control flow (before and after the award) in
/// earlier drafts of this function; kept as a named step for clarity even collapsed to one.
fn gauge_gained(state: &SimulationState, player_id: &str) -> u16 {
    state.player(player_id).map_or(0, |p| p.special_gauge)
}

/// Awards gauge to the actor for this action, per `CHARACTERS.md` §2's per-point rates
/// (`GAUGE_PER_DAMAGE_DEALT`, `GAUGE_PER_DAMAGE_TAKEN`, `GAUGE_PER_HEALED`), routed through
/// [`PlayerState::add_gauge`] so the per-action and full-gauge caps are enforced by the one
/// reviewed implementation of that cap rather than a second copy of it here.
fn award_gauge(state: &mut SimulationState, player_id: &str, dealt: u32, taken: u32, healed: u32) {
    let raw = dealt
        .saturating_mul(u32::from(GAUGE_PER_DAMAGE_DEALT))
        .saturating_add(taken.saturating_mul(u32::from(GAUGE_PER_DAMAGE_TAKEN)))
        .saturating_add(healed.saturating_mul(u32::from(GAUGE_PER_HEALED)));
    // `add_gauge` itself caps at `GAUGE_MAX_PER_ACTION`, so an oversized `raw` value only
    // needs to survive the u16 conversion without wrapping — saturating to `u16::MAX` does
    // that, and the real cap is enforced immediately afterward by `add_gauge`.
    let amount = u16::try_from(raw).unwrap_or(u16::MAX);
    if let Some(actor) = state.player_mut(player_id) {
        actor.add_gauge(amount);
    }
}

/// Resolves the point a [`StrikeAttack`] acts at and enforces the direct reach check.
///
/// With an explicit target, the point is the target's position, and it must be within
/// `strike.range` of the actor: a strike has no travel time, so a target outside reach is
/// not a miss to roll for, it is an illegal declaration. Without one, the point is the
/// actor's own position — the strike is an area effect centred on the actor.
fn resolve_strike_point(
    state: &SimulationState,
    command: &AbilityCommand,
    strike: StrikeAttack,
    actor_position: FixedPoint,
) -> Result<FixedPoint, CommandRejection> {
    let Some(target_id) = &command.target_player_id else {
        return Ok(actor_position);
    };
    let Some(target) = state.player(target_id) else {
        return Err(CommandRejection::InvalidTarget);
    };
    if !fixed::within_radius(actor_position, target.position, strike.range) {
        return Err(CommandRejection::InvalidTarget);
    }
    Ok(target.position)
}

/// Applies a [`TerrainProfile`] at `impact_position` (craters) or as a sweep from `origin`
/// to `impact_position` (digs), recording the resulting [`TerrainOperation`] and cell
/// count. A no-op for [`TerrainProfile::None`].
fn apply_terrain_profile(
    state: &mut SimulationState,
    profile: TerrainProfile,
    origin: FixedPoint,
    impact_position: FixedPoint,
    terrain_ops: &mut Vec<TerrainOperation>,
    terrain_cells_removed: &mut u32,
) {
    let shape = match profile {
        TerrainProfile::None => return,
        TerrainProfile::Crater { radius_cells } => TerrainShape::SubtractCircle {
            center: impact_position,
            radius_cells,
        },
        TerrainProfile::Dig { radius_cells, .. } => TerrainShape::SubtractCapsule {
            start: origin,
            end: impact_position,
            radius_cells,
        },
    };
    let op = TerrainOperation {
        sequence: state.next_terrain_sequence,
        shape,
        material_mask: DESTRUCTIBLE_TERRAIN,
    };
    // Monotonic per-match sequence; a client never supplies this.
    state.next_terrain_sequence = state.next_terrain_sequence.saturating_add(1);
    // Routed through `block_ops` rather than `terrain::apply_operation` so block health
    // stays the authority inside a block span (ADR 0005). Carving cells directly here would
    // hole a block while it still reported full health, which is the drift the block model
    // exists to prevent. Background terrain still receives a literal crater.
    let removed = crate::block_ops::apply_operation(state, &op);
    *terrain_cells_removed = terrain_cells_removed.saturating_add(removed);
    terrain_ops.push(op);
}

/// Applies backlash from a [`StrikeAttack`]'s own `self_damage` field — the one backlash
/// mechanism this module still resolves directly. [`EffectKind::SelfDamage`] (the type
/// contract's other backlash mechanism, attachable to either attack shape) now resolves
/// through [`resolve::resolve_effect`] alongside every other effect
/// (`resolve::support::resolve`, called from the effects loop in [`resolve_ability`]), not
/// here — see the module doc comment's "Scope of ability resolution" section. Returns the
/// amount dealt, still asserted directly by this module's own unit test; the effects loop's
/// caller does not use the return value, since it sums the itemized damage map instead
/// once every effect (including any `SelfDamage`) has also run.
///
/// Applied unconditionally and never special-cased to spare the actor: backlash can
/// eliminate its own user, exactly as `StrikeAttack::self_damage`'s doc comment states.
fn apply_backlash(
    state: &mut SimulationState,
    ability: &AbilityDefinition,
    actor_id: &str,
    damage_by_player: &mut BTreeMap<String, DamageEvent>,
) -> u32 {
    let mut total: u32 = 0;

    if let Attack::Strike(strike) = ability.attack
        && strike.self_damage > 0
    {
        let actual = deal_damage(
            state,
            damage_by_player,
            actor_id,
            strike.self_damage,
            DamageField::Backlash,
            false,
        );
        total = total.saturating_add(u32::from(actual));
    }

    total
}

/// Rolls a crit for one strike using the seeded match RNG.
///
/// A zero `crit_chance_basis_points` never rolls at all (matching every non-crit-capable
/// launch ability, whose `crit_damage_percent` equals `damage_percent`), so RNG state
/// consumption is exactly proportional to how many crit-capable strikes actually happened.
fn roll_crit(rng: &mut Rng, crit_chance_basis_points: u16) -> bool {
    crit_chance_basis_points > 0
        && rng.bounded(CRIT_ROLL_BOUND) < u32::from(crit_chance_basis_points)
}

/// The hit-point damage for one resolution of `ability`, non-critical or critical per
/// `is_crit`. Delegates to [`AbilityDefinition::damage`] / [`AbilityDefinition::crit_damage`]
/// — the canonical `BASE_ATTACK`-scaled computation already defined on the type — rather
/// than re-deriving the same scaling here. `None` (only reachable via `i32` overflow, not
/// credible for any launch-roster percentage) degrades to zero damage rather than
/// wrapping into a heal.
fn resolved_damage_hp(ability: &AbilityDefinition, is_crit: bool) -> u16 {
    let raw = if is_crit {
        ability.crit_damage()
    } else {
        ability.damage()
    };
    raw.and_then(|value| u16::try_from(value).ok()).unwrap_or(0)
}

/// Which itemized [`DamageEvent`] field a call to [`deal_damage`] contributes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DamageField {
    /// An exact direct hit.
    Direct,
    /// Backlash the actor dealt to themselves.
    Backlash,
}

/// Reduces `player_id`'s health by `amount` (saturating — health never goes negative),
/// records the *actual* amount that landed (capped at the player's pre-mutation health, so
/// an overkill hit reports true HP lost rather than the raw damage figure) into the
/// itemized [`DamageEvent`] for that player, and marks elimination if health reached zero.
/// Returns the actual amount applied, for gauge accounting.
///
/// A missing `player_id` (which validation should have already ruled out) is a no-op
/// rather than a panic, consistent with this crate's no-panic-on-untrusted-input rule.
fn deal_damage(
    state: &mut SimulationState,
    damage_by_player: &mut BTreeMap<String, DamageEvent>,
    player_id: &str,
    amount: u16,
    field: DamageField,
    is_crit: bool,
) -> u16 {
    if amount == 0 {
        return 0;
    }
    let Some(player) = state.player_mut(player_id) else {
        return 0;
    };
    let actual = amount.min(player.health);
    player.health = player.health.saturating_sub(amount);
    let eliminated_now = player.is_eliminated();

    let entry = damage_entry(damage_by_player, player_id);
    match field {
        DamageField::Direct => entry.direct = entry.direct.saturating_add(actual),
        DamageField::Backlash => entry.backlash = entry.backlash.saturating_add(actual),
    }
    entry.was_critical = entry.was_critical || is_crit;
    entry.eliminated = eliminated_now;

    actual
}

/// Returns the itemized [`DamageEvent`] for `player_id`, inserting a zeroed one if this is
/// the first event touching them this action.
fn damage_entry<'a>(
    damage_by_player: &'a mut BTreeMap<String, DamageEvent>,
    player_id: &str,
) -> &'a mut DamageEvent {
    damage_by_player
        .entry(player_id.to_string())
        .or_insert_with(|| DamageEvent {
            player_id: player_id.to_string(),
            direct: 0,
            splash: 0,
            backlash: 0,
            hazard: 0,
            wall_impact: 0,
            healed: 0,
            was_critical: false,
            knockback: FixedPoint::ZERO,
            eliminated: false,
        })
}

/// Validates and applies a [`PassiveChoiceCommand`] — the one-time passive selection made
/// the first time a character's gauge fills (`CHARACTERS.md` §2).
///
/// `types.rs` specifies the choice's own three checks (pending, not already chosen,
/// belongs to the player's character) without an explicit order for the surrounding
/// authorization checks; this mirrors [`validate_ability`]'s order for the checks both
/// share, on the same "duplicate and identity first" security reasoning.
pub fn apply_passive_choice(
    state: &mut SimulationState,
    command: &PassiveChoiceCommand,
) -> CommandResult {
    match validate_passive_choice(state, command) {
        Ok(character) => {
            let mut working = state.clone();
            let outcome = resolve_passive_choice(&mut working, command, character);
            *state = working;
            CommandResult::Accepted(Box::new(outcome))
        }
        Err(rejection) => CommandResult::Rejected(rejection),
    }
}

/// The validation half of [`apply_passive_choice`], split out so it can run against the
/// pre-mutation state exactly like [`validate_ability`] does.
fn validate_passive_choice(
    state: &SimulationState,
    command: &PassiveChoiceCommand,
) -> Result<&'static CharacterDefinition, CommandRejection> {
    if state.has_processed(&command.command_id) {
        return Err(CommandRejection::DuplicateCommand);
    }

    let Some(player) = state.player(&command.player_id) else {
        return Err(CommandRejection::PlayerEliminated);
    };
    if player.is_eliminated() {
        return Err(CommandRejection::PlayerEliminated);
    }

    if command.player_id != state.active_player_id {
        return Err(CommandRejection::NotActivePlayer);
    }

    if state.phase != MatchPhase::PassiveSelection || player.has_chosen_passive {
        return Err(CommandRejection::PassiveAlreadyChosen);
    }

    if command.expected_turn_number != state.turn_number {
        return Err(CommandRejection::TurnVersionMismatch);
    }

    let Some(character) = character::find(&player.character_id) else {
        return Err(CommandRejection::UnknownCharacter);
    };

    if !character
        .passives
        .iter()
        .any(|passive| passive.id == command.passive_id)
    {
        // The chosen id does not belong to this character's own pool — a client cannot
        // legally offer a passive from a different character's kit. Security event.
        return Err(CommandRejection::InvalidPassive);
    }

    Ok(character)
}

/// Performs the mutating half of [`apply_passive_choice`] on the caller's working copy.
fn resolve_passive_choice(
    state: &mut SimulationState,
    command: &PassiveChoiceCommand,
    _character: &'static CharacterDefinition,
) -> CommandOutcome {
    let turn_before = state.turn_number;

    if let Some(actor) = state.player_mut(&command.player_id) {
        actor.passive_id = Some(command.passive_id.clone());
        actor.has_chosen_passive = true;
    }

    if let Err(idx) = state
        .processed_command_ids
        .binary_search_by(|existing| existing.as_str().cmp(command.command_id.as_str()))
    {
        state
            .processed_command_ids
            .insert(idx, command.command_id.clone());
    }

    // Choosing a passive is not an attack: it does not consume the turn or the
    // has-attacked flag. The scheduler is responsible for advancing the phase once the
    // choice lands (out of scope for this module, which owns command validation and
    // application, not phase transitions).
    CommandOutcome {
        command_id: command.command_id.clone(),
        turn_number_before: turn_before,
        turn_number_after: state.turn_number,
        samples: Vec::new(),
        impact: None,
        terrain_ops: Vec::new(),
        damage: Vec::new(),
        objects_created: Vec::new(),
        gauge_gained: 0,
        terrain_cells_removed: 0,
        final_state_hash: hash::hash_state(state),
    }
}

#[cfg(test)]
// Tests may panic on a fixture invariant that must hold — a `let ... else { panic!() }`
// states the expectation far more clearly than threading a Result through every case.
// The production paths in this module remain panic-free, which is what the lint protects.
// Matches the precedent set by `terrain::tests` and `character::tests`.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::terrain;

    // -----------------------------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------------------------

    /// A minimal, deterministic 20x20 empty terrain mask for tests that do not exercise
    /// terrain-destroying abilities.
    fn empty_terrain() -> TerrainMask {
        TerrainMask {
            width: 20,
            height: 20,
            cells: vec![0u8; 400],
        }
    }

    fn player(id: &str, character_id: &str, position: FixedPoint) -> PlayerState {
        PlayerState {
            id: id.to_string(),
            team: 0,
            health: 999,
            max_health: 999,
            position,
            character_id: character_id.to_string(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    /// A two-player match with `attacker` (huck) active, in `AimingAndSelection`, turn 1,
    /// with real launch-roster character ids so `character::find` resolves.
    fn base_state() -> SimulationState {
        let attacker = player("attacker", "huck", FixedPoint::new(0, 0));
        let defender = player("defender", "huck", FixedPoint::new(1024, 0));
        SimulationState {
            blocks: Vec::new(),
            simulation_version: 2,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::AimingAndSelection,
            active_player_id: "attacker".to_string(),
            wind_per_tick: 0,
            movement_remaining: 0,
            has_attacked_this_turn: false,
            terrain: empty_terrain(),
            players: vec![attacker, defender],
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 12345,
        }
    }

    /// Same shape as [`base_state`] (turn 1, `AimingAndSelection`, empty terrain), but for
    /// tests that need `active_player_id` and the roster kit behind it to be something
    /// other than Huck — Zeke's Lifeshare/Mending Bolt, Natomica's Repulse, Arzum's Chain
    /// Strike — so the P1 wiring can be exercised end to end through the real
    /// [`apply_ability`] entry point rather than by calling a resolver directly.
    fn state_with_players(active_player_id: &str, players: Vec<PlayerState>) -> SimulationState {
        SimulationState {
            blocks: Vec::new(),
            simulation_version: 2,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::AimingAndSelection,
            active_player_id: active_player_id.to_string(),
            wind_per_tick: 0,
            movement_remaining: 0,
            has_attacked_this_turn: false,
            terrain: empty_terrain(),
            players,
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 12345,
        }
    }

    fn ability_command(command_id: &str, target: Option<&str>) -> AbilityCommand {
        AbilityCommand {
            command_id: command_id.to_string(),
            player_id: "attacker".to_string(),
            expected_turn_number: 1,
            slot: AbilitySlot::Basic,
            angle_millidegrees: 0,
            power_basis_points: 5_000,
            target_player_id: target.map(str::to_string),
            secondary_target_player_id: None,
        }
    }

    // -----------------------------------------------------------------------------------
    // validate_ability: pure rejection-path tests. None of these reach `resolve_ability`,
    // so none depend on the still-stubbed `hash`/`ballistics` modules.
    // -----------------------------------------------------------------------------------

    #[test]
    fn duplicate_command_id_is_rejected_first() {
        let mut state = base_state();
        state.processed_command_ids.push("cmd-1".to_string());
        let command = ability_command("cmd-1", Some("defender"));

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::DuplicateCommand)
        );
    }

    #[test]
    fn duplicate_check_precedes_every_other_check() {
        // Deliberately also violates NotActivePlayer (wrong active player) and
        // WrongPhase (MatchComplete). DuplicateCommand must still win.
        let mut state = base_state();
        state.processed_command_ids.push("cmd-1".to_string());
        state.active_player_id = "defender".to_string();
        state.phase = MatchPhase::MatchComplete;
        let command = ability_command("cmd-1", Some("defender"));

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::DuplicateCommand)
        );
    }

    #[test]
    fn unknown_player_id_is_rejected_as_eliminated() {
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.player_id = "ghost".to_string();

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::PlayerEliminated)
        );
    }

    #[test]
    fn eliminated_player_cannot_act() {
        let mut state = base_state();
        if let Some(attacker) = state.player_mut("attacker") {
            attacker.health = 0;
        }
        let command = ability_command("cmd-1", Some("defender"));

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::PlayerEliminated)
        );
    }

    #[test]
    fn non_active_player_is_rejected() {
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("attacker"));
        command.player_id = "defender".to_string();

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::NotActivePlayer)
        );
        assert!(CommandRejection::NotActivePlayer.is_security_event());
    }

    #[test]
    fn wrong_phase_rejects_ability_command() {
        let mut state = base_state();
        state.phase = MatchPhase::PassiveSelection;
        let command = ability_command("cmd-1", Some("defender"));

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::WrongPhase)
        );
    }

    #[test]
    fn stale_turn_number_is_rejected() {
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.expected_turn_number = 0;

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::TurnVersionMismatch)
        );
    }

    #[test]
    fn unknown_character_id_is_rejected() {
        let mut state = base_state();
        if let Some(attacker) = state.player_mut("attacker") {
            attacker.character_id = "not-a-real-character".to_string();
        }
        let command = ability_command("cmd-1", Some("defender"));

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::UnknownCharacter)
        );
        assert!(CommandRejection::UnknownCharacter.is_security_event());
    }

    #[test]
    fn ability_not_available_in_slot_is_rejected() {
        // Huck has no `BasicAlt` — only Aleph does.
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.slot = AbilitySlot::BasicAlt;

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::AbilityNotAvailable)
        );
        assert!(CommandRejection::AbilityNotAvailable.is_security_event());
    }

    #[test]
    fn special_with_empty_gauge_is_rejected() {
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.slot = AbilitySlot::Special;

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::GaugeNotReady)
        );
    }

    #[test]
    fn special_with_partial_gauge_is_rejected() {
        let mut state = base_state();
        if let Some(attacker) = state.player_mut("attacker") {
            attacker.special_gauge = GAUGE_FULL.saturating_sub(1);
        }
        let mut command = ability_command("cmd-1", Some("defender"));
        command.slot = AbilitySlot::Special;

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::GaugeNotReady)
        );
    }

    #[test]
    fn second_attack_in_one_turn_is_rejected() {
        let mut state = base_state();
        state.has_attacked_this_turn = true;
        let command = ability_command("cmd-1", Some("defender"));

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::AlreadyAttacked)
        );
    }

    #[test]
    fn negative_angle_is_rejected_not_clamped() {
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.angle_millidegrees = -1;

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::InputOutOfRange)
        );
        assert!(CommandRejection::InputOutOfRange.is_security_event());
    }

    #[test]
    fn angle_at_360_degrees_exclusive_bound_is_rejected() {
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.angle_millidegrees = 360_000;

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::InputOutOfRange)
        );
    }

    #[test]
    fn power_above_10000_basis_points_is_rejected_not_clamped() {
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.power_basis_points = 10_001;

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::InputOutOfRange)
        );
    }

    #[test]
    fn power_boundary_values_are_accepted() {
        let state = base_state();
        let mut low = ability_command("cmd-lo", Some("defender"));
        low.power_basis_points = 0;
        let mut high = ability_command("cmd-hi", Some("defender"));
        high.power_basis_points = 10_000;

        assert!(validate_ability(&state, &low).is_ok());
        assert!(validate_ability(&state, &high).is_ok());
    }

    #[test]
    fn target_that_does_not_exist_is_rejected() {
        let state = base_state();
        let command = ability_command("cmd-1", Some("nobody"));

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::InvalidTarget)
        );
    }

    #[test]
    fn eliminated_target_is_rejected() {
        let mut state = base_state();
        if let Some(defender) = state.player_mut("defender") {
            defender.health = 0;
        }
        let command = ability_command("cmd-1", Some("defender"));

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::InvalidTarget)
        );
    }

    #[test]
    fn secondary_target_equal_to_primary_is_rejected() {
        let state = base_state();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.secondary_target_player_id = Some("defender".to_string());

        assert_eq!(
            validate_ability(&state, &command),
            Err(CommandRejection::InvalidTarget)
        );
    }

    #[test]
    fn valid_command_resolves_the_roster_ability() {
        let state = base_state();
        let command = ability_command("cmd-1", Some("defender"));

        let Ok((character, ability)) = validate_ability(&state, &command) else {
            panic!("expected a valid command to resolve");
        };
        assert_eq!(character.id, "huck");
        assert_eq!(ability.id, "huck-haymaker");
    }

    // -----------------------------------------------------------------------------------
    // Rejection paths leave state byte-identical. Direct struct equality is used (rather
    // than routing through the still-stubbed `hash::hash_state`) because `SimulationState`
    // already derives `PartialEq`/`Eq`, which is strictly more precise than comparing
    // hashes and does not depend on a sibling module that has not landed yet.
    // -----------------------------------------------------------------------------------

    #[test]
    fn rejected_ability_command_leaves_state_unchanged() {
        let state = base_state();
        let before = state.clone();
        let mut mutable_state = state;
        // NotActivePlayer: defender is not the active player.
        let mut command = ability_command("cmd-1", Some("attacker"));
        command.player_id = "defender".to_string();

        let result = apply_ability(&mut mutable_state, &command);

        assert_eq!(
            result,
            CommandResult::Rejected(CommandRejection::NotActivePlayer)
        );
        assert_eq!(mutable_state, before);
    }

    #[test]
    fn rejected_passive_choice_leaves_state_unchanged() {
        let mut state = base_state();
        state.phase = MatchPhase::PassiveSelection;
        let before = state.clone();
        let command = PassiveChoiceCommand {
            command_id: "cmd-1".to_string(),
            player_id: "attacker".to_string(),
            expected_turn_number: 1,
            passive_id: "not-hucks-passive".to_string(),
        };

        let result = apply_passive_choice(&mut state, &command);

        assert_eq!(
            result,
            CommandResult::Rejected(CommandRejection::InvalidPassive)
        );
        assert_eq!(state, before);
    }

    // -----------------------------------------------------------------------------------
    // apply_passive_choice: full validation-order coverage. Does not touch `ballistics`,
    // but does call `hash::hash_state` on its accepted path (every `CommandOutcome` must
    // carry a `final_state_hash`), so the accepted-path assertions below depend on `hash.rs`
    // landing — see the module doc comment's cross-module dependency note.
    // -----------------------------------------------------------------------------------

    fn passive_command(passive_id: &str) -> PassiveChoiceCommand {
        PassiveChoiceCommand {
            command_id: "passive-1".to_string(),
            player_id: "attacker".to_string(),
            expected_turn_number: 1,
            passive_id: passive_id.to_string(),
        }
    }

    #[test]
    fn passive_choice_from_a_different_character_is_rejected() {
        let mut state = base_state();
        state.phase = MatchPhase::PassiveSelection;
        // A real passive, but Aleph's, not Huck's.
        let command = passive_command("aleph-volatile");

        assert_eq!(
            apply_passive_choice(&mut state, &command),
            CommandResult::Rejected(CommandRejection::InvalidPassive)
        );
        assert!(CommandRejection::InvalidPassive.is_security_event());
    }

    #[test]
    fn passive_choice_outside_passive_selection_phase_is_rejected() {
        let mut state = base_state(); // phase is AimingAndSelection
        let command = passive_command("huck-immovable");

        assert_eq!(
            apply_passive_choice(&mut state, &command),
            CommandResult::Rejected(CommandRejection::PassiveAlreadyChosen)
        );
    }

    #[test]
    fn passive_choice_a_second_time_is_rejected() {
        let mut state = base_state();
        state.phase = MatchPhase::PassiveSelection;
        if let Some(attacker) = state.player_mut("attacker") {
            attacker.has_chosen_passive = true;
        }
        let command = passive_command("huck-immovable");

        assert_eq!(
            apply_passive_choice(&mut state, &command),
            CommandResult::Rejected(CommandRejection::PassiveAlreadyChosen)
        );
    }

    #[test]
    fn valid_passive_choice_is_accepted_and_recorded() {
        let mut state = base_state();
        state.phase = MatchPhase::PassiveSelection;
        let command = passive_command("huck-immovable");

        let result = apply_passive_choice(&mut state, &command);

        let CommandResult::Accepted(outcome) = result else {
            panic!("expected the valid passive choice to be accepted");
        };
        assert_eq!(outcome.command_id, "passive-1");
        let Some(attacker) = state.player("attacker") else {
            panic!("attacker must still exist");
        };
        assert_eq!(attacker.passive_id.as_deref(), Some("huck-immovable"));
        assert!(attacker.has_chosen_passive);
        assert!(state.has_processed("passive-1"));
    }

    #[test]
    fn duplicate_passive_choice_does_not_reapply() {
        let mut state = base_state();
        state.phase = MatchPhase::PassiveSelection;
        let command = passive_command("huck-immovable");

        let first = apply_passive_choice(&mut state, &command);
        assert!(matches!(first, CommandResult::Accepted(_)));

        let after_first = state.clone();
        let second = apply_passive_choice(&mut state, &command);

        assert_eq!(
            second,
            CommandResult::Rejected(CommandRejection::DuplicateCommand)
        );
        assert_eq!(state, after_first);
    }

    // -----------------------------------------------------------------------------------
    // Private-helper unit tests: damage/heal/backlash accounting, independent of whether
    // `hash.rs`/`ballistics.rs` exist yet, since these functions never call either.
    // -----------------------------------------------------------------------------------

    #[test]
    fn deal_damage_records_actual_amount_and_elimination() {
        let mut state = base_state();
        if let Some(defender) = state.player_mut("defender") {
            defender.health = 10;
        }
        let mut damage = BTreeMap::new();

        let actual = deal_damage(
            &mut state,
            &mut damage,
            "defender",
            999,
            DamageField::Direct,
            true,
        );

        assert_eq!(
            actual, 10,
            "actual damage must be capped at the player's pre-hit health"
        );
        let Some(defender) = state.player("defender") else {
            panic!("defender must exist");
        };
        assert_eq!(defender.health, 0);
        assert!(defender.is_eliminated());
        let Some(event) = damage.get("defender") else {
            panic!("a damage event must be recorded");
        };
        assert_eq!(event.direct, 10);
        assert!(event.was_critical);
        assert!(event.eliminated);
    }

    #[test]
    fn backlash_via_strike_self_damage_can_eliminate_the_actor() {
        // No launch-roster ability currently declares `self_damage > 0`, so this exercises
        // the mechanism directly against a synthetic ability — exactly what
        // `StrikeAttack::self_damage`'s doc comment and the task's adversarial test list
        // require ("Backlash can eliminate the acting player").
        let mut state = base_state();
        if let Some(attacker) = state.player_mut("attacker") {
            attacker.health = 5;
        }
        let ability = AbilityDefinition {
            id: "test-self-destruct",
            display_name: "Test Self-Destruct",
            slot: AbilitySlot::Special,
            damage_percent: 0,
            crit_damage_percent: 0,
            crit_chance_basis_points: 0,
            strikes_per_turn: 1,
            attack: Attack::Strike(StrikeAttack {
                range: 0,
                terrain: TerrainProfile::None,
                self_damage: 5_000,
            }),
            effects: &[],
        };
        let mut damage = BTreeMap::new();

        let total = apply_backlash(&mut state, &ability, "attacker", &mut damage);

        assert_eq!(
            total, 5,
            "backlash must be capped at the actor's own pre-hit health"
        );
        let Some(attacker) = state.player("attacker") else {
            panic!("attacker must exist");
        };
        assert_eq!(attacker.health, 0);
        assert!(attacker.is_eliminated());
        let Some(event) = damage.get("attacker") else {
            panic!("a backlash event must be recorded for the actor");
        };
        assert_eq!(event.backlash, 5);
        assert!(event.eliminated);
    }

    #[test]
    fn self_damage_effect_still_deals_backlash_after_moving_into_the_resolver_layer() {
        // No launch character attaches `EffectKind::SelfDamage` (`character.rs` grepped:
        // zero hits), so this cannot go through the public `apply_ability` entry point
        // (which always resolves a real roster ability). It calls the private
        // `resolve_ability` — the same function `apply_ability` calls once validation has
        // passed — with a synthetic ability, matching the precedent every regression test
        // in this module already sets. See `resolve::support::tests` for the same effect
        // exercised in isolation.
        const ABILITY: AbilityDefinition = AbilityDefinition {
            id: "test-effect-backlash",
            display_name: "Test Effect Backlash",
            slot: AbilitySlot::Basic,
            damage_percent: 0,
            crit_damage_percent: 0,
            crit_chance_basis_points: 0,
            strikes_per_turn: 1,
            attack: Attack::Strike(StrikeAttack {
                range: 0,
                terrain: TerrainProfile::None,
                self_damage: 0,
            }),
            effects: &[SpecialEffect {
                trigger: EffectTrigger::OnFire,
                kind: EffectKind::SelfDamage,
                magnitude: 10, // 10% of BASE_ATTACK == 10 HP.
                magnitude_secondary: 0,
                duration_turns: 0,
            }],
        };
        let mut state = base_state();
        let command = ability_command("cmd-1", None);

        let result = resolve_ability(&mut state, &command, &ABILITY);

        let Ok(outcome) = result else {
            panic!("expected the ability to resolve: {result:?}");
        };
        let Some(event) = outcome
            .damage
            .iter()
            .find(|event| event.player_id == "attacker")
        else {
            panic!("attacker must have a backlash damage entry");
        };
        assert_eq!(
            event.backlash, 10,
            "moved into the resolver layer, the percent-of-BASE_ATTACK backlash convention \
             must be unchanged"
        );
    }

    #[test]
    fn heal_effect_still_heals_after_moving_into_the_resolver_layer() {
        // Zeke's Mending Bolt (Basic) carries `ZEKE_HEAL`: a real roster ability, so this
        // proves the behaviour survives the move all the way through the public
        // `apply_ability` entry point. `EffectKind::Heal` targets whoever the *command*
        // names, independent of where the bolt itself lands (`command.rs`'s own direct-hit
        // selection for a projectile never consults `command.target_player_id`), so the
        // target is placed far beyond any tier's maximum reach — well outside the terrain
        // bounds entirely — to rule out the bolt also landing a direct hit on it and
        // muddying the expected health delta.
        let mut target = player(
            "target",
            "huck",
            FixedPoint::new(500 * fixed::POSITION_SCALE, 0),
        );
        target.health = 100;
        target.max_health = 220;
        let mut state = state_with_players(
            "actor",
            vec![player("actor", "zeke", FixedPoint::new(0, 0)), target],
        );
        let command = AbilityCommand {
            command_id: "cmd-1".to_string(),
            player_id: "actor".to_string(),
            expected_turn_number: 1,
            slot: AbilitySlot::Basic,
            angle_millidegrees: 0,
            power_basis_points: 5_000,
            target_player_id: Some("target".to_string()),
            secondary_target_player_id: None,
        };

        let result = apply_ability(&mut state, &command);

        let CommandResult::Accepted(_outcome) = result else {
            panic!("expected Zeke's Mending Bolt to be accepted: {result:?}");
        };
        let Some(target) = state.player("target") else {
            panic!("target must exist");
        };
        assert_eq!(
            target.health, 122,
            "Mending Bolt's Heal effect must actually restore its 22 HP, regardless of \
             whether the bolt itself connects"
        );
    }

    #[test]
    fn health_transfer_never_reduces_actor_below_one_hp() {
        // Zeke's Lifeshare (Special): a real roster ability, exercised end to end through
        // `apply_ability` — the same regression `resolve::support::tests` covers in
        // isolation for the resolver itself.
        let mut actor = player("actor", "zeke", FixedPoint::new(0, 0));
        actor.health = 10;
        actor.special_gauge = GAUGE_FULL;
        // The target must actually be missing health, or the conserving transfer has
        // nothing to move and the actor-floor cap is never exercised.
        let mut target = player(
            "target",
            "huck",
            FixedPoint::new(2 * fixed::POSITION_SCALE, 0),
        );
        target.health = 100;
        let mut state = state_with_players("actor", vec![actor, target]);
        let command = AbilityCommand {
            command_id: "cmd-1".to_string(),
            player_id: "actor".to_string(),
            expected_turn_number: 1,
            slot: AbilitySlot::Special,
            angle_millidegrees: 0,
            power_basis_points: 5_000,
            target_player_id: Some("target".to_string()),
            secondary_target_player_id: None,
        };

        let result = apply_ability(&mut state, &command);

        let CommandResult::Accepted(_outcome) = result else {
            panic!("expected Zeke's Lifeshare to be accepted: {result:?}");
        };
        let Some(actor) = state.player("actor") else {
            panic!("actor must exist");
        };
        assert_eq!(
            actor.health, 1,
            "transfer must be capped so the actor keeps at least 1 HP"
        );
        let Some(target) = state.player("target") else {
            panic!("target must exist");
        };
        assert_eq!(
            target.health, 109,
            "exactly what left the actor must arrive"
        );
    }

    #[test]
    fn health_transfer_to_a_full_health_ally_moves_nothing() {
        // Regression: the transfer used to debit the actor before checking what the
        // target could receive, so aiming Lifeshare at a healthy ally destroyed the
        // actor's hit points and healed nobody. Health must be conserved — what leaves
        // the actor is exactly what arrives. Exercised end to end through `apply_ability`
        // with Zeke's real Lifeshare, so the fix is proven to have survived the move into
        // the resolver layer, not merely to hold in isolation.
        let mut actor = player("actor", "zeke", FixedPoint::new(0, 0));
        actor.health = 200;
        let target = player(
            "target",
            "huck",
            FixedPoint::new(2 * fixed::POSITION_SCALE, 0),
        );
        let mut state = state_with_players("actor", vec![actor, target]);
        if let Some(actor) = state.player_mut("actor") {
            actor.special_gauge = GAUGE_FULL;
        }
        let before_actor = state.player("actor").map(|player| player.health);
        let before_target = state.player("target").map(|player| player.health);
        let command = AbilityCommand {
            command_id: "cmd-1".to_string(),
            player_id: "actor".to_string(),
            expected_turn_number: 1,
            slot: AbilitySlot::Special,
            angle_millidegrees: 0,
            power_basis_points: 5_000,
            target_player_id: Some("target".to_string()),
            secondary_target_player_id: None,
        };

        let result = apply_ability(&mut state, &command);

        let CommandResult::Accepted(_outcome) = result else {
            panic!("expected Zeke's Lifeshare to be accepted: {result:?}");
        };
        assert_eq!(
            state.player("actor").map(|player| player.health),
            before_actor,
            "the actor must not lose health that nobody received",
        );
        assert_eq!(
            state.player("target").map(|player| player.health),
            before_target,
            "a full-health ally can receive nothing",
        );
    }

    // -----------------------------------------------------------------------------------
    // P1: `resolve_effect` is now reachable from a real command, for every effect kind —
    // not just the three `command.rs` used to resolve inline.
    // -----------------------------------------------------------------------------------

    #[test]
    fn teleport_effect_actually_relocates_arzum_through_apply_ability() {
        // The flagship example the wiring gap was named for (`todolist.md` P1): "In a real
        // match Arzum still does not teleport." This proves she now does, submitted as a
        // real command through the public entry point — not by calling
        // `relocation::resolve` directly, the way `relocation.rs`'s own tests do.
        let mut actor = player("actor", "arzum", FixedPoint::new(0, 0));
        actor.special_gauge = GAUGE_FULL;
        let target_position = FixedPoint::new(5 * fixed::POSITION_SCALE, 0);
        let target = player("target", "huck", target_position);
        let mut state = state_with_players("actor", vec![actor, target]);
        let command = AbilityCommand {
            command_id: "cmd-1".to_string(),
            player_id: "actor".to_string(),
            expected_turn_number: 1,
            slot: AbilitySlot::Special,
            angle_millidegrees: 0,
            power_basis_points: 5_000,
            target_player_id: Some("target".to_string()),
            secondary_target_player_id: None,
        };

        let result = apply_ability(&mut state, &command);

        let CommandResult::Accepted(_outcome) = result else {
            panic!("expected Arzum's Chain Strike to be accepted: {result:?}");
        };
        let Some(actor) = state.player("actor") else {
            panic!("actor must still exist");
        };
        // The only living opponent is `target` itself, so Chain Strike's "random nearby
        // enemy within 12 BW of the first target" draw has exactly one candidate — the
        // first target, trivially 0 BW from itself — making the destination deterministic
        // regardless of the RNG seed.
        assert_eq!(
            actor.position, target_position,
            "Chain Strike's Teleport effect must actually relocate the actor, not just \
             validate the command"
        );
    }

    #[test]
    fn push_effect_actually_displaces_the_target_through_apply_ability() {
        // No launch character attaches `EffectKind::Knockback` itself (`character.rs`
        // grepped: zero hits — `displacement.rs`'s own module doc comment naming
        // `ROBERTO_KNOCKBACK` as a live caller is stale). Natomica's Repulse attaches
        // `Push`, which shares the exact same resolver
        // (`resolve::displacement::resolve_radial_displacement`) as `Knockback` — both are
        // dispatched to it by `resolve_effect`'s own match arm. This proves that resolver
        // family is now reachable from a real command, submitted through the public
        // `apply_ability` entry point rather than by calling `displacement::resolve`
        // directly.
        let mut actor = player("actor", "natomica", FixedPoint::new(0, 0));
        actor.special_gauge = GAUGE_FULL;
        let target = player(
            "target",
            "natomica",
            FixedPoint::new(2 * fixed::POSITION_SCALE, 0),
        );
        let mut state = state_with_players("actor", vec![actor, target]);
        let before = state.player("target").map(|p| p.position);
        let command = AbilityCommand {
            command_id: "cmd-1".to_string(),
            player_id: "actor".to_string(),
            expected_turn_number: 1,
            slot: AbilitySlot::Special,
            angle_millidegrees: 0,
            power_basis_points: 5_000,
            target_player_id: None,
            secondary_target_player_id: None,
        };

        let result = apply_ability(&mut state, &command);

        let CommandResult::Accepted(_outcome) = result else {
            panic!("expected Natomica's Repulse to be accepted: {result:?}");
        };
        let after = state.player("target").map(|p| p.position);
        assert_ne!(
            before, after,
            "Push must have actually displaced the target, not merely validated the command"
        );
    }

    #[test]
    fn terrain_destroying_ability_reports_nonzero_terrain_cells_removed() {
        // Huck's Haymaker (`ability_command`'s own default) carves a crater on impact.
        // `base_state`'s terrain starts fully `Empty`, which would make this vacuous, so
        // solid ground is seeded around the defender first.
        let mut state = base_state();
        for y in 0..5i32 {
            for x in 0..5i32 {
                let _ = terrain::set_material(&mut state.terrain, x, y, Material::Soil);
            }
        }
        let command = ability_command("cmd-1", Some("defender"));

        let result = apply_ability(&mut state, &command);

        let CommandResult::Accepted(outcome) = result else {
            panic!("expected Haymaker to be accepted: {result:?}");
        };
        assert!(
            outcome.terrain_cells_removed > 0,
            "a terrain-destroying ability must report the cells it actually destroyed, not \
             the always-zero value from before P2"
        );
    }

    #[test]
    fn effect_resolution_failure_leaves_state_untouched() {
        // Huck's Body Throw (Special) needs both a primary and a secondary target for its
        // `Relocate` effect. Submitting it with only a primary is a legal *command* —
        // neither target field is individually required by `validate_ability`, and the
        // primary alone is enough to pass the reach check — but an *unresolvable* action:
        // the effects loop fails partway through, after this same command's own direct
        // damage (40% to the primary) has already landed in the cloned working copy
        // `resolve_ability` operates on. Proves `apply_ability`'s clone-and-discard
        // atomicity still holds even when the failure originates deep in the new effects
        // loop, not just at validation, and does so through the public entry point with a
        // real roster ability rather than a synthetic one.
        let mut state = base_state();
        if let Some(attacker) = state.player_mut("attacker") {
            attacker.special_gauge = GAUGE_FULL;
        }
        let before = state.clone();
        let mut command = ability_command("cmd-1", Some("defender"));
        command.slot = AbilitySlot::Special;
        // Deliberately no `secondary_target_player_id`: `Relocate` needs one.

        let result = apply_ability(&mut state, &command);

        assert_eq!(
            result,
            CommandResult::Rejected(CommandRejection::InvalidTarget)
        );
        assert_eq!(
            state, before,
            "a failure inside the effects loop, after damage already landed in the working \
             clone, must still leave the real state byte-identical"
        );
    }

    #[test]
    fn processed_command_ids_stay_sorted_after_many_insertions() {
        let mut state = base_state();
        state.phase = MatchPhase::PassiveSelection;
        // Insert out-of-lexicographic-order ids across several accepted commands and
        // confirm the ledger is sorted after each one, not just at the end.
        let ids = ["zzz-9", "aaa-1", "mmm-5", "bbb-2", "yyy-8"];
        for id in ids {
            if let Some(attacker) = state.player_mut("attacker") {
                attacker.has_chosen_passive = false;
            }
            let command = PassiveChoiceCommand {
                command_id: id.to_string(),
                player_id: "attacker".to_string(),
                expected_turn_number: state.turn_number,
                passive_id: "huck-immovable".to_string(),
            };
            let result = apply_passive_choice(&mut state, &command);
            assert!(
                matches!(result, CommandResult::Accepted(_)),
                "command {id} must be accepted"
            );
            assert!(
                state
                    .processed_command_ids
                    .windows(2)
                    .all(|pair| matches!(pair, [left, right] if left <= right)),
                "processed_command_ids must stay sorted after inserting {id}"
            );
        }
        assert_eq!(state.processed_command_ids.len(), ids.len());
    }

    // -----------------------------------------------------------------------------------
    // resolve_strike_point / target-area helpers.
    // -----------------------------------------------------------------------------------

    #[test]
    fn strike_point_out_of_reach_is_rejected() {
        let state = base_state();
        let command = ability_command("cmd-1", Some("defender"));
        let strike = StrikeAttack {
            range: 1, // far shorter than the 1024-unit gap between attacker and defender.
            terrain: TerrainProfile::None,
            self_damage: 0,
        };

        assert_eq!(
            resolve_strike_point(&state, &command, strike, FixedPoint::ZERO),
            Err(CommandRejection::InvalidTarget)
        );
    }

    #[test]
    fn strike_point_without_a_target_is_the_actor_position() {
        let state = base_state();
        let command = ability_command("cmd-1", None);
        let strike = StrikeAttack {
            range: 100_000,
            terrain: TerrainProfile::None,
            self_damage: 0,
        };
        let actor_position = FixedPoint::new(42, 7);

        assert_eq!(
            resolve_strike_point(&state, &command, strike, actor_position),
            Ok(actor_position)
        );
    }
}
