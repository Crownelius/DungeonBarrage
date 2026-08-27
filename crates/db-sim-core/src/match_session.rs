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
use crate::rng::Rng;
use crate::terrain;
use crate::types::{
    AbilityCommand, AbilitySlot, Attack, BallisticImpact, BallisticSample, CommandOutcome,
    CommandRejection, CommandResult, CritRoll, DamageEvent, EffectKind, ImpactCause, MatchPhase,
    PassiveChoiceCommand, PersistentObjectChange, PersistentObjectRemovalCause,
    PersistentObjectTransition, RandomOutcome, SimulationState, StatusChange, StatusTransition,
    StrikeDelivery, StrikeResolution, TurnEndReason,
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

/// A turn ended by the authority because its planning deadline expired.
///
/// **This is deliberately not a [`MatchCommandKind`] variant.** The server owns the clock
/// (`SECURITY_BASELINE.md` §2), and a client must never be able to end a turn — its own or
/// anyone else's — by claiming time ran out. Keeping timeout out of the client command union
/// makes that structural rather than a validation rule: a remote peer sends bytes that are
/// decoded into a `MatchCommand`, and no byte sequence decodes into this type. A validation
/// check could be bypassed by one decoding bug; an absent variant cannot be.
///
/// It still travels through the same session ledger as client commands, sharing one
/// idempotency key space, so a retried timeout replays rather than ending a second turn and a
/// client cannot reuse a timeout's id to smuggle a different action into its slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityTimeout {
    /// Client-contract schema version used to interpret this action.
    pub schema_version: u32,
    /// Deterministic match-unique idempotency key, sharing the client command id space.
    pub action_id: String,
    /// The player whose turn is being ended, validated against the active player.
    ///
    /// Required rather than implied so a timeout raced against a turn handover is refused
    /// instead of silently ending whoever happens to be active by the time it arrives.
    pub player_id: String,
    /// Turn number the authority observed when the deadline expired.
    pub expected_turn_number: u32,
    /// Session snapshot generation the authority observed when the deadline expired.
    pub expected_snapshot_generation: u64,
}

impl AuthorityTimeout {
    /// Returns the deterministic digest used by the session ledger.
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
        if !is_valid_match_local_id(&self.action_id) {
            return Err(SessionFault::InvalidCommand { field: "action_id" });
        }
        if !is_valid_match_local_id(&self.player_id) {
            return Err(SessionFault::InvalidCommand { field: "player_id" });
        }
        Ok(())
    }
}

impl Canonical for AuthorityTimeout {
    fn write_canonical(&self, hasher: &mut CanonicalHasher) {
        // A domain tag of its own, distinct from the client command's `0x20`. Without this a
        // timeout and a command carrying the same identifiers could hash identically, and the
        // ledger's digest comparison would stop being able to tell them apart.
        hasher.write_domain_separator(0x21);
        hasher.write_u32(self.schema_version);
        hasher.write_str(&self.action_id);
        hasher.write_str(&self.player_id);
        hasher.write_u32(self.expected_turn_number);
        hasher.write_u64(self.expected_snapshot_generation);
    }
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

/// One read-only ability-guide request.
///
/// Unlike [`MatchCommand`], a preview has no command ID and therefore can never enter the
/// idempotency ledger. The current authoritative turn number is read under the same session view;
/// snapshot generation is the caller's freshness token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityPreviewRequest {
    /// Client-contract schema version used to interpret this request.
    pub schema_version: u32,
    /// Session generation the guide was constructed against.
    pub expected_snapshot_generation: u64,
    /// Opaque player requesting the guide.
    pub player_id: String,
    /// Ability slot to inspect.
    pub slot: AbilitySlot,
    /// Fixed-point launch angle in millidegrees.
    pub angle_millidegrees: i32,
    /// Launch power in basis points.
    pub power_basis_points: i32,
    /// Optional primary target selection.
    pub target_player_id: Option<String>,
    /// Optional secondary target selection.
    pub secondary_target_player_id: Option<String>,
}

impl AbilityPreviewRequest {
    fn validate_structure(&self) -> Result<(), SessionFault> {
        if self.schema_version != CLIENT_CONTRACT_VERSION {
            return Err(SessionFault::UnsupportedSchema {
                expected: CLIENT_CONTRACT_VERSION,
                actual: self.schema_version,
            });
        }
        if !is_valid_match_local_id(&self.player_id) {
            return Err(SessionFault::InvalidCommand { field: "player id" });
        }
        for target in [
            self.target_player_id.as_deref(),
            self.secondary_target_player_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !is_valid_match_local_id(target) {
                return Err(SessionFault::InvalidCommand {
                    field: "target player id",
                });
            }
        }
        Ok(())
    }
}

/// Why a well-formed preview is not currently legal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewRejection {
    /// The caller previewed an older or future session generation.
    SnapshotGenerationMismatch {
        /// Generation named by the request.
        expected: u64,
        /// Current live generation.
        actual: u64,
    },
    /// The authoritative command rules reject this intent in the current state.
    Core(CommandRejection),
}

/// Read-only response for the initial closed ability-preview contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityPreviewResponse {
    /// Client-contract schema version.
    pub schema_version: u32,
    /// Live generation this response describes.
    pub snapshot_generation: u64,
    /// Whether the exact request could be submitted now.
    pub legal: bool,
    /// Refusal detail when `legal` is false.
    pub rejection_reason: Option<PreviewRejection>,
    /// Exact authoritative special-gauge cost; zero for non-special slots.
    pub gauge_cost: u16,
    /// Sorted primary-target IDs accepted by this ability with the request's other fields.
    pub legal_target_player_ids: Vec<String>,
    /// Static projectile guides against the current snapshot, with no damage or RNG result.
    pub projectile_traces: Vec<ProjectileTraceEvent>,
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

/// Identifiable authoritative provenance for an elimination.
///
/// The exact strike variant is producer-owned.  Itemized damage channels are retained when an
/// effect rather than a strike made the health-zero transition.  `AuthoritativeResolution` is the
/// honest fallback for host-owned settling/fall work whose current outcome DTO has no finer record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeProvenance {
    /// One exact producer-owned strike reduced the player to zero health.
    Strike {
        /// Opaque attacking player ID.
        owner_id: String,
        /// Stable ability definition ID.
        ability_id: String,
        /// Dense strike index from that ability outcome.
        strike_index: u16,
    },
    /// The acting player's own Backlash reduced them to zero health.
    Backlash {
        /// Opaque player who caused the action.
        owner_id: String,
        /// Stable ability definition ID.
        ability_id: String,
    },
    /// An effect's splash damage reduced the player to zero health.
    Splash {
        /// Opaque player who caused the action.
        owner_id: String,
        /// Stable ability definition ID.
        ability_id: String,
    },
    /// Collision damage after authoritative displacement reduced the player to zero health.
    WallImpact {
        /// Opaque player who caused the action.
        owner_id: String,
        /// Stable ability definition ID.
        ability_id: String,
    },
    /// Another itemized ability effect reduced the player to zero health.
    AbilityEffect {
        /// Opaque player who caused the action.
        owner_id: String,
        /// Stable ability definition ID.
        ability_id: String,
    },
    /// World-hazard damage reduced the player to zero health.
    Hazard,
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
    /// One resolver-owned public random result, emitted without exposing RNG state.
    RandomOutcome {
        /// Opaque player whose ability caused the draw.
        owner_id: String,
        /// Stable ability definition ID.
        ability_id: String,
        /// Exact bounded result recorded at the authoritative draw site.
        outcome: RandomOutcome,
    },
    /// One authoritative status transition, in the order the simulation produced it.
    ///
    /// Previously derived by diffing the pre- and post-snapshots, which could not represent
    /// a status applied and expired inside the same turn (it appears in neither snapshot),
    /// nor several charges consumed from one status by a single multi-strike ability (the
    /// diff shows one net change). The transition now comes from the resolver that caused
    /// it; `derive_events` additionally verifies that these records account for every
    /// difference the two snapshots do show.
    StatusChanged {
        /// Opaque player ID.
        player_id: String,
        /// Closed client status kind.
        kind: ClientStatusKind,
        /// What actually happened to the status.
        transition: StatusTransition,
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
    /// A persistent object was removed, for the reason its producer recorded.
    ///
    /// Previously derived by diffing snapshots, which could name no cause at all — every
    /// removal reported the same `AuthoritativeResolution` placeholder — and could not see
    /// an object spawned and removed inside one command, which appears in neither snapshot.
    /// A knife that lands and immediately chain-detonates is exactly that case.
    ObjectRemoved {
        /// Last authoritative projection before removal.
        previous: PersistentObjectSnapshot,
        /// Why the object left authoritative state.
        cause: PersistentObjectRemovalCause,
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

/// One retained first receipt, whoever authored it.
///
/// Client commands and authority actions share a single ledger because they share a single
/// identifier space. Two ledgers would let a client pick an id an authority action already
/// used and receive a different answer than the one the authority recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
enum LedgerRequest {
    /// A normalized command received from a client.
    Client(MatchCommand),
    /// An action authored by the server itself.
    Authority(AuthorityTimeout),
}

#[derive(Debug, Clone)]
struct LedgerEntry {
    request: LedgerRequest,
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
    request: &LedgerRequest,
    transition: &MatchTransition,
) -> Option<u64> {
    let request_bytes = match request {
        LedgerRequest::Client(command) => retained_command_bytes(command)?,
        LedgerRequest::Authority(timeout) => retained_authority_timeout_bytes(timeout)?,
    };
    request_bytes.checked_add(retained_transition_bytes(transition)?)
}

fn retained_authority_timeout_bytes(timeout: &AuthorityTimeout) -> Option<u64> {
    let AuthorityTimeout {
        schema_version,
        action_id,
        player_id,
        expected_turn_number,
        expected_snapshot_generation,
    } = timeout;
    let mut counter = RetainedByteCounter::default();
    counter.add(RETAINED_TOP_LEVEL_HEADER_BYTES)?;
    counter.u32(*schema_version)?;
    counter.string(action_id)?;
    counter.string(player_id)?;
    counter.u32(*expected_turn_number)?;
    counter.u64(*expected_snapshot_generation)?;
    Some(counter.finish())
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
                StrikeDelivery::Effect { kind } => {
                    counter.u8(2)?;
                    // Reuses the closed client vocabulary the snapshots already use, rather
                    // than a second effect-kind mapping that could drift away from it.
                    retained_client_status_kind_bytes(
                        counter,
                        crate::client_contract::snapshot_status_kind(kind),
                    )?;
                }
            }
            counter.u8(match strike.crit {
                CritRoll::NotEligible => 0,
                CritRoll::Missed => 1,
                CritRoll::Landed => 2,
                CritRoll::Forced => 3,
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
        PresentationEventKind::RandomOutcome {
            owner_id,
            ability_id,
            outcome,
        } => {
            counter.string(owner_id)?;
            counter.string(ability_id)?;
            retained_random_outcome_bytes(counter, outcome)?;
        }
        PresentationEventKind::StatusChanged {
            player_id,
            kind,
            transition,
        } => {
            counter.string(player_id)?;
            retained_client_status_kind_bytes(counter, *kind)?;
            retained_status_transition_bytes(counter, transition)?;
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
            counter.u8(match cause {
                PersistentObjectRemovalCause::Replaced => 0,
                PersistentObjectRemovalCause::CapacityEvicted => 1,
                PersistentObjectRemovalCause::Detonated => 2,
                PersistentObjectRemovalCause::Expired => 3,
                PersistentObjectRemovalCause::Destroyed => 4,
                PersistentObjectRemovalCause::OwnerEliminated => 5,
            })?;
            retained_persistent_object_bytes(counter, previous)?;
        }
        PresentationEventKind::PlayerEliminated { player_id, cause } => {
            counter.string(player_id)?;
            retained_change_provenance_bytes(counter, cause)?;
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

fn retained_random_outcome_bytes(
    counter: &mut RetainedByteCounter,
    outcome: &RandomOutcome,
) -> Option<()> {
    counter.tag()?;
    match outcome {
        RandomOutcome::ArzumChainStrikeTeleportTarget {
            candidate_count,
            selected_index,
            target_player_id,
            destination,
        } => {
            counter.u32(*candidate_count)?;
            counter.u32(*selected_index)?;
            counter.string(target_player_id)?;
            counter.i32(destination.x)?;
            counter.i32(destination.y)?;
        }
        RandomOutcome::AlephVeilstepTeleportPoint {
            axis_bound,
            x_result,
            y_result,
            fallback_used,
            drawn_point,
            destination,
        } => {
            counter.u32(*axis_bound)?;
            counter.u32(*x_result)?;
            counter.u32(*y_result)?;
            counter.boolean(*fallback_used)?;
            counter.i32(drawn_point.x)?;
            counter.i32(drawn_point.y)?;
            counter.i32(destination.x)?;
            counter.i32(destination.y)?;
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
    cause: &ChangeProvenance,
) -> Option<()> {
    match cause {
        ChangeProvenance::Strike {
            owner_id,
            ability_id,
            strike_index,
        } => {
            counter.tag()?;
            counter.string(owner_id)?;
            counter.string(ability_id)?;
            counter.u16(*strike_index)
        }
        ChangeProvenance::Backlash {
            owner_id,
            ability_id,
        }
        | ChangeProvenance::Splash {
            owner_id,
            ability_id,
        }
        | ChangeProvenance::WallImpact {
            owner_id,
            ability_id,
        }
        | ChangeProvenance::AbilityEffect {
            owner_id,
            ability_id,
        } => {
            counter.tag()?;
            counter.string(owner_id)?;
            counter.string(ability_id)
        }
        ChangeProvenance::Hazard | ChangeProvenance::AuthoritativeResolution => counter.tag(),
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

fn retained_status_transition_bytes(
    counter: &mut RetainedByteCounter,
    transition: &StatusTransition,
) -> Option<()> {
    // One discriminant byte plus that variant's own payload. Written as an exhaustive match
    // so a new transition variant cannot be added without accounting for its bytes.
    match transition {
        StatusTransition::Applied {
            magnitude,
            turns_remaining,
        } => {
            counter.u8(0)?;
            counter.i32(*magnitude)?;
            counter.u8(*turns_remaining)
        }
        StatusTransition::Refreshed {
            magnitude,
            turns_remaining,
            replaced_magnitude,
            replaced_turns_remaining,
        } => {
            counter.u8(1)?;
            counter.i32(*magnitude)?;
            counter.u8(*turns_remaining)?;
            counter.i32(*replaced_magnitude)?;
            counter.u8(*replaced_turns_remaining)
        }
        StatusTransition::ChargeConsumed { remaining } => {
            counter.u8(2)?;
            counter.i32(*remaining)
        }
        StatusTransition::Ticked { turns_remaining } => {
            counter.u8(3)?;
            counter.u8(*turns_remaining)
        }
        StatusTransition::Exhausted => counter.u8(4),
        StatusTransition::Expired => counter.u8(5),
    }
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

/// Opaque, complete restore input produced by [`MatchSessionHost::checkpoint`].
///
/// Fields are intentionally private: a caller can persist or transfer the whole value, but cannot
/// manufacture a host-only restore that silently discards first-receipt results. Restoration still
/// revalidates every entry and recomputes the exact retained-byte total before accepting it.
#[derive(Debug, Clone)]
pub struct MatchSessionCheckpoint {
    host: MatchHost,
    generation: u64,
    ledger: BTreeMap<String, LedgerEntry>,
    declared_ledger_len: usize,
    declared_ledger_bytes: u64,
    ledger_entry_limit: usize,
    ledger_byte_limit: u64,
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

    /// Captures the complete live session restore unit.
    ///
    /// The host and ledger cannot be requested separately. This is deliberate: restoring only the
    /// host would forget idempotency receipts and allow an already-applied command to execute again.
    ///
    /// # Errors
    ///
    /// Returns [`SessionFault::Closed`] after a terminal fault. A closed session is disposal-only
    /// and must not be revived by checkpointing it.
    pub fn checkpoint(&self) -> Result<MatchSessionCheckpoint, SessionFault> {
        if self.closed {
            return Err(SessionFault::Closed);
        }
        Ok(MatchSessionCheckpoint {
            host: self.host.clone(),
            generation: self.generation,
            ledger: self.ledger.clone(),
            declared_ledger_len: self.ledger.len(),
            declared_ledger_bytes: self.ledger_bytes,
            ledger_entry_limit: self.ledger_entry_limit,
            ledger_byte_limit: self.ledger_byte_limit,
        })
    }

    /// Restores a complete checkpoint after verifying its host/ledger relationship and exact
    /// retained-byte accounting.
    ///
    /// # Errors
    ///
    /// Returns [`SessionFault::ContractInvariant`] for a missing, duplicated, contradictory, or
    /// host-incoherent entry and [`SessionFault::ResourceLimit`] when the checkpoint crosses its
    /// retained-entry/byte bounds. No partially restored session is returned.
    pub fn restore(checkpoint: MatchSessionCheckpoint) -> Result<Self, SessionFault> {
        validate_checkpoint(&checkpoint)?;
        Ok(Self {
            host: checkpoint.host,
            generation: checkpoint.generation,
            ledger: checkpoint.ledger,
            ledger_entry_limit: checkpoint.ledger_entry_limit,
            ledger_bytes: checkpoint.declared_ledger_bytes,
            ledger_byte_limit: checkpoint.ledger_byte_limit,
            closed: false,
        })
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

    /// Computes one ability guide without mutating authoritative or session state.
    ///
    /// Legality is resolved on a disposable [`MatchHost`] clone so the answer follows the exact
    /// same validation/effect path as submission. Projectile guides are integrated separately
    /// against the immutable live snapshot: no damage, crit roll, relocation draw, terrain
    /// mutation, processed-command ID, generation, or ledger entry can escape the clone.
    ///
    /// # Errors
    ///
    /// Returns [`SessionFault::Closed`] after a terminal session fault, structure/schema faults
    /// for malformed typed input, or a simulation fault if a supposedly valid host clone cannot
    /// resolve its own rules. Ordinary gameplay and stale-generation refusals are successful
    /// responses with `legal == false`.
    pub fn preview(
        &self,
        request: &AbilityPreviewRequest,
    ) -> Result<AbilityPreviewResponse, SessionFault> {
        if self.closed {
            return Err(SessionFault::Closed);
        }
        request.validate_structure()?;
        if request.expected_snapshot_generation != self.generation {
            return Ok(AbilityPreviewResponse {
                schema_version: CLIENT_CONTRACT_VERSION,
                snapshot_generation: self.generation,
                legal: false,
                rejection_reason: Some(PreviewRejection::SnapshotGenerationMismatch {
                    expected: request.expected_snapshot_generation,
                    actual: self.generation,
                }),
                gauge_cost: 0,
                legal_target_player_ids: Vec::new(),
                projectile_traces: Vec::new(),
            });
        }

        let ability = self
            .host
            .state()
            .player(&request.player_id)
            .and_then(|player| character::find(&player.character_id))
            .and_then(|definition| definition.ability(request.slot));
        let gauge_cost = if request.slot.consumes_gauge() {
            crate::types::GAUGE_FULL
        } else {
            0
        };
        let exact_command = preview_ability_command(self.host.state(), request)?;
        let exact_rejection = preview_command_rejection(&self.host, &exact_command)?;
        let legal = exact_rejection.is_none();

        let mut legal_target_player_ids = Vec::new();
        for candidate in self
            .host
            .state()
            .players
            .iter()
            .filter(|player| !player.is_eliminated())
        {
            let mut candidate_request = request.clone();
            candidate_request.target_player_id = Some(candidate.id.clone());
            let candidate_command = preview_ability_command(self.host.state(), &candidate_request)?;
            if preview_command_rejection(&self.host, &candidate_command)?.is_none() {
                legal_target_player_ids.push(candidate.id.clone());
            }
        }
        legal_target_player_ids.sort();

        let projectile_traces = if legal {
            match ability {
                Some(ability) => preview_projectile_traces(self.host.state(), request, ability)?,
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };
        Ok(AbilityPreviewResponse {
            schema_version: CLIENT_CONTRACT_VERSION,
            snapshot_generation: self.generation,
            legal,
            rejection_reason: exact_rejection.map(PreviewRejection::Core),
            gauge_cost,
            legal_target_player_ids,
            projectile_traces,
        })
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
            // A client id colliding with an authority action's id is a conflict, never a
            // replay: the two are different requests that happen to share a key.
            if matches!(&entry.request, LedgerRequest::Client(existing) if *existing == command) {
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
            AppliedRecords {
                outcome: command_outcome.as_deref(),
                // Sourced from the host rather than the outcome so that commands producing
                // no outcome at all -- Move, Pass, and the authority timeout -- still
                // surface the end-of-turn transitions they caused.
                status_changes: working_host.status_changes(),
                object_changes: working_host.object_changes(),
            },
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
            LedgerRequest::Client(command),
            digest,
            transition,
            Some(working_host),
            post_generation,
        )
    }

    /// Ends the active player's turn because the authority's planning deadline expired.
    ///
    /// The server owns the clock (`SECURITY_BASELINE.md` §2). This is the only entry point
    /// that can end a turn on time, it takes an [`AuthorityTimeout`] rather than a
    /// [`MatchCommand`], and no client-decodable byte sequence produces that type — so a
    /// remote peer cannot reach this path at all, however malformed its input.
    ///
    /// Otherwise it behaves exactly like [`Self::apply`]: same generation checks, same
    /// idempotency ledger and identifier space, same ordered transition. A retried timeout
    /// replays its original result instead of ending a second turn.
    ///
    /// # Errors
    ///
    /// [`SessionFault::Closed`] once the session has closed, [`SessionFault::UnsupportedSchema`]
    /// or [`SessionFault::InvalidCommand`] for a malformed action, [`SessionFault::ResourceLimit`]
    /// at the ledger bounds, and [`SessionFault::ContractInvariant`] if the authoritative host
    /// contradicts itself. A refusal the authority could legitimately race into — a stale
    /// generation, the wrong player, the wrong phase — is a rejected transition, not an error.
    pub fn apply_authority_timeout(
        &mut self,
        timeout: AuthorityTimeout,
    ) -> Result<MatchTransition, SessionFault> {
        if self.closed {
            return Err(SessionFault::Closed);
        }
        timeout.validate_structure()?;
        let digest = timeout.canonical_digest();

        if let Some(entry) = self.ledger.get(&timeout.action_id) {
            // Only an identical authority action replays. A client command that reused this
            // id is a conflict, and so is a different timeout wearing the same id.
            if matches!(&entry.request, LedgerRequest::Authority(existing) if *existing == timeout)
            {
                if entry.canonical_digest != digest {
                    self.closed = true;
                    return Err(SessionFault::ContractInvariant);
                }
                let mut replay = entry.transition.clone();
                replay.disposition = TransitionDisposition::DuplicateReplay;
                return Ok(replay);
            }
            return Ok(
                self.current_rejection(&timeout.action_id, TransitionRejection::CommandIdConflict)
            );
        }

        if self.ledger.len() >= self.ledger_entry_limit {
            self.closed = true;
            return Err(SessionFault::ResourceLimit);
        }

        if timeout.expected_snapshot_generation != self.generation {
            let rejection = TransitionRejection::SnapshotGenerationMismatch {
                expected: timeout.expected_snapshot_generation,
                actual: self.generation,
            };
            return self.record_authority_rejection(timeout, digest, rejection);
        }

        if let Some(reason) = authority_timeout_rejection(self.host.state(), &timeout) {
            return self.record_authority_rejection(
                timeout,
                digest,
                TransitionRejection::Core(reason),
            );
        }

        let pre_state = self.host.state().clone();
        let pre_snapshot = self.snapshot();
        let mut working_host = self.host.clone();

        // `time_out_turn` refuses while a passive choice is owed. That is a legitimate race
        // rather than a contract breach — the interrupt may have been raised by the very
        // action that preceded this deadline — so it is reported as a refusal and the working
        // host is discarded unexamined.
        if working_host.time_out_turn().is_err() {
            return self.record_authority_rejection(
                timeout,
                digest,
                TransitionRejection::Core(CommandRejection::WrongPhase),
            );
        }

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

        let events = match derive_events(
            &timeout_as_pass(&timeout),
            &pre_state,
            working_host.state(),
            &pre_snapshot,
            &post_snapshot,
            AppliedRecords {
                // A timeout resolves no ability, so there is no outcome to carry. The
                // end-of-turn transitions it caused still come from the host.
                outcome: None,
                status_changes: working_host.status_changes(),
                object_changes: working_host.object_changes(),
            },
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
            command_id: timeout.action_id.clone(),
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
            LedgerRequest::Authority(timeout),
            digest,
            transition,
            Some(working_host),
            post_generation,
        )
    }

    fn record_authority_rejection(
        &mut self,
        timeout: AuthorityTimeout,
        digest: String,
        rejection: TransitionRejection,
    ) -> Result<MatchTransition, SessionFault> {
        let transition = self.current_rejection(&timeout.action_id, rejection);
        self.record_and_commit(
            LedgerRequest::Authority(timeout),
            digest,
            transition,
            None,
            self.generation,
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
        self.record_and_commit(
            LedgerRequest::Client(command),
            digest,
            transition,
            None,
            self.generation,
        )
    }

    fn record_and_commit(
        &mut self,
        request: LedgerRequest,
        canonical_digest: String,
        transition: MatchTransition,
        working_host: Option<MatchHost>,
        post_generation: u64,
    ) -> Result<MatchTransition, SessionFault> {
        use std::collections::btree_map::Entry;

        let Some(entry_bytes) = retained_ledger_entry_bytes(&request, &transition) else {
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

        let key = match &request {
            LedgerRequest::Client(command) => command.command_id.clone(),
            LedgerRequest::Authority(timeout) => timeout.action_id.clone(),
        };
        let retained_transition = transition.clone();
        match self.ledger.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(LedgerEntry {
                    request,
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

fn validate_checkpoint(checkpoint: &MatchSessionCheckpoint) -> Result<(), SessionFault> {
    if checkpoint.declared_ledger_len != checkpoint.ledger.len() {
        return Err(SessionFault::ContractInvariant);
    }
    if checkpoint.ledger_entry_limit > COMMAND_LEDGER_ENTRY_LIMIT
        || checkpoint.ledger_byte_limit > COMMAND_LEDGER_BYTE_LIMIT
        || checkpoint.ledger.len() > checkpoint.ledger_entry_limit
    {
        return Err(SessionFault::ResourceLimit);
    }

    let current_snapshot = MatchSnapshot::from_host(&checkpoint.host, checkpoint.generation);
    let mut recomputed_bytes = 0u64;
    let mut mutation_generations = BTreeSet::new();
    let mut accepted_processed_ids = BTreeSet::new();
    let mut saw_current_snapshot = checkpoint.ledger.is_empty() && checkpoint.generation == 0;

    for (key, entry) in &checkpoint.ledger {
        let (request_id, request_digest, is_processed_kind) = match &entry.request {
            LedgerRequest::Client(command) => {
                command.validate_structure()?;
                (
                    command.command_id.as_str(),
                    command.canonical_digest(),
                    matches!(
                        command.kind,
                        MatchCommandKind::Ability { .. } | MatchCommandKind::PassiveChoice { .. }
                    ),
                )
            }
            LedgerRequest::Authority(timeout) => {
                timeout.validate_structure()?;
                (
                    timeout.action_id.as_str(),
                    timeout.canonical_digest(),
                    false,
                )
            }
        };
        if key != request_id
            || entry.canonical_digest != request_digest
            || entry.transition.command_id != request_id
        {
            return Err(SessionFault::ContractInvariant);
        }
        validate_restored_transition(&entry.transition, checkpoint.generation)?;

        let entry_bytes = retained_ledger_entry_bytes(&entry.request, &entry.transition)
            .ok_or(SessionFault::ResourceLimit)?;
        recomputed_bytes = recomputed_bytes
            .checked_add(entry_bytes)
            .ok_or(SessionFault::ResourceLimit)?;
        if recomputed_bytes > checkpoint.ledger_byte_limit {
            return Err(SessionFault::ResourceLimit);
        }

        if entry.transition.disposition == TransitionDisposition::Accepted {
            if entry.transition.post_snapshot_generation > entry.transition.pre_snapshot_generation
                && !mutation_generations.insert(entry.transition.post_snapshot_generation)
            {
                return Err(SessionFault::ContractInvariant);
            }
            if is_processed_kind {
                accepted_processed_ids.insert(request_id);
            }
        } else if is_processed_kind && checkpoint.host.state().has_processed(request_id) {
            return Err(SessionFault::ContractInvariant);
        }

        if entry.transition.post_snapshot_generation == checkpoint.generation {
            if entry.transition.post_snapshot != current_snapshot {
                return Err(SessionFault::ContractInvariant);
            }
            saw_current_snapshot = true;
        }
    }

    if recomputed_bytes != checkpoint.declared_ledger_bytes
        || recomputed_bytes > checkpoint.ledger_byte_limit
        || !saw_current_snapshot
    {
        return Err(SessionFault::ContractInvariant);
    }

    let processed_ids: BTreeSet<&str> = checkpoint
        .host
        .state()
        .processed_command_ids
        .iter()
        .map(String::as_str)
        .collect();
    if processed_ids.len() != checkpoint.host.state().processed_command_ids.len()
        || processed_ids != accepted_processed_ids
    {
        return Err(SessionFault::ContractInvariant);
    }

    let mut expected_generation = 1u64;
    for generation in mutation_generations {
        if generation != expected_generation {
            return Err(SessionFault::ContractInvariant);
        }
        expected_generation = expected_generation
            .checked_add(1)
            .ok_or(SessionFault::ContractInvariant)?;
    }
    let completed_generation = expected_generation.saturating_sub(1);
    if completed_generation != checkpoint.generation {
        return Err(SessionFault::ContractInvariant);
    }
    Ok(())
}

fn validate_restored_transition(
    transition: &MatchTransition,
    live_generation: u64,
) -> Result<(), SessionFault> {
    if transition.schema_version != CLIENT_CONTRACT_VERSION
        || transition.post_snapshot.client_contract_version != CLIENT_CONTRACT_VERSION
        || transition.post_snapshot_generation != transition.post_snapshot.generation
        || transition.post_state_hash != transition.post_snapshot.authoritative_state_hash
        || transition.pre_snapshot_generation > transition.post_snapshot_generation
        || transition.post_snapshot_generation > live_generation
        || transition
            .post_snapshot_generation
            .saturating_sub(transition.pre_snapshot_generation)
            > 1
        || !transition.events.iter().enumerate().all(|(index, event)| {
            u32::try_from(index) == Ok(event.sequence)
                && transition
                    .events
                    .get(index.wrapping_sub(1))
                    .is_none_or(|previous| {
                        (previous.presentation_tick, previous.sequence)
                            < (event.presentation_tick, event.sequence)
                    })
        })
    {
        return Err(SessionFault::ContractInvariant);
    }

    match transition.disposition {
        TransitionDisposition::Accepted => {
            if transition.rejection_reason.is_some()
                || (transition.post_snapshot_generation == transition.pre_snapshot_generation
                    && (!transition.events.is_empty() || transition.input_lock_ticks != 0))
            {
                return Err(SessionFault::ContractInvariant);
            }
        }
        TransitionDisposition::Rejected => {
            if transition.rejection_reason.is_none()
                || transition.pre_snapshot_generation != transition.post_snapshot_generation
                || !transition.events.is_empty()
                || transition.input_lock_ticks != 0
            {
                return Err(SessionFault::ContractInvariant);
            }
        }
        TransitionDisposition::DuplicateReplay => {
            // The ledger stores the original first result. Replay disposition is applied only
            // to a detached response and must never overwrite that retained original.
            return Err(SessionFault::ContractInvariant);
        }
    }
    Ok(())
}

enum AppliedCommand {
    /// Boxed because `CommandOutcome` carries several provenance vectors and dwarfs the
    /// rejection variant; an unboxed union would pay that size on every rejection too.
    Accepted(Option<Box<CommandOutcome>>),
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
                CommandResult::Accepted(outcome) => Ok(AppliedCommand::Accepted(Some(outcome))),
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
                CommandResult::Accepted(outcome) => Ok(AppliedCommand::Accepted(Some(outcome))),
                CommandResult::Rejected(reason) => Ok(AppliedCommand::Rejected(reason)),
            }
        }
        MatchCommandKind::Pass => {
            host.pass_turn()?;
            Ok(AppliedCommand::Accepted(None))
        }
    }
}

fn preview_ability_command(
    state: &SimulationState,
    request: &AbilityPreviewRequest,
) -> Result<AbilityCommand, SessionFault> {
    let mut command_id = None;
    // There are `len + 1` candidate IDs and at most `len` distinct retained IDs, so one must
    // be absent. The bound avoids an unbounded search over authority-owned state.
    for index in 0..=state.processed_command_ids.len() {
        let candidate = format!("preview:{index}");
        if !state.has_processed(&candidate) {
            command_id = Some(candidate);
            break;
        }
    }
    let command_id = command_id.ok_or(SessionFault::ContractInvariant)?;
    Ok(AbilityCommand {
        command_id,
        player_id: request.player_id.clone(),
        expected_turn_number: state.turn_number,
        slot: request.slot,
        angle_millidegrees: request.angle_millidegrees,
        power_basis_points: request.power_basis_points,
        target_player_id: request.target_player_id.clone(),
        secondary_target_player_id: request.secondary_target_player_id.clone(),
    })
}

fn preview_command_rejection(
    host: &MatchHost,
    command: &AbilityCommand,
) -> Result<Option<CommandRejection>, SessionFault> {
    let mut working = host.clone();
    match working.submit_ability(command)? {
        CommandResult::Accepted(_) => Ok(None),
        CommandResult::Rejected(reason) => Ok(Some(reason)),
    }
}

fn preview_projectile_traces(
    state: &SimulationState,
    request: &AbilityPreviewRequest,
    ability: &crate::types::AbilityDefinition,
) -> Result<Vec<ProjectileTraceEvent>, SessionFault> {
    let Attack::Projectile(projectile) = ability.attack else {
        return Ok(Vec::new());
    };
    let actor_position = state
        .player(&request.player_id)
        .ok_or(SessionFault::ContractInvariant)?
        .position;
    let hitboxes: Vec<(String, crate::fixed::FixedPoint, i32)> = state
        .players
        .iter()
        .filter(|player| player.id != request.player_id && !player.is_eliminated())
        .map(|player| (player.id.clone(), player.position, crate::fixed::BODY_WIDTH))
        .collect();
    let input = crate::types::BallisticInput {
        origin: actor_position,
        angle_millidegrees: request.angle_millidegrees,
        power_basis_points: request.power_basis_points,
        wind_per_tick: state.wind_per_tick,
    };
    let mut traces = Vec::new();
    for sequence in 0..u32::from(ability.strikes_per_turn.max(1)) {
        let result = crate::ballistics::integrate(&input, &projectile, &state.terrain, &hitboxes)?;
        traces.push(ProjectileTraceEvent {
            trace_id: sequence,
            owner_id: request.player_id.clone(),
            ability_id: ability.id.to_owned(),
            samples: result
                .samples
                .into_iter()
                .map(snapshot_projectile_sample)
                .collect(),
            terminal_impact: snapshot_impact(result.impact),
        });
    }
    Ok(traces)
}

/// Whether the authoritative state refuses this timeout outright.
///
/// Mirrors [`preflight_rejection`] rather than reusing it, because a timeout is not a
/// `MatchCommand` and must not be made into one just to share a check. The rules are the same
/// in substance: the named player must still be the one on the clock.
fn authority_timeout_rejection(
    state: &SimulationState,
    timeout: &AuthorityTimeout,
) -> Option<CommandRejection> {
    if timeout.expected_turn_number != state.turn_number {
        return Some(CommandRejection::TurnVersionMismatch);
    }
    // A deadline that expired for one player must never end a different player's turn, which
    // is exactly what a timeout raced against a handover would otherwise do.
    if timeout.player_id != state.active_player_id {
        return Some(CommandRejection::NotActivePlayer);
    }
    if state
        .player(&timeout.player_id)
        .is_none_or(crate::types::PlayerState::is_eliminated)
    {
        return Some(CommandRejection::PlayerEliminated);
    }
    (!state.phase.accepts_ability_command()).then_some(CommandRejection::WrongPhase)
}

/// Projects a timeout onto the shape `derive_events` reads for non-ability commands.
///
/// `derive_events` only inspects `kind` to decide whether to walk an ability's traces, and a
/// timeout has none. Building this locally keeps `MatchCommandKind` free of a timeout variant,
/// which is the property that stops a client from ever selecting one.
fn timeout_as_pass(timeout: &AuthorityTimeout) -> MatchCommand {
    MatchCommand {
        schema_version: timeout.schema_version,
        command_id: timeout.action_id.clone(),
        player_id: timeout.player_id.clone(),
        expected_turn_number: timeout.expected_turn_number,
        expected_snapshot_generation: timeout.expected_snapshot_generation,
        kind: MatchCommandKind::Pass,
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

type SnapshotStatusMap = BTreeMap<String, BTreeMap<ClientStatusKind, StatusSnapshot>>;

/// Replays the producer-owned status record from the pre-snapshot and requires it to land
/// exactly on the post-snapshot.
///
/// Merely observing that a record names the same status kind as a snapshot difference is not
/// enough: a stale `Ticked` value, a refresh that lies about what it replaced, or one missing
/// charge consumption would all satisfy that weaker check. Replaying the transitions also keeps
/// invisible but legitimate lifecycles representable -- `Applied` followed by `Expired` can
/// return to the same empty post-state while still being validated step by step.
fn reconcile_status_changes(
    pre_snapshot: &MatchSnapshot,
    post_snapshot: &MatchSnapshot,
    status_changes: &[StatusChange],
) -> Result<(), SessionFault> {
    let mut shadow = snapshot_status_map(pre_snapshot)?;
    let expected = snapshot_status_map(post_snapshot)?;

    if shadow.keys().ne(expected.keys()) {
        return Err(SessionFault::ContractInvariant);
    }

    for change in status_changes {
        apply_status_change(&mut shadow, change)?;
    }

    if shadow != expected {
        return Err(SessionFault::ContractInvariant);
    }
    Ok(())
}

fn snapshot_status_map(snapshot: &MatchSnapshot) -> Result<SnapshotStatusMap, SessionFault> {
    let mut players = BTreeMap::new();
    for player in &snapshot.players {
        let mut statuses = BTreeMap::new();
        for status in &player.statuses {
            if statuses.insert(status.kind, status.clone()).is_some() {
                return Err(SessionFault::ContractInvariant);
            }
        }
        if players.insert(player.id.clone(), statuses).is_some() {
            return Err(SessionFault::ContractInvariant);
        }
    }
    Ok(players)
}

fn apply_status_change(
    shadow: &mut SnapshotStatusMap,
    change: &StatusChange,
) -> Result<(), SessionFault> {
    let Some(statuses) = shadow.get_mut(&change.player_id) else {
        return Err(SessionFault::ContractInvariant);
    };
    let kind = crate::client_contract::snapshot_status_kind(change.kind);

    match &change.transition {
        StatusTransition::Applied {
            magnitude,
            turns_remaining,
        } => {
            let status = StatusSnapshot {
                kind,
                magnitude: *magnitude,
                turns_remaining: *turns_remaining,
            };
            if statuses.insert(kind, status).is_some() {
                return Err(SessionFault::ContractInvariant);
            }
        }
        StatusTransition::Refreshed {
            magnitude,
            turns_remaining,
            replaced_magnitude,
            replaced_turns_remaining,
        } => {
            let Some(status) = statuses.get_mut(&kind) else {
                return Err(SessionFault::ContractInvariant);
            };
            if status.magnitude != *replaced_magnitude
                || status.turns_remaining != *replaced_turns_remaining
            {
                return Err(SessionFault::ContractInvariant);
            }
            status.magnitude = *magnitude;
            status.turns_remaining = *turns_remaining;
        }
        StatusTransition::ChargeConsumed { remaining } => {
            if *remaining <= 0 {
                return Err(SessionFault::ContractInvariant);
            }
            let Some(status) = statuses.get_mut(&kind) else {
                return Err(SessionFault::ContractInvariant);
            };
            if status.magnitude.checked_sub(1) != Some(*remaining) {
                return Err(SessionFault::ContractInvariant);
            }
            status.magnitude = *remaining;
        }
        StatusTransition::Ticked { turns_remaining } => {
            if *turns_remaining == 0 {
                return Err(SessionFault::ContractInvariant);
            }
            let Some(status) = statuses.get_mut(&kind) else {
                return Err(SessionFault::ContractInvariant);
            };
            if status.turns_remaining.checked_sub(1) != Some(*turns_remaining) {
                return Err(SessionFault::ContractInvariant);
            }
            status.turns_remaining = *turns_remaining;
        }
        StatusTransition::Exhausted => {
            if statuses.get(&kind).map(|status| status.magnitude) != Some(1) {
                return Err(SessionFault::ContractInvariant);
            }
            statuses.remove(&kind);
        }
        StatusTransition::Expired => {
            if statuses
                .get(&kind)
                .is_none_or(|status| status.turns_remaining > 1)
            {
                return Err(SessionFault::ContractInvariant);
            }
            statuses.remove(&kind);
        }
    }
    Ok(())
}

type SnapshotObjectMap = BTreeMap<u32, PersistentObjectSnapshot>;

/// Replays the ordered producer-owned object lifecycle against the pre-snapshot.
///
/// Snapshot diffs cannot see an object that was spawned and removed in one action, and a
/// sequence-only presence check accepts duplicate, unknown, or stale records. This replay
/// validates the complete object at each lifecycle boundary while still allowing a surviving
/// pre-existing object to change in place (reported separately as `ObjectChanged`).
fn reconcile_object_changes(
    pre_objects: &[PersistentObjectSnapshot],
    post_objects: &[PersistentObjectSnapshot],
    object_changes: &[PersistentObjectChange],
) -> Result<(), SessionFault> {
    let mut shadow = snapshot_object_map(pre_objects)?;
    let expected = snapshot_object_map(post_objects)?;
    let pre_sequences: BTreeSet<u32> = shadow.keys().copied().collect();
    let mut allocated_sequences = pre_sequences.clone();

    for change in object_changes {
        let projected = crate::client_contract::snapshot_object(&change.object);
        match change.transition {
            PersistentObjectTransition::Spawned => {
                // Object sequences are monotonic match identities and may never be reused,
                // even if an earlier object with the same sequence was removed in this call.
                if !allocated_sequences.insert(projected.sequence)
                    || shadow.insert(projected.sequence, projected).is_some()
                {
                    return Err(SessionFault::ContractInvariant);
                }
            }
            PersistentObjectTransition::Removed { .. } => {
                let Some(previous) = shadow.remove(&projected.sequence) else {
                    return Err(SessionFault::ContractInvariant);
                };
                if previous != projected {
                    return Err(SessionFault::ContractInvariant);
                }
            }
        }
    }

    if shadow.keys().ne(expected.keys()) {
        return Err(SessionFault::ContractInvariant);
    }

    // A newly spawned survivor must match its producer record byte for byte. Existing
    // survivors may legitimately mutate in place; their exact pre/post values are retained
    // by `ObjectChanged` until such mutation gains its own producer-owned transition.
    for (sequence, current) in &shadow {
        if !pre_sequences.contains(sequence) && expected.get(sequence) != Some(current) {
            return Err(SessionFault::ContractInvariant);
        }
    }

    Ok(())
}

fn snapshot_object_map(
    objects: &[PersistentObjectSnapshot],
) -> Result<SnapshotObjectMap, SessionFault> {
    let mut by_sequence = BTreeMap::new();
    for object in objects {
        if by_sequence
            .insert(object.sequence, object.clone())
            .is_some()
        {
            return Err(SessionFault::ContractInvariant);
        }
    }
    Ok(by_sequence)
}

/// Verifies that every public non-strike draw was recorded at its producer and that the
/// record agrees with the bounded generator result actually reachable from the pre-state.
///
/// This replay is validation only: presentation events are emitted from `outcome` below,
/// never synthesized from snapshots. The launch contract has two closed random effects, both
/// with non-critical primary attacks, so their draw order is exact and no private RNG state is
/// exposed outside this check.
fn reconcile_random_outcomes(
    command: &MatchCommand,
    ability: &crate::types::AbilityDefinition,
    pre_state: &SimulationState,
    post_state: &SimulationState,
    outcome: &CommandOutcome,
) -> Result<(), SessionFault> {
    match ability.id {
        "arzum-chain-strike" => {
            if outcome
                .strikes
                .iter()
                .any(|strike| strike.crit != CritRoll::NotEligible)
            {
                return Err(SessionFault::ContractInvariant);
            }
            let MatchCommandKind::Ability {
                target_player_id: Some(first_target_id),
                ..
            } = &command.kind
            else {
                return Err(SessionFault::ContractInvariant);
            };
            // Chain Strike chooses after its ordinary melee hit but before host-owned settling and
            // turn completion. Reconstruct only that draw-time state from the immutable pre-state
            // plus already-reconciled primary strike eliminations. Reading positions or liveness
            // from the final snapshot would infer a random choice from state that later mechanics
            // were allowed to change.
            let mut draw_state = pre_state.clone();
            for strike in &outcome.strikes {
                if matches!(strike.delivery, StrikeDelivery::Melee) && strike.eliminated_target {
                    let Some(player) = draw_state.player_mut(&strike.target_player_id) else {
                        return Err(SessionFault::ContractInvariant);
                    };
                    player.health = 0;
                }
            }
            let mut rng = Rng::from_state(pre_state.rng_state);
            let expected = crate::resolve::relocation::draw_arzum_chain_strike_target(
                &mut rng,
                &draw_state,
                &command.player_id,
                first_target_id,
            )?;
            match expected {
                Some(expected)
                    if outcome.random_outcomes.len() == 1
                        && outcome.random_outcomes.first() == Some(&expected) => {}
                None if outcome.random_outcomes.is_empty() => {}
                Some(_) | None => return Err(SessionFault::ContractInvariant),
            }
            if post_state.rng_state != rng.state() {
                return Err(SessionFault::ContractInvariant);
            }
        }
        "aleph-veilstep" => {
            if outcome
                .strikes
                .iter()
                .any(|strike| strike.crit != CritRoll::NotEligible)
            {
                return Err(SessionFault::ContractInvariant);
            }
            let Some(center) = pre_state
                .player(&command.player_id)
                .map(|player| player.position)
            else {
                return Err(SessionFault::ContractInvariant);
            };
            let Some(radius) = ability
                .effects
                .iter()
                .find(|effect| effect.kind == EffectKind::Teleport)
                .map(|effect| effect.magnitude)
            else {
                return Err(SessionFault::ContractInvariant);
            };
            let mut rng = Rng::from_state(pre_state.rng_state);
            let expected = crate::resolve::relocation::draw_aleph_veilstep_point(
                &mut rng,
                &pre_state.terrain,
                center,
                radius,
            )?;
            if outcome.random_outcomes.as_slice() != [expected]
                || post_state.rng_state != rng.state()
            {
                return Err(SessionFault::ContractInvariant);
            }
        }
        _ if outcome.random_outcomes.is_empty() => {}
        _ => return Err(SessionFault::ContractInvariant),
    }
    Ok(())
}

/// Reconciles every producer-owned strike against the attack definition, traces, aggregate
/// damage, and pre/post authoritative states.
///
/// A dense vector alone is not sufficient: deleting a record and renumbering the rest would still
/// look dense.  Projectile character impacts and melee target enumeration provide the independent
/// expected cardinality, while aggregate direct damage proves that a surviving record was not
/// silently altered.  Effects that intentionally aggregate direct damage without per-strike
/// records are excluded from the final sum check until their contract gains an equivalent record.
fn reconcile_strikes(
    command: &MatchCommand,
    ability: &crate::types::AbilityDefinition,
    pre_state: &SimulationState,
    post_state: &SimulationState,
    outcome: &CommandOutcome,
) -> Result<(), SessionFault> {
    // Re-run the authoritative producer from the immutable pre-state before applying any
    // aggregate or shape checks below. Those checks prove that records account for the final
    // state, but they cannot prove ordered per-strike facts on their own: two crit/damage pairs
    // can be exchanged while preserving every aggregate. The detached replay is the independent
    // source for exact draw order, damage clamping, delivery, and the one strike that actually
    // crossed a target from alive to eliminated. It cannot mutate the live/working host.
    let replay_command = match &command.kind {
        MatchCommandKind::Ability {
            slot,
            angle_millidegrees,
            power_basis_points,
            target_player_id,
            secondary_target_player_id,
        } => AbilityCommand {
            command_id: command.command_id.clone(),
            player_id: command.player_id.clone(),
            expected_turn_number: command.expected_turn_number,
            slot: *slot,
            angle_millidegrees: *angle_millidegrees,
            power_basis_points: *power_basis_points,
            target_player_id: target_player_id.clone(),
            secondary_target_player_id: secondary_target_player_id.clone(),
        },
        MatchCommandKind::Move { .. }
        | MatchCommandKind::PassiveChoice { .. }
        | MatchCommandKind::Pass => return Err(SessionFault::ContractInvariant),
    };
    let mut replay_state = pre_state.clone();
    let replayed = match crate::command::apply_ability(&mut replay_state, &replay_command) {
        CommandResult::Accepted(replayed) => replayed,
        CommandResult::Rejected(_) => return Err(SessionFault::ContractInvariant),
    };
    if replayed.projectile_traces != outcome.projectile_traces
        || replayed.strikes != outcome.strikes
        || replay_state.rng_state != post_state.rng_state
    {
        return Err(SessionFault::ContractInvariant);
    }

    let mut cited_traces = BTreeSet::new();
    let mut eliminating_targets = BTreeSet::new();
    let mut direct_by_target: BTreeMap<&str, u32> = BTreeMap::new();
    let mut melee_records: Vec<(&str, crate::fixed::FixedPoint)> = Vec::new();

    for (index, strike) in outcome.strikes.iter().enumerate() {
        if usize::from(strike.strike_index) != index
            || pre_state.player(&strike.target_player_id).is_none()
            || post_state.player(&strike.target_player_id).is_none()
        {
            return Err(SessionFault::ContractInvariant);
        }
        let counter = direct_by_target
            .entry(strike.target_player_id.as_str())
            .or_insert(0);
        *counter = counter
            .checked_add(u32::from(strike.damage_applied))
            .ok_or(SessionFault::ContractInvariant)?;

        match strike.delivery {
            StrikeDelivery::Projectile { trace_sequence } => {
                if !matches!(ability.attack, Attack::Projectile(_))
                    || !cited_traces.insert(trace_sequence)
                {
                    return Err(SessionFault::ContractInvariant);
                }
                let trace = outcome
                    .projectile_traces
                    .iter()
                    .find(|trace| trace.sequence == trace_sequence)
                    .ok_or(SessionFault::ContractInvariant)?;
                if trace.impact.cause != ImpactCause::Character
                    || trace.impact.position != strike.impact_point
                {
                    return Err(SessionFault::ContractInvariant);
                }
                reconcile_primary_crit(ability, strike.crit)?;
            }
            StrikeDelivery::Melee => {
                if !matches!(ability.attack, Attack::Strike(_)) {
                    return Err(SessionFault::ContractInvariant);
                }
                melee_records.push((&strike.target_player_id, strike.impact_point));
                reconcile_primary_crit(ability, strike.crit)?;
            }
            StrikeDelivery::Effect { kind } => {
                if kind != EffectKind::MultiStrike
                    || !ability.effects.iter().any(|effect| effect.kind == kind)
                {
                    return Err(SessionFault::ContractInvariant);
                }
            }
        }

        if strike.eliminated_target
            && (!eliminating_targets.insert(strike.target_player_id.as_str())
                || pre_state
                    .player(&strike.target_player_id)
                    .is_none_or(crate::types::PlayerState::is_eliminated)
                || post_state
                    .player(&strike.target_player_id)
                    .is_none_or(|player| !player.is_eliminated()))
        {
            return Err(SessionFault::ContractInvariant);
        }
    }

    match ability.attack {
        Attack::Projectile(_) => {
            let character_impacts: BTreeSet<u32> = outcome
                .projectile_traces
                .iter()
                .filter(|trace| trace.impact.cause == ImpactCause::Character)
                .map(|trace| trace.sequence)
                .collect();
            if cited_traces != character_impacts || !melee_records.is_empty() {
                return Err(SessionFault::ContractInvariant);
            }
        }
        Attack::Strike(strike_attack) => {
            if !cited_traces.is_empty() {
                return Err(SessionFault::ContractInvariant);
            }
            let actor_position = pre_state
                .player(&command.player_id)
                .ok_or(SessionFault::ContractInvariant)?
                .position;
            let (target_ids, impact_point): (Vec<&str>, crate::fixed::FixedPoint) =
                if let MatchCommandKind::Ability {
                    target_player_id: Some(target_id),
                    ..
                } = &command.kind
                {
                    let target = pre_state
                        .player(target_id)
                        .ok_or(SessionFault::ContractInvariant)?;
                    (vec![target_id.as_str()], target.position)
                } else {
                    (
                        pre_state
                            .players
                            .iter()
                            .filter(|player| {
                                player.id != command.player_id && !player.is_eliminated()
                            })
                            .filter(|player| {
                                crate::fixed::within_radius(
                                    player.position,
                                    actor_position,
                                    strike_attack.range,
                                )
                            })
                            .map(|player| player.id.as_str())
                            .collect(),
                        actor_position,
                    )
                };
            let mut expected = Vec::new();
            for _ in 0..ability.strikes_per_turn.max(1) {
                expected.extend(
                    target_ids
                        .iter()
                        .map(|target_id| (*target_id, impact_point)),
                );
            }
            if melee_records != expected {
                return Err(SessionFault::ContractInvariant);
            }
        }
    }

    let has_unrecorded_direct_effect = ability.effects.iter().any(|effect| {
        matches!(
            effect.kind,
            EffectKind::Cluster | EffectKind::Return | EffectKind::Tunnel
        )
    });
    if !has_unrecorded_direct_effect {
        let aggregate_by_target: BTreeMap<&str, u32> = outcome
            .damage
            .iter()
            .map(|damage| (damage.player_id.as_str(), u32::from(damage.direct)))
            .collect();
        if aggregate_by_target.len() != outcome.damage.len() {
            return Err(SessionFault::ContractInvariant);
        }
        for (target_id, direct) in direct_by_target {
            if aggregate_by_target.get(target_id).copied().unwrap_or(0) != direct {
                return Err(SessionFault::ContractInvariant);
            }
        }
        if aggregate_by_target.iter().any(|(target_id, direct)| {
            *direct > 0
                && !outcome
                    .strikes
                    .iter()
                    .any(|strike| strike.target_player_id == **target_id)
        }) {
            return Err(SessionFault::ContractInvariant);
        }
    }
    Ok(())
}

fn reconcile_primary_crit(
    ability: &crate::types::AbilityDefinition,
    crit: CritRoll,
) -> Result<(), SessionFault> {
    let can_crit = ability.crit_chance_basis_points > 0
        || ability.crit_damage_percent > ability.damage_percent;
    if (can_crit && crit == CritRoll::NotEligible) || (!can_crit && crit != CritRoll::NotEligible) {
        return Err(SessionFault::ContractInvariant);
    }
    Ok(())
}

/// Everything the authoritative layer recorded while applying one command.
///
/// Grouped rather than passed as loose parameters because they are one concept — the
/// producers' own account of what happened — and because a caller must not be able to supply
/// one without the others and leave the event stream half-explained.
struct AppliedRecords<'a> {
    /// The command outcome, absent for kinds that produce none.
    outcome: Option<&'a CommandOutcome>,
    /// Status transitions, in the order they happened.
    status_changes: &'a [StatusChange],
    /// Persistent-object transitions, in the order they happened.
    object_changes: &'a [PersistentObjectChange],
}

fn derive_events(
    command: &MatchCommand,
    pre_state: &SimulationState,
    post_state: &SimulationState,
    pre_snapshot: &MatchSnapshot,
    post_snapshot: &MatchSnapshot,
    records: AppliedRecords<'_>,
) -> Result<Vec<PresentationEvent>, SessionFault> {
    let AppliedRecords {
        outcome: command_outcome,
        status_changes,
        object_changes,
    } = records;
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
        reconcile_strikes(command, ability, pre_state, post_state, outcome)?;
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
                // An effect-delivered strike has no trace of its own, and effects resolve
                // after the ability's primary attack. It is presented at the action's
                // resolution tick, ranked after any projectile-delivered strike sharing
                // that tick, so the ordering matches the order the resolver produced them.
                StrikeDelivery::Effect { .. } => (resolution_tick, 4),
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

        reconcile_random_outcomes(command, ability, pre_state, post_state, outcome)?;
        for random_outcome in &outcome.random_outcomes {
            push_pending(
                &mut pending,
                resolution_tick,
                5,
                PresentationEventKind::RandomOutcome {
                    owner_id: command.player_id.clone(),
                    ability_id: ability.id.to_owned(),
                    outcome: random_outcome.clone(),
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

    reconcile_status_changes(pre_snapshot, post_snapshot, status_changes)?;
    // Emit from the producer record itself, once, rather than once per snapshot player.
    // Grouping by player would be deterministic but chronologically false for an interleaved
    // multi-player lifecycle such as A-applied, B-applied, A-expired, B-expired.
    for change in status_changes {
        push_pending(
            &mut pending,
            resolution_tick,
            15,
            PresentationEventKind::StatusChanged {
                player_id: change.player_id.clone(),
                kind: crate::client_contract::snapshot_status_kind(change.kind),
                transition: change.transition.clone(),
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
            let cause = elimination_cause(command, pre_state, command_outcome, &post_player.id)?;
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

    reconcile_object_changes(
        &pre_snapshot.persistent_objects,
        &post_snapshot.persistent_objects,
        object_changes,
    )?;

    // Spawns and removals come from the producers that caused them, in the order they
    // happened. A replacement removes then spawns and an eviction spawns then removes; only
    // an ordered stream keeps those distinguishable, and only a record can describe an
    // object that spawned and was removed inside this one command.
    for change in object_changes {
        let projection = crate::client_contract::snapshot_object(&change.object);
        let kind = match change.transition {
            PersistentObjectTransition::Spawned => {
                PresentationEventKind::ObjectSpawned { object: projection }
            }
            PersistentObjectTransition::Removed { cause } => PresentationEventKind::ObjectRemoved {
                previous: projection,
                cause,
            },
        };
        push_pending(&mut pending, resolution_tick, 16, kind)?;
    }

    // `ObjectChanged` stays snapshot-derived: an object that survived the command is fully
    // visible in both snapshots, and no producer records in-place mutation.
    for current in &post_snapshot.persistent_objects {
        match pre_snapshot
            .persistent_objects
            .iter()
            .find(|previous| previous.sequence == current.sequence)
        {
            Some(previous) if previous != current => push_pending(
                &mut pending,
                resolution_tick,
                17,
                PresentationEventKind::ObjectChanged {
                    previous: previous.clone(),
                    current: current.clone(),
                },
            )?,
            Some(_) => {}
            // A new object already passed the producer-record replay above.
            None => {}
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

fn elimination_cause(
    command: &MatchCommand,
    pre_state: &SimulationState,
    outcome: Option<&CommandOutcome>,
    player_id: &str,
) -> Result<ChangeProvenance, SessionFault> {
    let Some(outcome) = outcome else {
        return Ok(ChangeProvenance::AuthoritativeResolution);
    };

    let mut eliminating_strikes = outcome
        .strikes
        .iter()
        .filter(|strike| strike.target_player_id == player_id && strike.eliminated_target);
    if let Some(strike) = eliminating_strikes.next() {
        // A player cannot transition from living to eliminated twice without an intervening
        // revival, and no such mechanic exists. Two producer records claiming the kill are a
        // contradictory outcome and must fault before the working host is published.
        if eliminating_strikes.next().is_some() {
            return Err(SessionFault::ContractInvariant);
        }
        let ability_id = command_ability_id(command, pre_state)?;
        return Ok(ChangeProvenance::Strike {
            owner_id: command.player_id.clone(),
            ability_id,
            strike_index: strike.strike_index,
        });
    }

    let Some(damage) = outcome
        .damage
        .iter()
        .find(|damage| damage.player_id == player_id && damage.eliminated)
    else {
        // Host-owned settling can still eliminate after the command outcome was finalized.
        return Ok(ChangeProvenance::AuthoritativeResolution);
    };
    let ability_id = command_ability_id(command, pre_state)?;
    let attributed = |constructor: fn(String, String) -> ChangeProvenance| {
        constructor(command.player_id.clone(), ability_id.clone())
    };
    if damage.backlash > 0 {
        return Ok(attributed(|owner_id, ability_id| {
            ChangeProvenance::Backlash {
                owner_id,
                ability_id,
            }
        }));
    }
    if damage.wall_impact > 0 {
        return Ok(attributed(|owner_id, ability_id| {
            ChangeProvenance::WallImpact {
                owner_id,
                ability_id,
            }
        }));
    }
    if damage.splash > 0 {
        return Ok(attributed(|owner_id, ability_id| {
            ChangeProvenance::Splash {
                owner_id,
                ability_id,
            }
        }));
    }
    if damage.hazard > 0 {
        return Ok(ChangeProvenance::Hazard);
    }
    Ok(attributed(|owner_id, ability_id| {
        ChangeProvenance::AbilityEffect {
            owner_id,
            ability_id,
        }
    }))
}

fn command_ability_id(
    command: &MatchCommand,
    pre_state: &SimulationState,
) -> Result<String, SessionFault> {
    let MatchCommandKind::Ability { slot, .. } = command.kind else {
        return Err(SessionFault::ContractInvariant);
    };
    let player = pre_state
        .player(&command.player_id)
        .ok_or(SessionFault::ContractInvariant)?;
    let definition =
        character::find(&player.character_id).ok_or(SessionFault::ContractInvariant)?;
    definition
        .ability(slot)
        .map(|ability| ability.id.to_owned())
        .ok_or(SessionFault::ContractInvariant)
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
            retained_ledger_entry_bytes(
                &LedgerRequest::Client(retained_command.clone()),
                &transition
            )
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
            retained_ledger_entry_bytes(
                &LedgerRequest::Client(original_command.clone()),
                &original
            )
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
        let rejected_entry_bytes =
            retained_ledger_entry_bytes(&LedgerRequest::Client(stale.clone()), &first)
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
        let entry_bytes = retained_ledger_entry_bytes(
            &LedgerRequest::Client(probe_command.clone()),
            &probe_transition,
        )
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
    fn complete_checkpoint_restore_preserves_replay_and_can_continue() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let mut stale = pass_command(&session, "restore-stale");
        stale.expected_snapshot_generation = 99;
        let stale_first = session
            .apply(stale.clone())
            .expect("stale receipt must be retained");

        let move_command = command(
            &session,
            "restore-move",
            MatchCommandKind::Move { dx: POSITION_SCALE },
        );
        let move_first = session
            .apply(move_command.clone())
            .expect("move must resolve");
        let ability = ability_command(&session, "restore-ability");
        let ability_first = session
            .apply(ability.clone())
            .expect("ability must resolve");
        let expected_snapshot = session.snapshot();
        let expected_generation = session.generation();
        let expected_ledger_bytes = session.ledger_bytes();
        let expected_ledger_len = session.ledger_len();

        let checkpoint = session.checkpoint().expect("live session must checkpoint");
        let mut restored =
            MatchSessionHost::restore(checkpoint).expect("complete checkpoint must restore");

        assert_eq!(restored.snapshot(), expected_snapshot);
        assert_eq!(restored.generation(), expected_generation);
        assert_eq!(restored.ledger_len(), expected_ledger_len);
        assert_eq!(restored.ledger_bytes(), expected_ledger_bytes);
        for (request, first) in [
            (stale, stale_first),
            (move_command, move_first),
            (ability, ability_first),
        ] {
            let replay = restored.apply(request).expect("first receipt must replay");
            assert_eq!(replay.disposition, TransitionDisposition::DuplicateReplay);
            assert_eq!(replay.command_id, first.command_id);
            assert_eq!(replay.post_snapshot, first.post_snapshot);
            assert_eq!(replay.events, first.events);
        }
        assert_eq!(restored.snapshot(), expected_snapshot);
        assert_eq!(restored.ledger_len(), expected_ledger_len);
        assert_eq!(restored.ledger_bytes(), expected_ledger_bytes);

        let next = pass_command(&restored, "restore-continues");
        let continued = restored
            .apply(next)
            .expect("restored session must continue");
        assert_eq!(continued.disposition, TransitionDisposition::Accepted);
        assert_eq!(restored.generation(), expected_generation + 1);
    }

    #[test]
    fn restore_rejects_incomplete_or_misaccounted_ledgers() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let mut stale = pass_command(&session, "checkpoint-rejected");
        stale.expected_snapshot_generation = 99;
        session
            .apply(stale)
            .expect("stale receipt must be retained");
        let pass = pass_command(&session, "checkpoint-accepted");
        session.apply(pass).expect("pass must resolve");
        let checkpoint = session.checkpoint().expect("live session must checkpoint");

        let mut missing_rejection = checkpoint.clone();
        let _removed = missing_rejection.ledger.remove("checkpoint-rejected");
        assert!(matches!(
            MatchSessionHost::restore(missing_rejection),
            Err(SessionFault::ContractInvariant)
        ));

        let mut missing_acceptance = checkpoint.clone();
        let removed = missing_acceptance
            .ledger
            .remove("checkpoint-accepted")
            .expect("accepted entry must exist");
        let removed_bytes = retained_ledger_entry_bytes(&removed.request, &removed.transition)
            .expect("fixture entry must be countable");
        missing_acceptance.declared_ledger_len -= 1;
        missing_acceptance.declared_ledger_bytes -= removed_bytes;
        assert!(matches!(
            MatchSessionHost::restore(missing_acceptance),
            Err(SessionFault::ContractInvariant)
        ));

        let mut wrong_bytes = checkpoint.clone();
        wrong_bytes.declared_ledger_bytes = wrong_bytes.declared_ledger_bytes.saturating_add(1);
        assert!(matches!(
            MatchSessionHost::restore(wrong_bytes),
            Err(SessionFault::ContractInvariant)
        ));

        let mut wrong_digest = checkpoint.clone();
        wrong_digest
            .ledger
            .get_mut("checkpoint-accepted")
            .expect("accepted entry must exist")
            .canonical_digest = "0000000000000000".to_owned();
        assert!(matches!(
            MatchSessionHost::restore(wrong_digest),
            Err(SessionFault::ContractInvariant)
        ));

        let mut replay_in_ledger = checkpoint.clone();
        replay_in_ledger
            .ledger
            .get_mut("checkpoint-accepted")
            .expect("accepted entry must exist")
            .transition
            .disposition = TransitionDisposition::DuplicateReplay;
        assert!(matches!(
            MatchSessionHost::restore(replay_in_ledger),
            Err(SessionFault::ContractInvariant)
        ));
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
    use crate::match_setup::{MatchMode, MatchPlayerConfig, build_initial_state};
    use crate::types::Appearance;

    const TARGET: &str = "b-local-bot";

    fn karl_duel() -> MatchConfig {
        MatchConfig {
            // Seed 1 yields the stable ordered crit sequence Landed/Missed/Missed after
            // rejection sampling, which makes the draw-order mutation test non-vacuous.
            seed: 1,
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

    fn low_health_karl_session() -> MatchSessionHost {
        let mut state = build_initial_state(&karl_duel()).expect("fixture state must build");
        state
            .player_mut(TARGET)
            .expect("fixture target must exist")
            .health = 1;
        let host = MatchHost::start(state).expect("fixture match must start");
        MatchSessionHost::from_new_host(host)
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
                StrikeDelivery::Melee | StrikeDelivery::Effect { .. } => {
                    panic!("Carrion Call delivers every strike by projectile")
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

    #[test]
    fn omitted_or_tampered_strike_records_fail_before_session_publication() {
        let session = MatchSessionHost::create(&karl_duel()).expect("fixture session");
        let command = landing_volley(&session);
        let pre_state = session.host().state().clone();
        let mut working = session.host().clone();
        let outcome = match apply_to_working_host(&mut working, &command)
            .expect("fixture host application must not fault")
        {
            AppliedCommand::Accepted(Some(outcome)) => outcome,
            AppliedCommand::Accepted(None) => panic!("fixture ability must retain an outcome"),
            AppliedCommand::Rejected(reason) => panic!("fixture ability rejected: {reason:?}"),
        };
        let definition = character::find(
            &pre_state
                .player(&command.player_id)
                .expect("fixture actor")
                .character_id,
        )
        .expect("fixture definition");
        let ability = definition
            .ability(AbilitySlot::Basic)
            .expect("fixture basic");
        assert_eq!(
            reconcile_strikes(&command, ability, &pre_state, working.state(), &outcome),
            Ok(())
        );

        let mut missing = outcome.as_ref().clone();
        let _removed = missing.strikes.pop();
        assert_eq!(
            reconcile_strikes(&command, ability, &pre_state, working.state(), &missing),
            Err(SessionFault::ContractInvariant)
        );

        let mut tampered = outcome.as_ref().clone();
        let Some(first) = tampered.strikes.first_mut() else {
            panic!("fixture must contain strikes")
        };
        first.damage_applied = first.damage_applied.saturating_add(1);
        assert_eq!(
            reconcile_strikes(&command, ability, &pre_state, working.state(), &tampered),
            Err(SessionFault::ContractInvariant)
        );

        let mut reordered_draws = outcome.as_ref().clone();
        assert_eq!(
            reordered_draws
                .strikes
                .iter()
                .map(|strike| strike.crit)
                .collect::<Vec<_>>(),
            vec![CritRoll::Landed, CritRoll::Missed, CritRoll::Missed],
            "the mutation fixture must contain distinguishable ordered draws",
        );
        let first_pair = (
            reordered_draws.strikes[0].crit,
            reordered_draws.strikes[0].damage_applied,
        );
        let second_pair = (
            reordered_draws.strikes[1].crit,
            reordered_draws.strikes[1].damage_applied,
        );
        reordered_draws.strikes[0].crit = second_pair.0;
        reordered_draws.strikes[0].damage_applied = second_pair.1;
        reordered_draws.strikes[1].crit = first_pair.0;
        reordered_draws.strikes[1].damage_applied = first_pair.1;
        assert_eq!(
            reordered_draws
                .strikes
                .iter()
                .map(|strike| u32::from(strike.damage_applied))
                .sum::<u32>(),
            outcome
                .strikes
                .iter()
                .map(|strike| u32::from(strike.damage_applied))
                .sum::<u32>(),
            "the mutation must preserve the aggregate the older check accepted",
        );
        assert_eq!(
            reconcile_strikes(
                &command,
                ability,
                &pre_state,
                working.state(),
                &reordered_draws,
            ),
            Err(SessionFault::ContractInvariant)
        );

        let mut duplicate_kill = outcome.as_ref().clone();
        let Some(first) = duplicate_kill.strikes.first_mut() else {
            panic!("fixture must contain strikes")
        };
        first.eliminated_target = true;
        let Some(second) = duplicate_kill.strikes.get_mut(1) else {
            panic!("fixture must contain multiple strikes")
        };
        second.eliminated_target = true;
        assert_eq!(
            reconcile_strikes(
                &command,
                ability,
                &pre_state,
                working.state(),
                &duplicate_kill,
            ),
            Err(SessionFault::ContractInvariant)
        );

        // Reconciliation is performed against the working clone. The live session remains
        // generation zero with an empty ledger until every record passes and commit occurs.
        assert_eq!(session.host().state(), &pre_state);
        assert_eq!(session.generation(), 0);
        assert_eq!(session.ledger_len(), 0);
        assert_eq!(session.ledger_bytes(), 0);
    }

    #[test]
    fn omitting_the_exact_strike_elimination_flag_fails_reconciliation() {
        let session = low_health_karl_session();
        let command = landing_volley(&session);
        let pre_state = session.host().state().clone();
        let mut working = session.host().clone();
        let outcome = match apply_to_working_host(&mut working, &command)
            .expect("fixture host application must not fault")
        {
            AppliedCommand::Accepted(Some(outcome)) => outcome,
            AppliedCommand::Accepted(None) => panic!("fixture ability must retain an outcome"),
            AppliedCommand::Rejected(reason) => panic!("fixture ability rejected: {reason:?}"),
        };
        let definition = character::find(
            &pre_state
                .player(&command.player_id)
                .expect("fixture actor")
                .character_id,
        )
        .expect("fixture definition");
        let ability = definition
            .ability(AbilitySlot::Basic)
            .expect("fixture basic");
        let mut omitted = outcome.as_ref().clone();
        let killing_strike = omitted
            .strikes
            .iter_mut()
            .find(|strike| strike.eliminated_target)
            .expect("low-health fixture must contain a killing strike");
        killing_strike.eliminated_target = false;

        assert_eq!(
            reconcile_strikes(&command, ability, &pre_state, working.state(), &omitted),
            Err(SessionFault::ContractInvariant)
        );
        assert_eq!(session.host().state(), &pre_state);
        assert_eq!(session.generation(), 0);
        assert_eq!(session.ledger_len(), 0);
    }

    #[test]
    fn omitting_an_uncited_miss_trace_fails_exact_reconciliation() {
        let session = low_health_karl_session();
        let command = landing_volley(&session);
        let pre_state = session.host().state().clone();
        let mut working = session.host().clone();
        let outcome = match apply_to_working_host(&mut working, &command)
            .expect("fixture host application must not fault")
        {
            AppliedCommand::Accepted(Some(outcome)) => outcome,
            AppliedCommand::Accepted(None) => panic!("fixture ability must retain an outcome"),
            AppliedCommand::Rejected(reason) => panic!("fixture ability rejected: {reason:?}"),
        };
        let definition = character::find(
            &pre_state
                .player(&command.player_id)
                .expect("fixture actor")
                .character_id,
        )
        .expect("fixture definition");
        let ability = definition
            .ability(AbilitySlot::Basic)
            .expect("fixture basic");
        let mut omitted = outcome.as_ref().clone();
        let miss_index = omitted
            .projectile_traces
            .iter()
            .position(|trace| trace.impact.cause != ImpactCause::Character)
            .expect("later projectiles must miss after the first eliminates the only target");
        let removed_sequence = omitted.projectile_traces.remove(miss_index).sequence;
        assert!(
            omitted.strikes.iter().all(|strike| !matches!(
                strike.delivery,
                StrikeDelivery::Projectile { trace_sequence }
                    if trace_sequence == removed_sequence
            )),
            "the removed miss must be independently uncited by any strike",
        );

        assert_eq!(
            reconcile_strikes(&command, ability, &pre_state, working.state(), &omitted),
            Err(SessionFault::ContractInvariant)
        );
        assert_eq!(session.host().state(), &pre_state);
        assert_eq!(session.generation(), 0);
        assert_eq!(session.ledger_len(), 0);
    }
}

/// Producer-owned non-strike RNG provenance and fail-closed session reconciliation.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod random_outcome_tests {
    use super::*;
    use crate::fixed::BODY_WIDTH;
    use crate::match_setup::{MatchMode, MatchPlayerConfig, build_initial_state};
    use crate::types::{Appearance, GAUGE_FULL};

    fn player(player_id: &str, team: u8, character_id: &str) -> MatchPlayerConfig {
        MatchPlayerConfig {
            player_id: player_id.to_owned(),
            team,
            character_id: character_id.to_owned(),
            appearance: Appearance::default(),
        }
    }

    fn special_session(
        actor_character_id: &str,
        actor_passive_id: &str,
        seed: u64,
    ) -> MatchSessionHost {
        let config = MatchConfig {
            seed,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                player("a-actor", 0, actor_character_id),
                player("b-target", 1, "huck"),
            ],
        };
        let mut state = build_initial_state(&config).expect("fixture state must build");
        let actor_position = state.player("a-actor").expect("fixture actor").position;
        // Keep both players on the known-supported spawn cell. Moving the target sideways
        // before `MatchHost::start` can place it over a gap in the horizontal test map, and
        // the initial settle would correctly eliminate it before the special is submitted.
        let target_position = actor_position;

        let actor = state.player_mut("a-actor").expect("fixture actor");
        actor.special_gauge = GAUGE_FULL;
        actor.has_chosen_passive = true;
        actor.passive_id = Some(actor_passive_id.to_owned());
        state
            .player_mut("b-target")
            .expect("fixture target")
            .position = target_position;

        let host = MatchHost::start(state).expect("fixture match must start");
        MatchSessionHost::from_new_host(host)
    }

    fn special_command(session: &MatchSessionHost, command_id: &str) -> MatchCommand {
        MatchCommand {
            schema_version: CLIENT_CONTRACT_VERSION,
            command_id: command_id.to_owned(),
            player_id: session.host().active_player().to_owned(),
            expected_turn_number: session.host().state().turn_number,
            expected_snapshot_generation: session.generation(),
            kind: MatchCommandKind::Ability {
                slot: AbilitySlot::Special,
                angle_millidegrees: 45_000,
                power_basis_points: 5_000,
                target_player_id: Some("b-target".to_owned()),
                secondary_target_player_id: None,
            },
        }
    }

    fn applied_outcome(
        session: &MatchSessionHost,
        command: &MatchCommand,
    ) -> (SimulationState, SimulationState, Box<CommandOutcome>) {
        let pre_state = session.host().state().clone();
        let mut working = session.host().clone();
        let result = apply_to_working_host(&mut working, command)
            .expect("fixture host application must not fault");
        let outcome = match result {
            AppliedCommand::Accepted(Some(outcome)) => outcome,
            AppliedCommand::Accepted(None) => panic!("fixture special must retain its outcome"),
            AppliedCommand::Rejected(reason) => panic!("fixture special rejected: {reason:?}"),
        };
        (pre_state, working.state().clone(), outcome)
    }

    fn ability_for(
        state: &SimulationState,
        command: &MatchCommand,
    ) -> &'static crate::types::AbilityDefinition {
        let player = state
            .player(&command.player_id)
            .expect("fixture actor must exist");
        let definition = character::find(&player.character_id).expect("fixture definition");
        definition
            .ability(AbilitySlot::Special)
            .expect("fixture special ability")
    }

    #[test]
    fn arzum_target_draw_is_emitted_after_the_strike_with_exact_public_bounds() {
        let mut session = special_session("arzum", "arzum-momentum", 4_242);
        let command = special_command(&session, "arzum-random-target");
        let target_position = session
            .host()
            .state()
            .player("b-target")
            .expect("fixture target")
            .position;
        let pre_rng = session.host().state().rng_state;

        let transition = session.apply(command).expect("special must resolve");
        assert_eq!(
            transition.disposition,
            TransitionDisposition::Accepted,
            "unexpected rejection: {:?}",
            transition.rejection_reason,
        );
        let random_events: Vec<_> = transition
            .events
            .iter()
            .filter(|event| matches!(event.kind, PresentationEventKind::RandomOutcome { .. }))
            .collect();
        assert_eq!(random_events.len(), 1);
        let PresentationEventKind::RandomOutcome {
            owner_id,
            ability_id,
            outcome,
        } = &random_events[0].kind
        else {
            panic!("filtered event must be random outcome");
        };
        assert_eq!(owner_id, "a-actor");
        assert_eq!(ability_id, "arzum-chain-strike");
        assert_eq!(
            outcome,
            &RandomOutcome::ArzumChainStrikeTeleportTarget {
                candidate_count: 1,
                selected_index: 0,
                target_player_id: "b-target".to_owned(),
                destination: target_position,
            }
        );
        let strike_sequence = transition
            .events
            .iter()
            .find(|event| matches!(event.kind, PresentationEventKind::StrikeResolved { .. }))
            .expect("Arzum special must retain its first strike")
            .sequence;
        assert!(strike_sequence < random_events[0].sequence);
        assert_ne!(session.host().state().rng_state, pre_rng);
    }

    #[test]
    fn aleph_point_draw_is_emitted_with_the_bounded_pair_and_legal_destination() {
        let mut session = special_session("aleph", "aleph-volatile", 99);
        let command = special_command(&session, "aleph-random-point");
        let actor_before = session
            .host()
            .state()
            .player("a-actor")
            .expect("fixture actor")
            .position;

        let transition = session.apply(command).expect("special must resolve");
        assert_eq!(
            transition.disposition,
            TransitionDisposition::Accepted,
            "unexpected rejection: {:?}",
            transition.rejection_reason,
        );
        let random = transition
            .events
            .iter()
            .find_map(|event| match &event.kind {
                PresentationEventKind::RandomOutcome {
                    owner_id,
                    ability_id,
                    outcome,
                } => Some((owner_id, ability_id, outcome)),
                _ => None,
            })
            .expect("Veilstep must publish its point draw");
        assert_eq!(random.0, "a-actor");
        assert_eq!(random.1, "aleph-veilstep");
        let RandomOutcome::AlephVeilstepTeleportPoint {
            axis_bound,
            x_result,
            y_result,
            drawn_point,
            destination,
            ..
        } = random.2
        else {
            panic!("Veilstep must use the point outcome variant");
        };
        assert_eq!(
            *axis_bound,
            u32::try_from(16 * BODY_WIDTH + 1).expect("bound")
        );
        assert!(*x_result < *axis_bound && *y_result < *axis_bound);
        assert!(crate::fixed::within_radius(
            *drawn_point,
            actor_before,
            8 * BODY_WIDTH,
        ));
        assert_eq!(
            transition
                .post_snapshot
                .players
                .iter()
                .find(|player| player.id == "a-actor")
                .expect("post actor")
                .position
                .x,
            destination.x,
            "ordinary settling may change Y, but never the chosen destination X",
        );
    }

    #[test]
    fn omitted_duplicated_or_tampered_random_records_fail_reconciliation() {
        for (character_id, passive_id, seed) in [
            ("arzum", "arzum-momentum", 4_242),
            ("aleph", "aleph-volatile", 99),
        ] {
            let session = special_session(character_id, passive_id, seed);
            let command = special_command(&session, "mutation-random-outcome");
            let (pre_state, post_state, outcome) = applied_outcome(&session, &command);
            let ability = ability_for(&pre_state, &command);
            assert_eq!(
                reconcile_random_outcomes(&command, ability, &pre_state, &post_state, &outcome),
                Ok(())
            );

            if character_id == "arzum" {
                let mut altered_final_state = post_state.clone();
                let Some(recorded_target) =
                    outcome
                        .random_outcomes
                        .first()
                        .and_then(|record| match record {
                            RandomOutcome::ArzumChainStrikeTeleportTarget {
                                target_player_id,
                                ..
                            } => Some(target_player_id.as_str()),
                            RandomOutcome::AlephVeilstepTeleportPoint { .. } => None,
                        })
                else {
                    panic!("Arzum fixture must record its target")
                };
                let Some(target) = altered_final_state.player_mut(recorded_target) else {
                    panic!("recorded target must remain in state")
                };
                target.position.x = target
                    .position
                    .x
                    .saturating_add(100 * crate::fixed::BODY_WIDTH);
                assert_eq!(
                    reconcile_random_outcomes(
                        &command,
                        ability,
                        &pre_state,
                        &altered_final_state,
                        &outcome,
                    ),
                    Ok(()),
                    "Arzum reconciliation must use draw-time state, not final positions"
                );
            }

            let mut missing = outcome.as_ref().clone();
            missing.random_outcomes.clear();
            assert_eq!(
                reconcile_random_outcomes(&command, ability, &pre_state, &post_state, &missing),
                Err(SessionFault::ContractInvariant)
            );

            let mut duplicate = outcome.as_ref().clone();
            duplicate
                .random_outcomes
                .extend(outcome.random_outcomes.iter().cloned());
            assert_eq!(
                reconcile_random_outcomes(&command, ability, &pre_state, &post_state, &duplicate),
                Err(SessionFault::ContractInvariant)
            );

            let mut tampered = outcome.as_ref().clone();
            match tampered.random_outcomes.first_mut() {
                Some(RandomOutcome::ArzumChainStrikeTeleportTarget { selected_index, .. }) => {
                    *selected_index = selected_index.saturating_add(1)
                }
                Some(RandomOutcome::AlephVeilstepTeleportPoint { x_result, .. }) => {
                    *x_result = x_result.saturating_add(1)
                }
                None => panic!("fixture must contain a random outcome"),
            }
            assert_eq!(
                reconcile_random_outcomes(&command, ability, &pre_state, &post_state, &tampered),
                Err(SessionFault::ContractInvariant)
            );
        }
    }
}

/// Direct end-to-end transition scenarios required by `CLIENT_SPEC.md` § 20.1.
///
/// These deliberately use real roster definitions, the real horizontal map, `MatchHost`, and
/// `MatchSessionHost`.  Unit tests of the individual resolver or diff helper are valuable but do
/// not prove that the complete ordered event vocabulary survives the publication boundary.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod direct_transition_scenario_tests {
    use super::*;
    use crate::match_setup::{MatchMode, MatchPlayerConfig, build_initial_state};
    use crate::types::{Appearance, GAUGE_FULL};

    fn player(player_id: &str, team: u8, character_id: &str) -> MatchPlayerConfig {
        MatchPlayerConfig {
            player_id: player_id.to_owned(),
            team,
            character_id: character_id.to_owned(),
            appearance: Appearance::default(),
        }
    }

    fn config(actor_character_id: &str, target_character_id: &str) -> MatchConfig {
        MatchConfig {
            seed: 12_345,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                player("a-actor", 0, actor_character_id),
                player("b-target", 1, target_character_id),
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

    fn ability(
        session: &MatchSessionHost,
        command_id: &str,
        slot: AbilitySlot,
        target_player_id: Option<&str>,
    ) -> MatchCommand {
        command(
            session,
            command_id,
            MatchCommandKind::Ability {
                slot,
                angle_millidegrees: 0,
                power_basis_points: 5_000,
                target_player_id: target_player_id.map(str::to_owned),
                secondary_target_player_id: None,
            },
        )
    }

    fn event_index(
        transition: &MatchTransition,
        predicate: impl Fn(&PresentationEventKind) -> bool,
    ) -> usize {
        transition
            .events
            .iter()
            .position(|event| predicate(&event.kind))
            .expect("required event must be present")
    }

    fn assert_post_hash_is_live(session: &MatchSessionHost, transition: &MatchTransition) {
        let live = crate::hash::hash_state(session.host().state());
        assert_eq!(transition.post_state_hash, live);
        assert_eq!(transition.post_snapshot.authoritative_state_hash, live);
    }

    #[test]
    fn melee_terrain_and_block_mutation_are_one_ordered_real_transition() {
        let match_config = config("huck", "huck");
        let mut state = build_initial_state(&match_config).expect("fixture state must build");
        let actor_position = state.player("a-actor").expect("fixture actor").position;
        // Haymaker is melee-only. Keeping the target on the actor's supported spawn cell both
        // guarantees contact and centres its real crater over a real destructible map block.
        state
            .player_mut("b-target")
            .expect("fixture target")
            .position = actor_position;

        let host = MatchHost::start(state).expect("fixture match must start");
        let mut session = MatchSessionHost::from_new_host(host);
        let attack = ability(
            &session,
            "melee-terrain-passive",
            AbilitySlot::Basic,
            Some("b-target"),
        );
        let transition = session.apply(attack).expect("Haymaker must resolve");

        assert_eq!(transition.disposition, TransitionDisposition::Accepted);
        assert_post_hash_is_live(&session, &transition);

        let strike_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::StrikeResolved {
                    strike: StrikeResolution {
                        delivery: StrikeDelivery::Melee,
                        target_player_id,
                        ..
                    },
                    ..
                } if target_player_id == "b-target"
            )
        });
        let terrain_index = event_index(
            &transition,
            |kind| matches!(kind, PresentationEventKind::TerrainChanged { dirty_rectangles, .. } if !dirty_rectangles.is_empty()),
        );
        let block_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::BlockChanged {
                    previous_health: Some(previous),
                    new_health: Some(current),
                    ..
                } if current < previous
            )
        });
        assert!(strike_index < terrain_index);
        assert!(terrain_index < block_index);
    }

    #[test]
    fn passive_required_then_chosen_holds_and_resumes_the_same_real_turn() {
        let match_config = config("arzum", "huck");
        let mut state = build_initial_state(&match_config).expect("fixture state must build");
        let actor_position = state.player("a-actor").expect("fixture actor").position;
        state
            .player_mut("a-actor")
            .expect("fixture actor")
            .special_gauge = GAUGE_FULL - 1;
        // A point-blank projectile guarantees enough real damage to fill the final gauge unit,
        // while Arzum's basic has no terrain effect that could eliminate the fixture players.
        state
            .player_mut("b-target")
            .expect("fixture target")
            .position = actor_position;

        let host = MatchHost::start(state).expect("fixture match must start");
        let mut session = MatchSessionHost::from_new_host(host);
        let attack = ability(
            &session,
            "raise-passive-choice",
            AbilitySlot::Basic,
            Some("b-target"),
        );
        let transition = session.apply(attack).expect("Arzum basic must resolve");

        assert_eq!(transition.disposition, TransitionDisposition::Accepted);
        assert_eq!(session.host().phase(), MatchPhase::PassiveSelection);
        assert_post_hash_is_live(&session, &transition);
        let passive_required_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::PassiveChoiceRequired {
                    player_id,
                    passive_ids,
                } if player_id == "a-actor" && passive_ids.len() == 3
            )
        });
        let strike_index = event_index(&transition, |kind| {
            matches!(kind, PresentationEventKind::StrikeResolved { .. })
        });
        assert!(strike_index < passive_required_index);
        assert!(
            !transition
                .events
                .iter()
                .any(|event| matches!(event.kind, PresentationEventKind::TurnEnded { .. })),
            "the passive interrupt must hold the current turn open",
        );

        let choice = command(
            &session,
            "choose-arzum-passive",
            MatchCommandKind::PassiveChoice {
                passive_id: "arzum-momentum".to_owned(),
            },
        );
        let chosen = session.apply(choice).expect("passive choice must resolve");
        assert_post_hash_is_live(&session, &chosen);
        let chosen_index = event_index(&chosen, |kind| {
            matches!(
                kind,
                PresentationEventKind::PassiveChosen {
                    player_id,
                    passive_id,
                } if player_id == "a-actor" && passive_id == "arzum-momentum"
            )
        });
        let ended_index = event_index(&chosen, |kind| {
            matches!(
                kind,
                PresentationEventKind::TurnEnded {
                    player_id,
                    reason: ClientTurnEndReason::Attacked,
                } if player_id == "a-actor"
            )
        });
        let opened_index = event_index(&chosen, |kind| {
            matches!(
                kind,
                PresentationEventKind::TurnOpened { player_id, .. }
                    if player_id == "b-target"
            )
        });
        assert!(chosen_index < ended_index && ended_index < opened_index);
    }

    #[test]
    fn pass_reports_the_reason_and_opens_the_next_turn_in_order() {
        let mut session =
            MatchSessionHost::create(&config("zeke", "huck")).expect("fixture session must start");
        let pass = command(&session, "direct-pass", MatchCommandKind::Pass);
        let transition = session.apply(pass).expect("pass must resolve");

        assert_eq!(transition.disposition, TransitionDisposition::Accepted);
        assert_post_hash_is_live(&session, &transition);
        let ended_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::TurnEnded {
                    player_id,
                    reason: ClientTurnEndReason::Passed,
                } if player_id == "a-actor"
            )
        });
        let opened_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::TurnOpened {
                    player_id,
                    turn_number: 2,
                } if player_id == "b-target"
            )
        });
        assert!(ended_index < opened_index);
    }

    #[test]
    fn melee_elimination_is_attributed_before_victory_and_no_turn_reopens() {
        let match_config = config("natomica", "huck");
        let mut state = build_initial_state(&match_config).expect("fixture state must build");
        let actor_position = state.player("a-actor").expect("fixture actor").position;
        let actor = state.player_mut("a-actor").expect("fixture actor");
        actor.special_gauge = GAUGE_FULL;
        actor.has_chosen_passive = true;
        actor.passive_id = Some("natomica-stable-core".to_owned());
        let target = state.player_mut("b-target").expect("fixture target");
        target.position = actor_position;
        target.health = 1;

        let host = MatchHost::start(state).expect("fixture match must start");
        let mut session = MatchSessionHost::from_new_host(host);
        let attack = ability(&session, "victory-attribution", AbilitySlot::Special, None);
        let transition = session.apply(attack).expect("Repulse must resolve");

        assert_post_hash_is_live(&session, &transition);
        let strike_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::StrikeResolved {
                    strike: StrikeResolution {
                        target_player_id,
                        eliminated_target: true,
                        ..
                    },
                    ..
                } if target_player_id == "b-target"
            )
        });
        let eliminated_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::PlayerEliminated {
                    player_id,
                    cause: ChangeProvenance::Strike {
                        owner_id,
                        ability_id,
                        strike_index: 0,
                    },
                } if player_id == "b-target"
                    && owner_id == "a-actor"
                    && ability_id == "natomica-repulse"
            )
        });
        let turn_ended_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::TurnEnded {
                    player_id,
                    reason: ClientTurnEndReason::Attacked,
                } if player_id == "a-actor"
            )
        });
        let victory_index = event_index(&transition, |kind| {
            matches!(
                kind,
                PresentationEventKind::MatchCompleted {
                    outcome: ClientMatchOutcome::Victory { team: 0 },
                }
            )
        });
        assert!(strike_index < eliminated_index);
        assert!(eliminated_index < turn_ended_index);
        assert!(turn_ended_index < victory_index);
        assert!(
            !transition
                .events
                .iter()
                .any(|event| matches!(event.kind, PresentationEventKind::TurnOpened { .. })),
            "a completed match cannot reopen a planning turn",
        );
        assert_eq!(session.host().phase(), MatchPhase::MatchComplete);
        assert_eq!(
            session.host().outcome(),
            crate::types::MatchOutcome::Victory { team: 0 }
        );
    }
}

/// Read-only preview semantics, including the negative proof that no live state, RNG, generation,
/// or idempotency metadata changes even when the disposable legality clone runs a random ability.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod preview_tests {
    use super::*;
    use crate::match_setup::{MatchMode, MatchPlayerConfig, build_initial_state};
    use crate::types::{Appearance, GAUGE_FULL};

    fn config(actor_character_id: &str) -> MatchConfig {
        MatchConfig {
            seed: 9_876,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                MatchPlayerConfig {
                    player_id: "a-actor".to_owned(),
                    team: 0,
                    character_id: actor_character_id.to_owned(),
                    appearance: Appearance::default(),
                },
                MatchPlayerConfig {
                    player_id: "b-target".to_owned(),
                    team: 1,
                    character_id: "huck".to_owned(),
                    appearance: Appearance::default(),
                },
            ],
        }
    }

    fn request(session: &MatchSessionHost, slot: AbilitySlot) -> AbilityPreviewRequest {
        AbilityPreviewRequest {
            schema_version: CLIENT_CONTRACT_VERSION,
            expected_snapshot_generation: session.generation(),
            player_id: "a-actor".to_owned(),
            slot,
            angle_millidegrees: 45_000,
            power_basis_points: 1_500,
            target_player_id: None,
            secondary_target_player_id: None,
        }
    }

    fn assert_session_unchanged(
        session: &MatchSessionHost,
        before: &SimulationState,
        generation: u64,
        ledger_len: usize,
        ledger_bytes: u64,
    ) {
        assert_eq!(session.host().state(), before);
        assert_eq!(session.host().state().rng_state, before.rng_state);
        assert_eq!(session.generation(), generation);
        assert_eq!(session.ledger_len(), ledger_len);
        assert_eq!(session.ledger_bytes(), ledger_bytes);
    }

    #[test]
    fn projectile_preview_is_repeatable_and_mutates_nothing() {
        let session = MatchSessionHost::create(&config("zeke")).expect("fixture session");
        let preview_request = request(&session, AbilitySlot::Basic);
        let before = session.host().state().clone();

        let first = session
            .preview(&preview_request)
            .expect("preview must resolve");
        let second = session
            .preview(&preview_request)
            .expect("preview must repeat");

        assert_eq!(first, second);
        assert!(first.legal);
        assert_eq!(first.rejection_reason, None);
        assert_eq!(first.snapshot_generation, 0);
        assert_eq!(first.gauge_cost, 0);
        assert_eq!(first.projectile_traces.len(), 1);
        assert!(
            first
                .projectile_traces
                .first()
                .is_some_and(|trace| trace.samples.len() >= 2)
        );
        assert!(
            first
                .legal_target_player_ids
                .windows(2)
                .all(|pair| matches!(pair, [left, right] if left < right))
        );
        assert_session_unchanged(&session, &before, 0, 0, 0);
    }

    #[test]
    fn stale_and_illegal_previews_are_normal_non_mutating_responses() {
        let session = MatchSessionHost::create(&config("zeke")).expect("fixture session");
        let before = session.host().state().clone();
        let mut stale = request(&session, AbilitySlot::Basic);
        stale.expected_snapshot_generation = 1;

        let stale_response = session.preview(&stale).expect("staleness is not a fault");
        assert!(!stale_response.legal);
        assert_eq!(
            stale_response.rejection_reason,
            Some(PreviewRejection::SnapshotGenerationMismatch {
                expected: 1,
                actual: 0,
            })
        );
        assert!(stale_response.projectile_traces.is_empty());
        assert!(stale_response.legal_target_player_ids.is_empty());

        let mut illegal = request(&session, AbilitySlot::Basic);
        illegal.angle_millidegrees = 360_000;
        let illegal_response = session
            .preview(&illegal)
            .expect("gameplay refusal is normal");
        assert!(!illegal_response.legal);
        assert_eq!(
            illegal_response.rejection_reason,
            Some(PreviewRejection::Core(CommandRejection::InputOutOfRange))
        );
        assert!(illegal_response.projectile_traces.is_empty());

        let special = session
            .preview(&request(&session, AbilitySlot::Special))
            .expect("gauge refusal is normal");
        assert!(!special.legal);
        assert_eq!(special.gauge_cost, GAUGE_FULL);
        assert_eq!(
            special.rejection_reason,
            Some(PreviewRejection::Core(CommandRejection::GaugeNotReady))
        );
        assert_session_unchanged(&session, &before, 0, 0, 0);
    }

    #[test]
    fn random_ability_legality_runs_only_on_a_disposable_clone() {
        let match_config = config("aleph");
        let mut state = build_initial_state(&match_config).expect("fixture state must build");
        let actor_position = state.player("a-actor").expect("fixture actor").position;
        let actor = state.player_mut("a-actor").expect("fixture actor");
        actor.special_gauge = GAUGE_FULL;
        actor.has_chosen_passive = true;
        actor.passive_id = Some("aleph-volatile".to_owned());
        state
            .player_mut("b-target")
            .expect("fixture target")
            .position = actor_position;
        let host = MatchHost::start(state).expect("fixture match must start");
        let session = MatchSessionHost::from_new_host(host);
        let before = session.host().state().clone();
        let mut preview_request = request(&session, AbilitySlot::Special);
        preview_request.target_player_id = Some("b-target".to_owned());

        let first = session
            .preview(&preview_request)
            .expect("preview must resolve");
        let second = session
            .preview(&preview_request)
            .expect("preview must repeat");

        assert!(first.legal);
        assert_eq!(first, second);
        assert!(first.projectile_traces.is_empty());
        assert_eq!(first.gauge_cost, GAUGE_FULL);
        assert_session_unchanged(&session, &before, 0, 0, 0);
    }

    #[test]
    fn malformed_preview_structure_faults_without_mutation() {
        let session = MatchSessionHost::create(&config("zeke")).expect("fixture session");
        let before = session.host().state().clone();
        let mut malformed = request(&session, AbilitySlot::Basic);
        malformed.schema_version = CLIENT_CONTRACT_VERSION.saturating_add(1);

        assert_eq!(
            session.preview(&malformed),
            Err(SessionFault::UnsupportedSchema {
                expected: CLIENT_CONTRACT_VERSION,
                actual: CLIENT_CONTRACT_VERSION.saturating_add(1),
            })
        );
        assert_session_unchanged(&session, &before, 0, 0, 0);
    }
}

/// Status lifecycle reaching the client event stream.
///
/// The path these cover is the one a `CommandOutcome` cannot carry: a `Pass` produces no
/// outcome at all, yet still ends a turn and therefore still runs the status tick. Before
/// the transition records existed this was derived by diffing snapshots, which is why the
/// session sources them from the host instead.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod status_lifecycle_tests {
    use super::*;
    use crate::match_host::MatchHost;
    use crate::match_setup::{MatchMode, MatchPlayerConfig, build_initial_state};
    use crate::types::{Appearance, EffectKind, StatusEffect};

    fn duel() -> MatchConfig {
        MatchConfig {
            seed: 12_345,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                MatchPlayerConfig {
                    player_id: "a-local-player".to_owned(),
                    team: 0,
                    character_id: "zeke".to_owned(),
                    appearance: Appearance::default(),
                },
                MatchPlayerConfig {
                    player_id: "b-local-bot".to_owned(),
                    team: 1,
                    character_id: "huck".to_owned(),
                    appearance: Appearance::default(),
                },
            ],
        }
    }

    /// A session whose named player already carries `status`.
    ///
    /// Built through `build_initial_state` rather than `MatchSessionHost::create` because no
    /// launch-roster basic attack applies a status: the only two status effects in the game
    /// are Numa's Pin and Karl's Feeding Frenzy, both specials gated behind a full gauge.
    fn session_with_status(player_id: &str, status: StatusEffect) -> MatchSessionHost {
        let mut state = build_initial_state(&duel()).expect("fixture config must build");
        let target = state
            .player_mut(player_id)
            .expect("fixture player must exist");
        target.statuses.push(status);
        target.statuses.sort_by_key(|s| s.kind);
        let host = MatchHost::start(state).expect("fixture match must start");
        MatchSessionHost::from_new_host(host)
    }

    /// Local copy of the sibling test module's helper: that module is private to itself.
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

    fn status_events(
        transition: &MatchTransition,
    ) -> Vec<(String, ClientStatusKind, StatusTransition)> {
        transition
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                PresentationEventKind::StatusChanged {
                    player_id,
                    kind,
                    transition,
                } => Some((player_id.clone(), *kind, transition.clone())),
                _ => None,
            })
            .collect()
    }

    fn status_mut<'a>(
        snapshot: &'a mut MatchSnapshot,
        player_id: &str,
        kind: ClientStatusKind,
    ) -> &'a mut StatusSnapshot {
        snapshot
            .players
            .iter_mut()
            .find(|player| player.id == player_id)
            .and_then(|player| {
                player
                    .statuses
                    .iter_mut()
                    .find(|status| status.kind == kind)
            })
            .expect("fixture status must exist")
    }

    #[test]
    fn a_pass_surfaces_the_expiry_it_caused_despite_producing_no_outcome() {
        let mut session = session_with_status(
            "a-local-player",
            StatusEffect {
                kind: EffectKind::Lockdown,
                magnitude: 2,
                turns_remaining: 1,
            },
        );
        let command = command(&session, "pass-1", MatchCommandKind::Pass);
        let transition = session.apply(command).expect("pass must be accepted");

        let events = status_events(&transition);
        assert_eq!(
            events,
            vec![(
                "a-local-player".to_owned(),
                ClientStatusKind::Lockdown,
                StatusTransition::Expired,
            )],
            "the end-of-turn tick that removed the status must reach the client",
        );
    }

    #[test]
    fn a_surviving_status_reports_its_decrement_rather_than_nothing() {
        let mut session = session_with_status(
            "a-local-player",
            StatusEffect {
                kind: EffectKind::Lockdown,
                magnitude: 2,
                turns_remaining: 4,
            },
        );
        let command = command(&session, "pass-1", MatchCommandKind::Pass);
        let transition = session.apply(command).expect("pass must be accepted");

        assert_eq!(
            status_events(&transition),
            vec![(
                "a-local-player".to_owned(),
                ClientStatusKind::Lockdown,
                StatusTransition::Ticked { turns_remaining: 3 },
            )],
        );
    }

    #[test]
    fn a_turn_with_no_statuses_anywhere_emits_no_status_events() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let command = command(&session, "pass-1", MatchCommandKind::Pass);
        let transition = session.apply(command).expect("pass must be accepted");

        // Guards the reconciliation check from the opposite direction: it must not invent
        // events for statuses nobody has.
        assert!(status_events(&transition).is_empty());
    }

    #[test]
    fn each_command_reports_only_its_own_transitions() {
        let mut session = session_with_status(
            "a-local-player",
            StatusEffect {
                kind: EffectKind::Lockdown,
                magnitude: 2,
                turns_remaining: 1,
            },
        );
        let first = command(&session, "pass-1", MatchCommandKind::Pass);
        let first_transition = session.apply(first).expect("first pass must be accepted");
        assert_eq!(status_events(&first_transition).len(), 1);

        let second = command(&session, "pass-2", MatchCommandKind::Pass);
        let second_transition = session.apply(second).expect("second pass must be accepted");

        // The host clears its record at the start of every call. Without that, the expiry
        // above would be replayed here and the client would remove the status twice.
        assert!(
            status_events(&second_transition).is_empty(),
            "a later command must not repeat an earlier command's transitions",
        );
    }

    #[test]
    fn exact_status_replay_rejects_missing_duplicate_and_stale_ticks() {
        let session = session_with_status(
            "a-local-player",
            StatusEffect {
                kind: EffectKind::Lockdown,
                magnitude: 2,
                turns_remaining: 2,
            },
        );
        let pre = session.snapshot();
        let mut post = pre.clone();
        status_mut(&mut post, "a-local-player", ClientStatusKind::Lockdown).turns_remaining = 1;
        let tick = StatusChange {
            player_id: "a-local-player".to_owned(),
            kind: EffectKind::Lockdown,
            transition: StatusTransition::Ticked { turns_remaining: 1 },
        };

        assert_eq!(
            reconcile_status_changes(&pre, &post, core::slice::from_ref(&tick)),
            Ok(()),
        );
        assert_eq!(
            reconcile_status_changes(&pre, &post, &[]),
            Err(SessionFault::ContractInvariant),
        );
        assert_eq!(
            reconcile_status_changes(&pre, &post, &[tick.clone(), tick]),
            Err(SessionFault::ContractInvariant),
        );
        assert_eq!(
            reconcile_status_changes(
                &pre,
                &post,
                &[StatusChange {
                    player_id: "a-local-player".to_owned(),
                    kind: EffectKind::Lockdown,
                    transition: StatusTransition::Ticked { turns_remaining: 2 },
                }],
            ),
            Err(SessionFault::ContractInvariant),
        );
    }

    #[test]
    fn exact_status_replay_validates_refresh_and_charge_cardinality() {
        let session = session_with_status(
            "a-local-player",
            StatusEffect {
                kind: EffectKind::GuaranteeCrit,
                magnitude: 2,
                turns_remaining: u8::MAX,
            },
        );
        let pre = session.snapshot();
        let mut refreshed = pre.clone();
        let refreshed_status = status_mut(
            &mut refreshed,
            "a-local-player",
            ClientStatusKind::GuaranteeCrit,
        );
        refreshed_status.magnitude = 3;
        let correct_refresh = StatusChange {
            player_id: "a-local-player".to_owned(),
            kind: EffectKind::GuaranteeCrit,
            transition: StatusTransition::Refreshed {
                magnitude: 3,
                turns_remaining: u8::MAX,
                replaced_magnitude: 2,
                replaced_turns_remaining: u8::MAX,
            },
        };
        assert_eq!(
            reconcile_status_changes(&pre, &refreshed, core::slice::from_ref(&correct_refresh),),
            Ok(()),
        );
        let mut stale_refresh = correct_refresh;
        if let StatusTransition::Refreshed {
            replaced_magnitude, ..
        } = &mut stale_refresh.transition
        {
            *replaced_magnitude = 1;
        }
        assert_eq!(
            reconcile_status_changes(&pre, &refreshed, &[stale_refresh]),
            Err(SessionFault::ContractInvariant),
        );

        let mut exhausted = pre.clone();
        exhausted
            .players
            .iter_mut()
            .find(|player| player.id == "a-local-player")
            .expect("fixture player must exist")
            .statuses
            .clear();
        let exact = vec![
            StatusChange {
                player_id: "a-local-player".to_owned(),
                kind: EffectKind::GuaranteeCrit,
                transition: StatusTransition::ChargeConsumed { remaining: 1 },
            },
            StatusChange {
                player_id: "a-local-player".to_owned(),
                kind: EffectKind::GuaranteeCrit,
                transition: StatusTransition::Exhausted,
            },
        ];
        assert_eq!(reconcile_status_changes(&pre, &exhausted, &exact), Ok(()));
        assert_eq!(
            reconcile_status_changes(&pre, &exhausted, &exact[1..]),
            Err(SessionFault::ContractInvariant),
            "one missing charge transition must fail closed",
        );
    }

    #[test]
    fn exact_status_replay_accepts_an_invisible_lifecycle_and_rejects_unknown_players() {
        let session = MatchSessionHost::create(&duel()).expect("fixture session must start");
        let pre = session.snapshot();
        let changes = vec![
            StatusChange {
                player_id: "a-local-player".to_owned(),
                kind: EffectKind::Lockdown,
                transition: StatusTransition::Applied {
                    magnitude: 1,
                    turns_remaining: 1,
                },
            },
            StatusChange {
                player_id: "a-local-player".to_owned(),
                kind: EffectKind::Lockdown,
                transition: StatusTransition::Expired,
            },
        ];
        assert_eq!(reconcile_status_changes(&pre, &pre, &changes), Ok(()));

        let mut unknown = changes;
        unknown[0].player_id = "missing-player".to_owned();
        assert_eq!(
            reconcile_status_changes(&pre, &pre, &unknown),
            Err(SessionFault::ContractInvariant),
        );
    }
}

/// Persistent-object lifecycle reaching the client event stream.
///
/// Aleph's throwing knife is the only object producer reachable from a non-special ability,
/// and its chain detonation produces the case this contract exists for: a knife that spawns
/// and is removed inside one command appears in neither the pre- nor the post-snapshot, so
/// no diff can report it at all.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod object_lifecycle_tests {
    use super::*;
    use crate::fixed::FixedPoint;
    use crate::match_host::MatchHost;
    use crate::match_setup::{MatchMode, MatchPlayerConfig, build_initial_state};
    use crate::types::{Appearance, PersistentObject, PersistentObjectKind};

    const ALEPH: &str = "a-local-player";

    fn knife_duel() -> MatchConfig {
        MatchConfig {
            seed: 12_345,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                MatchPlayerConfig {
                    player_id: ALEPH.to_owned(),
                    team: 0,
                    character_id: "aleph".to_owned(),
                    appearance: Appearance::default(),
                },
                MatchPlayerConfig {
                    player_id: "b-local-bot".to_owned(),
                    team: 1,
                    character_id: "huck".to_owned(),
                    appearance: Appearance::default(),
                },
            ],
        }
    }

    fn command_for(session: &MatchSessionHost, id: &str, kind: MatchCommandKind) -> MatchCommand {
        MatchCommand {
            schema_version: CLIENT_CONTRACT_VERSION,
            command_id: id.to_owned(),
            player_id: session.host().active_player().to_owned(),
            expected_turn_number: session.host().state().turn_number,
            expected_snapshot_generation: session.generation(),
            kind,
        }
    }

    /// Empirically verified to land and embed a knife.
    fn throw_knife(session: &MatchSessionHost, id: &str) -> MatchCommand {
        command_for(
            session,
            id,
            MatchCommandKind::Ability {
                slot: AbilitySlot::BasicAlt,
                angle_millidegrees: 0,
                power_basis_points: 200,
                target_player_id: None,
                secondary_target_player_id: None,
            },
        )
    }

    fn spawns(transition: &MatchTransition) -> Vec<u32> {
        transition
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                PresentationEventKind::ObjectSpawned { object } => Some(object.sequence),
                _ => None,
            })
            .collect()
    }

    fn removals(transition: &MatchTransition) -> Vec<(u32, PersistentObjectRemovalCause)> {
        transition
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                PresentationEventKind::ObjectRemoved { previous, cause } => {
                    Some((previous.sequence, *cause))
                }
                _ => None,
            })
            .collect()
    }

    fn lifecycle_object(sequence: u32) -> PersistentObject {
        PersistentObject {
            sequence,
            owner_id: ALEPH.to_owned(),
            kind: PersistentObjectKind::EmbeddedKnife,
            position: FixedPoint::new(4_096, 2_048),
            health: 1,
            turns_remaining: u8::MAX,
        }
    }

    fn projected(object: &PersistentObject) -> PersistentObjectSnapshot {
        crate::client_contract::snapshot_object(object)
    }

    #[test]
    fn embedding_a_knife_reports_the_spawn() {
        let mut session = MatchSessionHost::create(&knife_duel()).expect("fixture session");
        let command = throw_knife(&session, "throw-1");
        let transition = session.apply(command).expect("throw must be accepted");

        assert_eq!(
            spawns(&transition),
            vec![0],
            "the embedded knife must be reported as a spawn",
        );
        assert!(removals(&transition).is_empty());
        assert_eq!(session.snapshot().persistent_objects.len(), 1);
    }

    #[test]
    fn a_knife_spawned_and_detonated_in_one_command_is_invisible_to_a_diff_but_fully_recorded() {
        let mut session = MatchSessionHost::create(&knife_duel()).expect("fixture session");
        let first = throw_knife(&session, "throw-1");
        session.apply(first).expect("first throw must be accepted");
        let pass = command_for(&session, "pass-1", MatchCommandKind::Pass);
        session.apply(pass).expect("pass must be accepted");

        let before: Vec<u32> = session
            .snapshot()
            .persistent_objects
            .iter()
            .map(|object| object.sequence)
            .collect();

        let second = throw_knife(&session, "throw-2");
        let transition = session
            .apply(second)
            .expect("second throw must be accepted");

        let after: Vec<u32> = session
            .snapshot()
            .persistent_objects
            .iter()
            .map(|object| object.sequence)
            .collect();

        // The fixture must genuinely reproduce the gap, or this test proves nothing: knife 1
        // is created and destroyed inside this single command, so it is absent from the
        // snapshot before it and the snapshot after it alike.
        assert_eq!(before, vec![0], "only the first knife exists going in");
        assert!(after.is_empty(), "both knives are gone coming out");
        assert!(
            !before.contains(&1) && !after.contains(&1),
            "knife 1 must appear in neither snapshot, or the gap is not being tested",
        );

        // A diff could only ever have reported knife 0 disappearing, with no cause and no
        // hint that knife 1 existed. The records describe all three transitions.
        assert_eq!(spawns(&transition), vec![1], "knife 1's spawn is reported");
        assert_eq!(
            removals(&transition),
            vec![
                (0, PersistentObjectRemovalCause::Detonated),
                (1, PersistentObjectRemovalCause::Detonated),
            ],
            "both detonations are reported, each naming why it happened",
        );
        let ordered_lifecycle: Vec<(&str, u32)> = transition
            .events
            .iter()
            .filter_map(|event| match &event.kind {
                PresentationEventKind::ObjectSpawned { object } => {
                    Some(("spawned", object.sequence))
                }
                PresentationEventKind::ObjectRemoved { previous, .. } => {
                    Some(("removed", previous.sequence))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            ordered_lifecycle,
            vec![("spawned", 1), ("removed", 0), ("removed", 1)],
            "session events must retain the producer's causal order",
        );
    }

    #[test]
    fn a_removal_names_a_real_cause_rather_than_a_placeholder() {
        let mut session = MatchSessionHost::create(&knife_duel()).expect("fixture session");
        session
            .apply(throw_knife(&session, "throw-1"))
            .expect("first throw must be accepted");
        let pass = command_for(&session, "pass-1", MatchCommandKind::Pass);
        session.apply(pass).expect("pass must be accepted");
        let transition = session
            .apply(throw_knife(&session, "throw-2"))
            .expect("second throw must be accepted");

        let causes = removals(&transition);
        assert!(
            !causes.is_empty(),
            "the fixture must actually remove objects"
        );
        for (sequence, cause) in causes {
            // Every removal carries the producer's own reason. Before these records existed
            // this field was a single constant that said only "something authoritative did
            // it", which is true of every removal and therefore tells a client nothing.
            assert_eq!(
                cause,
                PersistentObjectRemovalCause::Detonated,
                "knife {sequence} was removed by the chain detonation",
            );
            assert_eq!(cause.wire_name(), "detonated");
        }
    }

    #[test]
    fn exact_replay_accepts_a_transient_spawn_then_removal() {
        let object = lifecycle_object(7);
        let changes = vec![
            PersistentObjectChange {
                object: object.clone(),
                transition: PersistentObjectTransition::Spawned,
            },
            PersistentObjectChange {
                object,
                transition: PersistentObjectTransition::Removed {
                    cause: PersistentObjectRemovalCause::Detonated,
                },
            },
        ];

        assert_eq!(reconcile_object_changes(&[], &[], &changes), Ok(()));
    }

    #[test]
    fn exact_replay_rejects_missing_unknown_duplicate_and_stale_records() {
        let object = lifecycle_object(7);
        let pre = vec![projected(&object)];
        let removal = PersistentObjectChange {
            object: object.clone(),
            transition: PersistentObjectTransition::Removed {
                cause: PersistentObjectRemovalCause::Detonated,
            },
        };

        assert_eq!(
            reconcile_object_changes(&pre, &[], &[]),
            Err(SessionFault::ContractInvariant),
            "a real disappearance requires a producer record",
        );
        assert_eq!(
            reconcile_object_changes(&[], &[], core::slice::from_ref(&removal)),
            Err(SessionFault::ContractInvariant),
            "an unknown removal cannot be invented",
        );
        assert_eq!(
            reconcile_object_changes(&pre, &[], &[removal.clone(), removal.clone()]),
            Err(SessionFault::ContractInvariant),
            "the same object cannot be removed twice",
        );

        let mut stale = removal;
        stale.object.health = 0;
        assert_eq!(
            reconcile_object_changes(&pre, &[], &[stale]),
            Err(SessionFault::ContractInvariant),
            "the complete last object snapshot must match",
        );
    }

    #[test]
    fn exact_replay_rejects_unrecorded_or_reused_spawns() {
        let object = lifecycle_object(7);
        let post = vec![projected(&object)];
        let spawn = PersistentObjectChange {
            object: object.clone(),
            transition: PersistentObjectTransition::Spawned,
        };

        assert_eq!(
            reconcile_object_changes(&[], &post, &[]),
            Err(SessionFault::ContractInvariant),
            "an object cannot appear without its spawn record",
        );
        assert_eq!(
            reconcile_object_changes(&post, &post, core::slice::from_ref(&spawn),),
            Err(SessionFault::ContractInvariant),
            "an allocated sequence cannot be spawned again",
        );
        assert_eq!(
            reconcile_object_changes(&[], &post, &[spawn.clone(), spawn]),
            Err(SessionFault::ContractInvariant),
            "duplicate spawn records must fail closed",
        );
    }

    #[test]
    fn exact_replay_rejects_a_removal_when_the_object_survives() {
        let object = lifecycle_object(7);
        let snapshot = vec![projected(&object)];
        let removal = PersistentObjectChange {
            object,
            transition: PersistentObjectTransition::Removed {
                cause: PersistentObjectRemovalCause::Detonated,
            },
        };

        assert_eq!(
            reconcile_object_changes(&snapshot, &snapshot, &[removal]),
            Err(SessionFault::ContractInvariant),
        );
    }

    #[test]
    fn owner_cleanup_reaches_the_session_with_its_exact_cause() {
        let mut state = build_initial_state(&knife_duel()).expect("fixture state must build");
        let position = state
            .player(ALEPH)
            .map(|player| player.position)
            .expect("Aleph must exist");
        state.player_mut(ALEPH).expect("Aleph must exist").health = 0;
        let owned = PersistentObject {
            sequence: 0,
            owner_id: ALEPH.to_owned(),
            kind: PersistentObjectKind::EmbeddedKnife,
            position,
            health: 1,
            turns_remaining: u8::MAX,
        };
        state.objects.push(owned);
        state.next_object_sequence = 1;
        let host = MatchHost::start(state).expect("the surviving player must open the match");
        let mut session = MatchSessionHost::from_new_host(host);

        let pass = command_for(&session, "cleanup-pass", MatchCommandKind::Pass);
        let transition = session.apply(pass).expect("pass must reconcile cleanup");

        assert_eq!(
            removals(&transition),
            vec![(0, PersistentObjectRemovalCause::OwnerEliminated)],
        );
        assert!(session.snapshot().persistent_objects.is_empty());
    }
}

/// The authority-only timeout path.
///
/// The property these exist to protect is that a client can never end a turn by claiming time
/// ran out — its own turn or anyone else's. That is enforced structurally: timeout is not a
/// [`MatchCommandKind`] variant, so no decoded client command can select it. These tests cover
/// what a structural guarantee cannot: that the authority path itself is bounded, idempotent,
/// and refuses the races a real clock will produce.
#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]
mod authority_timeout_tests {
    use super::*;
    use crate::match_setup::{MatchMode, MatchPlayerConfig};
    use crate::types::{Appearance, TurnEndReason};

    fn duel() -> MatchConfig {
        MatchConfig {
            seed: 12_345,
            map_id: "horizontal-test-array".to_owned(),
            mode: MatchMode::TurnBased,
            players: vec![
                MatchPlayerConfig {
                    player_id: "a-local-player".to_owned(),
                    team: 0,
                    character_id: "zeke".to_owned(),
                    appearance: Appearance::default(),
                },
                MatchPlayerConfig {
                    player_id: "b-local-bot".to_owned(),
                    team: 1,
                    character_id: "huck".to_owned(),
                    appearance: Appearance::default(),
                },
            ],
        }
    }

    fn timeout_for(session: &MatchSessionHost, action_id: &str) -> AuthorityTimeout {
        AuthorityTimeout {
            schema_version: CLIENT_CONTRACT_VERSION,
            action_id: action_id.to_owned(),
            player_id: session.host().active_player().to_owned(),
            expected_turn_number: session.host().state().turn_number,
            expected_snapshot_generation: session.generation(),
        }
    }

    fn pass_for(session: &MatchSessionHost, command_id: &str) -> MatchCommand {
        MatchCommand {
            schema_version: CLIENT_CONTRACT_VERSION,
            command_id: command_id.to_owned(),
            player_id: session.host().active_player().to_owned(),
            expected_turn_number: session.host().state().turn_number,
            expected_snapshot_generation: session.generation(),
            kind: MatchCommandKind::Pass,
        }
    }

    #[test]
    fn a_timeout_ends_the_active_turn_and_records_why() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        let first = session.host().active_player().to_owned();
        let timeout = timeout_for(&session, "deadline-1");

        let transition = session
            .apply_authority_timeout(timeout)
            .expect("a well-formed timeout must be accepted");

        assert_eq!(transition.disposition, TransitionDisposition::Accepted);
        assert_eq!(transition.post_snapshot_generation, 1);
        assert_ne!(
            session.host().active_player(),
            first,
            "the turn must actually hand over",
        );
        // A timeout must stay distinguishable from a pass downstream: the result panel and
        // the turn-timeout metric both read this, and conflating them was `todolist.md` P11.
        assert_eq!(
            session.host().state().last_turn_end_reason,
            TurnEndReason::TimedOut,
        );
    }

    #[test]
    fn a_retried_timeout_replays_instead_of_ending_a_second_turn() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        let timeout = timeout_for(&session, "deadline-1");
        let first = session
            .apply_authority_timeout(timeout.clone())
            .expect("first delivery must be accepted");
        let turn_after_first = session.host().state().turn_number;

        let replay = session
            .apply_authority_timeout(timeout)
            .expect("a retry must be answered, not faulted");

        assert_eq!(replay.disposition, TransitionDisposition::DuplicateReplay);
        assert_eq!(
            replay.post_snapshot_generation,
            first.post_snapshot_generation
        );
        assert_eq!(
            session.host().state().turn_number,
            turn_after_first,
            "a redelivered timeout must not burn a second turn",
        );
    }

    #[test]
    fn a_timeout_naming_a_player_who_is_no_longer_active_is_refused() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        let stale = AuthorityTimeout {
            player_id: "b-local-bot".to_owned(),
            ..timeout_for(&session, "deadline-1")
        };
        assert_ne!(
            session.host().active_player(),
            "b-local-bot",
            "fixture must name the player who is not on the clock",
        );
        let before = session.host().state().turn_number;

        let transition = session
            .apply_authority_timeout(stale)
            .expect("a losing race is a refusal, not an error");

        // The real failure this prevents: a deadline that expired for one player arriving
        // after the turn already handed over, and ending the innocent player's turn instead.
        assert_eq!(transition.disposition, TransitionDisposition::Rejected);
        assert_eq!(
            transition.rejection_reason,
            Some(TransitionRejection::Core(CommandRejection::NotActivePlayer)),
        );
        assert_eq!(session.host().state().turn_number, before);
    }

    #[test]
    fn a_timeout_against_a_stale_generation_is_refused_without_mutating() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        let stale = AuthorityTimeout {
            expected_snapshot_generation: session.generation().saturating_add(7),
            ..timeout_for(&session, "deadline-1")
        };
        let before_hash = session.snapshot().authoritative_state_hash.clone();

        let transition = session
            .apply_authority_timeout(stale)
            .expect("a stale generation is a refusal");

        assert_eq!(transition.disposition, TransitionDisposition::Rejected);
        assert_eq!(
            session.generation(),
            0,
            "a refusal must not bump generation"
        );
        assert_eq!(
            session.snapshot().authoritative_state_hash,
            before_hash,
            "a refused timeout must leave authoritative state untouched",
        );
    }

    #[test]
    fn a_timeout_for_the_wrong_turn_number_is_refused() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        let stale = AuthorityTimeout {
            expected_turn_number: session.host().state().turn_number.saturating_add(3),
            ..timeout_for(&session, "deadline-1")
        };

        let transition = session
            .apply_authority_timeout(stale)
            .expect("a stale turn number is a refusal");

        assert_eq!(
            transition.rejection_reason,
            Some(TransitionRejection::Core(
                CommandRejection::TurnVersionMismatch
            )),
        );
    }

    #[test]
    fn a_client_cannot_reuse_an_authority_action_id_to_obtain_its_slot() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        session
            .apply_authority_timeout(timeout_for(&session, "shared-id"))
            .expect("timeout must be accepted");

        // The client now sends a command under the identifier the authority already used.
        // Answering it with the timeout's recorded transition would let a client learn — and
        // replay — an authority result it never authored, so this must conflict.
        let command = pass_for(&session, "shared-id");
        let transition = session.apply(command).expect("the collision is answerable");

        assert_eq!(transition.disposition, TransitionDisposition::Rejected);
        assert_eq!(
            transition.rejection_reason,
            Some(TransitionRejection::CommandIdConflict),
        );
        assert!(
            transition
                .rejection_reason
                .is_some_and(|r| r.is_security_event()),
            "an id collision across the authority boundary is security telemetry",
        );
    }

    #[test]
    fn an_authority_action_cannot_reuse_a_client_command_id_either() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        session
            .apply(pass_for(&session, "shared-id"))
            .expect("pass must be accepted");

        let transition = session
            .apply_authority_timeout(timeout_for(&session, "shared-id"))
            .expect("the collision is answerable");

        // Symmetric to the test above. One identifier space means one owner per identifier,
        // whichever side claimed it first.
        assert_eq!(transition.disposition, TransitionDisposition::Rejected);
        assert_eq!(
            transition.rejection_reason,
            Some(TransitionRejection::CommandIdConflict),
        );
    }

    #[test]
    fn two_different_timeouts_sharing_an_id_conflict_rather_than_replay() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        session
            .apply_authority_timeout(timeout_for(&session, "deadline-1"))
            .expect("first timeout must be accepted");

        // Same id, different content. Replaying the first would let a later, differently
        // scoped deadline silently inherit an earlier one's answer.
        let different = AuthorityTimeout {
            expected_turn_number: 99,
            ..timeout_for(&session, "deadline-1")
        };
        let transition = session
            .apply_authority_timeout(different)
            .expect("the collision is answerable");

        assert_eq!(
            transition.rejection_reason,
            Some(TransitionRejection::CommandIdConflict),
        );
    }

    #[test]
    fn a_malformed_authority_action_is_a_fault_rather_than_a_refusal() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");

        let bad_schema = AuthorityTimeout {
            schema_version: CLIENT_CONTRACT_VERSION.saturating_add(1),
            ..timeout_for(&session, "deadline-1")
        };
        assert_eq!(
            session.apply_authority_timeout(bad_schema),
            Err(SessionFault::UnsupportedSchema {
                expected: CLIENT_CONTRACT_VERSION,
                actual: CLIENT_CONTRACT_VERSION.saturating_add(1),
            }),
        );

        let bad_id = AuthorityTimeout {
            action_id: "not a valid id".to_owned(),
            ..timeout_for(&session, "deadline-1")
        };
        assert_eq!(
            session.apply_authority_timeout(bad_id),
            Err(SessionFault::InvalidCommand { field: "action_id" }),
        );

        // A malformed action is the authority's own bug, not a gameplay outcome, so it must
        // never be recorded as a refusal a client could observe and replay.
        assert_eq!(session.ledger_len(), 0);
    }

    #[test]
    fn the_authority_action_encoding_is_frozen() {
        let timeout = AuthorityTimeout {
            schema_version: CLIENT_CONTRACT_VERSION,
            action_id: "deadline-1".to_owned(),
            player_id: "a-local-player".to_owned(),
            expected_turn_number: 1,
            expected_snapshot_generation: 0,
        };

        // Frozen 2026-08-25. The ledger compares digests to decide whether a redelivered
        // action is the same action, so the encoding is a compatibility surface: silently
        // changing it would make previously recorded entries unrecognizable and turn replays
        // into conflicts. Changing this value is a deliberate act that needs the same
        // treatment as a golden-vector regeneration — a documented reason and a version bump.
        //
        // This also pins the `0x21` domain separator, which
        // `a_timeout_and_a_command_with_identical_fields_never_share_a_digest` does not: that
        // test passes even with a colliding tag, because a `MatchCommand` additionally
        // encodes its `kind`. Only a frozen value catches a change to the tag itself.
        assert_eq!(timeout.canonical_digest(), "8cac07183828cf43");
    }

    #[test]
    fn a_timeout_and_a_command_with_identical_fields_never_share_a_digest() {
        let session = MatchSessionHost::create(&duel()).expect("fixture session");
        let timeout = timeout_for(&session, "same-id");
        let command = pass_for(&session, "same-id");

        // Both carry the same schema, id, player, turn, and generation, and must still
        // digest differently. Note this holds even if the two domain tags collided, because a
        // `MatchCommand` also encodes its `kind` — so this asserts the property, while
        // `the_authority_action_encoding_is_frozen` is what actually pins the tag.
        assert_eq!(timeout.action_id, command.command_id);
        assert_eq!(timeout.player_id, command.player_id);
        assert_ne!(
            timeout.canonical_digest(),
            command.canonical_digest(),
            "an authority action must never digest identically to a client command",
        );
    }

    #[test]
    fn a_closed_session_refuses_the_authority_path_too() {
        let mut session = MatchSessionHost::create(&duel()).expect("fixture session");
        // Set directly: the session closes itself on a fault, and no public opener exists.
        session.closed = true;
        assert_eq!(
            session.apply_authority_timeout(timeout_for(&session, "deadline-1")),
            Err(SessionFault::Closed),
        );
    }
}
