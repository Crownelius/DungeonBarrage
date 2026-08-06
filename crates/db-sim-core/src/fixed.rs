//! Fixed-point arithmetic primitives.
//!
//! The authoritative simulation contains no floating point (ADR 0001). Every spatial
//! quantity is an [`i32`] in units of 1/[`POSITION_SCALE`] of a terrain cell, and every
//! intermediate that could overflow 32 bits is widened to [`i64`] explicitly.
//!
//! These functions are the *only* sanctioned way to multiply, divide, or scale spatial
//! values. Open-coding the arithmetic elsewhere risks a rounding difference that breaks
//! bit-exact parity with the TypeScript oracle.
//!
//! Integer division is this module's entire purpose, so `clippy::integer_division` is
//! allowed here and nowhere else. Every truncating divide below is deliberate and its
//! rounding rule is documented at the call site.

#![allow(
    clippy::integer_division,
    reason = "fixed-point arithmetic is integer division by definition; rounding rules are documented per function"
)]

/// Simulation units per terrain cell.
pub const POSITION_SCALE: i32 = 1_024;

/// One character body width (BW), the map-independent range unit used by `ARSENAL.md`.
pub const BODY_WIDTH: i32 = 4 * POSITION_SCALE;

/// Canonical 1.25 BW launch-roster melee reach.
pub const BASE_MELEE_RANGE: i32 = 5 * POSITION_SCALE;

/// Authoritative ticks per second. Rendering is independent of this.
pub const FIXED_TICK_RATE: u32 = 60;

/// Basis points denominator (100% = 10_000 bp).
pub const BASIS_POINTS: i32 = 10_000;

/// A position in fixed-point simulation units.
///
/// Integer-only by construction: there is no constructor that accepts a float.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FixedPoint {
    /// Horizontal component, in 1/[`POSITION_SCALE`] cell units.
    pub x: i32,
    /// Vertical component, in 1/[`POSITION_SCALE`] cell units. Positive is downward,
    /// matching the terrain mask's row-major top-left origin.
    pub y: i32,
}

impl FixedPoint {
    /// The origin.
    pub const ZERO: Self = Self { x: 0, y: 0 };

    /// Constructs a position from raw fixed-point components.
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Constructs a position from whole terrain cells.
    ///
    /// Returns [`None`] on overflow rather than wrapping, so an out-of-range map
    /// definition fails loudly at load time instead of producing a wrapped coordinate.
    #[must_use]
    pub const fn from_cells(x: i32, y: i32) -> Option<Self> {
        match (x.checked_mul(POSITION_SCALE), y.checked_mul(POSITION_SCALE)) {
            (Some(x), Some(y)) => Some(Self { x, y }),
            _ => None,
        }
    }

    /// The whole terrain cell containing this position.
    ///
    /// Uses floor division so that negative coordinates map to the cell below them
    /// rather than truncating toward zero — truncation would make cell 0 twice as wide
    /// as every other cell and put a seam at the origin.
    #[must_use]
    pub const fn to_cells(self) -> (i32, i32) {
        (
            self.x.div_euclid(POSITION_SCALE),
            self.y.div_euclid(POSITION_SCALE),
        )
    }

    /// Component-wise saturating addition.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self {
            x: self.x.saturating_add(other.x),
            y: self.y.saturating_add(other.y),
        }
    }

    /// Component-wise saturating subtraction.
    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self {
            x: self.x.saturating_sub(other.x),
            y: self.y.saturating_sub(other.y),
        }
    }
}

/// Divides and rounds half away from zero.
///
/// This is the exact rounding rule used by the TypeScript oracle's `roundDivide`, and
/// parity depends on matching it precisely. Rust's `/` truncates toward zero and
/// `div_euclid` floors; neither matches, so this must be used for every scaled divide.
///
/// # Panics
///
/// Never. A non-positive `denominator` returns [`None`].
#[must_use]
pub const fn round_divide(numerator: i64, denominator: i64) -> Option<i64> {
    if denominator <= 0 {
        return None;
    }
    let half = denominator / 2;
    if numerator < 0 {
        // i64::MIN has no positive counterpart, so negation is checked rather than
        // assumed. Reflecting the positive case keeps the rounding symmetric.
        let Some(positive) = numerator.checked_neg() else {
            return None;
        };
        let Some(shifted) = positive.checked_add(half) else {
            return None;
        };
        match shifted.checked_div(denominator) {
            Some(quotient) => quotient.checked_neg(),
            None => None,
        }
    } else {
        match numerator.checked_add(half) {
            Some(shifted) => shifted.checked_div(denominator),
            None => None,
        }
    }
}

/// Scales `value` by `numerator / denominator`, rounding half away from zero.
///
/// Widens to [`i64`] internally so that a large scale factor cannot overflow before the
/// divide brings the magnitude back down.
#[must_use]
pub const fn scale(value: i32, numerator: i32, denominator: i32) -> Option<i32> {
    // Two widened i32 values cannot overflow an i64 product, but the multiply stays
    // checked so the guarantee survives any future widening of the inputs.
    let Some(product) = (value as i64).checked_mul(numerator as i64) else {
        return None;
    };
    match round_divide(product, denominator as i64) {
        Some(scaled) if scaled >= i32::MIN as i64 && scaled <= i32::MAX as i64 => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the match guard proves the value is within i32; try_from is not const"
            )]
            Some(scaled as i32)
        }
        _ => None,
    }
}

/// Applies a basis-point factor to `value` (10_000 bp == 1.0), rounding half away from zero.
#[must_use]
pub const fn apply_basis_points(value: i32, basis_points: i32) -> Option<i32> {
    scale(value, basis_points, BASIS_POINTS)
}

/// Squared Euclidean distance between two positions.
///
/// Returns [`i64`] because the squared distance across a full map exceeds [`i32`].
/// Compare squared distances against squared radii; never take a square root, because a
/// fixed-point `sqrt` is an unnecessary source of rounding divergence.
///
/// Saturates rather than wrapping. Two extreme coordinates can square past [`i64::MAX`],
/// and a wrapped result would read as *adjacent* — turning an impossible distance into a
/// spurious hit. Saturation preserves the only property callers rely on: farther is never
/// reported as nearer.
#[must_use]
pub const fn distance_squared(a: FixedPoint, b: FixedPoint) -> i64 {
    let dx = (a.x as i64).saturating_sub(b.x as i64);
    let dy = (a.y as i64).saturating_sub(b.y as i64);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

/// Whether `a` and `b` are within `radius` of each other.
#[must_use]
pub const fn within_radius(a: FixedPoint, b: FixedPoint, radius: i32) -> bool {
    let r = radius as i64;
    distance_squared(a, b) <= r.saturating_mul(r)
}

/// Clamps `value` into `[min, max]`.
///
/// Returns `min` when the bounds are inverted, so a malformed definition degrades to a
/// deterministic value rather than an unpredictable one.
#[must_use]
pub const fn clamp(value: i32, min: i32, max: i32) -> i32 {
    if min > max {
        return min;
    }
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_divide_rounds_half_away_from_zero() {
        assert_eq!(round_divide(5, 2), Some(3));
        assert_eq!(round_divide(-5, 2), Some(-3));
        assert_eq!(round_divide(4, 2), Some(2));
        assert_eq!(round_divide(-4, 2), Some(-2));
        assert_eq!(round_divide(1, 2), Some(1));
        assert_eq!(round_divide(-1, 2), Some(-1));
        assert_eq!(round_divide(0, 7), Some(0));
    }

    #[test]
    fn round_divide_rejects_non_positive_denominators() {
        assert_eq!(round_divide(10, 0), None);
        assert_eq!(round_divide(10, -2), None);
    }

    #[test]
    fn to_cells_floors_through_the_origin() {
        // Truncation toward zero would make cell 0 span [-1023, 1023] — twice as wide as
        // every other cell. Floor division keeps every cell exactly POSITION_SCALE wide.
        assert_eq!(FixedPoint::new(-1, 0).to_cells().0, -1);
        assert_eq!(FixedPoint::new(0, 0).to_cells().0, 0);
        assert_eq!(FixedPoint::new(POSITION_SCALE - 1, 0).to_cells().0, 0);
        assert_eq!(FixedPoint::new(POSITION_SCALE, 0).to_cells().0, 1);
        assert_eq!(FixedPoint::new(-POSITION_SCALE, 0).to_cells().0, -1);
    }

    #[test]
    fn from_cells_reports_overflow_instead_of_wrapping() {
        assert_eq!(FixedPoint::from_cells(2, 3), Some(FixedPoint::new(2048, 3072)));
        assert_eq!(FixedPoint::from_cells(i32::MAX, 0), None);
    }

    #[test]
    fn scale_widens_before_dividing() {
        // Would overflow i32 if the multiply were not widened to i64 first.
        assert_eq!(scale(1_000_000, 10_000, 10_000), Some(1_000_000));
        assert_eq!(apply_basis_points(200, 7_500), Some(150));
        assert_eq!(apply_basis_points(1, 5_000), Some(1)); // half rounds away from zero
    }

    #[test]
    fn within_radius_uses_squared_comparison() {
        let origin = FixedPoint::ZERO;
        assert!(within_radius(origin, FixedPoint::new(3, 4), 5));
        assert!(!within_radius(origin, FixedPoint::new(3, 4), 4));
    }

    #[test]
    fn clamp_is_total_even_with_inverted_bounds() {
        assert_eq!(clamp(5, 0, 10), 5);
        assert_eq!(clamp(-5, 0, 10), 0);
        assert_eq!(clamp(50, 0, 10), 10);
        assert_eq!(clamp(5, 10, 0), 10);
    }
}
