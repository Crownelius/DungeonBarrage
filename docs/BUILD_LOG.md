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
