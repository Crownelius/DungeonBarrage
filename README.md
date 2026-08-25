# Dungeon Barrage

Turn-based artillery tactics with destructible terrain and a planned 24-character roster.

**Native desktop first.** The presentation client is Godot with C#; its local matches call the
authoritative Rust simulation through a client-only C ABI. The future match server is
Rust-native and will depend on the same core directly. There is exactly one implementation of
the game rules.

## Status

Early. The Rust `MatchHost` is tested end to end: it drives real maps, movement, ability
resolution, destructible blocks, turn rotation, status expiry, and victory through frozen,
versioned golden vectors in the current working tree. The C1 working tree now also has validated
match creation, atomic client snapshots, normalized commands, a generation/idempotency-owning
`MatchSessionHost` with exact entry/byte bounds, ordered transitions, exact terrain dirty row-runs,
and a shared raw JSON duel fixture with frozen direct-Rust hashes.

The game is still **not playable**. Transition provenance and preview remain incomplete,
`db-sim-ffi` does not yet expose a real match, and no C#/Godot client exists. Several mechanics also
remain partial: selected passives are recorded but not all applied, the turret/gas-cloud/Embers
lifecycles are incomplete, and the sudden-death hazard is absent. See
[`docs/CLIENT_SPEC.md`](docs/CLIENT_SPEC.md) §3 and §21 for the ordered gates,
[`docs/HANDOFF.md`](docs/HANDOFF.md) for the exact committed checkpoint and resume state, and
[`docs/BUILD_LOG.md`](docs/BUILD_LOG.md) for append-only engineering history.

## Layout

```
crates/
  db-sim-core/     Authoritative simulation. Rust, forbid(unsafe_code), no floating point.
  db-sim-ffi/      Client-only C ABI for P/Invoke from C#. The ONLY crate permitted `unsafe`.
  db-sim-wasm/     Dormant WASM boundary, retained in case web delivery returns.
docs/              Specifications, architecture decisions, build log.
reference/         Retired implementations, kept out of the build for reference only.
tests/fixtures/    Exact cross-language match request bytes and semantic expectations.
```

## Build

```powershell
.\scripts\verify-toolchain.ps1
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p db-sim-ffi --release
```

The FFI release artifact is a `cdylib` (`.dll` / `.so` / `.dylib`) loaded only by the C#
client. The future Rust-native server will link `db-sim-core` directly.

## Invariants

These are enforced by CI, not by convention. See
[`docs/SECURITY_BASELINE.md`](docs/SECURITY_BASELINE.md) §10.

- **No `unsafe`** anywhere except `db-sim-ffi`, where every block carries a `SAFETY` comment.
- **No floating point** in the authoritative core — results must be bit-identical across
  targets.
- **No ambient nondeterminism** — no wall clock, no OS entropy, no thread scheduling. The
  only randomness is the seeded match PRNG.
- **The client decides nothing.** Damage, terrain, turn order, currency, and ownership are
  server-owned. A native client is no more trusted than a browser one.
- **No secrets in the repository**, enforced by a pre-commit hook and a full-history scan.

## Architecture decisions

| ADR | Decision |
|---|---|
| [0001](docs/adr/0001-rust-wasm-core.md) | Rust simulation core, chosen over C++ for memory safety |
| [0002](docs/adr/0002-character-kits.md) | Character kits replace the three-slot loadout |
| [0003](docs/adr/0003-shared-trig-table.md) | Shared quantized sine table for cross-target determinism |
| [0004](docs/adr/0004-native-desktop-rust-csharp.md) | Native desktop C# client; TypeScript and web delivery removed |
| [0005](docs/adr/0005-destructible-blocks-with-health.md) | Addressable destructible terrain blocks with authoritative health |
| [0006](docs/adr/0006-client-and-server-language-boundaries.md) | Godot/C# presentation client; client-only C ABI; Rust-native future server |

## Contributing

Read [`docs/HANDOFF.md`](docs/HANDOFF.md) and
[`docs/MODULE_OWNERSHIP.md`](docs/MODULE_OWNERSHIP.md) first. They define the current safe resume
point, one-file-per-owner boundaries, and the constraints every module is held to.
