//! Shared data contract for the simulation core.
//!
//! **This module is the interface every other module agrees on.** Behavior modules
//! (`terrain`, `character`, `ballistics`, `command`, `rng`, `scheduler`) operate on these
//! types; they do not define their own. Changing a type here is a cross-cutting change
//! and must be coordinated, not made locally.
//!
//! Per ADR 0002, characters replace the three-slot loadout. A player picks one character
//! and its fixed kit is their whole moveset. The *shape* of an attack is unchanged from
//! the retired weapon model — a projectile is still a projectile — so terrain,
//! ballistics, damage, and knockback carry over untouched. Only the owner of an attack
//! changed, from "a weapon a player equipped" to "an ability a character has".

use crate::blocks::TerrainBlock;
use crate::fixed::{BODY_WIDTH, FixedPoint, POSITION_SCALE};

/// Reference value that every damage percentage scales from (`CHARACTERS.md` §2).
///
/// A "55% attack" deals 55 hit points. One shared reference keeps all 24 characters on a
/// comparable scale, and — unlike scaling from target max HP — it means a 400 HP tank is
/// genuinely tankier than a 190 HP assassin rather than merely having a bigger number.
pub const BASE_ATTACK: i32 = 100;

/// Special-gauge scale. The gauge is stored in hundredths so the fractional per-damage
/// gains in `CHARACTERS.md` §2 (+0.40 dealt, +0.25 taken, +0.30 healed) stay exact
/// integer arithmetic — a float here would break determinism (ADR 0001 §4).
pub const GAUGE_FULL: u16 = 10_000;

/// Maximum gauge charge from any single action, in hundredths (+45.00).
///
/// Without a cap, one large hit fills the gauge instantly and the special stops being
/// something a player builds toward.
pub const GAUGE_MAX_PER_ACTION: u16 = 4_500;

// ---------------------------------------------------------------------------
// Character shape
// ---------------------------------------------------------------------------

/// How far a character can reach (`CHARACTERS.md` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RangeTier {
    /// Must close completely. Terrain and positioning dominate.
    Melee,
    /// Short. Pressures adjacent ground but cannot cross the map.
    Tier1,
    /// Medium. The default engagement band.
    Tier2,
    /// Long. Threatens most of a standard map.
    Tier3,
}

impl RangeTier {
    /// Reach in fixed-point units.
    #[must_use]
    pub const fn reach(self) -> i32 {
        match self {
            // 1.25 BW, matching the retired melee standard so existing tuning transfers.
            // Written in POSITION_SCALE units because one BW is exactly four of them —
            // this states the fractional reach exactly, with no division to round.
            Self::Melee => 5 * POSITION_SCALE,
            Self::Tier1 => 8 * BODY_WIDTH,
            Self::Tier2 => 16 * BODY_WIDTH,
            Self::Tier3 => 26 * BODY_WIDTH,
        }
    }

    /// Stable wire identifier. Never localize these.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Melee => "melee",
            Self::Tier1 => "tier1",
            Self::Tier2 => "tier2",
            Self::Tier3 => "tier3",
        }
    }
}

/// How far a character moves per turn (`CHARACTERS.md` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MovementClass {
    /// 2.5 BW.
    Slow,
    /// 4 BW.
    Normal,
    /// 8 BW — double normal.
    Fast,
}

impl MovementClass {
    /// Movement allowance per turn, in fixed-point units.
    #[must_use]
    pub const fn per_turn(self) -> i32 {
        match self {
            // 2.5 BW, expressed exactly in POSITION_SCALE units (1 BW == 4 of them).
            Self::Slow => 10 * POSITION_SCALE,
            Self::Normal => 4 * BODY_WIDTH,
            Self::Fast => 8 * BODY_WIDTH,
        }
    }

    /// Stable wire identifier.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Slow => "slow",
            Self::Normal => "normal",
            Self::Fast => "fast",
        }
    }
}

/// Which of a character's abilities is being used.
///
/// Bounded at three so the HUD is bounded at three buttons. `BasicAlt` exists because
/// Aleph carries both a bow and throwing knives; modelling that as a second basic is
/// cheaper than a general per-character ability list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AbilitySlot {
    /// Primary basic attack. Always available, never gated.
    Basic,
    /// Optional second basic attack. Always available on characters that have one.
    BasicAlt,
    /// Special. Requires a full gauge, which it consumes entirely.
    Special,
}

impl AbilitySlot {
    /// All slots in canonical order. Iteration order is fixed for hashing.
    pub const ALL: [Self; 3] = [Self::Basic, Self::BasicAlt, Self::Special];

    /// Stable wire identifier.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Basic => "basic",
            Self::BasicAlt => "basicAlt",
            Self::Special => "special",
        }
    }

    /// Whether using this slot requires and consumes a full special gauge.
    #[must_use]
    pub const fn consumes_gauge(self) -> bool {
        matches!(self, Self::Special)
    }
}

// ---------------------------------------------------------------------------
// Ability definitions
// ---------------------------------------------------------------------------

/// When a special effect fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectTrigger {
    /// At the moment the ability is committed.
    OnFire,
    /// During projectile flight.
    OnFlight,
    /// On impact with terrain or a character.
    OnImpact,
    /// At the end of the acting player's turn.
    OnTurnEnd,
}

/// The reviewed vocabulary of ability effects.
///
/// This is a **closed set**. Character kits are data that reference these identifiers;
/// there is no scripting language and no dynamically loaded behavior
/// (`SECURITY_BASELINE.md` §6). Adding a variant requires a client and server build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EffectKind {
    /// Displaces the victim along the impact normal.
    Knockback,
    /// Reduces the victim's next-turn movement cap.
    Chill,
    /// Splits into submunitions.
    Cluster,
    /// Leaves marked contact zones that damage on entry.
    Embers,
    /// Bores through terrain before detonating.
    Tunnel,
    /// Follows an outbound-and-return path.
    Return,
    /// Displaces the *actor* opposite the shot.
    Recoil,
    /// Damages the acting player. Surfaced in the UI as **Backlash**, previewed before
    /// commitment, and never hideable by a skin.
    SelfDamage,
    /// Relocates the actor to a computed or chosen destination (Arzum, Aleph, Huck).
    Teleport,
    /// Pulls a target toward the actor, or the actor toward the target (Numa, Natomica).
    Pull,
    /// Pushes targets away from a point (Natomica's Repulse).
    Push,
    /// Extra damage when a pushed target strikes terrain (Natomica).
    WallImpact,
    /// Prevents a target from moving or being displaced for a number of turns (Numa).
    Lockdown,
    /// Spawns a persistent, destructible ally object (Emi's turret).
    SpawnTurret,
    /// Restores health to a chosen ally.
    Heal,
    /// Transfers health from the actor to an ally (Zeke's Lifeshare).
    HealthTransfer,
    /// Resolves the same attack several times in one turn (Karl).
    MultiStrike,
    /// Guarantees critical hits for a number of subsequent attacks (Karl).
    GuaranteeCrit,
    /// Leaves a persistent object embedded in terrain (Aleph's knives).
    EmbedProjectile,
    /// Detonates embedded objects in a deterministic chain (Aleph).
    ChainDetonate,
    /// Moves a target to another target's position (Huck's Body Throw).
    Relocate,
    /// Blocks line of sight within a radius for a number of turns (Aleph's gas).
    Obscure,
}

/// An effect attached to an ability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialEffect {
    /// When it fires.
    pub trigger: EffectTrigger,
    /// What it does.
    pub kind: EffectKind,
    /// Effect-specific magnitude, in the unit natural to `kind` — fixed-point distance
    /// for displacement, hit points for damage and healing, cell counts for terrain,
    /// percent-of-[`BASE_ATTACK`] for scaled damage.
    pub magnitude: i32,
    /// Secondary magnitude, for effects needing two numbers (a damage range's upper
    /// bound, a radius alongside a duration). Zero when unused.
    pub magnitude_secondary: i32,
    /// How many turns it persists. Zero for instantaneous effects.
    pub duration_turns: u8,
}

/// How an ability alters terrain on detonation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainProfile {
    /// Leaves terrain untouched.
    None,
    /// Removes a circle.
    Crater {
        /// Radius in whole terrain cells.
        radius_cells: u16,
    },
    /// Removes a capsule — a swept circle, used for digging and tunnelling.
    Dig {
        /// Radius in whole terrain cells.
        radius_cells: u16,
        /// Sweep length in whole terrain cells.
        length_cells: u16,
    },
}

/// A ballistic or direct projectile attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectileAttack {
    /// Fixed-point distance added per tick at full power.
    pub speed_per_tick: i32,
    /// Fixed-point downward acceleration added per tick.
    pub gravity_per_tick: i32,
    /// Wind response in basis points. `0` is wind-immune; Emi's dense cube is lowest in
    /// the roster at 2_000.
    pub wind_scale_basis_points: i32,
    /// Hard lifetime cap. Bounds worst-case server work and guarantees termination.
    pub max_ticks: u16,
    /// How many times the projectile bounces before detonating (Roberto's grenade: 1).
    pub bounces: u8,
    /// Terrain effect on detonation.
    pub terrain: TerrainProfile,
}

/// A close-range attack resolved without a flying projectile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrikeAttack {
    /// Reach in fixed-point units. Normally the character's [`RangeTier::reach`], but
    /// stated explicitly so an ability can differ from its character's default.
    pub range: i32,
    /// Terrain effect.
    pub terrain: TerrainProfile,
    /// Backlash dealt to the acting player. Can eliminate its user.
    pub self_damage: u16,
}

/// The two attack shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attack {
    /// Flies through the world.
    Projectile(ProjectileAttack),
    /// Resolves immediately within reach.
    Strike(StrikeAttack),
}

/// Maximum effects on one ability. Bounds definition size so a malformed content pack
/// cannot produce unbounded per-impact work.
pub const MAX_SPECIAL_EFFECTS: usize = 6;

/// A single ability belonging to a character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityDefinition {
    /// Stable identifier, e.g. `"arzum-chain-strike"`.
    pub id: &'static str,
    /// Player-facing name.
    pub display_name: &'static str,
    /// Which slot it occupies.
    pub slot: AbilitySlot,
    /// Damage as a percentage of [`BASE_ATTACK`]. `45` means 45 hit points.
    pub damage_percent: u16,
    /// Critical-hit damage percentage. Equal to `damage_percent` when the ability
    /// cannot crit.
    pub crit_damage_percent: u16,
    /// Crit chance in basis points. Zero when the ability cannot crit.
    pub crit_chance_basis_points: u16,
    /// How many times this ability resolves in one turn. `1` for most; Karl's basic is
    /// `3`.
    pub strikes_per_turn: u8,
    /// Attack shape and parameters.
    pub attack: Attack,
    /// Effects, in declaration order.
    pub effects: &'static [SpecialEffect],
}

impl AbilityDefinition {
    /// Damage in hit points for a non-critical hit.
    ///
    /// Returns [`None`] on overflow rather than wrapping — a wrapped damage value would
    /// heal the target.
    #[must_use]
    pub const fn damage(&self) -> Option<i32> {
        crate::fixed::scale(BASE_ATTACK, self.damage_percent as i32, 100)
    }

    /// Damage in hit points for a critical hit.
    #[must_use]
    pub const fn crit_damage(&self) -> Option<i32> {
        crate::fixed::scale(BASE_ATTACK, self.crit_damage_percent as i32, 100)
    }
}

// ---------------------------------------------------------------------------
// Passives
// ---------------------------------------------------------------------------

/// Maximum passives offered per character. The choice is presented as exactly three
/// options (`CHARACTERS.md` §2).
pub const PASSIVES_PER_CHARACTER: usize = 3;

/// A passive a player may select the first time their gauge fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassiveDefinition {
    /// Stable identifier, e.g. `"arzum-momentum"`.
    pub id: &'static str,
    /// Player-facing name.
    pub display_name: &'static str,
    /// What it modifies.
    pub kind: PassiveKind,
    /// Magnitude in the unit natural to `kind`.
    pub magnitude: i32,
}

/// The closed vocabulary of passive effects.
///
/// Closed for the same reason [`EffectKind`] is: passives are versioned data referencing
/// reviewed behavior, never downloadable logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PassiveKind {
    /// Flat bonus to maximum health.
    BonusMaxHealth,
    /// Flat bonus to movement per turn, fixed-point.
    BonusMovement,
    /// Additive crit chance, basis points.
    BonusCritChance,
    /// Percentage reduction to incoming damage, basis points.
    DamageReduction,
    /// Percentage bonus to outgoing damage, basis points.
    DamageBonus,
    /// Percentage bonus to healing done, basis points.
    HealingBonus,
    /// Extra turns on this character's duration effects.
    BonusDuration,
    /// Extra radius on this character's terrain effects, fixed-point.
    BonusRadius,
    /// Immunity to knockback, push, and pull.
    DisplacementImmunity,
    /// Immunity to the actor's own friendly fire.
    SelfDamageImmunity,
    /// Converts a random ability target or destination into a chosen one.
    RemoveRandomness,
    /// Raises a persistent-object cap (Aleph's embedded knives).
    BonusObjectCap,
    /// Heals the actor for a flat amount on each connecting attack.
    LifestealFlat,
}

// ---------------------------------------------------------------------------
// Character definitions
// ---------------------------------------------------------------------------

/// A complete, versioned playable character.
///
/// Definitions are immutable once used by a completed match. Balance changes publish a
/// new version rather than mutating in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterDefinition {
    /// Stable identifier, e.g. `"arzum"`.
    pub id: &'static str,
    /// Definition version.
    pub version: u32,
    /// Player-facing name.
    pub display_name: &'static str,
    /// Starting and maximum health.
    pub max_health: u16,
    /// Default reach.
    pub range_tier: RangeTier,
    /// Movement allowance class.
    pub movement: MovementClass,
    /// Primary basic attack.
    pub basic: AbilityDefinition,
    /// Optional second basic attack. Only Aleph has one at launch.
    pub basic_alt: Option<AbilityDefinition>,
    /// Special, gated by a full gauge.
    pub special: AbilityDefinition,
    /// Exactly [`PASSIVES_PER_CHARACTER`] options.
    pub passives: &'static [PassiveDefinition],
    /// Whether this character is free on every account (`CHARACTERS.md` §3).
    pub is_starter: bool,
    /// Credit cost when not a starter. Zero for starters.
    pub credit_cost: u32,
}

impl CharacterDefinition {
    /// The ability in `slot`, if this character has one there.
    #[must_use]
    pub const fn ability(&self, slot: AbilitySlot) -> Option<&AbilityDefinition> {
        match slot {
            AbilitySlot::Basic => Some(&self.basic),
            AbilitySlot::BasicAlt => self.basic_alt.as_ref(),
            AbilitySlot::Special => Some(&self.special),
        }
    }
}

// ---------------------------------------------------------------------------
// Terrain
// ---------------------------------------------------------------------------

/// Terrain material. Destructibility is per-material
/// (`PRODUCT_SPEC.md` §4): only the Breach Pick removes reinforced stone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Material {
    /// Open space.
    Empty = 0,
    /// Broadly destructible.
    Soil = 1,
    /// Broadly destructible.
    Wood = 2,
    /// Resists everything except the Breach Pick.
    ReinforcedStone = 3,
}

impl Material {
    /// Whether this cell blocks movement and projectiles.
    #[must_use]
    pub const fn is_solid(self) -> bool {
        !matches!(self, Self::Empty)
    }

    /// Reconstructs a material from its stored byte, mapping unknown values to
    /// [`Material::Empty`] so a corrupted mask degrades deterministically instead of
    /// producing undefined collision.
    #[must_use]
    pub const fn from_byte(value: u8) -> Self {
        match value {
            1 => Self::Soil,
            2 => Self::Wood,
            3 => Self::ReinforcedStone,
            _ => Self::Empty,
        }
    }
}

/// The authoritative terrain occupancy mask.
///
/// One byte per cell holding a [`Material`]. Deliberately coarser than the display
/// texture — collision at full visual resolution buys nothing and costs cache
/// (`PLATFORM_STRATEGY.md` §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainMask {
    /// Width in cells.
    pub width: u32,
    /// Height in cells.
    pub height: u32,
    /// Row-major cells, `width * height` long, origin top-left.
    pub cells: Vec<u8>,
}

/// Which way a damaged block loses its cells.
///
/// Most ammunition shortens a platform horizontally, because a shrinking ledge reads
/// clearly to a player deciding whether they can still stand there. Penetrating weapons —
/// drills, heavy shells — instead punch through it, thinning the platform from the impact
/// side rather than narrowing it.
///
/// Stored on the block rather than passed per-hit: erosion is recomputed from health every
/// time (ADR 0005 §2), so the axis has to survive between hits or a later shell would undo
/// an earlier drill's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum ErosionAxis {
    /// Erode whole columns — the platform gets narrower. The default.
    #[default]
    Columns,
    /// Erode whole rows — the platform gets thinner. Penetrating ammunition.
    Rows,
}

impl ErosionAxis {
    /// Stable wire identifier. Never localize these.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Columns => "columns",
            Self::Rows => "rows",
        }
    }
}

/// An ordered, replicable terrain mutation.
///
/// Clients apply these in strict `sequence` order; a gap triggers recovery rather than
/// best-effort application (`PLATFORM_STRATEGY.md` §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerrainOperation {
    /// Monotonic per-match ordering key.
    pub sequence: u32,
    /// Shape and extent.
    pub shape: TerrainShape,
    /// Which materials this operation is permitted to remove. A material outside the
    /// mask is left intact.
    pub material_mask: MaterialMask,
}

/// The geometry of a terrain operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainShape {
    /// A circle centred at `center` — the normal explosion crater.
    SubtractCircle {
        /// Centre, fixed-point.
        center: FixedPoint,
        /// Radius in whole cells.
        radius_cells: u16,
    },
    /// A swept circle from `start` to `end` — digging and tunnelling.
    SubtractCapsule {
        /// Sweep start, fixed-point.
        start: FixedPoint,
        /// Sweep end, fixed-point.
        end: FixedPoint,
        /// Radius in whole cells.
        radius_cells: u16,
    },
}

/// A set of materials an operation may remove, as a bitmask over [`Material`] discriminants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialMask(pub u8);

impl MaterialMask {
    /// Soil and wood — what an ordinary explosion removes.
    pub const SOFT: Self = Self((1 << 1) | (1 << 2));
    /// Every destructible material, including reinforced stone. Breach Pick only.
    pub const ALL: Self = Self((1 << 1) | (1 << 2) | (1 << 3));

    /// Whether `material` is removable under this mask.
    #[must_use]
    pub const fn permits(self, material: Material) -> bool {
        self.0 & (1u8 << (material as u8)) != 0
    }
}

// ---------------------------------------------------------------------------
// Ballistics
// ---------------------------------------------------------------------------

/// One sampled point on a projectile's path, for client playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BallisticSample {
    /// Tick index from launch.
    pub tick: u32,
    /// Position at this tick.
    pub position: FixedPoint,
}

/// What a projectile terminated against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactCause {
    /// Struck solid terrain.
    Terrain,
    /// Struck a character.
    Character,
    /// Left the playable bounds.
    OutOfBounds,
    /// Reached `max_ticks` without contact.
    Expired,
}

/// The terminal event of a projectile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BallisticImpact {
    /// Where it terminated.
    pub position: FixedPoint,
    /// Tick of termination.
    pub tick: u32,
    /// Why it terminated.
    pub cause: ImpactCause,
}

/// Inputs to a trajectory integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BallisticInput {
    /// Launch position.
    pub origin: FixedPoint,
    /// Launch angle in millidegrees, measured counter-clockwise from world +X.
    /// Quantized at the protocol boundary; no client float reaches the simulation.
    pub angle_millidegrees: i32,
    /// Launch power in basis points of the weapon's `speed_per_tick`.
    pub power_basis_points: i32,
    /// Horizontal wind acceleration per tick, fixed-point.
    pub wind_per_tick: i32,
}

/// The result of integrating a trajectory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BallisticResult {
    /// Sampled path for client playback.
    pub samples: Vec<BallisticSample>,
    /// Terminal event.
    pub impact: BallisticImpact,
}

/// One independently playable projectile trajectory produced by a command.
///
/// A multi-strike ability must not concatenate several paths into one ambiguous stream or
/// discard all but its final impact. `sequence` is zero-based within the command and gives
/// the presentation layer a stable identity without making that identity authoritative
/// match state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectileTrace {
    /// Zero-based launch order within the command.
    pub sequence: u32,
    /// Authoritative sampled path for this projectile only.
    pub samples: Vec<BallisticSample>,
    /// This projectile's terminal event.
    pub impact: BallisticImpact,
}

// ---------------------------------------------------------------------------
// Players and match state
// ---------------------------------------------------------------------------

/// Purely cosmetic appearance.
///
/// **Never** contributes to the state hash, collision, reach, or any gameplay value
/// (`ARSENAL.md`, cosmetic boundary). Carried in state only so the renderer can read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Appearance {
    /// Character skin identifier.
    pub skin_id: String,
    /// Per-slot ability effect skin identifiers.
    pub ability_skin_ids: [String; 3],
    /// Victory pose identifier.
    pub victory_pose_id: String,
}

/// An active status effect on a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusEffect {
    /// Which effect.
    pub kind: EffectKind,
    /// Effect magnitude.
    pub magnitude: i32,
    /// Turns remaining. Reaching zero removes the effect.
    pub turns_remaining: u8,
}

/// A persistent object owned by a player — Emi's turret, Aleph's embedded knives.
///
/// Distinct from terrain: these are destructible entities with their own health and
/// lifetime, and they resolve in a deterministic order keyed on `sequence`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistentObject {
    /// Monotonic per-match creation order. Drives deterministic chain resolution.
    pub sequence: u32,
    /// Which player owns it.
    pub owner_id: String,
    /// What it is.
    pub kind: PersistentObjectKind,
    /// Where it sits.
    pub position: FixedPoint,
    /// Remaining health. Zero removes it.
    pub health: u16,
    /// Turns remaining before it expires. `u8::MAX` means it never expires on its own
    /// (Aleph's knives persist until triggered or displaced by the cap).
    pub turns_remaining: u8,
}

/// Kinds of persistent object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PersistentObjectKind {
    /// Emi's cube turret. Fires only in response to Emi's own attacks.
    Turret,
    /// One of Aleph's embedded throwing knives. Detonates when another lands nearby.
    EmbeddedKnife,
    /// Aleph's gas cloud. Blocks line of sight.
    GasCloud,
}

/// One character's authoritative state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    /// Opaque server-generated identifier. Never an email, external subject, or platform
    /// id (`SECURITY_BASELINE.md` §4).
    pub id: String,
    /// Team index. Distinct values are opponents; equal values are allies.
    pub team: u8,
    /// Current health. Zero means eliminated.
    pub health: u16,
    /// Maximum health, from the character definition plus any passive bonus. Stored
    /// rather than re-derived so a mid-match definition change cannot retroactively move
    /// a player's ceiling.
    pub max_health: u16,
    /// Position, fixed-point.
    pub position: FixedPoint,
    /// Which character is being played.
    pub character_id: String,
    /// The passive chosen on first gauge fill. [`None`] until that choice is made — the
    /// choice happens mid-match, not at selection (`CHARACTERS.md` §2).
    pub passive_id: Option<String>,
    /// Special gauge in hundredths, `0..=`[`GAUGE_FULL`].
    pub special_gauge: u16,
    /// Whether the gauge has ever been full this match. Gates the one-time passive
    /// prompt, so a player who fills, spends, and refills is not asked twice.
    pub has_chosen_passive: bool,
    /// Active statuses, kept sorted by `kind` for canonical encoding.
    pub statuses: Vec<StatusEffect>,
    /// Cosmetic only. Excluded from the state hash.
    pub appearance: Appearance,
}

impl PlayerState {
    /// Whether the special is available.
    #[must_use]
    pub const fn special_ready(&self) -> bool {
        self.special_gauge >= GAUGE_FULL
    }

    /// Whether this character is eliminated.
    #[must_use]
    pub const fn is_eliminated(&self) -> bool {
        self.health == 0
    }

    /// Adds gauge charge, applying both the per-action cap and the ceiling.
    ///
    /// Saturating throughout: an overflowing charge must clamp to full, never wrap to
    /// empty and rob a player of an earned special.
    pub const fn add_gauge(&mut self, amount: u16) {
        let capped = if amount > GAUGE_MAX_PER_ACTION {
            GAUGE_MAX_PER_ACTION
        } else {
            amount
        };
        let raised = self.special_gauge.saturating_add(capped);
        self.special_gauge = if raised > GAUGE_FULL {
            GAUGE_FULL
        } else {
            raised
        };
    }
}

/// Where a match is in its turn cycle (`PRODUCT_SPEC.md` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MatchPhase {
    /// Pre-match presentation.
    MatchIntro,
    /// Turn beginning; scheduled environment changes applied.
    TurnStart,
    /// Player may move within their allowance.
    Movement,
    /// Player selects ability, angle, and power.
    AimingAndSelection,
    /// The gauge just filled for the first time; the passive choice is pending. Blocks
    /// ability commands until resolved so the choice cannot be skipped by firing.
    PassiveSelection,
    /// Command accepted; input locked.
    CommandLocked,
    /// Ability resolving.
    Resolution,
    /// Bodies, debris, and persistent objects settling.
    Settling,
    /// Statuses and object lifetimes ticking.
    StatusResolution,
    /// Checking win conditions.
    VictoryCheck,
    /// Terminal.
    MatchComplete,
}

impl MatchPhase {
    /// Whether an ability command may be accepted in this phase.
    #[must_use]
    pub const fn accepts_ability_command(self) -> bool {
        matches!(self, Self::Movement | Self::AimingAndSelection)
    }
}

/// How a match ended, or that it has not.
///
/// Checked after every action and at each turn boundary. `Draw` exists because sudden
/// death can eliminate the last two players simultaneously — a match must always reach a
/// terminal state (`PRODUCT_SPEC.md` §2), and "nobody won" is a legal one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOutcome {
    /// Still playing.
    InProgress,
    /// One team remains.
    Victory {
        /// The surviving team index.
        team: u8,
    },
    /// No team remains, or the match hit its hard duration bound with no winner.
    Draw,
}

/// Why a turn ended.
///
/// Recorded so the result panel and replay can distinguish a committed attack from a
/// timeout, which look identical in the resulting state but mean very different things
/// about the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TurnEndReason {
    /// The player committed their one attack.
    Attacked,
    /// The player explicitly passed. The default: a turn that ends without anyone saying
    /// why was, behaviourally, a pass.
    #[default]
    Passed,
    /// The planning deadline expired and the server applied a timeout action.
    TimedOut,
    /// The active player was eliminated mid-turn.
    Eliminated,
}

/// The complete authoritative match state.
///
/// Everything needed to reproduce the match deterministically from a seed and a command
/// log. Nothing render-related lives here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationState {
    /// Simulation rules version this match runs under.
    pub simulation_version: u32,
    /// Content tables version.
    pub content_version: u32,
    /// Authoritative tick counter.
    pub tick: u64,
    /// Monotonic turn counter.
    pub turn_number: u32,
    /// Current phase.
    pub phase: MatchPhase,
    /// Whose turn it is. Empty when no player is active.
    pub active_player_id: String,
    /// Horizontal wind acceleration per tick, fixed-point. Held constant for at least a
    /// full turn (`PRODUCT_SPEC.md` §4).
    pub wind_per_tick: i32,
    /// Movement allowance remaining to the active player this turn, fixed-point.
    pub movement_remaining: i32,
    /// Whether the active player has already committed their attack this turn.
    pub has_attacked_this_turn: bool,
    /// Terrain occupancy. Derived from `blocks` wherever a block covers a cell.
    pub terrain: TerrainMask,
    /// Destructible blocks, kept sorted by `id` so iteration is deterministic.
    ///
    /// Block health is authoritative; the cells in `terrain` are recomputed from it
    /// (ADR 0005). A cell covered by no block is background terrain and is carved directly.
    pub blocks: Vec<TerrainBlock>,
    /// Players, kept sorted by `id` so iteration is deterministic.
    pub players: Vec<PlayerState>,
    /// Persistent objects, kept sorted by `sequence`.
    pub objects: Vec<PersistentObject>,
    /// Accepted command identifiers, sorted, for idempotent rejection of replays.
    pub processed_command_ids: Vec<String>,
    /// Next terrain operation sequence number.
    pub next_terrain_sequence: u32,
    /// Next persistent object sequence number.
    pub next_object_sequence: u32,
    /// PRNG state. Advanced only through the seeded generator.
    pub rng_state: u64,
    /// Why the turn currently being wound down is ending.
    ///
    /// Set by the orchestrator *before* it drives the scheduler, because the orchestrator is
    /// the only layer that knows whether a turn ended by attack, pass, or timeout. The
    /// scheduler reads it when it commits the turn. Separate from `last_turn_end_reason`
    /// because this is an intent and that is a record — conflating them would mean a
    /// half-driven turn reports a reason that never happened.
    pub pending_turn_end_reason: TurnEndReason,
    /// Why the most recently completed turn ended.
    ///
    /// Written by `scheduler::end_turn`, hashed like any other gameplay field. Turn-timeout
    /// rate is a named launch metric (`PRODUCT_SPEC.md` §10) and cannot be measured from
    /// state that never records it.
    pub last_turn_end_reason: TurnEndReason,
}

impl SimulationState {
    /// Finds a player by id.
    #[must_use]
    pub fn player(&self, id: &str) -> Option<&PlayerState> {
        self.players.iter().find(|player| player.id == id)
    }

    /// Finds a player by id, mutably.
    pub fn player_mut(&mut self, id: &str) -> Option<&mut PlayerState> {
        self.players.iter_mut().find(|player| player.id == id)
    }

    /// Whether `command_id` has already been accepted.
    ///
    /// `processed_command_ids` is sorted, so this is a binary search — replay checking
    /// stays cheap as a long match accumulates commands.
    #[must_use]
    pub fn has_processed(&self, command_id: &str) -> bool {
        self.processed_command_ids
            .binary_search_by(|existing| existing.as_str().cmp(command_id))
            .is_ok()
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// A client's intent to use an ability.
///
/// Every field is validated before use; nothing here is trusted
/// (`SECURITY_BASELINE.md` §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityCommand {
    /// Idempotency key, unique within the match.
    pub command_id: String,
    /// Claimed actor. Verified against the session, never trusted as given.
    pub player_id: String,
    /// Turn number the client believed it was acting on.
    pub expected_turn_number: u32,
    /// Which ability slot is being used.
    pub slot: AbilitySlot,
    /// Launch angle, millidegrees.
    pub angle_millidegrees: i32,
    /// Launch power, basis points.
    pub power_basis_points: i32,
    /// Target player, for abilities that select one (Zeke's heal, Huck's throw).
    pub target_player_id: Option<String>,
    /// Secondary target, for abilities needing two (Huck's Body Throw destination).
    pub secondary_target_player_id: Option<String>,
}

/// A client's choice of passive, made the first time their gauge fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassiveChoiceCommand {
    /// Idempotency key, unique within the match.
    pub command_id: String,
    /// Claimed actor. Verified against the session.
    pub player_id: String,
    /// Turn number the client believed it was acting on.
    pub expected_turn_number: u32,
    /// The chosen passive. Must belong to the player's character.
    pub passive_id: String,
}

/// Why a command was refused.
///
/// Categorized so the room can route gameplay errors to the player and authorization
/// failures to the security log (`SECURITY_BASELINE.md` §9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRejection {
    /// Already accepted. The original result is returned; nothing mutates.
    DuplicateCommand,
    /// Not this player's turn. **Security event.**
    NotActivePlayer,
    /// The match is not accepting this command in its current phase.
    WrongPhase,
    /// The client's expected turn number is stale or ahead.
    TurnVersionMismatch,
    /// The acting player's character has no ability in that slot. **Security event.**
    AbilityNotAvailable,
    /// The special was used without a full gauge.
    GaugeNotReady,
    /// The player has already attacked this turn.
    AlreadyAttacked,
    /// Angle or power outside the permitted range. **Security event.**
    InputOutOfRange,
    /// The acting player is eliminated.
    PlayerEliminated,
    /// A required target was absent, eliminated, or not a legal target.
    InvalidTarget,
    /// The character id is not in the roster. **Security event.**
    UnknownCharacter,
    /// The chosen passive does not belong to the player's character. **Security event.**
    InvalidPassive,
    /// A passive choice arrived when none was pending, or a second time.
    PassiveAlreadyChosen,
}

impl CommandRejection {
    /// Whether this rejection indicates a client acting outside the rules rather than
    /// making an ordinary mistake. These are logged on the security channel.
    #[must_use]
    pub const fn is_security_event(self) -> bool {
        matches!(
            self,
            Self::NotActivePlayer
                | Self::AbilityNotAvailable
                | Self::InputOutOfRange
                | Self::UnknownCharacter
                | Self::InvalidPassive
        )
    }
}

/// Damage applied to one character, itemized.
///
/// Itemization is a product requirement, not a debugging aid: the result panel and replay
/// must identify the elimination cause (`PRODUCT_SPEC.md` §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageEvent {
    /// Who was affected.
    pub player_id: String,
    /// Damage from an exact direct hit.
    pub direct: u16,
    /// Damage from radial falloff.
    pub splash: u16,
    /// Damage the acting player dealt to themselves (Backlash).
    pub backlash: u16,
    /// Damage from a world hazard.
    pub hazard: u16,
    /// Extra damage from being driven into terrain (Natomica's Repulse).
    pub wall_impact: u16,
    /// Health restored. Separate from damage so a result panel can show both.
    pub healed: u16,
    /// Whether this hit was a critical.
    pub was_critical: bool,
    /// Displacement applied.
    pub knockback: FixedPoint,
    /// Whether this event eliminated the character.
    pub eliminated: bool,
}

/// Why a strike consumed — or did not consume — a crit roll from the seeded generator.
///
/// Recorded per strike rather than inferred, because RNG consumption is part of the
/// authoritative state transition: an observer that guesses wrong about whether a roll
/// happened will desynchronise on the very next draw. `roll_crit` skips the draw entirely
/// when an ability cannot crit, so "did not crit" and "never rolled" are different facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CritRoll {
    /// The ability has `crit_chance_basis_points == 0`. No draw was taken.
    NotEligible,
    /// A draw was taken and lost.
    Missed,
    /// A draw was taken and won.
    Landed,
}

impl CritRoll {
    /// Whether this strike actually consumed a value from the match generator.
    #[must_use]
    pub const fn consumed_draw(self) -> bool {
        matches!(self, Self::Missed | Self::Landed)
    }

    /// Whether this strike resolved as a critical hit.
    #[must_use]
    pub const fn is_critical(self) -> bool {
        matches!(self, Self::Landed)
    }

    /// Stable wire identifier. Never localize these.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::NotEligible => "notEligible",
            Self::Missed => "missed",
            Self::Landed => "landed",
        }
    }
}

/// How an individual strike reached its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrikeDelivery {
    /// Delivered by a projectile, identified by its [`ProjectileTrace::sequence`].
    Projectile {
        /// The launch-order sequence of the trace that delivered this strike.
        trace_sequence: u32,
    },
    /// Delivered by a close-range strike with no projectile.
    Melee,
}

/// One authoritative strike resolution, recorded where it happened.
///
/// `CommandOutcome::damage` aggregates every hit on a player into one itemized total, which
/// is what a result panel wants but destroys per-strike facts a client needs to animate:
/// Karl's basic resolves three times with three independent crit rolls, and an aggregate
/// `was_critical` cannot say which of them landed. These records are emitted by the resolver
/// at the moment of resolution and are never reconstructed from final state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrikeResolution {
    /// Zero-based index within this command's strike sequence, in resolution order.
    pub strike_index: u16,
    /// Opaque id of the player this strike hit.
    pub target_player_id: String,
    /// Where the strike resolved, in fixed-point simulation units.
    pub impact_point: FixedPoint,
    /// How the strike was delivered.
    pub delivery: StrikeDelivery,
    /// The crit draw, including whether one was taken at all.
    pub crit: CritRoll,
    /// Damage this strike actually applied after clamping to remaining health.
    ///
    /// The *applied* figure, not the nominal one: a 60-damage strike against a target on
    /// 10 health applies 10, and a client showing 60 would be lying about a kill.
    pub damage_applied: u16,
    /// Whether this specific strike reduced the target to zero health.
    pub eliminated_target: bool,
}

/// The authoritative outcome of an accepted command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutcome {
    /// The command this resolves.
    pub command_id: String,
    /// Turn number before application.
    pub turn_number_before: u32,
    /// Turn number after application.
    pub turn_number_after: u32,
    /// Independently identifiable projectile paths in launch order; empty for strikes.
    pub projectile_traces: Vec<ProjectileTrace>,
    /// Terrain mutations, in sequence order.
    pub terrain_ops: Vec<TerrainOperation>,
    /// Damage and healing applied, sorted by `player_id`.
    pub damage: Vec<DamageEvent>,
    /// Persistent objects created by this action.
    pub objects_created: Vec<PersistentObject>,
    /// Every individual strike resolution, in the order the resolver produced them.
    ///
    /// Emitted at the point of resolution. A consumer must never reconstruct these from
    /// `damage` or from final state — the aggregate deliberately cannot express which of
    /// several strikes crit, and guessing would put a presentation layer's animation out of
    /// step with the authoritative RNG stream.
    pub strikes: Vec<StrikeResolution>,
    /// Gauge charge gained by the actor, in hundredths.
    pub gauge_gained: u16,
    /// Terrain cells removed, for telemetry and the Excavator XP bonus.
    pub terrain_cells_removed: u32,
    /// State hash after application, for divergence detection.
    pub final_state_hash: String,
}

/// The result of submitting a command: accepted with an outcome, or refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    /// Accepted and applied.
    Accepted(Box<CommandOutcome>),
    /// Refused. State is unchanged.
    Rejected(CommandRejection),
}
