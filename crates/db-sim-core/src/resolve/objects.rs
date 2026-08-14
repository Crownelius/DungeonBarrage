//! Resolvers for `SpawnTurret`, `EmbedProjectile`, and `ChainDetonate`.
//!
//! This family owns the persistent-object lifecycle: Emi's Cube Turret, and Aleph's
//! embedded throwing knives together with the dagger chain that detonates them
//! (`CHARACTERS.md` §3.2, §3.6). Owned exclusively by this file
//! (`docs/MODULE_OWNERSHIP.md`) — nothing outside `resolve/objects.rs` mutates a
//! [`PersistentObject`], and this file never reaches into a sibling resolver.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{SimError, SimResult};
use crate::fixed::{self, FixedPoint};
use crate::types::{
    BASE_ATTACK, EffectKind, MaterialMask, PersistentObject, PersistentObjectKind, SpecialEffect,
    TerrainOperation, TerrainShape,
};

use super::ResolveContext;

/// Emi's Cube Turret's starting and maximum health (`CHARACTERS.md` §3.2: "a destructible
/// object with 80 HP").
///
/// `EMI_TURRET_SPAWN` in `character.rs` does not carry this figure — its
/// `magnitude_secondary` is unused (`0`) — so `CHARACTERS.md` is the only source, and this
/// constant is a direct transcription of it, not a guess.
const EMI_TURRET_HEALTH: u16 = 80;

/// Nominal, non-zero health for an embedded knife.
///
/// `CHARACTERS.md` never gives embedded knives their own health: they leave play only by
/// detonating ([`chain_detonate`]) or by cap eviction ([`embed_projectile`]), never by
/// being damaged down to zero the way a turret is. [`PersistentObject::health`] is
/// non-optional, so this documents "not independently destructible" rather than a balance
/// value — nothing in this file ever subtracts from it.
const EMBEDDED_KNIFE_HEALTH: u16 = 1;

/// 1.5 BW: how close a *landing* knife must be to an already-embedded one to trigger a
/// chain detonation (`CHARACTERS.md` §3.6: "a second knife strikes within 1.5 BW of an
/// embedded one").
///
/// This must equal `ALEPH_EMBED_KNIFE.magnitude` in `character.rs`, which documents the
/// identical figure for the identical reason but cannot be read from here directly:
/// `character.rs` and this file are separately owned (`MODULE_OWNERSHIP.md`), and a
/// [`SpecialEffect`] carries no back-reference to sibling effects on the same ability —
/// `ChainDetonate`'s own effect data (`ALEPH_CHAIN_DETONATE`) has no field for it.
/// `CHARACTERS.md` is authoritative for both copies.
const KNIFE_CHAIN_TRIGGER_RADIUS: i32 = 6 * fixed::POSITION_SCALE;

/// Resolves an objects-family effect: [`EffectKind::SpawnTurret`],
/// [`EffectKind::EmbedProjectile`], or [`EffectKind::ChainDetonate`].
///
/// # Errors
///
/// Returns an error when the effect's parameters are out of range or a fixed-point
/// computation would overflow.
pub fn resolve(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    match effect.kind {
        EffectKind::SpawnTurret => spawn_turret(ctx, effect),
        EffectKind::EmbedProjectile => embed_projectile(ctx, effect),
        EffectKind::ChainDetonate => chain_detonate(ctx, effect),
        // `resolve_effect` in `mod.rs` only ever routes these three kinds here. A
        // catch-all rather than an error keeps this file correct on its own terms without
        // needing to reach into `mod.rs`'s dispatch to prove it.
        _ => Ok(()),
    }
}

/// Resolves [`EffectKind::SpawnTurret`] (Emi's Cube Turret, `CHARACTERS.md` §3.2).
///
/// Removes any turret this actor already owns before inserting the new one, so "only one
/// Emi turret at a time" holds regardless of how many times this effect fires for the same
/// player. Other players' turrets are untouched — the cap is per-owner.
fn spawn_turret(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let actor_id = ctx.actor_id.to_owned();

    ctx.state.objects.retain(|object| {
        !(object.kind == PersistentObjectKind::Turret && object.owner_id == actor_id)
    });

    let sequence = ctx.next_object_sequence();
    let turret = PersistentObject {
        sequence,
        owner_id: actor_id,
        kind: PersistentObjectKind::Turret,
        position: ctx.impact_point,
        health: EMI_TURRET_HEALTH,
        turns_remaining: effect.duration_turns,
    };

    // `state.objects` is documented as kept sorted by `sequence`; appending is sufficient
    // to preserve that because `next_object_sequence` is strictly increasing and the
    // `retain` above only removes elements, never reorders the survivors.
    ctx.state.objects.push(turret.clone());
    ctx.objects_created.push(turret);
    Ok(())
}

/// Resolves [`EffectKind::EmbedProjectile`] (Aleph's Throwing Knife embed,
/// `CHARACTERS.md` §3.6).
///
/// The cap comes from `effect.magnitude_secondary` rather than a hardcoded `8`, so a
/// passive that raises it (`PassiveKind::BonusObjectCap`) only needs different published
/// data, never a change here.
fn embed_projectile(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let cap = usize::try_from(effect.magnitude_secondary).map_err(|_err| SimError::OutOfRange {
        field: "EmbedProjectile.magnitude_secondary (embedded knife cap)",
    })?;
    if cap == 0 {
        return Err(SimError::OutOfRange {
            field: "EmbedProjectile.magnitude_secondary (embedded knife cap)",
        });
    }

    let actor_id = ctx.actor_id.to_owned();
    let sequence = ctx.next_object_sequence();
    let knife = PersistentObject {
        sequence,
        owner_id: actor_id.clone(),
        kind: PersistentObjectKind::EmbeddedKnife,
        position: ctx.impact_point,
        health: EMBEDDED_KNIFE_HEALTH,
        turns_remaining: effect.duration_turns, // u8::MAX: persists until triggered or capped
    };

    ctx.state.objects.push(knife.clone());
    ctx.objects_created.push(knife);

    // Evict the oldest of this owner's knives beyond the cap (`CHARACTERS.md` §3.6: "the
    // oldest is removed when a ninth lands"). At most one knife is inserted per call, so
    // at most one eviction is ever needed to restore the invariant, and other owners'
    // knives are never touched — the cap is per-owner.
    let owner_knife_count = ctx
        .state
        .objects
        .iter()
        .filter(|object| {
            object.kind == PersistentObjectKind::EmbeddedKnife && object.owner_id == actor_id
        })
        .count();
    if owner_knife_count > cap {
        let oldest_sequence = ctx
            .state
            .objects
            .iter()
            .filter(|object| {
                object.kind == PersistentObjectKind::EmbeddedKnife && object.owner_id == actor_id
            })
            .map(|object| object.sequence)
            .min();
        if let Some(oldest_sequence) = oldest_sequence {
            ctx.state.objects.retain(|object| {
                !(object.kind == PersistentObjectKind::EmbeddedKnife
                    && object.owner_id == actor_id
                    && object.sequence == oldest_sequence)
            });
        }
    }

    Ok(())
}

/// Resolves [`EffectKind::ChainDetonate`] (Aleph's dagger chain, `CHARACTERS.md` §3.6).
///
/// Assumes [`embed_projectile`] has already embedded the knife that just landed at
/// `ctx.impact_point` — both effects live on the same ability
/// (`ALEPH_THROWING_KNIFE.effects` in `character.rs`) and resolve in that declared order.
///
/// # Determinism
///
/// Every step processes knives by ascending `sequence`, never by position or discovery
/// order (`BTreeMap`/`BTreeSet` throughout, no `HashMap`), and touches no randomness — the
/// same board always resolves the same chain, in the same order.
///
/// # Termination
///
/// `detonated` is a visited set: a sequence is only ever added to `pending` while it is
/// absent from `detonated` ([`propagate`]'s guard), and every loop iteration here moves
/// exactly one sequence out of `pending` and into `detonated`. With finitely many embedded
/// knives on the board, the loop runs at most once per knife regardless of how densely
/// they are packed — a proximity cycle (three knives all within blast radius of each
/// other) cannot loop.
fn chain_detonate(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let blast_radius = effect.magnitude_secondary;
    let Some(damage_hp_i32) = fixed::scale(BASE_ATTACK, effect.magnitude, 100) else {
        return Err(SimError::Overflow {
            context: "ChainDetonate damage percent to hit points",
        });
    };
    let Ok(damage_hp) = u16::try_from(damage_hp_i32) else {
        return Err(SimError::OutOfRange {
            field: "ChainDetonate.magnitude (damage percent)",
        });
    };
    let radius_cells = radius_to_terrain_cells(blast_radius)?;

    // Snapshot every embedded knife's position, keyed by creation sequence. Positions
    // never move mid-chain, so one snapshot taken now stays valid for the whole
    // resolution; ascending key order is exactly the processing order the chain must use.
    let knives: BTreeMap<u32, FixedPoint> = ctx
        .state
        .objects
        .iter()
        .filter(|object| object.kind == PersistentObjectKind::EmbeddedKnife)
        .map(|object| (object.sequence, object.position))
        .collect();

    // The knife `embed_projectile` just embedded this action: the highest-sequence knife
    // sitting exactly at the impact point. `knives` iterates in ascending sequence order,
    // so overwriting on every match leaves the *last* (highest) match once the loop ends.
    let mut landing_sequence: Option<u32> = None;
    for (&sequence, &position) in &knives {
        if position == ctx.impact_point {
            landing_sequence = Some(sequence);
        }
    }
    // No knife at the impact point means this effect fired without a preceding embed — no
    // current ability does that, but it is a no-op here rather than an assumption.
    let Some(landing_sequence) = landing_sequence else {
        return Ok(());
    };
    let Some(&landing_position) = knives.get(&landing_sequence) else {
        return Ok(()); // Unreachable given the lookup above, but never indexed blindly.
    };

    // Initial trigger: the landing knife detonates only if an existing knife is within the
    // 1.5 BW trigger radius. Every such neighbour detonates too — CHARACTERS.md's "a
    // second knife strikes within 1.5 BW of an embedded one" states one pairing, but
    // nothing in the spec caps how many knives can be in range of the landing point at
    // once.
    let mut pending: BTreeSet<u32> = BTreeSet::new();
    for (&sequence, &position) in &knives {
        if sequence != landing_sequence
            && fixed::within_radius(landing_position, position, KNIFE_CHAIN_TRIGGER_RADIUS)
        {
            pending.insert(sequence);
        }
    }
    if pending.is_empty() {
        // Landed with nothing nearby: stays embedded, exactly as `embed_projectile` left it.
        return Ok(());
    }
    pending.insert(landing_sequence);

    let mut detonated: BTreeSet<u32> = BTreeSet::new();

    while let Some(sequence) = pending.pop_first() {
        // Defense in depth: the insertion guard in the propagation loop below already
        // keeps a detonated sequence out of `pending`, but termination should not rest on
        // that alone.
        if detonated.contains(&sequence) {
            continue;
        }
        detonated.insert(sequence);

        let Some(&position) = knives.get(&sequence) else {
            continue; // The snapshot always has it; never index blindly regardless.
        };

        // Every living character caught in the blast takes damage, not only opponents —
        // this is a positional area effect at a point in space, the same way Roberto's
        // Heavy Ordnance applies friendly fire, not a targeted "enemies only" ability like
        // Natomica's Repulse.
        let caught: Vec<String> = ctx
            .state
            .players
            .iter()
            .filter(|player| {
                !player.is_eliminated()
                    && fixed::within_radius(position, player.position, blast_radius)
            })
            .map(|player| player.id.clone())
            .collect();
        for player_id in &caught {
            apply_detonation_damage(ctx, player_id, damage_hp);
        }

        let terrain_sequence = ctx.next_terrain_sequence();
        let op = TerrainOperation {
            sequence: terrain_sequence,
            shape: TerrainShape::SubtractCircle {
                center: position,
                radius_cells,
            },
            material_mask: MaterialMask::SOFT,
        };
        // Routed through `block_ops` so block health stays the authority inside a
        // block span (ADR 0005), and the count is accumulated rather than discarded
        // (`todolist.md` P2 -- it feeds the Excavator XP bonus).
        *ctx.terrain_cells_removed = ctx
            .terrain_cells_removed
            .saturating_add(crate::block_ops::apply_operation(ctx.state, &op));
        ctx.terrain_ops.push(op);

        // Propagation: any other still-embedded, not-yet-detonated knife caught in this
        // blast joins the chain.
        for (&other_sequence, &other_position) in &knives {
            if other_sequence != sequence
                && !detonated.contains(&other_sequence)
                && fixed::within_radius(position, other_position, blast_radius)
            {
                pending.insert(other_sequence);
            }
        }
    }

    // Detonated knives leave play; every other object (turrets, undetonated knives of
    // this or another owner) is untouched.
    ctx.state.objects.retain(|object| {
        !(object.kind == PersistentObjectKind::EmbeddedKnife
            && detonated.contains(&object.sequence))
    });

    Ok(())
}

/// Applies `amount` splash damage from a knife detonation to `player_id`.
///
/// Categorized as [`crate::types::DamageEvent::splash`] — an area effect centred on a
/// point in space, not an exact direct hit ([`crate::types::DamageEvent::direct`]) nor a
/// world hazard ([`crate::types::DamageEvent::hazard`], reserved for environmental terrain
/// features). Health saturates at zero; a missing or already-eliminated target is a silent
/// no-op, matching this crate's rule that untrusted or stale state never panics a
/// resolver.
fn apply_detonation_damage(ctx: &mut ResolveContext<'_>, player_id: &str, amount: u16) {
    let Some(player) = ctx.state.player_mut(player_id) else {
        return;
    };
    if player.is_eliminated() {
        return;
    }
    let previous_health = player.health;
    // Saturating: health floors at zero rather than wrapping past it.
    player.health = player.health.saturating_sub(amount);
    let actual = previous_health.saturating_sub(player.health);
    let eliminated = player.health == 0;
    // `player`'s borrow of `ctx.state` ends at its last use above; `ctx.damage_entry`
    // below needs the whole `ctx`, so nothing may still hold `player` at this point.

    let entry = ctx.damage_entry(player_id);
    entry.splash = entry.splash.saturating_add(actual);
    entry.eliminated = entry.eliminated || eliminated;
}

/// Converts a fixed-point radius (`POSITION_SCALE` units) to whole terrain cells for
/// [`TerrainShape::SubtractCircle`], rounding half away from zero via
/// [`fixed::round_divide`] — `fixed.rs`'s one sanctioned rounding rule.
fn radius_to_terrain_cells(radius: i32) -> SimResult<u16> {
    let Some(cells) = fixed::round_divide(i64::from(radius), i64::from(fixed::POSITION_SCALE))
    else {
        return Err(SimError::Overflow {
            context: "ChainDetonate blast radius to terrain cells",
        });
    };
    u16::try_from(cells).map_err(|_err| SimError::OutOfRange {
        field: "ChainDetonate.magnitude_secondary (blast radius)",
    })
}

#[cfg(test)]
// `let ... else { panic!() }` states a fixture invariant far more clearly than threading a
// `Result` through every test case. Matches the precedent set by `terrain::tests` and
// `command::tests`; production paths in this file remain panic-free.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::terrain;
    use crate::types::{
        Appearance, DamageEvent, EffectTrigger, MatchPhase, Material, PlayerState, SimulationState,
        TerrainMask,
    };

    // -----------------------------------------------------------------------------------
    // Fixtures
    // -----------------------------------------------------------------------------------

    /// A 60x20 terrain mask, larger than any blast radius exercised in these tests,
    /// filled uniformly with `fill` so cell state is easy to reason about by hand.
    fn terrain_mask(fill: Material) -> TerrainMask {
        TerrainMask {
            width: 60,
            height: 20,
            cells: vec![fill as u8; 1_200],
        }
    }

    fn player(id: &str, position: FixedPoint) -> PlayerState {
        PlayerState {
            id: id.to_string(),
            team: 0,
            health: 300,
            max_health: 300,
            position,
            character_id: "test-character".to_string(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn base_state(players: Vec<PlayerState>) -> SimulationState {
        SimulationState {
            blocks: Vec::new(),
            simulation_version: 1,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::Resolution,
            active_player_id: "actor".to_string(),
            wind_per_tick: 0,
            movement_remaining: 0,
            has_attacked_this_turn: true,
            terrain: terrain_mask(Material::Empty),
            players,
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 1,
        }
    }

    fn spawn_turret_effect() -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::SpawnTurret,
            magnitude: 3,
            magnitude_secondary: 0,
            duration_turns: 3,
        }
    }

    fn embed_projectile_effect(cap: i32) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::EmbedProjectile,
            magnitude: KNIFE_CHAIN_TRIGGER_RADIUS,
            magnitude_secondary: cap,
            duration_turns: u8::MAX,
        }
    }

    fn chain_detonate_effect() -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::ChainDetonate,
            magnitude: 70,
            magnitude_secondary: 10 * fixed::POSITION_SCALE,
            duration_turns: 0,
        }
    }

    /// Bundles everything [`ResolveContext`] borrows, so each test builds one without
    /// repeating five separate `let mut` bindings.
    struct Fixture {
        state: SimulationState,
        rng: Rng,
        damage: BTreeMap<String, DamageEvent>,
        terrain_cells_removed: u32,
        terrain_ops: Vec<TerrainOperation>,
        objects_created: Vec<PersistentObject>,
    }

    impl Fixture {
        fn new(players: Vec<PlayerState>) -> Self {
            Self {
                state: base_state(players),
                rng: Rng::from_state(1),
                damage: BTreeMap::new(),
                terrain_cells_removed: 0,
                terrain_ops: Vec::new(),
                objects_created: Vec::new(),
            }
        }

        fn resolve_effect(
            &mut self,
            actor_id: &'static str,
            impact_point: FixedPoint,
            effect: &SpecialEffect,
        ) -> SimResult<()> {
            let mut ctx = ResolveContext {
                state: &mut self.state,
                rng: &mut self.rng,
                actor_id,
                primary_target_id: None,
                secondary_target_id: None,
                impact_point,
                damage: &mut self.damage,
                terrain_cells_removed: &mut self.terrain_cells_removed,
                terrain_ops: &mut self.terrain_ops,
                objects_created: &mut self.objects_created,
            };
            resolve(&mut ctx, effect)
        }
    }

    // -----------------------------------------------------------------------------------
    // SpawnTurret
    // -----------------------------------------------------------------------------------

    #[test]
    fn spawn_turret_creates_turret_object() {
        let impact = FixedPoint::new(4_096, 0);
        let mut fixture = Fixture::new(vec![player("emi", FixedPoint::ZERO)]);

        let result = fixture.resolve_effect("emi", impact, &spawn_turret_effect());
        assert!(result.is_ok());

        let Some(turret) = fixture
            .state
            .objects
            .iter()
            .find(|object| object.kind == PersistentObjectKind::Turret)
        else {
            panic!("Emi's turret must exist after SpawnTurret resolves")
        };
        assert_eq!(turret.health, 80, "CHARACTERS.md \u{a7}3.2: 80 HP");
        assert_eq!(
            turret.turns_remaining, 3,
            "CHARACTERS.md \u{a7}3.2: 3 turns"
        );
        assert_eq!(turret.position, impact);
        assert_eq!(turret.owner_id, "emi");
        assert_eq!(fixture.objects_created.len(), 1);
    }

    #[test]
    fn spawn_turret_replaces_rather_than_accumulates() {
        let mut fixture = Fixture::new(vec![player("emi", FixedPoint::ZERO)]);
        let effect = spawn_turret_effect();

        let first_impact = FixedPoint::new(1_024, 0);
        assert!(fixture.resolve_effect("emi", first_impact, &effect).is_ok());
        let second_impact = FixedPoint::new(2_048, 0);
        assert!(
            fixture
                .resolve_effect("emi", second_impact, &effect)
                .is_ok()
        );

        let turrets: Vec<_> = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.kind == PersistentObjectKind::Turret)
            .collect();
        assert_eq!(turrets.len(), 1, "only one Emi turret may exist at a time");
        let Some(turret) = turrets.first() else {
            panic!("a turret must remain")
        };
        assert_eq!(
            turret.position, second_impact,
            "the second turret replaces the first"
        );
    }

    #[test]
    fn spawn_turret_is_scoped_per_owner() {
        let mut fixture = Fixture::new(vec![
            player("emi", FixedPoint::ZERO),
            player("aleph", FixedPoint::ZERO),
        ]);
        let effect = spawn_turret_effect();

        assert!(
            fixture
                .resolve_effect("emi", FixedPoint::new(1_000, 0), &effect)
                .is_ok()
        );
        assert!(
            fixture
                .resolve_effect("aleph", FixedPoint::new(2_000, 0), &effect)
                .is_ok()
        );

        let turrets: Vec<_> = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.kind == PersistentObjectKind::Turret)
            .collect();
        assert_eq!(
            turrets.len(),
            2,
            "different owners each get their own turret"
        );
    }

    // -----------------------------------------------------------------------------------
    // EmbedProjectile
    // -----------------------------------------------------------------------------------

    #[test]
    fn embed_projectile_creates_persistent_knife() {
        let impact = FixedPoint::new(5_000, 0);
        let mut fixture = Fixture::new(vec![player("aleph", FixedPoint::ZERO)]);

        let result = fixture.resolve_effect("aleph", impact, &embed_projectile_effect(8));
        assert!(result.is_ok());

        let knives: Vec<_> = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.kind == PersistentObjectKind::EmbeddedKnife)
            .collect();
        assert_eq!(knives.len(), 1);
        let Some(knife) = knives.first() else {
            panic!("the embedded knife must exist")
        };
        assert_eq!(knife.turns_remaining, u8::MAX, "persists until triggered");
        assert_eq!(knife.position, impact);
        assert_eq!(knife.owner_id, "aleph");
    }

    #[test]
    fn embed_projectile_evicts_oldest_when_cap_exceeded() {
        let mut fixture = Fixture::new(vec![player("aleph", FixedPoint::ZERO)]);
        let effect = embed_projectile_effect(8);

        // Nine widely-separated landing points; spacing is irrelevant to eviction, which
        // is capacity-only and never looks at position.
        let landings = [
            FixedPoint::new(0, 0),
            FixedPoint::new(100_000, 0),
            FixedPoint::new(200_000, 0),
            FixedPoint::new(300_000, 0),
            FixedPoint::new(400_000, 0),
            FixedPoint::new(500_000, 0),
            FixedPoint::new(600_000, 0),
            FixedPoint::new(700_000, 0),
            FixedPoint::new(800_000, 0),
        ];
        for landing in landings {
            assert!(fixture.resolve_effect("aleph", landing, &effect).is_ok());
        }

        let knives: Vec<_> = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.kind == PersistentObjectKind::EmbeddedKnife)
            .collect();
        assert_eq!(
            knives.len(),
            8,
            "cap of 8 must hold after a ninth knife lands"
        );

        let sequences: Vec<u32> = knives.iter().map(|knife| knife.sequence).collect();
        assert!(
            !sequences.contains(&0),
            "the oldest (sequence 0, first landing) must be evicted"
        );
        assert!(
            sequences.contains(&8),
            "the newest (sequence 8, ninth landing) must remain"
        );
    }

    #[test]
    fn embed_projectile_cap_is_scoped_per_owner() {
        let mut fixture = Fixture::new(vec![
            player("aleph-one", FixedPoint::ZERO),
            player("aleph-two", FixedPoint::ZERO),
        ]);
        let effect = embed_projectile_effect(2);

        let owner_one_landings = [
            FixedPoint::new(0, 0),
            FixedPoint::new(100_000, 0),
            FixedPoint::new(200_000, 0),
        ];
        for landing in owner_one_landings {
            assert!(
                fixture
                    .resolve_effect("aleph-one", landing, &effect)
                    .is_ok()
            );
        }

        let owner_two_landings = [
            FixedPoint::new(0, 500_000),
            FixedPoint::new(100_000, 500_000),
        ];
        for landing in owner_two_landings {
            assert!(
                fixture
                    .resolve_effect("aleph-two", landing, &effect)
                    .is_ok()
            );
        }

        let owner_one_count = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.owner_id == "aleph-one")
            .count();
        let owner_two_count = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.owner_id == "aleph-two")
            .count();
        assert_eq!(
            owner_one_count, 2,
            "aleph-one's cap of 2 must have evicted its oldest"
        );
        assert_eq!(
            owner_two_count, 2,
            "aleph-two's knives are unaffected by aleph-one's eviction"
        );
    }

    // -----------------------------------------------------------------------------------
    // ChainDetonate
    // -----------------------------------------------------------------------------------

    #[test]
    fn chain_detonate_leaves_a_solitary_landing_knife_embedded() {
        let landing = FixedPoint::new(50_000, 0);
        let mut fixture = Fixture::new(vec![player("aleph", FixedPoint::ZERO)]);

        assert!(
            fixture
                .resolve_effect("aleph", landing, &embed_projectile_effect(8))
                .is_ok()
        );
        assert!(
            fixture
                .resolve_effect("aleph", landing, &chain_detonate_effect())
                .is_ok()
        );

        let knives = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.kind == PersistentObjectKind::EmbeddedKnife)
            .count();
        assert_eq!(
            knives, 1,
            "a knife with no neighbour must stay embedded, not detonate"
        );
        assert!(fixture.terrain_ops.is_empty());
        assert!(fixture.damage.is_empty());
    }

    #[test]
    fn chain_of_three_knives_detonates_all_three() {
        // A(0) <-1.5BW-away-from-B(9000)? no(9000>6144) but within blast(10240) of B.
        // Landing C(14000) is within trigger(6144) of B(9000): 14000-9000=5000 < 6144.
        // B's detonation blast(10240) then reaches A: 9000 < 10240. A is never directly
        // reachable from C (14000 apart), only via B's propagation.
        let position_a = FixedPoint::new(0, 0);
        let position_b = FixedPoint::new(9_000, 0);
        let landing_c = FixedPoint::new(14_000, 0);

        let mut fixture = Fixture::new(vec![player("aleph", FixedPoint::new(-90_000, 0))]);
        fixture.state.terrain = terrain_mask(Material::Soil);

        let embed_effect = embed_projectile_effect(8);
        assert!(
            fixture
                .resolve_effect("aleph", position_a, &embed_effect)
                .is_ok()
        );
        assert!(
            fixture
                .resolve_effect("aleph", position_b, &embed_effect)
                .is_ok()
        );
        assert!(
            fixture
                .resolve_effect("aleph", landing_c, &embed_effect)
                .is_ok()
        );

        assert!(
            fixture
                .resolve_effect("aleph", landing_c, &chain_detonate_effect())
                .is_ok()
        );

        let remaining = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.kind == PersistentObjectKind::EmbeddedKnife)
            .count();
        assert_eq!(
            remaining, 0,
            "all three chained knives must detonate and leave play"
        );
        assert_eq!(
            fixture.terrain_ops.len(),
            3,
            "one terrain operation per detonation"
        );

        // Cell coordinates: position / POSITION_SCALE(1024), floored.
        assert_eq!(
            terrain::material_at(&fixture.state.terrain, 0, 0),
            Material::Empty
        );
        assert_eq!(
            terrain::material_at(&fixture.state.terrain, 8, 0),
            Material::Empty
        );
        assert_eq!(
            terrain::material_at(&fixture.state.terrain, 13, 0),
            Material::Empty
        );
    }

    #[test]
    fn chain_detonate_terminates_with_a_cyclic_knife_arrangement() {
        // A(0), B(5000), C(10000, landing): every pair is within the 2.5 BW blast radius
        // (10240) of every other — a proximity cycle. Without the visited-sequence guard
        // this would loop forever re-discovering the same neighbours.
        let position_a = FixedPoint::new(0, 0);
        let position_b = FixedPoint::new(5_000, 0);
        let landing_c = FixedPoint::new(10_000, 0);

        let mut fixture = Fixture::new(vec![
            player("aleph", FixedPoint::new(-90_000, 0)),
            player("defender", position_b),
        ]);

        let embed_effect = embed_projectile_effect(8);
        assert!(
            fixture
                .resolve_effect("aleph", position_a, &embed_effect)
                .is_ok()
        );
        assert!(
            fixture
                .resolve_effect("aleph", position_b, &embed_effect)
                .is_ok()
        );
        assert!(
            fixture
                .resolve_effect("aleph", landing_c, &embed_effect)
                .is_ok()
        );

        assert!(
            fixture
                .resolve_effect("aleph", landing_c, &chain_detonate_effect())
                .is_ok()
        );

        let remaining = fixture
            .state
            .objects
            .iter()
            .filter(|object| object.kind == PersistentObjectKind::EmbeddedKnife)
            .count();
        assert_eq!(
            remaining, 0,
            "the chain must still terminate with all three detonated"
        );

        // "defender" sits at B, within all three detonations' blast radius, so each of the
        // three must have contributed exactly once: not zero (a broken chain) and not more
        // than once each (a termination bug reprocessing a detonation).
        let Some(damage) = fixture.damage.get("defender") else {
            panic!("defender caught in the blast must have a damage entry")
        };
        assert_eq!(
            damage.splash, 210,
            "3 detonations x 70 damage each, no more and no less"
        );
        assert_eq!(fixture.state.player("defender").map(|p| p.health), Some(90));
    }

    #[test]
    fn chain_resolution_is_identical_across_two_runs_with_the_same_seed() {
        fn run_once() -> (
            Vec<TerrainOperation>,
            BTreeMap<String, DamageEvent>,
            Vec<PersistentObject>,
        ) {
            let position_a = FixedPoint::new(0, 0);
            let position_b = FixedPoint::new(5_000, 0);
            let landing_c = FixedPoint::new(10_000, 0);

            let mut fixture = Fixture::new(vec![
                player("aleph", FixedPoint::new(-90_000, 0)),
                player("defender", position_b),
            ]);

            let embed_effect = embed_projectile_effect(8);
            let _ = fixture.resolve_effect("aleph", position_a, &embed_effect);
            let _ = fixture.resolve_effect("aleph", position_b, &embed_effect);
            let _ = fixture.resolve_effect("aleph", landing_c, &embed_effect);
            let _ = fixture.resolve_effect("aleph", landing_c, &chain_detonate_effect());

            (
                fixture.terrain_ops.clone(),
                fixture.damage.clone(),
                fixture.state.objects.clone(),
            )
        }

        let (terrain_ops_1, damage_1, objects_1) = run_once();
        let (terrain_ops_2, damage_2, objects_2) = run_once();

        assert_eq!(
            terrain_ops_1, terrain_ops_2,
            "terrain ops must resolve in the same order both times"
        );
        assert_eq!(damage_1, damage_2);
        assert_eq!(objects_1, objects_2);
    }
}
