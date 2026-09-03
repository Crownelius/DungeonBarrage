using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public sealed class CameraDirectorTests
{
    [Fact]
    public void Default_state_has_no_pan_and_zero_effective_offset()
    {
        var camera = new CameraDirector();
        Assert.False(camera.HasManualPan);
        Assert.Equal(0f, camera.EffectiveOffset.X);
        Assert.Equal(0f, camera.EffectiveOffset.Y);
        Assert.Equal(CameraMode.Manual, camera.Mode);
    }

    [Fact]
    public void Panning_modifies_manual_offset_within_arena_limits()
    {
        var camera = new CameraDirector();
        camera.UpdateArenaLimits(arenaWidthPixels: 768f, arenaHeightPixels: 360f, viewportWidth: 1280f, viewportHeight: 720f);

        camera.Pan(50f, -30f);
        Assert.True(camera.HasManualPan);
        Assert.Equal(50f, camera.ManualOffset.X);
        Assert.Equal(-30f, camera.ManualOffset.Y);

        // Pan beyond limit clamps
        camera.Pan(2000f, 2000f);
        Assert.InRange(camera.ManualOffset.X, 400f, 800f);
        Assert.InRange(camera.ManualOffset.Y, 200f, 600f);
    }

    [Fact]
    public void SetImpulse_composes_with_manual_offset()
    {
        var camera = new CameraDirector();
        camera.Pan(100f, 50f);

        var impulse = new PresentationCameraImpulse(CellX: 2f, CellY: -1f);
        camera.SetImpulse(impulse, cellSize: 12f);

        Assert.Equal(24f, camera.ImpulseOffset.X);
        Assert.Equal(-12f, camera.ImpulseOffset.Y);

        Assert.Equal(124f, camera.EffectiveOffset.X);
        Assert.Equal(38f, camera.EffectiveOffset.Y);
    }

    [Fact]
    public void TrackPlayback_shifts_camera_when_projectile_leaves_deadzone()
    {
        var camera = new CameraDirector();
        camera.UpdateArenaLimits(arenaWidthPixels: 1000f, arenaHeightPixels: 600f, viewportWidth: 1280f, viewportHeight: 720f);

        // Projectile inside center deadzone produces no shift
        camera.TrackPlayback(new PresentationPoint(640f, 360f), viewportWidth: 1280f, viewportHeight: 720f);
        Assert.Equal(CameraMode.TrackPlayback, camera.Mode);
        Assert.Equal(0f, camera.PlaybackOffset.X);
        Assert.Equal(0f, camera.PlaybackOffset.Y);

        // Projectile far to the right shifts camera to the left (negative shift)
        camera.TrackPlayback(new PresentationPoint(1200f, 360f), viewportWidth: 1280f, viewportHeight: 720f);
        Assert.True(camera.PlaybackOffset.X < 0f);

        // Inactive projectile resets playback tracking to manual
        camera.TrackPlayback(null, viewportWidth: 1280f, viewportHeight: 720f);
        Assert.Equal(CameraMode.Manual, camera.Mode);
        Assert.Equal(0f, camera.PlaybackOffset.X);
    }

    [Fact]
    public void Reset_restores_clean_zero_offset()
    {
        var camera = new CameraDirector();
        camera.Pan(150f, -80f);
        camera.SetImpulse(new PresentationCameraImpulse(1f, 1f), 12f);
        camera.Reset();

        Assert.False(camera.HasManualPan);
        Assert.Equal(0f, camera.EffectiveOffset.X);
        Assert.Equal(0f, camera.EffectiveOffset.Y);
        Assert.Equal(CameraMode.Manual, camera.Mode);
    }
}
