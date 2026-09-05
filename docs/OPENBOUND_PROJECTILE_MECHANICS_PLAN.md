# OpenBound projectile-mechanics clean-room plan

Status: **reference audit complete; dormant Rust kernel implemented and tested**

Reference: [`WickedPeanuts/OpenBound`](https://github.com/WickedPeanuts/OpenBound) at default-branch
commit `d11e8127d4634e51e8c0519c1349ac9f512bb357` (`dev-2`). The scoped projectile, terrain,
weather, mine, and physics files are identical on `master`; `dev-3` changes only an unrelated font
enum inside the inspected `Parameter.cs` range.

## Licensing and implementation boundary

OpenBound is GPL-3.0-or-later. Dungeon Barrage is currently `UNLICENSED`. No OpenBound source,
assets, constants, class structure, or audiovisual expression may be copied into this repository
unless the project owner deliberately relicenses Dungeon Barrage compatibly.

This plan therefore treats OpenBound as behavioral research. Dungeon Barrage receives an original,
integer-only Rust implementation of the reusable mechanics, expressed through its own fixed-point
units, terrain materials, event vocabulary, safety limits, and tests. Source class names appear
below only to make audit coverage reviewable.

The new mechanics stay outside `character_roster`, command resolution, canonical encoding, FFI,
and client contracts. They compile and run under tests, but no current character can select them.
Activating one later requires a character design, authority integration, simulation-version review,
new golden vectors, client playback, bot behavior, and renderer evidence.

## Reference coverage matrix

| Reference surface | Behavior observed | Dungeon Barrage primitive |
|---|---|---|
| `Projectile` | fixed-step gravity/wind flight; sub-step terrain/body collision; bounds termination | existing `ballistics`, plus contact policy |
| `Projectile.Explode` / `Topography.CreateErosion` | radial damage and circular terrain removal | existing damage resolver plus dormant crater builder |
| `ArmorProjectile2` | primary impact followed by a weaker second blast | staged multi-blast |
| `ArmorProjectile3` | pauses and arms after flight time, then resumes with stronger payload | timed flight transform |
| `BigfootProjectileEmitter` | angle-spread, power-spread, and staggered salvos | deterministic volley scheduler |
| `DragonProjectileEmitter` | mixed power/angle staggered salvo | deterministic volley scheduler |
| `DragonProjectile3` | marker impact spawns delayed projectiles converging from a fan | impact convergence fan |
| `KnightProjectile1/2` | marker impact calls three/five straight satellite strikes from the owner side | satellite convergence fan |
| `KnightProjectile3` | seven delayed strikes from distributed satellite origins | delayed convergence fan |
| `LightningProjectile1` | impact adds one directional terrain-scanning discharge | last-surface beam |
| `LightningProjectile2` | impact adds four diagonal discharges | radial beam cascade |
| `LightningProjectile3` | impact adds a discharge at each nearby target | target-proximity beam cascade |
| `BeamDummyProjectile` | ray marches through terrain/bodies and detonates at the last contact | last-surface beam scanner |
| `ThorProjectile` / `ThorSatellite` | every impact can call a level-scaled satellite beam | global impact follow-up |
| `MageProjectile2` | two orbiting collision payloads converge toward the carrier path | converging helix |
| `MageProjectile3` | adds target shield to damage, then drains shield in an area | shield-purge impact |
| `IceProjectile2` | repeated hits reduce defense down to a floor | stacking armor shred |
| `RaonBaseProjectile1` | four independently colliding payloads orbit one ballistic carrier | orbiting payload carrier |
| `RaonProjectile2` | two terrain-only shots deploy persistent proximity mines | terrain-only deployment plus proximity mine |
| `RaonLauncherMineS2` | selects nearest target, wakes in range, walks on its own turn, contact-detonates | target-seeking walking mine |
| `RaonProjectile3` | terrain-only shot deploys a long-running mine that flips at obstacles | roaming fuse mine |
| `TricoBaseProjectile2` | three payloads rotate around a ballistic carrier | orbiting payload carrier |
| `TricoProjectile3` | first impact anchors eight timed blasts around a ring | sequenced impact ring |
| `TurtleProjectile2` | paired helical payloads collapse toward the carrier after a delay | converging helix |
| `TurtleProjectile3` | timed carrier splits into six weaker projectiles with angle spread | timed mid-flight split |
| `TeleportationBeacon` | ignores characters, lands on terrain, moves owner to last free point | terrain-only teleport landing |
| `Force` | one-time multiplicative plus flat damage increase | bounded damage amplifier |
| `Weakness` | one-time multiplicative plus flat damage reduction | bounded damage dampener |
| `Mirror` | horizontal reflection plus damage increase | reflection transform |
| `Tornado` | weather temporarily owns motion through a three-leg redirected path | external-path transform |
| `Electricity` | impacted projectile gains a discharge follow-up | impact beam modifier |
| `Random` | first eligible projectile activates a linked weather effect | one-shot environment trigger |
| `Thor` | globally attaches a satellite follow-up to every eligible projectile | global impact modifier |
| `WeatherHandler` | same-type zones merge; each type affects a projectile once | idempotent/mergeable modifier policy |
| terrain-only mine and beacon colliders | characters do not intercept placement shots | terrain-only contact policy |
| all multi-projectile classes | turn finalizes only after every dependent payload resolves | dependency-group completion |

Visual-only flipbooks, particles, camera tracking, sounds, and exact OpenBound balance numbers are
not mechanics and are intentionally excluded. Apparent reference defects (including unsafe array
indexing, floating-point divergence, unbounded ray loops, and suspect defense-floor logic) are not
reproduced.

## Dormant Rust kernel

The kernel supplies reusable, bounded pure functions for:

1. Contact policies and dependency-group completion.
2. Centered angle/power volleys with deterministic stagger timing.
3. Orbiting, converging, splitting, fan, ring, and staged payload plans.
4. Timed arming/upgrades and secondary blast phases.
5. Terrain-only landing, circular terrain-operation construction, and bounded last-surface beams.
6. Walking-mine target selection/steps and roaming-mine obstacle reflection.
7. Teleport landing at the last valid free sample.
8. Saturating damage amplification/dampening, armor shred, and shield purge.
9. Mirror, tornado, electricity, random-zone, and satellite follow-up descriptors.

All APIs use fixed-point integers and explicit hard caps. They do not mutate `SimulationState` and
cannot enter normal command resolution. Unit tests exercise every primitive and assert that the
mechanic catalog is exhaustive and duplicate-free.

Implemented at `crates/db-sim-core/src/projectile_mechanics.rs`: 39 named behavior families and
25 focused tests. The kernel reuses Dungeon Barrage terrain materials and operations, caps child
payloads at 32 and beam traversal at 2,048 samples, uses stable IDs for target ordering, and applies
saturating/checked arithmetic throughout. `SIMULATION_VERSION`, `CONTENT_VERSION`, canonical
state, frozen vectors, and current character kits are unchanged because the module is unreachable
from match commands.

## Activation checklist for a future character

- Select only the primitives required by the approved character kit.
- Integrate them in Rust command/ballistic resolution; do not implement gameplay in C#.
- Bound child projectile count, beam steps, persistent-object count, and total event count.
- Decide whether canonical state changes; bump `SIMULATION_VERSION` when replay outcomes can change.
- Regenerate and review golden vectors rather than accepting them mechanically.
- Publish explicit playback events and aim-preview traces through the versioned client contract.
- Teach the bot the new legal action and expected damage/terrain/displacement value.
- Add scenario tests for direct hit, terrain hit, miss/out-of-bounds, self-hit, stacked terrain,
  elimination, and every modifier interaction.
- Export the client and inspect the visible body hit, terrain mutation, full playback, and turn
  handoff before assigning the mechanic to a roster entry.
