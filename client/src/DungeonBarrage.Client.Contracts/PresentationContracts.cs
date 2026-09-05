using System.Text.Json.Serialization;

namespace DungeonBarrage.Client.Contracts;

/// <summary>Closed projectile-impact causes.</summary>
public enum ClientImpactCause
{
    /// <summary>Terrain stopped the projectile.</summary>
    Terrain,

    /// <summary>A character stopped the projectile.</summary>
    Character,

    /// <summary>The projectile left the authoritative bounds.</summary>
    OutOfBounds,

    /// <summary>The projectile exhausted its lifetime.</summary>
    Expired,
}

/// <summary>Closed critical-strike provenance.</summary>
public enum ClientCritRoll
{
    /// <summary>The strike was not eligible to crit.</summary>
    NotEligible,

    /// <summary>The crit roll missed.</summary>
    Missed,

    /// <summary>The crit roll landed.</summary>
    Landed,

    /// <summary>The crit was forced without a random draw.</summary>
    Forced,
}

/// <summary>Closed authoritative effect vocabulary.</summary>
public enum ClientEffectKind
{
    /// <summary>Knockback.</summary>
    Knockback,

    /// <summary>Chill.</summary>
    Chill,

    /// <summary>Cluster.</summary>
    Cluster,

    /// <summary>Embers.</summary>
    Embers,

    /// <summary>Tunnel.</summary>
    Tunnel,

    /// <summary>Return.</summary>
    Return,

    /// <summary>Recoil.</summary>
    Recoil,

    /// <summary>Self-inflicted damage.</summary>
    SelfDamage,

    /// <summary>Teleport.</summary>
    Teleport,

    /// <summary>Pull.</summary>
    Pull,

    /// <summary>Push.</summary>
    Push,

    /// <summary>Wall impact.</summary>
    WallImpact,

    /// <summary>Lockdown.</summary>
    Lockdown,

    /// <summary>Turret creation.</summary>
    SpawnTurret,

    /// <summary>Healing.</summary>
    Heal,

    /// <summary>Health transfer.</summary>
    HealthTransfer,

    /// <summary>Multiple strikes.</summary>
    MultiStrike,

    /// <summary>Guaranteed critical strike.</summary>
    GuaranteeCrit,

    /// <summary>Embedded projectile.</summary>
    EmbedProjectile,

    /// <summary>Chained detonation.</summary>
    ChainDetonate,

    /// <summary>Relocation.</summary>
    Relocate,

    /// <summary>Obscuring effect.</summary>
    Obscure,
}

/// <summary>Closed authoritative movement causes.</summary>
public enum ClientEntityMovementCause
{
    /// <summary>The accepted move request directly caused the motion.</summary>
    RequestedMove,

    /// <summary>Authoritative resolution caused or may have contributed to the motion.</summary>
    AuthoritativeResolution,
}

/// <summary>Closed persistent-object removal causes.</summary>
public enum ClientPersistentObjectRemovalCause
{
    /// <summary>A newer object replaced it.</summary>
    Replaced,

    /// <summary>The capacity policy evicted it.</summary>
    CapacityEvicted,

    /// <summary>It detonated.</summary>
    Detonated,

    /// <summary>Its authoritative lifetime expired.</summary>
    Expired,

    /// <summary>Authoritative damage destroyed it.</summary>
    Destroyed,

    /// <summary>Its owner was eliminated.</summary>
    OwnerEliminated,
}

/// <summary>Closed reasons for ending a turn.</summary>
public enum ClientTurnEndReason
{
    /// <summary>The player attacked.</summary>
    Attacked,

    /// <summary>The player passed.</summary>
    Passed,

    /// <summary>The authority timed the player out.</summary>
    TimedOut,

    /// <summary>The active player was eliminated.</summary>
    Eliminated,
}

/// <summary>Closed authoritative command-rejection names.</summary>
public enum ClientCommandRejectionReason
{
    /// <summary>The command was already processed.</summary>
    DuplicateCommand,

    /// <summary>The submitting player is eliminated.</summary>
    PlayerEliminated,

    /// <summary>The submitting player is not active.</summary>
    NotActivePlayer,

    /// <summary>The match is in the wrong phase.</summary>
    WrongPhase,

    /// <summary>The submitted turn version is stale.</summary>
    TurnVersionMismatch,

    /// <summary>The character identifier is unknown.</summary>
    UnknownCharacter,

    /// <summary>The requested ability is unavailable.</summary>
    AbilityNotAvailable,

    /// <summary>The special gauge is not ready.</summary>
    GaugeNotReady,

    /// <summary>The player already attacked this turn.</summary>
    AlreadyAttacked,

    /// <summary>An input lies outside the accepted range.</summary>
    InputOutOfRange,

    /// <summary>The requested target is invalid.</summary>
    InvalidTarget,

    /// <summary>The requested passive is invalid.</summary>
    InvalidPassive,

    /// <summary>The passive choice was already made.</summary>
    PassiveAlreadyChosen,
}

/// <summary>One sampled point on an authoritative projectile trace.</summary>
/// <param name="Tick">Presentation tick.</param>
/// <param name="Position">Sampled position.</param>
public sealed record ClientProjectileSample(uint Tick, ClientPosition Position);

/// <summary>An authoritative projectile impact.</summary>
/// <param name="Position">Impact position.</param>
/// <param name="Tick">Presentation tick.</param>
/// <param name="Cause">Impact cause.</param>
public sealed record ClientImpact(
    ClientPosition Position,
    uint Tick,
    ClientImpactCause Cause);

/// <summary>A complete authoritative projectile trace.</summary>
/// <param name="TraceId">Transition-local trace identifier.</param>
/// <param name="OwnerId">Owning player identifier.</param>
/// <param name="AbilityId">Ability identifier.</param>
/// <param name="Samples">Ordered sampled positions.</param>
/// <param name="TerminalImpact">Terminal impact.</param>
public sealed record ClientProjectileTrace(
    uint TraceId,
    string OwnerId,
    string AbilityId,
    IReadOnlyList<ClientProjectileSample> Samples,
    ClientImpact TerminalImpact);

/// <summary>Closed strike-delivery provenance.</summary>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(typeof(ClientProjectileStrikeDelivery), "projectile")]
[JsonDerivedType(typeof(ClientMeleeStrikeDelivery), "melee")]
[JsonDerivedType(typeof(ClientEffectStrikeDelivery), "effect")]
public abstract record ClientStrikeDelivery;

/// <summary>A projectile-delivered strike.</summary>
/// <param name="TraceSequence">Cited projectile trace sequence.</param>
public sealed record ClientProjectileStrikeDelivery(uint TraceSequence) : ClientStrikeDelivery;

/// <summary>A melee-delivered strike.</summary>
public sealed record ClientMeleeStrikeDelivery : ClientStrikeDelivery;

/// <summary>An effect-delivered strike.</summary>
/// <param name="EffectKind">Authoritative effect kind.</param>
public sealed record ClientEffectStrikeDelivery(ClientEffectKind EffectKind) : ClientStrikeDelivery;

/// <summary>One producer-owned strike resolution.</summary>
/// <param name="StrikeIndex">Dense strike index.</param>
/// <param name="TargetPlayerId">Target player identifier.</param>
/// <param name="ImpactPoint">Exact strike point.</param>
/// <param name="Delivery">Strike delivery provenance.</param>
/// <param name="Crit">Critical-strike provenance.</param>
/// <param name="DamageApplied">Applied damage.</param>
/// <param name="EliminatedTarget">Whether this strike eliminated the target.</param>
public sealed record ClientStrikeResolution(
    ushort StrikeIndex,
    string TargetPlayerId,
    ClientPosition ImpactPoint,
    ClientStrikeDelivery Delivery,
    ClientCritRoll Crit,
    ushort DamageApplied,
    bool EliminatedTarget);

/// <summary>An authoritative terrain cell rectangle.</summary>
/// <param name="X">Left cell coordinate.</param>
/// <param name="Y">Top cell coordinate.</param>
/// <param name="Width">Width in cells.</param>
/// <param name="Height">Height in cells.</param>
public sealed record ClientCellRectangle(int X, int Y, uint Width, uint Height);

/// <summary>An itemized authoritative health change.</summary>
/// <param name="Direct">Direct damage.</param>
/// <param name="Splash">Splash damage.</param>
/// <param name="Backlash">Backlash damage.</param>
/// <param name="Hazard">Hazard damage.</param>
/// <param name="WallImpact">Wall-impact damage.</param>
/// <param name="Healed">Health restored.</param>
/// <param name="WasCritical">Whether damage included a critical strike.</param>
/// <param name="Knockback">Authoritative knockback vector.</param>
/// <param name="Eliminated">Whether the aggregate change eliminated the player.</param>
public sealed record ClientDamageBreakdown(
    ushort Direct,
    ushort Splash,
    ushort Backlash,
    ushort Hazard,
    ushort WallImpact,
    ushort Healed,
    bool WasCritical,
    ClientPosition Knockback,
    bool Eliminated);

/// <summary>Closed public random-outcome provenance.</summary>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "purpose",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(
    typeof(ClientArzumChainStrikeTeleportTargetOutcome),
    "arzumChainStrikeTeleportTarget")]
[JsonDerivedType(
    typeof(ClientAlephVeilstepTeleportPointOutcome),
    "alephVeilstepTeleportPoint")]
public abstract record ClientRandomOutcome;

/// <summary>Arzum's producer-owned teleport-target selection.</summary>
/// <param name="CandidateCount">Number of candidates at the draw site.</param>
/// <param name="SelectedIndex">Selected candidate index.</param>
/// <param name="TargetPlayerId">Selected target.</param>
/// <param name="Destination">Resolved destination.</param>
public sealed record ClientArzumChainStrikeTeleportTargetOutcome(
    uint CandidateCount,
    uint SelectedIndex,
    string TargetPlayerId,
    ClientPosition Destination) : ClientRandomOutcome;

/// <summary>Aleph's producer-owned bounded teleport-point draw.</summary>
/// <param name="AxisBound">Bound used for each axis draw.</param>
/// <param name="XResult">Accepted bounded X result.</param>
/// <param name="YResult">Accepted bounded Y result.</param>
/// <param name="FallbackUsed">Whether placement correction used the fallback.</param>
/// <param name="DrawnPoint">Point produced by the draw.</param>
/// <param name="Destination">Legal corrected destination.</param>
public sealed record ClientAlephVeilstepTeleportPointOutcome(
    uint AxisBound,
    uint XResult,
    uint YResult,
    bool FallbackUsed,
    ClientPosition DrawnPoint,
    ClientPosition Destination) : ClientRandomOutcome;

/// <summary>Closed status-lifecycle transition.</summary>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(typeof(ClientStatusApplied), "applied")]
[JsonDerivedType(typeof(ClientStatusRefreshed), "refreshed")]
[JsonDerivedType(typeof(ClientStatusChargeConsumed), "chargeConsumed")]
[JsonDerivedType(typeof(ClientStatusTicked), "ticked")]
[JsonDerivedType(typeof(ClientStatusExhausted), "exhausted")]
[JsonDerivedType(typeof(ClientStatusExpired), "expired")]
public abstract record ClientStatusTransition;

/// <summary>A status was applied.</summary>
/// <param name="Magnitude">Applied magnitude.</param>
/// <param name="TurnsRemaining">Initial remaining turns.</param>
public sealed record ClientStatusApplied(int Magnitude, byte TurnsRemaining) : ClientStatusTransition;

/// <summary>An existing status was refreshed.</summary>
/// <param name="Magnitude">New magnitude.</param>
/// <param name="TurnsRemaining">New remaining turns.</param>
/// <param name="ReplacedMagnitude">Replaced magnitude.</param>
/// <param name="ReplacedTurnsRemaining">Replaced remaining turns.</param>
public sealed record ClientStatusRefreshed(
    int Magnitude,
    byte TurnsRemaining,
    int ReplacedMagnitude,
    byte ReplacedTurnsRemaining) : ClientStatusTransition;

/// <summary>A count-based status charge was consumed.</summary>
/// <param name="Remaining">Remaining charges.</param>
public sealed record ClientStatusChargeConsumed(int Remaining) : ClientStatusTransition;

/// <summary>A duration-based status ticked.</summary>
/// <param name="TurnsRemaining">Remaining affected-player turns.</param>
public sealed record ClientStatusTicked(byte TurnsRemaining) : ClientStatusTransition;

/// <summary>A count-based status was exhausted.</summary>
public sealed record ClientStatusExhausted : ClientStatusTransition;

/// <summary>A duration-based status expired.</summary>
public sealed record ClientStatusExpired : ClientStatusTransition;

/// <summary>Closed authoritative elimination provenance.</summary>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(typeof(ClientStrikeEliminationCause), "strike")]
[JsonDerivedType(typeof(ClientBacklashEliminationCause), "backlash")]
[JsonDerivedType(typeof(ClientSplashEliminationCause), "splash")]
[JsonDerivedType(typeof(ClientWallImpactEliminationCause), "wallImpact")]
[JsonDerivedType(typeof(ClientAbilityEffectEliminationCause), "abilityEffect")]
[JsonDerivedType(typeof(ClientHazardEliminationCause), "hazard")]
[JsonDerivedType(typeof(ClientAuthoritativeResolutionEliminationCause), "authoritativeResolution")]
public abstract record ClientEliminationCause;

/// <summary>A specific strike eliminated the player.</summary>
/// <param name="OwnerId">Attacking player.</param>
/// <param name="AbilityId">Attacking ability.</param>
/// <param name="StrikeIndex">Exact strike index.</param>
public sealed record ClientStrikeEliminationCause(
    string OwnerId,
    string AbilityId,
    ushort StrikeIndex) : ClientEliminationCause;

/// <summary>Backlash eliminated the player.</summary>
/// <param name="OwnerId">Ability owner.</param>
/// <param name="AbilityId">Ability identifier.</param>
public sealed record ClientBacklashEliminationCause(
    string OwnerId,
    string AbilityId) : ClientEliminationCause;

/// <summary>Splash damage eliminated the player.</summary>
/// <param name="OwnerId">Ability owner.</param>
/// <param name="AbilityId">Ability identifier.</param>
public sealed record ClientSplashEliminationCause(
    string OwnerId,
    string AbilityId) : ClientEliminationCause;

/// <summary>Wall-impact damage eliminated the player.</summary>
/// <param name="OwnerId">Ability owner.</param>
/// <param name="AbilityId">Ability identifier.</param>
public sealed record ClientWallImpactEliminationCause(
    string OwnerId,
    string AbilityId) : ClientEliminationCause;

/// <summary>An ability effect eliminated the player.</summary>
/// <param name="OwnerId">Ability owner.</param>
/// <param name="AbilityId">Ability identifier.</param>
public sealed record ClientAbilityEffectEliminationCause(
    string OwnerId,
    string AbilityId) : ClientEliminationCause;

/// <summary>A hazard eliminated the player.</summary>
public sealed record ClientHazardEliminationCause : ClientEliminationCause;

/// <summary>Conservative authoritative resolution eliminated the player.</summary>
public sealed record ClientAuthoritativeResolutionEliminationCause : ClientEliminationCause;

/// <summary>One ordered authoritative presentation event.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Unique transition-local sequence.</param>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(typeof(ClientProjectileTraceEvent), "projectileTrace")]
[JsonDerivedType(typeof(ClientImpactEvent), "impact")]
[JsonDerivedType(typeof(ClientStrikeResolvedEvent), "strikeResolved")]
[JsonDerivedType(typeof(ClientTerrainChangedEvent), "terrainChanged")]
[JsonDerivedType(typeof(ClientBlockChangedEvent), "blockChanged")]
[JsonDerivedType(typeof(ClientHealthChangedEvent), "healthChanged")]
[JsonDerivedType(typeof(ClientGaugeChangedEvent), "gaugeChanged")]
[JsonDerivedType(typeof(ClientRandomOutcomeEvent), "randomOutcome")]
[JsonDerivedType(typeof(ClientStatusChangedEvent), "statusChanged")]
[JsonDerivedType(typeof(ClientEntityMovedEvent), "entityMoved")]
[JsonDerivedType(typeof(ClientObjectSpawnedEvent), "objectSpawned")]
[JsonDerivedType(typeof(ClientObjectChangedEvent), "objectChanged")]
[JsonDerivedType(typeof(ClientObjectRemovedEvent), "objectRemoved")]
[JsonDerivedType(typeof(ClientPlayerEliminatedEvent), "playerEliminated")]
[JsonDerivedType(typeof(ClientPassiveChoiceRequiredEvent), "passiveChoiceRequired")]
[JsonDerivedType(typeof(ClientPassiveChosenEvent), "passiveChosen")]
[JsonDerivedType(typeof(ClientTurnEndedEvent), "turnEnded")]
[JsonDerivedType(typeof(ClientTurnOpenedEvent), "turnOpened")]
[JsonDerivedType(typeof(ClientMatchCompletedEvent), "matchCompleted")]
public abstract record ClientPresentationEvent(uint PresentationTick, uint Sequence);

/// <summary>Publishes one complete projectile trace.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="Trace">Projectile trace.</param>
public sealed record ClientProjectileTraceEvent(
    uint PresentationTick,
    uint Sequence,
    ClientProjectileTrace Trace) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes a projectile impact.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="TraceId">Cited trace.</param>
/// <param name="Impact">Impact data.</param>
public sealed record ClientImpactEvent(
    uint PresentationTick,
    uint Sequence,
    uint TraceId,
    ClientImpact Impact) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes one producer-owned strike result.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="OwnerId">Ability owner.</param>
/// <param name="AbilityId">Ability identifier.</param>
/// <param name="Strike">Strike resolution.</param>
public sealed record ClientStrikeResolvedEvent(
    uint PresentationTick,
    uint Sequence,
    string OwnerId,
    string AbilityId,
    ClientStrikeResolution Strike) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes authoritative dirty terrain rectangles.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="TerrainGeneration">New terrain generation.</param>
/// <param name="DirtyRectangles">Dirty cell rectangles.</param>
public sealed record ClientTerrainChangedEvent(
    uint PresentationTick,
    uint Sequence,
    uint TerrainGeneration,
    IReadOnlyList<ClientCellRectangle> DirtyRectangles) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes authoritative block mutation.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="BlockId">Block identifier.</param>
/// <param name="PreviousHealth">Previous health, when present.</param>
/// <param name="NewHealth">New health, when present.</param>
/// <param name="PreviousSurvivingBounds">Previous surviving bounds, when present.</param>
/// <param name="NewSurvivingBounds">New surviving bounds, when present.</param>
public sealed record ClientBlockChangedEvent(
    uint PresentationTick,
    uint Sequence,
    uint BlockId,
    ushort? PreviousHealth,
    ushort? NewHealth,
    ClientCellRectangle? PreviousSurvivingBounds,
    ClientCellRectangle? NewSurvivingBounds) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes authoritative player health mutation.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">Affected player.</param>
/// <param name="PreviousHealth">Previous health.</param>
/// <param name="NewHealth">New health.</param>
/// <param name="Breakdown">Itemized change, when available.</param>
public sealed record ClientHealthChangedEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    ushort PreviousHealth,
    ushort NewHealth,
    ClientDamageBreakdown? Breakdown) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes authoritative special-gauge mutation.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">Affected player.</param>
/// <param name="PreviousGauge">Previous gauge.</param>
/// <param name="NewGauge">New gauge.</param>
/// <param name="Delta">Actual signed gauge delta.</param>
public sealed record ClientGaugeChangedEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    ushort PreviousGauge,
    ushort NewGauge,
    int Delta) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes a bounded public random outcome.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="OwnerId">Ability owner.</param>
/// <param name="AbilityId">Ability identifier.</param>
/// <param name="Outcome">Producer-owned outcome.</param>
public sealed record ClientRandomOutcomeEvent(
    uint PresentationTick,
    uint Sequence,
    string OwnerId,
    string AbilityId,
    ClientRandomOutcome Outcome) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes one status-lifecycle transition.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">Affected player.</param>
/// <param name="StatusKind">Status kind.</param>
/// <param name="Transition">Exact lifecycle transition.</param>
public sealed record ClientStatusChangedEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    ClientStatusKind StatusKind,
    ClientStatusTransition Transition) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes authoritative entity motion.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">Moved player.</param>
/// <param name="Start">Start position.</param>
/// <param name="End">End position.</param>
/// <param name="Cause">Movement provenance.</param>
public sealed record ClientEntityMovedEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    ClientPosition Start,
    ClientPosition End,
    ClientEntityMovementCause Cause) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes persistent-object creation.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="SpawnedObject">Spawned object.</param>
public sealed record ClientObjectSpawnedEvent(
    uint PresentationTick,
    uint Sequence,
    [property: JsonPropertyName("object")] ClientPersistentObjectSnapshot SpawnedObject)
    : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes in-place persistent-object mutation.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="Previous">Previous object snapshot.</param>
/// <param name="Current">Current object snapshot.</param>
public sealed record ClientObjectChangedEvent(
    uint PresentationTick,
    uint Sequence,
    ClientPersistentObjectSnapshot Previous,
    ClientPersistentObjectSnapshot Current) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes persistent-object removal.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="Previous">Last object snapshot.</param>
/// <param name="Cause">Exact producer-owned removal cause.</param>
public sealed record ClientObjectRemovedEvent(
    uint PresentationTick,
    uint Sequence,
    ClientPersistentObjectSnapshot Previous,
    ClientPersistentObjectRemovalCause Cause) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes player elimination.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">Eliminated player.</param>
/// <param name="Cause">Elimination provenance.</param>
public sealed record ClientPlayerEliminatedEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    ClientEliminationCause Cause) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes an owed passive choice.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">Player who must choose.</param>
/// <param name="PassiveIds">Allowed passive identifiers.</param>
public sealed record ClientPassiveChoiceRequiredEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    IReadOnlyList<string> PassiveIds) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes an accepted passive choice.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">Choosing player.</param>
/// <param name="PassiveId">Accepted passive.</param>
public sealed record ClientPassiveChosenEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    string PassiveId) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes the authoritative end of a turn.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">Player whose turn ended.</param>
/// <param name="Reason">Authoritative end reason.</param>
public sealed record ClientTurnEndedEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    ClientTurnEndReason Reason) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes the authoritative opening of a turn.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="PlayerId">New active player.</param>
/// <param name="TurnNumber">New turn number.</param>
/// <param name="InputOpensAt">Input-open timestamp, when decorated.</param>
/// <param name="DeadlineAt">Planning deadline, when decorated.</param>
public sealed record ClientTurnOpenedEvent(
    uint PresentationTick,
    uint Sequence,
    string PlayerId,
    uint TurnNumber,
    ulong? InputOpensAt,
    ulong? DeadlineAt) : ClientPresentationEvent(PresentationTick, Sequence);

/// <summary>Publishes terminal match completion.</summary>
/// <param name="PresentationTick">Presentation tick.</param>
/// <param name="Sequence">Event sequence.</param>
/// <param name="Outcome">Terminal outcome.</param>
public sealed record ClientMatchCompletedEvent(
    uint PresentationTick,
    uint Sequence,
    ClientMatchOutcome Outcome) : ClientPresentationEvent(PresentationTick, Sequence);
