//! Map definitions: static block layout plus spawn points (`todolist.md` P5 groundwork,
//! and the test fixture `todolist.md` P3's damage side is proven against).
//!
//! A [`MapDefinition`] is authored content — the blocks and spawn points a match starts
//! with — not runtime state. [`build_mask`] is the one piece of behavior this file owns:
//! turning that authored data into the [`TerrainMask`] a fresh match's terrain begins as,
//! by replaying every block through [`crate::blocks::apply_to_mask`] exactly once. Nothing
//! here duplicates that erosion math; this file only ever calls into it, so "what does a
//! block's current health look like as cells" has exactly one authority
//! (`docs/adr/0005-destructible-blocks-with-health.md` §"Two representations of solidity
//! must not drift").
//!
//! [`horizontal_test_array`] is the owner-requested fixture for demonstrating destructible
//! blocks end to end: one legible row of same-sized, full-health blocks with a spawn point
//! over each, so `resolve::terrain_damage`'s tests have a shared, documented layout to
//! damage instead of an ad hoc setup per test.

use crate::blocks::TerrainBlock;
use crate::error::SimResult;
use crate::fixed::FixedPoint;
use crate::terrain;
use crate::types::{Material, TerrainMask};

/// A map's static layout: dimensions, destructible blocks, and spawn points.
///
/// Authored content, not match state. `SimulationState`'s live [`TerrainMask`] starts from
/// [`build_mask`] applied to one of these. `SimulationState` has no field yet to hold the
/// live `Vec<TerrainBlock>` a match would go on to damage — see
/// `resolve::terrain_damage`'s module doc comment ("A signature deviation, reported rather
/// than invented silently") for the gap and why this file does not attempt to close it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapDefinition {
    /// Stable identifier, e.g. `"horizontal-test-array"`.
    pub id: &'static str,
    /// Terrain mask width, in cells.
    pub width_cells: u32,
    /// Terrain mask height, in cells.
    pub height_cells: u32,
    /// Destructible blocks present at match start, at their starting health.
    pub blocks: Vec<TerrainBlock>,
    /// Player spawn positions, fixed-point.
    pub spawn_points: Vec<FixedPoint>,
}

/// Builds the initial [`TerrainMask`] for `map`: an empty mask of `map`'s dimensions with
/// every block in `map.blocks` written in at its starting health.
///
/// # Errors
///
/// Returns [`crate::error::SimError::OutOfRange`] if `map.width_cells` or
/// `map.height_cells` is zero, or their product would overflow — the same conditions
/// [`crate::terrain::create_mask`] itself rejects; the error is propagated unchanged.
pub fn build_mask(map: &MapDefinition) -> SimResult<TerrainMask> {
    let mut mask = terrain::create_mask(map.width_cells, map.height_cells, Material::Empty)?;
    for block in &map.blocks {
        // Cleared-cell count is meaningless here and deliberately discarded: nothing was
        // solid before a fresh mask was built, so growing a block into empty space is
        // never "cleared" (`blocks::apply_to_mask`'s own documented contract). No impact
        // position either — a map's initial layout has not been hit by anything yet, so
        // there is no "side that was hit" for erosion to be directed toward.
        let _ = crate::blocks::apply_to_mask(&mut mask, block, None);
    }
    Ok(mask)
}

// ---------------------------------------------------------------------------------------
// horizontal_test_array
// ---------------------------------------------------------------------------------------

/// Column width of every block in [`horizontal_test_array`], in cells.
const ROW_BLOCK_WIDTH_CELLS: u16 = 4;
/// Row height of every block in [`horizontal_test_array`], in cells.
const ROW_BLOCK_HEIGHT_CELLS: u16 = 3;
/// Horizontal gap between adjacent blocks in [`horizontal_test_array`], in cells.
/// Deliberately nonzero: it lets a radius-limited hit centred on one block's footprint
/// provably exclude its neighbours, which a contiguous row (zero gap, touching edges)
/// could not guarantee once a hit radius reached the shared edge.
const ROW_BLOCK_GAP_CELLS: i32 = 2;
/// How many blocks [`horizontal_test_array`] lays out. At least 8 per the owner's brief,
/// so a single hit on one interior block leaves a legible spread of untouched siblings on
/// both sides.
const ROW_BLOCK_COUNT: u32 = 8;
/// Cell row (`origin_cell_y`) every block's top edge sits on.
const ROW_TOP_Y: i32 = 8;
/// Starting and maximum health of every block in the fixture. A round number chosen so
/// "damage it for half its health" lands on an exact integer (`50`), not an approximation
/// from rounding.
const ROW_BLOCK_MAX_HEALTH: u16 = 100;
/// Cells of headroom between a spawn point and the block top it sits above.
const ROW_SPAWN_HEADROOM_CELLS: i32 = 2;
/// Horizontal offset from a block's left edge to its centre, in cells — half of
/// [`ROW_BLOCK_WIDTH_CELLS`]. Written as its own literal rather than a runtime `/ 2` so
/// there is no integer division to lint; the const assertion below keeps it honest against
/// the width constant it describes, and both sides share a type so no cast is needed to
/// compare them.
const ROW_SPAWN_X_OFFSET_CELLS: u16 = 2;
const _: () = assert!(ROW_SPAWN_X_OFFSET_CELLS * 2 == ROW_BLOCK_WIDTH_CELLS);
/// Map width: the last block's right edge — `(ROW_BLOCK_COUNT - 1) * stride +
/// ROW_BLOCK_WIDTH_CELLS`, where `stride` is `ROW_BLOCK_WIDTH_CELLS + ROW_BLOCK_GAP_CELLS`
/// (`6`), i.e. blocks span cells `0..46` — plus a margin so nothing touches the mask edge.
const ROW_MAP_WIDTH_CELLS: u32 = 50;
/// Map height: enough rows above the blocks for spawn headroom and sky, and below for
/// floor clearance. Not load-bearing for any test; chosen only to be comfortably larger
/// than the block row itself.
const ROW_MAP_HEIGHT_CELLS: u32 = 20;

/// The owner-requested destructible-block test fixture (`todolist.md` P3): a single
/// horizontal row of same-sized, full-health blocks, each with its own spawn point above
/// it. Legible by construction — every block is the same size, at the same height, evenly
/// spaced — so a test can name "block 3" and reason about exactly which cells that means
/// without cross-referencing a table.
///
/// ```text
///          S1    S2    S3    S4    S5    S6    S7    S8       y=6  (spawn points)
///
///         ┌──┐  ┌──┐  ┌──┐  ┌──┐  ┌──┐  ┌──┐  ┌──┐  ┌──┐
///         │B1│  │B2│  │B3│  │B4│  │B5│  │B6│  │B7│  │B8│       y=8..11 (blocks, 4 wide x 3 tall)
///         └──┘  └──┘  └──┘  └──┘  └──┘  └──┘  └──┘  └──┘
///    x=    0     6     12    18    24    30    36    42        (each block's origin_cell_x)
/// ```
///
/// Block `id`s run `1..=8` left to right, matching the diagram's labels — "block 3" means
/// `id == 3`, the block at `origin_cell_x == 12`. Every block is [`Material::Soil`]
/// (removable under [`crate::types::MaterialMask::SOFT`], the mask every launch-roster
/// ability uses) at [`ROW_BLOCK_MAX_HEALTH`] of [`ROW_BLOCK_MAX_HEALTH`] health, i.e. full
/// health.
#[must_use]
pub fn horizontal_test_array() -> MapDefinition {
    let mut blocks = Vec::with_capacity(ROW_BLOCK_COUNT as usize);
    let mut spawn_points = Vec::with_capacity(ROW_BLOCK_COUNT as usize);

    let stride_cells = i32::from(ROW_BLOCK_WIDTH_CELLS).saturating_add(ROW_BLOCK_GAP_CELLS);

    for index in 0..ROW_BLOCK_COUNT {
        // index is bounded by ROW_BLOCK_COUNT (8), far inside i32 range, but this loop is
        // the one place this fixture's data is computed rather than written as a literal,
        // so it earns the same checked/saturating treatment as runtime code rather than an
        // assumed-safe cast.
        let index_i32 = i32::try_from(index).unwrap_or(i32::MAX);
        let origin_x = index_i32.saturating_mul(stride_cells);

        blocks.push(TerrainBlock {
            // 1-indexed so "block 3" in a doc comment or a test name means `id == 3`
            // directly, matching the diagram's `B1..B8` labels.
            id: index.saturating_add(1),
            origin_cell_x: origin_x,
            origin_cell_y: ROW_TOP_Y,
            width_cells: ROW_BLOCK_WIDTH_CELLS,
            height_cells: ROW_BLOCK_HEIGHT_CELLS,
            material: Material::Soil,
            health: ROW_BLOCK_MAX_HEALTH,
            max_health: ROW_BLOCK_MAX_HEALTH,
        });

        let spawn_x = origin_x.saturating_add(i32::from(ROW_SPAWN_X_OFFSET_CELLS));
        let spawn_y = ROW_TOP_Y.saturating_sub(ROW_SPAWN_HEADROOM_CELLS);
        spawn_points.push(spawn_point(spawn_x, spawn_y));
    }

    MapDefinition {
        id: "horizontal-test-array",
        width_cells: ROW_MAP_WIDTH_CELLS,
        height_cells: ROW_MAP_HEIGHT_CELLS,
        blocks,
        spawn_points,
    }
}

/// Constructs a spawn point from whole cells, degrading to the origin — never panicking —
/// on the practically unreachable overflow case [`FixedPoint::from_cells`] guards against.
/// Every call site in this file uses fixture literals nowhere near that boundary; the
/// fallback exists so this function is unconditionally panic-free rather than "panic-free
/// because the inputs happen to be small right now."
fn spawn_point(x_cells: i32, y_cells: i32) -> FixedPoint {
    FixedPoint::from_cells(x_cells, y_cells).unwrap_or(FixedPoint::ZERO)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::blocks::solid_columns;
    use crate::terrain::material_at;

    #[test]
    fn horizontal_test_array_has_at_least_eight_full_health_blocks_of_one_size() {
        let map = horizontal_test_array();
        assert!(map.blocks.len() >= 8, "owner's brief: at least 8 blocks");
        for block in &map.blocks {
            assert_eq!(block.width_cells, ROW_BLOCK_WIDTH_CELLS);
            assert_eq!(block.height_cells, ROW_BLOCK_HEIGHT_CELLS);
            assert_eq!(block.origin_cell_y, ROW_TOP_Y);
            assert_eq!(block.material, Material::Soil);
            assert_eq!(block.health, block.max_health);
            assert_eq!(block.health, ROW_BLOCK_MAX_HEALTH);
        }
    }

    #[test]
    fn horizontal_test_array_blocks_are_ordered_left_to_right_by_id_with_no_overlap() {
        let map = horizontal_test_array();
        let mut previous_right_edge: Option<i32> = None;
        for (expected_id, block) in (1u32..).zip(map.blocks.iter()) {
            assert_eq!(block.id, expected_id);
            if let Some(previous) = previous_right_edge {
                assert!(
                    block.origin_cell_x >= previous,
                    "block {expected_id} must not overlap its predecessor"
                );
            }
            previous_right_edge = Some(
                block
                    .origin_cell_x
                    .saturating_add(i32::from(block.width_cells)),
            );
        }
    }

    #[test]
    fn horizontal_test_array_spawn_points_sit_above_each_block_with_headroom() {
        let map = horizontal_test_array();
        assert_eq!(map.spawn_points.len(), map.blocks.len());
        for (spawn, block) in map.spawn_points.iter().zip(map.blocks.iter()) {
            let (spawn_x, spawn_y) = spawn.to_cells();
            assert!(
                spawn_y < block.origin_cell_y,
                "spawn (y={spawn_y}) must sit above its block (top y={0})",
                block.origin_cell_y
            );
            assert!(spawn_x >= block.origin_cell_x);
            assert!(
                spawn_x
                    < block
                        .origin_cell_x
                        .saturating_add(i32::from(block.width_cells))
            );
        }
    }

    #[test]
    fn build_mask_writes_every_block_at_full_health() {
        let map = horizontal_test_array();
        let Ok(mask) = build_mask(&map) else {
            panic!("fixture invariant: horizontal_test_array must build a valid mask");
        };
        for block in &map.blocks {
            assert_eq!(
                solid_columns(block),
                block.width_cells,
                "block {} must be fully solid at full health",
                block.id
            );
            for local_x in 0..block.width_cells {
                let x = block.origin_cell_x.saturating_add(i32::from(local_x));
                for local_y in 0..block.height_cells {
                    let y = block.origin_cell_y.saturating_add(i32::from(local_y));
                    assert_eq!(material_at(&mask, x, y), Material::Soil, "x={x} y={y}");
                }
            }
        }
        // A cell above the block row is untouched sky.
        assert_eq!(material_at(&mask, 0, 0), Material::Empty);
    }

    #[test]
    fn build_mask_rejects_zero_width() {
        let map = MapDefinition {
            id: "degenerate",
            width_cells: 0,
            height_cells: 10,
            blocks: Vec::new(),
            spawn_points: Vec::new(),
        };
        assert!(build_mask(&map).is_err());
    }

    #[test]
    fn build_mask_clips_a_block_hanging_off_the_map_edge_without_panicking() {
        let block = TerrainBlock {
            id: 1,
            origin_cell_x: 8,
            origin_cell_y: 0,
            width_cells: 6,
            height_cells: 2,
            material: Material::Wood,
            health: 100,
            max_health: 100,
        };
        let map = MapDefinition {
            id: "clipped",
            width_cells: 10,
            height_cells: 5,
            blocks: vec![block],
            spawn_points: Vec::new(),
        };
        let Ok(mask) = build_mask(&map) else {
            panic!("fixture invariant: a valid, non-degenerate map must build");
        };
        assert_eq!(material_at(&mask, 9, 0), Material::Wood);
        // x=10..14 are outside the 10-wide mask; reading them must still return Empty,
        // never panic, confirming the block clipped instead of wrapping or erroring.
        assert_eq!(material_at(&mask, 10, 0), Material::Empty);
        assert_eq!(material_at(&mask, 13, 1), Material::Empty);
    }
}
