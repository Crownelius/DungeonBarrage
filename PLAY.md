# How to play Dungeon Barrage

Dungeon Barrage is a character tactics game. Choose one hero with a fixed kit, then decide
turn by turn whether to deal direct damage, reshape terrain, or force a ring-out. Rust is the
only gameplay authority; Godot/C# presents its roster, snapshots, previews, and transitions.

## Build

Use the canonical repository at `C:\Users\rsfit\DungeonBarrage`:

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
```

Open `client/src/DungeonBarrage.Client/project.godot` with the Godot 4.7.1 .NET editor.
`scripts/verify-toolchain.ps1` verifies the pinned Godot, .NET, Rust, and export-template setup.

## Start a match

1. Start from the main menu.
2. Choose one of the three maps in Local Setup and continue.
3. Choose Leslie, Crow, Erus, or Kreena on the single Character Select screen. Each card is
   a complete fixed kit; there is no equipment or ammunition screen.
4. Start the duel. The bot chooses a different launch character when possible.

## Turn controls

| Input | Action |
|---|---|
| `A` / `D` | Walk before committing the normal action |
| `Space` or `W` | Hop |
| `1` | Select Shot 1 |
| `2` | Select Shot 2 or melee |
| `3` | Select SS when its gauge is full |
| Left-drag and release | Aim and fire the selected action |
| Right-click | Cancel uncommitted aim |
| `P` | Pass |
| Arrow keys | Pan camera |
| `F` or `Home` | Focus the active character |

The two normal actions are always available and have infinite uses. Either normal action ends
the turn. A charged SS spends its gauge but is a free action: it may be used before or after the
one normal action. It cannot grant a second normal action.

Aim by pulling away from the intended direction and release to fire. The client shows exactly
one dotted guide derived from Rust's read-only preview. It is gold when the authoritative preview
hits a character and red when it does not. This guide is information only; Rust resolves the shot.

Walking reduces that turn's maximum shot power, so repositioning has a real cost. Direct damage
is never the only route: character actions retain terrain and displacement tools so a player can
switch to a ring-out plan whenever the arena state makes it better.

## Launch characters

| Character | HP | Movement | Fixed kit |
|---|---:|---|---|
| Leslie | 340 | Slow | Ant Glob, Tongue Whip, Corrosive Vomit Ooze |
| Crow | 250 | Fast | Precision .57, Heavy Revolver, Aerial Barrage |
| Erus | 270 | Normal | Curved Fireball, Staff Thrust, Celestial Staff |
| Kreena | 260 | Fast | Recurve Bow, Hunting Dagger, Global Magic Arrow |

The roster and availability rules come from Rust. Several unique SS mechanics are still on the
documented Phase 2 roadmap; see `docs/CHARACTER_SYSTEM_IMPLEMENTATION_PLAN.md` for the exact gap
between current resolver-backed approximations and their intended final behavior.

## Maps

| Map id | Layout |
|---|---|
| `crow-perch` | Wide main stage, two perches, and a high center platform |
| `broken-battlements` | Broad stage, stacked side ledges, and a two-column keep |
| `twin-spires` | Compact stage with three stacked columns |

Destroying support changes authoritative terrain and can make stacked blocks fall. A match ends
on the results screen with victory, defeat, or draw; Enter starts a fresh rematch.

## Contract versions and fixture

The current native boundary is ABI 4, client schema 2, `SIMULATION_VERSION` 10, and
`CONTENT_VERSION` 7. Schema-2 match creation sends `characterId`; it does not accept a player-made
loadout. Internal loadout/ammo fields are a temporary replay-migration bridge and are not UI.

The shared fixture is authoritative; read hashes from
`tests/fixtures/matches/horizontal-test-duel-v1/fixture.json`. Its current checkpoints are:

| Checkpoint | Hash |
|---|---|
| Initial | `5e95a1dd6ba37637` |
| After move | `d3681302b21ba8ef` |
| After ability | `06fa4183bbd03425` |

Do not update response JSON by hand. Regenerate it only through the ignored production-ABI writer:

```powershell
cargo test -p db-sim-ffi --lib -- --ignored regenerate_shared_response_fixtures_from_production_abi
```
