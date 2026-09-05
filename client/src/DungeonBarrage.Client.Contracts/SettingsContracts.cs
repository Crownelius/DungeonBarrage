using System.Text.Json;
using System.Text.Json.Serialization;

namespace DungeonBarrage.Client.Contracts;

/// <summary>
/// Performance quality tiers available in the client.
/// </summary>
public enum ClientPerformanceTier
{
    /// <summary>Low graphics quality and minimum particle density.</summary>
    Low = 0,

    /// <summary>Standard graphics quality.</summary>
    Medium = 1,

    /// <summary>Maximum graphics quality, MSAA, and full particle density.</summary>
    High = 2,
}

/// <summary>
/// Configures audio volume levels and mute status.
/// </summary>
/// <param name="MasterVolume">Master volume percentage from 0 to 100.</param>
/// <param name="SfxVolume">Sound effects volume percentage from 0 to 100.</param>
/// <param name="MusicVolume">Music volume percentage from 0 to 100.</param>
/// <param name="Muted">Whether all client audio is muted.</param>
public sealed record ClientAudioSettings(
    byte MasterVolume = 100,
    byte SfxVolume = 100,
    byte MusicVolume = 100,
    bool Muted = false)
{
    /// <summary>Default audio settings.</summary>
    public static ClientAudioSettings Default { get; } = new();

    /// <summary>Returns a copy with volume levels clamped to [0, 100].</summary>
    public ClientAudioSettings Clamp() => new(
        MasterVolume: Math.Clamp(MasterVolume, (byte)0, (byte)100),
        SfxVolume: Math.Clamp(SfxVolume, (byte)0, (byte)100),
        MusicVolume: Math.Clamp(MusicVolume, (byte)0, (byte)100),
        Muted: Muted);
}

/// <summary>
/// Configures accessibility preferences.
/// </summary>
/// <param name="HighContrast">Whether high contrast mode is enabled.</param>
/// <param name="TextScale">Text scaling multiplier between 0.8x and 2.0x.</param>
/// <param name="ReduceMotion">Whether motion animations are reduced.</param>
/// <param name="FocusHighlight">Whether visual focus indicators are highlighted.</param>
public sealed record ClientAccessibilitySettings(
    bool HighContrast = false,
    float TextScale = 1.0f,
    bool ReduceMotion = false,
    bool FocusHighlight = true)
{
    /// <summary>Default accessibility settings.</summary>
    public static ClientAccessibilitySettings Default { get; } = new();

    /// <summary>Returns a copy with text scaling clamped to [0.8, 2.0].</summary>
    public ClientAccessibilitySettings Clamp() => new(
        HighContrast: HighContrast,
        TextScale: Math.Clamp(TextScale, 0.8f, 2.0f),
        ReduceMotion: ReduceMotion,
        FocusHighlight: FocusHighlight);
}

/// <summary>
/// Configures graphics performance settings.
/// </summary>
/// <param name="Tier">Active performance tier.</param>
/// <param name="TargetFps">Target frame rate cap.</param>
/// <param name="VSync">Whether vertical sync is enabled.</param>
/// <param name="ParticleDensity">Particle visual density multiplier from 0.1 to 1.0.</param>
public sealed record ClientPerformanceSettings(
    ClientPerformanceTier Tier = ClientPerformanceTier.High,
    uint TargetFps = 60,
    bool VSync = true,
    float ParticleDensity = 1.0f)
{
    /// <summary>Default performance settings.</summary>
    public static ClientPerformanceSettings Default { get; } = new();

    /// <summary>Returns a copy with valid ranges and enum bounds.</summary>
    public ClientPerformanceSettings Clamp() => new(
        Tier: Enum.IsDefined(Tier) ? Tier : ClientPerformanceTier.High,
        TargetFps: Math.Clamp(TargetFps, 30u, 240u),
        VSync: VSync,
        ParticleDensity: Math.Clamp(ParticleDensity, 0.1f, 1.0f));
}

/// <summary>
/// Root user settings container persisted to disk.
/// </summary>
/// <param name="SchemaVersion">Settings schema version.</param>
/// <param name="PreferredLocale">Active BCP-47 language tag.</param>
/// <param name="Audio">Audio settings.</param>
/// <param name="Accessibility">Accessibility settings.</param>
/// <param name="Performance">Performance settings.</param>
public sealed record ClientUserSettingsContainer(
    uint SchemaVersion = 1,
    string PreferredLocale = "en-US",
    ClientAudioSettings Audio = null!,
    ClientAccessibilitySettings Accessibility = null!,
    ClientPerformanceSettings Performance = null!)
{
    /// <summary>Default user settings container.</summary>
    public static ClientUserSettingsContainer Default { get; } = new(
        SchemaVersion: 1,
        PreferredLocale: "en-US",
        Audio: ClientAudioSettings.Default,
        Accessibility: ClientAccessibilitySettings.Default,
        Performance: ClientPerformanceSettings.Default);

    /// <summary>Returns a copy with non-null clamped sub-containers.</summary>
    public ClientUserSettingsContainer Normalized() => new(
        SchemaVersion: SchemaVersion == 0 ? 1 : SchemaVersion,
        PreferredLocale: string.IsNullOrWhiteSpace(PreferredLocale) ? "en-US" : PreferredLocale.Trim(),
        Audio: (Audio ?? ClientAudioSettings.Default).Clamp(),
        Accessibility: (Accessibility ?? ClientAccessibilitySettings.Default).Clamp(),
        Performance: (Performance ?? ClientPerformanceSettings.Default).Clamp());
}
