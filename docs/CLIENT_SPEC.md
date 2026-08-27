# Dungeon Barrage client specification

**Status:** Version 2 implementation specification

**Updated:** 2026-08-26

**Launch client:** Godot 4.7.1 .NET, C# targeting `net10.0`

**Authoritative simulation:** Rust `db-sim-core`

**Local boundary:** coarse, versioned C ABI through `db-sim-ffi`

**Online boundary:** authoritative server protocol; no client prediction initially

**Related:** [ADR 0006](./adr/0006-client-and-server-language-boundaries.md) · [ADR 0004](./adr/0004-native-desktop-rust-csharp.md) · [ADR 0002](./adr/0002-character-kits.md) · [ADR 0005](./adr/0005-destructible-blocks-with-health.md) · [SECURITY_BASELINE.md](./SECURITY_BASELINE.md) · [CHARACTERS.md](./CHARACTERS.md) · [PRODUCT_SPEC.md](./PRODUCT_SPEC.md) · [PROGRAM_PLAN.md](./PROGRAM_PLAN.md)

This document defines an implementable native client, including the Rust contracts it requires.
It deliberately separates what exists from what must be built. A milestone is complete only when
its stated evidence exists; a class name or a green test that does not exercise a real match is not
evidence.

---

## 1. Authority and source precedence

The repository contains older documents written before the platform and character-kit pivots.
Use this precedence when two sources disagree:

1. The newest accepted ADR governing the disputed decision.
2. This document for client architecture, presentation, input, packaging, and client-facing
   Rust/FFI requirements.
3. Specialized current design documents such as `CHARACTERS.md`, `SECURITY_BASELINE.md`, and
   ADR 0005.
4. Non-superseded portions of `PRODUCT_SPEC.md` and `PLATFORM_STRATEGY.md`.
5. Historical plans and retired code under `reference/`.

In particular:

- ADR 0006 governs the current language boundaries: Godot/C# presentation, Rust simulation, and a
  future Rust-native server.
- ADR 0002 replaces the old three-weapon loadout with fixed character kits. Ammunition and the
  `main | secondary | meleeTool` equipment contract are retired. Client UI uses
  `basic | basicAlt | special`.
- ADR 0004's web removal still stands, but its ASP.NET/C# server choice is superseded by ADR 0006.
- ADR 0005 governs destructible block health and mask derivation.
- `PLATFORM_STRATEGY.md` remains useful for general authority, networking, privacy, and performance
  intent, but its PWA, browser, Electron, TypeScript, and Node-specific sections are historical.

Current code is evidence of implementation state, not permission to silently weaken this contract.
If the code cannot satisfy a requirement, record and resolve the gap before building a workaround in
C#.

### 1.1 The one gameplay rule

> **Presentation never decides an authoritative result.**

The Rust host owns legality, phase, active player, turn version, random draws, movement allowance,
projectiles, collision, terrain, blocks, health, status, gauge, passive selection, elimination, and
victory. The C# client submits intent and presents authoritative snapshots and transitions.

Godot physics, collision, navigation, and RNG are never used for gameplay. Cosmetic debris may use
Godot physics only when it cannot obscure or alter an authoritative result.

The local application contains an in-process authority for training and local matches. That does not
make the Godot view authoritative: `LocalMatchSession` owns the Rust host and clocks; scenes still
consume the same intent/snapshot/transition interface as a remote client.

### 1.2 What the client owns

- Rendering, animation, particles, camera, audio, and UI.
- Semantic input and uncommitted local aim/charge state.
- Playback progress through an already-authoritative transition.
- Device settings and local accessibility preferences.
- Connection presentation and retry requests.

### 1.3 Non-goals for the first client

- No online prediction or rollback.
- No second implementation of ballistics or rules in C#.
- No level editor, user-authored scripting, or downloaded executable content.
- No web export. Godot 4 C# web delivery is not part of the launch architecture.
- No ranked mode, public matchmaking, store, progression UI, or account system before the local
  match gate.
- No engine abstraction intended to make Godot, MonoGame, and another renderer interchangeable.
  Domain/session code remains Godot-free; scenes are allowed to be good Godot code.

---

## 2. Settled stack and supported targets

| Layer | Choice | Constraint |
|---|---|---|
| Presentation | Godot **4.7.1 .NET** | Use the .NET editor and matching export templates, not the standard editor |
| Client language | C# targeting **`net10.0`** | Nullable enabled; warnings are errors in owned code |
| Local simulation | Rust `db-sim-core` | Linked only through `db-sim-ffi` from C# |
| Native boundary | Coarse C ABI | Opaque handle plus versioned byte envelopes; no gameplay logic in FFI |
| Future server | Rust-native | Links `db-sim-core` directly; it does not P/Invoke through C# |
| Online client | Authoritative playback | No local simulation prediction until measured latency proves it necessary |

Pin the toolchain in the repository before C1:

- `global.json` pins a .NET 10 SDK accepted by Godot 4.7.1.
- `rust-toolchain.toml` pins the reviewed Rust toolchain.
- The Godot version appears in a bootstrap script and CI, not only in prose.
- NuGet restore uses a lock file.

Target order:

1. Windows x64 development and first playable export.
2. Linux x64, including Steam Deck desktop mode.
3. macOS x64 and arm64. A universal application must contain both matching native libraries and be
   signed as one bundle.

Cross-platform support is not claimed until an exported build, launched outside the source tree on
that platform, completes the smoke scenario in §20.5.

---

## 3. Truthful current state

The committed baseline from which this v2 work began was `fa7f0af`. C1 and C2 are now implemented
on `feat/c1-outcome-provenance`; a later capability remains incomplete until its gate in §20 passes:

- `db-sim-core` has real match orchestration, maps, movement, ability resolution, terrain blocks,
  passive interruption, status ticking, victory, and frozen versioned golden vectors in the
  committed repository.
- `MatchHost` currently resolves most of a turn synchronously before `submit_ability` returns.
  Transient phases are therefore not observable client animation states.
- A validated, transport-free `MatchConfig`/`create_match` path now constructs a real host from the
  horizontal-test map and character definitions.
- Every projectile now retains its own trace and impact. Host submissions now report the post-host
  turn number and state hash, including settling, passive interruption, and turn rotation. Reported
  gauge gain is the capped action delta rather than the resulting total.
- A read-only, engine-neutral core snapshot projection covers every authoritative state field a
  client must render, including deterministic turn order, elimination, terrain generation, sorted
  entities, and the exact host hash.
- The committed feature checkpoint contains a transport-free `MatchSessionHost`: one normalized typed
  `MatchCommand` union, canonical semantic digests, generation ownership, cloned-host application,
  retained accepted/rejected first results, exact duplicate replay, changed-command-ID security
  rejection, exact 16,384-entry/64 MiB canonical-byte resource bounds, and atomic
  `MatchTransition` plus post-snapshot/hash.
  Accepted operations increment generation exactly once only when authoritative state actually
  changes; a legal zero/blocked move remains accepted without inventing a generation.
- Its event builder preserves independent projectile traces/impacts; emits exact per-strike crit,
  damage, delivery, public random-outcome, status-lifecycle, and ordered object-lifecycle records;
  derives deterministic net movement, health, gauge, passive, elimination, turn, and outcome events;
  and produces exact changed-cell row-runs for terrain. Arzum target selection and Aleph point draws
  are recorded at their producer, reconciled against draw-time state and the bounded generator, and
  never inferred from the final snapshot. All current net movement is conservatively labelled
  authoritative resolution: without the post-walk/pre-settle path, even an unchanged final height
  cannot prove that no climb-and-settle occurred. `requestedMove` remains reserved for a richer
  movement outcome. Strike, random, status, and object records fail closed under omitted, duplicate,
  stale, or tampered provenance.
- Movement settling now ends an eliminated active player's turn and drives victory/rotation. The
  former path could leave a dead active player stranded in `Movement`. Terminal victory now also
  commits the final pending turn reason instead of exposing the prior turn's reason. Version 6 also
  makes status duration mean affected-player turns, makes Feeding Frenzy force/consume the next three
  live Carrion Call crits without RNG draws, and removes a defeated owner's persistent objects for
  ordinary damage/fall elimination. Every golden vector and shared hash was regenerated under the
  documented compatibility procedure.
- Authority-owned timeout is a separate non-client session entry point with the same bounded,
  idempotent ledger. Read-only ability preview is implemented on disposable clones, including normal
  stale-generation refusals and exact no-state/no-ledger/no-RNG assertions. Restore accepts only the
  opaque host-plus-complete-ledger checkpoint, revalidates entry relationships and transitions, and
  recomputes exact retained bytes before reopening it.
- `db-sim-ffi` now owns a real `MatchSessionHost` behind a serialized, poisonable handle. ABI version
  1 exposes exactly the ten version/create/apply/snapshot/terrain/preview/disposal symbols in §8.1.
  Inputs are closed, bounded DTOs; mutating calls resolve and serialize a clone before commit; output
  allocations are exact `Box<[u8]>` values reclaimed by Rust.
- The shared horizontal duel passes both direct Rust and the real C ABI with hashes
  `f67c5371bcddbdf5 → 378081bb2e830a5d → d8686762470c0c36`. Create, initial snapshot,
  preview, move, and ability responses are frozen byte-for-byte from the production serializer.
- The native release profile uses `panic = "unwind"`; the common guard is tested in release, every
  later call on a poisoned handle fails closed, and the complete ownership cycle passes Valgrind with
  zero definitely/indirectly lost bytes. CI enforces the release test, exact exports, and leak gate.
- No `client/` project exists.
- No match server exists.

Consequently, the next work is C3's headless .NET interop/session layer. Do not begin menu, HUD, or
Godot scene construction until the same raw fixture passes C# through the real release library.

---

## 4. Architecture and dependency rules

```text
Godot scenes and views
        │
        ▼
DungeonBarrage.Client          presentation coordination, view models, input contexts
        │
        ▼
DungeonBarrage.Client.Contracts  IMatchSession, commands, snapshots, transitions
        ▲                              ▲
        │                              │
LocalMatchSession                 RemoteMatchSession (future)
        │                              │
db-sim-ffi C ABI                  WSS protocol
        │                              │
db-sim-core                    Rust match server ── db-sim-core
```

Binding rules:

- `Client.Contracts` references neither Godot nor native interop.
- `Client.Interop` references `Client.Contracts`, owns all native calls, and references no Godot
  types.
- Godot scenes call `IMatchSession`; they never call native exports.
- Only `DbSimNative.cs` may declare `LibraryImport`/`DllImport`.
- A native handle is owned by exactly one `LocalMatchSession` and used through one serialized
  executor. `db-sim-ffi` is not assumed thread-safe.
- Background completion is marshalled onto Godot's main thread before touching a `GodotObject`.
- The future Rust server links the core directly. Network DTOs may mirror domain concepts, but the C
  ABI is not the network protocol.

### 4.1 Project layout

```text
client/
  DungeonBarrage.sln
  Directory.Build.props
  src/
    DungeonBarrage.Client.Contracts/
      IMatchSession.cs
      Commands.cs
      MatchConfig.cs
      MatchSnapshot.cs
      MatchTransition.cs
      PresentationEvents.cs
    DungeonBarrage.Client.Interop/
      DbSimNative.cs
      DbSimBuffer.cs
      MatchSafeHandle.cs
      LocalMatchSession.cs
      NativeLibraryResolver.cs
    DungeonBarrage.Client/
      DungeonBarrage.Client.csproj
      project.godot
      export_presets.cfg
      App/
      Match/
      UI/
      Scenes/
      Assets/
      Settings/
  tests/
    DungeonBarrage.Client.Contracts.Tests/
    DungeonBarrage.Client.Interop.Tests/
  native/
    win-x64/
    linux-x64/
    osx-x64/
    osx-arm64/
tests/
  fixtures/
    matches/                 machine-readable fixtures shared by Rust and C#

global.json                  pinned .NET SDK for the whole repository
rust-toolchain.toml          pinned Rust toolchain for the whole repository
```

Do not place test projects under the Godot resource root if that makes Godot import their files.

---

## 5. Two state machines, not one

### 5.1 Authoritative match state

Rust `MatchPhase` remains authoritative for command acceptance and replay/hash semantics. It may pass
through `CommandLocked`, `Resolution`, `Settling`, `StatusResolution`, and `VictoryCheck` entirely
inside one host call. The client must not poll those transient values to drive animation.

The snapshot normally exposes a stable input boundary:

- `Movement` or `AimingAndSelection`: the active player may submit allowed intent.
- `PassiveSelection`: only the owed passive choice is accepted.
- `MatchComplete`: terminal.

Other phases may appear in diagnostics or a reconnect snapshot, but no client correctness depends on
observing them for a particular number of frames.

### 5.2 Client playback state

```text
LoadingSnapshot
      ↓
ReadyForInput ──submit──> Submitting
      ↑                     │
      │                     ├── rejected ──> ReadyForInput
      │                     │
      │                     └── accepted ──> PlayingTransition
      │                                        │
      │                      ┌─────────────────┼──────────────────┐
      │                      ▼                 ▼                  ▼
      └──────────────── ReadyForInput   PassivePrompt      MatchComplete

Any state ── unrecoverable boundary/desync fault ──> MatchFaulted
```

`PlayingTransition` is local presentation state. It never advances Rust phases. The session may
already hold the authoritative post-transition snapshot while views animate from the prior visual
state. During playback all gameplay submission is blocked.

When playback ends, every view snaps to the post-transition snapshot before input opens. Cosmetic
interpolation never accumulates into the next transition.

### 5.3 Clock ownership

- Local: `LocalMatchSession`, not a Godot scene, owns a monotonic planning clock and calls the core's
  timeout operation. An accepted transition supplies `inputLockTicks`; the session opens the next
  planning window when that deterministic minimum has elapsed, whether cosmetic playback finished
  normally or had to be accelerated. No animation-complete callback starts the authority clock.
- Online: the Rust server supplies `inputOpensAt` and `deadlineAt` in server time. The client displays
  a clock using a measured offset. It never submits a timeout command.
- Replays: timeout is a recorded authoritative event, not reconstructed from wall-clock time.

---

## 6. Version model

Four versions have distinct purposes:

| Version | Governs | Compatibility rule |
|---|---|---|
| `ABI_VERSION` | Native export names, call signatures, buffer ownership | Exact match before first native handle is created |
| `SIMULATION_VERSION` | Deterministic gameplay/replay behavior | Pinned for a match; exact for local fixture replay |
| `CONTENT_VERSION` | Character, ability, passive, mode, and map definitions | Snapshot/asset manifest must agree |
| `PROTOCOL_VERSION` | Future client/server messages | Server advertises a supported range before join |

This is a **coarse ABI**: a small set of functions carries versioned envelopes. Adding a field to an
envelope does not add an export. Increment `ABI_VERSION` only when the native calling convention,
function set, ownership, or envelope decoding compatibility breaks. Every envelope also contains a
`schemaVersion` so decoders fail clearly rather than guessing.

Startup behavior:

1. Resolve the library from the application-owned native directory.
2. Call version functions only.
3. Compare `ABI_VERSION` exactly and verify supported simulation/content versions.
4. On mismatch, show a fatal repair/update screen. Do not throw an unhandled exception or attempt a
   match.

Online play does not require a local core match. The server protocol handshake, not the bundled
library's simulation version, determines online compatibility.

---

## 7. Domain contracts

The JSON definitions below are the normative schema for the initial gameplay ABI, not illustrative
pseudocode. C2 implements the Rust wire DTOs plus byte-for-byte fixtures; C3 implements matching C#
DTOs against those frozen bytes. Wire casing is `camelCase`; enums use the exact string values shown;
duplicate object keys, unknown fields, unknown enum values, non-integer numbers, and trailing data
are rejected. Inputs are UTF-8 bytes with an explicit length, not NUL-terminated strings.

Production JSON responses use deterministic struct field order, compact UTF-8, and exactly one
terminal LF. The shared C2 fixtures compare those bytes directly; no test-only serializer exists.

### 7.1 `MatchConfig`

`db_sim_match_create` receives this exact `MatchCreateRequest` shape:

```json
{
  "schemaVersion": 1,
  "matchId": "local-opaque-id",
  "simulationVersion": 6,
  "contentVersion": 1,
  "match": {
    "seed": 12345,
    "mapId": "horizontal-test-array",
    "mode": "turnBased",
    "players": [
      {
        "playerId": "local-player-1",
        "team": 0,
        "characterId": "huck",
        "appearance": {
          "skinId": "default",
          "abilitySkinIds": ["default", "default", "default"],
          "victoryPoseId": "default"
        }
      },
      {
        "playerId": "local-bot-1",
        "team": 1,
        "characterId": "zeke",
        "appearance": {
          "skinId": "default",
          "abilitySkinIds": ["default", "default", "default"],
          "victoryPoseId": "default"
        }
      }
    ]
  }
}
```

Requirements:

- The outer request is FFI/session metadata. It validates the exact simulation/content versions and
  retains `matchId` for diagnostics and envelopes; neither `matchId` nor snapshot generation is part
  of the authoritative state hash.
- The nested object maps one-to-one to the transport-free Rust `match_setup::MatchConfig`. Rust
  validates IDs, a two-to-four-player count, uniqueness, at least two teams, roster membership,
  spawn availability, and mode limits before allocating a live match. C2 also bounds every
  appearance identifier; C4 validates local appearance references against the version-matched
  presentation manifest before creation. Online ownership/entitlement validation remains server
  authority. Cosmetic appearance never enters the simulation hash.
- Player array order is lobby order and deterministically assigns map spawn points before players are
  sorted by ID. There is no client-supplied `spawnIndex` in the initial contract.
- Human/bot ownership is `LocalMatchSession` configuration and never crosses this gameplay creation
  envelope. Bot decisions are generated by a future Rust bot coordinator and submitted as ordinary
  normalized commands.
- A passive is not chosen here. It is selected when the authoritative match raises the first-gauge
  interrupt.
- The initial implementation supports one real fixture: a two-player duel on
  `horizontal-test-array`. General map loading follows only after that fixture passes end to end.

### 7.2 `MatchCommand`

Every mutating command has:

- `schemaVersion`.
- A deterministic, match-unique `commandId`.
- `playerId`.
- `expectedTurnNumber`.
- `expectedSnapshotGeneration`.
- One explicit command kind and its bounded payload.

Initial command kinds:

| Kind | Payload | Notes |
|---|---|---|
| `move` | signed fixed-point `dx` | Immediate authoritative movement; capped by allowance |
| `ability` | slot, angle millidegrees, power basis points, optional target IDs | No float or screen coordinate crosses the boundary |
| `passiveChoice` | passive ID | Accepted only during `PassiveSelection` |
| `pass` | no gameplay payload | Ends the current turn explicitly |

The exact discriminated shapes are:

```text
move:
  { schemaVersion, commandId, playerId, expectedTurnNumber,
    expectedSnapshotGeneration, kind: "move", dx }

ability:
  { schemaVersion, commandId, playerId, expectedTurnNumber,
    expectedSnapshotGeneration, kind: "ability",
    slot: "basic" | "basicAlt" | "special",
    angleMillidegrees, powerBasisPoints,
    targetPlayerId: string | null,
    secondaryTargetPlayerId: string | null }

passiveChoice:
  { schemaVersion, commandId, playerId, expectedTurnNumber,
    expectedSnapshotGeneration, kind: "passiveChoice", passiveId }

pass:
  { schemaVersion, commandId, playerId, expectedTurnNumber,
    expectedSnapshotGeneration, kind: "pass" }
```

All named fields are required, including nullable target IDs; no variant accepts fields from another
variant. `jump` is reserved and rejected until the core tracks its action budget. `timeout` is an
authority-only operation and a remote client can never send it.

For idempotency, Rust first decodes into this closed typed union and then hashes its deterministic
canonical encoding. JSON whitespace and object-key order therefore do not make semantically
identical commands different; a changed typed field does.

Idempotency behavior is exact:

- The Rust session-host layer, immediately outside `MatchHost`, owns the command ledger. It records
  canonical request bytes/digest plus the serialized result for every first well-formed receipt.
- First receipt applies at most once. The command ledger is session metadata: recording a rejection
  does not alter simulation state, snapshot generation, or the state hash.
- Same `commandId` plus identical request returns the original transition without mutation.
- Same `commandId` plus different request is rejected as a security event without mutation.
- Rejected, accepted, and duplicate results remain in the ledger for the lifetime of a live match.
  The initial limits are 16,384 entries and 64 MiB of canonical request/response bytes. The layer
  checks both limits before committing a cloned host; crossing either returns a resource-limit fault
  without mutation and closes the session to further commands. Server rate limits are additional.
- The ledger is persisted with an online match and reproduced from recorded commands during replay;
  it is not silently evicted. A rejected command never consumes a turn.

Tests use fixed command IDs from fixtures. Runtime UUIDs are recorded in the command log and replayed
unchanged.

### 7.3 `MatchSnapshot`

A snapshot is one atomic authoritative read, never a sequence of independently indexed FFI calls.
It contains:

- Envelope, ABI, simulation, content, and schema versions.
- Match ID, snapshot generation, authoritative tick, turn number, stable phase, active player,
  current/upcoming turn order, wind, movement remaining, and whether an attack is committed.
- Planning-window timestamps when a clock is active.
- Outcome, including winning team or draw.
- State hash for the exact authoritative state represented.
- Players sorted by ID: team, health/max health, fixed-point position, character ID, passive ID,
  gauge, statuses, elimination state, and appearance.
- Blocks sorted by ID: bounds, material, health/max health, and erosion axis.
- Persistent objects sorted by sequence: kind, owner, position, health, and remaining lifetime.
- Terrain width, height, and `terrainGeneration`.

The engine-neutral Rust core projection owns the state-derived fields, turn order, terrain
generation, and state hash. The Rust session-host/FFI adapter adds match ID, ABI/schema envelope,
snapshot generation, map metadata, and local clock timestamps while holding the same per-handle
lock. The serialized object is the composite of those layers; the C# client never joins two reads.

The terrain cell bytes are read through the separate coarse terrain export in §8.3 when
`terrainGeneration` changes. This avoids base64 expansion without returning one struct per cell.

Snapshots reference character/ability/passive IDs. Localized names and descriptions come from the
version-matched presentation manifest; authoritative numeric previews come from Rust.

### 7.4 `MatchTransition`

A transition is the atomic response to one command or one authority-generated action:

```text
MatchTransition
  schemaVersion
  commandId
  disposition: accepted | rejected | duplicateReplay
  rejectionReason: TransitionRejection | null
  preSnapshotGeneration
  postSnapshotGeneration
  presentationTickRate
  inputLockTicks
  events[]
  postSnapshot
  postStateHash
```

The rejection union is exact:

```text
TransitionRejection =
  { kind: "snapshotGenerationMismatch", expected: u64, actual: u64 }
  | { kind: "commandIdConflict" }
  | { kind: "core", reason: CommandRejectionName }

CommandRejectionName =
  "duplicateCommand" | "playerEliminated" | "notActivePlayer" | "wrongPhase"
  | "turnVersionMismatch" | "unknownCharacter" | "abilityNotAvailable"
  | "gaugeNotReady" | "alreadyAttacked" | "inputOutOfRange" | "invalidTarget"
  | "invalidPassive" | "passiveAlreadyChosen"
```

`rejectionReason` is a required field even when null. A duplicate replay returns the original
transition body with `disposition: "duplicateReplay"`; it does not invent a rejection.

Binding invariants:

- Snapshot generation is `u64` session-host metadata and is not part of canonical simulation state or
  its hash. The initial snapshot is generation `0`. Each accepted operation that mutates the host —
  move, ability plus all synchronous settling/interrupt work, passive choice, pass, timeout, or
  authority action — increments it exactly once after bounded serialization succeeds. Rejection does
  not increment it; duplicate replay returns the recorded original pre/post generations.
- `postStateHash` is computed by `MatchHost` after every mutation performed by that host call.
- `postStateHash == postSnapshot.stateHash == hash_state(host.state())` in the direct Rust path.
- An ordinary rejection has no events, identical pre/post generation, and an unchanged hash.
- A legal accepted operation that produces no authoritative change, such as a zero/fully blocked
  move, also keeps the same generation and has no fabricated state-change events. It is still
  retained for idempotent replay.
- Events are sorted by `(presentationTick, sequence)`; `sequence` is unique within the transition.
- Every projectile or moving entity has a stable transition-local ID. Samples from different
  projectiles are never concatenated into one anonymous list.
- `inputLockTicks` is the authority/session minimum before the next local planning window can open;
  it is computed from the last required event tick plus a versioned post-action lock. It is not the
  duration of optional camera, particle, or audio tails. A rejection uses zero. Online timestamps,
  not this relative value, govern the server deadline.
- The transition is sufficient to present the action; transient `MatchPhase` polling is not needed.
- The post-snapshot is the reconciliation authority. If a view ends elsewhere, it snaps to it.
- A `duplicateReplay` carries the recorded original post-snapshot, which may be older than the live
  session after later commands. It acknowledges the original receipt; a client must not install it
  over a newer generation.

### 7.5 Presentation events

Initial closed event kinds:

| Event | Required payload |
|---|---|
| `projectileTrace` | trace ID, owner, ability, sampled fixed-point positions with ticks, terminal impact |
| `impact` | trace ID, position, cause, material/entity target where known |
| `strikeResolved` | owner, ability, dense strike index, target, exact point, melee/projectile/effect delivery, cited trace where applicable, `notEligible`/`missed`/`landed`/`forced` crit provenance, applied damage, elimination flag |
| `terrainChanged` | terrain generation and authoritative dirty rectangles |
| `blockChanged` | block ID, previous/new health and surviving bounds |
| `healthChanged` | player ID and itemized direct/splash/Backlash/hazard/wall/heal values |
| `gaugeChanged` | player ID, previous/new gauge, actual delta |
| `randomOutcome` | roll purpose, bounded public result, and affected action/entity IDs |
| `statusChanged` | player ID, status kind, exact `Applied`, `Refreshed`, `ChargeConsumed`, `Ticked`, `Exhausted`, or `Expired` transition |
| `entityMoved` | player/object ID, cause, sampled or start/end positions |
| `objectSpawned` | complete persistent-object snapshot |
| `objectChanged` | complete previous/current object snapshots |
| `objectRemoved` | complete last object snapshot and exact closed producer cause (`replaced`, `capacityEvicted`, `detonated`, `ownerEliminated`; `expired`/`destroyed` remain reserved until those mechanics exist) |
| `playerEliminated` | player ID and identifiable cause |
| `passiveChoiceRequired` | player ID and three allowed passive IDs |
| `passiveChosen` | player ID and accepted passive ID |
| `turnEnded` | player ID and attacked/passed/timedOut/eliminated reason |
| `turnOpened` | `playerId`, `turnNumber`, required-nullable `inputOpensAt: u64 | null`, and required-nullable `deadlineAt: u64 | null` |
| `matchCompleted` | victory team or draw |

Damage, terrain, and health values are authoritative. Particle count, camera shake, easing, and the
spacing of purely cosmetic anticipation frames are not. An event at an impact tick is not shown
before the projectile reaches that tick.

If a mechanic cannot be represented by this vocabulary, extend and version the event contract in
Rust before implementing bespoke C# inference.

The C1 implementation is deliberately truthful where richer path provenance is not retained: a net
movement change may still carry only `authoritativeResolution`. Arzum/Aleph draws, strike crits,
ephemeral status lifecycles, and object spawn/removal causes are producer-owned and fail closed.
Arzum reconciliation reconstructs the post-primary-strike/pre-settling candidate state explicitly;
it never selects a target from the final snapshot.

### 7.6 Presentation manifest and previews

The versioned presentation manifest supplies localized keys, icons, animation/effect IDs, ability
names, concise rules, passive descriptions, and cosmetic compatibility. It is generated or validated
against Rust content IDs during the build. It never defines damage, range, gauge cost, or legality.

Exact Backlash cost and legal target sets come from a read-only Rust preview query. A preview:

- Operates on a clone/read-only view.
- Does not mutate tick, state, command ledger, or RNG.
- Returns only information the selected mode's official UI is allowed to show.
- Is available locally for training. The initial remote client does not run the core for prediction;
  a future server endpoint/event supplies any remote guide.

ABI schema version 1 has one closed preview request:

```text
AbilityPreviewRequest
  schemaVersion: 1
  expectedSnapshotGeneration
  playerId
  kind: "ability"
  slot: "basic" | "basicAlt" | "special"
  angleMillidegrees
  powerBasisPoints
  targetPlayerId: string | null
  secondaryTargetPlayerId: string | null

AbilityPreviewResponse
  schemaVersion: 1
  snapshotGeneration
  legal
  rejectionReason: PreviewRejection | null
  gaugeCost
  legalTargetPlayerIds[]
  projectileTraces[]

PreviewRejection =
  { kind: "snapshotGenerationMismatch", expected: u64, actual: u64 }
  | { kind: "core", reason: CommandRejectionName }
```

The response uses the same trace/sample DTO as a transition but contains no damage roll, hidden
random result, terrain mutation, or promised final impact against moving targets. IDs are sorted;
stale generation is a normal `legal: false` response. `rejectionReason` is required even when null.
Future preview kinds require a schema change.

A modified client can calculate its own trajectory from known state. Restricting the official ranked
guide is a UX/rules promise, not an anti-cheat guarantee. Server authority prevents forged outcomes,
not external aim assistance.

---

## 8. Coarse native ABI

### 8.1 Export surface

The initial gameplay ABI is intentionally small. It is ABI version 1 and replaces the unshipped,
non-versioned scaffold: `db_sim_create`, `db_sim_destroy`, `db_sim_state_hash`, and
`db_sim_string_free` are retired rather than supported in parallel.

```c
uint32_t db_sim_abi_version(void);
uint32_t db_sim_simulation_version(void);
uint32_t db_sim_content_version(void);

int32_t db_sim_match_create(
    const uint8_t* config_json,
    size_t config_len,
    SimHandle** handle_out,
    DbOwnedBuffer* response_out);

int32_t db_sim_match_apply(
    SimHandle* handle,
    const uint8_t* command_json,
    size_t command_len,
    DbOwnedBuffer* transition_out);

int32_t db_sim_match_snapshot(
    const SimHandle* handle,
    DbOwnedBuffer* snapshot_out);

int32_t db_sim_match_terrain(
    const SimHandle* handle,
    uint64_t known_generation,
    uint32_t* width_out,
    uint32_t* height_out,
    uint64_t* generation_out,
    DbOwnedBuffer* cells_out);

int32_t db_sim_match_preview(
    const SimHandle* handle,
    const uint8_t* request_json,
    size_t request_len,
    DbOwnedBuffer* preview_out);

void db_sim_match_destroy(SimHandle* handle);
void db_sim_buffer_free(DbOwnedBuffer* buffer);
```

`DbOwnedBuffer` is the only returned allocation:

```c
typedef struct {
    uint8_t* ptr;
    size_t len;
} DbOwnedBuffer;
```

Rust allocates it and Rust frees it. C# copies/parses it inside a `try/finally` and calls
`db_sim_buffer_free` exactly once. A zero-length buffer has `ptr == NULL`; free tolerates that form.
Every non-empty buffer is allocated as an exact Rust `Box<[u8]>`. Free reconstructs that boxed slice
from the original pointer and length, drops it, and then writes `{ NULL, 0 }` back to the caller's
struct. A `Vec` allocation with hidden spare capacity does not cross this two-field ABI.

`db_sim_match_create` owns a real `MatchSessionHost`, not a seed placeholder. Its exact response is:

```text
MatchCreateResponse =
  { schemaVersion: 1, created: true, diagnostic: null, snapshot: MatchSnapshot }
  | { schemaVersion: 1, created: false,
      diagnostic: { code: "invalidConfig", message: string }, snapshot: null }
```

All four fields are required in both variants. A domain-level invalid config is an `OK` ABI call with
a null handle and the failure variant; malformed bytes or an ABI fault use the negative statuses
below.

### 8.2 Status codes

ABI status and gameplay disposition are separate:

| Code | Meaning |
|---:|---|
| `0` | ABI call completed; inspect the response envelope |
| `-1` | required null pointer |
| `-2` | invalid UTF-8 or malformed envelope |
| `-3` | unsupported envelope schema/version |
| `-4` | caught panic or terminal internal/session fault; the handle is poisoned and the match must be abandoned |
| `-5` | response exceeded the documented cap |

An out-of-range aim, stale turn, or invalid target is a successful ABI call containing a rejected
`MatchTransition`; it is not a negative ABI status.

### 8.3 Terrain reads

`db_sim_match_terrain` returns raw row-major material bytes only when the generation differs from
`known_generation`. When unchanged it returns `OK`, the current dimensions/generation, and an empty
buffer. The byte count must equal `width * height` under checked arithmetic.

The transition supplies dirty rectangles for incremental rendering. A reconnect/full snapshot may
discard them and rebuild all chunks.

### 8.4 Safety, atomicity, and limits

- Every fallible export catches unwinding panics. The native FFI release profile must use
  `panic = "unwind"`; an aborting shipped profile does not satisfy this promise.
- Every exported function validates nulls and lengths before dereference.
- Arbitrary non-null pointers remain outside the C contract. Every pointer must be aligned and valid
  for its documented access for the full call. Output slots are pairwise non-overlapping, do not
  overlap input/handle storage, and own no live `DbOwnedBuffer` allocation when passed because Rust
  initializes them by assignment. C# satisfies this with distinct zeroed locals and `SafeHandle`;
  the Rust server never accepts handles from the network.
- No call mutates a match unless its complete response can be produced within the response cap.
  `apply` resolves against a working clone, serializes the bounded transition, and commits that
  working state only after serialization succeeds.
- Maximum input envelope: 256 KiB. Maximum create/transition/snapshot/preview response: 8 MiB.
  Maximum terrain bytes and map dimensions are validated by Rust content rules before match creation.
- JSON nesting depth is at most 12. `matchId`, `commandId`, and `playerId` are 1–64 ASCII bytes from
  `[A-Za-z0-9._:-]`; display names are separate localized/user-content fields. Definition IDs are
  1–64 lowercase ASCII bytes from `[a-z0-9-]`. Appearance IDs are 1–128 ASCII bytes from
  `[A-Za-z0-9._-]`. Parsers enforce schema collection counts and known fields before allocation;
  unknown command fields are rejected.
- Native calls for one handle are serialized. Destroy waits for any in-flight call through
  `SafeHandle` marshalling and the local executor.
- `SimHandle` contains a poison flag. The panic guard sets it before returning `INTERNAL_PANIC`;
  once required output slots and the live handle pointer validate, every later operation on that
  handle returns `INTERNAL_PANIC` without entering `MatchHost`, and only destroy remains permitted.
  Apply/preview check poison before request-pointer, length, UTF-8/JSON, or version validation so a
  malformed follow-up cannot mask the terminal session state.
- `db_sim_match_destroy(NULL)` and freeing an empty buffer are no-ops. Double destroy remains a
  caller bug prevented by `SafeHandle`; a freed `DbOwnedBuffer` is zeroed before wrapper disposal can
  repeat.
- Arbitrary non-null and already-destroyed pointers are outside the C ABI contract and are never
  dereferenced by correct C#. The native negative-path suite tests null and live handles; stale raw
  pointer injection is not claimed safe unless a future registry/generation-handle ADR replaces
  opaque pointers.

### 8.5 C# binding rules

- Prefer source-generated `LibraryImport` with UTF-8 byte spans/pointers; do not use implicit ANSI
  string marshalling.
- Native methods receive `MatchSafeHandle` directly so .NET holds a dangerous reference for the
  duration of each call.
- `LocalMatchSession` is `IAsyncDisposable`; disposal is idempotent.
- Native response decoding validates schema and size before allocating unbounded collections.
- C# zero-initializes every `DbOwnedBuffer`; `db_sim_buffer_free` clears its pointer and length after
  freeing so wrapper-level repeated disposal is harmless.
- No DTO contains `Godot.Vector2`, `Godot.Color`, `Node`, or another engine type.
- Godot editor hot reload does not preserve a live native match. Sessions dispose on tree exit and
  are recreated explicitly.

### 8.6 Native library resolution and packaging

Use `NativeLibrary.SetDllImportResolver` and application-owned absolute paths. Do not rely on the
working directory or the OS's broad DLL search path.

| RID | File | Development source |
|---|---|---|
| `win-x64` | `db_sim_ffi.dll` | `client/native/win-x64/` |
| `linux-x64` | `libdb_sim_ffi.so` | `client/native/linux-x64/` |
| `osx-x64` | `libdb_sim_ffi.dylib` | `client/native/osx-x64/` |
| `osx-arm64` | `libdb_sim_ffi.dylib` | `client/native/osx-arm64/` |

The export process copies the correct artifact beside the executable or into the platform's required
bundle location. macOS libraries participate in code signing. CI launches each export from a clean
temporary directory so an accidental source-tree search cannot make a broken package pass.

---

## 9. Match-session interface

The exact C# names may change, but the semantics may not:

```csharp
public interface IMatchSession : IAsyncDisposable
{
    MatchSessionCapabilities Capabilities { get; }
    MatchSnapshot CurrentSnapshot { get; }

    ValueTask<MatchTransition> SubmitAsync(
        MatchCommand command,
        CancellationToken cancellationToken);

    ValueTask<MatchPreview> PreviewAsync(
        PreviewRequest request,
        CancellationToken cancellationToken);

    IAsyncEnumerable<MatchSessionEvent> ReadEventsAsync(
        CancellationToken cancellationToken);
}
```

Rules:

- All submissions are asynchronous to scenes, even when local execution completes synchronously.
- At most one gameplay command is in flight from one local user.
- Cancellation stops waiting/presentation; it does not claim that a command already delivered to an
  authority was rolled back.
- `CurrentSnapshot` changes only after an accepted transition or installed reconnect snapshot.
- Session events carry remote-player/authority transitions, connection state, or replacement
  snapshots. They do not expose native pointers.

### 9.1 `LocalMatchSession`

- Owns one `MatchSafeHandle`, local clock, and optional Rust bot coordinator.
- Converts commands and native envelopes without introducing rules.
- Locks submission during transition playback according to the deterministic input-open boundary.
- Calls authority-only timeout itself when its planning deadline expires.

### 9.2 `RemoteMatchSession` — future

- Uses WSS and a versioned Rust-server protocol.
- Does not create a local simulation handle for prediction.
- Renders only server transitions and snapshots.
- Handles duplicate/reordered messages by server sequence and snapshot generation.
- On reconnect, installs one atomic snapshot, discards stale queued playback, and optionally plays a
  bounded server-supplied recent transition. It never simulates missing turns.
- If playback falls behind an authoritative input window, it shortens cosmetic holds or skips
  nonessential particles; it never changes event order or hides the resulting state.

Prediction may be proposed later only with measured same-region latency, a reconciliation design,
and an ADR. `IMatchSession` is the seam; speculative state is not prebuilt now.

---

## 10. Coordinates, ticks, and quantization

| Space | Unit | Authority |
|---|---|---|
| Simulation | fixed-point integer | Rust |
| Terrain | whole cells, row-major top-left origin | Rust |
| Presentation | Godot floating-point pixels | C# view only |

The snapshot/bootstrap contract exposes `positionScale` and `fixedTickRate`; C# does not duplicate
them as unexplained constants. Current values are `1024` fixed units per cell and `60` simulation
ticks per second.

Y is positive downward in simulation and screen space. Angles are millidegrees measured
counter-clockwise from world `+X`, exactly as the Rust contract defines. Power is basis points
`0..=10000`.

```csharp
public static Vector2 SimToScreen(
    int x,
    int y,
    int positionScale,
    float pixelsPerCell) => new(
        x / (float)positionScale * pixelsPerCell,
        y / (float)positionScale * pixelsPerCell);
```

Rules:

- Screen coordinates never cross the match boundary.
- C# may use floats for cursor movement and animation, then quantizes angle/power once with an
  explicitly tested rounding mode before constructing a command.
- Rust still range-checks; client clamping is usability, not security.
- Projectile playback interpolates only between authoritative samples. It never extrapolates beyond
  the terminal sample.
- Character/object interpolation ends exactly at the authoritative position. A new transition or
  snapshot cancels stale tweens.
- `pixelsPerCell` is art-direction configuration, not gameplay state.

---

## 11. Application flow and scene ownership

Initial flow:

```text
Boot → MainMenu → LocalSetup → CharacterSelect → Match → Results
                                                ↑         │
                                                └─rematch─┘
```

`Boot` verifies tool/runtime prerequisites, native library versions, presentation manifest, settings,
and required assets before enabling local play. A failure transitions to a recoverable/fatal problem
screen with a copyable diagnostic ID.

Suggested scene responsibilities:

```text
Main.tscn                    owns application navigation only
MainMenu.tscn                chooses local play/settings/quit
LocalSetup.tscn              map, mode, human/bot slots
CharacterSelect.tscn         character and cosmetic selection
Match.tscn                   composes controller, world, camera, HUD
Results.tscn                 authoritative result and rematch

Match/
  MatchController.cs         IMatchSession + playback coordination
  PlaybackCoordinator.cs     ordered PresentationEvent playback
  TerrainRenderer.cs         material mask chunks
  BlockRenderer.cs           block identity/health presentation
  CharacterView.cs           paper-doll layers and authoritative motion
  PersistentObjectView.cs
  ProjectilePlayer.cs
  EffectPlayer.cs
  CameraRig.cs
```

Scenes receive dependencies from the application composition root. They do not find native/session
singletons by string path.

---

## 12. Input contract

### 12.1 Contexts

Semantic actions are enabled by context so one physical control never triggers two match commands:

- `MenuNavigation`
- `MatchFreeCamera`
- `MatchMovementAim`
- `MatchTargetSelection`
- `PassiveSelection`
- `PlaybackLocked`
- `Results`

Only `MatchMovementAim` and `MatchTargetSelection` can build gameplay commands.

### 12.2 Default bindings

| Action | Keyboard | Mouse | Gamepad |
|---|---|---|---|
| Move left/right | A / D | optional held HUD buttons | Left stick X |
| Jump | W | HUD button | X |
| Aim | Up / Down | drag aim handle | Right stick |
| Charge/commit | hold/release Space | hold/release primary | RT |
| Fine aim | Shift | wheel | LT |
| Basic / Basic Alt / Special | 1 / 2 / 3 | ability bar | D-pad left / down / right |
| Pass | hold P | hold HUD button | hold D-pad up |
| Pan camera | Ctrl + arrows | middle drag | LB + right stick |
| Reset camera | R / Home | HUD button | right-stick click |
| Focus active character | F | HUD button | RB |
| Confirm | Enter | primary | A |
| Cancel | Escape | secondary | B |
| Scoreboard | hold Tab | HUD button | hold View |

Bindings are fully remappable. The settings UI detects conflicts within a context and requires the
player to resolve them; it may allow the same physical input in mutually exclusive contexts.

### 12.3 Movement

- Held movement emits at most one outstanding command and repeats at a capped 20 Hz.
- The default request quantum is one quarter cell. Analog magnitude changes repeat intent only after
  a deadzone; it does not grant a larger movement allowance.
- The authoritative returned distance and position drive presentation.
- Jump remains disabled until Rust tracks and enforces its action budget. The existence of a jump
  helper alone is not a release-ready rule.

### 12.4 Aim, power, and targeting

- Keyboard/controller aim uses fixed-rate quantized steps; fine aim uses the smaller documented step.
- Pointer aim computes a direction around the authoritative muzzle presentation socket, then sends
  only the quantized angle.
- Charge is uncommitted local state. Release constructs one ability command after required target
  selection. Cancel clears it without a native call.
- Targeted abilities enter `MatchTargetSelection`, highlight only the authoritative legal-target
  list, and require explicit confirmation. Stick/directional focus selects spatially and LB/RB cycle
  the bounded legal-target list in that context. No nearest-target guess is committed silently.
- UI displays the exact quantized angle/power that will be sent.
- Tuning constants such as repeat rate, aim step, fine step, and charge duration live in one reviewed
  presentation configuration and have input tests.

No hover-only, right-click-only, or mouse-precision-only action is permitted.

---

## 13. Rendering contract

### 13.1 Terrain

The terrain mask is an authoritative material-index texture, not final art. Render it through a
material/palette shader or use it to clip approved terrain art.

- Store the CPU mask in one row-major byte array.
- Divide large maps into 128×128-cell render chunks initially; benchmark may change the chunk size.
- A full snapshot rebuilds all chunks.
- `terrainChanged` marks authoritative dirty cell rectangles; update only intersecting chunks.
- Use nearest-neighbour sampling for the collision/readability edge. Decorative overlays may filter
  independently but cannot hide the solid/empty boundary.
- Validate every returned byte against known material values before upload.
- Never infer terrain from crater VFX.

The old `ImageTexture.Update(fullImage)` example is not an incremental algorithm and is not the
implementation target.

### 13.2 Destructible blocks

- Render solidity from the mask and identity/health from block snapshots/events.
- Never infer block health from occupied cells.
- Animate erosion toward the authoritative final mask, including row-axis erosion.
- A block health bar/tint and damage indication identify the addressable block.
- Dirty rectangles come from Rust because health-directed erosion can change cells outside a simple
  crater bounding box.

### 13.3 Characters

One view per player uses the shared paper-doll order:

```text
back accessory → hair back → rear arm → body → outfit → face
→ hair front → headwear → front arm → held ability skin
→ front accessory → combat effect
```

Required animation tags are `idle`, `walk`, `aim`, `charge`, `fire`, `melee`, `hit`, `fall`,
`victory`, and `defeat`. Frames expose a ground pivot plus `mainHand`, `offHand`, `muzzle`, and
`effectOrigin` sockets where applicable.

Cosmetics never change position, collision, reach, launch origin, timing, or visibility of the used
ability. A missing/incompatible layer falls back to a validated default rather than shifting a
socket.

### 13.4 Projectiles and events

- Create one player per `projectileTrace` ID.
- Interpolate by transition presentation tick between that trace's samples.
- Do not extrapolate beyond its last sample or merge samples from different traces.
- Play impact, terrain, damage, and elimination events at their ordered event tick.
- If rendering falls behind, drop cosmetic particles before skipping authoritative event
  presentation. The final snapshot is always installed.

### 13.5 Persistent objects and effects

Persistent objects have their own views keyed by authoritative sequence. Spawn/change/remove only
from events or reconciliation snapshots.

Effects are data-driven presentation definitions. Device-tier degradation reduces particle count,
debris lifetime, post-processing, and shake in that order. It never removes target markers, status
icons, terrain edges, damage breakdowns, or other tactical information.

---

## 14. HUD and match feedback

During an input window the HUD exposes:

- Active player and team, with icon/shape as well as color.
- Planning time remaining from the session authority.
- Current and upcoming turn order.
- Wind direction and numeric magnitude.
- Quantized angle and power.
- Selected Basic/Basic Alt/Special name, icon, availability, and concise rule.
- Special gauge as numeric percent and progress graphic.
- Movement allowance remaining.
- Health/max health and statuses for every player.
- Legal target and target-selection state when applicable.
- Exact predicted Backlash cost before commitment.
- Camera reset and focus-active-character controls.
- Connection/reconnect state online.

There is no ammunition or shield field unless a future accepted character/mode contract adds one.

### 14.1 Passive prompt

After playback reaches a post-snapshot whose stable phase is `PassiveSelection`, show the three
allowed character-specific passives from the authoritative event/manifest. Disable every other
gameplay action. It is not dismissible or skippable.

Local play does not let the planning timer expire behind this modal. Before online implementation,
the server specification must define a separate passive deadline and deterministic disconnect/
timeout choice; the client does not invent one.

### 14.2 Results and damage explanation

Show authoritative direct, splash, Backlash, hazard, wall-impact, healing, critical, knockback, and
elimination-cause information. Random rolls that materially affected the action are explained when
the transition exposes them. Never reduce an action to an unexplained final health number.

Results use the terminal snapshot and `matchCompleted` event. Rematch constructs a new config/seed
and handle; it never rewinds or reuses a completed host.

---

## 15. Camera and audio

Camera rules:

- Follow the active character by default during input.
- Frame each authoritative projectile trace during playback; widen rather than chase so fast that
  the projectile leaves frame.
- Allow manual pan and an explicit reset/focus action.
- Minimum zoom can frame the whole playable map.
- Show the cause of an unrecoverable fall/elimination.
- Reduced motion disables shake and aggressive zoom/pan without changing event timing.

Audio buses: master, music, effects, voice, and UI. Important cues—including timer warning, gauge
ready, impact material, rejection, and elimination—have synchronized visual equivalents. Voice is
optional and separately mutable.

---

## 16. Accessibility

These are release gates, not post-launch polish:

- No tactical information conveyed by color alone.
- Wind, angle, power, gauge, health, status duration, and timer have text/numeric forms.
- HUD and menus pass the selected contrast standard under supported UI scales.
- Reduced motion removes shake, limits flashes, and replaces rapid motion while preserving action
  order and planning time.
- Full keyboard focus navigation.
- Controller-only navigation through boot, setup, character selection, match, passive prompt,
  results, rematch, settings, and quit.
- Remappable controls with conflict detection, deadzone configuration, and glyph switching.
- Scalable UI text and safe-area support.
- Important effects audio has captions or visual equivalents.
- No hover-only information.

Automated checks cover focus reachability, missing accessible labels, and minimum UI-scale layouts;
manual testing covers readability and motion settings.

---

## 17. Settings, saves, and localization

Device settings live under Godot `user://` and are separate from future account state. Use a
versioned schema, bounded values, atomic temp-write/replace, and a last-known-good/default fallback
for corruption. Never put tokens or server credentials in the settings file.

Settings groups:

- Graphics: display mode, resolution, vsync, frame cap, particle density, UI scale.
- Accessibility: reduced motion, shake, flash reduction, high-clarity effects, captions.
- Audio: master, music, effects, voice, UI.
- Gameplay presentation: guide level where mode permits it, timer warning, camera follow, damage
  numbers.
- Controls: bindings, deadzones, sensitivity/rate, glyph family.

Gameplay-affecting mode policy is not a device setting. A local preference cannot enable a full
guide where an online mode disallows it.

All player-facing strings use localization keys from the start. Logs and stable wire IDs are not
localized. Layout tests include long-string expansion and supported UI scales.

---

## 18. Fault handling and diagnostics

| Condition | Client behavior |
|---|---|
| Native library missing/wrong architecture | Fatal boot repair screen with expected path/RID |
| ABI/simulation/content mismatch | Fatal local-play error; do not create a handle |
| Ordinary command rejection | Inline contextual feedback; keep the input window if authority does |
| Caught native panic | Dispose session, mark match faulted, never continue that state |
| Invalid native envelope | Treat as boundary fault, include schema/version in diagnostic |
| State-hash mismatch | Stop submission; local fails closed, online requests one authoritative resync then fails if repeated |
| Asset missing | Validated fallback if cosmetic; fatal content mismatch if tactical readability is affected |
| Settings corruption | Preserve bad file for diagnosis, load safe defaults, notify once |
| Network loss | Lock submission, show reconnect state, install only an atomic server snapshot |

Logs are structured and privacy-minimized. Include build, ABI/simulation/content/protocol versions,
match-local opaque ID, turn, command ID, status/rejection category, and diagnostic ID. Exclude tokens,
emails, provider IDs, chat content, and full IP addresses.

The UI offers a copyable diagnostic summary without exposing native pointers or secrets.

---

## 19. Performance contract

Measure release exports after a 30-second warm-up over at least 60 seconds. Report median and p95,
the machine profile, resolution, map fixture, particle tier, build commit, and Godot/Rust/.NET
versions. A number without that context is not a gate result.

Standard stress fixture:

- 512×256 terrain cells.
- Eight player views.
- 128 persistent-object views.
- A transition with eight independently sampled projectile traces.
- Dirty terrain updates spanning four chunks.
- Up to 2,000 cosmetic particles at the high tier.

| Area | Goal | Floor/failure threshold |
|---|---:|---:|
| Rendering | 60 fps | stable 30 fps |
| Frame time p95 while aiming | <16.7 ms | <33.3 ms |
| Repeated main-thread stalls | none >50 ms | any repeated stall fails |
| Normal dirty terrain update | <4 ms | <16.7 ms |
| Standard local shot resolution | <50 ms | profile before continuing |
| Transition JSON decode | <4 ms normal | <16.7 ms |
| Warm start to interactive menu | <2 s | <4 s |
| Process working set after warm-up | <350 MB | <500 MB |

Device tier is selected by measured frame-time history, not GPU-name strings. Degradation never
changes authoritative simulation or tactical readability.

---

## 20. Testing and release gates

### 20.1 Rust transition tests

- `postStateHash` equals the actual host hash after every host-owned mutation.
- Normal projectile, multi-strike, strike/melee, passive interruption, status tick, persistent
  object lifecycle, terrain/block change, elimination, pass, timeout, and victory each produce a
  representable ordered transition.
- Every projectile retains its own samples and impact.
- Feeding Frenzy followed by Carrion Call produces three `forced` crit records, consumes no crit RNG
  draws, and records two charge consumptions plus exhaustion; ordinary crits retain independent draws.
- Status transitions and ordered object lifecycles replay exactly from the pre-snapshot to the
  post-snapshot. Missing, duplicate, unknown, stale, or mismatched records fault before publication;
  a spawn-and-remove lifecycle invisible to snapshot diff remains representable.
- Status durations tick only on the affected player's own completed turns. Count-based
  `GuaranteeCrit` is consumed by strikes rather than the duration clock.
- Ordinary health-zero/fall elimination removes all owned objects once with `ownerEliminated`.
- An actor eliminated during settling cannot enter `PassiveSelection`; pass and timeout cannot bypass
  an owed passive choice.
- Rejection and duplicate replay have exact non-mutation/idempotency assertions.
- Dirty rectangles cover every changed terrain cell.

### 20.2 FFI tests

- Real config creates and snapshots a real `MatchHost`.
- Nulls, malformed UTF-8/JSON, unknown fields/enums, invalid lengths, oversized counts, poisoned live
  handles, and output caps fail without mutation or memory violation.
- Every status path returns initialized outputs.
- Buffer ownership is exercised on success and failure.
- A deliberate panic inside the common FFI guard is caught by a release-profile test. No test-only
  panic export is present in the shipped dynamic library.
- Repeated create/apply/snapshot/terrain/destroy loops are leak-checked.

### 20.3 Shared golden fixtures

Move client-consumable inputs and expected hashes into machine-readable fixtures under
`tests/fixtures/matches/`. Both direct Rust and C#→FFI tests consume the same bytes. Fixtures include
fixed command IDs and meaningful outcome assertions so a stable no-op cannot pass.

Parsing Rust test source from C# is prohibited.

### 20.4 Headless .NET tests

- Contract serialization and enum exhaustiveness.
- `SafeHandle` disposal under normal, exception, cancellation, and forced-GC paths.
- Native resolver selects only the expected RID directory.
- A scripted match through `LocalMatchSession` matches the direct Rust fixture hash.
- Interop assemblies load and test with no Godot dependency.
- Input context conflict and command-construction tests.

### 20.5 Godot/export smoke tests

The minimum smoke scenario:

1. Launch an export from outside the repository.
2. Reach the menu with correct build/version diagnostics.
3. Start the real horizontal-test duel.
4. Render terrain, blocks, and two placeholder characters from a snapshot.
5. Move, fire one real shot, play its transition, and reconcile to its post-snapshot.
6. Exit and verify clean native-handle disposal.

Run it first on Windows, then on every claimed target. Headless scene tests cover navigation and
input maps; a real rendered run remains required for graphics and packaging.

### 20.6 CI/build commands

The eventual pinned scripts wrap commands equivalent to:

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --release -p db-sim-ffi --locked
cargo build --release -p db-sim-ffi --locked

dotnet restore client/DungeonBarrage.sln --locked-mode
dotnet build client/DungeonBarrage.sln -c Release --no-restore
dotnet test client/DungeonBarrage.sln -c Release --no-build
dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore

godot --headless --path client/src/DungeonBarrage.Client --editor --quit-after 1
godot --headless --path client/src/DungeonBarrage.Client \
  --export-release "Windows Desktop" <artifact-path>
```

Use the pinned Godot 4.7.1 .NET executable; `godot` above is shorthand. CI adds an OS matrix when the
corresponding export becomes a claimed target. A C# job is added with C3, when it has a real contract
test to run.

Dependencies are locked, advisory-scanned, and license-reviewed. Release artifacts include version
metadata and checksums; signing/notarization is required before public distribution.

---

## 21. Ordered implementation milestones

### C0 — Decisions and reproducible toolchain

ADR 0006, this v2 specification, `global.json`, `rust-toolchain.toml`, governing-plan status
reconciliation, and `scripts/verify-toolchain.ps1` landed in the initial slice. On 2026-08-25 the
pinned Godot 4.7.1 .NET editor and matching `4.7.1.stable.mono` export templates were installed and
verified alongside .NET SDK 10.0.302 and Rust/Cargo 1.94.0. The validator checks the editor, template
version file, and Windows x86_64 debug/release template binaries. This local C0 gate is complete.
Per §20.6, the first .NET CI job arrives with real C3 tests and target-specific Godot/export jobs
arrive when C4 claims those targets; a job that exercises no client is not a C0 acceptance signal.

**Gate:** the pinned verifier reports the exact Godot/.NET/Rust versions on the development machine,
and no current-status or governing document assigns authoritative gameplay or the future server to
C#. Superseded ADRs remain unchanged as historical records.

### C1 — Rust transition contract — complete 2026-08-26

Delivered: transport-free `MatchConfig`, engine-neutral snapshots, multi-projectile traces,
post-host turn/hash semantics, correct gauge deltas, the normalized command union, canonical command
identity, generation-owning/idempotent `MatchSessionHost`, accepted/rejected duplicate replay,
atomic post-snapshot/hash transitions, deterministic net-diff events, exact terrain dirty row-runs,
bounded canonical ledger-byte accounting, authority-only timeout, exact strike/crit and status
provenance, detached ordered trace/strike replay (including miss traces, crit/damage order, and the
exact eliminating strike), ordered persistent-object lifecycle causes with exact replay reconciliation,
movement-fall elimination progression, and the version-6 lifecycle corrections, plus a strict
shared raw-request fixture bundle whose direct Rust replay freezes meaningful semantic expectations
and exact hashes; producer-owned Arzum/Aleph random outcomes; read-only preview; opaque verified
host-plus-ledger checkpoint restore; and direct passive, pass, timeout, terrain/block, strike-mutation,
and elimination/victory scenarios. Persistent-object expiry and destruction still need real
authoritative mechanics before their reserved causes can be emitted.

**Gate:** §20.1 passes, including a real multi-strike and
`transition.postStateHash == hash_state(host.state())`.

### C2 — Real coarse FFI — complete 2026-08-26

The placeholder was replaced with the Rust session host. C2 implements
create/apply/snapshot/terrain/preview, exact boxed-slice owned buffers, ABI versioning, handle
poisoning, strict closed DTOs, depth/count/byte limits, clone-serialize-commit atomicity, and the full
negative-path suite. Production response bytes are frozen in the shared fixture bundle. CI runs the
release guard, checks the exact export set, and Valgrind-checks repeated ownership cycles.

**Gate:** the horizontal-test duel executes through the C ABI with the same transitions and final
hash as the direct Rust fixture; leak and panic gates pass.

### C3 — Headless .NET session

Create contracts, interop, `SafeHandle`, native resolver, `LocalMatchSession`, and xUnit fixture
replay. Do not create Godot scenes yet.

**Gate:** the same machine-readable scripted match passes direct Rust and C#→FFI, with no Godot
assembly loaded.

### C4 — Minimal Godot render/export spike

Pin Godot 4.7.1 .NET in the project; render one authoritative snapshot with placeholder assets and
export it on Windows.

**Gate:** §20.5 steps 1–4 and 6 pass from a clean export directory.

### C5 — One playable authoritative turn

Add input contexts, movement, aim/charge, target selection needed by the fixture, transition
playback, terrain dirty updates, HUD essentials, and reconciliation.

**Gate:** a human moves and fires one complete turn without a debugger; input is locked during
playback; every view ends at the post-snapshot; direct and C# hashes still match.

### C6 — Complete local match

Add all nine starter kits, passive prompt, Rust bot, local clock/timeout, victory/results/rematch,
objects, statuses, camera, and full HUD.

**Gate:** a first-time player selects a character, completes and understands a bot match, and
rematches without developer explanation. Controller-only play completes the whole flow.

### C7 — Desktop release quality

Add validated art/audio manifests, accessibility, settings recovery, localization infrastructure,
performance tiers, Windows signing, Linux/macOS exports, and tester distribution.

**Gate:** accessibility, stress, clean-package, and target-platform gates pass with recorded
evidence.

### C8 — Authoritative online adapter

After the Rust server/protocol exists, add `RemoteMatchSession`, private rooms, clock offset,
reconnect snapshots, chat/mute/report UI, and spectator playback. Do not add prediction.

**Gate:** duplicate, late, reordered, malformed, cross-player, and version-mismatched commands cannot
alter server state; reconnect during planning and playback reaches the same snapshot/hash as an
uninterrupted observer.

---

## 22. Decisions still open

These do not block C1–C3 unless stated:

1. **Art direction:** pixel art or high-resolution illustration. This determines final
   `pixelsPerCell`, filtering beyond the authoritative mask edge, atlas budgets, and animation
   authoring. Placeholder shapes are mandatory until decided.
2. **Presentation timing:** spacing between simultaneous/multi-strike traces and maximum cosmetic
   holds. Rust must provide ordering; presentation durations need playtesting.
3. **Camera during another player's input:** follow the active actor by default, but the exact
   free-look policy is a UX decision.
4. **Launch architecture breadth:** Windows x64 is first; the date at which macOS/Linux become
   release-blocking depends on distribution planning, but no untested platform may be advertised.
5. **Numa Pin balance after corrected duration semantics:** the engine now interprets two turns as
   two turns of the affected player, independent of player count. Confirm whether the numeric value
   remains two during balance playtesting; do not restore global-action ticking.

Settled here and no longer open: Godot/C# versus an all-Rust client, bot location (Rust), local versus
server timer ownership, initial online prediction (none), and future server language (Rust).

---

## 23. Failure patterns to guard permanently

- Do not poll transient `MatchPhase` values as an animation timeline.
- Do not call a pre-settle command hash the final host hash.
- Do not merge multiple projectile traces or retain only the last impact.
- Do not infer block health from the mask or dirty cells from crater geometry alone.
- Do not send floats, screen positions, client timer values, hits, damage, terrain, or outcomes to an
  authority.
- Do not let a view call native exports or own a raw handle.
- Do not promise `catch_unwind` while shipping the FFI library with `panic = "abort"`.
- Do not use a local core for initial online prediction.
- Do not describe an official trajectory-guide restriction as anti-cheat enforcement.
- Do not add a green CI job until it executes a real fixture through the layer it claims to test.
- Check what calls a subsystem and what observable event it produces; correct, tested, unreachable
  code is still non-functional.
