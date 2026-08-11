//! Resolvers for [`EffectKind::Heal`], [`EffectKind::HealthTransfer`], and
//! [`EffectKind::SelfDamage`].
//!
//! # Why this file exists
//!
//! Before `resolve_effect` was wired into `command.rs::apply_ability` (`todolist.md` P1),
//! these three effects were the exception to the inert-effect problem: `command.rs`
//! resolved them inline (in its own private `apply_support_effects`/`apply_backlash`
//! helpers) while `resolve/mod.rs` routed the same three kinds to a silent `Ok(())` with a
//! comment explaining they lived elsewhere. That was **two resolution paths for one
//! concept** — the exact shape of bug that left the other 19 kinds inert in the first
//! place, just inverted (these three worked; the other 19 didn't). Now that
//! `resolve_effect` is reachable from a real command, there is no reason for a second path
//! to exist, so this file is it: the *only* place `Heal`, `HealthTransfer`, and
//! `SelfDamage` resolve. `command.rs` no longer touches any of the three.
//!
//! Every rule enforced below is a direct, behavior-preserving port of `command.rs`'s
//! former private helpers — including the conserving `HealthTransfer` guard (regression
//! tested there as `health_transfer_to_a_full_health_ally_moves_nothing`, and again here
//! and end-to-end in `command.rs`'s own test module). Only *where* this logic runs
//! changed, not what it does.

use crate::error::{SimError, SimResult};
use crate::fixed;
use crate::types::{BASE_ATTACK, EffectKind, SpecialEffect};

use super::ResolveContext;

/// Resolves a support-family effect: [`EffectKind::Heal`], [`EffectKind::HealthTransfer`],
/// or [`EffectKind::SelfDamage`].
///
/// # Errors
///
/// Never actually fails: every malformed magnitude degrades to a zero-effect no-op rather
/// than an error, exactly matching `command.rs`'s former behavior for these three. The
/// `Result` return stays for symmetry with every other resolver family's dispatch
/// signature, and so a future tightening of these checks would not need to change
/// `resolve_effect`'s dispatch in `mod.rs`. `resolve_effect` only ever routes these three
/// kinds here; the wildcard arm below is defensive, matching every sibling family's own
/// dispatch — a silent fallthrough is exactly how the original inert-effect gap happened.
pub fn resolve(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    match effect.kind {
        EffectKind::Heal => resolve_heal(ctx, effect),
        EffectKind::HealthTransfer => resolve_health_transfer(ctx, effect),
        EffectKind::SelfDamage => resolve_self_damage(ctx, effect),
        _ => Err(SimError::OutOfRange {
            field: "effect.kind (not a support-family effect)",
        }),
    }
}

// ---------------------------------------------------------------------------------------
// Heal
// ---------------------------------------------------------------------------------------

/// Heal: restores health to `ctx.primary_target_id`, or to the actor when no target was
/// given (Zeke's Mending Bolt heals a chosen ally; a targetless cast is a self-heal —
/// matching `command.rs`'s former `command.target_player_id.clone().unwrap_or_else(||
/// command.player_id.clone())`).
///
/// `effect.magnitude` is flat hit points, not a percent of [`BASE_ATTACK`] — matching how
/// `character.rs` actually encodes it (Zeke's `ZEKE_HEAL` magnitude of `22` is documented
/// there as "22 HP", not 22% of anything). A negative or overflowing magnitude degrades to
/// zero rather than becoming damage in disguise.
fn resolve_heal(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let target_id = ctx.primary_target_id.unwrap_or(ctx.actor_id).to_owned();
    let amount = u16::try_from(effect.magnitude).unwrap_or(0);
    apply_heal(ctx, &target_id, amount);
    Ok(())
}

// ---------------------------------------------------------------------------------------
// HealthTransfer
// ---------------------------------------------------------------------------------------

/// HealthTransfer: Zeke's Lifeshare. With a target, transfers health from the actor to
/// that ally; with none, restores the actor from their own pool (the same "no target means
/// self" reading [`resolve_heal`] gives a targetless `Heal`).
///
/// The transfer is **conserving**: health leaves the actor only if the target can actually
/// receive it, bounded by three things at once — `effect.magnitude`, what the actor can
/// spare above 1 HP (`CHARACTERS.md`'s guard against a support killing themselves with a
/// mis-click), and what the target is actually missing. Debiting before checking the third
/// bound destroys health outright: a transfer aimed at a full-health ally would cost the
/// actor their hit points and heal nobody, which is never what a support player intends —
/// this is `command.rs`'s former regression-tested fix, preserved exactly by computing all
/// three bounds from pre-mutation reads before touching either player's health.
fn resolve_health_transfer(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let magnitude = u16::try_from(effect.magnitude).unwrap_or(0);

    let Some(target_id) = ctx.primary_target_id else {
        // No target: restore the actor from their own pool. No debit, so none of the
        // conserving bounds below apply.
        let actor_id = ctx.actor_id.to_owned();
        apply_heal(ctx, &actor_id, magnitude);
        return Ok(());
    };
    let target_id = target_id.to_owned();
    let actor_id = ctx.actor_id.to_owned();

    let sparable = ctx
        .state
        .player(&actor_id)
        .map_or(0, |actor| actor.health.saturating_sub(1));
    let receivable = ctx
        .state
        .player(&target_id)
        .map_or(0, |target| target.max_health.saturating_sub(target.health));
    let transferred = magnitude.min(sparable).min(receivable);

    if transferred > 0 {
        if let Some(actor) = ctx.state.player_mut(&actor_id) {
            actor.health = actor.health.saturating_sub(transferred);
        }
        // `apply_heal` cannot return less than `transferred` here, because `receivable`
        // already bounded it — but it recomputes the actual credit from the target's own
        // state rather than trusting that invariant blindly.
        apply_heal(ctx, &target_id, transferred);
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------
// SelfDamage
// ---------------------------------------------------------------------------------------

/// SelfDamage: Backlash. Damages the acting player for `effect.magnitude` percent of
/// [`BASE_ATTACK`] — the convention [`crate::types::SpecialEffect::magnitude`] documents
/// for scaled damage, and the one `command.rs`'s former `magnitude_percent_to_hp` used.
///
/// Applied unconditionally: backlash can eliminate its own user, exactly as
/// `StrikeAttack::self_damage`'s doc comment states, and this resolver never special-cases
/// the actor to spare them. Distinct from — and additive with — `StrikeAttack::self_damage`
/// itself, which `command.rs` still applies directly as part of the attack shape; an
/// ability can carry both.
fn resolve_self_damage(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let amount = percent_to_hp(effect.magnitude);
    let actor_id = ctx.actor_id.to_owned();
    apply_backlash(ctx, &actor_id, amount);
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------

/// Converts a percent-of-[`BASE_ATTACK`] magnitude into hit points. `None` (only reachable
/// via genuine `i32` overflow, not credible for any real percentage) degrades to zero
/// rather than wrapping — a wrapped damage value would silently become a heal. Mirrors
/// `command.rs`'s former private `magnitude_percent_to_hp` and `attack_mods.rs`'s own
/// `percent_to_hp` (duplicated locally; both are private to their own files).
fn percent_to_hp(percent: i32) -> u16 {
    fixed::scale(BASE_ATTACK, percent, 100)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(0)
}

/// Restores `player_id`'s health by up to `amount`, capped at their max health, and
/// records the actual amount healed into their itemized `DamageEvent`. Mirrors
/// `command.rs`'s former private `heal` helper exactly.
fn apply_heal(ctx: &mut ResolveContext<'_>, player_id: &str, amount: u16) {
    if amount == 0 {
        return;
    }
    // Scoped so the mutable borrow of `ctx.state` (via `player`) ends before
    // `ctx.damage_entry` needs to borrow all of `ctx` again.
    let actual = {
        let Some(player) = ctx.state.player_mut(player_id) else {
            return;
        };
        let missing = player.max_health.saturating_sub(player.health);
        let actual = amount.min(missing);
        player.health = player.health.saturating_add(actual);
        actual
    };

    let entry = ctx.damage_entry(player_id);
    entry.healed = entry.healed.saturating_add(actual);
}

/// Reduces `player_id`'s health by `amount` (saturating — health never goes negative),
/// records the actual amount that landed into their itemized `DamageEvent::backlash`, and
/// marks elimination if health reached zero. Mirrors `command.rs`'s former private
/// `deal_damage` helper, scoped to the one field every effect in this file ever writes.
fn apply_backlash(ctx: &mut ResolveContext<'_>, player_id: &str, amount: u16) {
    if amount == 0 {
        return;
    }
    let (actual, eliminated_now) = {
        let Some(player) = ctx.state.player_mut(player_id) else {
            return;
        };
        let actual = amount.min(player.health);
        player.health = player.health.saturating_sub(amount);
        (actual, player.is_eliminated())
    };

    let entry = ctx.damage_entry(player_id);
    entry.backlash = entry.backlash.saturating_add(actual);
    entry.eliminated = entry.eliminated || eliminated_now;
}

#[cfg(test)]
// A fixture invariant that must hold is stated with `let ... else { panic!() }`, matching
// the precedent set by every other resolver family's own test module; production code in
// this file remains panic-free.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::fixed::FixedPoint;
    use crate::rng::Rng;
    use crate::types::{
        Appearance, DamageEvent, EffectTrigger, MatchPhase, PersistentObject, PlayerState,
        SimulationState, TerrainMask, TerrainOperation,
    };
    use std::collections::BTreeMap;

    fn make_player(id: &str, health: u16, max_health: u16) -> PlayerState {
        PlayerState {
            id: id.to_owned(),
            team: 0,
            health,
            max_health,
            position: FixedPoint::ZERO,
            character_id: "test-character".to_owned(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn test_state(players: Vec<PlayerState>) -> SimulationState {
        SimulationState {
            blocks: Vec::new(),
            simulation_version: 2,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::Resolution,
            active_player_id: "actor".to_owned(),
            wind_per_tick: 0,
            movement_remaining: 0,
            has_attacked_this_turn: false,
            terrain: TerrainMask {
                width: 1,
                height: 1,
                cells: vec![0u8],
            },
            players,
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 1,
        }
    }

    /// Bundles the owned pieces a `ResolveContext` borrows from, so each test can build a
    /// fresh context without repeating six separate `let mut` bindings. Mirrors every other
    /// resolver family's own `Harness`/`Fixture`.
    struct Harness {
        state: SimulationState,
        rng: Rng,
        damage: BTreeMap<String, DamageEvent>,
        terrain_ops: Vec<TerrainOperation>,
        objects_created: Vec<PersistentObject>,
        terrain_cells_removed: u32,
    }

    impl Harness {
        fn new(players: Vec<PlayerState>) -> Self {
            Self {
                state: test_state(players),
                rng: Rng::from_state(1),
                damage: BTreeMap::new(),
                terrain_ops: Vec::new(),
                objects_created: Vec::new(),
                terrain_cells_removed: 0,
            }
        }

        fn ctx<'a>(&'a mut self, primary_target_id: Option<&'a str>) -> ResolveContext<'a> {
            ResolveContext {
                state: &mut self.state,
                rng: &mut self.rng,
                actor_id: "actor",
                primary_target_id,
                secondary_target_id: None,
                impact_point: FixedPoint::ZERO,
                damage: &mut self.damage,
                terrain_ops: &mut self.terrain_ops,
                objects_created: &mut self.objects_created,
                terrain_cells_removed: &mut self.terrain_cells_removed,
            }
        }
    }

    fn heal_effect(magnitude: i32) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Heal,
            magnitude,
            magnitude_secondary: 0,
            duration_turns: 0,
        }
    }

    fn transfer_effect(magnitude: i32) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnFire,
            kind: EffectKind::HealthTransfer,
            magnitude,
            magnitude_secondary: 0,
            duration_turns: 0,
        }
    }

    fn self_damage_effect(magnitude: i32) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnFire,
            kind: EffectKind::SelfDamage,
            magnitude,
            magnitude_secondary: 0,
            duration_turns: 0,
        }
    }

    // -------------------------------------------------------------------------------
    // Heal
    // -------------------------------------------------------------------------------

    #[test]
    fn heal_restores_a_named_target_capped_at_missing_health() {
        let mut harness = Harness::new(vec![
            make_player("actor", 100, 100),
            make_player("target", 990, 999),
        ]);
        let mut ctx = harness.ctx(Some("target"));

        assert_eq!(resolve(&mut ctx, &heal_effect(50)), Ok(()));

        let Some(target) = harness.state.player("target") else {
            panic!("target must exist");
        };
        assert_eq!(
            target.health, 999,
            "heal must cap at missing health, not the raw magnitude"
        );
        let Some(event) = harness.damage.get("target") else {
            panic!("a healed damage entry must be recorded");
        };
        assert_eq!(event.healed, 9);
    }

    #[test]
    fn heal_without_a_target_restores_the_actor() {
        let mut harness = Harness::new(vec![make_player("actor", 50, 100)]);
        let mut ctx = harness.ctx(None);

        assert_eq!(resolve(&mut ctx, &heal_effect(30)), Ok(()));

        let Some(actor) = harness.state.player("actor") else {
            panic!("actor must exist");
        };
        assert_eq!(actor.health, 80);
    }

    // -------------------------------------------------------------------------------
    // HealthTransfer
    // -------------------------------------------------------------------------------

    #[test]
    fn health_transfer_never_reduces_the_actor_below_one_hp() {
        let mut harness = Harness::new(vec![
            make_player("actor", 10, 999),
            make_player("target", 100, 999),
        ]);
        let mut ctx = harness.ctx(Some("target"));

        assert_eq!(resolve(&mut ctx, &transfer_effect(100)), Ok(()));

        let Some(actor) = harness.state.player("actor") else {
            panic!("actor must exist");
        };
        assert_eq!(
            actor.health, 1,
            "transfer must be capped so the actor keeps at least 1 HP"
        );
        let Some(target) = harness.state.player("target") else {
            panic!("target must exist");
        };
        assert_eq!(
            target.health, 109,
            "exactly what left the actor must arrive"
        );
    }

    #[test]
    fn health_transfer_to_a_full_health_ally_moves_nothing() {
        // Regression: a transfer aimed at a healthy ally must not debit the actor when the
        // target can receive nothing — see this file's module doc comment.
        let mut harness = Harness::new(vec![
            make_player("actor", 200, 999),
            make_player("target", 999, 999),
        ]);
        let mut ctx = harness.ctx(Some("target"));

        assert_eq!(resolve(&mut ctx, &transfer_effect(100)), Ok(()));

        let Some(actor) = harness.state.player("actor") else {
            panic!("actor must exist");
        };
        assert_eq!(
            actor.health, 200,
            "a full-health ally can receive nothing, so nothing may leave the actor"
        );
        let Some(target) = harness.state.player("target") else {
            panic!("target must exist");
        };
        assert_eq!(target.health, 999);
        assert!(
            harness.damage.get("actor").is_none_or(|e| e.healed == 0),
            "no healing was actually done"
        );
    }

    #[test]
    fn health_transfer_without_a_target_restores_the_actor_with_no_debit() {
        let mut harness = Harness::new(vec![make_player("actor", 50, 100)]);
        let mut ctx = harness.ctx(None);

        assert_eq!(resolve(&mut ctx, &transfer_effect(30)), Ok(()));

        let Some(actor) = harness.state.player("actor") else {
            panic!("actor must exist");
        };
        assert_eq!(actor.health, 80);
    }

    // -------------------------------------------------------------------------------
    // SelfDamage
    // -------------------------------------------------------------------------------

    #[test]
    fn self_damage_deals_percent_of_base_attack_backlash() {
        let mut harness = Harness::new(vec![make_player("actor", 100, 100)]);
        let mut ctx = harness.ctx(None);

        assert_eq!(resolve(&mut ctx, &self_damage_effect(10)), Ok(()));

        let Some(actor) = harness.state.player("actor") else {
            panic!("actor must exist");
        };
        assert_eq!(actor.health, 90, "10% of BASE_ATTACK (100) is 10 HP");
        let Some(event) = harness.damage.get("actor") else {
            panic!("a backlash damage entry must be recorded");
        };
        assert_eq!(event.backlash, 10);
    }

    #[test]
    fn self_damage_can_eliminate_the_actor() {
        let mut harness = Harness::new(vec![make_player("actor", 5, 100)]);
        let mut ctx = harness.ctx(None);

        assert_eq!(resolve(&mut ctx, &self_damage_effect(10)), Ok(()));

        let Some(actor) = harness.state.player("actor") else {
            panic!("actor must exist");
        };
        assert_eq!(actor.health, 0);
        assert!(actor.is_eliminated());
        let Some(event) = harness.damage.get("actor") else {
            panic!("a backlash damage entry must be recorded");
        };
        assert!(event.eliminated);
    }

    // -------------------------------------------------------------------------------
    // Dispatch safety net
    // -------------------------------------------------------------------------------

    #[test]
    fn resolve_rejects_an_effect_kind_outside_this_family() {
        let mut harness = Harness::new(vec![make_player("actor", 100, 100)]);
        let mut ctx = harness.ctx(None);
        let effect = SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Knockback,
            magnitude: 0,
            magnitude_secondary: 0,
            duration_turns: 0,
        };

        assert!(resolve(&mut ctx, &effect).is_err());
    }
}
