# ADR 0003: A shared quantized trigonometry table

**Status:** Accepted (2026-08-06)
**Blocks:** Milestone M1 — TS↔Rust differential parity
**Related:** [adr/0001-rust-wasm-core.md](./0001-rust-wasm-core.md) §4

## Context

ADR 0001 §2 makes the TypeScript simulation a reference oracle: the Rust port must match
it **bit-exactly**, proven by a differential harness that gates merges.

Implementing `ballistics.rs` surfaced a case where that is structurally impossible as
written. `lib/game/simulation.ts` computes launch velocity with:

```js
Math.cos(radians)   // f64
Math.sin(radians)   // f64
```

The Rust core forbids floating point entirely (ADR 0001 §4, `clippy::float_arithmetic =
deny`), because bit-identical results across `wasm32`, `x86_64`, and `aarch64` are the
whole basis of the determinism contract.

Two independent problems, either of which alone is disqualifying:

1. **No fixed-point computation reproduces `Math.sin`/`Math.cos` bit-exactly.** This is not
   a matter of using more terms or a wider intermediate. The functions are defined over
   the reals and rounded to `f64`; a fixed-point evaluation is a different function.
2. **`Math.sin`/`Math.cos` are not bit-identical across JavaScript engines to begin with.**
   ECMA-262 explicitly permits implementation-dependent approximation for the
   transcendental functions. The oracle is therefore not even self-consistent across the
   browsers the game must support — two players on different engines could compute
   different trajectories from the same command.

The second point is the more important finding. The oracle's trig was *already* a latent
determinism defect in the TypeScript-only design; the Rust port did not create it, it
exposed it.

## Decision

**Both implementations use the same quantized integer sine table. Neither calls a
transcendental function on an authoritative path.**

- `ballistics.rs` ships `SINE_TABLE`: 361 entries, Q16 fixed-point, whole-degree
  resolution, with linear interpolation for sub-degree angles. Generated offline and
  committed as integer constants, not computed at startup.
- `lib/game/simulation.ts` is updated to use the **same 361 integer constants** and the
  same interpolation, replacing its `Math.cos`/`Math.sin` calls.
- The table is versioned with `SIMULATION_VERSION`. Changing an entry changes every
  trajectory and therefore every historical replay.

This makes the two implementations agree by *construction* rather than by approximation.
It is the only arrangement in which the differential harness can be meaningfully green.

## Consequences

**The oracle changes.** ADR 0001 §2 states the oracle is authoritative and the port
conforms to it. This is a deliberate, recorded exception: where the oracle's behaviour is
itself non-deterministic, conforming to it would mean reproducing a defect. `MODULE_OWNERSHIP.md`
already reserves exactly this — divergences are recorded in an ADR and the oracle is
updated, not silently worked around.

**Precision is bounded and stated.** Linear interpolation between whole-degree entries is
not "true" sine. The error is under 0.1% of the unit launch vector. For a game where
angle arrives quantized in millidegrees from a slider and the projectile then interacts
with a discrete terrain mask, this is far below the resolution any player can perceive or
exploit. Accuracy was never the property at stake — agreement was.

**Until the oracle is updated, the harness will show bounded deviation** on any
non-axis-aligned launch angle. That is expected and is not a Rust-side bug. M1's gate is
not met until the oracle change lands.

## Rejected alternatives

- **Reproduce `Math.sin` in fixed point.** Impossible, per §1 above.
- **Allow floating point in the Rust core for trig only.** Trades the entire determinism
  contract for one function, and does not even fix the problem — the JS side would still
  vary by engine.
- **Compare with a tolerance instead of bit-exactly.** A tolerance-based harness cannot
  distinguish "rounding differs in the last bit" from "the port diverges after a bounce",
  which is precisely what the harness exists to catch. Determinism is a boolean.
- **Higher-resolution table (e.g. per-millidegree).** 360,001 entries for precision no
  player can perceive, at real WASM payload cost. Revisit only if profiling or gameplay
  shows the interpolation error mattering.
