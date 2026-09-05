//! Effect resolution — the layer that turns a validated command into a game.
//!
//! `command.rs` decides whether an action is *permitted*. This module decides what it
//! *does*. The distinction is why 19 of 22 [`EffectKind`] variants were, for a time,
//! declared, validated, and completely inert: the command boundary was specified as
//! "validation + application" and delivered exactly that (`PROGRAM_PLAN.md` §2). As of
//! `todolist.md` P1, `command.rs::apply_ability` calls [`resolve_effect`] for every effect
//! on the resolved ability, in declaration order, after damage resolution — so every
//! variant this module can resolve is now actually reachable from a real command, not just
//! from a resolver's own unit tests.
//!
//! # The one rule
//!
//! **Every [`EffectKind`] variant must have a resolver arm.** [`resolve_effect`] matches
//! exhaustively with no `_ =>` catch-all, so adding a variant to the enum fails compilation
//! until it is resolved somewhere. That is deliberate: a silent fallthrough is precisely how
//! the inert-effect gap happened, and the compiler is a better gate than a review checklist.
//!
//! # Amortization
//!
//! Effects are grouped into families sharing their hard parts — displacement geometry,
//! target selection, object lifetime. This is what makes a new character cost ~1,000 engine
//! lines instead of ~3,000: character #10 composes existing resolvers rather than
//! re-deriving knockback for the tenth time.

pub mod attack_mods;
pub mod displacement;
pub mod objects;
pub mod relocation;
pub mod status;
pub mod support;
pub mod terrain_damage;

use crate::error::SimResult;
use crate::fixed::FixedPoint;
use crate::rng::Rng;
use crate::types::{
    DamageEvent, EffectKind, PersistentObjectChange, RandomOutcome, SimulationState, SpecialEffect,
    StatusChange, StrikeResolution, TerrainOperation,
};
use std::collections::BTreeMap;

/// Everything a resolver may read or mutate.
///
/// Passed by `&mut` rather than returned-and-merged so that effects compose in declaration
/// order and each one observes the results of those before it — Natomica's `WallImpact`
/// must see where `Push` actually left the target.
pub struct ResolveContext<'a> {
    /// Authoritative match state. Resolvers mutate this directly.
    pub state: &'a mut SimulationState,
    /// The seeded generator. The **only** entropy source; never use another.
    pub rng: &'a mut Rng,
    /// Who is acting.
    pub actor_id: &'a str,
    /// Player-selected primary target, where the ability takes one.
    pub primary_target_id: Option<&'a str>,
    /// Player-selected secondary target (Huck's Body Throw destination).
    pub secondary_target_id: Option<&'a str>,
    /// Where the attack landed. Origin of radial effects.
    pub impact_point: FixedPoint,
    /// Itemized damage and healing, keyed by player id. `BTreeMap` for deterministic
    /// iteration — a `HashMap` here would make the state hash allocator-dependent.
    pub damage: &'a mut BTreeMap<String, DamageEvent>,
    /// Terrain mutations produced by this action, in sequence order.
    pub terrain_ops: &'a mut Vec<TerrainOperation>,
    /// Persistent-object lifecycle transitions produced by this action, in exact order.
    pub object_changes: &'a mut Vec<PersistentObjectChange>,
    /// Public non-strike random outcomes produced by resolvers, in draw-site order.
    ///
    /// This records only bounded visible results. The generator state and rejected raw draws
    /// remain private authoritative details.
    pub random_outcomes: &'a mut Vec<RandomOutcome>,
    /// Terrain cells removed by this action, accumulated across every
    /// `terrain::apply_operation` call any resolver makes. Seeded by
    /// `command.rs::apply_ability` from the primary attack's own terrain removal and
    /// copied into `CommandOutcome::terrain_cells_removed` once every effect has resolved
    /// — this is what feeds the Excavator XP bonus (`docs/PROGRESSION.md` §2,
    /// `todolist.md` P2). A resolver that calls `terrain::apply_operation` must add the
    /// returned count here rather than discarding it with `let _removed = …`.
    pub terrain_cells_removed: &'a mut u32,
    /// Status lifecycle transitions produced by this action, in the order they happened.
    ///
    /// Recorded where the transition occurs, never diffed from the final status list: a
    /// status applied and expired within one turn, or a charge-based status decremented
    /// several times by one multi-strike ability, leaves no observable trace in a pre/post
    /// comparison.
    pub status_changes: &'a mut Vec<StatusChange>,
}

impl ResolveContext<'_> {
    /// Returns the itemized damage entry for `player_id`, inserting a zeroed one if this is
    /// the first effect to touch them this action.
    pub fn damage_entry(&mut self, player_id: &str) -> &mut DamageEvent {
        self.damage
            .entry(player_id.to_owned())
            .or_insert_with(|| DamageEvent {
                player_id: player_id.to_owned(),
                direct: 0,
                splash: 0,
                backlash: 0,
                hazard: 0,
                wall_impact: 0,
                healed: 0,
                was_critical: false,
                knockback: FixedPoint::ZERO,
                eliminated: false,
            })
    }

    /// Ids of every living player other than the actor, in deterministic (sorted) order.
    ///
    /// Sorted because several effects draw a random target from this list, and an unsorted
    /// candidate set would make the draw depend on storage order rather than on the seed.
    #[must_use]
    pub fn living_opponent_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .state
            .players
            .iter()
            .filter(|player| !player.is_eliminated() && player.id != self.actor_id)
            .map(|player| player.id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Next persistent-object sequence number, advancing the counter.
    ///
    /// Sequence order is what makes Aleph's dagger chains resolve identically everywhere,
    /// so it must come from here and never from insertion order.
    pub fn next_object_sequence(&mut self) -> u32 {
        let sequence = self.state.next_object_sequence;
        self.state.next_object_sequence = sequence.saturating_add(1);
        sequence
    }

    /// Next terrain-operation sequence number, advancing the counter.
    pub fn next_terrain_sequence(&mut self) -> u32 {
        let sequence = self.state.next_terrain_sequence;
        self.state.next_terrain_sequence = sequence.saturating_add(1);
        sequence
    }
}

/// Resolves one effect.
///
/// The match is **exhaustive by design** — no catch-all arm. Adding an [`EffectKind`]
/// variant breaks this build until a resolver exists for it.
///
/// # Errors
///
/// Returns [`crate::error::SimError`] when an effect's parameters are out of range or a
/// fixed-point computation would overflow. A resolver never panics on hostile input.
pub fn resolve_effect(
    ctx: &mut ResolveContext<'_>,
    effect: &SpecialEffect,
) -> SimResult<Vec<StrikeResolution>> {
    match effect.kind {
        EffectKind::Knockback
        | EffectKind::Push
        | EffectKind::Pull
        | EffectKind::Recoil
        | EffectKind::WallImpact => {
            displacement::resolve(ctx, effect)?;
            Ok(Vec::new())
        }

        EffectKind::Teleport | EffectKind::Relocate | EffectKind::Obscure => {
            relocation::resolve(ctx, effect)?;
            Ok(Vec::new())
        }

        EffectKind::SpawnTurret | EffectKind::EmbedProjectile | EffectKind::ChainDetonate => {
            objects::resolve(ctx, effect)?;
            Ok(Vec::new())
        }

        EffectKind::Chill | EffectKind::Lockdown | EffectKind::Embers => {
            status::resolve(ctx, effect)?;
            Ok(Vec::new())
        }

        EffectKind::MultiStrike => attack_mods::resolve_multi_strike_with_records(ctx, effect),

        EffectKind::GuaranteeCrit
        | EffectKind::Cluster
        | EffectKind::Return
        | EffectKind::Tunnel => {
            attack_mods::resolve(ctx, effect)?;
            Ok(Vec::new())
        }

        // Formerly resolved inline inside `command.rs`, with this arm routed to a silent
        // `Ok(())` — two resolution paths for one concept, and precisely the shape of bug
        // that left the other 19 kinds inert. `command.rs` no longer resolves these three
        // itself; `resolve::support` is now the only place they resolve (`todolist.md` P1).
        EffectKind::Heal | EffectKind::HealthTransfer | EffectKind::SelfDamage => {
            support::resolve(ctx, effect)?;
            Ok(Vec::new())
        }
    }
}
