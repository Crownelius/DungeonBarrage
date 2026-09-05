# Character system implementation plan

**Decision date:** 2026-09-04

**Status:** Phase 1 implemented and verified; Phase 2 specialty mechanics are next

**Supersedes:** the 32-item loadout/ammunition playable cut and the earlier tactical-ammo plan

## Product outcome

Dungeon Barrage is a character tactics game again. A player chooses one hero on one clean
selection screen, then always has two normal character actions and one charge-gated special:

| Key | Action | Turn cost |
|---|---|---|
| `1` | Shot 1 | Ends the turn after resolution |
| `2` | Shot 2 or melee | Ends the turn after resolution |
| `3` | SS | Requires a full gauge; spends the gauge but preserves the normal attack |

There is no player-visible ammunition. Neither normal action can be exhausted. Damage,
terrain destruction, displacement, and ring-outs remain available through character kits,
so the player can change tactics before committing a normal action. An SS may be used before
or after that action when charged.

## Language decision

Keep the existing Rust/C# split.

- Rust `db-sim-core` owns roster definitions, legal actions, action economy, collision,
  ballistics, damage, terrain, displacement, gauge, elimination, hashes, and replay behavior.
- C# owns Godot input and presentation. It consumes the Rust roster, snapshots, transitions,
  and previews; it never recomputes a hit.
- Rewriting the Godot client in Rust would replace mature editor/UI integration without making
  gameplay more authoritative. Rewriting the simulation in C# would create two rule engines.
  Neither improves the current architecture.

## Phase 0: contract migration

- [x] Bump the strict client contract to schema 2.
- [x] Replace client-authored loadouts with `characterId` in match creation.
- [x] Publish `characterId` in player snapshots and four fixed kits from `db_sim_roster`.
- [x] Derive transitional loadout/ammo fields inside Rust for replay compatibility.
- [x] Set all transitional counters to unlimited; clients do not display or choose them.
- [x] Bump `SIMULATION_VERSION` to 10 and `CONTENT_VERSION` to 7.
- [x] Regenerate frozen responses through the production C ABI and regenerate golden vectors.

The legacy item catalog and snapshot loadout/ammo fields remain only as an explicit migration
bridge for old resolver/replay coverage. New match creation cannot select an item or enter that
fallback. Remove these fields in a later schema bump after replay migration is designed.

## Phase 1: roster, selection, action economy, low-clutter aim

- [x] Launch roster: Leslie, Crow, Erus, Kreena with fixed HP, movement, and three actions.
- [x] Replace the four-page 32-item wizard with one four-card character screen.
- [x] Use one three-slot action bar: Shot 1, Shot 2/Melee, SS.
- [x] Keep both normal actions unlimited and available throughout the match.
- [x] Make SS gauge-gated and a free action that does not consume movement or the normal attack.
- [x] Permit switching to SS after the normal action; block a second normal action.
- [x] Render exactly one dotted trajectory selected from the authoritative preview.
- [x] Use the authoritative terminal impact: gold for a character hit, red otherwise.
- [x] Keep hit detection aligned to the visible character via Rust-published collision geometry.
- [x] Remove duplicated muzzle/impact guides and reduce redundant floating HUD text.

Phase 1 passed the full automated gates and an exported Godot 4.7.1 renderer run on 2026-09-04.
The captured 1280x720 evidence confirms the four-character screen, three-action bar, one dotted
gold character-hit guide, visible-body hit, terminal full match, results, rematch, timeout path,
and release-quality checks. A headless import alone would not have been sufficient.

## Phase 2: exact specialty mechanics

The current Phase 1 abilities use the existing closed resolver vocabulary. The roster and action
economy are real, but the following mechanics are deliberate approximations and must not be
reported as complete:

| Character | Required mechanic | Current approximation | Completion evidence |
|---|---|---|---|
| Leslie | Ant Glob rolls on ground before cluster detonation | Cluster projectile on impact | deterministic roll/path/cluster tests plus playback |
| Leslie | Persistent Corrosive Vomit Ooze hazard | target-bound Embers status | authoritative ground object, duration/tick tests, visual lifetime |
| Crow | Flight/aerial positioning identity | Fast ground movement | authority-owned flight state, collision/landing tests, readable animation |
| Erus | Celestial Staff attacks all enemies with 5% self-hit chance | single turret spawn | ordered all-target strikes, seeded self-hit draw, replay tests |
| Kreena | Global Magic Arrow reaches any valid enemy | ordinary long-range projectile | global targeting contract, obstruction policy, deterministic tests |

Implement these one mechanic at a time. Any new state or RNG consumption requires a simulation
version bump, canonical encoding review, fixture regeneration, and a dedicated scenario test.

## Phase 3: original visuals and teaching

- Give each hero original idle, move, aim, fire, hit, defeat, and victory presentation.
- Add distinct projectile/effect presentation for all twelve actions without copying OpenBound or
  Gunbound code, assets, values, names, or effects.
- Teach the tactical choice in one persistent, non-modal sentence on turn one. Do not restore a
  wizard or interrupt the arena with popups.
- Add a non-color miss cue (`MISS`) so red is not the only signal.
- Validate 1280x720 and 1920x1080, default and large UI, normal and reduced motion.

## Acceptance gates

Run from `C:\Users\rsfit\DungeonBarrage`:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --release -p db-sim-ffi --locked
cargo build --release -p db-sim-ffi --locked
cargo deny check
Copy-Item -Force .\target\release\db_sim_ffi.dll .\client\native\win-x64\db_sim_ffi.dll
dotnet format .\client\DungeonBarrage.sln --verify-no-changes --no-restore
dotnet test .\client\DungeonBarrage.sln -c Release --no-restore
git diff --check
```

Then export and run the C5/C6/timeout/C7 smoke paths. Inspect non-zero renderer-backed evidence;
do not infer visual correctness from tests or an empty screenshot.

## Next task

Implement Leslie's Ant Glob ground roll as the first Phase 2 mechanic because it establishes the
reusable authoritative ground-travel path needed by later unusual projectiles. Define its Rust
state/trace contract and exact slope, step, stop, collision, cluster-detonation, terrain-mutation,
preview, and bot rules before adding presentation.
