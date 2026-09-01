using System.Text.Json;
using Xunit;

namespace DungeonBarrage.Client.Contracts.Tests;

/// <summary>Exercises every response frozen from the production Rust serializer.</summary>
public sealed class FrozenResponseFixtureTests
{
    [Fact]
    public void Create_response_decodes_the_real_initial_snapshot()
    {
        var response = Deserialize<ClientCreateResponse>("create.json");

        Assert.True(response.Created);
        Assert.Null(response.Diagnostic);
        var snapshot = Assert.IsType<ClientMatchSnapshot>(response.Snapshot);
        Assert.Equal(1U, response.SchemaVersion);
        Assert.Equal("fixture-horizontal-duel-v1", snapshot.MatchId);
        Assert.Equal(1024, snapshot.PositionScale);
        Assert.Equal(60U, snapshot.FixedTickRate);
        Assert.False(string.IsNullOrEmpty(snapshot.StateHash));
        Assert.NotEqual("f67c5371bcddbdf5", snapshot.StateHash);
        Assert.IsType<ClientInProgressOutcome>(snapshot.Outcome);
        Assert.Equal(ClientMatchPhase.Movement, snapshot.Phase);
        Assert.Equal(8, snapshot.Blocks.Count);
        Assert.Equal(2, snapshot.Players.Count);
    }

    [Fact]
    public void Standalone_snapshot_decodes_every_nested_collection()
    {
        var snapshot = Deserialize<ClientMatchSnapshot>("snapshot-initial.json");

        // ABI version 4: db_sim_match_timeout's addition (docs/BUILD_LOG.md's C6 entry).
        Assert.Equal(4U, snapshot.AbiVersion);
        Assert.Equal(7U, snapshot.SimulationVersion);
        Assert.Equal(4U, snapshot.ContentVersion);
        Assert.Equal(1024, snapshot.PositionScale);
        Assert.Equal(60U, snapshot.FixedTickRate);
        Assert.Equal("a-local-player", snapshot.ActivePlayerId);
        Assert.Null(snapshot.InputOpensAt);
        Assert.Null(snapshot.DeadlineAt);
        Assert.All(snapshot.Blocks, block => Assert.Equal(ClientMaterial.Soil, block.Material));
        Assert.All(snapshot.Blocks, block => Assert.Equal(ClientErosionAxis.Columns, block.ErosionAxis));
        Assert.Equal("ramshot-cannon", snapshot.Players[0].Loadout.Main);
        Assert.Equal(new ClientPosition(2048, 7936), snapshot.Players[0].Position);
        Assert.Empty(snapshot.PersistentObjects);
    }

    [Fact]
    public void Preview_response_decodes_the_real_trace()
    {
        var preview = Deserialize<ClientAbilityPreviewResponse>("preview-basic.json");

        Assert.True(preview.Legal);
        Assert.Null(preview.RejectionReason);
        Assert.Equal(0, preview.GaugeCost);
        Assert.Equal(["a-local-player", "b-local-bot"], preview.LegalTargetPlayerIds);
        var trace = Assert.Single(preview.ProjectileTraces);
        Assert.Equal("ramshot-cannon", trace.AbilityId);
        Assert.Equal(6, trace.Samples.Count);
        Assert.Equal(ClientImpactCause.Character, trace.TerminalImpact.Cause);
    }

    [Fact]
    public void Move_transition_decodes_the_flattened_event_and_post_snapshot()
    {
        var transition = Deserialize<ClientMatchTransition>("001-move.json");

        Assert.Equal(ClientTransitionDisposition.Accepted, transition.Disposition);
        Assert.Null(transition.RejectionReason);
        Assert.Equal(0UL, transition.PreSnapshotGeneration);
        Assert.Equal(1UL, transition.PostSnapshotGeneration);
        var movement = Assert.IsType<ClientEntityMovedEvent>(Assert.Single(transition.Events));
        Assert.Equal("a-local-player", movement.PlayerId);
        Assert.Equal(ClientEntityMovementCause.AuthoritativeResolution, movement.Cause);
        Assert.Equal(new ClientPosition(2048, 7936), movement.Start);
        Assert.Equal(new ClientPosition(3072, 7936), movement.End);
        Assert.Equal("4610d8c64f1670b9", transition.PostSnapshot.StateHash);
        Assert.Equal(transition.PostSnapshot.StateHash, transition.PostStateHash);
    }

    [Fact]
    public void Ability_transition_decodes_every_frozen_event_variant()
    {
        var transition = Deserialize<ClientMatchTransition>("002-ability.json");

        Assert.IsType<ClientProjectileTraceEvent>(transition.Events[0]);
        Assert.IsType<ClientImpactEvent>(transition.Events[1]);
        Assert.Contains(transition.Events, e => e is ClientStrikeResolvedEvent);
        Assert.Contains(transition.Events, e => e is ClientPlayerEliminatedEvent);
        Assert.Contains(transition.Events, e => e is ClientMatchCompletedEvent);
        Assert.Contains(transition.Events, e => e is ClientTurnEndedEvent);
        Assert.Equal("1e5dff46164b909b", transition.PostStateHash);
        Assert.Equal(ClientMatchPhase.MatchComplete, transition.PostSnapshot.Phase);
    }

    private static T Deserialize<T>(string fileName)
        where T : class
    {
        return JsonSerializer.Deserialize<T>(Fixtures.Read(fileName).Span, ClientEnvelope.Options)
            ?? throw new InvalidOperationException($"Fixture '{fileName}' decoded to null.");
    }
}
