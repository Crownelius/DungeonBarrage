//! Fixed-point projectile trajectory integration with terrain and character collision.
//!
//! Ports `sampleBallisticTrajectory` / `sampleWeaponTrajectory` from
//! the retired TypeScript oracle (`reference/simulation-oracle-retired.ts`) to integer-only arithmetic. The integration order is
//! **semi-implicit (symplectic) Euler**: velocity is advanced by acceleration first,
//! then position is advanced by the *new* velocity, every tick. Matching this order is
//! not cosmetic — swapping it changes every trajectory's shape (semi-implicit Euler is
//! numerically stable for oscillatory/ballistic motion in a way explicit Euler is not),
//! so the oracle's tick body is reproduced line-for-line:
//! `velocity += acceleration; position += velocity;`.
//!
//! # Parity blocker: the oracle's launch vector is floating point
//!
//! `sampleBallisticTrajectory` (`lib/game/simulation.ts:582-585`) computes the initial
//! velocity with `Math.cos` / `Math.sin` on a `f64` radian value:
//!
//! ```ts
//! const radians = (input.angleMilliDegrees * Math.PI) / 180_000;
//! let velocityX = Math.round(Math.cos(radians) * launchSpeed);
//! let velocityY = -Math.round(Math.sin(radians) * launchSpeed);
//! ```
//!
//! This crate forbids floating point entirely (`clippy::float_arithmetic = "deny"`,
//! ADR 0001), and even if it didn't, IEEE-754 `f64` transcendental functions are not
//! guaranteed bit-identical across platforms/engines to begin with — `wasm32` and
//! `x86_64` `Math.cos` are not contractually the same libm. **There is no fixed-point
//! computation that reproduces `Math.cos`/`Math.sin` bit-exactly for arbitrary input**,
//! so this is a genuine parity gap, not an implementation shortcoming to be closed by
//! trying harder. It is reported upward (see the workspace blocker report) rather than
//! papered over.
//!
//! This module's answer is [`SINE_TABLE`]: a quantized, degree-resolution lookup table
//! with linear interpolation between entries, computed once offline (this file's
//! `python3` generation script is not part of the build) and compiled in as integer
//! constants. **The concrete recommendation is that the TypeScript oracle adopt the same
//! table** — replacing `Math.cos`/`Math.sin` with an equivalent quantized lookup — so
//! both implementations agree on the exact same discretized values instead of one side
//! computing "true" trigonometry that the other can only approximate. Until that oracle
//! change lands, differential parity tests against `sampleBallisticTrajectory` will show
//! small, bounded deviations (at most the table's linear-interpolation error against true
//! `sin`/`cos`, well under 0.1% of the unit vector) for any non-axis-aligned launch angle.
//!
//! # New behavior not in the oracle
//!
//! Character collision and multi-bounce reflection do not exist in
//! `sampleBallisticTrajectory` at all (ADR 0002: the retired weapon oracle never modeled
//! player-character projectile contact or a bouncing grenade). There is no oracle line to
//! match for these paths; they are designed fresh here and documented at the point of
//! definition ([`reflection_axes`]).

use crate::error::{SimError, SimResult};
use crate::fixed::{self, FixedPoint};
use crate::terrain;
use crate::types::{
    BallisticImpact, BallisticInput, BallisticResult, BallisticSample, ImpactCause,
    ProjectileAttack, TerrainMask,
};

// ---------------------------------------------------------------------------
// Fixed-point trigonometry
// ---------------------------------------------------------------------------

/// Fixed-point scale for [`SINE_TABLE`] entries: this value represents `1.0`.
///
/// Q16 (`2^16`): comfortably more precision than the integer velocities it multiplies
/// against need, while staying far inside `i64` headroom for the widened multiply in
/// [`fixed::scale`].
const SIN_SCALE: i32 = 65_536;

/// Degrees represented by one [`SINE_TABLE`] entry.
const DEGREE_STEP_MILLIDEGREES: i32 = 1_000;

/// One full turn, in millidegrees.
const FULL_CIRCLE_MILLIDEGREES: i32 = 360_000;

/// One quarter turn, in millidegrees. `cos(x) == sin(x + QUARTER_CIRCLE_MILLIDEGREES)`.
const QUARTER_CIRCLE_MILLIDEGREES: i32 = 90_000;

/// `sin(degrees)` for whole degrees `0..=360` (361 entries), each entry
/// `round_half_away_from_zero(sin(radians(degrees)) * SIN_SCALE)`.
///
/// Generated offline (a `python3` script using `math.sin`, run once by the author, is not
/// part of this crate or its build) since the crate forbids floating point at compile and
/// run time alike; only the resulting integers are compiled in. `cos` is derived from this
/// same table via `cos(x) = sin(x + 90 degrees)` in [`cosine_scaled`] rather than stored
/// twice, which halves the constant data and guarantees the two curves never drift apart.
///
/// Symmetric by construction: `table[0] == table[360] == 0`, `table[90] == SIN_SCALE`,
/// `table[180] == 0`, `table[270] == -SIN_SCALE`.
#[rustfmt::skip]
const SINE_TABLE: [i32; 361] = [
    0, 1144, 2287, 3430, 4572, 5712, 6850, 7987, 9121, 10252,
    11380, 12505, 13626, 14742, 15855, 16962, 18064, 19161, 20252, 21336,
    22415, 23486, 24550, 25607, 26656, 27697, 28729, 29753, 30767, 31772,
    32768, 33754, 34729, 35693, 36647, 37590, 38521, 39441, 40348, 41243,
    42126, 42995, 43852, 44695, 45525, 46341, 47143, 47930, 48703, 49461,
    50203, 50931, 51643, 52339, 53020, 53684, 54332, 54963, 55578, 56175,
    56756, 57319, 57865, 58393, 58903, 59396, 59870, 60326, 60764, 61183,
    61584, 61966, 62328, 62672, 62997, 63303, 63589, 63856, 64104, 64332,
    64540, 64729, 64898, 65048, 65177, 65287, 65376, 65446, 65496, 65526,
    65536, 65526, 65496, 65446, 65376, 65287, 65177, 65048, 64898, 64729,
    64540, 64332, 64104, 63856, 63589, 63303, 62997, 62672, 62328, 61966,
    61584, 61183, 60764, 60326, 59870, 59396, 58903, 58393, 57865, 57319,
    56756, 56175, 55578, 54963, 54332, 53684, 53020, 52339, 51643, 50931,
    50203, 49461, 48703, 47930, 47143, 46341, 45525, 44695, 43852, 42995,
    42126, 41243, 40348, 39441, 38521, 37590, 36647, 35693, 34729, 33754,
    32768, 31772, 30767, 29753, 28729, 27697, 26656, 25607, 24550, 23486,
    22415, 21336, 20252, 19161, 18064, 16962, 15855, 14742, 13626, 12505,
    11380, 10252, 9121, 7987, 6850, 5712, 4572, 3430, 2287, 1144,
    0, -1144, -2287, -3430, -4572, -5712, -6850, -7987, -9121, -10252,
    -11380, -12505, -13626, -14742, -15855, -16962, -18064, -19161, -20252, -21336,
    -22415, -23486, -24550, -25607, -26656, -27697, -28729, -29753, -30767, -31772,
    -32768, -33754, -34729, -35693, -36647, -37590, -38521, -39441, -40348, -41243,
    -42126, -42995, -43852, -44695, -45525, -46341, -47143, -47930, -48703, -49461,
    -50203, -50931, -51643, -52339, -53020, -53684, -54332, -54963, -55578, -56175,
    -56756, -57319, -57865, -58393, -58903, -59396, -59870, -60326, -60764, -61183,
    -61584, -61966, -62328, -62672, -62997, -63303, -63589, -63856, -64104, -64332,
    -64540, -64729, -64898, -65048, -65177, -65287, -65376, -65446, -65496, -65526,
    -65536, -65526, -65496, -65446, -65376, -65287, -65177, -65048, -64898, -64729,
    -64540, -64332, -64104, -63856, -63589, -63303, -62997, -62672, -62328, -61966,
    -61584, -61183, -60764, -60326, -59870, -59396, -58903, -58393, -57865, -57319,
    -56756, -56175, -55578, -54963, -54332, -53684, -53020, -52339, -51643, -50931,
    -50203, -49461, -48703, -47930, -47143, -46341, -45525, -44695, -43852, -42995,
    -42126, -41243, -40348, -39441, -38521, -37590, -36647, -35693, -34729, -33754,
    -32768, -31772, -30767, -29753, -28729, -27697, -26656, -25607, -24550, -23486,
    -22415, -21336, -20252, -19161, -18064, -16962, -15855, -14742, -13626, -12505,
    -11380, -10252, -9121, -7987, -6850, -5712, -4572, -3430, -2287, -1144,
    0,
];

/// `sin(angle_millidegrees)` scaled by [`SIN_SCALE`], linearly interpolated between the
/// whole-degree [`SINE_TABLE`] entries for sub-degree precision.
///
/// # Errors
///
/// Returns [`SimError::Overflow`] only for pathological inputs; every angle produced by
/// normal gameplay (quantized millidegrees on an `i32`) succeeds.
fn sine_scaled(angle_millidegrees: i32) -> SimResult<i32> {
    // rem_euclid always returns a value in [0, FULL_CIRCLE_MILLIDEGREES) for a positive
    // divisor, including for negative inputs and i32::MIN — this is integer modular
    // reduction, not floating-point trigonometry, and cannot panic for a positive rhs.
    let normalized = angle_millidegrees.rem_euclid(FULL_CIRCLE_MILLIDEGREES);

    #[expect(
        clippy::integer_division,
        reason = "table index: floor(normalized / 1000) selects the whole-degree entry at or below the angle; normalized is non-negative so floor == truncation"
    )]
    let degree_index = normalized / DEGREE_STEP_MILLIDEGREES;
    let remainder_millidegrees =
        normalized
            .checked_rem(DEGREE_STEP_MILLIDEGREES)
            .ok_or(SimError::Overflow {
                context: "ballistics::sine_table_remainder",
            })?;

    let index = usize::try_from(degree_index).map_err(|_| SimError::Overflow {
        context: "ballistics::sine_table_index",
    })?;
    let low = *SINE_TABLE.get(index).ok_or(SimError::Overflow {
        context: "ballistics::sine_table_low",
    })?;
    let high_index = index.checked_add(1).ok_or(SimError::Overflow {
        context: "ballistics::sine_table_high_index",
    })?;
    let high = *SINE_TABLE.get(high_index).ok_or(SimError::Overflow {
        context: "ballistics::sine_table_high",
    })?;

    let delta = high.checked_sub(low).ok_or(SimError::Overflow {
        context: "ballistics::sine_table_delta",
    })?;
    // Linear interpolation weight in [0, 1000): remainder_millidegrees / DEGREE_STEP.
    let interpolated = fixed::scale(delta, remainder_millidegrees, DEGREE_STEP_MILLIDEGREES)
        .ok_or(SimError::Overflow {
            context: "ballistics::sine_interpolate",
        })?;

    low.checked_add(interpolated).ok_or(SimError::Overflow {
        context: "ballistics::sine_result",
    })
}

/// `cos(angle_millidegrees)` scaled by [`SIN_SCALE`], via the identity
/// `cos(x) = sin(x + 90 degrees)` against the same [`SINE_TABLE`].
///
/// # Errors
///
/// Returns [`SimError::Overflow`] only for pathological inputs; see [`sine_scaled`].
fn cosine_scaled(angle_millidegrees: i32) -> SimResult<i32> {
    let shifted = angle_millidegrees
        .checked_add(QUARTER_CIRCLE_MILLIDEGREES)
        .ok_or(SimError::Overflow {
            context: "ballistics::cosine_shift",
        })?;
    sine_scaled(shifted)
}

// ---------------------------------------------------------------------------
// Sampling
// ---------------------------------------------------------------------------

/// Fixed tick stride between recorded playback samples, beyond the launch and impact
/// samples (which are always recorded regardless of this stride).
///
/// At [`fixed::FIXED_TICK_RATE`] (60 ticks/second) a stride of 4 records roughly 15
/// samples per second of flight — enough for a client to interpolate a visually smooth
/// arc — instead of one message per tick, which would be wasted bandwidth on a multi-
/// second flight (hundreds of ticks) that a player only watches, never inputs against.
const SAMPLE_STRIDE: u32 = 4;

/// Whether `tick` falls on the fixed playback stride (see [`SAMPLE_STRIDE`]).
///
/// `is_multiple_of` rather than `tick % SAMPLE_STRIDE == 0`: it expresses the same check
/// without a raw `%` operator, so there is no arithmetic expression here for
/// `clippy::arithmetic_side_effects` to have an opinion about in the first place.
#[must_use]
fn tick_on_stride(tick: u32) -> bool {
    tick.is_multiple_of(SAMPLE_STRIDE)
}

// ---------------------------------------------------------------------------
// Bounce reflection
// ---------------------------------------------------------------------------

/// Determines which velocity axis (or axes) should invert when a projectile bounces off
/// terrain, using only the binary occupancy mask and the last-valid (`prev`) and
/// would-be-solid (`next`) positions.
///
/// The terrain mask has no continuous surface geometry — each cell is simply solid or
/// empty — so there is no analytic surface normal to read off. Instead this uses the
/// standard axis-separated tile-collision technique: split the illegal step into its
/// horizontal-only and vertical-only components and test each in isolation.
///
/// - If moving *only* horizontally, `(next.x, prev.y)`, is already solid, the projectile
///   ran into a vertical wall face; its X velocity reflects.
/// - If moving *only* vertically, `(prev.x, next.y)`, is already solid, it ran into a
///   horizontal surface (floor or ceiling); its Y velocity reflects.
/// - Both can be true at a concave (inner) corner, in which case both axes reflect.
/// - If *neither* isolated half-step is solid, the diagonal step corner-cut through a
///   single solid cell that neither axial half-step touches (the classic tunneling gap in
///   grid collision). Both axes reflect as a deterministic fallback: a grenade clipping a
///   corner is still expected to bounce, not pass through.
///
/// This is fully deterministic: it is a pure function of the mask and the two positions,
/// with no ordering dependence and no randomness.
#[must_use]
fn reflection_axes(prev: FixedPoint, next: FixedPoint, terrain_mask: &TerrainMask) -> (bool, bool) {
    let horizontal_only = FixedPoint::new(next.x, prev.y);
    let vertical_only = FixedPoint::new(prev.x, next.y);
    let reflect_x = terrain::is_solid_at(terrain_mask, horizontal_only);
    let reflect_y = terrain::is_solid_at(terrain_mask, vertical_only);

    if reflect_x || reflect_y {
        (reflect_x, reflect_y)
    } else {
        // Corner-cut fallback: neither axial half-step is solid, yet the full diagonal
        // step is. Reflect both axes rather than let the projectile pass through.
        (true, true)
    }
}

// ---------------------------------------------------------------------------
// Bounds
// ---------------------------------------------------------------------------

/// The exclusive upper X bound of the playable area, in fixed-point units.
fn bounds_max_x(terrain_mask: &TerrainMask) -> SimResult<i32> {
    let width = i32::try_from(terrain_mask.width).map_err(|_| SimError::Overflow {
        context: "ballistics::bounds_max_x_width",
    })?;
    width
        .checked_mul(fixed::POSITION_SCALE)
        .ok_or(SimError::Overflow {
            context: "ballistics::bounds_max_x",
        })
}

/// The exclusive upper Y bound of the playable area, in fixed-point units.
///
/// There is deliberately no lower Y bound check, matching the oracle exactly: a shot fired
/// straight up may fly arbitrarily far above the map before gravity brings it back down.
fn bounds_max_y(terrain_mask: &TerrainMask) -> SimResult<i32> {
    let height = i32::try_from(terrain_mask.height).map_err(|_| SimError::Overflow {
        context: "ballistics::bounds_max_y_height",
    })?;
    height
        .checked_mul(fixed::POSITION_SCALE)
        .ok_or(SimError::Overflow {
            context: "ballistics::bounds_max_y",
        })
}

// ---------------------------------------------------------------------------
// Launch vector
// ---------------------------------------------------------------------------

/// Computes the initial `(velocity_x, velocity_y)` from the launch angle and power.
///
/// Mirrors the oracle's launch-vector computation (`launchSpeed` scaling, then splitting
/// into X/Y by cosine/sine) using the fixed-point [`SINE_TABLE`] in place of
/// `Math.cos`/`Math.sin` — see the module-level parity note. `velocity_y` is negated
/// because +Y is downward here (matching the terrain mask's top-left row-major origin)
/// while a positive launch angle should send the projectile upward, exactly as the oracle
/// negates its `Math.sin` term for the same reason.
fn launch_velocity(attack: &ProjectileAttack, input: &BallisticInput) -> SimResult<(i32, i32)> {
    // roundDivide(speedPerTick * powerBasisPoints, 10_000) in the oracle.
    let launch_speed = fixed::scale(
        attack.speed_per_tick,
        input.power_basis_points,
        fixed::BASIS_POINTS,
    )
    .ok_or(SimError::Overflow {
        context: "ballistics::launch_speed",
    })?;

    let cos_scaled = cosine_scaled(input.angle_millidegrees)?;
    let sin_scaled = sine_scaled(input.angle_millidegrees)?;

    let velocity_x =
        fixed::scale(launch_speed, cos_scaled, SIN_SCALE).ok_or(SimError::Overflow {
            context: "ballistics::velocity_x_launch",
        })?;
    let raw_velocity_y =
        fixed::scale(launch_speed, sin_scaled, SIN_SCALE).ok_or(SimError::Overflow {
            context: "ballistics::velocity_y_launch",
        })?;
    let velocity_y = raw_velocity_y.checked_neg().ok_or(SimError::Overflow {
        context: "ballistics::velocity_y_launch_negate",
    })?;

    Ok((velocity_x, velocity_y))
}

// ---------------------------------------------------------------------------
// Integration
// ---------------------------------------------------------------------------

/// Appends the terminal sample (always present regardless of stride) and builds the
/// final [`BallisticResult`] for an early termination (out-of-bounds, terrain, or
/// character contact). Not used for the `Expired` (`max_ticks` exhausted) path, which
/// must guard against double-appending a sample the stride already recorded on the final
/// tick — see the call site in [`integrate`].
#[must_use]
fn terminal_result(
    mut samples: Vec<BallisticSample>,
    position: FixedPoint,
    tick: u32,
    cause: ImpactCause,
) -> BallisticResult {
    samples.push(BallisticSample { tick, position });
    BallisticResult {
        samples,
        impact: BallisticImpact {
            position,
            tick,
            cause,
        },
    }
}

/// Integrates a projectile's trajectory from launch to termination.
///
/// Terminates on, in the order checked each tick: leaving the playable bounds
/// ([`ImpactCause::OutOfBounds`]), solid terrain — either detonating
/// ([`ImpactCause::Terrain`]) or reflecting, see below — character contact
/// ([`ImpactCause::Character`], squared-distance comparison against `collision_radius`,
/// never a square root), or exhausting `attack.max_ticks`
/// ([`ImpactCause::Expired`]).
///
/// `characters` is checked in the order given, so if two collision radii somehow overlap
/// at the same instant the earlier entry wins; this is deterministic because the caller's
/// ordering is deterministic. Whether the acting player's own character is included is a
/// caller decision — this function does not special-case self-hits — so a caller that
/// wants a launcher immune to their own projectile must omit it from `characters`.
///
/// `attack.bounces` reflects the projectile off terrain that many times (see
/// [`reflection_axes`]) before a further terrain hit finally detonates it; character
/// contact and leaving the bounds are never bounced, matching the intent that a grenade
/// bounces off walls and floors but still explodes on a direct hit.
///
/// The returned samples always include tick 0 (the origin) and the terminal tick,
/// regardless of [`SAMPLE_STRIDE`]; see the module's stride constant for why every tick
/// in between is not recorded.
///
/// # Errors
///
/// Returns [`SimError::OutOfRange`] if `input.power_basis_points` is outside `1..=10_000`
/// (matching the oracle's `powerBasisPoints` validation). Returns [`SimError::Overflow`]
/// if launch-time arithmetic (map size, launch speed) would exceed `i32`/`i64` range.
/// A shot that flies so far that a later position or velocity step cannot be represented
/// leaves the playable area ([`ImpactCause::OutOfBounds`]) instead of failing the command:
/// aiming at the sky or the screen edge is a miss, not a math error.
pub fn integrate(
    input: &BallisticInput,
    attack: &ProjectileAttack,
    terrain_mask: &TerrainMask,
    characters: &[(String, FixedPoint, i32)],
) -> SimResult<BallisticResult> {
    if input.power_basis_points < 1 || input.power_basis_points > fixed::BASIS_POINTS {
        return Err(SimError::OutOfRange {
            field: "power_basis_points",
        });
    }

    let max_x = bounds_max_x(terrain_mask)?;
    let max_y = bounds_max_y(terrain_mask)?;

    let (mut velocity_x, mut velocity_y) = launch_velocity(attack, input)?;
    // roundDivide(windPerTick * windScaleBasisPoints, 10_000) in the oracle. A
    // wind_scale_basis_points of 0 (wind-immune weapons) always yields exactly 0 here,
    // regardless of how extreme wind_per_tick is — `scale` multiplies by the numerator
    // before dividing, so a zero numerator zeroes the product outright.
    let wind_acceleration =
        fixed::apply_basis_points(input.wind_per_tick, attack.wind_scale_basis_points).ok_or(
            SimError::Overflow {
                context: "ballistics::wind_acceleration",
            },
        )?;

    let mut position = input.origin;
    let mut samples = vec![BallisticSample { tick: 0, position }];
    let mut bounces_remaining = attack.bounces;

    for tick in 1..=u32::from(attack.max_ticks) {
        // Semi-implicit Euler: velocity is advanced by acceleration BEFORE position is
        // advanced by velocity. This ordering is load-bearing for parity — see module docs.
        let Some(next_vx) = velocity_x.checked_add(wind_acceleration) else {
            return Ok(terminal_result(
                samples,
                position,
                tick,
                ImpactCause::OutOfBounds,
            ));
        };
        let Some(next_vy) = velocity_y.checked_add(attack.gravity_per_tick) else {
            return Ok(terminal_result(
                samples,
                position,
                tick,
                ImpactCause::OutOfBounds,
            ));
        };
        velocity_x = next_vx;
        velocity_y = next_vy;

        let Some(next_x) = position.x.checked_add(velocity_x) else {
            return Ok(terminal_result(
                samples,
                position,
                tick,
                ImpactCause::OutOfBounds,
            ));
        };
        let Some(next_y) = position.y.checked_add(velocity_y) else {
            return Ok(terminal_result(
                samples,
                position,
                tick,
                ImpactCause::OutOfBounds,
            ));
        };
        let candidate = FixedPoint::new(next_x, next_y);

        // Bounds: matches the oracle exactly — both edges on X, only the lower edge
        // absent on Y (a shot may fly arbitrarily far above the map before falling back).
        if candidate.x < 0 || candidate.x >= max_x || candidate.y >= max_y {
            return Ok(terminal_result(
                samples,
                candidate,
                tick,
                ImpactCause::OutOfBounds,
            ));
        }

        if terrain::is_solid_at(terrain_mask, candidate) {
            if bounces_remaining > 0 {
                // Guarded by the `> 0` check just above; saturating rather than plain `-`
                // per the workspace's checked/saturating arithmetic rule regardless.
                bounces_remaining = bounces_remaining.saturating_sub(1);
                let (reflect_x, reflect_y) = reflection_axes(position, candidate, terrain_mask);
                if reflect_x {
                    let Some(reflected) = velocity_x.checked_neg() else {
                        return Ok(terminal_result(
                            samples,
                            position,
                            tick,
                            ImpactCause::OutOfBounds,
                        ));
                    };
                    velocity_x = reflected;
                }
                if reflect_y {
                    let Some(reflected) = velocity_y.checked_neg() else {
                        return Ok(terminal_result(
                            samples,
                            position,
                            tick,
                            ImpactCause::OutOfBounds,
                        ));
                    };
                    velocity_y = reflected;
                }
                // Revert to the last valid (non-solid) position rather than the
                // intruding one. This is a coarse "undo the offending tick, redo it with
                // reflected velocity next tick" model: the grid mask has no fractional
                // penetration depth to reflect out of exactly, and reverting guarantees
                // the projectile never gets stuck sampling from inside solid terrain.
                // Record this tick so playback shows the ricochet instead of interpolating
                // through the wall (stride samples would skip this contact).
                samples.push(BallisticSample { tick, position });
                continue;
            }

            return Ok(terminal_result(
                samples,
                candidate,
                tick,
                ImpactCause::Terrain,
            ));
        }

        if characters.iter().any(|(_id, char_position, radius)| {
            fixed::within_radius(candidate, *char_position, *radius)
        }) {
            return Ok(terminal_result(
                samples,
                candidate,
                tick,
                ImpactCause::Character,
            ));
        }

        position = candidate;
        if tick_on_stride(tick) {
            samples.push(BallisticSample { tick, position });
        }
    }

    // max_ticks exhausted without termination. The final tick's position may already be
    // the last recorded sample (when max_ticks lands on the stride); guard against
    // recording it twice rather than reusing `terminal_result`, which always appends.
    let final_tick = u32::from(attack.max_ticks);
    if samples.last().is_none_or(|last| last.tick != final_tick) {
        samples.push(BallisticSample {
            tick: final_tick,
            position,
        });
    }

    Ok(BallisticResult {
        samples,
        impact: BallisticImpact {
            position,
            tick: final_tick,
            cause: ImpactCause::Expired,
        },
    })
}

#[cfg(test)]
// Test-only fixtures deliberately fail loudly (`panic!`, via the `must_succeed` /
// `must_exist` helpers below) on setup values that are wrong by construction — a failing
// fixture is a broken test, not untrusted runtime input, and this module never uses
// `.unwrap()`/`.expect()` even here. Matches the convention in `character.rs`'s test module.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::types::{Material, TerrainProfile};

    /// Unwraps a [`SimResult`], panicking with the error on `Err`.
    ///
    /// `.unwrap()`/`.expect()` are denied crate-wide, including in tests; this is the
    /// test-only equivalent, matching the "fail loudly" fixture convention.
    fn must_succeed<T>(result: SimResult<T>) -> T {
        match result {
            Ok(value) => value,
            Err(error) => panic!("expected Ok, got Err({error:?})"),
        }
    }

    /// Unwraps an [`Option`], panicking with `message` on `None`.
    fn must_exist<T>(value: Option<T>, message: &str) -> T {
        match value {
            Some(value) => value,
            None => panic!("{message}"),
        }
    }

    /// A generous, empty playable area with no solid cells and no characters, so tests
    /// only exercise the specific behavior under test.
    fn open_terrain(width_cells: u32, height_cells: u32) -> TerrainMask {
        must_succeed(terrain::create_mask(
            width_cells,
            height_cells,
            Material::Empty,
        ))
    }

    fn no_gravity_no_wind_attack(
        speed_per_tick: i32,
        max_ticks: u16,
        bounces: u8,
    ) -> ProjectileAttack {
        ProjectileAttack {
            speed_per_tick,
            gravity_per_tick: 0,
            wind_scale_basis_points: 0,
            max_ticks,
            bounces,
            terrain: TerrainProfile::None,
        }
    }

    // -----------------------------------------------------------------
    // Fixed-point trig
    // -----------------------------------------------------------------

    #[test]
    fn sine_scaled_matches_known_angles() {
        assert_eq!(sine_scaled(0).ok(), Some(0));
        assert_eq!(sine_scaled(90_000).ok(), Some(SIN_SCALE));
        assert_eq!(sine_scaled(180_000).ok(), Some(0));
        assert_eq!(sine_scaled(270_000).ok(), Some(-SIN_SCALE));
        assert_eq!(sine_scaled(360_000).ok(), Some(0));
    }

    #[test]
    fn sine_scaled_wraps_negative_and_large_angles() {
        // -90 degrees is the same direction as 270 degrees.
        assert_eq!(sine_scaled(-90_000).ok(), Some(-SIN_SCALE));
        // 720 + 90 degrees is two full turns past 90 degrees.
        assert_eq!(sine_scaled(810_000).ok(), Some(SIN_SCALE));
    }

    #[test]
    fn sine_scaled_interpolates_sub_degree_angles() {
        // Half a degree: exactly halfway between table[0] (0) and table[1] (1144).
        // 1144 * 500 / 1000 = 572 exactly, no rounding ambiguity.
        assert_eq!(sine_scaled(500).ok(), Some(572));
    }

    #[test]
    fn cosine_scaled_matches_known_angles() {
        assert_eq!(cosine_scaled(0).ok(), Some(SIN_SCALE));
        assert_eq!(cosine_scaled(90_000).ok(), Some(0));
        assert_eq!(cosine_scaled(180_000).ok(), Some(-SIN_SCALE));
        assert_eq!(cosine_scaled(270_000).ok(), Some(0));
    }

    // -----------------------------------------------------------------
    // Bounce reflection axis derivation
    // -----------------------------------------------------------------

    #[test]
    fn reflection_axes_detects_vertical_wall() {
        let mut mask = open_terrain(10, 10);
        // A vertical wall: solid column at cell x=5.
        let _ = terrain::set_material(&mut mask, 5, 3, Material::Wood);
        if let (Some(prev), Some(next)) =
            (FixedPoint::from_cells(4, 3), FixedPoint::from_cells(5, 3))
        {
            let (reflect_x, reflect_y) = reflection_axes(prev, next, &mask);
            assert!(
                reflect_x,
                "purely horizontal move into a wall must reflect X"
            );
            assert!(!reflect_y, "no vertical component should reflect");
        } else {
            panic!("from_cells failed");
        }
    }

    #[test]
    fn reflection_axes_detects_horizontal_floor() {
        let mut mask = open_terrain(10, 10);
        let _ = terrain::set_material(&mut mask, 3, 5, Material::Soil);
        if let (Some(prev), Some(next)) =
            (FixedPoint::from_cells(3, 4), FixedPoint::from_cells(3, 5))
        {
            let (reflect_x, reflect_y) = reflection_axes(prev, next, &mask);
            assert!(!reflect_x, "no horizontal component should reflect");
            assert!(
                reflect_y,
                "purely vertical move into a floor must reflect Y"
            );
        } else {
            panic!("from_cells failed");
        }
    }

    #[test]
    fn reflection_axes_falls_back_to_both_on_corner_clip() {
        let mut mask = open_terrain(10, 10);
        // Solid only at the diagonal target cell; both axis-isolated probes land on
        // empty cells, so neither individually detects the collision.
        let _ = terrain::set_material(&mut mask, 5, 5, Material::Wood);
        if let (Some(prev), Some(next)) =
            (FixedPoint::from_cells(4, 4), FixedPoint::from_cells(5, 5))
        {
            let (reflect_x, reflect_y) = reflection_axes(prev, next, &mask);
            assert!(
                reflect_x && reflect_y,
                "corner-cut fallback must reflect both axes"
            );
        } else {
            panic!("from_cells failed");
        }
    }

    // -----------------------------------------------------------------
    // integrate: known trajectory (hand-verified semi-implicit Euler)
    // -----------------------------------------------------------------

    #[test]
    fn integrate_known_trajectory_matches_hand_computed_positions() {
        // angle 0, full power, speed 1000, gravity 100, no wind: velocity_x is constant
        // at 1000/tick; velocity_y accumulates 100/tick, so position_y after n ticks is
        // the triangular number 100 * n * (n + 1) / 2.
        let attack = ProjectileAttack {
            speed_per_tick: 1000,
            gravity_per_tick: 100,
            wind_scale_basis_points: 0,
            max_ticks: 5,
            bounces: 0,
            terrain: TerrainProfile::None,
        };
        let input = BallisticInput {
            origin: FixedPoint::ZERO,
            angle_millidegrees: 0,
            power_basis_points: 10_000,
            wind_per_tick: 0,
        };
        let mask = open_terrain(1000, 1000);
        let result = must_succeed(integrate(&input, &attack, &mask, &[]));

        assert_eq!(result.impact.cause, ImpactCause::Expired);
        assert_eq!(result.impact.tick, 5);
        assert_eq!(result.impact.position, FixedPoint::new(5000, 1500));

        // Stride is 4: expect tick 0 (origin), tick 4 (stride), tick 5 (forced terminal).
        assert_eq!(result.samples.len(), 3);
        assert_eq!(
            result.samples.first(),
            Some(&BallisticSample {
                tick: 0,
                position: FixedPoint::ZERO
            })
        );
        assert_eq!(
            result.samples.get(1),
            Some(&BallisticSample {
                tick: 4,
                position: FixedPoint::new(4000, 1000)
            })
        );
        assert_eq!(
            result.samples.get(2),
            Some(&BallisticSample {
                tick: 5,
                position: FixedPoint::new(5000, 1500)
            })
        );
    }

    #[test]
    fn integrate_straight_up_shot_never_drifts_horizontally() {
        // angle 90 degrees (straight up) with no wind: velocity_x must be exactly zero
        // for the entire flight, so every sample's X equals the origin's X.
        let attack = ProjectileAttack {
            speed_per_tick: 2000,
            gravity_per_tick: 50,
            wind_scale_basis_points: 0,
            max_ticks: 200,
            bounces: 0,
            terrain: TerrainProfile::None,
        };
        let origin = FixedPoint::new(50_000, 100_000);
        let input = BallisticInput {
            origin,
            angle_millidegrees: 90_000,
            power_basis_points: 10_000,
            wind_per_tick: 0,
        };
        let mask = open_terrain(10_000, 10_000);
        let result = must_succeed(integrate(&input, &attack, &mask, &[]));

        for sample in &result.samples {
            assert_eq!(
                sample.position.x, origin.x,
                "tick {} drifted horizontally",
                sample.tick
            );
        }
        assert_eq!(result.impact.position.x, origin.x);
    }

    #[test]
    fn integrate_wind_immune_weapon_ignores_extreme_wind() {
        let attack = no_gravity_no_wind_attack(500, 10, 0); // wind_scale_basis_points: 0
        let input = BallisticInput {
            origin: FixedPoint::ZERO,
            angle_millidegrees: 0,
            power_basis_points: 10_000,
            wind_per_tick: 1_000_000, // extreme; must be fully ignored
        };
        let mask = open_terrain(1000, 1000);
        let result = must_succeed(integrate(&input, &attack, &mask, &[]));

        // With zero gravity and a zeroed wind contribution, horizontal velocity is
        // constant at the launch speed for the entire flight.
        assert_eq!(result.impact.cause, ImpactCause::Expired);
        assert_eq!(result.impact.position, FixedPoint::new(5000, 0));
    }

    #[test]
    fn integrate_terminates_on_solid_terrain() {
        let attack = no_gravity_no_wind_attack(200, 200, 0);
        let mut mask = open_terrain(15, 5);
        // Solid column at cell x=7 blocking the flight path along y cell 2.
        for y in 0..5 {
            let _ = terrain::set_material(&mut mask, 7, y, Material::Wood);
        }
        let origin = must_exist(
            FixedPoint::from_cells(2, 2),
            "from_cells must succeed for small cells",
        );
        let input = BallisticInput {
            origin,
            angle_millidegrees: 0,
            power_basis_points: 10_000,
            wind_per_tick: 0,
        };
        let result = must_succeed(integrate(&input, &attack, &mask, &[]));

        assert_eq!(result.impact.cause, ImpactCause::Terrain);
        assert_eq!(result.impact.tick, 26);
        // origin.x (2048) + 26 ticks * velocity_x (200/tick) = 7248, which lands in
        // solid cell 7 ([7168, 8192)).
        assert_eq!(result.impact.position, FixedPoint::new(7248, 2048));
        // The impact sample is always the last one, regardless of stride.
        assert_eq!(
            result.samples.last(),
            Some(&BallisticSample {
                tick: 26,
                position: FixedPoint::new(7248, 2048)
            })
        );
    }

    #[test]
    fn integrate_bounce_reflects_instead_of_terminating_then_exits_the_far_side() {
        // Identical setup to `integrate_terminates_on_solid_terrain`, except bounces: 1.
        // Without a bounce this hits the wall at tick 26 with cause Terrain (proven by
        // the sibling test above); with one bounce available it must instead reflect off
        // the wall and travel back out through the left edge of the map.
        let attack = no_gravity_no_wind_attack(200, 200, 1);
        let mut mask = open_terrain(15, 5);
        for y in 0..5 {
            let _ = terrain::set_material(&mut mask, 7, y, Material::Wood);
        }
        let origin = must_exist(
            FixedPoint::from_cells(2, 2),
            "from_cells must succeed for small cells",
        );
        let input = BallisticInput {
            origin,
            angle_millidegrees: 0,
            power_basis_points: 10_000,
            wind_per_tick: 0,
        };
        let result = must_succeed(integrate(&input, &attack, &mask, &[]));

        assert_eq!(result.impact.cause, ImpactCause::OutOfBounds);
        assert!(
            result.impact.tick > 26,
            "must terminate strictly after the bounce tick"
        );
        assert!(
            result.impact.position.x < 0,
            "must have exited through the left edge after reflecting"
        );
        assert!(
            result.samples.iter().any(|sample| sample.tick == 26),
            "the ricochet tick must be sampled so the client can draw the bounce"
        );
    }

    #[test]
    fn integrate_terminates_on_character_contact() {
        let attack = no_gravity_no_wind_attack(200, 200, 0);
        let mask = open_terrain(20, 5);
        let origin = must_exist(
            FixedPoint::from_cells(2, 2),
            "from_cells must succeed for small cells",
        );
        let input = BallisticInput {
            origin,
            angle_millidegrees: 0,
            power_basis_points: 10_000,
            wind_per_tick: 0,
        };
        // Exact position of tick 10 (2048 + 10*200 = 4048) with a tight radius so no
        // neighboring tick (spaced 200 apart) can also land inside it.
        let characters = vec![("target".to_string(), FixedPoint::new(4048, 2048), 50)];
        let result = must_succeed(integrate(&input, &attack, &mask, &characters));

        assert_eq!(result.impact.cause, ImpactCause::Character);
        assert_eq!(result.impact.tick, 10);
        assert_eq!(result.impact.position, FixedPoint::new(4048, 2048));
    }

    #[test]
    fn integrate_expires_at_max_ticks_with_no_termination() {
        let attack = no_gravity_no_wind_attack(1, 20, 0); // tiny speed: barely moves in 20 ticks
        let mask = open_terrain(1_000, 1_000);
        let input = BallisticInput {
            origin: FixedPoint::ZERO,
            angle_millidegrees: 0,
            power_basis_points: 10_000,
            wind_per_tick: 0,
        };
        let result = must_succeed(integrate(&input, &attack, &mask, &[]));

        assert_eq!(result.impact.cause, ImpactCause::Expired);
        assert_eq!(result.impact.tick, 20);
    }

    #[test]
    fn integrate_zero_max_ticks_expires_immediately_at_origin() {
        let attack = no_gravity_no_wind_attack(500, 0, 0);
        let mask = open_terrain(1000, 1000);
        let origin = FixedPoint::new(1234, 5678);
        let input = BallisticInput {
            origin,
            angle_millidegrees: 0,
            power_basis_points: 10_000,
            wind_per_tick: 0,
        };
        let result = must_succeed(integrate(&input, &attack, &mask, &[]));

        assert_eq!(result.impact.cause, ImpactCause::Expired);
        assert_eq!(result.impact.tick, 0);
        assert_eq!(result.impact.position, origin);
        assert_eq!(result.samples.len(), 1);
    }

    #[test]
    fn integrate_rejects_power_out_of_range() {
        let attack = no_gravity_no_wind_attack(500, 10, 0);
        let mask = open_terrain(100, 100);
        let mut input = BallisticInput {
            origin: FixedPoint::ZERO,
            angle_millidegrees: 0,
            power_basis_points: 0,
            wind_per_tick: 0,
        };
        assert_eq!(
            integrate(&input, &attack, &mask, &[]),
            Err(SimError::OutOfRange {
                field: "power_basis_points"
            })
        );

        input.power_basis_points = 10_001;
        assert_eq!(
            integrate(&input, &attack, &mask, &[]),
            Err(SimError::OutOfRange {
                field: "power_basis_points"
            })
        );
    }

    #[test]
    fn integrate_treats_a_skyward_overflow_as_leaving_the_map() {
        // No ceiling on Y (oracle parity): a vertical shot may climb arbitrarily far.
        // If that climb can no longer be represented, it is a miss, not Overflow — aiming
        // at the top of the screen must not fail the command as a math error.
        let attack = no_gravity_no_wind_attack(1_000_000, 3_000, 0);
        let mask = open_terrain(8, 8);
        let input = BallisticInput {
            origin: FixedPoint::new(1_024, 1_024),
            angle_millidegrees: 90_000,
            power_basis_points: 10_000,
            wind_per_tick: 0,
        };
        let result = must_succeed(integrate(&input, &attack, &mask, &[]));
        assert_eq!(result.impact.cause, ImpactCause::OutOfBounds);
        assert!(
            result.impact.tick > 0,
            "the projectile must leave after at least one integration step"
        );
    }

    // -----------------------------------------------------------------
    // Determinism
    // -----------------------------------------------------------------

    #[test]
    fn integrate_is_deterministic_across_repeated_calls() {
        let attack = ProjectileAttack {
            speed_per_tick: 777,
            gravity_per_tick: 33,
            wind_scale_basis_points: 6_000,
            max_ticks: 150,
            bounces: 1,
            terrain: TerrainProfile::None,
        };
        let mut mask = open_terrain(50, 50);
        for y in 10..40 {
            let _ = terrain::set_material(&mut mask, 40, y, Material::Wood);
        }
        let input = BallisticInput {
            origin: must_exist(FixedPoint::from_cells(5, 5), "from_cells must succeed"),
            angle_millidegrees: 12_345,
            power_basis_points: 8_500,
            wind_per_tick: 40,
        };
        let characters = vec![
            (
                "a".to_string(),
                must_exist(FixedPoint::from_cells(20, 20), "from_cells must succeed"),
                200,
            ),
            (
                "b".to_string(),
                must_exist(FixedPoint::from_cells(30, 8), "from_cells must succeed"),
                150,
            ),
        ];

        let first = must_succeed(integrate(&input, &attack, &mask, &characters));
        let second = must_succeed(integrate(&input, &attack, &mask, &characters));

        assert_eq!(
            first, second,
            "identical input must produce byte-identical output"
        );
    }
}
