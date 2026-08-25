# Dungeon Barrage — Known Problems and Open Work

**Read this before touching the engine.** Every entry names a real problem, why it matters,
and **at least two ways to solve it** with one recommended. If you disagree with the
recommendation, say so and pick the other — but do not invent a third silently.

**Status legend:** 🔴 blocking · 🟠 important · 🟡 deferred · ✅ resolved

Updated: 2026-08-24 · Companion to [`docs/CLIENT_SPEC.md`](docs/CLIENT_SPEC.md) (current client
milestones), [`docs/PROGRAM_PLAN.md`](docs/PROGRAM_PLAN.md) (historical plan),
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

## ✅ P2 — `terrain_cells_removed` reaches state from `command.rs` only — RESOLVED 2026-08-14

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

**Resolved as recommended in `ef3c41f`.** All four resolver producers now route terrain
operations through `block_ops::apply_operation` and saturating-add the returned cell count to
`ResolveContext::terrain_cells_removed`. `command.rs` publishes that accumulator, and its
terrain-destroying ability test asserts the result is nonzero. The historical partial diagnosis
above is retained because it records why testing only the consumer was insufficient.

---

## ✅ P3 — Terrain blocks have hit points, but craters still bypass them — RESOLVED 2026-08-14

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

**Resolved as recommended in `ef3c41f`.** `SimulationState.blocks` is authoritative and
hashed; authored maps populate it; every crater producer calls `block_ops::apply_operation`;
and erosion is ammunition-dependent through the stored `ErosionAxis` (columns by default,
rows for penetrating effects). Tests cover health/mask consistency, material permissions,
deterministic routing, and a real command path. The “WHAT REMAINS” section above is historical
evidence of the once-real gap, not current work.

---

## ✅ P4 — No turn scheduler; matches cannot progress — RESOLVED 2026-08-07

**The problem.** `MatchPhase` exists as an enum and is hashed. Nothing advances it. There is
no turn cycle, no timer, no next-player selection, no victory check.

**Solutions.**

1. **A `MatchScheduler` trait with a turn-based implementation.** *(Recommended.)* ADR 0001 §7
   already provisions this seam so the real-time PvP mode can supply a second implementation
   against the same terrain, collision, and damage.
2. **A concrete state machine in `scheduler.rs` with no trait**, added later if the second mode
   materializes. Less abstraction now; a refactor of a load-bearing file later.

**Resolved via solution 2**, plus `match_host.rs` as the orchestrator that drives it. The
trait can be extracted when the real-time mode actually needs it; extracting it now would be
an abstraction with one implementation.

**A match runs end to end.** `MatchHost` sequences scheduler, movement, command, victory, and
block damage. Proven by tests that start a real map with real characters, walk, pass, rotate,
eliminate a team, and reach a terminal state — including one that asserts a match of nothing
but passes still terminates via the hard turn limit.

---

## ✅ P5 — No movement, maps, or spawns — RESOLVED 2026-08-07

**The problem.** `movement_remaining` is tracked and hashed; walking, jumping, slopes, and
falls are unimplemented. There is no map definition, no loading, and no spawn placement.

**Solutions.**

1. **`movement.rs` + `map.rs`, reusing the swept-collision helper already proven in
   `resolve/displacement.rs`.** *(Recommended.)* That sweep already guarantees a character
   never ends up inside terrain; locomotion should not re-derive it.
2. **Fold movement into the scheduler** as part of the `Movement` phase. Fewer files, but it
   couples locomotion to turn structure and the real-time mode would have to unpick it.

**Resolved via solution 1.** `movement.rs` reuses the swept-collision technique proven in
`resolve/displacement.rs` rather than deriving a second one — two collision models would
disagree and one of them would let a player into a wall. `map.rs` supplies definitions,
spawns, and the eight-block horizontal test array.

---

## ✅ P6 — No golden-vector regression corpus — RESOLVED 2026-08-07

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

**Resolved via solution 1**, in `crates/db-sim-core/tests/golden_vectors.rs`. Five scripted
matches driven through `MatchHost` — the top of the engine, not individual helpers, so a
vector proves the whole loop still composes rather than that one function still behaves.

**The corpus guards itself against being vacuous.** Every vector asserts the script actually
changed the world, advanced turns, and (for combat vectors) dealt damage. That guard
immediately earned itself: the first "firing duel" fired three shots, hashed perfectly
stably, and dealt **zero damage** — it would have been frozen as combat coverage that covered
no combat. Two structural tests back it up: the same script hashes identically twice in one
process, and different seeds produce different matches (without which a corpus of identical
hashes looks green while testing nothing).

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

---

## ✅ P11 — `TurnEndReason` is accepted everywhere and recorded nowhere — RESOLVED 2026-08-07

**The problem.** `scheduler::end_turn` takes a `TurnEndReason` and matches it exhaustively,
but every arm does the same thing, and `leave_victory_check` hardcodes
`TurnEndReason::Attacked` regardless of how the turn actually ended. `MatchHost` threads the
real reason in and it is discarded. So a pass, a timeout, and a committed attack are
indistinguishable downstream.

**Why it matters.** `PRODUCT_SPEC.md` §2 wants the result panel and replay to distinguish a
timeout from a deliberate action — they look identical in the resulting state but say very
different things about the player. Turn-timeout rate is also a named launch metric
(`PRODUCT_SPEC.md` §10), and it cannot be measured from state that never records it.

**Solutions.**

1. **Record it on `SimulationState`** as `last_turn_end_reason`, hashed like any other
   gameplay field, and have `leave_victory_check` take the reason from its caller instead of
   hardcoding. *(Recommended.)* Small, and it makes the existing parameter honest.
2. **Emit it as a match event** rather than state, once an event log exists (P6 territory).
   Better long-term home — replay and telemetry both want events, not snapshots — but it
   depends on a system that does not exist yet.

**Trap:** the parameter currently *looks* wired. An exhaustive match on a value that changes
nothing reads as intentional design until you follow it to the call site.

**Resolution evidence.** `4ac6b09` added pending/last reason state and versioned it, but the
terminal branch still skipped `end_turn` and could leave the preceding turn's reason visible.
The 2026-08-24 working slice now commits `pending_turn_end_reason` before returning from a terminal
victory check. A scheduler regression and movement-fall host tests cover terminal and continuing
matches. Because this changes replay-visible state, `SIMULATION_VERSION` is 5 and every golden
vector records its prior v4 hash.

---

## ✅ P12 — The passive prompt only fired for the acting player — RESOLVED 2026-08-07

**The problem.** `MatchPhase::PassiveSelection` is now set by `MatchHost::submit_ability`
when an actor's gauge fills for the first time — but that is the *only* producer. A gauge
that fills from damage **taken** during another player's turn does not raise it, so a player
can reach a full gauge and never be offered the one-time passive choice until they next
attack.

**Why it matters.** `CHARACTERS.md` §2 says the choice happens the first time the gauge
fills, not the first time the player attacks afterwards.

**Solutions.**

1. **Check every player's gauge at the `StatusResolution` boundary**, not just the actor's,
   and raise the interrupt for whoever is owed a choice. *(Recommended.)* Catches gauge fills
   from damage taken and from healing an ally.
2. **Check at turn start for the incoming player only.** Simpler, but delays the prompt until
   that player's own turn, which is a visible lag between "gauge full" and "choose".

---

## 🟠 P13 — The native client path stops at a partial Rust session contract

**The problem.** ADR 0006 settled Godot/C# for presentation, Rust for authoritative rules,
and a client-only C ABI. The working tree now has validated match creation, atomic snapshots,
normalized commands, a generation/idempotency-owning `MatchSessionHost`, ordered net-diff
transitions, exact terrain dirty row-runs, and the first shared machine-readable match fixture.
It still has no preview contract, authority-timeout transition, complete per-strike/RNG/status
provenance, real FFI match handle, C# session, or Godot project.
The game is therefore not playable despite the simulation being substantial.

**Why it matters.** Starting scenes now would force C# to infer missing authoritative events or
bind to the placeholder FFI. Either creates a second behavior path and makes green UI tests say
nothing about the real Rust host.

**Solutions.**

1. **Continue the ordered C1 → C2 → C3 → C4 gates in `CLIENT_SPEC.md`.** *(Recommended.)*
   Finish truthful Rust provenance/preview and the remaining direct transition scenarios; then
   make the same raw fixture pass through the C ABI and headless C# before creating Godot scenes.
   This is slower to first pixels but every layer proves the one below it.
2. **Build a Godot vertical prototype against hand-authored C# DTOs now.** Faster visual feedback,
   but it must later be replaced and cannot validate authority, buffer ownership, duplicate replay,
   or hash parity. It repeats the repository's established “correct but unreachable” failure mode
   at the client boundary.

**Decision:** solution 1 is locked by ADR 0006 and `CLIENT_SPEC.md` §21. Current operational state,
exact commands, and ownership warnings live in `docs/HANDOFF.md`.

---

## 🟡 P9 — Deferred by owner decision

| Item | Note |
|---|---|
| Player-created champions | Post-launch. Analysis preserved in `PROGRAM_PLAN.md` §4 — at one champion/year a **review process** suffices; no sandbox needed |
| Real-time PvP mode | Deliberately last; shares terrain/collision/damage with the turn-based mode |
| 15 unspecified characters, 45 undrafted passives | Content backlog, real scheduling commitment |
| Godot/C# presentation client | Moved to active P13 and `CLIENT_SPEC.md` C1–C5; no project exists yet |
| Future match server | Rust-native per ADR 0006, not ASP.NET; deferred until the local-client gates establish the shared contract |

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
| P2 terrain-removal accounting | `ef3c41f` — every crater producer accumulates the routed count |
| P3 authoritative block routing | `ef3c41f` — state/hash/map wiring plus block-aware crater path |
| Repo published | `Crownelius/DungeonBarrage` |

---

## Conventions for anyone editing this file

- **Never delete a problem — move it to Resolved with a pointer.** The history of what was
  wrong is how the next person avoids repeating it.
- **Every problem needs ≥2 solutions.** One option is a decision already made; two is a
  choice, and the reasoning survives.
- **State traps explicitly.** Most entries here exist because something compiled, passed
  tests, and was still wrong.
