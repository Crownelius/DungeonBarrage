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

    /// <summary>The launch-default triangle: Ramshot Cannon, Recurve Bow, Trench Spade.</summary>
    /// <remarks>
    /// Mirrors <c>Loadout::launch_default()</c> on the Rust side. Used as the opponent's kit so a
    /// duel is not a mirror match by construction.
    /// </remarks>
    public static ClientLoadout LaunchDefaultLoadout { get; } =
        new("ramshot-cannon", "recurve-bow", "trench-spade");

    /// <summary>
    /// A two-player local duel. Each side carries its own loadout; there is no kit id.
    /// </summary>
    /// <remarks>
    /// The opponent's loadout is a separate argument on purpose. Passing the human's pick to both
    /// sides made every duel a mirror match and made the smoke report's two item fields
    /// indistinguishable, which is how a leftover per-side field went unnoticed.
    /// </remarks>
    /// <param name="simulationVersion">Native simulation version.</param>
    /// <param name="contentVersion">Native content version.</param>
    /// <param name="seed">Match seed.</param>
    /// <param name="matchId">Opaque match id.</param>
    /// <param name="mapId">Authored map id.</param>
    /// <param name="humanLoadout">Equipped items from <see cref="LoadoutPicker.Loadout"/>.</param>
    /// <param name="botLoadout">
    /// The opponent's equipped items. Defaults to <see cref="LaunchDefaultLoadout"/>.
    /// </param>
    /// <returns>The create request Confirm sends to <c>LocalMatchSession.Create</c>.</returns>
    public static ClientCreateRequest HumanVsBot(
        uint simulationVersion,
        uint contentVersion,
        ulong seed,
        string matchId,
        string mapId,
        ClientLoadout humanLoadout,
        ClientLoadout? botLoadout = null)
    {
        ArgumentNullException.ThrowIfNull(matchId);
        ArgumentNullException.ThrowIfNull(mapId);
        ArgumentNullException.ThrowIfNull(humanLoadout);
        var opponentLoadout = botLoadout ?? LaunchDefaultLoadout;

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
                    new ClientPlayerConfig("a-local-player", Team: 0, humanLoadout, DefaultAppearance),
                    new ClientPlayerConfig("b-local-bot", Team: 1, opponentLoadout, DefaultAppearance),
                ]));
    }
}
