namespace DungeonBarrage.Client.Contracts;

/// <summary>
/// A client's own local claim that its planning deadline expired for one player's turn.
/// </summary>
/// <remarks>
/// Mirrors <c>db-sim-ffi/src/wire.rs</c>'s <c>AuthorityTimeoutDto</c> field for field — a flat
/// shape with no <c>kind</c> discriminator, unlike <see cref="ClientMatchCommand"/>. That
/// asymmetry is deliberate: the native side accepts this only through the distinct
/// <c>db_sim_match_timeout</c> export, never through <c>db_sim_match_apply</c>'s command union, so
/// nothing here can be smuggled through an ordinary command payload
/// (<c>docs/CLIENT_SPEC.md</c> §9.1; <c>SECURITY_BASELINE.md</c> §2: the server owns the clock).
/// </remarks>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="ActionId">Deterministic match-unique idempotency key, sharing the command id space.</param>
/// <param name="PlayerId">The player whose turn is being ended, validated against the active player.</param>
/// <param name="ExpectedTurnNumber">Turn number observed when the deadline expired.</param>
/// <param name="ExpectedSnapshotGeneration">Session snapshot generation observed when the deadline expired.</param>
public sealed record ClientAuthorityTimeout(
    uint SchemaVersion,
    string ActionId,
    string PlayerId,
    uint ExpectedTurnNumber,
    ulong ExpectedSnapshotGeneration)
{
    /// <summary>Builds a timeout claim for the observed turn/generation.</summary>
    /// <param name="actionId">Deterministic match-unique idempotency key.</param>
    /// <param name="playerId">The player whose turn is being ended.</param>
    /// <param name="expectedTurnNumber">Turn number observed when the deadline expired.</param>
    /// <param name="expectedSnapshotGeneration">Session generation observed when the deadline expired.</param>
    public static ClientAuthorityTimeout Create(
        string actionId,
        string playerId,
        uint expectedTurnNumber,
        ulong expectedSnapshotGeneration) =>
        new(1, actionId, playerId, expectedTurnNumber, expectedSnapshotGeneration);
}
