# OpenBound clean-room reuse audit

**Audit date:** 2026-09-03  
**Reviewed source:** `rodrigobmg/OpenBound` at commit
`cb2fc197dd6b0498390219750ca6251bf0582d63`  
**DungeonBarrage position:** Rust remains the sole simulation authority; Godot/C# is a
consumer of versioned snapshots and presentation events.

This is a design/reference audit, not a code-import approval. It records what an implementation
agent may learn from a mature artillery-game client while protecting DungeonBarrage's authority
boundary and avoiding incompatible source or asset reuse.

## Licensing and provenance boundary

OpenBound is GPL-3.0-or-later. DungeonBarrage is currently `UNLICENSED`. Do not copy OpenBound C#,
translate it line-for-line to Rust, import its assets, reuse its animation tables, or carry over
commercial-game-derived maps, names, balances, sprites, sounds, paths, or extracted data. Direct
reuse would require a separate legal decision, compatible licensing, and source-provenance review.

Allowed work is clean-room implementation from high-level behavioral requirements already expressed
by DungeonBarrage's own contracts and tests. A future agent should read this document and the local
specification before looking at any external reference source, write original code in the target
architecture, and test it against DungeonBarrage fixtures rather than OpenBound outputs.

## What was useful

OpenBound's useful contribution is a mature inventory of *presentation responsibilities*, not an
implementation to port:

| Priority | Clean-room target | Source inspiration category | DungeonBarrage constraint |
|---|---|---|---|
| 1 | Actor feedback states | fire, hit, defeat, facing, fallback clips | derive only from Rust transition events; preserve `CharacterBodyGeometry` exactly |
| 2 | Combat effects | impact, material burst, floating feedback, cleanup | one cue per authoritative event; no damage or collision inference |
| 3 | Camera presentation | manual pan, projectile framing, impact response | compose draw-only offset; never mutate match state or manual pan |
| 4 | HUD/navigation | active turn, aim, health, status, results, dialogs | controlled through local client flow and snapshot reconciliation |
| 5 | Asset metadata/tools | pivots, sockets, atlas validation, thumbnails | original assets and portable generated metadata only |
| 6 | New mechanics | staged shots, objects, weather, consumables | original Rust authority rewrite with integer/replay/FFI gates |

The first completed clean-room slice is deliberately small:

- `TransitionCueResolver` maps `projectileTrace`, decreasing `healthChanged`, `impact`, and
  `playerEliminated` events to transient C# presentation cues.
- `Main` draws those cues around the existing projected circle and uses the exact authoritative
  impact position for the burst. It composes a temporary impact impulse with manual pan only for
  the active draw frame.
- Reduced motion retains fire/hit/impact information but disables camera shake.
- `entityMoved` is intentionally deferred: the current contract provides only a net
  `AuthoritativeResolution` ground-pivot change, not a safe walk, jump, or fall path.

## Do not port

### Client-authoritative simulation

Do not use OpenBound terrain, collision, projectile, gravity, damage, or player-motion code. Its
client owns mutable projectile impact/damage/terrain behavior, which conflicts directly with
DungeonBarrage's Rust `db-sim-core`, deterministic trace contract, preview/apply parity, replay,
and FFI hash gates.

Do not turn `ClientEntityMovedEvent.Start` or `.End` directly into a body center. They are ground
pivots. DungeonBarrage's visible/authoritative body is the fixed-point `CollisionCenter` plus
`CollisionRadius`, projected through `CharacterBodyGeometry`. Any future motion renderer must
apply a documented pivot delta to that circle, never draw a replacement hit shape.

### Networking, server, auth, launcher, and assets

Do not reuse OpenBound TCP/object wrappers, server/game handlers, authentication/token code,
database models, launcher code, vendor-specific extraction utilities, bundled binaries, or content
assets. In addition to licensing/provenance issues, those subsystems depend on unversioned or
client-trusting state flows that are incompatible with a Rust authoritative service.

### Exact content expression

Do not reuse character/mobile names, sprites, maps, frame durations, balance values, weather
parameters, or material imagery. Mechanics can be independently designed as versioned
DungeonBarrage content only after a Rust contract exists.

## Sequenced follow-up work

1. **Actor/paper-doll renderer:** add original manifest IDs, validated pivots/sockets, and a
   missing-clip fallback. Keep the same `CharacterBodyGeometry` circle as the drawn collision body.
2. **Camera director:** use a separate presentation controller to frame trace bounds and restore
   manual focus without adding a `Camera2D` coordinate rewrite to the current hand-drawn scene.
3. **Effect system:** replace hand-drawn one-shot accents with tiered, disposal-safe effects driven
   by the same transition event list. Target markers must remain at low quality and reduced motion.
4. **Motion only after contract support:** extend Rust with path samples or explicit movement
   provenance, then add snapshot/transition fixtures before interpolating a movement visual.
5. **Mechanics only in Rust:** model new projectile families, persistent objects, weather, and
   consumables as composable integer-authoritative effects. Require preview/apply parity,
   ordered-trace, replay/checkpoint, FFI, and golden-vector tests before C# presentation work.

## Required verification for every follow-up

- Preserve `CharacterGeometryTests`: a reported character impact must remain inside the same
  circle displayed by the client.
- Keep C# presentation code Godot-free where practical so behavior runs in CI without the engine.
- Run the Rust workspace gates whenever an authority contract changes; run `dotnet test` and a
  Godot build for C# presentation changes.
- For a visual change, run the exported C6 smoke and inspect a non-zero renderer-backed screenshot.
- Record any new manifest/schema/content version decision in `docs/HANDOFF.md`; do not silently
  turn an optional presentation cue into simulation state.

## References reviewed

High-level reference locations included OpenBound's `GameComponents/Animation` state/flipbook
classes, special-effect and particle handlers, camera/HUD classes, metadata/pivot tools, projectile
families, persistent-object examples, and weather handlers. They inform the category list above
only. No source was copied into this repository.
