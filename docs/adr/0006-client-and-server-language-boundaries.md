# ADR 0006: C# presentation client and Rust-native match server

**Status:** Accepted (2026-08-14)

**Amends:** `adr/0004-native-desktop-rust-csharp.md` (language boundary, client
framework, match-server language, and build targets)

**Preserves:** `adr/0001-rust-wasm-core.md` (one deterministic Rust simulation)

**Decided by:** Technical re-evaluation requested by the product owner before client
implementation

## Context

ADR 0004 made two decisions at once: use Godot 4 with C# for the native desktop
client, and use ASP.NET Core with P/Invoke for the future authoritative match server.
That decision removed TypeScript, but it did not separately evaluate the language
best suited to each remaining process.

The repository is still at the point where changing this boundary is inexpensive:

- `db-sim-core` is the existing, tested Rust authority.
- `db-sim-ffi` is a small placeholder. It exports version queries, a placeholder
  handle, and a state-hash call, but no real `MatchHost` gameplay API.
- No client project, match server, persistence service, or production network
  protocol exists.

The expensive client work ahead is not authoritative simulation. It is scene and
asset iteration, 2D rendering, HUD and menu construction, controller focus, audio,
animation, accessibility, and export packaging. The future server has the opposite
shape: it owns untrusted command validation and hosts the Rust simulation, but has no
need for a game editor or presentation framework.

The language decision must therefore optimize each process for its actual job rather
than minimize the number of languages at any cost.

## Decision

### 1. The presentation client is Godot 4.7.1 .NET with C# targeting .NET 10

The native desktop client uses:

- **Godot 4.7.1 .NET**, pinned exactly for the editor, NuGet SDK, export templates,
  and CI image.
- **C# targeting `net10.0`**, with the .NET SDK pinned by `global.json`.
- Godot's dedicated 2D renderer, scene system, `Control` UI system, input map,
  animation, audio, and asset pipeline.
- The Compatibility renderer as the initial baseline unless a measured visual
  requirement needs a different renderer; Godot recommends it as a starting point
  for 2D games and broad hardware coverage.

C# owns presentation only: input collection, local previews, rendering, timeline
playback, audio, menus, settings, accessibility, and platform UI. It does not decide
damage, collision, ammunition, terrain, turn order, rewards, or victory.

Godot officially supports C# on Windows, Linux, and macOS. Rust is available in Godot
through a community-maintained GDExtension binding rather than an officially
supported scripting language. The client favors the supported, editor-integrated
path because its dominant cost is presentation iteration, not simulation throughput.

The previous `.NET 8` target is retired. As of this decision, .NET 8 support ends on
2026-11-10, while .NET 10 is an active LTS release supported through 2028-11-14.
Godot permits a project to target a newer .NET framework than the minimum targeted by
its GodotSharp packages.

### 2. The authoritative simulation remains Rust and engine-independent

`db-sim-core` remains the only implementation of gameplay rules. It imports no
Godot, .NET, renderer, network transport, database, or platform SDK types.

Determinism comes from the Rust core's integer-only state transition rules, seeded
PRNG, canonical encoding, and golden vectors. The presentation language neither adds
nor weakens determinism as long as it sends quantized intents and never reimplements
rules.

Client and server artifacts are built from the same Rust source and simulation
version. They are not "literally the same compiled code": different operating systems,
architectures, and build roles necessarily produce different binaries.

### 3. The C ABI exists for the client only and is deliberately coarse

The Godot/C# client calls the local Rust core through `db-sim-ffi`. The future match
server does not.

The ABI exposes operations at match granularity:

- create or destroy a match;
- submit one complete, quantized intent;
- read one bounded snapshot or one complete command outcome;
- read version and state-hash information.

It must not expose a chatty API that crosses the boundary once per terrain cell,
render node, particle, or simulation tick. Variable-sized results use a two-call
size/query plus caller-owned-buffer pattern, or another equally bounded convention.
Every input length and output capacity is explicit.

The ABI has its own `ABI_VERSION`, independent from `SIMULATION_VERSION`,
`PROTOCOL_VERSION`, and `CONTENT_VERSION`. A local match checks ABI, simulation, and
content compatibility before creating a handle. `PROTOCOL_VERSION` is not an FFI export;
it belongs only to the future client/server handshake.
Flat structs use fixed-width integer fields and an explicitly tested layout. No Rust
enum layout, `bool`, borrowed pointer, collection, allocator-owned object, or floating
point value crosses the boundary.

The C# binding uses source-generated `[LibraryImport]` where supported and a
`SafeHandle` returned directly by the create function. Native declarations remain in
one Godot-free assembly so the exact shipped library can be tested under xUnit without
starting the editor.

### 4. The future authoritative match server is Rust-native

ADR 0004's ASP.NET Core match-server decision is superseded. When online play is
built, a Rust server crate depends directly on `db-sim-core` as a normal Cargo
dependency.

This removes an unnecessary native boundary from the most security-sensitive and
availability-sensitive process. The server gains compile-time access to core types,
shares the golden-vector suite, and ships without a .NET runtime plus a per-platform
Rust dynamic-library sidecar.

The Rust server choice does not prescribe a transport framework yet. HTTP/WebSocket
framework and deployment topology are selected during the online milestone, when
load, hosting, and observability requirements are measurable.

### 5. Online messages use versioned, language-neutral wire DTOs

The C# client and Rust server do not serialize internal `db-sim-core` structs as the
network protocol. A separate wire schema defines commands, receipts, events,
snapshots, reconnect payloads, and errors.

Wire DTO rules are:

- include `protocolVersion`, `simulationVersion`, and `contentVersion` where needed;
- use fixed-width quantized integers for gameplay values;
- bound all strings, collections, terrain payloads, and timeline samples;
- distinguish a rejected player command from a server or protocol fault;
- carry command IDs, expected state/turn versions, event sequence numbers, and final
  state hashes;
- generate or mechanically validate Rust and C# representations from one schema;
- never make wire serialization the canonical state-hash encoding.

The concrete codec is intentionally deferred until the online milestone. Selecting
JSON, Protocol Buffers, FlatBuffers, or another codec before there is a remote-match
slice would add dependencies without resolving a present constraint. The schema and
compatibility rules are mandatory; the encoding is replaceable.

### 6. Web and funded console delivery are explicit revisit gates

This decision assumes native PC/Steam first.

**Web gate.** Godot 4 C# projects currently cannot export to the web. If playable web
delivery becomes a requirement, stop before expanding the client and re-evaluate the
presentation stack. Candidate paths include a Godot build using a web-capable
language/extension boundary or a Rust-native web-capable engine. A second production
client is not added silently.

**Funded console gate.** Godot console delivery requires licensed third-party
middleware or a contracted port. W4 currently describes C# support as beta for
Nintendo Switch and Xbox Series and asks teams to contact it for other platforms.
If console-at-launch becomes funded or contractually required, obtain written
middleware confirmation for every target, C# runtime, and native Rust plugin before
substantial client implementation. Re-evaluate Unity, MonoGame, and the contracted
Godot path using actual commercial terms and target support.

An aspirational future console release does not justify giving up Godot's current 2D
and UI workflow. A funded simultaneous console requirement would.

## Consequences

### Required before adding gameplay exports to `db-sim-ffi`

At decision time the release profile used `panic = "abort"`, while `db-sim-ffi`
claimed that `catch_unwind` converted Rust panics to `INTERNAL_PANIC`. The first
implementation slice resolved that contradiction: the native release profile now uses
`panic = "unwind"`, and a controlled release-profile test proves the common FFI guard
returns `INTERNAL_PANIC` for an unwinding panic.

C2 must keep every fallible gameplay export behind that guard and add per-handle
poisoning: after a caught panic, only destroy may touch the handle. CI must run the FFI
tests in release mode, not merely build the dynamic library. The contract does not claim
to catch process aborts, allocation failure, stack overflow, or external termination.

The future Rust match server still needs an explicit containment design. Direct Rust
linkage removes FFI risk, but an abort or process-wide failure is not match-local. If the
server promises per-match containment, its shipped profile and worker boundary must prove
that behavior before online play.

### Accepted costs

- The client build uses Cargo, .NET, and Godot toolchains.
- Every supported client architecture needs a matching native Rust library and an
  export-packaging test.
- C# and Rust representations meet at a reviewed ABI locally and a reviewed wire
  schema online.
- Godot C# development uses an external IDE; Godot's built-in C# editor support is
  intentionally minimal.
- Web export remains unavailable under the adopted client stack.
- Console support remains a commercial gate rather than a portability promise.

### Gained

- The presentation client uses an officially supported language and a mature 2D/UI
  editor workflow.
- The authoritative server has no P/Invoke layer, duplicated native lifetime model,
  or Rust DLL discovery problem.
- The C ABI is smaller and lower-frequency because it serves only local client use.
- Server tests can drive `MatchHost` directly and share Rust fixtures and golden
  vectors.
- No existing client or server code must be migrated; both were unimplemented when
  this decision was accepted.

### Build and test implications

- Pin Godot 4.7.1, the `Godot.NET.Sdk` version, .NET 10 SDK, Rust toolchain, and each
  native target triple.
- Test the interop assembly against the real release library, not a managed mock.
- Gate repeated create/destroy, missing-library behavior, version mismatch, short
  buffers, invalid UTF-8, struct sizes/offsets, and deterministic golden replay.
- Export and boot a clean packaged build for each supported OS/architecture; a build
  is not accepted merely because it runs from the repository.
- Keep Godot scene/integration tests separate from the headless interop and protocol
  suites.

## Considered alternatives

### Keep C# for both client and match server

Rejected for the match server. ASP.NET Core is capable, but it would force the
authoritative service through an unsafe C ABI, duplicate all simulation-facing data
layouts, and deploy two runtimes without a project-specific benefit. No ASP.NET code
exists, so there is no migration cost to protect.

### Use Rust with Bevy for the entire client

Rejected for the first production client. Direct core access and one-language testing
are attractive, but Dungeon Barrage is UI- and asset-workflow-heavy. Bevy 0.19 still
publishes an explicit stability warning: important features and documentation remain
incomplete, breaking releases arrive frequently, and its own project recommends
Godot for a more feature-complete and stable large project. Eliminating the local ABI
does not offset owning more editor, UI, accessibility, and content tooling.

### Use Rust through Godot GDExtension

Rejected as the default presentation language. It would allow a direct Cargo
dependency on `db-sim-core`, but godot-rust is community-developed, has occasional
breaking API changes, and adds a second compatibility lifecycle alongside Godot.
It remains a viable revisit if profiling proves the coarse C# ABI is a real bottleneck
or the team deliberately accepts the binding risk for an all-Rust client.

### Use GDScript with a Rust extension

Rejected. GDScript is tightly integrated and excellent for prototypes, but the
project would still need a native Rust/C++ extension bridge. C# provides a more direct,
officially supported route to the existing C ABI plus stronger headless interop and
wire-contract testing.

### Use MonoGame with C#

Rejected as the default; retained as a fallback. MonoGame is a mature 2D framework
with broad platform support, but it provides a lower-level game loop and content
pipeline rather than Godot's scene, layout, theme, animation, and UI authoring
workflow. Choosing it means building substantially more presentation tooling.

### Use Unity with C#

Rejected under the current PC-first scope. Unity has the strongest commercial editor
and console path among the evaluated options, but adds proprietary terms,
subscription thresholds, and a larger engine surface that this custom-simulation 2D
game does not currently need. Reconsider it at the funded console gate, using then-
current licensing and platform access rather than today's assumptions.

### Use C++ for the client or simulation

Rejected. The presentation shell does not need C++ performance, and authoritative
simulation intentionally uses safe Rust to exclude memory-unsafety from untrusted
command processing.

## Primary sources checked for this decision

- Godot 4.7.1 release archive: <https://godotengine.org/download/archive/>
- Godot C# platform support and web limitation:
  <https://docs.godotengine.org/en/4.7/tutorials/scripting/c_sharp/index.html>
- Godot's officially supported and community-provided languages:
  <https://docs.godotengine.org/en/4.7/tutorials/scripting/other_languages.html>
- Godot 2D feature documentation:
  <https://docs.godotengine.org/en/4.7/tutorials/2d/index.html>
- Godot UI feature documentation:
  <https://docs.godotengine.org/en/4.7/tutorials/ui/index.html>
- Godot renderer guidance:
  <https://docs.godotengine.org/en/4.7/tutorials/rendering/renderers.html>
- Godot guidance on targeting newer .NET versions:
  <https://godotengine.org/article/godotsharp-packages-net8/>
- Microsoft .NET support lifecycle:
  <https://dotnet.microsoft.com/en-us/platform/support/policy>
- Microsoft native interop guidance:
  <https://learn.microsoft.com/en-us/dotnet/standard/native-interop/best-practices>
- Rust FFI panic and unwinding behavior:
  <https://doc.rust-lang.org/nomicon/ffi.html>
- godot-rust status and compatibility policy:
  <https://godot-rust.github.io/book/index.html>
  and <https://godot-rust.github.io/book/toolchain/compatibility.html>
- Bevy's current setup and stability warning:
  <https://bevy.org/learn/quick-start/getting-started/>
  and <https://bevy.org/learn/quick-start/introduction/>
- MonoGame platform support:
  <https://docs.monogame.net/articles/getting_started/platforms.html>
- Godot console model and W4 C# status:
  <https://godotengine.org/consoles/> and <https://www.w4games.com/w4consoles>
- Unity 6 support and current pricing model:
  <https://unity.com/releases/unity-6/support>
  and <https://unity.com/products/pricing-updates>

## Revisit if

- playable web delivery returns;
- a simultaneous console release is funded or contractually required;
- W4 or another licensed provider materially changes verified C# and native-plugin
  support for the target consoles;
- a measured interop profile shows the coarse client ABI is a material frame-time or
  iteration bottleneck;
- the project intentionally accepts Bevy's stability and tooling costs to become an
  all-Rust client; or
- Godot drops support for a required desktop platform or .NET runtime.
