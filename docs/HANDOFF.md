# Dungeon Barrage operational handoff

**Checkpoint date:** 2026-08-25

**Audience:** the next implementation agent, especially Claude Opus

**State:** verified committed C0/C1 checkpoint; Rust, release, supply-chain, Godot editor, and
Godot export-template gates pass

This is the mutable resume document. `BUILD_LOG.md` is append-only history,
`PROGRAM_PLAN.md` is a superseded historical plan, and `CLIENT_SPEC.md` is the governing client
implementation contract. Keep this file current whenever a work session changes the next safe
action.

---

## 1. Authority and reading order

Read these before editing:

1. `docs/HANDOFF.md` — current operational state and exact next work.
2. `docs/CLIENT_SPEC.md` — normative native-client architecture and C0–C7 gates.
3. `docs/adr/0006-client-and-server-language-boundaries.md` — settled language boundary.
4. `docs/MODULE_OWNERSHIP.md` — shared-tree ownership and safety rules.
5. `todolist.md` — mechanics problems and the active client-boundary problem.
6. `docs/SECURITY_BASELINE.md` — trust boundary and CI requirements.
7. `docs/BUILD_LOG.md` — append-only history and corrections.

When they conflict, the newest accepted ADR wins, then `CLIENT_SPEC.md` for client/contract work.
Do not rewrite older accepted ADRs to make history look consistent. ADR 0004's ASP.NET server and
ADR 0003's TypeScript-parity rationale are historical; ADR 0006 supersedes those portions.

---

## 2. Canonical repository identity

| Property | Value |
|---|---|
| Canonical working root | `C:\Users\rsfit\DungeonBarrage` |
| Branch | `main` tracking `origin/main` |
| Campaign base | `fa7f0af817975b4563bfb792296a44191960637a` |
| Baseline upstream | same commit |
| Current checkpoint | The commit containing this handoff; run `git rev-parse HEAD` for its ID |
| Worktree | Expected clean immediately after the checkpoint commit; always recheck |

`C:\Users\rsfit\OneDrive\Documents\DungeonBarrage` is a different, effectively empty repository
shown by some desktop-thread contexts. Do not edit, merge, or copy this work into that path. Begin
every shell session with:

```powershell
Set-Location -LiteralPath 'C:\Users\rsfit\DungeonBarrage'
git status --short --branch
git rev-parse HEAD
```

Never run `git reset --hard`, `git checkout --`, `git clean`, broad staging, or a bulk line-ending
rewrite here. Do not read, stage, print, or otherwise touch the ignored `.github-token`; it is
historical sensitive material and still needs owner-side rotation.

---

## 3. Settled architecture — do not reopen casually

ADR 0006 records the language decision after comparing C#, Rust, C++, TypeScript, Unity, Godot,
MonoGame, Bevy, and a Rust-only client:

- Godot 4.7.1 .NET + C# targets `net10.0` for presentation, input, accessibility, scenes, UI,
  platform integration, and content iteration.
- Rust `db-sim-core` is the only authoritative gameplay implementation.
- Local C# calls Rust through a coarse client-only C ABI in `db-sim-ffi`.
- The future authoritative match server is Rust-native and links `db-sim-core` directly; it does
  not P/Invoke through C#.
- The web client is retired. `db-sim-wasm` is dormant only as a deliberately gated revisit path.
- Consoles are a future business/SDK gate, not a reason to choose a worse launch stack now.

C# is the best language for the chosen Godot presentation layer; it is not the best place for
authoritative simulation or the future server. A Rust-only Godot GDExtension client would increase
FFI/editor/tooling friction without removing the need for a presentation engine. Do not port the
simulation to C#, and do not start Godot scenes before the C1–C3 contract gates.

---

## 4. Checkpoint inventory and intent

All entries below belong to the same reviewed implementation campaign and land together in the
checkpoint commit.

| Path | State and intent |
|---|---|
| `Cargo.toml` | Workspace lint/profile corrections; release unwinding is required for FFI panic containment. |
| `Cargo.lock` | Locks the serde/serde_json dev-only fixture-test graph; no new production core dependency. |
| `README.md` | Current Godot/C#/Rust architecture and truthful not-playable status. |
| `global.json` | Pins .NET SDK 10.0.302. |
| `rust-toolchain.toml` | Pins Rust 1.94.0 with required components. |
| `scripts/verify-toolchain.ps1` | Verifies pinned Rust/.NET/Godot plus the matching .NET template version and Windows x86_64 debug/release binaries. |
| `.gitattributes` | Forces LF for exact shared JSON fixture bytes. |
| `crates/db-sim-core/Cargo.toml` | Keeps production dependencies empty; adds serde/serde_json only for fixture tests. |
| `crates/db-sim-core/src/types.rs` | Per-projectile `ProjectileTrace` contract and corrected command outcome fields. |
| `crates/db-sim-core/src/command.rs` | Independent traces, post-action/gauge outcome corrections, routed terrain accounting. |
| `crates/db-sim-core/src/match_host.rs` | Host hash/turn fixes, clone support, passive guards, and movement-fall elimination progression. |
| `crates/db-sim-core/src/scheduler.rs` | Commits the final pending turn reason on the terminal victory path and tests that regression. |
| `crates/db-sim-core/src/match_setup.rs` | Validated real-map/real-roster `MatchConfig` construction plus shared opaque-ID validation. |
| `crates/db-sim-core/src/client_contract.rs` | Detached engine-neutral `MatchSnapshot` projection. |
| `crates/db-sim-core/src/match_session.rs` | Normalized command union, canonical digest, generations, complete bounded first-result ledger, duplicate replay, transitions, conservative movement provenance, net-diff events, exact terrain row-runs. |
| `crates/db-sim-core/src/lib.rs` | Wires the new core boundary modules and sets `SIMULATION_VERSION = 5` for the terminal-reason compatibility correction. |
| `crates/db-sim-core/src/resolve/attack_mods.rs` | Removes stale comments that incorrectly said terrain counts were discarded after `ef3c41f` fixed them. |
| `crates/db-sim-core/tests/shared_match_fixtures.rs` | Direct Rust consumer of exact shared request bytes and meaningful expectations. |
| `crates/db-sim-core/tests/golden_vectors.rs` | Regenerates all five whole-match vectors under version 5 with every prior v4 hash retained in comments. |
| `tests/fixtures/matches/**` | Cross-language raw match create/command bytes and frozen semantic manifest. |
| `crates/db-sim-ffi/Cargo.toml` | Isolates the sole audited `unsafe` exception and builds a native `cdylib`. |
| `crates/db-sim-ffi/src/lib.rs` | Still a scaffold, but with correct unwind containment and release test. Do not mistake it for a real match ABI. |
| `docs/CLIENT_SPEC.md` | Rewritten v2 implementation specification and live C1 progress/gaps. |
| `docs/adr/0006-client-and-server-language-boundaries.md` | Accepted language/engine boundary decision. |
| `docs/MODULE_OWNERSHIP.md` | Updated ownership for C1/C2 and the isolated FFI unsafe exception. |
| `docs/PROGRAM_PLAN.md` | Historical-plan supersession banner only; body intentionally preserved. |
| `todolist.md` | P2/P3 historical statuses corrected; active client-boundary P13 added. |
| `docs/BUILD_LOG.md` | Append-only checkpoint for the Aug-7/Aug-14/current work. |
| `docs/HANDOFF.md` | This mutable operational handoff and Opus prompt. |

Before landing anything, distinguish real diffs from `core.autocrlf` warnings. Do not normalize the
entire repository merely to silence Git's “LF will be replaced by CRLF” notice.

---

## 5. What is implemented and what the evidence means

### Implemented in the authoritative/direct Rust path

- Real `MatchHost` orchestration across maps, movement, abilities/effects, settling, status turns,
  blocks, pass/passive flow, victory, and hard match termination.
- Frozen Rust golden vectors. They prove self-consistency, not correctness against the retired
  TypeScript oracle.
- Validated transport-free match creation from a real roster/map.
- Detached atomic snapshot projection with deterministic ordering and exact state hash.
- Independent projectile traces with terminal impacts.
- Normalized typed commands with deterministic canonical digest. The session compares the full
  typed command as well as the digest, so a digest collision cannot authorize changed content.
- `MatchSessionHost` owns generation and all first well-formed accepted/rejected receipts.
  Identical ID/content returns the original transition as `duplicateReplay`; same ID/different
  content is a security rejection without mutation.
- Host application occurs on a clone. Generation increments exactly once only if
  `working_host.state() != live_host.state()`. A valid zero/blocked move is accepted, retained, and
  leaves generation unchanged.
- Transition/post-snapshot/live-host hashes are checked for equality.
- The retained ledger is bounded by both 16,384 first receipts and exactly 64 MiB of deterministic
  canonical typed command/transition bytes. Complete snapshots, events, strings, nested vectors,
  traces, and samples are counted with checked `u64` arithmetic; crossing either limit closes
  atomically before publishing the cloned host, generation, or ledger entry.
- Ordered presentation events truthfully combine recorded projectile/damage outcomes with net
  pre/post diffs. All current net movement is conservatively labelled authoritative resolution:
  without an intermediate post-walk/pre-settle path, even an unchanged final height cannot rule out
  a climb-and-settle. `RequestedMove` is reserved until that provenance exists. Terrain dirty
  rectangles are exact changed-cell row-runs.
- A movement fall that eliminates the active player now drives the normal eliminated-turn and
  victory/rotation path instead of stranding a dead actor.
- Terminal victory commits the pending final turn reason even though there is no next player and
  `end_turn` is skipped. This replay-visible correction is `SIMULATION_VERSION = 5`; the regenerated
  golden corpus records every prior v4 hash.
- The version-5 direct fixture hashes are initial `65ac3e53023ca6b0`, after move
  `9d92d3b5d5dad7d0`, and after ability/final `af724375e588d90b`.

### Important limits — do not call C1 complete

- `CommandOutcome` now carries per-strike provenance (`strikes: Vec<StrikeResolution>`): the
  resolution-order index, target, exact impact point, melee/projectile delivery with the citing
  trace sequence, the crit draw as `CritRoll::{NotEligible, Missed, Landed}`, applied damage, and
  whether that strike caused the elimination. Emitted at the point of resolution by both the
  projectile and melee producers in `command.rs`, and consumed by `match_session::derive_events`
  as one `StrikeResolved` event per strike.
- `CommandOutcome` now carries `status_changes: Vec<StatusChange>` covering every status
  transition: `Applied`, `Refreshed` (with the values it displaced), `ChargeConsumed`, `Ticked`,
  `Exhausted`, and `Expired`. Recorded at all four producers — `resolve::status::apply_status`,
  `resolve::status::tick_statuses`, and both halves of `GuaranteeCrit` in
  `resolve::attack_mods`. A status applied and expired inside one call, and several charges
  consumed by one multi-strike ability, are both fully reported. `MatchHost::status_changes()`
  is the same record for commands that produce no outcome (Move, Pass, timeout).
- `derive_events` emits these as `StatusChanged` and **cross-checks** them: any status the pre-
  and post-snapshots disagree about that no record explains is a `SessionFault::ContractInvariant`.
  A future producer that mutates `statuses` without recording fails closed rather than silently
  telling the client nothing happened.
- `CommandOutcome` still does not retain exact object removal causes, and has no
  `objects_removed` counterpart to `objects_created`. `match_session.rs` labels those net changes
  honestly and does not invent them.
- ~~Authority-generated timeout has no session entry point~~ — added; see §6 step 3.
- No read-only authoritative trajectory preview contract exists.
- Match ID, ABI/envelope version, clocks, and serialized terrain bytes remain adapter metadata.
- Duplicate replay deliberately contains the original (possibly old) post-snapshot. It is an
  acknowledgement and must never reconcile a client backward over a newer generation.
- The inner `SimulationState.processed_command_ids` list still covers accepted ability/passive
  commands only. The outer session ledger is the actual all-kind/all-result replay authority.
- The shared C1 fixture freezes semantic hashes and meaningful direct-Rust behavior. Full expected
  response JSON bytes must wait for C2's production serializer; do not bless a test-only serializer
  as the ABI.
- `db-sim-ffi` still creates a placeholder handle and placeholder state hash.
- No C# solution, Godot project, exported client, or match server exists.

### Mechanics/product gaps outside the immediate client boundary

- Chosen passive IDs are recorded, but several passive modifiers are not applied to gameplay.
- Turret, gas-cloud, and Embers lifecycle behavior remains incomplete.
- Sudden-death hazard behavior is absent.
- Fifteen launch characters and forty-five passives remain undesigned.
- Four character-rule decisions and the level-up reward imbalance remain owner decisions in
  `todolist.md` P10.

---

## 6. Exact next engineering sequence

Continue C1; do not begin C2 or Godot UI yet.

1. ~~Action impacts, per-strike crit/RNG records, status lifecycle records, object removal
   causes~~ — **step 1 is complete.** `PersistentObjectChange` replaces `objects_created` with an
   ordered spawn/remove stream naming a real `PersistentObjectRemovalCause`, consumed by
   `derive_events` under the same fail-closed reconciliation guard used for statuses.
   `RemovalCause::{Expired, Destroyed}` are defined but unreachable until a scheduler-owned
   object-lifetime tick and object targeting/damage exist; both say so in their doc comments.

   **Owner decision needed — this was a balance change, not only a provenance change.** Statuses
   now tick on the affected player's own turns rather than once per command submitted anywhere.
   A two-turn status is therefore roughly twice as long in a duel and four times as long in a
   four-player match. This is more correct — the same status no longer means different things at
   different table sizes — but Numa's Pin and any future Chill are directly affected and the
   numbers in `CHARACTERS.md` were written against the old reading.
2. Extend the session event builder and tests for real multi-strike, strike, random outcome,
   duration-one status, object lifecycle, block/terrain, elimination, passive selection/chosen,
   pass, timeout, and victory. Keep ordering deterministic and version the client contract if
   released semantics change.
3. ~~Add an authority-only timeout method to `MatchSessionHost`~~ — **done.**
   `apply_authority_timeout` takes an `AuthorityTimeout`, which is deliberately **not** a
   `MatchCommandKind` variant. A client sends bytes that a decoder turns into a `MatchCommand`, so
   an absent variant is a stronger guarantee than a validation rule that one decoding bug could
   bypass. It shares the single ledger and identifier space via
   `LedgerRequest::{Client, Authority}`, so an id claimed by either side conflicts for the other
   rather than replaying its answer. `player_id` is required and validated against the active
   player, so a deadline racing a turn handover cannot end an innocent player's turn.
4. Add the read-only preview DTO/path with stale-generation refusal and no mutation/RNG consumption.
5. Add restore semantics that require host plus the complete ledger and its verified byte count.
   Never expose public `from_host(host)` with an empty ledger.
6. Finish the composite session/ABI envelope and remaining direct §20.1 fixture scenarios. The
   baseline fixture must retain
   fixed IDs, nonzero movement, independent trace/sample minima, generation changes, turn handoff,
   and exact final hashes.
7. Only when C1 passes, replace the FFI placeholder with a real cloned-session create/apply/
   snapshot/terrain/preview ABI and bounded owned buffers (C2).
8. Only when the raw fixture passes through release FFI, create headless C# contracts/interop and
   `SafeHandle` tests (C3). Godot scenes start at C4.

The strike and status halves of that seam are now in place. The next seam is the same shape for
persistent objects: `resolve/objects.rs` and whatever removes turrets, knives, and gas clouds must
record why. Keep `types.rs` integrator-owned while this shared contract changes.

**Content gap found while wiring statuses.** Only two effects in the entire launch roster attach a
status: Numa's Pin (`Lockdown`) and Karl's Feeding Frenzy (`GuaranteeCrit`), and both are specials
gated behind a full gauge. `resolve::status::resolve_chill` and `resolve_embers` are fully
implemented and tested but **no ability references `EffectKind::Chill` or `EffectKind::Embers`**,
so neither can occur in a real match. This is a content gap, not a wiring bug — the fifteen
undesigned characters are expected to use them — but it means status behaviour is currently far
less exercised in real play than the test count suggests. It is also why the session-level status
tests seed a status directly instead of casting an ability to produce one.

**Reachability warning, confirmed by this slice.** `StrikeResolved` was previously gated on
`matches!(ability.attack, Attack::Strike(_))`. Karl's Carrion Call is the only multi-strike ability
in the roster and the one whose design note promises three independent crit rolls, and it is an
`Attack::Projectile` — so it emitted **zero** strike events while every test passed. This is the
fifth occurrence of the repository's signature failure mode. Emission is now driven by what the
outcome actually recorded, never by the ability's declared shape.

---

## 7. Verification contract

Run from the canonical root:

```powershell
git diff --check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --release -p db-sim-ffi
cargo build --release -p db-sim-ffi
cargo deny check
.\scripts\verify-toolchain.ps1
git status --short --branch
```

Expected toolchain behavior on this machine: .NET SDK 10.0.302, Rust/Cargo 1.94.0, Godot
`4.7.1.stable.mono.official.a13da4feb`, and the `4.7.1.stable.mono` export templates all pass. The
editor is installed through WinGet for the current user; `DUNGEON_BARRAGE_GODOT` points to its
versioned executable. Templates use Godot's standard per-user directory.

Latest verified results for this checkpoint:

| Gate | Result |
|---|---|
| `git diff --check` | pass; only non-failing `core.autocrlf` LF→CRLF notices |
| `cargo fmt --all --check` | pass |
| workspace clippy | pass with `-D warnings` |
| workspace tests | pass: 456 tests (440 core unit + 7 golden + 1 shared fixture + 7 FFI + 1 WASM) |
| release FFI tests/build | pass: 7 tests; optimized `cdylib` build succeeds |
| `cargo deny check` | pass: advisories, bans, licenses, and sources; only unmatched allow-list warnings |
| shared fixture v5 hashes | `65ac3e53023ca6b0` → `9d92d3b5d5dad7d0` → `af724375e588d90b` |
| exact request bytes | pass: UTF-8, no BOM/CR, one terminal LF for all three request files |
| toolchain script | pass: .NET 10.0.302, Rust/Cargo 1.94.0, Godot 4.7.1 .NET, and matching .NET templates |

---

## 8. Copy-paste Opus resume prompt

```text
You are continuing Dungeon Barrage from the reviewed C0/C1 checkpoint commit.

Canonical repo: C:\Users\rsfit\DungeonBarrage
Branch: main. Campaign base: fa7f0af817975b4563bfb792296a44191960637a. Run git rev-parse HEAD
to identify the newer local checkpoint commit; it has not been pushed unless the owner says so.
WARNING: C:\Users\rsfit\OneDrive\Documents\DungeonBarrage is a different empty repo. Do not use it.

First read, in order:
1. C:\Users\rsfit\DungeonBarrage\docs\HANDOFF.md
2. C:\Users\rsfit\DungeonBarrage\docs\CLIENT_SPEC.md
3. C:\Users\rsfit\DungeonBarrage\docs\adr\0006-client-and-server-language-boundaries.md
4. C:\Users\rsfit\DungeonBarrage\docs\MODULE_OWNERSHIP.md
5. C:\Users\rsfit\DungeonBarrage\docs\SECURITY_BASELINE.md
6. C:\Users\rsfit\DungeonBarrage\todolist.md
7. C:\Users\rsfit\DungeonBarrage\docs\BUILD_LOG.md

Start from a clean worktree. Do not reset, checkout, clean, bulk-stage, normalize line endings,
read/stage .github-token, or rewrite accepted historical ADRs. Do not push unless the owner
explicitly asks. Check git status and diff before editing.

Architecture is settled: Godot 4.7.1 .NET + C# is presentation; Rust db-sim-core is the only
authoritative gameplay; local C# uses the client-only coarse db-sim-ffi ABI; the future server is
Rust-native. Do not port rules to C# and do not start Godot scenes before C1-C3 gates.

Current objective: continue CLIENT_SPEC C1. The working tree already contains MatchConfig,
MatchSnapshot, independent ProjectileTrace values, corrected post-host hashes, and a new
MatchSessionHost with normalized MatchCommand, generation/idempotency ledger, duplicate replay,
ordered net-diff MatchTransition events, exact 16,384-entry/64 MiB ledger bounds, conservative
net-movement provenance (all current movement is authoritative resolution), exact terrain dirty
row-runs, shared raw JSON fixtures, and tests. It also fixes movement-fall elimination so a dead
active player cannot strand the match and commits the final turn reason on terminal victory. That
compatibility correction is SIMULATION_VERSION=5. The shared fixture hashes are
65ac3e53023ca6b0 initially, 9d92d3b5d5dad7d0 after movement, and af724375e588d90b after the
ability/final transition.

Next implementation: enrich authoritative CommandOutcome/resolvers with truthful action-impact,
per-strike/RNG, status-lifecycle, and object-removal provenance; consume it in match_session events;
then add authority timeout, preview, safe session-plus-ledger restore, and the remaining direct C1
scenarios. Never infer missing provenance from final state. Keep production db-sim-core
dependency-free; serde in that crate is dev-only. Full response JSON fixtures begin only with C2's
production serializer.

Before work:
Set-Location -LiteralPath 'C:\Users\rsfit\DungeonBarrage'
git status --short --branch
git rev-parse HEAD
git diff --check

After every coherent slice run:
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --release -p db-sim-ffi
cargo build --release -p db-sim-ffi
cargo deny check
.\scripts\verify-toolchain.ps1
git status --short --branch
git diff --check

Maintain docs/HANDOFF.md and append docs/BUILD_LOG.md. Report verified evidence separately from
design or blocked external prerequisites. Leave the tree usable and document every remaining gap.
```

## 9. Landing notes

The owner requested a local commit on 2026-08-25; the campaign lands in the commit containing this
handoff and was not pushed. The `SIMULATION_VERSION = 5` compatibility correction and regenerated
vector constants retain their previous values and rationale in the golden source and build log.
Future vector changes must follow that same explicit compatibility procedure.
