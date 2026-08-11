//! Applies ability-driven damage to destructible terrain blocks (ADR 0005, `todolist.md`
//! P3 — "the damage side").
//!
//! [`crate::blocks`] already proves the pure column-erosion math: given a block's health,
//! [`crate::blocks::solid_columns`] and [`crate::blocks::apply_to_mask`] compute exactly
//! which cells are solid, deterministically, with no side channel. What was still missing
//! is the piece that turns an ability impact into a *set of health changes* — deciding
//! which blocks an impact reaches, how much of `amount` each one absorbs, and then calling
//! back into [`crate::blocks`] so the mask never drifts from health. That is this file's
//! entire job.
//!
//! # A signature deviation, reported rather than invented silently
//!
//! This task's brief sketched `damage_blocks_in_radius(state: &mut SimulationState, ...)`.
//! **`SimulationState` (`crate::types`, frozen per `docs/MODULE_OWNERSHIP.md` rule 2) has
//! no `blocks` field.** ADR 0005 §1 says blocks belong at `SimulationState.blocks, sorted
//! by id`, but that field has not landed: `crate::blocks::TerrainBlock` exists, but nothing
//! in the frozen contract holds a collection of them, and `blocks` is not yet even declared
//! as a module in `lib.rs`. Editing `types.rs` is not this file's authority
//! (`docs/MODULE_OWNERSHIP.md` rule 2: "if a type is genuinely missing, the task stops and
//! reports it rather than inventing a local version").
//!
//! `resolve::attack_mods` hit the same shape of gap for `MultiStrike` (see that module's
//! own doc comment, "The `ResolveContext` gap this file works around") and resolved it by
//! working around the frozen contract in-file rather than editing it, with the workaround
//! documented and flagged as a judgement call rather than presented as settled design. This
//! file follows the same precedent: [`damage_blocks_in_radius`] takes `blocks: &mut
//! [TerrainBlock]` and `mask: &mut TerrainMask` directly instead of `state: &mut
//! SimulationState`. **This is reported as a blocker for the integrator, not silently
//! substituted**: once `SimulationState` gains `pub blocks: Vec<TerrainBlock>` (sorted by
//! id, per the ADR), the call site is exactly `damage_blocks_in_radius(&mut state.blocks,
//! &mut state.terrain, ...)` — a `&mut Vec<T>` argument already coerces to `&mut [T]`, so
//! nothing about this function's signature or body needs to change to make that call work.
//!
//! # Determinism
//!
//! Blocks are visited in ascending `id` order regardless of their position in the input
//! slice — the same "never trust storage order" discipline
//! `resolve::ResolveContext::living_opponent_ids` applies to players. Falloff, when
//! enabled, uses the same linear-in-basis-points ramp `resolve::displacement`'s own
//! (private, so reimplemented here rather than reached across the module-ownership
//! boundary) `falloff` helper uses: full `amount` at the impact point, ramping to zero at
//! `radius`. No square root, no RNG, no wall clock — only `(block, impact, radius, amount,
//! falloff, material_mask)` feeds the result, so replaying the same impact against the same
//! blocks always damages them identically.

use crate::blocks::{TerrainBlock, apply_to_mask, damage_block};
use crate::fixed::{BASIS_POINTS, FixedPoint, POSITION_SCALE, apply_basis_points};
use crate::types::{MaterialMask, TerrainMask};

/// Applies `amount` damage (with optional linear falloff) to every [`TerrainBlock`] in
/// `blocks` whose span intersects the circle of `radius` around `impact`, then rewrites
/// `mask` so its cells match the new health of every block this call actually damaged.
/// Returns the total number of cells cleared (solid before,
/// [`crate::types::Material::Empty`] after) across every block touched.
///
/// - A block whose `material` [`MaterialMask::permits`] refuses is skipped entirely —
///   untouched health, untouched cells. This is how reinforced stone resists a `SOFT`-mask
///   hit, the same rule [`crate::terrain::apply_operation`] already enforces for raw
///   terrain cells (ADR 0005 §3: "that rule now governs block damage too").
/// - A block outside the radius (its closest point farther than `radius` from `impact`) is
///   skipped entirely, at any health. A zero-width or zero-height block (no world
///   footprint) is likewise skipped, mirroring [`apply_to_mask`]'s own treatment of one.
/// - `radius <= 0` is treated as "no radius limit" — every permitted, non-degenerate block
///   is in range — mirroring `resolve::displacement::targets_in_radius`'s convention for
///   the same input shape.
/// - With `falloff`, a block's damage scales down linearly from `amount` at `impact` to
///   `0` at `radius`, based on the block's closest point to `impact` (so an impact that
///   lands *inside* a block's footprint always deals that block the full, unscaled
///   amount). Without it, every in-range, permitted block takes the full `amount`
///   regardless of exactly where in the radius it sits — this is how a `u16::MAX`-damage
///   weapon "levels every block it reaches" (ADR 0005 §3): full damage everywhere in
///   range, not a radial ramp that would leave the rim of the blast barely scratched.
/// - Blocks are visited in ascending `id` order, independent of `blocks`'s actual storage
///   order (see this module's doc comment, "Determinism").
/// - Each damaged block is immediately passed to [`apply_to_mask`] with `impact`'s cell-x,
///   so erosion always erodes toward the side that was actually hit, and the mask can
///   never observe a block's health without also observing its cells in the same call.
pub fn damage_blocks_in_radius(
    blocks: &mut [TerrainBlock],
    mask: &mut TerrainMask,
    impact: FixedPoint,
    radius: i32,
    amount: u16,
    falloff: bool,
    material_mask: MaterialMask,
) -> u32 {
    let (impact_cell_x, _impact_cell_y) = impact.to_cells();
    let mut total_cleared = 0u32;

    // Visit in ascending id order without reordering `blocks` itself: collect (id, index)
    // pairs and sort those, then look each block up by index. A tuple sorts lexically —
    // `id` first, `index` second — so even a malformed map with a repeated id still has a
    // total, storage-order-independent visiting order.
    let mut order: Vec<(u32, usize)> = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id, index))
        .collect();
    order.sort_unstable();

    for (_, index) in order {
        let Some(block) = blocks.get(index).copied() else {
            continue;
        };

        // A block with no footprint has nothing to damage or draw; skip it the same way
        // `blocks::apply_to_mask` already treats a zero-width block as a no-op.
        if block.width_cells == 0 || block.height_cells == 0 {
            continue;
        }
        if !material_mask.permits(block.material) {
            continue;
        }

        let distance_sq = block_distance_squared(&block, impact);
        if radius > 0 {
            let radius_wide = i64::from(radius);
            let radius_sq = radius_wide.saturating_mul(radius_wide);
            if distance_sq > radius_sq {
                continue;
            }
        }

        let applied = if falloff {
            falloff_amount(amount, distance_sq, radius)
        } else {
            amount
        };
        if applied == 0 {
            continue;
        }

        let Some(block_mut) = blocks.get_mut(index) else {
            continue;
        };
        // Absorbed amount is not needed here: `damage_block` already saturates health at
        // zero, and `apply_to_mask` below reads the resulting health directly rather than
        // this return value.
        let _absorbed = damage_block(block_mut, applied);
        let cleared = apply_to_mask(mask, block_mut, Some(impact_cell_x));
        total_cleared = total_cleared.saturating_add(cleared);
    }

    total_cleared
}

/// Squared fixed-point distance from `impact` to the nearest point on `block`'s
/// world-space footprint — the standard axis-aligned-rectangle-vs-point test, computed in
/// `i64` and saturating throughout so a malformed block (an origin near `i32::MAX`, say)
/// degrades to a very large but finite distance rather than overflowing. Zero exactly when
/// `impact` lies inside the block's span.
///
/// Callers must not pass a zero-width or zero-height block: [`damage_blocks_in_radius`]
/// already filters those out before reaching here. `left == right` (or `top == bottom`)
/// would still clamp correctly, but describes a block with nothing to damage, and this
/// function does not re-derive that check.
fn block_distance_squared(block: &TerrainBlock, impact: FixedPoint) -> i64 {
    let scale = i64::from(POSITION_SCALE);
    let left = i64::from(block.origin_cell_x).saturating_mul(scale);
    let top = i64::from(block.origin_cell_y).saturating_mul(scale);
    let right = left.saturating_add(i64::from(block.width_cells).saturating_mul(scale));
    let bottom = top.saturating_add(i64::from(block.height_cells).saturating_mul(scale));

    let impact_x = i64::from(impact.x);
    let impact_y = i64::from(impact.y);
    let clamped_x = impact_x.clamp(left, right);
    let clamped_y = impact_y.clamp(top, bottom);

    let dx = clamped_x.saturating_sub(impact_x);
    let dy = clamped_y.saturating_sub(impact_y);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

/// Scales `amount` by linear falloff from the impact point: full `amount` at
/// `distance_sq == 0`, ramping to `0` at `distance_sq >= radius^2`. Reimplements the same
/// formula `resolve::displacement`'s private `falloff` helper uses (that function is not
/// `pub`, so it cannot be called across the module-ownership boundary — see this file's
/// module doc comment) rather than a different curve, so a block and a character caught in
/// the same blast fall off the same way. `radius <= 0` returns `amount` unscaled,
/// mirroring `resolve::displacement::targets_in_radius`'s "no radius limit" convention for
/// the same input.
fn falloff_amount(amount: u16, distance_sq: i64, radius: i32) -> u16 {
    if radius <= 0 {
        return amount;
    }
    let radius_wide = i64::from(radius);
    // `radius > 0` here, so `radius_sq` is at least 1 on its own; `.max(1)` only guards the
    // divide below against a theoretical future change to this arithmetic, not against any
    // reachable input today.
    let radius_sq = radius_wide.saturating_mul(radius_wide).max(1);
    // `damage_blocks_in_radius` only calls this on a block already confirmed in range
    // (`distance_sq <= radius_sq`); `saturating_sub` keeps this total regardless, so a
    // block passed in out of range degrades to zero remaining rather than underflowing.
    let remaining_sq = radius_sq.saturating_sub(distance_sq);
    let ratio_bp = remaining_sq
        .saturating_mul(i64::from(BASIS_POINTS))
        .checked_div(radius_sq)
        .unwrap_or(0);
    // `ratio_bp` is `remaining_sq / radius_sq` (a fraction `<= 1`) scaled by
    // `BASIS_POINTS`, so it is bounded by `BASIS_POINTS` (10_000) and always fits `i32`;
    // `unwrap_or` names the unreachable-in-practice fallback rather than assuming the
    // proof holds forever.
    let ratio_bp_i32 = i32::try_from(ratio_bp).unwrap_or(BASIS_POINTS);
    let scaled = apply_basis_points(i32::from(amount), ratio_bp_i32).unwrap_or(0);
    u16::try_from(scaled.max(0)).unwrap_or(u16::MAX)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::blocks::solid_columns;
    use crate::terrain::material_at;
    use crate::types::ErosionAxis;
    use crate::types::Material;

    #[expect(
        clippy::too_many_arguments,
        reason = "a test fixture constructor naming every TerrainBlock field directly is clearer \
                  than grouping them into an ad hoc tuple/struct that only this helper would use; \
                  blocks::tests::make_block accepts the same shape minus `id`, which this file's \
                  tests need to vary"
    )]
    fn make_block(
        id: u32,
        origin_x: i32,
        origin_y: i32,
        width: u16,
        height: u16,
        material: Material,
        health: u16,
        max_health: u16,
    ) -> TerrainBlock {
        TerrainBlock {
            id,
            origin_cell_x: origin_x,
            origin_cell_y: origin_y,
            width_cells: width,
            height_cells: height,
            material,
            health,
            max_health,
            erosion_axis: ErosionAxis::default(),
        }
    }

    fn make_mask(width: u32, height: u32) -> TerrainMask {
        let Ok(mask) = crate::terrain::create_mask(width, height, Material::Empty) else {
            panic!("fixture invariant: create_mask must succeed for a valid, non-degenerate size");
        };
        mask
    }

    fn cells(x: i32, y: i32) -> FixedPoint {
        let Some(point) = FixedPoint::from_cells(x, y) else {
            panic!("fixture invariant: from_cells must succeed for small test coordinates");
        };
        point
    }

    /// Establishes `block`'s full pre-damage footprint in `mask`, mirroring what
    /// `map::build_mask` does at match start — so a test's subsequent damage call reports
    /// a real "was solid, is now empty" cleared count instead of growing into empty space.
    fn seed_full_health(mask: &mut TerrainMask, block: &TerrainBlock) {
        let _ = apply_to_mask(mask, block, None);
    }

    fn health_of(blocks: &[TerrainBlock], id: u32) -> Option<u16> {
        blocks
            .iter()
            .find(|block| block.id == id)
            .map(|block| block.health)
    }

    fn solid_columns_of(blocks: &[TerrainBlock], id: u32) -> u16 {
        blocks
            .iter()
            .find(|block| block.id == id)
            .map(solid_columns)
            .unwrap_or(0)
    }

    // ---------------------------------------------------------------------------
    // Small damage: partial destruction, rest standing and walkable
    // ---------------------------------------------------------------------------

    #[test]
    fn small_damage_removes_some_columns_leaving_rest_standing_and_walkable() {
        let block = make_block(1, 0, 0, 8, 2, Material::Soil, 100, 100);
        let mut mask = make_mask(10, 5);
        seed_full_health(&mut mask, &block);
        let mut blocks = vec![block];

        let impact = cells(0, 0); // inside the block's own left edge
        let cleared = damage_blocks_in_radius(
            &mut blocks,
            &mut mask,
            impact,
            20 * POSITION_SCALE,
            25,
            false,
            MaterialMask::SOFT,
        );

        assert_eq!(
            health_of(&blocks, 1),
            Some(75),
            "25 damage off 100 must leave exactly 75"
        );
        assert_eq!(solid_columns_of(&blocks, 1), 6, "ceil(8 * 75 / 100) == 6");
        assert_eq!(cleared, 4, "2 eroded columns * 2 rows == 4 cells cleared");

        // Impact at the left edge erodes the two columns nearest it (0, 1); columns 2..8
        // remain solid and walkable.
        for y in 0..2 {
            assert_eq!(material_at(&mask, 0, y), Material::Empty, "x=0 y={y}");
            assert_eq!(material_at(&mask, 1, y), Material::Empty, "x=1 y={y}");
            for x in 2..8 {
                assert_eq!(material_at(&mask, x, y), Material::Soil, "x={x} y={y}");
            }
        }
    }

    // ---------------------------------------------------------------------------
    // u16::MAX: the launch-default "levels everything it reaches" weapon
    // ---------------------------------------------------------------------------

    #[test]
    fn u16_max_damage_levels_every_block_it_reaches() {
        let block_a = make_block(1, 0, 0, 4, 2, Material::Soil, 200, 200);
        let block_b = make_block(2, 20, 0, 4, 2, Material::Wood, 50, 50);
        let mut mask = make_mask(30, 5);
        seed_full_health(&mut mask, &block_a);
        seed_full_health(&mut mask, &block_b);
        let mut blocks = vec![block_a, block_b];

        let impact = cells(2, 0); // inside block A; block B sits 18 cells away
        let radius = 30 * POSITION_SCALE; // generous enough to reach both
        let cleared = damage_blocks_in_radius(
            &mut blocks,
            &mut mask,
            impact,
            radius,
            u16::MAX,
            false,
            MaterialMask::SOFT,
        );

        assert_eq!(health_of(&blocks, 1), Some(0));
        assert_eq!(health_of(&blocks, 2), Some(0));
        assert_eq!(cleared, 16, "both 4x2 blocks fully cleared: 8 + 8 == 16");
        for x in 0..4 {
            for y in 0..2 {
                assert_eq!(
                    material_at(&mask, x, y),
                    Material::Empty,
                    "block A x={x} y={y}"
                );
            }
        }
        for x in 20..24 {
            for y in 0..2 {
                assert_eq!(
                    material_at(&mask, x, y),
                    Material::Empty,
                    "block B x={x} y={y}"
                );
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Falloff
    // ---------------------------------------------------------------------------

    #[test]
    fn damage_falls_off_with_distance_when_falloff_true() {
        let block_a = make_block(1, 0, 0, 4, 2, Material::Soil, 100, 100);
        let block_b = make_block(2, 10, 0, 4, 2, Material::Soil, 100, 100);
        let mut mask = make_mask(20, 5);
        seed_full_health(&mut mask, &block_a);
        seed_full_health(&mut mask, &block_b);
        let mut blocks = vec![block_a, block_b];

        let impact = cells(0, 0); // exactly inside block A: distance_sq to A is 0
        let radius = 12 * POSITION_SCALE;
        let _cleared = damage_blocks_in_radius(
            &mut blocks,
            &mut mask,
            impact,
            radius,
            40,
            true,
            MaterialMask::SOFT,
        );

        // Worked exactly: distance_sq(A) = 0, so A's ratio is BASIS_POINTS (full 40 dealt).
        // distance_sq(B) = (10 cells)^2 = 10240^2 = 104_857_600; radius_sq = (12 cells)^2 =
        // 12288^2 = 150_994_944; ratio_bp = (150_994_944 - 104_857_600) * 10_000 /
        // 150_994_944 = 3_055 (truncating); scaled = round_divide(40 * 3_055, 10_000) = 12.
        assert_eq!(
            health_of(&blocks, 1),
            Some(60),
            "block at the impact point takes the full 40"
        );
        assert_eq!(
            health_of(&blocks, 2),
            Some(88),
            "block 10 cells away, inside a 12-cell radius, ramps down to 12"
        );
    }

    // ---------------------------------------------------------------------------
    // Material mask: reinforced stone
    // ---------------------------------------------------------------------------

    #[test]
    fn reinforced_stone_survives_soft_mask_hit_entirely() {
        let block = make_block(1, 0, 0, 4, 2, Material::ReinforcedStone, 100, 100);
        let mut mask = make_mask(10, 5);
        seed_full_health(&mut mask, &block);
        let mut blocks = vec![block];

        let impact = cells(1, 0);
        let cleared = damage_blocks_in_radius(
            &mut blocks,
            &mut mask,
            impact,
            20 * POSITION_SCALE,
            u16::MAX,
            false,
            MaterialMask::SOFT,
        );

        assert_eq!(cleared, 0);
        assert_eq!(health_of(&blocks, 1), Some(100));
        for x in 0..4 {
            for y in 0..2 {
                assert_eq!(
                    material_at(&mask, x, y),
                    Material::ReinforcedStone,
                    "x={x} y={y}"
                );
            }
        }
    }

    // ---------------------------------------------------------------------------
    // Radius
    // ---------------------------------------------------------------------------

    #[test]
    fn blocks_outside_radius_are_untouched() {
        let block_near = make_block(1, 0, 0, 4, 2, Material::Soil, 100, 100);
        let block_far = make_block(2, 40, 0, 4, 2, Material::Soil, 100, 100);
        let mut mask = make_mask(50, 5);
        seed_full_health(&mut mask, &block_near);
        seed_full_health(&mut mask, &block_far);
        let mut blocks = vec![block_near, block_far];

        let impact = cells(0, 0);
        let radius = 5 * POSITION_SCALE; // reaches block_near; block_far is 36 cells away
        let cleared = damage_blocks_in_radius(
            &mut blocks,
            &mut mask,
            impact,
            radius,
            50,
            false,
            MaterialMask::SOFT,
        );

        assert_eq!(health_of(&blocks, 1), Some(50));
        assert_eq!(
            health_of(&blocks, 2),
            Some(100),
            "far outside the radius: untouched"
        );
        for x in 40..44 {
            for y in 0..2 {
                assert_eq!(
                    material_at(&mask, x, y),
                    Material::Soil,
                    "untouched block x={x} y={y}"
                );
            }
        }
        assert!(cleared > 0, "the near block must have lost some cells");
    }

    // ---------------------------------------------------------------------------
    // Determinism: id order, not storage order
    // ---------------------------------------------------------------------------

    #[test]
    fn iteration_order_is_by_id_not_by_storage_order() {
        let a = make_block(3, 0, 0, 4, 2, Material::Soil, 100, 100);
        let b = make_block(1, 10, 0, 4, 2, Material::Soil, 100, 100);
        let c = make_block(2, 20, 0, 4, 2, Material::Soil, 100, 100);

        let mut mask1 = make_mask(30, 5);
        seed_full_health(&mut mask1, &a);
        seed_full_health(&mut mask1, &b);
        seed_full_health(&mut mask1, &c);
        let mut blocks1 = vec![a, b, c]; // reverse-of-id storage order

        let mut mask2 = make_mask(30, 5);
        seed_full_health(&mut mask2, &b);
        seed_full_health(&mut mask2, &c);
        seed_full_health(&mut mask2, &a);
        let mut blocks2 = vec![b, c, a]; // ascending-id storage order

        let impact = cells(15, 0);
        let radius = 30 * POSITION_SCALE;

        let cleared1 = damage_blocks_in_radius(
            &mut blocks1,
            &mut mask1,
            impact,
            radius,
            40,
            true,
            MaterialMask::SOFT,
        );
        let cleared2 = damage_blocks_in_radius(
            &mut blocks2,
            &mut mask2,
            impact,
            radius,
            40,
            true,
            MaterialMask::SOFT,
        );

        assert_eq!(cleared1, cleared2);
        for id in [1u32, 2, 3] {
            assert_eq!(
                health_of(&blocks1, id),
                health_of(&blocks2, id),
                "id {id} must match regardless of storage order"
            );
        }
        assert_eq!(
            mask1, mask2,
            "byte-identical masks regardless of the input slice's order"
        );
    }

    // ---------------------------------------------------------------------------
    // The full horizontal test array
    // ---------------------------------------------------------------------------

    #[test]
    fn horizontal_test_array_hit_on_block_three_leaves_siblings_untouched() {
        let map = crate::map::horizontal_test_array();
        let Ok(mut mask) = crate::map::build_mask(&map) else {
            panic!("fixture invariant: horizontal_test_array must build a valid mask");
        };
        let mut blocks = map.blocks;

        let impact = cells(14, 9); // centre of block 3's footprint (origin x=12, width=4)
        let radius = 3 * POSITION_SCALE; // reaches only block 3; blocks 2 and 4 sit 4 cells away
        let cleared = damage_blocks_in_radius(
            &mut blocks,
            &mut mask,
            impact,
            radius,
            50,
            false,
            MaterialMask::SOFT,
        );

        assert_eq!(cleared, 6, "2 eroded columns * 3 rows == 6 cells");
        assert_eq!(
            health_of(&blocks, 3),
            Some(50),
            "block 3 takes exactly half its health"
        );
        assert_eq!(
            solid_columns_of(&blocks, 3),
            2,
            "half of 4 columns, rounded up, is 2"
        );

        for id in [1u32, 2, 4, 5, 6, 7, 8] {
            assert_eq!(
                health_of(&blocks, id),
                Some(100),
                "block {id} must be untouched"
            );
            assert_eq!(
                solid_columns_of(&blocks, id),
                4,
                "block {id} must keep every column"
            );
        }

        // Block 3 (origin x=12) erodes its two centre-most columns (13, 14 — nearest the
        // impact at x=14) and keeps its two edge columns (12, 15) standing, per
        // `blocks::erosion_rank`'s "nearest to the impact erodes first" rule.
        for y in 8..11 {
            assert_eq!(material_at(&mask, 12, y), Material::Soil, "x=12 y={y}");
            assert_eq!(material_at(&mask, 13, y), Material::Empty, "x=13 y={y}");
            assert_eq!(material_at(&mask, 14, y), Material::Empty, "x=14 y={y}");
            assert_eq!(material_at(&mask, 15, y), Material::Soil, "x=15 y={y}");
        }

        // Every sibling block's full footprint remains solid.
        for block in blocks.iter().filter(|block| block.id != 3) {
            for local_x in 0..block.width_cells {
                let x = block.origin_cell_x.saturating_add(i32::from(local_x));
                for y in 8..11 {
                    assert_eq!(
                        material_at(&mask, x, y),
                        Material::Soil,
                        "block id {} x={x} y={y}",
                        block.id
                    );
                }
            }
        }
    }
}
