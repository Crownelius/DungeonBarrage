//! Resolvers for Chill, Lockdown, and Embers, and status application semantics.
//!
//! This file owns two things: the three status-family effect resolvers, and the shared
//! rules for how *any* [`StatusEffect`] is attached to a player. `ARSENAL.md` guardrail 4
//! is binding for every status in the game, not just these three:
//!
//! - A status lasts at most one affected turn and does not stack, **except** Numa's Pin
//!   (Lockdown) and Emi's turret, which are ability durations with explicit published
//!   values. Embers is likewise given an explicit two-turn duration by this task's brief;
//!   `CHARACTERS.md` §6 lists only Pin and the turret as exceptions to the one-turn rule,
//!   which is a real gap in that document worth closing, but the brief's two-turn figure
//!   for Embers is authoritative here (see the reported judgement call).
//! - Reapplying a status of a kind a player already has **refreshes** it — magnitude and
//!   duration are replaced by the new application — rather than adding a second copy or
//!   summing magnitudes.
//! - `PlayerState.statuses` stays sorted by [`EffectKind`] at all times; the canonical
//!   hash encoding depends on it (`hash.rs` also defensively re-sorts before hashing, but
//!   the invariant is maintained here too so any other consumer may rely on it without
//!   re-sorting itself).
//!
//! # Representational note on Embers
//!
//! `ARSENAL.md`'s Cinder Cluster describes Embers as a *location-anchored* hazard: "inner
//! impacts leave clearly marked contact zones… a unit touching a zone takes damage at
//! turn start." The frozen type contract (`docs/MODULE_OWNERSHIP.md` rule 2) has no
//! location-bearing zone entity: [`crate::types::PersistentObjectKind`] is closed over
//! `Turret`, `EmbeddedKnife`, and `GasCloud`, none of which mean "damage zone", and
//! [`StatusEffect`] carries no position. Rather than invent a local type (forbidden) or
//! guess at a variant that does not exist, Embers is modelled here as a per-player status
//! applied to whoever is within the effect's radius of the impact point *at the moment of
//! resolution* — the closest faithful behaviour reachable with existing types. A unit that
//! walks into the same physical space later without a fresh impact is not retroactively
//! marked; only [`crate::types::PersistentObject`]-style world-anchored zones could do
//! that, and adding one is outside this file's authority. This is reported as a judgement
//! call, not silently assumed.
//!
//! The turn-start damage tick itself ("a unit touching one takes damage at turn start")
//! is intentionally **not** implemented in this file. [`tick_statuses`] is scoped by the
//! brief to decrement-and-expire only; dealing the per-turn damage described by a status's
//! `magnitude` is the responsibility of whatever turn scheduler eventually calls it, the
//! same way `displacement.rs` — not this file — is what makes Lockdown *do* anything.
//! `magnitude` on an applied Embers status is that future consumer's damage-per-turn
//! value.

use crate::error::{SimError, SimResult};
use crate::fixed::within_radius;
use crate::types::{EffectKind, PlayerState, SimulationState, SpecialEffect, StatusEffect};

use super::ResolveContext;

/// Resolves a status-family effect: [`EffectKind::Chill`], [`EffectKind::Lockdown`], or
/// [`EffectKind::Embers`].
///
/// # Errors
///
/// Returns an error when a required target is absent or does not resolve to a living
/// player in `ctx.state`. `resolve_effect` in `mod.rs` only ever routes these three kinds
/// here, so the wildcard arm below is defensive rather than a real code path.
pub fn resolve(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    match effect.kind {
        EffectKind::Chill => resolve_chill(ctx, effect),
        EffectKind::Lockdown => resolve_lockdown(ctx, effect),
        EffectKind::Embers => resolve_embers(ctx, effect),
        _ => Err(SimError::OutOfRange {
            field: "effect.kind (not a status-family effect)",
        }),
    }
}

/// Chill: reduces the victim's next-turn movement cap.
///
/// Applied to `ctx.primary_target_id` with `magnitude` and `duration_turns` copied
/// directly from `effect` — the brief states the values come from the effect as-is, with
/// no scaling performed here. Enforcing the "at most one affected turn" guardrail on
/// authored content is a validation-time concern (character/content review), not a
/// runtime one; this resolver transcribes whatever duration the ability was authored with.
fn resolve_chill(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let Some(target_id) = ctx.primary_target_id else {
        return Err(SimError::OutOfRange {
            field: "chill.primary_target_id",
        });
    };
    let Some(target) = ctx.state.player_mut(target_id) else {
        return Err(SimError::UnknownDefinition);
    };
    apply_status(
        target,
        StatusEffect {
            kind: EffectKind::Chill,
            magnitude: effect.magnitude,
            turns_remaining: effect.duration_turns,
        },
    );
    Ok(())
}

/// Lockdown: Numa's Pin. Locks a character in place for `effect.duration_turns` turns.
///
/// A locked character may still aim, fire, and use abilities — this resolver only ever
/// attaches the status; `displacement.rs` is what reads it and refuses movement or forced
/// displacement while it is present.
fn resolve_lockdown(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let Some(target_id) = ctx.primary_target_id else {
        return Err(SimError::OutOfRange {
            field: "lockdown.primary_target_id",
        });
    };
    let Some(target) = ctx.state.player_mut(target_id) else {
        return Err(SimError::UnknownDefinition);
    };
    apply_status(
        target,
        StatusEffect {
            kind: EffectKind::Lockdown,
            magnitude: effect.magnitude,
            turns_remaining: effect.duration_turns,
        },
    );
    Ok(())
}

/// Embers: marks every living player within `effect.magnitude_secondary` (the zone
/// radius, in fixed-point units) of `ctx.impact_point` with an Embers status carrying
/// `effect.magnitude` (per-turn damage, for a future scheduler to apply) and
/// `effect.duration_turns` (two turns, per this task's brief).
///
/// See the module-level "Representational note on Embers" for why this is
/// proximity-at-resolution-time rather than a standing world zone.
fn resolve_embers(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let impact_point = ctx.impact_point;
    for player in &mut ctx.state.players {
        // Eliminated players cannot be "touched" by anything further; skipping them also
        // keeps a dead roster slot from accumulating statuses that no scheduler will ever
        // clear for it.
        if player.is_eliminated() {
            continue;
        }
        if within_radius(player.position, impact_point, effect.magnitude_secondary) {
            apply_status(
                player,
                StatusEffect {
                    kind: EffectKind::Embers,
                    magnitude: effect.magnitude,
                    turns_remaining: effect.duration_turns,
                },
            );
        }
    }
    Ok(())
}

/// Attaches `status` to `player`, enforcing the refresh-not-stack rule shared by every
/// status in the game (`ARSENAL.md` guardrail 4): a status of the same `kind` already
/// present has its magnitude and duration **replaced** by the new application; it is
/// never duplicated and its magnitude is never summed with the old one.
///
/// Re-sorts `player.statuses` by `kind` after mutating rather than searching for a sorted
/// insertion point — simplest to reason about, and irrelevant in cost at the handful of
/// simultaneous statuses a single character can realistically carry.
fn apply_status(player: &mut PlayerState, status: StatusEffect) {
    if let Some(existing) = player
        .statuses
        .iter_mut()
        .find(|existing| existing.kind == status.kind)
    {
        *existing = status;
    } else {
        player.statuses.push(status);
    }
    player.statuses.sort_by_key(|s| s.kind);
}

/// Decrements `turns_remaining` on every status on every player by one, removing any that
/// reach zero, and leaves `statuses` sorted by `kind`.
///
/// Nothing calls this yet — it is here for the turn scheduler that will call it once per
/// turn during [`crate::types::MatchPhase::StatusResolution`]. A status system that cannot
/// expire is a bug waiting for that scheduler to exist, so it is written and tested now
/// rather than deferred alongside it.
///
/// `retain` preserves the relative order of the elements it keeps, and this function never
/// introduces a new status kind (only [`apply_status`] does that), so the vector's
/// sortedness from before the call carries through without an explicit re-sort.
pub fn tick_statuses(state: &mut SimulationState) {
    for player in &mut state.players {
        for status in &mut player.statuses {
            // Saturating rather than checked: a status already at zero should already
            // have been removed by a prior tick, but if one somehow survives at zero,
            // wrapping to `u8::MAX` would resurrect it as a near-permanent status. Sitting
            // at zero is the only possible saturation case, so this can never mask a real
            // decrement being silently dropped.
            status.turns_remaining = status.turns_remaining.saturating_sub(1);
        }
        player.statuses.retain(|status| status.turns_remaining > 0);
    }
}

#[cfg(test)]
// A fixture invariant that must hold is stated with `let ... else { panic!() }` because it
// documents the expectation better than threading a `Result` through every test. Matches
// the precedent in `command.rs::tests`. Production code in this module remains panic-free.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::fixed::FixedPoint;
    use crate::rng::Rng;
    use crate::types::TurnEndReason;
    use crate::types::{
        Appearance, DamageEvent, EffectTrigger, MatchPhase, PersistentObject, TerrainMask,
        TerrainOperation,
    };
    use std::collections::BTreeMap;

    // -----------------------------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------------------------

    fn make_player(id: &str, position: FixedPoint) -> PlayerState {
        PlayerState {
            id: id.to_string(),
            team: 0,
            health: 100,
            max_health: 100,
            position,
            character_id: "test-character".to_string(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn test_state(players: Vec<PlayerState>) -> SimulationState {
        SimulationState {
            pending_turn_end_reason: TurnEndReason::Passed,
            last_turn_end_reason: TurnEndReason::Passed,
            blocks: Vec::new(),
            simulation_version: 2,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::Resolution,
            active_player_id: "actor".to_string(),
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

    fn lockdown_effect(magnitude: i32, duration_turns: u8) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnFire,
            kind: EffectKind::Lockdown,
            magnitude,
            magnitude_secondary: 0,
            duration_turns,
        }
    }

    fn chill_effect(magnitude: i32, duration_turns: u8) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Chill,
            magnitude,
            magnitude_secondary: 0,
            duration_turns,
        }
    }

    fn embers_effect(magnitude: i32, radius: i32, duration_turns: u8) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Embers,
            magnitude,
            magnitude_secondary: radius,
            duration_turns,
        }
    }

    /// Bundles the owned pieces a `ResolveContext` borrows from, so each test can build a
    /// fresh context without repeating every field.
    struct Harness {
        state: SimulationState,
        rng: Rng,
        damage: BTreeMap<String, DamageEvent>,
        terrain_cells_removed: u32,
        terrain_ops: Vec<TerrainOperation>,
        objects_created: Vec<PersistentObject>,
    }

    impl Harness {
        fn new(players: Vec<PlayerState>) -> Self {
            Self {
                state: test_state(players),
                rng: Rng::from_state(1),
                damage: BTreeMap::new(),
                terrain_cells_removed: 0,
                terrain_ops: Vec::new(),
                objects_created: Vec::new(),
            }
        }

        fn ctx<'a>(
            &'a mut self,
            actor_id: &'a str,
            primary_target_id: Option<&'a str>,
            impact_point: FixedPoint,
        ) -> ResolveContext<'a> {
            ResolveContext {
                state: &mut self.state,
                rng: &mut self.rng,
                actor_id,
                primary_target_id,
                secondary_target_id: None,
                impact_point,
                damage: &mut self.damage,
                terrain_cells_removed: &mut self.terrain_cells_removed,
                terrain_ops: &mut self.terrain_ops,
                objects_created: &mut self.objects_created,
            }
        }
    }

    fn statuses_of<'a>(state: &'a SimulationState, player_id: &str) -> &'a [StatusEffect] {
        let Some(player) = state.player(player_id) else {
            panic!("fixture invariant: player `{player_id}` must exist");
        };
        &player.statuses
    }

    // -----------------------------------------------------------------------------------
    // Lockdown
    // -----------------------------------------------------------------------------------

    #[test]
    fn lockdown_applies_status_with_duration_from_effect() {
        let mut harness = Harness::new(vec![
            make_player("actor", FixedPoint::ZERO),
            make_player("target", FixedPoint::ZERO),
        ]);
        let effect = lockdown_effect(2, 2);
        let mut ctx = harness.ctx("actor", Some("target"), FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let statuses = statuses_of(&harness.state, "target");
        assert_eq!(statuses.len(), 1);
        let Some(status) = statuses.first() else {
            panic!("status must be present after application");
        };
        assert_eq!(status.kind, EffectKind::Lockdown);
        assert_eq!(status.magnitude, 2);
        assert_eq!(status.turns_remaining, 2);
    }

    #[test]
    fn lockdown_reapplication_refreshes_rather_than_stacks_or_sums() {
        let mut harness = Harness::new(vec![
            make_player("actor", FixedPoint::ZERO),
            make_player("target", FixedPoint::ZERO),
        ]);

        {
            let mut ctx = harness.ctx("actor", Some("target"), FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &lockdown_effect(2, 2)), Ok(()));
        }
        // Simulate Numa's Barbed Line passive re-pinning with a longer duration.
        {
            let mut ctx = harness.ctx("actor", Some("target"), FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &lockdown_effect(2, 3)), Ok(()));
        }

        let statuses = statuses_of(&harness.state, "target");
        // Exactly one Lockdown status: never a second copy.
        assert_eq!(statuses.len(), 1);
        let Some(status) = statuses.first() else {
            panic!("status must be present after application");
        };
        // Refreshed to the new duration, not summed to 2 + 3 = 5.
        assert_eq!(status.turns_remaining, 3);
        assert_eq!(status.magnitude, 2);
    }

    #[test]
    fn lockdown_without_primary_target_is_an_error() {
        let mut harness = Harness::new(vec![make_player("actor", FixedPoint::ZERO)]);
        let mut ctx = harness.ctx("actor", None, FixedPoint::ZERO);

        assert_eq!(
            resolve(&mut ctx, &lockdown_effect(2, 2)),
            Err(SimError::OutOfRange {
                field: "lockdown.primary_target_id"
            })
        );
    }

    #[test]
    fn lockdown_against_unknown_target_is_an_error() {
        let mut harness = Harness::new(vec![make_player("actor", FixedPoint::ZERO)]);
        let mut ctx = harness.ctx("actor", Some("ghost"), FixedPoint::ZERO);

        assert_eq!(
            resolve(&mut ctx, &lockdown_effect(2, 2)),
            Err(SimError::UnknownDefinition)
        );
    }

    // -----------------------------------------------------------------------------------
    // Chill
    // -----------------------------------------------------------------------------------

    #[test]
    fn chill_applies_movement_cap_reduction_with_magnitude_and_duration_from_effect() {
        let mut harness = Harness::new(vec![
            make_player("actor", FixedPoint::ZERO),
            make_player("target", FixedPoint::ZERO),
        ]);
        // Frostfall Mortar (ARSENAL.md): reduces next-turn movement cap by 2 BW, one turn.
        let effect = chill_effect(2 * crate::fixed::BODY_WIDTH, 1);
        let mut ctx = harness.ctx("actor", Some("target"), FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let statuses = statuses_of(&harness.state, "target");
        assert_eq!(statuses.len(), 1);
        let Some(status) = statuses.first() else {
            panic!("status must be present after application");
        };
        assert_eq!(status.kind, EffectKind::Chill);
        assert_eq!(status.magnitude, 2 * crate::fixed::BODY_WIDTH);
        assert_eq!(status.turns_remaining, 1);
    }

    #[test]
    fn chill_without_primary_target_is_an_error() {
        let mut harness = Harness::new(vec![make_player("actor", FixedPoint::ZERO)]);
        let mut ctx = harness.ctx("actor", None, FixedPoint::ZERO);

        assert_eq!(
            resolve(&mut ctx, &chill_effect(2, 1)),
            Err(SimError::OutOfRange {
                field: "chill.primary_target_id"
            })
        );
    }

    // -----------------------------------------------------------------------------------
    // Embers
    // -----------------------------------------------------------------------------------

    #[test]
    fn embers_marks_units_within_radius_of_the_impact_point() {
        let radius = 4 * crate::fixed::BODY_WIDTH;
        let inside = make_player("inside", FixedPoint::new(radius, 0)); // exactly on the boundary
        let outside = make_player("outside", FixedPoint::new(radius + 1, 0)); // just past it
        let mut harness = Harness::new(vec![
            make_player("actor", FixedPoint::ZERO),
            inside,
            outside,
        ]);
        // Cinder Cluster (ARSENAL.md): 5 damage, two-turn zone.
        let effect = embers_effect(5, radius, 2);
        let mut ctx = harness.ctx("actor", None, FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let inside_statuses = statuses_of(&harness.state, "inside");
        assert_eq!(inside_statuses.len(), 1);
        let Some(inside_status) = inside_statuses.first() else {
            panic!("status must be present for the player inside the zone");
        };
        assert_eq!(inside_status.kind, EffectKind::Embers);
        assert_eq!(inside_status.magnitude, 5);
        assert_eq!(inside_status.turns_remaining, 2);

        assert!(statuses_of(&harness.state, "outside").is_empty());
    }

    #[test]
    fn embers_skips_eliminated_players() {
        let mut dead = make_player("dead", FixedPoint::ZERO);
        dead.health = 0;
        let mut harness = Harness::new(vec![make_player("actor", FixedPoint::ZERO), dead]);
        let effect = embers_effect(5, crate::fixed::BODY_WIDTH, 2);
        let mut ctx = harness.ctx("actor", None, FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        assert!(statuses_of(&harness.state, "dead").is_empty());
    }

    #[test]
    fn embers_reapplication_refreshes_rather_than_stacking_damage() {
        let victim = make_player("victim", FixedPoint::ZERO);
        let mut harness = Harness::new(vec![make_player("actor", FixedPoint::ZERO), victim]);
        let first = embers_effect(5, crate::fixed::BODY_WIDTH, 2);
        let second = embers_effect(5, crate::fixed::BODY_WIDTH, 2);

        {
            let mut ctx = harness.ctx("actor", None, FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &first), Ok(()));
        }
        // A second overlapping bomblet lands on the same spot next impact.
        {
            let mut ctx = harness.ctx("actor", None, FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &second), Ok(()));
        }

        let statuses = statuses_of(&harness.state, "victim");
        // One zone's worth of marking, never two stacked copies with summed damage.
        assert_eq!(statuses.len(), 1);
        let Some(status) = statuses.first() else {
            panic!("status must be present after application");
        };
        assert_eq!(status.magnitude, 5);
    }

    // -----------------------------------------------------------------------------------
    // Sort invariant
    // -----------------------------------------------------------------------------------

    #[test]
    fn statuses_stay_sorted_by_kind_regardless_of_application_order() {
        let mut harness = Harness::new(vec![
            make_player("actor", FixedPoint::ZERO),
            make_player("target", FixedPoint::ZERO),
        ]);

        // Apply in an order deliberately different from EffectKind's declaration order
        // (Chill < Embers < Lockdown, per the enum's derived Ord) to prove the vector is
        // sorted afterwards rather than merely append-ordered.
        {
            let mut ctx = harness.ctx("actor", Some("target"), FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &lockdown_effect(1, 2)), Ok(()));
        }
        {
            let mut ctx = harness.ctx("actor", None, FixedPoint::ZERO);
            assert_eq!(
                resolve(&mut ctx, &embers_effect(5, crate::fixed::BODY_WIDTH, 2)),
                Ok(())
            );
        }
        {
            let mut ctx = harness.ctx("actor", Some("target"), FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &chill_effect(1, 1)), Ok(()));
        }

        let statuses = statuses_of(&harness.state, "target");
        let kinds: Vec<EffectKind> = statuses.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![EffectKind::Chill, EffectKind::Embers, EffectKind::Lockdown]
        );
        // Independently confirm sortedness rather than hard-coding the expectation twice.
        assert!(kinds.is_sorted());
    }

    // -----------------------------------------------------------------------------------
    // tick_statuses
    // -----------------------------------------------------------------------------------

    #[test]
    fn tick_statuses_decrements_and_removes_at_zero() {
        let mut player = make_player("target", FixedPoint::ZERO);
        player.statuses.push(StatusEffect {
            kind: EffectKind::Chill,
            magnitude: 1,
            turns_remaining: 1,
        });
        let mut state = test_state(vec![player]);

        tick_statuses(&mut state);

        assert!(statuses_of(&state, "target").is_empty());
    }

    #[test]
    fn a_status_survives_exactly_its_stated_number_of_turns() {
        // Lockdown applied for 2 turns must still be present after one tick and gone
        // after two — off by neither one.
        let mut harness = Harness::new(vec![
            make_player("actor", FixedPoint::ZERO),
            make_player("target", FixedPoint::ZERO),
        ]);
        {
            let mut ctx = harness.ctx("actor", Some("target"), FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &lockdown_effect(2, 2)), Ok(()));
        }
        assert_eq!(statuses_of(&harness.state, "target").len(), 1);

        tick_statuses(&mut harness.state);
        let statuses = statuses_of(&harness.state, "target");
        assert_eq!(statuses.len(), 1, "must still be present after one tick");
        let Some(status) = statuses.first() else {
            panic!("status must be present after one tick");
        };
        assert_eq!(status.turns_remaining, 1);

        tick_statuses(&mut harness.state);
        assert!(
            statuses_of(&harness.state, "target").is_empty(),
            "must be gone after the second tick"
        );
    }

    #[test]
    fn tick_statuses_leaves_survivors_sorted_by_kind() {
        let mut player = make_player("target", FixedPoint::ZERO);
        player.statuses = vec![
            StatusEffect {
                kind: EffectKind::Chill,
                magnitude: 1,
                turns_remaining: 3,
            },
            StatusEffect {
                kind: EffectKind::Lockdown,
                magnitude: 1,
                turns_remaining: 1,
            },
        ];
        let mut state = test_state(vec![player]);

        tick_statuses(&mut state); // Lockdown expires; Chill survives with 2 remaining.

        let statuses = statuses_of(&state, "target");
        assert_eq!(statuses.len(), 1);
        let Some(status) = statuses.first() else {
            panic!("Chill must survive the tick");
        };
        assert_eq!(status.kind, EffectKind::Chill);
        assert_eq!(status.turns_remaining, 2);
    }

    #[test]
    fn tick_statuses_decrements_independently_per_player() {
        let mut a = make_player("a", FixedPoint::ZERO);
        a.statuses.push(StatusEffect {
            kind: EffectKind::Chill,
            magnitude: 1,
            turns_remaining: 2,
        });
        let mut b = make_player("b", FixedPoint::ZERO);
        b.statuses.push(StatusEffect {
            kind: EffectKind::Embers,
            magnitude: 5,
            turns_remaining: 1,
        });
        let mut state = test_state(vec![a, b]);

        tick_statuses(&mut state);

        assert_eq!(statuses_of(&state, "a").len(), 1);
        assert!(statuses_of(&state, "b").is_empty());
    }
}
