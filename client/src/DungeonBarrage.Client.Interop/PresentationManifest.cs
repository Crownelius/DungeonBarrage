using System.Text.Json;
using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>One fighter's cosmetic catalog from the presentation manifest.</summary>
/// <param name="CharacterId">Fighter identifier. The playable cut requires <c>crow</c>.</param>
/// <param name="SkinIds">Allowed body skins.</param>
/// <param name="AbilitySkinIds">Allowed per-slot ability skins.</param>
/// <param name="VictoryPoseIds">Allowed victory poses.</param>
public sealed record PresentationCharacter(
    string CharacterId,
    IReadOnlyList<string> SkinIds,
    IReadOnlyList<string> AbilitySkinIds,
    IReadOnlyList<string> VictoryPoseIds);

/// <summary>The decoded presentation manifest, after version and appearance checks.</summary>
/// <param name="SchemaVersion">Manifest schema. Currently 1.</param>
/// <param name="ContentVersion">Must equal the native and request content versions.</param>
/// <param name="Characters">Cosmetic catalogs keyed by fighter id in the JSON array.</param>
public sealed record PresentationManifestDocument(
    uint SchemaVersion,
    uint ContentVersion,
    IReadOnlyList<PresentationCharacter> Characters);

/// <summary>
/// Validates the committed presentation manifest against a create request and the loaded native
/// library. Godot only reads the file; this is the check Confirm actually runs.
/// </summary>
public static class PresentationManifest
{
    /// <summary>The only schema this client knows how to read.</summary>
    public const uint SupportedSchemaVersion = 1;

    /// <summary>
    /// Parses <paramref name="utf8Json"/> and refuses a content-version mismatch before a match
    /// is created. A bump of <c>CONTENT_VERSION</c> that forgets
    /// <c>presentation-manifest-v1.json</c> must fail here, not only in a Godot export smoke.
    /// </summary>
    /// <param name="utf8Json">Exact bytes of the committed presentation manifest.</param>
    /// <param name="request">The create envelope about to be submitted.</param>
    /// <param name="nativeContentVersion">
    /// <see cref="LocalMatchSession.ContentVersion"/> from the loaded native library.
    /// </param>
    /// <returns>The decoded manifest.</returns>
    /// <exception cref="InvalidDataException">Schema, content version, or appearance failed.</exception>
    public static PresentationManifestDocument Validate(
        ReadOnlySpan<byte> utf8Json,
        ClientCreateRequest request,
        uint nativeContentVersion)
    {
        ArgumentNullException.ThrowIfNull(request);

        var manifest = JsonSerializer.Deserialize<PresentationManifestDocument>(
            utf8Json,
            ClientEnvelope.Options)
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

        var characters = new Dictionary<string, PresentationCharacter>(StringComparer.Ordinal);
        foreach (var character in manifest.Characters)
        {
            if (!characters.TryAdd(character.CharacterId, character))
            {
                throw new InvalidDataException("The presentation manifest contains a duplicate character ID.");
            }
        }

        if (!characters.TryGetValue("crow", out var crow))
        {
            throw new InvalidDataException("The presentation manifest must include the crow fighter.");
        }

        foreach (var player in request.Match.Players)
        {
            RequireContains(crow.SkinIds, player.Appearance.SkinId, "skin", "crow");
            RequireContains(
                crow.VictoryPoseIds,
                player.Appearance.VictoryPoseId,
                "victory pose",
                "crow");

            foreach (var abilitySkinId in player.Appearance.AbilitySkinIds)
            {
                RequireContains(
                    crow.AbilitySkinIds,
                    abilitySkinId,
                    "ability skin",
                    "crow");
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
