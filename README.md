# Dungeon Barrage

Turn-based artillery tactics with destructible terrain and 24 playable characters.

**Native desktop first.** The authoritative simulation is Rust; the client and match server
are C#, both calling the same Rust core over a C ABI. There is exactly one implementation of
the game rules.

## Status

Early. The simulation core compiles and is tested, but the game is **not playable** — 19 of
22 ability effects have no resolver yet, so 8 of the 9 starter characters do not function.
See [`docs/PROGRAM_PLAN.md`](docs/PROGRAM_PLAN.md) §1–2 for an honest breakdown, and
[`docs/BUILD_LOG.md`](docs/BUILD_LOG.md) for the full engineering history.

## Layout

```
crates/
  db-sim-core/     Authoritative simulation. Rust, forbid(unsafe_code), no floating point.
  db-sim-ffi/      C ABI for P/Invoke from C#. The ONLY crate permitted `unsafe`.
  db-sim-wasm/     Dormant WASM boundary, retained in case web delivery returns.
docs/              Specifications, architecture decisions, build log.
reference/         Retired implementations, kept out of the build for reference only.
```

## Build

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p db-sim-ffi --release
```

The release artifact is a `cdylib` (`.dll` / `.so` / `.dylib`) that the C# client and server
load via P/Invoke.

## Invariants

These are enforced by CI, not by convention. See
[`docs/SECURITY_BASELINE.md`](docs/SECURITY_BASELINE.md) §10.

- **No `unsafe`** anywhere except `db-sim-ffi`, where every block carries a `SAFETY` comment.
- **No floating point** in the authoritative core — results must be bit-identical across
  targets.
- **No ambient nondeterminism** — no wall clock, no OS entropy, no thread scheduling. The
  only randomness is the seeded match PRNG.
- **The client decides nothing.** Damage, terrain, ammunition, currency, and ownership are
  server-owned. A native client is no more trusted than a browser one.
- **No secrets in the repository**, enforced by a pre-commit hook and a full-history scan.

## Architecture decisions

| ADR | Decision |
|---|---|
| [0001](docs/adr/0001-rust-wasm-core.md) | Rust simulation core, chosen over C++ for memory safety |
| [0002](docs/adr/0002-character-kits.md) | Character kits replace the three-slot loadout |
| [0003](docs/adr/0003-shared-trig-table.md) | Shared quantized sine table for cross-target determinism |
| [0004](docs/adr/0004-native-desktop-rust-csharp.md) | Native desktop C# client; TypeScript removed |

## Contributing

Read [`docs/MODULE_OWNERSHIP.md`](docs/MODULE_OWNERSHIP.md) first. It defines one-file-per-owner
boundaries and the constraints every module is held to.
