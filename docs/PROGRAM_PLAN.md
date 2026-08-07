# Dungeon Barrage program plan

**Status:** Living plan. Updated at each milestone gate.
**Related:** [PRODUCT_SPEC.md](./PRODUCT_SPEC.md) · [CHARACTERS.md](./CHARACTERS.md) · [PLATFORM_STRATEGY.md](./PLATFORM_STRATEGY.md) · [adr/0001-rust-wasm-core.md](./adr/0001-rust-wasm-core.md) · [adr/0002-character-kits.md](./adr/0002-character-kits.md) · [adr/0003-shared-trig-table.md](./adr/0003-shared-trig-table.md) · [MODULE_OWNERSHIP.md](./MODULE_OWNERSHIP.md)

## 1. Where the project actually is

Honest status, so nothing is described as further along than it is.

| Area | State |
|---|---|
| Design documentation | Substantial and specific |
| TypeScript simulation | Working, tested, fixed-point, deterministic — now the reference oracle |
| React/canvas vertical slice | Renders and plays locally |
| Rust core foundation | `fixed`, `canonical`, `types`, `error` complete and tested |
| Rust engine modules | Compile and test clean, but **behaviorally inert** — see §2 |
| Effect resolvers | **3 of 22 implemented.** 8 of 9 characters do not function |
| Turn scheduler, maps, spawns, status ticking, event log, reconnect | **Absent** |
| TS↔Rust parity harness | **Not built, and blocked on ADR 0003.** Top technical risk |
| WASM client integration | Not started |
| Match server | Not started |
| Persistence | Schema defined; D1 binding still `null` |
| Progression / economy | Specified; not implemented |
| Real-time PvP mode | Structurally provisioned only; deliberately not implemented |
| Accounts, matchmaking, store | Not started |
| Git remote | Configured (`Crownelius/DungeonBarrage`, private). **Repo not yet created** — token lacks Administration permission |

## 2. Engine scope — corrected 2026-08-06

This section exists because the previous version of this plan encoded an assumption that
turned out to be wrong, and the wrong assumption produced a real gap. It is recorded rather
than quietly fixed.

### What was assumed

That characters would be **mostly data**: a closed vocabulary of reviewed effect
identifiers (`EffectKind`) composed by per-character definitions, so adding a character
would mean adding a table row rather than engine code.

### What is actually true

The product owner worked on Gunbound, where **the engine alone was ~20,000 lines** and
**each character release modified ~1,000 lines of engine code**. Characters are not pure
data. Each one lands real engine work.

### The gap this assumption caused

`EffectKind` has 22 variants. **Three have resolvers** — `Heal`, `HealthTransfer`,
`SelfDamage`. The other nineteen are declared in the type system, referenced by character
definitions, and enforced by roster validation, and then **nothing acts on them**.

| Character | Signature mechanic | Functions? |
|---|---|---|
| Arzum | Teleport chain-strike | ✗ `Teleport` inert |
| Emi | Cube turret | ✗ `SpawnTurret` inert |
| Karl | Three strikes per turn | ✗ `MultiStrike` inert |
| Huck | Body throw | ✗ `Relocate` inert |
| Numa | Harpoon pull, Pin | ✗ `Pull`, `Lockdown` inert |
| Aleph | Dagger chain | ✗ `EmbedProjectile`, `ChainDetonate` inert |
| Zeke | Heal / Lifeshare | ✓ works |
| Roberto | Knockback grenade | ✗ `Knockback` inert |
| Natomica | Repulse, wall impact | ✗ `Push`, `WallImpact` inert |

**One of nine starters functions end to end.** The roster is data-complete and almost
entirely non-functional. Also absent outright: turn scheduler, map and spawn system, status
ticking, persistent-object lifecycle, event log / replay, reconnect snapshot. Movement and
victory-condition fields exist and are hashed, but nothing implements the behavior.

The root cause was the module brief, not the implementation: `command.rs` was specified as
"validation + application" and delivered exactly that — a correct security boundary that
validates a command and then resolves almost none of what the command implies.

### Corrected sizing

Engine code currently stands at **5,019 lines** excluding tests (8,495 including them; the
earlier 8,495 figure conflated the two).

```
shared engine still missing      ~9,000   scheduler, movement, status and object
                                          lifecycle, maps, spawns, victory, event
                                          log, reconnect, protocol codec, bot AI,
                                          parity harness
effect resolver layer            ~2,500   the 19 inert kinds
24 characters × ~1,000          ~24,000
                                --------
                                 ~35,000
```

~20,000 of that is engine, matching the owner's figure. **The project is at roughly a
quarter of the engine, not a complete one.**

### Why the resolver layer must come before more characters

The ~1,000-lines-per-character figure only holds if a shared resolver layer exists to
amortize against. Without it, character #10 re-implements displacement for the tenth time
and the real cost is nearer 3,000 each. Building the resolver layer first is what makes the
owner's own number achievable.

This does not abandon the closed-vocabulary principle — the vocabulary stays closed for
security reasons (`SECURITY_BASELINE.md` §6: no scripting surface, no downloadable
behavior). What changes is the expectation that the vocabulary is *finished*. It grows with
the roster, by build, under review.

## 3. Team model

The product owner specified a seven-agent team. Mapped to what each tier is actually good at:

| Role | Model | Owns |
|---|---|---|
| Architect / integrator | Opus | Architecture, ADRs, backend, security, all git, all integration, final review |
| Lead / corrector | Sonnet | Reviews and fixes every Haiku module before it lands |
| Feature engineers ×2 | Sonnet | Parity-critical and security-critical modules |
| Implementers ×3 | Haiku | Mechanical, tightly-specified modules and data transcription |

**How the tiers are actually divided.** Haiku gets work where the specification is
complete enough that the answer is determined — data transcription, a well-known
algorithm, a mask operation with stated geometry. Sonnet gets work requiring judgement
under ambiguity — the canonical encoding, ballistic parity, the command security
boundary. Opus keeps anything where a wrong decision is expensive to reverse: interface
contracts, security posture, and what gets committed.

**The correction pipeline is not a formality.** Each Haiku module flows immediately into a
Sonnet review that reads the code, verifies every self-flagged judgement call
independently, and fixes defects directly. Review starts the moment a module lands rather
than waiting for the slowest one.

**Conflict prevention is structural, not conventional** ([MODULE_OWNERSHIP.md](./MODULE_OWNERSHIP.md)):
one file per owner, `types.rs` frozen as the shared contract, `lib.rs` owned centrally,
and an explicit instruction that a broken sibling module is not yours to fix. Agents are
told to report blockers rather than invent a local type — a wrong guess that compiles
costs more than a question.

## 4. Milestones and gates

Each gate is evidence, not a calendar date. A gate that has not produced its evidence has
not been passed, and the next milestone does not start.

### M0 — Foundation ✅ *complete*
Rust workspace, fixed-point math, canonical encoding, shared type contract, CI gates,
supply-chain policy, progression and security specifications.
**Gate:** clippy clean under `-D warnings`, wasm32 builds, gates verified by running them.

### M1 — Core port + parity ⬅ *current*
Foundation modules exist and are test-clean: `rng`, `terrain`, `character`, `hash`,
`ballistics`, `command` — 207 tests, clippy clean under `-D warnings`, wasm32 builds. They
are **not** a working engine; see §2.

Remaining, and both are oracle-side work:
1. Update `lib/game/simulation.ts` to the canonical byte encoding (ADR 0001 §5).
2. Update it to the shared quantized sine table (**ADR 0003**). The oracle's
   `Math.sin`/`Math.cos` cannot be reproduced in fixed point *and* are not bit-identical
   across JS engines — a latent determinism defect the port exposed rather than created.
3. Build the differential harness over a golden corpus.

**Gate:** thousands of seeded command sequences produce identical final state hashes in
both implementations. Until this passes, the port is unverified and nothing depends on it.

### M1.5 — Effect resolver layer 🔴 *the actual blocker*

The nineteen inert `EffectKind` variants, plus the four subsystems they cannot work
without. This is the milestone that turns a validated command into a game.

| Work | Est. lines |
|---|---:|
| Displacement family — `Knockback`, `Push`, `Pull`, `Recoil`, `WallImpact` | ~600 |
| Relocation family — `Teleport`, `Relocate`, `Obscure` | ~450 |
| Persistent objects — `SpawnTurret`, `EmbedProjectile`, `ChainDetonate`, lifecycle | ~700 |
| Status family — `Chill`, `Lockdown`, `Embers`, ticking and expiry | ~450 |
| Attack modifiers — `MultiStrike`, `GuaranteeCrit`, `Cluster`, `Return`, `Tunnel` | ~500 |
| Turn scheduler and match lifecycle state machine | ~800 |
| Movement and locomotion — walk, jump, slope, fall | ~700 |
| Victory, elimination, sudden death | ~300 |
| Map definition, loading, spawn placement and validation | ~600 |

**Gate:** all nine starters function end to end — every mechanic in `CHARACTERS.md` §3
observably resolves in a match, with a test per mechanic asserting the specific state
change. Not "compiles and validates": *does the thing the character sheet says it does.*

A useful secondary signal: the tenth character should cost close to 1,000 engine lines. If
it costs 3,000, the resolver layer is not yet doing its job and the roster should not
proceed.

### M2 — WASM client integration
`wasm-bindgen` boundary, the React client driving the Rust core, TS simulation retired
from the runtime path (retained in `reference/`).
**Gate:** the vertical slice plays identically on the Rust core; WASM payload ≤400 KB
compressed; no regression against the performance budgets in `PLATFORM_STRATEGY.md` §15.

### M3 — Vertical slice complete
Nine starter characters playable, three maps, layered cosmetic compositing, complete
match HUD with the special gauge and passive-choice prompt, training bot, rematch,
event replay.
**Gate:** a first-time player picks a character, plays, understands the result, and
rematches without explanation.

### M4 — Authoritative match server
Node/Colyseus rooms running the *natively compiled same core*, command validation,
reconnect snapshots, private room codes, guest sessions.
**Gate:** duplicate, late, reordered, malformed, and cross-player commands cannot alter
state; disconnect/reconnect recovers during both planning and resolution.

### M5 — Progression, economy, accounts
XP curve, level 0–55, the level-up choice, credit ledger, character shop, idempotent
grants, optional account conversion.
**Gate:** duplicate completion messages cannot grant twice — proven by test, not asserted;
economy ledger reconciles against cached balances; a client cannot assert its own level,
currency, or ownership.

### M6 — Web MVP
2–4 player rooms, free-for-all and 2v2, matchmaking, mute/report, telemetry, PWA install,
load testing.
**Gate:** representative 100/500/1000-CCU load meets latency and event-loop targets;
supported desktop browsers complete matches; backup restore rehearsed.

### M7 — Retention, then distribution
Only after retention data exists: more content, public matchmaking, balance seasons. Then
the Chrome and Steam decision gates in `PLATFORM_STRATEGY.md` §10–§11.

### M8 — Real-time PvP mode
The Brawlhalla-like second mode. Implements `MatchScheduler` against the *already proven*
shared terrain, collision, damage, and knockback. Deliberately last: building it before
the turn-based loop is proven means debugging two schedulers against an unvalidated core.

## 5. Standing practices

- **Commit every meaningful change**, with a message stating *why*, not just what.
- **Every gate runs locally before it is committed.** An untested gate is decoration —
  two bugs in the CI file itself were caught this way on the first day.
- **Findings are reported, not silently fixed.** The 50× imbalance in the level-up reward
  choice (`PROGRESSION.md` §4) is implemented as specified *and* flagged with tuning
  options. Scaling the design down is the product owner's call.
- **No control is weakened to make progress.** It is changed by an ADR, in the open, with
  the risk stated — or not at all.
- **A module brief is a scope decision.** The inert-resolver gap (§2) came from briefing
  `command.rs` as "validation + application" — it delivered exactly that, correctly, and
  the missing behavior was never anyone's assignment. When delegating, state what must
  *work*, not what must be *written*.
- **"Compiles and passes tests" is not "functions."** 207 green tests coexisted with 8 of 9
  characters doing nothing. Gates must assert observable behavior, not module health.

## 6. Open items for the product owner

1. **Create the GitHub repo.** The supplied fine-grained PAT authenticates and can push,
   but cannot *create* repositories (403 — it lacks Administration: write). Create
   `DungeonBarrage` as **private** at <https://github.com/new>, then `git push -u origin main`
   works immediately; the remote and credential helper are already configured.
   **Rotate that token** — it was transmitted as a plaintext file and through a chat
   context, so it must be considered compromised. Scope the replacement to this one
   repository rather than all 28.
2. **The reference screenshot** described in `PRODUCT_SPEC.md` §12 was supplied in an
   earlier session and is not present in the current one. Art-direction work that depends
   on it needs it re-attached.
3. **Level-up reward balance** — see `PROGRESSION.md` §4. Recommended: raise the credit
   option to 250–400. One versioned data change, no new systems.
4. **SOC 2** is an audited organizational attestation, not a code property
   (`SECURITY_BASELINE.md` §1). Engineering can build so an audit is achievable; the
   policy set, risk assessment, vendor management, training, access reviews, and the CPA
   engagement are owner responsibilities.
5. **Ranked arsenal normalization** (`PROGRESSION.md` §5). Progression gates weapons;
   `PRODUCT_SPEC.md` §8 promises rated modes expose the full arsenal to everyone. The
   proposed boundary keeps both, but it is a product decision worth confirming explicitly.
6. **Dependency advisories.** `npm audit` is down from 18 to 14; the remainder need
   upgrades outside the pinned ranges of `vinext`, `vite`, and `@cloudflare/vite-plugin`.
   Nearly all are dev/build-chain rather than shipped code, but they run on the developer
   machine. Worth a deliberate upgrade pass once the vinext version is stable.

7. **Character content backlog.** 15 of the 24 characters are unspecified, and 45 of the
   72 passives are undrafted (`CHARACTERS.md` §4, §7). Both are real scheduling
   commitments.
8. **Four character rules need confirmation** (`CHARACTERS.md` §7): Karl's 24%/74% vs the
   brief's 33%; Numa's harpoon direction threshold; Zeke's 22 HP heal reading; and whether
   Arzum's 50–200% ultimate roll should narrow in rated play. Karl's crit *chance* is
   additionally an unsourced 20% placeholder flagged during review.

## 7. Note for external review

This plan is intended to be reviewable by someone — or some model — without access to the
conversation that produced it. Two things are worth flagging to a reviewer:

1. **§2 is a correction, not a status report.** The engine was described as "complete" in an
   earlier revision. It was not, and the specific way it was wrong (declared-but-unresolved
   effects) is the kind of gap that compiles, tests green, and still ships a non-functional
   game. The sizing in §2 supersedes any earlier estimate in this repository.
2. **The sizing rests on the product owner's direct experience** working on Gunbound
   (~20k engine lines, ~1k per character release), not on a bottom-up estimate. A
   reviewer disagreeing with it should say which of the two figures they think is wrong and
   why, since everything downstream in §4 is derived from them.
