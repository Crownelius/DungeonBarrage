# ADR 0002: Character kits replace the three-slot loadout

**Status:** Accepted (2026-08-06)
**Supersedes:** `PRODUCT_SPEC.md` §3 (loadout contract); `ARSENAL.md` as the player-facing equipment system
**Decided by:** Product owner directive, with four clarifying decisions recorded in §3

## Context

The product design to this point was **equipment-driven**, in the Worms lineage: every
character shared a baseline, and expression came from choosing one Main, one Secondary,
and one Melee/Tool from a 12-weapon roster. `PRODUCT_SPEC.md` §5 explicitly required
"shared baseline health and movement" and "no paid stat bonuses".

The product owner has redirected to a **character-driven** model in the hero-shooter /
platform-fighter lineage: 24 characters, each with a fixed kit, asymmetric health (165–400),
asymmetric range and movement, a chargeable special, and a mid-match passive choice.

These are not compatible as a single system. Asymmetric characters *and* a free equipment
loadout would multiply the balance surface by 24 with no corresponding gain in legibility.

## Decision

Characters replace the loadout. A player selects one character; its kit is their whole
moveset.

### What is retired

- The `main` / `secondary` / `meleeTool` slot contract.
- `ARSENAL.md`'s 12 weapons as *player-selectable equipment*.
- Weapon purchases and weapon-based level-up rewards.

### What is retained, and why it matters

The retirement is at the **product** layer, not the **simulation** layer. The expensive,
already-built machinery survives intact:

| Retained | Now used for |
|---|---|
| Fixed-point math, terrain mask, terrain operations | Unchanged — character abilities reshape terrain identically |
| Ballistic integration and collision | Unchanged — Emi's cube and Roberto's grenade are projectiles |
| Damage / knockback / status resolution | Unchanged |
| Canonical encoding and state hashing | Unchanged |
| Seeded PRNG | Now also drives Arzum's target selection, Karl's crits, Aleph's blink |
| The closed behavior vocabulary | Character abilities compose from the same reviewed set |
| `WeaponDefinition` shape | Becomes `AbilityDefinition` — same fields, different owner |

This is why the in-flight core work was allowed to complete rather than being cancelled.
`terrain.rs`, `rng.rs`, `hash.rs`, and `ballistics.rs` are character-agnostic. Only the
*ownership* of an attack changes: from "a weapon a player equipped" to "an ability a
character has".

### The type change

```
Loadout { main, secondary, meleeTool }        →  CharacterSelection { character_id, passive_id }
WeaponSlot { Main, Secondary, MeleeTool }     →  AbilitySlot { Basic, BasicAlt, Special }
WeaponDefinition                              →  AbilityDefinition  (shape unchanged)
PlayerState.ammo: [AmmoCounter; 3]            →  PlayerState.special_gauge: u16 (hundredths)
```

`AbilitySlot::BasicAlt` exists because Aleph has two freely-chosen basic attacks (bow and
knife). Modelling that as a second basic is cheaper than a general per-character ability
list, and it bounds the UI at three buttons.

The gauge is stored in **hundredths** (`GAUGE_FULL = 10_000`), not as 0–100. `CHARACTERS.md`
§2's per-damage gains are fractional (+0.40 dealt, +0.25 taken, +0.30 healed) and a float
would break the determinism contract, so the scale absorbs the fraction and the arithmetic
stays integer. *(This paragraph corrects an earlier revision of this ADR, which specified
`u8` (0–100) — a type the implementation never used.)*

Ammunition disappears entirely. Basic attacks are unlimited; specials are gated by the
gauge. This removes the Longsword invariant (`ARSENAL.md`) along with the system it
constrained.

## Consequences

**Balance surface grows.** 24 asymmetric characters is a far larger tuning problem than 12
symmetric weapons, and it is permanent. The 48–52% win-rate target now applies per
character, per map, per skill band.

**Content commitment grows.** 24 characters × 3 passives = **72 passives**, plus 24 kits,
plus art and animation per character. `CHARACTERS.md` §4 proposes 27; 15 characters remain
entirely unspecified.

**Competitive access changes shape.** The previous fairness rule — rated modes normalize
the arsenal — does not transfer, because a character *is* the tactical identity. The owner
decided characters must be owned to be played in every mode, including rated. The nine
free starters covering all nine roles are what keeps this from being a power advantage;
see `CHARACTERS.md` §5 for the recorded risk and its mitigation trigger.

**Randomness enters committed actions.** Arzum's 50–200% ultimate roll and random second
target, Karl's crits, and Aleph's random blink destination all introduce variance into
decisions a player commits to. `PRODUCT_SPEC.md` warns that unexplained random damage
weakens competitive trust. Mitigation: every roll is drawn from the seeded match PRNG,
displayed in the result panel, and reproducible in replay. A narrower rated range for
Arzum is recommended but not applied.

## Rejected alternatives

- **Coexist (character + full loadout)** — multiplies the balance surface by 24 for depth
  the genre does not need.
- **Hybrid (character kit + one equipment slot)** — keeps some weapon-collection value,
  but leaves two half-systems to balance and two things for the shop to sell. Cleanliness
  of a single expression axis was judged worth more.
- **Cancel the in-flight core work** — would have discarded terrain, RNG, hashing, and
  ballistics, none of which depend on who owns an attack.

## 2026-09-04 implementation amendment

The product owner reaffirmed this ADR after the interim one-Crow, 32-item ammunition cut reduced
meaningful choice. Character kits are again the active product model. The first shippable roster is
deliberately narrowed from the earlier 24-character vision to four characters: Leslie, Crow, Erus,
and Kreena. This changes release scope, not the architectural decision that characters own their
complete movesets.

The schema-2 client selects only `characterId` and presents Shot 1, Shot 2/Melee, and SS. Both normal
actions are unlimited. SS is gauge-gated and does not consume the normal action, allowing it before
or after the turn's normal attack. The old item catalog and loadout/ammo state remain temporarily as
a Rust-only replay-migration bridge; they are not player-selectable content.

See `docs/CHARACTER_SYSTEM_IMPLEMENTATION_PLAN.md` for the phased mechanics plan and its explicit
list of current special-ability approximations.
