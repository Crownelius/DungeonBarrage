# Agent rules

The Rust simulation is the authority. `db-sim-core` is the only implementation of roster, legal actions, action economy, collision, ballistics, terrain, damage, displacement, gauge, elimination, hashes, and replay. The Godot C# client presents that result. It does not decide it. `db-sim-ffi` is the only crate that may use `unsafe`, and it is the only bridge the client calls. Do not reimplement rules in C#.

## Scope

One local turn-based duel. The roster is the four launch characters the simulation already owns: Leslie, Crow, Erus, and Kreena. Each has two unlimited weapons plus a charge-gated special, expanded by the simulation from `characterId`. Play is one local human against one CPU opponent. There is no hot-seat mode, weapon loadout screen, progression, or online play. Do not add them, and do not restore the retired ammunition picker.

## Build and test

Toolchain: Rust 1.94.0 (`rust-toolchain.toml`) and .NET SDK 10.0.302 (`global.json`). These commands were run on Linux against `0174bc8f` and passed:

```bash
cargo test --workspace --locked
cargo test --release -p db-sim-ffi --locked
cargo build --release -p db-sim-ffi --locked
dotnet restore client/DungeonBarrage.sln --locked-mode
dotnet build client/DungeonBarrage.sln -c Release --no-restore
dotnet test client/DungeonBarrage.sln -c Release --no-build
```

`dotnet test` needs the release `libdb_sim_ffi.so` (or the platform equivalent) from the FFI build above. The Godot 4.7.1 .NET editor is not part of CI.

## Change control

- One issue, one branch, one pull request.
- Never touch `.github-token`.
- Never merge to `main`.
- CI must pass before a pull request is treated as done.
- Do not edit GitHub Actions to make a change look green, and do not add deploy or publish configuration.
