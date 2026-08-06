//! Terrain occupancy mask and operations.
//!
//! The authoritative collision and destruction system for terrain. One byte per cell
//! holds a [`Material`], and operations remove cells according to shape and material mask.
//!
//! Parity with `lib/game/simulation.ts`: this module reimplements `createTerrainMask`,
//! `cloneTerrainMask`, `terrainCell`, `setTerrainCell`, `applyTerrainOperation`,
//! `subtractCircle`, `subtractCapsule`, and `pointInCapsule` bit-exactly, except that
//! the TS version stores 0/1 while this stores [`Material`] bytes.

use crate::error::{SimError, SimResult};
use crate::fixed::FixedPoint;
use crate::types::{Material, MaterialMask, TerrainMask, TerrainOperation, TerrainShape};

/// Creates a new terrain mask with the specified dimensions, filled with the given material.
///
/// # Errors
///
/// Returns `OutOfRange` if width or height is zero or would overflow when computing the
/// total cell count.
///
/// # Panics
///
/// Never. Returns a `SimResult` instead.
pub fn create_mask(width: u32, height: u32, fill: Material) -> SimResult<TerrainMask> {
    if width == 0 || height == 0 {
        return Err(SimError::OutOfRange {
            field: "terrain dimensions",
        });
    }

    // Check for overflow: width * height must fit in usize
    let Some(total_cells) = width.checked_mul(height) else {
        return Err(SimError::OutOfRange {
            field: "terrain cell count",
        });
    };

    let mut cells = vec![fill as u8; total_cells as usize];
    cells.shrink_to_fit();

    Ok(TerrainMask {
        width,
        height,
        cells,
    })
}

/// Returns the material at the given cell coordinates.
///
/// Out-of-bounds reads return [`Material::Empty`] without panicking or wrapping.
#[must_use]
#[inline]
pub fn material_at(mask: &TerrainMask, x_cell: i32, y_cell: i32) -> Material {
    // Bounds check: negative indices or >= dimensions return Empty
    if x_cell < 0 || y_cell < 0 {
        return Material::Empty;
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "non-negative values are guaranteed by preceding bounds checks"
    )]
    let x = x_cell as u32;
    #[expect(
        clippy::cast_sign_loss,
        reason = "non-negative values are guaranteed by preceding bounds checks"
    )]
    let y = y_cell as u32;

    if x >= mask.width || y >= mask.height {
        return Material::Empty;
    }

    // Safe: indices are in bounds after the checks above
    let index = (y as usize).checked_mul(mask.width as usize);
    let index = match index {
        Some(row_offset) => {
            // Reconstructing with checked_add for safety
            match row_offset.checked_add(x as usize) {
                Some(idx) => idx,
                None => return Material::Empty,
            }
        }
        None => return Material::Empty,
    };

    let byte_value = mask.cells.get(index).copied().unwrap_or(0);
    Material::from_byte(byte_value)
}

/// Sets the material at the given cell coordinates.
///
/// Returns `false` if the coordinates are out of bounds; `true` otherwise.
/// Out-of-bounds writes are silently ignored.
pub fn set_material(mask: &mut TerrainMask, x_cell: i32, y_cell: i32, material: Material) -> bool {
    // Bounds check
    if x_cell < 0 || y_cell < 0 {
        return false;
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "non-negative values are guaranteed by preceding bounds checks"
    )]
    let x = x_cell as u32;
    #[expect(
        clippy::cast_sign_loss,
        reason = "non-negative values are guaranteed by preceding bounds checks"
    )]
    let y = y_cell as u32;

    if x >= mask.width || y >= mask.height {
        return false;
    }

    // Safe: indices are in bounds after the checks above
    let index = (y as usize).checked_mul(mask.width as usize);
    let index = match index {
        Some(row_offset) => match row_offset.checked_add(x as usize) {
            Some(idx) if idx < mask.cells.len() => idx,
            _ => return false,
        },
        None => return false,
    };

    if let Some(cell) = mask.cells.get_mut(index) {
        *cell = material as u8;
        true
    } else {
        false
    }
}

/// Checks if a fixed-point position is solid terrain.
///
/// Converts the position to cell coordinates via [`FixedPoint::to_cells`] and returns
/// whether the material at that cell is solid.
#[must_use]
pub fn is_solid_at(mask: &TerrainMask, pos: FixedPoint) -> bool {
    let (x_cell, y_cell) = pos.to_cells();
    material_at(mask, x_cell, y_cell).is_solid()
}

/// Applies a terrain operation to the mask and returns the number of cells removed.
///
/// Dispatches on the operation's [`TerrainShape`] and respects the [`MaterialMask`]:
/// a material not permitted by the mask is left intact. This is how reinforced stone
/// resists everything except the Breach Pick.
///
/// Only iterates the bounding box of the operation, not the entire mask.
#[must_use]
pub fn apply_operation(
    mask: &mut TerrainMask,
    operation: &TerrainOperation,
) -> u32 {
    match operation.shape {
        TerrainShape::SubtractCircle { center, radius_cells } => {
            subtract_circle(mask, center, radius_cells, operation.material_mask)
        }
        TerrainShape::SubtractCapsule {
            start,
            end,
            radius_cells,
        } => subtract_capsule(mask, start, end, radius_cells, operation.material_mask),
    }
}

/// Subtracts a circle of terrain, removing only materials permitted by the mask.
fn subtract_circle(
    mask: &mut TerrainMask,
    center: FixedPoint,
    radius_cells: u16,
    material_mask: MaterialMask,
) -> u32 {
    let (center_x, center_y) = center.to_cells();
    let radius = radius_cells as i32;
    let radius_squared = (radius as i64).saturating_mul(radius as i64);

    // Compute bounding box, clamped to mask bounds
    let min_x = (center_x.saturating_sub(radius)).max(0);
    let max_x = (center_x.saturating_add(radius)).min((mask.width as i32).saturating_sub(1));
    let min_y = (center_y.saturating_sub(radius)).max(0);
    let max_y = (center_y.saturating_add(radius)).min((mask.height as i32).saturating_sub(1));

    let mut removed = 0u32;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            // Compute squared distance from (x, y) to center
            let dx = (x as i64).saturating_sub(center_x as i64);
            let dy = (y as i64).saturating_sub(center_y as i64);
            let dist_squared = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));

            if dist_squared <= radius_squared {
                // Within radius; check material and remove if permitted
                let current_material = material_at(mask, x, y);
                if current_material.is_solid() && material_mask.permits(current_material) {
                    let _ = set_material(mask, x, y, Material::Empty);
                    removed = removed.saturating_add(1);
                }
            }
        }
    }

    removed
}

/// Subtracts a capsule (swept circle) of terrain, removing only materials permitted by the mask.
fn subtract_capsule(
    mask: &mut TerrainMask,
    start: FixedPoint,
    end: FixedPoint,
    radius_cells: u16,
    material_mask: MaterialMask,
) -> u32 {
    let (start_x, start_y) = start.to_cells();
    let (end_x, end_y) = end.to_cells();
    let radius = radius_cells as i32;
    let radius_squared = (radius as i64).saturating_mul(radius as i64);

    // Compute bounding box of the entire capsule
    let min_x = (start_x.min(end_x).saturating_sub(radius)).max(0);
    let max_x = (start_x.max(end_x).saturating_add(radius)).min((mask.width as i32).saturating_sub(1));
    let min_y = (start_y.min(end_y).saturating_sub(radius)).max(0);
    let max_y = (start_y.max(end_y).saturating_add(radius)).min((mask.height as i32).saturating_sub(1));

    let mut removed = 0u32;

    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if point_in_capsule(x, y, start_x, start_y, end_x, end_y, radius_squared) {
                let current_material = material_at(mask, x, y);
                if current_material.is_solid() && material_mask.permits(current_material) {
                    let _ = set_material(mask, x, y, Material::Empty);
                    removed = removed.saturating_add(1);
                }
            }
        }
    }

    removed
}

/// Tests whether a point (in cell coordinates) is within a capsule (swept circle).
///
/// Uses point-to-segment squared distance, handling the degenerate zero-length segment
/// by treating it as a circle. This is the core geometric test that must match the
/// TypeScript oracle exactly.
fn point_in_capsule(
    px: i32,
    py: i32,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    radius_squared: i64,
) -> bool {
    let line_x = (end_x as i64).saturating_sub(start_x as i64);
    let line_y = (end_y as i64).saturating_sub(start_y as i64);
    let point_x = (px as i64).saturating_sub(start_x as i64);
    let point_y = (py as i64).saturating_sub(start_y as i64);

    let length_squared = line_x.saturating_mul(line_x).saturating_add(line_y.saturating_mul(line_y));

    // Degenerate case: start == end; treat as a circle
    if length_squared == 0 {
        let dist_squared = point_x.saturating_mul(point_x).saturating_add(point_y.saturating_mul(point_y));
        return dist_squared <= radius_squared;
    }

    let dot = point_x.saturating_mul(line_x).saturating_add(point_y.saturating_mul(line_y));

    // Point projects before the segment start
    if dot <= 0 {
        let dist_squared = point_x.saturating_mul(point_x).saturating_add(point_y.saturating_mul(point_y));
        return dist_squared <= radius_squared;
    }

    // Point projects after the segment end
    if dot >= length_squared {
        let end_x_diff = (px as i64).saturating_sub(end_x as i64);
        let end_y_diff = (py as i64).saturating_sub(end_y as i64);
        let dist_squared = end_x_diff.saturating_mul(end_x_diff).saturating_add(end_y_diff.saturating_mul(end_y_diff));
        return dist_squared <= radius_squared;
    }

    // Point projects onto the segment; use squared perpendicular distance
    // cross = pointX * lineY - pointY * lineX
    // distance_squared = cross^2 / length_squared
    // To avoid division, compare: cross^2 <= radius_squared * length_squared
    let cross = point_x.saturating_mul(line_y).saturating_sub(point_y.saturating_mul(line_x));
    let cross_squared = cross.saturating_mul(cross);
    let max_dist_squared = radius_squared.saturating_mul(length_squared);

    cross_squared <= max_dist_squared
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn create_mask_succeeds_with_valid_dimensions() {
        if let Ok(mask) = create_mask(10, 20, Material::Soil) {
            assert_eq!(mask.width, 10);
            assert_eq!(mask.height, 20);
            assert_eq!(mask.cells.len(), 200);
            assert!(mask.cells.iter().all(|&cell| cell == Material::Soil as u8));
        } else {
            panic!("create_mask failed");
        }
    }

    #[test]
    fn create_mask_rejects_zero_width() {
        let result = create_mask(0, 10, Material::Empty);
        assert!(matches!(result, Err(SimError::OutOfRange { field: "terrain dimensions" })));
    }

    #[test]
    fn create_mask_rejects_zero_height() {
        let result = create_mask(10, 0, Material::Empty);
        assert!(matches!(result, Err(SimError::OutOfRange { field: "terrain dimensions" })));
    }

    #[test]
    fn create_mask_rejects_overflow() {
        let result = create_mask(u32::MAX, u32::MAX, Material::Empty);
        assert!(matches!(result, Err(SimError::OutOfRange { field: "terrain cell count" })));
    }

    #[test]
    fn material_at_returns_correct_values() {
        if let Ok(mut mask) = create_mask(3, 3, Material::Empty) {
            let _ = set_material(&mut mask, 1, 1, Material::Soil);

            assert_eq!(material_at(&mask, 1, 1), Material::Soil);
            assert_eq!(material_at(&mask, 0, 0), Material::Empty);
            assert_eq!(material_at(&mask, 2, 2), Material::Empty);
        } else {
            panic!("create_mask failed");
        }
    }

    #[test]
    fn material_at_out_of_bounds_returns_empty() {
        if let Ok(mask) = create_mask(5, 5, Material::Soil) {
            assert_eq!(material_at(&mask, -1, 0), Material::Empty);
            assert_eq!(material_at(&mask, 0, -1), Material::Empty);
            assert_eq!(material_at(&mask, 5, 0), Material::Empty);
            assert_eq!(material_at(&mask, 0, 5), Material::Empty);
            assert_eq!(material_at(&mask, 10, 10), Material::Empty);
        } else {
            panic!("create_mask failed");
        }
    }

    #[test]
    fn set_material_updates_existing_cell() {
        if let Ok(mut mask) = create_mask(5, 5, Material::Empty) {
            assert!(set_material(&mut mask, 2, 2, Material::Wood));
            assert_eq!(material_at(&mask, 2, 2), Material::Wood);

            assert!(set_material(&mut mask, 2, 2, Material::ReinforcedStone));
            assert_eq!(material_at(&mask, 2, 2), Material::ReinforcedStone);
        } else {
            panic!("create_mask failed");
        }
    }

    #[test]
    fn set_material_returns_false_out_of_bounds() {
        if let Ok(mut mask) = create_mask(5, 5, Material::Empty) {
            assert!(!set_material(&mut mask, -1, 2, Material::Soil));
            assert!(!set_material(&mut mask, 2, -1, Material::Soil));
            assert!(!set_material(&mut mask, 5, 2, Material::Soil));
            assert!(!set_material(&mut mask, 2, 5, Material::Soil));
        } else {
            panic!("create_mask failed");
        }
    }

    #[test]
    fn is_solid_at_converts_fixed_point_to_cells() {
        if let (Ok(mut mask), Some(pos)) = (create_mask(10, 10, Material::Empty), FixedPoint::from_cells(2, 3)) {
            let _ = set_material(&mut mask, 2, 3, Material::Wood);
            assert!(is_solid_at(&mask, pos));

            if let Some(empty_pos) = FixedPoint::from_cells(0, 0) {
                assert!(!is_solid_at(&mask, empty_pos));
            } else {
                panic!("from_cells failed");
            }
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn subtract_circle_removes_cells_within_radius() {
        if let (Ok(mut mask), Some(center)) = (create_mask(20, 20, Material::Soil), FixedPoint::from_cells(10, 10)) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCircle {
                    center,
                    radius_cells: 3,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 10, 10), Material::Empty);
            assert_eq!(material_at(&mask, 10, 13), Material::Empty);
            assert_eq!(material_at(&mask, 13, 10), Material::Empty);
            assert_eq!(material_at(&mask, 10, 14), Material::Soil);

            assert!(removed > 0);
            assert!(removed < 100);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn subtract_circle_at_origin_handles_bounds() {
        if let (Ok(mut mask), Some(center)) = (create_mask(10, 10, Material::Wood), FixedPoint::from_cells(0, 0)) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCircle {
                    center,
                    radius_cells: 5,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 0, 0), Material::Empty);
            assert!(removed > 0);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn subtract_circle_respects_material_mask_soft() {
        if let Ok(mut mask) = create_mask(10, 10, Material::Empty) {
            for y in 0..10 {
                for x in 0..10 {
                    let material = if (x + y) % 2 == 0 {
                        Material::Soil
                    } else {
                        Material::ReinforcedStone
                    };
                    let _ = set_material(&mut mask, x as i32, y as i32, material);
                }
            }

            if let Some(center) = FixedPoint::from_cells(5, 5) {
                let op = TerrainOperation {
                    sequence: 0,
                    shape: TerrainShape::SubtractCircle {
                        center,
                        radius_cells: 3,
                    },
                    material_mask: MaterialMask::SOFT,
                };

                let _ = apply_operation(&mut mask, &op);
            } else {
                panic!("from_cells failed");
            }
        } else {
            panic!("create_mask failed");
        }
    }

    #[test]
    fn subtract_circle_respects_material_mask_all() {
        if let (Ok(mut mask), Some(center)) = (create_mask(10, 10, Material::ReinforcedStone), FixedPoint::from_cells(5, 5)) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCircle {
                    center,
                    radius_cells: 2,
                },
                material_mask: MaterialMask::ALL,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 5, 5), Material::Empty);
            assert!(removed > 0);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn subtract_capsule_line_segment() {
        if let (Ok(mut mask), Some(start), Some(end)) = (
            create_mask(20, 20, Material::Soil),
            FixedPoint::from_cells(5, 5),
            FixedPoint::from_cells(15, 5),
        ) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCapsule {
                    start,
                    end,
                    radius_cells: 2,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 5, 5), Material::Empty);
            assert_eq!(material_at(&mask, 10, 5), Material::Empty);
            assert_eq!(material_at(&mask, 15, 5), Material::Empty);
            assert_eq!(material_at(&mask, 10, 6), Material::Empty);
            assert_eq!(material_at(&mask, 10, 7), Material::Empty);
            assert_eq!(material_at(&mask, 10, 8), Material::Soil);

            assert!(removed > 0);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn subtract_capsule_degenerate_zero_length() {
        if let (Ok(mut mask), Some(pos)) = (create_mask(15, 15, Material::Soil), FixedPoint::from_cells(7, 7)) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCapsule {
                    start: pos,
                    end: pos,
                    radius_cells: 3,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 7, 7), Material::Empty);
            assert_eq!(material_at(&mask, 7, 10), Material::Empty);
            assert_eq!(material_at(&mask, 7, 11), Material::Soil);

            assert!(removed > 0);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn subtract_capsule_diagonal() {
        if let (Ok(mut mask), Some(start), Some(end)) = (
            create_mask(20, 20, Material::Wood),
            FixedPoint::from_cells(5, 5),
            FixedPoint::from_cells(15, 15),
        ) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCapsule {
                    start,
                    end,
                    radius_cells: 2,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 5, 5), Material::Empty);
            assert_eq!(material_at(&mask, 10, 10), Material::Empty);
            assert_eq!(material_at(&mask, 15, 15), Material::Empty);

            assert!(removed > 0);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn subtract_capsule_at_edges_no_overflow() {
        if let (Ok(mut mask), Some(start), Some(end)) = (
            create_mask(10, 10, Material::Soil),
            FixedPoint::from_cells(0, 0),
            FixedPoint::from_cells(9, 9),
        ) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCapsule {
                    start,
                    end,
                    radius_cells: 3,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert!(removed > 0);
            assert_eq!(material_at(&mask, 0, 0), Material::Empty);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn circle_radius_zero() {
        if let (Ok(mut mask), Some(center)) = (create_mask(5, 5, Material::Soil), FixedPoint::from_cells(2, 2)) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCircle {
                    center,
                    radius_cells: 0,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 2, 2), Material::Empty);
            assert_eq!(material_at(&mask, 2, 3), Material::Soil);
            assert_eq!(removed, 1);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn large_circle_respects_mask_bounds() {
        if let (Ok(mut mask), Some(center)) = (create_mask(10, 10, Material::Soil), FixedPoint::from_cells(5, 5)) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCircle {
                    center,
                    radius_cells: 100,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(removed, 100);
            assert!(mask.cells.iter().all(|&c| c == 0));
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn reinforced_stone_survives_soft_mask() {
        if let (Ok(mut mask), Some(center)) = (create_mask(5, 5, Material::ReinforcedStone), FixedPoint::from_cells(2, 2)) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCircle {
                    center,
                    radius_cells: 2,
                },
                material_mask: MaterialMask::SOFT,
            };

            let removed = apply_operation(&mut mask, &op);

            assert_eq!(removed, 0);
            assert_eq!(material_at(&mask, 2, 2), Material::ReinforcedStone);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn reinforced_stone_removed_with_all_mask() {
        if let (Ok(mut mask), Some(center)) = (create_mask(5, 5, Material::ReinforcedStone), FixedPoint::from_cells(2, 2)) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCircle {
                    center,
                    radius_cells: 2,
                },
                material_mask: MaterialMask::ALL,
            };

            let removed = apply_operation(&mut mask, &op);

            assert!(removed > 0);
            assert_eq!(material_at(&mask, 2, 2), Material::Empty);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn point_in_capsule_perpendicular_distance() {
        if let (Ok(mut mask), Some(start), Some(end)) = (
            create_mask(20, 20, Material::Soil),
            FixedPoint::from_cells(5, 5),
            FixedPoint::from_cells(15, 5),
        ) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCapsule {
                    start,
                    end,
                    radius_cells: 2,
                },
                material_mask: MaterialMask::SOFT,
            };

            let _ = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 10, 7), Material::Empty);
            assert_eq!(material_at(&mask, 10, 8), Material::Soil);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn capsule_start_cap() {
        if let (Ok(mut mask), Some(start), Some(end)) = (
            create_mask(15, 15, Material::Soil),
            FixedPoint::from_cells(5, 5),
            FixedPoint::from_cells(15, 5),
        ) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCapsule {
                    start,
                    end,
                    radius_cells: 2,
                },
                material_mask: MaterialMask::SOFT,
            };

            let _ = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 5, 7), Material::Empty);
            assert_eq!(material_at(&mask, 5, 8), Material::Soil);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }

    #[test]
    fn capsule_end_cap() {
        if let (Ok(mut mask), Some(start), Some(end)) = (
            create_mask(20, 20, Material::Soil),
            FixedPoint::from_cells(5, 5),
            FixedPoint::from_cells(15, 5),
        ) {
            let op = TerrainOperation {
                sequence: 0,
                shape: TerrainShape::SubtractCapsule {
                    start,
                    end,
                    radius_cells: 2,
                },
                material_mask: MaterialMask::SOFT,
            };

            let _ = apply_operation(&mut mask, &op);

            assert_eq!(material_at(&mask, 15, 7), Material::Empty);
            assert_eq!(material_at(&mask, 15, 8), Material::Soil);
        } else {
            panic!("create_mask or from_cells failed");
        }
    }
}
