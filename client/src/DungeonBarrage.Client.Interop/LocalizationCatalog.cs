using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Manages locale string tables, active language selection, and fallback resolution.
/// </summary>
public sealed class LocalizationCatalog
{
    /// <summary>Default fallback language tag ("en-US").</summary>
    public const string DefaultLocaleTag = "en-US";

    private readonly Dictionary<string, ClientLocalizedStringTable> _tables;

    /// <summary>Gets the currently active language tag.</summary>
    public string ActiveLocaleTag { get; private set; }

    /// <summary>Supported locale definitions in the application.</summary>
    public static IReadOnlyList<ClientLocaleDefinition> SupportedLocales { get; } =
    [
        new ClientLocaleDefinition("en-US", "English (US)"),
        new ClientLocaleDefinition("es-ES", "Español (España)"),
        new ClientLocaleDefinition("ja-JP", "日本語"),
    ];

    /// <summary>Initializes a new localization catalog with default built-in tables.</summary>
    public LocalizationCatalog(string activeLocaleTag = DefaultLocaleTag)
    {
        ActiveLocaleTag = IsSupported(activeLocaleTag) ? activeLocaleTag : DefaultLocaleTag;
        _tables = new Dictionary<string, ClientLocalizedStringTable>(StringComparer.OrdinalIgnoreCase);
        RegisterDefaultTables();
    }

    /// <summary>Checks whether a language tag is supported.</summary>
    public static bool IsSupported(string tag) =>
        SupportedLocales.Any(l => string.Equals(l.Tag, tag, StringComparison.OrdinalIgnoreCase));

    /// <summary>Registers or updates a translation table for a locale tag.</summary>
    public void RegisterTable(ClientLocalizedStringTable table)
    {
        ArgumentNullException.ThrowIfNull(table);
        _tables[table.Tag] = table;
    }

    /// <summary>Sets the active locale tag if supported.</summary>
    public bool SetLocale(string tag)
    {
        if (!IsSupported(tag))
        {
            return false;
        }

        ActiveLocaleTag = tag;
        return true;
    }

    /// <summary>Gets a localized string for a key, formatting optional parameters.</summary>
    public string Get(string key, params object[] args)
    {
        if (string.IsNullOrWhiteSpace(key))
        {
            return string.Empty;
        }

        if (_tables.TryGetValue(ActiveLocaleTag, out var activeTable) &&
            activeTable.Translations.TryGetValue(key, out var translation))
        {
            return FormatTranslation(translation, args);
        }

        if (!string.Equals(ActiveLocaleTag, DefaultLocaleTag, StringComparison.OrdinalIgnoreCase) &&
            _tables.TryGetValue(DefaultLocaleTag, out var defaultTable) &&
            defaultTable.Translations.TryGetValue(key, out var defaultTranslation))
        {
            return FormatTranslation(defaultTranslation, args);
        }

        return key;
    }

    private static string FormatTranslation(string pattern, object[] args)
    {
        if (args is null || args.Length == 0)
        {
            return pattern;
        }

        try
        {
            return string.Format(pattern, args);
        }
        catch (FormatException)
        {
            return pattern;
        }
    }

    private void RegisterDefaultTables()
    {
        var english = new ClientLocalizedStringTable(
            "en-US",
            new Dictionary<string, string>(StringComparer.Ordinal)
            {
                ["ui.title"] = "Dungeon Barrage",
                ["ui.select_champion"] = "Select Your Champion",
                ["ui.select_opponent"] = "Select Bot Opponent",
                ["ui.start_match"] = "Press ENTER to Start Match",
                ["ui.victory"] = "VICTORY",
                ["ui.defeat"] = "DEFEAT",
                ["ui.draw"] = "DRAW",
                ["ui.rematch_prompt"] = "Press R or ENTER for Rematch",
                ["ui.turn_format"] = "Turn {0}",
                ["ui.hp_format"] = "HP: {0}/{1}",
                ["ui.settings_audio"] = "Audio Settings",
                ["ui.settings_accessibility"] = "Accessibility",
            });

        var spanish = new ClientLocalizedStringTable(
            "es-ES",
            new Dictionary<string, string>(StringComparer.Ordinal)
            {
                ["ui.title"] = "Dungeon Barrage",
                ["ui.select_champion"] = "Selecciona tu Campeón",
                ["ui.select_opponent"] = "Selecciona Oponente Bot",
                ["ui.start_match"] = "Presiona ENTER para Empezar",
                ["ui.victory"] = "VICTORIA",
                ["ui.defeat"] = "DERROTA",
                ["ui.draw"] = "EMPATE",
                ["ui.rematch_prompt"] = "Presiona R o ENTER para Revancha",
                ["ui.turn_format"] = "Turno {0}",
                ["ui.hp_format"] = "PS: {0}/{1}",
                ["ui.settings_audio"] = "Ajustes de Audio",
                ["ui.settings_accessibility"] = "Accesibilidad",
            });

        var japanese = new ClientLocalizedStringTable(
            "ja-JP",
            new Dictionary<string, string>(StringComparer.Ordinal)
            {
                ["ui.title"] = "ダンジョン・バラージュ",
                ["ui.select_champion"] = "チャンピオンを選択",
                ["ui.select_opponent"] = "ボット対戦相手を選択",
                ["ui.start_match"] = "ENTERキーでマッチ開始",
                ["ui.victory"] = "勝利",
                ["ui.defeat"] = "敗北",
                ["ui.draw"] = "引き分け",
                ["ui.rematch_prompt"] = "RまたはENTERで再戦",
                ["ui.turn_format"] = "ターン {0}",
                ["ui.hp_format"] = "HP: {0}/{1}",
                ["ui.settings_audio"] = "オーディオ設定",
                ["ui.settings_accessibility"] = "アクセシビリティ",
            });

        RegisterTable(english);
        RegisterTable(spanish);
        RegisterTable(japanese);
    }
}
