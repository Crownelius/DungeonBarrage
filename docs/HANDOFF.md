# Dungeon Barrage operational handoff

**Checkpoint date:** 2026-09-05

**Audience:** the next implementation agent, including an Opus agent resuming this branch

**Branch:** public `main`; this continuation starts at user-designated merge `4811fca`

## Current product truth

Dungeon Barrage has returned to fixed character kits. The 32-item ammunition wizard is retired.
The launch roster is Leslie, Crow, Erus, and Kreena. A player selects one character on one screen
and always retains two unlimited normal actions plus a charge-gated SS. A normal action ends the
turn; SS is a free action usable before or after the normal action.

The two tactical routes must remain available throughout a match:

- damage through direct and area attacks;
- ring-outs through displacement, terrain destruction, and positioning.

The client presents one dotted authoritative aim preview. Gold means the preview hits a character;
red means it does not. Do not restore the old solid rubber-band plus separate impact line/arc stack.
The Rust-published body center/radius is the collision and drawing contract, so a visible hit and
an authoritative hit refer to the same body.

The active product priority is a start-to-finish retro-arcade UI overhaul. U1-U3 now replace the
title, arena setup, character select, combat HUD/action rail, results, and settings/accessibility
shell. `docs/UI_OVERHAUL_PLAN.md` is the governing visual plan and
`docs/design/retro-arcade-menu-concept-v2-leslie-frog.png` is the current concept board. The
ArtStation Gunbound Season 4 presentation is hierarchy inspiration only; do not copy its art.

**Franchise invariant:** Leslie is a frog. The v2 concept and current procedural runtime art make
the amphibian silhouette explicit. Never label a Crow/bird sheet as Leslie again.

## Repository and ownership

The canonical repository is `C:\Users\rsfit\DungeonBarrage`. The similarly named OneDrive
workspace is not the implementation repository.

Start every resumed session with:

```powershell
Set-Location -LiteralPath 'C:\Users\rsfit\DungeonBarrage'
git status --short --branch
git rev-parse HEAD
```

Do not reset, clean, or blanket-stage a shared worktree. The untracked
`character assets-unused/` directory predates this slice and is not owned by it. Do not stage it.
Do not touch `.github-token` or paste credentials into commands, logs, commits, or documentation.

## Architecture decision

Keep the existing language boundary:

- Rust `db-sim-core` is the only authority for roster, legal actions, action economy, collision,
  ballistics, terrain, damage, displacement, gauge, elimination, hashes, and replay.
- `db-sim-ffi` exposes a coarse versioned C ABI for the local client.
- Godot 4.7.1 .NET/C# owns input, animation, effects, camera, audio, and UI.
- A future server links the Rust core directly.

Moving Godot presentation to Rust would discard the established UI/editor integration without
improving authority. Moving simulation rules into C# would duplicate them. New mechanics belong in
Rust first and reach C# only through versioned DTOs, snapshots, previews, and transitions.

## Version boundary

| Boundary | Current value |
|---|---:|
| Native ABI | 4 |
| Client contract | 2 |
| Simulation | 10 |
| Content | 7 |

ABI remains 4 because the exported function set/signatures did not change. Schema 2 replaces
client-authored loadouts with `characterId`, publishes `characterId` in snapshots, and exposes the
four fixed kits through `db_sim_roster`.

`PlayerState.loadout` and ammo counters still exist internally as a deliberate replay-migration
bridge. Rust derives them from the selected character, all current counters are unlimited, and new
match creation cannot select legacy item IDs. `character.rs` retains the old catalog only for old
replay/low-level resolver coverage. Do not expose it in the UI.

## Implemented in the current working slice

- Four authoritative character profiles with fixed HP, movement, and three actions.
- Schema-2 `characterId` match creation and snapshot identity.
- One four-card character selection screen.
- Three-slot action bar and keyboard shortcuts `1`, `2`, `3`.
- Unlimited normal actions; gauge-gated free-action SS.
- Exactly one dotted preview chosen from the Rust preview result, gold for character hit and red
  otherwise.
- Visible-body hitbox alignment using Rust-published collision geometry.
- Presentation registry/animation coverage for all four character IDs.
- Updated shared C-ABI fixtures and Rust golden vectors.
- Frozen fixture hashes: initial `5e95a1dd6ba37637`, move `d3681302b21ba8ef`, ability
  `06fa4183bbd03425`.
- Retro-arcade U1-U3: branded opening flow, persistent two-tactic combat readout, unlimited-action
  labels, gold-hit/red-miss smoke captures, results routes, and a persisted settings shell.
- Leslie renders as an original procedural frog in title, picker, and combat until the production
  animation sheet replaces that honest stand-in.

The detailed governing plan is `docs/CHARACTER_SYSTEM_IMPLEMENTATION_PLAN.md`. `PLAY.md` is the
current build and control guide. `docs/CLIENT_SPEC.md` is the client contract.

## Honest gaps

The repository still lacks identity-specific production animation sheets. Leslie no longer uses the
wrong species—an original procedural frog now appears in title, picker, and combat—but that is a
temporary honest stand-in. Crow, Erus, and Kreena still reuse closely related Crow equipment sheets.
Original Leslie, Crow, Erus, and Kreena portrait/idle/action sheets are the largest franchise-art
gap and govern U4 acceptance.

The settings shell persists the existing model, and Reduce Motion is honored by current animation
systems. Actual audio playback plus global high-contrast and text-scale application are not yet
integrated. Do not infer feature completion from a saved toggle alone.

Phase 1 is accepted. A real Windows Desktop export rendered the character screen, three-action
bar, a single dotted gold hit guide terminating on the visible body, and terminal results at
1280x720. C6 completed all three maps, exercised human and bot turns, dropped stacked blocks, and
created/disposed a rematch. C6-timeout and C7 also passed. The reports and screenshots are in
`C:\tmp\DungeonBarrage-character-smoke-20260904` on the verification machine; the exact durable
results are recorded below and in `docs/BUILD_LOG.md`.

The Phase 1 abilities use the existing closed resolver vocabulary. These intended mechanics remain
approximations and must not be reported as finished:

| Character | Intended mechanic | Current implementation |
|---|---|---|
| Leslie | Ant Glob rolls along ground before cluster detonation | cluster projectile on impact |
| Leslie | persistent Corrosive Vomit Ooze hazard | target-bound Embers status |
| Crow | flight/aerial positioning identity | fast ground movement |
| Erus | Celestial Staff attacks all enemies and has seeded 5% self-hit | single turret spawn |
| Kreena | Global Magic Arrow reaches any valid enemy | ordinary long-range projectile |

Each authoritative mechanic needs scenario tests and playback evidence. Any state or RNG change
requires canonical encoding review, a simulation-version bump, fixture regeneration, and golden
vector regeneration.

## Next task

Implement U4 production art and release QA from `docs/UI_OVERHAUL_PLAN.md`: create original,
identity-specific character animation sheets beginning with Leslie's frog sheet; then apply global
high-contrast/text-scale behavior and wire audio settings to real audio buses. Preserve the U2/U3
screen hierarchy and rerun visible C5/C6/C7/timeout evidence. Leslie's Ant Glob ground-roll mechanic
remains the first deferred Phase 2 gameplay task; do not mix authority changes into art/UI work.

## Verified checkpoint

- Rust workspace: 514 passed, 0 failed, 1 explicitly ignored fixture writer.
- Release FFI: 23 passed, 0 failed, 1 explicitly ignored fixture writer.
- .NET: 12 contract + 152 interop = 164 passed, 0 failed.
- Exact CI restore `dotnet restore client/DungeonBarrage.sln --locked-mode`: passed after
  regenerating RID-neutral production lock files and restoring `GodotSharpEditor` to the app lock.
- The interop test project conditionally copies `db_sim_ffi.dll`, `libdb_sim_ffi.so`, or
  `libdb_sim_ffi.dylib` from `target/release` beside its test assembly. Keep this cross-platform;
  Linux CI must never depend on an ambient native-library search path.
- Strict Clippy, rustfmt check, `cargo deny`, .NET format, release export, and diff checks passed.
- C5: Crow selected through Character Select; one-cell move and direct Precision .57 hit; exactly
  one dotted guide; both gold-hit and red-miss captures; 34 real damage; fire/hit/impact cues;
  input lock/unlock; turn handoff.
- C6: roster 4; Kreena vs Erus; human/bot turns; all three maps; stacked blocks fell; terminal at
  turn 16; state hash `6b28cd9b5e7c4f3c`; rematch created and disposed cleanly.
- C7: title plus settings screen captured at 1280x720; all six existing settings/platform checks
  true.
- Current evidence root: `C:\tmp\DungeonBarrage-retro-ui-20260905\evidence`.
- A restricted-runner-only launch once failed to open `user://logs` and produced a Windows access
  violation dialog. Two normal-permission exports with explicit `--log-file` then completed C5 and
  C6 with clean logs and no lingering process. Keep explicit local log paths in automated runs.
- C6-timeout: visible countdown and automatic authority timeout passed.
- C7: settings recovery, audio clamping, UI scaling, localization, performance-tier switching,
  and multi-platform export presets passed.

## Required gates

Run from the canonical repository:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo test --release -p db-sim-ffi --locked
cargo build --release -p db-sim-ffi --locked
cargo deny check
Copy-Item -Force .\target\release\db_sim_ffi.dll .\client\native\win-x64\db_sim_ffi.dll
dotnet restore .\client\DungeonBarrage.sln --locked-mode
dotnet format .\client\DungeonBarrage.sln --verify-no-changes --no-restore
dotnet test .\client\DungeonBarrage.sln -c Release --no-restore
git diff --check
```

For renderer evidence, use the pinned Godot path verified by `scripts/verify-toolchain.ps1`, export
the Windows Desktop build outside the source tree, and run the C5/C6/timeout/C7 smoke entry points.
Inspect the reports and non-zero screenshots. Specifically confirm terminal bot play, results,
rematch, controller flow, clean handle disposal, all playable maps, falling stacked blocks, and the
new character/aim presentation. Do not accept `success: true` without checking those fields and
images.

## Commit discipline

Before committing, inspect `git diff --check`, `git diff --stat`, and `git status --short`. Stage
only owned files. Keep regenerated `crates/db-sim-core/tests/golden_vectors.rs` in a separate test
commit from the feature migration. Push the current branch only after every automated gate is green.
