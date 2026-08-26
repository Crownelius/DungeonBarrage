# Dungeon Barrage operational handoff

**Checkpoint date:** 2026-08-25

**Audience:** the next implementation agent, especially Claude Opus

**State:** C0 is complete; C1 is substantially implemented but remains open. The commit containing
this file is the current reviewed checkpoint. Run the commands below rather than trusting an older
chat summary.

This file is the mutable resume document. `docs/BUILD_LOG.md` is append-only history,
`docs/CLIENT_SPEC.md` is the governing native-client contract, and accepted ADRs record settled
architecture. Update this file whenever the next safe action changes.

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

Current landing branch: `feat/c1-outcome-provenance`, tracking
`origin/feat/c1-outcome-provenance`.

Important checkpoints before the commit containing this handoff:

- `97fd8b7` — status lifecycle transitions at every producer.
- `a6b9cf4` — ordered persistent-object lifecycle causes, exact status replay, forced-crit plumbing,
  affected-player status ticking, and associated session events.
- `5946c63` — authority-only timeout with bounded idempotent ledger semantics.

The current review started from `5946c63`. The final checkpoint is the newer commit containing this
file; obtain its ID with `git rev-parse HEAD`.

Do not run `git reset --hard`, `git checkout --`, `git clean`, broad staging, or a bulk line-ending
rewrite. Do not read, print, stage, or touch the ignored `.github-token`. A credential was pasted in
chat and must be rotated by the owner; use only already-configured Git credential storage for pushes.

---

## 2. Reading order and authority

Read in this order:

1. `docs/HANDOFF.md` — operational state and next work.
2. `docs/CLIENT_SPEC.md` — normative C0-C7 client plan and release gates.
3. `docs/adr/0006-client-and-server-language-boundaries.md` — accepted language boundary.
4. `docs/MODULE_OWNERSHIP.md` — shared-tree rules and file ownership.
5. `todolist.md` — mechanics gaps and open P13 client-boundary work.
6. `docs/SECURITY_BASELINE.md` — trust boundary and CI requirements.
7. `docs/BUILD_LOG.md` — append-only evidence and corrections.

If documents conflict, the newest accepted ADR wins, then `CLIENT_SPEC.md` for client/contract work.
Do not rewrite older accepted ADRs merely to make the history look consistent.

---

## 3. Settled language and engine decision

Do not reopen this without new evidence:

- Godot 4.7.1 .NET plus C# is the presentation layer: scenes, UI, input, accessibility, platform
  integration, and content iteration.
- Rust `db-sim-core` is the only authoritative gameplay implementation.
- Local C# calls Rust through a coarse, client-only C ABI in `db-sim-ffi`.
- A future authoritative server is Rust-native and links `db-sim-core` directly.
- The web client is retired. `db-sim-wasm` is dormant, not a second rules implementation.

C# remains the best fit for the chosen Godot presentation layer. Rust remains the best fit for the
deterministic authority and future server. A Rust-only GDExtension client would add editor/FFI
friction without removing the need for a presentation engine; putting rules in C# would create a
second authority. Do not start Godot scenes before C1-C3 pass.

Pinned dependencies are already installed on this machine and are verified by
`scripts/verify-toolchain.ps1`: Rust/Cargo 1.94.0, .NET SDK 10.0.302, Godot 4.7.1 .NET, and matching
4.7.1 .NET export templates. Re-run the verifier; do not assume an installation alone is evidence.

---

## 4. What is implemented now

### Session and replay boundary

- Validated real-map/real-roster `MatchConfig` creation.
- Detached, engine-neutral `MatchSnapshot` with exact authoritative state hash.
- Normalized typed `MatchCommand`, canonical semantic digest, and generation ownership.
- Clone-before-apply atomicity; rejected or faulted work cannot publish partial state.
- First-result ledger covering accepted and rejected receipts, exact duplicate replay, and
  same-ID/different-content conflict rejection.
- Exact 16,384-entry and 64 MiB deterministic retained-byte limits.
- Separate authority-only timeout entry point. Timeout is not a client command variant, shares the
  ID namespace/ledger, refuses stale turn/generation/player races, and replays idempotently.

### Action provenance

- Every real strike carries a dense resolution index, target, exact point, delivery
  (`Projectile`, `Melee`, or effect), cited trace where applicable, exact applied damage,
  elimination flag, and `CritRoll::{NotEligible, Missed, Landed, Forced}`.
- `Forced` is truthful: it is critical and consumes no RNG draw.
- Karl's live Feeding Frenzy now marks a target, survives duration ticking, forces the next three
  real Carrion Call projectile crits, records two charge consumptions plus exhaustion, and advances
  no crit RNG state for those forced strikes.
- Effect-delivered `MultiStrike` also produces one strike record per resolved strike.
- Public non-strike random outcomes are not implemented yet. Arzum/Aleph destination draws remain
  authoritative but are not represented as `randomOutcome` events.

### Status lifecycle

- Producers record exact `Applied`, `Refreshed`, `ChargeConsumed`, `Ticked`, `Exhausted`, and
  `Expired` transitions.
- The session replays every transition against a shadow pre-status map, validates exact old/new
  values and cardinality, preserves producer order, and requires an exact post-status map.
- Status durations tick only when the affected player completes a turn. Intervening players do not
  consume duration. `GuaranteeCrit` is count-based and never duration-ticks.

### Persistent-object lifecycle

- One ordered `PersistentObjectChange` stream preserves causal order across:
  - turret replacement: remove old, then spawn new;
  - knife-cap eviction: spawn new, then remove oldest;
  - chain detonation: spawn the landing knife, then remove every detonated knife in sequence order;
  - owner elimination: remove every owned object in sequence order.
- Removal causes are closed and producer-owned: `Replaced`, `CapacityEvicted`, `Detonated`,
  `OwnerEliminated`; `Expired` and `Destroyed` are reserved but unreachable.
- Session reconciliation exactly replays complete spawn/remove snapshots. Missing, unknown,
  duplicate, stale, mismatched, still-present removal, unrecorded spawn, and sequence reuse all fail
  with `SessionFault::ContractInvariant` before publication.
- A spawned-and-removed object absent from both snapshots is still emitted in exact causal order.
- Ordinary damage/fall health-zero cleanup now uses the canonical victory boundary, removes the
  defeated owner's objects once, and surfaces `OwnerEliminated`. Previously only explicit/hard-limit
  elimination performed that cleanup.

### Compatibility boundary

`SIMULATION_VERSION = 6` groups the deterministic gameplay corrections since v5:

1. durations count affected-player turns;
2. Feeding Frenzy affects live primary strikes and consumes no forced-crit RNG draws;
3. ordinary health-zero/fall elimination removes dead-owned persistent objects.

All five golden vectors were regenerated explicitly, with every v5 value retained in source comments:

| Vector | v5 | v6 |
|---|---:|---:|
| all passes | `b75ec70f007a7a7b` | `ecff79397aa402de` |
| walking duel | `0038e5ddfabfec81` | `af6978b06c1f9772` |
| firing duel | `9c53418575ea824d` | `a009c290a796d1ba` |
| mixed actions | `ea50d7336feb3a94` | `c29e2d75ceba7f33` |
| low-health duel | `323672057a1d53af` | `0c908bfce4b927d6` |

Shared direct fixture v6 hashes:

- initial: `f67c5371bcddbdf5`
- after move: `378081bb2e830a5d`
- after ability/final: `d8686762470c0c36`

---

## 5. Deliberate gaps — do not paper over them

C1 is not complete:

1. No producer-owned public record exists for non-strike RNG such as Arzum's chosen teleport target
   or Aleph's drawn teleport point.
2. No read-only trajectory/ability preview DTO or stale-generation refusal path exists.
3. No safe restore API exists. Restore must include the host plus the complete verified ledger and
   byte count; never expose a public `from_host(host)` that silently forgets replay receipts.
4. The composite session/ABI envelope and several direct transition scenarios remain open,
   especially passive selection/chosen, terrain/block change, and full victory attribution.
5. `db-sim-ffi` remains a placeholder handle rather than a real cloned session ABI.

Mechanics gaps outside the immediate provenance slice:

- Finite object lifetimes are not decremented. Do not emit `Expired` until a scheduler-owned,
  explicitly specified lifetime rule exists.
- Object health cannot currently be targeted or damaged. Do not emit `Destroyed` until a real
  damage producer exists.
- Several passive modifiers, gas-cloud/turret behavior, Embers content reachability, and sudden-death
  hazards remain incomplete.
- Numa Pin now lasts two turns of the affected player, independent of lobby size. That semantic rule
  is correct; the numeric value still needs balance confirmation.

Snapshot-derived `ObjectChanged` remains valid for a surviving object's full pre/post mutation, but
no production in-place object mutation exists today. Do not infer lifecycle causes from a diff.

---

## 6. Exact next engineering sequence

Continue C1; do not begin C2 or Godot UI yet.

1. Add a closed, producer-owned non-strike `RandomOutcome` record. Cover Arzum target selection and
   Aleph point draws without exposing private RNG state or pretending forced crits consumed a draw.
   Reconcile purpose, bound/result, affected actor/ability, and destination/target exactly.
2. Complete direct transition scenarios for passive required/chosen, terrain plus block mutation,
   elimination/victory attribution, pass, and authority timeout. Mutation tests must prove omitted or
   tampered producer records fail before host/generation/ledger publication.
3. Implement the read-only preview contract from `CLIENT_SPEC.md`: stale-generation refusal, no state
   mutation, no ledger mutation, and no RNG consumption.
4. Design restore as a verified session-plus-complete-ledger operation with exact retained-byte
   recomputation. Keep test-only constructors private.
5. Finish the composite session/ABI envelope and remaining raw direct fixtures.
6. Only after C1 passes, replace the FFI placeholder with the real cloned-session C ABI (C2).

Likely files for step 1: `types.rs`, `resolve/relocation.rs`, `resolve/mod.rs`, `command.rs`,
`match_host.rs`, and `match_session.rs`. Keep the record ordered with strike/status/object records;
never reconstruct a random choice from post-state.

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
$env:DUNGEON_BARRAGE_GODOT = [Environment]::GetEnvironmentVariable('DUNGEON_BARRAGE_GODOT', 'User')
.\scripts\verify-toolchain.ps1
git status --short --branch
```

Latest measured workspace test inventory for this checkpoint is 508:

- 492 `db-sim-core` unit tests
- 7 golden-vector tests
- 1 shared-fixture test
- 7 `db-sim-ffi` tests
- 1 `db-sim-wasm` test

The final landing must record all gates in `docs/BUILD_LOG.md`. `core.autocrlf` LF-to-CRLF notices are
non-failing warnings; do not normalize the repository to silence them.

---

## 8. Copy-paste Opus resume prompt

```text
Continue Dungeon Barrage from the reviewed C1 checkpoint.

Canonical repo: C:\Users\rsfit\DungeonBarrage
Branch: feat/c1-outcome-provenance tracking origin/feat/c1-outcome-provenance
Run git status --short --branch, git rev-parse HEAD, and git log -5 --oneline --decorate first.
Do not use C:\Users\rsfit\OneDrive\Documents\DungeonBarrage; it is a different empty repo.

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
authority and the future server is Rust-native. Do not start Godot scenes before C1-C3.

Current state: SIMULATION_VERSION=6. Strike/crit, exact status lifecycle, ordered object lifecycle,
ordinary eliminated-owner cleanup, and authority-only timeout are implemented. Status and object
records replay fail-closed. Workspace inventory is 508 tests. Shared v6 hashes are
f67c5371bcddbdf5 -> 378081bb2e830a5d -> d8686762470c0c36.

Next task: add producer-owned non-strike RandomOutcome provenance for Arzum/Aleph relocation draws,
then close remaining direct C1 event scenarios. Do not infer randomness from final state. Expired and
Destroyed object causes are reserved and must remain unreachable until real expiry/damage mechanics
exist. After that, implement read-only preview and verified session-plus-ledger restore.

Run every gate in HANDOFF section 7. Maintain HANDOFF and append BUILD_LOG; leave a clean, committed,
verified tree and report current commit/push state exactly.
```

---

## 9. Landing notes

The owner requested installation verification, documentation, a Git commit, and a push. Commit only
after every gate succeeds. Stage explicit reviewed paths and confirm `.github-token` is absent from
the index. Push with plain `git push` using configured credentials only; never place a token in a
command, URL, environment variable, file, log, or commit metadata.
