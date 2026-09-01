//! The one crow fighter and the item catalog that supplies its ammunition.
//!
//! Playable cut: kits are not in the envelope. Every player is [`CROW`]; equipped items
//! are ammo. Attack resolution still consumes [`crate::types::AbilityDefinition`] so
//! ballistics, terrain, and damage stay on one path.

use crate::error::{CharacterRejection, SimError, SimResult};
use crate::fixed::{BODY_WIDTH, POSITION_SCALE};
use crate::types::{
    AbilityDefinition, AbilitySlot, AmmoCounter, AmmoPolicy, Attack, CROW_ID, CROW_MAX_HEALTH,
    CharacterDefinition, EffectKind, EffectTrigger, ItemDefinition, Loadout, MovementClass,
    PlayerState, ProjectileAttack, RangeTier, SpecialEffect, StrikeAttack, TerrainProfile,
};

const PROJECTILE_TIER1_SPEED: i32 = 900;
const PROJECTILE_TIER1_GRAVITY: i32 = 20;
const PROJECTILE_TIER1_MAX_TICKS: u16 = 140;

const PROJECTILE_TIER2_SPEED: i32 = 1_200;
const PROJECTILE_TIER2_GRAVITY: i32 = 16;
const PROJECTILE_TIER2_MAX_TICKS: u16 = 200;

const PROJECTILE_TIER3_SPEED: i32 = 1_500;
const PROJECTILE_TIER3_GRAVITY: i32 = 12;
const PROJECTILE_TIER3_MAX_TICKS: u16 = 300;

/// Ramshot's shove, scoped to the blast it actually makes.
///
/// `magnitude_secondary` is the falloff radius, and it must stay greater than zero.
/// `displacement.rs` documents `0` as "the primary target only", but its implementation
/// reads `radius <= 0` as *no radius test at all*: `targets_in_radius` then collects every
/// living opponent anywhere on the map and `falloff` returns the full magnitude regardless
/// of distance. An aim-fired shell names no primary target, so a `0` here shoved every
/// opponent a flat 8 cells no matter where the shell landed — a shot falling 30 cells short
/// still launched the target off its 4-cell perch and out of the world, ending the match on
/// turn 1 (see `docs/BUILD_LOG.md`). Matching the radius to this item's own
/// `TerrainProfile::Crater` keeps the shove tied to the crater the player can actually see.
const RAMSHOT_KNOCKBACK: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Knockback,
    magnitude: RAMSHOT_KNOCKBACK_CELLS * POSITION_SCALE,
    magnitude_secondary: RAMSHOT_CRATER_RADIUS_FIXED,
    duration_turns: 0,
};

/// How far a direct Ramshot hit shoves its target, in cells.
///
/// Half `map::STACK_BLOCK_WIDTH` (4). At `2 * BODY_WIDTH` (eight cells) a direct hit cleared
/// any perch outright and dropped the target into the void, so the first accurate shot won
/// regardless of health: the bot ended crow-perch and broken-battlements on turn 1 while the
/// item's own damage (62 against 200 health, with three rounds) is tuned for roughly a
/// four-hit kill. Two cells still repositions — standing near an edge is still punished — but
/// it no longer substitutes for the damage race.
const RAMSHOT_KNOCKBACK_CELLS: i32 = 2;

/// Crater radius of [`RAMSHOT_CANNON_ABILITY`], in cells.
///
/// Named so the ability's terrain profile and [`RAMSHOT_KNOCKBACK`]'s falloff radius cannot
/// drift apart: the shove is meant to be the crater's shove.
const RAMSHOT_CRATER_RADIUS_CELLS: u16 = 6;

/// [`RAMSHOT_CRATER_RADIUS_CELLS`] in fixed-point units, for the knockback falloff.
///
/// The assertion below is what actually keeps the two in step, since the terrain profile
/// wants cells and the falloff radius wants fixed-point.
const RAMSHOT_CRATER_RADIUS_FIXED: i32 = 6 * POSITION_SCALE;
const _: () = assert!(RAMSHOT_CRATER_RADIUS_CELLS == 6);

const FROSTFALL_CHILL: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Chill,
    magnitude: 2 * BODY_WIDTH,
    magnitude_secondary: 0,
    duration_turns: 1,
};

const BLOOD_MAUL_BACKLASH: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::SelfDamage,
    magnitude: 14,
    magnitude_secondary: 0,
    duration_turns: 0,
};

const RAMSHOT_CANNON_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "ramshot-cannon",
    display_name: "Ramshot Cannon",
    slot: AbilitySlot::Basic,
    damage_percent: 62,
    crit_damage_percent: 62,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 10_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::Crater {
            radius_cells: RAMSHOT_CRATER_RADIUS_CELLS,
        },
    }),
    effects: &[RAMSHOT_KNOCKBACK],
};

const FROSTFALL_MORTAR_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "frostfall-mortar",
    display_name: "Frostfall Mortar",
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
        terrain: TerrainProfile::Crater { radius_cells: 8 },
    }),
    effects: &[FROSTFALL_CHILL],
};

const MOLE_DRILL_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "mole-drill",
    display_name: "Mole Drill",
    slot: AbilitySlot::Basic,
    damage_percent: 58,
    crit_damage_percent: 58,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER1_SPEED,
        gravity_per_tick: PROJECTILE_TIER1_GRAVITY,
        wind_scale_basis_points: 6_000,
        max_ticks: PROJECTILE_TIER1_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::Dig {
            radius_cells: 2,
            length_cells: 24,
        },
    }),
    effects: &[],
};

const RECURVE_BOW_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "recurve-bow",
    display_name: "Recurve Bow",
    slot: AbilitySlot::BasicAlt,
    damage_percent: 32,
    crit_damage_percent: 32,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: PROJECTILE_TIER2_GRAVITY,
        wind_scale_basis_points: 12_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[],
};

const LONGSWORD_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "longsword",
    display_name: "Longsword",
    slot: AbilitySlot::BasicAlt,
    damage_percent: 24,
    crit_damage_percent: 24,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: 10 * POSITION_SCALE,
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    effects: &[],
};

const SERVICE_PISTOL_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "service-pistol",
    display_name: "5.7 Service Pistol",
    slot: AbilitySlot::BasicAlt,
    damage_percent: 26,
    crit_damage_percent: 26,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER3_SPEED,
        gravity_per_tick: PROJECTILE_TIER3_GRAVITY,
        wind_scale_basis_points: 0,
        max_ticks: PROJECTILE_TIER3_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[],
};

const TRENCH_SPADE_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "trench-spade",
    display_name: "Trench Spade",
    slot: AbilitySlot::Special,
    damage_percent: 22,
    crit_damage_percent: 22,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Melee.reach(),
        terrain: TerrainProfile::Dig {
            radius_cells: 4,
            length_cells: 6,
        },
        self_damage: 0,
    }),
    effects: &[],
};

const BLOOD_MAUL_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "blood-maul",
    display_name: "Blood Maul",
    slot: AbilitySlot::Special,
    damage_percent: 52,
    crit_damage_percent: 52,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Melee.reach(),
        terrain: TerrainProfile::Crater { radius_cells: 2 },
        self_damage: 14,
    }),
    effects: &[BLOOD_MAUL_BACKLASH],
};

const BREACH_PICK_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "breach-pick",
    display_name: "Breach Pick",
    slot: AbilitySlot::Special,
    damage_percent: 30,
    crit_damage_percent: 30,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Strike(StrikeAttack {
        range: RangeTier::Melee.reach(),
        terrain: TerrainProfile::Dig {
            radius_cells: 3,
            length_cells: 4,
        },
        self_damage: 0,
    }),
    effects: &[],
};

const RAMSHOT_CANNON: ItemDefinition = ItemDefinition {
    id: "ramshot-cannon",
    display_name: "Ramshot Cannon",
    slot: AbilitySlot::Basic,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 3,
    ability: RAMSHOT_CANNON_ABILITY,
};

const FROSTFALL_MORTAR: ItemDefinition = ItemDefinition {
    id: "frostfall-mortar",
    display_name: "Frostfall Mortar",
    slot: AbilitySlot::Basic,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 3,
    ability: FROSTFALL_MORTAR_ABILITY,
};

const MOLE_DRILL: ItemDefinition = ItemDefinition {
    id: "mole-drill",
    display_name: "Mole Drill",
    slot: AbilitySlot::Basic,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 2,
    ability: MOLE_DRILL_ABILITY,
};

const RECURVE_BOW: ItemDefinition = ItemDefinition {
    id: "recurve-bow",
    display_name: "Recurve Bow",
    slot: AbilitySlot::BasicAlt,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 5,
    ability: RECURVE_BOW_ABILITY,
};

const LONGSWORD: ItemDefinition = ItemDefinition {
    id: "longsword",
    display_name: "Longsword",
    slot: AbilitySlot::BasicAlt,
    ammo_policy: AmmoPolicy::Unlimited,
    starting_ammo: 0,
    ability: LONGSWORD_ABILITY,
};

const SERVICE_PISTOL: ItemDefinition = ItemDefinition {
    id: "service-pistol",
    display_name: "5.7 Service Pistol",
    slot: AbilitySlot::BasicAlt,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 6,
    ability: SERVICE_PISTOL_ABILITY,
};

const TRENCH_SPADE: ItemDefinition = ItemDefinition {
    id: "trench-spade",
    display_name: "Trench Spade",
    slot: AbilitySlot::Special,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 4,
    ability: TRENCH_SPADE_ABILITY,
};

const BLOOD_MAUL: ItemDefinition = ItemDefinition {
    id: "blood-maul",
    display_name: "Blood Maul",
    slot: AbilitySlot::Special,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 2,
    ability: BLOOD_MAUL_ABILITY,
};

const BREACH_PICK: ItemDefinition = ItemDefinition {
    id: "breach-pick",
    display_name: "Breach Pick",
    slot: AbilitySlot::Special,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 3,
    ability: BREACH_PICK_ABILITY,
};

/// The one fighter in this envelope.
pub static CROW: CharacterDefinition = CharacterDefinition {
    id: CROW_ID,
    version: 1,
    display_name: "Crow",
    max_health: CROW_MAX_HEALTH,
    range_tier: RangeTier::Tier2,
    movement: MovementClass::Normal,
    is_starter: true,
    credit_cost: 0,
};

/// Launch roster: the crow only.
pub static LAUNCH_ROSTER: &[CharacterDefinition] = &[CROW];

/// Equipable items. Slot occupancy is exclusive: a loadout picks one per slot.
pub static LAUNCH_ITEMS: &[ItemDefinition] = &[
    RAMSHOT_CANNON,
    FROSTFALL_MORTAR,
    MOLE_DRILL,
    RECURVE_BOW,
    LONGSWORD,
    SERVICE_PISTOL,
    TRENCH_SPADE,
    BLOOD_MAUL,
    BREACH_PICK,
];

/// Finds the crow by identifier. Unknown ids, including retired kit ids, return [`None`].
pub fn find(id: &str) -> Option<&'static CharacterDefinition> {
    LAUNCH_ROSTER.iter().find(|fighter| fighter.id == id)
}

/// The one fighter definition.
#[must_use]
pub fn fighter() -> &'static CharacterDefinition {
    &CROW
}

/// Finds an item by stable identifier.
pub fn item(id: &str) -> Option<&'static ItemDefinition> {
    LAUNCH_ITEMS.iter().find(|candidate| candidate.id == id)
}

/// Items legal in `slot`, in catalog order.
pub fn items_in_slot(slot: AbilitySlot) -> impl Iterator<Item = &'static ItemDefinition> {
    LAUNCH_ITEMS
        .iter()
        .filter(move |candidate| candidate.slot == slot)
}

/// Equipped attack for `player` in `slot`, if the loadout item exists and belongs there.
pub fn equipped_ability(
    player: &PlayerState,
    slot: AbilitySlot,
) -> Option<&'static AbilityDefinition> {
    let equipped = item(player.loadout.item_id(slot))?;
    if equipped.slot == slot {
        Some(&equipped.ability)
    } else {
        None
    }
}

/// Starting ammo counters for a validated loadout.
///
/// # Errors
///
/// Returns [`SimError::UnknownDefinition`] when an identifier is missing or occupies the
/// wrong slot.
pub fn ammo_for_loadout(loadout: &Loadout) -> SimResult<[AmmoCounter; 3]> {
    let main = ammo_for_slot(loadout, AbilitySlot::Basic)?;
    let secondary = ammo_for_slot(loadout, AbilitySlot::BasicAlt)?;
    let melee = ammo_for_slot(loadout, AbilitySlot::Special)?;
    Ok([main, secondary, melee])
}

fn ammo_for_slot(loadout: &Loadout, slot: AbilitySlot) -> SimResult<AmmoCounter> {
    let Some(definition) = item(loadout.item_id(slot)) else {
        return Err(SimError::UnknownDefinition);
    };
    if definition.slot != slot {
        return Err(SimError::UnknownDefinition);
    }
    Ok(AmmoCounter {
        remaining: definition.starting_ammo,
        maximum: definition.starting_ammo,
        policy: definition.ammo_policy,
    })
}

/// Validates the crow fighter and the item catalog.
///
/// # Errors
///
/// Returns [`SimError::InvalidCharacter`] or [`SimError::InvalidRoster`] when the catalog
/// is internally inconsistent.
pub fn validate_roster() -> SimResult<()> {
    if LAUNCH_ROSTER.len() != 1 {
        return Err(SimError::InvalidRoster);
    }
    let Some(crow) = LAUNCH_ROSTER.first() else {
        return Err(SimError::InvalidRoster);
    };
    if crow.id != CROW_ID || crow.max_health != CROW_MAX_HEALTH || !crow.is_starter {
        return Err(SimError::InvalidCharacter {
            reason: CharacterRejection::StatOutOfRange,
        });
    }

    let mut ids = std::collections::HashSet::new();
    let mut unlimited_count: u32 = 0;
    for definition in LAUNCH_ITEMS {
        if definition.id.is_empty() || !ids.insert(definition.id) {
            return Err(SimError::InvalidCharacter {
                reason: CharacterRejection::DuplicateId,
            });
        }
        if definition.ability.slot != definition.slot || definition.ability.id != definition.id {
            return Err(SimError::InvalidCharacter {
                reason: CharacterRejection::SlotMismatch,
            });
        }
        match definition.ammo_policy {
            AmmoPolicy::Unlimited => {
                unlimited_count = unlimited_count.saturating_add(1);
                if definition.id != "longsword" {
                    return Err(SimError::InvalidRoster);
                }
            }
            AmmoPolicy::Finite => {
                if definition.starting_ammo == 0 {
                    return Err(SimError::InvalidCharacter {
                        reason: CharacterRejection::StatOutOfRange,
                    });
                }
            }
        }

        // A `Knockback`/`Push` with no radius does not mean "no area effect".
        // `displacement.rs` reads `magnitude_secondary <= 0` as *no radius test at all*:
        // `targets_in_radius` then collects every living opponent on the map and `falloff`
        // returns the full magnitude regardless of distance. That shipped once, on the
        // Ramshot Cannon at `CONTENT_VERSION` 2, and decided every match on turn 1. The
        // catalog refuses it here rather than trusting the next author to remember.
        for effect in definition.ability.effects {
            if matches!(effect.kind, EffectKind::Knockback | EffectKind::Push)
                && effect.magnitude_secondary <= 0
            {
                return Err(SimError::InvalidCharacter {
                    reason: CharacterRejection::StatOutOfRange,
                });
            }
        }
    }
    if unlimited_count != 1 {
        return Err(SimError::InvalidRoster);
    }
    for slot in AbilitySlot::ALL {
        if items_in_slot(slot).next().is_none() {
            return Err(SimError::InvalidRoster);
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::types::DEFAULT_AMMO;

    #[test]
    fn every_displacement_effect_declares_a_positive_falloff_radius() {
        // Regression guard for CONTENT_VERSION 3. `magnitude_secondary` is the falloff radius
        // for `Knockback`/`Push`, and `displacement.rs` treats `<= 0` as "no radius test at
        // all" -- full magnitude against every living opponent, wherever they are standing.
        // The Ramshot Cannon shipped that way and ended every duel on turn 1.
        for definition in LAUNCH_ITEMS {
            for effect in definition.ability.effects {
                if matches!(effect.kind, EffectKind::Knockback | EffectKind::Push) {
                    assert!(
                        effect.magnitude_secondary > 0,
                        "{} declares a {:?} with magnitude_secondary {}; a displacement radius                          must be positive or it applies to the whole map",
                        definition.id,
                        effect.kind,
                        effect.magnitude_secondary
                    );
                }
            }
        }
        validate_roster().expect("the shipped catalog must satisfy its own invariants");
    }

    #[test]
    fn the_catalog_is_self_consistent() {
        let Ok(()) = validate_roster() else {
            panic!("launch catalog must validate");
        };
    }

    #[test]
    fn only_the_crow_is_playable() {
        assert_eq!(find(CROW_ID).map(|fighter| fighter.id), Some(CROW_ID));
        assert!(find("huck").is_none());
        assert!(find("arzum").is_none());
        assert_eq!(LAUNCH_ROSTER.len(), 1);
    }

    #[test]
    fn default_ammo_matches_the_launch_loadout() {
        let Ok(ammo) = ammo_for_loadout(&Loadout::launch_default()) else {
            panic!("default loadout");
        };
        assert_eq!(ammo, DEFAULT_AMMO);
        assert!(item("ramshot-cannon").is_some());
        assert!(item("recurve-bow").is_some());
        assert!(item("trench-spade").is_some());
        let Some(sword) = item("longsword") else {
            panic!("longsword");
        };
        assert_eq!(sword.ammo_policy, AmmoPolicy::Unlimited);
    }

    #[test]
    fn each_slot_has_at_least_two_items() {
        for slot in AbilitySlot::ALL {
            assert!(
                items_in_slot(slot).count() >= 2,
                "slot {slot:?} needs a picker"
            );
        }
    }
}
