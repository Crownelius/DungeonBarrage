using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public sealed class MovementPlaybackTests
{
    private static ClientPlayerSnapshot CreateTestPlayer(
        string id = "player-1",
        int posX = 1000,
        int posY = 2000,
        int radius = 120) =>
        new(
            PlayerId: id,
            Team: 0,
            Health: 280,
            IsEliminated: false,
            MaxHealth: 280,
            Position: new ClientPosition(posX, posY),
            CollisionCenter: new ClientPosition(posX, posY - radius),
            CollisionRadius: radius,
            Loadout: new ClientLoadout("recurve-bow", "shell", "spade", "roost-crown"),
            Ammo: Array.Empty<ClientAmmoCounter>(),
            TrinketCharge: 0,
            Statuses: Array.Empty<ClientStatusSnapshot>(),
            Appearance: new ClientAppearance("default", Array.Empty<string>(), "default"));

    [Fact]
    public void InterpolatePlayer_AtStartTick_MatchesStartPivotsAndPreservesColliderRadius()
    {
        var player = CreateTestPlayer("player-1", 1000, 2000, 120);
        var move = new ClientEntityMovedEvent(
            PresentationTick: 0,
            Sequence: 1,
            PlayerId: "player-1",
            Start: new ClientPosition(1000, 2000),
            End: new ClientPosition(1500, 2200),
            Cause: ClientEntityMovementCause.AuthoritativeResolution);

        var result = MovementPlayback.InterpolatePlayer(player, move, currentTick: 0, lockTicks: 10);

        Assert.Equal(1000, result.Position.X);
        Assert.Equal(2000, result.Position.Y);
        Assert.Equal(1000, result.CollisionCenter.X);
        Assert.Equal(1880, result.CollisionCenter.Y);
        Assert.Equal(120, result.CollisionRadius);

        var geom = CharacterBodyGeometry.FromPlayer(result);
        Assert.Equal(120, geom.Radius);
        Assert.Equal(result.CollisionCenter, geom.Center);
    }

    [Fact]
    public void InterpolatePlayer_AtEndTick_MatchesEndPivotsAndPreservesColliderRadius()
    {
        var player = CreateTestPlayer("player-1", 1000, 2000, 120);
        var move = new ClientEntityMovedEvent(
            PresentationTick: 0,
            Sequence: 1,
            PlayerId: "player-1",
            Start: new ClientPosition(1000, 2000),
            End: new ClientPosition(1500, 2200),
            Cause: ClientEntityMovementCause.AuthoritativeResolution);

        var result = MovementPlayback.InterpolatePlayer(player, move, currentTick: 10, lockTicks: 10);

        Assert.Equal(1500, result.Position.X);
        Assert.Equal(2200, result.Position.Y);
        Assert.Equal(1500, result.CollisionCenter.X);
        Assert.Equal(2080, result.CollisionCenter.Y);
        Assert.Equal(120, result.CollisionRadius);

        var geom = CharacterBodyGeometry.FromPlayer(result);
        Assert.Equal(120, geom.Radius);
        Assert.Equal(result.CollisionCenter, geom.Center);
    }

    [Fact]
    public void InterpolatePlayer_AtMidpoint_SmoothlyLerpsBetweenEndpoints()
    {
        var player = CreateTestPlayer("player-1", 1000, 2000, 100);
        var move = new ClientEntityMovedEvent(
            PresentationTick: 0,
            Sequence: 1,
            PlayerId: "player-1",
            Start: new ClientPosition(1000, 2000),
            End: new ClientPosition(2000, 2000),
            Cause: ClientEntityMovementCause.AuthoritativeResolution);

        var result = MovementPlayback.InterpolatePlayer(player, move, currentTick: 5, lockTicks: 10);

        // At t = 0.5, smoothstep 3(0.5)^2 - 2(0.5)^3 = 0.75 - 0.25 = 0.5, so midpoint is exactly 1500
        Assert.Equal(1500, result.Position.X);
        Assert.Equal(2000, result.Position.Y);
        Assert.Equal(1500, result.CollisionCenter.X);
        Assert.Equal(1900, result.CollisionCenter.Y);
        Assert.Equal(100, result.CollisionRadius);
    }

    [Fact]
    public void InterpolatePlayer_WithReduceMotion_SnapsImmediatelyToEnd()
    {
        var player = CreateTestPlayer("player-1", 1000, 2000, 100);
        var move = new ClientEntityMovedEvent(
            PresentationTick: 0,
            Sequence: 1,
            PlayerId: "player-1",
            Start: new ClientPosition(1000, 2000),
            End: new ClientPosition(2000, 2400),
            Cause: ClientEntityMovementCause.AuthoritativeResolution);

        var result = MovementPlayback.InterpolatePlayer(
            player,
            move,
            currentTick: 2,
            lockTicks: 10,
            reduceMotion: true);

        Assert.Equal(2000, result.Position.X);
        Assert.Equal(2400, result.Position.Y);
        Assert.Equal(2000, result.CollisionCenter.X);
        Assert.Equal(2300, result.CollisionCenter.Y);
    }

    [Fact]
    public void InterpolatePlayer_WithPresentationTickDelay_StaysAtStartUntilImpact()
    {
        var player = CreateTestPlayer("player-1", 1000, 2000, 100);
        var move = new ClientEntityMovedEvent(
            PresentationTick: 10,
            Sequence: 1,
            PlayerId: "player-1",
            Start: new ClientPosition(1000, 2000),
            End: new ClientPosition(1600, 2000),
            Cause: ClientEntityMovementCause.AuthoritativeResolution);

        // Before impact tick: stays at start
        var preImpact = MovementPlayback.InterpolatePlayer(player, move, currentTick: 5, lockTicks: 20);
        Assert.Equal(1000, preImpact.Position.X);

        // At impact tick: still at start
        var atImpact = MovementPlayback.InterpolatePlayer(player, move, currentTick: 10, lockTicks: 20);
        Assert.Equal(1000, atImpact.Position.X);

        // Halfway through knockback: tick 15 of [10..20] is t = 0.5 -> 1300
        var midImpact = MovementPlayback.InterpolatePlayer(player, move, currentTick: 15, lockTicks: 20);
        Assert.Equal(1300, midImpact.Position.X);

        // At end: tick 20
        var postImpact = MovementPlayback.InterpolatePlayer(player, move, currentTick: 20, lockTicks: 20);
        Assert.Equal(1600, postImpact.Position.X);
    }

    [Fact]
    public void FindMovementEvent_ReturnsMatchingPlayerEvent()
    {
        var moveA = new ClientEntityMovedEvent(0, 1, "player-a", new ClientPosition(0, 0), new ClientPosition(10, 0), ClientEntityMovementCause.RequestedMove);
        var moveB = new ClientEntityMovedEvent(0, 2, "player-b", new ClientPosition(50, 0), new ClientPosition(60, 0), ClientEntityMovementCause.RequestedMove);
        var events = new ClientPresentationEvent[] { moveA, moveB };

        Assert.Same(moveA, MovementPlayback.FindMovementEvent(events, "player-a"));
        Assert.Same(moveB, MovementPlayback.FindMovementEvent(events, "player-b"));
        Assert.Null(MovementPlayback.FindMovementEvent(events, "player-c"));
    }
}
