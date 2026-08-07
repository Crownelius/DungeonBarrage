# ADR 0004: Native desktop client in C#; TypeScript removed entirely

**Status:** Accepted (2026-08-06)
**Supersedes:** `PLATFORM_STRATEGY.md` §1, §3, §9, §10, §13 · `adr/0001-rust-wasm-core.md` §2 (the oracle strategy)
**Amends:** `adr/0001-rust-wasm-core.md` §3 (build targets) · `adr/0003-shared-trig-table.md` (rationale, not the decision)
**Decided by:** Product owner directive — "redo everything in C# and Rust, not TypeScript" and "native desktop first, drop web delivery"

## Context

Two decisions arrived together and interact:

1. **No TypeScript.** The rationale given: writing a game engine in TypeScript is the wrong
   tool. The engine is in fact already Rust — `db-sim-core` is 5,019 lines — so this
   resolves to removing the TypeScript that surrounds it.
2. **Native desktop first, web delivery dropped.** This reverses `PLATFORM_STRATEGY.md` §1,
   which made a desktop website and installable PWA the canonical surface, with Steam and
   console as later adapters.

C# does not run in a browser without a game-engine runtime, so these two decisions are not
independent: choosing C# for the client and keeping a web-first strategy would have forced
Unity or Godot web export, at 5–20 MB compressed against a stated ≤15 MB total budget
(`PLATFORM_STRATEGY.md` §15). Dropping web delivery removes that conflict rather than
absorbing it.

## Decision

### Language boundary

| Layer | Language | Rationale |
|---|---|---|
| Authoritative simulation | **Rust** | Already built. `forbid(unsafe_code)`, no floating point, deterministic. Unchanged by this ADR. |
| Client — render, input, audio, UI | **C#** | Product-owner directive. Calls the Rust core over a C ABI. |
| Match server | **C#** (ASP.NET Core) | Calls the *same* Rust core. One implementation of the rules, two hosts. |
| Persistence | **C#** (EF Core) | Replaces Drizzle/D1, which was a web-platform choice. |
| Tooling and content validation | **Rust** or **C#** | No TypeScript. |

**There is still exactly one implementation of the game rules.** Client and server both
P/Invoke into `db-sim-core`. That property was the point of ADR 0001 and it survives intact
— arguably more cleanly, since a native `cdylib` is a more direct sharing mechanism than a
WASM module plus a native build.

### Client framework: Godot 4 with C#

Recommended and adopted, with the alternative recorded:

| | Godot 4 + C# | MonoGame | Unity |
|---|---|---|---|
| Editor, scene tree, UI system | included | write it yourself | included |
| 2D suitability | strong | strong | good |
| Desktop export | first-class | first-class | first-class |
| Licensing | MIT | MS-PL | commercial terms |
| Console path | third-party porting (W4) | consultation | first-party |

Godot is adopted because the project already owns its simulation and needs a *shell* —
rendering, input, audio, HUD, asset pipeline — not another physics engine. Godot supplies
that shell and its physics is simply not used, which is a clean fit. MonoGame remains a
reasonable fallback if Godot's C# tooling proves obstructive; the Rust boundary is
identical either way, so switching costs the shell, not the game.

**Godot's physics, collision, and RNG are never used for gameplay.** They are not
deterministic to the standard in ADR 0001 §4. The Rust core owns every authoritative value.

### Build target change

`adr/0001-rust-wasm-core.md` §3 targeted `wasm32-unknown-unknown` for the browser client.
That target is now **optional and unused**. The primary artifact is a native `cdylib`
(`.dll` / `.so` / `.dylib`) exposing a C ABI for P/Invoke. The `db-sim-wasm` crate is
retained but dormant — it costs nothing and preserves the option if web delivery ever
returns.

### The TypeScript oracle is retired — and what that costs

This is the load-bearing consequence and it is a genuine loss, not a formality.

ADR 0001 §2's central risk control was that `lib/game/simulation.ts` stayed frozen as a
reference oracle, and that a differential harness proved the Rust port matched it
bit-exactly. **That harness was never built** (it was the unmet M1 gate), and deleting the
oracle means it never will be. The Rust port's faithfulness to the original will not be
proven.

Accepted for three reasons:

1. The oracle was itself unvalidated — an implementation written in an earlier session, not
   a battle-tested system.
2. It had a **known determinism defect**: `Math.sin`/`Math.cos`, which ECMA-262 permits to
   vary by engine (ADR 0003). Conforming to it would have meant reproducing a bug.
3. Keeping it directly contradicts the no-TypeScript directive.

**Replacement control.** Cross-implementation parity is replaced by **frozen golden
vectors**: seeded command sequences and their resulting state hashes, generated from the
Rust core, committed, and asserted in CI. This proves *self-consistency* — that a refactor
cannot silently change behavior — but it does **not** prove correctness against an
independent implementation. That is strictly weaker, and the difference should not be
papered over. The golden corpus must therefore be generated only from a core whose
behavior has been reviewed, because it freezes whatever it is given, including bugs.

ADR 0003's shared sine table is **retained**. Its parity rationale is now moot, but its
determinism rationale is not: the table still guarantees identical results across
`x86_64`, `aarch64`, and any future console target.

### What is deleted

The entire web surface: the vinext/Next.js shell, the Cloudflare Worker, the PWA manifest
and service-worker plan, ChatGPT/workspace identity headers, the Drizzle/D1 schema, the
Vite/PostCSS/ESLint chain, the React canvas client, and the TypeScript simulation.

`PLATFORM_STRATEGY.md` §10 (Chrome Manifest V3) is **dead**, not deferred — it presupposed
a web client to companion.

## Consequences

**Lost.** Link-sharing onboarding, which `PLATFORM_STRATEGY.md` §3 rated the web build's
single largest advantage — a player can no longer try the game by clicking a URL. Instant
patch delivery is replaced by store update cadence. The web build's zero-install funnel is
gone, and with it the cheapest possible playtest loop.

**Gained.** No payload or cold-start budget. Full GPU access. No browser compatibility
matrix. A direct console path, since a native C# client ports far more plausibly than a
browser application. Steam moves from a post-MVP option (§11) to the primary distribution
channel.

**Unchanged — and this is worth stating.** `SECURITY_BASELINE.md` needs no revision. The
trust boundary was already drawn with the client entirely untrusted (§2), so a native
client is neither more nor less trusted than a browser one. Memory editing, packet forging,
and modified binaries were always in scope; the server already owns every outcome. A design
that had leaned on browser sandboxing would have needed rewriting here. This one does not.

`CHARACTERS.md`, `ARSENAL.md`, and `PROGRESSION.md` are unaffected — they are game design,
not platform.

**Risk.** Playtest feedback gets materially more expensive to collect. The web build's value
was never mainly technical; it was that a tester could be playing in ten seconds. Budget for
this deliberately — a signed desktop build with an easy distribution path to testers is now
a milestone requirement, not an afterthought.

## Rejected alternatives

- **Godot/Unity web export** — 5–20 MB against a ≤15 MB budget, with cold-start times that
  would have failed the §15 performance gates outright.
- **Keep the web client alongside a desktop one** — two clients against one server doubles
  the integration surface before either is proven.
- **Keep the TS oracle purely as a test fixture** — contradicts the directive, and its
  value was already undermined by its own determinism defect.
