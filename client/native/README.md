# Native simulation libraries

One `db-sim-ffi` build per advertised runtime identifier. `NativeLibraryResolver` loads exactly the
directory matching the running process and never falls back to another, so a wrong-architecture or
wrong-platform binary fails at load with a message naming the platform rather than somewhere deeper.

| RID | File |
|---|---|
| `win-x64` | `db_sim_ffi.dll` |
| `linux-x64` | `libdb_sim_ffi.so` |
| `osx-x64` | `libdb_sim_ffi.dylib` |
| `osx-arm64` | `libdb_sim_ffi.dylib` |

Only `win-x64` is populated today: it is the only RID this machine can build and the only one any
gate currently exercises. The other three directories exist so the layout is not invented later,
and an empty one is an honest statement that the target is unbuilt — not a claim it works.

Refresh with:

```
cargo build --release -p db-sim-ffi
```

then copy the artifact from `target/release/` into the matching directory.
