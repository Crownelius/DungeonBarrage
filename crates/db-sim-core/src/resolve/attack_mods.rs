//! Resolvers for [`EffectKind::MultiStrike`], [`EffectKind::GuaranteeCrit`],
//! [`EffectKind::Cluster`], [`EffectKind::Return`], and [`EffectKind::Tunnel`].
//!
//! # Roster status — read this before trusting any "Karl" example below
//!
//! **None of `MultiStrike`, `Cluster`, `Return`, or `Tunnel` is attached to any character
//! in the current roster.** `character.rs` was grepped for all four before writing this
//! file; zero hits. Karl's triple-strike basic (`CHARACTERS.md` §3.3: three attacks at
//! 24%, 74% on crit, each rolling its crit independently) is fully implemented already —
//! but through `AbilityDefinition::strikes_per_turn` plus generic per-strike crit rolling
//! in `command.rs::resolve_ability`, not through `EffectKind::MultiStrike`. Only
//! `GuaranteeCrit` has a real caller: Karl's Special, `KARL_GUARANTEE_CRIT` (magnitude
//! `3`, "next 3 attacks crit automatically").
//!
//! This matters for what "read what the roster writes" can mean here: for `GuaranteeCrit`
//! there is roster data to read, and this file reads it exactly. For the other four kinds
//! there is none — `mod.rs`'s dispatch is still exhaustive over all 21 [`EffectKind`]
//! variants, so a resolver must exist and be correct for whenever a character kit does
//! attach one, but this file is establishing the calling convention, not transcribing an
//! existing one. Every convention chosen below is called out explicitly as a judgement
//! call rather than presented as roster fact.
//!
//! # The `ResolveContext` gap this file works around
//!
//! [`ResolveContext`] carries no reference to the [`crate::types::AbilityDefinition`]
//! being resolved — no damage percent, no crit percent, no crit chance. That is fine for
//! `GuaranteeCrit` (it only touches a status, no damage math), but `MultiStrike`
//! fundamentally needs to know what "the attack" it is repeating deals, crit and uncrit.
//! Reproducing Karl's own worked example (`CHARACTERS.md` §3.3: 24% / 74% / 2,000 bp / 3
//! strikes) needs **four** independent numbers, and [`SpecialEffect`] has exactly three
//! numeric fields (`magnitude`, `magnitude_secondary`, `duration_turns`). Rather than add
//! a field to the frozen `types.rs` contract (not this file's authority) or guess a
//! plausible-looking single number and silently drop the rest (exactly what
//! `docs/MODULE_OWNERSHIP.md` rule 5 warns against), `MultiStrike` packs two of its four
//! numbers into `magnitude_secondary` — documented in full at
//! [`multi_strike_damage_percents`]. This is reported as a judgement call in this task's
//! structured output, not a silent invention.
//!
//! # Conventions established by this file (no character reads these yet)
//!
//! - **`MultiStrike`**: `magnitude` = strike count. `magnitude_secondary` = per-strike
//!   uncrit/crit damage, decimal-packed (see [`multi_strike_damage_percents`]).
//!   `duration_turns` = crit chance per strike, as a whole percent (`0..=100`) rather than
//!   `AbilityDefinition`'s basis points, because `duration_turns` is a `u8` and
//!   `0..=10_000` does not fit. `MultiStrike` is instantaneous (resolves fully within the
//!   turn it fires), so `duration_turns`'s ordinary "turns this persists" meaning is
//!   otherwise unused here — the same reuse `Cluster` makes below.
//! - **`GuaranteeCrit`**: `magnitude` = remaining forced-crit count (already how
//!   `character.rs` encodes `KARL_GUARANTEE_CRIT`; this file only decides *representation*
//!   of the resulting status, not the magnitude's meaning). See
//!   [`resolve_guarantee_crit`] for why the resulting status stores the countdown in its
//!   own `magnitude` rather than `turns_remaining`.
//! - **`Cluster`**: `magnitude` = submunition count. `magnitude_secondary` = per-submunition
//!   damage, percent-of-[`BASE_ATTACK`] (matches `types.rs`'s own stated convention for
//!   `SpecialEffect::magnitude`: "percent-of-BASE_ATTACK for scaled damage" — applied here
//!   to the secondary field because the primary is spent on count, exactly as
//!   `MultiStrike` does). `duration_turns` = per-target damage cap, percent-of-`BASE_ATTACK`,
//!   reused for the same instantaneous-effect reason as `MultiStrike`. Anchor figures are
//!   `ARSENAL.md`'s retired Cinder Cluster (5 bomblets, 16 each, 56 cap) — the weapon this
//!   effect kind was clearly cut from, per its own doc comment in `types.rs`: "Splits into
//!   submunitions."
//! - **`Return`**: `magnitude` = per-pass damage, percent-of-`BASE_ATTACK`.
//!   `magnitude_secondary` = per-target cap, percent-of-`BASE_ATTACK`. Anchor figures are
//!   the retired Returning Boomerang (18 per pass, 32 cap).
//! - **`Tunnel`**: `magnitude` = maximum bore distance in whole terrain cells.
//!   `magnitude_secondary` = detonation damage, percent-of-`BASE_ATTACK`. See
//!   [`resolve_tunnel`] for the direction judgement call.
//!
//! # Guardrail 3
//!
//! `ARSENAL.md`'s "Balance guardrails" §3: "Multi-hit weapons enforce their per-target cap
//! after all subprojectile contacts; separate impact events cannot bypass it." `Cluster`
//! and `Return` both compute every sub-hit's raw damage into a per-target accumulator
//! first and apply the capped total in one [`deal_damage`] call per target, only after
//! every submunition/pass has been accounted for — never per-hit, which is exactly the
//! bypass the guardrail names.

use crate::error::{SimError, SimResult};
use crate::fixed::{
    BODY_WIDTH, FixedPoint, POSITION_SCALE, distance_squared, scale, within_radius,
};
use crate::terrain;
use crate::types::{
    BASE_ATTACK, EffectKind, Material, MaterialMask, SimulationState, SpecialEffect, StatusEffect,
    TerrainOperation, TerrainShape,
};
use std::collections::BTreeMap;

use super::ResolveContext;

/// Mirrors `command.rs`'s private `CRIT_ROLL_BOUND` (and `fixed::BASIS_POINTS`: 10,000 =
/// 100.00%). Duplicated locally rather than imported — `command.rs` does not expose it,
/// and this file may not reach across the module-ownership boundary for anything but the
/// shared, frozen `types.rs`/`fixed.rs` contract (`docs/MODULE_OWNERSHIP.md` rule 1).
const CRIT_ROLL_BOUND: u32 = 10_000;

/// Decimal packing base for [`multi_strike_damage_percents`]. See that function's doc
/// comment for the full convention.
const MULTI_STRIKE_CRIT_PERCENT_SCALE: i32 = 1_000;

/// Small per-submunition crater radius. `ARSENAL.md`'s retired Cinder Cluster removes a
/// 0.7 body-width-radius crater per bomblet; `BODY_WIDTH` is 4 terrain cells, so 0.7 BW is
/// 2.8 cells, rounded to the nearest whole cell.
const CLUSTER_SUBMUNITION_CRATER_RADIUS_CELLS: u16 = 3;

/// Per-ring spread distance for [`cluster_submunition_landing`]. Deliberately smaller
/// than [`BODY_WIDTH`] so a target standing at the impact point is within `BODY_WIDTH` of
/// *every* ring-0 landing point, including the diagonal ones (offset magnitude is
/// `step * sqrt(2)`, and `2 * POSITION_SCALE * sqrt(2)` is about `2,896`, comfortably
/// under `BODY_WIDTH`'s `4,096`) — otherwise submunition spread geometry alone could let a
/// tightly grouped target dodge some of them, which "splits into submunitions" does not
/// describe.
const CLUSTER_RING_STEP: i32 = 2 * POSITION_SCALE;

/// Bounds worst-case per-impact work for `Tunnel`, the same way
/// `ProjectileAttack::max_ticks` bounds a projectile's flight. An authored `magnitude` is
/// reviewed content, not hostile input, but nothing requires trusting a single malformed
/// ability definition to walk an unbounded run of terrain cells.
const TUNNEL_MAX_BORE_CELLS: i32 = 64;

/// Bore-path capsule radius for `Tunnel`'s traversal.
const TUNNEL_BORE_RADIUS_CELLS: u16 = 1;

/// Detonation crater radius for `Tunnel`'s stopping point — slightly larger than the bore
/// itself, since the bore is "boring through" and the stop is "detonating".
const TUNNEL_DETONATION_RADIUS_CELLS: u16 = 2;

/// Resolves an attack_mods-family effect: [`EffectKind::MultiStrike`],
/// [`EffectKind::GuaranteeCrit`], [`EffectKind::Cluster`], [`EffectKind::Return`], or
/// [`EffectKind::Tunnel`].
///
/// # Errors
///
/// Returns an error when a required target is absent, does not resolve to a living player
/// in `ctx.state`, or a fixed-point computation would overflow. `resolve_effect` in
/// `mod.rs` only ever routes these five kinds here, so the wildcard arm below is
/// defensive rather than a real code path (mirrors `status::resolve`'s own defensive arm).
pub fn resolve(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    match effect.kind {
        EffectKind::MultiStrike => resolve_multi_strike(ctx, effect),
        EffectKind::GuaranteeCrit => resolve_guarantee_crit(ctx, effect),
        EffectKind::Cluster => resolve_cluster(ctx, effect),
        EffectKind::Return => resolve_return(ctx, effect),
        EffectKind::Tunnel => resolve_tunnel(ctx, effect),
        _ => Err(SimError::OutOfRange {
            field: "effect.kind (not an attack_mods-family effect)",
        }),
    }
}

// ---------------------------------------------------------------------------------------
// MultiStrike
// ---------------------------------------------------------------------------------------

/// MultiStrike: resolves the same attack `effect.magnitude` times against
/// `ctx.primary_target_id`, each strike rolling its own crit independently from
/// `ctx.rng` — *unless* the target carries an active [`EffectKind::GuaranteeCrit`] status
/// with a positive remaining count, in which case this strike is forced to crit without
/// touching the RNG, and the status's count is consumed by one
/// ([`consume_guarantee_crit`]). This is how Karl's Feeding Frenzy feeds his own
/// triple-strike, satisfying "make `MultiStrike` honour [`GuaranteeCrit`]".
///
/// Every strike's hit-point value (uncrit or crit, per [`multi_strike_damage_percents`])
/// is accumulated into `ctx.damage_entry(target).direct` via [`deal_damage`];
/// `was_critical` is set if *any* strike crit and is never unset by a later uncrit strike
/// (`deal_damage` only ever ORs it).
fn resolve_multi_strike(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let Some(target_id) = ctx.primary_target_id else {
        return Err(SimError::OutOfRange {
            field: "multi_strike.primary_target_id",
        });
    };

    let strikes = multi_strike_strike_count(effect);
    let (uncrit_percent, crit_percent) = multi_strike_damage_percents(effect);
    let uncrit_hp = percent_to_hp(uncrit_percent);
    let crit_hp = percent_to_hp(crit_percent);
    let crit_chance_basis_points = multi_strike_crit_chance_basis_points(effect);

    for _ in 0..strikes {
        let forced = consume_guarantee_crit(ctx, target_id);
        // Short-circuiting `||` is load-bearing: a forced strike must not also consume an
        // RNG draw, the same "consumption proportional to what actually rolled" rule
        // `command.rs::roll_crit` documents for its own zero-chance case.
        let is_crit = forced || ctx.rng.bounded(CRIT_ROLL_BOUND) < crit_chance_basis_points;
        let hp = if is_crit { crit_hp } else { uncrit_hp };
        deal_damage(ctx, target_id, hp, is_crit);
    }

    Ok(())
}

/// Strike count from `effect.magnitude`. Non-positive values clamp to `1` — an ability
/// author attaching a `MultiStrike` effect almost certainly means "at least one hit", not
/// "zero hits from a MultiStrike effect", and a silent zero-strike no-op would be a much
/// more confusing failure mode than a single strike.
fn multi_strike_strike_count(effect: &SpecialEffect) -> u32 {
    u32::try_from(effect.magnitude.max(1)).unwrap_or(1)
}

/// Decodes `effect.magnitude_secondary` into `(uncrit_percent, crit_percent)`, both
/// percent-of-[`BASE_ATTACK`].
///
/// Packed as `uncrit_percent + crit_percent * 1_000` (decimal, not bit-shifted, so the
/// raw stored integer is legible by inspection: Karl's 24% / 74% packs to
/// `24 + 74_000 == 74_024`). Both percentages are assumed under 1,000% of `BASE_ATTACK` —
/// generous headroom for any plausible ability. See the module doc comment's "The
/// `ResolveContext` gap this file works around" section for why packing was necessary at
/// all instead of a fourth field.
fn multi_strike_damage_percents(effect: &SpecialEffect) -> (i32, i32) {
    let packed = effect.magnitude_secondary.max(0);
    let crit_percent = packed
        .checked_div(MULTI_STRIKE_CRIT_PERCENT_SCALE)
        .unwrap_or(0);
    let uncrit_percent = packed
        .checked_rem(MULTI_STRIKE_CRIT_PERCENT_SCALE)
        .unwrap_or(0);
    (uncrit_percent, crit_percent)
}

/// Crit chance for each independent `MultiStrike` roll, in basis points.
///
/// `effect.duration_turns` carries this as a whole percent (`0..=100`) — a `u8` cannot
/// hold `AbilityDefinition::crit_chance_basis_points`'s native `0..=10_000` range. Karl's
/// own figure (2,000 bp) is exactly 20%, so the launch-roster anchor round-trips with no
/// precision loss; a future ability needing sub-percent crit chance specifically on a
/// `MultiStrike` effect will need a wider field than this convention offers.
fn multi_strike_crit_chance_basis_points(effect: &SpecialEffect) -> u32 {
    u32::from(effect.duration_turns).saturating_mul(100)
}

/// If `target_id` carries an active [`EffectKind::GuaranteeCrit`] status with a positive
/// remaining count, consumes one count (removing the status once exhausted) and returns
/// `true` — this strike is forced to crit, no RNG draw needed. Otherwise returns `false`
/// and leaves the target's statuses untouched.
fn consume_guarantee_crit(ctx: &mut ResolveContext<'_>, target_id: &str) -> bool {
    let Some(target) = ctx.state.player_mut(target_id) else {
        return false;
    };

    // Scoped so the mutable borrow of `target.statuses` (via `status`) ends before
    // `target.statuses.retain` needs its own mutable borrow below.
    let exhausted = {
        let Some(status) = target
            .statuses
            .iter_mut()
            .find(|status| status.kind == EffectKind::GuaranteeCrit)
        else {
            return false;
        };
        if status.magnitude <= 0 {
            return false;
        }
        // Checked, not saturating: `magnitude` was just proven positive above, so this
        // cannot underflow. `checked_sub` still guards the (unreachable) alternative
        // rather than assuming the invariant holds.
        status.magnitude = status.magnitude.checked_sub(1).unwrap_or(0);
        status.magnitude <= 0
    };

    if exhausted {
        target
            .statuses
            .retain(|status| status.kind != EffectKind::GuaranteeCrit);
    }
    true
}

// ---------------------------------------------------------------------------------------
// GuaranteeCrit
// ---------------------------------------------------------------------------------------

/// GuaranteeCrit: marks `ctx.primary_target_id` so the next `effect.magnitude` attacks
/// against them crit automatically (Karl's Feeding Frenzy, `CHARACTERS.md` §3.3's Special;
/// `character.rs`'s `KARL_GUARANTEE_CRIT` sets `magnitude: 3`).
///
/// Modelled as a [`StatusEffect`] whose **`magnitude` is a live countdown of remaining
/// forced crits**, decremented by [`consume_guarantee_crit`] each time `MultiStrike`
/// consumes one and removed once it hits zero — rather than gating on `turns_remaining`,
/// because Feeding Frenzy's duration is stated in *attacks affected*, not turns elapsed:
/// "the next 3 attacks", not "for 3 turns". `turns_remaining` is still set, to
/// [`u8::MAX`], for the same reason `PersistentObject::turns_remaining` uses that
/// sentinel ("never expires on its own"): it must not lapse from ordinary turn-based decay
/// before its real, count-based expiry does. `status::tick_statuses` will still erode it
/// by one per turn if nothing ever consumes it, so a match running past 255 turns would
/// eventually clear a stale, never-triggered mark — acceptable (no match plausibly runs
/// that long) and documented rather than silently relied upon.
///
/// Reapplying while a `GuaranteeCrit` status is already active **replaces** the remaining
/// count rather than summing it — the same refresh-not-stack rule
/// `resolve::status::apply_status` enforces for its own three status-family effects
/// (`ARSENAL.md` guardrail 4). Reimplemented locally rather than called, because that
/// helper is private to `status.rs` and this file may not reach into a sibling's private
/// surface (`docs/MODULE_OWNERSHIP.md` rule 1: one file, one owner).
fn resolve_guarantee_crit(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let Some(target_id) = ctx.primary_target_id else {
        return Err(SimError::OutOfRange {
            field: "guarantee_crit.primary_target_id",
        });
    };
    let Some(target) = ctx.state.player_mut(target_id) else {
        return Err(SimError::UnknownDefinition);
    };

    let status = StatusEffect {
        kind: EffectKind::GuaranteeCrit,
        magnitude: effect.magnitude.max(0),
        turns_remaining: u8::MAX,
    };

    if let Some(existing) = target
        .statuses
        .iter_mut()
        .find(|existing| existing.kind == EffectKind::GuaranteeCrit)
    {
        *existing = status;
    } else {
        target.statuses.push(status);
    }
    // `PlayerState.statuses` must stay sorted by `kind` at all times — the canonical hash
    // encoding depends on it (`status.rs`'s own `apply_status` doc comment states the
    // same invariant; re-affirmed here since this file cannot call that private helper).
    target.statuses.sort_by_key(|s| s.kind);

    Ok(())
}

// ---------------------------------------------------------------------------------------
// Cluster
// ---------------------------------------------------------------------------------------

/// Cluster: splits into `effect.magnitude` submunitions, each landing at its own
/// deterministically spread point around `ctx.impact_point`
/// ([`cluster_submunition_landing`]), each carving its own small crater
/// ([`CLUSTER_SUBMUNITION_CRATER_RADIUS_CELLS`]), and each dealing
/// `effect.magnitude_secondary` percent-of-[`BASE_ATTACK`] to the nearest living opponent
/// within one body width of its landing point ([`nearest_living_target`], mirroring
/// `command.rs`'s own direct-hit selection rule for a ballistic impact).
///
/// Guardrail 3: every submunition's damage is accumulated per target *before* any health
/// is touched, then clamped to `effect.duration_turns` percent-of-`BASE_ATTACK` (the
/// per-target cap) and applied in one [`deal_damage`] call per target — never per-hit, so
/// five small submunitions landing on one target cannot add up to more than the cap even
/// though no single submunition exceeds it on its own.
fn resolve_cluster(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let count = u32::try_from(effect.magnitude.max(0)).unwrap_or(0);
    let per_submunition_hp = percent_to_hp(effect.magnitude_secondary);
    let cap_hp = percent_to_hp(i32::from(effect.duration_turns));

    let impact = ctx.impact_point;
    let opponent_ids = ctx.living_opponent_ids();
    let mut raw_by_target: BTreeMap<String, u32> = BTreeMap::new();

    for index in 0..count {
        let landing = cluster_submunition_landing(impact, index);

        let crater_op = TerrainOperation {
            sequence: ctx.next_terrain_sequence(),
            shape: TerrainShape::SubtractCircle {
                center: landing,
                radius_cells: CLUSTER_SUBMUNITION_CRATER_RADIUS_CELLS,
            },
            material_mask: MaterialMask::SOFT,
        };
        // Cell count discarded: `ResolveContext` has nowhere to accumulate it until
        // `resolve_effect` is wired into `command.rs`. Tracked in PROGRAM_PLAN §6 —
        // `CommandOutcome::terrain_cells_removed` feeds the Excavator XP bonus.
        let _removed = terrain::apply_operation(&mut ctx.state.terrain, &crater_op);
        ctx.terrain_ops.push(crater_op);

        if let Some(target_id) = nearest_living_target(ctx.state, &opponent_ids, landing) {
            let counter = raw_by_target.entry(target_id).or_insert(0);
            *counter = counter.saturating_add(u32::from(per_submunition_hp));
        }
    }

    for (target_id, raw) in raw_by_target {
        let capped = raw.min(u32::from(cap_hp));
        let capped_hp = u16::try_from(capped).unwrap_or(u16::MAX);
        deal_damage(ctx, &target_id, capped_hp, false);
    }

    Ok(())
}

/// One of eight fixed compass directions, selected by `direction_index % 8`. A `match`
/// rather than an array-plus-index, so there is nothing here for `indexing_slicing` to
/// deny and every branch is total.
const fn cluster_spread_direction(direction_index: u32) -> (i32, i32) {
    match direction_index {
        0 => (1, 0),
        1 => (1, 1),
        2 => (0, 1),
        3 => (-1, 1),
        4 => (-1, 0),
        5 => (-1, -1),
        6 => (0, -1),
        // Covers 7 and, defensively, any value `checked_rem(8)` could not otherwise
        // produce.
        _ => (1, -1),
    }
}

/// Deterministic landing point for submunition `index`, spread around `impact` in
/// concentric eight-direction rings (`index`s 0-7 on ring 0, 8-15 on ring 1, and so on).
///
/// **Judgement call, documented rather than silently assumed**: a fixed compass pattern
/// was chosen over drawing from `ctx.rng`. The brief allows either ("If you use the rng,
/// that is fine; if you use a fixed pattern, that is also fine. Document which and why.")
/// — a fixed pattern was picked because it makes every submunition's landing point a pure
/// function of `impact` and `index`, which is simpler to reason about and to test than
/// replaying an RNG sequence, while satisfying the same determinism requirement equally
/// well (both are pure functions of already-deterministic inputs).
fn cluster_submunition_landing(impact: FixedPoint, index: u32) -> FixedPoint {
    let direction_index = index.checked_rem(8).unwrap_or(0);
    let ring = index.checked_div(8).unwrap_or(0);
    let (dx, dy) = cluster_spread_direction(direction_index);

    let ring_scale = i32::try_from(ring).unwrap_or(i32::MAX).saturating_add(1);
    let step = CLUSTER_RING_STEP.saturating_mul(ring_scale);
    let offset_x = dx.saturating_mul(step);
    let offset_y = dy.saturating_mul(step);

    FixedPoint::new(
        impact.x.saturating_add(offset_x),
        impact.y.saturating_add(offset_y),
    )
}

/// The nearest living opponent in `candidate_ids` within one body width of `point`, or
/// [`None`] if nothing qualifies. Mirrors `command.rs::resolve_ability`'s own direct-hit
/// selection for a projectile impact (nearest living, non-actor character within
/// `BODY_WIDTH`), reimplemented locally because that logic is private to `command.rs`.
///
/// `candidate_ids` is expected to already be sorted and actor-excluded (i.e.
/// `ctx.living_opponent_ids()`'s own contract), so ties resolve to the lexicographically
/// smallest id — deterministic, matching `Iterator::min_by_key`'s documented "first
/// element wins ties" behaviour.
fn nearest_living_target(
    state: &SimulationState,
    candidate_ids: &[String],
    point: FixedPoint,
) -> Option<String> {
    candidate_ids
        .iter()
        .filter(|id| {
            state
                .player(id.as_str())
                .is_some_and(|player| within_radius(player.position, point, BODY_WIDTH))
        })
        .min_by_key(|id| {
            state
                .player(id.as_str())
                .map_or(i64::MAX, |player| distance_squared(player.position, point))
        })
        .cloned()
}

// ---------------------------------------------------------------------------------------
// Return
// ---------------------------------------------------------------------------------------

/// Return: an outbound-and-return path, always exactly two passes — the shape the effect
/// kind is named for. The outbound pass hits `ctx.primary_target_id`; the return pass
/// hits `ctx.secondary_target_id` if the player chose a second target, or **re-hits the
/// primary target** otherwise. Re-hitting the same target on both passes is the ordinary
/// case (a boomerang thrown at, and returning past, the same spot can clip the same
/// target twice) and is exactly the scenario guardrail 3's cap exists for.
///
/// Both passes' raw damage (`effect.magnitude` percent-of-[`BASE_ATTACK`] each) is
/// accumulated per target before the per-target cap (`effect.magnitude_secondary`
/// percent-of-`BASE_ATTACK`) is applied — one [`deal_damage`] call per target, after both
/// passes are accounted for, never per-pass.
fn resolve_return(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let Some(primary_id) = ctx.primary_target_id else {
        return Err(SimError::OutOfRange {
            field: "return.primary_target_id",
        });
    };
    let return_id = ctx.secondary_target_id.unwrap_or(primary_id);

    let per_pass_hp = percent_to_hp(effect.magnitude);
    let cap_hp = percent_to_hp(effect.magnitude_secondary);

    let mut raw_by_target: BTreeMap<String, u32> = BTreeMap::new();
    for pass_target in [primary_id, return_id] {
        let counter = raw_by_target.entry(pass_target.to_owned()).or_insert(0);
        *counter = counter.saturating_add(u32::from(per_pass_hp));
    }

    for (target_id, raw) in raw_by_target {
        let capped = raw.min(u32::from(cap_hp));
        let capped_hp = u16::try_from(capped).unwrap_or(u16::MAX);
        deal_damage(ctx, &target_id, capped_hp, false);
    }

    Ok(())
}

// ---------------------------------------------------------------------------------------
// Tunnel
// ---------------------------------------------------------------------------------------

/// Tunnel: bores forward from `ctx.impact_point` through soil and wood, up to
/// `effect.magnitude` cells (capped at [`TUNNEL_MAX_BORE_CELLS`]), stopping at the first
/// [`Material::ReinforcedStone`] cell it cannot penetrate, then detonates at the stopping
/// point — a bore-path capsule (soft materials only) plus a slightly larger crater, and
/// `effect.magnitude_secondary` percent-of-[`BASE_ATTACK`] damage to
/// `ctx.primary_target_id` if one was given (no target is not an error: a tunnel that
/// detonates in open ground near nobody is a legal, if wasted, cast).
///
/// **Judgement call, documented rather than guessed around**: [`ResolveContext`] carries
/// no launch angle or direction, only `impact_point`. `Cluster` and `Return` above
/// sidestep this because their geometry is symmetric or target-anchored; `Tunnel` cannot,
/// because "bores through terrain" is inherently directional. This resolver always bores
/// along world +X (rightward), which is sufficient to prove and test the
/// stop-at-reinforced-stone mechanic the brief asks for, but is not a direction-aware
/// tunnel. A faithful implementation needs the firing angle threaded through
/// `ResolveContext` — flagged for the integrator rather than invented here.
fn resolve_tunnel(ctx: &mut ResolveContext<'_>, effect: &SpecialEffect) -> SimResult<()> {
    let max_distance_cells = effect.magnitude.clamp(0, TUNNEL_MAX_BORE_CELLS);
    let (start_x, start_y) = ctx.impact_point.to_cells();

    let mut stop_x = start_x;
    for _ in 0..max_distance_cells {
        let next_x = stop_x.saturating_add(1);
        let material = terrain::material_at(&ctx.state.terrain, next_x, start_y);
        if material == Material::ReinforcedStone {
            // Cannot penetrate; detonate at the last cell actually bored to, not here.
            break;
        }
        stop_x = next_x;
    }

    let Some(stop_point) = FixedPoint::from_cells(stop_x, start_y) else {
        return Err(SimError::Overflow {
            context: "tunnel.stop_point",
        });
    };

    // Bore out the path travelled. Soft materials only (`MaterialMask::SOFT`) — matching
    // every other launch-roster ability; `MaterialMask::ALL` is reserved for the Breach
    // Pick per `command.rs`'s own module doc comment, and nothing here asks for it.
    let bore_op = TerrainOperation {
        sequence: ctx.next_terrain_sequence(),
        shape: TerrainShape::SubtractCapsule {
            start: ctx.impact_point,
            end: stop_point,
            radius_cells: TUNNEL_BORE_RADIUS_CELLS,
        },
        material_mask: MaterialMask::SOFT,
    };
    // Cell count discarded: `ResolveContext` has nowhere to accumulate it until
    // `resolve_effect` is wired into `command.rs`. Tracked in PROGRAM_PLAN §6 —
    // `CommandOutcome::terrain_cells_removed` feeds the Excavator XP bonus.
    let _removed = terrain::apply_operation(&mut ctx.state.terrain, &bore_op);
    ctx.terrain_ops.push(bore_op);

    let detonation_op = TerrainOperation {
        sequence: ctx.next_terrain_sequence(),
        shape: TerrainShape::SubtractCircle {
            center: stop_point,
            radius_cells: TUNNEL_DETONATION_RADIUS_CELLS,
        },
        material_mask: MaterialMask::SOFT,
    };
    // Cell count discarded: `ResolveContext` has nowhere to accumulate it until
    // `resolve_effect` is wired into `command.rs`. Tracked in PROGRAM_PLAN §6 —
    // `CommandOutcome::terrain_cells_removed` feeds the Excavator XP bonus.
    let _removed = terrain::apply_operation(&mut ctx.state.terrain, &detonation_op);
    ctx.terrain_ops.push(detonation_op);

    if let Some(target_id) = ctx.primary_target_id {
        let hp = percent_to_hp(effect.magnitude_secondary);
        deal_damage(ctx, target_id, hp, false);
    }

    Ok(())
}

// ---------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------

/// Converts a percent-of-[`BASE_ATTACK`] magnitude into hit points — the same convention
/// `command.rs::magnitude_percent_to_hp` uses (duplicated locally; that function is
/// private to `command.rs`) and that `types.rs` documents directly on
/// `SpecialEffect::magnitude`: "percent-of-BASE_ATTACK for scaled damage". `None` (only
/// reachable via genuine `i32` overflow, not credible for any real percentage) degrades to
/// zero rather than wrapping — a wrapped damage value would silently become a heal.
fn percent_to_hp(percent: i32) -> u16 {
    scale(BASE_ATTACK, percent, 100)
        .and_then(|value| u16::try_from(value).ok())
        .unwrap_or(0)
}

/// Applies `amount` hit points of direct damage to `target_id`: reduces health
/// (saturating at zero), records the *actual* amount landed — capped at pre-mutation
/// health, so an overkill hit reports true HP lost rather than the raw damage figure,
/// matching `command.rs::deal_damage`'s own rule — into the itemized damage entry, and
/// ORs `is_critical` into `was_critical` (never un-flags an action a previous strike this
/// turn already marked critical).
///
/// A missing `target_id` is a no-op, not a panic — this crate never panics on data it did
/// not itself validate as present.
fn deal_damage(
    ctx: &mut ResolveContext<'_>,
    target_id: &str,
    amount: u16,
    is_critical: bool,
) -> u16 {
    if amount == 0 {
        return 0;
    }

    // Scoped so the mutable borrow of `ctx.state` (via `target`) ends before
    // `ctx.damage_entry` needs to borrow all of `ctx` again.
    let (actual, eliminated_now) = {
        let Some(target) = ctx.state.player_mut(target_id) else {
            return 0;
        };
        let actual = amount.min(target.health);
        target.health = target.health.saturating_sub(amount);
        (actual, target.is_eliminated())
    };

    let entry = ctx.damage_entry(target_id);
    entry.direct = entry.direct.saturating_add(actual);
    entry.was_critical = entry.was_critical || is_critical;
    entry.eliminated = entry.eliminated || eliminated_now;

    actual
}

#[cfg(test)]
// A fixture invariant that must hold is stated with `let ... else { panic!() }`, matching
// the precedent in `command.rs::tests` and `status.rs::tests`, because it documents the
// expectation better than threading a `Result` through every test. Production code in
// this module remains panic-free.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::types::{
        Appearance, DamageEvent, EffectTrigger, MatchPhase, PersistentObject, PlayerState,
        TerrainMask,
    };

    // -------------------------------------------------------------------------------
    // Fixtures
    // -------------------------------------------------------------------------------

    fn make_player(id: &str, position: FixedPoint, health: u16) -> PlayerState {
        PlayerState {
            id: id.to_string(),
            team: 0,
            health,
            max_health: health,
            position,
            character_id: "test-character".to_string(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn test_state(players: Vec<PlayerState>) -> SimulationState {
        SimulationState {
            blocks: Vec::new(),
            simulation_version: 2,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::Resolution,
            active_player_id: "actor".to_string(),
            wind_per_tick: 0,
            movement_remaining: 0,
            has_attacked_this_turn: false,
            terrain: TerrainMask {
                width: 40,
                height: 40,
                cells: vec![0u8; 1600],
            },
            players,
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 1,
        }
    }

    /// Bundles the owned pieces a `ResolveContext` borrows from, so each test can build a
    /// fresh context without repeating every field. Mirrors `status.rs::tests::Harness`.
    struct Harness {
        state: SimulationState,
        rng: Rng,
        damage: BTreeMap<String, DamageEvent>,
        terrain_cells_removed: u32,
        terrain_ops: Vec<TerrainOperation>,
        objects_created: Vec<PersistentObject>,
    }

    impl Harness {
        fn new(players: Vec<PlayerState>, seed: u64) -> Self {
            Self {
                state: test_state(players),
                rng: Rng::from_state(seed),
                damage: BTreeMap::new(),
                terrain_cells_removed: 0,
                terrain_ops: Vec::new(),
                objects_created: Vec::new(),
            }
        }

        fn ctx<'a>(
            &'a mut self,
            actor_id: &'a str,
            primary_target_id: Option<&'a str>,
            secondary_target_id: Option<&'a str>,
            impact_point: FixedPoint,
        ) -> ResolveContext<'a> {
            ResolveContext {
                state: &mut self.state,
                rng: &mut self.rng,
                actor_id,
                primary_target_id,
                secondary_target_id,
                impact_point,
                damage: &mut self.damage,
                terrain_cells_removed: &mut self.terrain_cells_removed,
                terrain_ops: &mut self.terrain_ops,
                objects_created: &mut self.objects_created,
            }
        }
    }

    fn multi_strike_effect(
        strike_count: i32,
        uncrit_percent: i32,
        crit_percent: i32,
        crit_chance_percent: u8,
    ) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::MultiStrike,
            magnitude: strike_count,
            magnitude_secondary: uncrit_percent
                .saturating_add(crit_percent.saturating_mul(MULTI_STRIKE_CRIT_PERCENT_SCALE)),
            duration_turns: crit_chance_percent,
        }
    }

    fn guarantee_crit_effect(magnitude: i32) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnFire,
            kind: EffectKind::GuaranteeCrit,
            magnitude,
            magnitude_secondary: 0,
            duration_turns: 0,
        }
    }

    fn cluster_effect(
        submunition_count: i32,
        per_submunition_percent: i32,
        cap_percent: u8,
    ) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Cluster,
            magnitude: submunition_count,
            magnitude_secondary: per_submunition_percent,
            duration_turns: cap_percent,
        }
    }

    fn return_effect(per_pass_percent: i32, cap_percent: i32) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnImpact,
            kind: EffectKind::Return,
            magnitude: per_pass_percent,
            magnitude_secondary: cap_percent,
            duration_turns: 0,
        }
    }

    fn tunnel_effect(max_bore_cells: i32, detonation_percent: i32) -> SpecialEffect {
        SpecialEffect {
            trigger: EffectTrigger::OnFire,
            kind: EffectKind::Tunnel,
            magnitude: max_bore_cells,
            magnitude_secondary: detonation_percent,
            duration_turns: 0,
        }
    }

    fn damage_of<'a>(harness: &'a Harness, player_id: &str) -> Option<&'a DamageEvent> {
        harness.damage.get(player_id)
    }

    fn statuses_of<'a>(state: &'a SimulationState, player_id: &str) -> &'a [StatusEffect] {
        let Some(player) = state.player(player_id) else {
            panic!("fixture invariant: player `{player_id}` must exist");
        };
        &player.statuses
    }

    // -------------------------------------------------------------------------------
    // MultiStrike
    // -------------------------------------------------------------------------------

    #[test]
    fn multi_strike_three_uncrit_strikes_total_karls_72_percent() {
        // Zero crit chance: every strike is forced uncrit, isolating the "resolves the
        // attack N times and accumulates" behaviour from crit rolling.
        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            1,
        );
        let effect = multi_strike_effect(3, 24, 74, 0);
        let mut ctx = harness.ctx("actor", Some("target"), None, FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let Some(event) = damage_of(&harness, "target") else {
            panic!("target must have taken damage");
        };
        assert_eq!(
            event.direct, 72,
            "three strikes at 24% uncrit must total 72, not one strike's 24"
        );
        assert!(!event.was_critical);
    }

    #[test]
    fn multi_strike_each_strike_rolls_crit_independently() {
        // Search for a seed where, at Karl's own 20% (2,000 bp) chance, exactly one of
        // three independent rolls crits — proving the roll happens per-strike, not once
        // for the whole action. The search replays the exact sequence `resolve_multi_strike`
        // performs (three consecutive `bounded(10_000) < 2_000` draws) against a bare
        // `Rng`, so the seed it finds is guaranteed to reproduce through the real resolver.
        let mut found_seed = None;
        for candidate in 0u64..500 {
            let mut probe = Rng::from_state(candidate);
            let crit_count = (0..3)
                .filter(|_| probe.bounded(CRIT_ROLL_BOUND) < 2_000)
                .count();
            if crit_count == 1 {
                found_seed = Some(candidate);
                break;
            }
        }
        let Some(seed) = found_seed else {
            panic!(
                "fixture invariant: at least one seed under 500 must produce exactly one crit in three 20% rolls"
            );
        };

        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            seed,
        );
        let effect = multi_strike_effect(3, 24, 74, 20);
        let mut ctx = harness.ctx("actor", Some("target"), None, FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let Some(event) = damage_of(&harness, "target") else {
            panic!("target must have taken damage");
        };
        // Two uncrit strikes (24 each) plus one crit strike (74): 24 + 24 + 74 == 122.
        assert_eq!(event.direct, 122);
        assert!(event.was_critical);
    }

    #[test]
    fn multi_strike_honours_guarantee_crit_and_consumes_it() {
        // Zero crit chance on the MultiStrike itself: any crit observed can only have come
        // from the GuaranteeCrit status, not an RNG roll.
        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            7,
        );

        {
            let mark = guarantee_crit_effect(2);
            let mut ctx = harness.ctx("actor", Some("target"), None, FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &mark), Ok(()));
        }
        let statuses = statuses_of(&harness.state, "target");
        assert_eq!(statuses.len(), 1);
        let Some(status) = statuses.first() else {
            panic!("fixture invariant: exactly one status must be present");
        };
        assert_eq!(status.kind, EffectKind::GuaranteeCrit);
        assert_eq!(status.magnitude, 2);

        // Three strikes, but only two forced crits available: the first two must crit
        // (74 each), the third must not (24, since the MultiStrike's own chance is 0%).
        let strikes = multi_strike_effect(3, 24, 74, 0);
        let mut ctx = harness.ctx("actor", Some("target"), None, FixedPoint::ZERO);
        assert_eq!(resolve(&mut ctx, &strikes), Ok(()));

        let Some(event) = damage_of(&harness, "target") else {
            panic!("target must have taken damage");
        };
        assert_eq!(event.direct, 74 + 74 + 24);
        assert!(event.was_critical);

        // The status must be fully consumed — two forced crits exhausted its count of 2.
        let statuses_after = statuses_of(&harness.state, "target");
        assert!(
            statuses_after.is_empty(),
            "GuaranteeCrit must be removed once its forced-crit count reaches zero"
        );
    }

    #[test]
    fn guarantee_crit_reapplication_refreshes_rather_than_stacks() {
        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            3,
        );

        {
            let first = guarantee_crit_effect(3);
            let mut ctx = harness.ctx("actor", Some("target"), None, FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &first), Ok(()));
        }
        {
            let second = guarantee_crit_effect(1);
            let mut ctx = harness.ctx("actor", Some("target"), None, FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &second), Ok(()));
        }

        let statuses = statuses_of(&harness.state, "target");
        assert_eq!(
            statuses.len(),
            1,
            "reapplication must replace, never duplicate"
        );
        let Some(status) = statuses.first() else {
            panic!("fixture invariant: exactly one status must be present");
        };
        assert_eq!(
            status.magnitude, 1,
            "reapplication must refresh to the new count, not sum with the old one",
        );
    }

    // -------------------------------------------------------------------------------
    // Cluster
    // -------------------------------------------------------------------------------

    #[test]
    fn cluster_per_target_cap_holds_when_all_submunitions_hit_one_target() {
        // Target sits exactly at the impact point, within `BODY_WIDTH` of every ring-0
        // landing point (see `CLUSTER_RING_STEP`'s doc comment). Five submunitions at 16%
        // each would total 80% uncapped; the cap is 56%.
        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            11,
        );
        let effect = cluster_effect(5, 16, 56);
        let mut ctx = harness.ctx("actor", None, None, FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let Some(event) = damage_of(&harness, "target") else {
            panic!("target must have taken damage from at least one submunition");
        };
        assert_eq!(
            event.direct, 56,
            "five 16% submunitions on one target must cap at 56, not sum to 80"
        );
        assert_eq!(
            harness.terrain_ops.len(),
            5,
            "each submunition must leave its own crater"
        );
    }

    #[test]
    fn cluster_uncapped_total_when_below_the_cap() {
        // A single submunition well under the cap: the target should take exactly its
        // share, proving the cap does not clamp normal, non-stacked hits.
        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            11,
        );
        let effect = cluster_effect(1, 16, 56);
        let mut ctx = harness.ctx("actor", None, None, FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let Some(event) = damage_of(&harness, "target") else {
            panic!("target must have taken damage");
        };
        assert_eq!(event.direct, 16);
    }

    #[test]
    fn cluster_submunition_landing_spreads_around_impact_deterministically() {
        let impact = FixedPoint::new(10_000, 20_000);
        let first = cluster_submunition_landing(impact, 0);
        let second = cluster_submunition_landing(impact, 1);

        assert_ne!(
            first, second,
            "distinct submunitions must land at distinct points"
        );
        assert_eq!(
            first,
            cluster_submunition_landing(impact, 0),
            "landing points are a pure function of impact and index"
        );
        // Direction 0 is due +X at one ring step.
        assert_eq!(
            first,
            FixedPoint::new(impact.x + CLUSTER_RING_STEP, impact.y)
        );
    }

    // -------------------------------------------------------------------------------
    // Return
    // -------------------------------------------------------------------------------

    #[test]
    fn return_per_target_cap_holds_when_both_passes_hit_the_same_target() {
        // No secondary target given: the return pass re-hits the primary, matching the
        // boomerang's "same target on both legs" case. 18 + 18 == 36 uncapped, capped
        // at 32.
        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            5,
        );
        let effect = return_effect(18, 32);
        let mut ctx = harness.ctx("actor", Some("target"), None, FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let Some(event) = damage_of(&harness, "target") else {
            panic!("target must have taken damage");
        };
        assert_eq!(
            event.direct, 32,
            "two 18% passes on one target must cap at 32, not sum to 36"
        );
    }

    #[test]
    fn return_with_distinct_targets_caps_each_independently() {
        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("outbound", FixedPoint::ZERO, 400),
                make_player("inbound", FixedPoint::ZERO, 400),
            ],
            5,
        );
        let effect = return_effect(18, 32);
        let mut ctx = harness.ctx("actor", Some("outbound"), Some("inbound"), FixedPoint::ZERO);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let Some(outbound_event) = damage_of(&harness, "outbound") else {
            panic!("outbound target must have taken damage");
        };
        let Some(inbound_event) = damage_of(&harness, "inbound") else {
            panic!("inbound target must have taken damage");
        };
        assert_eq!(
            outbound_event.direct, 18,
            "a single pass is well under the cap"
        );
        assert_eq!(inbound_event.direct, 18);
    }

    // -------------------------------------------------------------------------------
    // Tunnel
    // -------------------------------------------------------------------------------

    #[test]
    fn tunnel_stops_at_reinforced_stone() {
        let mut harness = Harness::new(vec![make_player("actor", FixedPoint::ZERO, 100)], 2);
        // Soil out to x=5, reinforced stone at x=5, more soil beyond it. Impact at x=0.
        for x in 0..10 {
            let material = if x == 5 {
                Material::ReinforcedStone
            } else {
                Material::Soil
            };
            terrain::set_material(&mut harness.state.terrain, x, 0, material);
        }
        let Some(impact) = FixedPoint::from_cells(0, 0) else {
            panic!("fixture invariant: from_cells(0, 0) must succeed");
        };
        let effect = tunnel_effect(20, 30);
        let mut ctx = harness.ctx("actor", None, None, impact);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        // Cells strictly before the stone are cleared by the bore/detonation.
        assert_eq!(
            terrain::material_at(&harness.state.terrain, 4, 0),
            Material::Empty
        );
        // THE invariant: the reinforced cell survives untouched. Tunnel cannot penetrate it
        // (`ARSENAL.md`: "It cannot penetrate reinforced stone"), and `MaterialMask::SOFT`
        // is what enforces that — not the bore's stopping distance.
        assert_eq!(
            terrain::material_at(&harness.state.terrain, 5, 0),
            Material::ReinforcedStone
        );
        // Soil at cell 7 is beyond the 2-cell detonation radius centred on cell 4, so it is
        // untouched — proving the bore genuinely stopped rather than continuing through.
        assert_eq!(
            terrain::material_at(&harness.state.terrain, 7, 0),
            Material::Soil
        );
    }

    #[test]
    fn tunnel_detonation_is_radial_and_does_not_treat_stone_as_cover() {
        // Documents deliberate behaviour rather than asserting an aspiration. Cell 6 sits
        // behind the reinforced cell but within the detonation radius, and it IS cleared:
        // `terrain::subtract_circle` is purely radial with no line-of-sight occlusion, which
        // is engine-wide — every explosion in the game behaves this way, not just Tunnel.
        //
        // Whether solid terrain should shield material behind it from blasts is a real design
        // question, but it is a global one about crater semantics. It is recorded in
        // PROGRAM_PLAN §6 rather than special-cased here, because solving it in one resolver
        // would make Tunnel inconsistent with every other explosion.
        let mut harness = Harness::new(vec![make_player("actor", FixedPoint::ZERO, 100)], 2);
        for x in 0..10 {
            let material = if x == 5 {
                Material::ReinforcedStone
            } else {
                Material::Soil
            };
            terrain::set_material(&mut harness.state.terrain, x, 0, material);
        }
        let Some(impact) = FixedPoint::from_cells(0, 0) else {
            panic!("fixture invariant: from_cells(0, 0) must succeed");
        };
        let effect = tunnel_effect(20, 30);
        let mut ctx = harness.ctx("actor", None, None, impact);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        assert_eq!(
            terrain::material_at(&harness.state.terrain, 6, 0),
            Material::Empty,
            "craters are radial; stone is not cover against the blast itself",
        );
    }

    #[test]
    fn tunnel_deals_detonation_damage_to_primary_target() {
        let mut harness = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::new(3 * POSITION_SCALE, 0), 400),
            ],
            2,
        );
        let Some(impact) = FixedPoint::from_cells(0, 0) else {
            panic!("fixture invariant: from_cells(0, 0) must succeed");
        };
        let effect = tunnel_effect(5, 30);
        let mut ctx = harness.ctx("actor", Some("target"), None, impact);

        assert_eq!(resolve(&mut ctx, &effect), Ok(()));

        let Some(event) = damage_of(&harness, "target") else {
            panic!("target must have taken detonation damage");
        };
        assert_eq!(event.direct, 30);
    }

    // -------------------------------------------------------------------------------
    // Determinism
    // -------------------------------------------------------------------------------

    #[test]
    fn determinism_two_identical_runs_produce_identical_results() {
        let effect = multi_strike_effect(3, 24, 74, 20);

        let mut first = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            42,
        );
        {
            let mut ctx = first.ctx("actor", Some("target"), None, FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &effect), Ok(()));
        }

        let mut second = Harness::new(
            vec![
                make_player("actor", FixedPoint::ZERO, 100),
                make_player("target", FixedPoint::ZERO, 400),
            ],
            42,
        );
        {
            let mut ctx = second.ctx("actor", Some("target"), None, FixedPoint::ZERO);
            assert_eq!(resolve(&mut ctx, &effect), Ok(()));
        }

        assert_eq!(damage_of(&first, "target"), damage_of(&second, "target"));
        assert_eq!(first.rng.state(), second.rng.state());
        assert_eq!(first.state.terrain, second.state.terrain);
    }
}
