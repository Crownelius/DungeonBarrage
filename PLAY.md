# How to play Dungeon Barrage (playable cut)

Every fighter is the **crow**. Equipped items are ammunition. There is no character-kit
picker. Rust is the only authority for damage, collision, ammo, terrain, collapse, and
victory. Godot presents snapshots.

## Build

From `C:\Users\rsfit\DungeonBarrage`:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --release -p db-sim-ffi --locked
cargo build --release -p db-sim-ffi --locked
cargo deny check

Copy-Item -Force .\target\release\db_sim_ffi.dll .\client\native\win-x64\db_sim_ffi.dll

dotnet test .\client\DungeonBarrage.sln -c Release
```

Open the Godot 4.7.1 .NET editor (pinned by `scripts/verify-toolchain.ps1`) on
`client/src/DungeonBarrage.Client/project.godot`.

## How to play

1. Click or press Enter on the menu.
2. **Local setup:** left/right chooses one of the three stacked maps. Enter continues.
3. **Loadout picker:** tiles are items. Click a tile (or use arrows) to **equip it into its
   slot** — a main-slot click replaces only main, and so on. The default triangle is
   Ramshot Cannon / Recurve Bow / Trench Spade. Enter starts a local duel versus the bot
   with the equipped loadout.
4. **Aim:** drag from your crow to set angle and power, then release to fire the selected
   slot (`1` main, `2` secondary, `3` melee/tool). Left/right walk. `P` passes.
5. Destroying a supporting block makes the stacked blocks **fall in sim state**; Godot
   redraws the snapshot. The match ends with a visible victory, draw, or lose on the
   results screen. Enter rematches.

## Maps

| Map id | Layout |
|---|---|
| `crow-perch` | Two facing towers, three destructible blocks high, spawns on the crowns. |
| `broken-battlements` | Stacked spawn ledges plus a three-layer keep in the middle. |
| `twin-spires` | Outer 2-high columns and a 3-high centre stack. |

`horizontal-test-array` remains the C2 FFI duel fixture (one unstacked row of blocks). It
is not one of the three playable stacked maps.

## Envelope

`SIMULATION_VERSION` is 7. Create/command JSON has no `characterId` and no kits. Each
player sends `loadout: { main, secondary, meleeTool }`. Finite items spend ammo; the
Longsword is the only unlimited item.
