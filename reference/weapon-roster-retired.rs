//! Weapon roster and validation.
//!
//! This module defines the complete launch arsenal as static data and validates it
//! against known constraints. All weapons are versioned immutably per match, allowing
//! balance tuning without breaking existing replays (`PLATFORM_STRATEGY.md` §6).
//!
//! Source of truth: `ARSENAL.md`. Cross-validated against `lib/game/simulation.ts`
//! launchWeapons array.

use crate::fixed::{BASE_MELEE_RANGE, BODY_WIDTH};
use crate::types::{
    AmmoPolicy, Attack, EffectKind, EffectTrigger, ProjectileAttack,
    SpecialEffect, StrikeAttack, TerrainProfile, WeaponDefinition, WeaponSlot,
    MAX_SPECIAL_EFFECTS,
};
use crate::SimResult;

// ---------------------------------------------------------------------------
// Main slot (map-scale artillery with defining special effects)
// ---------------------------------------------------------------------------

/// Ramshot Cannon: the dependable calibration weapon with strong knockback.
const RAMSHOT_CANNON: WeaponDefinition = WeaponDefinition {
    id: "ramshot-cannon",
    version: 1,
    display_name: "Ramshot Cannon",
    slot: WeaponSlot::Main,
    ammo: AmmoPolicy::Finite { capacity: 3 },
    action_point_cost: 3,
    base_damage: 62,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: 420,
        gravity_per_tick: 18,
        wind_scale_basis_points: 7_500,
        max_ticks: 240,
        terrain: TerrainProfile::Crater { radius_cells: 6 },
    }),
    special_effects: &[SpecialEffect {
        trigger: EffectTrigger::OnImpact,
        kind: EffectKind::Knockback,
        magnitude: 1_800,
        duration_turns: 0,
    }],
    ranked_enabled: true,
};

/// Frostfall Mortar: high arc with chilling status effect.
const FROSTFALL_MORTAR: WeaponDefinition = WeaponDefinition {
    id: "frostfall-mortar",
    version: 1,
    display_name: "Frostfall Mortar",
    slot: WeaponSlot::Main,
    ammo: AmmoPolicy::Finite { capacity: 3 },
    action_point_cost: 3,
    base_damage: 48,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: 350,
        gravity_per_tick: 22,
        wind_scale_basis_points: 9_000,
        max_ticks: 260,
        terrain: TerrainProfile::Crater { radius_cells: 8 },
    }),
    special_effects: &[SpecialEffect {
        trigger: EffectTrigger::OnImpact,
        kind: EffectKind::Chill,
        // Magnitude: 2 BW of movement reduction = 2 * BODY_WIDTH = 8192
        magnitude: 2 * BODY_WIDTH,
        duration_turns: 1,
    }],
    ranked_enabled: true,
};

/// Mole Drill: tunneling weapon that bores through terrain before detonation.
///
/// **Resolved discrepancy:** ARSENAL.md describes the tunnel as "0.6 BW wide." Applying
/// the same width-is-diameter convention confirmed against the oracle for Trench Spade and
/// Breach Pick below (`radius_cells = round(width_bw / 2 * 4)`) gives
/// `0.6 / 2 * 4 = 1.2` cells, which rounds to `1` — exactly the oracle's `radiusCells: 1`
/// used here. There is no real disagreement between ARSENAL.md and the oracle once the
/// diameter/radius convention is applied consistently; a prior review pass flagged this as
/// a blocker by comparing the raw 0.6 BW figure directly against `radius_cells` without
/// halving it first.
const MOLE_DRILL: WeaponDefinition = WeaponDefinition {
    id: "mole-drill",
    version: 1,
    display_name: "Mole Drill",
    slot: WeaponSlot::Main,
    ammo: AmmoPolicy::Finite { capacity: 2 },
    action_point_cost: 4,
    base_damage: 58,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: 440,
        gravity_per_tick: 13,
        wind_scale_basis_points: 3_500,
        max_ticks: 220,
        terrain: TerrainProfile::Dig {
            radius_cells: 1,
            length_cells: 24,
        },
    }),
    special_effects: &[SpecialEffect {
        trigger: EffectTrigger::OnImpact,
        kind: EffectKind::Tunnel,
        // Magnitude: 6 BW tunnel limit = 6 * BODY_WIDTH = 24576
        magnitude: 6 * BODY_WIDTH,
        duration_turns: 0,
    }],
    ranked_enabled: true,
};

/// Cinder Cluster: multi-hit weapon with persistent ember zones.
const CINDER_CLUSTER: WeaponDefinition = WeaponDefinition {
    id: "cinder-cluster",
    version: 1,
    display_name: "Cinder Cluster",
    slot: WeaponSlot::Main,
    ammo: AmmoPolicy::Finite { capacity: 2 },
    action_point_cost: 4,
    base_damage: 56,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: 390,
        gravity_per_tick: 19,
        wind_scale_basis_points: 10_000,
        max_ticks: 240,
        terrain: TerrainProfile::Crater { radius_cells: 3 },
    }),
    special_effects: &[
        SpecialEffect {
            trigger: EffectTrigger::OnFlight,
            kind: EffectKind::Cluster,
            magnitude: 5, // Five bomblets
            duration_turns: 0,
        },
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Embers,
            magnitude: 5, // Zone damage per turn
            duration_turns: 2,
        },
    ],
    ranked_enabled: true,
};

// ---------------------------------------------------------------------------
// Secondary slot (faster / more precise backup weapons)
// ---------------------------------------------------------------------------

/// Recurve Bow: high wind response, long-range secondary.
const RECURVE_BOW: WeaponDefinition = WeaponDefinition {
    id: "recurve-bow",
    version: 1,
    display_name: "Recurve Bow",
    slot: WeaponSlot::Secondary,
    ammo: AmmoPolicy::Finite { capacity: 5 },
    action_point_cost: 2,
    base_damage: 32,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: 500,
        gravity_per_tick: 16,
        wind_scale_basis_points: 12_000,
        max_ticks: 210,
        terrain: TerrainProfile::None,
    }),
    special_effects: &[],
    ranked_enabled: true,
};

/// Longsword: the only infinite-ammo weapon. Single-target, twice standard melee reach.
///
/// **Longsword Invariant (ARSENAL.md):** This is the only weapon in Dungeon Barrage
/// with infinite ammunition or durability. Roster validation enforces this as a
/// hard constraint.
const LONGSWORD: WeaponDefinition = WeaponDefinition {
    id: "longsword",
    version: 1,
    display_name: "Longsword",
    slot: WeaponSlot::Secondary,
    ammo: AmmoPolicy::Infinite,
    action_point_cost: 2,
    base_damage: 24,
    attack: Attack::Strike(StrikeAttack {
        range: BASE_MELEE_RANGE * 2,
        terrain: TerrainProfile::None,
        self_damage: 0,
    }),
    special_effects: &[],
    ranked_enabled: true,
};

/// Returning Boomerang: curved path attacks around cover, durability per throw.
const RETURNING_BOOMERANG: WeaponDefinition = WeaponDefinition {
    id: "returning-boomerang",
    version: 1,
    display_name: "Returning Boomerang",
    slot: WeaponSlot::Secondary,
    ammo: AmmoPolicy::Finite { capacity: 3 },
    action_point_cost: 2,
    base_damage: 32,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: 430,
        gravity_per_tick: 9,
        wind_scale_basis_points: 8_000,
        max_ticks: 180,
        terrain: TerrainProfile::None,
    }),
    special_effects: &[SpecialEffect {
        trigger: EffectTrigger::OnFlight,
        kind: EffectKind::Return,
        magnitude: 90, // Outbound-and-return path angle (degrees)
        duration_turns: 0,
    }],
    ranked_enabled: true,
};

/// 5.7 Service Pistol: precise, flat ballistics, wind-immune.
///
/// **Reported divergence (unresolved, per `MODULE_OWNERSHIP.md` "report, do not silently
/// pick"):** ARSENAL.md's table calls this weapon the "5.7 Service Pistol." The oracle's
/// `launchWeapons` entry instead uses id `"needle-7"` and display name "Needle-7 Service
/// Pistol." `id` is the stable wire identifier shared with the oracle's `WeaponId` type, so
/// it follows the oracle here for protocol parity — ARSENAL.md does not specify ids at all,
/// so there is no real disagreement on `id`. `display_name` is purely cosmetic (never
/// contributes to the state hash or gameplay, `ARSENAL.md`'s cosmetic boundary) and here
/// follows the oracle rather than ARSENAL.md's table string; that specific choice is a
/// content/branding call for design to confirm, not a gameplay defect.
const SERVICE_PISTOL: WeaponDefinition = WeaponDefinition {
    id: "needle-7",
    version: 1,
    display_name: "Needle-7 Service Pistol",
    slot: WeaponSlot::Secondary,
    ammo: AmmoPolicy::Finite { capacity: 6 },
    action_point_cost: 2,
    base_damage: 26,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: 760,
        gravity_per_tick: 4,
        wind_scale_basis_points: 0, // Wind-immune
        max_ticks: 120,
        terrain: TerrainProfile::None,
    }),
    special_effects: &[],
    ranked_enabled: true,
};

/// Heavy Revolver: high knockback with recoil that displaces the shooter.
///
/// Recoil magnitude: 0.5 BW = `BODY_WIDTH.saturating_div(2)`. `BODY_WIDTH` is a fixed
/// positive constant so this can never actually saturate; the checked-style method keeps
/// the computation consistent with the crate's "checked or saturating arithmetic" rule
/// (`docs/MODULE_OWNERSHIP.md`) without open-coding a raw `/` outside `fixed.rs`, which is
/// documented as the only sanctioned place for scaled division.
const HEAVY_REVOLVER: WeaponDefinition = WeaponDefinition {
    id: "heavy-revolver",
    version: 1,
    display_name: "Heavy Revolver",
    slot: WeaponSlot::Secondary,
    ammo: AmmoPolicy::Finite { capacity: 3 },
    action_point_cost: 3,
    base_damage: 42,
    attack: Attack::Projectile(ProjectileAttack {
        speed_per_tick: 700,
        gravity_per_tick: 5,
        wind_scale_basis_points: 0, // Wind-immune
        max_ticks: 120,
        terrain: TerrainProfile::None,
    }),
    special_effects: &[
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Knockback,
            magnitude: 900,
            duration_turns: 0,
        },
        SpecialEffect {
            trigger: EffectTrigger::OnFire,
            kind: EffectKind::Recoil,
            magnitude: BODY_WIDTH.saturating_div(2), // 0.5 BW backward displacement
            duration_turns: 0,
        },
    ],
    ranked_enabled: true,
};

// ---------------------------------------------------------------------------
// MeleeTool slot (short-range attacks or terrain tools)
// ---------------------------------------------------------------------------

/// Trench Spade: dig tool for close combat, removes soft terrain in a capsule.
///
/// **Derived value:** The ARSENAL specifies a 1.5 × 1.0 BW capsule dig.
/// The perpendicular radius (1.5 BW / 2 = 0.75 BW ≈ 3 cells) matches the oracle.
/// The sweep length (1.0 BW = 4 cells) is derived from ARSENAL but not specified
/// in the oracle's launchWeapons array (oracle strikes omit lengthCells).
const TRENCH_SPADE: WeaponDefinition = WeaponDefinition {
    id: "trench-spade",
    version: 1,
    display_name: "Trench Spade",
    slot: WeaponSlot::MeleeTool,
    ammo: AmmoPolicy::Finite { capacity: 4 },
    action_point_cost: 1,
    base_damage: 22,
    attack: Attack::Strike(StrikeAttack {
        range: BASE_MELEE_RANGE,
        terrain: TerrainProfile::Dig {
            radius_cells: 3,
            length_cells: 4,
        },
        self_damage: 0,
    }),
    special_effects: &[],
    ranked_enabled: true,
};

/// Blood Maul: high-damage melee with unavoidable self-damage.
///
/// **Discrepancy note:** ARSENAL specifies a 0.5 BW-radius impact divot (crater),
/// but the oracle launchWeapons array does not include terrain for Blood Maul.
/// Following ARSENAL.md as source of truth per protocol, the crater is included
/// here. This represents a localized impact effect distinct from the melee range.
const BLOOD_MAUL: WeaponDefinition = WeaponDefinition {
    id: "blood-maul",
    version: 1,
    display_name: "Blood Maul",
    slot: WeaponSlot::MeleeTool,
    ammo: AmmoPolicy::Finite { capacity: 2 },
    action_point_cost: 2,
    base_damage: 52,
    attack: Attack::Strike(StrikeAttack {
        range: BASE_MELEE_RANGE,
        terrain: TerrainProfile::Crater { radius_cells: 2 }, // 0.5 BW ≈ 2 cells
        self_damage: 14,
    }),
    special_effects: &[SpecialEffect {
        trigger: EffectTrigger::OnImpact,
        kind: EffectKind::SelfDamage,
        magnitude: 14,
        duration_turns: 0,
    }],
    ranked_enabled: true,
};

/// Breach Pick: specialized tool for penetrating reinforced stone barriers.
///
/// **Derived value:** The ARSENAL specifies a 0.9 × 0.75 BW pocket.
/// The perpendicular radius (0.9 BW / 2 = 0.45 BW ≈ 2 cells) matches the oracle.
/// The sweep length (0.75 BW ≈ 3 cells) is derived from ARSENAL but not specified
/// in the oracle's launchWeapons array (oracle strikes omit lengthCells).
const BREACH_PICK: WeaponDefinition = WeaponDefinition {
    id: "breach-pick",
    version: 1,
    display_name: "Breach Pick",
    slot: WeaponSlot::MeleeTool,
    ammo: AmmoPolicy::Finite { capacity: 3 },
    action_point_cost: 2,
    base_damage: 30,
    attack: Attack::Strike(StrikeAttack {
        range: BASE_MELEE_RANGE,
        terrain: TerrainProfile::Dig {
            radius_cells: 2,
            length_cells: 3,
        },
        self_damage: 0,
    }),
    special_effects: &[],
    ranked_enabled: true,
};

// ---------------------------------------------------------------------------
// Roster
// ---------------------------------------------------------------------------

/// The complete launch arsenal: 4 main, 5 secondary, 3 melee/tool weapons.
///
/// Static data that requires zero allocation. Order follows the protocol:
/// mains, then secondaries, then melee/tools, to match the oracle's launchWeapons
/// array for deterministic iteration.
pub static LAUNCH_ROSTER: &[WeaponDefinition] = &[
    // Mains (4 weapons)
    RAMSHOT_CANNON,
    FROSTFALL_MORTAR,
    MOLE_DRILL,
    CINDER_CLUSTER,
    // Secondaries (5 weapons)
    RECURVE_BOW,
    LONGSWORD,
    RETURNING_BOOMERANG,
    SERVICE_PISTOL,
    HEAVY_REVOLVER,
    // Melee/Tools (3 weapons)
    TRENCH_SPADE,
    BLOOD_MAUL,
    BREACH_PICK,
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Finds a weapon definition by id, returning a reference to the static definition.
///
/// # Example
/// ```ignore
/// if let Some(weapon) = find("ramshot-cannon") {
///     println!("Damage: {}", weapon.base_damage);
/// }
/// ```
#[must_use]
pub fn find(id: &str) -> Option<&'static WeaponDefinition> {
    LAUNCH_ROSTER.iter().find(|w| w.id == id)
}

/// Exclusive upper bound of the Main section's array positions (`0..MAIN_SECTION_END`).
const MAIN_SECTION_END: usize = 4;

/// Exclusive upper bound of the Secondary section's array positions
/// (`MAIN_SECTION_END..SECONDARY_SECTION_END`). Everything from here on is MeleeTool.
const SECONDARY_SECTION_END: usize = 9;

/// Required number of melee/tool weapons. Kept as its own named constant (rather than
/// derived from the input slice's length) because [`validate`] is also exercised in tests
/// against deliberately short synthetic rosters, where the roster's own length must never
/// be allowed to silently redefine what "correct" means.
const EXPECTED_MELEE_COUNT: usize = 3;

/// Core roster validation, generic over the slice under test.
///
/// Kept separate from [`validate_roster`] purely so the roster's self-consistency rules can
/// be exercised in tests against synthetic malformed rosters, not just the real
/// [`LAUNCH_ROSTER`] (which by construction never exercises any of this function's error
/// paths). See the `tests` module below.
///
/// This function checks:
/// - Every weapon id is unique and non-empty
/// - Every weapon's slot matches its roster section (implies exactly 4 main, 5 secondary,
///   3 melee/tool, and thus that the roster is exactly 12 weapons long)
/// - **EXACTLY ONE** weapon has `AmmoPolicy::Infinite`, and it is the Longsword
/// - Longsword range is exactly 2× `BASE_MELEE_RANGE`
/// - All other melee/tools use a `Strike` attack with range exactly `BASE_MELEE_RANGE`
///   (a melee/tool weapon using a `Projectile` attack is rejected, not silently skipped)
/// - `special_effects.len() <= MAX_SPECIAL_EFFECTS`
/// - `action_point_cost` is in range 1..=4
/// - `base_damage` is non-zero
/// - No weapon has both infinite ammo and a terrain-destroying profile
///
/// # Errors
/// Returns `SimError::InvalidRoster` if any check fails.
fn validate(roster: &[WeaponDefinition]) -> SimResult<()> {
    let mut seen_ids = std::collections::BTreeSet::new();
    let mut main_count: usize = 0;
    let mut secondary_count: usize = 0;
    let mut melee_count: usize = 0;
    let mut infinite_weapons: Vec<&'static str> = Vec::new();

    for (idx, weapon) in roster.iter().enumerate() {
        // Check id is unique and non-empty
        if weapon.id.is_empty() {
            return Err(crate::SimError::InvalidRoster);
        }
        if !seen_ids.insert(weapon.id) {
            return Err(crate::SimError::InvalidRoster);
        }

        // Check slot consistency: mains first, then secondaries, then melee/tools.
        match weapon.slot {
            WeaponSlot::Main => {
                main_count = main_count.saturating_add(1);
                if idx >= MAIN_SECTION_END {
                    return Err(crate::SimError::InvalidRoster);
                }
            }
            WeaponSlot::Secondary => {
                secondary_count = secondary_count.saturating_add(1);
                if !(MAIN_SECTION_END..SECONDARY_SECTION_END).contains(&idx) {
                    return Err(crate::SimError::InvalidRoster);
                }
            }
            WeaponSlot::MeleeTool => {
                melee_count = melee_count.saturating_add(1);
                if idx < SECONDARY_SECTION_END {
                    return Err(crate::SimError::InvalidRoster);
                }
            }
        }

        // Check special effects count
        if weapon.special_effects.len() > MAX_SPECIAL_EFFECTS {
            return Err(crate::SimError::InvalidRoster);
        }

        // Check action point cost: 1..=4 per ARSENAL.md guardrail 4
        if !(1..=4).contains(&weapon.action_point_cost) {
            return Err(crate::SimError::InvalidRoster);
        }

        // Check base damage is non-zero
        if weapon.base_damage == 0 {
            return Err(crate::SimError::InvalidRoster);
        }

        // Track infinite-ammo weapons, and forbid infinite ammo combined with terrain
        // destruction in the same pass (both conditions key off the same `Infinite` check).
        if matches!(weapon.ammo, AmmoPolicy::Infinite) {
            infinite_weapons.push(weapon.id);
            let destroys_terrain = match weapon.attack {
                Attack::Projectile(proj) => !matches!(proj.terrain, TerrainProfile::None),
                Attack::Strike(strike) => !matches!(strike.terrain, TerrainProfile::None),
            };
            if destroys_terrain {
                return Err(crate::SimError::InvalidRoster);
            }
        }
    }

    // Check exact slot counts. This is the point at which the loop above's per-item
    // position checks are known to have all passed, so this also certifies the roster is
    // exactly MAIN_SECTION_END + (SECONDARY_SECTION_END - MAIN_SECTION_END) +
    // EXPECTED_MELEE_COUNT weapons long.
    if main_count != MAIN_SECTION_END
        || secondary_count != SECONDARY_SECTION_END.saturating_sub(MAIN_SECTION_END)
        || melee_count != EXPECTED_MELEE_COUNT
    {
        return Err(crate::SimError::InvalidRoster);
    }

    // **Longsword Invariant:** Exactly one infinite-ammo weapon, and it is the Longsword.
    let has_only_longsword_infinite =
        infinite_weapons.len() == 1 && infinite_weapons.first().is_some_and(|id| *id == "longsword");
    if !has_only_longsword_infinite {
        return Err(crate::SimError::InvalidRoster);
    }

    // Longsword range must be exactly 2× BASE_MELEE_RANGE.
    let longsword = roster
        .iter()
        .find(|w| w.id == "longsword")
        .ok_or(crate::SimError::InvalidRoster)?;

    let Attack::Strike(longsword_strike) = longsword.attack else {
        return Err(crate::SimError::InvalidRoster);
    };
    let expected_longsword_range = BASE_MELEE_RANGE
        .checked_mul(2)
        .ok_or(crate::SimError::InvalidRoster)?;
    if longsword_strike.range != expected_longsword_range {
        return Err(crate::SimError::InvalidRoster);
    }

    // Every other melee/tool must use a Strike attack with range exactly BASE_MELEE_RANGE.
    // A melee/tool weapon that used a Projectile attack instead of Strike must also be
    // rejected here, not silently pass through unchecked.
    for weapon in roster {
        if weapon.slot != WeaponSlot::MeleeTool || weapon.id == "longsword" {
            continue;
        }
        let Attack::Strike(strike) = weapon.attack else {
            return Err(crate::SimError::InvalidRoster);
        };
        if strike.range != BASE_MELEE_RANGE {
            return Err(crate::SimError::InvalidRoster);
        }
    }

    Ok(())
}

/// Validates the launch roster at load time, enforcing gameplay invariants.
///
/// See [`validate`] for the full list of checks. The roster is proven valid by the
/// `roster_is_valid` test below, so a bad roster should never reach a player.
///
/// # Errors
/// Returns `SimError::InvalidRoster` if any check fails.
///
/// # Panics
/// Never panics on correct roster data. Returns `SimError::InvalidRoster` on any
/// validation failure.
pub fn validate_roster() -> SimResult<()> {
    validate(LAUNCH_ROSTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Fixtures for the negative-path tests below.
    //
    // `validate_roster()` only ever runs against the real `LAUNCH_ROSTER`, which by
    // construction never exercises any of `validate`'s error paths. To prove those paths
    // actually reject what they claim to reject (rather than merely never triggering),
    // these tests clone the real, valid roster and corrupt exactly one field per test, so
    // each test isolates exactly one invariant. Helpers use `.get_mut()` rather than
    // indexing, matching the crate's `indexing_slicing = "deny"` rule.
    // -----------------------------------------------------------------------

    fn set_id(roster: &mut [WeaponDefinition], idx: usize, id: &'static str) {
        if let Some(weapon) = roster.get_mut(idx) {
            weapon.id = id;
        }
    }

    fn set_slot(roster: &mut [WeaponDefinition], idx: usize, slot: WeaponSlot) {
        if let Some(weapon) = roster.get_mut(idx) {
            weapon.slot = slot;
        }
    }

    fn set_attack(roster: &mut [WeaponDefinition], idx: usize, attack: Attack) {
        if let Some(weapon) = roster.get_mut(idx) {
            weapon.attack = attack;
        }
    }

    fn set_ammo(roster: &mut [WeaponDefinition], idx: usize, ammo: AmmoPolicy) {
        if let Some(weapon) = roster.get_mut(idx) {
            weapon.ammo = ammo;
        }
    }

    #[test]
    fn roster_is_valid() {
        assert!(validate_roster().is_ok(), "Launch roster must be valid");
    }

    #[test]
    fn roster_has_exactly_twelve_weapons() {
        assert_eq!(LAUNCH_ROSTER.len(), 12);
    }

    #[test]
    fn all_main_weapons_come_first() {
        for (idx, weapon) in LAUNCH_ROSTER.iter().enumerate() {
            if idx < MAIN_SECTION_END {
                assert_eq!(weapon.slot, WeaponSlot::Main);
            } else {
                assert_ne!(weapon.slot, WeaponSlot::Main);
            }
        }
    }

    #[test]
    fn all_secondary_weapons_come_second() {
        for (idx, weapon) in LAUNCH_ROSTER.iter().enumerate() {
            if (MAIN_SECTION_END..SECONDARY_SECTION_END).contains(&idx) {
                assert_eq!(weapon.slot, WeaponSlot::Secondary);
            } else if idx >= MAIN_SECTION_END {
                assert_ne!(weapon.slot, WeaponSlot::Secondary);
            }
        }
    }

    #[test]
    fn all_melee_tools_come_last() {
        for (idx, weapon) in LAUNCH_ROSTER.iter().enumerate() {
            if idx >= SECONDARY_SECTION_END {
                assert_eq!(weapon.slot, WeaponSlot::MeleeTool);
            } else {
                assert_ne!(weapon.slot, WeaponSlot::MeleeTool);
            }
        }
    }

    #[test]
    fn all_weapon_ids_are_unique() {
        let mut ids = std::collections::BTreeSet::new();
        for weapon in LAUNCH_ROSTER {
            assert!(ids.insert(weapon.id), "Duplicate weapon id: {}", weapon.id);
        }
    }

    #[test]
    fn all_weapon_ids_are_non_empty() {
        for weapon in LAUNCH_ROSTER {
            assert!(!weapon.id.is_empty());
        }
    }

    #[test]
    fn longsword_is_only_infinite_weapon() {
        let infinite: Vec<_> = LAUNCH_ROSTER
            .iter()
            .filter(|w| matches!(w.ammo, AmmoPolicy::Infinite))
            .collect();
        assert_eq!(infinite.len(), 1);
        let Some(weapon) = infinite.first() else {
            unreachable!("Expected one infinite weapon");
        };
        assert_eq!(weapon.id, "longsword");
    }

    #[test]
    fn longsword_range_is_exactly_double_base_melee() {
        let Some(longsword) = LAUNCH_ROSTER.iter().find(|w| w.id == "longsword") else {
            unreachable!("Longsword must exist in roster");
        };
        let Attack::Strike(strike) = longsword.attack else {
            unreachable!("Longsword must use Strike attack");
        };
        assert_eq!(strike.range, BASE_MELEE_RANGE * 2);
    }

    #[test]
    fn all_other_melee_tools_have_base_melee_range() {
        for weapon in LAUNCH_ROSTER {
            if weapon.slot != WeaponSlot::MeleeTool || weapon.id == "longsword" {
                continue;
            }
            let Attack::Strike(strike) = weapon.attack else {
                unreachable!("{} must use Strike attack", weapon.id);
            };
            assert_eq!(
                strike.range, BASE_MELEE_RANGE,
                "{} must have BASE_MELEE_RANGE",
                weapon.id
            );
        }
    }

    #[test]
    fn special_effects_within_limit() {
        for weapon in LAUNCH_ROSTER {
            assert!(
                weapon.special_effects.len() <= MAX_SPECIAL_EFFECTS,
                "{} has too many special effects",
                weapon.id
            );
        }
    }

    #[test]
    fn action_point_costs_in_valid_range() {
        for weapon in LAUNCH_ROSTER {
            assert!(
                (1..=4).contains(&weapon.action_point_cost),
                "{} AP cost out of range: {}",
                weapon.id,
                weapon.action_point_cost
            );
        }
    }

    #[test]
    fn all_weapons_have_nonzero_damage() {
        for weapon in LAUNCH_ROSTER {
            assert!(
                weapon.base_damage > 0,
                "{} has zero base damage",
                weapon.id
            );
        }
    }

    #[test]
    fn no_infinite_ammo_with_terrain_destruction() {
        for weapon in LAUNCH_ROSTER {
            if !matches!(weapon.ammo, AmmoPolicy::Infinite) {
                continue;
            }
            match weapon.attack {
                Attack::Projectile(proj) => {
                    assert!(
                        matches!(proj.terrain, TerrainProfile::None),
                        "{} has infinite ammo but destroys terrain",
                        weapon.id
                    );
                }
                Attack::Strike(strike) => {
                    assert!(
                        matches!(strike.terrain, TerrainProfile::None),
                        "{} has infinite ammo but destroys terrain",
                        weapon.id
                    );
                }
            }
        }
    }

    #[test]
    fn find_retrieves_weapons_by_id() {
        assert!(find("ramshot-cannon").is_some());
        assert!(find("longsword").is_some());
        assert!(find("trench-spade").is_some());
        assert!(find("nonexistent-weapon").is_none());
    }

    #[test]
    fn find_rejects_empty_id() {
        assert!(find("").is_none());
    }

    #[test]
    fn find_returns_correct_weapon_data() {
        let Some(ramshot) = find("ramshot-cannon") else {
            unreachable!("Ramshot must exist");
        };
        assert_eq!(ramshot.base_damage, 62);
        assert_eq!(ramshot.action_point_cost, 3);
        assert_eq!(ramshot.slot, WeaponSlot::Main);
    }

    #[test]
    fn finite_weapons_have_positive_capacity() {
        for weapon in LAUNCH_ROSTER {
            if let AmmoPolicy::Finite { capacity } = weapon.ammo {
                assert!(capacity > 0, "{} has zero capacity", weapon.id);
            }
        }
    }

    #[test]
    fn cinder_cluster_has_exactly_two_special_effects() {
        let Some(cinder) = find("cinder-cluster") else {
            unreachable!("Cinder Cluster must exist");
        };
        assert_eq!(cinder.special_effects.len(), 2);
    }

    #[test]
    fn heavy_revolver_has_exactly_two_special_effects() {
        let Some(revolver) = find("heavy-revolver") else {
            unreachable!("Heavy Revolver must exist");
        };
        assert_eq!(revolver.special_effects.len(), 2);
    }

    #[test]
    fn blood_maul_self_damage_matches_magnitude() {
        let Some(blood_maul) = find("blood-maul") else {
            unreachable!("Blood Maul must exist");
        };
        let Attack::Strike(strike) = blood_maul.attack else {
            unreachable!("Blood Maul must be a strike");
        };
        assert_eq!(strike.self_damage, 14);
        let Some(effect) = blood_maul.special_effects.first() else {
            unreachable!("Blood Maul must have a special effect");
        };
        assert_eq!(effect.magnitude, 14);
    }

    // -----------------------------------------------------------------------
    // Negative-path tests: `validate` must reject each of these, not merely never
    // encounter them. Every fixture starts from a full clone of the real, valid roster so
    // each test isolates exactly one broken invariant.
    // -----------------------------------------------------------------------

    #[test]
    fn rejects_duplicate_weapon_id() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        set_id(&mut roster, 1, "ramshot-cannon"); // collides with index 0
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_empty_weapon_id() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        set_id(&mut roster, 0, "");
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_slot_position_mismatch() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        // Index 0 is a Main position; relabeling its slot without moving it desyncs the
        // declared slot from the section it sits in.
        set_slot(&mut roster, 0, WeaponSlot::Secondary);
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_wrong_melee_tool_count() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        roster.pop(); // drops Breach Pick, leaving only 2 melee/tools instead of 3
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_two_infinite_weapons() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        set_ammo(&mut roster, 4, AmmoPolicy::Infinite); // Recurve Bow, alongside Longsword
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_infinite_weapon_that_is_not_longsword() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        set_id(&mut roster, 5, "long-blade"); // Longsword's slot, renamed
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_longsword_with_wrong_range() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        set_attack(
            &mut roster,
            5,
            Attack::Strike(StrikeAttack {
                range: BASE_MELEE_RANGE, // standard reach instead of the mandated 2x
                terrain: TerrainProfile::None,
                self_damage: 0,
            }),
        );
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_other_melee_tool_with_wrong_range() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        set_attack(
            &mut roster,
            9, // Trench Spade
            Attack::Strike(StrikeAttack {
                range: BASE_MELEE_RANGE.saturating_mul(2),
                terrain: TerrainProfile::None,
                self_damage: 0,
            }),
        );
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_melee_tool_using_projectile_attack() {
        // A melee/tool weapon that used a Projectile attack instead of Strike must be
        // rejected outright, not silently skipped by the range check.
        let mut roster = LAUNCH_ROSTER.to_vec();
        set_attack(
            &mut roster,
            9, // Trench Spade
            Attack::Projectile(ProjectileAttack {
                speed_per_tick: 1,
                gravity_per_tick: 0,
                wind_scale_basis_points: 0,
                max_ticks: 1,
                terrain: TerrainProfile::None,
            }),
        );
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_special_effects_over_limit() {
        const TOO_MANY: [SpecialEffect; MAX_SPECIAL_EFFECTS + 1] = [SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Knockback,
            magnitude: 1,
            duration_turns: 0,
        }; MAX_SPECIAL_EFFECTS + 1];
        let mut roster = LAUNCH_ROSTER.to_vec();
        if let Some(weapon) = roster.get_mut(0) {
            weapon.special_effects = &TOO_MANY;
        }
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_action_point_cost_below_one() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        if let Some(weapon) = roster.get_mut(0) {
            weapon.action_point_cost = 0;
        }
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_action_point_cost_above_four() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        if let Some(weapon) = roster.get_mut(0) {
            weapon.action_point_cost = 5;
        }
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_zero_base_damage() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        if let Some(weapon) = roster.get_mut(0) {
            weapon.base_damage = 0;
        }
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }

    #[test]
    fn rejects_infinite_ammo_with_terrain_destruction() {
        let mut roster = LAUNCH_ROSTER.to_vec();
        set_attack(
            &mut roster,
            5, // Longsword: still infinite ammo, still the correct 2x range
            Attack::Strike(StrikeAttack {
                range: BASE_MELEE_RANGE.saturating_mul(2),
                terrain: TerrainProfile::Crater { radius_cells: 1 },
                self_damage: 0,
            }),
        );
        assert_eq!(validate(&roster), Err(crate::SimError::InvalidRoster));
    }
}
