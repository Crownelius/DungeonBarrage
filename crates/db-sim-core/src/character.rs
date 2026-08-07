//! Character roster data and validation.
//!
//! Defines the nine starter characters and the associated validation logic.
//! Per ADR 0002, characters replace the three-slot loadout; each character
//! carries a fixed kit of abilities and passives.

use crate::error::{CharacterRejection, SimError, SimResult};
use crate::fixed::{BODY_WIDTH, POSITION_SCALE};
use crate::types::*;
use std::collections::HashSet;

// Derived ballistic values by range tier. NOT specified in CHARACTERS.md; every value
// below is a judgement call for review/tuning once ballistics.rs lands.
//
// For a simple per-tick Euler integration (position += speed_per_tick horizontally,
// downward velocity += gravity_per_tick each tick), maximum range at a 45-degree launch
// is speed^2 / gravity, and worst-case flight time (a near-vertical launch) is
// 2 * speed / gravity. The previous derivation (Tier1 500/30, Tier2 700/20, Tier3 900/15)
// computed to only 8_333 / 24_500 / 54_000 respectively at 45 degrees — 25-40% of each
// tier's actual reach (32_768 / 65_536 / 106_496) — so a character firing at max power
// could never hit a target anywhere near their advertised range. The values below target
// roughly 1.3-1.8x headroom over reach at 45 degrees, with max_ticks kept comfortably
// above the worst-case (vertical-launch) flight time so a high arc cannot expire early:
// Tier 1 (reach 32768): speed 900, gravity 20 -> 40_500 at 45 deg (1.24x); flight time
//   caps at 90 ticks (vertical); max_ticks 140.
// Tier 2 (reach 65536): speed 1200, gravity 16 -> 90_000 at 45 deg (1.37x); flight time
//   caps at 150 ticks (vertical); max_ticks 200.
// Tier 3 (reach 106496): speed 1500, gravity 12 -> 187_500 at 45 deg (1.76x); flight time
//   caps at 250 ticks (vertical); max_ticks 300.
const PROJECTILE_TIER1_SPEED: i32 = 900;
const PROJECTILE_TIER1_GRAVITY: i32 = 20;
const PROJECTILE_TIER1_MAX_TICKS: u16 = 140;

const PROJECTILE_TIER2_SPEED: i32 = 1_200;
const PROJECTILE_TIER2_GRAVITY: i32 = 16;
const PROJECTILE_TIER2_MAX_TICKS: u16 = 200;

const PROJECTILE_TIER3_SPEED: i32 = 1_500;
const PROJECTILE_TIER3_GRAVITY: i32 = 12;
const PROJECTILE_TIER3_MAX_TICKS: u16 = 300;

// Common BW-based radii in cells (1 BW = 4 cells).
const RADIUS_2_0_BW: u16 = 8; // 2.0 BW in cells

// Arzum abilities
const ARZUM_LUNGE: AbilityDefinition = AbilityDefinition {
    id: "arzum-lunge",
    display_name: "Lunge",
    slot: AbilitySlot::Basic,
    damage_percent: 45,
    crit_damage_percent: 45,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER1_SPEED,
        gravity_per_tick: PROJECTILE_TIER1_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER1_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[],
};

const ARZUM_CHAIN_STRIKE: AbilityDefinition = AbilityDefinition {
    id: "arzum-chain-strike",
    display_name: "Chain Strike",
    slot: AbilitySlot::Special,
    // First hit: fixed, non-critical baseline (CHARACTERS.md gives no number for
    // "attacks the target"; 50 matches the second hit's roll floor below).
    damage_percent: 50,
    crit_damage_percent: 50,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Tier1.reach(),
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    effects: &[ARZUM_CHAIN_STRIKE_TELEPORT],
};

// Per the brief: magnitude = 50, magnitude_secondary = 200 encodes the second hit's
// 50..=200 percent damage roll. That claims both of SpecialEffect's numeric slots, so the
// "random nearby enemy... within 12 BW of the first target" search radius from
// CHARACTERS.md §3.1 is NOT separately encoded here — it is a resolver-side constant
// (12 * BODY_WIDTH) until the data model grows a third field.
const ARZUM_CHAIN_STRIKE_TELEPORT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::Teleport,
    magnitude: 50,            // Second hit damage range lower bound (50-200%)
    magnitude_secondary: 200, // Second hit damage range upper bound (50-200%)
    duration_turns: 0,
};

const ARZUM_MOMENTUM: PassiveDefinition = PassiveDefinition {
    id: "arzum-momentum",
    display_name: "Momentum",
    kind: PassiveKind::BonusMovement,
    magnitude: 2 * BODY_WIDTH,
};

const ARZUM_CHAIN_REACTION: PassiveDefinition = PassiveDefinition {
    id: "arzum-chain-reaction",
    display_name: "Chain Reaction",
    // Placeholder: "rolls twice, taking the higher" keeps the roll random (unlike
    // Aleph's Mistwalker, which genuinely removes a random destination); RemoveRandomness
    // is the closest fit in the closed PassiveKind set but does not model advantage-rolls.
    kind: PassiveKind::RemoveRandomness,
    magnitude: 1,
};

const ARZUM_LIGHTFOOT: PassiveDefinition = PassiveDefinition {
    id: "arzum-lightfoot",
    display_name: "Lightfoot",
    // Placeholder: "no fall damage" has no equivalent in the closed PassiveKind set.
    kind: PassiveKind::DamageReduction,
    magnitude: 0,
};

const ARZUM_PASSIVES: &[PassiveDefinition] =
    &[ARZUM_MOMENTUM, ARZUM_CHAIN_REACTION, ARZUM_LIGHTFOOT];

// Emi abilities
const EMI_HEAVY_CUBE: AbilityDefinition = AbilityDefinition {
    id: "emi-heavy-cube",
    display_name: "Heavy Cube",
    slot: AbilitySlot::Basic,
    damage_percent: 50,
    crit_damage_percent: 50,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 2_000, // Lowest wind response in roster
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[],
};

const EMI_CUBE_TURRET: AbilityDefinition = AbilityDefinition {
    id: "emi-cube-turret",
    display_name: "Cube Turret",
    slot: AbilitySlot::Special,
    damage_percent: 0,
    crit_damage_percent: 0,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 2_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[EMI_TURRET_SPAWN],
};

const EMI_TURRET_SPAWN: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::SpawnTurret,
    magnitude: 3, // Turret lasts 3 turns
    magnitude_secondary: 0,
    duration_turns: 3,
};

const EMI_REINFORCED_CASING: PassiveDefinition = PassiveDefinition {
    id: "emi-reinforced-casing",
    display_name: "Reinforced Casing",
    kind: PassiveKind::BonusDuration,
    magnitude: 1,
};

const EMI_DENSE_PAYLOAD: PassiveDefinition = PassiveDefinition {
    id: "emi-dense-payload",
    display_name: "Dense Payload",
    // Placeholder: "basic attack ignores wind entirely" has no equivalent in the closed
    // PassiveKind set.
    kind: PassiveKind::RemoveRandomness,
    magnitude: 1,
};

const EMI_OVERWATCH: PassiveDefinition = PassiveDefinition {
    id: "emi-overwatch",
    display_name: "Overwatch",
    kind: PassiveKind::BonusRadius,
    magnitude: 4 * BODY_WIDTH,
};

const EMI_PASSIVES: &[PassiveDefinition] =
    &[EMI_REINFORCED_CASING, EMI_DENSE_PAYLOAD, EMI_OVERWATCH];

// Karl abilities
const KARL_CARRION_CALL: AbilityDefinition = AbilityDefinition {
    id: "karl-carrion-call",
    display_name: "Carrion Call",
    slot: AbilitySlot::Basic,
    damage_percent: 24,
    crit_damage_percent: 74,
    // NOT specified in CHARACTERS.md: the doc states the 24%/74% figures and that "each
    // of the three attacks rolls its crit independently" but never gives a chance. A
    // naive 50% would push Karl's expected damage to ~147%/turn against a stated 72%
    // uncrit baseline — wildly overtuned relative to the doc's framing of Karl as merely
    // "the highest sustained damage of the starters." 20% keeps the expected turn total
    // (~94.5%) a modest step above the uncrit floor. Needs designer confirmation.
    crit_chance_basis_points: 2_000,
    strikes_per_turn: 3,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER1_SPEED,
        gravity_per_tick: PROJECTILE_TIER1_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER1_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[],
};

const KARL_FEEDING_FRENZY: AbilityDefinition = AbilityDefinition {
    id: "karl-feeding-frenzy",
    display_name: "Feeding Frenzy",
    slot: AbilitySlot::Special,
    damage_percent: 0,
    crit_damage_percent: 0,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Tier1.reach(),
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    effects: &[KARL_GUARANTEE_CRIT],
};

const KARL_GUARANTEE_CRIT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::GuaranteeCrit,
    magnitude: 3, // Next 3 attacks crit automatically
    magnitude_secondary: 0,
    duration_turns: 0,
};

const KARL_PACK_LEADER: PassiveDefinition = PassiveDefinition {
    id: "karl-pack-leader",
    display_name: "Pack Leader",
    kind: PassiveKind::BonusCritChance,
    magnitude: 1_500, // +15%
};

const KARL_THICK_HIDE: PassiveDefinition = PassiveDefinition {
    id: "karl-thick-hide",
    display_name: "Thick Hide",
    kind: PassiveKind::DamageReduction,
    magnitude: 1_000, // 10% reduction (basis points)
};

const KARL_SCAVENGER: PassiveDefinition = PassiveDefinition {
    id: "karl-scavenger",
    display_name: "Scavenger",
    kind: PassiveKind::LifestealFlat,
    magnitude: 8,
};

const KARL_PASSIVES: &[PassiveDefinition] = &[KARL_PACK_LEADER, KARL_THICK_HIDE, KARL_SCAVENGER];

// Huck abilities
const HUCK_HAYMAKER: AbilityDefinition = AbilityDefinition {
    id: "huck-haymaker",
    display_name: "Haymaker",
    slot: AbilitySlot::Basic,
    damage_percent: 60,
    crit_damage_percent: 60,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Melee.reach(),
        terrain: TerrainProfile::Crater {
            radius_cells: RADIUS_2_0_BW,
        },
        self_damage: 0,
    }),
    effects: &[],
};

const HUCK_BODY_THROW: AbilityDefinition = AbilityDefinition {
    id: "huck-body-throw",
    display_name: "Body Throw",
    slot: AbilitySlot::Special,
    damage_percent: 40, // First target takes 40%; second takes 25% from effect
    crit_damage_percent: 40,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Melee.reach(),
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    effects: &[HUCK_RELOCATE],
};

const HUCK_RELOCATE: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::Relocate,
    magnitude: 25, // Second target takes 25% damage
    magnitude_secondary: 0,
    duration_turns: 0,
};

const HUCK_IMMOVABLE: PassiveDefinition = PassiveDefinition {
    id: "huck-immovable",
    display_name: "Immovable",
    kind: PassiveKind::DisplacementImmunity,
    magnitude: 1,
};

const HUCK_DEMOLITION: PassiveDefinition = PassiveDefinition {
    id: "huck-demolition",
    display_name: "Demolition",
    kind: PassiveKind::BonusRadius,
    // BonusRadius is an additive bonus (see Emi's Overwatch), not a replacement value.
    // Haymaker radius 2.0 -> 3.0 BW is a +1.0 BW bonus.
    magnitude: BODY_WIDTH,
};

const HUCK_FOLLOW_THROUGH: PassiveDefinition = PassiveDefinition {
    id: "huck-follow-through",
    display_name: "Follow Through",
    kind: PassiveKind::DamageBonus,
    magnitude: 2_000, // Body Throw +20% to both targets
};

const HUCK_PASSIVES: &[PassiveDefinition] = &[HUCK_IMMOVABLE, HUCK_DEMOLITION, HUCK_FOLLOW_THROUGH];

// Numa abilities
const NUMA_HARPOON: AbilityDefinition = AbilityDefinition {
    id: "numa-harpoon",
    display_name: "Harpoon",
    slot: AbilitySlot::Basic,
    damage_percent: 42,
    crit_damage_percent: 42,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER3_SPEED,
        gravity_per_tick: PROJECTILE_TIER3_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER3_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[NUMA_HARPOON_PULL],
};

const NUMA_HARPOON_PULL: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Pull,
    magnitude: 1,               // Pulls toward/from target based on target health
    magnitude_secondary: 5_000, // 50% HP threshold, basis points (10_000 == 100%)
    duration_turns: 0,
};

const NUMA_PIN: AbilityDefinition = AbilityDefinition {
    id: "numa-pin",
    display_name: "Pin",
    slot: AbilitySlot::Special,
    damage_percent: 0,
    crit_damage_percent: 0,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Tier3.reach(),
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    effects: &[NUMA_LOCKDOWN],
};

const NUMA_LOCKDOWN: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::Lockdown,
    magnitude: 2, // Lasts 2 turns
    magnitude_secondary: 0,
    duration_turns: 2,
};

const NUMA_EXECUTIONER: PassiveDefinition = PassiveDefinition {
    id: "numa-executioner",
    display_name: "Executioner",
    kind: PassiveKind::DamageBonus,
    magnitude: 3_000, // +30% damage vs low-HP targets
};

const NUMA_BARBED_LINE: PassiveDefinition = PassiveDefinition {
    id: "numa-barbed-line",
    display_name: "Barbed Line",
    kind: PassiveKind::BonusDuration,
    magnitude: 1, // Pin lasts 3 turns instead of 2
};

const NUMA_GHOST: PassiveDefinition = PassiveDefinition {
    id: "numa-ghost",
    display_name: "Ghost",
    kind: PassiveKind::RemoveRandomness,
    magnitude: 1,
};

const NUMA_PASSIVES: &[PassiveDefinition] = &[NUMA_EXECUTIONER, NUMA_BARBED_LINE, NUMA_GHOST];

// Aleph abilities
const ALEPH_BOW: AbilityDefinition = AbilityDefinition {
    id: "aleph-bow",
    display_name: "Bow",
    slot: AbilitySlot::Basic,
    damage_percent: 20,
    crit_damage_percent: 20,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[],
};

const ALEPH_THROWING_KNIFE: AbilityDefinition = AbilityDefinition {
    id: "aleph-throwing-knife",
    display_name: "Throwing Knife",
    slot: AbilitySlot::BasicAlt,
    damage_percent: 60,
    crit_damage_percent: 60,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[ALEPH_EMBED_KNIFE, ALEPH_CHAIN_DETONATE],
};

const ALEPH_EMBED_KNIFE: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::EmbedProjectile,
    magnitude: 6 * POSITION_SCALE, // 1.5 BW: proximity to an embedded knife that triggers a chain
    magnitude_secondary: 8,        // Embedded knife cap; the oldest is removed when a ninth lands
    duration_turns: u8::MAX,       // Persists indefinitely until detonated or capped
};

// Aleph's dagger chain (CHARACTERS.md §3.6): a second knife landing within 1.5 BW of an
// embedded one detonates both for 70% damage in a 2.5 BW radius and removes terrain in
// that same radius; a detonation reaching another embedded knife triggers it too, in
// deterministic ascending-embed-sequence order (resolver responsibility, not data).
const ALEPH_CHAIN_DETONATE: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::ChainDetonate,
    magnitude: 70,                            // 70% damage, percent-of-BASE_ATTACK
    magnitude_secondary: 10 * POSITION_SCALE, // 2.5 BW: detonation damage + terrain-removal radius
    duration_turns: 0,
};

const ALEPH_VEILSTEP: AbilityDefinition = AbilityDefinition {
    id: "aleph-veilstep",
    display_name: "Veilstep",
    slot: AbilitySlot::Special,
    damage_percent: 0,
    crit_damage_percent: 0,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: 8 * BODY_WIDTH, // 8 BW gas cloud radius
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    effects: &[ALEPH_OBSCURE, ALEPH_RANDOM_TELEPORT],
};

const ALEPH_OBSCURE: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::Obscure,
    magnitude: 8 * BODY_WIDTH, // 8 BW radius
    magnitude_secondary: 0,
    duration_turns: 2,
};

const ALEPH_RANDOM_TELEPORT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::Teleport,
    magnitude: 8 * BODY_WIDTH, // Random destination within 8 BW gas cloud
    magnitude_secondary: 0,
    duration_turns: 0,
};

const ALEPH_VOLATILE: PassiveDefinition = PassiveDefinition {
    id: "aleph-volatile",
    display_name: "Volatile",
    kind: PassiveKind::DamageBonus,
    // DamageBonus is a direct additive percentage (see Huck's Follow Through, Numa's
    // Executioner): the doc's 70% -> 95% is a flat +25 percentage points, i.e. 2_500 bp.
    magnitude: 2_500,
};

const ALEPH_DEEP_QUIVER: PassiveDefinition = PassiveDefinition {
    id: "aleph-deep-quiver",
    display_name: "Deep Quiver",
    kind: PassiveKind::BonusObjectCap,
    magnitude: 4, // Embedded knife cap 8 → 12
};

const ALEPH_MISTWALKER: PassiveDefinition = PassiveDefinition {
    id: "aleph-mistwalker",
    display_name: "Mistwalker",
    kind: PassiveKind::RemoveRandomness,
    magnitude: 1,
};

const ALEPH_PASSIVES: &[PassiveDefinition] = &[ALEPH_VOLATILE, ALEPH_DEEP_QUIVER, ALEPH_MISTWALKER];

// Zeke abilities
const ZEKE_MENDING_BOLT: AbilityDefinition = AbilityDefinition {
    id: "zeke-mending-bolt",
    display_name: "Mending Bolt",
    slot: AbilitySlot::Basic,
    damage_percent: 41,
    crit_damage_percent: 41,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER3_SPEED,
        gravity_per_tick: PROJECTILE_TIER3_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER3_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[ZEKE_HEAL],
};

const ZEKE_HEAL: SpecialEffect = SpecialEffect {
    // Mending Bolt is a projectile; the doc says it deals damage and heals
    // "simultaneously", which ties the heal to the bolt actually landing, not to Zeke
    // committing the cast. Matches the OnImpact convention used by every other
    // projectile-attached effect in this file (Numa's pull, Emi's turret, Aleph's embed,
    // Natomica's self-pull) — a miss or terrain block should not grant a free heal.
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Heal,
    magnitude: 22, // Heals chosen ally for 22 HP; Zeke gets 11 HP in FFA
    magnitude_secondary: 0,
    duration_turns: 0,
};

const ZEKE_LIFESHARE: AbilityDefinition = AbilityDefinition {
    id: "zeke-lifeshare",
    display_name: "Lifeshare",
    slot: AbilitySlot::Special,
    damage_percent: 0,
    crit_damage_percent: 0,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Tier3.reach(),
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    effects: &[ZEKE_TRANSFER_HEAL],
};

const ZEKE_TRANSFER_HEAL: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::HealthTransfer,
    magnitude: 100, // Transfer up to 100 HP, or restore 100 HP to self
    magnitude_secondary: 0,
    duration_turns: 0,
};

const ZEKE_FIELD_MEDIC: PassiveDefinition = PassiveDefinition {
    id: "zeke-field-medic",
    display_name: "Field Medic",
    kind: PassiveKind::HealingBonus,
    magnitude: 4_000, // +40% healing effectiveness
};

const ZEKE_TRANSFUSION: PassiveDefinition = PassiveDefinition {
    id: "zeke-transfusion",
    display_name: "Transfusion",
    kind: PassiveKind::DamageReduction, // Placeholder: Transfusion has no direct PassiveKind equivalent
    magnitude: 0,
};

const ZEKE_GUARDIAN: PassiveDefinition = PassiveDefinition {
    id: "zeke-guardian",
    display_name: "Guardian",
    kind: PassiveKind::DamageReduction,
    magnitude: 1_000, // 10% damage reduction to nearby allies
};

const ZEKE_PASSIVES: &[PassiveDefinition] = &[ZEKE_FIELD_MEDIC, ZEKE_TRANSFUSION, ZEKE_GUARDIAN];

// Roberto abilities
const ROBERTO_BOUNCING_GRENADE: AbilityDefinition = AbilityDefinition {
    id: "roberto-bouncing-grenade",
    display_name: "Bouncing Grenade",
    slot: AbilitySlot::Basic,
    damage_percent: 55,
    crit_damage_percent: 55,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 1, // Bounces exactly once
        terrain: TerrainProfile::None,
    }),
    effects: &[],
};

const ROBERTO_HEAVY_ORDNANCE: AbilityDefinition = AbilityDefinition {
    id: "roberto-heavy-ordnance",
    display_name: "Heavy Ordnance",
    slot: AbilitySlot::Special,
    damage_percent: 65,
    crit_damage_percent: 65,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::Crater {
            radius_cells: 16, // Larger crater than basic
        },
    }),
    effects: &[],
};

const ROBERTO_DOUBLE_BOUNCE: PassiveDefinition = PassiveDefinition {
    id: "roberto-double-bounce",
    display_name: "Double Bounce",
    // Placeholder: "bounces twice instead of once" is a bonus to ProjectileAttack.bounces,
    // not a turn duration. BonusDuration is the least-wrong fit in the closed PassiveKind
    // set, but a resolver must not read `magnitude` as extra turns here.
    kind: PassiveKind::BonusDuration,
    magnitude: 1,
};

const ROBERTO_BLAST_SHIELDING: PassiveDefinition = PassiveDefinition {
    id: "roberto-blast-shielding",
    display_name: "Blast Shielding",
    kind: PassiveKind::SelfDamageImmunity,
    magnitude: 1,
};

const ROBERTO_SHRAPNEL: PassiveDefinition = PassiveDefinition {
    id: "roberto-shrapnel",
    display_name: "Shrapnel",
    kind: PassiveKind::BonusDuration, // Represents 2-turn contact zone duration
    magnitude: 2,
};

const ROBERTO_PASSIVES: &[PassiveDefinition] = &[
    ROBERTO_DOUBLE_BOUNCE,
    ROBERTO_BLAST_SHIELDING,
    ROBERTO_SHRAPNEL,
];

// Natomica abilities
const NATOMICA_CLAYMORE_HOOK: AbilityDefinition = AbilityDefinition {
    id: "natomica-claymore-hook",
    display_name: "Claymore Hook",
    slot: AbilitySlot::Basic,
    damage_percent: 48,
    crit_damage_percent: 48,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[NATOMICA_PULL_SELF],
};

const NATOMICA_PULL_SELF: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Pull,
    magnitude: 1, // Pulls Natomica toward the target
    magnitude_secondary: 0,
    duration_turns: 0,
};

const NATOMICA_REPULSE: AbilityDefinition = AbilityDefinition {
    id: "natomica-repulse",
    display_name: "Repulse",
    slot: AbilitySlot::Special,
    damage_percent: 55,
    crit_damage_percent: 55,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: 10 * BODY_WIDTH, // 10 BW radius for push
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    effects: &[NATOMICA_PUSH, NATOMICA_WALL_IMPACT],
};

const NATOMICA_PUSH: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Push,
    magnitude: 10 * BODY_WIDTH,
    magnitude_secondary: 0,
    duration_turns: 0,
};

const NATOMICA_WALL_IMPACT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::WallImpact,
    magnitude: 25, // +25% damage on terrain impact
    magnitude_secondary: 0,
    duration_turns: 0,
};

const NATOMICA_ANCHORED: PassiveDefinition = PassiveDefinition {
    id: "natomica-anchored",
    display_name: "Anchored",
    // Placeholder: "pulls the target instead when they are lighter" is a deterministic,
    // weight-based redirect of Claymore Hook's pull direction — it has nothing to do with
    // randomness. RemoveRandomness is reused only because the closed PassiveKind set has
    // no matching variant; a resolver must not treat this as removing an RNG roll.
    kind: PassiveKind::RemoveRandomness,
    magnitude: 1,
};

const NATOMICA_KINETIC: PassiveDefinition = PassiveDefinition {
    id: "natomica-kinetic",
    display_name: "Kinetic",
    kind: PassiveKind::DamageBonus,
    // DamageBonus is a direct additive percentage (see Huck's Follow Through, Numa's
    // Executioner): the doc's 25% -> 45% wall-impact bonus is a flat +20 percentage
    // points, i.e. 2_000 bp, added on top of NATOMICA_WALL_IMPACT's base magnitude (25).
    magnitude: 2_000,
};

const NATOMICA_BULWARK: PassiveDefinition = PassiveDefinition {
    id: "natomica-bulwark",
    display_name: "Bulwark",
    kind: PassiveKind::BonusMaxHealth,
    magnitude: 60,
};

const NATOMICA_PASSIVES: &[PassiveDefinition] =
    &[NATOMICA_ANCHORED, NATOMICA_KINETIC, NATOMICA_BULWARK];

// Character definitions
const ARZUM: CharacterDefinition = CharacterDefinition {
    id: "arzum",
    version: 1,
    display_name: "Arzum",
    max_health: 300,
    range_tier: RangeTier::Tier1,
    movement: MovementClass::Fast,
    basic: ARZUM_LUNGE,
    basic_alt: None,
    special: ARZUM_CHAIN_STRIKE,
    passives: ARZUM_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

const EMI: CharacterDefinition = CharacterDefinition {
    id: "emi",
    version: 1,
    display_name: "Emi",
    max_health: 300,
    range_tier: RangeTier::Tier2,
    movement: MovementClass::Normal,
    basic: EMI_HEAVY_CUBE,
    basic_alt: None,
    special: EMI_CUBE_TURRET,
    passives: EMI_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

const KARL: CharacterDefinition = CharacterDefinition {
    id: "karl",
    version: 1,
    display_name: "Karl",
    max_health: 360,
    range_tier: RangeTier::Tier1,
    movement: MovementClass::Slow,
    basic: KARL_CARRION_CALL,
    basic_alt: None,
    special: KARL_FEEDING_FRENZY,
    passives: KARL_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

const HUCK: CharacterDefinition = CharacterDefinition {
    id: "huck",
    version: 1,
    display_name: "Huck",
    max_health: 400,
    range_tier: RangeTier::Melee,
    movement: MovementClass::Normal,
    basic: HUCK_HAYMAKER,
    basic_alt: None,
    special: HUCK_BODY_THROW,
    passives: HUCK_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

const NUMA: CharacterDefinition = CharacterDefinition {
    id: "numa",
    version: 1,
    display_name: "Numa",
    max_health: 190,
    range_tier: RangeTier::Tier3,
    movement: MovementClass::Normal,
    basic: NUMA_HARPOON,
    basic_alt: None,
    special: NUMA_PIN,
    passives: NUMA_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

const ALEPH: CharacterDefinition = CharacterDefinition {
    id: "aleph",
    version: 1,
    display_name: "Aleph",
    max_health: 240,
    range_tier: RangeTier::Tier2,
    movement: MovementClass::Normal,
    basic: ALEPH_BOW,
    basic_alt: Some(ALEPH_THROWING_KNIFE),
    special: ALEPH_VEILSTEP,
    passives: ALEPH_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

const ZEKE: CharacterDefinition = CharacterDefinition {
    id: "zeke",
    version: 1,
    display_name: "Zeke",
    max_health: 220,
    range_tier: RangeTier::Tier3,
    movement: MovementClass::Normal,
    basic: ZEKE_MENDING_BOLT,
    basic_alt: None,
    special: ZEKE_LIFESHARE,
    passives: ZEKE_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

const ROBERTO: CharacterDefinition = CharacterDefinition {
    id: "roberto",
    version: 1,
    display_name: "Roberto",
    max_health: 165,
    range_tier: RangeTier::Tier2,
    movement: MovementClass::Normal,
    basic: ROBERTO_BOUNCING_GRENADE,
    basic_alt: None,
    special: ROBERTO_HEAVY_ORDNANCE,
    passives: ROBERTO_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

const NATOMICA: CharacterDefinition = CharacterDefinition {
    id: "natomica",
    version: 1,
    display_name: "Natomica",
    max_health: 400,
    range_tier: RangeTier::Tier2,
    movement: MovementClass::Normal,
    basic: NATOMICA_CLAYMORE_HOOK,
    basic_alt: None,
    special: NATOMICA_REPULSE,
    passives: NATOMICA_PASSIVES,
    is_starter: true,
    credit_cost: 0,
};

/// The nine starter characters, available on every account.
///
/// Ordered by roster position as listed in CHARACTERS.md §3.
pub static LAUNCH_ROSTER: &[CharacterDefinition] =
    &[ARZUM, EMI, KARL, HUCK, NUMA, ALEPH, ZEKE, ROBERTO, NATOMICA];

/// Finds a character by stable identifier.
///
/// Returns a reference to the character definition if found, [`None`] otherwise.
/// Character IDs are stable across game versions; use [`CharacterDefinition::version`]
/// to track balance changes.
#[must_use]
pub fn find(id: &str) -> Option<&'static CharacterDefinition> {
    LAUNCH_ROSTER.iter().find(|c| c.id == id)
}

/// Validates the launch roster for consistency and legal definitions.
///
/// Checks:
/// - All character IDs and ability IDs are unique and non-empty.
/// - Each ability's slot matches its field position.
/// - Exactly [`PASSIVES_PER_CHARACTER`] unique passives per character.
/// - Health in valid range (150–450, catches typos; actual roster spans 165–400).
/// - Damage and crit percentages are non-zero and sensible.
/// - Crit chance consistency: zero chance ⟺ crit damage equals base damage.
/// - Strikes per turn in valid range (1–3).
/// - Effect count per ability at most [`MAX_SPECIAL_EFFECTS`].
/// - All starters have `is_starter=true` and `credit_cost=0`.
/// - Only Aleph has a `basic_alt`, checked by identity, not merely by count.
///
/// Returns [`Ok`] if the roster is valid, [`Err`] with a detailed reason otherwise.
pub fn validate_roster() -> SimResult<()> {
    validate_roster_slice(LAUNCH_ROSTER)
}

/// The actual implementation behind [`validate_roster`], parameterized over the roster so
/// it can be exercised against deliberately broken fixtures in tests. [`LAUNCH_ROSTER`] is
/// `'static`, so the public entry point above is a zero-cost wrapper.
fn validate_roster_slice(roster: &[CharacterDefinition]) -> SimResult<()> {
    let mut character_ids = HashSet::new();
    let mut all_ability_ids = HashSet::new();
    let mut basic_alt_count: u32 = 0;

    // Per-character checks.
    for character in roster {
        // Character ID must be unique and non-empty.
        if character.id.is_empty() {
            return Err(SimError::InvalidCharacter {
                reason: CharacterRejection::DuplicateId,
            });
        }
        if !character_ids.insert(character.id) {
            return Err(SimError::InvalidCharacter {
                reason: CharacterRejection::DuplicateId,
            });
        }

        // All starters must have is_starter=true and credit_cost=0.
        if !character.is_starter || character.credit_cost != 0 {
            return Err(SimError::InvalidCharacter {
                reason: CharacterRejection::StatOutOfRange,
            });
        }

        // Health bounds check (150–450 catches typos; actual roster is 165–400).
        if character.max_health < 150 || character.max_health > 450 {
            return Err(SimError::InvalidCharacter {
                reason: CharacterRejection::StatOutOfRange,
            });
        }

        // Validate basic ability. `?` propagates validate_ability's actual rejection
        // reason (e.g. SlotMismatch) instead of masking it.
        validate_ability(&mut all_ability_ids, &character.basic, AbilitySlot::Basic)?;
        validate_ability_stats(&character.basic)?;

        // Validate basic_alt if present. Only Aleph may declare one — checked by identity
        // here, not just by count, so a broken roster that moves the slot to a different
        // character (while keeping the count at one) is still rejected.
        if let Some(basic_alt) = &character.basic_alt {
            basic_alt_count = basic_alt_count.saturating_add(1);
            if character.id != "aleph" {
                return Err(SimError::InvalidCharacter {
                    reason: CharacterRejection::StatOutOfRange,
                });
            }
            validate_ability(&mut all_ability_ids, basic_alt, AbilitySlot::BasicAlt)?;
            validate_ability_stats(basic_alt)?;
        }

        // Validate special ability.
        validate_ability(
            &mut all_ability_ids,
            &character.special,
            AbilitySlot::Special,
        )?;
        validate_ability_stats(&character.special)?;

        // Validate passives: exactly PASSIVES_PER_CHARACTER with unique IDs.
        if character.passives.len() != PASSIVES_PER_CHARACTER {
            return Err(SimError::InvalidCharacter {
                reason: CharacterRejection::WrongPassiveCount,
            });
        }
        let mut passive_ids = HashSet::new();
        for passive in character.passives {
            if passive.id.is_empty() {
                return Err(SimError::InvalidCharacter {
                    reason: CharacterRejection::DuplicateId,
                });
            }
            if !passive_ids.insert(passive.id) || all_ability_ids.contains(passive.id) {
                return Err(SimError::InvalidCharacter {
                    reason: CharacterRejection::DuplicateId,
                });
            }
        }
    }

    // Aleph must actually be the one carrying the basic_alt (count == 1 here implies the
    // identity check above already passed for that one character).
    if basic_alt_count != 1 {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::StatOutOfRange,
        });
    }

    Ok(())
}

/// Validates an individual ability's slot and ID consistency.
fn validate_ability(
    all_ids: &mut HashSet<&'static str>,
    ability: &AbilityDefinition,
    expected_slot: AbilitySlot,
) -> SimResult<()> {
    if ability.id.is_empty() {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::DuplicateId,
        });
    }
    if ability.slot != expected_slot {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::SlotMismatch,
        });
    }
    if !all_ids.insert(ability.id) {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::DuplicateId,
        });
    }
    Ok(())
}

/// Validates an ability's damage, crit, and strike parameters.
fn validate_ability_stats(ability: &AbilityDefinition) -> SimResult<()> {
    // For non-damaging abilities (spawning, buffing), damage may be zero. For damage-dealing
    // abilities, damage and crit must both be non-zero.
    let has_damage = ability.damage_percent != 0;
    let has_crit_damage = ability.crit_damage_percent != 0;

    if has_damage != has_crit_damage {
        // One is zero and the other isn't; inconsistent.
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::StatOutOfRange,
        });
    }

    // If the ability deals damage, crit damage must be at least as much as base damage.
    if has_damage && ability.crit_damage_percent < ability.damage_percent {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::StatOutOfRange,
        });
    }

    // Crit chance consistency: zero iff crit damage equals base damage. Applied
    // unconditionally (not just for damaging abilities) so a malformed non-damaging
    // ability cannot smuggle in a nonzero crit chance that would never resolve, e.g.
    // damage_percent == crit_damage_percent == 0 but crit_chance_basis_points != 0.
    let has_crit_chance = ability.crit_chance_basis_points != 0;
    let crit_matches_base = ability.crit_damage_percent == ability.damage_percent;
    if has_crit_chance == crit_matches_base {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::StatOutOfRange,
        });
    }

    // Strikes per turn must be 1–3.
    if ability.strikes_per_turn < 1 || ability.strikes_per_turn > 3 {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::StatOutOfRange,
        });
    }

    // Bounds definition size so a malformed content pack cannot produce unbounded
    // per-impact work (see MAX_SPECIAL_EFFECTS's own doc comment).
    if ability.effects.len() > MAX_SPECIAL_EFFECTS {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::StatOutOfRange,
        });
    }

    Ok(())
}

#[cfg(test)]
// Test-only fixtures deliberately fail loudly (`let ... else { panic!() }`) when a lookup
// that must succeed doesn't; that is what a test is for. Matches the precedent set by
// `terrain::tests`.
#[allow(clippy::panic)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------------------
    // Roster-level smoke tests
    // -----------------------------------------------------------------------------------

    /// The launch roster must pass all validation checks.
    #[test]
    fn launch_roster_validates() {
        assert!(validate_roster().is_ok());
    }

    /// Every starter must be findable by ID.
    #[test]
    fn all_starters_are_findable() {
        for character in LAUNCH_ROSTER {
            let Some(found) = find(character.id) else {
                panic!("every roster character must be findable");
            };
            assert_eq!(found.id, character.id);
        }
    }

    /// An unknown id returns `None` rather than panicking.
    #[test]
    fn unknown_id_returns_none() {
        assert!(find("not-a-real-character").is_none());
        assert!(find("").is_none());
    }

    /// Exactly nine starters exist.
    #[test]
    fn roster_has_exactly_nine_starters() {
        assert_eq!(LAUNCH_ROSTER.len(), 9);
        for character in LAUNCH_ROSTER {
            assert!(character.is_starter);
            assert_eq!(character.credit_cost, 0);
        }
    }

    /// Only Aleph has a second basic attack.
    #[test]
    fn only_aleph_has_basic_alt() {
        let aleph_alt_count = LAUNCH_ROSTER
            .iter()
            .filter(|c| c.basic_alt.is_some())
            .count();
        assert_eq!(aleph_alt_count, 1);
        let Some(aleph) = find("aleph") else {
            panic!("aleph must exist");
        };
        assert!(aleph.basic_alt.is_some());
        for character in LAUNCH_ROSTER.iter().filter(|c| c.id != "aleph") {
            assert!(
                character.basic_alt.is_none(),
                "{} unexpectedly has a basic_alt",
                character.id
            );
        }
    }

    // -----------------------------------------------------------------------------------
    // Transcription fidelity: every value below is checked directly against
    // docs/CHARACTERS.md §3.
    // -----------------------------------------------------------------------------------

    /// Every starter's headline stats must match CHARACTERS.md §3's roster table exactly.
    #[test]
    fn starter_core_stats_match_characters_md() {
        let expected: &[(&str, u16, RangeTier, MovementClass)] = &[
            ("arzum", 300, RangeTier::Tier1, MovementClass::Fast),
            ("emi", 300, RangeTier::Tier2, MovementClass::Normal),
            ("karl", 360, RangeTier::Tier1, MovementClass::Slow),
            ("huck", 400, RangeTier::Melee, MovementClass::Normal),
            ("numa", 190, RangeTier::Tier3, MovementClass::Normal),
            ("aleph", 240, RangeTier::Tier2, MovementClass::Normal),
            ("zeke", 220, RangeTier::Tier3, MovementClass::Normal),
            ("roberto", 165, RangeTier::Tier2, MovementClass::Normal),
            ("natomica", 400, RangeTier::Tier2, MovementClass::Normal),
        ];
        assert_eq!(
            expected.len(),
            LAUNCH_ROSTER.len(),
            "test table must cover the whole roster"
        );
        for (id, hp, tier, movement) in expected {
            let Some(character) = find(id) else {
                panic!("missing starter {id}");
            };
            assert_eq!(character.max_health, *hp, "{id} max_health");
            assert_eq!(character.range_tier, *tier, "{id} range_tier");
            assert_eq!(character.movement, *movement, "{id} movement");
        }
    }

    /// Every starter's basic-attack damage percentage must match CHARACTERS.md §3.
    #[test]
    fn starter_basic_damage_matches_characters_md() {
        let expected: &[(&str, u16)] = &[
            ("arzum", 45),
            ("emi", 50),
            ("karl", 24),
            ("huck", 60),
            ("numa", 42),
            ("aleph", 20), // Bow
            ("zeke", 41),
            ("roberto", 55),
            ("natomica", 48),
        ];
        for (id, dmg) in expected {
            let Some(character) = find(id) else {
                panic!("missing starter {id}");
            };
            assert_eq!(
                character.basic.damage_percent, *dmg,
                "{id} basic damage_percent"
            );
        }
    }

    /// Karl's per-hit damage resolves the CHARACTERS.md §7 discrepancy at 24%/74% crit,
    /// three strikes per turn, exactly as the doc states it implements.
    #[test]
    fn karl_carrion_call_matches_documented_resolution() {
        let Some(karl) = find("karl") else {
            panic!("karl must exist");
        };
        assert_eq!(karl.basic.damage_percent, 24);
        assert_eq!(karl.basic.crit_damage_percent, 74);
        assert_eq!(karl.basic.strikes_per_turn, 3);
    }

    /// Huck's Haymaker destroys terrain in a 2.0 BW radius (applied around both Huck and
    /// his target by the resolver; the data need only carry the radius).
    #[test]
    fn huck_haymaker_terrain_radius_is_2_bw() {
        let Some(huck) = find("huck") else {
            panic!("huck must exist");
        };
        match huck.basic.attack {
            Attack::Strike(StrikeAttack {
                terrain: TerrainProfile::Crater { radius_cells },
                ..
            }) => {
                assert_eq!(radius_cells, 8, "2.0 BW == 8 cells at 4 cells/BW");
            }
            _ => panic!("Huck's Haymaker must be a melee strike with a crater terrain profile"),
        }
    }

    /// Huck's Body Throw splits 40% to the thrown character and 25% to the destination.
    #[test]
    fn huck_body_throw_damage_splits_40_and_25() {
        let Some(huck) = find("huck") else {
            panic!("huck must exist");
        };
        assert_eq!(huck.special.damage_percent, 40);
        let Some(relocate) = huck
            .special
            .effects
            .iter()
            .find(|e| e.kind == EffectKind::Relocate)
        else {
            panic!("Body Throw must carry a Relocate effect");
        };
        assert_eq!(relocate.magnitude, 25);
    }

    /// Roberto's Bouncing Grenade bounces exactly once.
    #[test]
    fn roberto_grenade_bounces_once() {
        let Some(roberto) = find("roberto") else {
            panic!("roberto must exist");
        };
        match roberto.basic.attack {
            Attack::Projectile(ProjectileAttack { bounces, .. }) => assert_eq!(bounces, 1),
            Attack::Strike(_) => panic!("Roberto's basic must be a projectile"),
        }
    }

    /// Zeke's basic deals damage and heals in the same action, and the heal only resolves
    /// if the bolt actually connects.
    #[test]
    fn zeke_mending_bolt_deals_damage_and_heals() {
        let Some(zeke) = find("zeke") else {
            panic!("zeke must exist");
        };
        assert_eq!(zeke.basic.damage_percent, 41);
        let Some(heal) = zeke
            .basic
            .effects
            .iter()
            .find(|e| e.kind == EffectKind::Heal)
        else {
            panic!("Mending Bolt must carry a Heal effect");
        };
        assert_eq!(heal.magnitude, 22);
        assert_eq!(
            heal.trigger,
            EffectTrigger::OnImpact,
            "heal must be tied to the bolt landing"
        );
    }

    /// Arzum's special damage range is 50..=200 percent (CHARACTERS.md flags this for
    /// balance review, but this is the value it implements).
    #[test]
    fn arzum_chain_strike_second_hit_ranges_50_to_200_percent() {
        let Some(arzum) = find("arzum") else {
            panic!("arzum must exist");
        };
        let Some(teleport) = arzum
            .special
            .effects
            .iter()
            .find(|e| e.kind == EffectKind::Teleport)
        else {
            panic!("Chain Strike must carry a Teleport effect");
        };
        assert_eq!(
            teleport.magnitude, 50,
            "second hit damage range lower bound"
        );
        assert_eq!(
            teleport.magnitude_secondary, 200,
            "second hit damage range upper bound"
        );
    }

    /// Emi's cube has the lowest wind response in the roster (2,000 bp).
    #[test]
    fn emi_wind_response_is_lowest_in_roster() {
        let Some(emi) = find("emi") else {
            panic!("emi must exist");
        };
        let emi_wind = match emi.basic.attack {
            Attack::Projectile(ProjectileAttack {
                wind_scale_basis_points,
                ..
            }) => wind_scale_basis_points,
            Attack::Strike(_) => panic!("Emi's basic must be a projectile"),
        };
        assert_eq!(emi_wind, 2_000);

        for character in LAUNCH_ROSTER {
            for ability in [
                Some(&character.basic),
                character.basic_alt.as_ref(),
                Some(&character.special),
            ]
            .into_iter()
            .flatten()
            {
                if let Attack::Projectile(ProjectileAttack {
                    wind_scale_basis_points,
                    ..
                }) = ability.attack
                {
                    assert!(
                        wind_scale_basis_points >= emi_wind,
                        "{} ({}) has lower wind response than Emi's cube",
                        character.id,
                        ability.id
                    );
                }
            }
        }
    }

    /// Aleph's throwing knife deals 60% damage and embeds where it lands.
    #[test]
    fn aleph_throwing_knife_deals_60_percent_and_embeds() {
        let Some(aleph) = find("aleph") else {
            panic!("aleph must exist");
        };
        let Some(knife) = aleph.basic_alt.as_ref() else {
            panic!("aleph must have a basic_alt");
        };
        assert_eq!(knife.damage_percent, 60);
        assert!(
            knife
                .effects
                .iter()
                .any(|e| e.kind == EffectKind::EmbedProjectile)
        );
    }

    /// Aleph's bow deals 20% damage — cheap and high-frequency next to the 60% knife.
    #[test]
    fn aleph_bow_deals_20_percent() {
        let Some(aleph) = find("aleph") else {
            panic!("aleph must exist");
        };
        assert_eq!(aleph.basic.damage_percent, 20);
    }

    /// Aleph's dagger chain detonates for 70% damage in a 2.5 BW radius when a second
    /// knife lands within 1.5 BW of an embedded one, and the embed cap is 8.
    #[test]
    fn aleph_dagger_chain_matches_characters_md() {
        let Some(aleph) = find("aleph") else {
            panic!("aleph must exist");
        };
        let Some(knife) = aleph.basic_alt.as_ref() else {
            panic!("aleph must have a basic_alt");
        };

        let Some(chain) = knife
            .effects
            .iter()
            .find(|e| e.kind == EffectKind::ChainDetonate)
        else {
            panic!(
                "throwing knife must carry a ChainDetonate effect (CHARACTERS.md's dagger chain)"
            );
        };
        assert_eq!(chain.magnitude, 70, "chain detonation damage percent");
        assert_eq!(
            chain.magnitude_secondary,
            10 * POSITION_SCALE,
            "2.5 BW detonation/terrain radius"
        );

        let Some(embed) = knife
            .effects
            .iter()
            .find(|e| e.kind == EffectKind::EmbedProjectile)
        else {
            panic!("throwing knife must carry an EmbedProjectile effect");
        };
        assert_eq!(
            embed.magnitude,
            6 * POSITION_SCALE,
            "1.5 BW chain-trigger proximity"
        );
        assert_eq!(embed.magnitude_secondary, 8, "embedded knife cap");
    }

    /// Natomica's wall-impact bonus is a separate 25% damage line, matching the
    /// documented 13.75 = 0.25 * 55 relationship to Repulse's own damage.
    #[test]
    fn natomica_repulse_and_wall_impact_match_characters_md() {
        let Some(natomica) = find("natomica") else {
            panic!("natomica must exist");
        };
        assert_eq!(natomica.special.damage_percent, 55);
        let Some(wall_impact) = natomica
            .special
            .effects
            .iter()
            .find(|e| e.kind == EffectKind::WallImpact)
        else {
            panic!("Repulse must carry a WallImpact effect");
        };
        assert_eq!(wall_impact.magnitude, 25);
    }

    /// Numa's harpoon direction threshold is 50% of target max HP, stored as basis points
    /// (10_000 == 100%), not as a raw percent.
    #[test]
    fn numa_harpoon_threshold_is_50_percent_in_basis_points() {
        let Some(numa) = find("numa") else {
            panic!("numa must exist");
        };
        let Some(pull) = numa
            .basic
            .effects
            .iter()
            .find(|e| e.kind == EffectKind::Pull)
        else {
            panic!("Harpoon must carry a Pull effect");
        };
        assert_eq!(
            pull.magnitude_secondary, 5_000,
            "50% in basis points, not 50_000"
        );
    }

    // -----------------------------------------------------------------------------------
    // Ballistic sanity: every derived value must actually be able to reach its tier's
    // stated range, not just look plausible in isolation.
    // -----------------------------------------------------------------------------------

    /// At a 45-degree launch, a simple Euler-integrated projectile's maximum range is
    /// speed^2 / gravity. Every tier's derived speed/gravity pair must clear its tier's
    /// reach, or a character firing at max power could never hit their own max range.
    #[test]
    fn projectile_tiers_can_reach_their_own_range_tier() {
        let cases: &[(i32, i32, i32)] = &[
            (
                PROJECTILE_TIER1_SPEED,
                PROJECTILE_TIER1_GRAVITY,
                RangeTier::Tier1.reach(),
            ),
            (
                PROJECTILE_TIER2_SPEED,
                PROJECTILE_TIER2_GRAVITY,
                RangeTier::Tier2.reach(),
            ),
            (
                PROJECTILE_TIER3_SPEED,
                PROJECTILE_TIER3_GRAVITY,
                RangeTier::Tier3.reach(),
            ),
        ];
        for (speed, gravity, reach) in cases {
            // max_range = speed^2 / gravity >= reach  <=>  speed^2 >= reach * gravity
            // (gravity > 0 here), stated as a multiplication so no division is needed.
            let speed_squared = i64::from(*speed) * i64::from(*speed);
            let required = i64::from(*reach) * i64::from(*gravity);
            assert!(
                speed_squared >= required,
                "speed={speed} gravity={gravity} cannot reach tier reach {reach} at 45 degrees"
            );
        }
    }

    /// `max_ticks` must comfortably exceed the worst-case (vertical-launch) hang time,
    /// `2 * speed / gravity`, or a high-arc shot expires before landing.
    #[test]
    fn projectile_tiers_max_ticks_exceed_worst_case_hang_time() {
        let cases: &[(i32, i32, u16)] = &[
            (
                PROJECTILE_TIER1_SPEED,
                PROJECTILE_TIER1_GRAVITY,
                PROJECTILE_TIER1_MAX_TICKS,
            ),
            (
                PROJECTILE_TIER2_SPEED,
                PROJECTILE_TIER2_GRAVITY,
                PROJECTILE_TIER2_MAX_TICKS,
            ),
            (
                PROJECTILE_TIER3_SPEED,
                PROJECTILE_TIER3_GRAVITY,
                PROJECTILE_TIER3_MAX_TICKS,
            ),
        ];
        for (speed, gravity, max_ticks) in cases {
            // max_ticks > 2 * speed / gravity  <=>  max_ticks * gravity > 2 * speed
            // (gravity > 0 here), stated as a multiplication so no division is needed.
            let max_ticks_times_gravity = i64::from(*max_ticks) * i64::from(*gravity);
            let twice_speed = 2 * i64::from(*speed);
            assert!(
                max_ticks_times_gravity > twice_speed,
                "max_ticks={max_ticks} does not exceed worst-case (vertical-launch) hang time"
            );
        }
    }

    // -----------------------------------------------------------------------------------
    // validate_roster: negative-case coverage. LAUNCH_ROSTER is static and always valid,
    // so every rejection branch below is exercised through validate_roster_slice against
    // deliberately broken fixtures.
    // -----------------------------------------------------------------------------------

    const DUMMY_PASSIVE_A: PassiveDefinition = PassiveDefinition {
        id: "dummy-passive-a",
        display_name: "A",
        kind: PassiveKind::BonusMaxHealth,
        magnitude: 1,
    };
    const DUMMY_PASSIVE_B: PassiveDefinition = PassiveDefinition {
        id: "dummy-passive-b",
        display_name: "B",
        kind: PassiveKind::BonusMaxHealth,
        magnitude: 1,
    };
    const DUMMY_PASSIVE_C: PassiveDefinition = PassiveDefinition {
        id: "dummy-passive-c",
        display_name: "C",
        kind: PassiveKind::BonusMaxHealth,
        magnitude: 1,
    };
    const DUMMY_PASSIVE_D: PassiveDefinition = PassiveDefinition {
        id: "dummy-passive-d",
        display_name: "D",
        kind: PassiveKind::BonusMaxHealth,
        magnitude: 1,
    };
    const DUMMY_PASSIVES: &[PassiveDefinition] =
        &[DUMMY_PASSIVE_A, DUMMY_PASSIVE_B, DUMMY_PASSIVE_C];

    fn dummy_ability(id: &'static str, slot: AbilitySlot) -> AbilityDefinition {
        AbilityDefinition {
            id,
            display_name: "Dummy",
            slot,
            damage_percent: 0,
            crit_damage_percent: 0,
            crit_chance_basis_points: 0,
            strikes_per_turn: 1,
            attack: Attack::Strike(StrikeAttack {
                range: 0,
                terrain: TerrainProfile::None,
                self_damage: 0,
            }),
            effects: &[],
        }
    }

    /// A minimal, individually-valid character fixture. `id`, `basic_id`, and
    /// `special_id` are all caller-supplied so multiple fixtures can share one roster
    /// without an unintended ability-id collision masking the condition under test.
    fn dummy_character(
        id: &'static str,
        basic_id: &'static str,
        special_id: &'static str,
    ) -> CharacterDefinition {
        CharacterDefinition {
            id,
            version: 1,
            display_name: "Dummy",
            max_health: 300,
            range_tier: RangeTier::Tier2,
            movement: MovementClass::Normal,
            basic: dummy_ability(basic_id, AbilitySlot::Basic),
            basic_alt: None,
            special: dummy_ability(special_id, AbilitySlot::Special),
            passives: DUMMY_PASSIVES,
            is_starter: true,
            credit_cost: 0,
        }
    }

    #[track_caller]
    fn assert_rejected(result: SimResult<()>, expected: CharacterRejection) {
        match result {
            Err(SimError::InvalidCharacter { reason }) => {
                assert_eq!(reason, expected, "wrong rejection reason");
            }
            other => panic!("expected InvalidCharacter({expected:?}), got {other:?}"),
        }
    }

    /// Sanity check: the dummy fixture used by every negative test below is itself valid
    /// in isolation, aside from lacking Aleph's basic_alt (covered separately).
    #[test]
    fn dummy_character_without_aleph_fails_only_the_basic_alt_check() {
        let c = dummy_character("dummy", "dummy-basic", "dummy-special");
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn duplicate_character_id_is_rejected() {
        let a = dummy_character("dup", "dup-basic-1", "dup-special-1");
        let b = dummy_character("dup", "dup-basic-2", "dup-special-2");
        assert_rejected(
            validate_roster_slice(&[a, b]),
            CharacterRejection::DuplicateId,
        );
    }

    #[test]
    fn empty_character_id_is_rejected() {
        let c = dummy_character("", "empty-id-basic", "empty-id-special");
        assert_rejected(validate_roster_slice(&[c]), CharacterRejection::DuplicateId);
    }

    #[test]
    fn non_starter_is_rejected() {
        let mut c = dummy_character("non-starter", "non-starter-basic", "non-starter-special");
        c.is_starter = false;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn nonzero_credit_cost_is_rejected() {
        let mut c = dummy_character("paid", "paid-basic", "paid-special");
        c.credit_cost = 2_300;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn health_below_minimum_is_rejected() {
        let mut c = dummy_character("frail", "frail-basic", "frail-special");
        c.max_health = 149;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn health_above_maximum_is_rejected() {
        let mut c = dummy_character("colossus", "colossus-basic", "colossus-special");
        c.max_health = 451;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn health_at_bounds_is_accepted() {
        // 150 and 450 are inclusive bounds.
        let mut low = dummy_character("floor", "floor-basic", "floor-special");
        low.max_health = 150;
        let mut high = dummy_character("aleph", "ceiling-basic", "ceiling-special");
        high.max_health = 450;
        high.basic_alt = Some(dummy_ability("ceiling-alt", AbilitySlot::BasicAlt));
        assert!(validate_roster_slice(&[low, high]).is_ok());
    }

    /// Regression test: before this fix, `validate_roster` collapsed every ability
    /// rejection into `DuplicateId` via an unconditional `map_err`, hiding a slot
    /// mismatch behind the wrong reason.
    #[test]
    fn ability_slot_mismatch_is_rejected_with_correct_reason() {
        let mut c = dummy_character("mismatched", "mismatched-basic", "mismatched-special");
        c.basic.slot = AbilitySlot::Special;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::SlotMismatch,
        );
    }

    #[test]
    fn basic_alt_slot_mismatch_is_rejected_with_correct_reason() {
        let mut c = dummy_character("aleph", "aleph-basic", "aleph-special");
        c.basic_alt = Some(dummy_ability("aleph-alt", AbilitySlot::Basic));
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::SlotMismatch,
        );
    }

    #[test]
    fn special_slot_mismatch_is_rejected_with_correct_reason() {
        let mut c = dummy_character(
            "mismatched-special",
            "mismatched-special-basic",
            "mismatched-special-special",
        );
        c.special.slot = AbilitySlot::Basic;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::SlotMismatch,
        );
    }

    #[test]
    fn wrong_passive_count_too_few_is_rejected() {
        const TOO_FEW: &[PassiveDefinition] = &[DUMMY_PASSIVE_A, DUMMY_PASSIVE_B];
        let mut c = dummy_character("few-passives", "few-basic", "few-special");
        c.passives = TOO_FEW;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::WrongPassiveCount,
        );
    }

    #[test]
    fn wrong_passive_count_too_many_is_rejected() {
        const EXTRA: &[PassiveDefinition] = &[
            DUMMY_PASSIVE_A,
            DUMMY_PASSIVE_B,
            DUMMY_PASSIVE_C,
            DUMMY_PASSIVE_D,
        ];
        let mut c = dummy_character("many-passives", "many-basic", "many-special");
        c.passives = EXTRA;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::WrongPassiveCount,
        );
    }

    #[test]
    fn duplicate_passive_id_within_character_is_rejected() {
        const DUPES: &[PassiveDefinition] = &[DUMMY_PASSIVE_A, DUMMY_PASSIVE_A, DUMMY_PASSIVE_B];
        let mut c = dummy_character("dupe-passive", "dupe-basic", "dupe-special");
        c.passives = DUPES;
        assert_rejected(validate_roster_slice(&[c]), CharacterRejection::DuplicateId);
    }

    #[test]
    fn empty_passive_id_is_rejected() {
        const EMPTY_ID: PassiveDefinition = PassiveDefinition {
            id: "",
            display_name: "Empty",
            kind: PassiveKind::BonusMaxHealth,
            magnitude: 1,
        };
        const WITH_EMPTY: &[PassiveDefinition] = &[EMPTY_ID, DUMMY_PASSIVE_B, DUMMY_PASSIVE_C];
        let mut c = dummy_character(
            "empty-passive",
            "empty-passive-basic",
            "empty-passive-special",
        );
        c.passives = WITH_EMPTY;
        assert_rejected(validate_roster_slice(&[c]), CharacterRejection::DuplicateId);
    }

    /// Regression test: before the identity check was added, only the *count* of
    /// characters with a basic_alt was checked, so a broken roster that moved the slot to
    /// a non-Aleph character (while keeping the count at exactly one) would incorrectly
    /// pass.
    #[test]
    fn non_aleph_with_basic_alt_is_rejected() {
        let mut c = dummy_character("not-aleph", "not-aleph-basic", "not-aleph-special");
        c.basic_alt = Some(dummy_ability("not-aleph-alt", AbilitySlot::BasicAlt));
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn roster_without_any_basic_alt_is_rejected() {
        let c = dummy_character("no-alt", "no-alt-basic", "no-alt-special");
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn damage_crit_zero_mismatch_is_rejected() {
        let mut c = dummy_character("mismatch-dmg", "mismatch-dmg-basic", "mismatch-dmg-special");
        c.basic.damage_percent = 40;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn crit_damage_below_base_is_rejected() {
        let mut c = dummy_character("low-crit", "low-crit-basic", "low-crit-special");
        c.basic.damage_percent = 40;
        c.basic.crit_damage_percent = 30;
        c.basic.crit_chance_basis_points = 1_000;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    /// Regression test: the crit-chance-consistency check must apply even to
    /// non-damaging abilities, not only when `damage_percent != 0`.
    #[test]
    fn nonzero_crit_chance_on_non_damaging_ability_is_rejected() {
        let mut c = dummy_character("phantom-crit", "phantom-crit-basic", "phantom-crit-special");
        c.basic.crit_chance_basis_points = 500;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn zero_crit_chance_with_mismatched_crit_and_base_is_rejected() {
        let mut c = dummy_character("stale-crit", "stale-crit-basic", "stale-crit-special");
        c.basic.damage_percent = 40;
        c.basic.crit_damage_percent = 60;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn strikes_per_turn_zero_is_rejected() {
        let mut c = dummy_character("no-strikes", "no-strikes-basic", "no-strikes-special");
        c.basic.strikes_per_turn = 0;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn strikes_per_turn_above_three_is_rejected() {
        let mut c = dummy_character("many-strikes", "many-strikes-basic", "many-strikes-special");
        c.basic.strikes_per_turn = 4;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    /// Regression test: `MAX_SPECIAL_EFFECTS` was previously unenforced anywhere in the
    /// crate.
    #[test]
    fn too_many_effects_is_rejected() {
        const EFFECT: SpecialEffect = SpecialEffect {
            trigger: EffectTrigger::OnFire,
            kind: EffectKind::Knockback,
            magnitude: 1,
            magnitude_secondary: 0,
            duration_turns: 0,
        };
        const TOO_MANY: &[SpecialEffect] =
            &[EFFECT, EFFECT, EFFECT, EFFECT, EFFECT, EFFECT, EFFECT];
        assert_eq!(TOO_MANY.len(), MAX_SPECIAL_EFFECTS + 1);
        let mut c = dummy_character("overloaded", "overloaded-basic", "overloaded-special");
        c.basic.effects = TOO_MANY;
        assert_rejected(
            validate_roster_slice(&[c]),
            CharacterRejection::StatOutOfRange,
        );
    }

    #[test]
    fn duplicate_ability_id_across_characters_is_rejected() {
        let a = dummy_character("first", "shared-id", "first-special");
        let b = dummy_character("second", "shared-id", "second-special");
        assert_rejected(
            validate_roster_slice(&[a, b]),
            CharacterRejection::DuplicateId,
        );
    }

    #[test]
    fn passive_id_colliding_with_an_ability_id_is_rejected() {
        const COLLIDING: PassiveDefinition = PassiveDefinition {
            id: "collide-basic", // matches the character's own basic ability id below
            display_name: "Collider",
            kind: PassiveKind::BonusMaxHealth,
            magnitude: 1,
        };
        const PASSIVES: &[PassiveDefinition] = &[COLLIDING, DUMMY_PASSIVE_B, DUMMY_PASSIVE_C];
        let mut c = dummy_character("collide", "collide-basic", "collide-special");
        c.passives = PASSIVES;
        assert_rejected(validate_roster_slice(&[c]), CharacterRejection::DuplicateId);
    }
}
