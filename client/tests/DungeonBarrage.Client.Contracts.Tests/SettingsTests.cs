using DungeonBarrage.Client.Contracts;
using Xunit;

namespace DungeonBarrage.Client.Contracts.Tests;

public class SettingsTests
{
    [Fact]
    public void AudioSettings_ClampsVolumeTo100()
    {
        var invalid = new ClientAudioSettings(MasterVolume: 255, SfxVolume: 150, MusicVolume: 80);
        var clamped = invalid.Clamp();

        Assert.Equal((byte)100, clamped.MasterVolume);
        Assert.Equal((byte)100, clamped.SfxVolume);
        Assert.Equal((byte)80, clamped.MusicVolume);
    }

    [Fact]
    public void AccessibilitySettings_ClampsTextScaleToBounds()
    {
        var invalidLow = new ClientAccessibilitySettings(TextScale: 0.2f);
        var invalidHigh = new ClientAccessibilitySettings(TextScale: 3.5f);

        Assert.Equal(0.8f, invalidLow.Clamp().TextScale);
        Assert.Equal(2.0f, invalidHigh.Clamp().TextScale);
    }

    [Fact]
    public void PerformanceSettings_ClampsFpsAndParticles()
    {
        var invalid = new ClientPerformanceSettings(TargetFps: 10, ParticleDensity: 2.5f);
        var clamped = invalid.Clamp();

        Assert.Equal(30u, clamped.TargetFps);
        Assert.Equal(1.0f, clamped.ParticleDensity);
    }
}
