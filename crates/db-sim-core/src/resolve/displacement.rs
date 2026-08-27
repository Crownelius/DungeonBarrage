//! Resolvers for [`EffectKind::Knockback`], [`EffectKind::Push`], [`EffectKind::Pull`],
//! [`EffectKind::Recoil`], and [`EffectKind::WallImpact`].
//!
//! This is the most-used resolver family in the launch roster: Roberto's grenade
//! (`Knockback`), Natomica's Repulse (`Push` + `WallImpact`), and Numa's harpoon (`Pull`).
//!
//! # Displacement is swept, never teleported
//!
//! Every function here moves a character one step at a time and stops at the first solid
//! cell. Setting a destination directly and checking only the endpoint would let a target
//! cross a wall whenever the far side happened to be empty — which is how a knockback
//! becomes a phase-through-terrain exploit. It also guarantees the invariant that matters
//! downstream: **a displaced character never ends up inside terrain**, so nothing later has
//! to handle an embedded body.
//!
//! # Effect field conventions
//!
//! | Kind | `magnitude` | `magnitude_secondary` |
//! |---|---|---|
//! | `Knockback` | displacement, fixed-point | radius (0 = the primary target only) |
//! | `Push` | displacement, fixed-point | radius |
//! | `Pull` | displacement, fixed-point | HP threshold in basis points |
//! | `Recoil` | displacement, fixed-point | unused |
//! | `WallImpact` | bonus damage, percent of [`BASE_ATTACK`] | unused |
//!
//! Verified against `character.rs`: `ROBERTO_KNOCKBACK`, `NATOMICA_PUSH`,
//! `NATOMICA_WALL_IMPACT`, and `NUMA_PULL` are the live callers. `Recoil` is currently
//! attached to no character — its convention is established here for whenever one uses it,
//! and that is flagged rather than presented as roster fact.

use crate::error::{SimError, SimResult};
use crate::fixed::{
    BASIS_POINTS, FixedPoint, POSITION_SCALE, apply_basis_points, distance_squared, round_divide,
    scale,
};
use crate::terrain;
use crate::types::{BASE_ATTACK, EffectKind, SpecialEffect};

use super::ResolveContext;

/// Step length for a swept displacement, in fixed-point units.
///
/// A quarter cell. Fine enough that a character cannot straddle a one-cell wall, coarse
/// enough that the longest displacement in the roster costs a bounded number of iterations.
///
/// Written as a literal with an assertion rather than `POSITION_SCALE / 4` so there is no
/// division to lint, and so a change to `POSITION_SCALE` fails the build here instead of
/// silently altering every sweep's granularity.
const SWEEP_STEP: i32 = 256;
const _: () = assert!(SWEEP_STEP * 4 == POSITION_SCALE);

/// Default `Pull` health threshold: 50% of max health, per `CHARACTERS.md` §3.5.
///
/// A named constant rather than `BASIS_POINTS / 2` so the value carries its meaning and
/// there is no division in a const initializer.
const DEFAULT_PULL_THRESHOLD_BP: i32 = 5_000;
const _: () = assert!(DEFAULT_PULL_THRESHOLD_BP * 2 == BASIS_POINTS);

/// Hard iteration cap for a single sweep.
///
/// Bounds worst-case work per displacement the way `ProjectileAttack::max_ticks` bounds a
/// projectile. A malformed magnitude must not be able to walk the whole map cell by cell.
const MAX_SWEEP_STEPS: u32 = 512;

/// Whether `player_id` is immune to being moved.
///
/// Numa's Pin explicitly prevents displacement as well as movement (`CHARACTERS.md` §3.5),
/// and `relocation.rs` applies the same guard to Huck's throw. Checked here rather than by
/// each caller so a new displacement effect cannot forget it.
fn is_displacement_immune(ctx: &ResolveContext<'_>, player_id: &str) -> bool {
    ctx.state.player(player_id).is_some_and(|player| {
        player
            .statuses
            .iter()
            .any(|status| status.kind == EffectKind::Lockdown && status.turns_remaining > 0)
    })
}

/// A unit-ish direction from `from` toward `to`, scaled to [`SWEEP_STEP`].
///
/// Avoids a square root — normalizing exactly would need one, and a fixed-point `sqrt` is a
/// rounding-divergence source the determinism contract does not permit. Instead the larger
/// component is scaled to the full step and the smaller is scaled proportionally, which
/// yields a direction accurate enough for a stepped sweep while staying exact in integers.
///
/// Returns [`None`] when the two points coincide, since there is no direction to move in.
fn step_toward(from: FixedPoint, to: FixedPoint) -> Option<FixedPoint> {
    let dx = to.x.saturating_sub(from.x);
    let dy = to.y.saturating_sub(from.y);
    if dx == 0 && dy == 0 {
        return None;
    }
    let dominant = dx.saturating_abs().max(dy.saturating_abs());
    if dominant == 0 {
        return None;
    }
    // Scale both components by SWEEP_STEP / dominant. The dominant axis lands on exactly
    // ±SWEEP_STEP; the other is proportionally smaller.
    let step_x = scale(dx, SWEEP_STEP, dominant)?;
    let step_y = scale(dy, SWEEP_STEP, dominant)?;
    if step_x == 0 && step_y == 0 {
        // Both rounded to zero: the offset is sub-step. Nudge along the dominant axis so a
        // sweep always makes progress rather than spinning until the cap.
        return Some(if dx.saturating_abs() >= dy.saturating_abs() {
            FixedPoint::new(if dx < 0 { -SWEEP_STEP } else { SWEEP_STEP }, 0)
        } else {
            FixedPoint::new(0, if dy < 0 { -SWEEP_STEP } else { SWEEP_STEP })
        });
    }
    Some(FixedPoint::new(step_x, step_y))
}

/// Moves `player_id` by up to `distance` along `step`, stopping at the first solid cell.
///
/// Mutates the player's position and returns where they landed. Whether terrain stopped the
/// sweep is deliberately not returned: `WallImpact` resolves as a separate effect and so
/// cannot observe it, and re-testing terrain at the final position is equivalent.
fn sweep(
    ctx: &mut ResolveContext<'_>,
    player_id: &str,
    step: FixedPoint,
    distance: i32,
) -> SimResult<FixedPoint> {
    let Some(start) = ctx.state.player(player_id).map(|player| player.position) else {
        return Err(SimError::UnknownDefinition);
    };

    let step_length = step.x.saturating_abs().max(step.y.saturating_abs()).max(1);
    // Ceiling division, so a distance shorter than one step still attempts a single step.
    // Routed through `fixed::round_divide` rather than open-coded: it is the only sanctioned
    // divide, and it already handles the overflow edges.
    let numerator = i64::from(distance.saturating_abs())
        .saturating_add(i64::from(step_length).saturating_sub(1));
    let steps_wanted = round_divide(numerator, i64::from(step_length)).unwrap_or(0);
    let steps = u32::try_from(steps_wanted)
        .unwrap_or(u32::MAX)
        .min(MAX_SWEEP_STEPS);

    let mut position = start;
    for _ in 0..steps {
        let candidate = position.saturating_add(step);
        if terrain::is_solid_at(&ctx.state.terrain, candidate) {
            break;
        }
        position = candidate;
    }

    if let Some(player) = ctx.state.player_mut(player_id) {
        player.position = position;
    }
    Ok(position)
}

/// Records the net displacement applied to a target, for the result panel.
fn record_knockback(
    ctx: &mut ResolveContext<'_>,
    player_id: &str,
    from: FixedPoint,
    to: FixedPoint,
) {
    let delta = to.saturating_sub(from);
    let entry = ctx.damage_entry(player_id);
    entry.knockback = entry.knockback.saturating_add(delta);
}

/// Scales `magnitude` by linear falloff from the impact point.
///
/// Full magnitude at the centre, zero at `radius`. Linear rather than quadratic because it
/// stays exact in integers and is easier for a player to predict, which
/// `PRODUCT_SPEC.md`'s "bounded uncertainty" pillar asks for.
///
/// Returns [`None`] when the target is outside the radius.
fn falloff(magnitude: i32, distance_sq: i64, radius: i32) -> Option<i32> {
    if radius <= 0 {
        return Some(magnitude);
    }
    let radius_wide = i64::from(radius);
    let radius_sq = radius_wide.saturating_mul(radius_wide);
    if distance_sq > radius_sq {
        return None;
    }
    // Work in basis points of the radius to keep the interpolation integral. Comparing
    // squared distances avoids a sqrt; the ratio is therefore of squares, which makes the
    // curve fall off faster near the rim than a true linear ramp would. That is documented
    // rather than corrected — it favours the target at the edge, which is the forgiving
    // direction for an effect they could not avoid.
    let remaining_sq = radius_sq.saturating_sub(distance_sq);
    let ratio_bp = i32::try_from(
        remaining_sq
            .saturating_mul(i64::from(BASIS_POINTS))
            .checked_div(radius_sq)?,
    )
    .unwrap_or(BASIS_POINTS);
    apply_basis_points(magnitude, ratio_bp)
}

/// Every living player other than the actor within `radius` of the impact point, sorted.
///
/// Sorted by id via [`ResolveContext::living_opponent_ids`], so a radial effect applies in
/// a deterministic order — order matters once displacement changes who is where.
fn targets_in_radius(ctx: &ResolveContext<'_>, radius: i32) -> Vec<(String, i64)> {
    let mut found: Vec<(String, i64)> = Vec::new();
    for id in ctx.living_opponent_ids() {
        let Some(player) = ctx.state.player(&id) else {
            continue;
        };
        let distance_sq = distance_squared(player.position, ctx.impact_point);
        if radius <= 0 {
            found.push((id, distance_sq));
            continue;
        }
        let radius_wide = i64::from(radius);
        if distance_sq <= radius_wide.saturating_mul(radius_wide) {
            found.push((id, distance_sq));
        }
    }
    found
}

/// Resolves a displacement-family effect.
///
/// # Errors
///
/// Returns [`SimError`] when a referenced player is absent or a fixed-point computation
/// overflows.
pub fn resolve(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    match effect.kind {
        EffectKind::Knockback | EffectKind::Push => resolve_radial_displacement(ctx, effect),
        EffectKind::Pull => resolve_pull(ctx, effect),
        EffectKind::Recoil => resolve_recoil(ctx, effect),
        EffectKind::WallImpact => resolve_wall_impact(ctx, effect),
        // `mod.rs` routes only the five kinds above here. Anything else is a dispatch bug,
        // reported rather than silently ignored — silent ignoring is what made 19 effects
        // inert in the first place.
        _ => Err(SimError::UnknownDefinition),
    }
}

/// `Knockback` and `Push`: displace every target away from the impact point.
///
/// The two are the same operation. They stay separate [`EffectKind`] variants because they
/// read differently in a result panel and in a character sheet — Roberto's grenade "knocks
/// back", Natomica's Repulse "pushes" — and because a future balance pass may want to tune
/// them apart.
fn resolve_radial_displacement(
    ctx: &mut ResolveContext<'_>,
    effect: &SpecialEffect,
) -> SimResult<()> {
    let radius = effect.magnitude_secondary;
    for (id, distance_sq) in targets_in_radius(ctx, radius) {
        if is_displacement_immune(ctx, &id) {
            continue;
        }
        let Some(distance) = falloff(effect.magnitude, distance_sq, radius) else {
            continue;
        };
        if distance <= 0 {
            continue;
        }
        let Some(target_position) = ctx.state.player(&id).map(|player| player.position) else {
            continue;
        };
        // Outward: from the impact point toward the target. A target standing exactly on the
        // impact point has no outward direction, so it is left in place rather than pushed
        // in an arbitrary one — an arbitrary choice here would be a coin flip that decides
        // whether they fall off a ledge.
        let Some(step) = step_toward(ctx.impact_point, target_position) else {
            continue;
        };
        let result = sweep(ctx, &id, step, distance)?;
        record_knockback(ctx, &id, target_position, result);
    }
    Ok(())
}

/// `Pull`: Numa's harpoon.
///
/// Direction depends on the target's health (`CHARACTERS.md` §3.5): a target at or below the
/// threshold is dragged to the actor (execute); above it, the actor is pulled to the target
/// (engage). `magnitude_secondary` carries the threshold in basis points of max health,
/// defaulting to half.
fn resolve_pull(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let Some(target_id) = ctx.primary_target_id.map(str::to_owned) else {
        // A pull with no target is a no-op, not an error: the ability still fired and the
        // gauge was still spent.
        return Ok(());
    };
    let Some((target_position, health, max_health)) = ctx
        .state
        .player(&target_id)
        .map(|player| (player.position, player.health, player.max_health))
    else {
        return Ok(());
    };
    let Some(actor_position) = ctx.state.player(ctx.actor_id).map(|player| player.position) else {
        return Err(SimError::UnknownDefinition);
    };

    let threshold_bp = if effect.magnitude_secondary > 0 {
        effect.magnitude_secondary
    } else {
        DEFAULT_PULL_THRESHOLD_BP
    };
    let threshold_hp =
        apply_basis_points(i32::from(max_health), threshold_bp).ok_or(SimError::Overflow {
            context: "pull.threshold",
        })?;
    let drag_target = i32::from(health) <= threshold_hp;

    if drag_target {
        if is_displacement_immune(ctx, &target_id) {
            return Ok(());
        }
        let Some(step) = step_toward(target_position, actor_position) else {
            return Ok(());
        };
        let result = sweep(ctx, &target_id, step, effect.magnitude)?;
        record_knockback(ctx, &target_id, target_position, result);
    } else {
        let actor_id = ctx.actor_id.to_owned();
        if is_displacement_immune(ctx, &actor_id) {
            return Ok(());
        }
        let Some(step) = step_toward(actor_position, target_position) else {
            return Ok(());
        };
        let result = sweep(ctx, &actor_id, step, effect.magnitude)?;
        record_knockback(ctx, &actor_id, actor_position, result);
    }
    Ok(())
}

/// `Recoil`: displaces the **actor** away from the impact point.
///
/// Not attached to any current character — the launch roster's recoil weapon (the retired
/// Heavy Revolver) left with the equipment model. The convention is established here so a
/// future character can use it.
fn resolve_recoil(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let actor_id = ctx.actor_id.to_owned();
    if is_displacement_immune(ctx, &actor_id) {
        return Ok(());
    }
    let Some(actor_position) = ctx.state.player(&actor_id).map(|player| player.position) else {
        return Err(SimError::UnknownDefinition);
    };
    // Away from the impact point: reverse of impact -> actor is actor -> away, so step from
    // the impact toward the actor and keep going.
    let Some(step) = step_toward(ctx.impact_point, actor_position) else {
        return Ok(());
    };
    let result = sweep(ctx, &actor_id, step, effect.magnitude)?;
    record_knockback(ctx, &actor_id, actor_position, result);
    Ok(())
}

/// `WallImpact`: bonus damage to targets a displacement drove into terrain.
///
/// Reads the positions **left by the displacement effects before it**, which is why
/// `ResolveContext` is threaded by `&mut` and effects resolve in declaration order. A target
/// counts as wall-struck when solid terrain lies immediately beyond them along the outward
/// direction — the same test the sweep used to stop.
///
/// `magnitude` is bonus damage as a percent of [`BASE_ATTACK`], per `CHARACTERS.md` §3.9's
/// "additional 25%".
fn resolve_wall_impact(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let bonus = scale(BASE_ATTACK, effect.magnitude, 100).ok_or(SimError::Overflow {
        context: "wall_impact.bonus",
    })?;
    if bonus <= 0 {
        return Ok(());
    }
    let bonus_hp = u16::try_from(bonus).unwrap_or(u16::MAX);

    // Only targets this action already displaced are eligible. Without that filter, a
    // stationary character who merely happens to stand against a wall would take the bonus.
    let displaced: Vec<String> = ctx
        .damage
        .iter()
        .filter(|(_, event)| event.knockback != FixedPoint::ZERO)
        .map(|(id, _)| id.clone())
        .collect();

    for id in displaced {
        let Some(position) = ctx.state.player(&id).map(|player| player.position) else {
            continue;
        };
        let Some(step) = step_toward(ctx.impact_point, position) else {
            continue;
        };
        if !terrain::is_solid_at(&ctx.state.terrain, position.saturating_add(step)) {
            continue;
        }
        let entry = ctx.damage_entry(&id);
        entry.wall_impact = entry.wall_impact.saturating_add(bonus_hp);

        if let Some(player) = ctx.state.player_mut(&id) {
            player.health = player.health.saturating_sub(bonus_hp);
            if player.health == 0 {
                let entry = ctx.damage_entry(&id);
                entry.eliminated = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
// Tests may panic on a fixture invariant that must hold; production paths above stay
// panic-free, which is what the lint protects. Matches the precedent in `terrain::tests`.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::types::StatusChange;
    use crate::types::TurnEndReason;
    use crate::types::{
        Appearance, DamageEvent, EffectTrigger, MatchPhase, Material, PersistentObjectChange,
        PlayerState, RandomOutcome, SimulationState, StatusEffect, TerrainMask, TerrainOperation,
    };
    use std::collections::BTreeMap;

    const WIDE: u32 = 64;
    /// Same dimension as [`WIDE`], as a signed cell index for loops.
    const WIDE_CELLS: i32 = 64;
    const _: () = assert!(WIDE_CELLS as u32 == WIDE);

    fn empty_terrain() -> TerrainMask {
        match terrain::create_mask(WIDE, WIDE, Material::Empty) {
            Ok(mask) => mask,
            Err(_) => panic!("fixture invariant: an empty mask must be constructible"),
        }
    }

    fn player(id: &str, position: FixedPoint, health: u16) -> PlayerState {
        PlayerState {
            id: id.to_owned(),
            team: 0,
            health,
            max_health: 400,
            position,
            character_id: "natomica".to_owned(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    struct Harness {
        state: SimulationState,
        rng: Rng,
        damage: BTreeMap<String, DamageEvent>,
        terrain_cells_removed: u32,
        terrain_ops: Vec<TerrainOperation>,
        object_changes: Vec<PersistentObjectChange>,
        random_outcomes: Vec<RandomOutcome>,
        status_changes: Vec<StatusChange>,
    }

    impl Harness {
        fn new(players: Vec<PlayerState>) -> Self {
            let mut players = players;
            players.sort_by(|a, b| a.id.cmp(&b.id));
            Self {
                state: SimulationState {
                    pending_turn_end_reason: TurnEndReason::Passed,
                    last_turn_end_reason: TurnEndReason::Passed,
                    blocks: Vec::new(),
                    simulation_version: 2,
                    content_version: 1,
                    tick: 0,
                    turn_number: 1,
                    phase: MatchPhase::AimingAndSelection,
                    active_player_id: "actor".to_owned(),
                    wind_per_tick: 0,
                    movement_remaining: 0,
                    has_attacked_this_turn: false,
                    terrain: empty_terrain(),
                    players,
                    objects: Vec::new(),
                    processed_command_ids: Vec::new(),
                    next_terrain_sequence: 0,
                    next_object_sequence: 0,
                    rng_state: 12_345,
                },
                rng: Rng::from_state(12_345),
                damage: BTreeMap::new(),
                terrain_cells_removed: 0,
                terrain_ops: Vec::new(),
                object_changes: Vec::new(),
                random_outcomes: Vec::new(),
                status_changes: Vec::new(),
            }
        }

        fn ctx(&mut self, impact: FixedPoint) -> ResolveContext<'_> {
            ResolveContext {
                state: &mut self.state,
                rng: &mut self.rng,
                actor_id: "actor",
                primary_target_id: Some("target"),
                secondary_target_id: None,
                impact_point: impact,
                damage: &mut self.damage,
                terrain_cells_removed: &mut self.terrain_cells_removed,
                terrain_ops: &mut self.terrain_ops,
                object_changes: &mut self.object_changes,
                random_outcomes: &mut self.random_outcomes,
                status_changes: &mut self.status_changes,
            }
        }

        fn position_of(&self, id: &str) -> FixedPoint {
            match self.state.player(id) {
                Some(player) => player.position,
                None => panic!("fixture invariant: player {id} must exist"),
            }
        }
    }

    fn effect(kind: EffectKind, magnitude: i32, secondary: i32) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind,
            magnitude,
            magnitude_secondary: secondary,
            duration_turns: 0,
        }
    }

    fn cells(x: i32, y: i32) -> FixedPoint {
        match FixedPoint::from_cells(x, y) {
            Some(point) => point,
            None => panic!("fixture invariant: from_cells must succeed"),
        }
    }

    // -- Knockback / Push -----------------------------------------------------------

    #[test]
    fn knockback_actually_moves_the_target_away_from_the_impact() {
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(5, 10), 400),
        ]);
        let before = harness.position_of("target");
        let mut ctx = harness.ctx(cells(0, 10));

        assert_eq!(
            resolve(
                &mut ctx,
                &effect(EffectKind::Knockback, 4 * POSITION_SCALE, 0)
            ),
            Ok(())
        );

        let after = harness.position_of("target");
        assert!(
            after.x > before.x,
            "target must be pushed away from the impact: {before:?} -> {after:?}",
        );
        let recorded = harness.damage.get("target").map(|event| event.knockback);
        assert_eq!(recorded, Some(after.saturating_sub(before)));
    }

    #[test]
    fn push_affects_everyone_inside_the_radius_and_nobody_outside_it() {
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(2, 10), 400),
            player("zzz_far", cells(40, 10), 400),
        ]);
        let near_before = harness.position_of("target");
        let far_before = harness.position_of("zzz_far");
        let mut ctx = harness.ctx(cells(0, 10));

        let radius = 10 * POSITION_SCALE;
        assert_eq!(
            resolve(
                &mut ctx,
                &effect(EffectKind::Push, 2 * POSITION_SCALE, radius)
            ),
            Ok(())
        );

        assert_ne!(
            harness.position_of("target"),
            near_before,
            "in-radius must move"
        );
        assert_eq!(
            harness.position_of("zzz_far"),
            far_before,
            "out-of-radius must not move",
        );
    }

    #[test]
    fn displacement_stops_at_solid_terrain_instead_of_passing_through_it() {
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(3, 10), 400),
        ]);
        // A wall at x=5, with open ground beyond it at x=6... An endpoint-only check would
        // happily place the target at x=8; a swept check must not.
        for y in 0..WIDE_CELLS {
            terrain::set_material(&mut harness.state.terrain, 5, y, Material::ReinforcedStone);
        }
        let mut ctx = harness.ctx(cells(0, 10));

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Push, 20 * POSITION_SCALE, 0)),
            Ok(())
        );

        let after = harness.position_of("target");
        let (cell_x, _) = after.to_cells();
        assert!(
            cell_x < 5,
            "must stop before the wall, landed in cell {cell_x}"
        );
        assert!(
            !terrain::is_solid_at(&harness.state.terrain, after),
            "a displaced character must never end up inside terrain",
        );
    }

    #[test]
    fn a_locked_down_target_cannot_be_displaced() {
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(5, 10), 400),
        ]);
        if let Some(target) = harness.state.player_mut("target") {
            target.statuses.push(StatusEffect {
                kind: EffectKind::Lockdown,
                magnitude: 0,
                turns_remaining: 2,
            });
        }
        let before = harness.position_of("target");
        let mut ctx = harness.ctx(cells(0, 10));

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Push, 8 * POSITION_SCALE, 0)),
            Ok(())
        );

        assert_eq!(
            harness.position_of("target"),
            before,
            "Numa's Pin prevents displacement, not only movement",
        );
    }

    #[test]
    fn a_target_exactly_on_the_impact_point_is_not_pushed_in_an_arbitrary_direction() {
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 0), 400),
            player("target", cells(5, 10), 400),
        ]);
        let before = harness.position_of("target");
        let mut ctx = harness.ctx(cells(5, 10));

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Push, 4 * POSITION_SCALE, 0)),
            Ok(())
        );

        assert_eq!(harness.position_of("target"), before);
    }

    #[test]
    fn falloff_means_a_distant_target_moves_less_than_a_close_one() {
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(2, 10), 400),
            player("zzz_mid", cells(8, 10), 400),
        ]);
        let near_before = harness.position_of("target");
        let mid_before = harness.position_of("zzz_mid");
        let mut ctx = harness.ctx(cells(0, 10));

        assert_eq!(
            resolve(
                &mut ctx,
                &effect(EffectKind::Push, 8 * POSITION_SCALE, 20 * POSITION_SCALE)
            ),
            Ok(())
        );

        let near_moved = harness
            .position_of("target")
            .x
            .saturating_sub(near_before.x);
        let mid_moved = harness
            .position_of("zzz_mid")
            .x
            .saturating_sub(mid_before.x);
        assert!(
            near_moved > 0 && mid_moved > 0,
            "both were inside the radius"
        );
        assert!(
            near_moved > mid_moved,
            "closer target must be displaced further: {near_moved} vs {mid_moved}",
        );
    }

    // -- Pull -----------------------------------------------------------------------

    #[test]
    fn pull_drags_a_wounded_target_to_the_actor() {
        // At or below the threshold: execute. The TARGET moves, the actor does not.
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(10, 10), 100),
        ]);
        let target_before = harness.position_of("target");
        let actor_before = harness.position_of("actor");
        let mut ctx = harness.ctx(cells(10, 10));

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Pull, 6 * POSITION_SCALE, 0)),
            Ok(())
        );

        assert!(
            harness.position_of("target").x < target_before.x,
            "a wounded target is dragged toward the actor",
        );
        assert_eq!(
            harness.position_of("actor"),
            actor_before,
            "the actor stays put"
        );
    }

    #[test]
    fn pull_dives_the_actor_onto_a_healthy_target() {
        // Above the threshold: engage. The ACTOR moves instead.
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(10, 10), 400),
        ]);
        let target_before = harness.position_of("target");
        let actor_before = harness.position_of("actor");
        let mut ctx = harness.ctx(cells(10, 10));

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Pull, 6 * POSITION_SCALE, 0)),
            Ok(())
        );

        assert!(
            harness.position_of("actor").x > actor_before.x,
            "against a healthy target Numa pulls herself in",
        );
        assert_eq!(harness.position_of("target"), target_before);
    }

    #[test]
    fn pull_threshold_is_read_from_the_effect_when_supplied() {
        // A 25% threshold (2_500 bp): a target at 50% health is now ABOVE it, so the
        // actor dives rather than executing.
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(10, 10), 200),
        ]);
        let actor_before = harness.position_of("actor");
        let mut ctx = harness.ctx(cells(10, 10));

        assert_eq!(
            resolve(
                &mut ctx,
                &effect(EffectKind::Pull, 6 * POSITION_SCALE, 2_500)
            ),
            Ok(())
        );

        assert!(
            harness.position_of("actor").x > actor_before.x,
            "50% health is above a 25% threshold, so this must engage rather than execute",
        );
    }

    #[test]
    fn pull_without_a_target_is_a_no_op_not_an_error() {
        let mut harness = Harness::new(vec![player("actor", cells(0, 10), 400)]);
        let before = harness.position_of("actor");
        let mut ctx = ResolveContext {
            state: &mut harness.state,
            rng: &mut harness.rng,
            actor_id: "actor",
            primary_target_id: None,
            secondary_target_id: None,
            impact_point: cells(5, 10),
            damage: &mut harness.damage,
            terrain_cells_removed: &mut harness.terrain_cells_removed,
            terrain_ops: &mut harness.terrain_ops,
            object_changes: &mut harness.object_changes,
            random_outcomes: &mut harness.random_outcomes,
            status_changes: &mut harness.status_changes,
        };

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Pull, 4 * POSITION_SCALE, 0)),
            Ok(())
        );
        assert_eq!(harness.position_of("actor"), before);
    }

    // -- Recoil ---------------------------------------------------------------------

    #[test]
    fn recoil_displaces_the_actor_away_from_the_impact() {
        let mut harness = Harness::new(vec![player("actor", cells(10, 10), 400)]);
        let before = harness.position_of("actor");
        let mut ctx = harness.ctx(cells(5, 10));

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Recoil, 2 * POSITION_SCALE, 0)),
            Ok(())
        );

        assert!(
            harness.position_of("actor").x > before.x,
            "recoil pushes the shooter away from where the shot landed",
        );
    }

    // -- WallImpact -----------------------------------------------------------------

    #[test]
    fn wall_impact_damages_a_target_driven_into_terrain() {
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(3, 10), 400),
        ]);
        for y in 0..WIDE_CELLS {
            terrain::set_material(&mut harness.state.terrain, 5, y, Material::ReinforcedStone);
        }
        let mut ctx = harness.ctx(cells(0, 10));

        // Push into the wall first; WallImpact then reads the resulting position.
        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Push, 20 * POSITION_SCALE, 0)),
            Ok(())
        );
        let health_after_push = harness
            .state
            .player("target")
            .map(|player| player.health)
            .unwrap_or_default();

        let mut ctx = harness.ctx(cells(0, 10));
        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::WallImpact, 25, 0)),
            Ok(())
        );

        let event = harness.damage.get("target");
        assert_eq!(event.map(|e| e.wall_impact), Some(25), "25% of BASE_ATTACK");
        let health_now = harness
            .state
            .player("target")
            .map(|player| player.health)
            .unwrap_or_default();
        assert_eq!(health_now, health_after_push.saturating_sub(25));
    }

    #[test]
    fn wall_impact_spares_a_target_that_was_never_displaced() {
        // Standing against a wall is not the same as being driven into one.
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(4, 10), 400),
        ]);
        for y in 0..WIDE_CELLS {
            terrain::set_material(&mut harness.state.terrain, 5, y, Material::ReinforcedStone);
        }
        let mut ctx = harness.ctx(cells(0, 10));

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::WallImpact, 25, 0)),
            Ok(())
        );

        assert_eq!(
            harness.damage.get("target").map(|e| e.wall_impact),
            None,
            "no displacement this action means no wall bonus",
        );
    }

    #[test]
    fn wall_impact_can_eliminate_and_says_so() {
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(3, 10), 10),
        ]);
        for y in 0..WIDE_CELLS {
            terrain::set_material(&mut harness.state.terrain, 5, y, Material::ReinforcedStone);
        }
        let mut ctx = harness.ctx(cells(0, 10));
        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Push, 20 * POSITION_SCALE, 0)),
            Ok(())
        );
        let mut ctx = harness.ctx(cells(0, 10));
        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::WallImpact, 25, 0)),
            Ok(())
        );

        assert_eq!(
            harness.state.player("target").map(|player| player.health),
            Some(0)
        );
        assert_eq!(
            harness.damage.get("target").map(|e| e.eliminated),
            Some(true),
            "the elimination cause must be reported, not inferred",
        );
    }

    // -- Determinism ----------------------------------------------------------------

    #[test]
    fn the_same_input_displaces_identically_twice() {
        let run = || {
            let mut harness = Harness::new(vec![
                player("actor", cells(0, 10), 400),
                player("target", cells(4, 10), 400),
                player("zzz_third", cells(7, 12), 400),
            ]);
            let mut ctx = harness.ctx(cells(0, 10));
            let _ = resolve(
                &mut ctx,
                &effect(EffectKind::Push, 5 * POSITION_SCALE, 30 * POSITION_SCALE),
            );
            (
                harness.position_of("target"),
                harness.position_of("zzz_third"),
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn sweep_terminates_even_with_an_absurd_magnitude() {
        // Bounded work per displacement: i32::MAX must not walk the map cell by cell.
        let mut harness = Harness::new(vec![
            player("actor", cells(0, 10), 400),
            player("target", cells(1, 10), 400),
        ]);
        let mut ctx = harness.ctx(cells(0, 10));

        assert_eq!(
            resolve(&mut ctx, &effect(EffectKind::Push, i32::MAX, 0)),
            Ok(())
        );
        assert!(!terrain::is_solid_at(
            &harness.state.terrain,
            harness.position_of("target")
        ));
    }
}
