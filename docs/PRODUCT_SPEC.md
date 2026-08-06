# Dungeon Barrage Product Specification

**Status:** Implementation baseline  
**Product:** Dungeon Barrage  
**Canonical launch surface:** Desktop website and installable PWA  
**Document purpose:** Define the playable product, rules, content boundaries, and acceptance criteria. Platform packaging and deployment details live in [PLATFORM_STRATEGY.md](./PLATFORM_STRATEGY.md). Exact launch weapon values live in [ARSENAL.md](./ARSENAL.md); when a general example here conflicts with a weapon row there, `ARSENAL.md` governs the weapon value.

## 1. Product definition

Dungeon Barrage is a turn-based artillery tactics game set in destructible side-view dungeons. Each player brings a customizable sprite and a three-part loadout, then uses movement, angle, power, wind, terrain destruction, direct damage, and displacement to outplay opponents.

The game should feel easy to read on a first turn and deep after dozens of matches. A shot may create value by dealing damage, opening a tunnel, removing cover, pushing an opponent toward a hazard, or changing the geometry for a later turn.

### Player promise

- Every shot changes the tactical situation.
- Loadouts create distinct play styles without selling competitive power.
- Characters and weapons are highly personalizable without changing collision or reach.
- Resolution is spectacular, readable, and trustworthy.
- A complete match fits into a short play session.

### Initial audience

- Players who enjoy Worms-like artillery tactics, trick shots, and destructible terrain.
- Players who enjoy layered avatar and equipment customization.
- Social groups looking for short private matches.
- Strategy players who prefer prediction and positioning over reaction speed.

### Product pillars

1. **Ballistic mastery.** Angle, power, wind, projectile behavior, and terrain geometry should reward practice.
2. **Terrain is game state.** Digging, craters, tunnels, ledges, and hazards matter as much as raw damage.
3. **Three-slot expression.** Main, Secondary, and Melee/Tool choices must answer different tactical problems.
4. **Readable spectacle.** The pause between commitment and impact is part of the fun; effects must never obscure the result.
5. **Cosmetic identity.** The avatar and weapon-skin system supports many visual combinations while gameplay remains fair.
6. **Authoritative fairness.** The server, not a player's browser, decides timers, ammunition, collisions, damage, terrain changes, and rewards.

### Non-goals for the first release

- Full three-dimensional movement or voxel terrain.
- Large sequential battle-royale lobbies.
- Crafting, vehicles, ropes, jetpacks, or free-form grappling.
- Paid character statistics, paid damage bonuses, or paid ammunition advantages.
- A player-authored weapon scripting language.
- User-uploaded cosmetics.
- Simultaneous release on website, Chrome Web Store, Steam, and consoles.

## 2. Game format

### Presentation and simulation

- Gameplay is authoritative two-dimensional simulation on a side-view plane.
- Backgrounds, lighting, particles, and character shading may create a 2.5D appearance.
- Render meshes, sprite silhouettes, and cosmetic layers never define collision.
- Destructible terrain uses an occupancy mask and ordered terrain operations rather than full 3D geometry.

### Match sizes and modes

| Stage | Players | Modes | Match target |
|---|---:|---|---:|
| Vertical slice | 1 player versus bot; private 1v1 | Training, duel | 5-10 minutes |
| MVP | 2-4 players | Training, free-for-all, 2v2 | 8-15 minutes |
| Post-MVP | Mode-dependent | Ranked, squad, challenges | Defined per mode |

The MVP uses one active character per player. Multi-character squads are deferred because they multiply turn wait time, content requirements, and state complexity.

### Provisional match tuning

These values are initial playtest defaults, not permanent balance promises.

| Rule | Initial value |
|---|---:|
| Character health | 200 HP |
| Planning timer | 25 seconds |
| Turn action budget | 4 AP |
| Walk up to 2 body widths | 1 AP |
| Walk over 2 and up to 4 body widths | 2 AP |
| Jump | 1 AP |
| Committed attacks | One per turn |
| Typical resolution | 2-6 seconds |
| Maximum unresolved projectile lifetime | 8 seconds |
| Post-impact settle window | Up to 2 seconds |
| Sudden-death trigger | Turn 12 in a duel |
| Sudden-death behavior | Rising hazard plus increasing damage pressure |

### Turn state machine

```text
MATCH_INTRO
-> TURN_START
-> MOVEMENT
-> AIMING_AND_SELECTION
-> COMMAND_LOCKED
-> PROJECTILE_OR_MELEE_RESOLUTION
-> TERRAIN_AND_DAMAGE_RESOLUTION
-> SETTLING
-> STATUS_RESOLUTION
-> VICTORY_CHECK
-> NEXT_TURN or MATCH_COMPLETE
```

Rules:

- Fixed alternating or clockwise turns ship first.
- Movement and aim may be revised until an attack is committed or the timer expires.
- A weapon consumes its listed AP and ends the turn; unused AP cannot be spent afterward.
- The server applies a deterministic timeout action when no valid command arrives.
- Gunbound-like initiative delay is a post-MVP ruleset, not an MVP dependency.
- Randomness is seeded, bounded, and visible where it affects competitive decisions.

### Victory and elimination

- The standard victory condition is last player or team standing.
- A character is eliminated at zero health or after contact with a lethal world hazard.
- Unrecoverable falls resolve consistently and visibly; the camera must show the elimination cause.
- Sudden death must bound a stalled match's duration.

## 3. Loadout contract

Every character equips exactly one item in each of three slots before entering a match.

| Slot | Product term | Tactical job | Core constraint |
|---|---|---|---|
| 1 | **Main** | Signature artillery attack with a special effect | Limited ammunition; strongest terrain or area influence |
| 2 | **Secondary** | Reliable alternate attack or unusual trajectory | Includes bows, handguns, the boomerang, and the extended-reach longsword |
| 3 | **Melee/Tool** | Close-range damage, digging, breaching, or risky burst | Standard reach is 1.25 body widths unless explicitly modified |

“Off-hand” may appear in player-facing flavor text, but the stable product and data term is `secondary`. The third slot uses `meleeTool` so digging implements and combat tools share one unambiguous category.

### Equip invariants

- A saved or submitted loadout is invalid unless all three slots contain an allowed definition.
- A weapon definition belongs to one gameplay slot only.
- Skins never satisfy a slot by themselves; a skin decorates an owned weapon definition.
- A player cannot swap equipped definitions after a match begins unless a future mode explicitly allows it.
- Ranked rules, when added, expose the complete competitive weapon set without payment or excessive grind.
- Server content versions determine the legal definitions and balance values for each match.

### Ammunition policies

| Policy | Behavior |
|---|---|
| `finite` | Decrement once, and only once, after the server accepts the attack command |
| `unlimited` | Never decrements and displays an infinity symbol instead of a number |
| `cooldown` | Not used in the vertical slice; reserved for later modes |

The Longsword is the only weapon in the game with `unlimited` ammunition or durability. Its 2.5-body-width effective reach is exactly twice the standard 1.25-body-width Melee/Tool reach. Unlimited ammunition does not mean unlimited actions; it still consumes AP and the turn's one committed attack.

### Weapon behavior vocabulary

Weapon definitions are versioned data that reference a small set of reviewed behaviors. They do not contain remotely downloaded executable code.

```ts
type LoadoutSlot = "main" | "secondary" | "meleeTool";
type AmmoPolicy = "finite" | "unlimited" | "cooldown";

interface WeaponDefinition {
  id: string;
  version: number;
  displayName: string;
  slot: LoadoutSlot;
  ammoPolicy: AmmoPolicy;
  startingAmmo?: number;
  rangeClass: "projectile" | "melee" | "extendedMelee";
  behaviorId: string;
  damage: DamageConfig;
  terrain: TerrainConfig;
  tags: string[];
  rankedEnabled: boolean;
}
```

The initial behavior vocabulary is:

- `ballisticImpact`
- `highArcImpact`
- `drillThenDetonate`
- `bounceThenDetonate`
- `returnToOwner`
- `directProjectile`
- `meleeArc`
- `subtractTerrain`
- `applyStatus`
- `applyKnockback`
- `applyBacklash`

Damage radius, terrain radius, and knockback radius are separate values. No weapon should simultaneously provide the best damage, accuracy, crater size, and displacement.

### IP-safe target arsenal

The names below are product names, not references to real manufacturers or models.

#### Vertical-slice arsenal

| Slot | Weapon | Purpose | Signature rule |
|---|---|---|---|
| Main | Ramshot Cannon | Dependable calibration and displacement | Concussion supplies the launch arsenal's strongest knockback |
| Main | Mole Drill | Protected-target attack | Bores through soil or wood before detonating |
| Secondary | Returning Boomerang | Curved return path | Can damage on outbound and return paths, with a per-target cap |
| Secondary | Longsword | Reliable close attack | Unlimited ammunition and exactly 2x standard melee reach |
| Melee/Tool | Trench Spade | Terrain shaping | Chooses a normal Strike or a capsule-shaped Dig action |
| Melee/Tool | Blood Maul | Risk-reward burst | Deals high target damage and an unavoidable, clearly disclosed Backlash cost to its user |

This set proves the hardest reusable systems: ballistic calibration, displacement, terrain traversal, return trajectories, extended melee, digging, and self-cost.

#### First-release Secondary catalog

| Product name | Archetype | Notes |
|---|---|---|
| Recurve Bow | Bow | Ballistic precision weapon with high wind response |
| Longsword | Sword | Unlimited ammunition; extended melee reach |
| Returning Boomerang | Boomerang | Returning projectile with outbound/return hit rules and finite durability |
| 5.7 Service Pistol | Lightweight pistol | Generic caliber description and an original fictional visual design |
| Heavy Revolver | Revolver | Fictional high-knockback archetype with limited ammunition and recoil displacement |

Do not use `Five-seveN`, `FN Five-seveN`, manufacturer logos, copied model geometry, or manufacturer-specific finish patterns. “5.7 Service Pistol” is a generic caliber/archetype description and must use an original fictional silhouette. “Heavy Revolver” replaces “Magnum revolver” as the display name; generic archetype terms may remain in internal tags.

### Backlash language

The UI groups explicit user damage under **Backlash** and may use more specific language such as recoil damage where appropriate. Art and copy must not frame the action as self-harm. The preview shows the exact cost before commitment. The Blood Maul's 14 self-damage resolves simultaneously, bypasses ordinary shields, and may eliminate its user as defined in `ARSENAL.md`.

## 4. Projectile, damage, and terrain rules

### Projectile model

- Simulation advances at a fixed step independent of rendering frame rate.
- Angle, power, position, velocity, and wind are quantized at protocol boundaries.
- Each weapon defines wind influence and may use a reviewed state machine for special behavior.
- Training may show a full predicted trajectory; competitive modes show only a short guide segment.
- A client prediction is advisory. Only the authoritative result confirms impact.

### Damage and displacement

- Direct-hit bonus, splash falloff, status damage, Backlash, and hazard damage are itemized separately.
- Knockback uses a separate curve from damage.
- Friendly fire is enabled in team play unless a mode definition says otherwise.
- A result panel and replay event must identify the elimination cause.

### Terrain

- Terrain consists of a visual texture, an authoritative occupancy mask, material tags, and ordered operations.
- Initial materials are `soil`, `wood`, and `reinforcedStone`.
- `soil` and `wood` are broadly destructible; only the Breach Pick removes `reinforcedStone` in the launch arsenal.
- Initial operations are `subtractCircle`, `subtractCapsule`, and `subtractPolygon`.
- The Trench Spade uses `subtractCapsule`; explosions normally use `subtractCircle`.
- Visual debris is cosmetic and cannot deal unreported gameplay damage.
- Collision rebuilding is limited to dirty regions.

## 5. Avatar and skin system

The customization model is an original layered paper-doll system. MapleStory and Dungeon Fighter Online may be used internally only as high-level references for player choice; they are not visual templates.

### Appearance versus gameplay

```ts
interface CharacterAppearance {
  bodyId: string;
  skinToneId: string;
  faceId: string;
  hairBackId?: string;
  hairFrontId?: string;
  headwearId?: string;
  outfitId: string;
  accessoryBackId?: string;
  accessoryFrontId?: string;
  paletteOverrides?: Record<string, string>;
}

interface CosmeticLoadout {
  mainSkinId: string;
  secondarySkinId: string;
  meleeToolSkinId: string;
  impactEffectId?: string;
  victoryPoseId?: string;
}
```

Appearance and cosmetic data never contain health, movement, damage, ammunition, reach, projectile, or collision values.

### Layer order

The default side-facing order is:

```text
back accessory
hair back
rear arm
body
outfit
face
hair front
headwear
front arm
held weapon skin
front accessory
combat effect
```

Individual poses may define reviewed z-order overrides, but a cosmetic may not invent its own animation timing.

### Shared animation contract

All wearable layers for a body rig provide matching frame tags and pivots:

- `idle`
- `walk`
- `aim`
- `charge`
- `fire`
- `melee`
- `hit`
- `fall`
- `victory`
- `defeat`

Every frame exposes a ground pivot plus `mainHand`, `offHand`, `muzzle`, and `effectOrigin` sockets where relevant. Left-facing presentation may mirror approved layers; asymmetrical art requires explicit left/right variants.

### Cosmetic fairness rules

- All cosmetics use the same character collision capsule.
- Weapon skins use the base weapon's reach, projectile origin, and hit tests.
- No skin may hide the active weapon class or materially reduce a silhouette's readability.
- Muzzle flashes and projectile skins must preserve timing and collision readability.
- Rarity affects presentation and acquisition only.
- Competitive effects provide a reduced-motion and high-clarity fallback.

### Vertical-slice customization budget

- One body rig.
- Three hairstyles.
- Three faces.
- Two outfits.
- Three palette choices.
- One alternate skin for each of the six slice weapons.
- One victory pose.

This is enough to validate compositing, sockets, persistence shape, and match readability without building a store.

## 6. User experience

### Match HUD

The match view must expose:

- Active player and team.
- Turn timer.
- Current and upcoming turn order.
- Wind direction and magnitude.
- Angle and power.
- Selected slot, weapon name, ammunition, and concise special rule.
- Movement allowance.
- Health, shields, statuses, and predicted Backlash cost.
- Camera reset and active-character focus.
- Connection and reconnection state.

### Desktop controls

| Action | Keyboard | Pointer |
|---|---|---|
| Move | A/D or left/right | Optional held HUD buttons |
| Aim | W/S or up/down | Drag aim handle |
| Charge power | Hold/release Space | Hold/release primary button |
| Fine aim | Shift modifier | Mouse wheel over focused game canvas |
| Change slot/weapon | 1/2/3 | Loadout bar |
| Camera pan | Arrow modifier or edge pan | Middle drag |
| Reset camera | R or Home | HUD button |
| Cancel uncommitted choice | Escape | Secondary button |

Browser scrolling, selection, and context-menu suppression apply only while the player has deliberately focused the game canvas.

### Accessibility baseline

- Critical state is not conveyed by color alone.
- Wind, angle, power, ammunition, and health have text or numeric forms.
- Color palettes pass contrast checks in the web shell and critical HUD.
- Reduced motion removes camera shake and limits flashes without changing timing.
- Effects audio has captions or visual equivalents for important cues.
- Controls are remappable through an action map, even if the first UI exposes only presets.
- UI supports keyboard focus and future controller focus without hover-only actions.

## 7. Authority, fairness, and networking contract

### Server-owned state

The authoritative match host owns:

- Turn phase, timer, and active player.
- Match seed and random draws.
- Legal loadouts and definition versions.
- Ammunition and Backlash.
- Projectile paths and collision results.
- Terrain occupancy and terrain operations.
- Damage, knockback, statuses, and elimination.
- Match result and progression rewards.

The client owns only presentation, local settings, uncommitted aim controls, camera position, and cosmetic animation.

### Command model

A client submits intent with an idempotent command ID, expected state version, expected turn ID, selected equipped weapon, and quantized inputs. The server validates the command and returns an authoritative timeline of projectile samples, impacts, terrain operations, state changes, eliminations, and a final state hash.

Late, duplicated, reordered, malformed, out-of-turn, or impossible commands cannot mutate match state.

### Offline training

Training uses the same `MatchHost` interface with an in-process authoritative host. It must not fork gameplay rules into a separate tutorial implementation. Bot difficulty changes candidate search and aim error; it does not ignore wind, collision, ammunition, or hazards.

## 8. Progression and monetization boundaries

### MVP progression

- Guest play with optional account conversion.
- Account XP and cosmetic unlocks.
- Weapon proficiency challenges that grant cosmetics or profile badges only.
- No character-level stat growth in competitive modes.
- No paid competitive weapon access.

### Acceptable future purchases

- Character appearance parts.
- Weapon skins.
- Victory poses, emotes, banners, and impact-effect variants.
- A transparent seasonal cosmetic track.

Random paid rewards, trading, user-generated markets, and player-to-player resale are outside the MVP and require separate legal, ratings, economy, and abuse review.

## 9. Delivery scope

### Milestone 0: firing-loop spike

Deliver:

- One map and one fixed character.
- Standard ballistic shot, wind, damage, crater, knockback, and hazard elimination.
- Fixed-step repeatability test.

Exit gate: internal testers voluntarily replay complete local duels, and identical seed/command inputs produce identical final state hashes.

### Milestone 1: vertical slice

Deliver:

- Training bot and private 1v1.
- Six slice weapons across all three slots.
- Layered customization budget defined above.
- One polished map, complete match HUD, rematch, and event replay.
- Guest identity only.

Exit gate: a first-time player can customize, equip, join, complete, understand, and rematch without developer explanation.

### Milestone 2: web MVP

Deliver:

- Two-to-four-player rooms.
- Training, free-for-all, and 2v2.
- Three maps.
- Four Main weapons, all five planned Secondaries, and three Melee/Tool weapons defined in `ARSENAL.md`.
- Reconnect, mute, report, match summaries, optional accounts, XP, and cosmetics.
- Installable desktop PWA.

Exit gate: remote matches survive duplicate/late commands and disconnect/reconnect tests; supported desktop browsers complete the same match successfully.

### Milestone 3: retention and distribution

Only after retention and completion data exist:

- More challenges and cosmetics.
- Public matchmaking.
- Balance seasons.
- Chrome companion evaluation.
- Steam wrapper evaluation.

### Explicitly deferred

- Ranked ladder.
- Battle royale.
- Multi-character squads.
- Crafting and vehicles.
- Cross-platform commerce.
- Console client.
- Large live-service store.

## 10. Success measures

Vertical-slice measures:

- Tutorial or first-match completion.
- Time to first valid shot.
- Match completion and rematch rate.
- Average planning and resolution duration.
- Weapon selection and successful-use distribution.
- Frequency of terrain-value plays versus direct-damage plays.
- Disconnect and invalid-command rates.
- Frame-time and load-time percentiles.

Do not use win rate alone to judge a weapon. Compare expected value, pick rate, accuracy, damage, displacement, terrain change, self-cost, and performance by player skill band.

## 11. Acceptance requirements

### Gameplay

- A match always reaches victory or sudden-death completion.
- Longsword use never decrements ammunition and always consumes the attack action.
- Longsword reach is 2.5 body widths, exactly twice the 1.25-body-width standard Melee/Tool reach.
- Digging affects only permitted materials; only the Breach Pick can remove reinforced stone.
- Backlash is previewed, resolved once, logged separately, and cannot be hidden by a skin.
- Boomerang outbound/return hit caps are deterministic.

### Determinism and authority

- Thousands of seeded command sequences reproduce matching final hashes.
- Duplicate commands cannot consume ammunition or apply damage twice.
- Terrain snapshots plus later operations reconstruct the same occupancy mask.
- A client cannot equip unavailable definitions, alter balance data, extend a timer, or report its own hit.

### Customization

- Every valid appearance combination can play all required animation tags.
- Missing pivots, sockets, frames, or skin compatibility fail asset validation.
- Cosmetics do not modify authoritative collision, reach, or projectile values.
- Left/right facing does not swap unintended asymmetrical details.

### Performance

- Target 60 fps on the main desktop profile and a stable 30 fps fallback.
- Terrain updates remain under one 16.7 ms frame in normal cases.
- Initial playable download target is under 15 MB compressed and must remain under 25 MB.
- Match-critical assets load before optional cosmetic atlases.

### Product integrity

- No public-facing copy claims affiliation with MapleStory, Dungeon Fighter Online, Worms, Gunbound, FN Herstal, or another reference property.
- All shipped character, weapon, interface, audio, and environment art is original or properly licensed.
- The Dungeon Barrage title receives formal trademark clearance before substantial marketing spend.
- Rating and storefront disclosures are updated when violence, online interaction, purchases, random items, or shipped generative-AI content changes.

## 12. Reference and art-direction guardrails

The supplied artillery screenshot is a reference for a readable side-view battlefield, elevated terrain, player labels, visible health, colorful projectile trails, and dramatic depth. It is not a compositional template.

Use:

- Original dungeon silhouettes, materials, hazards, proportions, palettes, and UI.
- Clear foreground terrain against softer background depth.
- Large readable player labels and status values.
- Distinct trail colors that also differ by shape or pattern.
- Original humanoid sprite proportions and animation timing.

Do not copy:

- Terrain outlines, exact camera composition, fonts, HUD placement, character silhouettes, projectile effects, or color relationships from the screenshot.
- MapleStory or Dungeon Fighter Online hairstyles, garments, faces, poses, icons, slot layouts, store presentation, animation frames, or promotional language.
- Real firearm logos, exact trade dress, or model geometry.

The safe public description is: **“an original layered paper-doll avatar system with independently skinnable weapons.”**
