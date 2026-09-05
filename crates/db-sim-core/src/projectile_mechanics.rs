//! Dormant projectile-mechanics laboratory.
//!
//! This module contains deterministic, bounded building blocks for projectile and terrain
//! interactions that are not assigned to the current roster. It deliberately does not mutate
//! [`crate::types::SimulationState`], participate in command resolution, appear in canonical
//! encoding, or cross the FFI boundary. A future character may opt into a reviewed subset only
//! after authority integration, a simulation-version decision, golden vectors, and client
//! playback have landed.
//!
//! The vocabulary was produced by a clean-room behavioral audit documented in
//! `docs/OPENBOUND_PROJECTILE_MECHANICS_PLAN.md`. The implementation is original to Dungeon
//! Barrage and uses its integer-only fixed-point conventions and explicit work limits.

use crate::error::{SimError, SimResult};
use crate::fixed::{self, FixedPoint, POSITION_SCALE};
use crate::terrain;
use crate::types::{MaterialMask, TerrainMask, TerrainOperation, TerrainShape};

/// Maximum dependent payloads one primitive may schedule.
pub const MAX_CHILD_PROJECTILES: u8 = 32;

/// Maximum cells a dormant beam query may inspect.
pub const MAX_BEAM_STEPS: u16 = 2_048;

const CIRCLE_SCALE: i32 = 1_024;

// A fixed 16-direction unit circle, clockwise in screen coordinates (+Y is down).
// Values are rounded once at authoring time and never depend on a platform math library.
const UNIT_CIRCLE_16: [(i32, i32); 16] = [
    (1_024, 0),
    (946, 392),
    (724, 724),
    (392, 946),
    (0, 1_024),
    (-392, 946),
    (-724, 724),
    (-946, 392),
    (-1_024, 0),
    (-946, -392),
    (-724, -724),
    (-392, -946),
    (0, -1_024),
    (392, -946),
    (724, -724),
    (946, -392),
];

/// Reusable behaviors held in reserve for future character kits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProjectileMechanic {
    /// Ordinary fixed-step gravity/wind flight.
    BallisticArc,
    /// Collision is sampled between visible frames to prevent tunnelling.
    SubstepCollision,
    /// Flight terminates at bounds or a hard lifetime.
    BoundsExpiry,
    /// Damage falls with distance from an impact.
    RadialDamage,
    /// An impact removes a circular area of permitted terrain materials.
    CircularTerrainBlast,
    /// A payload starts after a scheduled delay.
    SpawnDelay,
    /// A payload pauses before resuming flight.
    FreezeDelay,
    /// A parent action completes only after all dependent payloads resolve.
    DependencyCompletion,
    /// One contact produces two separately configured blast phases.
    DoubleBlast,
    /// Flight time arms or upgrades a payload.
    TimedArming,
    /// Child shots use centered angular offsets.
    AngleSpreadVolley,
    /// Child shots use ordered power offsets.
    PowerSpreadVolley,
    /// Child shots launch on deterministic stagger ticks.
    StaggeredVolley,
    /// A marker impact calls projectiles inward from a fan of origins.
    ImpactConvergenceFan,
    /// A remote or global source calls a projectile toward an impact.
    SatelliteConvergence,
    /// Independently colliding payloads orbit a ballistic carrier.
    OrbitingPayloadCarrier,
    /// Orbit radius contracts after a configured delay.
    ConvergingHelix,
    /// A carrier becomes multiple weaker child projectiles during flight.
    TimedSplit,
    /// One impact anchors a timed sequence around a ring.
    SequencedImpactRing,
    /// A bounded ray resolves at its final solid contact.
    LastSurfaceBeam,
    /// One impact emits beams in several directions.
    RadialBeamCascade,
    /// Nearby targets each receive a follow-up beam.
    TargetBeamCascade,
    /// Every eligible impact receives a remote follow-up.
    GlobalImpactFollowup,
    /// Characters do not intercept a terrain-placement projectile.
    TerrainOnlyContact,
    /// Terrain contact deploys one or more dormant proximity mines.
    DeployProximityMine,
    /// A mine chooses and steps toward the nearest stable target.
    TargetSeekingMine,
    /// Terrain contact deploys a long-running forward mine.
    DeployRoamingMine,
    /// A roaming mine reverses at an obstacle.
    ObstacleReflectingMine,
    /// Terrain contact relocates the actor to the last free sample.
    TeleportLanding,
    /// Repeated hits lower defense to a configured floor.
    ArmorShred,
    /// An impact adds current shield to damage and then removes that shield.
    ShieldPurge,
    /// An environment zone increases payload damage once.
    DamageAmplifier,
    /// An environment zone decreases payload damage once.
    DamageDampener,
    /// An environment zone reflects horizontal flight.
    HorizontalMirror,
    /// An environment zone temporarily replaces the projectile path.
    TornadoRedirect,
    /// An environment zone attaches a beam to the next impact.
    ElectricImpactFollowup,
    /// The first eligible contact activates a linked environment effect.
    RandomEnvironmentTrigger,
    /// Overlapping same-type environment zones combine deterministically.
    MergeableEnvironmentZone,
    /// Unsupported terrain settles after destructive operations.
    TerrainCollapse,
}

impl ProjectileMechanic {
    /// Exhaustive catalog used by design tooling and coverage tests.
    pub const ALL: [Self; 39] = [
        Self::BallisticArc,
        Self::SubstepCollision,
        Self::BoundsExpiry,
        Self::RadialDamage,
        Self::CircularTerrainBlast,
        Self::SpawnDelay,
        Self::FreezeDelay,
        Self::DependencyCompletion,
        Self::DoubleBlast,
        Self::TimedArming,
        Self::AngleSpreadVolley,
        Self::PowerSpreadVolley,
        Self::StaggeredVolley,
        Self::ImpactConvergenceFan,
        Self::SatelliteConvergence,
        Self::OrbitingPayloadCarrier,
        Self::ConvergingHelix,
        Self::TimedSplit,
        Self::SequencedImpactRing,
        Self::LastSurfaceBeam,
        Self::RadialBeamCascade,
        Self::TargetBeamCascade,
        Self::GlobalImpactFollowup,
        Self::TerrainOnlyContact,
        Self::DeployProximityMine,
        Self::TargetSeekingMine,
        Self::DeployRoamingMine,
        Self::ObstacleReflectingMine,
        Self::TeleportLanding,
        Self::ArmorShred,
        Self::ShieldPurge,
        Self::DamageAmplifier,
        Self::DamageDampener,
        Self::HorizontalMirror,
        Self::TornadoRedirect,
        Self::ElectricImpactFollowup,
        Self::RandomEnvironmentTrigger,
        Self::MergeableEnvironmentZone,
        Self::TerrainCollapse,
    ];

    /// Stable design identifier. These names are not wire protocol values.
    #[must_use]
    pub const fn design_name(self) -> &'static str {
        match self {
            Self::BallisticArc => "ballistic-arc",
            Self::SubstepCollision => "substep-collision",
            Self::BoundsExpiry => "bounds-expiry",
            Self::RadialDamage => "radial-damage",
            Self::CircularTerrainBlast => "circular-terrain-blast",
            Self::SpawnDelay => "spawn-delay",
            Self::FreezeDelay => "freeze-delay",
            Self::DependencyCompletion => "dependency-completion",
            Self::DoubleBlast => "double-blast",
            Self::TimedArming => "timed-arming",
            Self::AngleSpreadVolley => "angle-spread-volley",
            Self::PowerSpreadVolley => "power-spread-volley",
            Self::StaggeredVolley => "staggered-volley",
            Self::ImpactConvergenceFan => "impact-convergence-fan",
            Self::SatelliteConvergence => "satellite-convergence",
            Self::OrbitingPayloadCarrier => "orbiting-payload-carrier",
            Self::ConvergingHelix => "converging-helix",
            Self::TimedSplit => "timed-split",
            Self::SequencedImpactRing => "sequenced-impact-ring",
            Self::LastSurfaceBeam => "last-surface-beam",
            Self::RadialBeamCascade => "radial-beam-cascade",
            Self::TargetBeamCascade => "target-beam-cascade",
            Self::GlobalImpactFollowup => "global-impact-followup",
            Self::TerrainOnlyContact => "terrain-only-contact",
            Self::DeployProximityMine => "deploy-proximity-mine",
            Self::TargetSeekingMine => "target-seeking-mine",
            Self::DeployRoamingMine => "deploy-roaming-mine",
            Self::ObstacleReflectingMine => "obstacle-reflecting-mine",
            Self::TeleportLanding => "teleport-landing",
            Self::ArmorShred => "armor-shred",
            Self::ShieldPurge => "shield-purge",
            Self::DamageAmplifier => "damage-amplifier",
            Self::DamageDampener => "damage-dampener",
            Self::HorizontalMirror => "horizontal-mirror",
            Self::TornadoRedirect => "tornado-redirect",
            Self::ElectricImpactFollowup => "electric-impact-followup",
            Self::RandomEnvironmentTrigger => "random-environment-trigger",
            Self::MergeableEnvironmentZone => "mergeable-environment-zone",
            Self::TerrainCollapse => "terrain-collapse",
        }
    }
}

/// Determines what can stop a projectile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactPolicy {
    /// Terrain and character bodies both stop flight.
    TerrainOrCharacter,
    /// Only terrain stops flight; character bodies are ignored for placement.
    TerrainOnly,
    /// A bounded beam remembers its last solid contact rather than stopping at the first.
    LastSurfaceAlongBeam,
}

/// Reports whether an observed contact stops flight under `policy`.
#[must_use]
pub const fn contact_stops_flight(
    policy: ContactPolicy,
    touched_terrain: bool,
    touched_character: bool,
) -> bool {
    match policy {
        ContactPolicy::TerrainOrCharacter => touched_terrain || touched_character,
        ContactPolicy::TerrainOnly => touched_terrain,
        ContactPolicy::LastSurfaceAlongBeam => false,
    }
}

/// Damage and crater carried by one impact phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Payload {
    /// Maximum damage at the impact center.
    pub damage: u16,
    /// Circular terrain radius in cells.
    pub crater_radius_cells: u16,
}

/// Computes deterministic squared-distance radial falloff.
///
/// Damage is full at the center, reaches zero at the radius, and never divides by target distance.
/// The squared falloff is intentionally a Dungeon Barrage rule, not a copy of another engine's
/// singular inverse-distance formula.
#[must_use]
pub fn radial_damage(base_damage: u16, impact: FixedPoint, target: FixedPoint, radius: i32) -> u16 {
    if radius < 0 {
        return 0;
    }
    let distance_squared = squared_distance(impact, target);
    let radius = i64::from(radius);
    let radius_squared = radius.saturating_mul(radius);
    if distance_squared > radius_squared {
        return 0;
    }
    if radius_squared == 0 {
        return if distance_squared == 0 {
            base_damage
        } else {
            0
        };
    }
    let remaining = radius_squared.saturating_sub(distance_squared);
    let numerator = i64::from(base_damage).saturating_mul(remaining);
    fixed::round_divide(numerator, radius_squared)
        .and_then(|damage| u16::try_from(damage).ok())
        .unwrap_or(0)
}

/// One payload scheduled relative to its parent action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledProjectile {
    /// Stable zero-based child ordinal.
    pub ordinal: u8,
    /// Launch angle in millidegrees.
    pub angle_millidegrees: i32,
    /// Launch power in fixed-point units.
    pub power: i32,
    /// Delay after the parent action, in authority ticks.
    pub delay_ticks: u16,
}

/// Centered multi-projectile scheduling parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolleySpec {
    /// Number of child projectiles.
    pub count: u8,
    /// Difference between adjacent launch angles, in millidegrees.
    pub angle_step_millidegrees: i32,
    /// Power added for each successive child.
    pub power_step: i32,
    /// Delay added for each successive child.
    pub stagger_ticks: u16,
}

/// Produces a centered, ordered, bounded volley schedule.
///
/// # Errors
///
/// Rejects zero children, more than [`MAX_CHILD_PROJECTILES`], and arithmetic overflow.
pub fn schedule_volley(
    spec: VolleySpec,
    base_angle_millidegrees: i32,
    base_power: i32,
) -> SimResult<Vec<ScheduledProjectile>> {
    if spec.count == 0 || spec.count > MAX_CHILD_PROJECTILES {
        return Err(SimError::OutOfRange {
            field: "volley child count",
        });
    }

    let mut scheduled = Vec::with_capacity(spec.count as usize);
    let last_ordinal = i64::from(spec.count.saturating_sub(1));
    for ordinal in 0..spec.count {
        let centered_twice = i64::from(ordinal)
            .saturating_mul(2)
            .saturating_sub(last_ordinal);
        let angle_numerator = centered_twice
            .checked_mul(i64::from(spec.angle_step_millidegrees))
            .ok_or(SimError::Overflow {
                context: "projectile_mechanics::volley_angle",
            })?;
        let angle_offset = fixed::round_divide(angle_numerator, 2).ok_or(SimError::Overflow {
            context: "projectile_mechanics::volley_angle_round",
        })?;
        let angle_offset = i32::try_from(angle_offset).map_err(|_| SimError::Overflow {
            context: "projectile_mechanics::volley_angle_i32",
        })?;
        let angle_millidegrees =
            base_angle_millidegrees
                .checked_add(angle_offset)
                .ok_or(SimError::Overflow {
                    context: "projectile_mechanics::volley_angle_add",
                })?;
        let power_offset =
            i32::from(ordinal)
                .checked_mul(spec.power_step)
                .ok_or(SimError::Overflow {
                    context: "projectile_mechanics::volley_power",
                })?;
        let power = base_power
            .checked_add(power_offset)
            .ok_or(SimError::Overflow {
                context: "projectile_mechanics::volley_power_add",
            })?;
        let delay_ticks =
            u16::from(ordinal)
                .checked_mul(spec.stagger_ticks)
                .ok_or(SimError::Overflow {
                    context: "projectile_mechanics::volley_delay",
                })?;
        scheduled.push(ScheduledProjectile {
            ordinal,
            angle_millidegrees,
            power,
            delay_ticks,
        });
    }

    Ok(scheduled)
}

/// Tracks completion of a bounded group of dependent payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DependencyGroup {
    expected: u8,
    resolved: u8,
}

impl DependencyGroup {
    /// Creates a group with at least one and at most [`MAX_CHILD_PROJECTILES`] member.
    ///
    /// # Errors
    ///
    /// Rejects an out-of-range member count.
    pub fn new(expected: u8) -> SimResult<Self> {
        if expected == 0 || expected > MAX_CHILD_PROJECTILES {
            return Err(SimError::OutOfRange {
                field: "dependency group size",
            });
        }
        Ok(Self {
            expected,
            resolved: 0,
        })
    }

    /// Records one payload resolution and returns whether the group is now complete.
    ///
    /// # Errors
    ///
    /// Rejects a resolution after the group has already completed.
    pub fn resolve_one(&mut self) -> SimResult<bool> {
        if self.resolved >= self.expected {
            return Err(SimError::OutOfRange {
                field: "dependency group resolution",
            });
        }
        self.resolved = self.resolved.saturating_add(1);
        Ok(self.resolved == self.expected)
    }

    /// Number of payloads still unresolved.
    #[must_use]
    pub const fn remaining(self) -> u8 {
        self.expected.saturating_sub(self.resolved)
    }
}

/// Timed mutation applied to a projectile during flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedPayloadUpgrade {
    /// Tick on which the payload becomes armed.
    pub arm_after_ticks: u16,
    /// Number of ticks motion remains paused while arming.
    pub pause_ticks: u16,
    /// Damage added once armed.
    pub damage_bonus: u16,
    /// Crater radius added once armed.
    pub crater_radius_bonus: u16,
}

/// Result of querying a timed payload upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedPayloadState {
    /// Payload at the queried tick.
    pub payload: Payload,
    /// Whether the upgrade has applied.
    pub armed: bool,
    /// Whether projectile motion should currently pause.
    pub motion_paused: bool,
}

/// Resolves arming state without mutating a live projectile.
#[must_use]
pub fn payload_at_tick(
    base: Payload,
    upgrade: TimedPayloadUpgrade,
    elapsed_ticks: u16,
) -> TimedPayloadState {
    let pause_end = upgrade.arm_after_ticks.saturating_add(upgrade.pause_ticks);
    let armed = elapsed_ticks >= pause_end;
    let motion_paused = elapsed_ticks >= upgrade.arm_after_ticks && elapsed_ticks < pause_end;
    let payload = if armed {
        Payload {
            damage: base.damage.saturating_add(upgrade.damage_bonus),
            crater_radius_cells: base
                .crater_radius_cells
                .saturating_add(upgrade.crater_radius_bonus),
        }
    } else {
        base
    };
    TimedPayloadState {
        payload,
        armed,
        motion_paused,
    }
}

/// One scheduled explosion in a staged or ring sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledBlast {
    /// Tick offset from the triggering impact.
    pub delay_ticks: u16,
    /// Blast center.
    pub position: FixedPoint,
    /// Damage and crater carried by this phase.
    pub payload: Payload,
}

/// Geometry and timing for a sequenced impact ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpactRingSpec {
    /// Number of blasts in the sequence.
    pub count: u8,
    /// Distance from the anchor in fixed-point units.
    pub radius: i32,
    /// First phase in sixteenths of a clockwise turn.
    pub starting_phase: u8,
    /// Phase difference between successive positions.
    pub phase_step: u8,
    /// Delay between successive blasts.
    pub interval_ticks: u16,
    /// Whether positions alternate clockwise and counter-clockwise around the start.
    pub alternating: bool,
}

/// Builds the two phases of a same-position double blast.
#[must_use]
pub const fn double_blast(
    position: FixedPoint,
    first: Payload,
    second: Payload,
    second_delay_ticks: u16,
) -> [ScheduledBlast; 2] {
    [
        ScheduledBlast {
            delay_ticks: 0,
            position,
            payload: first,
        },
        ScheduledBlast {
            delay_ticks: second_delay_ticks,
            position,
            payload: second,
        },
    ]
}

/// Returns a fixed-point offset on a deterministic 16-direction circle.
///
/// `phase` wraps modulo 16 and advances clockwise in screen coordinates.
///
/// # Errors
///
/// Returns [`SimError::Overflow`] when the scaled offset cannot fit in an `i32`.
pub fn orbit_offset(radius: i32, phase: u8) -> SimResult<FixedPoint> {
    if radius < 0 {
        return Err(SimError::OutOfRange {
            field: "orbit radius",
        });
    }
    let index = usize::from(phase % 16);
    let Some(&(unit_x, unit_y)) = UNIT_CIRCLE_16.get(index) else {
        return Err(SimError::OutOfRange {
            field: "orbit phase",
        });
    };
    let x = fixed::scale(radius, unit_x, CIRCLE_SCALE).ok_or(SimError::Overflow {
        context: "projectile_mechanics::orbit_x",
    })?;
    let y = fixed::scale(radius, unit_y, CIRCLE_SCALE).ok_or(SimError::Overflow {
        context: "projectile_mechanics::orbit_y",
    })?;
    Ok(FixedPoint::new(x, y))
}

/// Positions one orbiting collision payload around a carrier.
///
/// # Errors
///
/// Propagates invalid radius or scaling errors from [`orbit_offset`].
pub fn orbit_position(carrier: FixedPoint, radius: i32, phase: u8) -> SimResult<FixedPoint> {
    Ok(carrier.saturating_add(orbit_offset(radius, phase)?))
}

/// One projectile travelling from a distributed source toward a shared target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvergingProjectile {
    /// Stable zero-based child ordinal.
    pub ordinal: u8,
    /// Distributed launch position.
    pub start: FixedPoint,
    /// Shared convergence point.
    pub target: FixedPoint,
    /// Delay after the marker impact.
    pub delay_ticks: u16,
}

/// Geometry and timing for an impact- or satellite-origin convergence fan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvergenceFanSpec {
    /// Number of converging projectiles.
    pub count: u8,
    /// Distance of every source from `source_center`.
    pub source_radius: i32,
    /// First source phase in sixteenths of a clockwise turn.
    pub starting_phase: u8,
    /// Phase difference between sources.
    pub phase_step: u8,
    /// Delay between successive projectiles.
    pub stagger_ticks: u16,
}

/// Schedules projectiles from a fan of sources toward one target.
///
/// # Errors
///
/// Rejects an empty fan, more than 16 members, a negative source radius, or delay overflow.
pub fn schedule_convergence_fan(
    source_center: FixedPoint,
    target: FixedPoint,
    spec: ConvergenceFanSpec,
) -> SimResult<Vec<ConvergingProjectile>> {
    if spec.count == 0 || spec.count > 16 {
        return Err(SimError::OutOfRange {
            field: "convergence fan count",
        });
    }
    let mut projectiles = Vec::with_capacity(usize::from(spec.count));
    for ordinal in 0..spec.count {
        let phase = i32::from(spec.starting_phase)
            .saturating_add(i32::from(ordinal).saturating_mul(i32::from(spec.phase_step)))
            .rem_euclid(16);
        let phase = u8::try_from(phase).map_err(|_| SimError::Overflow {
            context: "projectile_mechanics::convergence_phase",
        })?;
        let delay_ticks =
            u16::from(ordinal)
                .checked_mul(spec.stagger_ticks)
                .ok_or(SimError::Overflow {
                    context: "projectile_mechanics::convergence_delay",
                })?;
        projectiles.push(ConvergingProjectile {
            ordinal,
            start: orbit_position(source_center, spec.source_radius, phase)?,
            target,
            delay_ticks,
        });
    }
    Ok(projectiles)
}

/// Contracts an orbit radius after a delay, saturating at zero.
#[must_use]
pub fn converging_radius(
    initial_radius: i32,
    elapsed_ticks: u16,
    converge_after_ticks: u16,
    contraction_per_tick: i32,
) -> i32 {
    if initial_radius <= 0 || contraction_per_tick <= 0 || elapsed_ticks <= converge_after_ticks {
        return initial_radius.max(0);
    }
    let active_ticks = i32::from(elapsed_ticks.saturating_sub(converge_after_ticks));
    initial_radius
        .saturating_sub(active_ticks.saturating_mul(contraction_per_tick))
        .max(0)
}

/// Creates a timed mid-flight split by reusing the bounded volley scheduler.
///
/// Child power is scaled by `power_basis_points`, and every child begins at `split_tick` plus its
/// own stagger delay.
///
/// # Errors
///
/// Propagates invalid volley parameters, invalid negative power, and arithmetic overflow.
pub fn schedule_split(
    spec: VolleySpec,
    base_angle_millidegrees: i32,
    parent_power: i32,
    power_basis_points: i32,
    split_tick: u16,
) -> SimResult<Vec<ScheduledProjectile>> {
    let child_power =
        fixed::apply_basis_points(parent_power, power_basis_points).ok_or(SimError::Overflow {
            context: "projectile_mechanics::split_power",
        })?;
    if child_power < 0 {
        return Err(SimError::OutOfRange {
            field: "split child power",
        });
    }
    let mut children = schedule_volley(spec, base_angle_millidegrees, child_power)?;
    for child in &mut children {
        child.delay_ticks =
            child
                .delay_ticks
                .checked_add(split_tick)
                .ok_or(SimError::Overflow {
                    context: "projectile_mechanics::split_delay",
                })?;
    }
    Ok(children)
}

/// Builds a bounded sequence of blasts around an impact center.
///
/// `phase_step` is measured in sixteenths of a turn. When `alternating` is true, the order is
/// center phase, one step clockwise, one counter-clockwise, two clockwise, and so on.
///
/// # Errors
///
/// Rejects an empty sequence, more than 16 blasts, a negative radius, or delay overflow.
pub fn schedule_impact_ring(
    center: FixedPoint,
    payload: Payload,
    spec: ImpactRingSpec,
) -> SimResult<Vec<ScheduledBlast>> {
    if spec.count == 0 || spec.count > 16 {
        return Err(SimError::OutOfRange {
            field: "impact ring count",
        });
    }
    let mut blasts = Vec::with_capacity(usize::from(spec.count));
    for ordinal in 0..spec.count {
        let sequence_offset = if spec.alternating {
            if ordinal == 0 {
                0
            } else {
                // Non-negative pair grouping: ordinals 1/2 use magnitude 1, 3/4 use 2.
                let magnitude = i32::from(ordinal.saturating_add(1) >> 1);
                if ordinal % 2 == 1 {
                    magnitude
                } else {
                    magnitude.saturating_neg()
                }
            }
        } else {
            i32::from(ordinal)
        };
        let phase = i32::from(spec.starting_phase)
            .saturating_add(sequence_offset.saturating_mul(i32::from(spec.phase_step)))
            .rem_euclid(16);
        let phase = u8::try_from(phase).map_err(|_| SimError::Overflow {
            context: "projectile_mechanics::ring_phase",
        })?;
        let delay_ticks =
            u16::from(ordinal)
                .checked_mul(spec.interval_ticks)
                .ok_or(SimError::Overflow {
                    context: "projectile_mechanics::ring_delay",
                })?;
        blasts.push(ScheduledBlast {
            delay_ticks,
            position: center.saturating_add(orbit_offset(spec.radius, phase)?),
            payload,
        });
    }
    Ok(blasts)
}

/// Builds a normal circular terrain removal operation for a projectile impact.
#[must_use]
pub const fn circular_terrain_blast(
    sequence: u32,
    center: FixedPoint,
    radius_cells: u16,
    material_mask: MaterialMask,
) -> TerrainOperation {
    TerrainOperation {
        sequence,
        shape: TerrainShape::SubtractCircle {
            center,
            radius_cells,
        },
        material_mask,
    }
}

/// Result of a bounded last-surface beam scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeamScan {
    /// Final solid terrain position encountered, if any.
    pub last_surface: Option<FixedPoint>,
    /// Last in-bounds position inspected.
    pub end_position: FixedPoint,
    /// Number of positions inspected.
    pub inspected_steps: u16,
}

/// Walks a ray through the terrain and remembers its final solid contact.
///
/// Unlike an ordinary projectile, a beam does not stop at its first surface. This makes separated
/// platforms and overhangs meaningful while the hard step cap guarantees termination.
///
/// # Errors
///
/// Rejects a zero step and a cap outside `1..=MAX_BEAM_STEPS`.
pub fn scan_last_surface(
    mask: &TerrainMask,
    start: FixedPoint,
    step: FixedPoint,
    max_steps: u16,
) -> SimResult<BeamScan> {
    if step == FixedPoint::ZERO {
        return Err(SimError::OutOfRange { field: "beam step" });
    }
    if max_steps == 0 || max_steps > MAX_BEAM_STEPS {
        return Err(SimError::OutOfRange {
            field: "beam step cap",
        });
    }

    let mut position = start;
    let mut last_surface = None;
    let mut inspected_steps = 0u16;
    for _ in 0..max_steps {
        let candidate = position.saturating_add(step);
        let (cell_x, cell_y) = candidate.to_cells();
        if !cell_in_bounds(mask, cell_x, cell_y) {
            break;
        }
        position = candidate;
        inspected_steps = inspected_steps.saturating_add(1);
        if terrain::is_solid_at(mask, position) {
            last_surface = Some(position);
        }
    }
    Ok(BeamScan {
        last_surface,
        end_position: position,
        inspected_steps,
    })
}

fn cell_in_bounds(mask: &TerrainMask, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 {
        return false;
    }
    let Ok(x) = u32::try_from(x) else {
        return false;
    };
    let Ok(y) = u32::try_from(y) else {
        return false;
    };
    x < mask.width && y < mask.height
}

/// Chooses the last free trajectory sample before terrain contact.
///
/// Character bodies are intentionally absent from this API, making it suitable for terrain-only
/// mine placement and teleport beacons.
#[must_use]
pub fn last_free_before_terrain(mask: &TerrainMask, samples: &[FixedPoint]) -> Option<FixedPoint> {
    let mut last_free = None;
    for &sample in samples.iter().take(usize::from(MAX_BEAM_STEPS)) {
        let (x, y) = sample.to_cells();
        if !cell_in_bounds(mask, x, y) || terrain::is_solid_at(mask, sample) {
            break;
        }
        last_free = Some(sample);
    }
    last_free
}

/// Stable target candidate for an autonomous mine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MineTarget {
    /// Stable authority-owned identifier used for tie-breaking.
    pub stable_id: u32,
    /// Current target position.
    pub position: FixedPoint,
}

/// Chooses the closest target, breaking equal-distance ties by stable identifier.
#[must_use]
pub fn nearest_mine_target(origin: FixedPoint, targets: &[MineTarget]) -> Option<MineTarget> {
    targets
        .iter()
        .copied()
        .min_by_key(|target| (squared_distance(origin, target.position), target.stable_id))
}

fn squared_distance(a: FixedPoint, b: FixedPoint) -> i64 {
    let dx = i64::from(a.x).saturating_sub(i64::from(b.x));
    let dy = i64::from(a.y).saturating_sub(i64::from(b.y));
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

/// Returns whether a mine target is inside its activation radius.
#[must_use]
pub fn mine_target_in_range(origin: FixedPoint, target: FixedPoint, radius: i32) -> bool {
    if radius < 0 {
        return false;
    }
    let radius = i64::from(radius);
    squared_distance(origin, target) <= radius.saturating_mul(radius)
}

/// Advances a walking mine horizontally toward a target by at most `maximum_step`.
#[must_use]
pub fn seek_mine_step(origin: FixedPoint, target: FixedPoint, maximum_step: i32) -> FixedPoint {
    if maximum_step <= 0 {
        return origin;
    }
    let difference = target.x.saturating_sub(origin.x);
    let distance = difference.saturating_abs().min(maximum_step);
    FixedPoint::new(
        origin
            .x
            .saturating_add(distance.saturating_mul(difference.signum())),
        origin.y,
    )
}

/// Reverses a roaming mine's horizontal step after an obstacle.
#[must_use]
pub const fn reflect_roaming_step(step: FixedPoint) -> FixedPoint {
    FixedPoint::new(step.x.saturating_neg(), step.y)
}

/// Applies a multiplicative and flat damage modifier with saturation.
///
/// `basis_points` uses 10,000 as 100%; `flat_delta` may be negative for a dampening zone.
///
/// # Errors
///
/// Returns an overflow error if the fixed-point multiplication cannot be represented.
pub fn modify_damage(base_damage: u16, basis_points: i32, flat_delta: i32) -> SimResult<u16> {
    let scaled = fixed::apply_basis_points(i32::from(base_damage), basis_points).ok_or(
        SimError::Overflow {
            context: "projectile_mechanics::damage_modifier",
        },
    )?;
    let modified = scaled
        .saturating_add(flat_delta)
        .clamp(0, i32::from(u16::MAX));
    u16::try_from(modified).map_err(|_| SimError::Overflow {
        context: "projectile_mechanics::damage_modifier_u16",
    })
}

/// Applies one stacking defense reduction without crossing its negative floor.
#[must_use]
pub fn apply_armor_shred(current_modifier: i16, reduction_per_hit: u16, floor: i16) -> i16 {
    let reduced = i32::from(current_modifier).saturating_sub(i32::from(reduction_per_hit));
    let bounded = reduced.max(i32::from(floor));
    let bounded = bounded.min(i32::from(current_modifier));
    i16::try_from(bounded).unwrap_or(current_modifier)
}

/// Damage and shield state produced by a shield-purge impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShieldPurgeOutcome {
    /// Total impact damage after adding the target's current shield.
    pub damage: u16,
    /// Shield remaining after the impact.
    pub shield_remaining: u16,
}

/// Adds the target's current shield to damage, then removes the shield.
#[must_use]
pub const fn purge_shield(base_damage: u16, current_shield: u16) -> ShieldPurgeOutcome {
    ShieldPurgeOutcome {
        damage: base_damage.saturating_add(current_shield),
        shield_remaining: 0,
    }
}

/// Projectile-affecting environment behaviors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentModifier {
    /// Increases payload damage.
    AmplifyDamage,
    /// Decreases payload damage.
    DampenDamage,
    /// Reflects horizontal velocity and may increase damage.
    MirrorHorizontal,
    /// Temporarily replaces normal movement with a redirected path.
    TornadoPath,
    /// Adds a beam on impact.
    ElectricFollowup,
    /// Activates one linked environment zone.
    ActivateLinkedZone,
    /// Adds a remote satellite strike on impact.
    SatelliteFollowup,
}

impl EnvironmentModifier {
    const fn bit(self) -> u16 {
        match self {
            Self::AmplifyDamage => 1 << 0,
            Self::DampenDamage => 1 << 1,
            Self::MirrorHorizontal => 1 << 2,
            Self::TornadoPath => 1 << 3,
            Self::ElectricFollowup => 1 << 4,
            Self::ActivateLinkedZone => 1 << 5,
            Self::SatelliteFollowup => 1 << 6,
        }
    }
}

/// Idempotence guard recording which environment types have affected a projectile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnvironmentModifierSet(u16);

impl EnvironmentModifierSet {
    /// Records `modifier` and returns `true` only on its first application.
    pub fn apply_once(&mut self, modifier: EnvironmentModifier) -> bool {
        let bit = modifier.bit();
        if self.0 & bit != 0 {
            return false;
        }
        self.0 |= bit;
        true
    }

    /// Reports whether a modifier was already applied.
    #[must_use]
    pub const fn contains(self, modifier: EnvironmentModifier) -> bool {
        self.0 & modifier.bit() != 0
    }
}

/// One leg of a path imposed by an environment zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedirectLeg {
    /// Fixed movement delta for each tick of this leg.
    pub delta_per_tick: FixedPoint,
    /// Whether normal ballistics should be restored after this leg.
    pub restores_ballistics: bool,
}

/// Builds the three-leg redirected motion used by a tornado-like zone.
///
/// The projectile first continues through the near side, then crosses with horizontal direction
/// reversed, then exits along its original direction and resumes ordinary ballistics.
#[must_use]
pub const fn tornado_redirect(incoming_step: FixedPoint) -> [RedirectLeg; 3] {
    [
        RedirectLeg {
            delta_per_tick: incoming_step,
            restores_ballistics: false,
        },
        RedirectLeg {
            delta_per_tick: FixedPoint::new(incoming_step.x.saturating_neg(), incoming_step.y),
            restores_ballistics: false,
        },
        RedirectLeg {
            delta_per_tick: incoming_step,
            restores_ballistics: true,
        },
    ]
}

/// Reflects horizontal projectile velocity while preserving vertical velocity.
#[must_use]
pub const fn mirror_horizontal(velocity: FixedPoint) -> FixedPoint {
    FixedPoint::new(velocity.x.saturating_neg(), velocity.y)
}

/// Mergeable environment-zone data independent of rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentZone {
    /// Environment behavior carried by the zone.
    pub modifier: EnvironmentModifier,
    /// Horizontal center in fixed-point units.
    pub center_x: i32,
    /// Relative zone strength in basis points.
    pub strength_basis_points: u16,
}

/// Merges two zones of the same type by averaging their centers and adding strength.
///
/// # Errors
///
/// Rejects zones with different modifiers.
pub fn merge_environment_zones(
    first: EnvironmentZone,
    second: EnvironmentZone,
) -> SimResult<EnvironmentZone> {
    if first.modifier != second.modifier {
        return Err(SimError::OutOfRange {
            field: "environment modifier merge",
        });
    }
    let center_sum = i64::from(first.center_x).saturating_add(i64::from(second.center_x));
    let center_x = fixed::round_divide(center_sum, 2)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(SimError::Overflow {
            context: "projectile_mechanics::zone_center",
        })?;
    Ok(EnvironmentZone {
        modifier: first.modifier,
        center_x,
        strength_basis_points: first
            .strength_basis_points
            .saturating_add(second.strength_basis_points),
    })
}

/// One target selected for a proximity beam cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeamTarget {
    /// Stable authority-owned identifier.
    pub stable_id: u32,
    /// Target position.
    pub position: FixedPoint,
}

/// Returns in-range beam targets in stable identifier order.
#[must_use]
pub fn targets_for_beam_cascade(
    center: FixedPoint,
    radius: i32,
    targets: &[BeamTarget],
) -> Vec<BeamTarget> {
    if radius < 0 {
        return Vec::new();
    }
    let radius = i64::from(radius);
    let radius_squared = radius.saturating_mul(radius);
    let maximum = usize::from(MAX_CHILD_PROJECTILES);
    let mut selected = Vec::with_capacity(maximum);
    for &target in targets {
        if squared_distance(center, target.position) > radius_squared {
            continue;
        }
        let insertion =
            selected.partition_point(|existing: &BeamTarget| existing.stable_id < target.stable_id);
        if selected
            .get(insertion)
            .is_some_and(|existing| existing.stable_id == target.stable_id)
        {
            continue;
        }
        if selected.len() < maximum {
            selected.insert(insertion, target);
        } else if insertion < maximum {
            selected.insert(insertion, target);
            let _ = selected.pop();
        }
    }
    selected
}

/// Converts a whole-cell radius to fixed-point units without wrapping.
///
/// # Errors
///
/// Returns overflow when `radius_cells * POSITION_SCALE` cannot fit in `i32`.
pub fn radius_from_cells(radius_cells: u16) -> SimResult<i32> {
    i32::from(radius_cells)
        .checked_mul(POSITION_SCALE)
        .ok_or(SimError::Overflow {
            context: "projectile_mechanics::radius_from_cells",
        })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::terrain::{apply_operation, create_mask, set_material};
    use crate::types::Material;

    fn point(x: i32, y: i32) -> FixedPoint {
        FixedPoint::from_cells(x, y).unwrap_or(FixedPoint::ZERO)
    }

    #[test]
    fn mechanic_catalog_is_exhaustive_and_duplicate_free() {
        assert_eq!(ProjectileMechanic::ALL.len(), 39);
        let names: BTreeSet<&str> = ProjectileMechanic::ALL
            .iter()
            .map(|mechanic| mechanic.design_name())
            .collect();
        assert_eq!(names.len(), ProjectileMechanic::ALL.len());
    }

    #[test]
    fn contact_policy_keeps_placement_shots_character_transparent() {
        assert!(contact_stops_flight(
            ContactPolicy::TerrainOrCharacter,
            false,
            true
        ));
        assert!(!contact_stops_flight(
            ContactPolicy::TerrainOnly,
            false,
            true
        ));
        assert!(contact_stops_flight(
            ContactPolicy::TerrainOnly,
            true,
            false
        ));
        assert!(!contact_stops_flight(
            ContactPolicy::LastSurfaceAlongBeam,
            true,
            true
        ));
    }

    #[test]
    fn radial_damage_has_safe_center_edge_and_outside_values() {
        let radius = 4 * POSITION_SCALE;
        assert_eq!(radial_damage(100, point(4, 4), point(4, 4), radius), 100);
        assert_eq!(radial_damage(100, point(4, 4), point(8, 4), radius), 0);
        assert_eq!(radial_damage(100, point(4, 4), point(9, 4), radius), 0);
        assert_eq!(radial_damage(100, point(4, 4), point(6, 4), radius), 75);
    }

    #[test]
    fn volley_centers_even_spread_and_staggers_deterministically() {
        let scheduled = schedule_volley(
            VolleySpec {
                count: 4,
                angle_step_millidegrees: 2_000,
                power_step: 100,
                stagger_ticks: 3,
            },
            45_000,
            2_000,
        )
        .unwrap_or_default();
        let summary: Vec<(i32, i32, u16)> = scheduled
            .iter()
            .map(|child| (child.angle_millidegrees, child.power, child.delay_ticks))
            .collect();
        assert_eq!(
            summary,
            vec![
                (42_000, 2_000, 0),
                (44_000, 2_100, 3),
                (46_000, 2_200, 6),
                (48_000, 2_300, 9),
            ]
        );
    }

    #[test]
    fn volley_rejects_unbounded_child_counts() {
        let invalid = VolleySpec {
            count: MAX_CHILD_PROJECTILES.saturating_add(1),
            angle_step_millidegrees: 0,
            power_step: 0,
            stagger_ticks: 0,
        };
        assert!(schedule_volley(invalid, 0, 0).is_err());
    }

    #[test]
    fn dependency_group_completes_exactly_once() {
        let mut group = DependencyGroup::new(3).unwrap_or(DependencyGroup {
            expected: 3,
            resolved: 0,
        });
        assert_eq!(group.resolve_one(), Ok(false));
        assert_eq!(group.resolve_one(), Ok(false));
        assert_eq!(group.resolve_one(), Ok(true));
        assert_eq!(group.remaining(), 0);
        assert!(group.resolve_one().is_err());
    }

    #[test]
    fn timed_payload_pauses_then_arms() {
        let base = Payload {
            damage: 20,
            crater_radius_cells: 2,
        };
        let upgrade = TimedPayloadUpgrade {
            arm_after_ticks: 10,
            pause_ticks: 3,
            damage_bonus: 7,
            crater_radius_bonus: 1,
        };
        assert_eq!(payload_at_tick(base, upgrade, 9).payload, base);
        assert!(payload_at_tick(base, upgrade, 10).motion_paused);
        let armed = payload_at_tick(base, upgrade, 13);
        assert!(armed.armed);
        assert!(!armed.motion_paused);
        assert_eq!(armed.payload.damage, 27);
        assert_eq!(armed.payload.crater_radius_cells, 3);
    }

    #[test]
    fn double_blast_preserves_two_payload_phases() {
        let phases = double_blast(
            point(4, 5),
            Payload {
                damage: 30,
                crater_radius_cells: 3,
            },
            Payload {
                damage: 10,
                crater_radius_cells: 1,
            },
            6,
        );
        assert_eq!(phases[0].delay_ticks, 0);
        assert_eq!(phases[1].delay_ticks, 6);
        assert_eq!(phases[1].payload.damage, 10);
    }

    #[test]
    fn orbit_and_convergence_are_integer_only_and_bounded() {
        assert_eq!(orbit_offset(POSITION_SCALE, 0), Ok(point(1, 0)));
        assert_eq!(orbit_offset(POSITION_SCALE, 4), Ok(point(0, 1)));
        assert_eq!(
            orbit_position(point(5, 5), POSITION_SCALE, 8),
            Ok(point(4, 5))
        );
        assert_eq!(converging_radius(1_000, 10, 10, 100), 1_000);
        assert_eq!(converging_radius(1_000, 15, 10, 100), 500);
        assert_eq!(converging_radius(1_000, 30, 10, 100), 0);
    }

    #[test]
    fn convergence_fan_distributes_sources_and_staggers_arrival() {
        let target = point(10, 8);
        let projectiles = schedule_convergence_fan(
            point(10, 8),
            target,
            ConvergenceFanSpec {
                count: 4,
                source_radius: 2 * POSITION_SCALE,
                starting_phase: 0,
                phase_step: 4,
                stagger_ticks: 5,
            },
        )
        .unwrap_or_default();
        assert_eq!(projectiles.len(), 4);
        assert_eq!(
            projectiles.first().map(|shot| shot.start),
            Some(point(12, 8))
        );
        assert_eq!(
            projectiles.get(1).map(|shot| shot.start),
            Some(point(10, 10))
        );
        assert!(projectiles.iter().all(|shot| shot.target == target));
        assert_eq!(projectiles.last().map(|shot| shot.delay_ticks), Some(15));
    }

    #[test]
    fn split_inherits_parent_angle_and_scales_power() {
        let children = schedule_split(
            VolleySpec {
                count: 3,
                angle_step_millidegrees: 3_000,
                power_step: 0,
                stagger_ticks: 1,
            },
            60_000,
            4_000,
            5_000,
            90,
        )
        .unwrap_or_default();
        assert_eq!(children.len(), 3);
        assert_eq!(
            children.first().map(|child| child.angle_millidegrees),
            Some(57_000)
        );
        assert!(children.iter().all(|child| child.power == 2_000));
        assert_eq!(children.last().map(|child| child.delay_ticks), Some(92));
    }

    #[test]
    fn impact_ring_alternates_around_anchor() {
        let blasts = schedule_impact_ring(
            point(10, 10),
            Payload {
                damage: 5,
                crater_radius_cells: 1,
            },
            ImpactRingSpec {
                count: 5,
                radius: POSITION_SCALE,
                starting_phase: 0,
                phase_step: 2,
                interval_ticks: 4,
                alternating: true,
            },
        )
        .unwrap_or_default();
        let positions: Vec<FixedPoint> = blasts.iter().map(|blast| blast.position).collect();
        assert_eq!(positions.first(), Some(&point(11, 10)));
        assert_eq!(
            positions.get(1),
            Some(&FixedPoint::new(
                10 * POSITION_SCALE + 724,
                10 * POSITION_SCALE + 724
            ))
        );
        assert_eq!(
            positions.get(2),
            Some(&FixedPoint::new(
                10 * POSITION_SCALE + 724,
                10 * POSITION_SCALE - 724
            ))
        );
        assert_eq!(blasts.last().map(|blast| blast.delay_ticks), Some(16));
    }

    #[test]
    fn circular_blast_uses_existing_material_rules() {
        let mut mask = create_mask(9, 9, Material::Soil).unwrap_or(TerrainMask {
            width: 9,
            height: 9,
            cells: vec![Material::Soil as u8; 81],
        });
        let _ = set_material(&mut mask, 4, 4, Material::ReinforcedStone);
        let operation = circular_terrain_blast(7, point(4, 4), 2, MaterialMask::SOFT);
        let removed = apply_operation(&mut mask, &operation);
        assert!(removed > 0);
        assert_eq!(terrain::material_at(&mask, 4, 4), Material::ReinforcedStone);
        assert_eq!(terrain::material_at(&mask, 4, 3), Material::Empty);
    }

    #[test]
    fn beam_remembers_last_separated_surface() {
        let mut mask = create_mask(10, 3, Material::Empty).unwrap_or(TerrainMask {
            width: 10,
            height: 3,
            cells: vec![0; 30],
        });
        let _ = set_material(&mut mask, 3, 1, Material::Wood);
        let _ = set_material(&mut mask, 7, 1, Material::Soil);
        let scan = scan_last_surface(&mask, point(0, 1), point(1, 0), 20).unwrap_or(BeamScan {
            last_surface: None,
            end_position: FixedPoint::ZERO,
            inspected_steps: 0,
        });
        assert_eq!(scan.last_surface, Some(point(7, 1)));
        assert_eq!(scan.end_position, point(9, 1));
        assert_eq!(scan.inspected_steps, 9);
    }

    #[test]
    fn beam_rejects_zero_step_and_excessive_work() {
        let mask = create_mask(2, 2, Material::Empty).unwrap_or(TerrainMask {
            width: 2,
            height: 2,
            cells: vec![0; 4],
        });
        assert!(scan_last_surface(&mask, point(0, 0), FixedPoint::ZERO, 2).is_err());
        assert!(
            scan_last_surface(
                &mask,
                point(0, 0),
                point(1, 0),
                MAX_BEAM_STEPS.saturating_add(1)
            )
            .is_err()
        );
    }

    #[test]
    fn terrain_only_landing_uses_last_free_sample() {
        let mut mask = create_mask(6, 2, Material::Empty).unwrap_or(TerrainMask {
            width: 6,
            height: 2,
            cells: vec![0; 12],
        });
        let _ = set_material(&mut mask, 4, 1, Material::Soil);
        let samples = [
            point(1, 1),
            point(2, 1),
            point(3, 1),
            point(4, 1),
            point(5, 1),
        ];
        assert_eq!(last_free_before_terrain(&mask, &samples), Some(point(3, 1)));
    }

    #[test]
    fn mine_targeting_is_stable_and_motion_is_bounded() {
        let targets = [
            MineTarget {
                stable_id: 9,
                position: point(-2, 0),
            },
            MineTarget {
                stable_id: 3,
                position: point(2, 0),
            },
        ];
        assert_eq!(nearest_mine_target(point(0, 0), &targets), Some(targets[1]));
        assert!(mine_target_in_range(
            point(0, 0),
            point(2, 0),
            2 * POSITION_SCALE
        ));
        assert_eq!(
            seek_mine_step(point(0, 0), point(3, 0), POSITION_SCALE),
            point(1, 0)
        );
        assert_eq!(reflect_roaming_step(point(1, 0)), point(-1, 0));
    }

    #[test]
    fn damage_modifiers_saturate_and_dampen() {
        assert_eq!(modify_damage(100, 12_000, 10), Ok(130));
        assert_eq!(modify_damage(100, 8_000, -10), Ok(70));
        assert_eq!(modify_damage(5, 5_000, -20), Ok(0));
        assert_eq!(modify_damage(u16::MAX, 20_000, 100), Ok(u16::MAX));
    }

    #[test]
    fn armor_shred_stacks_to_floor_without_healing_existing_debuff() {
        assert_eq!(apply_armor_shred(0, 5, -35), -5);
        assert_eq!(apply_armor_shred(-33, 5, -35), -35);
        assert_eq!(apply_armor_shred(-40, 5, -35), -40);
    }

    #[test]
    fn shield_purge_adds_and_removes_shield_with_saturation() {
        assert_eq!(
            purge_shield(30, 20),
            ShieldPurgeOutcome {
                damage: 50,
                shield_remaining: 0,
            }
        );
        assert_eq!(purge_shield(u16::MAX, 1).damage, u16::MAX);
    }

    #[test]
    fn environment_modifiers_apply_once_per_type() {
        let mut applied = EnvironmentModifierSet::default();
        assert!(applied.apply_once(EnvironmentModifier::AmplifyDamage));
        assert!(!applied.apply_once(EnvironmentModifier::AmplifyDamage));
        assert!(applied.apply_once(EnvironmentModifier::ElectricFollowup));
        assert!(applied.contains(EnvironmentModifier::AmplifyDamage));
        assert!(applied.contains(EnvironmentModifier::ElectricFollowup));
    }

    #[test]
    fn mirror_and_tornado_preserve_vertical_motion() {
        let incoming = FixedPoint::new(20, -5);
        assert_eq!(mirror_horizontal(incoming), FixedPoint::new(-20, -5));
        let legs = tornado_redirect(incoming);
        assert_eq!(legs[0].delta_per_tick, incoming);
        assert_eq!(legs[1].delta_per_tick, FixedPoint::new(-20, -5));
        assert!(legs[2].restores_ballistics);
    }

    #[test]
    fn environment_zones_merge_only_with_same_type() {
        let first = EnvironmentZone {
            modifier: EnvironmentModifier::AmplifyDamage,
            center_x: 10,
            strength_basis_points: 5_000,
        };
        let second = EnvironmentZone {
            modifier: EnvironmentModifier::AmplifyDamage,
            center_x: 20,
            strength_basis_points: 6_000,
        };
        assert_eq!(
            merge_environment_zones(first, second),
            Ok(EnvironmentZone {
                modifier: EnvironmentModifier::AmplifyDamage,
                center_x: 15,
                strength_basis_points: 11_000,
            })
        );
        let different = EnvironmentZone {
            modifier: EnvironmentModifier::DampenDamage,
            ..second
        };
        assert!(merge_environment_zones(first, different).is_err());
    }

    #[test]
    fn beam_cascade_is_filtered_sorted_and_bounded() {
        let targets: Vec<BeamTarget> = (0..40)
            .rev()
            .map(|stable_id| BeamTarget {
                stable_id,
                position: point(1, 0),
            })
            .collect();
        let selected = targets_for_beam_cascade(point(0, 0), 2 * POSITION_SCALE, &targets);
        assert_eq!(selected.len(), usize::from(MAX_CHILD_PROJECTILES));
        assert_eq!(selected.first().map(|target| target.stable_id), Some(0));
        assert_eq!(selected.last().map(|target| target.stable_id), Some(31));

        let duplicate = BeamTarget {
            stable_id: 7,
            position: point(1, 0),
        };
        let duplicates = [duplicate, duplicate];
        assert_eq!(
            targets_for_beam_cascade(point(0, 0), 2 * POSITION_SCALE, &duplicates).len(),
            1
        );
    }

    #[test]
    fn cell_radius_conversion_uses_authoritative_scale() {
        assert_eq!(radius_from_cells(3), Ok(3 * POSITION_SCALE));
    }
}
