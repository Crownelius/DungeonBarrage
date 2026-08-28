# Dungeon Barrage operational handoff

**Checkpoint date:** 2026-08-27

**Audience:** the next implementation agent, especially Claude Opus

**State:** C0 through C7 are complete — authority-only turn timeout, the coarse C ABI, the
headless .NET interop/session layer, the Godot render/export spike, playable turn execution
(move, aim/fire, input lock, HUD, reconciliation), full 9-starter roster selection with automated
bot opponent turn execution, passive selection modal, terminal match results screen, rematch system,
audio settings recovery, accessibility text scaling, localization catalog, performance tiers,
cross-platform export presets (Windows Desktop, Linux/X11, macOS), and automated CLI verification suites.
All local Rust, release, supply-chain, toolchain, fixture-byte, export-surface, Valgrind, and secret-scan
gates pass. The commit containing this file is the landing checkpoint; verify its exact ID and upstream
state rather than trusting a chat summary.

This is the mutable resume document. `docs/CLIENT_SPEC.md` is the normative client contract,
`docs/BUILD_LOG.md` is append-only evidence, and accepted ADRs retain the architectural history.

---

## 1. Start here

```powershell
Set-Location -LiteralPath 'C:\Users\rsfit\DungeonBarrage'
git status --short --branch
git rev-parse HEAD
git log -5 --oneline --decorate
git diff --check
```

Canonical repository: `C:\Users\rsfit\DungeonBarrage`.

`C:\Users\rsfit\OneDrive\Documents\DungeonBarrage` is a different, effectively empty repository
that can appear in desktop context. Do not copy or merge work into it.

Landing branch: `feat/c1-outcome-provenance`, tracking
`origin/feat/c1-outcome-provenance`.

The reviewed predecessor was `0d60938`. The final checkpoint is
the newer commit containing this handoff; obtain it with `git rev-parse HEAD`.

Do not run `git reset --hard`, `git checkout --`, `git clean`, broad staging, or a bulk line-ending
rewrite. Do not read, print, stage, or touch the ignored `.github-token`. A credential was pasted in
chat and must be revoked/rotated by the owner. Push only with plain `git push` through already
configured Git credentials.

---

## 2. Reading order and authority

Read in this order:

1. `docs/HANDOFF.md` — current operational state and next work.
2. `docs/CLIENT_SPEC.md` — normative C0–C7 contract and gates.
3. `docs/adr/0006-client-and-server-language-boundaries.md` — accepted language boundary.
4. `docs/MODULE_OWNERSHIP.md` — shared-tree ownership.
5. `todolist.md` — mechanics gaps and active P13/P14 work.
6. `docs/SECURITY_BASELINE.md` — trust boundary and CI requirements.
7. `docs/BUILD_LOG.md` — append-only evidence and corrections.

If documents conflict, the newest accepted ADR wins, then `CLIENT_SPEC.md` for client/contract work.
Historical plans and accepted ADR context may truthfully describe the repository as it was when the
decision was made; do not rewrite them to make history look current.

---

## 3. Settled language and engine decision

Do not reopen this without new evidence:

- Godot 4.7.1 .NET plus C# owns presentation: scenes, UI, input, accessibility, platform
  integration, and content iteration.
- Rust `db-sim-core` is the only authoritative gameplay implementation.
- Local C# calls Rust through the coarse, client-only C ABI in `db-sim-ffi`.
- A future authoritative server is Rust-native and links `db-sim-core` directly.
- The web client is retired. `db-sim-wasm` is dormant, not a second rules implementation.

C# remains the best fit for Godot presentation. Rust remains the best fit for deterministic
authority and the future server. A Rust-only GDExtension client would add editor and binding friction
without removing the presentation engine; moving rules into C# would create a second authority.

Installed and verified on this machine:

- Rust/Cargo 1.94.0.
- .NET SDK 10.0.302.
- Godot 4.7.1 .NET editor.
- Matching `4.7.1.stable.mono` export templates, including Windows x86_64 debug/release binaries.
- Ubuntu/WSL2 Valgrind 3.26.0 for the native ownership gate.

`scripts/verify-toolchain.ps1` is the source of truth for the first four entries.

---

## 4. C1 is complete — Rust transition contract

### Session, replay, and restore

- Validated real-map/real-roster `MatchConfig` creation.
- Detached engine-neutral `MatchSnapshot` with exact authoritative state hash.
- Closed normalized `MatchCommand`, canonical semantic digest, and publication generation.
- Clone-before-apply atomicity; rejected or faulted work cannot publish partial state.
- First-result ledger for accepted and rejected receipts, exact duplicate replay, and
  same-ID/different-content conflict rejection.
- Exact 16,384-entry and 64 MiB deterministic retained-byte limits.
- Separate authority-only timeout path sharing the ID namespace and ledger. No client command variant
  can encode timeout.
- `MatchSessionCheckpoint` is an opaque host-plus-complete-ledger value. Restore validates declared
  length, configured limits, request IDs/digests, transition structure, generation continuity,
  processed command IDs, the current snapshot, and an exact retained-byte recomputation. Closed
  sessions cannot checkpoint.

Checkpoint restore is currently an in-process Rust API, not an on-disk serialization format. A
future server persistence adapter must serialize the whole opaque unit and integrity-protect its
container; it must never expose or reconstruct `from_host(host)` while discarding replay receipts.

### Producer-owned action provenance

- Every strike has a dense index, target, exact point, delivery, cited trace where applicable,
  `NotEligible`/`Missed`/`Landed`/`Forced` crit provenance, applied damage, and elimination flag.
- Detached exact replay checks every projectile trace (including uncited misses) and every ordered
  strike field, then independently checks cardinality, citations, melee targets/points, aggregate
  direct damage, and elimination attribution. Swapped crit/damage pairs, omitted kill flags, omitted
  traces, and other tampering fail before host, generation, or ledger publication.
- Arzum's target selection records candidate count, selected index, target, and destination at the
  draw site. Reconciliation derives the post-primary-strike/pre-settling state and does not infer the
  choice from final positions.
- Aleph's Veilstep records axis bound, accepted/final bounded X/Y results, fallback use, drawn point,
  and legal corrected destination. No private generator state is published.
- Status producers record exact `Applied`, `Refreshed`, `ChargeConsumed`, `Ticked`, `Exhausted`, and
  `Expired` transitions; replay applies them against a shadow pre-status map.
- Persistent-object producers preserve causal spawn/removal order with exact `Replaced`,
  `CapacityEvicted`, `Detonated`, and `OwnerEliminated` causes. `Expired` and `Destroyed` remain
  reserved and unreachable.
- Elimination provenance distinguishes exact strike, Backlash, splash, wall impact, ability effect,
  hazard, and conservative authoritative-resolution causes.

### Read-only preview and direct scenarios

- `MatchSessionHost::preview(&self, ...)` returns exact gauge cost, sorted legal target IDs, and
  static projectile traces.
- Stale generation is a normal `legal:false` response.
- Legality executes only on disposable clones. Tests prove no live state, RNG, generation, or ledger
  mutation, including RNG-heavy Aleph.
- Direct end-to-end scenarios cover passive required/chosen interruption, pass and next-turn order,
  authority timeout, melee terrain plus block mutation, exact strike mutation failures, and
  elimination/victory ordering with no terminal turn reopen.

### Compatibility

`SIMULATION_VERSION` remains 6 for this slice because it adds session/provenance/adapter surfaces,
not a new authoritative state transition. The version-6 hashes remain:

| Fixture/vector | Hash |
|---|---:|
| Shared fixture initial | `f67c5371bcddbdf5` |
| Shared fixture after move | `378081bb2e830a5d` |
| Shared fixture after ability/final | `d8686762470c0c36` |
| All passes | `ecff79397aa402de` |
| Walking duel | `af6978b06c1f9772` |
| Firing duel | `a009c290a796d1ba` |
| Mixed actions | `c29e2d75ceba7f33` |
| Low-health duel | `0c908bfce4b927d6` |

---

## 5. C2 is complete — real coarse C ABI

`db-sim-ffi` contains no gameplay rules. Its production serializer is the only JSON wire
implementation; `db-sim-core` remains serialization-dependency-free.

### Exact exports

ABI version 1 exports exactly:

```text
db_sim_abi_version
db_sim_simulation_version
db_sim_content_version
db_sim_match_create
db_sim_match_apply
db_sim_match_snapshot
db_sim_match_terrain
db_sim_match_preview
db_sim_match_destroy
db_sim_buffer_free
```

The retired scaffold symbols and any test-only panic symbol are absent. Windows `dumpbin /exports`
and Linux `nm -D --defined-only` both reported this exact ten-symbol `db_sim_*` surface.

**Superseded by ABI version 3** (C6, see §7d): `db_sim_match_bot_decide` (version 2) and
`db_sim_roster` (version 3) bring the export count to twelve, confirmed via a direct PE
export-table read of the release DLL each time. This is a historical record of version 1's
surface, not a claim about the current one.

### Boundary behavior

- Create owns a real `MatchSessionHost`. A domain-invalid config is ABI `OK` with `created:false`, a
  diagnostic, null handle, and no partial session.
- Snapshot is one composite envelope: schema/ABI/simulation/content versions, match/map metadata,
  session generation, complete projected state, nullable local timestamps, and state hash.
- `turnOpened` also carries required-nullable `inputOpensAt` and `deadlineAt`; the local C2 adapter
  emits null until C3's monotonic session clock decorates them, while a future server supplies time.
- Apply decodes a strict command, resolves a cloned session, serializes and bounds the complete
  transition, then commits only after every fallible step succeeds.
- Preview is read-only. Terrain returns raw row-major bytes only when its generation changed.
- Inputs reject malformed UTF-8/JSON, duplicate/unknown fields, unknown enums, non-integers, trailing
  data, missing nullable fields, depth over 12, size over 256 KiB, and player collections over four.
- Responses are bounded at 8 MiB and serialized as compact UTF-8 plus one terminal LF.
- Every non-null output is initialized before every status return, including partial-null calls.
- `DbOwnedBuffer` is an exact `Box<[u8]>`; Rust frees it and clears `{ptr,len}` first.
- The unsafe contract requires aligned, valid, non-overlapping output slots that do not overlap input
  or handle storage and do not own a still-live buffer. C3 must use distinct zeroed locals.
- Each handle has a mutex and atomic poison bit. After required output slots and the live handle
  pointer validate, a contained panic or terminal invariant makes every later operation return
  `-4`; destroy remains the only permitted call. Apply/preview check poison before request pointer,
  size, decoding, or version validation, so malformed follow-ups cannot mask terminal state.

Status codes remain `0`, `-1` through `-5` exactly as specified. Gameplay rejections remain `OK`
transitions and never masquerade as ABI faults.

### Shared byte fixtures

`tests/fixtures/matches/horizontal-test-duel-v1/` now contains:

- exact create, move, ability, and preview request bytes;
- a strict semantic manifest consumed by direct Rust;
- byte-for-byte production responses for create, initial snapshot, preview, move, and ability.

The direct core test validates the request bundle and preview before replay. The FFI test feeds the
same bytes unchanged through the real ABI, compares every response byte, and asserts the same three
state hashes. There is no test-only serializer.

### C2 negative and memory gates

The 13-test FFI suite covers versions, real fixture parity, malformed/unsupported inputs, required
nullable fields, unknown variants, oversized collections/lengths/depth, invalid domain config,
response-cap non-commit, all negative-output initialization, controlled panic poisoning across all
live operations, exact buffer ownership, and 64 complete lifecycle repetitions.

Valgrind 3.26.0 ran the release lifecycle test under WSL2:

```text
84,625 allocations; 84,623 frees; 9,450,420 bytes allocated
definitely lost: 0 bytes
indirectly lost: 0 bytes
ERROR SUMMARY: 0
```

The two remaining runtime-held blocks were 48 bytes possibly lost and 544 bytes still reachable from
the Rust test runtime, not definitely/indirectly lost ABI allocations. CI fails on definite or
indirect leaks and also enforces the release panic test and exact export list.

---

## 6. Deliberate gaps — do not paper over them

These are outside the completed C1/C2 boundary:

- ~~No C# project, `SafeHandle`, managed DTO layer, or `LocalMatchSession` exists~~ — all added in
  C3. ~~No Godot project exists~~ — added in C4, but it is a render/export spike only: a menu, a
  static placeholder render of the fixture duel, and clean disposal. There is no movement, no aim,
  no firing, and no HUD; that is C5.
- Arzum's documented 50–200% Chain Strike second-hit damage is not implemented. The current special
  performs its first strike, records/selects a target, and teleports. The owner must settle the rated
  random-damage rule before Rust gains the real second strike and another producer-owned outcome.
  See `todolist.md` P14.
- Finite object lifetimes are not decremented. Do not emit `Expired` without a scheduler-owned rule.
- Object health cannot be targeted/damaged. Do not emit `Destroyed` without a real producer.
- Turret/gas-cloud behavior beyond object creation, several passive modifiers, Embers content
  reachability, and sudden-death hazard application remain incomplete.
- Numa Pin correctly lasts two affected-player turns; whether the numeric value stays two is an owner
  balance decision.
- `EntityMoved` remains conservatively `authoritativeResolution` because the command result does not
  retain a post-walk/pre-settle subpath.
- `ObjectChanged` can describe snapshot-derived in-place mutation, but no production in-place object
  mutation exists yet.

Do not silently turn any of these into C# inference. Extend/version the Rust authority first.

---

## 7. C3 is complete — headless .NET interop and session layer

`client/` holds a Godot-free `net10.0` solution: `DungeonBarrage.Client.Contracts` (strict managed
envelope DTOs), `DungeonBarrage.Client.Interop` (`DbSimNative`, `DbSimBuffer`, `MatchSafeHandle`,
`NativeLibraryResolver`, `LocalMatchSession`), and `DungeonBarrage.Client.Interop.Tests`.

The gate is met: the frozen request files replay through the **real release** `db_sim_ffi` and match
the frozen response files **byte for byte**, ending on `d8686762470c0c36`. 25 .NET tests cover
fixture parity, disposal under normal/failure/cancellation/GC exits, status translation, and DTO
strictness. All four documented .NET gates pass, plus every Rust gate (530 tests).

See `docs/BUILD_LOG.md` for the design reasoning, the CA5392/CA5393 analyzer conflict and why
`AssemblyDirectory` is correct here, and the mutation checks.

---

## 7c. C4 is complete — Godot render/export spike

`client/src/DungeonBarrage.Client` is a real Godot 4.7.1 .NET project: `project.godot`,
`export_presets.cfg` (Windows Desktop), `Scenes/Main.tscn`, and `App/Main.cs` driving a menu →
real-fixture-duel → static placeholder render, plus a `--c4-smoke-report`/`--c4-screenshot`
automation path (`App/C4Smoke.cs`) that turns CLIENT_SPEC §20.5 steps 1–4 and 6 into a
machine-checkable JSON report instead of a one-time human walkthrough.

The full contract is now modeled: `PresentationContracts.cs`, `ResponseContracts.cs`, and
`SnapshotContracts.cs` cover every closed enum and every `PresentationEventKind` variant from
`match_session.rs`, verified against the real frozen fixtures in
`DungeonBarrage.Client.Contracts.Tests` (5 tests). Rust's `client_contract.rs` now also emits
`positionScale`/`fixedTickRate` on every snapshot — the two constants a client needs to convert an
authoritative fixed-point position into screen pixels and pace presentation — regenerated through
the fixture corpus's sole legitimate writer (`regenerate_shared_response_fixtures_from_production_abi`,
`#[ignore]`-gated in `db-sim-ffi/src/tests.rs`) so the frozen bytes are the production serializer's
own output, never a hand-edited or test-only shape.

**The gate is met, with real evidence, not an assertion of one.** A release export built from a
clean `--headless --export-release "Windows Desktop"` run, launched from outside the repository:

- Headless smoke run: bootstraps the real `horizontal-test-duel-v1` fixture through the real
  release `db_sim_ffi`, reaching `stateHash f67c5371bcddbdf5` — the same hash the direct Rust and
  C# fixture-parity tests assert — with correct terrain (50×20, 96 solid cells), 8 blocks, 2
  players, and `sessionDisposed: true` / `disposedSessionRejectedReuse: true` proving clean native
  handle disposal (`ObjectDisposedException` on reuse after `Dispose()`).
- Windowed smoke run: the same pipeline, plus a genuine 1280×720 GPU-rendered screenshot (real
  OpenGL context, real NVIDIA device) showing the 8 placeholder blocks and both players — zeke and
  huck — at their correct authoritative positions and health, with the frozen hash burned into the
  HUD text. See `docs/BUILD_LOG.md` for the image.

Three real bugs were found and fixed by actually running the gates rather than trusting that the
code compiled, in order: a `ulong`→`long` CS1503 in `PresentationManifest.cs`; Godot's C# exporter
requiring a solution file colocated with `project.godot` (a Godot-imposed constraint CLIENT_SPEC's
file tree did not anticipate — see `docs/BUILD_LOG.md` for why the top-level `client/DungeonBarrage.sln`
does not replace it); and a locally installed export-template directory misnamed `4.7.1.stable`
instead of `4.7.1.stable.mono` (a machine toolchain fix, not a repository change). A fourth issue was
a genuine logic bug, not a build failure: the very first windowed screenshot captured a blank frame,
because `_Ready` runs before the engine's first process/draw cycle — `QueueRedraw()` had not
actually painted anything yet. Fixed by awaiting two real `ProcessFrame` signals (the Godot C#
idiom for this) before capturing.

---

## 7d. C5 is complete — one playable authoritative turn

`DungeonBarrage.Client.Contracts/CommandContracts.cs` adds a `ClientMatchCommand` polymorphic
envelope (`kind` discriminator, one sealed record per variant) matching `db-sim-ffi`'s
`MatchCommandDto` field-for-field. `DungeonBarrage.Client.Interop/Match/LiveMatch.cs` — moved out of
the Godot project into the Godot-free Interop assembly so it stays headlessly testable — owns one
live match's authoritative state: `SubmitMoveAsync`/`SubmitAbilityAsync`/`SubmitPassAsync` each mint
a fresh command id, submit through `LocalMatchSession`, and reconcile `CurrentSnapshot` **only** from
the returned `PostSnapshot` — never a locally predicted or animated value, per the C5 gate's "every
view ends at the post-snapshot" clause. Terrain re-reads only on a reported
`ClientTerrainChangedEvent`, not on every command.

`Main.cs` wires real input: `ui_left`/`ui_right` for movement, left-click-drag for aim/charge, and a
real UI lock timer (`_inputLockedUntilMsec`) that engages for `InputLockTicks` after every submitted
command, at `PresentationTickRate`. A minimal HUD (active player, phase, health, gauge, turn/gen/hash)
makes one full turn legible. `--c5-smoke-report`/`--c5-screenshot` (`App/C5Smoke.cs`) turn CLIENT_SPEC
§20.5's "a human moves and fires one complete turn" gate into a machine-checkable report, run both
headlessly and windowed.

**The gate is met, with real evidence.** A real windowed run of the scripted move-then-ability
sequence: move accepted with 0 lock ticks (a plain reposition has nothing to play back), ability
accepted with a real 7-tick lock that engaged immediately and correctly lifted after waiting the
window out, real damage landed (huck 400 → 359 HP), the turn handed to the other player
(`b-local-bot`, turn 2), and the reconciled view hash matched the ability transition's own
`PostSnapshot` hash. The 1280×720 screenshot shows the HUD text, both players at their post-turn
positions, and the terrain — see `docs/BUILD_LOG.md` for the image and the full report JSON.

One finding, not a bug: `hash_state` deliberately folds the sorted `processed_command_ids` set into
the authoritative state hash (`db-sim-core/src/hash.rs`, domain `0x04`), so a `LiveMatch`-driven
session — which mints its own command ids rather than replaying the fixture's literal
`"fixture-move-001"`/`"fixture-ability-002"` — can never reproduce the frozen fixture's exact hash,
by design. An early smoke/test pass asserted that equality and failed; the fix was to the test's
expectation, not the production code. What C5's tests and smoke report check instead is what is
actually invariant regardless of command id: disposition, concrete gameplay facts (damage, turn
handoff), and reconciliation against the command's own `PostSnapshot`. Full trace in `docs/BUILD_LOG.md`.

### C6 — complete local match (verified; scene/UI polish tracked separately)

**Already in place, discovered while scoping C6 (do not re-derive or redo):** all nine starter
kits (`crates/db-sim-core/src/character.rs`'s `LAUNCH_ROSTER`, `validate_roster()` asserts exactly
nine), the passive-selection phase and prompt-raising logic (`MatchHost::raise_passive_selection_if_due`),
and victory/objects/statuses are fully modeled and resolver-complete in Rust — none of that is a C6
gap. The actual C6 gaps are almost entirely client-side (character select, passive-prompt UI, a
local planning clock, `Results.tscn`/rematch, camera, full HUD) plus one true engine gap: a bot.

**Done — the Rust bot** (`crates/db-sim-core/src/bot.rs`, `pub mod bot;` in `lib.rs`). `bot::decide`
observes a `SimulationState` exactly as a human client would and proposes one ordinary
`MatchCommandKind`, holding no privileged access — the caller submits its result through the same
`MatchHost` entry points a human command goes through, so a bot's shot is validated identically
(`docs/PRODUCT_SPEC.md`: "Bot difficulty changes candidate search and aim error; it does not ignore
wind, collision, ammunition, or hazards"). Two `BotDifficulty` presets (`Casual`/`Standard`) tune
grid-search resolution and aim-error jitter; every candidate projectile is scored with the real
`ballistics::integrate`, not an approximation. 9 tests, including two full `MatchHost`-driven duels
(a melee Arzum and a projectile Zeke) that assert zero rejected commands and a real win. See
`docs/BUILD_LOG.md`'s C6 entry for a real bug this found and fixed: the melee-closing heuristic
originally walked the bot onto the target's exact tile, which then detonated Huck's own Crater
terrain effect under both fighters simultaneously.

**Done — the bot-decide export, ABI version 2** (`crates/db-sim-ffi/src/lib.rs`'s
`db_sim_match_bot_decide`, `crates/db-sim-ffi/src/wire.rs`'s `BotDecisionRequestDto`/
`WireBotDecision`). Read-only, like `db_sim_match_preview`: it observes the live handle's state and
returns a decision shaped like `MatchCommandDto`'s own `kind` variants minus the session-bookkeeping
fields (`commandId`/`expectedTurnNumber`/`expectedSnapshotGeneration`) — those belong to whichever
caller turns the decision into an ordinary command and submits it through the existing
`db_sim_match_apply`, never a special mutation route. `ABI_VERSION` is now `2` (a function-set
addition, per CLIENT_SPEC §8's own versioning rule); confirmed via a direct PE export-table read
that the release DLL exports exactly eleven `db_sim_*` symbols, the original ten plus
`db_sim_match_bot_decide`, nothing else. The frozen fixture corpus was regenerated for the new
`abiVersion:2` field — every `stateHash` is unchanged (`f67c5371bcddbdf5` → `378081bb2e830a5d` →
`d8686762470c0c36`), confirming this touched only version metadata, not gameplay. 3 new
`db-sim-ffi` tests, including one proving a decision call never mutates the session (two snapshots
taken around several `bot_decide` calls are byte-identical).

**Done — the C# bot-decide consumer.** `client/native/win-x64/db_sim_ffi.dll` is rebuilt and
recopied (ABI version 2, verified with a re-export and a rerun of the C5 windowed/headless smoke
report — identical results, no regression). `DbSimNative.MatchBotDecide` is the new
`LibraryImport`; `LocalMatchSession.DecideBotActionAsync` mirrors `PreviewAsync` exactly (a
read-only call, same `WithBytesAsync`/`Check`/`Copy` plumbing). `DungeonBarrage.Client.Contracts`
gained `BotContracts.cs`: `ClientBotDifficulty`, `ClientBotDecisionRequest`, and the polymorphic
`ClientBotDecision`/`ClientBotMoveDecision`/`ClientBotAbilityDecision`/
`ClientBotPassiveChoiceDecision`/`ClientBotPassDecision` — the exact mirror of
`WireBotDecision`/`WireBotAction` on the Rust side. `ClientMatchCommand` also gained the
`PassiveChoice` factory it was previously missing, and `LiveMatch` gained
`SubmitPassiveChoiceAsync` (filling the same gap) plus `SubmitBotDecisionAsync` — one
decide-then-submit call, matching `bot::decide`'s own "at most two calls per turn" contract; the
caller drives that shape by invoking it again after the first result, exactly as a human's own
move-then-fire submissions already do.

Two stale test assertions expecting ABI version 1 (`FixtureParityTests.cs`,
`FrozenResponseFixtureTests.cs`) are updated to 2. 3 new `DungeonBarrage.Client.Interop.Tests`:
a positive-path decision call, a non-mutation proof (five decisions bracketed by identical
snapshots), and — the strongest evidence — a bot playing **both sides** of the real fixture
(Zeke, ranged-only, versus Huck, melee-only) to a real terminal outcome with zero rejected
submissions, exercising the grid-search and melee-closing paths in one run.

**Done — roster exposure, character select, and the full local match flow.** `db_sim_roster`
(`crates/db-sim-ffi`, `ABI_VERSION` now `3`) serializes the full nine-character launch roster
without needing a live handle — the first handle-less buffer-returning export, added following
`db_sim_match_preview`'s pattern but with `guard(None, ...)`/`serialize_status(None, ...)`, already
proven legal by `db_sim_match_create`'s own pre-handle path. `RosterCatalog.Get()` (C#, Interop) is
a small static class deliberately outside `LocalMatchSession` — a roster listing has no live handle
or session to poison and needs none. `Main.cs` gained a real `CharacterSelect` state
(`EnterCharacterSelect`/`HandleCharacterSelectInput`/`DrawCharacterSelect`) between the menu and a
match: all nine characters, live stat/ability/passive-preview data pulled from the real roster (not
placeholders), up/down to pick a human champion and left/right to pick the bot's, wired into a real
`ClientCreateRequest` built through the already-existing `ClientMatchConfig`/`ClientPlayerConfig`/
`ClientAppearance` types (`FixtureMatchBootstrapper.StartLive`, a sibling to the untouched fixture
`Start()` C4/C5 still depend on). `presentation-manifest-v1.json` was extended from two characters
to all nine so manifest validation does not reject a real selection. Passive selection
(`DrawPassiveSelectModal`), automatic bot turns (`_Process`-driven `SubmitBotDecisionAsync`), a
results/rematch screen, and ability-slot/camera-reset hotkeys round out the flow. A `C6SmokeReport`
(`--c6-smoke-report`/`--c6-screenshot`) automates CLIENT_SPEC §20.5's evidence requirement.

**Two real bugs were found and fixed by independently re-verifying this rather than trusting the
"complete" commit's headless-only report** — see `docs/BUILD_LOG.md`'s C6 verification-pass entry
for the full trace:

1. `NativeLibraryResolver.CandidatePaths()` had picked up a hardcoded `C:\Users\rsfit\...` absolute
   path and several working-directory-based search candidates — a portability defect, and a direct
   contradiction of the file's own documented security invariant ("never the working directory").
   Neither was load-bearing (the test project already copies the DLL beside its own output; the
   Godot export bundles it at the assembly-relative path the original design already searched).
   Reverted to the original two-candidate, assembly-directory-only design.
2. `ClientMatchSnapshot.Outcome` is non-nullable — always populated, with `ClientInProgressOutcome`
   as its "still playing" value — but three places in `Main.cs` checked it against `null` instead of
   pattern-matching the type. Effect: the bot could never take its turn automatically in real
   interactive play (the auto-trigger's `is null` check was always false), and the results modal
   rendered from the very first frame of any match, mislabeled "DRAW" (the gating checks' `is not
   null` was always true). Found by actually looking at a windowed screenshot rather than trusting
   `success: true` — it showed "MATCH COMPLETE — DRAW" at turn 3 with both players still above 80%
   health, a state no real victory/draw condition produces. Fixed all three sites to pattern-match
   `ClientInProgressOutcome` instead.

The C6 smoke path itself was also strengthened to actually exercise character select (a
character-select screenshot, captured before confirming) and to prove a real terminal outcome (loops
bot decisions for whichever player is active until `Outcome` genuinely leaves
`ClientInProgressOutcome`, bounded at 300, instead of stopping after one action and reporting
success regardless).

46 total client tests (9 Contracts.Tests + 37 Interop.Tests), all passing in Debug and Release.
Windowed screenshots confirm both the character-select screen and a real 12-turn victory
(`finalStateHash: 9c3abe727f40e45d`) render correctly.

**Done — `LocalSetup` screen and a Smash-Bros-style character-select redesign.** A read-only
`LocalSetup` screen (`EnterLocalSetup`/`HandleLocalSetupInput`/`DrawLocalSetup`) now sits between the
main menu and character select, showing the one map/mode/slot pairing that exists today and saying so
explicitly rather than faking selectable options. Character select was rebuilt around a reference
Super Smash Bros Ultimate "Solo Battle" screenshot: a 5-wide grid of 76×76 tiles, a per-tile
non-interruptible hover-float animation (a tile only ever reads its latest desired hover state once
its current motion fully completes — never mid-flight), a detail panel, and P1/CPU selection cards.
Portrait art is deliberately placeholder (colored tiles with a monogram letter) — the user-supplied
image folder was missing 4 of 10 requested files and 3 of the remaining 6 depicted a real public
figure in AI-generated satirical scenarios, which was flagged rather than used silently; the user
chose placeholders over either substitute. Two more real bugs were found and fixed verifying this
pass — a screenshot capture missing the established two-`ProcessFrame`-await pattern, and a genuine
concurrency race between the smoke test's manual match-driving and `Main._Process`'s own automatic
bot-turn handler (confirmed via nondeterministic `finalStateHash` across runs before the fix, and
byte-identical hashes after) — see `docs/BUILD_LOG.md`'s follow-up C6 entry for the full trace.

**Still open — deliberately narrowed, not a full C7 UI pass:**

1. Character select, LocalSetup, passive prompt, and results remain hand-drawn text in `Main.cs`'s
   existing `_Draw()`/`_UnhandledInput` state machine, consistent with every other screen so far —
   not dedicated `.tscn` scenes with `Control` nodes or the controller-only navigation CLIENT_SPEC
   §16 eventually requires as a release gate. That is real scene-composition work, not a C6 gap.
2. Real character portrait art does not exist yet; character select uses placeholder colored tiles
   by explicit user choice (see above), pending art direction.
3. Camera is a fixed placeholder viewport (`_cameraOffset`, reset by `F`/`Home`), not the
   follow/frame/zoom behavior CLIENT_SPEC §15 describes.

**Done — the local planning clock (CLIENT_SPEC §9.1), completing the item this section used to list
as open.** The core's `apply_authority_timeout` (already complete and tested, a distinct
`AuthorityTimeout` entry point deliberately kept outside the client command union) is now exposed as
`db_sim_match_timeout` (`ABI_VERSION` 4, thirteenth export), with a full stack on top: a low-level C#
consumer (`LocalMatchSession.TimeoutAsync`, `ClientAuthorityTimeout`), `LiveMatch.PlanningDeadlineUtc`
— a client-local wall-clock deadline kept as its own property rather than written into
`ClientMatchSnapshot.DeadlineAt`, which would have broken `LiveMatch`'s own "state is exactly what the
authority returned" invariant — a 30-second default duration (documented client policy, not a
CLIENT_SPEC mandate), an automatic trigger in `Main._Process` mirroring the pre-existing bot-turn
auto-trigger exactly, and a `"time to act: {n}s"` HUD countdown. Proven end to end by a new smoke path
(`--c6t-smoke-report`/`--c6t-screenshot`) that boots a real match and deliberately never acts, letting
the real production trigger end the turn on its own — which also surfaced a genuine headless-vs-
windowed timing gap (a windowed export's frame loop, launched unfocused by an automated tool, runs far
slower than headless; a poll bound sized in frames rather than wall-clock time under-waited) fixed by
bounding the wait on elapsed real time instead. See `docs/BUILD_LOG.md`'s "the local planning clock
itself" entry for the full trace.

**Gate:** a first-time player selects a character, completes and understands a bot match, and
rematches without developer explanation — met, with real windowed-screenshot evidence, for the
data/flow/mechanics half of that sentence, now including a real local planning clock. Controller-only
play (§16) and dedicated scene files remain open, tracked above.

---

## 7b. Superseded C3 sequence (retained for provenance)

Do not reopen C1/C2 or begin Godot scenes unless a gate regresses.

1. Create the Godot-free .NET solution and interop assembly targeting `net10.0`; restore with a
   committed lock file.
2. Implement source-generated `LibraryImport`, the exact `DbOwnedBuffer`, and a `MatchSafeHandle`.
   Copy/parse/free every native response in `try/finally`; native methods receive the safe handle so
   .NET holds a dangerous reference for the call.
3. Implement strict managed DTOs for the frozen camelCase envelopes and closed enums. Do not add a
   managed gameplay model or serializer variant.
4. Implement RID-only native resolution for `win-x64`, `linux-x64`, `osx-x64`, and `osx-arm64` paths;
   tests on this machine exercise only the advertised Windows RID.
5. Implement a single-call-at-a-time `LocalMatchSession` executor owning create/apply/snapshot/
   terrain/preview and rejecting installation of an older duplicate-replay snapshot.
6. Make xUnit feed the existing request files unchanged through the real release library and compare
   the existing response files byte-for-byte. The final hash must remain `d8686762470c0c36`.
7. Add disposal tests for normal completion, parse exception, cancellation, and forced GC, plus
   malformed/status translation tests. Keep all tests free of a Godot dependency.
8. Only after every C3 gate passes, update this handoff and begin C4's Godot shell.

---

## 8. Verification contract and latest evidence

Run from the canonical root:

```powershell
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --release -p db-sim-ffi --locked
cargo build --release -p db-sim-ffi --locked
cargo deny check
$env:DUNGEON_BARRAGE_GODOT = [Environment]::GetEnvironmentVariable('DUNGEON_BARRAGE_GODOT', 'User')
.\scripts\verify-toolchain.ps1
git status --short --branch
```

Latest inventory: 547 passing tests.

- 517 `db-sim-core` unit tests (includes 9 for the C6 `bot` module).
- 7 golden-vector tests.
- 1 shared direct fixture test.
- 21 real `db-sim-ffi` tests (includes 3 for `db_sim_match_bot_decide`, 2 for `db_sim_roster`, 3 for
  `db_sim_match_timeout`).
- 1 dormant `db-sim-wasm` test.

Additional native gates passed:

- release FFI test: 21 pass (historical figure at C2 landing was 13; see §7d for the current
  13-export/ABI-version-4 surface);
- Windows release DLL build: pass;
- Windows/Linux exact export surface: 10 expected symbols at C2 landing, now 13 (§5, §7d);
- WSL2 Valgrind release lifecycle: zero definite/indirect leaks, zero errors;
- `cargo deny`: advisories, bans, licenses, and sources pass (unused allow-list warnings only);
- toolchain verifier: exact .NET/Rust/Godot/template versions pass;
- frozen production response bytes: pass.

`core.autocrlf` LF-to-CRLF notices are non-failing warnings. Do not normalize the repository to
silence them.

.NET inventory: 52 passing tests — 43 `DungeonBarrage.Client.Interop.Tests`, 9
`DungeonBarrage.Client.Contracts.Tests`. Godot gates: headless editor import, headless
`--export-release "Windows Desktop"`, and a real windowed run all pass; see §7c (C4) and §7d (C5/C6).
Native library is `db_sim_ffi.dll` ABI version 4 (§7d).

---

## 9. Copy-paste Opus resume prompt

```text
Continue Dungeon Barrage from the completed C1-C5 checkpoint and implement C6 only.

Canonical repo: C:\Users\rsfit\DungeonBarrage
Branch: feat/c1-outcome-provenance tracking origin/feat/c1-outcome-provenance
Run git status --short --branch, git rev-parse HEAD, git log -5 --oneline --decorate, and git diff
--check first. Do not use C:\Users\rsfit\OneDrive\Documents\DungeonBarrage; it is a different empty
repo.

Read in order:
1. docs/HANDOFF.md
2. docs/CLIENT_SPEC.md
3. docs/adr/0006-client-and-server-language-boundaries.md
4. docs/MODULE_OWNERSHIP.md
5. todolist.md
6. docs/SECURITY_BASELINE.md
7. docs/BUILD_LOG.md

Do not reset, checkout, clean, bulk-stage, normalize line endings, read/stage .github-token, or
rewrite accepted ADR history. Preserve unrelated work. Use apply_patch for edits.

Architecture is settled: Godot 4.7.1 .NET + C# is presentation; Rust db-sim-core is the only
authority and the future server is Rust-native. C1 through C5 are complete. ABI version 1 has the
exact ten exports listed in HANDOFF section 5, strict bounded JSON, clone-serialize-commit apply,
poisoned handles, and exact Rust-owned boxed buffers. The shared fixture freezes create/snapshot/
preview/move/ability response bytes and hashes f67c5371bcddbdf5 -> 378081bb2e830a5d ->
d8686762470c0c36. `client/src/DungeonBarrage.Client` drives a real live turn — move, aim/fire an
ability, input locked during playback, view reconciled to `PostSnapshot` — verified by a real
windowed smoke run with a screenshot, not just a compile. Read HANDOFF §7d before touching
`LiveMatch`/`CommandContracts`: `hash_state` intentionally folds `processed_command_ids` into the
state hash, so a live-driven session's hash is never expected to equal a frozen fixture's hash — do
not "fix" that by trying to make command ids match.

Next task: C6, complete local match, only. Add all nine starter kits (only two placeholder kits
exist today); the passive prompt; a Rust-driven local bot opponent; a local clock/timeout;
victory/results/rematch; objects; statuses; a camera; and the full HUD beyond C5's essentials
(active player, phase, health, gauge). Do not add gameplay rules to C#, and do not infer missing
mechanics. Arzum's random second hit, object expiry/destruction, richer turret/gas behavior,
remaining passives/hazards, and Numa balance are explicit gaps — resolve them as part of C6's own
scope only if CLIENT_SPEC requires it for a complete local match, not speculatively.

Run every gate in HANDOFF section 8 plus the CLIENT_SPEC Godot gates (headless editor import,
export-release, and a real windowed smoke run — a screenshot proving pixels painted, not just that
the export succeeded). Maintain HANDOFF and append BUILD_LOG; leave a clean, committed, verified
tree and report commit/push state exactly.
```

---

## 10. Landing notes

The owner requested dependency installation/verification, complete documentation, a Git commit, and
a push. Commit only after every gate succeeds. Stage explicit reviewed paths and confirm
`.github-token` is absent from the index. Push with plain `git push` using configured credentials
only; never place a token in a command, URL, environment variable, file, log, or commit metadata.
