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
| Rust behavior modules | Complete — 207 tests, clippy clean, wasm32 builds |
| TS↔Rust parity harness | **Not built, and blocked on ADR 0003.** Top technical risk |
| WASM client integration | Not started |
| Match server | Not started |
| Persistence | Schema defined; D1 binding still `null` |
| Progression / economy | Specified; not implemented |
| Real-time PvP mode | Structurally provisioned only; deliberately not implemented |
| Accounts, matchmaking, store | Not started |
| Git remote | **None configured** — commits are local only |

## 2. Team model

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

## 3. Milestones and gates

Each gate is evidence, not a calendar date. A gate that has not produced its evidence has
not been passed, and the next milestone does not start.

### M0 — Foundation ✅ *complete*
Rust workspace, fixed-point math, canonical encoding, shared type contract, CI gates,
supply-chain policy, progression and security specifications.
**Gate:** clippy clean under `-D warnings`, wasm32 builds, gates verified by running them.

### M1 — Core port + parity ⬅ *current*
Behavior modules **complete**: `rng`, `terrain`, `character`, `hash`, `ballistics`,
`command` — 207 tests, clippy clean under `-D warnings`, wasm32 builds.

Remaining, and both are oracle-side work:
1. Update `lib/game/simulation.ts` to the canonical byte encoding (ADR 0001 §5).
2. Update it to the shared quantized sine table (**ADR 0003**). The oracle's
   `Math.sin`/`Math.cos` cannot be reproduced in fixed point *and* are not bit-identical
   across JS engines — a latent determinism defect the port exposed rather than created.
3. Build the differential harness over a golden corpus.

**Gate:** thousands of seeded command sequences produce identical final state hashes in
both implementations. Until this passes, the port is unverified and nothing depends on it.

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

## 4. Standing practices

- **Commit every meaningful change**, with a message stating *why*, not just what.
- **Every gate runs locally before it is committed.** An untested gate is decoration —
  two bugs in the CI file itself were caught this way on the first day.
- **Findings are reported, not silently fixed.** The 50× imbalance in the level-up reward
  choice (`PROGRESSION.md` §4) is implemented as specified *and* flagged with tuning
  options. Scaling the design down is the product owner's call.
- **No control is weakened to make progress.** It is changed by an ADR, in the open, with
  the risk stated — or not at all.

## 5. Open items for the product owner

1. **Git remote.** No remote is configured, so "upload to git with every change" is
   currently local commits only. A GitHub remote plus authorization is needed; the GitHub
   connector in this environment is unauthenticated.
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
