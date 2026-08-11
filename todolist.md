# Dungeon Barrage — Known Problems and Open Work

**Read this before touching the engine.** Every entry names a real problem, why it matters,
and **at least two ways to solve it** with one recommended. If you disagree with the
recommendation, say so and pick the other — but do not invent a third silently.

**Status legend:** 🔴 blocking · 🟠 important · 🟡 deferred · ✅ resolved

Updated: 2026-08-07 · Companion to [`docs/PROGRAM_PLAN.md`](docs/PROGRAM_PLAN.md) (milestones)
and [`docs/BUILD_LOG.md`](docs/BUILD_LOG.md) (history).

---

## ✅ P1 — `resolve_effect` is not wired into `command.rs` — RESOLVED 2026-08-07

**The problem.** All 22 effect resolvers exist and pass 282 tests, but **nothing calls them
from a real command.** `command.rs::apply_ability` resolves damage and the three legacy
effects (`Heal`, `HealthTransfer`, `SelfDamage`) inline, and never invokes
`resolve::resolve_effect`. So in an actual match, Arzum still does not teleport.

**Why it matters.** This is the same failure mode as the original inert-effect gap: code that
is correct, tested, and unreachable. Tests passing is not the same as the game working.

**Solutions.**

1. **Call `resolve_effect` from `apply_ability` after damage resolution.** *(Recommended.)*
   Build a `ResolveContext` from the already-computed damage map, terrain ops, and object
   list, then iterate `ability.effects` in declaration order. Order matters — `WallImpact`
   must observe where `Push` left the target. Move the three legacy effects out of
   `command.rs` in the *same* change so there is exactly one resolution path, not two.
2. **Introduce an `ActionPipeline` that owns the whole sequence** (validate → resolve attack
   → resolve effects → settle → hash). More structural, and it is where the turn scheduler
   will eventually live, but it is a larger refactor of a security-critical file.

**Trap:** do not leave the legacy three in `command.rs` "for now". Two resolution paths for
one concept is how effects go inert.

**Resolved as recommended.** `apply_ability` builds one `ResolveContext` and iterates
`ability.effects` in declaration order. `Heal`, `HealthTransfer`, and `SelfDamage` moved to
`resolve/support.rs` in the same change, so there is exactly one resolution path.

---

## 🟠 P2 — `terrain_cells_removed` reaches state from `command.rs` only — PARTIAL

**The problem.** `terrain::apply_operation` returns the count of cells destroyed.
`CommandOutcome::terrain_cells_removed` needs it — the Excavator XP bonus
(`docs/PROGRESSION.md` §2) and the terrain-value telemetry both read it. `ResolveContext`
has nowhere to put it, so every resolver discards the value with `let _removed = …`.

**Why it matters.** A progression bonus that silently always evaluates to zero is worse than
an absent one: it looks implemented.

**Solutions.**

1. **Add `terrain_cells_removed: &'a mut u32` to `ResolveContext`.** *(Recommended.)* Every
   resolver accumulates into it; `apply_ability` seeds it and copies the total into
   `CommandOutcome`. Small, direct, and impossible to forget once the field exists. Costs one
   edit per test harness (there are five).
2. **Have `resolve_effect` return a `ResolveSummary { cells_removed, … }`** that the caller
   folds. Keeps `ResolveContext` read-mostly, but every family's signature changes and
   summaries must be merged correctly at each call site — more places to get it wrong.

**Do both at once with P1** — the count has nowhere to go until the wiring exists.

**Half done, and the half that is missing is the half that matters.** `ResolveContext` gained
the accumulator and `command.rs` seeds it and copies the total into `CommandOutcome` — but
**four resolver call sites still discard the count**: `attack_mods.rs` ×3 (cluster crater,
tunnel bore, tunnel detonation) and `objects.rs` ×1, all still `let _removed = …`. Those are
the resolvers that actually destroy terrain, so the Excavator bonus still reads zero for every
ability that goes through them.

This was closed prematurely on 2026-08-07 after verifying `command.rs` alone. Recorded rather
than quietly amended: **verifying the consumer is not verifying the producers.** The remaining
fix is mechanical — accumulate into `*ctx.terrain_cells_removed` at each of the four sites —
and is folded into the P3 crater-routing work, since those call sites are exactly the ones
being rerouted.

---

## 🟠 P3 — Terrain blocks have hit points, but craters still bypass them

**The problem.** `TerrainMask` stores one `Material` byte per cell. A cell is solid or empty,
full stop. The product requirement is that **destructible blocks have HP, and remaining HP
maps proportionally to remaining usable surface area** — a block at 40% health offers 40% of
its original standing room. Different ammunition must affect terrain differently.

Broader requirement: **everything on screen except the landscape background has hit points.**
Characters and persistent objects already do; terrain does not.

**Why it matters.** This changes the terrain model, which every collision, sweep, ballistic
trajectory, and the state hash depend on. Getting the representation wrong is expensive to
undo later.

**Solutions.**

1. **`TerrainBlock` entities that own cell spans; HP drives deterministic erosion.**
   *(Recommended.)* A block is a rectangle of cells with `health` and `max_health`. Damage
   reduces HP; an erosion function converts HP% into exactly which cells remain solid, eroding
   edge-inward so a platform *shrinks* rather than sinks. The cell mask stays the single
   collision/render source of truth, so **every existing system keeps working unchanged** —
   sweeps, ballistics, and `is_solid_at` need no edits. Blocks become addressable entities,
   which is what "everything has hit points" needs and what portals and drones will reuse.
2. **Per-cell HP: widen `cells` to `(Material, u8)`.** Simpler conceptually — a cell dies at
   zero and block health is just the surviving fraction. But it doubles mask memory, changes
   the hash encoding, gives no addressable block entity for the UI or for damage attribution,
   and makes "40% health = 40% surface area" emergent rather than guaranteed.

**Trap:** erosion order must be deterministic and documented. A random or
iteration-order-dependent erosion breaks replay and the state hash.

**Solution 1 implemented as a library, and that is the problem.** `blocks.rs`, `map.rs`, and
`resolve/terrain_damage.rs` are correct and well tested — column erosion, impact-directed,
recomputed from health — but on 2026-08-07 an audit found the system was **unreachable from
actual gameplay**:

- `SimulationState` had **no `blocks` field**, so block health had no home in authoritative
  match state.
- Blocks were **absent from the state hash**, which ADR 0005 explicitly requires.
- `damage_blocks_in_radius` took `&mut [TerrainBlock]` and was called from **nowhere** but its
  own tests.
- `map::horizontal_test_array` was referenced only by tests.

The mechanic worked in a test tube. In a real match no block existed and nothing could damage
one. This is the *third* occurrence of the project's signature failure — correct, tested,
unreachable code — and it passed review because the agent's evidence was true but scoped to
the library rather than the game. **When a report says a behaviour is proven, check what calls
it, not just what tests it.**

**WHAT REMAINS — this is why P3 is still open.** Crater operations
(`terrain::apply_operation`) still write cells directly inside a block's span without
reducing its health. A block can therefore be holed while reporting full health, which is
the exact drift the block model exists to prevent. Two fixes:

1. **Route crater operations through block damage.** *(Recommended.)* When a crater
   intersects a block, convert the overlap into block damage and let erosion decide the
   cells. Keeps one authority. Costs a rework of how craters interact with `apply_operation`.
2. **Let craters carve freely and treat block health as advisory** for rendering and UI only.
   Cheaper, but abandons "percent health equals percent surface" the moment any explosion
   lands, which is the requirement itself.

**Open sub-question for the owner:** should erosion be edge-inward (platform shrinks
horizontally), top-down (platform thins), or ammunition-dependent? Recommendation is
edge-inward as the default with an ammunition-specific override, because a shrinking ledge
reads clearly to a player deciding whether they can still stand there.

---

## 🟠 P4 — No turn scheduler; matches cannot progress

**The problem.** `MatchPhase` exists as an enum and is hashed. Nothing advances it. There is
no turn cycle, no timer, no next-player selection, no victory check.

**Solutions.**

1. **A `MatchScheduler` trait with a turn-based implementation.** *(Recommended.)* ADR 0001 §7
   already provisions this seam so the real-time PvP mode can supply a second implementation
   against the same terrain, collision, and damage.
2. **A concrete state machine in `scheduler.rs` with no trait**, added later if the second mode
   materializes. Less abstraction now; a refactor of a load-bearing file later.

---

## 🟠 P5 — No movement, maps, or spawns

**The problem.** `movement_remaining` is tracked and hashed; walking, jumping, slopes, and
falls are unimplemented. There is no map definition, no loading, and no spawn placement.

**Solutions.**

1. **`movement.rs` + `map.rs`, reusing the swept-collision helper already proven in
   `resolve/displacement.rs`.** *(Recommended.)* That sweep already guarantees a character
   never ends up inside terrain; locomotion should not re-derive it.
2. **Fold movement into the scheduler** as part of the `Movement` phase. Fewer files, but it
   couples locomotion to turn structure and the real-time mode would have to unpick it.

---

## 🟠 P6 — No golden-vector regression corpus

**The problem.** The TypeScript parity oracle was retired (ADR 0004) and its replacement was
never built. There is currently **no protection against a refactor silently changing game
behaviour**.

**Solutions.**

1. **Frozen golden vectors: seeded command sequences plus their state hashes, committed and
   asserted in CI.** *(Recommended.)* Proves self-consistency across builds and targets.
2. **Property-based testing** (invariants like "health never exceeds max", "no character
   inside terrain") instead of fixed vectors. Catches different bugs — complementary, not a
   substitute, because it cannot detect a deliberate-looking behaviour change.

**Honest limitation:** golden vectors freeze whatever they are given, bugs included. Generate
them only from reviewed code, and never regenerate one inside a feature commit.

---

## 🟠 P7 — Blast occlusion is unimplemented, engine-wide

**The problem.** `terrain::subtract_circle` is purely radial. Solid terrain does not shield
anything behind it from an explosion, so a stone wall is not cover against a blast.

**Solutions.**

1. **Leave it radial and document it.** *(Recommended for now.)* It is consistent across every
   explosion, cheap, and predictable. Currently documented by
   `attack_mods::tunnel_detonation_is_radial_and_does_not_treat_stone_as_cover`.
2. **Add a line-of-sight test per affected cell/character.** More physical, but it is a global
   change to crater semantics, costs a raycast per candidate, and interacts with P3 — a
   partially-eroded block is partially transparent, which needs its own rule.

**Do not solve this in one resolver.** It is global or it is nothing.

---

## 🟠 P8 — Agent session limits are eating the delegation budget

**The problem.** Six workflow runs; **14 agent failures**, all session limits. Twice, agents
wrote their files and died before reporting, so the work was recovered only by inspecting the
tree. The workflow runtime's failure record lives in a transcript, not the repository.

**Solutions.**

1. **Smaller fleets (3–5 agents), and check the working tree before assuming a failed agent
   produced nothing.** *(Recommended.)* Three of four "failed" M1.5 agents had complete files.
2. **Write run outcomes to the repository** so a delegation failure is auditable from git
   rather than from a transcript that expires.

---

## 🟡 P9 — Deferred by owner decision

| Item | Note |
|---|---|
| Player-created champions | Post-launch. Analysis preserved in `PROGRAM_PLAN.md` §4 — at one champion/year a **review process** suffices; no sandbox needed |
| Real-time PvP mode | Deliberately last; shares terrain/collision/damage with the turn-based mode |
| 15 unspecified characters, 45 undrafted passives | Content backlog, real scheduling commitment |
| C# client (Godot 4) | Not started; ADR 0004 |
| ASP.NET match server | Not started; ADR 0004 |

---

## 🟡 P10 — Outstanding owner actions

1. **Rotate the GitHub PAT.** It was transmitted as a plaintext file and through a chat
   context. Scope the replacement to this repository alone rather than all 28.
2. **Confirm four character rules** (`docs/CHARACTERS.md` §7): Karl's 24%/74% vs the brief's
   33%; Numa's harpoon threshold; Zeke's 22 HP heal reading; Arzum's 50–200% roll in rated play.
   Karl's crit *chance* is additionally an unsourced 20% placeholder.
3. **Level-up reward balance** — the character option dominates the credit option 46×
   (`docs/PROGRESSION.md` §4). Recommended fix is a one-line data change.

---

## ✅ Resolved

| Item | Where |
|---|---|
| All 22 effect kinds have resolvers | `crates/db-sim-core/src/resolve/` — 282 tests |
| RNG modulo bias | `rng.rs` — off-by-one rejection threshold, undetectable statistically |
| Terrain truncating cast | `terrain.rs` — wrapped negative, silently touched zero cells |
| Zeke's Lifeshare destroying health | `command.rs` — debited before checking receivable |
| 14 roster defects | `character.rs` — incl. missing ChainDetonate, 10× harpoon threshold error |
| Repo published | `Crownelius/DungeonBarrage` |

---

## Conventions for anyone editing this file

- **Never delete a problem — move it to Resolved with a pointer.** The history of what was
  wrong is how the next person avoids repeating it.
- **Every problem needs ≥2 solutions.** One option is a decision already made; two is a
  choice, and the reasoning survives.
- **State traps explicitly.** Most entries here exist because something compiled, passed
  tests, and was still wrong.
