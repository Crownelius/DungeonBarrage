# ADR 0001: Rust/WebAssembly simulation core

**Status:** Accepted (2026-08-06)
**Supersedes:** `PLATFORM_STRATEGY.md` §12 "Rust/WASM decision"
**Decided by:** Product owner directive — "utilize webassembly so that the source code can be mostly C++ or Rust"

## Context

`PLATFORM_STRATEGY.md` §12 previously advised *against* a Rust core, on the grounds that
portability was hypothetical and dual implementations were a maintenance risk. That advice
assumed console work was unfunded and that TypeScript performance was adequate.

The product owner has directed that the codebase be mostly Rust or C++, compiled to
WebAssembly. That is an explicit scope decision, not a hypothetical. This ADR records the
pivot and the constraints that keep it from becoming the risk §12 warned about.

## Decision

### 1. Rust, not C++

| Criterion | Rust | C++ / Emscripten |
|---|---|---|
| Memory safety in the authoritative path | Guaranteed absent `unsafe` | Manual; UB is exploitable |
| WASM toolchain | First-class (`wasm32-unknown-unknown`, `wasm-bindgen`) | Emscripten, heavier runtime |
| Deterministic integer math | `i32`/`i64` with explicit wrapping semantics | Implementation-defined edges |
| Native server build | Same crate, `cargo build` | Separate build system |
| Supply-chain audit | `cargo audit`, `cargo deny` | Ad hoc |
| Console portability | Tier-2/3 targets exist | Viable but no safety benefit |

The product owner's stated top priority is *"This should not create ANY security vulnerabilities
for the user, or the host."* An authoritative simulation that parses untrusted network commands is
exactly where memory-unsafety becomes remote code execution. Rust removes that class of bug by
construction. C++ is therefore rejected.

**The core crate denies `unsafe` outright** (`#![forbid(unsafe_code)]`). No exceptions in
`db-sim-core`; any `unsafe` needed at the WASM or FFI boundary lives in a separate, minimal,
individually reviewed crate.

### 2. Port against an oracle — do not rewrite

`lib/game/simulation.ts` (1080 lines) is a working, tested, fixed-point deterministic simulation.
It is **not** deleted. It is frozen as the **reference oracle**.

Every Rust module must produce byte-identical results to its TypeScript counterpart for the same
inputs, proven by a differential test harness (`tests/parity/`). The TS implementation is retired
only after the Rust core passes parity on the full golden corpus, and even then it is retained in
`reference/` for future differential work.

This converts "rewrite the engine and hope" into "port with a continuously checked equivalence
proof." It is the single most important risk control in this ADR.

### 3. One simulation, three consumers

```
                 db-sim-core  (Rust, no_std-compatible, forbid(unsafe_code))
                       │
      ┌────────────────┼────────────────────┐
      │                │                    │
  wasm32 build     native build        native build
  (browser client) (match server)      (future console)
```

There is never a second implementation of the game rules. The client and server run the *same
compiled logic*, differing only in target triple. This directly answers §12's "maintaining two
simulation implementations becomes a demonstrated risk" concern — we are removing the second
implementation, not adding one.

### 4. Determinism contract

Binding rules for every line of code in `db-sim-core`:

- **No floating point** in any authoritative path. Not `f32`, not `f64`, not as an intermediate.
  Fixed-point `i32`/`i64` only, with `POSITION_SCALE = 1024`.
- **No `HashMap` iteration** in hashed or ordered output. Use `BTreeMap`, or sort explicitly.
- **No wall-clock, no thread scheduling, no ambient randomness.** The only entropy source is the
  match seed, threaded explicitly through a versioned PRNG.
- **Explicit overflow semantics.** Arithmetic that can overflow uses `checked_*` /
  `saturating_*` and states which. Release-mode wrapping is never relied upon.
- **Canonical byte encoding for hashing.** See below.

### 5. Canonical encoding replaces `JSON.stringify`

The current TS state hash serializes with `JSON.stringify`. That is a **latent parity bug**: JSON
number formatting, key escaping, and Unicode handling are JavaScript-engine semantics that Rust
will not reproduce by accident.

Both sides therefore implement an explicit, versioned, length-prefixed binary encoding
(`CANONICAL_ENCODING_VERSION`) before FNV-1a hashing:

- Integers: fixed-width little-endian, explicit width per field.
- Strings: `u32` byte length prefix + UTF-8 bytes. No escaping.
- Collections: `u32` count prefix, entries sorted by a stated key.
- Terrain: dimensions, then raw cells.

The TypeScript oracle is updated to the same encoding so parity is testable. This is a
hash-format change; `SIMULATION_VERSION` increments accordingly. No completed matches exist, so
there is nothing to invalidate.

### 6. Slot naming corrected during the port

The TS code uses `["main", "offHand", "melee"]`; `PRODUCT_SPEC.md` §3 mandates
`main | secondary | meleeTool`. The spec governs. The Rust port adopts the spec names, and the
TS oracle is updated in the same change so parity is preserved.

### 7. Mode-parameterized scheduler (Brawlhalla-mode readiness)

A second real-time PvP mode is a stated product requirement. The core is therefore split so that
the expensive, shared parts are written once:

| Layer | Turn-based artillery | Real-time platform fighter |
|---|---|---|
| Fixed-point math, terrain mask, collision | **shared** | **shared** |
| Damage, knockback, hazards, elimination | **shared** | **shared** |
| Weapon behavior vocabulary | **shared** | **shared** |
| Scheduler | turn state machine, AP budget | 60 Hz input tick, per-frame inputs |
| Command envelope | one committed action per turn | input frame stream |

The scheduler is a trait (`MatchScheduler`); modes are implementations. This is a *structural*
provision made now because retrofitting it later would mean re-deriving collision and damage.
The real-time mode is **not** implemented until the turn-based vertical slice ships.

## Consequences

**Accepted costs**
- Build complexity: `wasm-pack` + `cargo` join the Node toolchain; CI must build both.
- Two languages in the repo during the port window.
- WASM payload adds to the initial-download budget (see mitigation).

**Guardrails**
- The parity harness is a merge gate. A Rust module that diverges from the oracle cannot land.
- `forbid(unsafe_code)` in the core is a merge gate.
- `cargo deny` (licenses, advisories, duplicate versions) is a merge gate.
- WASM binary size is budgeted at **≤ 400 KB compressed** for the core, inside the existing
  "under 3 MB compressed code" budget in `PLATFORM_STRATEGY.md` §15. `wasm-opt -Oz`, `panic=abort`,
  `lto=true`, `codegen-units=1`, and `strip` are mandatory in release.
- The TS simulation is not deleted until parity is green on the full corpus.

**Rejected alternatives**
- *Keep TypeScript only* — contradicts an explicit product-owner directive.
- *C++/Emscripten* — no memory-safety guarantee, which conflicts with the stated top priority.
- *Rewrite without an oracle* — discards a working tested implementation and its determinism
  properties for no benefit.

## Revisit if

- WASM cold-start or payload measurably harms first-play time beyond the §15 budget.
- A console platform is confirmed and its toolchain rejects the Rust targets in use.
