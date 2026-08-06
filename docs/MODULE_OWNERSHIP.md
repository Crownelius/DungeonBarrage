# Module ownership and the parallel-work protocol

This file exists to prevent the specific failure the product owner named: *"foresee
problems that are caused by lack of communication with the rest of the team."*

Concurrent agents editing shared code is the most reliable way to produce a broken tree.
The protocol below removes the possibility structurally rather than relying on
coordination.

## The rules

1. **One file, one owner.** Every task below owns exactly one file. No task edits a file
   it does not own, for any reason — not to fix a typo, not to add a helper, not to
   "quickly correct" a neighbour.
2. **`types.rs` is the contract and is frozen during parallel work.** All shared data
   structures live there. A module that needs a type reads it; it never redefines one and
   never adds a variant. If a type is genuinely missing, the task stops and reports it
   rather than inventing a local version — two local versions of one concept is exactly
   the drift this protocol prevents.
3. **`lib.rs` is owned by the integrator.** Module declarations are added centrally. A
   task never edits `lib.rs`.
4. **A broken sibling is not your bug.** While work is in flight, `cargo build` may report
   errors in files being written concurrently. Filter to your own file:
   ```
   cargo build 2>&1 | grep <your-file>.rs
   ```
   Fix only what that reports. Do not "helpfully" repair a teammate's half-written module.
5. **Report blockers, do not route around them.** A missing type, an ambiguous spec value,
   or a contradiction between `ARSENAL.md` and `types.rs` is reported upward. Guessing
   produces code that compiles and is wrong, which is more expensive than stopping.

## Ownership

| File | Scope | Assigned |
|---|---|---|
| `fixed.rs` | Fixed-point math primitives | Integrator (complete) |
| `canonical.rs` | Byte encoding + FNV-1a hashing | Integrator (complete) |
| `types.rs` | Shared data contract | Integrator (complete, frozen) |
| `error.rs` | Error types | Integrator (complete) |
| `lib.rs` | Module wiring, versions | Integrator |
| `rng.rs` | Versioned seeded PRNG | Implementation task |
| `terrain.rs` | Occupancy mask, terrain operations | Implementation task |
| `weapon.rs` | Weapon roster data + validation | Implementation task |
| `ballistics.rs` | Trajectory integration + collision | Implementation task |
| `hash.rs` | `Canonical` impls for state types | Implementation task |
| `command.rs` | Command validation + application | Implementation task |

## Constraints binding every module

Enforced by `Cargo.toml` lints and CI (`SECURITY_BASELINE.md` §10). These are not
suggestions; a violation fails the build.

- **No `unsafe`.** Forbidden workspace-wide.
- **No floating point.** `clippy::float_arithmetic` is `deny`. Use `fixed.rs`.
- **No `unwrap`, `expect`, or `panic!`.** Return `SimResult`. Untrusted input must never
  panic the room process.
- **No raw indexing or slicing.** Use `.get()`, `.iter()`, or iterator adapters.
- **Checked or saturating arithmetic**, with the choice justified in a comment. Wrapping
  is never relied upon.
- **No `HashMap` iteration feeding hashed or ordered output.** Sort explicitly, or use
  `BTreeMap`.
- **No wall clock, no ambient randomness, no thread scheduling.** The only entropy is the
  match seed threaded through `rng.rs`.
- **Cosmetic data never affects gameplay or the hash.**

## Parity obligation

`lib/game/simulation.ts` is the reference oracle (ADR 0001). A module that reimplements
oracle behaviour must match it **bit-exactly**, including rounding. Where this port
deliberately diverges — the `main`/`secondary`/`meleeTool` slot rename, and the canonical
encoding replacing `JSON.stringify` — the divergence is recorded in ADR 0001 §5–§6 and the
oracle is updated to match, not the other way round.

Any *other* divergence discovered during implementation is a finding to report, not a
liberty to take.
