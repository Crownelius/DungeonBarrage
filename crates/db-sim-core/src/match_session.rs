//! Transport-free match session contract for local clients and future Rust servers.
//!
//! [`MatchHost`] deliberately owns only authoritative game orchestration. This module is
//! the immediately surrounding session boundary described by `CLIENT_SPEC.md`: it accepts
//! one normalized command union, owns publication generations, retains first results for
//! idempotent replay, derives presentation events, and never exposes mutable host state.
//!
//! JSON, a C ABI, sockets, match IDs, and wall-clock deadlines remain adapter concerns. An
//! adapter decodes into [`MatchCommand`], calls [`MatchSessionHost::apply`], and encodes the
//! returned [`MatchTransition`]. This keeps both the Godot/C# local client and a future
//! Rust-native server on one semantic path without putting serialization dependencies in
//! the authoritative core.

use core::fmt;
use std::collections::{BTreeMap, BTreeSet};

use crate::canonical::{Canonical, CanonicalHasher, hash_canonical};
use crate::character;
use crate::client_contract::{
    AppearanceSnapshot, BlockSnapshot, CLIENT_CONTRACT_VERSION, ClientErosionAxis,
    ClientMatchOutcome, ClientMatchPhase, ClientMaterial, ClientObjectKind, ClientStatusKind,
    ClientTurnEndReason, MatchSnapshot, PersistentObjectSnapshot, PlayerSnapshot, PositionSnapshot,
    StatusSnapshot,
};
use crate::error::{SimError, SimResult};
use crate::fixed::FIXED_TICK_RATE;
use crate::match_host::MatchHost;
use crate::match_setup::{MatchConfig, create_match, is_valid_match_local_id};
use crate::terrain;
use crate::types::{
    AbilityCommand, AbilitySlot, BallisticImpact, BallisticSample, CommandOutcome,
    CommandRejection, CommandResult, CritRoll, DamageEvent, ImpactCause, MatchPhase,
    PassiveChoiceCommand, SimulationState, StrikeDelivery, StrikeResolution, TurnEndReason,
};

/// Maximum first-receipt results retained by one live session.
///
/// Entries are never evicted: silently forgetting a command would make a late retry apply
/// twice. Reaching this bound closes the session without mutating authoritative state.
pub const COMMAND_LEDGER_ENTRY_LIMIT: usize = 16_384;

/// Maximum canonical request/response bytes retained by one live session.
///
/// This is 64 mebibytes, not 64 decimal megabytes. [`MatchSessionHost::ledger_bytes`]
/// reports the same deterministic logical-byte measure. The measure is independent of
/// allocator capacity and platform word size: integers use their fixed wire widths, enum
/// discriminants and option tags use one byte, strings and sequences use a four-byte length
/// followed by their complete contents, and each top-level command/transition includes the
/// canonical encoding header and domain separator. Arithmetic is checked before publication.
pub const COMMAND_LEDGER_BYTE_LIMIT: u64 = 64 * 1024 * 1024;

/// Versioned cosmetic hold after the last required presentation event.
///
/// Zero is intentional for the first contract: projectile/impact ticks already keep input
/// locked through required playback, while an additional feel-based delay has not been
/// playtested. Changing this value is a client-contract decision, not a simulation change.
pub const POST_ACTION_LOCK_TICKS: u32 = 0;

/// One closed, normalized command submitted to a match session.
///
/// Wire adapters must reject unknown/missing fields before constructing this type. Typed
/// equality is semantic request identity, so JSON whitespace or object-key order can never
/// affect idempotency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchCommand {
    /// Client-contract schema version used to interpret this command.
    pub schema_version: u32,
    /// Deterministic match-unique idempotency key.
    pub command_id: String,
    /// Claimed actor, validated against the active player.
    pub player_id: String,
    /// Turn number observed when the command was constructed.
    pub expected_turn_number: u32,
    /// Session snapshot generation observed when the command was constructed.
    pub expected_snapshot_generation: u64,
    /// Closed gameplay intent and its bounded payload.
    pub kind: MatchCommandKind,
}

impl MatchCommand {
    /// Returns the deterministic digest used by the session ledger.
    ///
    /// The ledger also compares the complete typed command, so an FNV collision cannot turn
    /// a changed request into an accepted duplicate replay.
    #[must_use]
    pub fn canonical_digest(&self) -> String {
        hash_canonical(self)
    }

    fn validate_structure(&self) -> Result<(), SessionFault> {
        if self.schema_version != CLIENT_CONTRACT_VERSION {
            return Err(SessionFault::UnsupportedSchema {
                expected: CLIENT_CONTRACT_VERSION,
                actual: self.schema_version,
            });
        }
        if !is_valid_match_local_id(&self.command_id) {
            return Err(SessionFault::InvalidCommand {
                field: "command id",
            });
        }
        if !is_valid_match_local_id(&self.player_id) {
            return Err(SessionFault::InvalidCommand { field: "player id" });
        }

        match &self.kind {
            MatchCommandKind::Move { .. } | MatchCommandKind::Pass => {}
            MatchCommandKind::Ability {
                target_player_id,
                secondary_target_player_id,
                ..
            } => {
                for target in [target_player_id, secondary_target_player_id]
                    .into_iter()
                    .flatten()
                {
                    if !is_valid_match_local_id(target) {
                        return Err(SessionFault::InvalidCommand {
                            field: "target player id",
                        });
                    }
                }
            }
            MatchCommandKind::PassiveChoice { passive_id } => {
                if !is_valid_definition_id(passive_id) {
                    return Err(SessionFault::InvalidCommand {
                        field: "passive id",
                    });
                }
            }
        }
        Ok(())
    }
}

impl Canonical for MatchCommand {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        // Private domain tag for a normalized client command. This encoding is independent
        // of the canonical state sections and intentionally includes all nullable fields.
        hasher.write_domain_separator(0x20);
        hasher.write_u32(self.schema_version);
        hasher.write_str(&self.command_id);
        hasher.write_str(&self.player_id);
        hasher.write_u32(self.expected_turn_number);
        hasher.write_u64(self.expected_snapshot_generation);

        match &self.kind {
            MatchCommandKind::Move { dx } => {
                hasher.write_u8(0);
                hasher.write_i32(*dx);
            }
            MatchCommandKind::Ability {
                slot,
                angle_millidegrees,
                power_basis_points,
                target_player_id,
                secondary_target_player_id,
            } => {
                hasher.write_u8(1);
                hasher.write_u8(ability_slot_tag(*slot));
                hasher.write_i32(*angle_millidegrees);
                hasher.write_i32(*power_basis_points);
                write_optional_str(hasher, target_player_id.as_deref());
                write_optional_str(hasher, secondary_target_player_id.as_deref());
            }
            MatchCommandKind::PassiveChoice { passive_id } => {
                hasher.write_u8(2);
                hasher.write_str(passive_id);
            }
            MatchCommandKind::Pass => hasher.write_u8(3),
        }
    }
}

/// Closed gameplay command kinds accepted from a client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchCommandKind {
    /// Move horizontally by a fixed-point delta, bounded by authoritative allowance.
    Move {
        /// Requested signed fixed-point horizontal displacement.
        dx: i32,
    },
    /// Commit one character ability.
    Ability {
        /// Character ability slot.
        slot: AbilitySlot,
        /// Launch angle in integer millidegrees.
        angle_millidegrees: i32,
        /// Launch power in basis points.
        power_basis_points: i32,
        /// Optional primary player target.
        target_player_id: Option<String>,
        /// Optional secondary player target.
        secondary_target_player_id: Option<String>,
    },
    /// Resolve the one-time passive selection interrupt.
    PassiveChoice {
        /// Stable passive definition identifier.
        passive_id: String,
    },
    /// End the active turn without attacking.
    Pass,
}

/// Whether a transition is a new success, a new refusal, or a replayed first result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionDisposition {
    /// The first receipt was accepted.
    Accepted,
    /// The first receipt or a conflicting reuse was rejected.
    Rejected,
    /// The exact original first result is being replayed without mutation.
    DuplicateReplay,
}

/// Why a well-formed normalized command was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionRejection {
    /// The command targeted a stale or future session generation.
    SnapshotGenerationMismatch {
        /// Generation named by the command.
        expected: u64,
        /// Current session generation when first received.
        actual: u64,
    },
    /// An existing command ID was reused for different normalized content.
    CommandIdConflict,
    /// The authoritative command layer refused the gameplay intent.
    Core(CommandRejection),
}

impl TransitionRejection {
    /// Whether the refusal should be copied to a security telemetry channel.
    #[must_use]
    pub const fn is_security_event(&self) -> bool {
        match self {
            Self::CommandIdConflict => true,
            Self::Core(reason) => reason.is_security_event(),
            Self::SnapshotGenerationMismatch { .. } => false,
        }
    }
}

/// A non-gameplay failure at the session boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionFault {
    /// A typed command still violated a normalized-field constraint.
    InvalidCommand {
        /// Stable field label suitable for adapter diagnostics.
        field: &'static str,
    },
    /// The command schema is not the core contract this build implements.
    UnsupportedSchema {
        /// Schema supported by this build.
        expected: u32,
        /// Schema supplied by the caller.
        actual: u32,
    },
    /// The authoritative simulation returned an internal fallible error.
    Simulation(SimError),
    /// Retaining another first result would cross a session resource limit.
    ResourceLimit,
    /// The publication generation reached `u64::MAX` and cannot advance safely.
    GenerationExhausted,
    /// A supposedly impossible mismatch was found between host, snapshot, and outcome.
    ContractInvariant,
    /// A prior terminal fault closed the session; only disposal is valid afterward.
    Closed,
}

impl fmt::Display for SessionFault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCommand { field } => {
                write!(formatter, "invalid normalized command: {field}")
            }
            Self::UnsupportedSchema { expected, actual } => write!(
                formatter,
                "unsupported client schema {actual}; expected {expected}",
            ),
            Self::Simulation(error) => write!(formatter, "simulation fault: {error}"),
            Self::ResourceLimit => formatter.write_str("session command ledger limit exceeded"),
            Self::GenerationExhausted => formatter.write_str("session generation exhausted"),
            Self::ContractInvariant => formatter.write_str("session contract invariant failed"),
            Self::Closed => formatter.write_str("session is closed after a terminal fault"),
        }
    }
}

impl core::error::Error for SessionFault {}

impl From<SimError> for SessionFault {
    fn from(error: SimError) -> Self {
        Self::Simulation(error)
    }
}

/// Inclusive-origin rectangle in authoritative terrain-cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CellRectangle {
    /// Leftmost cell coordinate.
    pub x: i32,
    /// Topmost cell coordinate.
    pub y: i32,
    /// Width in cells; always positive for emitted rectangles.
    pub width: u32,
    /// Height in cells; always positive for emitted rectangles.
    pub height: u32,
}

/// Client-facing projectile path sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectileSampleSnapshot {
    /// Simulation tick relative to launch.
    pub tick: u32,
    /// Authoritative fixed-point position.
    pub position: PositionSnapshot,
}

/// Client-facing reason a projectile stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientImpactCause {
    /// Solid terrain was struck.
    Terrain,
    /// A character was struck; the current core does not retain its ID here.
    Character,
    /// The projectile left playable bounds.
    OutOfBounds,
    /// The projectile reached its deterministic lifetime cap.
    Expired,
}

/// Client-facing terminal projectile impact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpactSnapshot {
    /// Authoritative fixed-point impact position.
    pub position: PositionSnapshot,
    /// Simulation tick relative to launch.
    pub tick: u32,
    /// Why the projectile stopped.
    pub cause: ClientImpactCause,
}

/// One independently identifiable projectile path for presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectileTraceEvent {
    /// Stable transition-local trace identifier.
    pub trace_id: u32,
    /// Opaque owner player ID.
    pub owner_id: String,
    /// Stable ability definition ID.
    pub ability_id: String,
    /// Authoritative samples in ascending tick order.
    pub samples: Vec<ProjectileSampleSnapshot>,
    /// Terminal impact for this trace.
    pub terminal_impact: ImpactSnapshot,
}

/// Itemized damage/healing provenance retained by the command outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageBreakdown {
    /// Exact direct-hit damage.
    pub direct: u16,
    /// Radial splash damage.
    pub splash: u16,
    /// Self-damage attributed as Backlash.
    pub backlash: u16,
    /// World-hazard damage.
    pub hazard: u16,
    /// Terrain-collision damage.
    pub wall_impact: u16,
    /// Health restored during the same action.
    pub healed: u16,
    /// Whether at least one aggregated hit was critical.
    pub was_critical: bool,
    /// Net authoritative displacement recorded with the damage event.
    pub knockback: PositionSnapshot,
    /// Whether the recorded damage event marked the player eliminated.
    pub eliminated: bool,
}

/// Known provenance for a net state change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeProvenance {
    /// The command outcome retained itemized provenance.
    RecordedOutcome,
    /// The change is authoritative but the current outcome DTO does not retain its cause.
    AuthoritativeResolution,
}

/// Why an entity's position changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMovementCause {
    /// Net displacement proven to be solely the active player's accepted horizontal move.
    ///
    /// Reserved for the richer movement outcome contract. The current host does not retain
    /// the post-walk/pre-settle path needed to prove this — even a climb followed by settling
    /// can finish at its original height — so the current event builder never emits it.
    RequestedMove,
    /// Knockback, settling, relocation, or turn-boundary work owned by the host.
    AuthoritativeResolution,
}

/// One ordered presentation event kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresentationEventKind {
    /// A complete independently sampled projectile path.
    ProjectileTrace(ProjectileTraceEvent),
    /// A projectile terminal impact, addressable by trace ID.
    Impact {
        /// Transition-local trace identifier.
        trace_id: u32,
        /// Terminal impact details.
        impact: ImpactSnapshot,
    },
    /// One individual damage resolution, carrying the resolver's own record of it.
    ///
    /// Emitted once per strike the authoritative resolver actually performed, for melee and
    /// projectile deliveries alike — a projectile's [`PresentationEventKind::Impact`] is the
    /// ballistic terminal event, which is a different fact from the damage resolution that
    /// followed it. The embedded [`StrikeResolution`] is the resolver's verbatim record, not
    /// a reconstruction: nothing here is inferred from final state.
    StrikeResolved {
        /// Opaque attacking player ID.
        owner_id: String,
        /// Stable ability definition ID.
        ability_id: String,
        /// The authoritative per-strike record, including its crit draw and impact point.
        strike: StrikeResolution,
    },
    /// The terrain mask changed and these exact row-runs must be refreshed.
    TerrainChanged {
        /// New authoritative terrain-operation generation.
        terrain_generation: u32,
        /// Exact changed-cell row-runs, sorted by `(y, x)`.
        dirty_rectangles: Vec<CellRectangle>,
    },
    /// One destructible block's health or surviving bounds changed.
    BlockChanged {
        /// Stable map-authored block ID.
        block_id: u32,
        /// Previous health, or `None` if newly introduced.
        previous_health: Option<u16>,
        /// New health, or `None` if removed.
        new_health: Option<u16>,
        /// Previous material-backed surviving bounds.
        previous_surviving_bounds: Option<CellRectangle>,
        /// New material-backed surviving bounds.
        new_surviving_bounds: Option<CellRectangle>,
    },
    /// One player's net health changed during this host operation.
    HealthChanged {
        /// Opaque player ID.
        player_id: String,
        /// Health before the operation.
        previous_health: u16,
        /// Health after all synchronous host work.
        new_health: u16,
        /// Itemization when retained by `CommandOutcome`; otherwise explicitly absent.
        breakdown: Option<DamageBreakdown>,
    },
    /// One player's special gauge changed.
    GaugeChanged {
        /// Opaque player ID.
        player_id: String,
        /// Gauge before the operation.
        previous_gauge: u16,
        /// Gauge after the operation.
        new_gauge: u16,
        /// Signed actual change, computed from the two authoritative values.
        delta: i32,
    },
    /// One active status was added, changed, or removed.
    StatusChanged {
        /// Opaque player ID.
        player_id: String,
        /// Closed client status kind.
        kind: ClientStatusKind,
        /// Previous value, absent for an addition.
        previous: Option<StatusSnapshot>,
        /// New value, absent for a removal.
        current: Option<StatusSnapshot>,
    },
    /// One player's authoritative position changed.
    EntityMoved {
        /// Opaque player ID.
        player_id: String,
        /// Position before the operation.
        start: PositionSnapshot,
        /// Position after all synchronous host work.
        end: PositionSnapshot,
        /// Best truthful provenance available from the current outcome contract.
        cause: EntityMovementCause,
    },
    /// A persistent object was created.
    ObjectSpawned {
        /// Complete new object projection.
        object: PersistentObjectSnapshot,
    },
    /// A persistent object remained but its projected state changed.
    ObjectChanged {
        /// Projection before the operation.
        previous: PersistentObjectSnapshot,
        /// Projection after the operation.
        current: PersistentObjectSnapshot,
    },
    /// A persistent object was removed.
    ObjectRemoved {
        /// Last authoritative projection before removal.
        previous: PersistentObjectSnapshot,
        /// Provenance retained for the removal.
        cause: ChangeProvenance,
    },
    /// A living player became eliminated.
    PlayerEliminated {
        /// Opaque player ID.
        player_id: String,
        /// Whether itemized command provenance exists.
        cause: ChangeProvenance,
    },
    /// A one-time passive choice is now required before the turn can continue.
    PassiveChoiceRequired {
        /// Opaque player ID owing the choice.
        player_id: String,
        /// Stable passive definition IDs, sorted.
        passive_ids: Vec<String>,
    },
    /// A one-time passive choice was accepted.
    PassiveChosen {
        /// Opaque player ID.
        player_id: String,
        /// Stable chosen passive ID.
        passive_id: String,
    },
    /// The previous active player's turn ended.
    TurnEnded {
        /// Opaque player ID whose turn ended.
        player_id: String,
        /// Authoritative reason recorded by the scheduler.
        reason: ClientTurnEndReason,
    },
    /// A new planning turn is open.
    TurnOpened {
        /// Opaque active player ID.
        player_id: String,
        /// New authoritative turn number.
        turn_number: u32,
    },
    /// The match became terminal.
    MatchCompleted {
        /// Authoritative victory/draw outcome.
        outcome: ClientMatchOutcome,
    },
}

/// One event with deterministic presentation ordering metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationEvent {
    /// Relative presentation tick.
    pub presentation_tick: u32,
    /// Unique zero-based ordering key within this transition.
    pub sequence: u32,
    /// Closed event payload.
    pub kind: PresentationEventKind,
}

/// Atomic response to one command receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchTransition {
    /// Client-contract schema version.
    pub schema_version: u32,
    /// Command ID this response binds to.
    pub command_id: String,
    /// Whether this is a new acceptance/refusal or an exact replay.
    pub disposition: TransitionDisposition,
    /// Refusal detail; preserved on duplicate replay of a rejected first receipt.
    pub rejection_reason: Option<TransitionRejection>,
    /// Generation observed before the original first receipt.
    pub pre_snapshot_generation: u64,
    /// Generation after the original first receipt.
    pub post_snapshot_generation: u64,
    /// Tick rate used by `presentation_tick` and `input_lock_ticks`.
    pub presentation_tick_rate: u32,
    /// Earliest relative tick at which a subsequent planning window may open.
    pub input_lock_ticks: u32,
    /// Deterministically ordered presentation events.
    pub events: Vec<PresentationEvent>,
    /// Detached authoritative projection after the original receipt.
    pub post_snapshot: MatchSnapshot,
    /// Hash repeated at the envelope level for cheap divergence checks.
    pub post_state_hash: String,
}

#[derive(Debug, Clone)]
struct LedgerEntry {
    command: MatchCommand,
    canonical_digest: String,
    transition: MatchTransition,
}

// Stable logical-wire accounting for retained commands and transitions. This intentionally
// does not use `size_of`, `String::capacity`, or `Vec::capacity`: those values vary by target,
// allocator, and mutation history. The two top-level values each include the canonical
// encoding version (`u32`) and a domain separator (`0xff`, domain tag). Variable-size values
// fail closed if their four-byte length prefix cannot represent the actual payload.
const RETAINED_TOP_LEVEL_HEADER_BYTES: u64 = 4 + 2;

#[derive(Debug, Default)]
struct RetainedByteCounter {
    bytes: u64,
}

impl RetainedByteCounter {
    fn add(&mut self, bytes: u64) -> Option<()> {
        self.bytes = self.bytes.checked_add(bytes)?;
        Some(())
    }

    fn u8(&mut self, _value: u8) -> Option<()> {
        self.add(1)
    }

    fn u16(&mut self, _value: u16) -> Option<()> {
        self.add(2)
    }

    fn u32(&mut self, _value: u32) -> Option<()> {
        self.add(4)
    }

    fn i32(&mut self, _value: i32) -> Option<()> {
        self.add(4)
    }

    fn u64(&mut self, _value: u64) -> Option<()> {
        self.add(8)
    }

    fn boolean(&mut self, value: bool) -> Option<()> {
        self.u8(u8::from(value))
    }

    fn tag(&mut self) -> Option<()> {
        self.add(1)
    }

    fn length(&mut self, length: usize) -> Option<()> {
        let length = u32::try_from(length).ok()?;
        self.u32(length)
    }

    fn string(&mut self, value: &str) -> Option<()> {
        self.length(value.len())?;
        self.add(u64::try_from(value.len()).ok()?)
    }

    const fn finish(self) -> u64 {
        self.bytes
    }
}

fn retained_ledger_entry_bytes(
    command: &MatchCommand,
    transition: &MatchTransition,
) -> Option<u64> {
    retained_command_bytes(command)?.checked_add(retained_transition_bytes(transition)?)
}

fn retained_command_bytes(command: &MatchCommand) -> Option<u64> {
    let MatchCommand {
        schema_version,
        command_id,
        player_id,
        expected_turn_number,
        expected_snapshot_generation,
        kind,
    } = command;
    let mut counter = RetainedByteCounter::default();
    counter.add(RETAINED_TOP_LEVEL_HEADER_BYTES)?;
    counter.u32(*schema_version)?;
    counter.string(command_id)?;
    counter.string(player_id)?;
    counter.u32(*expected_turn_number)?;
    counter.u64(*expected_snapshot_generation)?;
    counter.tag()?;
    match kind {
        MatchCommandKind::Move { dx } => counter.i32(*dx)?,
        MatchCommandKind::Ability {
            slot,
            angle_millidegrees,
            power_basis_points,
            target_player_id,
            secondary_target_player_id,
        } => {
            counter.u8(ability_slot_tag(*slot))?;
            counter.i32(*angle_millidegrees)?;
            counter.i32(*power_basis_points)?;
            retained_optional_string_bytes(&mut counter, target_player_id.as_deref())?;
            retained_optional_string_bytes(&mut counter, secondary_target_player_id.as_deref())?;
        }
        MatchCommandKind::PassiveChoice { passive_id } => counter.string(passive_id)?,
        MatchCommandKind::Pass => {}
    }
    Some(counter.finish())
}

fn retained_transition_bytes(transition: &MatchTransition) -> Option<u64> {
    let MatchTransition {
        schema_version,
        command_id,
        disposition,
        rejection_reason,
        pre_snapshot_generation,
        post_snapshot_generation,
        presentation_tick_rate,
        input_lock_ticks,
        events,
        post_snapshot,
        post_state_hash,
    } = transition;
    let mut counter = RetainedByteCounter::default();
    counter.add(RETAINED_TOP_LEVEL_HEADER_BYTES)?;
    counter.u32(*schema_version)?;
    counter.string(command_id)?;
    retained_transition_disposition_bytes(&mut counter, *disposition)?;
    counter.boolean(rejection_reason.is_some())?;
    if let Some(rejection) = rejection_reason {
        retained_transition_rejection_bytes(&mut counter, rejection)?;
    }
    counter.u64(*pre_snapshot_generation)?;
    counter.u64(*post_snapshot_generation)?;
    counter.u32(*presentation_tick_rate)?;
    counter.u32(*input_lock_ticks)?;
    counter.length(events.len())?;
    for event in events {
        retained_presentation_event_bytes(&mut counter, event)?;
    }
    retained_match_snapshot_bytes(&mut counter, post_snapshot)?;
    counter.string(post_state_hash)?;
    Some(counter.finish())
}

fn retained_optional_string_bytes(
    counter: &mut RetainedByteCounter,
    value: Option<&str>,
) -> Option<()> {
    counter.boolean(value.is_some())?;
    if let Some(value) = value {
        counter.string(value)?;
    }
    Some(())
}

fn retained_transition_disposition_bytes(
    counter: &mut RetainedByteCounter,
    disposition: TransitionDisposition,
) -> Option<()> {
    match disposition {
        TransitionDisposition::Accepted
        | TransitionDisposition::Rejected
        | TransitionDisposition::DuplicateReplay => counter.tag(),
    }
}

fn retained_transition_rejection_bytes(
    counter: &mut RetainedByteCounter,
    rejection: &TransitionRejection,
) -> Option<()> {
    counter.tag()?;
    match rejection {
        TransitionRejection::SnapshotGenerationMismatch { expected, actual } => {
            counter.u64(*expected)?;
            counter.u64(*actual)?;
        }
        TransitionRejection::CommandIdConflict => {}
        TransitionRejection::Core(rejection) => {
            retained_command_rejection_bytes(counter, *rejection)?;
        }
    }
    Some(())
}

fn retained_command_rejection_bytes(
    counter: &mut RetainedByteCounter,
    rejection: CommandRejection,
) -> Option<()> {
    match rejection {
        CommandRejection::DuplicateCommand
        | CommandRejection::NotActivePlayer
        | CommandRejection::WrongPhase
        | CommandRejection::TurnVersionMismatch
        | CommandRejection::AbilityNotAvailable
        | CommandRejection::GaugeNotReady
        | CommandRejection::AlreadyAttacked
        | CommandRejection::InputOutOfRange
        | CommandRejection::PlayerEliminated
        | CommandRejection::InvalidTarget
        | CommandRejection::UnknownCharacter
        | CommandRejection::InvalidPassive
        | CommandRejection::PassiveAlreadyChosen => counter.tag(),
    }
}

fn retained_presentation_event_bytes(
    counter: &mut RetainedByteCounter,
    event: &PresentationEvent,
) -> Option<()> {
    let PresentationEvent {
        presentation_tick,
        sequence,
        kind,
    } = event;
    counter.u32(*presentation_tick)?;
    counter.u32(*sequence)?;
    retained_presentation_event_kind_bytes(counter, kind)
}

fn retained_presentation_event_kind_bytes(
    counter: &mut RetainedByteCounter,
    kind: &PresentationEventKind,
) -> Option<()> {
    counter.tag()?;
    match kind {
        PresentationEventKind::ProjectileTrace(trace) => {
            retained_projectile_trace_bytes(counter, trace)?;
        }
        PresentationEventKind::Impact { trace_id, impact } => {
            counter.u32(*trace_id)?;
            retained_impact_bytes(counter, *impact)?;
        }
        PresentationEventKind::StrikeResolved {
            owner_id,
            ability_id,
            strike,
        } => {
            counter.string(owner_id)?;
            counter.string(ability_id)?;
            counter.u16(strike.strike_index)?;
            counter.string(&strike.target_player_id)?;
            counter.i32(strike.impact_point.x)?;
            counter.i32(strike.impact_point.y)?;
            match strike.delivery {
                StrikeDelivery::Projectile { trace_sequence } => {
                    counter.u8(0)?;
                    counter.u32(trace_sequence)?;
                }
                StrikeDelivery::Melee => counter.u8(1)?,
            }
            counter.u8(match strike.crit {
                CritRoll::NotEligible => 0,
                CritRoll::Missed => 1,
                CritRoll::Landed => 2,
            })?;
            counter.u16(strike.damage_applied)?;
            counter.boolean(strike.eliminated_target)?;
        }
        PresentationEventKind::TerrainChanged {
            terrain_generation,
            dirty_rectangles,
        } => {
            counter.u32(*terrain_generation)?;
            counter.length(dirty_rectangles.len())?;
            for rectangle in dirty_rectangles {
                retained_cell_rectangle_bytes(counter, *rectangle)?;
            }
        }
        PresentationEventKind::BlockChanged {
            block_id,
            previous_health,
            new_health,
            previous_surviving_bounds,
            new_surviving_bounds,
        } => {
            counter.u32(*block_id)?;
            retained_optional_u16_bytes(counter, *previous_health)?;
            retained_optional_u16_bytes(counter, *new_health)?;
            retained_optional_rectangle_bytes(counter, *previous_surviving_bounds)?;
            retained_optional_rectangle_bytes(counter, *new_surviving_bounds)?;
        }
        PresentationEventKind::HealthChanged {
            player_id,
            previous_health,
            new_health,
            breakdown,
        } => {
            counter.string(player_id)?;
            counter.u16(*previous_health)?;
            counter.u16(*new_health)?;
            counter.boolean(breakdown.is_some())?;
            if let Some(breakdown) = breakdown {
                retained_damage_breakdown_bytes(counter, breakdown)?;
            }
        }
        PresentationEventKind::GaugeChanged {
            player_id,
            previous_gauge,
            new_gauge,
            delta,
        } => {
            counter.string(player_id)?;
            counter.u16(*previous_gauge)?;
            counter.u16(*new_gauge)?;
            counter.i32(*delta)?;
        }
        PresentationEventKind::StatusChanged {
            player_id,
            kind,
            previous,
            current,
        } => {
            counter.string(player_id)?;
            retained_client_status_kind_bytes(counter, *kind)?;
            retained_optional_status_bytes(counter, previous.as_ref())?;
            retained_optional_status_bytes(counter, current.as_ref())?;
        }
        PresentationEventKind::EntityMoved {
            player_id,
            start,
            end,
            cause,
        } => {
            counter.string(player_id)?;
            retained_position_bytes(counter, *start)?;
            retained_position_bytes(counter, *end)?;
            retained_entity_movement_cause_bytes(counter, *cause)?;
        }
        PresentationEventKind::ObjectSpawned { object } => {
            retained_persistent_object_bytes(counter, object)?;
        }
        PresentationEventKind::ObjectChanged { previous, current } => {
            retained_persistent_object_bytes(counter, previous)?;
            retained_persistent_object_bytes(counter, current)?;
        }
        PresentationEventKind::ObjectRemoved { previous, cause } => {
            retained_persistent_object_bytes(counter, previous)?;
            retained_change_provenance_bytes(counter, *cause)?;
        }
        PresentationEventKind::PlayerEliminated { player_id, cause } => {
            counter.string(player_id)?;
            retained_change_provenance_bytes(counter, *cause)?;
        }
        PresentationEventKind::PassiveChoiceRequired {
            player_id,
            passive_ids,
        } => {
            counter.string(player_id)?;
            counter.length(passive_ids.len())?;
            for passive_id in passive_ids {
                counter.string(passive_id)?;
            }
        }
        PresentationEventKind::PassiveChosen {
            player_id,
            passive_id,
        } => {
            counter.string(player_id)?;
            counter.string(passive_id)?;
        }
        PresentationEventKind::TurnEnded { player_id, reason } => {
            counter.string(player_id)?;
            retained_turn_end_reason_bytes(counter, *reason)?;
        }
        PresentationEventKind::TurnOpened {
            player_id,
            turn_number,
        } => {
            counter.string(player_id)?;
            counter.u32(*turn_number)?;
        }
        PresentationEventKind::MatchCompleted { outcome } => {
            retained_match_outcome_bytes(counter, *outcome)?;
        }
    }
    Some(())
}

fn retained_projectile_trace_bytes(
    counter: &mut RetainedByteCounter,
    trace: &ProjectileTraceEvent,
) -> Option<()> {
    let ProjectileTraceEvent {
        trace_id,
        owner_id,
        ability_id,
        samples,
        terminal_impact,
    } = trace;
    counter.u32(*trace_id)?;
    counter.string(owner_id)?;
    counter.string(ability_id)?;
    counter.length(samples.len())?;
    for sample in samples {
        retained_projectile_sample_bytes(counter, *sample)?;
    }
    retained_impact_bytes(counter, *terminal_impact)
}

fn retained_projectile_sample_bytes(
    counter: &mut RetainedByteCounter,
    sample: ProjectileSampleSnapshot,
) -> Option<()> {
    let ProjectileSampleSnapshot { tick, position } = sample;
    counter.u32(tick)?;
    retained_position_bytes(counter, position)
}

fn retained_impact_bytes(counter: &mut RetainedByteCounter, impact: ImpactSnapshot) -> Option<()> {
    let ImpactSnapshot {
        position,
        tick,
        cause,
    } = impact;
    retained_position_bytes(counter, position)?;
    counter.u32(tick)?;
    retained_impact_cause_bytes(counter, cause)
}

fn retained_impact_cause_bytes(
    counter: &mut RetainedByteCounter,
    cause: ClientImpactCause,
) -> Option<()> {
    match cause {
        ClientImpactCause::Terrain
        | ClientImpactCause::Character
        | ClientImpactCause::OutOfBounds
        | ClientImpactCause::Expired => counter.tag(),
    }
}

fn retained_cell_rectangle_bytes(
    counter: &mut RetainedByteCounter,
    rectangle: CellRectangle,
) -> Option<()> {
    let CellRectangle {
        x,
        y,
        width,
        height,
    } = rectangle;
    counter.i32(x)?;
    counter.i32(y)?;
    counter.u32(width)?;
    counter.u32(height)
}

fn retained_optional_rectangle_bytes(
    counter: &mut RetainedByteCounter,
    rectangle: Option<CellRectangle>,
) -> Option<()> {
    counter.boolean(rectangle.is_some())?;
    if let Some(rectangle) = rectangle {
        retained_cell_rectangle_bytes(counter, rectangle)?;
    }
    Some(())
}

fn retained_optional_u16_bytes(
    counter: &mut RetainedByteCounter,
    value: Option<u16>,
) -> Option<()> {
    counter.boolean(value.is_some())?;
    if let Some(value) = value {
        counter.u16(value)?;
    }
    Some(())
}

fn retained_damage_breakdown_bytes(
    counter: &mut RetainedByteCounter,
    breakdown: &DamageBreakdown,
) -> Option<()> {
    let DamageBreakdown {
        direct,
        splash,
        backlash,
        hazard,
        wall_impact,
        healed,
        was_critical,
        knockback,
        eliminated,
    } = breakdown;
    counter.u16(*direct)?;
    counter.u16(*splash)?;
    counter.u16(*backlash)?;
    counter.u16(*hazard)?;
    counter.u16(*wall_impact)?;
    counter.u16(*healed)?;
    counter.boolean(*was_critical)?;
    retained_position_bytes(counter, *knockback)?;
    counter.boolean(*eliminated)
}

fn retained_entity_movement_cause_bytes(
    counter: &mut RetainedByteCounter,
    cause: EntityMovementCause,
) -> Option<()> {
    match cause {
        EntityMovementCause::RequestedMove | EntityMovementCause::AuthoritativeResolution => {
            counter.tag()
        }
    }
}

fn retained_change_provenance_bytes(
    counter: &mut RetainedByteCounter,
    cause: ChangeProvenance,
) -> Option<()> {
    match cause {
        ChangeProvenance::RecordedOutcome | ChangeProvenance::AuthoritativeResolution => {
            counter.tag()
        }
    }
}

fn retained_turn_end_reason_bytes(
    counter: &mut RetainedByteCounter,
    reason: ClientTurnEndReason,
) -> Option<()> {
    match reason {
        ClientTurnEndReason::Attacked
        | ClientTurnEndReason::Passed
        | ClientTurnEndReason::TimedOut
        | ClientTurnEndReason::Eliminated => counter.tag(),
    }
}

fn retained_match_outcome_bytes(
    counter: &mut RetainedByteCounter,
    outcome: ClientMatchOutcome,
) -> Option<()> {
    counter.tag()?;
    match outcome {
        ClientMatchOutcome::InProgress | ClientMatchOutcome::Draw => {}
        ClientMatchOutcome::Victory { team } => counter.u8(team)?,
    }
    Some(())
}

fn retained_position_bytes(
    counter: &mut RetainedByteCounter,
    position: PositionSnapshot,
) -> Option<()> {
    let PositionSnapshot { x, y } = position;
    counter.i32(x)?;
    counter.i32(y)
}

fn retained_match_snapshot_bytes(
    counter: &mut RetainedByteCounter,
    snapshot: &MatchSnapshot,
) -> Option<()> {
    let MatchSnapshot {
        client_contract_version,
        simulation_version,
        content_version,
        generation,
        tick,
        turn_number,
        phase,
        active_player_id,
        current_and_upcoming_player_ids,
        wind_per_tick,
        movement_remaining,
        has_attacked_this_turn,
        outcome,
        terrain_width,
        terrain_height,
        terrain_generation,
        blocks,
        players,
        persistent_objects,
        authoritative_state_hash,
    } = snapshot;
    counter.u32(*client_contract_version)?;
    counter.u32(*simulation_version)?;
    counter.u32(*content_version)?;
    counter.u64(*generation)?;
    counter.u64(*tick)?;
    counter.u32(*turn_number)?;
    retained_match_phase_bytes(counter, *phase)?;
    retained_optional_string_bytes(counter, active_player_id.as_deref())?;
    counter.length(current_and_upcoming_player_ids.len())?;
    for player_id in current_and_upcoming_player_ids {
        counter.string(player_id)?;
    }
    counter.i32(*wind_per_tick)?;
    counter.i32(*movement_remaining)?;
    counter.boolean(*has_attacked_this_turn)?;
    retained_match_outcome_bytes(counter, *outcome)?;
    counter.u32(*terrain_width)?;
    counter.u32(*terrain_height)?;
    counter.u32(*terrain_generation)?;
    counter.length(blocks.len())?;
    for block in blocks {
        retained_block_snapshot_bytes(counter, block)?;
    }
    counter.length(players.len())?;
    for player in players {
        retained_player_snapshot_bytes(counter, player)?;
    }
    counter.length(persistent_objects.len())?;
    for object in persistent_objects {
        retained_persistent_object_bytes(counter, object)?;
    }
    counter.string(authoritative_state_hash)
}

fn retained_match_phase_bytes(
    counter: &mut RetainedByteCounter,
    phase: ClientMatchPhase,
) -> Option<()> {
    match phase {
        ClientMatchPhase::MatchIntro
        | ClientMatchPhase::TurnStart
        | ClientMatchPhase::Movement
        | ClientMatchPhase::AimingAndSelection
        | ClientMatchPhase::PassiveSelection
        | ClientMatchPhase::CommandLocked
        | ClientMatchPhase::Resolution
        | ClientMatchPhase::Settling
        | ClientMatchPhase::StatusResolution
        | ClientMatchPhase::VictoryCheck
        | ClientMatchPhase::MatchComplete => counter.tag(),
    }
}

fn retained_block_snapshot_bytes(
    counter: &mut RetainedByteCounter,
    block: &BlockSnapshot,
) -> Option<()> {
    let BlockSnapshot {
        id,
        origin_cell_x,
        origin_cell_y,
        width_cells,
        height_cells,
        material,
        health,
        max_health,
        erosion_axis,
    } = block;
    counter.u32(*id)?;
    counter.i32(*origin_cell_x)?;
    counter.i32(*origin_cell_y)?;
    counter.u16(*width_cells)?;
    counter.u16(*height_cells)?;
    retained_material_bytes(counter, *material)?;
    counter.u16(*health)?;
    counter.u16(*max_health)?;
    retained_erosion_axis_bytes(counter, *erosion_axis)
}

fn retained_material_bytes(
    counter: &mut RetainedByteCounter,
    material: ClientMaterial,
) -> Option<()> {
    match material {
        ClientMaterial::Empty
        | ClientMaterial::Soil
        | ClientMaterial::Wood
        | ClientMaterial::ReinforcedStone => counter.tag(),
    }
}

fn retained_erosion_axis_bytes(
    counter: &mut RetainedByteCounter,
    erosion_axis: ClientErosionAxis,
) -> Option<()> {
    match erosion_axis {
        ClientErosionAxis::Columns | ClientErosionAxis::Rows => counter.tag(),
    }
}

fn retained_player_snapshot_bytes(
    counter: &mut RetainedByteCounter,
    player: &PlayerSnapshot,
) -> Option<()> {
    let PlayerSnapshot {
        id,
        team,
        health,
        is_eliminated,
        max_health,
        position,
        character_id,
        passive_id,
        special_gauge,
        has_chosen_passive,
        statuses,
        appearance,
    } = player;
    counter.string(id)?;
    counter.u8(*team)?;
    counter.u16(*health)?;
    counter.boolean(*is_eliminated)?;
    counter.u16(*max_health)?;
    retained_position_bytes(counter, *position)?;
    counter.string(character_id)?;
    retained_optional_string_bytes(counter, passive_id.as_deref())?;
    counter.u16(*special_gauge)?;
    counter.boolean(*has_chosen_passive)?;
    counter.length(statuses.len())?;
    for status in statuses {
        retained_status_snapshot_bytes(counter, status)?;
    }
    retained_appearance_snapshot_bytes(counter, appearance)
}

fn retained_status_snapshot_bytes(
    counter: &mut RetainedByteCounter,
    status: &StatusSnapshot,
) -> Option<()> {
    let StatusSnapshot {
        kind,
        magnitude,
        turns_remaining,
    } = status;
    retained_client_status_kind_bytes(counter, *kind)?;
    counter.i32(*magnitude)?;
    counter.u8(*turns_remaining)
}

fn retained_optional_status_bytes(
    counter: &mut RetainedByteCounter,
    status: Option<&StatusSnapshot>,
) -> Option<()> {
    counter.boolean(status.is_some())?;
    if let Some(status) = status {
        retained_status_snapshot_bytes(counter, status)?;
    }
    Some(())
}

fn retained_client_status_kind_bytes(
    counter: &mut RetainedByteCounter,
    kind: ClientStatusKind,
) -> Option<()> {
    match kind {
        ClientStatusKind::Knockback
        | ClientStatusKind::Chill
        | ClientStatusKind::Cluster
        | ClientStatusKind::Embers
        | ClientStatusKind::Tunnel
        | ClientStatusKind::Return
        | ClientStatusKind::Recoil
        | ClientStatusKind::SelfDamage
        | ClientStatusKind::Teleport
        | ClientStatusKind::Pull
        | ClientStatusKind::Push
        | ClientStatusKind::WallImpact
        | ClientStatusKind::Lockdown
        | ClientStatusKind::SpawnTurret
        | ClientStatusKind::Heal
        | ClientStatusKind::HealthTransfer
        | ClientStatusKind::MultiStrike
        | ClientStatusKind::GuaranteeCrit
        | ClientStatusKind::EmbedProjectile
        | ClientStatusKind::ChainDetonate
        | ClientStatusKind::Relocate
        | ClientStatusKind::Obscure => counter.tag(),
    }
}

fn retained_appearance_snapshot_bytes(
    counter: &mut RetainedByteCounter,
    appearance: &AppearanceSnapshot,
) -> Option<()> {
    let AppearanceSnapshot {
        skin_id,
        ability_skin_ids,
        victory_pose_id,
    } = appearance;
    counter.string(skin_id)?;
    counter.length(ability_skin_ids.len())?;
    for ability_skin_id in ability_skin_ids {
        counter.string(ability_skin_id)?;
    }
    counter.string(victory_pose_id)
}

fn retained_persistent_object_bytes(
    counter: &mut RetainedByteCounter,
    object: &PersistentObjectSnapshot,
) -> Option<()> {
    let PersistentObjectSnapshot {
        sequence,
        owner_id,
        kind,
        position,
        health,
        turns_remaining,
    } = object;
    counter.u32(*sequence)?;
    counter.string(owner_id)?;
    retained_object_kind_bytes(counter, *kind)?;
    retained_position_bytes(counter, *position)?;
    counter.u16(*health)?;
    counter.u8(*turns_remaining)
}

fn retained_object_kind_bytes(
    counter: &mut RetainedByteCounter,
    kind: ClientObjectKind,
) -> Option<()> {
    match kind {
        ClientObjectKind::Turret | ClientObjectKind::EmbeddedKnife | ClientObjectKind::GasCloud => {
            counter.tag()
        }
    }
}

/// Owns one authoritative host plus session publication and idempotency metadata.
#[derive(Debug, Clone)]
pub struct MatchSessionHost {
    host: MatchHost,
    generation: u64,
    ledger: BTreeMap<String, LedgerEntry>,
    ledger_entry_limit: usize,
    ledger_bytes: u64,
    ledger_byte_limit: u64,
    closed: bool,
}

impl MatchSessionHost {
    /// Validates `config`, creates the authoritative match, and opens generation zero.
    ///
    /// # Errors
    ///
    /// Propagates validation, map construction, and scheduler errors from
    /// [`create_match`].
    pub fn create(config: &MatchConfig) -> SimResult<Self> {
        create_match(config).map(Self::from_new_host)
    }

    /// Current publication generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Creates one detached snapshot of the current host and generation.
    #[must_use]
    pub fn snapshot(&self) -> MatchSnapshot {
        MatchSnapshot::from_host(&self.host, self.generation)
    }

    /// Read-only access to the authoritative host for direct-server integration and tests.
    ///
    /// There is deliberately no mutable counterpart: bypassing [`Self::apply`] would skip
    /// generation and idempotency bookkeeping.
    #[must_use]
    pub const fn host(&self) -> &MatchHost {
        &self.host
    }

    /// Number of first well-formed receipts retained for idempotent replay.
    #[must_use]
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// Deterministic canonical request/response bytes retained for exact replay.
    ///
    /// This measures the complete typed command and transition for each first receipt;
    /// allocator bookkeeping, map indexes, and the derived digest are deliberately excluded.
    /// See [`COMMAND_LEDGER_BYTE_LIMIT`] for the stable encoding rules.
    #[must_use]
    pub const fn ledger_bytes(&self) -> u64 {
        self.ledger_bytes
    }

    /// Whether a terminal resource/invariant fault has closed the session.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// Applies one normalized command atomically and retains its first result.
    ///
    /// Exact duplicates return the recorded original transition with
    /// [`TransitionDisposition::DuplicateReplay`], even if the live match has since
    /// advanced. Consumers must therefore treat a duplicate transition as acknowledgement
    /// of the original receipt, never as a new current-state snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`SessionFault`] for malformed normalized input, unsupported schemas,
    /// simulation faults, exhausted generation/resource bounds, or a previously closed
    /// session. Gameplay refusals are successful calls with a rejected transition.
    pub fn apply(&mut self, command: MatchCommand) -> Result<MatchTransition, SessionFault> {
        if self.closed {
            return Err(SessionFault::Closed);
        }
        command.validate_structure()?;
        let digest = command.canonical_digest();

        if let Some(entry) = self.ledger.get(&command.command_id) {
            if entry.command == command {
                if entry.canonical_digest != digest {
                    self.closed = true;
                    return Err(SessionFault::ContractInvariant);
                }
                let mut replay = entry.transition.clone();
                replay.disposition = TransitionDisposition::DuplicateReplay;
                return Ok(replay);
            }

            return Ok(
                self.current_rejection(&command.command_id, TransitionRejection::CommandIdConflict)
            );
        }

        // Check before resolving against a working host. First receipts are never evicted,
        // including rejections, so there is no safe recovery after this bound is reached.
        if self.ledger.len() >= self.ledger_entry_limit {
            self.closed = true;
            return Err(SessionFault::ResourceLimit);
        }

        if command.expected_snapshot_generation != self.generation {
            let rejection = TransitionRejection::SnapshotGenerationMismatch {
                expected: command.expected_snapshot_generation,
                actual: self.generation,
            };
            return self.record_current_rejection(command, digest, rejection);
        }

        if let Some(reason) = preflight_rejection(self.host.state(), &command) {
            return self.record_current_rejection(
                command,
                digest,
                TransitionRejection::Core(reason),
            );
        }

        let pre_state = self.host.state().clone();
        let pre_snapshot = self.snapshot();
        let mut working_host = self.host.clone();
        let command_outcome = match apply_to_working_host(&mut working_host, &command)? {
            AppliedCommand::Accepted(outcome) => outcome,
            AppliedCommand::Rejected(reason) => {
                if working_host.state() != self.host.state() {
                    self.closed = true;
                    return Err(SessionFault::ContractInvariant);
                }
                return self.record_current_rejection(
                    command,
                    digest,
                    TransitionRejection::Core(reason),
                );
            }
        };

        let mutated = working_host.state() != self.host.state();
        let post_generation = if mutated {
            let Some(next) = self.generation.checked_add(1) else {
                self.closed = true;
                return Err(SessionFault::GenerationExhausted);
            };
            next
        } else {
            self.generation
        };
        let post_snapshot = MatchSnapshot::from_host(&working_host, post_generation);

        if command_outcome.as_ref().is_some_and(|outcome| {
            outcome.final_state_hash != post_snapshot.authoritative_state_hash
        }) {
            self.closed = true;
            return Err(SessionFault::ContractInvariant);
        }

        let events = match derive_events(
            &command,
            &pre_state,
            working_host.state(),
            &pre_snapshot,
            &post_snapshot,
            command_outcome.as_ref(),
        ) {
            Ok(events) => events,
            Err(fault) => {
                self.closed = true;
                return Err(fault);
            }
        };
        let input_lock_ticks = events
            .last()
            .map_or(0, |event| event.presentation_tick)
            .saturating_add(POST_ACTION_LOCK_TICKS);
        let post_state_hash = post_snapshot.authoritative_state_hash.clone();
        let transition = MatchTransition {
            schema_version: CLIENT_CONTRACT_VERSION,
            command_id: command.command_id.clone(),
            disposition: TransitionDisposition::Accepted,
            rejection_reason: None,
            pre_snapshot_generation: self.generation,
            post_snapshot_generation: post_generation,
            presentation_tick_rate: FIXED_TICK_RATE,
            input_lock_ticks,
            events,
            post_snapshot,
            post_state_hash,
        };

        self.record_and_commit(
            command,
            digest,
            transition,
            Some(working_host),
            post_generation,
        )
    }

    fn from_new_host(host: MatchHost) -> Self {
        Self {
            host,
            generation: 0,
            ledger: BTreeMap::new(),
            ledger_entry_limit: COMMAND_LEDGER_ENTRY_LIMIT,
            ledger_bytes: 0,
            ledger_byte_limit: COMMAND_LEDGER_BYTE_LIMIT,
            closed: false,
        }
    }

    #[cfg(test)]
    fn with_ledger_entry_limit(host: MatchHost, ledger_entry_limit: usize) -> Self {
        let mut session = Self::from_new_host(host);
        session.ledger_entry_limit = ledger_entry_limit;
        session
    }

    #[cfg(test)]
    fn with_ledger_limits(
        host: MatchHost,
        ledger_entry_limit: usize,
        ledger_byte_limit: u64,
    ) -> Self {
        let mut session = Self::from_new_host(host);
        session.ledger_entry_limit = ledger_entry_limit;
        session.ledger_byte_limit = ledger_byte_limit;
        session
    }

    fn current_rejection(
        &self,
        command_id: &str,
        rejection: TransitionRejection,
    ) -> MatchTransition {
        let snapshot = self.snapshot();
        let post_state_hash = snapshot.authoritative_state_hash.clone();
        MatchTransition {
            schema_version: CLIENT_CONTRACT_VERSION,
            command_id: command_id.to_owned(),
            disposition: TransitionDisposition::Rejected,
            rejection_reason: Some(rejection),
            pre_snapshot_generation: self.generation,
            post_snapshot_generation: self.generation,
            presentation_tick_rate: FIXED_TICK_RATE,
            input_lock_ticks: 0,
            events: Vec::new(),
            post_snapshot: snapshot,
            post_state_hash,
        }
    }

    fn record_current_rejection(
        &mut self,
        command: MatchCommand,
        digest: String,
        rejection: TransitionRejection,
    ) -> Result<MatchTransition, SessionFault> {
        let transition = self.current_rejection(&command.command_id, rejection);
        self.record_and_commit(command, digest, transition, None, self.generation)
    }

    fn record_and_commit(
        &mut self,
        command: MatchCommand,
        canonical_digest: String,
        transition: MatchTransition,
        working_host: Option<MatchHost>,
        post_generation: u64,
    ) -> Result<MatchTransition, SessionFault> {
        use std::collections::btree_map::Entry;

        let Some(entry_bytes) = retained_ledger_entry_bytes(&command, &transition) else {
            self.closed = true;
            return Err(SessionFault::ResourceLimit);
        };
        let Some(next_ledger_bytes) = self.ledger_bytes.checked_add(entry_bytes) else {
            self.closed = true;
            return Err(SessionFault::ResourceLimit);
        };
        if next_ledger_bytes > self.ledger_byte_limit {
            self.closed = true;
            return Err(SessionFault::ResourceLimit);
        }

        let key = command.command_id.clone();
        let retained_transition = transition.clone();
        match self.ledger.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(LedgerEntry {
                    command,
                    canonical_digest,
                    transition: retained_transition,
                });
            }
            Entry::Occupied(_) => {
                self.closed = true;
                return Err(SessionFault::ContractInvariant);
            }
        }

        // No fallible work follows this point. The host, generation, and ledger publication
        // become visible as one session operation.
        if let Some(host) = working_host {
            self.host = host;
        }
        self.generation = post_generation;
        self.ledger_bytes = next_ledger_bytes;
        Ok(transition)
    }
}

enum AppliedCommand {
    Accepted(Option<CommandOutcome>),
    Rejected(CommandRejection),
}

fn apply_to_working_host(
    host: &mut MatchHost,
    command: &MatchCommand,
) -> Result<AppliedCommand, SessionFault> {
    match &command.kind {
        MatchCommandKind::Move { dx } => {
            let _travelled = host.submit_move(&command.player_id, *dx)?;
            Ok(AppliedCommand::Accepted(None))
        }
        MatchCommandKind::Ability {
            slot,
            angle_millidegrees,
            power_basis_points,
            target_player_id,
            secondary_target_player_id,
        } => {
            let ability = AbilityCommand {
                command_id: command.command_id.clone(),
                player_id: command.player_id.clone(),
                expected_turn_number: command.expected_turn_number,
                slot: *slot,
                angle_millidegrees: *angle_millidegrees,
                power_basis_points: *power_basis_points,
                target_player_id: target_player_id.clone(),
                secondary_target_player_id: secondary_target_player_id.clone(),
            };
            match host.submit_ability(&ability)? {
                CommandResult::Accepted(outcome) => Ok(AppliedCommand::Accepted(Some(*outcome))),
                CommandResult::Rejected(reason) => Ok(AppliedCommand::Rejected(reason)),
            }
        }
        MatchCommandKind::PassiveChoice { passive_id } => {
            let choice = PassiveChoiceCommand {
                command_id: command.command_id.clone(),
                player_id: command.player_id.clone(),
                expected_turn_number: command.expected_turn_number,
                passive_id: passive_id.clone(),
            };
            match host.submit_passive_choice(&choice)? {
                CommandResult::Accepted(outcome) => Ok(AppliedCommand::Accepted(Some(*outcome))),
                CommandResult::Rejected(reason) => Ok(AppliedCommand::Rejected(reason)),
            }
        }
        MatchCommandKind::Pass => {
            host.pass_turn()?;
            Ok(AppliedCommand::Accepted(None))
        }
    }
}

fn preflight_rejection(
    state: &SimulationState,
    command: &MatchCommand,
) -> Option<CommandRejection> {
    if command.expected_turn_number != state.turn_number {
        return Some(CommandRejection::TurnVersionMismatch);
    }
    if command.player_id != state.active_player_id {
        return Some(CommandRejection::NotActivePlayer);
    }
    if state
        .player(&command.player_id)
        .is_none_or(crate::types::PlayerState::is_eliminated)
    {
        return Some(CommandRejection::PlayerEliminated);
    }

    let phase_is_valid = match command.kind {
        MatchCommandKind::Move { .. }
        | MatchCommandKind::Ability { .. }
        | MatchCommandKind::Pass => state.phase.accepts_ability_command(),
        MatchCommandKind::PassiveChoice { .. } => state.phase == MatchPhase::PassiveSelection,
    };
    (!phase_is_valid).then_some(CommandRejection::WrongPhase)
}

#[derive(Debug)]
struct PendingEvent {
    presentation_tick: u32,
    rank: u8,
    insertion_order: u32,
    kind: PresentationEventKind,
}

fn derive_events(
    command: &MatchCommand,
    pre_state: &SimulationState,
    post_state: &SimulationState,
    pre_snapshot: &MatchSnapshot,
    post_snapshot: &MatchSnapshot,
    command_outcome: Option<&CommandOutcome>,
) -> Result<Vec<PresentationEvent>, SessionFault> {
    let mut pending = Vec::new();
    let mut resolution_tick = 0u32;

    if let MatchCommandKind::Ability { slot, .. } = command.kind {
        let Some(player) = pre_state.player(&command.player_id) else {
            return Err(SessionFault::ContractInvariant);
        };
        let Some(definition) = character::find(&player.character_id) else {
            return Err(SessionFault::ContractInvariant);
        };
        let Some(ability) = definition.ability(slot) else {
            return Err(SessionFault::ContractInvariant);
        };

        let Some(outcome) = command_outcome else {
            return Err(SessionFault::ContractInvariant);
        };
        let mut trace_ids = BTreeSet::new();
        for trace in &outcome.projectile_traces {
            if !trace_ids.insert(trace.sequence)
                || !trace
                    .samples
                    .windows(2)
                    .all(|pair| matches!(pair, [left, right] if left.tick <= right.tick))
            {
                return Err(SessionFault::ContractInvariant);
            }
            let impact = snapshot_impact(trace.impact);
            resolution_tick = resolution_tick.max(impact.tick);
            let samples = trace
                .samples
                .iter()
                .copied()
                .map(snapshot_projectile_sample)
                .collect();
            let trace_event = ProjectileTraceEvent {
                trace_id: trace.sequence,
                owner_id: command.player_id.clone(),
                ability_id: ability.id.to_owned(),
                samples,
                terminal_impact: impact,
            };
            push_pending(
                &mut pending,
                0,
                0,
                PresentationEventKind::ProjectileTrace(trace_event),
            )?;
            push_pending(
                &mut pending,
                impact.tick,
                2,
                PresentationEventKind::Impact {
                    trace_id: trace.sequence,
                    impact,
                },
            )?;
        }

        // One event per strike the resolver actually performed. The count comes from the
        // outcome, never from `ability.strikes_per_turn`: a three-strike ability that found
        // no target performs zero, and a client told otherwise would animate phantom hits.
        for strike in &outcome.strikes {
            // A projectile-delivered strike is presented at its own trace's impact tick, so
            // the damage lands with the projectile rather than at the start of the turn.
            let (tick, rank) = match strike.delivery {
                StrikeDelivery::Projectile { trace_sequence } => {
                    let Some(trace) = outcome
                        .projectile_traces
                        .iter()
                        .find(|t| t.sequence == trace_sequence)
                    else {
                        // A strike citing a trace the outcome does not contain means the
                        // two halves of the record disagree; never paper over it.
                        return Err(SessionFault::ContractInvariant);
                    };
                    (trace.impact.tick, 3)
                }
                StrikeDelivery::Melee => (0, 1),
            };
            push_pending(
                &mut pending,
                tick,
                rank,
                PresentationEventKind::StrikeResolved {
                    owner_id: command.player_id.clone(),
                    ability_id: ability.id.to_owned(),
                    strike: strike.clone(),
                },
            )?;
        }
    }

    let dirty_rectangles = terrain_dirty_rectangles(pre_state, post_state)?;
    let terrain_generation_changed =
        pre_state.next_terrain_sequence != post_state.next_terrain_sequence;
    if !dirty_rectangles.is_empty() && !terrain_generation_changed {
        // Changed terrain without a generation makes the coarse terrain cache incoherent.
        return Err(SessionFault::ContractInvariant);
    }
    if terrain_generation_changed {
        push_pending(
            &mut pending,
            resolution_tick,
            10,
            PresentationEventKind::TerrainChanged {
                terrain_generation: post_state.next_terrain_sequence,
                dirty_rectangles,
            },
        )?;
    }

    let block_ids: BTreeSet<u32> = pre_state
        .blocks
        .iter()
        .chain(post_state.blocks.iter())
        .map(|block| block.id)
        .collect();
    for block_id in block_ids {
        let previous = pre_state.blocks.iter().find(|block| block.id == block_id);
        let current = post_state.blocks.iter().find(|block| block.id == block_id);
        if previous == current {
            continue;
        }
        push_pending(
            &mut pending,
            resolution_tick,
            11,
            PresentationEventKind::BlockChanged {
                block_id,
                previous_health: previous.map(|block| block.health),
                new_health: current.map(|block| block.health),
                previous_surviving_bounds: surviving_block_bounds(pre_state, block_id)?,
                new_surviving_bounds: surviving_block_bounds(post_state, block_id)?,
            },
        )?;
    }

    for post_player in &post_snapshot.players {
        let Some(pre_player) = pre_snapshot
            .players
            .iter()
            .find(|player| player.id == post_player.id)
        else {
            return Err(SessionFault::ContractInvariant);
        };
        let breakdown = damage_breakdown(command_outcome, &post_player.id);
        let recorded_elimination = breakdown.as_ref().is_some_and(|item| item.eliminated);

        if pre_player.position != post_player.position {
            // `submit_move` walks and settles synchronously. A net pre/post displacement cannot
            // prove which segment each coordinate came from: a climb may even settle back to its
            // starting height. Until MatchHost retains the intermediate path, labelling any part
            // as requested movement would invent provenance.
            let cause = EntityMovementCause::AuthoritativeResolution;
            push_pending(
                &mut pending,
                resolution_tick,
                12,
                PresentationEventKind::EntityMoved {
                    player_id: post_player.id.clone(),
                    start: pre_player.position,
                    end: post_player.position,
                    cause,
                },
            )?;
        }

        if pre_player.health != post_player.health || breakdown.is_some() {
            push_pending(
                &mut pending,
                resolution_tick,
                13,
                PresentationEventKind::HealthChanged {
                    player_id: post_player.id.clone(),
                    previous_health: pre_player.health,
                    new_health: post_player.health,
                    breakdown,
                },
            )?;
        }

        if pre_player.special_gauge != post_player.special_gauge {
            let delta = i32::from(post_player.special_gauge)
                .saturating_sub(i32::from(pre_player.special_gauge));
            push_pending(
                &mut pending,
                resolution_tick,
                14,
                PresentationEventKind::GaugeChanged {
                    player_id: post_player.id.clone(),
                    previous_gauge: pre_player.special_gauge,
                    new_gauge: post_player.special_gauge,
                    delta,
                },
            )?;
        }

        let status_kinds: BTreeSet<ClientStatusKind> = pre_player
            .statuses
            .iter()
            .chain(post_player.statuses.iter())
            .map(|status| status.kind)
            .collect();
        for kind in status_kinds {
            let previous = pre_player
                .statuses
                .iter()
                .find(|status| status.kind == kind)
                .cloned();
            let current = post_player
                .statuses
                .iter()
                .find(|status| status.kind == kind)
                .cloned();
            if previous != current {
                push_pending(
                    &mut pending,
                    resolution_tick,
                    15,
                    PresentationEventKind::StatusChanged {
                        player_id: post_player.id.clone(),
                        kind,
                        previous,
                        current,
                    },
                )?;
            }
        }

        if pre_player.passive_id != post_player.passive_id
            && let Some(passive_id) = &post_player.passive_id
        {
            push_pending(
                &mut pending,
                resolution_tick,
                18,
                PresentationEventKind::PassiveChosen {
                    player_id: post_player.id.clone(),
                    passive_id: passive_id.clone(),
                },
            )?;
        }

        if !pre_player.is_eliminated && post_player.is_eliminated {
            let cause = if recorded_elimination {
                ChangeProvenance::RecordedOutcome
            } else {
                ChangeProvenance::AuthoritativeResolution
            };
            push_pending(
                &mut pending,
                resolution_tick,
                17,
                PresentationEventKind::PlayerEliminated {
                    player_id: post_player.id.clone(),
                    cause,
                },
            )?;
        }
    }

    for previous in &pre_snapshot.persistent_objects {
        if post_snapshot
            .persistent_objects
            .iter()
            .all(|current| current.sequence != previous.sequence)
        {
            push_pending(
                &mut pending,
                resolution_tick,
                16,
                PresentationEventKind::ObjectRemoved {
                    previous: previous.clone(),
                    cause: ChangeProvenance::AuthoritativeResolution,
                },
            )?;
        }
    }
    for current in &post_snapshot.persistent_objects {
        match pre_snapshot
            .persistent_objects
            .iter()
            .find(|previous| previous.sequence == current.sequence)
        {
            None => push_pending(
                &mut pending,
                resolution_tick,
                16,
                PresentationEventKind::ObjectSpawned {
                    object: current.clone(),
                },
            )?,
            Some(previous) if previous != current => push_pending(
                &mut pending,
                resolution_tick,
                16,
                PresentationEventKind::ObjectChanged {
                    previous: previous.clone(),
                    current: current.clone(),
                },
            )?,
            Some(_) => {}
        }
    }

    if pre_state.phase != MatchPhase::PassiveSelection
        && post_state.phase == MatchPhase::PassiveSelection
    {
        let Some(player) = post_state.player(&post_state.active_player_id) else {
            return Err(SessionFault::ContractInvariant);
        };
        let Some(definition) = character::find(&player.character_id) else {
            return Err(SessionFault::ContractInvariant);
        };
        let mut passive_ids: Vec<String> = definition
            .passives
            .iter()
            .map(|passive| passive.id.to_owned())
            .collect();
        passive_ids.sort();
        push_pending(
            &mut pending,
            resolution_tick,
            18,
            PresentationEventKind::PassiveChoiceRequired {
                player_id: player.id.clone(),
                passive_ids,
            },
        )?;
    }

    let turn_ended = pre_state.active_player_id != post_state.active_player_id
        || pre_state.turn_number != post_state.turn_number
        || (pre_state.phase != MatchPhase::MatchComplete
            && post_state.phase == MatchPhase::MatchComplete);
    if turn_ended && !pre_state.active_player_id.is_empty() {
        push_pending(
            &mut pending,
            resolution_tick,
            19,
            PresentationEventKind::TurnEnded {
                player_id: pre_state.active_player_id.clone(),
                reason: snapshot_turn_end_reason(post_state.last_turn_end_reason),
            },
        )?;
    }

    if pre_snapshot.outcome != post_snapshot.outcome
        && !matches!(post_snapshot.outcome, ClientMatchOutcome::InProgress)
    {
        push_pending(
            &mut pending,
            resolution_tick,
            20,
            PresentationEventKind::MatchCompleted {
                outcome: post_snapshot.outcome,
            },
        )?;
    }

    if turn_ended
        && post_state.phase != MatchPhase::MatchComplete
        && !post_state.active_player_id.is_empty()
    {
        push_pending(
            &mut pending,
            resolution_tick,
            21,
            PresentationEventKind::TurnOpened {
                player_id: post_state.active_player_id.clone(),
                turn_number: post_state.turn_number,
            },
        )?;
    }

    pending.sort_by_key(|event| (event.presentation_tick, event.rank, event.insertion_order));
    pending
        .into_iter()
        .enumerate()
        .map(|(index, event)| {
            let sequence = u32::try_from(index).map_err(|_| SessionFault::ContractInvariant)?;
            Ok(PresentationEvent {
                presentation_tick: event.presentation_tick,
                sequence,
                kind: event.kind,
            })
        })
        .collect()
}

fn push_pending(
    pending: &mut Vec<PendingEvent>,
    presentation_tick: u32,
    rank: u8,
    kind: PresentationEventKind,
) -> Result<(), SessionFault> {
    let insertion_order =
        u32::try_from(pending.len()).map_err(|_| SessionFault::ContractInvariant)?;
    pending.push(PendingEvent {
        presentation_tick,
        rank,
        insertion_order,
        kind,
    });
    Ok(())
}

fn terrain_dirty_rectangles(
    previous: &SimulationState,
    current: &SimulationState,
) -> Result<Vec<CellRectangle>, SessionFault> {
    if previous.terrain.width != current.terrain.width
        || previous.terrain.height != current.terrain.height
    {
        return Err(SessionFault::ContractInvariant);
    }
    let expected_len = terrain_cell_count(previous)?;
    if previous.terrain.cells.len() != expected_len || current.terrain.cells.len() != expected_len {
        return Err(SessionFault::ContractInvariant);
    }
    if expected_len == 0 {
        return Ok(Vec::new());
    }
    let width =
        usize::try_from(previous.terrain.width).map_err(|_| SessionFault::ContractInvariant)?;
    if width == 0 {
        return Err(SessionFault::ContractInvariant);
    }

    let mut rectangles = Vec::new();
    let mut current_run: Option<CellRectangle> = None;
    for (index, (before, after)) in previous
        .terrain
        .cells
        .iter()
        .zip(current.terrain.cells.iter())
        .enumerate()
    {
        if before == after {
            continue;
        }
        let Some(x_index) = index.checked_rem(width) else {
            return Err(SessionFault::ContractInvariant);
        };
        let Some(y_index) = index.checked_div(width) else {
            return Err(SessionFault::ContractInvariant);
        };
        let x = i32::try_from(x_index).map_err(|_| SessionFault::ContractInvariant)?;
        let y = i32::try_from(y_index).map_err(|_| SessionFault::ContractInvariant)?;

        if let Some(run) = current_run.as_mut() {
            let run_width =
                i32::try_from(run.width).map_err(|_| SessionFault::ContractInvariant)?;
            if run.y == y && run.x.checked_add(run_width) == Some(x) {
                run.width = run
                    .width
                    .checked_add(1)
                    .ok_or(SessionFault::ContractInvariant)?;
                continue;
            }
        }
        if let Some(run) = current_run.take() {
            rectangles.push(run);
        }
        current_run = Some(CellRectangle {
            x,
            y,
            width: 1,
            height: 1,
        });
    }
    if let Some(run) = current_run {
        rectangles.push(run);
    }
    Ok(rectangles)
}

fn terrain_cell_count(state: &SimulationState) -> Result<usize, SessionFault> {
    let cells = u64::from(state.terrain.width)
        .checked_mul(u64::from(state.terrain.height))
        .ok_or(SessionFault::ContractInvariant)?;
    usize::try_from(cells).map_err(|_| SessionFault::ContractInvariant)
}

fn surviving_block_bounds(
    state: &SimulationState,
    block_id: u32,
) -> Result<Option<CellRectangle>, SessionFault> {
    let Some(block) = state.blocks.iter().find(|block| block.id == block_id) else {
        return Ok(None);
    };
    if block.health == 0 {
        return Ok(None);
    }

    let mut min_x: Option<i32> = None;
    let mut min_y: Option<i32> = None;
    let mut max_x: Option<i32> = None;
    let mut max_y: Option<i32> = None;
    for y_offset in 0..u32::from(block.height_cells) {
        let y_offset_i32 = i32::try_from(y_offset).map_err(|_| SessionFault::ContractInvariant)?;
        let y = block
            .origin_cell_y
            .checked_add(y_offset_i32)
            .ok_or(SessionFault::ContractInvariant)?;
        for x_offset in 0..u32::from(block.width_cells) {
            let x_offset_i32 =
                i32::try_from(x_offset).map_err(|_| SessionFault::ContractInvariant)?;
            let x = block
                .origin_cell_x
                .checked_add(x_offset_i32)
                .ok_or(SessionFault::ContractInvariant)?;
            if terrain::material_at(&state.terrain, x, y) != block.material {
                continue;
            }
            min_x = Some(min_x.map_or(x, |value| value.min(x)));
            min_y = Some(min_y.map_or(y, |value| value.min(y)));
            max_x = Some(max_x.map_or(x, |value| value.max(x)));
            max_y = Some(max_y.map_or(y, |value| value.max(y)));
        }
    }

    let (Some(min_x), Some(min_y), Some(max_x), Some(max_y)) = (min_x, min_y, max_x, max_y) else {
        return Ok(None);
    };
    let width_i32 = max_x
        .checked_sub(min_x)
        .and_then(|value| value.checked_add(1))
        .ok_or(SessionFault::ContractInvariant)?;
    let height_i32 = max_y
        .checked_sub(min_y)
        .and_then(|value| value.checked_add(1))
        .ok_or(SessionFault::ContractInvariant)?;
    let width = u32::try_from(width_i32).map_err(|_| SessionFault::ContractInvariant)?;
    let height = u32::try_from(height_i32).map_err(|_| SessionFault::ContractInvariant)?;
    Ok(Some(CellRectangle {
        x: min_x,
        y: min_y,
        width,
        height,
    }))
}

fn damage_breakdown(outcome: Option<&CommandOutcome>, player_id: &str) -> Option<DamageBreakdown> {
    outcome
        .and_then(|value| {
            value
                .damage
                .iter()
                .find(|damage| damage.player_id == player_id)
        })
        .map(snapshot_damage)
}

const fn snapshot_damage(damage: &DamageEvent) -> DamageBreakdown {
    DamageBreakdown {
        direct: damage.direct,
        splash: damage.splash,
        backlash: damage.backlash,
        hazard: damage.hazard,
        wall_impact: damage.wall_impact,
        healed: damage.healed,
        was_critical: damage.was_critical,
        knockback: snapshot_position(damage.knockback),
        eliminated: damage.eliminated,
    }
}

const fn snapshot_projectile_sample(sample: BallisticSample) -> ProjectileSampleSnapshot {
    ProjectileSampleSnapshot {
        tick: sample.tick,
        position: snapshot_position(sample.position),
    }
}

const fn snapshot_impact(impact: BallisticImpact) -> ImpactSnapshot {
    ImpactSnapshot {
        position: snapshot_position(impact.position),
        tick: impact.tick,
        cause: match impact.cause {
            ImpactCause::Terrain => ClientImpactCause::Terrain,
            ImpactCause::Character => ClientImpactCause::Character,
            ImpactCause::OutOfBounds => ClientImpactCause::OutOfBounds,
            ImpactCause::Expired => ClientImpactCause::Expired,
        },
    }
}

const fn snapshot_position(position: crate::fixed::FixedPoint) -> PositionSnapshot {
    PositionSnapshot {
        x: position.x,
        y: position.y,
    }
}

const fn snapshot_turn_end_reason(reason: TurnEndReason) -> ClientTurnEndReason {
    match reason {
        TurnEndReason::Attacked => ClientTurnEndReason::Attacked,
        TurnEndReason::Passed => ClientTurnEndReason::Passed,
        TurnEndReason::TimedOut => ClientTurnEndReason::TimedOut,
        TurnEndReason::Eliminated => ClientTurnEndReason::Eliminated,
    }
}

const fn ability_slot_tag(slot: AbilitySlot) -> u8 {
    match slot {
        AbilitySlot::Basic => 0,
        AbilitySlot::BasicAlt => 1,
        AbilitySlot::Special => 2,
    }
}

fn write_optional_str(hasher: &mut CanonicalHasher, value: Option<&str>) {
    hasher.write_bool(value.is_some());
    if let Some(text) = value {
        hasher.write_str(text);
    }
}

fn is_valid_definition_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= crate::match_setup::MAX_PLAYER_ID_BYTES
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::*;
    use crate::POSITION_SCALE;
    use crate::hash::hash_state;
    use crate::match_setup::{MatchMode, MatchPlayerConfig, build_initial_state};
    use crate::types::{Appearance, Material};

    fn player(player_id: &str, team: u8, character_id: &str) -> MatchPlayerConfig {
        MatchPlayerConfig {
            player_id: player_id.to_owned(),
            team,
            character_id: character_id.to_owned(),
            appearance: Appearance::default(),
        }
    }

    fn duel() -> MatchConfig {
        MatchConfig {
            seed: 12_345,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                player("a-local-player", 0, "zeke"),
                player("b-local-bot", 1, "huck"),
            ],
        }
    }

    fn command(
        session: &MatchSessionHost,
        command_id: &str,
        kind: MatchCommandKind,
    ) -> MatchCommand {
        MatchCommand {
            schema_version: CLIENT_CONTRACT_VERSION,
            command_id: command_id.to_owned(),
            player_id: session.host().active_player().to_owned(),
            expected_turn_number: session.host().state().turn_number,
            expected_snapshot_generation: session.generation(),
            kind,
        }
    }

    fn pass_command(session: &MatchSessionHost, command_id: &str) -> MatchCommand {
        command(session, command_id, MatchCommandKind::Pass)
    }

    fn ability_command(session: &MatchSessionHost, command_id: &str) -> MatchCommand {
        command(
            session,
            command_id,
            MatchCommandKind::Ability {
                slot: AbilitySlot::Basic,
                angle_millidegrees: 45_000,
                power_basis_points: 1_500,
                target_player_id: None,
                secondary_target_player_id: None,
            },
        )
    }

    #[test]
    fn a_new_session_publishes_generation_zero_and_the_live_hash() {
        let session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let snapshot = session.snapshot();

        assert_eq!(session.generation(), 0);
        assert_eq!(snapshot.generation, 0);
        assert_eq!(
            snapshot.authoritative_state_hash,
            hash_state(session.host().state()),
        );
        assert_eq!(session.ledger_len(), 0);
        assert_eq!(session.ledger_bytes(), 0);
    }

    #[test]
    fn command_accounting_uses_the_documented_canonical_widths() {
        let session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let pass = pass_command(&session, "accounted-pass");
        let expected = RETAINED_TOP_LEVEL_HEADER_BYTES
            + 4
            + 4
            + u64::try_from(pass.command_id.len()).expect("fixture length must fit")
            + 4
            + u64::try_from(pass.player_id.len()).expect("fixture length must fit")
            + 4
            + 8
            + 1;

        assert_eq!(retained_command_bytes(&pass), Some(expected));
    }

    #[test]
    fn canonical_command_identity_covers_every_typed_field() {
        let session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let first = ability_command(&session, "canonical-command");
        let identical = first.clone();
        let mut changed = first.clone();
        let MatchCommandKind::Ability {
            power_basis_points, ..
        } = &mut changed.kind
        else {
            panic!("fixture command must be an ability");
        };
        *power_basis_points = power_basis_points.saturating_add(1);

        assert_eq!(first.canonical_digest(), identical.canonical_digest());
        assert_ne!(first.canonical_digest(), changed.canonical_digest());
    }

    #[test]
    fn movement_increments_generation_once_and_emits_the_authoritative_net_position() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let actor = session.host().active_player().to_owned();
        let move_command = command(
            &session,
            "move-one-cell",
            MatchCommandKind::Move { dx: POSITION_SCALE },
        );

        let transition = session
            .apply(move_command)
            .expect("valid movement must produce a transition");

        assert_eq!(transition.disposition, TransitionDisposition::Accepted);
        assert_eq!(transition.pre_snapshot_generation, 0);
        assert_eq!(transition.post_snapshot_generation, 1);
        assert_eq!(session.generation(), 1);
        assert!(transition.events.iter().any(|event| matches!(
            &event.kind,
            PresentationEventKind::EntityMoved { player_id, start, end, cause }
                if player_id == &actor
                    && start != end
                    && *cause == EntityMovementCause::AuthoritativeResolution
        )));
        assert_eq!(
            transition.post_state_hash,
            hash_state(session.host().state()),
        );
        assert_eq!(
            transition.post_state_hash,
            transition.post_snapshot.authoritative_state_hash,
        );
    }

    #[test]
    fn requested_move_cause_remains_reserved_without_intermediate_path_provenance() {
        for (command_id, dx) in [
            ("reserved-positive-move", POSITION_SCALE),
            ("reserved-negative-move", POSITION_SCALE.saturating_neg()),
        ] {
            let mut session =
                MatchSessionHost::create(&duel()).expect("fixture session must start");
            let move_command = command(&session, command_id, MatchCommandKind::Move { dx });
            let transition = session
                .apply(move_command)
                .expect("valid movement must produce a transition");

            assert!(transition.events.iter().all(|event| !matches!(
                &event.kind,
                PresentationEventKind::EntityMoved {
                    cause: EntityMovementCause::RequestedMove,
                    ..
                }
            )));
        }
    }

    #[test]
    fn an_accepted_zero_move_does_not_invent_a_generation() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let before_hash = hash_state(session.host().state());
        let zero_move = command(&session, "zero-move", MatchCommandKind::Move { dx: 0 });
        let retained_command = zero_move.clone();

        let transition = session
            .apply(zero_move)
            .expect("a bounded zero move is a valid no-change receipt");

        assert_eq!(transition.disposition, TransitionDisposition::Accepted);
        assert_eq!(transition.pre_snapshot_generation, 0);
        assert_eq!(transition.post_snapshot_generation, 0);
        assert_eq!(session.generation(), 0);
        assert_eq!(transition.post_state_hash, before_hash);
        assert!(transition.events.is_empty());
        assert_eq!(session.ledger_len(), 1);
        assert_eq!(
            session.ledger_bytes(),
            retained_ledger_entry_bytes(&retained_command, &transition)
                .expect("fixture entry must be countable"),
        );
    }

    #[test]
    fn an_ability_transition_has_independent_trace_and_impact_events() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let ability = ability_command(&session, "projectile-ability");

        let transition = session
            .apply(ability)
            .expect("valid basic ability must resolve");

        assert_eq!(transition.disposition, TransitionDisposition::Accepted);
        assert_eq!(transition.post_snapshot_generation, 1);
        assert!(transition.events.iter().any(|event| matches!(
            &event.kind,
            PresentationEventKind::ProjectileTrace(trace)
                if trace.trace_id == 0 && trace.samples.len() >= 2
        )));
        assert!(transition.events.iter().any(|event| matches!(
            event.kind,
            PresentationEventKind::Impact { trace_id: 0, .. }
        )));
        assert!(transition.events.windows(2).all(|pair| matches!(
            pair,
            [left, right]
                if (left.presentation_tick, left.sequence)
                    < (right.presentation_tick, right.sequence)
        )));
        assert_eq!(
            transition.post_state_hash,
            transition.post_snapshot.authoritative_state_hash,
        );
        assert_eq!(
            transition.post_state_hash,
            hash_state(session.host().state()),
        );
    }

    #[test]
    fn exact_duplicate_replays_the_original_transition_without_reconciliation() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let pass = pass_command(&session, "pass-once");
        let original_command = pass.clone();
        let original = session
            .apply(pass.clone())
            .expect("first pass must resolve");
        let live_generation = session.generation();
        let live_hash = hash_state(session.host().state());

        let duplicate = session.apply(pass).expect("duplicate must replay");

        assert_eq!(
            duplicate.disposition,
            TransitionDisposition::DuplicateReplay
        );
        assert_eq!(
            duplicate.pre_snapshot_generation,
            original.pre_snapshot_generation
        );
        assert_eq!(
            duplicate.post_snapshot_generation,
            original.post_snapshot_generation
        );
        assert_eq!(duplicate.post_snapshot, original.post_snapshot);
        assert_eq!(duplicate.events, original.events);
        assert_eq!(session.generation(), live_generation);
        assert_eq!(hash_state(session.host().state()), live_hash);
        assert_eq!(session.ledger_len(), 1);
        assert_eq!(
            session.ledger_bytes(),
            retained_ledger_entry_bytes(&original_command, &original)
                .expect("fixture entry must be countable"),
        );
    }

    #[test]
    fn rejected_first_receipt_is_retained_and_replayed_after_the_match_advances() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let mut stale = pass_command(&session, "stale-first");
        stale.expected_snapshot_generation = 99;
        let first = session
            .apply(stale.clone())
            .expect("staleness is a domain rejection");
        assert_eq!(first.disposition, TransitionDisposition::Rejected);
        assert_eq!(first.post_snapshot_generation, 0);
        let rejected_entry_bytes = retained_ledger_entry_bytes(&stale, &first)
            .expect("rejected fixture entry must be countable");
        assert_eq!(session.ledger_bytes(), rejected_entry_bytes);

        let valid_pass = pass_command(&session, "advance-after-rejection");
        session.apply(valid_pass).expect("valid pass must advance");
        assert_eq!(session.generation(), 1);
        let live_hash = hash_state(session.host().state());
        let live_ledger_bytes = session.ledger_bytes();

        let replay = session.apply(stale).expect("rejection must replay exactly");
        assert_eq!(replay.disposition, TransitionDisposition::DuplicateReplay);
        assert_eq!(replay.post_snapshot_generation, 0);
        assert_eq!(replay.rejection_reason, first.rejection_reason);
        assert_eq!(session.generation(), 1);
        assert_eq!(hash_state(session.host().state()), live_hash);
        assert_eq!(session.ledger_bytes(), live_ledger_bytes);
    }

    #[test]
    fn changed_content_under_an_existing_id_is_a_security_rejection() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let original = pass_command(&session, "conflicting-id");
        session
            .apply(original.clone())
            .expect("first receipt must resolve");
        let generation = session.generation();
        let state_hash = hash_state(session.host().state());
        let ledger_bytes = session.ledger_bytes();
        let mut conflict = original;
        conflict.kind = MatchCommandKind::Move { dx: POSITION_SCALE };

        let transition = session
            .apply(conflict)
            .expect("conflict is represented as a transition");

        assert_eq!(transition.disposition, TransitionDisposition::Rejected);
        assert_eq!(
            transition.rejection_reason,
            Some(TransitionRejection::CommandIdConflict),
        );
        assert!(
            transition
                .rejection_reason
                .as_ref()
                .is_some_and(TransitionRejection::is_security_event),
        );
        assert_eq!(transition.pre_snapshot_generation, generation);
        assert_eq!(transition.post_snapshot_generation, generation);
        assert_eq!(session.ledger_len(), 1);
        assert_eq!(session.ledger_bytes(), ledger_bytes);
        assert_eq!(hash_state(session.host().state()), state_hash);
    }

    #[test]
    fn ledger_limit_faults_before_mutation_and_closes_the_session() {
        let host = create_match(&duel()).expect("fixture host must start");
        let mut session = MatchSessionHost::with_ledger_entry_limit(host, 1);
        let first = pass_command(&session, "first-ledger-entry");
        session.apply(first).expect("first entry must fit");
        let generation = session.generation();
        let state_hash = hash_state(session.host().state());
        let ledger_bytes = session.ledger_bytes();
        let second = pass_command(&session, "over-ledger-limit");

        assert_eq!(
            session.apply(second.clone()),
            Err(SessionFault::ResourceLimit)
        );
        assert!(session.is_closed());
        assert_eq!(session.generation(), generation);
        assert_eq!(hash_state(session.host().state()), state_hash);
        assert_eq!(session.ledger_bytes(), ledger_bytes);
        assert_eq!(session.apply(second), Err(SessionFault::Closed));
    }

    #[test]
    fn byte_limit_accepts_an_exact_fit_and_rejects_crossing_atomically() {
        let mut probe = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let probe_command = command(
            &probe,
            "byte-limited-move",
            MatchCommandKind::Move { dx: POSITION_SCALE },
        );
        let probe_transition = probe
            .apply(probe_command.clone())
            .expect("probe command must resolve");
        let entry_bytes = retained_ledger_entry_bytes(&probe_command, &probe_transition)
            .expect("fixture entry must be countable");

        let exact_host = create_match(&duel()).expect("fixture host must start");
        let mut exact = MatchSessionHost::with_ledger_limits(
            exact_host,
            COMMAND_LEDGER_ENTRY_LIMIT,
            entry_bytes,
        );
        let exact_command = command(
            &exact,
            "byte-limited-move",
            MatchCommandKind::Move { dx: POSITION_SCALE },
        );
        exact
            .apply(exact_command)
            .expect("an exact byte-limit fit must be retained");
        assert_eq!(exact.ledger_bytes(), entry_bytes);
        assert!(!exact.is_closed());

        let capped_host = create_match(&duel()).expect("fixture host must start");
        let mut capped = MatchSessionHost::with_ledger_limits(
            capped_host,
            COMMAND_LEDGER_ENTRY_LIMIT,
            entry_bytes - 1,
        );
        let before = capped.host().state().clone();
        let capped_command = command(
            &capped,
            "byte-limited-move",
            MatchCommandKind::Move { dx: POSITION_SCALE },
        );

        assert_eq!(
            capped.apply(capped_command),
            Err(SessionFault::ResourceLimit),
        );
        assert!(capped.is_closed());
        assert_eq!(capped.host().state(), &before);
        assert_eq!(capped.generation(), 0);
        assert_eq!(capped.ledger_len(), 0);
        assert_eq!(capped.ledger_bytes(), 0);
    }

    #[test]
    fn checked_ledger_byte_overflow_faults_without_publication() {
        let host = create_match(&duel()).expect("fixture host must start");
        let mut session =
            MatchSessionHost::with_ledger_limits(host, COMMAND_LEDGER_ENTRY_LIMIT, u64::MAX);
        session.ledger_bytes = u64::MAX;
        let before = session.host().state().clone();
        let move_command = command(
            &session,
            "overflowing-byte-count",
            MatchCommandKind::Move { dx: POSITION_SCALE },
        );

        assert_eq!(
            session.apply(move_command),
            Err(SessionFault::ResourceLimit),
        );
        assert!(session.is_closed());
        assert_eq!(session.host().state(), &before);
        assert_eq!(session.generation(), 0);
        assert_eq!(session.ledger_len(), 0);
        assert_eq!(session.ledger_bytes(), u64::MAX);
    }

    #[test]
    fn projectile_sample_vectors_are_counted_in_full() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let ability = ability_command(&session, "large-trace-accounting");
        let mut transition = session.apply(ability).expect("ability must resolve");
        let before =
            retained_transition_bytes(&transition).expect("fixture transition must be countable");
        let trace = transition
            .events
            .iter_mut()
            .find_map(|event| match &mut event.kind {
                PresentationEventKind::ProjectileTrace(trace) => Some(trace),
                _ => None,
            })
            .expect("ability fixture must retain a projectile trace");
        let sample = *trace.samples.first().expect("trace must have samples");
        const EXTRA_SAMPLES: usize = 4_096;
        trace.samples.extend(vec![sample; EXTRA_SAMPLES]);

        let after = retained_transition_bytes(&transition)
            .expect("expanded fixture transition must be countable");
        let expected_growth =
            u64::try_from(EXTRA_SAMPLES).expect("fixture count must fit") * (4 + 4 + 4);
        assert_eq!(after - before, expected_growth);
    }

    #[test]
    fn generation_exhaustion_faults_without_committing_the_working_host() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        session.generation = u64::MAX;
        let before = session.host().state().clone();
        let move_command = command(
            &session,
            "generation-exhausted",
            MatchCommandKind::Move { dx: POSITION_SCALE },
        );

        assert_eq!(
            session.apply(move_command),
            Err(SessionFault::GenerationExhausted),
        );
        assert!(session.is_closed());
        assert_eq!(session.generation(), u64::MAX);
        assert_eq!(session.host().state(), &before);
        assert_eq!(session.ledger_len(), 0);
    }

    #[test]
    fn malformed_normalized_command_never_consumes_ledger_space() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let mut malformed = pass_command(&session, "valid-before-edit");
        malformed.command_id = "contains space".to_owned();

        assert_eq!(
            session.apply(malformed),
            Err(SessionFault::InvalidCommand {
                field: "command id"
            }),
        );
        assert_eq!(session.ledger_len(), 0);
        assert_eq!(session.generation(), 0);
    }

    #[test]
    fn dirty_rectangles_are_exact_sorted_changed_cell_row_runs() {
        let previous = build_initial_state(&duel()).expect("fixture state must build");
        let mut current = previous.clone();
        for (x, y) in [(1, 1), (2, 1), (4, 1), (4, 2)] {
            let before = terrain::material_at(&current.terrain, x, y);
            let replacement = if before == Material::Empty {
                Material::Soil
            } else {
                Material::Empty
            };
            assert!(terrain::set_material(
                &mut current.terrain,
                x,
                y,
                replacement
            ));
        }

        let rectangles =
            terrain_dirty_rectangles(&previous, &current).expect("valid masks must diff");

        assert_eq!(
            rectangles,
            vec![
                CellRectangle {
                    x: 1,
                    y: 1,
                    width: 2,
                    height: 1,
                },
                CellRectangle {
                    x: 4,
                    y: 1,
                    width: 1,
                    height: 1,
                },
                CellRectangle {
                    x: 4,
                    y: 2,
                    width: 1,
                    height: 1,
                },
            ],
        );
    }
}

/// Per-strike provenance, verified end to end against the authoritative health change.
///
/// These exist because the field they cover is exactly the kind this repository has shipped
/// broken before: produced by a resolver, structurally correct, and reaching no consumer.
/// Karl's Carrion Call is the only multi-strike ability in the roster and is a *projectile*,
/// so the previous `matches!(ability.attack, Attack::Strike(_))` emission gate meant the one
/// ability whose design doc promises three independent crit rolls emitted no strike event at
/// all. A vacuous pass would hide that again, so every assertion below is anchored to a
/// scenario proven to land all three strikes.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod strike_provenance_tests {
    use super::*;
    use crate::match_setup::{MatchMode, MatchPlayerConfig};
    use crate::types::Appearance;

    const TARGET: &str = "b-local-bot";

    fn karl_duel() -> MatchConfig {
        MatchConfig {
            seed: 12_345,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                MatchPlayerConfig {
                    player_id: "a-local-player".to_owned(),
                    team: 0,
                    character_id: "karl".to_owned(),
                    appearance: Appearance::default(),
                },
                MatchPlayerConfig {
                    player_id: TARGET.to_owned(),
                    team: 1,
                    character_id: "huck".to_owned(),
                    appearance: Appearance::default(),
                },
            ],
        }
    }

    /// Flat shot empirically verified to land every one of Karl's three strikes.
    fn landing_volley(session: &MatchSessionHost) -> MatchCommand {
        MatchCommand {
            schema_version: CLIENT_CONTRACT_VERSION,
            command_id: "karl-volley".to_owned(),
            player_id: session.host().active_player().to_owned(),
            expected_turn_number: session.host().state().turn_number,
            expected_snapshot_generation: session.generation(),
            kind: MatchCommandKind::Ability {
                slot: AbilitySlot::Basic,
                angle_millidegrees: 0,
                power_basis_points: 4_600,
                target_player_id: None,
                secondary_target_player_id: None,
            },
        }
    }

    fn health_of(session: &MatchSessionHost, id: &str) -> u16 {
        session
            .host()
            .state()
            .player(id)
            .expect("fixture player must exist")
            .health
    }

    fn strikes(transition: &MatchTransition) -> Vec<StrikeResolution> {
        transition
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                PresentationEventKind::StrikeResolved { strike, .. } => Some(strike.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_multi_strike_projectile_emits_one_record_per_strike_it_actually_landed() {
        let mut session = MatchSessionHost::create(&karl_duel()).expect("fixture session");
        let command = landing_volley(&session);
        let transition = session.apply(command).expect("volley must be accepted");
        let landed = strikes(&transition);

        // Guard against a vacuous pass: if this scenario ever stops landing, every
        // assertion below would hold trivially over an empty vector and prove nothing.
        assert_eq!(
            landed.len(),
            3,
            "Carrion Call must land three strikes in this fixture; a change in ballistics \
             or spawn placement has silently defanged this test",
        );

        // Indices are the resolver's own resolution order: dense and zero-based.
        let indices: Vec<u16> = landed.iter().map(|s| s.strike_index).collect();
        assert_eq!(indices, vec![0, 1, 2]);

        for strike in &landed {
            assert_eq!(strike.target_player_id, TARGET);
            assert!(
                strike.damage_applied > 0,
                "a landed strike that applied no damage is not a landed strike",
            );
        }
    }

    #[test]
    fn every_strike_cites_a_projectile_trace_the_outcome_actually_contains() {
        let mut session = MatchSessionHost::create(&karl_duel()).expect("fixture session");
        let command = landing_volley(&session);
        let transition = session.apply(command).expect("volley must be accepted");
        let landed = strikes(&transition);
        assert_eq!(landed.len(), 3);

        let trace_ids: Vec<u32> = transition
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                PresentationEventKind::ProjectileTrace(trace) => Some(trace.trace_id),
                _ => None,
            })
            .collect();

        let mut cited = Vec::new();
        for strike in &landed {
            match strike.delivery {
                StrikeDelivery::Projectile { trace_sequence } => {
                    assert!(
                        trace_ids.contains(&trace_sequence),
                        "strike cites trace {trace_sequence}, which the transition never emitted",
                    );
                    cited.push(trace_sequence);
                }
                StrikeDelivery::Melee => {
                    panic!("Carrion Call is a projectile ability; a melee delivery is wrong")
                }
            }
        }

        // Each strike is delivered by its own distinct projectile, not three readings of one.
        cited.sort_unstable();
        cited.dedup();
        assert_eq!(
            cited.len(),
            3,
            "the three strikes must cite three distinct traces",
        );
    }

    #[test]
    fn each_strike_records_its_own_independent_crit_draw() {
        let mut session = MatchSessionHost::create(&karl_duel()).expect("fixture session");
        let command = landing_volley(&session);
        let transition = session.apply(command).expect("volley must be accepted");
        let landed = strikes(&transition);
        assert_eq!(landed.len(), 3);

        for strike in &landed {
            // Carrion Call has a non-zero crit chance, so every strike must actually have
            // drawn. `NotEligible` here would mean the record misreports RNG consumption,
            // which desynchronises any consumer tracking the generator.
            assert_ne!(
                strike.crit,
                CritRoll::NotEligible,
                "a crit-capable ability must record a real draw for every strike",
            );
            assert!(strike.crit.consumed_draw());
        }

        // The per-strike flag is only meaningful if it tracks per-strike damage.
        if let (Some(crit), Some(plain)) = (
            landed.iter().find(|s| s.crit.is_critical()),
            landed.iter().find(|s| !s.crit.is_critical()),
        ) {
            assert!(
                crit.damage_applied > plain.damage_applied,
                "a critical strike ({}) must exceed a non-critical one ({})",
                crit.damage_applied,
                plain.damage_applied,
            );
        }
    }

    #[test]
    fn per_strike_damage_reconciles_exactly_with_the_authoritative_health_change() {
        let mut session = MatchSessionHost::create(&karl_duel()).expect("fixture session");
        let before = health_of(&session, TARGET);
        let command = landing_volley(&session);
        let transition = session.apply(command).expect("volley must be accepted");
        let after = health_of(&session, TARGET);
        let landed = strikes(&transition);
        assert_eq!(landed.len(), 3);

        let recorded: u32 = landed.iter().map(|s| u32::from(s.damage_applied)).sum();

        // Carrion Call carries no effects and no self-damage, so the target's entire health
        // loss this command is the sum of these three strikes. This is the assertion that
        // makes the records trustworthy: they are not merely well-formed, they add up to
        // what the authoritative simulation actually did.
        assert_eq!(
            recorded,
            u32::from(before - after),
            "per-strike damage must account for the whole authoritative health change",
        );
        assert!(recorded > 0, "the fixture must actually deal damage");
    }

    #[test]
    fn a_strike_is_presented_at_its_own_projectiles_impact_tick() {
        let mut session = MatchSessionHost::create(&karl_duel()).expect("fixture session");
        let command = landing_volley(&session);
        let transition = session.apply(command).expect("volley must be accepted");

        let mut impact_ticks = BTreeMap::new();
        for event in &transition.events {
            if let PresentationEventKind::ProjectileTrace(trace) = &event.kind {
                impact_ticks.insert(trace.trace_id, trace.terminal_impact.tick);
            }
        }

        let mut checked = 0;
        for event in &transition.events {
            if let PresentationEventKind::StrikeResolved { strike, .. } = &event.kind
                && let StrikeDelivery::Projectile { trace_sequence } = strike.delivery
            {
                let expected = impact_ticks
                    .get(&trace_sequence)
                    .copied()
                    .expect("cited trace must exist");
                assert_eq!(
                    event.presentation_tick, expected,
                    "damage must be presented when its projectile lands, not at turn start",
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 3, "all three strikes must have been checked");
    }
}
