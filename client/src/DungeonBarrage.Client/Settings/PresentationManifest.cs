using System.Text.Json;
using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Settings;

internal sealed record PresentationCharacter(
    string CharacterId,
    IReadOnlyList<string> SkinIds,
    IReadOnlyList<string> AbilitySkinIds,
    IReadOnlyList<string> VictoryPoseIds);

internal sealed record PresentationManifest(
    uint SchemaVersion,
    uint ContentVersion,
    IReadOnlyList<PresentationCharacter> Characters)
{
    private const uint SupportedSchemaVersion = 1;
    private const string ResourcePath = "res://Settings/presentation-manifest-v1.json";

    internal static PresentationManifest LoadAndValidate(
        ClientCreateRequest request,
        uint nativeContentVersion)
    {
        using var file = Godot.FileAccess.Open(ResourcePath, Godot.FileAccess.ModeFlags.Read);
        if (file is null)
        {
            throw new InvalidDataException(
                $"The presentation manifest could not be opened: {ResourcePath} " +
                $"({Godot.FileAccess.GetOpenError()}).");
        }

        // `GetLength()` returns `ulong`; `GetBuffer` takes `long`. A manifest large enough to
        // overflow that conversion could not exist as a Godot text resource in the first place,
        // so this is a defensive bound rather than a realistic runtime path.
        var bytes = file.GetBuffer(checked((long)file.GetLength()));
        var manifest = JsonSerializer.Deserialize<PresentationManifest>(bytes, ClientEnvelope.Options)
            ?? throw new InvalidDataException("The presentation manifest decoded to null.");

        if (manifest.SchemaVersion != SupportedSchemaVersion)
        {
            throw new InvalidDataException(
                $"Presentation schema {manifest.SchemaVersion} is unsupported; expected {SupportedSchemaVersion}.");
        }

        if (manifest.ContentVersion != request.ContentVersion ||
            manifest.ContentVersion != nativeContentVersion)
        {
            throw new InvalidDataException(
                $"Presentation content {manifest.ContentVersion}, request content " +
                $"{request.ContentVersion}, and native content {nativeContentVersion} must match.");
        }

        var characters = manifest.Characters.ToDictionary(
            character => character.CharacterId,
            StringComparer.Ordinal);
        if (characters.Count != manifest.Characters.Count)
        {
            throw new InvalidDataException("The presentation manifest contains a duplicate character ID.");
        }

        foreach (var player in request.Match.Players)
        {
            if (!characters.TryGetValue(player.CharacterId, out var character))
            {
                throw new InvalidDataException(
                    $"No presentation entry exists for character '{player.CharacterId}'.");
            }

            RequireContains(character.SkinIds, player.Appearance.SkinId, "skin", player.CharacterId);
            RequireContains(
                character.VictoryPoseIds,
                player.Appearance.VictoryPoseId,
                "victory pose",
                player.CharacterId);

            foreach (var abilitySkinId in player.Appearance.AbilitySkinIds)
            {
                RequireContains(
                    character.AbilitySkinIds,
                    abilitySkinId,
                    "ability skin",
                    player.CharacterId);
            }
        }

        return manifest;
    }

    private static void RequireContains(
        IReadOnlyList<string> allowed,
        string selected,
        string category,
        string characterId)
    {
        if (!allowed.Contains(selected, StringComparer.Ordinal))
        {
            throw new InvalidDataException(
                $"Unknown {category} '{selected}' for character '{characterId}'.");
        }
    }
}
