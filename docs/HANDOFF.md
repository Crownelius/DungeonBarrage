# Dungeon Barrage operational handoff

**Checkpoint date:** 2026-09-03

**Audience:** the next implementation agent

**State:** Playable cut on `SIMULATION_VERSION` 9 / `CONTENT_VERSION` 6. The
match-create/command envelope no longer carries `characterId` or character kits. Every
fighter is the one crow. Equipped items are ammunition. Loadout is a four-page wizard
(ranged, melee, one-shot secondary, crown/anklet) with eight items per page. Crowns and
anklets charge on two damaging hits. Leftover C1 timeout/preview/object-provenance kit
tests are `#[ignore]` until revisited.

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
| Envelope: no `characterId`/kits; one crow; items as ammo | **implemented** (`SIMULATION_VERSION` 9, `CONTENT_VERSION` 6). Melee-style stages, crow 280 HP, 2–3 cell craters. Eight ranged, eight one-shot secondaries, eight melee skins, eight charge-gated crowns/anklets. Ramshot knockback is two cells. |
| Visible fighter body equals the authoritative hitbox | **implemented**: `BODY_WIDTH` is a diameter; Rust publishes a half-body-width collision radius centered one radius above the ground pivot. Preview, apply, bot aim, projectile muzzle/playback, and Godot drawing all consume that geometry. Frozen preview/apply impacts are asserted inside the displayed target. |
| FFI `create`/`apply`/`snapshot` hashes equal direct Rust | **implemented** (`ffi_create_apply_snapshot_matches_direct_rust_on_the_duel_blocks_path`) |
| 3 stacked maps + bot reaches win/lose | **implemented**: bot-to-terminal on all three maps (`maps_bot_outcome`); Godot C6 `mapsCompleted: crow-perch,broken-battlements,twin-spires` |
| Stacked structures fall in sim state when support is destroyed | **implemented**: `destroying_support_on_each_stacked_map_drops_the_crown`; C6 `stackedBlocksFell: true` |
| C3 C# `SafeHandle` vs release FFI | **implemented** on the Godot-free interop tests; recopy `target/release/db_sim_ffi.dll` to `client/native/win-x64/` |
| C4 Godot loadout picker, aim, falling structures, local match | **implemented**: sequential 8-tile pages. Slingshot aim: drag **away** from the opponent; gold line is first impact; dotted arc is the flight. `A`/`D` walk, Space jump, arrows pan camera. Walking cuts this turn's max power (10% floor). Returning Boomerang bounces and craters. |
| C5 human-finishable match + `PLAY.md` | **implemented**: C5 now starts through the picker on `crow-perch`, not the embedded fixture. Windowed C5 shows MATCH COMPLETE. Not a live human sitting at the keyboard. |
| Leftover C1 kit tests | **not restored** (41 `#[ignore]` kit tests stay kit-shaped). Crow-envelope timeout + preview covered on `crow-perch`. Object-spawn kits (knives/turrets) are not in this catalog. |
| Steam page | **not started** (after C5 only) |
| `broken-battlements` spawn ledges | **open owner call**: bot-vs-bot can still end 0hp vs 200hp by a fall. Remedy is floor/wider ledges, not another knockback cut. |

Language boundary (ADR 0006) is unchanged: Godot/C# presents; Rust is the only authority.

**Adding an item with a displacement effect:** `magnitude_secondary` on a `Knockback` or `Push` is
a falloff radius, and `displacement.rs` reads `<= 0` as *no radius test at all* — full magnitude
against every living opponent anywhere on the map, not "the primary target only" as an earlier
comment claimed. The Ramshot shipped that way and ended every duel on turn 1. `validate_roster()`
now rejects a non-positive radius, so the catalog will refuse it rather than shipping it.

---

## 3. Envelope

Create request player object:

```json
{
  "playerId": "a-local-player",
  "team": 0,
  "loadout": {
    "main": "ramshot-cannon",
    "secondary": "ramshot-shell",
    "meleeTool": "trench-spade",
    "trinket": "ember-crown"
  },
  "appearance": { "skinId": "default", "abilitySkinIds": ["default", "default", "default"], "victoryPoseId": "default" }
}
```

Ability commands use `"slot": "main" | "secondary" | "meleeTool" | "trinket"`.

Shared-fixture hashes at `SIMULATION_VERSION` 9 / `CONTENT_VERSION` 6:

| Vector | Hash |
|---|---|
| Initial | `1028333c8a2e9f0f` |
| After move | `b0e9ba84389a6797` |
| After ability / final | `682f0e2a57b7debd` |

**Read these from the fixture files, not from this table.** Every content bump moves all three,
because `content_version` is part of the hashed state. This table has already been stale once: it
carried the `CONTENT_VERSION` 2 values (`864c1ec2512a0327` / `57dc7133b8667daf` /
`03388514a9108085`) after content moved to 4. The source of truth is
`tests/fixtures/matches/horizontal-test-duel-v1/responses/*.json`, and
`shared_match_fixtures` is what enforces it.

### Bumping `CONTENT_VERSION`

Any change to an item, ability, effect, or map is a content change. It is a six-step edit, and
missing any one of them fails somewhere far from the cause:

1. `crates/db-sim-core/src/lib.rs` — `CONTENT_VERSION`.
2. `client/src/DungeonBarrage.Client/Settings/presentation-manifest-v1.json` — `contentVersion`.
   `PresentationManifestTests` now fails in `dotnet test` if you forget; before that gate existed
   this only surfaced as "Presentation content N, request content M ... must match" at Confirm, in
   an exported build.
3. Fixture inputs: `create-request.json` and `fixture.json` — `contentVersion`.
4. Regenerate the frozen corpus:
   `cargo test -p db-sim-ffi --lib -- --ignored regenerate_shared_response_fixtures_from_production_abi`.
   That writer is the only sanctioned one; it goes through the production ABI.
5. Sync every pinned hash: `fixture.json`, `crates/db-sim-ffi/src/tests.rs`,
   `FrozenResponseFixtureTests.cs`, `CommandRoundTripTests.cs`.
6. Regenerate all five golden vectors, recording the old value and the reason beside each
   constant — `crates/db-sim-core/tests/golden_vectors.rs` documents the rule.

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
CI repeats the Godot-free C# gate on Linux against a freshly built `libdb_sim_ffi.so`.

Unit tests are not sufficient on their own for anything that touches content, the manifest, or the
Godot screens. Export and run the C6 smoke as well:

```powershell
& $env:DUNGEON_BARRAGE_GODOT --headless --path client/src/DungeonBarrage.Client `
    --export-release "Windows Desktop" <out>/DungeonBarrage.exe
<out>/DungeonBarrage.exe --headless -- --c6-smoke-report <out>/report.json --c6-screenshot <out>/shot.png
```

`success: true` is not the whole check — read the report. `allPlayableMapsCompleted`,
`stackedBlocksFell`, clean rematch disposal, and non-zero dimensions from the renderer-backed
(non-headless) run are required. `turnsPlayed` is diagnostic rather than a fixed threshold after
the simulation-9 hitbox correction; the smaller visible target can make bot-only matches reach
the hard turn limit. This smoke proves the complete client flow, not the still-required human
playthrough. `humanMainItemId` and `botMainItemId` must differ, since both once read from the same
field and hid that the bot mirrored the player's loadout.
