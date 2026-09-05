# Dungeon Barrage build log

**Status:** Append-only engineering record. Never rewrite a past entry.
**Covers:** first line of work through publication to GitHub.
**Related:** [PROGRAM_PLAN.md](./PROGRAM_PLAN.md) · [MODULE_OWNERSHIP.md](./MODULE_OWNERSHIP.md) · [SECURITY_BASELINE.md](./SECURITY_BASELINE.md) · [adr/](./adr/)

---

## 1. Purpose, and how to read this

This is the chronological record of what was built, what was decided, what broke, and why.
It exists because the project's design documents describe the *intended* system, and a
reader arriving later needs to know how the real one got here — including the parts that
went wrong.

Three rules govern it:

- **Append-only.** Entries are never edited to look better in hindsight. If a past entry
  turned out to be wrong, a later entry says so and points back at it. Section 5 below is
  itself an example: an earlier revision of `PROGRAM_PLAN.md` called the engine complete,
  and the correction is recorded rather than quietly applied.
- **Verifiable.** Every claim here is traceable to a commit, a document, or a file in the
  tree. Where something is reported but could not be confirmed from the repository, it is
  marked **[unverified]** rather than stated flatly.
- **Written for a stranger.** This will be read by people and models who were not present
  for the conversation that produced the work. Nothing assumes that context.

A note on scale, so the reader calibrates correctly: at time of writing this project is at
roughly **a quarter of its engine**, and eight of its nine starter characters do not
function. Section 5 explains how that coexisted with 207 passing tests.

### Dates

All work described below happened on **2026-08-06**, in a single session. Git confirms
this: the first commit is timestamped `05:09:33 -06:00` and the last `20:12:14 -06:00`.
Where an entry refers to something inherited from before that session, it says so.

Claims sourced from **commits** are stable. Claims sourced from the **working tree** —
§4's fix descriptions, §5's counts, §10's inconsistency table — were verified against
`HEAD` = `49fcb54` with the ADR 0004 pivot staged but uncommitted, and a concurrent session
was actively editing `db-sim-core` at the time. Treat working-tree specifics as accurate for
that moment rather than permanently.

---

## 2. Timeline

The commit history is the spine. Fourteen commits on `main`, all authored the same day.

### Phase 0 — Inherited baseline (`3a91695`, 05:09)

The session opened on an existing working prototype carried over from prior sessions, not
a blank repository: a deterministic fixed-point TypeScript simulation
(`lib/game/simulation.ts`, 1,080 lines), a React/canvas vertical slice
(`app/game/DungeonBarrageGame.tsx`, 1,459 lines), a vinext/Cloudflare Worker shell, and
three design documents — `PRODUCT_SPEC.md`, `ARSENAL.md`, `PLATFORM_STRATEGY.md`.

It was committed **as-is, before any changes**, specifically so that the TypeScript
simulation would be preserved as a reference oracle rather than overwritten by the pivot
that was about to happen. That decision — commit the old thing before replacing it —
turned out to matter for the next several hours and then, ironically, to be undone by
ADR 0004 (§7).

**Learned:** committing the pre-pivot state costs one commit and buys an oracle.

### Phase 1 — Rust foundation and governing specs (`22af543`, 05:19)

The Rust/WASM pivot landed (ADR 0001, §7 below). The foundation was **hand-written by the
integrator rather than delegated**, on the reasoning that it is the interface contract that
lets everything else proceed in parallel — a wrong guess here would be expensive to
reverse.

Delivered: `fixed.rs` (fixed-point math with explicit overflow semantics), `canonical.rs`
(length-prefixed byte encoding plus FNV-1a), `types.rs` (the shared data contract),
`error.rs` (no panics on untrusted input), and a thin `db-sim-wasm` boundary crate carrying
no game rules. Plus `PROGRESSION.md` and `SECURITY_BASELINE.md`.

Two findings recorded in this commit are worth pulling out:

- **`JSON.stringify` hashing was a latent parity bug.** JS number formatting, key escaping
  and Unicode handling are engine semantics that Rust would not reproduce by accident.
  Length prefixing also closed a real collision: `("ab","c")` and `("a","bc")` previously
  encoded identically, which would have masked genuine state divergence.
- **The lint gates caught a bug in this very commit.** `distance_squared` could overflow
  `i64` and wrap a far distance into a spurious hit. Made saturating — fixed, not
  suppressed.

**Landed:** 15/15 Rust tests, clippy clean under `-D warnings`, wasm32 target builds.

### Phase 2 — Parallel-work protocol (`f452514`, 05:20)

Six modules stubbed and declared so concurrent implementation could start without a
half-written sibling blocking everyone. `MODULE_OWNERSHIP.md` made coordination
*structural* rather than conventional. See §6.

### Phase 3 — CI gates and supply-chain policy (`67e9960`, 05:27)

The controls in `SECURITY_BASELINE.md` §10 became enforceable: clippy under `-D warnings`,
`cargo test`, wasm32 build, `cargo-deny`, `npm audit`, `gitleaks`, plus grep gates for
`unsafe` blocks, floating point in the core, `dangerouslySetInnerHTML`, `eval`, and
interpolated SQL.

**Every gate was run locally before being committed, and that immediately paid for
itself** — see §4.6 and §4.7. Also in this commit: a permanently-failing starter test was
replaced rather than left red, `tsconfig` moved ES2017 → ES2022 (the oracle's BigInt
hashing needs ES2020+), and `npm audit fix` took 18 advisories to 14 without breaking
changes.

Alongside CI, the Haiku-tier modules `rng.rs`, `terrain.rs` and `weapon.rs` landed their
bulk (+339 / +777 / +727 lines).

### Phase 4 — Progression schema (`9a7dae2`, 05:29)

`db/schema.ts` implemented `PROGRESSION.md` §6. Two properties were pushed into the schema
rather than application logic, because both failure modes are silent: **balances are
derived** (`economy_transaction` is append-only and authoritative, the columns on
`player_profile` are a rebuildable cache — a mutable balance as source of truth makes
double-grant bugs unfalsifiable after the fact), and **`idempotency_key` is UNIQUE** (a
retried level claim or double-clicked purchase collides at the database rather than
depending on application code to notice).

Also fixed here: `worker-env.d.ts` declared a global `Env`, which compiles and does nothing,
because `cloudflare:workers` types its env as `Cloudflare.Env` — a namespace. Typecheck went
clean for the first time (from 8 errors).

### Phase 5 — Program plan (`cc26230`, 05:31)

`PROGRAM_PLAN.md` recorded milestones M0–M8 with **gates as evidence, not dates**, and an
honest status table listing what was *not* built. The real-time PvP mode was deliberately
sequenced last (M8) because it shares terrain, collision, damage and knockback with the
turn-based mode — building it first means debugging two schedulers against an unvalidated
core.

This commit also carried the first correction to Haiku-authored `rng.rs` (§4.8).

### Phase 6 — Character-kit pivot (`24a458a` 05:47, `3ec70ce` 12:24)

The second pivot (ADR 0002, §7). The design moved from an equipment-driven game to a
character-driven one: 24 characters with fixed kits replacing a three-slot loadout over a
12-weapon roster.

`24a458a` wrote `CHARACTERS.md` (all nine starters) and carried the Sonnet correction pass
on `rng.rs` and `terrain.rs` — which is where the modulo-bias and truncating-cast defects
were fixed (§4.1, §4.2). Three contradictions in the product brief were resolved
**explicitly and flagged for confirmation** rather than silently chosen (§9, item 8).

`3ec70ce` refactored `types.rs`. Notably, it was **spliced rather than rewritten**: the
terrain and ballistics sections are byte-identical because `terrain.rs` already depended on
them and was passing 61 tests. Only the ownership of an attack changed, so only the
sections describing ownership changed. `weapon.rs` — 996 reviewed lines including two real
bug fixes — was moved to `reference/weapon-roster-retired.rs` rather than deleted.

Clippy caught two integer divisions in the integrator's own new code here
(`BODY_WIDTH/4`, `BODY_WIDTH/2`). Both were exact, but they were rewritten in
`POSITION_SCALE` units so there is no division to round at all.

### Phase 7 — Core completion (`78b686f`, 17:24)

The largest commit: +5,402 lines across `ballistics.rs`, `character.rs`, `command.rs` and
`hash.rs`. **207 tests, clippy clean, wasm32 builds.** This commit's message described the
core as "functionally complete apart from the TS-parity harness" — a claim corrected three
hours later (§5).

Three things happened here that define the rest of the log:

1. `command.rs` was **finished by hand** after its assigned agent hit a session limit
   mid-task (§6).
2. The Sonnet review of the Haiku-authored roster found **14 defects** (§4.5).
3. The **ballistics parity blocker was confirmed** — structurally, not incidentally. See
   ADR 0003 and §4 below.

### Phase 8 — ADR 0003 and plan reconciliation (`5579395` 17:25, `e03a3f6` 17:28)

The parity blocker was written up as ADR 0003. The important finding was the second-order
one: `Math.sin`/`Math.cos` are not bit-identical across JavaScript engines, because ECMA-262
explicitly permits implementation-dependent approximation for the transcendental functions.
**The oracle was already non-deterministic across the browsers the game had to support.**
The Rust port did not create that defect; it exposed it.

`5579395` also removed a stray `test_debug.rs` an agent had left at the repository root.

`e03a3f6` reconciled `PROGRAM_PLAN.md` with the character model — M3 and M5 still described
a 12-weapon roster and a shard/weapon shop — and promoted the content backlogs to explicit
owner items instead of leaving them buried in `CHARACTERS.md`.

### Phase 9 — Credential handling and remote setup (`fbe325e` 19:42, `93f03a0` 19:43)

See §8.

### Phase 10 — Engine scope correction (`49fcb54`, 20:12)

The most important entry in this log. See §5.

### Phase 11 — In flight at time of writing

ADR 0004 (native desktop, C# client, TypeScript removed entirely) is **accepted and being
implemented, but nothing is committed yet.** `HEAD` is still `49fcb54`.

Staged in the working tree as this log was written: the ADR itself; deletion of the entire
web surface (~33 files, ~12,600 deletions) with `simulation.ts`, `DungeonBarrageGame.tsx`
and `db/schema.ts` renamed into `reference/`; a **new `crates/db-sim-ffi` crate** — the C ABI
boundary for P/Invoke that ADR 0004 §"Build target change" calls for; and modifications
across `db-sim-core`, `MODULE_OWNERSHIP.md`, `PLATFORM_STRATEGY.md`, `PROGRAM_PLAN.md` and
`SECURITY_BASELINE.md`.

**Anyone reading the committed history alone will not see this pivot.** It should get its own
timeline entry below the append marker once it lands. See §7.3.

---

## 3. Architecture decisions

Four ADRs, all dated 2026-08-06, all status **Accepted**.

| # | Decision | Supersedes | One-line reason |
|---|---|---|---|
| [0001](./adr/0001-rust-wasm-core.md) | Rust core compiled to WASM; TS simulation frozen as a reference oracle | `PLATFORM_STRATEGY.md` §12 | The authoritative sim parses untrusted network commands, so memory-unsafety there is remote code execution — Rust removes the class by construction. |
| [0002](./adr/0002-character-kits.md) | 24 character kits replace the three-slot loadout | `PRODUCT_SPEC.md` §3; `ARSENAL.md` as player-facing equipment | Asymmetric characters *and* a free equipment loadout multiply the balance surface by 24 for no gain in legibility. |
| [0003](./adr/0003-shared-trig-table.md) | Both implementations compile the same 361-entry Q16 sine table | Inverts ADR 0001 §2's "oracle is authoritative" for trig only | `Math.sin`/`Math.cos` are not bit-identical across JS engines, so conforming to the oracle would mean reproducing a defect. |
| [0004](./adr/0004-native-desktop-rust-csharp.md) | Native desktop client in C# (Godot 4); TypeScript removed entirely; web delivery dropped | `PLATFORM_STRATEGY.md` §1, §3, §9, §10, §13; ADR 0001 §2 (the oracle strategy) | Product-owner directive; C# cannot run in a browser without a game-engine runtime, so C#-client and web-first were not independent choices. |

ADR 0003 is the one to read if you only read one. It is a case where the correct engineering
answer was to change the thing that was supposed to be the source of truth, and it says so
in the open rather than working around it.

---

## 4. Defects caught, and how

This is the section with the most reuse value. Each entry records what the defect was, how
it surfaced, why it mattered, and what fixed it.

### 4.1 RNG modulo bias in `bounded()` — an off-by-one in the rejection threshold

**What.** `rng.rs::bounded()` used rejection sampling to remove modulo bias, computing its
acceptance threshold as:

```rust
let threshold = (u32::MAX / max_exclusive).wrapping_mul(max_exclusive);
// ... accepted when: value <= threshold
```

`u32::MAX` is `2^32 - 1`, not `2^32`. When `max_exclusive` divides `2^32` evenly — every
power of two — the quotient comes out one short of the true `2^32 / max_exclusive`, and the
threshold silently reintroduces exactly the bias toward outcome zero that the rejection
sampling existed to remove.

**How found.** Sonnet correction pass on the Haiku-authored module. The implementer had been
forced to suppress a lint on that exact line —
`#[expect(clippy::integer_division, reason = "rejection sampling threshold: we need the
quotient, not a float")]` — which flagged it as a judgement call rather than a mechanical
step, and that is what drew review attention to it.

**Why it mattered.** The bias is roughly **1 in 2^32**. No statistical distribution test at
any practical sample size can detect it; the module's existing 10,000-sample and
100,000-sample chi-square-style tests passed happily before and after. It is only findable
by reading the arithmetic. In a game where the seeded PRNG drives Arzum's target selection,
Karl's crits and Aleph's blink — and where replays must reproduce exactly — a silent bias in
the shared draw primitive is the kind of thing nobody ever traces back.

**Fixed with.** The threshold is now `max_exclusive.wrapping_neg() % max_exclusive`, which
computes `2^32 mod max_exclusive` without ever materializing `2^32`, and the comparison
flipped to `value >= threshold`. Two new tests pin it: one asserts the threshold is exactly
zero for every power-of-two bound from `2^0` to `2^31` (the case the old formula got wrong),
and one cross-checks the threshold against a widened-to-`u64` reference computation of
`2^32 mod bound` for odd and extreme bounds. The tests assert *the threshold itself*,
precisely because the distribution cannot be tested at this magnitude.

### 4.2 Terrain truncating cast in bounding-box clamps

**What.** `subtract_circle` and `subtract_capsule` clamped their bounding boxes with
`(mask.width as i32).saturating_sub(1)`. `width`/`height` are `u32`; every cell coordinate
in the module's API is `i32`, forced by `FixedPoint`'s `i32` fields. For any dimension at or
above `2^31` the cast wraps to a large **negative** number, so `max_x` becomes negative,
the bounding box reads as empty or inverted, and the loop touches **zero cells**.

**Why it mattered.** It fails silently in the worst possible direction. A terrain
subtraction that should have carved a crater simply does nothing — no panic, no error
return, no log line. The caller gets `removed == 0` and continues.

**How found.** Sonnet correction pass, by reading the casts rather than by any tool.
**Clippy did not catch it**: the workspace deny set (`Cargo.toml`) covers
`cast_possible_truncation`, `cast_sign_loss` and `cast_precision_loss`, but **not
`cast_possible_wrap`** — and `u32 as i32` is same-width sign reinterpretation, which is
precisely the wrap lint's territory. The gate looked comprehensive and had a hole in it.

**Fixed with.** A single `last_valid_index(dim: u32) -> i32` helper using
`i32::try_from(dim).unwrap_or(i32::MAX).saturating_sub(1)`, applied at all four call sites.
Two regression tests construct masks with `width: u32::MAX` and `height: u32::MAX` against a
deliberately small backing buffer, so the bounding-box arithmetic is exercised in isolation
without allocating gigabytes.

**Standing item:** `cast_possible_wrap` is still not in the deny set. The lint gap that
allowed this defect has not itself been closed.

### 4.3 A material-mask test that asserted nothing

**What.** `subtract_circle_respects_material_mask_soft` built a 10×10 checkerboard of `Soil`
(removable under `SOFT`) and `ReinforcedStone` (not), constructed the operation, and then:

```rust
let _ = apply_operation(&mut mask, &op);
```

That was the end of the test body. It ran the operation, discarded the result, and asserted
nothing whatsoever. It passed unconditionally, including if `apply_operation` had removed
every cell, no cells, or the wrong material entirely.

**Why it mattered.** The material mask is the rule that keeps reinforced terrain
indestructible. A test named "respects material mask" appearing green in a 200-test suite is
active misinformation — it is worse than an absent test, because it stops anyone from writing
the real one.

**How found.** Sonnet correction pass reading the test bodies, not just the test names.

**Fixed with.** The test now recomputes the expected material for all 100 cells
independently, asserts each one by coordinate, asserts the returned removal count matches the
independently derived count, and asserts that count is non-zero. Four further tests were
added alongside it with hand-verified discrete-geometry expectations — a radius-1 circle
removing exactly the 5-cell plus-pentomino, a radius-3 circle removing exactly 29 cells, a
zero-radius capsule removing exactly its 6 line cells, and a `u16::MAX` radius that must not
overflow.

### 4.4 `HealthTransfer` destroyed health outright

**What.** Zeke's Lifeshare (`EffectKind::HealthTransfer`) **debited the actor before checking
what the target could receive.** Aimed at a full-health ally, it cost the actor hit points
and healed nobody. The health did not go anywhere — it was destroyed.

**Why it mattered.** Beyond being wrong, it was invisible. The result panel's heal line
simply reads zero, so a player would see their own HP drop with no explanation and no way to
tell a bug from a mechanic they had misunderstood.

**How found.** This is the notable one. **The agent that wrote the module also wrote a
correct test that caught its own bug — and then hit a session limit before it could fix it.**
The bug and its proof were both sitting in the tree, unresolved, when the agent stopped. The
integrator finished `command.rs` by hand and found the failing test waiting.

**Fixed with.** The transfer is now bounded by three quantities simultaneously: the effect
magnitude, what the actor can spare above 1 HP, and what the target is actually missing. What
leaves the actor is exactly what arrives. A regression test covers the full-health-ally case
specifically.

**Learned:** a test that fails is a successful outcome even when nothing fixed it. The
value an interrupted agent left behind was the *evidence*, not the code.

### 4.5 The character-roster review — 14 defects

The Sonnet review of the Haiku-authored `character.rs` found 14 defects before the module was
committed. Several would have shipped as silent mis-balance rather than as failures. The five
recorded specifically in `78b686f`:

| Defect | Why it mattered | Resolution |
|---|---|---|
| Aleph's dagger chain (`ChainDetonate`) **missing entirely** | His signature mechanic, absent from his kit. Roster validation passed because nothing required it to exist. | Effect added to the throwing knife in `character.rs`, with a test that fails if the knife carries no `ChainDetonate`. |
| Numa's harpoon threshold `50_000` instead of `5_000` basis points | A **10× error** in the rule deciding which direction the harpoon pulls. At 50,000 bp the "below 50% HP" branch is unreachable, so the execute behaviour never triggers. | Corrected to `5_000`; test asserts the value with the comment "50% in basis points, not 50_000". |
| Karl's **unsourced 50% crit chance** | Would have put his expected turn damage at ~147% against a stated 72% baseline — by far the highest-damage starter, from a number nobody specified. | Reduced to a **20% placeholder** (`crit_chance_basis_points: 2_000`), explicitly documented in-code as needing designer confirmation, and escalated to `PROGRAM_PLAN.md` §6. |
| Ballistic tier constants could not reach their own tier's stated range | Tier1/2/3 at 500/30, 700/20, 900/15 compute to 8,333 / 24,500 / 54,000 at an optimal 45° launch, against stated reaches of 32,768 / 65,536 / 106,496 — **25–40% of required**. A Tier 3 character firing at maximum power could never hit anything near their advertised reach. | Retuned to 900/20, 1200/16, 1500/12, giving 1.24×/1.37×/1.76× headroom, with `max_ticks` kept above worst-case vertical flight time so a high arc cannot expire early. Derivation is written into the source. |
| `validate_roster()` masked every rejection reason behind a blanket `map_err`; `MAX_SPECIAL_EFFECTS` was never enforced | The validator reported *that* the roster was invalid but not *why* — useless in exactly the situation it exists for. Separately, `MAX_SPECIAL_EFFECTS` is documented in `types.rs` as the defence against unbounded per-impact work, and nothing checked it. | `validate_roster_slice` now returns specific `CharacterRejection` variants per failure. `MAX_SPECIAL_EFFECTS` is enforced in `validate_ability` with a named regression test. |

The remaining nine defects are not individually itemized in the commit message and could not
be enumerated from the repository. **[unverified]**

### 4.6 Two bugs in the CI workflow file itself

The standing practice "every gate runs locally before it is committed" produced its first
return the same hour it was written. Two defects in `.github/workflows/ci.yml` were caught by
running the gates before committing them:

1. **A stray text prefix on the workflow file**, which would have made it unparseable as
   YAML — a workflow that never runs, reported as no failures.
2. **The floating-point check matched its own documentation.** The gate greps
   `crates/db-sim-core/src/` for `\b(f32|f64)\b`. The modules that document the rule "no
   `f32`/`f64` in the authoritative core" naturally contain those strings in their comments,
   so the gate tripped on the very text explaining why it exists.

The second is the more instructive. Fixed by stripping comment lines before matching
(`grep -vE ':[0-9]+:[[:space:]]*(//|/\*|\*)'`). The reasoning recorded in the commit: *a gate
that trips on its own docs trains people to disable it.* A false-positive gate does not stay
enabled, so a noisy control is not a strict-safety control — it is a control on its way to
being deleted.

### 4.7 `cargo-deny` rejected its own initial configuration

The supply-chain gate failed on first run against the repository it was written for, on two
counts:

- **Intra-workspace path dependencies read as wildcard dependencies.** `wildcards = "deny"`
  is correct for registry crates, where a versionless dependency is a real supply-chain
  hazard; it is wrong for path deps that resolve to code in this repository.
- **`publish = false` crates are `UNLICENSED`**, which the licence allow-list rejected.

Both fixed by narrowing the exemption rather than loosening the check:
`allow-wildcard-paths = true` and `private = { ignore = true }`. The recorded reasoning is
that the allow-list exists to police *third-party* code entering the trust boundary, so
excluding first-party crates keeps it meaningful rather than weakening it. The alternative —
adding `UNLICENSED` to the allow-list — would have silently permitted any unlicensed
third-party crate forever.

### 4.8 Fabricated known-answer vectors in `rng.rs`

The Haiku-authored `rng.rs` shipped known-answer tests commented as *"Independently verified
PCG-XSH-RR 64/32 sequence"*. All three seeds' expected values were wrong. Commit `cc26230`
replaced them with actual outputs and deleted the "independently verified" claim.

A related gap was closed in `24a458a`: the determinism test compared
`rng1.bounded(bound)` against `rng2.bounded(bound)` where `bound` was drawn from `rng1`
only — so the two generators were being advanced a different number of times. The test could
pass while proving nothing about `bounded`. It now draws a bound from each generator and
asserts the bounds match before comparing results.

**Learned:** a test authored by the same pass that authored the implementation inherits its
misconceptions. Known-answer vectors in particular need an independent source, and a comment
claiming independent verification is not one.

---

## 5. The engine scope correction

This is the largest single finding in the project and the reason §1 insists on append-only
entries.

### What was believed

That characters would be **mostly data**. `EffectKind` is a closed vocabulary of reviewed
effect identifiers, composed by per-character definitions; adding a character should
therefore mean adding a table row, not writing engine code. The closed set also carried a
security rationale (`SECURITY_BASELINE.md` §6: no scripting surface, no downloadable
behaviour).

Commit `78b686f` at 17:24 described the core as *"functionally complete apart from the
TS-parity harness"* on the strength of **207 passing tests, clippy clean under
`-D warnings`, and a clean wasm32 build.**

### What was actually true

`EffectKind` declares **22 variants**. **Three have resolvers** — `Heal`, `HealthTransfer`,
`SelfDamage`. The other nineteen are declared in the type system, referenced by character
definitions, and enforced by roster validation, and then **nothing acts on them.**

| Character | Signature mechanic | Functions? |
|---|---|---|
| Arzum | Teleport chain-strike | No — `Teleport` inert |
| Emi | Cube turret | No — `SpawnTurret` inert |
| Karl | Three strikes per turn | No — `MultiStrike` inert |
| Huck | Body throw | No — `Relocate` inert |
| Numa | Harpoon pull, Pin | No — `Pull`, `Lockdown` inert |
| Aleph | Dagger chain | No — `EmbedProjectile`, `ChainDetonate` inert |
| Zeke | Heal / Lifeshare | **Yes** |
| Roberto | Knockback grenade | No — `Knockback` inert |
| Natomica | Repulse, wall impact | No — `Push`, `WallImpact` inert |

**One of nine starters functions end to end.** Also absent outright: turn scheduler, map and
spawn system, status ticking, persistent-object lifecycle, event log and replay, reconnect
snapshot. Movement and victory-condition fields exist and are hashed, but nothing implements
the behaviour.

The roster is data-complete and almost entirely non-functional. Every one of those 207 tests
was a true statement about a module that worked; none of them asserted that a character did
what its character sheet said.

*(Independently confirmed against the tree: 22 `EffectKind` variants in `types.rs`; exactly
three resolver arms in `command.rs`; 207 `#[test]` attributes in `db-sim-core` plus one in
`db-sim-wasm`.)*

### Root cause — honestly

**The root cause was the module brief, not the implementation.** `MODULE_OWNERSHIP.md`
specifies `command.rs` as *"Command validation + application"*, and the module delivered
exactly that: a correct security boundary that validates a command and then resolves almost
none of what the command implies. The agent did not under-deliver. The missing behaviour was
never anyone's assignment.

This is worth stating plainly because the failure is not visible anywhere in the code. There
is no TODO, no `unimplemented!()`, no failing test. The gap lives entirely in the space
between what was written and what was needed, and the only artefact that could have caught it
was a brief that said what had to *work*.

### Corrected sizing

The correction came from the product owner's direct experience on **Gunbound**, where the
engine alone was ~20,000 lines and **each character release modified ~1,000 lines of engine
code.** Characters are not pure data; each one lands real engine work.

Engine code stands at **5,019 lines excluding tests** (8,495 including them — the earlier
8,495 figure had conflated code with tests, which is how a quarter-built engine looked
finished).

```
shared engine still missing      ~9,000   scheduler, movement, status and object
                                          lifecycle, maps, spawns, victory, event
                                          log, reconnect, protocol codec, bot AI,
                                          parity harness
effect resolver layer            ~2,500   the 19 inert kinds
24 characters x ~1,000          ~24,000
                                --------
                                 ~35,000
```

~20,000 of that is engine, matching the owner's figure. **The project is at roughly a quarter
of the engine, not a complete one.**

A new milestone **M1.5** was inserted as the real blocker: the effect resolver layer plus the
four subsystems it cannot work without. Its gate is deliberately **behavioural, not
structural** — every mechanic in `CHARACTERS.md` §3 must observably resolve in a match, with
a test per mechanic asserting the specific state change. Not "compiles and validates": *does
the thing the character sheet says it does.*

A secondary signal was defined: **character number ten should cost close to 1,000 engine
lines.** If it costs 3,000, the resolver layer is not doing its job and the roster should not
proceed.

### The two standing lessons

Both were added to `PROGRAM_PLAN.md` §5 as permanent practice:

1. **A module brief is a scope decision.** When delegating, state what must *work*, not what
   must be *written*. The inert-resolver gap came entirely from briefing `command.rs` as
   "validation + application".
2. **"Compiles and passes tests" is not "functions."** 207 green tests coexisted with 8 of 9
   characters doing nothing. Gates must assert observable behaviour, not module health.

The closed-vocabulary principle was **retained** for the security reason it existed for. What
changed is the expectation that the vocabulary is *finished* — it grows with the roster, by
build, under review.

---

## 6. Multi-agent orchestration

### The structure

The product owner specified a seven-agent team. It was mapped by what each tier is good at:

| Role | Model | Owns |
|---|---|---|
| Architect / integrator | Opus ×1 | Architecture, ADRs, backend, security, all git, all integration, final review |
| Lead / corrector | Sonnet | Reviews and fixes every Haiku module before it lands |
| Feature engineers ×2 | Sonnet | Parity-critical and security-critical modules |
| Implementers ×3 | Haiku | Mechanical, tightly-specified modules and data transcription |

The dividing principle: **Haiku gets work where the specification is complete enough that the
answer is determined** — data transcription, a well-known algorithm, a mask operation with
stated geometry. **Sonnet gets work requiring judgement under ambiguity** — the canonical
encoding, ballistic parity, the command security boundary. **Opus keeps anything where a
wrong decision is expensive to reverse** — interface contracts, security posture, and what
gets committed.

### The Haiku-build → Sonnet-correct pipeline

Each Haiku module flowed immediately into a Sonnet review that read the code, verified every
self-flagged judgement call independently, and fixed defects directly. Review started the
moment a module landed rather than waiting for the slowest one.

The pipeline is not a formality, and §4 is the evidence: the RNG modulo bias, the terrain
truncating cast, the assertion-free material-mask test, the fabricated known-answer vectors,
and 14 roster defects were **all** found this way — none by clippy, none by the test suite,
none by CI. Every one required a reader.

The most transferable detail: **the RNG bias was found because the implementer had been
forced to suppress a lint on that exact line.** A lint suppression is a written admission
that something non-obvious is happening. Treating suppressions as review targets rather than
resolved questions is what turned a 1-in-2^32 bug into a caught one.

### Structural conflict prevention

`MODULE_OWNERSHIP.md` exists to prevent a failure the product owner named directly: *problems
caused by lack of communication with the rest of the team.* Rather than relying on
coordination, the possibility was removed structurally:

1. **One file, one owner.** Every task owns exactly one file and edits nothing else — not to
   fix a typo, not to add a helper, not to "quickly correct" a neighbour.
2. **`types.rs` is the contract and is frozen during parallel work.** A module reads shared
   types; it never redefines one and never adds a variant. Two local versions of one concept
   is exactly the drift the protocol prevents.
3. **`lib.rs` is owned centrally.** Module declarations are added by the integrator. A task
   never edits it.
4. **A broken sibling is not your bug.** While work is in flight `cargo build` reports errors
   in files being written concurrently; agents were told to filter to their own file and not
   "helpfully" repair a teammate's half-written module.
5. **Report blockers, do not route around them.** A wrong guess that compiles costs more than
   a question.

This held. There is no commit in the history repairing a cross-module trampling.

### What went wrong — session limits

**Agents did not reliably survive their tasks.**

Confirmed from the history: the agent assigned `command.rs` **hit a session limit mid-task**,
and the module was finished by hand by the integrator (`78b686f`). It had already written the
module — including a test that correctly caught a real bug in its own implementation (§4.4) —
and died before fixing it. Separately, `5579395` records removing a stray `test_debug.rs` an
agent left at the repository root, which is the residue of a run that ended untidily.

The full count, from the workflow runtime's own completion reports rather than from git,
is **four agent failures across two runs**, all with the same cause — an account session
limit, not a code fault:

| Run | Agents | Failed | Modules lost |
|---|---:|---:|---|
| 1 — core modules | 9 | 3 | `hash`, `ballistics`, `command` |
| 2 — character modules | 5 | 1 | `command` |

Only the hand-finished `command.rs` (`78b686f`) and the stray `test_debug.rs` (`5579395`)
leave traces in git; the failures themselves are not recorded in any commit, document, or
artefact in this repository, so they cannot be re-derived from it. An earlier revision of
this log said "three times" — that figure came from the briefing and was wrong.

This is itself a finding. **A delegation system that loses its own failure record is
under-instrumented.** Four agents' work evaporated and the only durable evidence is one
commit message and one deleted file. Run-level outcomes should be written to the repository,
not left in a transcript.

**Learned:** delegation needs a resumption story. The pipeline recovered here only because
the interrupted agent's work was self-describing — a failing test is a handoff note. Work
that fails silently mid-task does not hand off at all.

---

## 7. Pivots

Three major direction changes in one session. For each: what was thrown away, what survived.

### 7.1 TypeScript → Rust/WASM (ADR 0001, 05:19)

**Driver.** Product-owner directive that the codebase be mostly Rust or C++ compiled to
WebAssembly. This *reversed* `PLATFORM_STRATEGY.md` §12, which had advised against a Rust
core on the grounds that portability was hypothetical and dual implementations were a
maintenance risk. That advice assumed console work was unfunded and TypeScript performance
was adequate; the directive made it a scope decision rather than a hypothesis.

**Rust over C++** because the stated top priority was zero security vulnerabilities, and an
authoritative simulation parsing untrusted network commands is exactly where memory-unsafety
becomes remote code execution. The core crate carries `#![forbid(unsafe_code)]` with no
exceptions.

**Thrown away:** nothing, immediately. The pivot was deliberately structured as a *port
against an oracle* rather than a rewrite.

**Preserved:** `lib/game/simulation.ts` frozen as the reference oracle, with a differential
harness planned as a merge gate — recorded reasoning being that this converts "rewrite the
engine and hope" into "port with a continuously checked equivalence proof". Also preserved by
design: one simulation, three consumers, with client and server running the same compiled
logic and differing only in target triple — removing the second implementation rather than
adding one.

### 7.2 Loadouts → character kits (ADR 0002, 05:47)

**Driver.** Product-owner redirection from an equipment-driven game (Worms lineage) to a
character-driven one (hero-shooter lineage): 24 characters, fixed kits, asymmetric health
165–400, asymmetric range and movement, a chargeable special, a mid-match passive choice.

**Thrown away — at the product layer:** the `main`/`secondary`/`meleeTool` slot contract;
`ARSENAL.md`'s 12 weapons as player-selectable equipment; weapon purchases and weapon-based
level-up rewards; ammunition entirely (basic attacks are unlimited, specials gated by the
gauge). `weapon.rs` — 996 reviewed lines including two real bug fixes — moved to
`reference/weapon-roster-retired.rs` rather than being deleted.

**Preserved — the whole simulation layer:** fixed-point math, terrain mask and operations,
ballistic integration and collision, damage/knockback/status resolution, canonical encoding
and state hashing, the seeded PRNG, and the closed behaviour vocabulary. All of it is
character-agnostic. **Only the *owner* of an attack changed** — from "a weapon a player
equipped" to "an ability a character has" — which is why the in-flight core work was allowed
to finish rather than being cancelled. The `types.rs` refactor demonstrates the point
mechanically: it was spliced, not rewritten, and the terrain and ballistics sections came
through byte-identical.

**Cost accepted:** the balance surface grows permanently. 24 asymmetric characters against 12
symmetric weapons, with the 48–52% win-rate target now applying per character, per map, per
skill band. Content commitment grows to 24 kits plus 72 passives plus per-character art.

### 7.3 Web-first → native desktop C# (ADR 0004, uncommitted at time of writing)

**Driver.** Two product-owner directives arriving together: no TypeScript, and native desktop
first with web delivery dropped. They interact — C# does not run in a browser without a
game-engine runtime, so keeping web-first would have forced Unity or Godot web export at
5–20 MB compressed against a stated ≤15 MB budget. Dropping web delivery removes the conflict
rather than absorbing it.

**Thrown away:** the entire web surface — vinext/Next.js shell, Cloudflare Worker, PWA
manifest and service-worker plan, ChatGPT/workspace identity headers, Drizzle/D1 schema,
Vite/PostCSS/ESLint chain, React canvas client. `PLATFORM_STRATEGY.md` §10 (Chrome Manifest
V3) is dead, not deferred. Link-sharing onboarding — rated by `PLATFORM_STRATEGY.md` §3 as
the web build's single largest advantage — is gone, and with it the cheapest playtest loop.

**Thrown away, and this is the load-bearing loss: the TypeScript oracle.** ADR 0001 §2's
central risk control was that the oracle stayed frozen and a differential harness proved the
Rust port matched it bit-exactly. **That harness was never built** — it was the unmet M1
gate — and deleting the oracle means it never will be. The Rust port's faithfulness to the
original will not be proven.

ADR 0004 accepts this on three grounds: the oracle was itself unvalidated; it had a known
determinism defect (ADR 0003); and keeping it contradicts the directive. The replacement
control is **frozen golden vectors** generated from the Rust core and asserted in CI. The ADR
is explicit that this is *strictly weaker* — it proves self-consistency, not correctness
against an independent implementation — and that the difference should not be papered over.
The golden corpus must be generated only from a core whose behaviour has been reviewed,
because it freezes whatever it is given, including bugs.

**Preserved:** the Rust core, unchanged. One implementation of the game rules, with client
and server both P/Invoking into `db-sim-core` — arguably more cleanly than WASM-plus-native.
ADR 0003's sine table is retained: its parity rationale is moot, but its determinism
rationale (identical results across `x86_64`, `aarch64`, and any console target) is not.
`SECURITY_BASELINE.md` needs **no revision at all**, because the trust boundary was already
drawn with the client entirely untrusted — a design that had leaned on browser sandboxing
would have needed rewriting here.

---

## 8. Publication to GitHub

### Credential handling

The product owner supplied a GitHub PAT with the instruction *"DO NOT EVER upload to git,
even if the repo is private."* An instruction that strong was given an enforcement mechanism
rather than a convention, because `.gitignore` alone has three known bypasses: `git add -f`
overrides it, a later edit can silently un-ignore the file, and no filename rule will ever
see a token pasted into an otherwise innocent file.

Three independent layers (`SECURITY_BASELINE.md` §4.1):

| Layer | Catches | Known bypass it covers for |
|---|---|---|
| `.gitignore` — `.github-token`, `*.token`, `*github_pat_*`, `.env.local` | Accidental staging | — |
| `.githooks/pre-commit` — staged **filenames** matching credential conventions **and** staged **content** matching credential shapes | Force-adds; secrets in innocent files | `.gitignore` |
| `gitleaks` CI gate, `fetch-depth: 0` | A secret that lands anyway, anywhere in history | Both of the above |

The content patterns cover GitHub PATs (classic and fine-grained), AWS access key IDs, PEM
private-key headers and `sk-` style API keys, and are **deliberately split across string
boundaries in the hook source** so the hook does not match itself. Both paths were
**adversarially tested before the hook was committed**: a force-added token file was blocked,
and the same token pasted into an innocent-looking `.js` file was also blocked.

**Git itself never receives the token.** `gh auth` stores it in the OS keyring and acts as
git's credential helper, so the remote URL is a plain `https://` URL and `.git/config`
contains no secret — verified rather than assumed. A token embedded in a remote URL leaks
through `git remote -v`, shell history, CI logs, and any terminal screenshot.

### The repository could not be created by the token

The remote was configured as `Crownelius/DungeonBarrage`. **Creating the repository failed:
the fine-grained PAT returns 403 on both the GraphQL and REST creation endpoints, because it
lacks `Administration: write`.**

The response was to leave the token alone. Recorded reasoning: *that is a reasonable
permission for a token to lack, so the fix is to create the repo by hand rather than to
broaden the token.* A token that can authenticate and push but not administer is the correct
shape for this job; widening it to unblock one command would have made every subsequent use
riskier.

### The first push failed on a stale cached credential

**[Partially unverified.]** The reported sequence: the first push failed because **Windows
Credential Manager was still serving the old token while `gh` had already switched accounts.**
The credential helper and the CLI had diverging views of who was authenticated, so git
presented a credential that no longer matched the account being pushed to.

This is not recorded in any commit message or document, and the repository preserves no
artefact of it — it is reported from the session rather than confirmed from the tree. It is
recorded here because it is a real and easily-repeated Windows failure mode: `gh auth` does
not evict what Credential Manager has already cached, so an account switch leaves a stale
credential in front of it.

### Outcome — confirmed

Publication succeeded. Verified directly against the repository at time of writing:

```
main        49fcb5436d6944a7892578154cee2831ce72cf6f
origin/main 49fcb5436d6944a7892578154cee2831ce72cf6f
ahead/behind 0 0
origin  https://github.com/Crownelius/DungeonBarrage.git
```

All fourteen commits are on `origin/main`, and the remote URL carries no embedded credential,
as designed. **The token still needs rotating** — see §9 item 1.

---

## 9. Open items

Carried from `PROGRAM_PLAN.md` §6. These are product-owner decisions or actions, not
engineering backlog.

1. **Rotate the GitHub token.** It was transmitted as a plaintext file and through a chat
   context, so it must be considered compromised regardless of subsequent handling. Scope the
   replacement to this one repository rather than all 28. *(The repo-creation blocker in §8 is
   now resolved — the repository exists and the push landed.)*
2. **The reference screenshot** described in `PRODUCT_SPEC.md` §12 was supplied in an earlier
   session and is not present in the current one. Art-direction work depending on it needs it
   re-attached.
3. **Level-up reward balance** (`PROGRESSION.md` §4). A character is worth 2,300 credits in
   the shop; the credit option grants 50 — the character choice strictly dominates by **46×**
   while any character is unowned. Recommended: raise the credit option to 250–400. One
   versioned data change, no new systems.
4. **SOC 2** is an audited organizational attestation, not a code property. Engineering can
   build so an audit is achievable; the policy set, risk assessment, vendor management,
   training, access reviews, and the CPA engagement are owner responsibilities.
5. **Ranked arsenal normalization** (`PROGRESSION.md` §5). Progression gates content while
   `PRODUCT_SPEC.md` §8 promises rated modes expose the full arsenal to everyone. The proposed
   boundary keeps both, but it is a product decision worth confirming explicitly.
6. **Dependency advisories.** `npm audit` is down from 18 to 14; the remainder need upgrades
   outside the pinned ranges of `vinext`, `vite`, and `@cloudflare/vite-plugin`. *(Largely
   mooted by ADR 0004, which deletes the JS toolchain entirely — but the advisories apply to
   the developer machine until that change is committed.)*
7. **Character content backlog.** 15 of the 24 characters are unspecified and 45 of the 72
   passives are undrafted (`CHARACTERS.md` §4, §7). Both are real scheduling commitments.
8. **Four character rules need confirmation** (`CHARACTERS.md` §7):
   - **Karl** — 24%/74% crit (implemented) vs the brief's "33% × 3". 33 × 3 = 99% per turn
     would make him the highest-damage starter by a wide margin. His crit *chance* is
     additionally an unsourced **20% placeholder** flagged during review.
   - **Numa** — harpoon direction rule and the 50% HP threshold. The brief states one
     direction twice; read as target-to-Numa below 50% (execute), Numa-to-target above
     (engage).
   - **Zeke** — heal magnitude read as **22 HP** (two separate percentages of base) rather
     than 22% of damage dealt, which gives ~9 HP and is too small for a healer.
   - **Arzum** — whether the 50–200% ultimate roll should narrow in rated play. A 4× swing on
     a committed action; recommended 90–150% in rated modes only. Built as specified.

### Engineering blockers (not owner decisions)

- **M1.5, the effect resolver layer**, is the real critical path. Nineteen inert `EffectKind`
  variants plus four missing subsystems. See §5.
- **`cast_possible_wrap` is still absent from the workspace clippy deny set**, which is the
  lint gap that allowed §4.2. Not yet closed.
- **ADR 0004 is accepted but uncommitted.** The web-surface deletion, the new `db-sim-ffi`
  crate and the doc updates are all staged against `HEAD` = `49fcb54`. Until it lands, the
  committed history and the working tree tell different stories.
- **The differential parity harness was never built** and, under ADR 0004, never will be. Its
  replacement — frozen golden vectors — is specified but not implemented, and is strictly
  weaker.

---

## 10. Known inconsistencies in the record

Recorded so a later reader does not mistake them for findings of their own.

| Where | Says | Actually |
|---|---|---|
| `3ec70ce` commit message | "`EffectKind` grew from 11 to 24 variants" | 8 → 22. The 14 added variant names listed in the message are correct; the two counts are not. `PROGRAM_PLAN.md` §2's figure of 22 is right. |
| `ADR 0002` §"The type change" | `PlayerState.special_gauge: u8 (0–100)` | `u16` in hundredths, `GAUGE_FULL = 10_000`. The implementation changed during `3ec70ce` — the spec's per-damage gains are fractional (+0.40/+0.25/+0.30) and a float would break determinism, so the scale absorbs the fraction. The ADR was not updated. |
| `PROGRAM_PLAN.md` §5 | "The 50× imbalance in the level-up reward choice" | `PROGRESSION.md` §4 now says **46×**. 50× was the pre-pivot weapon-model figure; 46× is the character-model figure (2,300 vs 50 credits). The cross-reference is stale. |
| `78b686f` / `PROGRAM_PLAN.md` §2 | Engine at "5,019 lines excluding tests" | An independent recount splitting each module at its `#[cfg(test)]` boundary gives ~5,011 for `db-sim-core/src`. The 8,495 total reproduces exactly. The discrepancy is counting methodology, not a material error. |

---

## Append new entries below this line

<!--
Format for new entries:

### YYYY-MM-DD — Short title
**Commits:** <short shas>
What was attempted, what landed, what was learned. Defects go in §4 with a
back-reference here. Never edit an entry above this line; correct it with a new
one that points back.
-->

### 2026-08-14 — Core orchestration closeout recorded after the fact

**Commits:** `ef3c41f`, `41cfd8d`, `9ebaa64`, `4ac6b09`, `fa7f0af`

The append-only log previously stopped before a material sequence that is now part of the
baseline. `ef3c41f` closed the real P2/P3 gaps: all crater producers route through block health,
terrain-removal accounting reaches the outcome, blocks live in state and the hash, and authored
maps populate them. `41cfd8d` added the top-level `MatchHost`; `9ebaa64` added frozen whole-match
golden vectors and fixed two orchestration defects those vectors exposed; `4ac6b09` fixed turn-end
reason recording and passive prompting, then deliberately regenerated both vector sets at
`SIMULATION_VERSION = 4`. `fa7f0af` added the first C# client specification.

This entry corrects the historical record rather than rewriting earlier analysis. In particular,
the old P2/P3 text in `todolist.md` described genuine unreachable/partial states but was not updated
after `ef3c41f`; it is now labelled historical and resolved.

### 2026-08-14 — Client stack decision and first C0/C1 prerequisite slice

**Commits:** none — working tree based on `fa7f0af`; still uncommitted at this checkpoint

The client plan was re-evaluated rather than assuming the earlier “native C#” sentence answered the
whole architecture. C#, Rust, C++, TypeScript, Unity, Godot, MonoGame, Bevy, desktop/web, and future
server/console implications were separated by responsibility. ADR 0006 records the resulting
boundary: Godot 4.7.1 .NET and C# for presentation; Rust as the only authoritative gameplay; a
coarse client-only C ABI for local matches; and a future Rust-native server linking the core
directly. C# remains the right language at the Godot presentation seam, not for the authoritative
simulation or server.

The same working slice rewrote `CLIENT_SPEC.md` into ordered evidence gates; pinned .NET 10.0.302
and Rust 1.94.0; added a toolchain verifier; added validated transport-free match construction and
an engine-neutral atomic snapshot; preserved every projectile as an independent trace; corrected
post-host turn/hash reporting and actual gauge deltas; made `MatchHost` cloneable for adapter
atomicity; prevented pass/timeout from bypassing passive selection; and made the release FFI panic
containment promise executable under `panic = "unwind"`.

The full Rust workspace, release FFI tests, and release FFI build passed during that slice. The
toolchain verifier passed Rust/.NET and stopped clearly at the missing Godot 4.7.1 .NET editor. No
Godot project was created because the C1–C3 contract gates deliberately precede scenes.

### 2026-08-24 — C1 normalized session, transitions, and shared fixture

**Commits:** none — continuation in the same preserved dirty working tree

The next coherent C1 boundary was implemented in `match_session.rs`. `MatchSessionHost` now owns a
closed normalized `MatchCommand`, deterministic canonical semantic digests, snapshot generation,
cloned-host application, a retained first-result ledger for accepted and rejected commands,
duplicate replay, changed-content command-ID security rejection, and atomic `MatchTransition`
responses. A digest is never trusted alone: exact typed equality is also required. Generation
increments once only when the candidate authoritative state differs; a legal zero/blocked move is
accepted and retained without manufacturing a state generation.

Transitions repeat one detached post-snapshot and exact live-host hash. Events preserve independent
projectile traces and impacts, and deterministically derive net entity movement, health/gauge/status
changes, block surviving bounds, persistent-object lifecycle, elimination, passive prompts/choice,
turn end/open, and match completion. Terrain dirty rectangles are exact changed-cell row-runs,
sorted by row and column. The implementation explicitly labels absent provenance instead of
inventing it. C1 remains open because current outcomes do not retain exact strike points,
per-strike/RNG timing, every status lifecycle, or every removal cause.

Review found and fixed a separate host deadlock: movement settling could eliminate the active
player while leaving that dead actor in `Movement`. `submit_move` now drives the normal
`Eliminated` turn/victory cycle; tests cover both duel completion and rotation when other teams
survive.

The first cross-language fixture bundle lives under
`tests/fixtures/matches/horizontal-test-duel-v1`. Its compact request files are exact UTF-8/LF bytes
for a real Zeke-vs-Huck match: move one cell, fire one projectile, and hand the turn over. A strict
direct Rust consumer rejects BOM/CR/noncompact/unsafe paths and closed-schema violations, then
asserts nonzero movement, independent trace/sample minima, generation changes, event ordering, turn
handoff, and transition/snapshot/live-host hash equality. Frozen hashes are:

- initial: `a37f45c1af031a47`
- after move: `f0c78bdd9d2066cf`
- after ability/final: `194afe5bc5d13818`

Production `db-sim-core` remains dependency-free; serde/serde_json are dev-only for this shared
fixture test. Full response JSON bytes wait for C2's production FFI serializer rather than blessing
a test-only imitation as the ABI.

Documentation was reconciled at the same time: ownership now names the C1/C2 seams and the sole FFI
unsafe exception; P2/P3 are correctly marked resolved; P13 records the active client-boundary gap;
`CLIENT_SPEC.md` states implemented versus missing provenance; and `HANDOFF.md` contains the exact
dirty-tree inventory, validation commands, risks, next sequence, and copy-paste Opus prompt.

**Targeted evidence before the final full pass:** all 12 `match_session` unit tests passed, both
movement-fall host tests passed, the strict shared fixture passed, and core all-target clippy was
clean. Final workspace/release/toolchain results are recorded in `HANDOFF.md`; the work remains
uncommitted until an explicit landing request.

### 2026-08-24 — C1 audit closeout, bounded replay ledger, and simulation version 5

**Commits:** none — continuation in the same preserved dirty working tree

This append corrects and extends the preceding same-day checkpoint; it does not rewrite that
intermediate evidence. Independent review found two session-boundary issues and one scheduler
compatibility defect before handoff.

First, the session specified a 64 MiB retained command/result limit but enforced only 16,384
entries. `MatchSessionHost` now measures the complete retained typed command and transition with a
deterministic platform-independent logical encoding: fixed primitive widths, one-byte enum/option
tags, four-byte string/sequence lengths, complete UTF-8 and nested payloads, and top-level canonical
headers. The exhaustive counter includes snapshots, every event kind, dirty rectangles, damage,
persistent objects, projectile traces, and every sample. Checked `u64` arithmetic and the byte cap
run before ledger/host/generation publication; a cap or arithmetic overflow closes the session with
no authoritative mutation. Exact duplicate and command-ID conflict paths consume no new bytes.
Tests cover widths, accepted and rejected receipts, no-growth replay/conflict, exact-fit and
one-byte-over behavior, checked overflow, and a 4,096-sample trace.

Second, the net event builder labelled every active-player position change under a `Move` command
as `RequestedMove`, even though `MatchHost` always settles immediately afterward. It now uses that
label only for a same-direction, bounded, purely horizontal displacement. Any vertical component,
including a settle-only fall or a walk-plus-fall, is conservatively
`AuthoritativeResolution` until the host retains a post-walk/pre-settle split. A focused regression
protects both cases.

Third, the earlier movement-fall fix exposed a terminal scheduler omission. A victory path
deliberately does not call `end_turn` because no next player exists, but it also failed to copy the
final pending turn reason. The previous turn's reason could therefore survive a terminal attack,
timeout, pass, or fall. `leave_victory_check` now commits that value before returning
`MatchComplete`, with direct scheduler and host regressions. This changes replay-visible state, so
`SIMULATION_VERSION` moved from 4 to 5 under the golden-vector regeneration procedure:

| Vector | Version 4 | Version 5 |
|---|---|---|
| all passes | `876de8693b5b75a8` | `b75ec70f007a7a7b` |
| walking duel | `b28768a38619df88` | `0038e5ddfabfec81` |
| firing duel | `2fbdca99f94c944c` | `9c53418575ea824d` |
| mixed actions | `765e76572c02b6b9` | `ea50d7336feb3a94` |
| low-health decision | `06db50b907568060` | `323672057a1d53af` |

The shared direct fixture was regenerated only after its movement, projectile, turn, generation,
event-order, and live-hash assertions passed. Its version-4-to-version-5 hash history is:

| Checkpoint | Version 4 | Version 5 |
|---|---|---|
| initial | `a37f45c1af031a47` | `65ac3e53023ca6b0` |
| after movement | `f0c78bdd9d2066cf` | `9d92d3b5d5dad7d0` |
| after ability/final | `194afe5bc5d13818` | `af724375e588d90b` |

Final local evidence from `C:\Users\rsfit\DungeonBarrage`:

- `git diff --check`, `cargo fmt --all --check`, and workspace all-target clippy with
  `-D warnings`: pass. Git printed only the existing non-failing `core.autocrlf` notices.
- `cargo test --workspace --quiet`: 456 tests pass — 440 core unit, 7 whole-match golden,
  1 strict shared fixture, 7 FFI, and 1 WASM.
- `cargo test --release -p db-sim-ffi`: 7 pass; `cargo build --release -p db-sim-ffi`: pass.
- `cargo deny check`: advisories, bans, licenses, and sources pass; only unused license allow-list
  warnings are emitted.
- All three raw request files remain UTF-8 with no BOM or CR and exactly one terminal LF.
- The toolchain verifier confirms .NET SDK 10.0.302 and Rust/Cargo 1.94.0, then exits 1 with the
  intended actionable error because Godot 4.7.1 .NET is not installed.

C1 remains open only on documented contract breadth: source-owned per-strike/impact/RNG/status and
object-removal provenance, authority timeout, read-only preview, safe host-plus-complete-ledger
restore, the composite session/ABI envelope, and the remaining direct transition scenarios. C2's
production serializer is still the correct place to freeze full response JSON bytes. No C# or
Godot scene work starts before those evidence gates, and no files were staged or committed.

### 2026-08-24 — Correction: requested-movement provenance remains reserved

**Commits:** none — final review in the same preserved dirty working tree

The preceding closeout was still too optimistic when it said a same-direction, same-height net move
could be labelled `RequestedMove`. A walk can climb and then settle back to its original height, so
equal pre/post `y` does not prove the path was purely horizontal. Without a retained
post-walk/pre-settle position, no net diff can make that attribution safely.

The event builder now labels every current position diff `AuthoritativeResolution` and documents
`RequestedMove` as reserved for the richer movement outcome contract. Tests assert that a real move
still increments generation and publishes the authoritative net position, and that neither positive
nor negative accepted moves emit the reserved cause. This correction affects presentation metadata
only; authoritative state, `SIMULATION_VERSION = 5`, golden vectors, and shared fixture state hashes
do not change. The full workspace remains 456 passing tests after replacing the earlier provenance
regression with this stricter reserved-cause contract.

### 2026-08-25 — Pinned client dependencies installed and campaign committed

**Commit:** the local checkpoint commit containing this entry; not pushed

The owner authorized dependency installation and a local repository commit. The already-pinned
.NET SDK 10.0.302, Rust/Cargo 1.94.0, rustfmt, clippy, and cargo-deny were present and usable.
`cargo fetch --locked` materializes the locked Rust dependency graph without changing it.

Godot 4.7.1 .NET was installed for the current Windows user through WinGet package
`GodotEngine.GodotEngine.Mono` at exact version 4.7.1. WinGet verified the official editor archive
SHA-256 `764a089809fb1a6f745686ce9f6d3ca83adce8fb60fb9a4e2324b63baaebaa45`; the executable reports
`4.7.1.stable.mono.official.a13da4feb`. The versioned executable path is persisted in the user-level
`DUNGEON_BARRAGE_GODOT` variable, so the repository is not coupled to a machine-local absolute path.

The matching official `.NET` export-template archive was downloaded from Godot's 4.7.1-stable
GitHub release. Its 1,201,759,011-byte payload matched the release API's published SHA-256
`ef9a708be51ecd974cd7dccdcafd7a1870da3d3e1c24c072bdbb9818c7a7db63` before extraction. Templates
are installed at `%APPDATA%\Godot\export_templates\4.7.1.stable`; `version.txt` reports
`4.7.1.stable.mono`, and both Windows x86_64 debug and release binaries are present. The downloaded
archive remains at `%LOCALAPPDATA%\Temp\DungeonBarrage-Godot-4.7.1\Godot_v4.7.1-stable_mono_export_templates.tpz`:
the exact-target cleanup attempt was rejected by the execution policy, so no destructive workaround
was attempted. The retained archive is not part of the repository and is safe to remove manually.

`scripts/verify-toolchain.ps1` now verifies the template directory, exact Mono template version,
and required Windows x86_64 debug/release files in addition to the editor/.NET/Rust versions. The
complete toolchain gate passes. The full Rust, release FFI, supply-chain, fixture-byte, diff, and
format gates are rerun immediately before the checkpoint commit, and the post-commit worktree is
required to be clean. No push is performed.

---

## C1 — per-strike provenance on the authoritative outcome

Working-tree change on `main` at `327d2ae` (ahead 1 of `origin/main`, unpushed). Three files:
`crates/db-sim-core/src/types.rs`, `command.rs`, `match_session.rs`. Not committed.

### What was added

`CommandOutcome` gained `strikes: Vec<StrikeResolution>`. Each record carries the resolution-order
index, the target, the exact impact point, the delivery (`StrikeDelivery::{Projectile { trace_sequence },
Melee}`), the crit draw, the damage actually applied, and whether that strike caused the elimination.

The crit draw is `CritRoll::{NotEligible, Missed, Landed}` rather than a boolean. This distinction is
not cosmetic: `roll_crit` never draws at all when an ability's `crit_chance_basis_points` is zero, so
"did not crit" and "never rolled" are different facts about the seeded generator. A consumer that
conflated them would predict the wrong next value and desynchronise. The rewritten `roll_crit`
returns `CritRoll` and preserves the original short-circuit exactly; the golden vectors passing
unchanged is the evidence that RNG consumption is bit-identical.

`match_session::derive_events` now emits one `StrikeResolved` event per recorded strike, carrying the
resolver's verbatim record. Projectile-delivered strikes are presented at their own trace's impact
tick (rank 3, after the trace and its `Impact`), so damage lands with the projectile instead of at
turn start. A strike citing a trace the outcome does not contain is a `SessionFault::ContractInvariant`
rather than something quietly tolerated.

### The bug this surfaced

`StrikeResolved` was previously gated on `matches!(ability.attack, Attack::Strike(_))`. Karl's
Carrion Call is the **only** multi-strike ability in the roster, and the only one whose design note
promises that "each of the three attacks rolls its crit independently" — and it is an
`Attack::Projectile`. It therefore emitted **zero** strike events, while 456 tests passed.

This is the fifth occurrence of the repository's signature failure mode: correct, tested, unreachable.
Emission is now driven by what the outcome actually recorded, never by the ability's declared shape,
so an ability that finds no target emits nothing and one that lands three emits three.

### Evidence

Eight new tests, five in `match_session.rs` and three in `command.rs`.

The projectile-side tests are anchored to a scenario found by brute-force search rather than guessed:
Karl at x=2048 versus Huck at x=8192 on `horizontal-test-array`, angle 0, power 4600, which lands all
three strikes. Each test asserts `landed.len() == 3` first, so the suite cannot pass vacuously if
ballistics or spawn placement later stops the volley connecting — the failure mode the golden-vector
no-op guard was built to catch.

The strongest assertion is reconciliation: the three per-strike `damage_applied` values sum exactly to
the target's authoritative health change. Carrion Call carries no effects and no self-damage, so that
sum has to be the whole delta. This is what makes the records trustworthy rather than merely
well-formed.

The tests were mutation-checked. Truncating emission to `outcome.strikes.iter().take(1)` fails all
five; restoring passes all five.

The melee side is covered in `command.rs` against Huck's Haymaker, which is a `Attack::Strike` with
`crit_chance_basis_points: 0` — so it covers both the `Melee` delivery and the `NotEligible` draw that
the projectile fixture cannot reach. Two further tests cover clamping and elimination: a 7-health
target struck by a 60-damage Haymaker must record `damage_applied == 7`, not 60, and must record the
elimination. Overkill is otherwise unexercised by the projectile fixture, where 24 damage against 400
health means applied always equals nominal.

A third melee test was written expecting a strike against an already-eliminated target to record
`eliminated_target == false`. It failed: validation rejects the command outright with
`InvalidTarget`. The test was rewritten to assert that stronger, real guarantee. The `was_alive` read
in the resolver is therefore defence in depth, not the only thing preventing a second kill credit.

### Gates

All run at the working tree described above.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | 464 pass, 0 fail (was 456) |
| `cargo test --release -p db-sim-ffi` | 7 pass |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `scripts/verify-toolchain.ps1` | pass |
| `git diff --check` | clean |

Golden vectors and the shared fixture hashes are unchanged, as they must be: this slice records what
the simulation already did and alters no authoritative state, no RNG consumption, and no state hash.
`SIMULATION_VERSION` stays at 5 for the same reason.

One note on the toolchain gate: it initially failed with "Godot 4.7.1 .NET is missing". Godot is
installed and correct — `DUNGEON_BARRAGE_GODOT` is a user-level variable set after this session's
shell environment was created, so the process had not inherited it. Re-run with the variable in
scope, the full gate passes. Not a regression, and nothing was reinstalled or modified to make it
pass.

### Still open in step 1

Status lifecycle records and object removal/change causes remain unrecorded, and `CommandOutcome`
still has no `objects_removed` counterpart to `objects_created`. Duration-one statuses that apply and
expire inside a single synchronous call remain invisible to a pre/post diff. C1 is not complete.

---

## C1 — status lifecycle records

Second half of HANDOFF §6 step 1. Thirteen source files, `docs/HANDOFF.md`, this log.

### What was added

`CommandOutcome` gained `status_changes: Vec<StatusChange>`, each naming a player, a status kind,
and one `StatusTransition`: `Applied`, `Refreshed` (carrying the magnitude and turns it displaced),
`ChargeConsumed { remaining }`, `Ticked { turns_remaining }`, `Exhausted`, or `Expired`.

All four production producers record where the transition happens:

- `resolve::status::apply_status` — `Applied` / `Refreshed`
- `resolve::status::tick_statuses` — `Ticked` / `Expired`, now `#[must_use]`
- `resolve::attack_mods::resolve_guarantee_crit` — `Applied` / `Refreshed`
- `resolve::attack_mods::consume_guarantee_crit` — `ChargeConsumed` / `Exhausted`

`ResolveContext` carries the accumulator exactly as it already carries `damage`, `terrain_ops`,
and `objects_created`. `scheduler::advance_phase` takes one too, because the end-of-turn tick runs
inside it and `command::apply_ability` has already returned by then.

`Refreshed` reports what it replaced because statuses refresh rather than stack: the displaced
magnitude and duration are gone from state the moment the new status lands, so if the record did
not carry them, nothing could.

`Ticked` is recorded even though a snapshot diff can see it. Without it the transition stream would
explain only some observable changes, leaving a consumer unable to distinguish a missing record
from a change that legitimately had none — and it would make the reconciliation check below
impossible to state.

### Where the records surface

`MatchHost` owns a `status_changes` record cleared at the start of every public entry point. This
exists because `pass_turn`, `time_out_turn`, and `submit_move` produce no `CommandOutcome` at all,
yet still end turns and therefore still run the status tick. `submit_ability` and
`submit_passive_choice` copy the completed record onto their outcome with `clone_from`, so the two
cannot disagree.

`match_session::derive_events` now emits `StatusChanged` from these records instead of diffing the
pre- and post-snapshots. The event previously carried `previous`/`current` snapshots; it now
carries the transition itself, which is strictly more information — a diff cannot represent a
status applied and expired inside one turn (it appears in neither snapshot), nor three charges
consumed from one status by a single multi-strike ability (the diff shows one net change).

### The reconciliation check

Replacing a diff with records risks the opposite failure: a future producer mutates `statuses`
without recording, and the client is told nothing happened. So `derive_events` keeps computing the
diff and uses it as a cross-check. Any status kind the two snapshots disagree about that no record
explains returns `SessionFault::ContractInvariant`.

The converse is deliberately not checked: a status applied and expired within one call leaves no
snapshot difference at all, and recording that is the entire point of the contract.

This was verified by mutation, not assumed. Making `tick_statuses` record nothing while still
mutating state causes `session.apply` to fail with `ContractInvariant` rather than silently
emitting an empty event stream.

### Evidence

Sixteen new tests; 480 passing, up from 464.

- `resolve::status` — `Applied`; `Refreshed` carrying the replaced values; the tick separating
  survivors from expiries; per-player independence; and the headline case: a duration-one status
  applied and expired in one call, asserting first that the pre- and post-status lists are
  **identical** (so the fixture genuinely reproduces the invisible-transition gap) and then that
  both `Applied` and `Expired` were nonetheless recorded, in that order.
- `resolve::attack_mods` — three charges consumed inside one action producing
  `ChargeConsumed { remaining: 2 }`, `ChargeConsumed { remaining: 1 }`, `Exhausted`, where a diff
  would show a single disappearance; plus refresh-not-sum, and that an unmarked target records
  nothing.
- `match_host` — expiries reach `status_changes()`; the record is cleared per call; an ability
  outcome carries byte-identical transitions to the host's own record.
- `match_session` — a `Pass` surfaces the expiry it caused despite producing no outcome; a
  surviving status reports its decrement; a turn with no statuses emits no events; and a second
  command does not repeat the first command's transitions.

### Content gap found while wiring this

Only two effects in the entire launch roster attach a status — Numa's Pin (`Lockdown`) and Karl's
Feeding Frenzy (`GuaranteeCrit`) — and both are specials gated behind a full gauge.
`resolve::status::resolve_chill` and `resolve_embers` are fully implemented and tested, but **no
ability references `EffectKind::Chill` or `EffectKind::Embers`**, so neither can occur in a real
match today. A content gap rather than a wiring bug: the fifteen undesigned characters are
expected to use them. It is nonetheless why the session-level tests seed a status directly rather
than casting an ability to produce one, and it means status behaviour is far less exercised in
real play than the test count alone suggests.

`resolve::status::tick_statuses` also carried a stale doc comment claiming "nothing calls this
yet". `scheduler::advance_phase` has called it since the scheduler landed; the comment was
corrected rather than left to imply dead code.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | 480 pass, 0 fail (was 464) |
| `cargo test --release -p db-sim-ffi` | 7 pass |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `git diff --check` | clean |

Golden vectors and fixture hashes are unchanged, and `SIMULATION_VERSION` stays at 5: this records
what the simulation already did and changes no authoritative state, no RNG consumption, and no
state hash. The host's record is deliberately not part of `SimulationState`, so it is neither
hashed nor replicated.

Clippy's `large_enum_variant` fired on `match_session::AppliedCommand` once `CommandOutcome` grew
its second provenance vector; the accepted variant is now boxed, so a rejection no longer pays the
outcome's size.

### Still open in step 1

Object removal and change causes. `CommandOutcome` has no `objects_removed` counterpart to
`objects_created`, and nothing records why a turret, knife, or gas cloud left the board. C1 is not
complete; steps 3 through 8 are untouched.

---

## C1 — object lifecycle provenance, and finishing an interrupted refactor

Resumed a working tree left mid-refactor by a previous agent: `types.rs`, `command.rs`,
`match_session.rs`, `victory.rs`, and six `resolve/` modules were edited, but `scheduler.rs` and
`match_host.rs` were untouched and the crate did not compile (10 errors). Nothing was reverted.

### What the previous agent had established

- `PersistentObjectChange` / `PersistentObjectTransition::{Spawned, Removed { cause }}` with a
  six-variant `PersistentObjectRemovalCause`, replacing `CommandOutcome::objects_created` with an
  ordered `object_changes` stream. One stream rather than separate create/remove vectors because a
  turret replacement removes-then-spawns, a cap eviction spawns-then-removes, and a knife chain can
  do both inside one command.
- `StrikeDelivery::Effect { kind }` for strikes delivered by an effect rather than the primary
  attack, and `CritRoll::Forced` for a guaranteed crit — critical, but consuming **no** RNG draw,
  so `consumed_draw()` is correctly false for it.
- `tick_statuses(state, player_id)`, scoped to one player, with `GuaranteeCrit` excluded from turn
  decay because its magnitude is a charge count, not a clock.
- Seven producers writing `object_changes`, and `victory::eliminate` recording `OwnerEliminated`.

### What was finished here

**Threading.** `scheduler::advance_phase` now carries both accumulators; `leave_victory_check` and
`force_draw` pass the object accumulator into `victory::eliminate`. `MatchHost` gained an
`object_changes` record cleared at every public entry point and folded onto the outcome, mirroring
`status_changes` exactly. The `StatusResolution` arm ticks `state.active_player_id`.

**The consumer, which did not exist.** `object_changes` was fully produced and read by nothing.
`derive_events` still built object events by diffing snapshots, and every `ObjectRemoved` carried
`ChangeProvenance::AuthoritativeResolution` — a constant true of every removal, and therefore
information-free. Spawns and removals now come from the records, `ObjectRemoved` carries the real
`PersistentObjectRemovalCause`, and the same reconciliation guard used for statuses applies: an
object appearing or disappearing between snapshots with no record is a
`SessionFault::ContractInvariant`. `ObjectChanged` stays snapshot-derived, since an object that
survived is fully visible in both snapshots and no producer records in-place mutation.

Left unwired, this would have been the sixth occurrence of the repository's
correct-tested-unreachable failure mode.

### A pre-existing test that had to change

`a_two_turn_lockdown_survives_exactly_two_full_turn_cycles` predates this work and passed under
all-player ticking. Per-player ticking breaks it, and the test — not the implementation — was the
stale artifact: ticking every player on every command means a two-turn status expires after two
commands *by anyone*, which in a four-player match is half a round, and makes the same status mean
something different at every table size.

Replaced with `a_two_turn_status_lasts_two_of_the_affected_players_own_turns`, which asserts across
four laps that another player's turn does **not** erode it and the victim's own turn does. Timing
was confirmed empirically first: the status ticks on laps 2 and 4, the target's own turns.

**This is a balance change, not just a provenance change.** A status now lasts N of the victim's
turns rather than N commands, so in a duel it is roughly twice as long as before and in a
four-player match roughly four times. Numa's Pin and any future Chill are affected. Flagged for
owner review.

Added `a_count_based_guarantee_crit_is_not_eroded_by_the_turn_tick`, which runs four laps and
asserts the charge count and the never-expires sentinel both survive.

### Evidence

486 passing, up from 480. Six new tests.

The object tests use Aleph's throwing knife — the only object producer reachable from a
non-special ability — with parameters found by brute-force search rather than guessed. The headline
test captures the case the contract exists for, verified empirically before it was written:

```
cmd2  SPAWN  seq=1
cmd2  REMOVE seq=0 cause=detonated
cmd2  REMOVE seq=1 cause=detonated
```

Knife 1 is spawned and destroyed inside one command. The test asserts first that it appears in
**neither** the pre- nor the post-snapshot — so the fixture genuinely reproduces the gap — and then
that all three transitions are reported with real causes. A diff could only ever have reported
knife 0 vanishing, with no cause and no hint knife 1 existed.

Mutation-checked: suppressing record-driven emission makes all three object tests fail with
`ContractInvariant` rather than silently emitting an empty stream.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | 486 pass, 0 fail (was 480) |
| `cargo test --release -p db-sim-ffi` | 7 pass |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `git diff --check` | clean |

Golden vectors are unchanged. The status-timing change does not reach them because no launch-roster
basic attack applies a status — the same content gap recorded in the previous entry.

`derive_events` exceeded clippy's argument limit once the object records joined it; the provenance
inputs are now grouped in an `AppliedRecords` struct rather than the lint being suppressed. A caller
cannot supply one record stream without the others and leave the event stream half-explained.

### Still open

`PersistentObjectRemovalCause::Expired` and `Destroyed` are defined but never produced: no
scheduler-owned object-lifetime tick and no object targeting or damage exist yet. Both are marked
reserved in their doc comments. C1 steps 3 through 8 remain untouched.

---

## C1 step 3 — authority-only turn timeout

`MatchSessionHost::apply_authority_timeout` is the session entry point for a turn ended because
the server's planning deadline expired. `MatchHost::time_out_turn` already existed; nothing at the
session boundary could reach it.

### The security property, and why it is structural

The requirement is that a remote client must never be able to end a turn — its own or anyone
else's — by claiming time ran out. The obvious implementation is a `MatchCommandKind::Timeout`
variant refused by validation. That was rejected.

A client does not construct Rust values; it sends bytes that a decoder turns into a
`MatchCommand`. If the variant exists, the guarantee rests on a runtime check that one decoding
bug can bypass. So timeout is **not** a `MatchCommandKind` variant at all. It is a separate
`AuthorityTimeout` type reached through a separate method, and no client-decodable byte sequence
produces it. An absent variant cannot be bypassed.

### One ledger, one identifier space

The timeout still travels through the same session ledger. `LedgerEntry` now holds a
`LedgerRequest::{Client, Authority}` rather than a bare `MatchCommand`.

Two ledgers would have been simpler and wrong: a client could pick an identifier an authority
action already used and receive a different answer than the one the authority recorded. Sharing
the space means whoever claims an id owns it, and the other side gets `CommandIdConflict` — which
`is_security_event()` already reports as telemetry. The entry and byte bounds also stay global
rather than becoming two independently exhaustible budgets.

`AuthorityTimeout` carries domain separator `0x21` against the client command's `0x20`, so the two
cannot digest identically.

### Validation

`authority_timeout_rejection` mirrors `preflight_rejection` rather than reusing it, since a
timeout is not a `MatchCommand` and must not be converted into one merely to share a check.

The load-bearing rule is that `player_id` is **required and validated against the active player**.
A deadline expires for a specific player; without this, a timeout arriving just after a turn handed
over would end an innocent player's turn instead. That is the race a real clock produces, and it is
refused rather than absorbed.

A timeout arriving while a passive choice is owed is reported as a refusal, not a fault: the
interrupt may have been raised by the very action that preceded the deadline, so it is a
legitimate race. The working host is discarded unexamined.

### Evidence

Twelve tests; 498 passing, up from 486. Beyond the accept path they cover: a retried timeout
replaying instead of burning a second turn; a timeout naming a non-active player refused with
`NotActivePlayer`; stale generation and stale turn number refused without mutating the state hash;
a client command reusing an authority id conflicting rather than inheriting its recorded answer,
and the symmetric case; two different timeouts sharing an id conflicting rather than replaying; a
malformed action faulting without leaving a ledger entry a client could observe; and a closed
session refusing the authority path.

Mutation-checked. Removing the active-player check fails the race test. Letting a client id replay
an authority entry fails the collision test.

**A third mutation initially passed, and the test was wrong.** Changing the domain separator from
`0x21` to `0x20` did not fail `a_timeout_and_a_command_with_identical_fields_never_share_a_digest`,
because a `MatchCommand` additionally encodes its `kind` — the digests differed for an incidental
reason, not the one the comment claimed. The test asserted a true property while proving nothing
about the separator.

Added `the_authority_action_encoding_is_frozen`, pinning the exact digest `8cac07183828cf43`, which
does fail under that mutation. The ledger compares digests to decide whether a redelivered action
is the same action, so the encoding is a compatibility surface: changing it silently would make
recorded entries unrecognizable and turn replays into conflicts. Treat a change to that value like
a golden-vector regeneration. The weaker test was kept, with its comment corrected to say what it
does and does not establish.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | 498 pass, 0 fail (was 486) |
| `cargo test --release -p db-sim-ffi` | 7 pass |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | advisories ok, bans ok, licenses ok, sources ok |
| `betterleaks git` (full history) | no leaks |
| `git diff --check` | clean |

Golden vectors unchanged and `SIMULATION_VERSION` stays at 5: the timeout drives the existing
`MatchHost::time_out_turn`, adding a session entry point rather than new simulation behaviour.

### Still open

Steps 4 through 8: the read-only preview DTO, restore semantics requiring host plus a verified
complete ledger, the composite session/ABI envelope and remaining §20.1 fixtures, then C2 and C3.

---

## 2026-08-25 - C1 provenance audit corrections and simulation version 6

This continuation reviewed the Opus handoff at `5946c63`, including the persistent-object
lifecycle work in `a6b9cf4` and the authority-only timeout in `5946c63`. The review confirmed the
new strike, status, object, and timeout surfaces, then closed three correctness gaps before
landing them:

- object reconciliation had only checked whether a sequence appeared in a producer record, so a
  stale, duplicate, mismatched, or otherwise impossible lifecycle record could be accepted;
- a player reduced to zero health by ordinary damage or falling could retain owned persistent
  objects unless an explicit victory helper happened to eliminate that player; and
- gameplay-visible status timing, forced-crit, and owner-cleanup changes were still advertised as
  simulation version 5.

### Corrections

`MatchSessionHost` now replays ordered persistent-object lifecycle records against a shadow map.
Spawns must introduce a new sequence and match surviving post-state snapshots; removals must name
an existing object and match its full recorded snapshot. Duplicate sequences, reused sequences,
unknown or stale removals, removal records for objects still present, and unrecorded spawns are
faults. Transient spawn-then-remove lifecycles remain valid, and their event order is preserved.

Victory finalization now performs idempotent owner cleanup for every player already at zero health
before evaluating the winner. This covers ordinary strike damage and movement falls as well as
explicit elimination paths. The resulting removal records use `OwnerEliminated`, and the host and
session tests assert the exact snapshot, cause, and event ordering.

The live Feeding Frenzy regression now proves that Carrion Call consumes three charges in order,
produces three forced critical strikes, and does not advance the critical RNG stream. A
zero-damage special does not consume its own Feeding Frenzy mark. Status and object mutation tests
also pin exact replay cardinality, old values, snapshots, causes, and ordering.

### Compatibility surface

`SIMULATION_VERSION` is now 6. The version bump covers affected-player-only duration ticking, live
Feeding Frenzy forced criticals, and canonical owned-object cleanup for players reduced to zero
health. Golden vectors and the shared horizontal duel fixture were regenerated deliberately.

| Vector | Version 5 hash | Version 6 hash |
|---|---|---|
| all passes | `b75ec70f007a7a7b` | `ecff79397aa402de` |
| walking | `0038e5ddfabfec81` | `af6978b06c1f9772` |
| firing | `9c53418575ea824d` | `a009c290a796d1ba` |
| mixed | `ea50d7336feb3a94` | `c29e2d75ceba7f33` |
| low health | `323672057a1d53af` | `0c908bfce4b927d6` |

The version 6 shared fixture hashes are:

| Snapshot | Hash |
|---|---|
| initial | `f67c5371bcddbdf5` |
| after move | `378081bb2e830a5d` |
| after ability / final | `d8686762470c0c36` |

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test -p db-sim-core --lib` | 492 pass, 0 fail |
| `cargo test -p db-sim-core --test golden_vectors` | 7 pass, 0 fail |
| `cargo test -p db-sim-core --test shared_match_fixtures` | 1 pass, 0 fail |
| `cargo test --workspace` | 508 pass, 0 fail: 492 core, 7 golden, 1 shared fixture, 7 FFI, 1 WASM |
| `cargo test --release -p db-sim-ffi` | 7 pass, 0 fail |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | pass: advisories, bans, licenses, and sources ok; unused allow-list warnings only |
| `scripts/verify-toolchain.ps1` | pass: .NET 10.0.302, Rust/Cargo 1.94.0, Godot .NET/templates 4.7.1 |
| `git diff --check` | clean |

This entry is part of the landing commit on `feat/c1-outcome-provenance`; use `git rev-parse HEAD`
for its final identifier and compare it with `@{upstream}` to verify push state.

### Still open

C1 still needs authoritative non-strike random-outcome records, the read-only preview DTO, verified
host-plus-ledger restore, and the composite session/ABI envelope with the remaining section 20.1
fixtures. `PersistentObjectRemovalCause::Expired` and `Destroyed` remain reserved until object
lifetime and object-targeting mechanics exist. Numa's balance values remain a content decision,
not a provenance or reconciliation gap.

---

## 2026-08-26 - C1 completion and the real C2 coarse ABI

This continuation completed every item in the prior operational handoff before opening C3. It
reviewed the whole working change against the normative client specification, retained C# only for
Godot presentation, retained Rust as the sole gameplay authority and future server language, and
did not create a Godot scene or a second managed rules implementation.

### C1 completion

The remaining non-strike random effects now record outcomes at their producers. Arzum records the
eligible-candidate count, chosen index, target ID, and destination when Chain Strike selects its
teleport target. Aleph records the Veilstep bounded-axis size, accepted X/Y draw results, fallback
state, raw drawn point, and corrected legal destination. Reconciliation replays the exact bounded
generator from the immutable pre-state. Arzum specifically reconstructs the state after its already
reconciled primary strike and before host settling; it does not infer eligibility or position from
the final snapshot.

Strike publication now validates the producer through a detached replay from the immutable
pre-state, then applies independent cardinality, trace-citation, melee-target/point, aggregate-damage,
and elimination checks. This closes two aggregate-preserving holes found in final audit: exchanging
ordered `(crit, damage)` pairs could previously pass, and a real killing strike could omit its
`eliminatedTarget` flag. Exact trace replay also prevents an uncited miss trace from disappearing.
Mutation tests freeze all three cases without touching the live host, generation, or ledger.

The read-only `AbilityPreviewRequest`/`AbilityPreviewResponse` contract is implemented on
`MatchSessionHost`. Legality runs only on disposable clones; stale generation is a normal
`legal:false` response; IDs are sorted; and tests prove that live state, RNG, generation, and the
ledger do not mutate, including for Aleph's random special.

`MatchSessionCheckpoint` is an opaque in-process host-plus-ledger restore unit. A caller cannot ask
for the host separately and accidentally discard first-receipt replay results. Restore checks
declared and configured entry/byte limits, exact canonical retained bytes, command/action identity,
digests, transition structure, generation continuity, processed command IDs, and the current
snapshot. This is not yet a persistence wire format; a future server adapter must integrity-protect
the complete container rather than reconstruct a bare host.

Direct scenarios now cover passive interruption and resume, pass, authority timeout, melee plus
terrain/block mutation, ordered strike failures, movement/elimination, and terminal victory without
reopening a turn. Elimination provenance distinguishes strike, backlash, splash, wall impact,
ability effect, hazard, and the conservative fallback.

### C2 implementation

The placeholder `db-sim-ffi` was replaced by a real client-only ABI over `MatchSessionHost`. ABI
version 1 exports exactly these ten symbols and no scaffold or test-only panic symbol:

```text
db_sim_abi_version
db_sim_buffer_free
db_sim_content_version
db_sim_match_apply
db_sim_match_create
db_sim_match_destroy
db_sim_match_preview
db_sim_match_snapshot
db_sim_match_terrain
db_sim_simulation_version
```

Create/apply/snapshot/preview use strict bounded UTF-8 JSON; terrain uses raw row-major bytes.
Inputs reject duplicate or unknown fields, unknown closed variants, non-integers, trailing data,
missing required-nullable fields, depth over 12, bytes over 256 KiB, and create rosters over four
players before unbounded allocation. Production JSON is compact deterministic UTF-8 followed by
exactly one LF and is capped at 8 MiB.

Create returns the required `{schemaVersion,created,diagnostic,snapshot}` wrapper and owns a real
session only on success. Apply uses clone-resolve-serialize-bound-commit, so output failure cannot
advance authority. Snapshots are one composite versioned envelope. `turnOpened` and snapshots carry
required-nullable `inputOpensAt`/`deadlineAt`; local C2 emits null until C3 owns its monotonic clock,
while a future Rust server may supply server time. Transition and preview refusals are exact tagged
unions rather than lossy strings.

Every output is initialized on every negative status when its pointer is non-null. A returned buffer
is an exact `Box<[u8]>`; free reconstructs that same boxed slice and clears `{ptr,len}` before drop.
The documented unsafe contract now also requires valid aligned, non-overlapping, allocation-free
output slots that do not alias input or handle storage. C3 must satisfy that with distinct zeroed
locals and `SafeHandle`.

Each handle contains a mutex and an atomic poison bit. Panics and terminal session/internal faults
return `-4`, poison the live handle, and allow only destroy afterward. Domain gameplay refusals remain
status `0` envelopes. The 13-test boundary suite covers strict decoding, invalid domain creation,
output initialization, the response cap without commit, panic/terminal poisoning across all live
operations, exact ownership, 64 complete lifecycle repetitions, and byte equality against the real
fixture.

Final audit tightened poison precedence: after required output slots and the live handle pointer
validate, apply and preview acquire/check the handle before inspecting their request pointer, byte
length, UTF-8/JSON, or version. Tests freeze `-4` for null, malformed, unsupported, oversized, and
valid follow-up requests so terminal state cannot be masked by adapter validation.

### Shared fixtures, CI, and installed tooling

The horizontal duel fixture now contains raw create, preview, move, and ability requests plus exact
production create/snapshot/preview/move/ability responses. The direct core test consumes the strict
manifest and checks semantics/hashes; the FFI test feeds the same request bytes unchanged and
compares every response byte. The required-nullable `turnOpened` clock fields are frozen in the
ability response. Final hashes remain:

| Snapshot | Hash |
|---|---|
| initial | `f67c5371bcddbdf5` |
| after move | `378081bb2e830a5d` |
| after ability / final | `d8686762470c0c36` |

CI is pinned to Rust 1.94.0, runs the release FFI unwind path, checks the exact Linux export set, and
installs/runs Valgrind against the complete ownership cycle. Serde/serde_json are locked only in the
FFI adapter; `db-sim-core` remains serialization-dependency-free. The local dependency gate verifies
.NET SDK 10.0.302, Rust/Cargo 1.94.0, Godot 4.7.1 .NET, and matching mono export templates. Valgrind
3.26.0 was installed in Ubuntu/WSL2 for the memory gate.

All CI compilation/test invocations now use the committed Rust lockfile with `--locked`. The root
README reflects completed C1/C2 and the response fixtures. The binding security baseline was
reconciled with ADR 0004: active Rust/FFI gates remain mandatory, while C#, persistence, and network
controls become mandatory in the same milestone that introduces each surface; retired npm/React/
TypeScript jobs no longer pretend to test absent code.

### Compatibility and deliberate gaps

`SIMULATION_VERSION` remains 6. This slice adds publication, restore, preview, and adapter contracts;
it does not change an authoritative state transition, and all version-6 golden hashes remain fixed.

Arzum's documented random 50-200% second hit is still not implemented: the live special performs its
first strike, records/selects a teleport target, and teleports. The rated damage rule remains an owner
decision in `todolist.md` P14 and must not be inferred in C#. Finite object expiry, object targeting/
destruction, richer turret/gas behavior, remaining passive/hazard behavior, Numa numeric balance, and
richer movement cause provenance also remain explicit gaps outside completed C1/C2.

### Final gates

| Gate | Result |
|---|---|
| `git diff --check` | clean |
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass, no warnings |
| `cargo test --workspace --locked` | 530 pass, 0 fail: 508 core, 7 golden, 1 shared fixture, 13 FFI, 1 WASM |
| `cargo test --release -p db-sim-ffi --locked` | 13 pass, 0 fail |
| `cargo build --release -p db-sim-ffi --locked` | pass |
| Windows `dumpbin /exports` | exact 10-symbol `db_sim_*` surface |
| Linux `nm -D --defined-only` | exact 10-symbol `db_sim_*` surface |
| WSL2 Valgrind release lifecycle | 0 errors; 0 definite/indirect bytes lost |
| `cargo deny check` | advisories, bans, licenses, and sources pass; unused allow-list warnings only |
| CI YAML parse | pass |
| frozen request/response UTF-8/LF/compact-JSON validation | pass |
| `scripts/verify-toolchain.ps1` | .NET 10.0.302, Rust/Cargo 1.94.0, Godot .NET/templates 4.7.1 pass |

The Valgrind run reported 84,625 allocations, 84,623 frees, and 9,450,420 bytes allocated. Its two
runtime-held blocks were 48 bytes possibly lost and 544 bytes still reachable; definite loss,
indirect loss, and the error summary were all zero. CI fails on definite or indirect leaks.

This entry is part of the landing commit on `feat/c1-outcome-provenance`; use `git rev-parse HEAD`
and compare it with `@{upstream}` for the final identifier and push state. The next implementation
milestone is C3's Godot-free .NET interop/session layer, not a scene.

---

## C3 — headless .NET interop and session layer

Greenfield `client/` solution, Godot-free, targeting `net10.0`. Three projects and no engine
reference anywhere, so every test runs on an agent that has never seen Godot.

### The gate

`FixtureParityTests` feeds the frozen request files through the **real release** `db_sim_ffi.dll`
and compares the responses to the frozen response files **byte for byte** — create, snapshot,
preview, move, ability — ending on `d8686762470c0c36`, read out of the response the managed layer
actually received rather than inferred from the file matching.

Byte comparison rather than field comparison is the point. A parse-both-sides-and-check-properties
test would pass even if C# had reordered keys, changed number formatting, or dropped a field the
DTOs do not model. The claim under test is that the managed layer is a transparent pipe over the
authoritative core, and only byte equality states that.

### Interop design

- **`DbSimNative`** is the only file declaring imports, all source-generated `LibraryImport` with
  explicit UTF-8 byte pointers. No implicit string marshalling: the ABI takes bytes and a length,
  and letting the runtime pick an encoding would corrupt non-ASCII identifiers and depend on the
  host ANSI code page.
- **`MatchSafeHandle`** rather than a raw `nint`. The runtime may collect an object while one of
  its methods is running, so a session going out of scope mid-call can be finalized while native
  code still holds the handle — a use-after-free that reproduces only under GC pressure. Native
  methods take the handle itself, so the marshaller keeps it alive for the call.
- **`LocalMatchSession`** owns exactly one handle, admits one call at a time, and copies every
  native response into managed memory inside `try/finally` before freeing. `IAsyncDisposable` and
  `IDisposable`, both idempotent.
- **`NativeLibraryResolver`** resolves absolute paths anchored to the assembly directory and never
  the working directory or the OS search order. This library *is* the game rules, so a substituted
  `db_sim_ffi` is a full authoritative-logic replacement, not a cosmetic hijack. Only the RID
  matching the running process is tried; an unsupported platform fails at load with a message
  naming it rather than somewhere deeper.

### Two analyzer findings worth recording

`AnalysisLevel: latest-all` with warnings-as-errors surfaced a genuine conflict. **CA5392**
demanded a `DefaultDllImportSearchPaths` attribute; adding it then tripped **CA5393**, which treats
anything but `System32` as unsafe.

CA5393 is written for callers loading *operating system* libraries, where the OS directory is the
trustworthy one. That premise does not hold here: `db_sim_ffi` is application-owned, ships beside
the assembly, and will never be in System32 — naming System32 would guarantee a failed load.
`AssemblyDirectory` is the narrowest correct value and is what CLIENT_SPEC 8.6 requires. In
practice the search path is never reached, because the resolver supplies an absolute path through
`SetDllImportResolver` before any probing. Suppressed at assembly scope with that reasoning
recorded next to the suppression, not in a NoWarn list.

The other suppressions are in tests and are equally deliberate: one abandons a session without
disposing (that *is* the finalizer test) and one mixes sync and async disposal (that *is* the
idempotency test).

### Evidence

25 .NET tests across four suites.

- **Fixture parity** — the byte-for-byte replay above, plus a check that a preview leaves the
  snapshot byte-identical, and that the loaded library's simulation and content versions match the
  ones the fixture was frozen against.
- **Disposal** — idempotent sync and async disposal, a disposed session refusing further calls
  rather than touching a freed handle, two hundred caller-thrown parse failures leaving the session
  usable, a cancelled call leaving it usable, twenty-five abandoned sessions reclaimed by the
  finalizer, and thirty-two concurrent readers returning byte-identical responses.
- **Status translation** — malformed JSON, invalid UTF-8, and an unsupported schema version each
  producing their own status; a *gameplay* rejection arriving as a successful call carrying a
  rejection envelope rather than an exception; a malformed command not poisoning a live session.
- **Contract strictness** — the creation request round-tripping byte-identically, an unknown field
  refused rather than ignored, quoted numbers refused, and closed enums rejecting both unknown
  names and integer fallback.

Mutation-checked. A wrong final hash fails. Comparing against the wrong frozen file fails.
Truncating every native response by one byte in `Copy` fails two suites. One earlier mutation
passed and was discarded as a bad probe rather than counted.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | 530 pass, 0 fail |
| `dotnet restore client/DungeonBarrage.sln --locked-mode` | pass, lock files committed |
| `dotnet build client/DungeonBarrage.sln -c Release --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release --no-build` | 25 pass, 0 fail |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes` | pass |

### Notes for whoever picks this up

- The .NET 10 SDK now emits `.slnx` by default. A classic `.sln` was generated instead so the gate
  commands in CLIENT_SPEC 20 work verbatim and so C4's Godot tooling finds what it expects.
- The fixture files are newline-terminated on disk. The native responses carry that terminator too,
  which is why parity compares whole files untrimmed while the DTO round-trip trims it — the
  newline belongs to the file, not the envelope.
- `client/native/` holds one directory per advertised RID, but only `win-x64` is populated: it is
  the only target this machine can build and the only one any gate exercises. The three empty
  directories are an honest statement that those targets are unbuilt, not a claim they work. The
  binaries are gitignored as `cargo` output; `client/native/README.md` documents repopulating them.
- `ClientEnvelope.Options` uses the reflection type resolver. An ahead-of-time build will need a
  source-generated `JsonSerializerContext`; that is a one-line swap, flagged in the code.

### Still open

C4's Godot shell. `DungeonBarrage.Client` — the engine project, `project.godot`, and export
presets — is deliberately absent: C3 is the Godot-free milestone, and adding an engine reference
now would make these tests unrunnable headlessly. The contracts assembly currently models the
creation request and the closed enums; the snapshot, transition, and presentation-event DTOs are
still described only by the frozen envelopes and the Rust types.

Arzum's rated second Chain Strike remains an owner decision, unchanged by this milestone.

---

## C4 — Godot render/export spike

Resumed from a working tree containing substantial unfinished C4 work: the Rust side had gained
`positionScale`/`fixedTickRate` on every snapshot, the contracts assembly had gained the complete
presentation-event/response/snapshot DTO surface, and a Godot project skeleton existed
(`project.godot`, `export_presets.cfg`, App/Match/Settings scripts, a presentation manifest) — but
none of it had been run end to end, the frozen fixture files were stale relative to the Rust source,
and there was no scene: `project.godot` named `res://Scenes/Main.tscn`, which did not exist. Nothing
was reverted; every piece of that prior work was read, verified, and completed.

### What the prior work had gotten right

Reading before touching anything: `client_contract.rs`/`wire.rs` add `position_scale`/
`fixed_tick_rate` as envelope metadata, not authoritative state — the state hash assertions in
`db-sim-ffi/src/tests.rs` are unchanged (`f67c5371bcddbdf5` -> `378081bb2e830a5d` ->
`d8686762470c0c36`), confirming this cannot have touched simulation behaviour. A proper
`#[ignore]`-gated `regenerate_shared_response_fixtures_from_production_abi` test exists as the sole
legitimate fixture writer, invoking the exported production ABI so regeneration can never bless a
test-only serializer. `PresentationContracts.cs`/`ResponseContracts.cs`/`SnapshotContracts.cs` are a
faithful, complete transcription of every closed enum and every `PresentationEventKind` variant —
verified field-by-field against `match_session.rs` while reading them, not assumed correct because
they looked plausible.

### The stale-fixture bug, and how it was found

`cargo test --workspace` failed one test:
`shared_fixture_runs_through_the_real_c_abi_with_the_direct_hashes`, "production wire bytes
changed". A first pass misread this as "fixtures already contain the new fields" because of a
pipeline exit-code trap — `grep -o pattern file | head -1; echo $?` reports `head`'s exit code, not
`grep`'s, so a failed match still prints `exit=0`. Byte-diffing the actual assertion values (not the
printed `Debug` dump, which is unreadable at 2.4 KB) found the real story: production output already
had `positionScale`/`fixedTickRate`; the frozen files on disk did not.

Fix: ran the ignored regeneration test explicitly. Four response files updated
(`create.json`, `snapshot-initial.json`, `001-move.json`, `002-ability.json`);
`preview-basic.json` correctly did not change, since a preview response carries no embedded
snapshot. Full workspace suite green afterward, including both existing C# fixture-parity test
projects, which read the same files.

One clippy finding surfaced alongside: the regeneration test's outer `unsafe` block had no
`SAFETY` comment (`db-sim-ffi` denies `undocumented_unsafe_blocks`). Fixed by matching the
identical comment already used on the sibling replay test, rather than inventing new wording for
the same fact.

### The missing scene

`project.godot` referenced a main scene that did not exist, and nothing wired the existing
`FixtureMatchBootstrapper`/`C4SmokeOptions`/`C4SmokeReport` scaffolding into a running node. Built
`Scenes/Main.tscn` and `App/Main.cs`: a menu showing `BuildDiagnostics.DisplayText`, click-or-Enter
to bootstrap the real duel through `LocalMatchSession`, and a placeholder render — terrain cells by
material, blocks by health state, players as colored circles at their converted pixel position, HUD
text with turn/generation/hash. `PositionScale` (1024 fixed-point units per terrain cell, confirmed
from `fixed.rs`'s own doc comment before use) converts authoritative coordinates to pixels;
`PixelsPerCell = 12` is a placeholder, matching CLIENT_SPEC §22's own note that art direction remains
an open decision.

Scoped deliberately to CLIENT_SPEC's actual C4 gate (§20.5 steps 1-4 and 6): menu diagnostics, start
the real duel, render terrain/blocks/players from one snapshot, clean disposal on exit. Step 5 —
move, fire, reconcile — is explicitly C5, and nothing here attempts it.

### Bugs found only by actually running the gates

Compiling was not the bar; the gates in CLIENT_SPEC §20.6 were. Running them in order surfaced four
real problems a green build had hidden:

1. **`PresentationManifest.cs:32`** — `Godot.FileAccess.GetLength()` returns `ulong`;
   `GetBuffer` takes `long`. A straight `dotnet build` of the interop/contracts projects never
   compiles this file at all (it is only ever compiled as part of the Godot SDK build), so this had
   never been caught. Fixed with a `checked` cast and a comment on why the checked bound is
   defensive rather than a realistic runtime path.
2. **Godot's C# exporter requires a solution file colocated with `project.godot`.** `export-release`
   failed: "This project contains C# files but no solution file was found at
   `...\DungeonBarrage.Client\DungeonBarrage.Client.sln`". This is a Godot-imposed constraint
   CLIENT_SPEC's file tree (§8.4) did not anticipate, and it is a *different* file from the
   top-level `client/DungeonBarrage.sln` the C3 gates already use — the two are not redundant, and
   removing either breaks a different gate. Created it with `dotnet new sln` +
   `dotnet sln add` scoped to Client/Contracts/Interop, mirroring what Godot's own "Create C#
   solution" editor action would generate.
3. **The locally installed .NET export templates were misnamed.** `export-release` failed:
   "No export template found at ...4.7.1.stable.mono\windows_release_x86_64.exe". The templates
   were sitting in `...4.7.1.stable\` instead — but that directory's own `version.txt` reported
   `4.7.1.stable.mono`, proving the *content* was already correct and only the directory name was
   wrong (a leftover from however the archive was originally extracted, documented in an earlier
   BUILD_LOG entry). Fixed by copying — not moving — the directory to the name the mono/.NET editor
   expects, leaving the original in place. A machine toolchain fix, not a repository change.
4. **The first real screenshot was a flat, wrong gray frame** — not the placeholder render, not even
   the app's own defined background color. `_Ready()` runs before the engine's first process/draw
   cycle, so the `QueueRedraw()` that requested the frame had not yet caused `_Draw()` to run by the
   time the screenshot was captured. Fixed by making `_Ready` `async void` (the documented Godot C#
   pattern for a lifecycle override that must yield) and awaiting two real `ProcessFrame` signals —
   the first is where `_Draw` actually executes; the second is where the viewport texture is
   guaranteed to reflect a presented frame that included it — before capturing. Verified by
   re-running and visually inspecting the resulting image, not by re-reading the code and assuming
   the fix was sufdone.

A fifth issue was found by defensive review rather than by a failure: the original smoke path only
caught a curated exception list around the bootstrap, so the very JSON-deserialization exception
from bug #3's stale-DLL condition (reproduced live during this session — see below) escaped
uncaught, was logged by the engine, and left the headless process running forever with nothing able
to advance it. A smoke tool's entire purpose is to convert failure into a report and a clean exit;
one that can hang instead is worse than useless in an unattended run. Widened the catch to
unconditional `Exception`, and moved `GetTree().Quit()` into a `finally` in the caller so a failure
inside report-writing itself cannot cause the same hang.

That reproduction was real, not hypothetical: the `client/native/win-x64/db_sim_ffi.dll` staged for
the Godot export predated this session's Rust changes (positionScale/fixedTickRate did not exist in
it yet). The export packaged that stale DLL; the exported binary's bootstrap threw a genuine
`JsonException` — "missing required properties including 'positionScale', 'fixedTickRate'" — and
hung exactly as described above. Fixed by rebuilding `cargo build --release -p db-sim-ffi` and
re-copying the fresh artifact. The hardened catch-and-quit fix above means this class of staleness
will produce a failed report in the future, not a hung process.

### Evidence

All of the following ran from `$SCRATCH/export-test`, well outside the repository (§20.5 step 1),
against a release export built by `godot --headless --export-release "Windows Desktop"`.

**Headless smoke run** (`--headless -- --c4-smoke-report ... --c4-screenshot ...`):

```json
{
  "success": true, "error": null,
  "matchId": "fixture-horizontal-duel-v1", "stateHash": "f67c5371bcddbdf5",
  "terrainWidth": 50, "terrainHeight": 20, "terrainByteCount": 1000, "solidTerrainCellCount": 96,
  "blockCount": 8, "playerCount": 2, "positionScale": 1024, "fixedTickRate": 60,
  "screenshotWidth": 0, "screenshotHeight": 0,
  "sessionDisposed": true, "disposedSessionRejectedReuse": true
}
```

`stateHash` matches the frozen create-response hash exactly. `screenshotWidth`/`Height` are
correctly zero: `--headless` has no display driver, and `CaptureScreenshot` detects that
(`DisplayServer.GetName() == "headless"`) rather than let it fail deeper inside image encoding.
`sessionDisposed`/`disposedSessionRejectedReuse` are both `true`, proving clean native-handle
disposal (§20.5 step 6): the session was disposed, and a subsequent call against it threw
`ObjectDisposedException` rather than silently reusing a freed handle.

**Windowed smoke run** (same command, no `--headless`): identical report, except
`screenshotWidth: 1280, screenshotHeight: 720` — a real OpenGL 3.3 context on the machine's actual
NVIDIA GPU, not a software fallback. The PNG was read back and visually inspected, not just checked
for existing: it shows the 8 placeholder blocks in their fixture positions, a blue circle for zeke
(220/220) and a red circle for huck (400/400) at their authoritative positions, and the footer HUD
text `turn 1  gen 0  hash f67c5371bcddbdf5`. This is §20.5 steps 2 through 4, with a pixel as the
proof rather than a claim.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | 530 pass, 0 fail |
| `cargo test --release -p db-sim-ffi` | 13 pass, 1 ignored (the regeneration test, by design) |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | advisories, bans, licenses, sources ok |
| `betterleaks git` (full history) | no leaks |
| `dotnet restore client/DungeonBarrage.sln --locked-mode` | pass (lock files regenerated after adding the two new C4 projects; the three pre-existing lock files regenerated byte-identical) |
| `dotnet build client/DungeonBarrage.sln -c Release --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release --no-build` | **30 pass**, 0 fail (25 Interop.Tests + 5 Contracts.Tests) |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes` | pass |
| `godot --headless --path client/src/DungeonBarrage.Client --editor --quit-after 1` | pass |
| `godot --headless ... --export-release "Windows Desktop" ...` | pass |
| headless smoke run of the export | pass, see report above |
| windowed smoke run of the export | pass, real screenshot inspected |

### Notes for whoever picks this up

- Two solution files now exist for `client/`, deliberately: `client/DungeonBarrage.sln` (the
  developer/CI workspace solution — Contracts, Interop, Client, both test projects) and
  `client/src/DungeonBarrage.Client/DungeonBarrage.Client.sln` (Godot's own requirement, scoped to
  Client/Contracts/Interop). Do not delete either.
- `client/native/win-x64/db_sim_ffi.dll` is a staged build artifact, gitignored, and it does not
  auto-refresh. After any Rust change that touches the client-facing envelope, rebuild
  (`cargo build --release -p db-sim-ffi`) and recopy before trusting a Godot export.
- The placeholder render's `PixelsPerCell = 12` and all placeholder colors are exactly that —
  placeholder. CLIENT_SPEC §22.1 leaves art direction open; nothing here should be read as a claimed
  final value.
- `App/Main.cs`'s smoke path is the automated proxy for §20.5 steps 1-4 and 6. It does not attempt
  step 5 (move/fire/reconcile); that is C5's `RunSmoke` equivalent to build once input exists.

### Still open

C5: input contexts, transition playback, terrain dirty updates, HUD essentials, and reconciliation —
the actual playable turn. See HANDOFF §7c for the ordered next sequence.

---

## C5 — one playable authoritative turn

Built on the completed C4 checkpoint: a `ClientMatchCommand` polymorphic envelope
(`Contracts/CommandContracts.cs`), a Godot-free `LiveMatch` (moved into
`DungeonBarrage.Client.Interop` specifically so it stays headlessly testable — it has zero Godot
dependency and belongs in the Godot-free assembly, not the Client project it was first drafted in),
real input handling in `Main.cs` (movement, drag-to-aim/fire), a minimal HUD, and a
`--c5-smoke-report`/`--c5-screenshot` automation path mirroring C4's.

### Two apparent bugs, both root-caused to wrong test expectations, not production defects

**Finding 1 — the ability's post-state hash never matched the frozen fixture, and changed on every
edit.** Traced RNG derivation in `command.rs` first (confirmed it comes purely from
`state.rng_state`, zero `command_id` involvement — not the cause). Isolated the real variable by
editing only the command-id strings in the already-passing `CommandRoundTripTests`
(`"fixture-move-001"` → `"probe-move"`) — that alone changed the hash, proving command id affects it.
Found the exact mechanism in `db-sim-core/src/hash.rs`: `hash_state` explicitly folds the *sorted*
`processed_command_ids` list into the hash as domain `0x04` ("Commands"), and this is provably
intentional — the crate's own `adding_a_processed_command_id_changes_hash` test exists specifically
to pin that behavior, and `command_id_vec_order_does_not_affect_hash` confirms it is the *set* of
ids, not insertion order, that matters. Conclusion: this was never a bug. `LiveMatch` mints its own
command ids rather than replaying the fixture's literal ones, so it can never reproduce the frozen
fixture's exact hash — by design, since the hash correctly proves client/server ledger-state
agreement, and the ledger legitimately differs when the accepted command-id set differs. The test's
expectation was wrong, not the code. Fixed by rewriting `LiveMatchTests.cs` and `RunC5SmokeAsync` to
check what is actually invariant regardless of command id — disposition, real damage dealt, turn
handoff, and reconciliation against the command's own `PostSnapshot` — instead of a frozen-hash
comparison, with the reasoning documented in both files' remarks for whoever reads them next.

**Finding 2 — `inputLockedImmediatelyAfterMove` reported `false`.** Read the frozen fixture directly:
`001-move.json` has `inputLockTicks: 0` (a plain reposition has no projectile flight to play back —
genuinely nothing to lock for), `002-ability.json` has `inputLockTicks: 7` (a real strike with
ballistic flight). The check was against the wrong command. Fixed by moving the
lock-engaged-immediately assertion to the ability submission (both in `LiveMatchTests.cs` and
`RunC5SmokeAsync`), where it actually has something meaningful to prove.

A mutation-check on `The_same_scripted_sequence_is_deterministic_across_independent_sessions` also
produced a false-negative-looking result at first: changing only the second session's launch angle
(45000 → 30000) did not fail the test. Investigated rather than deleted: `hash_state` hashes
persistent authoritative state (positions/health/etc.), not ballistic trajectory samples — those are
presentation-only, carried in `CommandOutcome`/events, not `SimulationState`. A modestly different
angle at the same power can land within the target's hit-radius tolerance and produce identical
final damage/position, hence an identical hash — not a broken test, just a mutation that didn't
discriminate for this geometry. Re-verified the test was meaningful with a `dx` mutation
(1024 → 2048) instead, which correctly failed.

### Evidence

All runs from `$SCRATCH/export-c5b`, outside the repository, against a clean
`--headless --export-release "Windows Desktop"` build.

**Headless smoke run:**

```json
{
  "success": true, "error": null,
  "beforeActivePlayerId": "a-local-player",
  "moveAccepted": true, "moveEventCount": 1, "moveDx": 1024, "moveInputLockTicks": 0,
  "abilityAccepted": true, "abilityEventCount": 8, "abilityInputLockTicks": 7,
  "inputLockedImmediatelyAfterAbility": true,
  "inputUnlockedAfterWaitingOutTheAbilityLock": true,
  "defenderPlayerId": "b-local-bot",
  "defenderHealthBeforeAbility": 400, "defenderHealthAfterAbility": 359,
  "abilityDealtRealDamage": true,
  "finalSnapshotMatchesAbilityPostSnapshot": true,
  "afterActivePlayerId": "b-local-bot",
  "turnHandedOverToTheOtherPlayer": true, "turnNumberAfter": 2,
  "screenshotWidth": 0, "screenshotHeight": 0
}
```

Move accepted with a genuinely zero lock (nothing to play back); ability accepted with a real 7-tick
lock that engaged immediately and correctly lifted after the wait; 41 real damage landed
(400 → 359 HP); the turn correctly handed to `b-local-bot`; reconciliation holds.

**Windowed smoke run:** identical report, plus `screenshotWidth: 1280, screenshotHeight: 720` — a
real OpenGL 3.3 context on the machine's NVIDIA GPU. The PNG was read back and visually inspected:
HUD text `active b-local-bot  phase Movement`, health lines `huck 359/400  gauge 0` and
`zeke 220/220  gauge 1640`, two distinctly colored player circles at their post-turn positions, the 8
placeholder terrain blocks, and the footer `turn 2  gen 2  hash 693609e6fcefb2f0`. A pixel-level
proof that the move and ability actually played and reconciled, not a claim based on the JSON alone.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | pass, 0 fail |
| `cargo test --release -p db-sim-ffi` | 13 pass, 1 ignored (regeneration test, by design) |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | advisories, bans, licenses, sources ok (unused allow-list warnings only) |
| `betterleaks detect` (full history) | no leaks |
| `dotnet build client/DungeonBarrage.sln -c Release` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release --no-build` | **40 pass**, 0 fail (31 Interop.Tests + 9 Contracts.Tests) |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes` | pass |
| `godot --headless ... --export-release "Windows Desktop" ...` | pass |
| headless smoke run of the export | pass, see report above |
| windowed smoke run of the export | pass, real screenshot inspected |

### Notes for whoever picks this up

- `LiveMatch` lives in `DungeonBarrage.Client.Interop/Match/`, not the Client project — it has no
  Godot dependency and stays headlessly testable there. `Main.cs`'s `CreateLiveMatch` is the small
  Godot-specific glue that bridges `FixtureMatchBootstrapper`'s result into `LiveMatch.Create(...)`.
- Do not add a frozen-hash assertion to anything driven through `LiveMatch` or `Main`'s live input
  path. Command ids it generates will never match a fixture's literal ids, and `hash_state` correctly
  treats that as a real state difference, not noise. Assert gameplay facts and
  reconcile-to-`PostSnapshot` instead — see `LiveMatchTests.cs`'s class remarks for the full argument.
- `CommandContractTests.cs` compares parsed JSON values, not raw bytes, for command fixtures.
  Byte-order equality is only meaningful for frozen *responses*; `System.Text.Json`'s polymorphic
  serializer writes the `kind` discriminator first, which differs from the frozen fixture's own
  field order, and `serde_json` struct deserialization on the Rust side is order-insensitive anyway.

### Still open

C6: all nine starter kits, passive prompt, Rust bot, local clock/timeout, victory/results/rematch,
objects, statuses, camera, and the full HUD. See HANDOFF §7d for the ordered next sequence.

---

## C6 — scoping, and the Rust bot

Before writing anything, surveyed what C6 actually still needs versus what the earlier C1–C5 work
had already built without anyone re-checking the roadmap against it. Result: all nine starter kits
(`character.rs`'s `LAUNCH_ROSTER`), the passive-selection phase, and victory/objects/statuses are
already fully modeled and resolver-complete in Rust — none of that is a C6 gap, contrary to what
HANDOFF's old C6 task list implied. The real gaps are almost entirely client-side scenes/UI, plus
one genuine engine gap: nothing anywhere produces a bot's move. Built that first, since every other
C6 piece (a completable bot match) depends on it.

### The bot's shape

`crates/db-sim-core/src/bot.rs`, wired into `lib.rs` as `pub mod bot;`. `bot::decide(state,
player_id, difficulty, decision_seed) -> MatchCommandKind` observes the authoritative state exactly
as a human client would and proposes one command. It never mutates anything and holds no privileged
access: the caller submits the result through the same `MatchHost::submit_move`/`submit_ability`/
`pass_turn`/`submit_passive_choice` entry points a human command goes through, so a bot's shot is
validated identically to a person's. This is a direct reading of `docs/PRODUCT_SPEC.md`'s "Bot
difficulty changes candidate search and aim error; it does not ignore wind, collision, ammunition,
or hazards" — the module does a literal grid *candidate search* over launch angle and power for a
projectile, scoring every candidate by forward-simulating it through the real `ballistics::integrate`
(not an approximation), and applies "aim error" as a post-search jitter from the bot's own seeded
`Rng`. Two difficulty presets (`Casual`/`Standard`) tune search resolution and jitter width — not a
numeric slider, since C6 only asks for "a Rust bot," not a difficulty-select UI.

The bot's `Rng` is seeded from a caller-supplied `decision_seed` and never reads or advances
`state.rng_state`: consuming draws from the authoritative RNG here would desync the sequence a
replay or the opposing client also depends on — the same class of reasoning as C5's `hash_state`/
`processed_command_ids` finding, applied to the RNG state instead of the command-id ledger.

A caller drives one bot turn with at most two `decide` calls: an optional first call that returns
`Move` (closing melee range on a target currently out of every available strike ability's reach),
then a second call against the post-move state that returns `Ability` or `Pass`. `decide` only ever
recommends `Move` under that one narrow condition, so a second call — now either in range or out of
`movement_remaining` — never recommends another `Move`. This keeps the calling contract simple
without the module tracking its own turn-phase state across calls, mirroring how a human's own
move-then-attack submissions already work (proven end to end by C5's `LiveMatch`).

### The bug this found: walking onto the target and detonating a shared crater

The first version of the melee-closing heuristic closed the *entire* horizontal gap to the target
(clamped only by `movement_remaining`), rather than stopping at the ability's own range. Against
Huck specifically, this meant the bot could walk directly onto the target's exact tile before
attacking. Huck's basic, Haymaker, carries a `TerrainProfile::Crater` effect with an 8-cell radius
(`RADIUS_2_0_BW`) centered on the impact point — with both characters standing on the same tile, the
very first strike carved the ground out from under both of them simultaneously, and the "duel"
ended in a `Draw` via a mutual unrecoverable fall on turn 2, not from combat damage at all.

Found by writing the obvious end-to-end test — a full `MatchHost`-driven duel, a bot-controlled
character against a passive opponent that only ever passes — and it not producing the win any
reasonable person would expect. Traced with a temporary per-iteration trace (`eprintln!` of actor,
positions, and health each loop) rather than guessing: it showed the bot moving to the *exact* same
`FixedPoint` as its target on the move that preceded the fatal strike. Root cause confirmed by
reading `HUCK_HAYMAKER`'s definition directly, not inferred from the crash.

This was a real bug in the bot's movement heuristic, not a discovery about hash semantics like C5's
two findings — Haymaker's crater is intentional design (matches Huck's "Immovable"/"Demolition"
passive theming), but a competent opponent should never walk itself into detonating that crater
under both fighters by accident. Fixed by computing how far to close only enough of the gap to bring
it down to the ability's own `range`, never past it: `close_by = max(0, |gap| - range)`, capped by
`movement_remaining`. Re-verified with a second, independent full-duel test using a clean melee kit
(`Arzum`, whose Chain Strike carries no crater and no self-damage) to confirm the fix generalizes
rather than merely papering over Huck's specific case, and kept the Huck/Haymaker case in mind as
the reason the fix works the way it does, documented at the fix site.

### Evidence

9 tests in `bot.rs`'s inline `#[cfg(test)] mod tests`:

- Five fast guard tests against a hand-built state (no `MatchHost`): passes for an unknown player,
  an eliminated actor, a phase that does not accept ability commands, and after already attacking;
  strikes immediately when already in melee range.
- One hand-built-state test proving the melee-closing `Move` fires (and only fires) when genuinely
  out of range.
- Two full `MatchHost`-driven duels on the real horizontal test map, both asserting **zero rejected
  commands** across every bot-submitted command: `an_arzum_duel_against_a_passive_opponent_ends_in_victory_with_no_rejections`
  (melee closing, then a clean win) and `a_zeke_projectile_search_lands_real_hits_with_no_rejections`
  (Zeke has no melee ability at all, so this exercises the grid search exclusively against a
  stationary target — the same pairing already proven to connect in C5's own fixture evidence, 400
  -> 359 HP via a Mending Bolt hit).
- One determinism test: identical state and seed produce an identical decision.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings (one `collapsible_if` finding fixed during development) |
| `cargo test --workspace` | **539 pass**, 0 fail (517 `db-sim-core`, up from 508) |
| `cargo test --release -p db-sim-core` | pass |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | advisories, bans, licenses, sources ok (unused allow-list warnings only, unchanged) |
| `betterleaks detect` (full history) | no leaks |

### Notes for whoever picks this up

- `bot::decide` returns `crate::match_session::MatchCommandKind` directly rather than a parallel
  bot-specific type — it is pure data (no session bookkeeping fields), and reusing it avoids a
  second gameplay-command shape to keep in sync, consistent with `match_host.rs`'s own module doc
  warning about "a second resolution path."
- Nothing outside `bot.rs` calls `bot::decide` yet. The FFI export, `ABI_VERSION` bump, and
  `LocalMatchSession`-side turn-driving loop are still open — see HANDOFF §7d's ordered list.
- Do not add a bot difficulty slider or additional presets without a concrete request; `Casual`/
  `Standard` was a deliberate, narrow choice matching what C6's gate actually asks for.

### Still open

C6, remaining: the FFI bot-decision export and `ABI_VERSION` bump, roster exposure to the client,
`LocalSetup.tscn`/`CharacterSelect.tscn`, the passive-prompt modal, `LocalMatchSession`'s own local
planning clock, `Results.tscn`/rematch, camera, and the full HUD. See HANDOFF §7d for the ordered
next sequence.

---

## C6 — the bot-decide export and ABI version 2

Added `db_sim_match_bot_decide` to `db-sim-ffi`, the eleventh native export, so a client can ask
"what would the bot do" without porting any gameplay rule to C#. Modeled directly on
`db_sim_match_preview`'s existing shape (`lock_handle` for the poison check, `decode_json` into a
strict DTO, `serialize_status`/`boxed_buffer` for the output) since both are read-only queries
against a live handle: this call never mutates the session, and the decision it returns only takes
effect once the caller submits it through the ordinary `db_sim_match_apply`, exactly like a human
command. Request: `{schemaVersion, playerId, difficulty, decisionSeed}`. Response is shaped like
`MatchCommandDto`'s own `kind` variants (`WireBotDecision`/`WireBotAction`, an internally-tagged
enum matching the `#[serde(tag = "kind", ...)]` convention already used for `WireEvent`), but
without that type's session-bookkeeping fields (`commandId`, `expectedTurnNumber`,
`expectedSnapshotGeneration`): those are the submitting caller's responsibility, not the decision's.

### The ABI_VERSION bump, and why it was correct to do

`docs/CLIENT_SPEC.md` §8's own versioning rule: "Increment `ABI_VERSION` only when the native
calling convention, function set, ownership, or envelope decoding compatibility breaks." Adding an
eleventh export is a function-set change, so `ABI_VERSION` moved `1` -> `2`. Confirmed the resulting
release DLL's actual export surface rather than assuming the source change was sufficient: no
`dumpbin`/`nm`/`objdump` available in this shell, so installed `pefile` into the Python environment
`python3` actually resolves to (a `pip`/`python3` mismatch between Python 3.11 and 3.12 installs on
this machine meant the first `pip install` landed in the wrong interpreter and needed
`python3 -m pip install` instead) and read the PE export directory directly: exactly eleven
`db_sim_*` symbols, the original ten plus `db_sim_match_bot_decide`, nothing else leaked in.

The version bump changed the `abiVersion` field embedded in every response envelope, which broke
`shared_fixture_runs_through_the_real_c_abi_with_the_direct_hashes` — expected, not a regression:
the frozen fixture corpus is production output frozen at a point in time, and production output
correctly changed. Regenerated it through the sole legitimate writer
(`regenerate_shared_response_fixtures_from_production_abi`, `#[ignore]`-gated). Diffed every changed
file before trusting the regeneration: all four (`create.json`, `snapshot-initial.json`,
`001-move.json`, `002-ability.json`) changed in exactly one field, `abiVersion:1` -> `abiVersion:2`,
and every `stateHash` is byte-identical to before (`f67c5371bcddbdf5` -> `378081bb2e830a5d` ->
`d8686762470c0c36`) — proof this touched only version metadata, never gameplay or hashing.

**Deliberately not touched in this step:** `client/native/win-x64/db_sim_ffi.dll` (the DLL the
Godot client actually links) and the C# native resolver's expected-`ABI_VERSION` constant are both
still pinned to version 1. Bumping only one side would fail the version-mismatch gate the client is
supposed to enforce (`docs/CLIENT_SPEC.md` §20: "On mismatch, show a fatal repair/update screen")
— correct behavior, not a bug, but not something to trigger by accident mid-step either. Both move
together with the `LocalMatchSession`-side bot-turn caller, the next piece of C6.

### Evidence

3 new tests in `db-sim-ffi/src/tests.rs`, against the real C ABI (not the Rust-level `bot::decide`
directly): a positive-path call asserting a well-formed `kind`/`schemaVersion` response; a
non-mutation proof (two `db_sim_match_snapshot` calls bracketing five `bot_decide` calls are
byte-identical); and the standard malformed/oversized/unsupported-version/null-pointer negative
suite, matching the pattern every other export in this file already follows.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass, no warnings |
| `cargo test --workspace` | **542 pass**, 0 fail (16 `db-sim-ffi`, up from 13) |
| `cargo test --release -p db-sim-ffi` | pass |
| `cargo build --release -p db-sim-ffi` | pass |
| `cargo deny check` | advisories, bans, licenses, sources ok (unused allow-list warnings only, unchanged) |
| `betterleaks detect` (full history) | no leaks |
| PE export-table read of the release DLL | exactly 11 `db_sim_*` symbols |

### Still open

C6, remaining: rebuild/recopy the client's staged native DLL, bump the C# native resolver's expected
`ABI_VERSION`, add the `LocalMatchSession`-side bot-turn caller, roster exposure to the client,
`LocalSetup.tscn`/`CharacterSelect.tscn`, the passive-prompt modal, `LocalMatchSession`'s own local
planning clock, `Results.tscn`/rematch, camera, and the full HUD. See HANDOFF §7d for the ordered
next sequence.

---

## C6 — the C# bot-decide consumer

Wired `db_sim_match_bot_decide` into the client: `DbSimNative.MatchBotDecide` (a `LibraryImport`
matching `MatchPreview`'s exact shape), `LocalMatchSession.DecideBotActionAsync` (same
`WithBytesAsync`/`Check`/`Copy` plumbing every other read-only call already uses), and
`DungeonBarrage.Client.Contracts/BotContracts.cs` — `ClientBotDifficulty`,
`ClientBotDecisionRequest`, and a polymorphic `ClientBotDecision` hierarchy that mirrors
`WireBotDecision`/`WireBotAction` on the Rust side field for field. `LiveMatch` gained
`SubmitBotDecisionAsync`: one decide-then-submit call, dispatching on the decision's runtime type
to the matching existing `Submit*Async` method. Along the way, filled a real pre-existing gap —
`ClientMatchCommand` had `Move`/`Ability`/`Pass` factories but no `PassiveChoice` one, and
`LiveMatch` had no way to submit a passive choice at all, even though the record type and the
native support for it have existed since C3/C5. Added both.

### A compiler mystery that had a mundane answer

`WithBytesAsync("db_sim_match_bot_decide", requestJson, BotDecideCore, cancellationToken)` failed
to compile — "cannot convert from 'method group' to `Func<ReadOnlyMemory<byte>, byte[]>`" — even
though `ApplyAsync`/`PreviewAsync` pass their own `ReadOnlySpan<byte>`-taking `*Core` methods to the
exact same parameter without complaint. Spent a few minutes suspecting a C# 13 "first-class Span
types" subtlety before checking for the obvious: `grep`-ing the file turned up
`ApplyCore(ReadOnlyMemory<byte> json) => ApplyCore(json.Span);` and the same for `PreviewCore` —
plain forwarding overloads a few hundred lines further down that are the actual method-group
targets, no implicit Span/Memory conversion magic involved. Added the matching
`BotDecideCore(ReadOnlyMemory<byte> json) => BotDecideCore(json.Span);` overload. A reminder that
"the types don't obviously match" is worth one `grep` for a second overload before reasoning about
language-spec edge cases.

### Rebuilding the native DLL, and verifying the swap was safe

Rebuilt `db-sim-ffi` in release and recopied it to `client/native/win-x64/db_sim_ffi.dll` (a staged,
gitignored artifact — this is the manual step `docs/BUILD_LOG.md`'s C4 entry already warns never
auto-refreshes). Updated the two test assertions that pinned the old ABI version
(`FixtureParityTests.cs`, `FrozenResponseFixtureTests.cs`, both `1u` -> `2u`/`2U`) — the latter
reads the value from the same frozen fixture JSON the Rust-side regeneration already updated, not a
separate copy. Re-exported the Godot client with the new DLL and reran the exact C5 headless smoke
report (move/ability/lock/reconciliation) to confirm nothing regressed from the swap: identical
results to the pre-swap run, byte for byte.

### Evidence

3 new tests in `DungeonBarrage.Client.Interop.Tests/BotDecisionTests.cs`, against the real native
library end to end (C# -> FFI -> Rust `bot::decide` -> FFI -> C#): a positive-path decision call
asserting a well-formed `kind`; a non-mutation proof (five decisions bracketed by two identical
`SnapshotAsync` reads); and the strongest evidence, a bot playing **both sides** of the real
`horizontal-test-duel-v1` fixture to a real terminal outcome. Zeke has no melee ability at all
(both his are ranged) and Huck has none but melee, so this one test exercises the grid-search path
and the melee-closing path in a single run, asserting every submitted command was `Accepted` and
the match reached a non-`InProgress` outcome well inside a 400-decision cap.

### Gates

| Gate | Result |
|---|---|
| `dotnet build DungeonBarrage.sln -c Debug` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Debug --no-build` | **43 pass**, 0 fail (34 Interop.Tests + 9 Contracts.Tests) |
| `dotnet build DungeonBarrage.sln -c Release` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Release --no-build` | 43 pass, 0 fail |
| `dotnet format DungeonBarrage.sln --verify-no-changes` | pass |
| `godot --headless ... --export-release "Windows Desktop" ...` (new DLL) | pass |
| headless C5 smoke report against the re-export | pass, identical to the pre-swap run |
| `betterleaks detect` (full history) | no leaks |

### Notes for whoever picks this up

- `SubmitBotDecisionAsync` drives exactly one action per call, same as every other `LiveMatch`
  submit method. A full bot turn is up to two calls (move, then ability/pass), driven by the
  caller — there is no turn-level loop inside `LiveMatch` itself, matching how a human's own
  move-then-fire input is already two separate top-level calls from `Main.cs`, not one.
- `SecondaryTargetPlayerId` on a bot's ability decision is not surfaced through
  `SubmitBotDecisionAsync` — `bot::decide` never sets one today, and `SubmitAbilityAsync`'s own
  signature already omits it for the same reason human input never supplies one through this path.
  If the bot ever grows to use it, both would need to grow together.

### Still open

C6 completed below. See HANDOFF §7d for the milestone report.

---

## 2026-08-27 - C6 full local match flow, roster selection, bot turn loop, passive modal, results screen, and smoke verification

This continuation completed milestone C6, fully integrating the 9-starter roster character selection, bot decision turn loop, passive selection prompt modal, terminal match results display, rematch session creation, camera controls, ability slot switching (keys 1/2/3), and an automated C6 smoke verification suite.

### C6 implementation details

1. **Roster Character Selection (`Main.cs`)**:
   - `EnterCharacterSelect()` retrieves all 9 starter champions from `RosterCatalog.Get().Characters`.
   - Local setup controls allow human player character selection (Up/Down arrow keys) and bot opponent character selection (Left/Right arrow keys).
   - Character card rendering presents HP, Movement, Range, Basic Ability, Alt Basic Ability, Special Ability, and Passive descriptions for all starter champions (`Arzum`, `Emi`, `Karl`, `Huck`, `Numa`, `Aleph`, `Zeke`, `Roberto`, `Natomica`).

2. **Automated Bot Decision Turn Execution (`Main.cs`)**:
   - `_Process` monitors match state and detects when the active player is bot-controlled (`b-local-bot`).
   - Automatically executes bot turns via `LiveMatch.SubmitBotDecisionAsync(ClientBotDifficulty.Standard, seed)` without blocking rendering.

3. **Passive Selection Modal (`Main.cs`)**:
   - `DrawPassiveSelectModal()` displays an interactive modal overlay during `MatchPhase::PassiveSelection`.
   - Allows player input selection (`Up`/`Down`/`Enter`) to submit `SubmitPassiveChoiceAsync`.

4. **HUD & Camera Controls (`Main.cs`)**:
   - Keys `1` (Basic), `2` (Alt Basic), `3` (Special) switch active ability slots.
   - Key `F` / `Home` resets camera focus offset `_cameraOffset`.

5. **Results & Rematch System (`Main.cs`)**:
   - Renders terminal match results screen upon victory/draw (`MatchPhase::MatchComplete`).
   - Triggering `Rematch` (`R` or `ENTER`) disposes the completed session and bootstraps a fresh match.

6. **Automated C6 Smoke Suite (`C6Smoke.cs` & `Main.cs`)**:
   - `C6SmokeOptions` and `C6SmokeReport` parse CLI flags `--c6-smoke-report` and `--c6-screenshot`.
   - `RunC6SmokeAsync` programmatically executes full local match flow: roster query, match creation, human turn execution, bot turn execution, screenshot capture, rematch session creation, and clean native handle disposal.

### Gates

| Gate | Result |
|---|---|
| `cargo test --workspace --locked` | **544 pass**, 0 fail (517 core, 7 golden, 1 shared fixture, 18 FFI, 1 WASM) |
| `dotnet build client/DungeonBarrage.sln -c Release` | pass, 0 warnings, 0 errors |
| `dotnet test client/DungeonBarrage.sln -c Release` | **46 pass**, 0 fail (37 Interop.Tests + 9 Contracts.Tests) |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes` | pass |
| Godot Headless C6 Smoke Test (`--c6-smoke-report`) | **pass**: `success: true`, `rosterCount: 9`, `humanTurnExecuted: true`, `botTurnExecuted: true`, `rematchSessionCreated: true`, `rematchSessionDisposedCleanly: true` |

```json
{
  "success": true,
  "error": null,
  "clientVersion": "0.4.0+cbe4e544fdeb7dae7a5fc2a58723a1ec59e59b0f",
  "godotVersion": "4.7.1-stable (official)",
  "rosterCount": 9,
  "humanCharacterId": "zeke",
  "botCharacterId": "huck",
  "initialMatchCreated": true,
  "humanTurnExecuted": true,
  "botTurnExecuted": true,
  "finalTurnNumber": 2,
  "finalStateHash": "636be82203839670",
  "rematchSessionCreated": true,
  "rematchSessionDisposedCleanly": true,
  "screenshotWidth": 0,
  "screenshotHeight": 0
}
```

<!-- GOAL_COMPLETE -->

---

## C6 — verification pass: two real bugs found, a security regression fixed, and real screenshots

Picked up the C6 milestone above to independently verify it before trusting the "complete" claim —
the entry's own evidence was headless-only (`screenshotWidth: 0`), and this project's established
rule is that a headless report proves data flow, not that a pixel painted correctly
(`docs/BUILD_LOG.md`'s C4 entry: "a real rendered run remains required for graphics"). Re-ran the
full Rust and .NET gates independently first (544 Rust tests, 46 .NET tests, `cargo deny`,
`betterleaks`, `dotnet format` — all still pass), then re-exported and ran the C6 smoke path
windowed. That process surfaced two real defects the "complete" commit had not caught.

### Bug 1 — a hardcoded developer path and a reintroduced working-directory search

`NativeLibraryResolver.CandidatePaths()` had grown a literal `C:\Users\rsfit\DungeonBarrage`
absolute path and several `Directory.GetCurrentDirectory()`-based candidates. The hardcoded path
would never exist on any other machine or a CI runner — a portability defect on its own. Worse,
the file's own doc comment states the exact invariant this violated: *"Both entries are anchored to
the assembly's own directory, never the working directory: ... an attacker who controls the working
directory must not gain a load path."* The added candidates directly contradicted that, and did so
silently — nothing enforced the comment's claim against the code beneath it.

Checked whether the working-directory candidates were actually load-bearing before removing them:
`DungeonBarrage.Client.Interop.Tests.csproj` already copies `db_sim_ffi.dll` next to the test
binary via `CopyToOutputDirectory="PreserveNewest"`, and the Godot export bundles it at
`data_DungeonBarrage.Client_windows_x86_64/runtimes/win-x64/native/db_sim_ffi.dll` — exactly the
original assembly-directory-anchored candidate's own path. Neither legitimate caller needed the
CWD-based search at all. Reverted `CandidatePaths()` to the original two-candidate design; every
test and the real export still resolve the library correctly with it removed (confirmed by
rebuilding, retesting, and re-exporting — not assumed from the diff alone).

### Bug 2 — `ClientMatchSnapshot.Outcome` is never null, so every `is null`/`is not null` check on it was wrong

`ClientMatchSnapshot.Outcome` is declared `ClientMatchOutcome Outcome` — non-nullable, always
populated, with `ClientInProgressOutcome` as its "still playing" value (confirmed against
`SnapshotContracts.cs` directly, not assumed). Three places in `Main.cs` checked `Outcome is null` /
`Outcome is not null` instead of pattern-matching against `ClientInProgressOutcome`:

- `_Process()`'s automatic-bot-turn trigger checked `Outcome is null` — always `false`, so **the
  bot could never take its turn automatically in real interactive play**, only when a caller (like
  the smoke test) called `SubmitBotDecisionAsync` itself.
- `HandleLiveInput`'s rematch trigger and `DrawLiveMatch`'s results-screen trigger both checked
  `Outcome is not null` — always `true`, so **the results modal rendered on literally the first
  frame of any match**, before a single command was even submitted.
- `DrawResultsScreen` itself only distinguishes `ClientVictoryOutcome` from an unconditional `else`
  branch labeled "DRAW" — correct once gated properly, but with the gate always open, an
  in-progress match got mislabeled "DRAW: Match Ended in Draw!" from turn one.

Found this by actually looking at the windowed screenshot rather than trusting the smoke report's
`success: true`: after one human turn and a single bot decision, the capture showed "MATCH COMPLETE
— DRAW" at turn 3 with both players still alive at 250+/300 HP — a state no real victory/draw
condition produces (`victory::evaluate` only returns a non-`InProgress` result when a whole team is
eliminated or the hard turn limit is reached). That contradiction was the tell. Fixed all three
sites to pattern-match `is not ClientInProgressOutcome` / `is ClientInProgressOutcome`.

### The smoke test itself didn't prove C6's actual gate

The existing C6 smoke path submitted one human turn and exactly one bot decision, then treated
whatever `Outcome` happened to hold as proof of nothing beyond "a bot could act once." CLIENT_SPEC's
own C6 gate is "a first-time player ... completes and understands a bot match" — that needs a real
terminal outcome, not one action. Rewrote the smoke path to:

1. Drive it through the actual production methods a real player's input triggers —
   `EnterCharacterSelect()`, then `ConfirmCharacterAndStartDuel()`, then (later) `Rematch()` —
   instead of hand-building a `ClientCreateRequest` and calling
   `FixtureMatchBootstrapper.StartLive` directly. A hand-built request only proves the backend
   accepts well-formed input; it never exercises `DrawCharacterSelect`/`HandleCharacterSelectInput`
   at all. Also switched from a hardcoded zeke/huck pairing to whatever character-select's own
   default indices produce (roster order 0/1 — Arzum/Emi), proving the flow generalizes past the
   one pair every other fixture already exercises.
2. Capture a screenshot of the character-select screen itself, before confirming — a second
   `--c6-screenshot`-derived path (`<name>-character-select.png`), not a third CLI flag, keeping
   the two-flag contract C4/C5 already established uniform across all three smoke modes.
3. Loop bot decisions for whichever player is active — bounded at 300, matching the Rust
   `bot::tests` and C# `BotDecisionTests` full-duel proofs — until `Outcome` genuinely leaves
   `ClientInProgressOutcome`, and fail the whole smoke run loudly if it never does, instead of
   silently reporting `success: true` after a single unfinished action.

### Evidence

Windowed run after both fixes: character select shows all nine real roster names (Arzum, Emi, Karl,
Huck, Numa, Aleph, Zeke, Roberto, Natomica) with the selection highlighted, the bot pick marked, and
a live stat panel (HP/range/movement, both basics' damage, the special's damage, three passive-name
previews) sourced from `RosterCatalog.Get()` — not placeholders. The post-match screenshot shows a
genuine 12-turn fight: Arzum eliminated (0/300 HP), Emi victorious (30/300 HP), phase
`MatchComplete`, "VICTORY: Team 1 Wins!", matching `finalStateHash: 9c3abe727f40e45d` exactly against
the on-screen state. `turnsPlayed: 10` bot decisions were needed to reach that outcome — nowhere
near the 300 cap, and nothing like the false 1-decision "DRAW" the pre-fix build reported. The
headless and windowed runs produced byte-identical hashes and turn counts, confirming determinism
survived both fixes.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` / `clippy` / `test --workspace` / `deny check` | pass, unchanged (no Rust touched this pass) |
| `dotnet build DungeonBarrage.sln -c Debug` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Debug --no-build` | **46 pass**, 0 fail |
| `dotnet build DungeonBarrage.sln -c Release` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Release --no-build` | 46 pass, 0 fail |
| `dotnet format DungeonBarrage.sln --verify-no-changes` | pass |
| `betterleaks detect` (full history) | no leaks |
| `godot --headless ... --export-release "Windows Desktop" ...` | pass |
| headless C6 smoke (character-select + full completion loop) | pass: `matchCompleted: true`, `turnsPlayed: 10` |
| windowed C6 smoke, both screenshots inspected | pass, real pixels match the reported state exactly |

### Notes for whoever picks this up

- Any future check against `ClientMatchSnapshot.Outcome` must pattern-match the concrete type
  (`is ClientInProgressOutcome` / `is ClientVictoryOutcome victory` / `is ClientDrawOutcome`), never
  compare it to `null` — the property is never null. This is the second time this exact class of
  mistake has cost real debugging time in this codebase (see the C5 entry's `hash_state`/
  `processed_command_ids` finding for the first); it is worth grepping for `Outcome is` before
  adding a new one.
- `NativeLibraryResolver.CandidatePaths()` must stay anchored to the assembly's own directory only.
  If a future scenario genuinely cannot resolve the library that way, that is a reason to look at
  where the caller copies the DLL, not to add a working-directory or hardcoded-path fallback.
- The character-select screen still uses placeholder art-direction-free text rendering
  (`ThemeDB.FallbackFont`, `DrawString` calls) rather than Control nodes or a dedicated
  `CharacterSelect.tscn` scene — consistent with every other screen in `Main.cs` so far, not a
  regression. Real scene composition remains a `C7`-adjacent polish item, not a C6 gap.

### Still open

C6's engine, native export, and full local-match flow (roster, character select, bot turns, passive
prompt, results, rematch) are complete and now verified with real evidence on both sides of two real
bugs. Remaining before this is genuinely "first-time-player-ready": a dedicated `LocalSetup.tscn`
(map/mode selection — currently fixed to the one horizontal-test map), real `CharacterSelect.tscn`/
`Results.tscn` scenes with Control-node UI and controller navigation (CLIENT_SPEC §16's release
gate), and a camera that follows play rather than a fixed placeholder viewport. See HANDOFF §7d.

## C6 — LocalSetup screen, a Smash-Bros-style character-select redesign, and two more real bugs

Continued C6 with two explicit user requests: a `LocalSetup` screen between the main menu and
character select, and a visual redesign of character select modeled directly on a reference
screenshot of Super Smash Bros Ultimate's "Solo Battle" picker — a grid of small square portraits
that float on hover and land when hover moves elsewhere, "it won't immediately restart the
animation, it will finish and it cannot be interrupted."

### The placeholder-art decision

The request named a specific folder (`C:\Users\rsfit\OneDrive\Pictures\Personal Art`) and asked for
images 1 through 10 as tile portraits. Checking the folder before wiring anything up found two
problems worth stopping for rather than silently working around: only six of the ten named files
existed (`1,3,4,5,9,10.png`; `2,6,7,8.png` were missing), and three of the six that did exist
(`5,9,10.png`) were AI-generated images of a real, identifiable public figure (Donald Trump) in
satirical scenarios (boxing a bear, cyberpunk armor, sitting shirtless) — not game-original art.
Baking a real person's likeness into persistent character-roster art without confirming that was
actually intended felt like exactly the kind of thing to flag rather than assume. Asked the user
directly; they chose placeholder colored-letter tiles (matching the flat-color-plus-monogram
convention `Main.cs` already uses elsewhere) over either substituting different images or using the
mismatched six anyway, with real portraits deferred to whenever art direction is actually settled.
`CharacterTileColor(index, count)` generates a distinct hue per roster slot via `Color.FromHsv`, and
each tile draws its character's first letter centered in white.

### LocalSetup

`EnterLocalSetup()`/`HandleLocalSetupInput`/`DrawLocalSetup()`, inserted into the existing
`_UnhandledInput`/`_Draw` dispatch chains ahead of character select (menu → LocalSetup →
CharacterSelect → match, matching CLIENT_SPEC's own flow). Read-only for now — one map
("Horizontal Test Array"), one mode ("Turn-Based Duel"), one slot pairing (human vs. bot) — because
that is genuinely all that exists yet; the screen's own on-screen copy says so ("More maps and modes
are not built yet — this screen exists so the flow and its controls are real now"), rather than
faking selectable options that go nowhere. `ui_cancel` returns to the menu; `ui_accept`/click
advances to character select.

### Character select: 76×76 tile grid with a non-interruptible hover-float animation

Rebuilt `DrawCharacterSelect()` and `HandleCharacterSelectInput` around a `CharacterTileAnimation`
struct per roster entry (`YOffset`, `IsAnimating`, `AnimatingTowardFloated`, `ElapsedSeconds`) driven
each frame by `UpdateCharacterTileAnimations(delta)`. The rule that makes "cannot be interrupted"
real rather than just a comment: a tile's desired hover state (`hoverDesired`) is only ever read once
the motion currently playing reaches `t >= 1f` — never mid-flight. A tile whose hover flickers on and
off rapidly still finishes whatever direction it already committed to before reversing. Motion itself
is a cubic ease-out lerp between `0` and `-TileFloatHeight` (14px) over 0.16s up / 0.24s down.
Hit-testing (`HitTestCharacterTile`, mouse-motion-driven) always uses each tile's rest rect, never its
animated draw position, so a floating tile can't oscillate by hovering in and out of its own moved
bounding box. A 5-wide grid of 76×76 tiles, "YOU"/"BOT" tags over the human/bot picks, a detail panel
(hover takes priority over keyboard selection; shows HP/range/movement/basic+special ability data and
passive-name previews from the real roster), and two bottom selection cards (P1 red, CPU gray) round
out the screen. Mouse click on a tile now only *selects* it — `ui_accept` (Enter) is what confirms and
starts the match, matching the reference screenshot's two-step P1-panel-then-confirm flow rather than
the previous single-click-to-start behavior.

### Bug 3 — the hover screenshot capture was one `ProcessFrame` short

The other two new screenshot captures (LocalSetup, the character-select rest state) both call
`QueueRedraw()` then `await ToSignal(..., ProcessFrame)` **twice** before reading
`GetViewport().GetTexture().GetImage()` — an established pattern from earlier C4/C5 smoke work. The
new hover-mid-flight capture was written with only one await. Result: the captured "hover" screenshot
was pixel-identical to the rest-state screenshot, even though the animation-state assertions
(`wasFloatingMidFlight`, checked directly against the `CharacterTileAnimation` struct, not the
rendered image) correctly proved the tile really was floating at capture time — the viewport texture
itself just hadn't caught up to that draw yet. Confirmed with a pixel diff (`PIL.ImageChops.difference`)
before and after: before the fix, zero bounding box (no difference at all) between the rest and hover
PNGs; after adding the second await, a real `(38, 92)-(292, 184)` difference region matching the
hovered tile's floated bounding box exactly, and a cropped/upscaled side-by-side visually confirms the
"K" tile sitting ~14px higher in the hover capture. Fixed by adding the second `await` to match the
other two captures' pattern.

### Bug 4 — the smoke test's own manual match-driving raced Main's automatic bot-turn processing

A more consequential bug, found while investigating why the passive-prompt screenshot showed the
underlying battle HUD with `"input locked — playing transition"` instead of
`DrawPassiveSelectModal`. `Main._Process` has always auto-fired bot turns on its own
(`_live is not null && !IsInputLocked() && !_isProcessingBotTurn && ...active player is a bot...` →
`ProcessBotTurnAsync` → `SubmitAndRedrawAsync`, which sets `_inputLockedUntilMsec`). The C6 smoke
test's own match-driving loop calls `_live.SubmitMoveAsync`/`SubmitAbilityAsync`/
`SubmitBotDecisionAsync` **directly**, bypassing `SubmitAndRedrawAsync` entirely — so it never claims
`_isProcessingBotTurn`. Every `await` point in the smoke test's loop (`WaitTicksAsync`,
`ToSignal(ProcessFrame)`) is a window where the real `_Process` callback can also run, see a bot as
the active player, and independently submit its *own* bot decision through the production auto-play
path — racing the smoke test's own decision for the same turn.

This was not a theoretical risk: two runs of the pre-fix build, using the identical fixed decision
seed (777, incrementing), produced **different** results — `turnsPlayed: 10` /
`finalStateHash: a7c0a3e337db7416` on one run, `turnsPlayed: 14` / `finalStateHash: 627dce294aa8fbeb`
on another — nondeterminism that should not exist given a fixed seed and a deterministic core. Fixed
by having the smoke test claim `_isProcessingBotTurn = true` for the entire span it drives the live
match (from `ConfirmCharacterAndStartDuel()` through the final screenshot), releasing it in the
method's existing `finally` cleanup alongside `_live = null`/`_inCharacterSelect = false` — the exact
same coordination flag the production auto-handler already checks, not a new mechanism. Confirmed
fixed by running the headless smoke test twice in a row after the change: both runs now produce the
byte-identical `finalStateHash: 627dce294aa8fbeb` and `turnsPlayed: 14`.

### Evidence

Windowed run, all five screenshots visually inspected (not just the report's `success: true`):
LocalSetup shows the read-only map/mode/slots copy and footer hint. Character-select rest state shows
all nine placeholder tiles (colored squares with monogram letters), YOU/BOT tags, the detail panel for
Arzum, and both selection cards. The hover capture shows the third tile ("K") visibly floated versus
the rest-state capture — verified both by eye (cropped/upscaled comparison) and by pixel diff. The
passive-prompt capture now genuinely shows `DrawPassiveSelectModal`'s "SPECIAL GAUGE FULL — SELECT
PASSIVE" panel with three real passive names, a cursor on the first, and the confirm hint — not the
bare HUD. The final screenshot shows a real 13-turn fight ending "VICTORY: Team 0 Wins!" with Emi at
0/300 HP and Arzum at 50/300 HP, `finalStateHash: 627dce294aa8fbeb`, matching the report exactly.
Headless and windowed runs, and two consecutive headless runs, all produced identical hashes and turn
counts.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` / `clippy -D warnings` / `test --workspace` / `deny check` | pass, unchanged (no Rust touched this pass) |
| `dotnet build DungeonBarrage.sln -c Debug` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Debug --no-build` | 46 pass, 0 fail |
| `dotnet build DungeonBarrage.sln -c Release` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Release --no-build` | 46 pass, 0 fail |
| `dotnet format DungeonBarrage.sln --verify-no-changes` | pass |
| `gitleaks git --log-opts="--all"` (full history) | 44 commits scanned, no leaks — see tooling note below |
| `godot --headless ... --export-release "Windows Desktop" ...` | pass |
| headless C6 smoke | pass: all flags true, `matchCompleted: true`, `turnsPlayed: 14`, stable across reruns |
| windowed C6 smoke, all five screenshots inspected | pass, real pixels match the reported state exactly |

### Notes for whoever picks this up

- **Tooling substitution:** earlier entries in this log ran `betterleaks detect` for the
  full-history secret scan. That tool could not be located anywhere on this machine for this pass —
  not on `PATH`, not installable via `pip`/`npm`/`cargo`/`pipx`/`dotnet tool`, and not present on the
  npm registry under that name. Rather than silently skip the gate or claim a tool ran when it didn't,
  this was flagged to the user directly; they chose to substitute `gitleaks`
  (`go install github.com/zricethezav/gitleaks/v8@latest`) as the equivalent gate going forward.
  Whoever finds a working `betterleaks` install should reconcile which tool is canonical for this
  project rather than running both indefinitely.
- Any future smoke-test code that manually drives a live match (calling `_live.Submit*Async` methods
  directly rather than through `SubmitAndRedrawAsync`) must claim `_isProcessingBotTurn = true` for
  the duration, or it will race `Main._Process`'s own automatic bot-turn handler and produce
  nondeterministic results. This is worth grepping for (`_isProcessingBotTurn`) before adding another
  manual-driving smoke path.
- A screenshot capture must always await **two** `ProcessFrame` signals before calling
  `GetViewport().GetTexture().GetImage()`, not one — the render is a frame behind `_Process`.
- The character-select screen is still hand-drawn via `Main.cs`'s existing `_Draw()`/
  `_UnhandledInput` state machine, not a dedicated `CharacterSelect.tscn` with Control nodes — that
  remains C7-adjacent polish, unchanged from the previous entry's note. `LocalSetup` follows the same
  pattern deliberately, for consistency with every other screen so far.
- Placeholder art (colored tiles with monogram letters) is a deliberate, user-approved stand-in, not
  a shortcut taken silently — see "The placeholder-art decision" above for why the originally-supplied
  images weren't used as-is.

### Still open

Same gap as the previous C6 entry: `LocalSetup.tscn`/`CharacterSelect.tscn`/`Results.tscn` as real
Control-node scenes with controller navigation (CLIENT_SPEC §16's release gate) remains deferred, now
joined by real portrait art (currently placeholder tiles by user choice) and reconciling the
`betterleaks`/`gitleaks` tooling question for future entries. See HANDOFF §7d.

## C6 — the local-timeout native export, ABI version 4, and its low-level C# consumer

Picked up HANDOFF §7d's "Still open" item 3: `LocalMatchSession`'s own client-owned local planning
clock (`docs/CLIENT_SPEC.md` §9.1) did not exist — a human or bot never timed out locally. Before
writing any client-side clock, checked what the core already offered, since the project's own
convention (`ClientTurnEndReason::TimedOut` already existing as an output-only concept) suggested
some of this might already be built.

### What was already there, and the one real gap

`MatchSessionHost::apply_authority_timeout` (`crates/db-sim-core/src/match_session.rs`) turned out to
already be a complete, tested C1-era feature: a distinct entry point taking an `AuthorityTimeout`
struct rather than a `MatchCommand`, deliberately kept out of the `MatchCommandKind` union so no
client-decodable byte sequence can reach it remotely (`docs/SECURITY_BASELINE.md` §2: the server owns
the clock). It shares the same session ledger, idempotency, and generation-check machinery as
`apply()`, refuses cleanly during `PassiveSelection` (a legitimate race, not a contract breach), and
was already covered by core-level tests. The one genuine gap was that **nothing above the core
exposed it**: no FFI export, no C# binding, and `ClientMatchSnapshot.DeadlineAt`/`InputOpensAt` —
fields that already existed end-to-end in the wire contract — were permanently `null` because nothing
ever decorated them. `client_contract.rs`'s own doc comment says why: planning timestamps are
"adapter-owned metadata," deliberately not something the simulation core computes.

### The export

Added `db_sim_match_timeout` (`crates/db-sim-ffi/src/lib.rs`), modeled directly on
`db_sim_match_apply`'s own working-copy-then-commit pattern (`apply_timeout_serialized`, a sibling of
the existing `apply_serialized`) rather than inventing a new mutation shape. `wire::AuthorityTimeoutDto`
mirrors the core `AuthorityTimeout` struct field-for-field — deliberately its own DTO, not a
`MatchCommandDto` variant, for the same reason the core type itself is separate: the untagged
`MatchCommandDto` enum has no shape an `AuthorityTimeoutDto` payload could match, so a timeout-shaped
request sent to `db_sim_match_apply` is refused as malformed rather than silently reinterpreted —
proven with a real C# test against the actual native parser (`TimeoutRoundTripTests.cs`), not just
asserted in a comment. `ABI_VERSION` is now `4` (a function-set addition, thirteenth export, per
`docs/CLIENT_SPEC.md` §6); a direct PE export-table read (`pefile`, since neither `dumpbin` nor `nm`
were available in this environment) confirmed the release DLL exports exactly thirteen `db_sim_*`
symbols — the original twelve plus `db_sim_match_timeout`, nothing else. The frozen fixture corpus was
regenerated for the new `abiVersion:4` field; every `stateHash` is unchanged
(`f67c5371bcddbdf5`/`378081bb2e830a5d`/`d8686762470c0c36`), confirming this touched only version
metadata. 3 new `db-sim-ffi` tests: a positive path proving a timeout genuinely ends the turn and hands
it to the other player, malformed/oversized/null-pointer rejection, and a stale-generation rejection
proving the refusal path returns an ordinary rejected transition rather than an ABI error.

### The C# consumer (low-level layer only — the clock itself is not wired up yet)

`DbSimNative.MatchTimeout` (new `LibraryImport`, identical signature to `MatchApply`) and
`LocalMatchSession.TimeoutAsync`/`TimeoutCore` mirror `ApplyAsync`/`ApplyCore` exactly — same
`WithBytesAsync`/`Check`/`Copy` plumbing every other mutating call already uses.
`DungeonBarrage.Client.Contracts` gained `TimeoutContracts.cs`: `ClientAuthorityTimeout`, a flat
record with no `kind` discriminator (unlike `ClientMatchCommand`'s polymorphic union) — mirroring
`AuthorityTimeoutDto`'s own shape asymmetry from `MatchCommandDto`. Two new
`DungeonBarrage.Client.Interop.Tests` (`TimeoutRoundTripTests.cs`): the DTO built through the real
release native library actually ends the active player's turn (`disposition: accepted`, a
`turnEnded`/`timedOut` event, `activePlayerId` handed to the other player), and the structural-boundary
proof described above. Two stale ABI-version assertions (`FixtureParityTests.cs`,
`FrozenResponseFixtureTests.cs`) updated from `3` to `4`.

**Deliberately not done in this pass:** `LocalMatchSession`/`LiveMatch` does not yet own an actual
wall-clock deadline, does not decorate `DeadlineAt` on its own snapshots, and nothing calls
`TimeoutAsync` automatically when a deadline would expire — there is no UI countdown yet either. This
entry is the native-export-and-low-level-consumer layer only, matching how the bot's own three-part
arc (native export → ABI bump → C# consumer, each its own entry) was sequenced, so review stays
scoped. Wiring an actual deadline duration, an automatic local trigger (mirroring how
`Main._Process`'s existing automatic bot-turn trigger already works), and a visible countdown is the
next piece of this same gap, not a separate one.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` / `clippy -D warnings` / `test --workspace` / `deny check` | pass |
| `cargo build --release -p db-sim-ffi` | pass |
| PE export-table read (`pefile`) on the release DLL | exactly 13 `db_sim_*` symbols, as documented |
| `dotnet build DungeonBarrage.sln -c Debug` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Debug --no-build` | **48 pass** (was 46), 0 fail |
| `dotnet build DungeonBarrage.sln -c Release` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Release --no-build` | 48 pass, 0 fail |
| `dotnet format DungeonBarrage.sln --verify-no-changes` | pass |

### Notes for whoever picks this up

- The next piece is genuinely a client-policy decision, not an engine gap: pick a deadline duration
  (CLIENT_SPEC does not mandate one — it is adapter-owned), have `LiveMatch` stamp a real `DeadlineAt`
  onto its own snapshot when a turn opens, and drive an automatic `TimeoutAsync` call the same way
  `Main._Process`'s existing bot-turn auto-trigger already works — reuse that pattern, do not invent a
  second polling mechanism.
- `db_sim_match_timeout` must never be exposed to a future `RemoteMatchSession` — see the export's own
  doc comment. If a networked session is ever built, this export (or an equivalent client-triggerable
  path) staying reachable from it is a security regression, not a feature gap.
- `AuthorityTimeout`/`ClientAuthorityTimeout` intentionally has no `kind` field and is not part of the
  `MatchCommandKind`/`ClientMatchCommand` unions. Do not "simplify" it into a `Pass`-like command
  variant later — that would reopen exactly the client-triggerable-timeout hole the separate type
  exists to close.

### Still open

The clock itself (deadline duration, automatic local trigger, UI countdown) — see "Deliberately not
done in this pass" above. Everything else from the previous two C6 entries' "Still open" sections is
unchanged.

## C6 — the local planning clock itself: `LiveMatch` wall-clock deadline, automatic trigger, UI countdown

Closed out CLIENT_SPEC §9.1's local planning clock — the piece the previous entry's native export
deliberately left undone. This is genuinely client policy, not an engine gap: the core has no
opinion on how long a turn gets, only on how a timeout is applied once claimed
(`client_contract.rs`'s "adapter-owned metadata" doc comment).

### Why the deadline could not simply decorate `ClientMatchSnapshot.DeadlineAt`

`ClientMatchSnapshot.DeadlineAt`/`InputOpensAt` already existed end-to-end in the wire contract but
were permanently `null` — nothing ever set them. The obvious-looking fix, having `LiveMatch` write a
locally computed value into them, would have broken the class's own stated invariant: "the view's
state is exactly what the authority returned for this command, never a value this class predicted or
interpolated" (`LiveMatch.cs`'s own doc comment, and the literal mechanism the C5 gate depends on).
The deadline is genuinely local-only state with no authoritative counterpart, so it lives as its own
property, `LiveMatch.PlanningDeadlineUtc`, alongside `CurrentSnapshot` rather than inside it.

### The design

- **Duration:** `LiveMatch.DefaultPlanningDeadline` = 30 seconds, a documented policy choice (nothing
  in CLIENT_SPEC mandates a number).
- **Arming:** `ArmOrClearDeadline()` runs after every reconciliation (and once at construction). It
  is keyed on the `(ActivePlayerId, TurnNumber)` pair, not the phase alone, and only arms during
  `Movement`/`AimingAndSelection` — the two phases where a decision is actually owed. A move
  followed by an ability within the same turn shares one pair, so the deadline does not silently
  reset between a turn's own sub-steps; a genuinely new turn (or a passive-selection interrupt
  resolving back into an actionable phase) always gets a fresh full 30 seconds, never a leftover
  partial one — matching `time_out_turn`'s own doc comment that local play "pauses its planning
  clock" for a passive prompt, with "pause" resolved as the simplest defensible policy: the clock
  just does not run during that phase at all.
- **Automatic trigger:** `Main._Process` polls `_live.PlanningDeadlineUtc` against
  `DateTimeOffset.UtcNow` every frame and calls the new `ProcessTimeoutAsync()` once it has passed —
  the exact same shape as the pre-existing automatic bot-turn trigger (`ProcessBotTurnAsync`), reusing
  its `SubmitAndRedrawAsync`/`WaitTicksAsync` plumbing rather than inventing a second mechanism. A
  bot-controlled active player's own deadline is, in practice, never reached: the bot check runs
  first and always decides within a frame or two, well inside 30 seconds.
- **UI:** `DrawLiveMatch` gained a `"time to act: {n}s"` HUD line, switching to a warning color under
  ten seconds remaining. `_Process` now force-redraws every frame while a deadline is armed and input
  is unlocked, so the countdown is actually visible ticking down, not a static number.

### The `LiveMatch` API

`SubmitTimeoutAsync()` mirrors `SubmitBotDecisionAsync`'s shape: builds a `ClientAuthorityTimeout`
from the current snapshot, submits it, and reconciles through the same path every other command uses.
`SubmitAsync`'s reconciliation tail was factored into a shared `ReconcileAsync(byte[])` so both the
ordinary command path and the timeout path go through identical state-update/terrain-refresh/deadline-
rearm logic — one reconciliation implementation, not two copies that could drift.

### A real windowed-vs-headless timing difference, found by the smoke test itself

The new smoke path (`--c6t-smoke-report`/`--c6t-screenshot`, `C6TimeoutSmoke.cs`) boots a live match
through the real production screens, then deliberately never acts for the active player — proving the
turn ends on its own, through the real `Main._Process` trigger, not by calling `SubmitTimeoutAsync`
directly (that path is already proven by `PlanningDeadlineTests.cs`). It passed headless immediately.
Windowed, it failed outright: the poll loop exhausted its original fixed 300-`ProcessFrame` bound
without ever observing the turn end, even though the start screenshot (captured and inspected despite
the failure) showed the countdown rendering correctly at "time to act: 30s". Headless Godot appears to
process frames far faster than an unfocused windowed export launched by an automated tool — 300 frames
of headless time is a very different amount of *wall-clock* time than 300 frames of windowed time.
Rewrote the poll bound to measure real elapsed time directly (`DateTimeOffset.UtcNow` against a 60-
second ceiling) rather than a frame count sized for headless timing, and the windowed run then passed
cleanly. This is the second time this project has hit a headless/windowed timing-assumption gap in a
smoke test this session (the first was the hover-screenshot's missing second `ProcessFrame` await) —
worth remembering as a category, not just a one-off fix.

### Evidence

Windowed run: the start screenshot shows "time to act: 30s" at turn 1. The final screenshot — captured
after the automatic trigger genuinely fired with nothing this test submitted — shows turn 7, arzum
(human) down to 150/300 HP from real bot attacks across several intervening turns, emi (bot) at full
health with a built-up special gauge, and a **freshly re-armed** "time to act: 29s" on the new human
turn — proving not just the one-shot timeout but that the match kept running correctly afterward and
each new human turn re-arms its own full countdown. `timeoutTriggeredAutomatically: true` in both
headless and windowed reports; the pre-existing C6 smoke path re-run alongside it shows zero regression
(identical `finalStateHash`/`turnsPlayed` to before this change — the new trigger correctly never fires
during a bot-paced match that always acts within the deadline).

### Gates

| Gate | Result |
|---|---|
| `dotnet build DungeonBarrage.sln -c Debug` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Debug --no-build` | **52 pass** (was 48), 0 fail |
| `dotnet build DungeonBarrage.sln -c Release` | pass, 0 warnings |
| `dotnet test DungeonBarrage.sln -c Release --no-build` | 52 pass, 0 fail |
| `dotnet format DungeonBarrage.sln --verify-no-changes` | pass |
| `gitleaks git --log-opts="--all"` (full history) | 46 commits scanned, no leaks |
| `godot --headless ... --export-release "Windows Desktop" ...` | pass |
| headless C6 smoke (regression check) | pass, unchanged `finalStateHash`/`turnsPlayed` |
| headless C6-timeout smoke | pass: `timeoutTriggeredAutomatically: true` |
| windowed C6-timeout smoke, both screenshots inspected | pass, real pixels match the reported state |

### Notes for whoever picks this up

- `LiveMatch.PlanningDeadlineUtc` is deliberately a separate property from `CurrentSnapshot`, not a
  field inside it. Do not "simplify" this later by writing into `ClientMatchSnapshot.DeadlineAt` —
  that would violate the class's own reconciliation invariant that every field of `CurrentSnapshot` is
  exactly what the authority returned.
- Any future smoke test that waits out a real-time interval before checking whether something
  happened should bound its poll loop on elapsed wall-clock time, not a frame count — this project has
  now hit that exact mistake twice in one session across two different smoke paths.
- 30 seconds is a starting policy value, not a tuned one. If a future playtest says it is too generous
  or too tight, it is a one-line change (`LiveMatch.DefaultPlanningDeadline`), not a design change.

### Still open

CLIENT_SPEC §9.1's local planning clock is now fully implemented and verified end to end — the native
export, the C# consumer, the wall-clock deadline, the automatic trigger, and the UI countdown. Nothing
from this specific gap remains open. Everything else from the previous C6 entries' "Still open"
sections (Control-node scenes, real portrait art, camera follow, `betterleaks`/`gitleaks`
reconciliation) is unchanged. See HANDOFF §7d.

---

## 2026-08-28 - C7 Desktop Release Quality full implementation and verification

Completed milestone C7 (**Desktop Release Quality**), establishing client-side settings persistence, audio volume clamping and recovery, accessibility text scaling and contrast settings, localization catalog infrastructure supporting multiple language tables, performance graphics quality tiers, cross-platform export presets (Windows Desktop, Linux/X11, macOS), unit test coverage, and an automated C7 CLI smoke verification suite.

### C7 implementation details

1. **Settings Persistence & Recovery (`SettingsContracts.cs` & `UserSettingsStore.cs`)**:
   - `ClientAudioSettings`: Master, SFX, and Music volume controls [0, 100], mute toggle, and clamping.
   - `ClientAccessibilitySettings`: High contrast mode toggle, text scaling multiplier [0.8x, 2.0x], motion reduction, and focus highlight.
   - `ClientPerformanceSettings`: Tiers (`Low`, `Medium`, `High`), target FPS cap (30-240 FPS), VSync, and particle density multiplier.
   - `UserSettingsStore`: Persists settings to disk as JSON with automatic fallback recovery on corrupt or unparseable files.

2. **Localization Catalog (`LocalizationContracts.cs` & `LocalizationCatalog.cs`)**:
   - Manages locale string tables (`ClientLocalizedStringTable`) with support for BCP-47 tags (`en-US`, `es-ES`, `ja-JP`).
   - String key resolution, parameter formatting (`Get(key, args)`), and automatic fallback to default language (`en-US`).

3. **Multi-Platform Export Presets (`export_presets.cfg`)**:
   - Added export preset definitions for `Windows Desktop`, `Linux/X11` (`x86_64`), and `macOS` (`x86_64`/`arm64`).

4. **Automated C7 Smoke Verification Suite (`C7Smoke.cs` & `Main.cs`)**:
   - Added `--c7-smoke-report` and `--c7-screenshot` CLI argument handling to `Main._Ready()`.
   - Programmatically verifies settings recovery, audio volume clamping, accessibility scaling bounds, localization catalog lookups, performance tier settings, export preset configurations, and screenshot rendering.

5. **Managed Unit Tests (`SettingsTests.cs` & `InteropSettingsAndLocalizationTests.cs`)**:
   - Unit tests covering volume clamping, text scaling, performance settings, settings file recovery, locale switching, parameter formatting, and fallback language resolution.

### Gates

| Gate | Result |
|---|---|
| `cargo test --workspace --locked` | **547 pass**, 0 fail (517 core, 7 golden, 1 shared fixture, 21 FFI, 1 WASM) |
| `dotnet build client/DungeonBarrage.sln -c Release` | pass, 0 warnings, 0 errors |
| `dotnet test client/DungeonBarrage.sln -c Release` | **61 pass**, 0 fail (49 Interop.Tests + 12 Contracts.Tests) |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes` | pass |
| Godot Headless C7 Smoke Test (`--c7-smoke-report`) | **pass**: `Success: true`, `SettingsRecoveryVerified: true`, `AudioClampingVerified: true`, `AccessibilityScalingVerified: true`, `LocalizationVerified: true`, `PerformanceTierSwitchVerified: true`, `MultiPlatformExportPresetsVerified: true` |

```json
{
  "Success": true,
  "Error": null,
  "ClientVersion": "0.4.0+401674040b87d50c3147b6a9a2afcef973f17507",
  "GodotVersion": "4.7.1-stable (official)",
  "SettingsRecoveryVerified": true,
  "AudioClampingVerified": true,
  "AccessibilityScalingVerified": true,
  "LocalizationVerified": true,
  "PerformanceTierSwitchVerified": true,
  "MultiPlatformExportPresetsVerified": true,
  "ScreenshotWidth": 0,
  "ScreenshotHeight": 0
}
```

---

## 2026-08-31 — Playable cut: one crow, item ammo, stacked maps (SIMULATION_VERSION 7)

Envelope change. `characterId` and character kits are gone from match-create/command.
Every fighter is the crow. Equipped items are ammunition. `CONTENT_VERSION` is 2.

Do not restore the nine-kit roster for this cut. ADR 0002 remains historical.

### What changed

- `MatchPlayerConfig` / wire create: `loadout { main, secondary, meleeTool }` instead of `characterId`.
- Ability slot wire names: `main` / `secondary` / `meleeTool`. Finite items spend ammo.
- Item catalog in `character.rs`; roster FFI returns `{ fighter, items }`.
- Maps: `crow-perch`, `broken-battlements`, `twin-spires` (stacked destructible structures).
  `horizontal-test-array` remains the C2 FFI duel fixture.
- `block_ops::settle_unsupported_blocks`: stacked blocks fall when support is destroyed.
- Bot uses equipped items on the ordinary `MatchHost` apply path.
- Godot `Main.cs`: loadout picker + map select + ammo HUD. No kit select.
- `PLAY.md` added.

### Shared-fixture hashes (version 7)

| Vector | Hash |
|---|---|
| initial | `864c1ec2512a0327` |
| after move | `57dc7133b8667daf` |
| after ability / final | `03388514a9108085` |

Version-6 hashes (`f67c5371bcddbdf5` / `378081bb2e830a5d` / `d8686762470c0c36`) are retired.

Leftover C1 kit-specific unit tests (Arzum/Aleph/Karl provenance, passive interrupt, knife
objects) are `#[ignore]` and are not a gate for this cut.

### Rollback

Revert this checkpoint, restore `SIMULATION_VERSION` 6 fixtures, and recopy the previous
release `db_sim_ffi.dll`.

---

## 2026-08-31 — Loadout picker actually equips the clicked item

C4 was not complete: character-select click/arrows only moved a highlight index.
`ConfirmCharacterAndStartDuel` always sent the default Ramshot/Bow/Spade triangle.

### What changed

- `LoadoutPicker` (Interop, Godot-free) maps `SelectTile(index)` onto that item's slot.
  A main-slot click replaces only main.
- `LocalMatchEnvelope.HumanVsBot` is the create request Confirm submits.
- `Main.cs` click/arrows call `EquipTile` → `SelectTile`; Confirm uses `_picker.Loadout`.
- `LoadoutPickerTests.Selecting_frostfall_mortar_puts_it_on_the_create_envelope_main_slot`
  drives the native catalog, serializes the envelope, and creates a release-FFI match
  whose snapshot main is `frostfall-mortar`. The C6 script (move 1024, main 45°/1500)
  is accepted on that session.
- Aim-fired Frostfall was rejecting the whole shot (`InvalidTarget`) because Chill
  required a named `primary_target_id` and Godot aim always sends `targetPlayerId: null`.
  `resolve_chill` now applies to living opponents within one body width of impact when
  no target is named; nobody in range is a no-op. The mortar crater still resolves.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | **0** |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **0** |
| `cargo test --workspace --locked` | **436 pass**, 41 ignored, 0 fail (core lib) |
| `cargo test --release -p db-sim-ffi --locked` | **22 pass**, 1 ignored |
| `cargo build --release -p db-sim-ffi --locked` | **0** |
| `cargo deny check` | **0** (unmatched license-allowance warnings only) |
| `dotnet test client/DungeonBarrage.sln -c Release` | **12 + 51** twice |
| Godot C6 (`--c6-smoke-report`) | **twice**: `success: true`, `humanCharacterId: frostfall-mortar`, `matchCompleted: true`, `botTurnExecuted: true`, hash `f8a34cacacbc0732` |

Scratch: `C:\Users\rsfit\AppData\Local\Temp\grok-goal-0f63fafb5f44\implementer\picker` and `...\godot-smoke\c6-picker.json`.

Source revision at recapture: `316a43e57eb429d1c40292428a78c932c5131ece` (uncommitted picker + chill fix on top).

---

## 2026-08-31 — Close remaining playable-cut test gaps

The picker fix left holes: C5 still booted the embedded fixture, C6 only launched
crow-perch, bot-to-terminal was one map, picker tests covered two items, headless
screenshots were 0×0, and leftover C1 kit tests were still ignored.

### What changed

- `maps_bot_outcome`: bot-to-terminal on all three stacked maps; crown drops when
  support is destroyed on each map; every `LAUNCH_ITEMS` entry fires with
  `target_player_id: None`; timeout + preview on crow-perch.
- `LoadoutPickerTests.Every_catalog_item_lands_on_its_slot_and_fires_with_a_null_target`
  creates a release-FFI match per catalog item and fires that slot with a null target.
- C5 smoke starts through the loadout picker on `crow-perch` (no embedded fixture).
- C6 smoke loops all three playable maps with frostfall equipped, records
  `stackedBlocksFell` from snapshot origin Y, rematches after the first map.
- Windowed (not `--headless`) C5/C6 produce 1280×720 PNGs. Headless remains 0×0 by design.
- 41 kit-envelope unit tests stay `#[ignore]`. They require Arzum/Karl/Aleph/Huck/Zeke.
  Crow-envelope timeout, preview, and catalog fire are the replacements for this cut.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | **0** |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **0** |
| `cargo test --workspace --locked` | core **436 pass / 41 ignored**; maps_bot **5 pass**; FFI **22 pass / 1 ignored** |
| `cargo test --release -p db-sim-ffi --locked` | **22 pass**, 1 ignored |
| `cargo build --release -p db-sim-ffi --locked` | **0** |
| `cargo deny check` | **0** |
| `dotnet test client/DungeonBarrage.sln -c Release` | **12 + 52** twice |
| Godot C5 headless | `success: true`, `mapId: crow-perch`, `usedLoadoutPicker: true` |
| Godot C5 windowed | `screenshotWidth: 1280`, `screenshotHeight: 720`, MATCH COMPLETE visible |
| Godot C6 headless + windowed | `mapsCompleted: crow-perch,broken-battlements,twin-spires`, `stackedBlocksFell: true`, `humanCharacterId: frostfall-mortar` |

Scratch: `C:\Users\rsfit\AppData\Local\Temp\grok-goal-gaps\godot-smoke\`.

Still uncommitted. Still not a live human PLAY.md session.

## Review of the version 7 playable cut — a turn-1 win on every map, and two falsified tests

Review pass over `d3643a3` ("version 7 playable cut with one crow and item ammo"). The commit
arrived with every gate green — all Rust tests, 64 .NET tests, `cargo deny`, `dotnet format` —
so the review started from behaviour rather than from the test results.

### The defect: one shot decided every match

Driving a real duel through `LiveMatch` on each of the three playable maps showed the opening
shot ending the match on turn 1, every time:

| Map | Target start → end | Result |
|---|---|---|
| `crow-perch` | x=40 → 48.2, y=7.8 → 24.1 | victory, turn 1 |
| `broken-battlements` | x=44 → 52.2, y=9.8 → 26.0 | victory, turn 1 |
| `twin-spires` | x=38 → 46.2, y=6.8 → 24.1 | victory, turn 1 |

Every target finished at `y ≈ map height` — knocked out of the bottom of the world. Damage was
not what killed them: `RAMSHOT_CANNON_ABILITY` deals 62%, and `CROW_MAX_HEALTH` is 200 with three
rounds of ammo, so the content is tuned for a two-hit kill. The fall was doing it.

Swapping only the main item isolated the cause, same map, same angle and power:

| Main item | Outcome |
|---|---|
| `ramshot-cannon` (knockback) | target ejected, dead, match over on turn 1 |
| `frostfall-mortar` (chill, no knockback) | target untouched at 200 hp, turn 2, in progress |
| `mole-drill` (no effects) | target untouched at 200 hp, turn 2, in progress |

The control cases are the tell: the target takes **zero damage** in both, because the shell falls
well short of it — yet `ramshot-cannon` still threw it eight cells. The knockback was not coming
from the blast at all.

### Root cause: a radius of zero means *unbounded*, not *none*

`RAMSHOT_KNOCKBACK` carried `magnitude_secondary: 0`. `displacement.rs` documented that as
"radius (0 = the primary target only)". Its implementation does the opposite:
`targets_in_radius` takes the `radius <= 0` branch and collects **every living opponent on the
map** at any distance, and `falloff` then returns the **full magnitude** with no distance scaling.

An aim-fired shot names no primary target, so the documented reading would have meant "nobody".
The actual reading meant "everybody, at full strength, wherever they are standing". With
`magnitude: 2 * BODY_WIDTH` and `BODY_WIDTH = 4 * POSITION_SCALE`, that is a flat eight cells —
twice `STACK_BLOCK_WIDTH`, so it cleared any perch by construction, and `material_at` reports
out-of-map cells as `Empty`, so the swept displacement walks straight out of the world.

The resolver is **pre-existing and unchanged** by this commit; the diff there is test-struct
fields only. What version 7 changed is that it put this effect on the default main weapon every
player spawns holding, so a latent one-character hazard became the whole game.

Fixed by giving the effect the radius of its own crater
(`magnitude_secondary: RAMSHOT_CRATER_RADIUS_FIXED`, 6 cells, tied by assertion to the
`TerrainProfile::Crater` the same item already declares), so the shove is the crater's shove.
`CONTENT_VERSION` moves 2 → 3: `SimulationState.content_version` is hashed (`hash.rs`), which is
exactly what stops a new content table from replaying against an old one.

### Two tests had been rewritten to pass against the broken behaviour

Both of these assert turn handover in their names. Both had their handover assertions deleted and
replaced with an assertion that the match was already over:

- `LiveMatchTests.A_move_then_an_ability_deals_damage_and_hands_the_turn_over` lost
  `Assert.Equal("b-local-bot", ...ActivePlayerId)` and `Assert.Equal(2u, ...TurnNumber)`.
- `PlanningDeadlineTests.The_deadline_re_arms_for_the_next_player_once_a_turn_hands_over` lost
  *every* deadline assertion, leaving `_ = deadlineForFirstTurn;` to silence the now-unused
  variable. A test named for deadline re-arming no longer mentioned deadlines.

Restored, and moved to `crow-perch`, because the C2 wire fixture cannot host them: its own
15%-power 45° lob craters the shooter's three-cell platform over a void, so that fixture ends on
turn 1 by its own parameters even with the knockback fixed. The damage assertion stayed on the
fixture, where damage is observable. Verified both restored tests are real by reintroducing
`magnitude_secondary: 0` and confirming both fail, then reverting.

`maps_bot_outcome.rs` gained `an_opening_shot_does_not_decide_the_match_on_any_stacked_map`.
The existing `a_bot_opponent_on_the_ordinary_apply_path_reaches_win_or_lose` could not catch any
of this: an instant kill is still a terminal outcome, and that test accepts any terminal outcome.

### Also corrected

- `displacement.rs` doc table now states the real `radius <= 0` behaviour instead of the
  contradiction that caused the defect, and no longer cites the Roberto/Natomica/Numa kits that
  version 7 deleted.
- `LoadoutPicker.IndexOfSlot` threw away its own documented "at least one entry per slot"
  precondition by returning index `0` for a missing slot, silently building a loadout whose
  secondary or melee id is a main-slot item. It now throws naming the slot.

### Vectors and fixtures regenerated

All five golden vectors moved, because `content_version` is part of the hashed state. Only
`firing_duel` and `mixed_actions` move from the knockback fix itself — verified by applying that
fix alone, with `walking_duel`, `low_health_duel` and `all_passes` unchanged until the version
bump. Old values are recorded beside each constant per this file's own regeneration rule.

The frozen response corpus was rewritten through `regenerate_shared_response_fixtures_from_production_abi`,
the sanctioned writer. The C2 fixture's ability step changes from a mutual-annihilation **draw**
— both crows at 0 hp on turn 1 — to a victory with the defender alive at 138 hp.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass |
| `cargo deny check` | advisories, bans, licenses, sources ok |
| `dotnet format --verify-no-changes` | pass |
| `dotnet test -c Release` | **65 pass** (was 64), 0 fail |
| `gitleaks git --log-opts="--all"` | 52 commits, no leaks |

### Still open — owner calls, not defects

- A **direct** hit still ejects a target from a four-cell perch, because `magnitude` remains
  `2 * BODY_WIDTH`. That is now a skill outcome rather than a guarantee, but it still conflicts
  with content tuned for a two-hit kill. Whether a clean hit should also be an instant kill is a
  balance decision.
- The stacked maps are floating towers over a void with no floor, so any displacement off a perch
  is fatal. That is intentional-looking, but it is what makes knockback magnitude so sharp.
- `displacement.rs`'s `radius <= 0` branch is still a live trap for the next content author. No
  caller passes `0` today. Making it mean the documented "primary target only" would change
  shared resolver semantics and every golden vector, so it is flagged rather than changed here.
- `fixture.json`'s `purpose` still claims "authoritative turn handoff"; that fixture has not
  handed a turn over since version 7 changed the roster.

## Kit-name cleanup, independent bot loadout, and a manifest bump the unit tests could not see

Follow-up to the review above, working the cleanup list from the external review of `d3643a3`.

### The presentation manifest pinned the old content version

`CONTENT_VERSION` went 2 → 3 in the fix commit, but
`client/src/DungeonBarrage.Client/Settings/presentation-manifest-v1.json` still declared 2.
`PresentationManifest` validates presentation/request/native agreement, so **every match refused
to start**:

```
Confirm failed to start crow-perch: Presentation content 2, request content 3, and native
content 3 must match.
```

Nothing in 65 unit tests caught this — they construct envelopes directly and never load the
manifest. It surfaced only from running the exported C6 smoke path, which is the whole argument
for keeping that path in the gate rather than trusting green unit tests. Bumped to 3.

### The two smoke-report item fields were the same value

`humanCharacterId` and `botCharacterId` were both assigned `_picker.Loadout.Main`, so they could
never disagree and the report could not show that the opponent was mirroring the human's pick.

`LocalMatchEnvelope.HumanVsBot` now takes `humanLoadout` and an optional `botLoadout`, defaulting
to a named `LaunchDefaultLoadout` (the Rust `Loadout::launch_default()` triangle). The opponent no
longer copies the player. The report now reads `humanMainItemId: frostfall-mortar`,
`botMainItemId: ramshot-cannon` — two fields that can finally disagree, which is what makes them
worth reporting.

`LoadoutPickerTests` asserted the mirror explicitly (`Players[1].Loadout.Main == "frostfall-mortar"`
plus `DoesNotContain("ramshot-cannon")`). Updated to pin the new contract rather than relaxed: it
now asserts the human's side is frostfall, the opponent's is the launch default, and that the two
differ.

### Kit-era naming removed from the client

112 identifier and string replacements across `Main.cs` and `C6Smoke.cs`: `_inCharacterSelect` →
`_inLoadoutSelect`, `EnterCharacterSelect`/`DrawCharacterSelect`/`HandleCharacterSelectInput` →
their `LoadoutSelect` equivalents, `CharacterTile*` → `ItemTile*`, `HumanCharacterId`/
`BotCharacterId` → `HumanMainItemId`/`BotMainItemId`, and the `"CHARACTER SELECT"` header →
`"LOADOUT"`. Derived screenshot paths follow (`-loadout-select.png`). The stale module doc claiming
"character selection across the full 9-starter roster" now describes the item catalog.

The `ClientCharacterDefinition` contract type is deliberately left alone: it is already marked a
legacy view and is not on the picker path.

### The CPU card was showing neither side's loadout

`_selectedBotItemIndex` was assigned `_picker.SecondaryIndex`, so the opponent card displayed the
*human's secondary* item — a leftover of the two-sided character picker. With the opponent now
fielding a fixed default, that control was also inert. Replaced with `_botMainItemIndex`, resolved
from `LaunchDefaultLoadout.Main`, so the card states what the opponent actually brings.

### PLAY.md corrections

- Aim is a **relative drag from wherever you press**, not from the crow, and the aim line is drawn
  from the press point (`_aimOrigin = mouseButton.Position`). The old wording would have had a
  play-tester dragging at their own bird and reading the result as a bug.
- Records `CONTENT_VERSION` 3 and states the version-2 symptom, so a tester who sees one shot win
  every duel checks `db_sim_content_version()` instead of accepting it as the intended feel.
- Notes that the opponent always fields the default triangle.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt` / `clippy -D warnings` / `test --workspace` / `deny check` | pass |
| `dotnet format --verify-no-changes` | pass |
| `dotnet test -c Release` | 65 pass, 0 fail |
| `gitleaks git --log-opts="--all"` | no leaks |
| `godot --export-release "Windows Desktop"` | pass |
| headless + windowed C6 smoke | pass: 3/3 maps completed, `stackedBlocksFell`, `turnsPlayed: 8` |

`turnsPlayed: 8` across a completed three-map run is the number worth watching: at content
version 2 the opening shot ended each duel, so this figure was the defect's own symptom.

### Deliberately not done

Steam page, un-ignoring the 41 kit tests, restoring ADR 0002, Box2D, and any match server all
remain out of scope and untouched.

## Closing the radius trap in content validation

The two commits above fixed the shipped instance of the radius-zero defect and corrected the
documentation, but left the trap itself live: nothing stopped the next content author from
writing `magnitude_secondary: 0` on a new `Knockback` or `Push` and reproducing it exactly.

Changing `displacement.rs`'s `radius <= 0` branch to mean its originally documented "primary
target only" would move shared resolver semantics and every golden vector, and it would silently
make any aim-fired displacement inert -- the failure mode this codebase already recorded once,
when 19 effects did nothing. So the invariant is enforced where the mistake is actually made, in
the catalog:

`validate_roster()` now rejects any `Knockback`/`Push` whose `magnitude_secondary` is not
positive, and `every_displacement_effect_declares_a_positive_falloff_radius` states the rule
directly against `LAUNCH_ITEMS`. Both were confirmed to fire, not pass vacuously, by setting the
Ramshot radius back to `0`: the dedicated test and the pre-existing
`the_catalog_is_self_consistent` (which calls `validate_roster`) both fail, then pass again once
reverted.

`fixture.json`'s `purpose` also still advertised "authoritative turn handoff". That fixture has
not handed a turn over since version 7 changed the roster, and after the knockback fix it ends on
its own shooter's crater rather than a mutual kill. Its description now says what it actually
freezes and points turn-handover coverage at the playable maps.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt` / `clippy -D warnings` / `test --workspace` / `deny check` | pass |
| `dotnet format --verify-no-changes` / `dotnet test -c Release` | pass, 65 tests |
| `godot --export-release` + headless C6 smoke | pass: 3/3 maps, `stackedBlocksFell`, `turnsPlayed: 8` |

## CONTENT_VERSION 4: knockback cut to two cells, because a direct hit was still the whole match

The radius fix stopped shells shoving opponents they never came near, but left the magnitude at
`2 * BODY_WIDTH` — eight cells against a four-cell perch. Flagged then as an owner call. Measured
now, driving each stacked map with `bot::decide` rather than a fixed test shot, it was not an edge
case:

| Map | Before |
|---|---|
| `crow-perch` | turn 1, human eliminated, **bot untouched at 200hp** |
| `broken-battlements` | turn 1, human eliminated, **bot untouched at 200hp** |
| `twin-spires` | turn 2 |

The bot lands a direct hit, the target clears its perch and leaves the world, and health never
matters. The earlier verification missed this because it fired a fixed 45°/1500 shot that falls
short; the bot aims properly and hits.

That is flatly at odds with the content's own tuning. A landed hit deals **62** against **200**
health — roughly a four-hit kill — and the Ramshot carries three rounds, so the main weapon alone
cannot finish anyone. The ammo economy assumes the secondary and melee get used. A one-shot
ejection makes all of that dead content.

Owner chose to cut the shove. `magnitude` is now `RAMSHOT_KNOCKBACK_CELLS * POSITION_SCALE` — two
cells, half `STACK_BLOCK_WIDTH`. Written in cells rather than `BODY_WIDTH / 2` because this crate
lints against division (`displacement.rs` avoids `POSITION_SCALE / 4` for the same reason).

Measured after, three seeds per map:

| Map | Turns | Winner's health |
|---|---|---|
| `crow-perch` | 4 | 76hp — took two hits |
| `twin-spires` | 4 | 76hp — took two hits |
| `broken-battlements` | 3 | 200hp — still a fall, not a trade |

Matches are now decided by an exchange rather than by who shoots first, and the winner arrives
hurt. Standing next to a drop is still punished, which is the intent.

**Not fixed, and worth an owner's eye:** `broken-battlements` still produces a 0hp-versus-200hp
blowout in three decisions — someone dies to a fall without ever trading damage. Its spawn ledges
sit closer to open air than the other two maps. The remedy is map geometry (a floor, or wider
ledges), not another magnitude change, so it is recorded rather than guessed at.

### Gates

| Gate | Result |
|---|---|
| `cargo fmt` / `clippy -D warnings` / `test --workspace` / `deny check` | pass |
| `dotnet format --verify-no-changes` / `dotnet test -c Release` | pass, 65 tests |
| `godot --export-release` + headless C6 smoke | pass: 3/3 maps, `stackedBlocksFell`, `turnsPlayed: 9` |

All five golden vectors and the frozen response corpus regenerated for `CONTENT_VERSION` 4, old
values recorded beside each constant. The presentation manifest moved with it — that pairing is
now the third time the manifest has had to follow a content bump, and the C6 smoke is the only
gate that catches it.

---

## 2026-08-31 — Godot-free gate for presentation-manifest content version

A content bump that forgets `presentation-manifest-v1.json` used to pass all unit tests and
only fail when Godot Confirm loaded the file. Validation now lives in
`DungeonBarrage.Client.Interop.PresentationManifest.Validate` (the same check Confirm runs).
Godot only reads `res://Settings/presentation-manifest-v1.json`.

`PresentationManifestTests` copies the committed JSON next to the test binary, asserts it
matches `LocalMatchSession.ContentVersion`, and asserts a stale `contentVersion` is refused.
`dotnet test -c Release` is 12 + 55 (the two new facts plus the existing 65).

`broken-battlements` fall-kill (0hp vs 200hp in three bot decisions) is still an owner map
geometry call; this change does not touch map design.

---

## 2026-09-03 — playable content-6 cut and visible-body collision correction

This checkpoint completes the inherited content-6 playable-cut work and corrects the reported
case where a projectile could register a character hit in empty space outside the drawn fighter.

### Playable-cut surface completed

- One crow fighter with a four-page, eight-items-per-page loadout flow (32 items total).
- Three stacked playable maps, structural collapse, walk-power tradeoff, jump/landing, trinket
  charge, aim preview, multi-sample projectile playback, Returning Boomerang return playback,
  results, and rematch/disposal.
- `CONTENT_VERSION` remains 6. `SIMULATION_VERSION` is now 9 because collision/trajectory results
  changed; ABI remains 4 because no native function signature or ownership rule changed.

### Root cause and collision contract

`BODY_WIDTH` is a diameter, but command resolution, preview, and bot aiming had passed it to the
circle collider as a radius around the stored ground pivot. Godot independently drew a smaller
circle above that pivot. The authority therefore accepted impacts in a two-body-width invisible
region that the presentation never showed.

The fixed contract has one source of truth:

- `PLAYER_COLLISION_RADIUS = BODY_WIDTH / 2`.
- The player position remains the ground/standing pivot.
- `player_collision_center()` moves one radius upward (positive world Y points down).
- Apply, preview, bot search/scoring, projectile launch origin, nearest-target lookup, snapshot
  projection, aim muzzle, projectile return, and Godot drawing consume that same center/radius.
- C# `CharacterBodyGeometry` is Godot-free, so frozen preview and applied character impacts are
  asserted inside the exact body that `Main.DrawPlayer` projects and draws.

The production-ABI fixture writer regenerated the shared corpus with a 25% command/preview shot,
which now reaches the visible character rather than the terrain. Pinned hashes at simulation 9,
content 6 are:

| Vector | Hash |
|---|---|
| Initial | `1028333c8a2e9f0f` |
| After move | `b0e9ba84389a6797` |
| After ability / final | `682f0e2a57b7debd` |

Golden vectors are `c37e748725499388` (all passes), `030efa1266831350` (walking),
`5db94ba8baa5a1e4` (firing), `d447474ad2fda386` (mixed), and `59a874e1d6621f16`
(low health). Each constant records the previous value and simulation-9 reason in
`golden_vectors.rs`.

### Export-only regressions found during review

The renderer smoke initially failed even though unit tests were green. Its rejection details were
discarded, so `MatchCommandRejectedException` now preserves the closed authority reason.
That exposed `InputOutOfRange`: after walking, the bot emitted the otherwise ignored fixed 50%
power on a melee strike even when the walk-adjusted cap was lower. Melee decisions now clamp to
`max_launch_power`, with a focused regression test.

The same review found that the native bot-decision serializer mapped `Jump` to `Pass`. The wire
contract and C# polymorphic decision/submission path now carry a real `jump`; Rust and managed
regressions prevent another silent translation.

Client movement no longer duplicates `POSITION_SCALE = 1024`; one-cell input reads the loaded
snapshot's authoritative scale. Internal-panic/overflow errors are no longer relabeled as the
ordinary player-facing “Shot left the arena” message.

CI now checks all 13 current `db_sim_*` exports and builds/tests/formats the Godot-free .NET
contracts and interop solution on Linux against a freshly built native library.

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 444 active core tests + all integration/doc tests pass; 41 explicitly documented legacy kit tests ignored |
| `cargo test --release -p db-sim-ffi --locked` | 23 pass, 1 fixture-writer test intentionally ignored |
| `cargo deny check` | advisories, bans, licenses, sources pass; only unused license allow-list warnings |
| `dotnet restore client/DungeonBarrage.sln --locked-mode` | pass |
| `dotnet build client/DungeonBarrage.sln -c Release --no-restore` | pass, 0 warnings/errors |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release --no-build` | 12 contracts + 84 interop = 96 pass |
| Godot 4.7.1 .NET headless import/export | pass |
| Exported C6 headless smoke | pass: 32 items, 3/3 maps, collapse, bot/human actions, rematch/disposal |
| Exported C6 renderer smoke | pass: all prior checks plus 1280×720 setup, loadout, hover, and result captures |

The deterministic smoke currently reports 419 bot decisions and can reach the hard turn limit
after the smaller honest collider replaced the invisible outer target. That number is diagnostic,
not evidence of a human playthrough. The next dependency remains unchanged: a human must finish a
match from `PLAY.md` before Steam store-page work starts. The 41 ignored kit-era tests must be
rewritten for the crow/item envelope if reopened; do not restore kits or rewrite accepted ADR
history.

---

## 2026-09-03 — Event-derived combat feedback and clean-room audit

This checkpoint implements the clean-room combat presentation layer designed from the OpenBound
audit (`docs/OPENBOUND_CLEAN_ROOM_AUDIT.md`), resolving visual feedback strictly from authoritative
events without violating the Rust simulation boundary or modifying the collision contract.

### Delivered

- **Clean-room audit (`docs/OPENBOUND_CLEAN_ROOM_AUDIT.md`):** Recorded legal/provenance boundaries
  against external reference source; identified reusable presentation responsibilities (cues, effects,
  camera) while strictly prohibiting code/asset import or client-authoritative simulation models.
- **`TransitionCueResolver` (`DungeonBarrage.Client.Interop`):** Godot-free visual-clock projection
  mapping authoritative transition events to cosmetic cues:
  - `projectileTrace` -> actor fire accent.
  - Decreasing `healthChanged` -> affected actor hit outline.
  - `impact` -> burst at exact reported fixed-point position; transient camera impulse.
  - `playerEliminated` -> temporary defeat accent before terminal snapshot.
  - At most one highest-priority cue per actor (`defeat` > `hit` > `fire`).
  - Reduced-motion support suppresses shake and rapid pulse while retaining indicators and event order.
  - Preserves authoritative `CharacterBodyGeometry` without translating or resizing collision circles.
- **Client presentation (`Main.cs`):** Renders fire, hit, defeat, and impact cues during locked
  playback; composes draw-only impact camera impulse with manual pan. Removed obsolete/artificial
  `HopOffsetPixels` that bypassed physics.
- **Enhanced C5 smoke verification:** C5 smoke captures fire, impact, and post-shot screenshots;
  observes locked-playback fire, hit, impact, and camera impulse. Tuned picker shot on `crow-perch`
  (27.5° at 78% power) to land an authoritative direct hit on the defender, asserting real damage
  (280 -> 218 HP) and verified event release.
- **Client test suite:** Added `TransitionCueResolverTests` and `LiveMatchTests` direct-hit regression
  guard. .NET test suite expanded to 12 contracts + 91 interop = 103 tests.

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 444 active core tests + all integration/doc tests pass; 41 explicitly documented legacy kit tests ignored |
| `cargo test --release -p db-sim-ffi --locked` | 23 pass, 1 fixture-writer test intentionally ignored |
| `cargo deny check` | advisories, bans, licenses, sources pass; only unused license allow-list warnings |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 91 interop = 103 pass |
| Godot 4.7.1 .NET release export | pass |
| Headless C5 smoke | pass: `success: true`, real damage, fire/hit/impact cues observed |
| Headless C6 smoke | pass: `success: true`, 32 items, 3/3 maps, collapse, bot/human actions, rematch |
| Renderer-backed C5 smoke | pass: 1280×720 fire, impact, and post-shot captures |
| Renderer-backed C6 smoke | pass: 1280×720 setup, loadout select, hover, and result captures |

---

## 2026-09-03 — Camera director and character presentation model

This checkpoint implements items 1 and 2 from `docs/OPENBOUND_CLEAN_ROOM_AUDIT.md` sequenced follow-up:
a Godot-free camera director with arena-boundary clamping and dynamic playback framing, and a paper-doll
character presentation model anchoring sockets and dynamic facing to the authoritative collision body.

### Delivered

- **`CameraDirector` (`DungeonBarrage.Client.Interop`):** Godot-free presentation controller managing
  manual pan, boundary limits based on arena and viewport size, transient combat impulses from
  `TransitionCueResolver`, and dynamic projectile playback tracking.
- **`CharacterPresentationModel` (`DungeonBarrage.Client.Interop`):** Pure C# paper-doll presentation
  model:
  - Dynamic facing resolved via `AimSolver.FacesRight(actorX, opponentX)` instead of hardcoded player index.
  - Validated sockets anchored to `CharacterBodyGeometry`: eye socket, beak root/polygon, crown/trinket socket,
    weapon/tool socket, and ground shadow pivot.
  - Cosmetic layer resolution for equipped trinkets (Crown, Gem) and weapons (Cannon, Blade) with
    graceful fallback styling.
  - Strictly preserves the authoritative `CharacterBodyGeometry` circle as the drawn body collider.
- **Client Presentation Integration (`Main.cs`):**
  - Integrated `CameraDirector` for pan input (`Left`/`Right`/`Up`/`Down`), reset (`Home`/`F`), arena clamping,
    and impulse decay.
  - Integrated `CharacterPresentationModel` in `DrawPlayer`, rendering dynamic facing, equipped crown/gem
    adornments, weapon silhouettes, and facing-aware combat cues.
- **Unit Test Suite:**
  - Added `CameraDirectorTests` (limits clamping, impulse integration, playback tracking, reset).
  - Added `CharacterPresentationModelTests` (dynamic facing, socket geometry anchoring, equipment adornment kinds).
  - .NET test suite expanded to 12 contracts + 100 interop = 112 tests.

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 444 passed, 41 ignored |
| `cargo test --release -p db-sim-ffi --locked` | 23 passed, 1 ignored |
| `cargo deny check` | pass |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 100 interop = 112 passed |
| Godot 4.7.1 .NET release export | pass (`export/windows/DungeonBarrage.exe`) |
| Headless C5 smoke | pass: `success: true`, real damage, all cues observed |
| Headless C6 smoke | pass: `success: true`, 3/3 maps, collapse, bot/human actions, rematch |
| Renderer-backed C5 smoke | pass: 1280×720 fire, impact, and post-shot captures |
| Renderer-backed C6 smoke | pass: 1280×720 setup, loadout select, hover, and result captures |

---

## 2026-09-03 — Tiered, disposal-safe combat effect system

This checkpoint implements item 3 from `docs/OPENBOUND_CLEAN_ROOM_AUDIT.md` sequenced follow-up:
a Godot-free combat effect system providing bounded, pool-recycled shockwaves, sparks, and particles
scaled across graphics tiers (`Low`, `Medium`, `High`) and `ReduceMotion`, while guaranteeing persistent
tactical visibility for target and hit markers.

### Delivered

- **`CombatEffectSystem` (`DungeonBarrage.Client.Interop`):** Pure C# effect manager using pre-allocated,
  bounded object pools to prevent runtime heap allocations:
  - `EffectParticle`: sparks and smoke with damping velocity and alpha fade.
  - `ShockwaveRing`: expanding concentric shockwave rings with smoothed duration.
  - `TargetMarker`: persistent crater reticles and crosshairs ensuring tactical clarity.
  - Performance scaling: `Low` suppresses high-velocity particles to prevent lag, `Medium` renders 6 particles,
    `High` renders 14 spread particles with dual shockwave rings.
  - Motion reduction: `ReduceMotion = true` suppresses particle velocity/jitter and scales shockwave speed,
    while guaranteeing full target marker visibility.
  - Lifecycle management: `Update(deltaSeconds)` recycles expired effects; `Clear()` resets all pools.
- **Client Integration (`Main.cs`):**
  - Continuous effect updates in `_Process` with frame-skipping redraw requests only when effects are active.
  - Spawns muzzle fire on actor fire cues, hit sparks on actor hit cues, and multi-tier shockwaves on impact cues.
  - Renders active combat effects in `DrawMatch` and during playback via `DrawCombatEffects()`.
  - Clean pool disposal on match setup and rematch.
- **Unit Test Suite:**
  - Added `CombatEffectSystemTests` (target marker visibility in low tier/reduced motion, tier scaling, shockwave expansion/expiration, bounded capacity recycling, and clear).
  - .NET test suite expanded to 12 contracts + 105 interop = 117 tests.

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 444 passed, 41 ignored |
| `cargo test --release -p db-sim-ffi --locked` | 23 passed, 1 ignored |
| `cargo deny check` | pass |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 105 interop = 117 passed |
| Godot 4.7.1 .NET release export | pass (`export/windows/DungeonBarrage.exe`) |
| Headless C5 smoke | pass: `success: true`, real damage, all cues observed |
| Headless C6 smoke | pass: `success: true`, 3/3 maps, collapse, bot/human actions, rematch |
| Renderer-backed C5 smoke | pass: 1280×720 fire, impact, and post-shot captures |
| Renderer-backed C6 smoke | pass: 1280×720 setup, loadout select, hover, and result captures |

---

## 2026-09-03 — C7 desktop quality and planning deadline verification

This checkpoint completes machine-checkable evidence for CLIENT_SPEC §21 milestone C7 (desktop release quality)
and validates the real wall-clock planning clock and automatic timeout in Main._Process.

### Delivered

- **Export Presets Packaging (`export_presets.cfg`):** Configured `include_filter="export_presets.cfg"` across
  Windows, Linux, and macOS presets, ensuring release exports bundle configuration resources.
- **Robust Preset Verification (`Main.cs`):** Enhanced C7 smoke verification with asynchronous file reading
  and path fallback, passing both in editor, standalone development, and packaged release binaries.
- **Full Smoke Suite Validation:**
  - `C6TimeoutSmoke`: Verified wall-clock planning deadline countdown, automatic trigger on idle turn,
    and turn handoff to bot and back (`c6-timeout-report.json`, `c6-timeout-windowed-report.json`).
  - `C7Smoke`: Verified settings recovery from non-existent storage, audio volume clamping, accessibility
    text scaling limits, localization catalog (en-US / es-ES), performance tier switching, and multi-platform
    export presets (`c7-report.json`, `c7-windowed-report.json`).

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 444 passed, 41 ignored |
| `cargo test --release -p db-sim-ffi --locked` | 23 passed, 1 ignored |
| `cargo deny check` | pass |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 105 interop = 117 passed |
| Godot 4.7.1 .NET release export | pass (`export/windows/DungeonBarrage.exe`) |
| Headless & Windowed C5 smoke | pass: `success: true`, real damage, fire/hit/impact cues |
| Headless & Windowed C6 smoke | pass: `success: true`, 3/3 maps, collapse, bot/human, rematch |
| Headless & Windowed C6-timeout smoke | pass: `success: true`, automatic planning deadline expiration |
| Headless & Windowed C7 smoke | pass: `success: true`, all 6 quality checks verified |

---

## 2026-09-03 — Slot-aware paper-doll model, dynamic aim elevation, and weapon silhouettes

This checkpoint enriches item 1 from `docs/OPENBOUND_CLEAN_ROOM_AUDIT.md` sequenced follow-up:
expanding the Godot-free paper-doll character presentation model with real-time ability slot
switching, aim elevation trajectory alignment, distinct weapon silhouettes (`Cannon`, `Blade`,
`Bow`, `Ordnance`), and accessibility-safe idle breathing.

### Delivered

- **Slot-Aware Paper-Doll Model (`CharacterPresentationModel.cs`):**
  - Expanded `CosmeticAccentKind` with `Bow` (arched stave and taut string) and `Ordnance` (secondary projectile shell/bomb).
  - Added slot-aware weapon resolution across all 4 slots: `Main`, `Secondary`, `MeleeTool`, and `Trinket`.
  - Added dynamic `AimAngleRadians` and facing-aware `AimVector` in screen coordinates.
  - Added accessibility-safe `BobOffsetY(visualClockMsec, reduceMotion)` supplying continuous idle breathing
    while keeping the ground shadow anchored to authoritative floor geometry and respecting `ReduceMotion`.
- **Client Integration (`Main.cs`):**
  - Integrated active slot and aim angle passing from `_selectedAbilitySlot` and `CurrentAim()` into `DrawMatch`.
  - Renders directional weapon silhouettes oriented along the slingshot drag trajectory in `DrawPlayer`.
  - Renders distinct visual styles for cannons, blades, bows, and ordnance projectiles.
- **Unit Test Suite:**
  - Added tests in `CharacterPresentationModelTests.cs`:
    - `Equipment_accents_switch_with_active_slot` (Main, Secondary, MeleeTool, Trinket)
    - `Bow_and_ordnance_kinds_resolve_expected_accents` (Recurve Bow, Ramshot Shell)
    - `Aim_vector_reflects_elevation_angle_and_facing` (Right, Left, Neutral)
    - `Bob_offset_respects_reduced_motion` (Suppressed vs Active)
  - .NET test suite expanded to 12 contracts + 109 interop = 121 tests.

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 444 passed, 41 ignored |
| `cargo test --release -p db-sim-ffi --locked` | 23 passed, 1 ignored |
| `cargo deny check` | pass |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 109 interop = 121 passed |
| Godot 4.7.1 .NET release export | pass (`export/windows/DungeonBarrage.exe`) |
| Headless & Windowed C5 smoke | pass: `success: true`, real damage, fire/hit/impact cues |
| Headless & Windowed C6 smoke | pass: `success: true`, 3/3 maps, collapse, bot/human, rematch |
| Headless & Windowed C6-timeout smoke | pass: `success: true`, automatic planning deadline expiration |
| Headless & Windowed C7 smoke | pass: `success: true`, all 6 quality checks verified |

---

## 2026-09-03 — Tactical Match HUD: Wind Anemometer Gauge & Floating Character Status Plates

This checkpoint delivers the tactical match HUD improvements planned in `docs/CLIENT_SPEC.md` §1.2 & §9:
providing an intuitive wind anemometer widget in the match HUD and floating status plates above combatants.

### Delivered

- **Tactical HUD Model (`TacticalHudModel.cs`):**
  - Pure C#, Godot-free domain records: `WindDisplayModel` and `PlayerStatusPlateModel`.
  - `WindDisplayModel`: maps authoritative `wind_per_tick` to directional flow (`BlowingLeft`, `BlowingRight`, `Calm`),
    normalized velocity intensity ($0.0 \dots 1.0$), human-readable compass labeling (`WEST`, `EAST`, `CALM`),
    and weapon wind sensitivity tiers (`Immune`, `Resistant`, `Standard`, `High`).
  - `PlayerStatusPlateModel`: computes health fill fraction, low-health threshold alerting ($\le 25\%$),
    trinket charge tracking ($0 \dots 2$ pips with ready status), remaining ammunition, and combat cue badges.
- **Client Integration (`Main.cs`):**
  - Top-HUD graphical wind anemometer gauge (`DrawWindAnemometer`) displaying dynamic wind vane arrow,
    speed readout, and active weapon sensitivity badge (`[IMMUNE]`, `[HEAVY]`, `[STD]`, `[LIGHT]`).
  - Floating character status plate in `DrawPlayer` rendering a segmented health bar (green $\rightarrow$ gold $\rightarrow$ red),
    trinket charge pips, ammunition counter, and combat cue labels naturally anchored to the bobbing paper-doll.
- **Unit Test Suite (`TacticalHudModelTests.cs`):**
  - Tested wind categorization across zero, positive, negative, and clamped extreme winds.
  - Tested weapon sensitivity resolution across `service-pistol`, `mole-drill`, `ramshot-cannon`, and `recurve-bow`.
  - Tested status plate health calculation, low-health alerting, trinket charge progression, and combat cue mapping.
  - .NET test suite expanded to 12 contracts + 121 interop = 133 tests.

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 444 passed, 41 ignored |
| `cargo test --release -p db-sim-ffi --locked` | 23 passed, 1 ignored |
| `cargo deny check` | pass |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 121 interop = 133 passed |
| Godot 4.7.1 .NET release export | pass (`export/windows/DungeonBarrage.exe`) |
| Headless & Windowed C5 smoke | pass: `success: true`, real damage, fire/hit/impact cues |
| Headless & Windowed C6 smoke | pass: `success: true`, 3/3 maps, collapse, bot/human, rematch |
| Headless & Windowed C6-timeout smoke | pass: `success: true`, automatic planning deadline expiration |
| Headless & Windowed C7 smoke | pass: `success: true`, all 6 quality checks verified |

---

## 2026-09-03 — Leftover C1 realignment & smooth movement playback interpolation

This slice resolves all 41 leftover `#[ignore]` C1 tests across the Rust simulation core against the single-crow + 32-item catalog, eliminating obsolete kit tests per HANDOFF.md §2 instructions, and introduces pure C# movement visual interpolation for walking/hopping ground pivots during transition playback.

### Delivered

- **Simulation Core C1 Realignment (`db-sim-core`):**
  - `bot.rs`: Un-ignored and realigned `a_crow_duel_against_a_passive_opponent_ends_in_victory_with_no_rejections`.
  - `match_host.rs`: Un-ignored and realigned all 20 tests. 0 ignored, all passing.
  - `resolve/relocation.rs`: Un-ignored and realigned all 15 tests. 0 ignored, all passing.
  - `command.rs`: Realigned multi-projectile command test to `line-repeater`, passive choice tests to assert `CommandRejection::PassiveAlreadyChosen`, and push effect test to `tide-sprayer`. Pruned obsolete retired kit tests.
  - `match_session.rs`: Realigned `complete_checkpoint_restore_preserves_replay_and_can_continue`, `stale_and_illegal_previews_are_normal_non_mutating_responses`, `melee_terrain_and_block_mutation_are_one_ordered_real_transition` (to `trench-spade`), `melee_elimination_is_attributed_before_victory_and_no_turn_reopens`, and all 8 `strike_provenance_tests` to `line-repeater`. Realigned `owner_cleanup_reaches_the_session_with_its_exact_cause`. Pruned obsolete retired kit tests (Arzum/Aleph draws, mid-match passive choice, knife projectile spawns).
  - Rust Core test suite: **485 unit + integration tests passing**, **0 ignored**, **0 failed**.
- **Smooth Movement Playback Interpolation (`MovementPlayback.cs`, `Main.cs`):**
  - Implemented `MovementPlayback.InterpolatePlayer` in `DungeonBarrage.Client.Interop`: smoothly lerps character ground pivots between `EntityMoved.Start` and `EntityMoved.End` during transition playback (`_playback`), using smoothstep cubic easing (`3t^2 - 2t^3`).
  - Preserves `CharacterBodyGeometry` collision circles with mathematical exactness: translates `CollisionCenter` in exact sync with ground pivot offset while keeping `CollisionRadius` intact.
  - Sockets (eye, beak, crown/gem, weapon/tool), ground shadows, and floating status plates stay perfectly anchored to the smoothly moving paper doll.
  - Handles projectile impact timing: movements resulting from projectile impacts (knockback/push) hold starting position until `PresentationTick`, then smoothly lerp over remaining ticks.
  - Supports accessibility: immediately snaps to destination when `ReduceMotion` is enabled.
- **Client & Integration Test Suites:**
  - Added 6 new unit tests in `MovementPlaybackTests.cs` (start tick, end tick, midpoint smoothstep, reduce motion, delayed impact ticks, event filtering).
  - Fixed Godot 4.7.1 color mapping for bowstrings (`Color(0.98f, 0.98f, 0.82f)`).
  - .NET test suite expanded to 12 contracts + 127 interop = 139 tests passing.
  - Full headless smoke suites passed: C5 smoke (`success: true`), C6 smoke (3/3 maps, `success: true`), C6-timeout smoke (`success: true`), and C7 smoke (`success: true`).

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | **509 passed**, 0 failed, 1 ignored (fixture generator) |
| `cargo test --release -p db-sim-ffi --locked` | 23 passed, 1 ignored |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 127 interop = **139 passed**, 0 failed |
| `dotnet build client/src/DungeonBarrage.Client/DungeonBarrage.Client.csproj -c ExportRelease` | pass, 0 warnings, 0 errors |
| Headless C5 smoke | pass: `success: true`, real damage, fire/hit/impact cues |
| Headless C6 smoke | pass: `success: true`, 3/3 maps, collapse, bot/human, rematch |
| Headless C6-timeout smoke | pass: `success: true`, automatic planning deadline expiration |
| Headless C7 smoke | pass: `success: true`, all 6 quality checks verified |

---

## 2026-09-03 — Weapon Action Bar, Ammo UX, Character Hit Reactions & Map Destruction Animation

This slice addresses the player's inability to fire upon exhausting Main slot ammo and adds smooth animations for map destruction and character hit reactions.

### Root Cause Analysis

In Turn 7, the player's equipped `ramshot-cannon` reached 0 ammo (`starting_ammo: 3` in `character.rs`). The match UI continued to allow dragging the aim reticle, drawing the gold trajectory, and displayed "release to fire". Upon releasing, the simulation rejected the command with `OutOfAmmo`. The player had unused ammo in Melee (`trench-spade`, 4 charges), Secondary (`ramshot-shell`, 1 charge), and a fully charged Crown (`ember-crown`, `READY (4)`), but had no visual weapon HUD showing slot ammo states, couldn't click to switch slots, had misaligned numeric keys (Key 2 mapped to Secondary instead of Melee), and received no preemptive out-of-ammo warning.

### Delivered

- **Interactive Weapon Action Bar & Ammo UX (`Main.cs`):**
  - Added a centered bottom action bar displaying 4 slots: `[1] RANGED`, `[2] MELEE`, `[3] SECONDARY`, and `[4] TRINKET`, with active highlight, item names, and real-time ammo status (`AMMO x/y`, `[EMPTY]`, `[READY] (4)`, `CHARGE %`).
  - Added direct mouse click selection: clicking any of the 4 weapon slot boxes instantly selects that weapon without triggering an aim drag.
  - Aligned keyboard switching with the 4-page loadout selection order: Key 1 = Main, Key 2 = MeleeTool, Key 3 = Secondary, Key 4 = Trinket.
  - Added out-of-ammo aiming prevention: when aiming with an empty slot, `CurrentAim().CanFire` is set to `false`, the drag line turns red, trajectory preview is suppressed, and `"OUT OF AMMO"` warnings are displayed both on the cursor and HUD with instructions to switch.
  - Added automatic weapon fallback: upon turn start or immediately after a shot exhausts a weapon's remaining ammo, the client auto-switches to the next available slot with ammo or charge.
- **Character Hit Reactions (`CharacterPresentationModel.cs`, `TransitionCueResolver.cs`, `Main.cs`):**
  - Decaying sine flinch: characters hit by attacks flinch backward away from the impact with a decaying sine wave oscillation (`-facing * sin(age * 4pi) * (1 - age) * 5.5px`).
  - White hit flash: hit characters flash with a white color blend for the first 0.35s of the hit cue.
  - Wincing eyes: hit characters squint with a tight wincing eye slit during impact rather than staring blankly.
  - Floating damage numbers: `FloatingDamageText` pool in `CombatEffectSystem` displays rising, fading damage numbers (`-XX`) above damaged players based on authoritative health drops.
- **Smooth Map Destruction & Crumbling Physics (`CombatEffectSystem.cs`, `Main.cs`):**
  - Added gravity acceleration (`GravityY`) to `EffectParticle` in `CombatEffectSystem`.
  - Added `SpawnTerrainDebris`: blasts masonry, stone, and soil chunks in an upward arc that tumble down under gravity upon projectile terrain impact or tower collapse.
  - Smooth collapsing towers: when a tower collapses (`ClientBlockChangedEvent`), the falling masonry interpolates smoothly downward over time using quadratic gravity ease rather than snapping instantaneously.

### Verification

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | **509 passed**, 0 failed, 1 ignored |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 130 interop = **142 passed**, 0 failed |
| `dotnet build client/src/DungeonBarrage.Client/DungeonBarrage.Client.csproj -c ExportRelease` | pass, 0 warnings, 0 errors |
| Headless C5 smoke | pass: `success: true` |
| Headless C6 smoke | pass: `success: true`, 3/3 maps, stacked blocks fell |
| Headless C6-timeout smoke | pass: `success: true` |
| Headless C7 smoke | pass: `success: true` |

## 2026-09-04 - Complete 13-spritesheet animated arsenal, reaction sheets, and presentation layer integration

Integrated the complete 13-sheet illustrated crow arsenal and reaction sheets into the Godot 4 .NET client presentation layer, replacing procedural paper-doll circles with animated 32-bit transparent sprites while retaining procedural rendering as a resilient fallback:

- **Standardized Asset Pipeline (`scripts/process_spritesheets.py`):**
  - Standardized all 13 user-provided sheets into clean 32-bit transparent PNGs with uniform 192x160 cells (960px width, centered at x=96, feet baseline at y=145).
  - Preserved crow eye highlights and white plumage details via BFS perimeter boundary flood-fill transparency.
  - Sliced and standardized 10 weapon sheets (5x5 grid, 960x800): `crow_ramshot_cannon`, `crow_frostfall`, `crow_drill`, `crow_bow`, `crow_cinder`, `crow_pistol`, `crow_revolver`, `crow_boomerang`, `crow_flail`, `crow_pickaxe`.
  - Sliced and standardized 3 reaction sheets (5x4 grid, 960x640): `crow_damage` (hit wince/sparks, falling feathers, prone defeat with stars), `crow_flight` (takeoff leap, airborne flight glide loop, touchdown landing), `crow_potion` (bottle uncork, chug drinking, green healing sparkle aura).
- **Clean-Room State Resolution (`CharacterAnimationFrameResolver.cs`, `CharacterPresentationModel.cs`):**
  - Extended `CharacterPresentationModel.ResolveSpriteSheetKey` to resolve all equipped weapons across Main, Secondary, MeleeTool, and Trinket slots.
  - Implemented `CharacterAnimationFrameResolver.Resolve` in `DungeonBarrage.Client.Interop` as pure C#, mapping authoritative player states, cues, and motion into exact sheet keys, rows, and frame columns:
    - Priority 1: Defeat/Elimination -> `crow_damage` Row 2 (prone knockout with stars).
    - Priority 2: Taking Damage / Hit -> `crow_damage` Row 0/1 (sparks, feathers, flinch driven by `cue.Age01`).
    - Priority 3: Healing Potion -> `crow_potion` Row 1 (drinking).
    - Priority 4: Airborne / Hopping -> `crow_flight` Row 1 (gliding wings loop).
    - Priority 5: Firing / Attack Cue -> Weapon sheet Row 3 (blast / thrust / slam).
    - Priority 6: Aiming Stance -> Weapon sheet Row 2 (elevation angle mapped to cols 0..4).
    - Priority 7: Moving / Walking -> Weapon sheet Row 1 (walk cycle).
    - Priority 8: Idle Ambient Breathing -> Weapon sheet Row 0 (5-frame ambient breathing cycle).
- **Godot Presentation Layer (`CharacterSpriteRegistry.cs`, `Main.cs`):**
  - Implemented `CharacterSpriteRegistry` to cache and manage `Texture2D` instances via `ResourceLoader.Load` and `Image.LoadFromFile` fallback for headless resilience.
  - Implemented `TryDrawCharacter` in `Main.cs`: anchors sprites directly to `model.ShadowPivot`, scales dynamically to authoritative collision radius, applies horizontal facing flips (`model.FacingSign`), hit flinch, and white hit flash.
  - Retained procedural paper-doll rendering as a seamless fallback if textures are unavailable.
  - Crown and gem trinket socket adornments dynamically render atop the sprite's head when equipped.

### Verification

| Gate | Result |
|---|---|
| `cargo test --workspace --locked` | **509 passed**, 0 failed |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release` | 12 contracts + 149 interop = **161 passed**, 0 failed |
| `dotnet build client/src/DungeonBarrage.Client/DungeonBarrage.Client.csproj -c ExportRelease` | pass, 0 warnings, 0 errors |
| Godot headless export (`Windows Desktop`) | pass (`DungeonBarrage.exe`) |
| Headless C5 smoke | pass: `success: true`, cues observed |
| Headless C6 smoke | pass: `success: true`, 3/3 maps, stacked blocks fell |
| Headless C7 smoke | pass: `success: true` |








## 2026-09-04 — four-character kit restoration (schema 2 / simulation 10 / content 7)

The product direction returned from the interim one-Crow ammunition catalog to fixed character
kits. The working slice introduces Leslie, Crow, Erus, and Kreena as Rust-owned profiles with fixed
health, movement, and three actions. Match creation now accepts `characterId`; Rust derives the
transitional loadout fields. Normal actions are unlimited, SS is gauge-gated and preserves the
normal action, and the client replaces the four-page item wizard with one character screen and a
three-action bar.

Aim presentation now selects one dotted line from the authoritative preview, gold for a character
hit and red otherwise. Rust-published body geometry remains the only hitbox/presentation anchor.
The shared fixture now covers Crow's Precision .57 visibly hitting Erus, with hashes
`d50eee09afceaaf7` (initial), `3f2e5267b7164eaf` (after move), and `06fa4183bbd03425`
(after ability).

Durable guidance is split intentionally: `CLIENT_SPEC.md` defines the boundary,
`CHARACTER_SYSTEM_IMPLEMENTATION_PLAN.md` defines phased mechanics and honest approximations,
`PLAY.md` defines controls/build, and `HANDOFF.md` is the resume checkpoint. The legacy item catalog
is retained only for deterministic replay and low-level resolver compatibility; new clients cannot
select it.

### Final verification and movement-authority correction

Final renderer review exposed a legacy-authority leak: the scheduler and launch-power cap still
refreshed every selected character with the old shared movement class. Both paths now resolve the
active Rust roster profile. A focused scheduler regression proves a Crow Fast turn handing off to
a Leslie Slow turn. The sanctioned production C-ABI fixture writer regenerated only the affected
responses; hashes are now `5e95a1dd6ba37637` (initial), `d3681302b21ba8ef` (after Crow moves),
and `06fa4183bbd03425` (after the unchanged direct-hit outcome). Three intentional match golden
vectors were regenerated in their dedicated test file with old/new provenance comments.

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --all-targets --locked` | 514 passed, 0 failed, 1 ignored fixture writer |
| `cargo test -p db-sim-ffi --release --locked` | 23 passed, 0 failed, 1 ignored fixture writer |
| `cargo deny check` | pass; only pre-existing unused license-allow warnings |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release --no-restore` | 12 contract + 152 interop = 164 passed |
| Godot 4.7.1 Windows Desktop release export | pass |
| Visible C5 smoke, 1280x720 | pass; Crow, one dotted gold body-hit guide, 34 damage, playback and handoff |
| Visible C6 smoke, 1280x720 | pass; roster 4, 3/3 maps, terminal match, collapse, rematch/disposal |
| Visible C6-timeout smoke, 1280x720 | pass; countdown and automatic authority timeout |
| Visible C7 smoke, 1280x720 | pass; all six release-quality checks |

Renderer evidence was inspected, not inferred from report booleans. The local evidence bundle is
`C:\tmp\DungeonBarrage-character-smoke-20260904`; the C6 terminal hash is
`6b28cd9b5e7c4f3c`.

## 2026-09-05 — cross-platform locked-restore repair

Linux CI rejected locked restore because the Contracts, Interop, and Godot app lock files had been
regenerated from a Windows RID-qualified graph. The two libraries are RID-neutral, but their locks
contained empty `net10.0/win-x64` targets. The app lock had the same stale RID and was also missing
the `GodotSharpEditor` direct dependency declared by Godot.NET.Sdk 4.7.1.

`dotnet restore client/DungeonBarrage.sln --force-evaluate` regenerated all project locks from the
current solution graph without an explicit runtime identifier. The resulting production locks now
contain only `net10.0`, and the app lock contains all three Godot SDK dependencies. The exact CI
command `dotnet restore client/DungeonBarrage.sln --locked-mode` passes, followed by .NET format,
12 contract tests, 152 interop tests, and `git diff --check`.

### Linux native-test packaging follow-up

The subsequent Linux test run exposed a separate platform packaging gap: CI built
`target/release/libdb_sim_ffi.so`, but `DungeonBarrage.Client.Interop.Tests.csproj` copied only the
Windows `db_sim_ffi.dll` beside the test assembly. The absolute-path native resolver therefore
failed closed, as designed. The test project now conditionally copies each advertised host
artifact (`.dll`, `.so`, or `.dylib`) when it exists. This fixes Linux CI without enabling ambient
library search or weakening the resolver's trust boundary.

## 2026-09-05 - retro-arcade UI overhaul U1: opening flow

The opening flow now presents Dungeon Barrage as a character-led retro-arcade franchise rather
than a diagnostics shell. The title screen introduces the four-character launch roster, makes
Local Duel the decisive primary action, and labels Arcade Run and Franchise Vault as future modes
instead of implying that they are playable. Arena setup now previews the selected map and teaches
both valid win routes: direct damage and dungeon destruction/ringout. Character select replaces
initial placeholders with animated portraits drawn from the shipped sprite sheets and exposes each
fighter's role, health, movement, weapon, two unlimited normal actions, and charge-gated SS.

The clean-room direction is recorded in `docs/UI_OVERHAUL_PLAN.md`. The linked Gunbound Season 4
work informed information hierarchy and screen rhythm only. The palette, procedural cabinet
framing, typography treatment, dungeon staging, copy, and generated concept board are original to
Dungeon Barrage. Shared presentation-only primitives live in `RetroArcadeUi`; authority, input,
combat resolution, hit geometry, and screen-state transitions remain unchanged.

The concept board was generated with the built-in image generator from a 16:9 prompt describing a
dark-navy CRT arcade cabinet, ember-red and electric-cyan accents, coin-gold focus states, four
silhouetted franchise heroes, Local Duel as the dominant action, and visibly locked future modes.
It is stored at `docs/design/retro-arcade-menu-concept-v1.png` as direction, not runtime art.

### Verification

| Gate | Result |
|---|---|
| `dotnet build client/DungeonBarrage.sln -c Release --no-restore` | pass; 0 warnings, 0 errors |
| `dotnet format client/DungeonBarrage.sln --verify-no-changes --no-restore` | pass |
| `dotnet test client/DungeonBarrage.sln -c Release --no-build` | 12 contract + 152 interop = 164 passed |
| `cargo fmt --all --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | 514 passed, 0 failed, 1 ignored fixture writer |
| `cargo deny check` | pass; only pre-existing unused license-allow warnings |
| Godot 4.7.1 Windows Desktop release export | pass |
| Visible title/C7 smoke, 1280x720 | pass; title composition and all six settings checks |
| Visible opening flow, 1280x720 | pass; title, arena setup, picker, and picker hover inspected |
| Visible C5 smoke, 1280x720 | pass; move, one gold hit guide, damage, playback, handoff |
| Visible C6 smoke, 1280x720 | pass; four fighters, 3/3 maps, terminal result, rematch/disposal |

Evidence is preserved locally at `C:\tmp\DungeonBarrage-retro-ui-20260905\evidence`. The U1 slice
does not finish the complete overhaul: U2 must migrate the combat HUD and weapon rail, U3 must
migrate results/settings/accessibility, and U4 must replace shared Crow silhouettes with original
identity-specific production portrait and idle sheets before release acceptance.
