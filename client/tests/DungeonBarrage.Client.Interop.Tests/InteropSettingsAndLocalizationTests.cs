using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public class InteropSettingsAndLocalizationTests
{
    [Fact]
    public void Catalog_ResolvesEnglishKeysByDefault()
    {
        var catalog = new LocalizationCatalog("en-US");
        var title = catalog.Get("ui.title");
        var start = catalog.Get("ui.start_match");

        Assert.Equal("Dungeon Barrage", title);
        Assert.Equal("Press ENTER to Start Match", start);
    }

    [Fact]
    public void Catalog_SwitchesToSpanishLocale()
    {
        var catalog = new LocalizationCatalog("es-ES");
        var victory = catalog.Get("ui.victory");
        var rematch = catalog.Get("ui.rematch_prompt");

        Assert.Equal("VICTORIA", victory);
        Assert.Equal("Presiona R o ENTER para Revancha", rematch);
    }

    [Fact]
    public void Catalog_FormatsParametersCorrectly()
    {
        var catalog = new LocalizationCatalog("en-US");
        var turn = catalog.Get("ui.turn_format", 5);
        var hp = catalog.Get("ui.hp_format", 120, 200);

        Assert.Equal("Turn 5", turn);
        Assert.Equal("HP: 120/200", hp);
    }

    [Fact]
    public void Catalog_FallsBackToEnglishForMissingTranslationInSupportedLocale()
    {
        var catalog = new LocalizationCatalog("es-ES");
        var missingInSpanishKey = "ui.only_in_english";

        catalog.RegisterTable(new ClientLocalizedStringTable("en-US", new Dictionary<string, string>
        {
            [missingInSpanishKey] = "English Only String",
        }));

        var resolved = catalog.Get(missingInSpanishKey);
        Assert.Equal("English Only String", resolved);
    }

    [Fact]
    public void UserSettingsStore_RecoversCorruptFileSafelyToDefaults()
    {
        var tempFile = Path.Combine(Path.GetTempPath(), $"corrupt_settings_{Guid.NewGuid():N}.json");
        try
        {
            File.WriteAllText(tempFile, "{ corrupt json data: [[[ ");
            var loaded = UserSettingsStore.Load(tempFile);

            Assert.NotNull(loaded);
            Assert.Equal(1u, loaded.SchemaVersion);
            Assert.Equal("en-US", loaded.PreferredLocale);
            Assert.Equal((byte)100, loaded.Audio.MasterVolume);
            Assert.Equal(1.0f, loaded.Accessibility.TextScale);
            Assert.Equal(ClientPerformanceTier.High, loaded.Performance.Tier);
        }
        finally
        {
            if (File.Exists(tempFile))
            {
                File.Delete(tempFile);
            }
        }
    }

    [Fact]
    public void UserSettingsStore_RoundTripsSuccessfully()
    {
        var tempFile = Path.Combine(Path.GetTempPath(), $"valid_settings_{Guid.NewGuid():N}.json");
        try
        {
            var custom = new ClientUserSettingsContainer(
                SchemaVersion: 1,
                PreferredLocale: "es-ES",
                Audio: new ClientAudioSettings(MasterVolume: 80, SfxVolume: 90, MusicVolume: 70),
                Accessibility: new ClientAccessibilitySettings(HighContrast: true, TextScale: 1.25f),
                Performance: new ClientPerformanceSettings(Tier: ClientPerformanceTier.Medium, TargetFps: 60));

            var saved = UserSettingsStore.Save(tempFile, custom);
            Assert.True(saved);

            var reloaded = UserSettingsStore.Load(tempFile);
            Assert.Equal("es-ES", reloaded.PreferredLocale);
            Assert.Equal((byte)80, reloaded.Audio.MasterVolume);
            Assert.True(reloaded.Accessibility.HighContrast);
            Assert.Equal(1.25f, reloaded.Accessibility.TextScale);
            Assert.Equal(ClientPerformanceTier.Medium, reloaded.Performance.Tier);
        }
        finally
        {
            if (File.Exists(tempFile))
            {
                File.Delete(tempFile);
            }
        }
    }
}
