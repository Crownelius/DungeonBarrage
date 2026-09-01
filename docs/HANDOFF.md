# Dungeon Barrage operational handoff

**Checkpoint date:** 2026-08-31

**Audience:** the next implementation agent

**State:** Playable cut on `SIMULATION_VERSION` 7. The match-create/command envelope no
longer carries `characterId` or character kits. Every fighter is the one crow. Equipped
items are ammunition. Leftover C1 timeout/preview/object-provenance kit tests are
`#[ignore]` until revisited; a real turn already runs on the existing duel + blocks.

Steam page work is **after C5 only** and is not part of this checkpoint.

This is the mutable resume document. `docs/CLIENT_SPEC.md` is the historical client
contract. Accepted ADRs retain architectural history (ADR 0002 kits are **not** restored
for this cut; do not rewrite ADR history). `PLAY.md` is how to build and play.

---

## 1. Start here

```powershell
Set-Location -LiteralPath 'C:\Users\rsfit\DungeonBarrage'
git status --short --branch
git rev-parse HEAD
```

Canonical repository: `C:\Users\rsfit\DungeonBarrage`.

Do not copy work into `C:\Users\rsfit\OneDrive\Documents\DungeonBarrage`.

Do not run `git reset --hard`, `git checkout --`, or `git clean`. Do not touch
`.github-token`.

---

## 2. Product truth for this cut

| Claim | Status |
|---|---|
| Envelope: no `characterId`/kits; one crow; items as ammo | **implemented** (`SIMULATION_VERSION` 7, `CONTENT_VERSION` 4). Ramshot knockback is two cells. A healthy match runs 3–4 turns and the winner finishes hurt. |
| FFI `create`/`apply`/`snapshot` hashes equal direct Rust | **implemented** (`ffi_create_apply_snapshot_matches_direct_rust_on_the_duel_blocks_path`) |
| 3 stacked maps + bot reaches win/lose | **implemented**: bot-to-terminal on all three maps (`maps_bot_outcome`); Godot C6 `mapsCompleted: crow-perch,broken-battlements,twin-spires` |
| Stacked structures fall in sim state when support is destroyed | **implemented**: `destroying_support_on_each_stacked_map_drops_the_crown`; C6 `stackedBlocksFell: true` |
| C3 C# `SafeHandle` vs release FFI | **implemented** on the Godot-free interop tests; recopy `target/release/db_sim_ffi.dll` to `client/native/win-x64/` |
| C4 Godot loadout picker, aim, falling structures, local match | **implemented**: every catalog item SelectTile + null-target fire; windowed C5/C6 screenshots 1280×720 |
| C5 human-finishable match + `PLAY.md` | **implemented**: C5 now starts through the picker on `crow-perch`, not the embedded fixture. Windowed C5 shows MATCH COMPLETE. Not a live human sitting at the keyboard. |
| Leftover C1 kit tests | **not restored** (41 `#[ignore]` kit tests stay kit-shaped). Crow-envelope timeout + preview covered on `crow-perch`. Object-spawn kits (knives/turrets) are not in this catalog. |
| Steam page | **not started** (after C5 only) |
| `broken-battlements` spawn ledges | **open owner call**: bot-vs-bot can still end 0hp vs 200hp by a fall. Remedy is floor/wider ledges, not another knockback cut. |

Language boundary (ADR 0006) is unchanged: Godot/C# presents; Rust is the only authority.

---

## 3. Envelope

Create request player object:

```json
{
  "playerId": "a-local-player",
  "team": 0,
  "loadout": {
    "main": "ramshot-cannon",
    "secondary": "recurve-bow",
    "meleeTool": "trench-spade"
  },
  "appearance": { "skinId": "default", "abilitySkinIds": ["default", "default", "default"], "victoryPoseId": "default" }
}
```

Ability commands use `"slot": "main" | "secondary" | "meleeTool"`.

Shared-fixture hashes at version 7:

| Vector | Hash |
|---|---|
| Initial | `864c1ec2512a0327` |
| After move | `57dc7133b8667daf` |
| After ability / final | `03388514a9108085` |

Do not assert the version-6 hashes (`f67c5371bcddbdf5` / `378081bb2e830a5d` / `d8686762470c0c36`).

---

## 4. Next dependency-driven step

After a human finishes a match from `PLAY.md`, Steam page work may start. Do not author
store copy during this cut.

If leftover C1 is reopened, un-ignore the kit tests only after they are rewritten against
the crow + item envelope — do not restore kits.

---

## 5. Gates

From the repo root, every slice:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `cargo test --release -p db-sim-ffi --locked`
- `cargo build --release -p db-sim-ffi --locked`
- `cargo deny check`

Then recopy the release FFI next to the C# RID dir and run `dotnet test client/DungeonBarrage.sln -c Release`.
