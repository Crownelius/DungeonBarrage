using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Builds the local human-vs-bot create envelope the Godot confirm path submits.
/// </summary>
public static class LocalMatchEnvelope
{
    /// <summary>Default cosmetic selection. Skins never grant gameplay.</summary>
    public static ClientAppearance DefaultAppearance { get; } =
        new("default", ["default", "default", "default"], "default");

    /// <summary>Default human character.</summary>
    public const string LaunchDefaultCharacterId = "crow";

    /// <summary>Default opponent character, intentionally different from the human default.</summary>
    public const string LaunchDefaultBotCharacterId = "erus";

    /// <summary>
    /// A two-player local duel. Each side selects one fixed character kit.
    /// </summary>
    /// <remarks>
    /// The opponent's character is a separate argument so the default duel is not a mirror match.
    /// </remarks>
    /// <param name="simulationVersion">Native simulation version.</param>
    /// <param name="contentVersion">Native content version.</param>
    /// <param name="seed">Match seed.</param>
    /// <param name="matchId">Opaque match id.</param>
    /// <param name="mapId">Authored map id.</param>
    /// <param name="humanCharacterId">Character selected on the character screen.</param>
    /// <param name="botCharacterId">
    /// The opponent's character. Defaults to <see cref="LaunchDefaultBotCharacterId"/>.
    /// </param>
    /// <returns>The create request Confirm sends to <c>LocalMatchSession.Create</c>.</returns>
    public static ClientCreateRequest HumanVsBot(
        uint simulationVersion,
        uint contentVersion,
        ulong seed,
        string matchId,
        string mapId,
        string humanCharacterId,
        string? botCharacterId = null)
    {
        ArgumentNullException.ThrowIfNull(matchId);
        ArgumentNullException.ThrowIfNull(mapId);
        ArgumentException.ThrowIfNullOrWhiteSpace(humanCharacterId);
        var opponentCharacter = botCharacterId ?? LaunchDefaultBotCharacterId;

        return new ClientCreateRequest(
            SchemaVersion: ClientContract.CurrentSchemaVersion,
            MatchId: matchId,
            SimulationVersion: simulationVersion,
            ContentVersion: contentVersion,
            Match: new ClientMatchConfig(
                Seed: seed,
                MapId: mapId,
                Mode: "turnBased",
                Players:
                [
                    new ClientPlayerConfig("a-local-player", Team: 0, humanCharacterId, DefaultAppearance),
                    new ClientPlayerConfig("b-local-bot", Team: 1, opponentCharacter, DefaultAppearance),
                ]));
    }
}
