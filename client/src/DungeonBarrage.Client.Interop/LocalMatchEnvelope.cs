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

    /// <summary>
    /// A two-player local duel. Both sides receive the same picked loadout; there is no kit id.
    /// </summary>
    /// <param name="simulationVersion">Native simulation version.</param>
    /// <param name="contentVersion">Native content version.</param>
    /// <param name="seed">Match seed.</param>
    /// <param name="matchId">Opaque match id.</param>
    /// <param name="mapId">Authored map id.</param>
    /// <param name="loadout">Equipped items from <see cref="LoadoutPicker.Loadout"/>.</param>
    /// <returns>The create request Confirm sends to <c>LocalMatchSession.Create</c>.</returns>
    public static ClientCreateRequest HumanVsBot(
        uint simulationVersion,
        uint contentVersion,
        ulong seed,
        string matchId,
        string mapId,
        ClientLoadout loadout)
    {
        ArgumentNullException.ThrowIfNull(matchId);
        ArgumentNullException.ThrowIfNull(mapId);
        ArgumentNullException.ThrowIfNull(loadout);

        return new ClientCreateRequest(
            SchemaVersion: 1,
            MatchId: matchId,
            SimulationVersion: simulationVersion,
            ContentVersion: contentVersion,
            Match: new ClientMatchConfig(
                Seed: seed,
                MapId: mapId,
                Mode: "turnBased",
                Players:
                [
                    new ClientPlayerConfig("a-local-player", Team: 0, loadout, DefaultAppearance),
                    new ClientPlayerConfig("b-local-bot", Team: 1, loadout, DefaultAppearance),
                ]));
    }
}
