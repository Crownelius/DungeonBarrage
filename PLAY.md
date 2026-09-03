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
3. **Loadout:** four pages, eight tiles each: ranged, then melee, then a one-shot
   secondary, then a crown or anklet. Click a tile to equip it — the gold tile with the
   **EQUIPPED** badge is the current pick, and the strip at the top shows all four slots.
   Click **NEXT SLOT** (or press Enter) to continue; on the last page click **START DUEL**.
   Esc goes back a page. Defaults are Ramshot Cannon / Trench Spade / Ramshot Shell /
   Ember Crown. The bot always fields that default, so changing your pick changes only
   your own side.
4. **Move, then shoot.** `A`/`D` walk. `Space` (or `W`) hops; gravity lands the crow
   on the same column (they cannot hang in the air to shoot). Arrow keys pan the
   camera (`F`/`Home` reset). Each cell you walk or hop spends walk allowance and
   **cuts this turn's maximum shot power** (a full-move turn still has a 10% floor).
   Then **aim:** left-click anywhere and **drag away from the other crow** (left crow
   pulls left, right crow pulls right; pull down as well for a high lob). The rubber
   band is your pull. A **gold line from your crow marks the first impact**. The
   **dotted arc** is the full flight, including bounces. Red rubber band = wrong way
   or too short. A 20-cell pull is 100% of this turn's max. Release to fire.
   Right-click cancels. Every shot plays its full flight, then a HIT flash; the
   Returning Boomerang also flies back to the thrower after impact. You cannot
   act again until that playback finishes. Returning Boomerang bounces twice
   and craters on landing.
   Keys `1`–`4` select weapons. `P` passes. Crowns fill after two damaging hits.
5. Destroying a supporting block makes the stacked blocks **fall in sim state**; Godot
   redraws the snapshot. The match ends with a visible victory, draw, or lose on the
   results screen. Enter rematches.

## Maps

| Map id | Layout (Melee-style) |
|---|---|
| `crow-perch` | Battlefield: stone main stage, two 3-high perches, high center plat. |
| `broken-battlements` | Fountain/Dream Land: wide stage, stacked side ledges, two-column keep. |
| `twin-spires` | Yoshi's Story: smaller stage with three stacked columns (2 / 3 / 2). |

`horizontal-test-array` remains the C2 FFI duel fixture (one unstacked row of blocks). It
is not one of the three playable stacked maps.

## Envelope

`SIMULATION_VERSION` is 9 and `CONTENT_VERSION` is 6. Version 9 corrects character
collision: the displayed crow now uses the exact center and radius published by Rust, so a
character hit cannot land on the old invisible outer circle. Create/command JSON has no
`characterId` and no kits. Each player sends
`loadout: { main, secondary, meleeTool, trinket }`. Ability slots are
`main | secondary | meleeTool | trinket`. Combat items spend ammo; secondaries start
with one round. Crowns and anklets are unlimited and spend a full trinket charge
instead. Melee items share one strike and differ only visually.

Content version 6 gives every playable map a reinforced main stage (Melee-style) and
raises crow health to 280; craters are 2–3 cells so a boomerang cannot delete a perch
and the void in one throw. Version 4 cut Ramshot knockback to two cells; version 3
scoped it to the crater. If one shot still wins outright, you are on an older content
table — check `db_sim_content_version()` before reading anything into the result.
