using System.Text.Json.Serialization;

namespace DungeonBarrage.Client.Contracts;

/// <summary>How aggressively the native bot searches for a shot and how much it misses by.</summary>
/// <remarks>
/// Mirrors <c>db_sim_core::bot::BotDifficulty</c> exactly — two fixed presets, not a numeric
/// slider, since C6 asks for "a Rust bot," not a difficulty-select UI.
/// </remarks>
public enum ClientBotDifficulty
{
    /// <summary>Coarse search, generous aim error. A forgiving first opponent.</summary>
    Casual,

    /// <summary>Finer search, tighter aim error. A competent, still-beatable opponent.</summary>
    Standard,
}

/// <summary>A request for the native bot coordinator's proposed action for one player.</summary>
/// <remarks>
/// Mirrors <c>db-sim-ffi/src/wire.rs</c>'s <c>BotDecisionRequestDto</c> field for field. This is
/// a read-only query, not a command: the native side never mutates the session for this call, and
/// the caller must submit the returned decision through the ordinary apply path itself.
/// </remarks>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="PlayerId">The player the bot is deciding for.</param>
/// <param name="Difficulty">Search resolution and aim-error preset.</param>
/// <param name="DecisionSeed">
/// Seeds only the bot's own aim-jitter/passive-tie-break RNG — never the match's authoritative
/// RNG, so repeated or replayed decisions cannot desync the sequence a replay depends on.
/// </param>
public sealed record ClientBotDecisionRequest(
    uint SchemaVersion,
    string PlayerId,
    ClientBotDifficulty Difficulty,
    ulong DecisionSeed);

/// <summary>
/// One proposed action from the native bot coordinator, shaped like <see cref="ClientMatchCommand"/>'s
/// own <c>kind</c> variants but without any session-bookkeeping fields — the caller supplies a
/// fresh <c>commandId</c> and reads the current <c>expectedTurnNumber</c>/
/// <c>expectedSnapshotGeneration</c> itself before submitting through the ordinary apply path.
/// </summary>
/// <param name="SchemaVersion">Client-contract schema version.</param>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(typeof(ClientBotMoveDecision), "move")]
[JsonDerivedType(typeof(ClientBotAbilityDecision), "ability")]
[JsonDerivedType(typeof(ClientBotPassiveChoiceDecision), "passiveChoice")]
[JsonDerivedType(typeof(ClientBotPassDecision), "pass")]
[JsonDerivedType(typeof(ClientBotJumpDecision), "jump")]
public abstract record ClientBotDecision(uint SchemaVersion);

/// <summary>The bot wants to move horizontally.</summary>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="Dx">Requested signed fixed-point horizontal displacement.</param>
public sealed record ClientBotMoveDecision(uint SchemaVersion, int Dx)
    : ClientBotDecision(SchemaVersion);

/// <summary>The bot wants to fire an ability.</summary>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="Slot">Character ability slot.</param>
/// <param name="AngleMillidegrees">Launch angle in integer millidegrees.</param>
/// <param name="PowerBasisPoints">Launch power in basis points.</param>
/// <param name="TargetPlayerId">Optional primary player target.</param>
/// <param name="SecondaryTargetPlayerId">Optional secondary player target.</param>
public sealed record ClientBotAbilityDecision(
    uint SchemaVersion,
    ClientAbilitySlot Slot,
    int AngleMillidegrees,
    int PowerBasisPoints,
    string? TargetPlayerId,
    string? SecondaryTargetPlayerId) : ClientBotDecision(SchemaVersion);

/// <summary>The bot wants to resolve its one-time passive selection.</summary>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="PassiveId">Stable passive definition identifier.</param>
public sealed record ClientBotPassiveChoiceDecision(uint SchemaVersion, string PassiveId)
    : ClientBotDecision(SchemaVersion);

/// <summary>The bot wants to end its turn without acting.</summary>
/// <param name="SchemaVersion">Client-contract schema version.</param>
public sealed record ClientBotPassDecision(uint SchemaVersion) : ClientBotDecision(SchemaVersion);

/// <summary>The bot wants to hop straight up.</summary>
/// <param name="SchemaVersion">Client-contract schema version.</param>
public sealed record ClientBotJumpDecision(uint SchemaVersion) : ClientBotDecision(SchemaVersion);
