using System.Text.Json;
using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Helper store for loading and persisting user settings containers to disk.
/// </summary>
public static class UserSettingsStore
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        WriteIndented = true,
        PropertyNameCaseInsensitive = true,
    };

    /// <summary>Loads settings from a JSON file, returning defaults on missing or corrupt files.</summary>
    public static ClientUserSettingsContainer Load(string filePath)
    {
        if (string.IsNullOrWhiteSpace(filePath) || !File.Exists(filePath))
        {
            return ClientUserSettingsContainer.Default;
        }

        try
        {
            var text = File.ReadAllText(filePath);
            var parsed = JsonSerializer.Deserialize<ClientUserSettingsContainer>(text, JsonOptions);
            return (parsed ?? ClientUserSettingsContainer.Default).Normalized();
        }
        catch (Exception)
        {
            // Corrupt or unparseable settings files recover safely to defaults
            return ClientUserSettingsContainer.Default;
        }
    }

    /// <summary>Persists user settings to a JSON file on disk.</summary>
    public static bool Save(string filePath, ClientUserSettingsContainer settings)
    {
        ArgumentNullException.ThrowIfNull(settings);

        if (string.IsNullOrWhiteSpace(filePath))
        {
            return false;
        }

        try
        {
            var directory = Path.GetDirectoryName(filePath);
            if (!string.IsNullOrWhiteSpace(directory))
            {
                Directory.CreateDirectory(directory);
            }

            var normalized = settings.Normalized();
            var json = JsonSerializer.Serialize(normalized, JsonOptions);
            File.WriteAllText(filePath, json);
            return true;
        }
        catch (Exception)
        {
            return false;
        }
    }
}
