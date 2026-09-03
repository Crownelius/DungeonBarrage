using System.Diagnostics.CodeAnalysis;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using DungeonBarrage.Client.Match;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// <see cref="LiveMatch.PlanningDeadlineUtc"/> and <see cref="LiveMatch.SubmitTimeoutAsync"/>
/// against the real native library — the client-owned local planning clock CLIENT_SPEC.md §9.1
/// describes, layered on the native <c>db_sim_match_timeout</c> export <c>TimeoutRoundTripTests</c>
/// proves directly.
/// </summary>
public sealed class PlanningDeadlineTests
{
    [Fact]
    public async Task A_deadline_is_armed_as_soon_as_a_match_starts_in_an_actionable_phase()
    {
        await using var live = await CreateLiveAsync();

        Assert.NotNull(live.PlanningDeadlineUtc);
        Assert.Equal(ClientMatchPhase.Movement, live.CurrentSnapshot.Phase);
        var remaining = live.PlanningDeadlineUtc!.Value - DateTimeOffset.UtcNow;
        Assert.True(remaining > TimeSpan.Zero && remaining <= LiveMatch.DefaultPlanningDeadline);
    }

    [Fact]
    public async Task The_deadline_does_not_reset_between_a_move_and_a_follow_up_ability_in_the_same_turn()
    {
        // One deadline governs the whole turn, not each sub-step of it: a move and its follow-up
        // ability share one (playerId, turnNumber) pair, so re-arming between them would let a
        // player silently reset their own clock by acting.
        await using var live = await CreateLiveAsync();
        var deadlineAfterCreate = live.PlanningDeadlineUtc;

        _ = await live.SubmitMoveAsync(1024);

        Assert.Equal(deadlineAfterCreate, live.PlanningDeadlineUtc);
    }

    [Fact]
    public async Task The_deadline_re_arms_for_the_next_player_once_a_turn_hands_over()
    {
        // A playable map, not the C2 wire fixture: that fixture's own lob ends the match on
        // turn 1, so no turn ever hands over there and this assertion could not be made.
        await using var live = await CreateLiveOnAsync("crow-perch");
        var deadlineForFirstTurn = live.PlanningDeadlineUtc;

        _ = await live.SubmitMoveAsync(1024);
        _ = await live.SubmitAbilityAsync(ClientAbilitySlot.Main, 45_000, 1_500, null);

        Assert.Equal("b-local-bot", live.CurrentSnapshot.ActivePlayerId);
        Assert.NotNull(live.PlanningDeadlineUtc);
        Assert.NotEqual(deadlineForFirstTurn, live.PlanningDeadlineUtc);
    }

    [Fact]
    public async Task SubmitTimeoutAsync_ends_the_active_players_turn_through_the_real_native_library()
    {
        await using var live = await CreateLiveAsync();

        var transition = await live.SubmitTimeoutAsync();

        Assert.Equal(ClientTransitionDisposition.Accepted, transition.Disposition);
        Assert.Contains(transition.Events, e => e is ClientTurnEndedEvent ended && ended.Reason == ClientTurnEndReason.TimedOut);
        Assert.Equal("b-local-bot", live.CurrentSnapshot.ActivePlayerId);
        Assert.Equal(2u, live.CurrentSnapshot.TurnNumber);

        // The C5 reconciliation rule applies here exactly as it does to an ordinary command: the
        // view's state is exactly what the authority returned, never a value this class predicted.
        Assert.Equal(transition.PostSnapshot.StateHash, live.CurrentSnapshot.StateHash);
    }

    [SuppressMessage(
        "Reliability",
        "CA2000:Dispose objects before losing scope",
        Justification =
            "Ownership transfers to the returned LiveMatch, which every caller disposes via " +
            "'await using'.")]
    private static async Task<LiveMatch> CreateLiveAsync()
    {
        var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);
        var createResponse = System.Text.Json.JsonSerializer.Deserialize<ClientCreateResponse>(
            session.CreateResponse.Span, ClientEnvelope.Options)!;
        var terrain = await session.TerrainAsync(ulong.MaxValue);
        return LiveMatch.Create(session, createResponse.Snapshot!, terrain, "test");
    }

    [SuppressMessage(
        "Reliability",
        "CA2000:Dispose objects before losing scope",
        Justification =
            "Ownership transfers to the returned LiveMatch, which every caller disposes via " +
            "'await using'.")]
    private static async Task<LiveMatch> CreateLiveOnAsync(string mapId)
    {
        var request = LocalMatchEnvelope.HumanVsBot(
            LocalMatchSession.SimulationVersion,
            LocalMatchSession.ContentVersion,
            seed: 12345,
            matchId: "test-match",
            mapId: mapId,
            humanLoadout: LocalMatchEnvelope.LaunchDefaultLoadout);
        var session = LocalMatchSession.Create(
            System.Text.Json.JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options));
        var createResponse = System.Text.Json.JsonSerializer.Deserialize<ClientCreateResponse>(
            session.CreateResponse.Span, ClientEnvelope.Options)!;
        var terrain = await session.TerrainAsync(ulong.MaxValue);
        return LiveMatch.Create(session, createResponse.Snapshot!, terrain, "test");
    }
}
