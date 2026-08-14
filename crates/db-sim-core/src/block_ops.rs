//! Routing terrain operations through block health.
//!
//! [`apply_operation`] is the entry point every ability must use instead of calling
//! [`crate::terrain::apply_operation`] directly. It is what closes the drift ADR 0005 exists
//! to prevent: without it, a crater carves a hole through a block's cells while the block
//! still reports full health, so "percent health equals percent surface" quietly stops being
//! true the first time anything explodes.
//!
//! # How it works
//!
//! 1. **Carve normally.** Background terrain — anything no block covers — keeps its literal
//!    crater, which is what makes a landscape read like a landscape.
//! 2. **Measure what the carve took from each block.** That count *is* the overlap, so no
//!    shape-membership maths is duplicated here, and the material mask is honoured for free:
//!    [`crate::terrain::apply_operation`] already refuses to clear a material the mask
//!    forbids, so a reinforced block reports zero cleared and takes no damage.
//! 3. **Convert the overlap to proportional damage and re-derive the block from health.**
//!
//! Step 3 overwrites the crater *inside* a block's span. That is deliberate: a block does not
//! get holes, it shrinks (ADR 0005 §2). The amount removed still matches what the explosion
//! would have taken — only its shape is governed by health.
//!
//! Blocks are visited in ascending `id`, never storage order, so the result is identical on
//! every machine.

use crate::blocks::{TerrainBlock, apply_to_mask, damage_block, solid_columns};
use crate::terrain::material_at;
use crate::types::{SimulationState, TerrainMask, TerrainOperation, TerrainShape};

/// A cell-space rectangle, inclusive on both ends.
type CellRect = (i32, i32, i32, i32);

/// The cells a terrain operation could touch, before clipping to the mask.
fn operation_bounds(op: &TerrainOperation) -> CellRect {
    match op.shape {
        TerrainShape::SubtractCircle {
            center,
            radius_cells,
        } => {
            let (cx, cy) = center.to_cells();
            let r = i32::from(radius_cells);
            (
                cx.saturating_sub(r),
                cy.saturating_sub(r),
                cx.saturating_add(r),
                cy.saturating_add(r),
            )
        }
        TerrainShape::SubtractCapsule {
            start,
            end,
            radius_cells,
        } => {
            let (sx, sy) = start.to_cells();
            let (ex, ey) = end.to_cells();
            let r = i32::from(radius_cells);
            (
                sx.min(ex).saturating_sub(r),
                sy.min(ey).saturating_sub(r),
                sx.max(ex).saturating_add(r),
                sy.max(ey).saturating_add(r),
            )
        }
    }
}

/// The cells a block occupies.
fn block_bounds(block: &TerrainBlock) -> CellRect {
    let last_x = block
        .origin_cell_x
        .saturating_add(i32::from(block.width_cells))
        .saturating_sub(1);
    let last_y = block
        .origin_cell_y
        .saturating_add(i32::from(block.height_cells))
        .saturating_sub(1);
    (block.origin_cell_x, block.origin_cell_y, last_x, last_y)
}

/// Whether two inclusive rectangles share any cell.
const fn rects_overlap(a: CellRect, b: CellRect) -> bool {
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

/// The smallest rectangle containing both.
const fn union_rect(a: CellRect, b: CellRect) -> CellRect {
    (
        if a.0 < b.0 { a.0 } else { b.0 },
        if a.1 < b.1 { a.1 } else { b.1 },
        if a.2 > b.2 { a.2 } else { b.2 },
        if a.3 > b.3 { a.3 } else { b.3 },
    )
}

/// Counts solid cells inside `rect`.
///
/// Iterates with an explicit guard against `i32` wraparound rather than a `for` range, so a
/// malformed rectangle reaching the integer extremes terminates instead of looping forever.
fn count_solid(mask: &TerrainMask, rect: CellRect) -> u32 {
    let (min_x, min_y, max_x, max_y) = rect;
    let mut solid = 0u32;
    let mut y = min_y;
    loop {
        if y > max_y {
            break;
        }
        let mut x = min_x;
        loop {
            if x > max_x {
                break;
            }
            if material_at(mask, x, y).is_solid() {
                solid = solid.saturating_add(1);
            }
            let Some(next) = x.checked_add(1) else {
                break;
            };
            x = next;
        }
        let Some(next) = y.checked_add(1) else {
            break;
        };
        y = next;
    }
    solid
}

/// How many of `block`'s cells should be solid at its current health.
fn expected_solid_cells(block: &TerrainBlock) -> u32 {
    u32::from(solid_columns(block)).saturating_mul(u32::from(block.height_cells))
}

/// Ceiling division that never divides by zero and never wraps.
fn ceil_div(numerator: u64, denominator: u64) -> Option<u64> {
    if denominator == 0 {
        return None;
    }
    let adjusted = numerator.checked_add(denominator.checked_sub(1)?)?;
    // `checked_div` rather than `/`: the zero case is already excluded above, and this keeps
    // every arithmetic step checked without a `clippy::integer_division` suppression.
    adjusted.checked_div(denominator)
}

/// Damage proportional to the share of a block's cells an operation cleared.
///
/// A crater taking a third of a block's cells costs it a third of its maximum health, so
/// "percent health equals percent surface" survives contact with explosions.
fn proportional_damage(block: &TerrainBlock, cleared: u32, total_cells: u32) -> u16 {
    let numerator = u64::from(block.max_health).saturating_mul(u64::from(cleared));
    let Some(scaled) = ceil_div(numerator, u64::from(total_cells)) else {
        return 0;
    };
    u16::try_from(scaled).unwrap_or(u16::MAX)
}

/// Applies a terrain operation with block health as the authority.
///
/// Returns the net number of cells that went from solid to empty. See the module docs for
/// why this exists and what it guarantees.
pub fn apply_operation(state: &mut SimulationState, op: &TerrainOperation) -> u32 {
    let op_rect = operation_bounds(op);

    // Measure over the operation's footprint plus every block it touches, because re-deriving
    // a block rewrites its whole span — which can extend beyond the crater.
    let mut measured = op_rect;
    for block in &state.blocks {
        let rect = block_bounds(block);
        if rects_overlap(op_rect, rect) {
            measured = union_rect(measured, rect);
        }
    }

    let before = count_solid(&state.terrain, measured);
    // The raw count is deliberately discarded: it describes the carve alone, and the
    // re-derivation below restores any cell health says should still stand. The number this
    // function returns is measured from the mask afterwards, so it reflects the net result.
    let _carved = crate::terrain::apply_operation(&mut state.terrain, op);

    let (impact_x, _) = match op.shape {
        TerrainShape::SubtractCircle { center, .. } => center.to_cells(),
        // For a bore, the far end is where it stopped — the point damage radiates from.
        TerrainShape::SubtractCapsule { end, .. } => end.to_cells(),
    };

    // Take the list so blocks can be mutated while the mask is borrowed; restored below.
    let mut blocks = core::mem::take(&mut state.blocks);
    let mut order: Vec<(u32, usize)> = blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id, index))
        .collect();
    // Sort the visiting order rather than the list itself, so `state.blocks` keeps whatever
    // order it had. A tuple sorts by id first, so even a malformed map with a duplicated id
    // still has a total, storage-order-independent order.
    order.sort_unstable();

    for (_, index) in order {
        let Some(block) = blocks.get_mut(index) else {
            continue;
        };
        if !rects_overlap(op_rect, block_bounds(block)) {
            continue;
        }
        let total_cells =
            u32::from(block.width_cells).saturating_mul(u32::from(block.height_cells));
        if total_cells == 0 {
            continue;
        }

        // What the carve actually took from this block. Zero for a material the operation's
        // mask forbids, which is how reinforced stone resists block damage too.
        let remaining_here = count_solid(&state.terrain, block_bounds(block));
        let cleared_here = expected_solid_cells(block).saturating_sub(remaining_here);
        if cleared_here > 0 {
            let damage = proportional_damage(block, cleared_here, total_cells);
            damage_block(block, damage);
        }

        // Re-derive from health whether or not damage landed: this restores any cell the
        // carve took that health says should still be standing, which is what stops the two
        // representations drifting apart.
        apply_to_mask(&mut state.terrain, block, Some(impact_x));
    }
    state.blocks = blocks;

    let after = count_solid(&state.terrain, measured);
    before.saturating_sub(after)
}

#[cfg(test)]
// Tests may panic on a fixture invariant that must hold; production paths above may not.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::fixed::FixedPoint;
    use crate::terrain::create_mask;
    use crate::types::{ErosionAxis, MatchPhase, Material, MaterialMask};

    fn state_with(blocks: Vec<TerrainBlock>, fill: Material) -> SimulationState {
        let Ok(terrain) = create_mask(48, 24, fill) else {
            panic!("fixture invariant: mask must build");
        };
        let mut state = SimulationState {
            simulation_version: 2,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::Resolution,
            active_player_id: String::new(),
            wind_per_tick: 0,
            movement_remaining: 0,
            has_attacked_this_turn: false,
            terrain,
            blocks,
            players: Vec::new(),
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 1,
        };
        // Start every block consistent with its health, as map load does.
        let blocks = core::mem::take(&mut state.blocks);
        for block in &blocks {
            apply_to_mask(&mut state.terrain, block, None);
        }
        state.blocks = blocks;
        state
    }

    fn block(id: u32, x: i32, y: i32, w: u16, h: u16, material: Material) -> TerrainBlock {
        TerrainBlock {
            id,
            origin_cell_x: x,
            origin_cell_y: y,
            width_cells: w,
            height_cells: h,
            material,
            health: 100,
            max_health: 100,
            erosion_axis: ErosionAxis::default(),
        }
    }

    fn crater(cx: i32, cy: i32, radius: u16, mask: MaterialMask) -> TerrainOperation {
        let Some(center) = FixedPoint::from_cells(cx, cy) else {
            panic!("fixture invariant: from_cells must succeed");
        };
        TerrainOperation {
            sequence: 0,
            shape: TerrainShape::SubtractCircle {
                center,
                radius_cells: radius,
            },
            material_mask: mask,
        }
    }

    #[test]
    fn a_crater_on_a_block_reduces_its_health_instead_of_holing_it() {
        // THE bug this module exists to fix: before routing, a crater cleared cells while
        // the block still reported full health.
        let mut state = state_with(
            vec![block(1, 10, 10, 8, 2, Material::Soil)],
            Material::Empty,
        );
        let removed = apply_operation(&mut state, &crater(13, 10, 2, MaterialMask::SOFT));

        let Some(hit) = state.blocks.first() else {
            panic!("fixture invariant: one block");
        };
        assert!(
            hit.health < hit.max_health,
            "the crater must cost health, not just cells: {} of {}",
            hit.health,
            hit.max_health,
        );
        assert!(removed > 0, "cells were cleared");
    }

    #[test]
    fn cells_stay_consistent_with_health_after_a_crater() {
        // The invariant: solid cells inside a block always equal what its health says.
        let mut state = state_with(
            vec![block(1, 10, 10, 8, 2, Material::Soil)],
            Material::Empty,
        );
        apply_operation(&mut state, &crater(11, 10, 3, MaterialMask::SOFT));

        let Some(hit) = state.blocks.first() else {
            panic!("fixture invariant: one block");
        };
        let solid = count_solid(&state.terrain, block_bounds(hit));
        assert_eq!(
            solid,
            expected_solid_cells(hit),
            "a block must never be holed independently of its health",
        );
    }

    #[test]
    fn background_terrain_still_gets_a_literal_crater() {
        // Blocks shrink; the landscape around them does not.
        let mut state = state_with(Vec::new(), Material::Soil);
        let removed = apply_operation(&mut state, &crater(20, 12, 3, MaterialMask::SOFT));
        assert!(removed > 0);
        assert_eq!(
            crate::terrain::material_at(&state.terrain, 20, 12),
            Material::Empty,
            "the centre of a crater in open ground is cleared",
        );
    }

    #[test]
    fn a_reinforced_block_takes_no_damage_from_a_soft_crater() {
        let mut state = state_with(
            vec![block(1, 10, 10, 8, 2, Material::ReinforcedStone)],
            Material::Empty,
        );
        apply_operation(&mut state, &crater(13, 10, 3, MaterialMask::SOFT));

        let Some(hit) = state.blocks.first() else {
            panic!("fixture invariant: one block");
        };
        assert_eq!(hit.health, hit.max_health, "SOFT cannot damage stone");
    }

    #[test]
    fn a_block_outside_the_radius_is_untouched() {
        let mut state = state_with(
            vec![
                block(1, 2, 10, 4, 2, Material::Soil),
                block(2, 30, 10, 4, 2, Material::Soil),
            ],
            Material::Empty,
        );
        apply_operation(&mut state, &crater(3, 10, 2, MaterialMask::SOFT));

        let Some(far) = state.blocks.get(1) else {
            panic!("fixture invariant: two blocks");
        };
        assert_eq!(far.health, far.max_health);
        assert_eq!(count_solid(&state.terrain, block_bounds(far)), 8);
    }

    #[test]
    fn the_same_operation_is_identical_twice() {
        let run = || {
            let mut state = state_with(
                vec![
                    block(1, 10, 10, 8, 2, Material::Soil),
                    block(2, 20, 10, 8, 2, Material::Soil),
                ],
                Material::Empty,
            );
            let removed = apply_operation(&mut state, &crater(14, 10, 4, MaterialMask::SOFT));
            let healths: Vec<u16> = state.blocks.iter().map(|b| b.health).collect();
            (removed, healths, state.terrain.cells.clone())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn enough_damage_destroys_a_block_entirely() {
        let mut state = state_with(
            vec![block(1, 10, 10, 4, 2, Material::Soil)],
            Material::Empty,
        );
        // A radius large enough to cover the whole block clears every cell, so the
        // proportional damage is its full health.
        apply_operation(&mut state, &crater(11, 10, 12, MaterialMask::SOFT));

        let Some(hit) = state.blocks.first() else {
            panic!("fixture invariant: one block");
        };
        assert_eq!(hit.health, 0);
        assert_eq!(count_solid(&state.terrain, block_bounds(hit)), 0);
    }
}
