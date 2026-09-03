using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// The visible body, authoritative character collider, and projectile-impact playback share one
/// geometry contract without requiring Godot on the test runner.
/// </summary>
public sealed class CharacterGeometryTests
{
    [Fact]
    public void Containment_includes_the_circle_boundary_and_rejects_the_old_invisible_area()
    {
        var body = new CharacterBodyGeometry(new ClientPosition(100, 200), 50);

        Assert.True(body.Contains(new ClientPosition(100, 200)));
        Assert.True(body.Contains(new ClientPosition(130, 240)));
        Assert.True(body.Contains(new ClientPosition(150, 200)));
        Assert.False(body.Contains(new ClientPosition(151, 200)));
        Assert.False(body.Contains(new ClientPosition(140, 240)));
        Assert.False(body.Contains(new ClientPosition(int.MaxValue, int.MinValue)));
    }

    [Fact]
    public void Projection_preserves_the_authoritative_center_radius_and_boundary()
    {
        var body = new CharacterBodyGeometry(new ClientPosition(2048, 4096), 1024);
        var worldOrigin = new PresentationPoint(10, 20);
        var cameraOffset = new PresentationPoint(-2, 3);

        var projected = body.Project(
            positionScale: 1024,
            pixelsPerCell: 16,
            worldOrigin,
            cameraOffset);
        var boundary = WorldProjection.ToPresentation(
            new ClientPosition(3072, 4096),
            positionScale: 1024,
            pixelsPerCell: 16,
            worldOrigin,
            cameraOffset);
        var outside = WorldProjection.ToPresentation(
            new ClientPosition(3073, 4096),
            positionScale: 1024,
            pixelsPerCell: 16,
            worldOrigin,
            cameraOffset);

        Assert.Equal(new PresentationPoint(40, 87), projected.Center);
        Assert.Equal(16, projected.Radius);
        Assert.True(projected.Contains(boundary));
        Assert.False(projected.Contains(outside));
    }

    [Theory]
    [InlineData(0, 12f)]
    [InlineData(-1, 12f)]
    [InlineData(1024, 0f)]
    [InlineData(1024, -1f)]
    public void Projection_rejects_invalid_scales(int positionScale, float pixelsPerCell)
    {
        var body = new CharacterBodyGeometry(new ClientPosition(0, 0), 10);

        Assert.Throws<ArgumentOutOfRangeException>(() => body.Project(
            positionScale,
            pixelsPerCell,
            default,
            default));
    }

    [Fact]
    public void Projection_rejects_non_finite_scale_and_non_positive_radius()
    {
        var body = new CharacterBodyGeometry(new ClientPosition(0, 0), 10);
        var invalidBody = new CharacterBodyGeometry(new ClientPosition(0, 0), 0);

        Assert.Throws<ArgumentOutOfRangeException>(() => body.Project(1024, float.NaN, default, default));
        Assert.Throws<ArgumentOutOfRangeException>(() => body.Project(1024, float.PositiveInfinity, default, default));
        Assert.Throws<InvalidDataException>(() => invalidBody.Project(1024, 12, default, default));
    }

    [Fact]
    public void Frozen_preview_character_impact_overlaps_the_visible_target_body()
    {
        var snapshot = Deserialize<ClientMatchSnapshot>("responses/snapshot-initial.json");
        var preview = Deserialize<ClientAbilityPreviewResponse>("responses/preview-basic.json");
        var target = Assert.Single(snapshot.Players, player => player.PlayerId == "b-local-bot");
        var trace = Assert.Single(preview.ProjectileTraces);

        Assert.Equal(ClientImpactCause.Character, trace.TerminalImpact.Cause);
        AssertColliderAnchoredToGroundPivot(target);
        Assert.True(
            CharacterBodyGeometry.FromPlayer(target).Contains(trace.TerminalImpact.Position),
            $"Character impact {trace.TerminalImpact.Position} is outside target " +
            $"center {target.CollisionCenter}, radius {target.CollisionRadius}.");
    }

    [Fact]
    public void Frozen_applied_character_impact_overlaps_the_struck_players_visible_body()
    {
        var move = Deserialize<ClientMatchTransition>("responses/001-move.json");
        var ability = Deserialize<ClientMatchTransition>("responses/002-ability.json");
        var traceEvent = Assert.Single(ability.Events.OfType<ClientProjectileTraceEvent>());
        var strikeEvent = Assert.Single(ability.Events.OfType<ClientStrikeResolvedEvent>());
        var target = Assert.Single(
            move.PostSnapshot.Players,
            player => player.PlayerId == strikeEvent.Strike.TargetPlayerId);

        Assert.Equal(ClientImpactCause.Character, traceEvent.Trace.TerminalImpact.Cause);
        Assert.Equal(traceEvent.Trace.TerminalImpact.Position, strikeEvent.Strike.ImpactPoint);
        AssertColliderAnchoredToGroundPivot(target);
        Assert.True(
            CharacterBodyGeometry.FromPlayer(target).Contains(traceEvent.Trace.TerminalImpact.Position),
            $"Character impact {traceEvent.Trace.TerminalImpact.Position} is outside struck player " +
            $"center {target.CollisionCenter}, radius {target.CollisionRadius}.");
    }

    private static void AssertColliderAnchoredToGroundPivot(ClientPlayerSnapshot player)
    {
        Assert.True(player.CollisionRadius > 0);
        Assert.Equal(player.Position.X, player.CollisionCenter.X);
        Assert.Equal(player.Position.Y - player.CollisionRadius, player.CollisionCenter.Y);
    }

    private static T Deserialize<T>(string relativePath)
        where T : class =>
        JsonSerializer.Deserialize<T>(Fixtures.Read(relativePath).Span, ClientEnvelope.Options)
        ?? throw new InvalidDataException($"Fixture '{relativePath}' decoded to null.");
}
