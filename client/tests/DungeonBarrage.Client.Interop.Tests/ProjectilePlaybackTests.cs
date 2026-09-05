using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>Projectile views follow one trace's samples. They do not jump to the impact.</summary>
public sealed class ProjectilePlaybackTests
{
    [Fact]
    public void Mid_flight_is_the_linear_midpoint_of_the_surrounding_samples()
    {
        var trace = new ClientProjectileTrace(
            TraceId: 0,
            OwnerId: "a-local-player",
            AbilityId: "ramshot-cannon",
            Samples:
            [
                new ClientProjectileSample(0, new ClientPosition(0, 0)),
                new ClientProjectileSample(10, new ClientPosition(1000, 400)),
            ],
            TerminalImpact: new ClientImpact(new ClientPosition(1000, 400), 10, ClientImpactCause.Terrain));

        var mid = ProjectilePlayback.PositionAt(trace, 5);
        Assert.NotNull(mid);
        Assert.Equal(new ClientPosition(500, 200), mid);
    }

    [Fact]
    public void After_the_last_sample_the_view_stays_on_the_terminal_point()
    {
        var trace = new ClientProjectileTrace(
            TraceId: 0,
            OwnerId: "a-local-player",
            AbilityId: "ramshot-cannon",
            Samples:
            [
                new ClientProjectileSample(0, new ClientPosition(0, 0)),
                new ClientProjectileSample(4, new ClientPosition(40, 8)),
            ],
            TerminalImpact: new ClientImpact(new ClientPosition(40, 8), 4, ClientImpactCause.Character));

        Assert.Equal(new ClientPosition(40, 8), ProjectilePlayback.PositionAt(trace, 4));
        Assert.Equal(new ClientPosition(40, 8), ProjectilePlayback.PositionAt(trace, 99));
    }

    [Fact]
    public void TickAt_does_not_run_past_the_lock_window()
    {
        Assert.Equal(0u, ProjectilePlayback.TickAt(0, 60, 9));
        Assert.Equal(6u, ProjectilePlayback.TickAt(100, 60, 9));
        Assert.Equal(9u, ProjectilePlayback.TickAt(10_000, 60, 9));
    }

    [Fact]
    public void A_short_flight_is_slowed_so_the_hit_is_watchable()
    {
        var rate = ProjectilePlayback.VisualTickRate(lastSampleTick: 30, authorityTickRate: 60);
        Assert.True(rate < 60, "a 0.5s sim flight must play slower than 60 Hz");
        var msec = ProjectilePlayback.PlaybackMsec(30, rate);
        Assert.True(msec >= ProjectilePlayback.MinimumFlightMsec + ProjectilePlayback.ImpactHoldMsec);
    }

    [Fact]
    public void A_returning_weapon_keeps_the_authoritative_hit_and_appends_a_catch_leg()
    {
        Assert.True(ProjectilePlayback.IsReturningWeapon("returning-boomerang"));
        Assert.False(ProjectilePlayback.IsReturningWeapon("ramshot-cannon"));

        var outbound = new ClientProjectileTrace(
            TraceId: 0,
            OwnerId: "a-local-player",
            AbilityId: "returning-boomerang",
            Samples:
            [
                new ClientProjectileSample(0, new ClientPosition(0, 0)),
                new ClientProjectileSample(20, new ClientPosition(2000, 400)),
            ],
            TerminalImpact: new ClientImpact(new ClientPosition(2000, 400), 20, ClientImpactCause.Character));

        var returned = ProjectilePlayback.WithReturnTo(
            outbound,
            catcher: new ClientPosition(0, 0),
            ProjectilePlayback.ReturnLegTicks);

        Assert.Equal(20u, returned.TerminalImpact.Tick);
        Assert.Equal(new ClientPosition(2000, 400), returned.TerminalImpact.Position);
        var last = returned.Samples[returned.Samples.Count - 1];
        Assert.Equal(20u + ProjectilePlayback.ReturnLegTicks, last.Tick);
        Assert.Equal(new ClientPosition(0, 0), last.Position);
        Assert.Equal(new ClientPosition(2000, 400), ProjectilePlayback.PositionAt(returned, 20));
    }
}
