# Dungeon Barrage Progression and Economy

**Status:** Specification baseline
**Owner:** Backend / authoritative services
**Related:** [PRODUCT_SPEC.md](./PRODUCT_SPEC.md) §8, [ARSENAL.md](./ARSENAL.md), [SECURITY_BASELINE.md](./SECURITY_BASELINE.md)

Every value in this document is **server-owned**. A client never reports XP, currency, level,
ownership, or a level-up claim. See §6.

## 1. Levels

- Players begin at **Level 0**.
- The **maximum level is 55**. It is a hard cap, not a soft cap; XP earned at 55 is discarded
  for level purposes (match statistics still record it).
- Levelling is account-wide, not per-character and not per-weapon.

### XP curve

XP required to advance *from* level `L` to `L+1`:

```
xpForLevel(L) = 100 + 20*L + floor(L*L / 4)
```

| From level | XP to next | Cumulative |
|---:|---:|---:|
| 0 | 100 | 100 |
| 10 | 325 | 2,175 |
| 25 | 756 | 10,455 |
| 40 | 1,300 | 25,610 |
| 54 | 1,909 | 48,689 |

Total to Level 55 is **48,689 XP**. At the match rewards in §2 that is roughly **240 completed
matches**, or about 40 hours at a 10-minute match target. The curve is a versioned table
(`PROGRESSION_VERSION`), not a hardcoded formula in the client — retuning must not retroactively
change a player's level.

## 2. Earning XP

Every battle entered awards XP on **completion**, win or lose. Bonuses stack additively and are
each computed by the authoritative match host from the match event log.

| Source | XP | Notes |
|---|---:|---|
| Match completed | 120 | Requires a valid completion (§6.3) |
| Victory | +80 | Team modes award to every member of the winning team |
| First victory of the UTC day | +150 | Once per account per UTC day |
| Precision — ≥60% direct-hit rate, min 4 attacks | +40 | Encourages aiming over spam |
| Excavator — ≥1500 terrain cells removed | +30 | Rewards terrain-value play over raw damage |
| Environmental elimination | +50 each, cap +100 | Hazard/fall kills |
| Comeback — won a match after dropping below 25% HP | +60 | |
| Weapon proficiency — used a weapon with <10 recorded uses | +25, cap +50 | Onboards the arsenal |
| Survivor — finished a match at full health | +35 | |
| Rematch — completed a rematch with the same lobby | +20, cap +60/day | Retention, capped to resist farming |

**Per-match XP is capped at 600.** The cap is enforced after summation and is recorded in the
transaction so anomalies are auditable.

Forfeits, concessions, and disconnect-losses award the 120 completion XP only if the match met
the validity floor in §6.3. They never award victory or bonus XP.

## 3. Currency

One earned currency ledger (`shards`). No premium currency exists at this stage; if one is added
later it is a **separate ledger** and must never be exchangeable in either direction without an
explicit, separately reviewed decision.

| Source | Shards |
|---|---:|
| Victory | 120 |
| Completed loss | 40 |
| Level-up choice (see §4) | 50 |
| First victory of the UTC day | +60 |

A weapon in the shop costs **2,500 shards** — roughly 21 victories, or 62 completed losses.

**Ammunition is always free.** Ammunition and melee durability are match-scoped resources granted
in full at match start from the weapon definition. They are never purchasable, never persist
between matches, and no shop entry may sell, extend, or refill them. This is an invariant: a
storefront that sells ammunition converts the economy into pay-for-power.

## 4. Level-up reward choice

On each level gained, the player is granted **exactly one choice** among:

1. **A cosmetic** — one unowned cosmetic from the level's eligible pool.
2. **50 shards.**
3. **A new weapon** — one unowned weapon definition, permanently unlocked.

Rules:

- The choice is presented once per level and must be claimed to take effect. Unclaimed choices
  queue in order and persist indefinitely.
- Claiming is an **idempotent, server-validated transaction** keyed on `(playerId, level)`. A
  replayed claim returns the original result and grants nothing further.
- The **weapon option is offered only while the player has at least one unowned weapon.** Once
  the arsenal is complete the option is withdrawn and the choice is cosmetic vs shards.
- The **cosmetic option is offered only while an unowned eligible cosmetic exists**, on the same
  rule.
- If only one option remains eligible, it is still an explicit claim — never auto-granted.
- Multiple levels gained from one match produce that many separate queued choices.

### Known imbalance — flagged, not silently "fixed"

As specified, the three options are not close in value. A weapon is worth 2,500 shards in the
shop; the shard option grants 50. The weapon choice **strictly dominates the shard choice by 50×**
for as long as any weapon is unowned. With a 12-weapon launch arsenal, a rational player takes
weapons for their first 12 levels and the choice is not a real decision until level 12.

This is implemented as specified. Two tuning options are recorded for the product owner, neither
applied without a decision:

- **Option A — raise the shard grant** to 250–400, so the choice trades ~6–10 levels of shards
  against one weapon. Preserves the weapon-hunting fantasy, makes the shard pick defensible for a
  player saving toward a specific shop weapon.
- **Option B — tier the reward by level band**, e.g. weapons offered on levels 1–20, shards
  scaling 50 → 300 across the track, cosmetics weighted to later levels where the arsenal is done.

Recommendation: **Option A**, as a one-line versioned data change. It requires no new systems.

## 5. Competitive fairness boundary

`PRODUCT_SPEC.md` §8 states the competitive weapon set must be available to all ranked players and
that no mode sells competitive power. Level- and shop-gating weapons conflicts with that for any
skill-rated mode.

The boundary that preserves both:

| Mode class | Arsenal |
|---|---|
| Casual, social, private, training | Player's **unlocked** arsenal. Progression is expressed here. |
| Ranked / skill-rated (post-MVP) | **Normalized** — the full competitive arsenal, for everyone, regardless of level or purchases. |

Progression therefore governs *casual expression and collection*, never rated outcomes. Cosmetics
are unrestricted in every mode because they carry no gameplay values (`ARSENAL.md`, cosmetic
boundary).

A player entering ranked with 3 unlocked weapons and one with all 12 field the same options. This
must be stated plainly in the UI at the ranked entry point, or the progression system will read as
pay/grind-to-win regardless of the underlying truth.

## 6. Integrity

### 6.1 Server authority

XP, shards, level, unlocks, and claims are computed and stored exclusively by the authoritative
service. The client receives results to display. A client-submitted XP total, currency total,
level, ownership assertion, or "I levelled up" message is rejected, logged as a
`PROGRESSION_CLIENT_ASSERTION` security event, and never applied.

### 6.2 Idempotency

Every ledger mutation is an append-only transaction carrying:

```
idempotency_key   -- unique; replay returns the original row and mutates nothing
player_profile_id
ledger            -- 'xp' | 'shards'
amount_delta
source_type       -- 'match_completion' | 'level_choice' | 'purchase' | 'admin_adjustment'
source_id
created_at        -- server clock
```

Idempotency keys are deterministic, not random:

- Match rewards: `match:{matchId}:{playerProfileId}:{sourceType}`
- Level choice: `level:{playerProfileId}:{level}`
- Purchase: `purchase:{playerProfileId}:{clientPurchaseId}`

A duplicate match-completion message, a retried claim, or a double-clicked purchase therefore
cannot grant twice. This is an acceptance requirement in `PRODUCT_SPEC.md` §11 and is tested
directly.

Balances are **derived** by summing the ledger, with a periodically checkpointed materialized
balance for read performance. A materialized balance that disagrees with its ledger sum is a
alertable integrity fault, and the ledger wins.

### 6.3 Match validity floor

A match awards nothing unless the authoritative host certifies:

- The match reached a terminal state through the normal state machine.
- Duration ≥ 90 seconds of active match time.
- Each rewarded participant committed ≥ 2 valid actions.
- The result was signed by the room process that owned the match.

This blocks the cheapest farming loops: instant-forfeit cycling, idle lobbies, and self-matching.

### 6.4 Anti-abuse

- Per-account rate limits on match reward accrual, with anomaly alerting on accounts in the
  top percentile of shards-per-hour.
- Collusion signals — repeated same-pair matches with lopsided, low-action results — are flagged
  for review rather than auto-punished.
- Purchases validate affordability **inside the same transaction** as the debit, under a
  serializable isolation level or an equivalent conditional update. Read-then-write without
  atomicity is a duplication exploit and is prohibited.
- Negative balances are impossible by constraint (`CHECK (balance >= 0)`), not merely by
  application logic.
- Admin adjustments use a distinct `source_type`, require a recorded actor, and are surfaced in
  the audit log (`SECURITY_BASELINE.md`).

### 6.5 Versioning

Every reward computation records `progressionVersion`. Retuning publishes a new version; it never
recomputes or revokes historical grants.
