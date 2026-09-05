using System.Text.Json.Serialization;

namespace DungeonBarrage.Client.Contracts;

/// <summary>Closed transition-rejection details.</summary>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(
    typeof(ClientSnapshotGenerationMismatchTransitionRejection),
    "snapshotGenerationMismatch")]
[JsonDerivedType(typeof(ClientCommandIdConflictTransitionRejection), "commandIdConflict")]
[JsonDerivedType(typeof(ClientCoreTransitionRejection), "core")]
public abstract record ClientTransitionRejection;

/// <summary>The command cited a stale snapshot generation.</summary>
/// <param name="Expected">Generation required by the command.</param>
/// <param name="Actual">Current session generation.</param>
public sealed record ClientSnapshotGenerationMismatchTransitionRejection(
    ulong Expected,
    ulong Actual) : ClientTransitionRejection;

/// <summary>The command ID was already used for different semantic content.</summary>
public sealed record ClientCommandIdConflictTransitionRejection : ClientTransitionRejection;

/// <summary>The authoritative core rejected the command.</summary>
/// <param name="Reason">Closed authoritative rejection reason.</param>
public sealed record ClientCoreTransitionRejection(
    ClientCommandRejectionReason Reason) : ClientTransitionRejection;

/// <summary>The atomic response to one client or authority command.</summary>
/// <param name="SchemaVersion">Client schema version.</param>
/// <param name="CommandId">Command identifier.</param>
/// <param name="Disposition">Accepted, rejected, or duplicate replay.</param>
/// <param name="RejectionReason">Required-nullable rejection details.</param>
/// <param name="PreSnapshotGeneration">Generation before resolution.</param>
/// <param name="PostSnapshotGeneration">Generation after resolution.</param>
/// <param name="PresentationTickRate">Presentation ticks per second.</param>
/// <param name="InputLockTicks">Minimum input lock in presentation ticks.</param>
/// <param name="Events">Ordered presentation events.</param>
/// <param name="PostSnapshot">Authoritative reconciliation snapshot.</param>
/// <param name="PostStateHash">Exact post-state hash.</param>
public sealed record ClientMatchTransition(
    uint SchemaVersion,
    string CommandId,
    ClientTransitionDisposition Disposition,
    ClientTransitionRejection? RejectionReason,
    ulong PreSnapshotGeneration,
    ulong PostSnapshotGeneration,
    uint PresentationTickRate,
    uint InputLockTicks,
    IReadOnlyList<ClientPresentationEvent> Events,
    ClientMatchSnapshot PostSnapshot,
    string PostStateHash);

/// <summary>Closed preview-rejection details.</summary>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(
    typeof(ClientSnapshotGenerationMismatchPreviewRejection),
    "snapshotGenerationMismatch")]
[JsonDerivedType(typeof(ClientCorePreviewRejection), "core")]
public abstract record ClientPreviewRejection;

/// <summary>The preview cited a stale snapshot generation.</summary>
/// <param name="Expected">Generation required by the preview.</param>
/// <param name="Actual">Current session generation.</param>
public sealed record ClientSnapshotGenerationMismatchPreviewRejection(
    ulong Expected,
    ulong Actual) : ClientPreviewRejection;

/// <summary>The authoritative core refused the preview.</summary>
/// <param name="Reason">Closed authoritative rejection reason.</param>
public sealed record ClientCorePreviewRejection(
    ClientCommandRejectionReason Reason) : ClientPreviewRejection;

/// <summary>The complete response to an ability-preview request.</summary>
/// <param name="SchemaVersion">Client schema version.</param>
/// <param name="SnapshotGeneration">Generation previewed.</param>
/// <param name="Legal">Whether the request is legal.</param>
/// <param name="RejectionReason">Required-nullable rejection details.</param>
/// <param name="GaugeCost">Exact gauge cost.</param>
/// <param name="LegalTargetPlayerIds">Sorted legal target identifiers.</param>
/// <param name="ProjectileTraces">Static authoritative guide traces.</param>
public sealed record ClientAbilityPreviewResponse(
    uint SchemaVersion,
    ulong SnapshotGeneration,
    bool Legal,
    ClientPreviewRejection? RejectionReason,
    ushort GaugeCost,
    IReadOnlyList<string> LegalTargetPlayerIds,
    IReadOnlyList<ClientProjectileTrace> ProjectileTraces);
