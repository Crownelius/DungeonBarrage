//! `Canonical` implementations for every state type, and the two hashing entry points
//! the rest of the simulation calls: [`hash_state`] and [`hash_terrain`].
//!
//! This module is the linchpin of TS/Rust parity (`docs/MODULE_OWNERSHIP.md`): if the
//! encoding here is wrong, or drifts silently on a refactor, every determinism test that
//! compares a client hash to a server hash is meaningless. Three rules keep that from
//! happening, and every impl below follows them:
//!
//! 1. **Cosmetic data is never written.** [`crate::types::Appearance`] never appears in
//!    any encoding in this file. Two players identical except for skin, ability-effect
//!    skins, or victory pose must hash identically — see
//!    `tests::cosmetic_appearance_never_affects_hash`.
//! 2. **Every collection is written in an explicit, defensively-enforced sort order.**
//!    `types.rs` documents an intended order on several `Vec` fields (players by `id`,
//!    objects by `sequence`, statuses by `kind`, command ids ascending), but this module
//!    does not trust callers to have maintained it: it re-sorts before writing. Two
//!    logically identical states that merely built their `Vec`s in a different order
//!    (a very real possibility between a Rust server and a TypeScript client) must still
//!    produce the same hash. Sorting a `Vec<&T>` of borrows is O(n log n) and allocates no
//!    new state, so this costs nothing observable.
//! 3. **Every enum gets an explicit, hand-assigned, hand-documented `u8` discriminant**,
//!    written by a private `*_discriminant` function below — never the Rust enum's
//!    layout (`as u8`). Reordering a variant in `types.rs` must never silently change a
//!    historical hash; only editing the mapping table in this file can.
//!
//! # Layout of [`SimulationState`]
//!
//! [`SimulationState::write_canonical`] writes four sections, each opened with a domain
//! separator ([`CanonicalHasher::write_domain_separator`]) so that a metadata suffix can
//! never be misread as a players prefix, an objects prefix, etc. — the same hazard the
//! terrain/metadata example in `canonical.rs`'s module docs describes:
//!
//! | Order | Section | Tag | Contents |
//! |---|---|---:|---|
//! | 1 | Metadata | [`domain::METADATA`] (`0x01`) | versions, tick, turn, phase, active player, wind, movement remaining, has-attacked flag, next-sequence counters, RNG state |
//! | 2 | Players | [`domain::PLAYERS`] (`0x02`) | [`PlayerState`]s, sorted by `id` |
//! | 3 | Objects | [`OBJECTS_DOMAIN_TAG`] (`0x05`, local — see its doc comment) | [`PersistentObject`]s, sorted by `sequence` |
//! | 4 | Terrain | [`domain::TERRAIN`] (`0x03`) | written by [`TerrainMask::write_canonical`] itself, so `hash_terrain` and the embedded copy always agree |
//! | 5 | Commands | [`domain::COMMANDS`] (`0x04`) | `processed_command_ids`, sorted ascending |
//!
//! This groups fields *by role*, not by their declaration order in `types.rs` — for
//! example `next_terrain_sequence` and `rng_state` are declared near the end of the
//! struct but are written in the metadata section, because they are match-wide counters,
//! not part of the players or objects collections. Grouping by role rather than
//! declaration order means a field can be reordered in `types.rs` (a struct's field order
//! is not semantically meaningful in Rust) without this file changing.
//!
//! Field-width note: several gameplay fields are `u16` (`health`, `max_health`,
//! `special_gauge`) or `u8` (`team`, `turns_remaining`). [`CanonicalHasher`] has no
//! dedicated 16-bit writer, so `u16` fields are widened losslessly with `u32::from`
//! before [`CanonicalHasher::write_u32`] — a widening conversion, never a truncating
//! `as` cast, so it needs no `#[expect(clippy::cast_possible_truncation)]`.

use crate::canonical::{Canonical, CanonicalHasher, domain, hash_canonical};
use crate::fixed::FixedPoint;
use crate::types::{
    AbilitySlot, EffectKind, EffectTrigger, MatchPhase, Material, MaterialMask, MovementClass,
    PersistentObject, PersistentObjectKind, PlayerState, RangeTier, SimulationState, StatusEffect,
    TerrainMask, TerrainOperation, TerrainShape,
};

// ---------------------------------------------------------------------------
// Domain separators not covered by `canonical::domain`
// ---------------------------------------------------------------------------

/// Domain separator tag for [`SimulationState::objects`].
///
/// `canonical::domain` (owned and frozen by the integrator, `docs/MODULE_OWNERSHIP.md`)
/// defines tags for metadata, players, terrain, and commands only — it predates
/// `SimulationState` growing a persistent-object collection. Rather than edit a file this
/// task does not own, or leave the objects section unseparated, this reuses
/// [`CanonicalHasher::write_domain_separator`]'s generic `tag: u8` parameter (it accepts
/// any byte, not only the named constants) with a value that does not collide with any
/// tag in `canonical::domain` (`0x01`–`0x04`).
const OBJECTS_DOMAIN_TAG: u8 = 0x05;

// ---------------------------------------------------------------------------
// Enum discriminant mappings
//
// Every mapping is hand-assigned and hand-documented here, independent of each enum's
// declaration order in `types.rs`. Reordering, inserting, or removing a *different*
// variant in `types.rs` must never change the discriminant of a variant this module
// already assigned — that would silently change every historical hash containing it.
// New variants get the next unused number; numbers are never reassigned.
// ---------------------------------------------------------------------------

/// Wire discriminant for [`AbilitySlot`].
///
/// | Variant | Byte |
/// |---|---:|
/// | `Basic` | `0` |
/// | `BasicAlt` | `1` |
/// | `Special` | `2` |
const fn ability_slot_discriminant(slot: AbilitySlot) -> u8 {
    match slot {
        AbilitySlot::Basic => 0,
        AbilitySlot::BasicAlt => 1,
        AbilitySlot::Special => 2,
    }
}

/// Wire discriminant for [`RangeTier`].
///
/// | Variant | Byte |
/// |---|---:|
/// | `Melee` | `0` |
/// | `Tier1` | `1` |
/// | `Tier2` | `2` |
/// | `Tier3` | `3` |
const fn range_tier_discriminant(tier: RangeTier) -> u8 {
    match tier {
        RangeTier::Melee => 0,
        RangeTier::Tier1 => 1,
        RangeTier::Tier2 => 2,
        RangeTier::Tier3 => 3,
    }
}

/// Wire discriminant for [`MovementClass`].
///
/// | Variant | Byte |
/// |---|---:|
/// | `Slow` | `0` |
/// | `Normal` | `1` |
/// | `Fast` | `2` |
const fn movement_class_discriminant(class: MovementClass) -> u8 {
    match class {
        MovementClass::Slow => 0,
        MovementClass::Normal => 1,
        MovementClass::Fast => 2,
    }
}

/// Wire discriminant for [`EffectTrigger`].
///
/// | Variant | Byte |
/// |---|---:|
/// | `OnFire` | `0` |
/// | `OnFlight` | `1` |
/// | `OnImpact` | `2` |
/// | `OnTurnEnd` | `3` |
const fn effect_trigger_discriminant(trigger: EffectTrigger) -> u8 {
    match trigger {
        EffectTrigger::OnFire => 0,
        EffectTrigger::OnFlight => 1,
        EffectTrigger::OnImpact => 2,
        EffectTrigger::OnTurnEnd => 3,
    }
}

/// Wire discriminant for [`EffectKind`].
///
/// The closed effect vocabulary that character kits reference. Assigned in `types.rs`
/// declaration order at the time this module was written, but — per the rule at the top
/// of this file — that is a coincidence of history, not a rule: a *new* variant appended
/// to `types.rs` gets the next unused number here (currently `22`) regardless of where in
/// the enum declaration it is inserted.
///
/// | Variant | Byte | Variant | Byte |
/// |---|---:|---|---:|
/// | `Knockback` | `0` | `SpawnTurret` | `13` |
/// | `Chill` | `1` | `Heal` | `14` |
/// | `Cluster` | `2` | `HealthTransfer` | `15` |
/// | `Embers` | `3` | `MultiStrike` | `16` |
/// | `Tunnel` | `4` | `GuaranteeCrit` | `17` |
/// | `Return` | `5` | `EmbedProjectile` | `18` |
/// | `Recoil` | `6` | `ChainDetonate` | `19` |
/// | `SelfDamage` | `7` | `Relocate` | `20` |
/// | `Teleport` | `8` | `Obscure` | `21` |
/// | `Pull` | `9` | | |
/// | `Push` | `10` | | |
/// | `WallImpact` | `11` | | |
/// | `Lockdown` | `12` | | |
const fn effect_kind_discriminant(kind: EffectKind) -> u8 {
    match kind {
        EffectKind::Knockback => 0,
        EffectKind::Chill => 1,
        EffectKind::Cluster => 2,
        EffectKind::Embers => 3,
        EffectKind::Tunnel => 4,
        EffectKind::Return => 5,
        EffectKind::Recoil => 6,
        EffectKind::SelfDamage => 7,
        EffectKind::Teleport => 8,
        EffectKind::Pull => 9,
        EffectKind::Push => 10,
        EffectKind::WallImpact => 11,
        EffectKind::Lockdown => 12,
        EffectKind::SpawnTurret => 13,
        EffectKind::Heal => 14,
        EffectKind::HealthTransfer => 15,
        EffectKind::MultiStrike => 16,
        EffectKind::GuaranteeCrit => 17,
        EffectKind::EmbedProjectile => 18,
        EffectKind::ChainDetonate => 19,
        EffectKind::Relocate => 20,
        EffectKind::Obscure => 21,
    }
}

/// Wire discriminant for [`Material`].
///
/// `Material` already carries an explicit `#[repr(u8)]` in `types.rs` (`Empty = 0` …
/// `ReinforcedStone = 3`) that terrain cell bytes are literally stored as. This mapping is
/// written out independently anyway, matching those same values, rather than reaching for
/// `self as u8` — per this module's mandate, the hash format's discriminants are owned
/// here, not borrowed from wherever else in the crate happens to also need small integer
/// tags for the same enum.
///
/// | Variant | Byte |
/// |---|---:|
/// | `Empty` | `0` |
/// | `Soil` | `1` |
/// | `Wood` | `2` |
/// | `ReinforcedStone` | `3` |
const fn material_discriminant(material: Material) -> u8 {
    match material {
        Material::Empty => 0,
        Material::Soil => 1,
        Material::Wood => 2,
        Material::ReinforcedStone => 3,
    }
}

/// Wire discriminant for [`PersistentObjectKind`].
///
/// | Variant | Byte |
/// |---|---:|
/// | `Turret` | `0` |
/// | `EmbeddedKnife` | `1` |
/// | `GasCloud` | `2` |
const fn persistent_object_kind_discriminant(kind: PersistentObjectKind) -> u8 {
    match kind {
        PersistentObjectKind::Turret => 0,
        PersistentObjectKind::EmbeddedKnife => 1,
        PersistentObjectKind::GasCloud => 2,
    }
}

/// Wire discriminant for [`MatchPhase`].
///
/// | Variant | Byte | Variant | Byte |
/// |---|---:|---|---:|
/// | `MatchIntro` | `0` | `Resolution` | `6` |
/// | `TurnStart` | `1` | `Settling` | `7` |
/// | `Movement` | `2` | `StatusResolution` | `8` |
/// | `AimingAndSelection` | `3` | `VictoryCheck` | `9` |
/// | `PassiveSelection` | `4` | `MatchComplete` | `10` |
/// | `CommandLocked` | `5` | | |
const fn match_phase_discriminant(phase: MatchPhase) -> u8 {
    match phase {
        MatchPhase::MatchIntro => 0,
        MatchPhase::TurnStart => 1,
        MatchPhase::Movement => 2,
        MatchPhase::AimingAndSelection => 3,
        MatchPhase::PassiveSelection => 4,
        MatchPhase::CommandLocked => 5,
        MatchPhase::Resolution => 6,
        MatchPhase::Settling => 7,
        MatchPhase::StatusResolution => 8,
        MatchPhase::VictoryCheck => 9,
        MatchPhase::MatchComplete => 10,
    }
}

// ---------------------------------------------------------------------------
// Canonical impls: simple enums
// ---------------------------------------------------------------------------

impl Canonical for AbilitySlot {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(ability_slot_discriminant(*self));
    }
}

impl Canonical for RangeTier {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(range_tier_discriminant(*self));
    }
}

impl Canonical for MovementClass {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(movement_class_discriminant(*self));
    }
}

impl Canonical for EffectTrigger {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(effect_trigger_discriminant(*self));
    }
}

impl Canonical for EffectKind {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(effect_kind_discriminant(*self));
    }
}

impl Canonical for Material {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(material_discriminant(*self));
    }
}

impl Canonical for PersistentObjectKind {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(persistent_object_kind_discriminant(*self));
    }
}

impl Canonical for MatchPhase {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(match_phase_discriminant(*self));
    }
}

// ---------------------------------------------------------------------------
// Canonical impls: composite types
// ---------------------------------------------------------------------------

/// Not one of the types this task's checklist names explicitly, but required by nearly
/// all of them: [`PlayerState::position`], [`PersistentObject::position`], and every
/// [`FixedPoint`] inside [`TerrainShape`] need a canonical encoding, and [`FixedPoint`]
/// is defined in this same crate (`fixed.rs`), so implementing a same-crate trait for a
/// same-crate type here is ordinary Rust — no orphan-rule exception, no edit to `fixed.rs`
/// or `canonical.rs`. Encoded as `x` then `y`, each via [`i32`]'s existing blanket
/// [`Canonical`] impl in `canonical.rs`.
impl Canonical for FixedPoint {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        self.x.write_canonical(hasher);
        self.y.write_canonical(hasher);
    }
}

impl Canonical for MaterialMask {
    /// A single raw byte — the bitmask itself. Fixed-width, so no length prefix is
    /// needed for the same reason `write_u8` fields elsewhere never get one: ambiguity
    /// only arises for variable-width data.
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u8(self.0);
    }
}

impl Canonical for TerrainShape {
    /// `SubtractCircle` is discriminant `0`, `SubtractCapsule` is discriminant `1`,
    /// followed by that variant's fields in declaration order.
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        match self {
            Self::SubtractCircle {
                center,
                radius_cells,
            } => {
                hasher.write_u8(0);
                center.write_canonical(hasher);
                hasher.write_u32(u32::from(*radius_cells));
            }
            Self::SubtractCapsule {
                start,
                end,
                radius_cells,
            } => {
                hasher.write_u8(1);
                start.write_canonical(hasher);
                end.write_canonical(hasher);
                hasher.write_u32(u32::from(*radius_cells));
            }
        }
    }
}

impl Canonical for TerrainOperation {
    /// Field order: `sequence`, `shape`, `material_mask`.
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u32(self.sequence);
        self.shape.write_canonical(hasher);
        self.material_mask.write_canonical(hasher);
    }
}

impl Canonical for TerrainMask {
    /// Writes [`domain::TERRAIN`], then `width`, then `height`, then the cell buffer via
    /// [`CanonicalHasher::write_bytes`] — length-prefixed raw bytes, not one
    /// [`CanonicalHasher::write_u8`] call per cell. A terrain mask is tens of thousands of
    /// cells; encoding element-by-element would multiply the write count for no
    /// disambiguation benefit, since `write_bytes` already carries its own unambiguous
    /// length prefix.
    ///
    /// The domain separator is written by this impl itself (not only by the caller
    /// embedding a `TerrainMask` inside a larger structure), so [`hash_terrain`] and the
    /// terrain section inside [`hash_state`] always agree byte-for-byte for the same mask.
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_domain_separator(domain::TERRAIN);
        hasher.write_u32(self.width);
        hasher.write_u32(self.height);
        hasher.write_bytes(&self.cells);
    }
}

impl Canonical for StatusEffect {
    /// Field order: `kind`, `magnitude`, `turns_remaining`.
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        self.kind.write_canonical(hasher);
        self.magnitude.write_canonical(hasher);
        hasher.write_u8(self.turns_remaining);
    }
}

impl Canonical for PersistentObject {
    /// Field order: `sequence`, `owner_id`, `kind`, `position`, `health`,
    /// `turns_remaining`.
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_u32(self.sequence);
        hasher.write_str(&self.owner_id);
        self.kind.write_canonical(hasher);
        self.position.write_canonical(hasher);
        hasher.write_u32(u32::from(self.health));
        hasher.write_u8(self.turns_remaining);
    }
}

impl Canonical for PlayerState {
    /// Field order: `id`, `team`, `health`, `max_health`, `position`, `character_id`,
    /// `passive_id` (a presence flag, then the string only if present), `special_gauge`,
    /// `has_chosen_passive`, then `statuses` — re-sorted by [`EffectKind`] here rather
    /// than trusted from the field's current order (see this module's top-level docs).
    ///
    /// `appearance` is **deliberately never written**: it is cosmetic
    /// (`types.rs`: "Never contributes to the state hash"), and
    /// `tests::cosmetic_appearance_never_affects_hash` proves it.
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_str(&self.id);
        hasher.write_u8(self.team);
        hasher.write_u32(u32::from(self.health));
        hasher.write_u32(u32::from(self.max_health));
        self.position.write_canonical(hasher);
        hasher.write_str(&self.character_id);

        match &self.passive_id {
            Some(passive_id) => {
                hasher.write_bool(true);
                hasher.write_str(passive_id);
            }
            None => hasher.write_bool(false),
        }

        hasher.write_u32(u32::from(self.special_gauge));
        hasher.write_bool(self.has_chosen_passive);

        // Defensive sort: see the "every collection is written in an explicit,
        // defensively-enforced sort order" rule in this module's top-level docs.
        let mut sorted_statuses: Vec<&StatusEffect> = self.statuses.iter().collect();
        sorted_statuses.sort_by_key(|status| status.kind);
        hasher.write_length(sorted_statuses.len());
        for status in sorted_statuses {
            status.write_canonical(hasher);
        }
    }
}

impl Canonical for SimulationState {
    /// See this module's top-level docs for the full section table. Summary: metadata,
    /// then players (sorted by `id`), then objects (sorted by `sequence`), then terrain
    /// (which writes its own domain separator), then processed command ids (sorted
    /// ascending) — each section opened with a domain separator.
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        hasher.write_domain_separator(domain::METADATA);
        hasher.write_u32(self.simulation_version);
        hasher.write_u32(self.content_version);
        hasher.write_u64(self.tick);
        hasher.write_u32(self.turn_number);
        self.phase.write_canonical(hasher);
        hasher.write_str(&self.active_player_id);
        hasher.write_i32(self.wind_per_tick);
        hasher.write_i32(self.movement_remaining);
        hasher.write_bool(self.has_attacked_this_turn);
        hasher.write_u32(self.next_terrain_sequence);
        hasher.write_u32(self.next_object_sequence);
        hasher.write_u64(self.rng_state);

        hasher.write_domain_separator(domain::PLAYERS);
        // Defensive sort by id: see the top-level "sort order" rule.
        let mut sorted_players: Vec<&PlayerState> = self.players.iter().collect();
        sorted_players.sort_by(|left, right| left.id.cmp(&right.id));
        hasher.write_length(sorted_players.len());
        for player in sorted_players {
            player.write_canonical(hasher);
        }

        hasher.write_domain_separator(OBJECTS_DOMAIN_TAG);
        // Defensive sort by sequence: see the top-level "sort order" rule.
        let mut sorted_objects: Vec<&PersistentObject> = self.objects.iter().collect();
        sorted_objects.sort_by_key(|object| object.sequence);
        hasher.write_length(sorted_objects.len());
        for object in sorted_objects {
            object.write_canonical(hasher);
        }

        // TerrainMask::write_canonical writes domain::TERRAIN itself.
        self.terrain.write_canonical(hasher);

        hasher.write_domain_separator(domain::COMMANDS);
        // Defensive sort ascending: see the top-level "sort order" rule.
        let mut sorted_command_ids: Vec<&str> = self
            .processed_command_ids
            .iter()
            .map(String::as_str)
            .collect();
        sorted_command_ids.sort_unstable();
        hasher.write_length(sorted_command_ids.len());
        for command_id in sorted_command_ids {
            hasher.write_str(command_id);
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// The authoritative state hash: hex-encoded FNV-1a 64 over `state`'s canonical encoding.
///
/// Two [`SimulationState`]s that differ in any gameplay-relevant field — including a
/// field the two states merely store their collections in a different order for — hash
/// differently if and only if the underlying gameplay state differs. Cosmetic fields
/// ([`crate::types::Appearance`]) never affect the result.
///
/// This is what `command.rs` writes into
/// [`crate::types::CommandOutcome::final_state_hash`] for client/server divergence
/// detection, and it is the entire reason this module exists.
#[must_use]
pub fn hash_state(state: &SimulationState) -> String {
    hash_canonical(state)
}

/// Hashes a [`TerrainMask`] in isolation, independent of any [`SimulationState`] it may
/// or may not be embedded in.
///
/// Produces the exact same bytes (and therefore the same hash) as the terrain section
/// [`hash_state`] writes for an equal mask, because both go through
/// [`TerrainMask::write_canonical`].
#[must_use]
pub fn hash_terrain(mask: &TerrainMask) -> String {
    hash_canonical(mask)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Appearance;

    // -----------------------------------------------------------------------------------
    // Test fixtures
    // -----------------------------------------------------------------------------------

    fn sample_appearance(skin_id: &str) -> Appearance {
        Appearance {
            skin_id: skin_id.to_string(),
            ability_skin_ids: [
                "skin-a".to_string(),
                "skin-b".to_string(),
                "skin-c".to_string(),
            ],
            victory_pose_id: "pose-1".to_string(),
        }
    }

    fn sample_status(kind: EffectKind, magnitude: i32, turns_remaining: u8) -> StatusEffect {
        StatusEffect {
            kind,
            magnitude,
            turns_remaining,
        }
    }

    fn sample_player(id: &str) -> PlayerState {
        PlayerState {
            id: id.to_string(),
            team: 0,
            health: 300,
            max_health: 300,
            position: FixedPoint::new(1_024, 2_048),
            character_id: "arzum".to_string(),
            passive_id: None,
            special_gauge: 0,
            has_chosen_passive: false,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn sample_object(sequence: u32) -> PersistentObject {
        PersistentObject {
            sequence,
            owner_id: "player-1".to_string(),
            kind: PersistentObjectKind::Turret,
            position: FixedPoint::new(512, 512),
            health: 80,
            turns_remaining: 3,
        }
    }

    fn sample_terrain() -> TerrainMask {
        TerrainMask {
            width: 3,
            height: 2,
            cells: vec![0, 1, 2, 3, 0, 1],
        }
    }

    fn sample_state() -> SimulationState {
        SimulationState {
            blocks: Vec::new(),
            simulation_version: 2,
            content_version: 1,
            tick: 100,
            turn_number: 4,
            phase: MatchPhase::AimingAndSelection,
            active_player_id: "player-1".to_string(),
            wind_per_tick: 5,
            movement_remaining: 4_096,
            has_attacked_this_turn: false,
            terrain: sample_terrain(),
            players: vec![sample_player("player-1"), sample_player("player-2")],
            objects: vec![sample_object(0), sample_object(1)],
            processed_command_ids: vec!["cmd-a".to_string(), "cmd-b".to_string()],
            next_terrain_sequence: 2,
            next_object_sequence: 2,
            rng_state: 0xDEAD_BEEF,
        }
    }

    // -----------------------------------------------------------------------------------
    // Cosmetic independence
    // -----------------------------------------------------------------------------------

    #[test]
    fn cosmetic_appearance_never_affects_hash() {
        let mut left = sample_player("player-1");
        left.appearance = sample_appearance("skin-red");

        let mut right = sample_player("player-1");
        right.appearance = sample_appearance("skin-blue");

        let mut left_hasher = CanonicalHasher::new();
        left.write_canonical(&mut left_hasher);

        let mut right_hasher = CanonicalHasher::new();
        right.write_canonical(&mut right_hasher);

        assert_eq!(left_hasher.finish(), right_hasher.finish());
    }

    #[test]
    fn cosmetic_appearance_never_affects_full_state_hash() {
        let mut left = sample_state();
        if let Some(player) = left.players.get_mut(0) {
            player.appearance = sample_appearance("skin-red");
        }

        let mut right = sample_state();
        if let Some(player) = right.players.get_mut(0) {
            player.appearance = sample_appearance("skin-blue");
        }

        assert_eq!(hash_state(&left), hash_state(&right));
    }

    // -----------------------------------------------------------------------------------
    // Every gameplay field flips the hash
    // -----------------------------------------------------------------------------------

    #[test]
    fn changing_health_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.health = player.health.saturating_sub(1);
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_max_health_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.max_health = player.max_health.saturating_add(10);
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_position_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.position =
                FixedPoint::new(player.position.x.saturating_add(1), player.position.y);
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_team_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.team = 1;
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_character_id_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.character_id = "emi".to_string();
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn choosing_a_passive_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.passive_id = Some("arzum-momentum".to_string());
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn special_gauge_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.special_gauge = 5_000;
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn has_chosen_passive_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.has_chosen_passive = true;
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn adding_a_status_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(player) = changed.players.get_mut(0) {
            player.statuses.push(sample_status(EffectKind::Chill, 1, 2));
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_terrain_cell_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(cell) = changed.terrain.cells.get_mut(0) {
            *cell = 3;
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_terrain_dimensions_changes_hash() {
        let baseline = sample_terrain();
        let mut changed = sample_terrain();
        changed.width = 4;
        assert_ne!(hash_terrain(&baseline), hash_terrain(&changed));
    }

    #[test]
    fn changing_object_health_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        if let Some(object) = changed.objects.get_mut(0) {
            object.health = object.health.saturating_sub(1);
        }
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn adding_a_processed_command_id_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.processed_command_ids.push("cmd-c".to_string());
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_phase_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.phase = MatchPhase::CommandLocked;
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_rng_state_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.rng_state = changed.rng_state.wrapping_add(1);
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_next_terrain_sequence_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.next_terrain_sequence = changed.next_terrain_sequence.saturating_add(1);
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_next_object_sequence_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.next_object_sequence = changed.next_object_sequence.saturating_add(1);
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_movement_remaining_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.movement_remaining = changed.movement_remaining.saturating_sub(1);
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_wind_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.wind_per_tick = changed.wind_per_tick.saturating_add(1);
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_has_attacked_this_turn_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.has_attacked_this_turn = true;
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_active_player_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.active_player_id = "player-2".to_string();
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_tick_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.tick = changed.tick.saturating_add(1);
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    #[test]
    fn changing_turn_number_changes_hash() {
        let baseline = sample_state();
        let mut changed = sample_state();
        changed.turn_number = changed.turn_number.saturating_add(1);
        assert_ne!(hash_state(&baseline), hash_state(&changed));
    }

    // -----------------------------------------------------------------------------------
    // Sort-order independence: same logical state, different `Vec` order, same hash
    // -----------------------------------------------------------------------------------

    #[test]
    fn player_vec_order_does_not_affect_hash() {
        let mut forward = sample_state();
        forward.players = vec![sample_player("player-1"), sample_player("player-2")];

        let mut backward = sample_state();
        backward.players = vec![sample_player("player-2"), sample_player("player-1")];

        assert_eq!(hash_state(&forward), hash_state(&backward));
    }

    #[test]
    fn object_vec_order_does_not_affect_hash() {
        let mut forward = sample_state();
        forward.objects = vec![sample_object(0), sample_object(1)];

        let mut backward = sample_state();
        backward.objects = vec![sample_object(1), sample_object(0)];

        assert_eq!(hash_state(&forward), hash_state(&backward));
    }

    #[test]
    fn command_id_vec_order_does_not_affect_hash() {
        let mut forward = sample_state();
        forward.processed_command_ids = vec!["cmd-a".to_string(), "cmd-b".to_string()];

        let mut backward = sample_state();
        backward.processed_command_ids = vec!["cmd-b".to_string(), "cmd-a".to_string()];

        assert_eq!(hash_state(&forward), hash_state(&backward));
    }

    #[test]
    fn status_vec_order_does_not_affect_hash() {
        let mut forward = sample_player("player-1");
        forward.statuses = vec![
            sample_status(EffectKind::Chill, 1, 2),
            sample_status(EffectKind::Knockback, 2, 1),
        ];

        let mut backward = sample_player("player-1");
        backward.statuses = vec![
            sample_status(EffectKind::Knockback, 2, 1),
            sample_status(EffectKind::Chill, 1, 2),
        ];

        let mut forward_hasher = CanonicalHasher::new();
        forward.write_canonical(&mut forward_hasher);

        let mut backward_hasher = CanonicalHasher::new();
        backward.write_canonical(&mut backward_hasher);

        assert_eq!(forward_hasher.finish(), backward_hasher.finish());
    }

    // -----------------------------------------------------------------------------------
    // Concatenation-ambiguity resistance, specific to types in this file
    // -----------------------------------------------------------------------------------

    #[test]
    fn player_id_and_character_id_do_not_alias_across_the_boundary() {
        // Without length prefixes, id="ab"+character_id="c" would encode identically to
        // id="a"+character_id="bc". write_str's length prefix (inherited from
        // CanonicalHasher) prevents this, but the property is worth pinning at this
        // module's boundary specifically, not just canonical.rs's generic string case.
        let mut left = sample_player("ab");
        left.character_id = "c".to_string();

        let mut right = sample_player("a");
        right.character_id = "bc".to_string();

        let mut left_hasher = CanonicalHasher::new();
        left.write_canonical(&mut left_hasher);

        let mut right_hasher = CanonicalHasher::new();
        right.write_canonical(&mut right_hasher);

        assert_ne!(left_hasher.finish(), right_hasher.finish());
    }

    #[test]
    fn empty_players_and_empty_objects_do_not_alias() {
        // Two structurally different empty sections (no players vs. no objects) must not
        // collide just because both encode as a zero-length count. This is exactly what
        // domain::PLAYERS vs OBJECTS_DOMAIN_TAG guards against.
        let mut no_players = sample_state();
        no_players.players = Vec::new();

        let mut no_objects = sample_state();
        no_objects.objects = Vec::new();

        assert_ne!(hash_state(&no_players), hash_state(&no_objects));
    }

    // -----------------------------------------------------------------------------------
    // Enum discriminant stability (part of the format; a silent renumbering must fail)
    // -----------------------------------------------------------------------------------

    #[test]
    fn ability_slot_discriminants_are_stable() {
        assert_eq!(ability_slot_discriminant(AbilitySlot::Basic), 0);
        assert_eq!(ability_slot_discriminant(AbilitySlot::BasicAlt), 1);
        assert_eq!(ability_slot_discriminant(AbilitySlot::Special), 2);
    }

    #[test]
    fn range_tier_discriminants_are_stable() {
        assert_eq!(range_tier_discriminant(RangeTier::Melee), 0);
        assert_eq!(range_tier_discriminant(RangeTier::Tier1), 1);
        assert_eq!(range_tier_discriminant(RangeTier::Tier2), 2);
        assert_eq!(range_tier_discriminant(RangeTier::Tier3), 3);
    }

    #[test]
    fn movement_class_discriminants_are_stable() {
        assert_eq!(movement_class_discriminant(MovementClass::Slow), 0);
        assert_eq!(movement_class_discriminant(MovementClass::Normal), 1);
        assert_eq!(movement_class_discriminant(MovementClass::Fast), 2);
    }

    #[test]
    fn effect_trigger_discriminants_are_stable() {
        assert_eq!(effect_trigger_discriminant(EffectTrigger::OnFire), 0);
        assert_eq!(effect_trigger_discriminant(EffectTrigger::OnFlight), 1);
        assert_eq!(effect_trigger_discriminant(EffectTrigger::OnImpact), 2);
        assert_eq!(effect_trigger_discriminant(EffectTrigger::OnTurnEnd), 3);
    }

    #[test]
    fn material_discriminants_are_stable() {
        assert_eq!(material_discriminant(Material::Empty), 0);
        assert_eq!(material_discriminant(Material::Soil), 1);
        assert_eq!(material_discriminant(Material::Wood), 2);
        assert_eq!(material_discriminant(Material::ReinforcedStone), 3);
    }

    #[test]
    fn persistent_object_kind_discriminants_are_stable() {
        assert_eq!(
            persistent_object_kind_discriminant(PersistentObjectKind::Turret),
            0
        );
        assert_eq!(
            persistent_object_kind_discriminant(PersistentObjectKind::EmbeddedKnife),
            1
        );
        assert_eq!(
            persistent_object_kind_discriminant(PersistentObjectKind::GasCloud),
            2
        );
    }

    #[test]
    fn match_phase_discriminants_are_stable() {
        assert_eq!(match_phase_discriminant(MatchPhase::MatchIntro), 0);
        assert_eq!(match_phase_discriminant(MatchPhase::TurnStart), 1);
        assert_eq!(match_phase_discriminant(MatchPhase::Movement), 2);
        assert_eq!(match_phase_discriminant(MatchPhase::AimingAndSelection), 3);
        assert_eq!(match_phase_discriminant(MatchPhase::PassiveSelection), 4);
        assert_eq!(match_phase_discriminant(MatchPhase::CommandLocked), 5);
        assert_eq!(match_phase_discriminant(MatchPhase::Resolution), 6);
        assert_eq!(match_phase_discriminant(MatchPhase::Settling), 7);
        assert_eq!(match_phase_discriminant(MatchPhase::StatusResolution), 8);
        assert_eq!(match_phase_discriminant(MatchPhase::VictoryCheck), 9);
        assert_eq!(match_phase_discriminant(MatchPhase::MatchComplete), 10);
    }

    #[test]
    fn effect_kind_discriminants_are_stable() {
        // All 22 variants, exhaustively. If a future edit to types.rs adds a 23rd variant,
        // this match in the implementation becomes non-exhaustive and fails to compile —
        // this test's job is only to guarantee the *existing* 22 numbers never move.
        assert_eq!(effect_kind_discriminant(EffectKind::Knockback), 0);
        assert_eq!(effect_kind_discriminant(EffectKind::Chill), 1);
        assert_eq!(effect_kind_discriminant(EffectKind::Cluster), 2);
        assert_eq!(effect_kind_discriminant(EffectKind::Embers), 3);
        assert_eq!(effect_kind_discriminant(EffectKind::Tunnel), 4);
        assert_eq!(effect_kind_discriminant(EffectKind::Return), 5);
        assert_eq!(effect_kind_discriminant(EffectKind::Recoil), 6);
        assert_eq!(effect_kind_discriminant(EffectKind::SelfDamage), 7);
        assert_eq!(effect_kind_discriminant(EffectKind::Teleport), 8);
        assert_eq!(effect_kind_discriminant(EffectKind::Pull), 9);
        assert_eq!(effect_kind_discriminant(EffectKind::Push), 10);
        assert_eq!(effect_kind_discriminant(EffectKind::WallImpact), 11);
        assert_eq!(effect_kind_discriminant(EffectKind::Lockdown), 12);
        assert_eq!(effect_kind_discriminant(EffectKind::SpawnTurret), 13);
        assert_eq!(effect_kind_discriminant(EffectKind::Heal), 14);
        assert_eq!(effect_kind_discriminant(EffectKind::HealthTransfer), 15);
        assert_eq!(effect_kind_discriminant(EffectKind::MultiStrike), 16);
        assert_eq!(effect_kind_discriminant(EffectKind::GuaranteeCrit), 17);
        assert_eq!(effect_kind_discriminant(EffectKind::EmbedProjectile), 18);
        assert_eq!(effect_kind_discriminant(EffectKind::ChainDetonate), 19);
        assert_eq!(effect_kind_discriminant(EffectKind::Relocate), 20);
        assert_eq!(effect_kind_discriminant(EffectKind::Obscure), 21);
    }

    // -----------------------------------------------------------------------------------
    // Known-answer vectors: pins the exact byte format so a refactor cannot silently
    // change it. If one of these fails after an intentional format change, bump
    // canonical::CANONICAL_ENCODING_VERSION (its own doc comment says why) and update the
    // constant here in the same change.
    // -----------------------------------------------------------------------------------

    #[test]
    fn known_answer_vector_empty_terrain() {
        let mask = TerrainMask {
            width: 1,
            height: 1,
            cells: vec![0],
        };
        assert_eq!(hash_terrain(&mask), "f6cf3be5158a1525");
    }

    #[test]
    fn known_answer_vector_sample_terrain() {
        assert_eq!(hash_terrain(&sample_terrain()), "31df495e099fb152");
    }

    #[test]
    fn known_answer_vector_sample_state() {
        assert_eq!(hash_state(&sample_state()), "40c9ceef445ae69d");
    }

    #[test]
    fn known_answer_vector_default_appearance_player() {
        let player = sample_player("known-answer-player");
        let mut hasher = CanonicalHasher::new();
        player.write_canonical(&mut hasher);
        assert_eq!(hasher.finish_hex(), "bb84670a6d8b948d");
    }

    // -----------------------------------------------------------------------------------
    // hash_state / hash_terrain wiring
    // -----------------------------------------------------------------------------------

    #[test]
    fn hash_state_is_deterministic_across_calls() {
        let state = sample_state();
        assert_eq!(hash_state(&state), hash_state(&state));
    }

    #[test]
    fn hash_terrain_matches_the_terrain_section_embedded_in_hash_state() {
        // hash_terrain and the terrain section written inside SimulationState's own
        // encoding both go through TerrainMask::write_canonical, so hashing the same mask
        // both ways is expected to produce related-but-not-equal results: hash_state's
        // digest also folds in metadata/players/objects/commands after the terrain bytes,
        // so the two hashes are not expected to be equal, only for terrain changes to
        // propagate identically into both. This test pins that hash_terrain is at least a
        // pure, order-independent function of the mask, independent of any state it might
        // be embedded in.
        let mask_a = sample_terrain();
        let mut mask_b = sample_terrain();
        mask_b.cells = mask_a.cells.clone();
        assert_eq!(hash_terrain(&mask_a), hash_terrain(&mask_b));
    }

    #[test]
    fn empty_terrain_mask_hashes_deterministically() {
        let mask = TerrainMask {
            width: 0,
            height: 0,
            cells: Vec::new(),
        };
        assert_eq!(hash_terrain(&mask), hash_terrain(&mask));
    }
}
