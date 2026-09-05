//! Launch character profiles for the character-driven playable cut.
//!
//! A match-create request names one character. The authority, never the client, expands
//! that identifier into a fixed three-action kit: Shot 1, Shot 2/Melee, and a charged SS.
//! The older item catalog remains temporarily available for replay migration, but it is not
//! a player-selection or combat-authority boundary for this roster.

use crate::fixed::POSITION_SCALE;
use crate::types::{
    AbilityDefinition, AbilitySlot, Attack, EffectKind, EffectTrigger, Loadout, MovementClass,
    ProjectileAttack, RangeTier, SpecialEffect, StrikeAttack, TerrainProfile,
};

const TIER1_SPEED: i32 = 900;
const TIER1_GRAVITY: i32 = 20;
const TIER1_TICKS: u16 = 140;
const TIER2_SPEED: i32 = 1_200;
const TIER2_GRAVITY: i32 = 16;
const TIER2_TICKS: u16 = 200;
const TIER3_SPEED: i32 = 1_500;
const TIER3_TICKS: u16 = 300;

/// Stable launch-character identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CharacterId {
    /// Ground-crawling siege and terrain disruptor.
    Leslie,
    /// Aerial gunslinger and precision skirmisher.
    Crow,
    /// Parabolic artillery wizard and autonomous-battery controller.
    Erus,
    /// Fast long-range huntress and global finisher.
    Kreena,
}

impl CharacterId {
    /// Stable wire identifier.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Leslie => "leslie",
            Self::Crow => "crow",
            Self::Erus => "erus",
            Self::Kreena => "kreena",
        }
    }
}

/// One authority-owned fixed character kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterProfile {
    /// Stable identifier.
    pub id: CharacterId,
    /// Player-facing name.
    pub name: &'static str,
    /// Short role description for character select.
    pub role: &'static str,
    /// Starting and maximum health.
    pub max_health: u16,
    /// Authoritative movement class.
    pub movement_class: MovementClass,
    /// Explicit fixed-point allowance advertised to clients.
    pub movement_allowance: i32,
    /// Signature primary action.
    pub shot1: AbilityDefinition,
    /// Tactical second shot or melee action.
    pub shot2_or_melee: AbilityDefinition,
    /// Gauge-gated SS action.
    pub special: AbilityDefinition,
}

impl CharacterProfile {
    /// Returns the fixed ability in a visible action-bar slot.
    #[must_use]
    pub const fn ability(&self, slot: AbilitySlot) -> Option<&AbilityDefinition> {
        match slot {
            AbilitySlot::Basic => Some(&self.shot1),
            AbilitySlot::BasicAlt => Some(&self.shot2_or_melee),
            AbilitySlot::Trinket => Some(&self.special),
            AbilitySlot::Special => None,
        }
    }

    /// Transitional state layout derived by the authority, never accepted from clients.
    /// These fields disappear when replay migration off the item envelope is complete.
    #[must_use]
    pub fn derived_loadout(&self) -> Loadout {
        Loadout {
            main: self.shot1.id.to_owned(),
            secondary: self.shot2_or_melee.id.to_owned(),
            // Retained only for legacy replay/resolver coverage. The character UI has
            // three actions and never exposes this retired fourth combat slot.
            melee_tool: "trench-spade".to_owned(),
            trinket: self.special.id.to_owned(),
        }
    }
}

struct ProjectileSpec {
    damage_percent: u16,
    crit_damage_percent: u16,
    crit_chance_basis_points: u16,
    strikes_per_turn: u8,
    speed_per_tick: i32,
    gravity_per_tick: i32,
    wind_scale_basis_points: i32,
    max_ticks: u16,
    bounces: u8,
    terrain: TerrainProfile,
    effects: &'static [SpecialEffect],
}

const fn projectile(
    id: &'static str,
    display_name: &'static str,
    slot: AbilitySlot,
    spec: ProjectileSpec,
) -> AbilityDefinition {
    AbilityDefinition {
        id,
        display_name,
        slot,
        damage_percent: spec.damage_percent,
        crit_damage_percent: spec.crit_damage_percent,
        crit_chance_basis_points: spec.crit_chance_basis_points,
        strikes_per_turn: spec.strikes_per_turn,
        attack: Attack::Projectile(ProjectileAttack {
            speed_per_tick: spec.speed_per_tick,
            gravity_per_tick: spec.gravity_per_tick,
            wind_scale_basis_points: spec.wind_scale_basis_points,
            max_ticks: spec.max_ticks,
            bounces: spec.bounces,
            terrain: spec.terrain,
        }),
        effects: spec.effects,
    }
}

struct StrikeSpec {
    damage_percent: u16,
    crit_damage_percent: u16,
    crit_chance_basis_points: u16,
    range: i32,
    effects: &'static [SpecialEffect],
}

const fn strike(
    id: &'static str,
    display_name: &'static str,
    slot: AbilitySlot,
    spec: StrikeSpec,
) -> AbilityDefinition {
    AbilityDefinition {
        id,
        display_name,
        slot,
        damage_percent: spec.damage_percent,
        crit_damage_percent: spec.crit_damage_percent,
        crit_chance_basis_points: spec.crit_chance_basis_points,
        strikes_per_turn: 1,
        attack: Attack::Strike(StrikeAttack {
            range: spec.range,
            terrain: TerrainProfile::None,
            self_damage: 0,
        }),
        effects: spec.effects,
    }
}

const LESLIE_CLUSTER: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Cluster,
    magnitude: 3,
    magnitude_secondary: 12,
    duration_turns: 30,
};
const LESLIE_PULL: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Pull,
    magnitude: 2 * POSITION_SCALE,
    magnitude_secondary: 3 * POSITION_SCALE,
    duration_turns: 0,
};
const LESLIE_OOZE_BURN: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Embers,
    magnitude: 12,
    magnitude_secondary: 4 * POSITION_SCALE,
    duration_turns: 3,
};
const CROW_REVOLVER_KNOCKBACK: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Knockback,
    magnitude: 3 * POSITION_SCALE,
    magnitude_secondary: 2 * POSITION_SCALE,
    duration_turns: 0,
};
const ERUS_BURN: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Embers,
    magnitude: 10,
    magnitude_secondary: 3 * POSITION_SCALE,
    duration_turns: 2,
};
const ERUS_REPEL: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Push,
    magnitude: 3 * POSITION_SCALE,
    magnitude_secondary: 3 * POSITION_SCALE,
    duration_turns: 0,
};
const ERUS_STAFF_BATTERY: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::SpawnTurret,
    magnitude: 0,
    magnitude_secondary: 0,
    duration_turns: 3,
};
const KREENA_DISENGAGE: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::Recoil,
    magnitude: 2 * POSITION_SCALE,
    magnitude_secondary: 0,
    duration_turns: 0,
};

const LESLIE_ANT_GLOB: AbilityDefinition = projectile(
    "leslie-ant-glob",
    "Ant Glob",
    AbilitySlot::Basic,
    ProjectileSpec {
        damage_percent: 34,
        crit_damage_percent: 34,
        crit_chance_basis_points: 0,
        strikes_per_turn: 1,
        speed_per_tick: TIER1_SPEED,
        gravity_per_tick: TIER1_GRAVITY,
        wind_scale_basis_points: 7_000,
        max_ticks: TIER1_TICKS,
        bounces: 1,
        terrain: TerrainProfile::None,
        effects: &[LESLIE_CLUSTER],
    },
);
const LESLIE_TONGUE_WHIP: AbilityDefinition = strike(
    "leslie-tongue-whip",
    "Tongue Whip",
    AbilitySlot::BasicAlt,
    StrikeSpec {
        damage_percent: 28,
        crit_damage_percent: 28,
        crit_chance_basis_points: 0,
        range: 3 * POSITION_SCALE,
        effects: &[LESLIE_PULL],
    },
);
const LESLIE_CORROSIVE_OOZE: AbilityDefinition = projectile(
    "leslie-corrosive-ooze",
    "Corrosive Vomit Ooze",
    AbilitySlot::Trinket,
    ProjectileSpec {
        damage_percent: 18,
        crit_damage_percent: 18,
        crit_chance_basis_points: 0,
        strikes_per_turn: 1,
        speed_per_tick: TIER1_SPEED,
        gravity_per_tick: TIER1_GRAVITY,
        wind_scale_basis_points: 3_000,
        max_ticks: TIER1_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
        effects: &[LESLIE_OOZE_BURN],
    },
);
const CROW_PRECISION: AbilityDefinition = projectile(
    "crow-precision-57",
    "5.7 High-Velocity Precision",
    AbilitySlot::Basic,
    ProjectileSpec {
        damage_percent: 34,
        crit_damage_percent: 50,
        crit_chance_basis_points: 1_500,
        strikes_per_turn: 1,
        speed_per_tick: TIER3_SPEED,
        gravity_per_tick: 0,
        wind_scale_basis_points: 0,
        max_ticks: TIER3_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
        effects: &[],
    },
);
const CROW_HEAVY_REVOLVER: AbilityDefinition = projectile(
    "crow-heavy-revolver",
    "Heavy Revolver",
    AbilitySlot::BasicAlt,
    ProjectileSpec {
        damage_percent: 42,
        crit_damage_percent: 42,
        crit_chance_basis_points: 0,
        strikes_per_turn: 1,
        speed_per_tick: TIER3_SPEED,
        gravity_per_tick: 2,
        wind_scale_basis_points: 0,
        max_ticks: TIER3_TICKS,
        bounces: 0,
        terrain: TerrainProfile::Dig {
            radius_cells: 1,
            length_cells: 4,
        },
        effects: &[CROW_REVOLVER_KNOCKBACK],
    },
);
const CROW_AERIAL_BARRAGE: AbilityDefinition = projectile(
    "crow-aerial-barrage",
    "Aerial Barrage",
    AbilitySlot::Trinket,
    ProjectileSpec {
        damage_percent: 18,
        crit_damage_percent: 18,
        crit_chance_basis_points: 0,
        strikes_per_turn: 4,
        speed_per_tick: TIER3_SPEED,
        gravity_per_tick: 4,
        wind_scale_basis_points: 0,
        max_ticks: TIER2_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
        effects: &[],
    },
);
const ERUS_CURVED_FIREBALL: AbilityDefinition = projectile(
    "erus-curved-fireball",
    "Curved Fireball",
    AbilitySlot::Basic,
    ProjectileSpec {
        damage_percent: 44,
        crit_damage_percent: 44,
        crit_chance_basis_points: 0,
        strikes_per_turn: 1,
        speed_per_tick: TIER2_SPEED,
        gravity_per_tick: TIER2_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: TIER2_TICKS,
        bounces: 0,
        terrain: TerrainProfile::Crater { radius_cells: 2 },
        effects: &[ERUS_BURN],
    },
);
const ERUS_STAFF_THRUST: AbilityDefinition = strike(
    "erus-staff-thrust",
    "Staff Thrust",
    AbilitySlot::BasicAlt,
    StrikeSpec {
        damage_percent: 24,
        crit_damage_percent: 24,
        crit_chance_basis_points: 0,
        range: RangeTier::Melee.reach(),
        effects: &[ERUS_REPEL],
    },
);
const ERUS_CELESTIAL_STAFF: AbilityDefinition = projectile(
    "erus-celestial-staff",
    "Celestial Staff Battery",
    AbilitySlot::Trinket,
    ProjectileSpec {
        damage_percent: 0,
        crit_damage_percent: 0,
        crit_chance_basis_points: 0,
        strikes_per_turn: 1,
        speed_per_tick: TIER2_SPEED,
        gravity_per_tick: TIER2_GRAVITY,
        wind_scale_basis_points: 6_000,
        max_ticks: TIER2_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
        effects: &[ERUS_STAFF_BATTERY],
    },
);
const KREENA_RECURVE_BOW: AbilityDefinition = projectile(
    "kreena-recurve-bow",
    "Recurve Bow",
    AbilitySlot::Basic,
    ProjectileSpec {
        damage_percent: 38,
        crit_damage_percent: 50,
        crit_chance_basis_points: 1_000,
        strikes_per_turn: 1,
        speed_per_tick: TIER2_SPEED,
        gravity_per_tick: 12,
        wind_scale_basis_points: 8_000,
        max_ticks: TIER2_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
        effects: &[],
    },
);
const KREENA_HUNTING_DAGGER: AbilityDefinition = strike(
    "kreena-hunting-dagger",
    "Hunting Dagger",
    AbilitySlot::BasicAlt,
    StrikeSpec {
        damage_percent: 36,
        crit_damage_percent: 52,
        crit_chance_basis_points: 1_500,
        range: 3 * POSITION_SCALE,
        effects: &[KREENA_DISENGAGE],
    },
);
const KREENA_GLOBAL_ARROW: AbilityDefinition = projectile(
    "kreena-global-magic-arrow",
    "Global Magic Arrow",
    AbilitySlot::Trinket,
    ProjectileSpec {
        damage_percent: 58,
        crit_damage_percent: 58,
        crit_chance_basis_points: 0,
        strikes_per_turn: 1,
        speed_per_tick: TIER3_SPEED,
        gravity_per_tick: 8,
        wind_scale_basis_points: 0,
        max_ticks: TIER3_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
        effects: &[],
    },
);

/// Four-character launch roster. Order is stable UI/content data.
pub static LAUNCH_CHARACTERS: &[CharacterProfile] = &[
    CharacterProfile {
        id: CharacterId::Leslie,
        name: "Leslie",
        role: "Siege / terrain disruptor",
        max_health: 340,
        movement_class: MovementClass::Slow,
        movement_allowance: MovementClass::Slow.per_turn(),
        shot1: LESLIE_ANT_GLOB,
        shot2_or_melee: LESLIE_TONGUE_WHIP,
        special: LESLIE_CORROSIVE_OOZE,
    },
    CharacterProfile {
        id: CharacterId::Crow,
        name: "Crow",
        role: "Aerial precision skirmisher",
        max_health: 250,
        movement_class: MovementClass::Fast,
        movement_allowance: MovementClass::Fast.per_turn(),
        shot1: CROW_PRECISION,
        shot2_or_melee: CROW_HEAVY_REVOLVER,
        special: CROW_AERIAL_BARRAGE,
    },
    CharacterProfile {
        id: CharacterId::Erus,
        name: "Erus",
        role: "Arcane artillery / sky battery",
        max_health: 270,
        movement_class: MovementClass::Normal,
        movement_allowance: MovementClass::Normal.per_turn(),
        shot1: ERUS_CURVED_FIREBALL,
        shot2_or_melee: ERUS_STAFF_THRUST,
        special: ERUS_CELESTIAL_STAFF,
    },
    CharacterProfile {
        id: CharacterId::Kreena,
        name: "Kreena",
        role: "Long-range mobile finisher",
        max_health: 260,
        movement_class: MovementClass::Fast,
        movement_allowance: MovementClass::Fast.per_turn(),
        shot1: KREENA_RECURVE_BOW,
        shot2_or_melee: KREENA_HUNTING_DAGGER,
        special: KREENA_GLOBAL_ARROW,
    },
];

/// Finds a launch character by stable wire id.
#[must_use]
pub fn find(id: &str) -> Option<&'static CharacterProfile> {
    LAUNCH_CHARACTERS
        .iter()
        .find(|profile| profile.id.wire_name() == id)
}

/// Resolves the authority-derived profile stored in the transitional player layout.
#[must_use]
pub fn for_player(player: &crate::types::PlayerState) -> Option<&'static CharacterProfile> {
    find_by_kit(
        &player.loadout.main,
        &player.loadout.secondary,
        &player.loadout.trinket,
    )
}

/// Resolves a character from the authority-derived ability identifiers in a snapshot.
#[must_use]
pub fn find_by_kit(
    shot1_id: &str,
    shot2_or_melee_id: &str,
    special_id: &str,
) -> Option<&'static CharacterProfile> {
    LAUNCH_CHARACTERS.iter().find(|profile| {
        profile.shot1.id == shot1_id
            && profile.shot2_or_melee.id == shot2_or_melee_id
            && profile.special.id == special_id
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_roster_has_four_unique_complete_kits() {
        let mut ids = std::collections::BTreeSet::new();
        assert_eq!(LAUNCH_CHARACTERS.len(), 4);
        for profile in LAUNCH_CHARACTERS {
            assert!(ids.insert(profile.id.wire_name()));
            assert!(profile.max_health > 0);
            assert_eq!(
                profile.movement_allowance,
                profile.movement_class.per_turn()
            );
            assert_eq!(profile.shot1.slot, AbilitySlot::Basic);
            assert_eq!(profile.shot2_or_melee.slot, AbilitySlot::BasicAlt);
            assert_eq!(profile.special.slot, AbilitySlot::Trinket);
        }
    }

    #[test]
    fn every_character_has_two_normal_actions_and_one_ss() {
        for profile in LAUNCH_CHARACTERS {
            assert!(profile.ability(AbilitySlot::Basic).is_some());
            assert!(profile.ability(AbilitySlot::BasicAlt).is_some());
            assert!(profile.ability(AbilitySlot::Special).is_none());
            assert!(profile.ability(AbilitySlot::Trinket).is_some());
        }
    }
}
