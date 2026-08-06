//! Shared data contract for the simulation core.
//!
//! **This module is the interface every other module agrees on.** Behavior modules
//! (`terrain`, `weapon`, `ballistics`, `command`, `rng`, `scheduler`) operate on these
//! types; they do not define their own. Changing a type here is a cross-cutting change
//! and must be coordinated, not made locally.
//!
//! Naming follows `PRODUCT_SPEC.md` §3 (`main` / `secondary` / `meleeTool`), which
//! supersedes the `offHand` / `melee` names used by the TypeScript oracle. The oracle is
//! updated to match in the same change so parity is preserved (ADR 0001 §6).

use crate::fixed::FixedPoint;

// ---------------------------------------------------------------------------
// Loadout slots
// ---------------------------------------------------------------------------

/// The three mutually exclusive equipment slots. Every character fills all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WeaponSlot {
    /// Signature map-scale artillery with a defining special effect.
    Main,
    /// Faster or more precise backup: bows, handguns, boomerang, longsword.
    Secondary,
    /// Short-range attack or terrain tool.
    MeleeTool,
}

impl WeaponSlot {
    /// All slots in canonical order. Iteration order is fixed for hashing.
    pub const ALL: [Self; 3] = [Self::Main, Self::Secondary, Self::MeleeTool];

    /// The stable wire identifier for this slot.
    ///
    /// Used in the canonical encoding and the protocol. Never localize these.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Secondary => "secondary",
            Self::MeleeTool => "meleeTool",
        }
    }
}

/// How a weapon's charges behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmmoPolicy {
    /// A finite server-owned charge count, decremented once per accepted attack.
    Finite {
        /// Charges granted at match start.
        capacity: u16,
    },
    /// Never decrements.
    ///
    /// The Longsword is the **only** weapon permitted this policy
    /// (`ARSENAL.md`, Longsword invariant). Roster validation enforces it.
    Infinite,
}

// ---------------------------------------------------------------------------
// Weapon definitions
// ---------------------------------------------------------------------------

/// When a special effect fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectTrigger {
    /// At the moment the attack is committed.
    OnFire,
    /// During projectile flight.
    OnFlight,
    /// On impact with terrain or a character.
    OnImpact,
    /// At the end of the acting player's turn.
    OnTurnEnd,
}

/// The reviewed vocabulary of special effects.
///
/// This is a **closed set**. Weapon definitions are data that reference these
/// identifiers; there is no scripting language and no dynamically loaded behavior
/// (`SECURITY_BASELINE.md` §6). Adding a variant requires a client and server build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Displaces the *shooter* opposite the shot.
    Recoil,
    /// Damages the acting player. Surfaced in the UI as **Backlash**, previewed before
    /// commitment, resolved simultaneously with target damage, and never hideable by a
    /// skin (`PRODUCT_SPEC.md` §3).
    SelfDamage,
}

/// A special effect attached to a weapon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpecialEffect {
    /// When it fires.
    pub trigger: EffectTrigger,
    /// What it does.
    pub kind: EffectKind,
    /// Effect-specific magnitude, in the unit natural to `kind` (fixed-point distance
    /// for displacement, hit points for damage, cell counts for terrain).
    pub magnitude: i32,
    /// How many of the victim's turns it persists. Status effects last at most one
    /// affected turn and never stack (`ARSENAL.md` guardrail 4).
    pub duration_turns: u8,
}

/// How a projectile alters terrain on detonation.
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
    /// Wind response, in basis points. `0` means wind-immune (the 5.7 Service Pistol);
    /// higher values mean stronger drift (the Recurve Bow).
    pub wind_scale_basis_points: i32,
    /// Hard lifetime cap. Bounds worst-case server work per shot and guarantees a shot
    /// always terminates (`PRODUCT_SPEC.md` §2: 8-second unresolved projectile limit).
    pub max_ticks: u16,
    /// Terrain effect on detonation.
    pub terrain: TerrainProfile,
}

/// A close-range attack resolved without a flying projectile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrikeAttack {
    /// Reach in fixed-point units. Standard is [`crate::fixed::BASE_MELEE_RANGE`]
    /// (1.25 BW); the Longsword is exactly twice that.
    pub range: i32,
    /// Terrain effect, for digging and breaching tools.
    pub terrain: TerrainProfile,
    /// Backlash dealt to the acting player. Bypasses ordinary shields and can eliminate
    /// its user (`ARSENAL.md` guardrail 6).
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

/// Maximum special effects on one weapon. Bounds the definition size so a hostile or
/// malformed content pack cannot produce unbounded per-impact work.
pub const MAX_SPECIAL_EFFECTS: usize = 4;

/// A complete, versioned weapon definition.
///
/// Definitions are immutable once used by a completed match. Balance changes publish a
/// new version rather than mutating in place (`PLATFORM_STRATEGY.md` §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponDefinition {
    /// Stable identifier, e.g. `"ramshot-cannon"`.
    pub id: &'static str,
    /// Definition version.
    pub version: u32,
    /// Player-facing name.
    pub display_name: &'static str,
    /// The one slot this weapon occupies.
    pub slot: WeaponSlot,
    /// Charge behavior.
    pub ammo: AmmoPolicy,
    /// Action points consumed. Firing also ends the turn.
    pub action_point_cost: u8,
    /// Damage on an exact direct hit.
    pub base_damage: u16,
    /// Attack shape and parameters.
    pub attack: Attack,
    /// Special effects, in declaration order.
    pub special_effects: &'static [SpecialEffect],
    /// Whether this weapon is legal in rated modes.
    pub ranked_enabled: bool,
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

// ---------------------------------------------------------------------------
// Players and match state
// ---------------------------------------------------------------------------

/// A player's three equipped weapons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loadout {
    /// Main-slot weapon id.
    pub main: String,
    /// Secondary-slot weapon id.
    pub secondary: String,
    /// Melee/tool-slot weapon id.
    pub melee_tool: String,
}

impl Loadout {
    /// The equipped id for `slot`.
    #[must_use]
    pub fn slot(&self, slot: WeaponSlot) -> &str {
        match slot {
            WeaponSlot::Main => &self.main,
            WeaponSlot::Secondary => &self.secondary,
            WeaponSlot::MeleeTool => &self.melee_tool,
        }
    }
}

/// Purely cosmetic appearance.
///
/// **Never** contributes to the state hash, collision, reach, or any gameplay value
/// (`ARSENAL.md`, cosmetic boundary). Carried in state only so the renderer can read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Appearance {
    /// Body rig identifier.
    pub body_id: String,
    /// Outfit identifier.
    pub outfit_id: String,
    /// Per-slot weapon skin identifiers.
    pub weapon_skin_ids: [String; 3],
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

/// A charge counter for one equipped weapon.
///
/// [`None`] means the weapon's policy is [`AmmoPolicy::Infinite`] and it has no counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmmoCounter {
    /// Remaining charges, or [`None`] for infinite.
    pub remaining: Option<u16>,
}

/// One character's authoritative state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerState {
    /// Opaque server-generated identifier. Never an email, external subject, or
    /// platform id (`SECURITY_BASELINE.md` §4).
    pub id: String,
    /// Team index. Distinct values are opponents; equal values are allies.
    pub team: u8,
    /// Current health. Zero means eliminated.
    pub health: u16,
    /// Position, fixed-point.
    pub position: FixedPoint,
    /// Equipped weapons.
    pub loadout: Loadout,
    /// Charges per slot, indexed parallel to [`WeaponSlot::ALL`].
    pub ammo: [AmmoCounter; 3],
    /// Active statuses, kept sorted by `kind` discriminant for canonical encoding.
    pub statuses: Vec<StatusEffect>,
    /// Cosmetic only. Excluded from the state hash.
    pub appearance: Appearance,
}

/// Where a match is in its turn cycle (`PRODUCT_SPEC.md` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchPhase {
    /// Pre-match presentation.
    MatchIntro,
    /// Turn beginning; scheduled environment changes applied.
    TurnStart,
    /// Player may move within the AP budget.
    Movement,
    /// Player selects weapon, angle, and power.
    AimingAndSelection,
    /// Command accepted; input locked.
    CommandLocked,
    /// Projectile or melee resolving.
    Resolution,
    /// Bodies and debris settling.
    Settling,
    /// Statuses ticking.
    StatusResolution,
    /// Checking win conditions.
    VictoryCheck,
    /// Terminal.
    MatchComplete,
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
    /// Action points remaining to the active player this turn.
    pub action_points_remaining: u8,
    /// Terrain occupancy.
    pub terrain: TerrainMask,
    /// Players, kept sorted by `id` so iteration is deterministic.
    pub players: Vec<PlayerState>,
    /// Accepted command identifiers, sorted, for idempotent rejection of replays.
    pub processed_command_ids: Vec<String>,
    /// Next terrain operation sequence number.
    pub next_terrain_sequence: u32,
    /// PRNG state. Advanced only through the seeded generator.
    pub rng_state: u64,
}

impl SimulationState {
    /// Finds a player by id.
    #[must_use]
    pub fn player(&self, id: &str) -> Option<&PlayerState> {
        self.players.iter().find(|player| player.id == id)
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

/// A client's intent to act.
///
/// Every field is validated before use; nothing here is trusted
/// (`SECURITY_BASELINE.md` §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponCommand {
    /// Idempotency key, unique within the match.
    pub command_id: String,
    /// Claimed actor. Verified against the session, never trusted as given.
    pub player_id: String,
    /// State version the client believed it was acting on.
    pub expected_turn_number: u32,
    /// Which equipped slot is firing.
    pub slot: WeaponSlot,
    /// Weapon id, cross-checked against the equipped loadout.
    pub weapon_id: String,
    /// Launch angle, millidegrees.
    pub angle_millidegrees: i32,
    /// Launch power, basis points.
    pub power_basis_points: i32,
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
    /// The match is not accepting commands in its current phase.
    WrongPhase,
    /// The client's expected turn number is stale or ahead.
    TurnVersionMismatch,
    /// The named weapon is not equipped in the named slot. **Security event.**
    WeaponNotEquipped,
    /// No charges remain.
    OutOfAmmo,
    /// Insufficient action points.
    InsufficientActionPoints,
    /// Angle or power outside the permitted range. **Security event.**
    InputOutOfRange,
    /// The acting player is eliminated.
    PlayerEliminated,
    /// The weapon id is not in the roster. **Security event.**
    UnknownWeapon,
}

impl CommandRejection {
    /// Whether this rejection indicates a client acting outside the rules rather than
    /// making an ordinary mistake. These are logged on the security channel.
    #[must_use]
    pub const fn is_security_event(self) -> bool {
        matches!(
            self,
            Self::NotActivePlayer
                | Self::WeaponNotEquipped
                | Self::InputOutOfRange
                | Self::UnknownWeapon
        )
    }
}

/// Damage applied to one character, itemized.
///
/// Itemization is a product requirement, not a debugging aid: the result panel and replay
/// must identify the elimination cause (`PRODUCT_SPEC.md` §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageEvent {
    /// Who was damaged.
    pub player_id: String,
    /// Damage from an exact direct hit.
    pub direct: u16,
    /// Damage from radial falloff.
    pub splash: u16,
    /// Damage the acting player dealt to themselves (Backlash).
    pub backlash: u16,
    /// Damage from a world hazard.
    pub hazard: u16,
    /// Displacement applied.
    pub knockback: FixedPoint,
    /// Whether this event eliminated the character.
    pub eliminated: bool,
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
    /// Sampled projectile path, empty for strikes.
    pub samples: Vec<BallisticSample>,
    /// Terminal projectile event, absent for strikes.
    pub impact: Option<BallisticImpact>,
    /// Terrain mutations, in sequence order.
    pub terrain_ops: Vec<TerrainOperation>,
    /// Damage applied, sorted by `player_id`.
    pub damage: Vec<DamageEvent>,
    /// Charges consumed.
    pub ammo_consumed: u16,
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
