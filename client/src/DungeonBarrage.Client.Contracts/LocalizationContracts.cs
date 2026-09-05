using System.Text.Json.Serialization;

namespace DungeonBarrage.Client.Contracts;

/// <summary>
/// Definition for a supported locale in the client.
/// </summary>
/// <param name="Tag">BCP-47 language tag (e.g. "en-US").</param>
/// <param name="DisplayName">Human readable language display name.</param>
/// <param name="RightToLeft">Whether the locale uses right-to-left text direction.</param>
public sealed record ClientLocaleDefinition(
    string Tag,
    string DisplayName,
    bool RightToLeft = false);

/// <summary>
/// Table of localized translation strings keyed by identifier.
/// </summary>
/// <param name="Tag">Locale language tag.</param>
/// <param name="Translations">Mapping of localization keys to localized format strings.</param>
public sealed record ClientLocalizedStringTable(
    string Tag,
    IReadOnlyDictionary<string, string> Translations)
{
    /// <summary>Creates an empty translation table for a given locale tag.</summary>
    public static ClientLocalizedStringTable Empty(string tag) => new(tag, new Dictionary<string, string>());
}
