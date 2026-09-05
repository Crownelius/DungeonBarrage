//! Legacy ability catalog and compatibility helpers.
//!
//! Schema-2 match creation uses [`crate::character_roster`] and derives a fixed character
//! kit. This catalog remains for deterministic replay migration and low-level resolver
//! coverage; it is not player-selectable. Attack resolution still consumes
//! [`crate::types::AbilityDefinition`] so all abilities share one rules path.

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

/// Unlimited secondary fallback damage. It is deliberately below every finite main's
/// per-command value and carries no terrain, displacement, or status effect.
const SECONDARY_DAMAGE_PERCENT: u16 = 16;

/// All melee/tools provide the loadout-independent ring-out route. The short reach is the
/// positional cost; the action itself cannot disappear because a durability counter ran out.
const MELEE_TOOL_SHOVE: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Push,
    magnitude: 2 * POSITION_SCALE,
    magnitude_secondary: 5 * POSITION_SCALE,
    duration_turns: 0,
};

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
const RAMSHOT_CRATER_RADIUS_CELLS: u16 = 3;

/// [`RAMSHOT_CRATER_RADIUS_CELLS`] in fixed-point units, for the knockback falloff.
///
/// The assertion below is what actually keeps the two in step, since the terrain profile
/// wants cells and the falloff radius wants fixed-point.
const RAMSHOT_CRATER_RADIUS_FIXED: i32 = 3 * POSITION_SCALE;
const _: () = assert!(RAMSHOT_CRATER_RADIUS_CELLS == 3);

const FROSTFALL_CHILL: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Chill,
    magnitude: 2 * BODY_WIDTH,
    magnitude_secondary: 0,
    duration_turns: 1,
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
        terrain: TerrainProfile::Crater { radius_cells: 3 },
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
    slot: AbilitySlot::Basic,
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

const SERVICE_PISTOL_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "service-pistol",
    display_name: "5.7 Service Pistol",
    slot: AbilitySlot::Basic,
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

const fn melee_ability(id: &'static str, display_name: &'static str) -> AbilityDefinition {
    AbilityDefinition {
        id,
        display_name,
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
        effects: &[MELEE_TOOL_SHOVE],
    }
}

const fn melee_item(id: &'static str, display_name: &'static str) -> ItemDefinition {
    ItemDefinition {
        id,
        display_name,
        slot: AbilitySlot::Special,
        ammo_policy: AmmoPolicy::Unlimited,
        starting_ammo: 0,
        ability: melee_ability(id, display_name),
    }
}

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
    slot: AbilitySlot::Basic,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 5,
    ability: RECURVE_BOW_ABILITY,
};

const SERVICE_PISTOL: ItemDefinition = ItemDefinition {
    id: "service-pistol",
    display_name: "5.7 Service Pistol",
    slot: AbilitySlot::Basic,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 6,
    ability: SERVICE_PISTOL_ABILITY,
};

const TRENCH_SPADE: ItemDefinition = melee_item("trench-spade", "Trench Spade");

const BLOOD_MAUL: ItemDefinition = melee_item("blood-maul", "Blood Maul");

const BREACH_PICK: ItemDefinition = ItemDefinition {
    id: "breach-pick",
    display_name: "Breach Pick",
    slot: AbilitySlot::Special,
    ammo_policy: AmmoPolicy::Unlimited,
    starting_ammo: 0,
    ability: melee_ability("breach-pick", "Breach Pick"),
};

const LINE_REPEATER_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "line-repeater",
    display_name: "Line Repeater",
    slot: AbilitySlot::Basic,
    damage_percent: 16,
    crit_damage_percent: 16,
    crit_chance_basis_points: 0,
    strikes_per_turn: 4,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER3_SPEED,
        gravity_per_tick: 4,
        wind_scale_basis_points: 0,
        max_ticks: PROJECTILE_TIER1_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[],
};

const RETURNING_BOOMERANG_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "returning-boomerang",
    display_name: "Returning Boomerang",
    slot: AbilitySlot::Basic,
    damage_percent: 28,
    crit_damage_percent: 28,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER2_SPEED,
        gravity_per_tick: 8,
        wind_scale_basis_points: 4_000,
        max_ticks: PROJECTILE_TIER2_MAX_TICKS,
        bounces: 2,
        terrain: TerrainProfile::Crater { radius_cells: 2 },
    }),
    effects: &[],
};

const TIDE_SPRAYER_PUSH: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Push,
    magnitude: 2 * POSITION_SCALE,
    magnitude_secondary: 4 * POSITION_SCALE,
    duration_turns: 0,
};

const TIDE_SPRAYER_ABILITY: AbilityDefinition = AbilityDefinition {
    id: "tide-sprayer",
    display_name: "Tide Sprayer",
    slot: AbilitySlot::Basic,
    damage_percent: 18,
    crit_damage_percent: 18,
    crit_chance_basis_points: 0,
    strikes_per_turn: 1,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: PROJECTILE_TIER1_SPEED,
        gravity_per_tick: PROJECTILE_TIER1_GRAVITY,
        wind_scale_basis_points: 2_000,
        max_ticks: PROJECTILE_TIER1_MAX_TICKS,
        bounces: 0,
        terrain: TerrainProfile::None,
    }),
    effects: &[TIDE_SPRAYER_PUSH],
};

const LINE_REPEATER: ItemDefinition = ItemDefinition {
    id: "line-repeater",
    display_name: "Line Repeater",
    slot: AbilitySlot::Basic,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 4,
    ability: LINE_REPEATER_ABILITY,
};

const RETURNING_BOOMERANG: ItemDefinition = ItemDefinition {
    id: "returning-boomerang",
    display_name: "Returning Boomerang",
    slot: AbilitySlot::Basic,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 3,
    ability: RETURNING_BOOMERANG_ABILITY,
};

const TIDE_SPRAYER: ItemDefinition = ItemDefinition {
    id: "tide-sprayer",
    display_name: "Tide Sprayer",
    slot: AbilitySlot::Basic,
    ammo_policy: AmmoPolicy::Finite,
    starting_ammo: 5,
    ability: TIDE_SPRAYER_ABILITY,
};

const LONGSWORD: ItemDefinition = melee_item("longsword", "Longsword");
const CROW_BEAK: ItemDefinition = melee_item("crow-beak", "Crow Beak");
const IRON_FAN: ItemDefinition = melee_item("iron-fan", "Iron Fan");
const HOOK_BILL: ItemDefinition = melee_item("hook-bill", "Hook Bill");
const RUST_CLEAVER: ItemDefinition = melee_item("rust-cleaver", "Rust Cleaver");

const fn secondary_of(
    main: AbilityDefinition,
    id: &'static str,
    display_name: &'static str,
) -> ItemDefinition {
    let mut ability = main;
    ability.id = id;
    ability.display_name = display_name;
    ability.slot = AbilitySlot::BasicAlt;
    ability.damage_percent = SECONDARY_DAMAGE_PERCENT;
    ability.crit_damage_percent = SECONDARY_DAMAGE_PERCENT;
    ability.crit_chance_basis_points = 0;
    ability.strikes_per_turn = 1;
    ability.effects = &[];
    ability.attack = match ability.attack {
        Attack::Projectile(mut projectile) => {
            projectile.terrain = TerrainProfile::None;
            Attack::Projectile(projectile)
        }
        Attack::Strike(mut strike) => {
            strike.terrain = TerrainProfile::None;
            strike.self_damage = 0;
            Attack::Strike(strike)
        }
    };
    ItemDefinition {
        id,
        display_name,
        slot: AbilitySlot::BasicAlt,
        ammo_policy: AmmoPolicy::Unlimited,
        starting_ammo: 0,
        ability,
    }
}

const RAMSHOT_SHELL: ItemDefinition =
    secondary_of(RAMSHOT_CANNON_ABILITY, "ramshot-shell", "Ramshot Sidearm");
const FROSTFALL_SHELL: ItemDefinition = secondary_of(
    FROSTFALL_MORTAR_ABILITY,
    "frostfall-shell",
    "Frostfall Sidearm",
);
const MOLE_CHARGE: ItemDefinition = secondary_of(MOLE_DRILL_ABILITY, "mole-charge", "Mole Sidearm");
const BOW_BODKIN: ItemDefinition = secondary_of(RECURVE_BOW_ABILITY, "bow-bodkin", "Bow Sidearm");
const PISTOL_MAG: ItemDefinition =
    secondary_of(SERVICE_PISTOL_ABILITY, "pistol-mag", "Pistol Sidearm");
const REPEATER_BELT: ItemDefinition =
    secondary_of(LINE_REPEATER_ABILITY, "repeater-belt", "Repeater Sidearm");
const BOOMERANG_FINISHER: ItemDefinition = secondary_of(
    RETURNING_BOOMERANG_ABILITY,
    "boomerang-finisher",
    "Boomerang Sidearm",
);
const TIDE_BLADDER: ItemDefinition =
    secondary_of(TIDE_SPRAYER_ABILITY, "tide-bladder", "Tide Sidearm");

const EMBER_CROWN_EFFECT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Embers,
    magnitude: 12,
    magnitude_secondary: 4 * POSITION_SCALE,
    duration_turns: 2,
};

const FROST_CROWN_EFFECT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Chill,
    magnitude: 2 * BODY_WIDTH,
    magnitude_secondary: 0,
    duration_turns: 1,
};

const GALE_ANKLET_EFFECT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Knockback,
    magnitude: 2 * POSITION_SCALE,
    magnitude_secondary: 6 * POSITION_SCALE,
    duration_turns: 0,
};

const TIDE_ANKLET_EFFECT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnImpact,
    kind: EffectKind::Push,
    magnitude: 3 * POSITION_SCALE,
    magnitude_secondary: 6 * POSITION_SCALE,
    duration_turns: 0,
};

const fn trinket_ability(
    id: &'static str,
    display_name: &'static str,
    effects: &'static [SpecialEffect],
) -> AbilityDefinition {
    AbilityDefinition {
        id,
        display_name,
        slot: AbilitySlot::Trinket,
        damage_percent: 20,
        crit_damage_percent: 20,
        crit_chance_basis_points: 0,
        strikes_per_turn: 1,
        attack: Attack::Strike(StrikeAttack {
            range: RangeTier::Melee.reach(),
            terrain: TerrainProfile::None,
            self_damage: 0,
        }),
        effects,
    }
}

const fn trinket_item(
    id: &'static str,
    display_name: &'static str,
    effects: &'static [SpecialEffect],
) -> ItemDefinition {
    ItemDefinition {
        id,
        display_name,
        slot: AbilitySlot::Trinket,
        ammo_policy: AmmoPolicy::Unlimited,
        starting_ammo: 0,
        ability: trinket_ability(id, display_name, effects),
    }
}

const SPARK_CROWN_EFFECT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::GuaranteeCrit,
    magnitude: 1,
    magnitude_secondary: 0,
    duration_turns: 0,
};

const SPRING_ANKLET_EFFECT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::Recoil,
    magnitude: 3 * POSITION_SCALE,
    magnitude_secondary: 0,
    duration_turns: 0,
};

const BURROW_ANKLET_EFFECT: SpecialEffect = SpecialEffect {
    trigger: EffectTrigger::OnFire,
    kind: EffectKind::Tunnel,
    magnitude: 12,
    magnitude_secondary: 20,
    duration_turns: 0,
};

const EMBER_CROWN: ItemDefinition =
    trinket_item("ember-crown", "Ember Crown", &[EMBER_CROWN_EFFECT]);
const FROST_CROWN: ItemDefinition =
    trinket_item("frost-crown", "Frost Crown", &[FROST_CROWN_EFFECT]);
const SPARK_CROWN: ItemDefinition =
    trinket_item("spark-crown", "Spark Crown", &[SPARK_CROWN_EFFECT]);
const ROOST_CROWN: ItemDefinition = trinket_item(
    "roost-crown",
    "Roost Crown",
    &[SpecialEffect {
        trigger: EffectTrigger::OnFire,
        kind: EffectKind::Heal,
        magnitude: 24,
        magnitude_secondary: 0,
        duration_turns: 0,
    }],
);
const GALE_ANKLET: ItemDefinition =
    trinket_item("gale-anklet", "Gale Anklet", &[GALE_ANKLET_EFFECT]);
const TIDE_ANKLET: ItemDefinition =
    trinket_item("tide-anklet", "Tide Anklet", &[TIDE_ANKLET_EFFECT]);
const SPRING_ANKLET: ItemDefinition =
    trinket_item("spring-anklet", "Spring Anklet", &[SPRING_ANKLET_EFFECT]);
const BURROW_ANKLET: ItemDefinition =
    trinket_item("burrow-anklet", "Burrow Anklet", &[BURROW_ANKLET_EFFECT]);

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
    SERVICE_PISTOL,
    LINE_REPEATER,
    RETURNING_BOOMERANG,
    TIDE_SPRAYER,
    RAMSHOT_SHELL,
    FROSTFALL_SHELL,
    MOLE_CHARGE,
    BOW_BODKIN,
    PISTOL_MAG,
    REPEATER_BELT,
    BOOMERANG_FINISHER,
    TIDE_BLADDER,
    TRENCH_SPADE,
    BLOOD_MAUL,
    BREACH_PICK,
    LONGSWORD,
    CROW_BEAK,
    IRON_FAN,
    HOOK_BILL,
    RUST_CLEAVER,
    EMBER_CROWN,
    FROST_CROWN,
    SPARK_CROWN,
    ROOST_CROWN,
    GALE_ANKLET,
    TIDE_ANKLET,
    SPRING_ANKLET,
    BURROW_ANKLET,
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

/// Fixed character action for `player` in `slot`.
///
/// The player layout still carries an authority-derived loadout while replay migration is
/// in progress, but no client chooses those identifiers and the item catalog is not consulted.
pub fn equipped_ability(
    player: &PlayerState,
    slot: AbilitySlot,
) -> Option<&'static AbilityDefinition> {
    crate::character_roster::for_player(player)
        .and_then(|profile| profile.ability(slot))
        .or_else(|| {
            // Versioned replays may still contain the retired item layout. New match
            // creation cannot reach this fallback: it accepts a launch character id and
            // derives the fixed kit.
            item(player.loadout.item_id(slot))
                .filter(|definition| definition.slot == slot)
                .map(|definition| &definition.ability)
        })
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

/// Validates the retained legacy fighter and item catalog.
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
        match (definition.slot, definition.ammo_policy) {
            (AbilitySlot::Trinket, AmmoPolicy::Unlimited) => {
                if definition.ability.effects.is_empty() {
                    return Err(SimError::InvalidCharacter {
                        reason: CharacterRejection::StatOutOfRange,
                    });
                }
            }
            (AbilitySlot::BasicAlt, AmmoPolicy::Unlimited) => {
                let terrain_free = match definition.ability.attack {
                    Attack::Projectile(projectile) => projectile.terrain == TerrainProfile::None,
                    Attack::Strike(strike) => strike.terrain == TerrainProfile::None,
                };
                if definition.starting_ammo != 0
                    || definition.ability.damage_percent != SECONDARY_DAMAGE_PERCENT
                    || definition.ability.strikes_per_turn != 1
                    || !definition.ability.effects.is_empty()
                    || !terrain_free
                {
                    return Err(SimError::InvalidCharacter {
                        reason: CharacterRejection::StatOutOfRange,
                    });
                }
            }
            (AbilitySlot::Special, AmmoPolicy::Unlimited) => {
                if definition.starting_ammo != 0
                    || !definition.ability.effects.iter().any(|effect| {
                        matches!(effect.kind, EffectKind::Knockback | EffectKind::Push)
                    })
                {
                    return Err(SimError::InvalidCharacter {
                        reason: CharacterRejection::StatOutOfRange,
                    });
                }
            }
            (_, AmmoPolicy::Finite) => {
                if definition.starting_ammo == 0 {
                    return Err(SimError::InvalidCharacter {
                        reason: CharacterRejection::StatOutOfRange,
                    });
                }
            }
            (_, AmmoPolicy::Unlimited) => {
                return Err(SimError::InvalidRoster);
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
    for slot in AbilitySlot::ALL {
        if items_in_slot(slot).count() != 8 {
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
    fn legacy_catalog_exposes_only_its_retained_crow_definition() {
        assert_eq!(find(CROW_ID).map(|fighter| fighter.id), Some(CROW_ID));
        assert!(find("huck").is_none());
        assert!(find("arzum").is_none());
        assert_eq!(LAUNCH_ROSTER.len(), 1);
    }

    #[test]
    fn default_state_uses_crows_fixed_unlimited_kit() {
        let loadout = Loadout::launch_default();
        assert_eq!(
            crate::character_roster::find_by_kit(
                &loadout.main,
                &loadout.secondary,
                &loadout.trinket,
            )
            .map(|profile| profile.id.wire_name()),
            Some("crow")
        );
        assert!(DEFAULT_AMMO.iter().all(|counter| {
            counter.policy == AmmoPolicy::Unlimited
                && counter.remaining == 0
                && counter.maximum == 0
        }));
        assert!(item("ramshot-cannon").is_some());
        assert!(item("ramshot-shell").is_some());
        assert!(item("trench-spade").is_some());
        assert!(item("ember-crown").is_some());
        let Some(shell) = item("ramshot-shell") else {
            panic!("ramshot-shell");
        };
        assert_eq!(shell.starting_ammo, 0);
        assert_eq!(shell.ammo_policy, AmmoPolicy::Unlimited);
        assert_eq!(shell.ability.damage_percent, SECONDARY_DAMAGE_PERCENT);
        assert!(shell.ability.effects.is_empty());
    }

    #[test]
    fn every_loadout_keeps_unlimited_damage_and_ring_out_routes() {
        for secondary in items_in_slot(AbilitySlot::BasicAlt) {
            assert_eq!(secondary.ammo_policy, AmmoPolicy::Unlimited);
            assert_eq!(secondary.ability.damage_percent, SECONDARY_DAMAGE_PERCENT);
            assert!(secondary.ability.effects.is_empty());
        }

        for tool in items_in_slot(AbilitySlot::Special) {
            assert_eq!(tool.ammo_policy, AmmoPolicy::Unlimited);
            assert!(tool.ability.effects.iter().any(|effect| {
                matches!(effect.kind, EffectKind::Knockback | EffectKind::Push)
                    && effect.magnitude > 0
                    && effect.magnitude_secondary > 0
            }));
        }
    }

    #[test]
    fn each_slot_has_eight_items() {
        for slot in AbilitySlot::ALL {
            assert_eq!(
                items_in_slot(slot).count(),
                8,
                "slot {slot:?} must offer eight picker tiles"
            );
        }
    }

    #[test]
    fn every_trinket_has_a_distinct_primary_effect() {
        let mut kinds = std::collections::HashSet::new();
        for definition in items_in_slot(AbilitySlot::Trinket) {
            let Some(effect) = definition.ability.effects.first() else {
                panic!("{} has no special", definition.id);
            };
            assert!(
                kinds.insert(effect.kind),
                "{} reuses {:?}; each crown or anklet must charge a unique special",
                definition.id,
                effect.kind
            );
        }
        assert_eq!(kinds.len(), 8);
    }
}
