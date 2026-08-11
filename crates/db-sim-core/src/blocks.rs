//! Destructible terrain blocks with hit points (ADR 0005, `todolist.md` P3).
//!
//! A [`TerrainBlock`] is a rectangle of cells with health. The cell mask
//! ([`crate::types::TerrainMask`]) stays the single collision and rendering source of
//! truth — every function here either reads a block's own fields or rewrites the mask's
//! cells to match them. Nothing in this module reads or writes anything else, so sweeps,
//! ballistics, and `is_solid_at` need no changes to gain destructible blocks.
//!
//! # Purity is the whole safety property
//!
//! [`solid_columns`], [`surviving_columns`], and [`apply_to_mask`] are pure functions of
//! `(block.width_cells, block.health, block.max_health, impact_cell_x)`. None of them
//! read iteration order, use the RNG, or accumulate onto a previous mask — every call
//! recomputes from `health` alone. That is what makes replay exact: applying the same
//! damage sequence on two machines, or applying the same block twice in a row, always
//! produces the same mask (`apply_to_mask` is idempotent by construction, not by
//! coincidence).
//!
//! # Column erosion
//!
//! Health maps to surface area by eroding whole columns rather than a perimeter ring,
//! because the surface a player cares about is the walkable top edge — eroding a
//! perimeter would delete that edge first and leave an unreachable core. `solid_columns`
//! is `ceil(width_cells * health / max_health)`, so any health above zero keeps at least
//! one column and zero health keeps none. Which columns survive depends on
//! `impact_cell_x`: with an impact, the columns nearest it erode first (a hit on the
//! right end eats the right end); without one, both edges erode inward symmetrically so
//! the block shrinks toward its centre. Ties are broken by ascending cell x in both
//! cases, so the choice never depends on iteration order.

use crate::fixed::{BASIS_POINTS, scale};
use crate::terrain::{material_at, set_material};
use crate::types::{ErosionAxis, Material, TerrainMask};

/// A rectangle of terrain cells with hit points (ADR 0005).
///
/// The mask remains the collision and rendering truth; this struct is the entity that
/// drives it. `health` reaching zero does not remove the block from wherever it is
/// stored — it means every one of its cells has eroded to [`Material::Empty`], which
/// [`apply_to_mask`] enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainBlock {
    /// Stable identifier, assigned at map load. Never reused within a match.
    pub id: u32,
    /// Leftmost cell column the block occupies, inclusive.
    pub origin_cell_x: i32,
    /// Topmost cell row the block occupies, inclusive.
    pub origin_cell_y: i32,
    /// Width in cells.
    pub width_cells: u16,
    /// Height in cells.
    pub height_cells: u16,
    /// The material written to cells this block still occupies.
    pub material: Material,
    /// Current health. Zero means every column has eroded away.
    pub health: u16,
    /// Health at full integrity. [`solid_columns`] treats zero here as a degenerate
    /// block with no solid columns, since dividing by it is meaningless.
    pub max_health: u16,
    /// Which way this block erodes as it loses health.
    ///
    /// Set by whatever ammunition last damaged it, and stored rather than passed per-hit:
    /// erosion is recomputed from health every time, so the axis has to survive between
    /// hits or a later shell would undo an earlier drill's shape.
    pub erosion_axis: ErosionAxis,
}

/// Ceiling division for non-negative integers: `ceil(numerator / denominator)`.
///
/// Returns [`None`] if `denominator` is zero, or in the practically unreachable case
/// that the `(numerator + denominator - 1) / denominator` identity would overflow
/// `u64` — callers translate either case into a safe degenerate default rather than
/// dividing by zero or wrapping.
fn ceil_div_u64(numerator: u64, denominator: u64) -> Option<u64> {
    if denominator == 0 {
        return None;
    }
    // denominator != 0 is guaranteed by the check above, so subtracting 1 cannot
    // underflow; checked_sub is used anyway so every arithmetic step in this module is
    // checked or saturating, never bare, per project convention.
    let denominator_minus_one = denominator.checked_sub(1)?;
    let adjusted = numerator.checked_add(denominator_minus_one)?;
    // checked_div rather than `/`: denominator != 0 is already guaranteed above, but
    // this keeps every arithmetic step in the function checked without needing a
    // clippy::integer_division suppression. Pre-incrementing the numerator by
    // (denominator - 1) turns truncating division into the ceiling, which is the
    // documented rounding rule for surface-area-from-health (ADR 0005 §2).
    adjusted.checked_div(denominator)
}

/// How many of `block`'s columns are solid at its current health.
///
/// `ceil(width_cells * health / max_health)` — the literal reading of "percent of
/// health equals percent of usable surface" (ADR 0005 §2). Ceiling rounding guarantees
/// any health above zero keeps at least one column, and zero health keeps none.
///
/// Degrades to zero, never to a panic, if `max_health` is zero (nothing to divide by)
/// or `width_cells` is zero (nothing to hold a column).
#[must_use]
pub fn solid_columns(block: &TerrainBlock) -> u16 {
    if block.width_cells == 0 || block.health == 0 || block.max_health == 0 {
        return 0;
    }

    let width = u64::from(block.width_cells);
    let health = u64::from(block.health);
    let max_health = u64::from(block.max_health);

    // u16 * u16 is at most ~4.29e9, far inside u64::MAX; checked_mul stays in place so
    // the guarantee does not silently depend on these fields never being widened later.
    let Some(product) = width.checked_mul(health) else {
        // Unreachable for any valid u16 width/health. Degrades to "no solid columns" —
        // the same fail-safe direction Material::from_byte takes for a corrupted byte —
        // rather than fully solid, so a corrupted computation can never grant standing
        // room that shouldn't exist.
        return 0;
    };

    let Some(columns) = ceil_div_u64(product, max_health) else {
        return 0;
    };

    // health must never exceed max_health by construction, but a malformed block must
    // still degrade to "fully solid" rather than a column count apply_to_mask cannot
    // reconcile with width_cells.
    let clamped = columns.min(width);

    #[expect(
        clippy::cast_possible_truncation,
        reason = "clamped is bounded above by `width`, itself a u16 widened to u64, so it \
                  always fits back into u16"
    )]
    let result = clamped as u16;
    result
}

/// The absolute cell-x coordinate of `block`'s `local_column`-th column (0-indexed from
/// its left edge).
fn absolute_column_x(block: &TerrainBlock, local_column: u16) -> i32 {
    // width_cells is at most u16::MAX, so widening to i32 before adding cannot lose
    // information. saturating_add absorbs the case where origin_cell_x sits near
    // i32::MAX on a malformed block, clamping rather than wrapping into a bogus
    // negative column.
    block.origin_cell_x.saturating_add(i32::from(local_column))
}

/// Ranks `local_column` by how soon it erodes as health drops — lower sorts first.
///
/// With an impact position, distance from the impact ranks the column: nearest first,
/// so damage eats inward from where it landed. Without one, distance from the nearer
/// edge ranks it instead, so both edges erode inward symmetrically and the block
/// shrinks toward its centre (ADR 0005 §2).
fn erosion_rank(block: &TerrainBlock, local_column: u16, impact_cell_x: Option<i32>) -> u64 {
    match impact_cell_x {
        Some(impact_x) => {
            let abs_x = absolute_column_x(block, local_column);
            // Widen to i64: abs_x and impact_x are both i32, and a maximally separated
            // pair differs by close to 2^32, which overflows i32 subtraction.
            // unsigned_abs never panics (unlike i64::abs at i64::MIN), and the actual
            // magnitude here is nowhere near i64::MIN.
            let diff = i64::from(abs_x).saturating_sub(i64::from(impact_x));
            diff.unsigned_abs()
        }
        None => {
            let local = u64::from(local_column);
            // Callers only reach here once width_cells > 0 has already been confirmed
            // (solid_columns returns 0, short-circuiting eroded_columns, whenever
            // width_cells == 0), so this subtraction never underflows in practice;
            // saturating_sub is kept anyway per the checked/saturating convention.
            let last = u64::from(block.width_cells.saturating_sub(1));
            let from_right_edge = last.saturating_sub(local);
            local.min(from_right_edge)
        }
    }
}

/// For every local column of `block`, whether health has eroded it away.
///
/// The single computation shared by [`surviving_columns`] and [`apply_to_mask`], so the
/// two can never disagree about which columns are solid. Recomputed from `health` on
/// every call — never accumulated — which is what makes this a pure function of state.
fn eroded_columns(block: &TerrainBlock, impact_cell_x: Option<i32>) -> Vec<bool> {
    let width = block.width_cells;
    let mut eroded = vec![false; usize::from(width)];
    if width == 0 {
        return eroded;
    }

    let solid = solid_columns(block);
    if solid >= width {
        // Fully solid: nothing erodes.
        return eroded;
    }
    if solid == 0 {
        return vec![true; usize::from(width)];
    }

    // Number of columns this call erodes. Derived from `health` alone every time, per
    // this module's purity requirement — never subtracted from a previous erosion.
    let eroded_count = width.saturating_sub(solid);

    // Rank every local column by how soon it erodes; tuple ordering compares `rank`
    // first, then `abs_x` as the tie-break ADR 0005 §2 specifies ("ties broken by
    // ascending x"), then `local` (redundant with `abs_x` under a fixed origin, but
    // keeps the order total even for a malformed block whose saturating math collapses
    // two columns onto the same absolute x).
    let mut ranked: Vec<(u64, i32, u16)> = (0..width)
        .map(|local| {
            let abs_x = absolute_column_x(block, local);
            (erosion_rank(block, local, impact_cell_x), abs_x, local)
        })
        .collect();
    ranked.sort_unstable();

    for &(_, _, local) in ranked.iter().take(usize::from(eroded_count)) {
        if let Some(flag) = eroded.get_mut(usize::from(local)) {
            *flag = true;
        }
    }

    eroded
}

/// The absolute cell-x coordinates of `block`'s columns that remain solid at its
/// current health, ascending.
///
/// Erosion removes columns nearest `impact_cell_x` first; with [`None`], it erodes
/// edge-inward from both sides so the block shrinks toward its centre (ADR 0005 §2).
/// Pure: calling this twice with the same `block` and `impact_cell_x` always returns
/// the same list, because it is derived from `health` alone.
#[must_use]
pub fn surviving_columns(block: &TerrainBlock, impact_cell_x: Option<i32>) -> Vec<i32> {
    let eroded = eroded_columns(block, impact_cell_x);
    (0..block.width_cells)
        .filter(|&local| !eroded.get(usize::from(local)).copied().unwrap_or(true))
        .map(|local| absolute_column_x(block, local))
        .collect()
}

/// Rewrites `block`'s cells in `mask` to match its current health, and returns how many
/// cells this call cleared (were solid before, are [`Material::Empty`] after).
///
/// Surviving columns are set to `block.material`; eroded columns are set to
/// [`Material::Empty`]. A cell outside `mask`'s bounds is simply not written — the block
/// clips rather than panicking or wrapping. Idempotent: calling this twice in a row with
/// an unchanged `block` produces the same mask both times, and the second call reports
/// zero newly cleared cells, because eroded cells are already `Empty` by then.
pub fn apply_to_mask(
    mask: &mut TerrainMask,
    block: &TerrainBlock,
    impact_cell_x: Option<i32>,
) -> u32 {
    let eroded = eroded_columns(block, impact_cell_x);
    let mut cleared = 0u32;

    for local_x in 0..block.width_cells {
        let is_eroded = eroded.get(usize::from(local_x)).copied().unwrap_or(true);
        let material = if is_eroded {
            Material::Empty
        } else {
            block.material
        };
        let abs_x = absolute_column_x(block, local_x);

        for local_y in 0..block.height_cells {
            // height_cells is at most u16::MAX, so widening before adding cannot lose
            // information; saturating_add clamps a malformed origin instead of wrapping.
            let abs_y = block.origin_cell_y.saturating_add(i32::from(local_y));

            let previously_solid = material_at(mask, abs_x, abs_y).is_solid();
            let wrote = set_material(mask, abs_x, abs_y, material);

            // Only a cell that actually exists in `mask` (wrote == true, i.e. the block
            // clipped rather than reaching out of bounds here), was solid before, and is
            // eroded now counts as cleared — which is exactly what makes a second call
            // with the same health report zero.
            if wrote && is_eroded && previously_solid {
                cleared = cleared.saturating_add(1);
            }
        }
    }

    cleared
}

/// Reduces `block.health` by `amount`, saturating at zero, and returns how much damage
/// was actually absorbed — never more than the block had left.
pub fn damage_block(block: &mut TerrainBlock, amount: u16) -> u16 {
    let absorbed = amount.min(block.health);
    block.health = block.health.saturating_sub(amount);
    absorbed
}

/// `block`'s health as basis points of `max_health` (`10_000` == full health).
///
/// Degrades to zero, never to a panic, when `max_health` is zero.
#[must_use]
pub fn health_basis_points(block: &TerrainBlock) -> i32 {
    if block.max_health == 0 {
        return 0;
    }

    let health = i32::from(block.health);
    let max_health = i32::from(block.max_health);

    // scale widens to i64 internally, so this cannot overflow even though `health` and
    // `max_health` are both u16-derived; it also owns the rounding rule (half away from
    // zero), keeping this consistent with every other basis-point conversion in the crate.
    // unwrap_or_default (never unwrap/expect) is exact here: scale's only failure mode
    // given u16-derived inputs is unreachable, and 0 is the same degrade-to-safe value
    // this function already returns for max_health == 0.
    scale(health, BASIS_POINTS, max_health).unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    fn make_block(
        origin_x: i32,
        origin_y: i32,
        width: u16,
        height: u16,
        material: Material,
        health: u16,
        max_health: u16,
    ) -> TerrainBlock {
        TerrainBlock {
            id: 1,
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

    // ---------------------------------------------------------------------------
    // solid_columns: exact counts at named health percentages
    // ---------------------------------------------------------------------------

    #[test]
    fn solid_columns_100_percent_keeps_full_width() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 100, 100);
        assert_eq!(solid_columns(&block), 8);
    }

    #[test]
    fn solid_columns_75_percent_rounds_up() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 75, 100);
        // ceil(8 * 0.75) = ceil(6.0) = 6.
        assert_eq!(solid_columns(&block), 6);
    }

    #[test]
    fn solid_columns_50_percent() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 50, 100);
        assert_eq!(solid_columns(&block), 4);
    }

    #[test]
    fn solid_columns_25_percent_rounds_up() {
        let block = make_block(0, 0, 9, 2, Material::Soil, 25, 100);
        // ceil(9 * 0.25) = ceil(2.25) = 3, not 2 — proves rounding is ceiling, not
        // truncation.
        assert_eq!(solid_columns(&block), 3);
    }

    #[test]
    fn solid_columns_one_hp_keeps_minimum_one_column() {
        let block = make_block(0, 0, 100, 2, Material::Soil, 1, 10_000);
        assert_eq!(solid_columns(&block), 1);
    }

    #[test]
    fn solid_columns_zero_hp_keeps_no_columns() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 0, 100);
        assert_eq!(solid_columns(&block), 0);
    }

    #[test]
    fn solid_columns_zero_width_block_is_zero_without_panic() {
        let block = make_block(0, 0, 0, 2, Material::Soil, 100, 100);
        assert_eq!(solid_columns(&block), 0);
    }

    #[test]
    fn solid_columns_zero_max_health_is_zero_without_panic() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 50, 0);
        assert_eq!(solid_columns(&block), 0);
    }

    // ---------------------------------------------------------------------------
    // surviving_columns: impact direction and edge-inward erosion
    // ---------------------------------------------------------------------------

    #[test]
    fn surviving_columns_impact_on_right_end_erodes_right_end() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 50, 100); // 4 solid columns
        let impact_at_right_edge = 7; // local column 7, the block's rightmost column
        let surviving = surviving_columns(&block, Some(impact_at_right_edge));
        assert_eq!(surviving, vec![0, 1, 2, 3]);
    }

    #[test]
    fn surviving_columns_impact_on_left_end_erodes_left_end() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 50, 100); // 4 solid columns
        let impact_at_left_edge = 0; // local column 0, the block's leftmost column
        let surviving = surviving_columns(&block, Some(impact_at_left_edge));
        assert_eq!(surviving, vec![4, 5, 6, 7]);
    }

    #[test]
    fn surviving_columns_impact_off_the_right_edge_still_erodes_right() {
        let block = make_block(10, 0, 8, 2, Material::Soil, 50, 100); // abs x 10..17
        // Impact well to the right of the block entirely — still nearer the block's
        // right edge than its left, so the same side erodes.
        let surviving = surviving_columns(&block, Some(100));
        assert_eq!(surviving, vec![10, 11, 12, 13]);
    }

    #[test]
    fn surviving_columns_no_impact_erodes_edge_inward_toward_centre() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 50, 100); // 4 solid columns
        let surviving = surviving_columns(&block, None);
        // Both edges erode inward symmetrically, leaving the central 4 columns.
        assert_eq!(surviving, vec![2, 3, 4, 5]);
    }

    #[test]
    fn surviving_columns_no_impact_odd_width_centres_on_the_middle_column() {
        let block = make_block(0, 0, 7, 2, Material::Soil, 30, 100); // ceil(7*0.30)=3
        assert_eq!(solid_columns(&block), 3);
        let surviving = surviving_columns(&block, None);
        assert_eq!(surviving, vec![2, 3, 4]);
    }

    #[test]
    fn surviving_columns_full_health_returns_every_column_ascending() {
        let block = make_block(5, 0, 4, 2, Material::Soil, 100, 100);
        let surviving = surviving_columns(&block, Some(6));
        assert_eq!(surviving, vec![5, 6, 7, 8]);
    }

    #[test]
    fn surviving_columns_zero_health_returns_empty() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 0, 100);
        assert_eq!(surviving_columns(&block, Some(3)), Vec::<i32>::new());
    }

    #[test]
    fn surviving_columns_zero_width_block_returns_empty_without_panic() {
        let block = make_block(0, 0, 0, 2, Material::Soil, 100, 100);
        assert_eq!(surviving_columns(&block, None), Vec::<i32>::new());
    }

    // ---------------------------------------------------------------------------
    // apply_to_mask: writes, clearing count, idempotency, clipping, determinism
    // ---------------------------------------------------------------------------

    fn make_test_mask(width: u32, height: u32) -> TerrainMask {
        if let Ok(mask) = crate::terrain::create_mask(width, height, Material::Empty) {
            mask
        } else {
            panic!("create_mask failed for a valid, non-degenerate size");
        }
    }

    #[test]
    fn apply_to_mask_writes_material_and_returns_cleared_count() {
        let mut mask = make_test_mask(12, 6);
        let full = make_block(2, 2, 8, 3, Material::Soil, 100, 100);

        // Establish the block at full health first: nothing was solid before, so
        // nothing is "cleared" by growing into empty space.
        let first = apply_to_mask(&mut mask, &full, None);
        assert_eq!(first, 0);
        for x in 2..10 {
            for y in 2..5 {
                assert_eq!(material_at(&mask, x, y), Material::Soil, "x={x} y={y}");
            }
        }

        // Now damage it to 50% with an impact at its left edge: the left 4 columns
        // erode, and every one of their cells was solid a moment ago.
        let damaged = make_block(2, 2, 8, 3, Material::Soil, 50, 100);
        let cleared = apply_to_mask(&mut mask, &damaged, Some(2));
        assert_eq!(cleared, 4 * 3);

        for x in 2..6 {
            for y in 2..5 {
                assert_eq!(material_at(&mask, x, y), Material::Empty, "x={x} y={y}");
            }
        }
        for x in 6..10 {
            for y in 2..5 {
                assert_eq!(material_at(&mask, x, y), Material::Soil, "x={x} y={y}");
            }
        }
    }

    #[test]
    fn apply_to_mask_is_idempotent() {
        let mut mask = make_test_mask(12, 6);
        let full = make_block(2, 2, 8, 3, Material::Soil, 100, 100);
        let _ = apply_to_mask(&mut mask, &full, None);

        let damaged = make_block(2, 2, 8, 3, Material::Soil, 50, 100);
        let first_cleared = apply_to_mask(&mut mask, &damaged, Some(2));
        let mask_after_first = mask.clone();

        let second_cleared = apply_to_mask(&mut mask, &damaged, Some(2));

        assert!(first_cleared > 0);
        assert_eq!(second_cleared, 0);
        assert_eq!(mask, mask_after_first);
    }

    #[test]
    fn apply_to_mask_clips_out_of_bounds_block_without_panic() {
        let mut mask = make_test_mask(5, 5);
        // Spans x in [-3, 4], well outside the mask on the left, and y in [-3, 4],
        // outside on top. Only the intersection with the 5x5 mask should be touched.
        let block = make_block(-3, -3, 8, 8, Material::Wood, 100, 100);

        let cleared = apply_to_mask(&mut mask, &block, None);

        // Growing into previously-Empty cells never counts as "cleared".
        assert_eq!(cleared, 0);
        for x in 0..5 {
            for y in 0..5 {
                assert_eq!(material_at(&mask, x, y), Material::Wood, "x={x} y={y}");
            }
        }
    }

    #[test]
    fn apply_to_mask_fully_out_of_bounds_block_does_nothing_and_clips_to_zero() {
        let mut mask = make_test_mask(5, 5);
        let block = make_block(100, 100, 4, 4, Material::Wood, 100, 100);

        let cleared = apply_to_mask(&mut mask, &block, None);

        assert_eq!(cleared, 0);
        for x in 0..5 {
            for y in 0..5 {
                assert_eq!(material_at(&mask, x, y), Material::Empty, "x={x} y={y}");
            }
        }
    }

    #[test]
    fn apply_to_mask_zero_width_block_does_nothing() {
        let mut mask = make_test_mask(5, 5);
        let untouched = mask.clone();
        let block = make_block(1, 1, 0, 3, Material::Wood, 100, 100);

        let cleared = apply_to_mask(&mut mask, &block, None);

        assert_eq!(cleared, 0);
        assert_eq!(mask, untouched);
    }

    #[test]
    fn apply_to_mask_same_inputs_produce_byte_identical_masks_twice() {
        let full = make_block(1, 1, 6, 4, Material::Soil, 100, 100);
        let damaged = make_block(1, 1, 6, 4, Material::Soil, 33, 100);

        let mut mask_a = make_test_mask(10, 10);
        let _ = apply_to_mask(&mut mask_a, &full, None);
        let cleared_a = apply_to_mask(&mut mask_a, &damaged, Some(6));

        let mut mask_b = make_test_mask(10, 10);
        let _ = apply_to_mask(&mut mask_b, &full, None);
        let cleared_b = apply_to_mask(&mut mask_b, &damaged, Some(6));

        assert_eq!(cleared_a, cleared_b);
        assert_eq!(mask_a, mask_b);
        assert_eq!(mask_a.cells, mask_b.cells);
    }

    // ---------------------------------------------------------------------------
    // damage_block
    // ---------------------------------------------------------------------------

    #[test]
    fn damage_block_reduces_health_by_the_full_amount_when_it_has_enough() {
        let mut block = make_block(0, 0, 8, 2, Material::Soil, 100, 100);
        let absorbed = damage_block(&mut block, 30);
        assert_eq!(absorbed, 30);
        assert_eq!(block.health, 70);
    }

    #[test]
    fn damage_block_saturates_at_zero() {
        let mut block = make_block(0, 0, 8, 2, Material::Soil, 20, 100);
        let absorbed = damage_block(&mut block, 500);
        assert_eq!(block.health, 0);
        // Absorbed is capped at what the block actually had — never more.
        assert_eq!(absorbed, 20);
    }

    #[test]
    fn damage_block_on_already_dead_block_absorbs_nothing() {
        let mut block = make_block(0, 0, 8, 2, Material::Soil, 0, 100);
        let absorbed = damage_block(&mut block, 50);
        assert_eq!(absorbed, 0);
        assert_eq!(block.health, 0);
    }

    // ---------------------------------------------------------------------------
    // health_basis_points
    // ---------------------------------------------------------------------------

    #[test]
    fn health_basis_points_full_health_is_ten_thousand() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 100, 100);
        assert_eq!(health_basis_points(&block), 10_000);
    }

    #[test]
    fn health_basis_points_zero_health_is_zero() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 0, 100);
        assert_eq!(health_basis_points(&block), 0);
    }

    #[test]
    fn health_basis_points_half_health_is_five_thousand() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 50, 100);
        assert_eq!(health_basis_points(&block), 5_000);
    }

    #[test]
    fn health_basis_points_zero_max_health_is_zero_without_panic() {
        let block = make_block(0, 0, 8, 2, Material::Soil, 0, 0);
        assert_eq!(health_basis_points(&block), 0);
    }
}
