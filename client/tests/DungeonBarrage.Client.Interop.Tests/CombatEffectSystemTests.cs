using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public sealed class CombatEffectSystemTests
{
    [Fact]
    public void TargetMarker_is_always_spawned_even_on_low_tier_and_reduced_motion()
    {
        var effects = new CombatEffectSystem();
        var impactPos = new PresentationPoint(250f, 150f);

        effects.SpawnImpact(impactPos, cellSize: 12f, tier: ClientPerformanceTier.Low, reduceMotion: true);

        Assert.Single(effects.ActiveTargetMarkers);
        Assert.Equal(impactPos.X, effects.ActiveTargetMarkers[0].Position.X);
        Assert.Equal(impactPos.Y, effects.ActiveTargetMarkers[0].Position.Y);
        Assert.Empty(effects.ActiveParticles);
        Assert.Single(effects.ActiveShockwaves);
    }

    [Fact]
    public void Particle_count_scales_across_performance_tiers()
    {
        var lowEffects = new CombatEffectSystem();
        lowEffects.SpawnImpact(new PresentationPoint(100f, 100f), 12f, ClientPerformanceTier.Low, reduceMotion: false);
        Assert.Empty(lowEffects.ActiveParticles);

        var medEffects = new CombatEffectSystem();
        medEffects.SpawnImpact(new PresentationPoint(100f, 100f), 12f, ClientPerformanceTier.Medium, reduceMotion: false);
        Assert.Equal(6, medEffects.ActiveParticles.Count);

        var highEffects = new CombatEffectSystem();
        highEffects.SpawnImpact(new PresentationPoint(100f, 100f), 12f, ClientPerformanceTier.High, reduceMotion: false);
        Assert.Equal(14, highEffects.ActiveParticles.Count);
        Assert.Equal(2, highEffects.ActiveShockwaves.Count);
    }

    [Fact]
    public void Shockwave_expands_over_time_and_expires()
    {
        var effects = new CombatEffectSystem();
        effects.SpawnImpact(new PresentationPoint(100f, 100f), 12f, ClientPerformanceTier.Medium, reduceMotion: false);

        var ring = effects.ActiveShockwaves[0];
        Assert.Equal(ring.StartRadius, ring.CurrentRadius);

        effects.Update(0.2f);
        Assert.True(ring.CurrentRadius > ring.StartRadius);
        Assert.True(ring.Alpha < 1f);

        // Advance past lifetime
        effects.Update(0.5f);
        Assert.Empty(effects.ActiveShockwaves);
    }

    [Fact]
    public void Bounded_capacity_prevents_uncontrolled_pool_growth()
    {
        var effects = new CombatEffectSystem();

        // Spawn 100 impacts rapidly
        for (var i = 0; i < 100; i++)
        {
            effects.SpawnImpact(new PresentationPoint(100f + i, 100f), 12f, ClientPerformanceTier.High, reduceMotion: false);
        }

        Assert.True(effects.ActiveParticles.Count <= 256);
        Assert.True(effects.ActiveShockwaves.Count <= 32);
        Assert.True(effects.ActiveTargetMarkers.Count <= 16);

        // Advance time until all effects expire
        effects.Update(2.0f);
        Assert.Empty(effects.ActiveParticles);
        Assert.Empty(effects.ActiveShockwaves);
        Assert.Empty(effects.ActiveTargetMarkers);
    }

    [Fact]
    public void Clear_removes_all_active_effects()
    {
        var effects = new CombatEffectSystem();
        effects.SpawnImpact(new PresentationPoint(100f, 100f), 12f, ClientPerformanceTier.High, reduceMotion: false);
        effects.SpawnMuzzleFire(new PresentationPoint(50f, 50f), 1f, 12f, ClientPerformanceTier.High, reduceMotion: false);
        effects.SpawnHitSparks(new PresentationPoint(200f, 200f), 12f, ClientPerformanceTier.High, reduceMotion: false);

        Assert.NotEmpty(effects.ActiveParticles);
        Assert.NotEmpty(effects.ActiveShockwaves);
        Assert.NotEmpty(effects.ActiveTargetMarkers);

        effects.Clear();

        Assert.Empty(effects.ActiveParticles);
        Assert.Empty(effects.ActiveShockwaves);
        Assert.Empty(effects.ActiveTargetMarkers);
    }
}
