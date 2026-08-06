# Dungeon Barrage Arsenal

This document defines the launch arsenal and the balance contract for character loadouts. It is gameplay data, not a cosmetic catalogue.

## Loadout and slot philosophy

Every character equips exactly one weapon from each of three mutually exclusive slots:

- **Main:** a map-scale ballistic weapon with finite ammunition, meaningful terrain interaction, and a defining special effect.
- **Secondary:** a faster or more precise backup weapon with limited terrain impact. This is the player-facing name for the off-hand slot because the category includes bows, swords, and handguns.
- **Melee/Tool:** a short-range attack or terrain tool whose limited durability pays for digging, breaching, displacement, or high-risk damage.

The three slots should create a tactical triangle. Mains reshape the battle, secondaries finish or pressure exposed targets, and melee/tools solve close-range or terrain problems. No weapon may occupy multiple slots, and a loadout cannot equip two definitions from the same slot.

## Shared combat scale

- Standard health: **200 HP**.
- `BW` means one character body width and is the map-independent unit used for range and terrain dimensions.
- Each turn grants **4 AP**.
- Moving up to 2 BW costs 1 AP; moving more than 2 BW and up to 4 BW costs 2 AP. A jump costs 1 AP.
- Aiming, changing facing, and switching between the three equipped weapons are free until an action is committed.
- A weapon action consumes its listed AP and ends the turn. Remaining AP cannot be used after firing, and a player cannot combine two attacks in one turn.
- Ammunition and durability are per match. Melee/tool durability is represented by the same authoritative charge counter as ammunition.
- Splash damage, recoil, friendly fire, and explicit self-damage can eliminate the acting character.
- Damage in the tables is total damage to an exact direct hit. A value in parentheses is the maximum splash damage before radial falloff, not additional damage.

## Main weapons

All main weapons use angle-and-power aiming and are affected by wind according to their projectile definition. Every main has a readable special effect and finite ammunition.

| Weapon | Ammo | AP | Damage | Effective range | Terrain | Special |
|---|---:|---:|---:|---:|---|---|
| **Ramshot Cannon** | 3 | 3 | 62 (48) | Map-wide ballistic | Removes a 1.6 BW-radius crater. | **Concussion:** strongest launch-arsenal knockback, but only average terrain removal. It is the dependable calibration weapon. |
| **Frostfall Mortar** | 3 | 3 | 48 (42) | 6-28 BW, high arc | Removes a 1.9 BW-radius crater. | **Chill:** a direct hit or inner-radius blast reduces the victim's next-turn movement cap by 2 BW. Chill lasts one affected turn and cannot stack. |
| **Mole Drill** | 2 | 4 | 58 (44) | 4-24 BW | Bores a tunnel up to 6 BW long and 0.6 BW wide through soil or wood, then removes a 1.2 BW-radius crater. | **Undermine:** detonates on emerging from terrain, striking a unit, or reaching its tunnel limit. It cannot penetrate reinforced stone. |
| **Cinder Cluster** | 2 | 4 | 16 per bomblet; 56 per-target impact cap | 8-28 BW | Splits into five bomblets, each removing a 0.7 BW-radius crater. | **Embers:** inner impacts leave clearly marked contact zones for two turns. A unit touching a zone takes 5 damage at turn start. Multiple zones do not multiply this damage. |

## Secondary weapons

Secondaries trade the terrain control and blast radius of mains for lower AP cost, precision, unusual trajectories, or resource reliability.

| Weapon | Ammo | AP | Damage | Effective range | Terrain | Special |
|---|---:|---:|---:|---:|---|---|
| **Recurve Bow** | 5 | 2 | 32 | 2-18 BW ballistic | None; embedded arrows are visual only. | High wind response and projectile drop make it a high-skill long-range secondary. |
| **Longsword** | Infinite | 2 | 24 | 2.5 BW | None | A single-target facing arc with modest knockback. Its reach is exactly twice the standard 1.25 BW melee reach. |
| **Returning Boomerang** | 3 durability | 2 | 18 per pass; 32 per-target cap | Up to 12 BW outbound | None | Its curved outbound-and-return path can attack around cover. Every throw consumes one durability even when the boomerang is visually caught. |
| **5.7 Service Pistol** | 6 | 2 | 26, falling to 20 at maximum range | 12 BW | None | Precise, flat, and unaffected by wind. The generic name and original visual design avoid copying a branded commercial handgun. |
| **Heavy Revolver** | 3 | 3 | 42 | 8 BW | None | High target knockback. Recoil moves the shooter 0.5 BW opposite the shot, creating deliberate ledge risk. |

## Melee/tool weapons

Standard melee reach is **1.25 BW**. Their charges represent wear or expendable power and do not regenerate during a match.

| Weapon | Durability | AP | Damage | Effective range | Terrain | Special |
|---|---:|---:|---:|---:|---|---|
| **Trench Spade** | 4 | 1 | 22 Strike / 12 Dig | 1.25 BW | Dig removes a 1.5 x 1.0 BW capsule from soil or wood. | Choose Strike or Dig. Dig may clip one adjacent enemy for its lower damage value. |
| **Blood Maul** | 2 | 2 | 52 to target; 14 to user | 1.25 BW | Removes a small 0.5 BW-radius impact divot. | High-risk damage. Self-damage is simultaneous, unavoidable, cannot be shielded, and can eliminate the user. |
| **Breach Pick** | 3 | 2 | 30 Strike / 16 Breach | 1.25 BW | Breach removes a 0.9 x 0.75 BW pocket from reinforced stone. | Gives close-range access through material that resists both the Trench Spade and Mole Drill. |

## Longsword invariant

The Longsword is the **only weapon in Dungeon Barrage with infinite ammunition or durability**. This is a gameplay invariant, not a launch-season tuning choice.

To preserve it:

- Boomerangs consume durability even when caught.
- Every main, firearm, bow, and melee/tool has a finite server-owned charge count.
- No skin, perk, character cosmetic, pickup, or crafting modifier may create another infinite-use weapon.
- The Longsword remains single-target, terrain-neutral, lower-damage than finite burst options, and dependent on close exposure.
- Its 2.5 BW reach is exactly double standard melee range; skins cannot lengthen its blade socket or attack volume.

If a finite loadout runs dry, its exhausted slots remain unavailable. It does not receive an undocumented infinite basic attack.

## Cosmetic weapon-skin boundary

Weapon identity and appearance are separate records: an immutable/versioned `weaponId` owns gameplay, while `skinId` selects presentation.

A skin may change:

- Weapon sprite or model art within the approved silhouette envelope.
- Palette, material, trail colour, muzzle flash, impact decal, sound set, and inspect animation.
- Non-gameplay naming and rarity presentation.

A skin must never change:

- Damage, splash, status, knockback, self-damage, ammunition, durability, AP cost, or wind response.
- Projectile speed, gravity, trajectory, lifetime, fuse, split timing, collision, or terrain operation.
- Muzzle socket, character pivot, hitbox, attack reach, animation timing, or input window.
- Projectile readability or audio telegraph enough to hide which weapon was fired.

Effects must remain visible against light and dark terrain and understandable without colour alone. Competitive clients may substitute standardized effects without changing gameplay.

## Balance guardrails

1. A full-health character must not die to one ordinary weapon hit. Environmental elimination is the intentional exception.
2. No main weapon may rank above average in more than two of damage, accuracy, crater size, knockback, range, and AP efficiency.
3. Multi-hit weapons enforce their per-target cap after all subprojectile contacts; separate impact events cannot bypass it.
4. Status effects last no more than one affected turn, do not stack, and never remove an entire turn.
5. The Longsword stays below finite-secondary burst damage and cannot damage terrain or strike multiple targets.
6. Blood Maul self-damage resolves simultaneously with target damage and bypasses ordinary shields or damage conversion.
7. Damage radius, terrain radius, and knockback radius are separately tunable. Increasing one does not silently increase the others.
8. Terrain operations enforce a minimum stable feature thickness so explosions cannot create invisible collision slivers.
9. Ranked ammunition pickups are disabled or deterministic. Casual pickups may restore one charge to a finite weapon but never alter its maximum.
10. The server owns loadout validation, charge counts, damage, terrain changes, statuses, and action completion. Definitions are versioned per match for deterministic replays.
11. Spawn protection and map validation must prevent an unanswerable first-turn environmental elimination.
12. After sufficient telemetry, target slot-adjusted win rates of approximately 48-52 percent. Investigate weapons outside 45-55 percent or with extreme pick rates after controlling for player skill and map.

These values are the launch baseline. Balance patches may publish new versioned definitions, but they must preserve the slot philosophy, cosmetic boundary, and Longsword invariant.
