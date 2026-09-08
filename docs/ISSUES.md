# Known issues

Living defects. Do not delete an entry because a later patch looks related — close it
with the commit that actually removed the behavior.

## Projectiles can hit the collision circle before they overlap the printed sprite

**Status:** open. Presentation now draws the authoritative circle so the printed hitbox
matches the strike test; the circle is still wider than the illustrated fighter.

The authority collision body is a circle of radius `PLAYER_COLLISION_RADIUS`
(`BODY_WIDTH / 2` = two terrain cells) centered one radius above the standing pivot
(`fixed.rs`, SIMULATION_VERSION 9). The C# sprite is scaled to `radius * 2.3` tall and
anchored at the feet (`CharacterSpriteRegistry.TryDrawCharacter`). A side-view crow or
frog is much narrower than a four-cell-wide circle, so a shot that misses the pixels can
still report `ImpactCause::Character`.

This is not a second, invisible collider. Preview gold and applied hits both use the same
Rust-published center/radius. Shrinking the circle requires a simulation-version bump,
new golden vectors, and a visible-body re-check. Do not special-case C# hit tests.

## Launch-kit terrain destruction is concentrated in two abilities plus Cluster

**Status:** by design of the Phase 1 approximations; easy to misread.

Of the twelve launch actions, only Crow Heavy Revolver (`TerrainProfile::Dig`) and Erus
Curved Fireball (`TerrainProfile::Crater { 2 }`) declare a terrain profile. Leslie Ant
Glob still destroys terrain because `EffectKind::Cluster` spawns three radius-3 SOFT
craters around impact (`resolve/attack_mods.rs`). Everything else is `TerrainProfile::None`.
Ring-outs therefore come from Cluster, Dig, Crater, knockback/push/pull, and then
`movement::settle` after the footing is gone — not from every shot.

## Embers is a target-bound status, not a ground hazard

Leslie Corrosive Vomit Ooze and Erus Curved Fireball attach `EffectKind::Embers` to whoever
is in radius at impact. A later walk into that cell is not damaged. Persistent Ooze is a
Phase 2 authority task.

## Blocks shrink; they do not crater

ADR 0005: a SOFT crater overlapping a block deals proportional health damage and
re-derives the footprint from health (usually by deleting columns). Background empty
cells around the Melee-style stages are already `Empty`, so there is no landscape to
hole. Destroyed soil perches fall via `block_ops::settle_unsupported_blocks` onto the
ReinforcedStone main stage. A fighter with no cell underfoot is walked downward by
`movement::settle` until they stand or pass the bottom of the mask, at which point
health is set to 0 (unrecoverable fall). SOFT ammunition cannot delete the stage.

## Main.cs is still a 4k-line screen God-class

Title, arena setup, character select, match HUD, results, and settings all live in
`App/Main.cs`. Terrain occupancy painting is now `PixelTerrainAtlas` plus
`PixelTerrainSampler`. Further splits should keep gameplay decisions out of C#.

## Dormant projectile kernel is not in the match

`projectile_mechanics.rs` is tested and unreachable from the roster, commands, FFI, and
client. Do not wire a primitive to a character from C# or by editing kit data alone.

## Franchise art gap

Leslie uses a procedural frog stand-in. Crow, Erus, and Kreena still share Crow equipment
sheets. U4 in `docs/UI_OVERHAUL_PLAN.md` remains open.
