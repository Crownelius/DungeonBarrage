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
2. **`types.rs` is the authoritative simulation contract and is frozen during parallel
   work.** Authoritative gameplay structures live there. Client/session DTOs live in their
   named boundary modules and must not be smuggled into `types.rs` merely because more than
   one adapter consumes them. If an authoritative type is genuinely missing, the task stops
   and reports it rather than inventing a local version — two local versions of one concept
   is exactly the drift this protocol prevents. The integrator may deliberately unfreeze it
   for one coordinated contract slice, such as adding resolver-owned event provenance, after
   stopping parallel writers and updating every producer/consumer and the golden evidence
   together.
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
| `types.rs` | Shared authoritative data contract | Integrator (strike/status/object/random provenance complete; frozen after C1) |
| `error.rs` | Error types | Integrator (complete) |
| `lib.rs` | Module wiring, versions | Integrator |
| `rng.rs` | Versioned seeded PRNG | Implementation task |
| `terrain.rs` | Occupancy mask, terrain operations | Implementation task |
| `character.rs` | Character roster data + validation | Implementation task |
| `ballistics.rs` | Trajectory integration + collision | Implementation task |
| `projectile_mechanics.rs` | Dormant bounded projectile-pattern and environment-interaction kernel; no roster/command wiring | Integrator (OpenBound behavioral research slice complete) |
| `hash.rs` | `Canonical` impls for state types | Implementation task |
| `command.rs` | Command validation + application; ordered action records | Integrator during C1 contract slices |
| `match_host.rs` | Authoritative orchestration; no transport/session policy | Integrator |
| `scheduler.rs` | Phase progression, turn reasons, victory handoff | Integrator |
| `resolve/status.rs` | Status application and affected-player duration transitions | Integrator (current lifecycle slice complete) |
| `resolve/attack_mods.rs` | Attack modifiers, forced-crit charges, effect strikes | Integrator (current provenance slice complete) |
| `resolve/objects.rs` | Turret/knife creation, replacement, cap eviction, detonation | Integrator (current lifecycle slice complete) |
| `resolve/relocation.rs` | Teleport/relocate/obscure; gas-cloud creation and RNG draws | Integrator (producer-owned Arzum/Aleph outcomes complete) |
| `victory.rs` | Victory evaluation and canonical eliminated-owner object cleanup | Integrator |
| `match_setup.rs` | Validated transport-free match construction | Integrator (C1 slice complete) |
| `client_contract.rs` | Engine-neutral read-only snapshot projection | Integrator (C1 slice complete) |
| `match_session.rs` | Normalized commands, generations, bounded idempotency ledger, transitions | Integrator (C1 random outcomes, exact trace/strike replay, preview, checkpoint restore, and direct scenarios complete) |
| `db-sim-ffi/**` | Sole native ABI and `unsafe` boundary | Integrator (C2 real session ABI, required-nullable clock fields, and pointer/ownership contract complete; C3 consumes it without adding rules) |
| `tests/fixtures/matches/**` | Shared machine-readable client fixtures | Integrator (direct requests plus byte-exact production C2 responses); schema changes require review |
| `crates/db-sim-core/tests/shared_match_fixtures.rs` | Strict direct-session consumer of shared bytes | Integrator (C1 replay plus preview and C2 manifest paths complete) |
| `crates/db-sim-core/tests/golden_vectors.rs` | Versioned whole-match replay hashes | Integrator; regenerate only under the documented compatibility procedure |
| `docs/HANDOFF.md` | Mutable operational handoff for the next agent | Documentation task |
| `docs/BUILD_LOG.md` | Append-only historical checkpoints | Integrator appends; existing entries are never rewritten |

## Constraints binding every module

Enforced by `Cargo.toml` lints and CI (`SECURITY_BASELINE.md` §10). These are not
suggestions; a violation fails the build.

- **No `unsafe` in authoritative or presentation-independent Rust.** It is forbidden by
  workspace default. `db-sim-ffi` is the one explicit crate-level exception and must keep
  every unsafe function contract and unsafe block documented; CI must reject `unsafe`
  anywhere else.
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

The TypeScript reference oracle was retired with the web surface (ADR 0004). There is no
second implementation to check against, so the obligation changed shape rather than
disappearing.

**Frozen golden vectors** replace cross-implementation parity: seeded command sequences and
their state hashes, committed and asserted in CI. A module must not change a committed
vector. If a change is genuinely correct and the vector is genuinely wrong, regenerate it in
a separate, clearly-labelled commit that says why — never fold a vector change into a
feature commit, because that is indistinguishable from silently breaking determinism.

The corpus freezes whatever it is given, including bugs. It is only as good as the review
that preceded generation.
