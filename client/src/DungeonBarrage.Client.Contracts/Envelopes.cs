using System.Text.Json;
using System.Text.Json.Serialization;

namespace DungeonBarrage.Client.Contracts;

/// <summary>
/// Serializer settings for every client envelope.
/// </summary>
/// <remarks>
/// <para>
/// Strict by construction. Unmapped members are a hard failure rather than silently dropped: the
/// authoritative core is the only thing that decides what a response contains, so a field the
/// managed layer does not know about means the two are out of step, and discovering that at the
/// boundary is far cheaper than discovering it as missing gameplay later.
/// </para>
/// <para>
/// There is exactly one configuration. A second "lenient" variant would inevitably become the one
/// used in production, which is how a schema mismatch turns into a silent data-loss bug.
/// </para>
/// </remarks>
public static class ClientEnvelope
{
    /// <summary>The single serializer configuration for the frozen camelCase envelopes.</summary>
    public static JsonSerializerOptions Options { get; } = Create();

    private static JsonSerializerOptions Create()
    {
        var options = new JsonSerializerOptions
        {
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
            PropertyNameCaseInsensitive = false,
            UnmappedMemberHandling = JsonUnmappedMemberHandling.Disallow,
            NumberHandling = JsonNumberHandling.Strict,
            ReadCommentHandling = JsonCommentHandling.Disallow,
            AllowTrailingCommas = false,
            AllowDuplicateProperties = false,
            AllowOutOfOrderMetadataProperties = true,
            DefaultIgnoreCondition = JsonIgnoreCondition.Never,
            RespectNullableAnnotations = true,
            RespectRequiredConstructorParameters = true,
            WriteIndented = false,
        };

        // Closed enums travel as camelCase strings and must round-trip exactly. Allowing integer
        // fallback would let an unknown discriminant arrive as a number and be accepted.
        options.Converters.Add(new JsonStringEnumConverter(JsonNamingPolicy.CamelCase, allowIntegerValues: false));

        // Frozen before first use so nothing can mutate the shared configuration at runtime and
        // change how envelopes are interpreted midway through a session. The reflection resolver
        // is populated explicitly; a future ahead-of-time build will need a source-generated
        // `JsonSerializerContext` here instead, which is a swap of this one line.
        options.MakeReadOnly(populateMissingResolver: true);
        return options;
    }
}

/// <summary>Match phases a client may observe.</summary>
public enum ClientMatchPhase
{
    /// <summary>Pre-match introduction.</summary>
    MatchIntro,

    /// <summary>A turn is opening.</summary>
    TurnStart,

    /// <summary>Movement is accepted.</summary>
    Movement,

    /// <summary>Aiming and ability selection are accepted.</summary>
    AimingAndSelection,

    /// <summary>A command is locked in and resolving.</summary>
    CommandLocked,

    /// <summary>Resolution is running.</summary>
    Resolution,

    /// <summary>Physics is settling.</summary>
    Settling,

    /// <summary>The one-time passive choice is owed.</summary>
    PassiveSelection,

    /// <summary>End-of-turn status resolution.</summary>
    StatusResolution,

    /// <summary>Victory evaluation.</summary>
    VictoryCheck,

    /// <summary>The match is over.</summary>
    MatchComplete,
}

/// <summary>Whether a transition was accepted, refused, or replayed.</summary>
public enum ClientTransitionDisposition
{
    /// <summary>The first receipt was accepted.</summary>
    Accepted,

    /// <summary>The first receipt was refused.</summary>
    Rejected,

    /// <summary>The original first result is being replayed without mutation.</summary>
    DuplicateReplay,
}

/// <summary>Character ability slots.</summary>
public enum ClientAbilitySlot
{
    /// <summary>Primary basic attack.</summary>
    Basic,

    /// <summary>Optional second basic attack.</summary>
    BasicAlt,

    /// <summary>Gauge-consuming special.</summary>
    Special,
}

/// <summary>Cosmetic appearance selected before the match.</summary>
/// <param name="SkinId">Character skin identifier.</param>
/// <param name="AbilitySkinIds">Per-ability skin identifiers, in slot order.</param>
/// <param name="VictoryPoseId">Victory pose identifier.</param>
public sealed record ClientAppearance(
    [property: JsonPropertyName("skinId")] string SkinId,
    [property: JsonPropertyName("abilitySkinIds")] IReadOnlyList<string> AbilitySkinIds,
    [property: JsonPropertyName("victoryPoseId")] string VictoryPoseId);

/// <summary>One player in a creation request.</summary>
/// <param name="PlayerId">Opaque match-local player identifier.</param>
/// <param name="Team">Team number; equal values are allies.</param>
/// <param name="CharacterId">Stable character definition identifier.</param>
/// <param name="Appearance">Cosmetic-only selection.</param>
public sealed record ClientPlayerConfig(
    [property: JsonPropertyName("playerId")] string PlayerId,
    [property: JsonPropertyName("team")] byte Team,
    [property: JsonPropertyName("characterId")] string CharacterId,
    [property: JsonPropertyName("appearance")] ClientAppearance Appearance);

/// <summary>The match body of a creation request.</summary>
/// <param name="Seed">Explicit entropy for the deterministic match generator.</param>
/// <param name="MapId">Stable authored map identifier.</param>
/// <param name="Mode">Scheduler model.</param>
/// <param name="Players">Lobby order.</param>
public sealed record ClientMatchConfig(
    [property: JsonPropertyName("seed")] ulong Seed,
    [property: JsonPropertyName("mapId")] string MapId,
    [property: JsonPropertyName("mode")] string Mode,
    [property: JsonPropertyName("players")] IReadOnlyList<ClientPlayerConfig> Players);

/// <summary>A complete match-creation request envelope.</summary>
/// <param name="SchemaVersion">Client contract schema version.</param>
/// <param name="MatchId">Stable identifier for this match.</param>
/// <param name="SimulationVersion">Simulation version the client was built against.</param>
/// <param name="ContentVersion">Content version the client was built against.</param>
/// <param name="Match">The match configuration.</param>
public sealed record ClientCreateRequest(
    [property: JsonPropertyName("schemaVersion")] uint SchemaVersion,
    [property: JsonPropertyName("matchId")] string MatchId,
    [property: JsonPropertyName("simulationVersion")] uint SimulationVersion,
    [property: JsonPropertyName("contentVersion")] uint ContentVersion,
    [property: JsonPropertyName("match")] ClientMatchConfig Match);
