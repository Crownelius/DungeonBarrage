# Dungeon Barrage Character Roster

**Status:** Specification baseline
**Supersedes:** `PRODUCT_SPEC.md` §3 (three-slot loadout contract) and `ARSENAL.md` as the primary expression system
**Related:** [PROGRESSION.md](./PROGRESSION.md) · [PRODUCT_SPEC.md](./PRODUCT_SPEC.md) · [adr/0002-character-kits.md](./adr/0002-character-kits.md)

## 1. The model

Dungeon Barrage has **24 playable characters**. A player picks one character per match;
that character's fixed kit is their entire moveset. This replaces the Main / Secondary /
Melee-Tool loadout — character identity, not equipment, is the expression axis.

Every character has:

| Element | Description |
|---|---|
| **Health** | Asymmetric, 165–400 HP |
| **Special gauge** | 0–100. At 100, the special ability becomes available |
| **Basic attack** | Always available, no charge cost |
| **Special ability** | Consumes the full gauge |
| **Passive** | Chosen **once per match**, the first time the gauge fills |
| **Range tier** | Melee, Tier 1, Tier 2, or Tier 3 |
| **Movement** | Slow, Normal, or Fast |

**Nine characters are starters**, owned by every account from the first match. The other
fifteen cost **2,300 credits** each.

## 2. Global scale

Every damage figure in this document is a percentage of a single reference value.

```
BASE_ATTACK = 100
```

So "55% damage" means **55 hit points**. One reference value keeps all 24 characters
comparable and preserves the existing balance scale — `ARSENAL.md`'s weapons dealt 48–62
against 200 HP, so a 3–4 hit kill. Against the new 165–400 HP range, typical attacks now
resolve in 3–7 hits, which suits a turn-based game where each turn must feel consequential
but a single mistake should not end a match.

Percentages are **not** relative to target HP. Making them so would erase the point of a
400 HP tank — it would die in the same number of hits as a 190 HP assassin.

### Range tiers

| Tier | Reach | Intent |
|---|---:|---|
| Melee | 1.25 BW | Must close completely; terrain and positioning dominate |
| Tier 1 | 8 BW | Short. Pressures adjacent ground, cannot cross the map |
| Tier 2 | 16 BW | Medium. The default engagement band |
| Tier 3 | 26 BW | Long. Threatens most of a standard map |

`BW` is one character body width, as in `ARSENAL.md`.

### Movement

| Class | Per turn |
|---|---:|
| Slow | 2.5 BW |
| Normal | 4 BW |
| Fast | 8 BW |

### Special gauge

The gauge fills from **both** a per-turn trickle and combat participation, so a zoned-out
or heavily-outranged player still eventually gets their special, while aggression is
rewarded.

| Source | Gain |
|---|---:|
| Turn start (own turn) | +8 |
| Per point of damage dealt | +0.40 |
| Per point of damage taken | +0.25 |
| Ally healed (per point) | +0.30 |

**Caps that matter:**
- Maximum **+45 from any single action**. Without this, one large hit charges the gauge
  instantly and the ability stops being something you build toward.
- The gauge does not carry past 100, and excess is discarded.
- It does **not** reset on death in modes with respawn; it resets at match start only.

An unaided character reaches 100 in **13 turns** on trickle alone, or roughly 4–6 turns
while actively fighting. The gauge value is public to all players — a hidden ultimate
timer removes the counterplay that makes it interesting.

### The passive choice

The **first** time a character's gauge reaches 100 in a match, the player chooses one of
**three character-specific passives** before spending the special. The choice is locked
for the remainder of that match.

This puts a real decision at the moment of the first power spike, and it means the same
character can be played toward different roles as a match develops. Passive pools are
per-character; §4 proposes pools for the nine starters.

## 3. Starter roster

All nine are free on every account.

| # | Character | HP | Range | Movement | Role |
|---:|---|---:|---|---|---|
| 1 | Arzum | 300 | Tier 1 | Fast (2×) | Diver |
| 2 | Emi | 300 | Tier 2 | Normal | Zone control |
| 3 | Karl | 360 | Tier 1 | Slow | Attrition brawler |
| 4 | Huck | 400 | Melee | Normal | Tank / displacement |
| 5 | Numa | 190 | Tier 3 | Normal | Assassin |
| 6 | Aleph | 240 | Tier 2 | Normal | All-rounder / trapper |
| 7 | Zeke | 220 | Tier 3 | Normal | Support |
| 8 | Roberto | 165 | Tier 2 | Normal | Grenadier |
| 9 | Natomica | 400 | Tier 2 | Normal | Bruiser / displacement |

---

### 1. Arzum — Diver

**300 HP · Tier 1 · Fast (8 BW/turn)**

His double movement is the whole identity: he closes distance nobody else can, at the cost
of short reach and no way to threaten from safety.

- **Basic — Lunge.** 45% damage at Tier 1 range.
- **Special — Chain Strike.** Attacks the target, then teleports to a **random** nearby
  enemy and attacks them for **50–200%** damage.
  - "Nearby" is within 12 BW of the first target.
  - The second target is chosen uniformly at random from eligible enemies, from the
    seeded match PRNG. If none are eligible, the special still resolves its first hit and
    the gauge is still consumed.
  - The damage roll is **shown before commitment is finalized** and recorded in the
    result panel.

> **Flagged for balance review.** A 50–200% roll is a 4× swing on a committed ultimate —
> 50 damage or 200 damage from the same decision. `PRODUCT_SPEC.md` §"bounded uncertainty"
> warns that unexplained random damage weakens competitive trust. Implemented as
> specified. Recommendation: keep the full range in casual, narrow to **90–150%** in rated
> modes, published as a versioned rule. This preserves the gamble where it is fun and
> removes it where it decides ladder placement.

---

### 2. Emi — Zone control

**300 HP · Tier 2 · Normal**

- **Basic — Heavy Cube.** 50% damage. The cube is dense: **wind response 2,000 bp**
  (lowest in the roster), so Emi is the most reliable calibrator in bad wind.
- **Special — Cube Turret.** Throws a cube that flies to the target location and becomes a
  turret for **3 turns**. While it lives, any enemy Emi attacks that is also **within 10 BW
  of the turret** takes one additional basic attack from it.
  - The turret is a destructible object with 80 HP, not terrain.
  - It fires only in response to Emi's attacks — it never acts on its own turn, so it
    cannot stall a match.
  - Only one Emi turret exists at a time; a second replaces the first.

---

### 3. Karl — Attrition brawler

**360 HP · Tier 1 · Slow (2.5 BW/turn)**

Throws meat; a rottweiler and a flock of crows converge on the target and drag the meat
away. High total output at close range, punished badly by kiting.

- **Basic — Carrion Call.** **Three attacks per turn**, each dealing **24%**, or **74% on
  a critical hit**. Uncrit total 72% per turn — the highest sustained damage of the
  starters, balanced by Tier 1 reach and the slowest movement in the roster.
- Each of the three attacks rolls its crit independently from the match PRNG.
- **Special — Feeding Frenzy.** The pack fixates: the next 3 attacks against the marked
  target crit automatically.

> **Discrepancy noted.** The brief states three attacks "each dealing a 33% damage" *and*
> that the basic attack deals 24% / 74% crit. These conflict. Implemented at **24% / 74%**
> because that figure is stated more specifically and yields a coherent 72% turn total;
> 33% × 3 = 99% per turn would make Karl the highest-damage starter by a wide margin.
> Confirm which was intended.

---

### 4. Huck — Tank / displacement

**400 HP · Melee only · Normal**

- **Basic — Haymaker.** 60% damage. Destroys terrain in a **2.0 BW radius around both
  Huck and his target** — the only basic attack in the roster that reshapes the map, which
  makes Huck a tunneller as much as a fighter.
- **Special — Body Throw.** Teleports to one target, then relocates them to a second
  target's position.
  - Both targets are player-selected, not random.
  - The thrown character takes **40% damage**; the destination character takes **25%**.
  - Enables environmental kills by throwing an enemy into a hazard, which is a stated
    genre pillar.

---

### 5. Numa — Assassin

**190 HP · Tier 3 · Normal**

Lowest health but longest reach among the starters. She either executes or dives, and the
harpoon decides which.

- **Basic — Harpoon.** 42% damage, then repositions based on the target's health:
  - Target **at or below 50%** max HP → **the target is dragged to Numa** (execute).
  - Target **above 50%** max HP → **Numa is pulled to the target** (engage).
  - The threshold is visible on the target's health bar before committing, so the
    reposition is a decision rather than a surprise.
- **Special — Pin.** Locks a character in place for **2 turns**. They may aim, fire, and
  use abilities, but cannot move or be displaced.

> **Interpretation.** The brief reads "drags her to her target or her to the target,
> depending on HP", which states the same direction twice. The rule above is the reading
> that makes both halves meaningful. Confirm the threshold and direction.

---

### 6. Aleph — All-rounder / trapper

**240 HP · Tier 2 · Normal**

Two basic attacks, chosen freely each turn.

- **Basic A — Bow.** 20% damage. Cheap, accurate, high wind response.
- **Basic B — Throwing Knife.** 60% damage. **The knife embeds where it lands and
  persists.**
- **Dagger chain — new mechanic.** An embedded knife stays in the terrain indefinitely. If
  a *second* knife strikes within **1.5 BW** of an embedded one, both detonate for **70%
  damage** in a 2.5 BW radius and remove terrain in that radius.
  - Chains propagate: a detonation that reaches another embedded knife triggers it too,
    resolved in a deterministic order (ascending embed sequence) so the same board always
    resolves identically.
  - Embedded knives are visible to **all** players. A hidden minefield is not counterplay.
  - Cap of **8 embedded knives** per Aleph; the oldest is removed when a ninth lands.
- **Special — Veilstep.** Emits a gas cloud of 8 BW radius centred on Aleph, then
  teleports him to a **random** position within it, drawn from the seeded PRNG. The cloud
  blocks line of sight for 2 turns.

Environmental impact is otherwise low — Aleph reshapes the map through dagger chains, not
through raw blast radius.

---

### 7. Zeke — Support

**220 HP · Tier 3 · Normal**

The only starter whose basic attack helps a teammate.

- **Basic — Mending Bolt.** Deals **41% damage** to an enemy and simultaneously heals a
  chosen ally for **22%** (22 HP). In free-for-all, with no ally to target, the heal
  applies to Zeke at **half value** (11 HP) so he is not a dead pick outside team modes.
- **Special — Lifeshare.** Either:
  - **Transfer:** give a chosen teammate health from Zeke's own pool, up to 100 HP, or
  - **Restore:** heal Zeke for a flat **100 HP**.
  - Transfer cannot reduce Zeke below 1 HP — a support cannot be made to kill themselves
    by a mis-click.

> **Interpretation.** "Heals an ally for 22% of 41% damage" is ambiguous. Read as two
> separate percentages of `BASE_ATTACK`: 41 damage dealt, 22 healed. The alternative
> reading (22% *of the damage dealt* = 9 HP) is too small to matter for a character whose
> identity is healing. Confirm.

---

### 8. Roberto — Grenadier

**165 HP · Tier 2 · Normal**

Lowest health in the roster. Reshapes terrain more than anyone except Huck.

- **Basic — Bouncing Grenade.** **55% damage.** Bounces exactly **once** off terrain
  before detonating, which lets it reach behind cover — the bank shot is the skill
  expression.
- **Special — Heavy Ordnance.** A large bomb dealing **65% to enemies** and **85% to
  terrain**. The largest crater in the starter roster.
  - Terrain and damage radii are tuned separately, per `ARSENAL.md` guardrail 7.
  - Friendly fire applies. At 165 HP, Roberto can comfortably kill himself with his own
    ultimate, and the blast preview shows this before commitment.

---

### 9. Natomica — Bruiser / displacement

**400 HP · Tier 2 · Normal**

- **Basic — Claymore Hook.** 48% damage, and **pulls Natomica toward the target**,
  closing distance on her own terms. She is a 400 HP body that chooses its engagements.
- **Special — Repulse.** Pushes all enemies within 10 BW away for **55% damage**. An enemy
  driven into terrain takes an additional **25%** (13.75 damage) on impact.
  - The wall bonus is a separate damage line in the result panel, so players can learn
    the interaction rather than guess at it.
  - Pairs directly with hazards: pushing an enemy off a ledge is often worth more than the
    damage.

---

## 4. Proposed passive pools

**These are proposals, not settled content.** The passive *system* is specified above; the
passives themselves were not, so three are drafted per starter to make the system
implementable and reviewable. Replace freely — they are versioned data, and changing them
requires no code.

The remaining fifteen characters need pools authored as they are designed: **72 passives
total across 24 characters** is a substantial content commitment worth scheduling
deliberately.

| Character | Passive A | Passive B | Passive C |
|---|---|---|---|
| Arzum | **Momentum** — +2 BW movement after any kill | **Chain Reaction** — Chain Strike's second hit rolls twice, taking the higher | **Lightfoot** — takes no fall damage |
| Emi | **Reinforced Casing** — turret lasts 4 turns instead of 3 | **Dense Payload** — basic attack ignores wind entirely | **Overwatch** — turret range 10 → 14 BW |
| Karl | **Pack Leader** — crit chance +15% | **Thick Hide** — takes 10% less damage while below 50% HP | **Scavenger** — heal 8 HP per attack that connects |
| Huck | **Immovable** — immune to knockback and displacement | **Demolition** — Haymaker terrain radius 2.0 → 3.0 BW | **Follow Through** — Body Throw deals +20% to both targets |
| Numa | **Executioner** — +30% damage against targets below 35% HP | **Barbed Line** — Pin lasts 3 turns instead of 2 | **Ghost** — first attack each match against a full-health target crits |
| Aleph | **Volatile** — dagger detonations 70% → 95% | **Deep Quiver** — embedded knife cap 8 → 12 | **Mistwalker** — Veilstep destination is chosen, not random |
| Zeke | **Field Medic** — heals are +40% effective | **Transfusion** — Lifeshare Transfer costs Zeke half the HP it gives | **Guardian** — allies within 6 BW take 10% less damage |
| Roberto | **Double Bounce** — grenade bounces twice instead of once | **Blast Shielding** — immune to own friendly fire | **Shrapnel** — Heavy Ordnance leaves a 2-turn contact zone |
| Natomica | **Anchored** — Claymore Hook pulls the *target* instead when they are lighter | **Kinetic** — wall-impact bonus 25% → 45% | **Bulwark** — +60 max HP |

## 5. Fairness and competitive access

Per the product owner's decision, **a character must be owned to be played, in every
mode** — including rated.

What makes this workable rather than pay-to-win:

- **Nine starters are free on every account, permanently.** They span diver, zone control,
  brawler, tank, assassin, all-rounder, support, grenadier, and bruiser — a complete role
  spread. No player is ever locked out of a viable competitive pick.
- **Credits are earned, not sold.** There is no purchase path for credits at this stage
  (`PROGRESSION.md` §3). Roster completion is a time investment, not a spending one.
- **No character is strictly stronger than a starter.** The remaining fifteen must be
  designed as *lateral* additions — new tactical shapes, not power upgrades. This is a
  binding design constraint, and the balance target in `ARSENAL.md` guardrail 12
  (48–52% slot-adjusted win rate) applies to every character regardless of unlock cost.

Recorded honestly: an owned-to-play model still means a full-roster player has more
counterpick options than a nine-character player in a rated setting. The mitigations above
reduce that to an options advantage rather than a power advantage. If ladder data later
shows roster size correlating with rating independent of skill, the mitigation is a free
rotation on top of ownership.

## 6. What this supersedes

| Superseded | Status |
|---|---|
| `PRODUCT_SPEC.md` §3 three-slot loadout | Retired. Characters replace equipment as the expression axis |
| `ARSENAL.md` 12-weapon roster | Retired as player-selectable equipment |
| `PROGRESSION.md` weapon purchases at 2,500 | Replaced by character purchases at 2,300 |
| Level-up "new weapon" option | Becomes "new character" |

**Not superseded — still binding:**

- The cosmetic boundary. Skins change appearance only, never damage, reach, hitbox,
  timing, or readability (`ARSENAL.md`).
- Damage / terrain / knockback radii remain separately tunable (guardrail 7).
- Status effects last at most one affected turn and do not stack (guardrail 4), **except**
  Numa's Pin and Emi's turret, which are ability durations with explicit published values.
- Server authority over every value in this document (`SECURITY_BASELINE.md` §2).
- The weapon *behavior vocabulary* (`ballisticImpact`, `drillThenDetonate`, `meleeArc`,
  `applyKnockback`, …) is retained and reused — character abilities are composed from the
  same reviewed, closed set. No character introduces downloadable or scripted behavior.

## 7. Open questions

1. Karl's per-hit damage: **24%/74% crit** (implemented) vs the brief's "33%".
2. Numa's harpoon direction rule and the 50% HP threshold.
3. Zeke's heal magnitude: **22 HP** (implemented) vs 22% of damage dealt (≈9 HP).
4. Arzum's 50–200% ultimate roll in rated play.
5. Passive pools for all 24 characters — §4 proposes 27 of the 72.
6. The remaining **15 characters** are unspecified.
