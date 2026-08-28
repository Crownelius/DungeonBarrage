using System.Text.Json.Serialization;

namespace DungeonBarrage.Client.Contracts;

/// <summary>
/// One client-authored match command, in the exact flattened envelope the native ABI parses.
/// </summary>
/// <remarks>
/// The wire shape is a flattened discriminated union — the variant's own fields sit directly on
/// the top-level object alongside the shared header fields, selected by <c>kind</c> — not a
/// nested <c>value</c> object. This mirrors <c>db-sim-ffi/src/wire.rs</c>'s <c>MatchCommandDto</c>
/// exactly, field for field, because the native side deserializes with
/// <c>#[serde(deny_unknown_fields)]</c> on every variant: an extra or missing field is a hard
/// parse failure at the boundary, not a warning.
/// </remarks>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="CommandId">Deterministic match-unique idempotency key.</param>
/// <param name="PlayerId">Claimed actor, validated against the active player.</param>
/// <param name="ExpectedTurnNumber">Turn number observed when the command was constructed.</param>
/// <param name="ExpectedSnapshotGeneration">Session snapshot generation observed at construction.</param>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(typeof(ClientMoveCommand), "move")]
[JsonDerivedType(typeof(ClientAbilityCommand), "ability")]
[JsonDerivedType(typeof(ClientPassiveChoiceCommand), "passiveChoice")]
[JsonDerivedType(typeof(ClientPassCommand), "pass")]
public abstract record ClientMatchCommand(
    uint SchemaVersion,
    string CommandId,
    string PlayerId,
    uint ExpectedTurnNumber,
    ulong ExpectedSnapshotGeneration)
{
    /// <summary>Builds a horizontal move command.</summary>
    /// <param name="commandId">Deterministic match-unique idempotency key.</param>
    /// <param name="playerId">Claimed actor.</param>
    /// <param name="expectedTurnNumber">Turn number observed when constructed.</param>
    /// <param name="expectedSnapshotGeneration">Session generation observed when constructed.</param>
    /// <param name="dx">Requested signed fixed-point horizontal displacement.</param>
    /// <returns>The command, base-typed so serialization always includes the discriminator.</returns>
    public static ClientMatchCommand Move(
        string commandId,
        string playerId,
        uint expectedTurnNumber,
        ulong expectedSnapshotGeneration,
        int dx) =>
        new ClientMoveCommand(1, commandId, playerId, expectedTurnNumber, expectedSnapshotGeneration, dx);

    /// <summary>Builds an ability command.</summary>
    /// <param name="commandId">Deterministic match-unique idempotency key.</param>
    /// <param name="playerId">Claimed actor.</param>
    /// <param name="expectedTurnNumber">Turn number observed when constructed.</param>
    /// <param name="expectedSnapshotGeneration">Session generation observed when constructed.</param>
    /// <param name="slot">Character ability slot.</param>
    /// <param name="angleMillidegrees">Launch angle in integer millidegrees.</param>
    /// <param name="powerBasisPoints">Launch power in basis points.</param>
    /// <param name="targetPlayerId">Optional primary player target.</param>
    /// <param name="secondaryTargetPlayerId">Optional secondary player target.</param>
    /// <returns>The command, base-typed so serialization always includes the discriminator.</returns>
    public static ClientMatchCommand Ability(
        string commandId,
        string playerId,
        uint expectedTurnNumber,
        ulong expectedSnapshotGeneration,
        ClientAbilitySlot slot,
        int angleMillidegrees,
        int powerBasisPoints,
        string? targetPlayerId,
        string? secondaryTargetPlayerId) =>
        new ClientAbilityCommand(
            1,
            commandId,
            playerId,
            expectedTurnNumber,
            expectedSnapshotGeneration,
            slot,
            angleMillidegrees,
            powerBasisPoints,
            targetPlayerId,
            secondaryTargetPlayerId);

    /// <summary>Builds a pass command.</summary>
    /// <param name="commandId">Deterministic match-unique idempotency key.</param>
    /// <param name="playerId">Claimed actor.</param>
    /// <param name="expectedTurnNumber">Turn number observed when constructed.</param>
    /// <param name="expectedSnapshotGeneration">Session generation observed when constructed.</param>
    /// <returns>The command, base-typed so serialization always includes the discriminator.</returns>
    public static ClientMatchCommand Pass(
        string commandId,
        string playerId,
        uint expectedTurnNumber,
        ulong expectedSnapshotGeneration) =>
        new ClientPassCommand(1, commandId, playerId, expectedTurnNumber, expectedSnapshotGeneration);
}

/// <summary>Move horizontally by a fixed-point delta, bounded by authoritative allowance.</summary>
/// <param name="Dx">Requested signed fixed-point horizontal displacement.</param>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="CommandId">Deterministic match-unique idempotency key.</param>
/// <param name="PlayerId">Claimed actor, validated against the active player.</param>
/// <param name="ExpectedTurnNumber">Turn number observed when the command was constructed.</param>
/// <param name="ExpectedSnapshotGeneration">Session snapshot generation observed at construction.</param>
public sealed record ClientMoveCommand(
    uint SchemaVersion,
    string CommandId,
    string PlayerId,
    uint ExpectedTurnNumber,
    ulong ExpectedSnapshotGeneration,
    int Dx)
    : ClientMatchCommand(SchemaVersion, CommandId, PlayerId, ExpectedTurnNumber, ExpectedSnapshotGeneration);

/// <summary>Commit one character ability.</summary>
/// <param name="Slot">Character ability slot.</param>
/// <param name="AngleMillidegrees">Launch angle in integer millidegrees.</param>
/// <param name="PowerBasisPoints">Launch power in basis points.</param>
/// <param name="TargetPlayerId">Optional primary player target.</param>
/// <param name="SecondaryTargetPlayerId">Optional secondary player target.</param>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="CommandId">Deterministic match-unique idempotency key.</param>
/// <param name="PlayerId">Claimed actor, validated against the active player.</param>
/// <param name="ExpectedTurnNumber">Turn number observed when the command was constructed.</param>
/// <param name="ExpectedSnapshotGeneration">Session snapshot generation observed at construction.</param>
public sealed record ClientAbilityCommand(
    uint SchemaVersion,
    string CommandId,
    string PlayerId,
    uint ExpectedTurnNumber,
    ulong ExpectedSnapshotGeneration,
    ClientAbilitySlot Slot,
    int AngleMillidegrees,
    int PowerBasisPoints,
    string? TargetPlayerId,
    string? SecondaryTargetPlayerId)
    : ClientMatchCommand(SchemaVersion, CommandId, PlayerId, ExpectedTurnNumber, ExpectedSnapshotGeneration);

/// <summary>Resolve the one-time passive selection interrupt.</summary>
/// <param name="PassiveId">Stable passive definition identifier.</param>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="CommandId">Deterministic match-unique idempotency key.</param>
/// <param name="PlayerId">Claimed actor, validated against the active player.</param>
/// <param name="ExpectedTurnNumber">Turn number observed when the command was constructed.</param>
/// <param name="ExpectedSnapshotGeneration">Session snapshot generation observed at construction.</param>
public sealed record ClientPassiveChoiceCommand(
    uint SchemaVersion,
    string CommandId,
    string PlayerId,
    uint ExpectedTurnNumber,
    ulong ExpectedSnapshotGeneration,
    string PassiveId)
    : ClientMatchCommand(SchemaVersion, CommandId, PlayerId, ExpectedTurnNumber, ExpectedSnapshotGeneration);

/// <summary>End the active turn without attacking.</summary>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="CommandId">Deterministic match-unique idempotency key.</param>
/// <param name="PlayerId">Claimed actor, validated against the active player.</param>
/// <param name="ExpectedTurnNumber">Turn number observed when the command was constructed.</param>
/// <param name="ExpectedSnapshotGeneration">Session snapshot generation observed at construction.</param>
public sealed record ClientPassCommand(
    uint SchemaVersion,
    string CommandId,
    string PlayerId,
    uint ExpectedTurnNumber,
    ulong ExpectedSnapshotGeneration)
    : ClientMatchCommand(SchemaVersion, CommandId, PlayerId, ExpectedTurnNumber, ExpectedSnapshotGeneration);
