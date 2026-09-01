//! Locomotion: walking, jumping, slope climbing, and falling (`todolist.md` P5).
//!
//! Before this file, `movement_remaining` was tracked and hashed but nothing spent it —
//! characters could not move. Every function here mutates [`SimulationState`] directly and
//! is meant to be called from the turn scheduler during the `Movement` phase (`walk`,
//! `jump`) and the `Settling` phase (`settle`), per `PRODUCT_SPEC.md` §2's turn state
//! machine.
//!
//! # Swept collision, reused rather than re-derived
//!
//! `resolve::displacement::sweep` already proves the technique this module depends on:
//! step toward the destination in small increments and stop at the first solid cell, never
//! set a destination and check only the endpoint. Setting an endpoint and checking only that
//! cell would let a character cross a one-cell-thick wall whenever the far side happened to
//! be open.
//!
//! That function is private to its module, and locomotion's stopping rules genuinely differ
//! from a displacement effect's: `walk` must account for exactly how much of the movement
//! allowance was spent (a displacement effect does not track a budget), and both `walk` and
//! `settle` need a per-step *decision* (climb this obstruction, or land here) rather than
//! only a stop/continue test. So this module writes its own stepping loops using the same
//! technique instead of calling `sweep` directly. **Flagged as a judgement call**: the
//! integrator may want to factor a shared stepping primitive out of both modules once a
//! third caller shows up and the right generalization is clearer.
//!
//! # Field conventions
//!
//! `state.movement_remaining` is spent by [`walk`] only. [`jump`] does not touch it — see
//! that function's doc comment for why. `state.terrain` is read-only here; nothing in this
//! module mutates terrain.

use crate::error::{SimError, SimResult};
use crate::fixed::{FixedPoint, POSITION_SCALE, round_divide};
use crate::terrain;
use crate::types::{EffectKind, SimulationState};

// ---------------------------------------------------------------------------------------
// Tuning constants
// ---------------------------------------------------------------------------------------

/// Step length for every swept motion in this module, in fixed-point units.
///
/// A quarter cell — the same granularity `resolve::displacement::SWEEP_STEP` uses, fine
/// enough that a character cannot straddle a one-cell wall or floor, coarse enough that the
/// longest walk or fall in a match costs a bounded number of iterations. Written as a
/// literal with an assertion, not `POSITION_SCALE / 4`, so there is no division to lint and
/// a future change to `POSITION_SCALE` fails the build here instead of silently changing
/// this module's step granularity.
const MOVE_STEP: i32 = 256;
const _: () = assert!(MOVE_STEP * 4 == POSITION_SCALE);

/// Maximum terrain height, in whole cells, a walking step climbs automatically.
///
/// One cell. Documented rather than left implicit: it is the boundary between "a step you
/// walk up without breaking stride" and "a wall you must jump or path around." A taller
/// obstruction blocks [`walk`]'s sweep exactly like solid terrain that cannot be climbed at
/// all — the sweep stops there rather than passing through or over it.
const STEP_LIMIT_CELLS: i32 = 1;

/// Hard iteration cap for a single call to [`walk`] or [`jump`].
///
/// Mirrors `resolve::displacement::MAX_SWEEP_STEPS`: bounds worst-case per-call work so a
/// malformed or extreme allowance cannot walk the whole map cell by cell.
const MAX_WALK_STEPS: u32 = 2_048;

/// Hard iteration cap for one player's fall inside [`settle`].
///
/// At [`MOVE_STEP`] per step this covers roughly 1,024 terrain cells of fall distance, far
/// past any map this project ships. Bounded the same way [`MAX_WALK_STEPS`] is: a
/// pathological map height must not be able to spin this loop unbounded.
const MAX_FALL_STEPS: u32 = 4_096;

/// Vertical rise of a jump, in fixed-point units.
///
/// Two cells: enough headroom to clear a [`STEP_LIMIT_CELLS`]-tall step without any
/// horizontal walk at all, modest enough that a jump reads as a hop.
///
/// `PRODUCT_SPEC.md`'s provisional tuning table prices a jump at its own AP cost, separate
/// from a walk's. Nothing in [`SimulationState`] currently tracks an AP-style budget apart
/// from `movement_remaining`, which is defined as horizontal walking allowance — inventing a
/// field for it here would be exactly the "invent a type" the brief says to stop and report
/// instead. So [`jump`] only moves the character and spends nothing; gating how often a
/// player may jump per turn is left for whichever module ends up owning turn budgeting.
/// **Flagged as a judgement call.**
const JUMP_HEIGHT: i32 = 2 * POSITION_SCALE;

// ---------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------

/// Whether `player_id` is under Numa's Pin (`EffectKind::Lockdown` with turns remaining).
///
/// `types.rs` documents Lockdown as preventing a target from "moving or being displaced";
/// `resolve::displacement::is_displacement_immune` already gates the displacement half. This
/// gates the movement half for both [`walk`] and [`jump`]. Extending the guard to `jump` is
/// a judgement call — only `walk` is named explicitly in the brief, but a pin that stops
/// walking and not jumping would leave a hole in the same mechanic.
fn is_locked_down(state: &SimulationState, player_id: &str) -> bool {
    state.player(player_id).is_some_and(|player| {
        player
            .statuses
            .iter()
            .any(|status| status.kind == EffectKind::Lockdown && status.turns_remaining > 0)
    })
}

/// Number of `step_length`-sized increments needed to cover `distance`, rounded up.
///
/// Same ceiling-via-[`round_divide`] technique `resolve::displacement::sweep` uses: add
/// `step_length - 1` before dividing so a distance shorter than one step still yields at
/// least one step. Any extra rounding `round_divide` itself introduces only ever adds a step
/// (never removes a needed one), which is harmless here because every caller also checks the
/// remaining distance on each iteration and stops as soon as it reaches zero.
fn steps_needed(distance: i32, step_length: i32) -> u32 {
    let step_length = step_length.max(1);
    // Widened to i64 before adding so this cannot overflow i32 the way
    // `distance + step_length - 1` could for a `distance` near `i32::MAX`.
    let numerator = i64::from(distance.saturating_abs())
        .saturating_add(i64::from(step_length).saturating_sub(1));
    let steps_wanted = round_divide(numerator, i64::from(step_length)).unwrap_or(0);
    u32::try_from(steps_wanted).unwrap_or(u32::MAX)
}

/// Whether `position`'s cell lies below the bottom of the map — the unrecoverable-fall
/// condition `PRODUCT_SPEC.md` §2 requires resolve consistently and visibly.
///
/// Only the vertical bound is checked: gravity in [`settle`] only ever moves a falling
/// character down, so testing `y` alone is exhaustive for that caller.
/// `terrain::is_solid_at` already treats any out-of-bounds cell — including past the bottom
/// edge — as [`crate::types::Material::Empty`], which is exactly why this needs to be tested
/// explicitly: without it, a bottomless mask read would let a character fall forever instead
/// of ever being judged eliminated.
fn is_below_map(state: &SimulationState, position: FixedPoint) -> bool {
    let (_, y_cell) = position.to_cells();
    // `TerrainMask::height` is `u32`; no map this project constructs comes within orders of
    // magnitude of `i32::MAX` cells tall, but `try_from` keeps the conversion total instead
    // of assuming that and reaching for an `as` cast.
    let height_cells = i32::try_from(state.terrain.height).unwrap_or(i32::MAX);
    y_cell >= height_cells
}

/// Whether a character could stand at `position`: the cell itself is open, and the cell one
/// full cell below is solid ground to stand on.
///
/// Used by [`settle`] to decide who needs gravity applied this call. Public because the
/// turn scheduler and map validation both have an obvious use for the same question (e.g.
/// rejecting a spawn point that floats).
#[must_use]
pub fn can_stand_at(state: &SimulationState, position: FixedPoint) -> bool {
    if terrain::is_solid_at(&state.terrain, position) {
        // Embedded in terrain is never "standing" — unreachable given the sweep invariant
        // every mover in this module upholds, but this stays correct even if it happens.
        return false;
    }
    let below = FixedPoint::new(position.x, position.y.saturating_add(POSITION_SCALE));
    terrain::is_solid_at(&state.terrain, below)
}

// ---------------------------------------------------------------------------------------
// Locomotion
// ---------------------------------------------------------------------------------------

/// Walks `player_id` horizontally by up to `dx` fixed-point units — positive is `+X`,
/// negative is `-X` — stopping at the first cell swept collision cannot pass, and climbing
/// any step at most [`STEP_LIMIT_CELLS`] tall along the way rather than stopping dead at it.
///
/// Deducts the distance actually travelled — never more than `state.movement_remaining` —
/// from that allowance, and returns it, so a walk longer than the allowance simply travels
/// only what remains. A locked-down player (Numa's Pin) cannot walk: this returns `Ok(0)`
/// without touching position or allowance, a legal no-op rather than an error, the same way
/// `resolve::displacement`'s effects treat a pull with no target.
///
/// # Errors
///
/// Returns [`SimError::UnknownDefinition`] if `player_id` does not name a player in `state`.
pub fn walk(state: &mut SimulationState, player_id: &str, dx: i32) -> SimResult<i32> {
    let Some(start) = state.player(player_id).map(|player| player.position) else {
        return Err(SimError::UnknownDefinition);
    };
    if is_locked_down(state, player_id) {
        return Ok(0);
    }
    if dx == 0 {
        return Ok(0);
    }

    let direction: i32 = if dx > 0 { 1 } else { -1 };
    let requested = dx.saturating_abs();
    // The allowance field is a plain i32; clamping a corrupted or not-yet-initialized
    // negative allowance to zero refuses movement instead of granting it for free.
    let allowance = state.movement_remaining.max(0);
    let budget = requested.min(allowance);
    if budget <= 0 {
        return Ok(0);
    }

    let steps = steps_needed(budget, MOVE_STEP).min(MAX_WALK_STEPS);
    let mut position = start;
    let mut travelled: i32 = 0;

    for _ in 0..steps {
        let left = budget.saturating_sub(travelled);
        if left <= 0 {
            break;
        }
        let step_len = left.min(MOVE_STEP);
        let candidate_x = position
            .x
            .saturating_add(direction.saturating_mul(step_len));
        let flat = FixedPoint::new(candidate_x, position.y);

        if !terrain::is_solid_at(&state.terrain, flat) {
            position = flat;
            travelled = travelled.saturating_add(step_len);
            continue;
        }

        // Blocked at the current height. Try climbing up to STEP_LIMIT_CELLS terrain cells,
        // greedily — a one-cell step costs exactly one cell of rise, not the full limit.
        let mut climbed_to = None;
        for climb_cells in 1..=STEP_LIMIT_CELLS {
            // STEP_LIMIT_CELLS is a small fixed constant, so this multiply cannot approach
            // i32 overflow, but it stays saturating like every other position arithmetic
            // here rather than relying on that.
            let raised_y = position
                .y
                .saturating_sub(climb_cells.saturating_mul(POSITION_SCALE));
            let raised = FixedPoint::new(candidate_x, raised_y);
            if !terrain::is_solid_at(&state.terrain, raised) {
                climbed_to = Some(raised);
                break;
            }
        }

        match climbed_to {
            Some(raised) => {
                position = raised;
                travelled = travelled.saturating_add(step_len);
            }
            // Nothing within the climb limit is clear: a wall, not a step. Stop here,
            // exactly like resolve::displacement::sweep stops at solid terrain — the
            // character is left at the last confirmed-open position, never inside terrain.
            None => break,
        }
    }

    if let Some(player) = state.player_mut(player_id) {
        player.position = position;
    }
    // Only the distance actually covered is spent, never the distance requested.
    state.movement_remaining = state.movement_remaining.saturating_sub(travelled);
    Ok(travelled)
}

/// Jumps `player_id` straight up by up to [`JUMP_HEIGHT`], stopping at the first solid cell
/// overhead — a ceiling blocks a jump exactly like a wall blocks a walk.
///
/// Locked-down players cannot jump either; see [`is_locked_down`]'s doc comment for why the
/// guard extends past the walk-only case named in the brief. Returns the actual vertical
/// rise. Does not touch `state.movement_remaining` — see [`JUMP_HEIGHT`]'s doc comment.
///
/// # Errors
///
/// Returns [`SimError::UnknownDefinition`] if `player_id` does not name a player in `state`.
pub fn jump(state: &mut SimulationState, player_id: &str) -> SimResult<i32> {
    let Some(start) = state.player(player_id).map(|player| player.position) else {
        return Err(SimError::UnknownDefinition);
    };
    if is_locked_down(state, player_id) {
        return Ok(0);
    }

    let steps = steps_needed(JUMP_HEIGHT, MOVE_STEP).min(MAX_WALK_STEPS);
    let mut position = start;
    let mut risen: i32 = 0;

    for _ in 0..steps {
        let left = JUMP_HEIGHT.saturating_sub(risen);
        if left <= 0 {
            break;
        }
        let step_len = left.min(MOVE_STEP);
        let candidate = FixedPoint::new(position.x, position.y.saturating_sub(step_len));
        if terrain::is_solid_at(&state.terrain, candidate) {
            // A ceiling stops the jump exactly where a wall stops a walk.
            break;
        }
        position = candidate;
        risen = risen.saturating_add(step_len);
    }

    if let Some(player) = state.player_mut(player_id) {
        player.position = position;
    }
    Ok(risen)
}

/// Applies gravity once to every player not currently supported by solid ground.
///
/// Each unsupported player sweeps downward, [`MOVE_STEP`] at a time, until either landing on
/// solid ground ([`can_stand_at`] becomes true there) or falling below the bottom of the
/// map. The latter is `PRODUCT_SPEC.md` §2's unrecoverable fall: it sets health to zero
/// rather than letting the character fall forever off the bottom of the mask, so
/// `victory.rs` can observe the elimination the same way it observes any other zero-health
/// player. Already-eliminated players are left alone.
///
/// Returns how many players moved at all this call — landed or eliminated — not how many
/// were eliminated specifically.
///
/// # Errors
///
/// Never actually fails today; the `SimResult` return keeps this consistent with the rest
/// of the module and leaves room for a future validity check without a breaking signature
/// change.
pub fn settle(state: &mut SimulationState) -> SimResult<u32> {
    // Players are kept sorted by id (types.rs's documented invariant), so collecting ids up
    // front and iterating them in that order is deterministic without a second sort here.
    let ids: Vec<String> = state
        .players
        .iter()
        .map(|player| player.id.clone())
        .collect();
    let mut moved = 0u32;

    for id in ids {
        let Some(player) = state.player(&id) else {
            continue;
        };
        if player.is_eliminated() {
            continue;
        }
        let start = player.position;
        if can_stand_at(state, start) {
            continue;
        }

        let mut position = start;
        let mut fell_out_of_bounds = false;
        for _ in 0..MAX_FALL_STEPS {
            let candidate = FixedPoint::new(position.x, position.y.saturating_add(MOVE_STEP));
            if is_below_map(state, candidate) {
                position = candidate;
                fell_out_of_bounds = true;
                break;
            }
            if terrain::is_solid_at(&state.terrain, candidate) {
                // Landed: candidate is inside solid ground, so `position` — the last
                // confirmed-open cell — is where the fall stops.
                break;
            }
            position = candidate;
        }

        if position != start {
            moved = moved.saturating_add(1);
        }
        if let Some(player_mut) = state.player_mut(&id) {
            player_mut.position = position;
            if fell_out_of_bounds {
                player_mut.health = 0;
            }
        }
    }

    Ok(moved)
}

#[cfg(test)]
// Tests may panic on a fixture invariant that must hold; production paths above stay
// panic-free, which is what the lint protects. Matches the precedent in `terrain::tests`
// and `resolve::displacement::tests`.
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use crate::types::TurnEndReason;
    use crate::types::{
        Appearance, EffectKind, MatchPhase, Material, PlayerState, SimulationState, StatusEffect,
        TerrainMask,
    };

    fn empty_terrain(width: u32, height: u32) -> TerrainMask {
        match terrain::create_mask(width, height, Material::Empty) {
            Ok(mask) => mask,
            Err(_) => panic!("fixture invariant: an empty mask must be constructible"),
        }
    }

    fn player(id: &str, position: FixedPoint) -> PlayerState {
        PlayerState {
            id: id.to_owned(),
            team: 0,
            health: 200,
            max_health: 200,
            position,
            loadout: crate::types::Loadout::launch_default(),
            ammo: crate::types::DEFAULT_AMMO,
            statuses: Vec::new(),
            appearance: Appearance::default(),
        }
    }

    fn state_with(
        players: Vec<PlayerState>,
        terrain: TerrainMask,
        movement_remaining: i32,
    ) -> SimulationState {
        let mut players = players;
        players.sort_by(|a, b| a.id.cmp(&b.id));
        SimulationState {
            pending_turn_end_reason: TurnEndReason::Passed,
            last_turn_end_reason: TurnEndReason::Passed,
            simulation_version: 2,
            content_version: 1,
            tick: 0,
            turn_number: 1,
            phase: MatchPhase::Movement,
            active_player_id: "hero".to_owned(),
            wind_per_tick: 0,
            movement_remaining,
            has_attacked_this_turn: false,
            terrain,
            blocks: Vec::new(),
            players,
            objects: Vec::new(),
            processed_command_ids: Vec::new(),
            next_terrain_sequence: 0,
            next_object_sequence: 0,
            rng_state: 1,
        }
    }

    fn cells(x: i32, y: i32) -> FixedPoint {
        match FixedPoint::from_cells(x, y) {
            Some(point) => point,
            None => panic!("fixture invariant: from_cells must succeed"),
        }
    }

    fn position_of(state: &SimulationState, id: &str) -> FixedPoint {
        match state.player(id) {
            Some(player) => player.position,
            None => panic!("fixture invariant: player {id} must exist"),
        }
    }

    // -- walk: basic motion and allowance accounting --------------------------------

    #[test]
    fn walk_moves_position_and_deducts_the_allowance() {
        let mut state = state_with(
            vec![player("hero", cells(5, 5))],
            empty_terrain(64, 64),
            16_384,
        );

        let result = walk(&mut state, "hero", 3 * POSITION_SCALE);

        assert_eq!(result, Ok(3 * POSITION_SCALE));
        assert_eq!(position_of(&state, "hero"), cells(8, 5));
        assert_eq!(state.movement_remaining, 16_384 - 3 * POSITION_SCALE);
    }

    #[test]
    fn walk_leftward_moves_position_negative() {
        let mut state = state_with(
            vec![player("hero", cells(10, 5))],
            empty_terrain(64, 64),
            16_384,
        );

        let result = walk(&mut state, "hero", -2 * POSITION_SCALE);

        assert_eq!(result, Ok(2 * POSITION_SCALE));
        assert_eq!(position_of(&state, "hero"), cells(8, 5));
    }

    #[test]
    fn walk_is_capped_by_movement_remaining_and_travels_only_what_remains() {
        let mut state = state_with(
            vec![player("hero", cells(5, 5))],
            empty_terrain(64, 64),
            500,
        );
        let before = position_of(&state, "hero");

        let result = walk(&mut state, "hero", 2_000);

        assert_eq!(result, Ok(500), "must travel only the remaining allowance");
        assert_eq!(state.movement_remaining, 0);
        assert_eq!(position_of(&state, "hero").x, before.x + 500);
    }

    #[test]
    fn walk_at_zero_remaining_moves_nothing() {
        let mut state = state_with(vec![player("hero", cells(5, 5))], empty_terrain(64, 64), 0);
        let before = position_of(&state, "hero");

        let result = walk(&mut state, "hero", 100);

        assert_eq!(result, Ok(0));
        assert_eq!(position_of(&state, "hero"), before);
        assert_eq!(state.movement_remaining, 0);
    }

    #[test]
    fn walk_rejects_an_unknown_player() {
        let mut state = state_with(
            vec![player("hero", cells(5, 5))],
            empty_terrain(64, 64),
            1_000,
        );
        assert_eq!(
            walk(&mut state, "ghost", 100),
            Err(SimError::UnknownDefinition)
        );
    }

    // -- walk: terrain collision -----------------------------------------------------

    #[test]
    fn walk_stops_at_a_wall_and_the_character_never_ends_up_inside_it() {
        let mut terrain = empty_terrain(64, 64);
        // A wall at cell x=15, every row — far taller than any climbable step.
        for y in 0..64i32 {
            terrain::set_material(&mut terrain, 15, y, Material::ReinforcedStone);
        }
        let mut state = state_with(
            vec![player("hero", cells(5, 10))],
            terrain,
            30 * POSITION_SCALE,
        );

        let result = walk(&mut state, "hero", 20 * POSITION_SCALE);

        let Ok(travelled) = result else {
            panic!("walk must succeed against a wall, not error");
        };
        assert!(
            travelled < 20 * POSITION_SCALE,
            "the wall must stop the walk short of what was requested"
        );
        let after = position_of(&state, "hero");
        assert!(after.to_cells().0 < 15, "must stop before cell 15");
        assert!(
            !terrain::is_solid_at(&state.terrain, after),
            "a walked character must never end up inside terrain"
        );
    }

    // -- walk: slope climbing ----------------------------------------------------------

    #[test]
    fn walk_climbs_a_one_cell_step_instead_of_stopping_dead() {
        let mut terrain = empty_terrain(64, 64);
        // From x=10 onward, the walking row (y=10) itself becomes solid — a platform that
        // rises by exactly one cell relative to the open corridor the character starts in.
        for x in 10..30i32 {
            terrain::set_material(&mut terrain, x, 10, Material::Soil);
        }
        let mut state = state_with(
            vec![player("hero", cells(5, 10))],
            terrain,
            15 * POSITION_SCALE,
        );

        let result = walk(&mut state, "hero", 15 * POSITION_SCALE);

        assert_eq!(
            result,
            Ok(15 * POSITION_SCALE),
            "a one-cell step must not cost any of the allowance beyond the horizontal distance"
        );
        let after = position_of(&state, "hero");
        assert_eq!(
            after,
            cells(20, 9),
            "climbed exactly one cell up, x unobstructed"
        );
        assert!(!terrain::is_solid_at(&state.terrain, after));
    }

    #[test]
    fn walk_does_not_climb_a_wall_taller_than_the_step_limit() {
        let mut terrain = empty_terrain(64, 64);
        // From x=10 onward, rows 8, 9, and 10 are all solid: a three-cell wall, taller than
        // the one-cell climb limit, at the same location the previous test's one-cell step
        // sat.
        for x in 10..30i32 {
            for y in 8..=10i32 {
                terrain::set_material(&mut terrain, x, y, Material::Soil);
            }
        }
        let mut state = state_with(
            vec![player("hero", cells(5, 10))],
            terrain,
            15 * POSITION_SCALE,
        );

        let result = walk(&mut state, "hero", 15 * POSITION_SCALE);

        let Ok(travelled) = result else {
            panic!("walk must succeed against a tall wall, not error");
        };
        assert!(
            travelled < 15 * POSITION_SCALE,
            "a three-cell wall must not be climbed"
        );
        let after = position_of(&state, "hero");
        assert!(after.to_cells().0 < 10, "must stop before the wall");
        assert_eq!(
            after.to_cells().1,
            10,
            "height must be unchanged — no climb happened"
        );
        assert!(!terrain::is_solid_at(&state.terrain, after));
    }

    // -- walk: Lockdown -----------------------------------------------------------------

    #[test]
    fn a_locked_down_character_cannot_walk() {
        let mut hero = player("hero", cells(5, 5));
        hero.statuses.push(StatusEffect {
            kind: EffectKind::Lockdown,
            magnitude: 0,
            turns_remaining: 2,
        });
        let mut state = state_with(vec![hero], empty_terrain(64, 64), 10_000);
        let before = position_of(&state, "hero");

        let result = walk(&mut state, "hero", 5 * POSITION_SCALE);

        assert_eq!(
            result,
            Ok(0),
            "Numa's Pin prevents movement, not only displacement"
        );
        assert_eq!(position_of(&state, "hero"), before);
        assert_eq!(state.movement_remaining, 10_000);
    }

    // -- can_stand_at ---------------------------------------------------------------------

    #[test]
    fn can_stand_at_is_true_directly_above_solid_ground() {
        let mut terrain = empty_terrain(20, 20);
        terrain::set_material(&mut terrain, 5, 12, Material::Soil);
        let state = state_with(Vec::new(), terrain, 0);

        assert!(can_stand_at(&state, cells(5, 11)));
    }

    #[test]
    fn can_stand_at_is_false_in_open_air() {
        let state = state_with(Vec::new(), empty_terrain(20, 20), 0);
        assert!(!can_stand_at(&state, cells(5, 5)));
    }

    #[test]
    fn can_stand_at_is_false_when_embedded_in_terrain() {
        let mut terrain = empty_terrain(20, 20);
        terrain::set_material(&mut terrain, 5, 5, Material::Soil);
        let state = state_with(Vec::new(), terrain, 0);

        assert!(!can_stand_at(&state, cells(5, 5)));
    }

    // -- settle: falling --------------------------------------------------------------

    #[test]
    fn settle_drops_a_mid_air_character_onto_the_surface_below() {
        let mut terrain = empty_terrain(20, 20);
        for x in 0..20i32 {
            terrain::set_material(&mut terrain, x, 12, Material::Soil);
        }
        let mut state = state_with(vec![player("hero", cells(5, 3))], terrain, 0);

        let moved = settle(&mut state);

        assert_eq!(moved, Ok(1));
        let after = position_of(&state, "hero");
        assert_eq!(
            after.to_cells().1,
            11,
            "must land on the cell directly above the floor"
        );
        assert!(
            can_stand_at(&state, after),
            "must be supported after landing"
        );
        assert!(!terrain::is_solid_at(&state.terrain, after));
    }

    #[test]
    fn settle_leaves_a_grounded_character_untouched() {
        let mut terrain = empty_terrain(20, 20);
        for x in 0..20i32 {
            terrain::set_material(&mut terrain, x, 12, Material::Soil);
        }
        let mut state = state_with(vec![player("hero", cells(5, 11))], terrain, 0);
        let before = position_of(&state, "hero");

        let moved = settle(&mut state);

        assert_eq!(moved, Ok(0));
        assert_eq!(position_of(&state, "hero"), before);
    }

    #[test]
    fn settle_eliminates_a_character_that_falls_out_of_bounds() {
        // No floor anywhere: a pit all the way to the bottom of a small map.
        let mut state = state_with(vec![player("hero", cells(5, 3))], empty_terrain(10, 20), 0);

        let moved = settle(&mut state);

        assert_eq!(moved, Ok(1));
        let after = match state.player("hero") {
            Some(player) => player,
            None => panic!("fixture invariant: hero must still exist"),
        };
        assert_eq!(
            after.health, 0,
            "an unrecoverable fall must eliminate the character"
        );
    }

    #[test]
    fn settle_skips_an_already_eliminated_character() {
        let mut hero = player("hero", cells(5, 3));
        hero.health = 0;
        let mut state = state_with(vec![hero], empty_terrain(10, 20), 0);

        let moved = settle(&mut state);

        assert_eq!(moved, Ok(0), "an eliminated character is not settled again");
    }

    // -- jump ---------------------------------------------------------------------------

    #[test]
    fn jump_rises_by_the_full_height_when_there_is_headroom() {
        let mut state = state_with(
            vec![player("hero", cells(10, 10))],
            empty_terrain(64, 64),
            0,
        );
        let before = position_of(&state, "hero");

        let result = jump(&mut state, "hero");

        assert_eq!(result, Ok(JUMP_HEIGHT));
        let after = position_of(&state, "hero");
        assert_eq!(after.x, before.x);
        assert_eq!(after.y, before.y - JUMP_HEIGHT);
    }

    #[test]
    fn jump_is_blocked_immediately_by_a_ceiling() {
        let mut terrain = empty_terrain(64, 64);
        terrain::set_material(&mut terrain, 10, 9, Material::ReinforcedStone);
        let mut state = state_with(vec![player("hero", cells(10, 10))], terrain, 0);
        let before = position_of(&state, "hero");

        let result = jump(&mut state, "hero");

        assert_eq!(result, Ok(0));
        assert_eq!(position_of(&state, "hero"), before);
    }

    #[test]
    fn a_locked_down_character_cannot_jump() {
        let mut hero = player("hero", cells(10, 10));
        hero.statuses.push(StatusEffect {
            kind: EffectKind::Lockdown,
            magnitude: 0,
            turns_remaining: 1,
        });
        let mut state = state_with(vec![hero], empty_terrain(64, 64), 0);
        let before = position_of(&state, "hero");

        let result = jump(&mut state, "hero");

        assert_eq!(result, Ok(0));
        assert_eq!(position_of(&state, "hero"), before);
    }

    #[test]
    fn jump_rejects_an_unknown_player() {
        let mut state = state_with(vec![player("hero", cells(5, 5))], empty_terrain(64, 64), 0);
        assert_eq!(jump(&mut state, "ghost"), Err(SimError::UnknownDefinition));
    }
}
