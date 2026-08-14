# Dungeon Barrage C# Client Specification

**Status:** Implementation specification — nothing built yet
**Engine:** Godot 4.x with C# (.NET 8)
**Related:** [adr/0004-native-desktop-rust-csharp.md](./adr/0004-native-desktop-rust-csharp.md) · [SECURITY_BASELINE.md](./SECURITY_BASELINE.md) · [CHARACTERS.md](./CHARACTERS.md) · [PRODUCT_SPEC.md](./PRODUCT_SPEC.md) · [PROGRAM_PLAN.md](./PROGRAM_PLAN.md)

This document is written so an engineer who has never seen the project can build the client
from it. Where a decision is already made it says so and why; where one is open it says that
too rather than inventing an answer.

---

## 1. What the client is

A native desktop application that **renders the authoritative simulation and collects player
intent.** Nothing else.

The simulation is `db-sim-core`, a Rust crate. The client calls into it over a C ABI. In a
local match the client hosts the core in-process; online, the same core runs on the server and
the client's copy is used only for prediction and playback. **The rules are identical in both
cases because it is literally the same compiled code.**

### The one rule

> **The client decides nothing.**

Not damage, not whether a shot hit, not ammunition, not terrain changes, not whose turn it is,
not who won. It sends intents and renders results. `SECURITY_BASELINE.md` §2 draws the trust
boundary with the client entirely on the untrusted side, and a native client is neither more
nor less trusted than a browser one — memory editing and packet forging were always in scope.

If you find yourself writing `if (damage > health) player.Die()` in C#, stop. That decision
belongs in Rust, and duplicating it creates a second source of truth that will disagree.

### What it is not

- Not a second implementation of the rules.
- Not a physics simulation. **Godot's physics, collision, and RNG are never used for
  gameplay.** They are not deterministic to the standard ADR 0001 §4 requires. Godot may
  animate cosmetic debris; it may not decide where a character stands.
- Not a level editor (deferred).
- Not the match server.

---

## 2. Why Godot 4 with C#

Decided in ADR 0004. Recorded here so the reasoning survives:

| Need | Why Godot supplies it |
|---|---|
| A shell, not an engine | The project owns its simulation; it needs rendering, input, audio, UI, and an asset pipeline |
| 2D strength | The game is a 2D side-view artillery game |
| Desktop export | First-class Windows/macOS/Linux |
| Licensing | MIT — no revenue terms, no runtime fees |
| C# support | .NET 8, first-class P/Invoke |

**MonoGame remains the recorded fallback.** If Godot's C# tooling proves obstructive, the
Rust boundary is identical either way — switching costs the shell, not the game. That property
is why the boundary is drawn where it is.

Console porting via a third-party house (W4 Games) is the known path, not a promise.

---

## 3. The FFI boundary

### 3.1 Current state — read this before writing bindings

`crates/db-sim-ffi` exists and exports only:

```c
uint32_t db_sim_simulation_version(void);
uint32_t db_sim_protocol_version(void);
uint32_t db_sim_content_version(void);
SimHandle* db_sim_create(uint64_t seed);
void      db_sim_destroy(SimHandle* handle);
int32_t   db_sim_state_hash(const SimHandle* handle, char* out, size_t out_len);
void      db_sim_string_free(char* ptr);
```

**The match API is not exported yet.** `MatchHost`, `submit_move`, `submit_ability`,
`pass_turn`, and state readback all need adding to `db-sim-ffi` before the client can do
anything. §3.4 specifies what to add. Do not begin the C# side expecting these to exist.

### 3.2 Status codes

Every fallible export returns `int32_t`:

| Value | Meaning |
|---:|---|
| `0` | `OK` |
| `-1` | `NULL_POINTER` — a required pointer was null |
| `-2` | `INVALID_UTF8` |
| `-3` | `REJECTED` — the simulation refused. **Not a fault.** A game-rules outcome |
| `-4` | `INTERNAL_PANIC` — a bug in the core. The process survives; abandon the match |
| `-5` | `BUFFER_TOO_SMALL` — nothing was written |

`REJECTED` is normal and frequent. Treat it as information for the player ("out of range"),
not an error dialog.

`INTERNAL_PANIC` is not normal. Log it, surface a "match desynchronised" state, and do not
continue playing — the state may be inconsistent.

### 3.3 Safety contract

Guaranteed by the Rust side:

- **No exported function panics across the boundary.** Every one is `catch_unwind`-wrapped.
  Unwinding across FFI is undefined behaviour and would take down a server holding other
  players' matches.
- **A null handle is tolerated, not undefined.** Every function checks and returns a status.
- **No floating point crosses the boundary.** Every gameplay scalar is a quantized integer.
- Strings out are UTF-8, NUL-terminated, owned by Rust, freed with `db_sim_string_free`.
  **Never free them with the .NET allocator** — the allocators differ and it will corrupt the
  heap.

Required of the C# side:

- A handle is opaque. Never construct, offset, or compare one.
- Destroy exactly once. Double-free is undefined behaviour on your side of the line.
- Do not hold a handle across an AppDomain reload without re-validating it.

### 3.4 Exports the client needs added

Specified here so the Rust work is unambiguous. Buffer-out rather than returning allocations,
so the caller controls lifetime.

```c
// Lifecycle
SimHandle* db_sim_match_start(uint64_t seed, const char* map_id);
void       db_sim_match_destroy(SimHandle*);

// Intents
int32_t db_sim_submit_move(SimHandle*, const char* player_id, int32_t dx, int32_t* travelled_out);
int32_t db_sim_submit_ability(SimHandle*, const AbilityCommandFfi* command, char* outcome_json, size_t len);
int32_t db_sim_submit_passive(SimHandle*, const char* player_id, const char* passive_id);
int32_t db_sim_pass_turn(SimHandle*);

// Readback
int32_t db_sim_phase(const SimHandle*, int32_t* phase_out);
int32_t db_sim_active_player(const SimHandle*, char* out, size_t len);
int32_t db_sim_outcome(const SimHandle*, int32_t* outcome_out);
int32_t db_sim_player_count(const SimHandle*, uint32_t* out);
int32_t db_sim_player_snapshot(const SimHandle*, uint32_t index, PlayerSnapshotFfi* out);
int32_t db_sim_block_count(const SimHandle*, uint32_t* out);
int32_t db_sim_block_snapshot(const SimHandle*, uint32_t index, BlockSnapshotFfi* out);
int32_t db_sim_terrain_dimensions(const SimHandle*, uint32_t* w, uint32_t* h);
int32_t db_sim_terrain_cells(const SimHandle*, uint8_t* out, size_t len);
int32_t db_sim_state_hash(const SimHandle*, char* out, size_t len);
```

**On the projectile timeline:** `CommandOutcome` carries sampled path points, impacts, terrain
operations, itemized damage, and created objects. That is a nested, variable-length structure
and marshalling it field-by-field across FFI would be miserable. Serialize it to JSON in Rust
and parse it in C#. This is **presentation data only** — it never feeds a decision — so the
cost is one allocation per action and the risk is nil. Do not use JSON for anything
authoritative.

`#[repr(C)]` structs for the flat snapshots:

```c
typedef struct {
    char     id[64];          // NUL-padded
    uint8_t  team;
    uint16_t health;
    uint16_t max_health;
    int32_t  position_x;      // fixed-point, POSITION_SCALE units per cell
    int32_t  position_y;
    uint16_t special_gauge;   // hundredths, 0..=10000
    uint8_t  has_chosen_passive;
    char     character_id[32];
} PlayerSnapshotFfi;

typedef struct {
    uint32_t id;
    int32_t  origin_cell_x;
    int32_t  origin_cell_y;
    uint16_t width_cells;
    uint16_t height_cells;
    uint16_t health;
    uint16_t max_health;
    uint8_t  material;
    uint8_t  erosion_axis;
} BlockSnapshotFfi;
```

Fixed-size `char` arrays rather than pointers: no lifetime question, no free, and a truncated
id fails loudly rather than dangling.

### 3.5 The C# binding layer

One file, `Interop/DbSim.cs`, is the **only** place `DllImport` appears. Everything else
talks to a wrapper.

```csharp
internal static class DbSimNative
{
    private const string Lib = "db_sim_ffi";

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern uint db_sim_simulation_version();

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern IntPtr db_sim_match_start(ulong seed,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string mapId);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern void db_sim_match_destroy(IntPtr handle);

    [DllImport(Lib, CallingConvention = CallingConvention.Cdecl)]
    internal static extern int db_sim_submit_move(IntPtr handle,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string playerId, int dx, out int travelled);
}
```

Wrapped in a `SafeHandle` so a leak is impossible even on an exception path:

```csharp
public sealed class MatchHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    public MatchHandle() : base(ownsHandle: true) { }
    protected override bool ReleaseHandle()
    {
        DbSimNative.db_sim_match_destroy(handle);
        return true;
    }
}
```

**Version check at startup, before anything else:**

```csharp
if (DbSimNative.db_sim_simulation_version() != ExpectedSimulationVersion)
    throw new InvalidOperationException(
        "Native simulation version mismatch — refusing to start. " +
        "A client and core built from different revisions will desynchronise silently.");
```

This must be a hard failure. `SIMULATION_VERSION` has already moved 1→4 during development;
a mismatched pair produces divergent matches with no visible symptom until the state hashes
disagree.

### 3.6 Native library placement

| Platform | File | Location |
|---|---|---|
| Windows | `db_sim_ffi.dll` | `client/bin/win-x64/` |
| Linux | `libdb_sim_ffi.so` | `client/bin/linux-x64/` |
| macOS | `libdb_sim_ffi.dylib` | `client/bin/osx-x64/` (universal preferred) |

Built by `cargo build -p db-sim-ffi --release` and copied by a pre-build step. The export
preset must include them or the shipped game will start and immediately fail its version
check — which is the correct failure, but only if the check exists.

---

## 4. Coordinate systems

Three, and confusing them is the most likely early bug.

| Space | Unit | Where |
|---|---|---|
| **Simulation** | fixed-point `i32`; `POSITION_SCALE = 1024` per cell | everything from the core |
| **Cell** | whole terrain cells | terrain mask, block spans |
| **Screen** | Godot pixels (`float`) | rendering only |

```csharp
public static class Coord
{
    public const int PositionScale = 1024;
    public const int BodyWidth     = 4 * PositionScale;   // one character body width
    public const float PixelsPerCell = 16f;               // art-direction choice

    public static Vector2 SimToScreen(int simX, int simY) => new(
        simX / (float)PositionScale * PixelsPerCell,
        simY / (float)PositionScale * PixelsPerCell);

    // Screen -> sim is for AIMING PREVIEWS ONLY. The value sent to the core is a quantized
    // angle and power, never a screen coordinate: the server must never receive a number
    // whose meaning depends on the client's resolution.
    public static int ScreenToSim(float px) =>
        (int)MathF.Round(px / PixelsPerCell * PositionScale);
}
```

**Y is positive downward** in both simulation and screen space, matching the terrain mask's
row-major top-left origin. Do not flip it.

`PixelsPerCell = 16` is a starting value. The horizontal test map is 50×20 cells → 800×320
pixels, which is small; expect to render at 2–4× zoom or increase the constant once art
exists.

---

## 5. Project layout

```
client/
  DungeonBarrage.csproj
  project.godot
  Interop/
    DbSimNative.cs          the only file with DllImport
    MatchHandle.cs          SafeHandle wrapper
    Snapshots.cs            [StructLayout] mirrors of the FFI structs
    LocalMatch.cs           idiomatic C# facade over the handle
  Scenes/
    Main.tscn               root, scene switching
    MainMenu.tscn
    CharacterSelect.tscn
    Match.tscn              the match view
    Results.tscn
  Match/
    MatchController.cs      owns LocalMatch, drives the view
    TerrainRenderer.cs
    BlockRenderer.cs
    CharacterView.cs
    ProjectilePlayer.cs
    EffectPlayer.cs
    CameraRig.cs
  UI/
    Hud.tscn / Hud.cs
    AimControl.cs
    AbilityBar.cs
    PassivePrompt.cs
    NetworkIndicator.cs
  Assets/
    Characters/<id>/        sprites, animation, sockets
    Terrain/
    Effects/
    Audio/
    Fonts/
  Settings/
    SettingsStore.cs
    InputMap.cs
```

**`Interop/` never references Godot types, and `Match/` never calls `DbSimNative` directly.**
That separation is what lets the interop layer be unit-tested headlessly and what stops
rendering concerns leaking into the boundary.

---

## 6. The match loop from the client's side

### 6.1 States

```
MainMenu → CharacterSelect → Match → Results
                               ↑        │
                               └────────┘  rematch
```

### 6.2 Within a match

The core's `MatchPhase` is authoritative. The client reads it and renders accordingly; it
never sets it.

| `MatchPhase` | Client behaviour |
|---|---|
| `MatchIntro` | Intro animation, camera sweep, roster reveal |
| `TurnStart` | Highlight the active character, refresh HUD, start the turn clock |
| `Movement` | **Accept input.** Movement and aiming both legal here |
| `AimingAndSelection` | Accept input. Same as above; the core distinguishes, the player need not |
| `PassiveSelection` | **Show the passive prompt. Block all other input.** |
| `CommandLocked` | Input off. Brief muzzle anticipation |
| `Resolution` | Play the projectile timeline from the outcome |
| `Settling` | Debris, dust, falling characters |
| `StatusResolution` | Status icons tick; damage-over-time numbers |
| `VictoryCheck` | Nothing visible — usually a single frame |
| `MatchComplete` | Freeze, then transition to Results |

**Never advance the phase locally to make the UI feel responsive.** Poll after each submitted
intent. Local aim and charge animation are fine and encouraged — they are not state.

### 6.3 Submitting an ability

```csharp
var command = new AbilityCommand {
    CommandId   = Guid.NewGuid().ToString(),   // idempotency key
    PlayerId    = _localPlayerId,
    ExpectedTurnNumber = _match.TurnNumber,    // read fresh; never cache across a turn
    Slot        = selectedSlot,
    AngleMillidegrees = (int)MathF.Round(aimAngleDegrees * 1000f),
    PowerBasisPoints  = (int)MathF.Round(chargeFraction * 10000f),
};

var outcome = _match.SubmitAbility(command);
if (outcome.Rejected) { ShowRejection(outcome.Reason); return; }
await _projectilePlayer.Play(outcome.Timeline);
RefreshFromCore();
```

`ExpectedTurnNumber` must be read fresh. `turn_number` previously advanced twice per action —
a real bug, now fixed — and a cached value would have been silently rejected as stale. Read
it, do not remember it.

**Angle and power are quantized at the boundary.** Millidegrees and basis points, integers.
Never send a float, never send a screen coordinate.

---

## 7. Rendering

### 7.1 Terrain

The core owns a byte-per-cell mask (`0` Empty, `1` Soil, `2` Wood, `3` ReinforcedStone).

**Do not draw a sprite per cell.** A 50×20 map is 1,000 cells; a real map will be far larger.
Build one `ImageTexture` from the mask and update only dirty regions.

```csharp
// Full rebuild on match start; per-action partial updates thereafter.
private void RebuildTerrain(ReadOnlySpan<byte> cells, uint w, uint h)
{
    var image = Image.CreateEmpty((int)w, (int)h, false, Image.Format.Rgba8);
    for (int y = 0; y < h; y++)
        for (int x = 0; x < w; x++)
            image.SetPixel(x, y, MaterialColor(cells[y * (int)w + x]));
    _texture.Update(image);
}
```

After an action, the outcome carries `terrainOps` with shapes and radii. Recompute only the
bounding box of each op — `PLATFORM_STRATEGY.md` §15 budgets a normal crater under 4 ms, and
a full rebuild will not meet that on a large map.

Use **nearest-neighbour filtering.** Linear filtering on a cell mask produces soft edges that
lie about where collision actually is, and players will aim at the lie.

### 7.2 Blocks

Blocks are rendered from the same mask — their cells *are* mask cells — but they are
**addressable entities** and want their own presentation:

- A health bar or tint per block, since health is what the player is actually attacking.
- Damage numbers on hit.
- **Erosion reads as the block shrinking, not as a hole appearing.** Health maps to surviving
  columns (ADR 0005), so a damaged platform gets narrower (or thinner, on the `Rows` axis).
  Animate the transition; a snap looks like a rendering glitch.

Read block health via `db_sim_block_snapshot`. **Do not infer it from the mask** — that is
backwards, and the whole point of the block model is that health is the authority.

### 7.3 Characters

One `Node2D` per player. Layered sprites for the paper-doll system (`PRODUCT_SPEC.md` §5):

```
back accessory → hair back → rear arm → body → outfit → face
→ hair front → headwear → front arm → held weapon skin
→ front accessory → combat effect
```

Every wearable layer for a rig shares frame tags and pivots: `idle`, `walk`, `aim`, `charge`,
`fire`, `melee`, `hit`, `fall`, `victory`, `defeat`. Every frame exposes a ground pivot plus
`mainHand`, `offHand`, `muzzle`, and `effectOrigin` sockets.

**Cosmetics never affect gameplay.** All characters share one collision capsule regardless of
skin. A skin may not change reach, hitbox, projectile origin, or animation timing, and may not
obscure which ability was used.

Position comes from the core every frame it changes. Interpolate for smoothness, but **snap to
the authoritative position** whenever they differ by more than a threshold — a client that
smooths away a correction is lying to the player about where they are.

### 7.4 Projectile playback

The outcome carries sampled path points with tick indices. The core samples at a fixed stride
(not every tick) to bound bandwidth.

```csharp
// Interpolate between samples against a wall clock; the SAMPLES are authoritative, the
// interpolation is not. Never extrapolate past the final sample — the impact point is
// exactly where the core says it is.
float t = elapsed / SecondsPerTick;
var (a, b) = SurroundingSamples(timeline, t);
sprite.Position = Coord.SimToScreen(Lerp(a, b, Fraction(t)));
```

Play the impact, terrain change, and damage numbers **at the impact sample**, never earlier.
Showing a hit before the core confirms it is the one visual lie that directly undermines trust
in an authoritative game.

### 7.5 Effects

Data-driven, not bespoke per-ability scene code. An effect definition names textures, counts,
lifetimes, speeds, and camera shake.

Cap active particles (~500–2,000, device-tier dependent) and degrade automatically. Reduce
particle count, debris lifetime, and post-processing **before** touching anything that affects
readability — and never reduce simulation fidelity, which the client does not control anyway.

---

## 8. Input

### 8.1 Default bindings

| Action | Keyboard | Mouse | Gamepad |
|---|---|---|---|
| Move left / right | A / D, ← / → | — | Left stick X |
| Aim up / down | W / S, ↑ / ↓ | Drag aim handle | Right stick Y |
| Charge power | Hold Space | Hold LMB | Hold RT |
| Fine aim | Shift + aim | Wheel | LT + stick |
| Ability 1 / 2 / 3 | 1 / 2 / 3 | Ability bar | Y / X / B |
| Camera pan | Middle drag / edge | Middle drag | Right stick + LB |
| Reset camera | R, Home | HUD button | Right stick click |
| Confirm | Enter | LMB | A |
| Cancel | Escape | RMB | B |
| Pass turn | P | HUD button | Back |
| Scoreboard | Tab (hold) | — | Select (hold) |

### 8.2 Requirements

- **Semantic action map**, not raw keys: `MoveLeft`, `AimUp`, `Charge`, `Commit`, `Cancel`,
  `FocusCharacter`. Godot's `InputMap` supplies this; use it from day one so remapping and
  controller support are not retrofits.
- **Fully remappable**, even if the first UI exposes only presets.
- **Never assume a mouse.** No hover-only affordance, no right-click-only action. Console is a
  stated future target and controller-only navigation is a Steam release gate.
- **Mouse sensitivity must not be a competitive advantage.** Fine aim is a modifier with a
  fixed increment, not "move the mouse slower".
- Suppress browser-style scroll/context behaviour only while the game surface has deliberate
  focus — irrelevant on desktop today, but the rule survives for a future web build.

---

## 9. HUD

`PRODUCT_SPEC.md` §6 requires all of the following visible during a match. This is a
checklist, not a suggestion:

- [ ] Active player and team
- [ ] Turn timer
- [ ] Current and upcoming turn order
- [ ] Wind direction and magnitude — **with a numeric readout**, not just an arrow
- [ ] Angle and power — numeric
- [ ] Selected ability, its name, and a concise special rule
- [ ] **Special gauge**, 0–100% (the core stores hundredths; divide by 100 for display)
- [ ] Movement allowance remaining
- [ ] Health, max health, and active statuses for every player
- [ ] Predicted Backlash cost **before commitment** — never a surprise
- [ ] Camera reset and focus-active-character controls
- [ ] Connection state (online only)
- [ ] Chat, mute, report (online only)

### The passive prompt

When `MatchPhase == PassiveSelection`, show three character-specific options and **block all
other input.** This is a one-time per-match choice at the player's first power spike; it must
not be dismissible or skippable. The core enforces this too — commands are refused in that
phase — but the UI should not let a player try.

### Result panel

Damage is itemized by the core: direct, splash, backlash, hazard, wall impact, healing,
critical flag, knockback vector, elimination flag. **Show the breakdown**, not just a final
health number. `PRODUCT_SPEC.md` §4 requires the elimination cause be identifiable, and the
itemization exists specifically so players can learn the systems rather than guess at them.

---

## 10. Camera

- Follows the active character by default; frames the projectile during resolution.
- Manual pan with a spring return to the action.
- Zoom limits chosen so the whole map fits at minimum zoom — artillery is a game about reading
  geometry, and a player who cannot see the target cannot plan.
- **Reduced-motion setting removes camera shake entirely** without changing timing.
- Never move the camera so fast that the projectile leaves frame; if the shot outruns the
  camera, widen instead of chasing.

---

## 11. Audio

| Category | Notes |
|---|---|
| Music | Menu, match, victory. Independent volume bus |
| Ability SFX | Per-ability, per-skin variants permitted |
| Impact | Varies by material — soil, wood, stone, character |
| UI | Selection, confirm, reject, timer warning |
| Voice | Per-character, optional; must be mutable separately |

**Every important audio cue needs a visual equivalent** (`PRODUCT_SPEC.md` §6). A player with
sound off must lose no information — including the turn-timer warning and the
gauge-ready cue.

---

## 12. Accessibility

Not a post-launch pass. Gate items from `PRODUCT_SPEC.md` §6:

- **No information conveyed by colour alone.** Team identity needs shape or icon as well as
  colour; projectile trails must differ in shape or pattern, not just hue.
- Wind, angle, power, gauge, and health all have **text or numeric** forms.
- Contrast checked on the HUD and all menus.
- **Reduced motion** removes shake and limits flashes, without changing timing — a
  reduced-motion player must not be at a competitive disadvantage.
- Full keyboard focus navigation; controller focus navigation before any console work.
- Scalable UI text.
- Remappable controls via the action map.

---

## 13. Settings

Persisted with Godot's `user://` config. **Device settings and account progression are separate
stores** (`PLATFORM_STRATEGY.md` §8) — graphics settings are per-machine, unlocks are per-account.

Graphics: resolution, fullscreen/borderless, vsync, particle density, screen shake, reduced
motion, UI scale.
Audio: master, music, SFX, voice, UI.
Gameplay: trajectory-guide level (see below), turn-timer warning threshold, camera follow
strength, damage-number display.
Controls: full remap, gamepad glyph style, stick deadzone.

**Trajectory guide, by mode** (`PRODUCT_SPEC.md`): training shows the full predicted arc and
impact marker; casual shows the first 20–35% of the path; ranked shows muzzle direction and
charge only. The client must not offer a "full arc" option in ranked — that would be a
client-side competitive advantage, which is exactly the thing the authority model exists to
prevent.

---

## 14. Local vs online

**Build local-only first.** `MatchHost` in-process, no network. This is a complete, playable
game against a bot and is the fastest path to knowing whether the firing loop is fun.

Online (M4+) changes only *where the authority lives*:

| Concern | Local | Online |
|---|---|---|
| Authority | in-process core | server's core |
| Intent | direct call | WSS message |
| Result | returned outcome | authoritative event |
| Client's core | is the authority | prediction + playback only |

`LocalMatch` and a future `RemoteMatch` implement one interface, so `MatchController` does not
know which it is talking to. That seam pays for itself precisely once — but it pays completely.

Online additions: reconnect with state snapshot, server clock offset, connection indicator,
turn deadline from the server (**never the local clock**), and a spectator path.

---

## 15. Performance budgets

From `PLATFORM_STRATEGY.md` §15, minus the web-specific entries:

| Target | Goal | Floor |
|---|---:|---:|
| Frame rate | 60 fps | stable 30 |
| Frame time p95 | < 16.7 ms | < 33.3 ms |
| Long tasks while aiming | none > 50 ms | rare, never repeated |
| Terrain update, normal crater | < 4 ms | < 16.7 ms |
| Warm start | < 2 s | < 4 s |
| Process memory | < 350 MB | < 500 MB |

Detect device tier by **measured frame time**, not by GPU name strings. Degrade particles,
debris lifetime, and post-processing first; never degrade readability.

---

## 16. Testing

| Layer | How |
|---|---|
| Interop | Headless xUnit against the real native library. No Godot dependency — this is why `Interop/` may not reference Godot |
| Determinism | Run a scripted match through C#, compare the state hash against `crates/db-sim-core/tests/golden_vectors.rs`. **A mismatch means the binding is wrong**, since the corpus is already frozen |
| UI | Godot integration tests for scene transitions and input mapping |
| Manual | The `horizontal_test_array` map is the standard scenario: 8 blocks, 8 spawns, 50×20 cells |

The determinism test is the important one. It is the only thing that catches a marshalling bug
that produces *plausible* values — and a plausible wrong value is the hardest kind to notice.

---

## 17. Build and CI

```bash
cargo build -p db-sim-ffi --release      # produces the native library
dotnet build client/DungeonBarrage.csproj
godot --headless --export-release "Windows Desktop" build/DungeonBarrage.exe
```

CI additions (`.github/workflows/ci.yml` currently has no C# job — add one only when there is
something to test; a job that tests nothing still reports green, which is worse than absent):

- `dotnet build` with warnings as errors
- `dotnet test` for interop and determinism
- `dotnet format --verify-no-changes`
- Native library present in the export preset
- **Version-match check**: the C# `ExpectedSimulationVersion` equals Rust's
  `SIMULATION_VERSION`. This one is worth automating early — it has already moved four times.

---

## 18. Client milestones

**C1 — Interop spike.** Load the library, call the version functions, start and destroy a
match. Nothing rendered. *Gate:* version check passes; no leaks under repeated start/destroy.

**C2 — Static render.** Draw terrain, blocks, and characters from a real snapshot. No input.
*Gate:* the `horizontal_test_array` map renders recognisably and matches the mask exactly.

**C3 — One playable turn.** Movement, aiming, firing, projectile playback, terrain update.
*Gate:* a human can take a turn end to end without a debugger.

**C4 — Complete local match.** Full HUD, turn cycle, passive prompt, victory, results, rematch.
*Gate:* a first-time player completes a match and understands the result without explanation.
This is `PROGRAM_PLAN.md`'s M3 gate.

**C5 — Polish.** Effects, audio, accessibility, settings, controller.
*Gate:* accessibility checklist passes; controller-only navigation completes a whole match.

**C6 — Online.** `RemoteMatch`, reconnect, spectate.

---

## 19. Open questions

Genuinely undecided. Do not resolve them silently.

1. **Art direction.** Pixel art or high-resolution illustrated? This drives `PixelsPerCell`,
   filtering, atlas budgets, and animation authoring. The reference screenshot cited in
   `PRODUCT_SPEC.md` §12 was supplied in an earlier session and is not currently in the repo.
2. **Bot AI location.** Rust (deterministic, shared with the server, replayable) or C#
   (easier to iterate)? **Recommendation: Rust** — a bot that behaves differently in a replay
   than it did live is a debugging nightmare, and training mode uses the same host as a real
   match by design.
3. **Turn timer.** `PRODUCT_SPEC.md` §2 sets 25 seconds of planning, but the core has no timer
   — `time_out_turn` exists and nothing calls it. Locally the client can own the clock;
   online the **server must**, and the client only displays it.
4. **Camera during opponent turns.** Follow their action, or let the player free-look and plan?
   §"waiting-time UX" argues for permitting non-state-changing activity.
5. **`PixelsPerCell`.** 16 is a placeholder. Depends entirely on (1).

---

## 20. Things that will bite

Collected from what has already gone wrong in this codebase. Every one of these is a real
pattern here, not a hypothetical.

- **Do not cache `turn_number`.** Read it fresh for every command. It desynchronised once
  already.
- **Do not infer block health from the mask.** Health is the authority; the mask is derived.
- **Do not use Godot physics for anything the player can feel.** It is not deterministic.
- **Do not smooth away an authoritative correction.** Snap when the divergence is real.
- **Do not show a hit before the core confirms it.**
- **Do not let `Interop/` reference Godot types.** It kills headless testing, which is where
  the determinism test lives.
- **Do not add a C# CI job before it tests something.** A green job that runs nothing is worse
  than no job — this project has shipped correct, tested, unreachable code four times, and a
  vacuous gate is the same failure wearing a different hat.
- **Check what calls your code, not just what tests it.** That single question would have
  caught all four.
