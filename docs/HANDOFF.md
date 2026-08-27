# Dungeon Barrage operational handoff

**Checkpoint date:** 2026-08-26

**Audience:** the next implementation agent, especially Claude Opus

**State:** the C0 local toolchain gate, C1, and C2 are implemented. All local Rust, release,
supply-chain, toolchain, fixture-byte, export-surface, and Valgrind gates pass. The commit containing
this file is the landing checkpoint; verify its exact ID and upstream state rather than trusting a
chat summary.

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

The reviewed predecessor was `6a161c645fed45a585342f0a3a33e2abf604d13f`. The final checkpoint is
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

- ~~No C# project, `SafeHandle`, managed DTO layer, or `LocalMatchSession` exists~~ — all added in C3. No Godot project exists; that is C4. The original note read: C3 is
  next; do not begin scenes first.
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

### Next: C4's Godot shell

Do not reopen C1/C2/C3 unless a gate regresses.

1. Add `DungeonBarrage.Client` — the engine project, `project.godot`, export presets — referencing
   the interop assembly. Keep every existing test Godot-free; adding an engine reference to the
   current test projects would make them unrunnable headlessly, which is the property C3 bought.
2. Model the remaining envelopes in the contracts assembly. Only the creation request and the
   closed enums exist today; snapshot, transition, and presentation-event DTOs are still described
   only by the frozen envelopes and the Rust types.
3. Populate `client/native/` for the other advertised RIDs when those targets are actually built.
   Three directories are deliberately empty rather than filled with untested binaries.

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

Latest inventory: 530 passing tests.

- 508 `db-sim-core` unit tests.
- 7 golden-vector tests.
- 1 shared direct fixture test.
- 13 real `db-sim-ffi` tests.
- 1 dormant `db-sim-wasm` test.

Additional native gates passed:

- release FFI test: 13 pass;
- Windows release DLL build: pass;
- Windows/Linux exact export surface: 10 expected symbols;
- WSL2 Valgrind release lifecycle: zero definite/indirect leaks, zero errors;
- `cargo deny`: advisories, bans, licenses, and sources pass (unused allow-list warnings only);
- toolchain verifier: exact .NET/Rust/Godot/template versions pass;
- frozen production response bytes: pass.

`core.autocrlf` LF-to-CRLF notices are non-failing warnings. Do not normalize the repository to
silence them.

---

## 9. Copy-paste Opus resume prompt

```text
Continue Dungeon Barrage from the completed C1/C2 checkpoint and implement C3 only.

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
authority and the future server is Rust-native. C1 and C2 are complete. ABI version 1 has the exact
ten exports listed in HANDOFF section 5, strict bounded JSON, clone-serialize-commit apply, poisoned
handles, and exact Rust-owned boxed buffers. The shared fixture freezes create/snapshot/preview/move/
ability response bytes and hashes f67c5371bcddbdf5 -> 378081bb2e830a5d -> d8686762470c0c36.

Next task: C3 headless .NET interop/session only. Add LibraryImport, MatchSafeHandle, exact DTOs,
RID-only native resolution, LocalMatchSession, and xUnit tests against the real release DLL and the
existing raw fixture. Do not add gameplay rules to C#, do not start Godot scenes, and do not infer
missing mechanics. Arzum's random second hit, object expiry/destruction, richer turret/gas behavior,
remaining passives/hazards, and Numa balance are explicit gaps, not C3 work.

Run every gate in HANDOFF section 8 plus the CLIENT_SPEC C3 .NET gates. Maintain HANDOFF and append
BUILD_LOG; leave a clean, committed, verified tree and report commit/push state exactly.
```

---

## 10. Landing notes

The owner requested dependency installation/verification, complete documentation, a Git commit, and
a push. Commit only after every gate succeeds. Stage explicit reviewed paths and confirm
`.github-token` is absent from the index. Push with plain `git push` using configured credentials
only; never place a token in a command, URL, environment variable, file, log, or commit metadata.
