# Dungeon Barrage

Turn-based artillery tactics with destructible terrain and a planned 24-character roster.

**Native desktop first.** The presentation client is Godot with C#; its local matches call the
authoritative Rust simulation through a client-only C ABI. The future match server is
Rust-native and will depend on the same core directly. There is exactly one implementation of
the game rules.

## Status

The C0 local toolchain gate, C1 Rust transition contract, and C2 client-only C ABI are complete at
this checkpoint. `MatchSessionHost` owns validated match creation, atomic snapshots and commands,
bounded idempotent receipts, authority-only timeouts, read-only previews, complete in-process
checkpoint/restore, and producer-owned random/strike/status provenance with detached exact replay.
The real `db-sim-ffi` adapter owns a session behind a poisonable serialized handle and exposes the
ten frozen ABI-version-1 exports with strict bounded JSON, Rust-owned buffers, and exact
cross-boundary request/response fixtures. The current workspace has 530 passing Rust tests, plus
exact Windows/Linux export and Valgrind ownership gates.

The game is still **not playable** because C3's Godot-free C# interop/session layer and the Godot
presentation project do not yet exist. Several mechanics also remain deliberately partial: Arzum's
rated Chain Strike second hit awaits an owner decision; selected passives are not all applied; the
turret/gas-cloud/Embers lifecycles are incomplete; and the sudden-death hazard is absent. See
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
tests/fixtures/    Exact cross-language requests, responses, and semantic expectations.
```

## Build

```powershell
.\scripts\verify-toolchain.ps1
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --release -p db-sim-ffi --locked
cargo build --release -p db-sim-ffi --locked
cargo deny check
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
