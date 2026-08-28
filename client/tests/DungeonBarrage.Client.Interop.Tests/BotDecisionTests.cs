using System.Diagnostics.CodeAnalysis;
using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using DungeonBarrage.Client.Match;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// <c>db_sim_match_bot_decide</c> through the real native library — both the raw session call
/// and <see cref="LiveMatch.SubmitBotDecisionAsync"/>'s full decide-then-submit round trip.
/// </summary>
public sealed class BotDecisionTests
{
    [Fact]
    public async Task Deciding_for_the_active_player_returns_a_well_formed_action()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        var request = new ClientBotDecisionRequest(1, "a-local-player", ClientBotDifficulty.Standard, 1);
        var requestBytes = JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options);
        var responseBytes = await session.DecideBotActionAsync(requestBytes);

        var decision = JsonSerializer.Deserialize<ClientBotDecision>(responseBytes, ClientEnvelope.Options);
        Assert.NotNull(decision);
        Assert.Equal(1u, decision.SchemaVersion);
        Assert.True(
            decision is ClientBotMoveDecision or ClientBotAbilityDecision or ClientBotPassiveChoiceDecision
                or ClientBotPassDecision,
            $"unexpected decision type: {decision.GetType()}");
    }

    [Fact]
    public async Task Deciding_does_not_mutate_the_session()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);
        var before = await session.SnapshotAsync();

        var request = new ClientBotDecisionRequest(1, "a-local-player", ClientBotDifficulty.Casual, 1);
        var requestBytes = JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options);
        for (var i = 0; i < 5; i++)
        {
            await session.DecideBotActionAsync(requestBytes);
        }

        var after = await session.SnapshotAsync();
        Assert.Equal(before, after);
    }

    [Fact]
    public async Task A_bot_playing_both_sides_completes_the_match_with_no_rejections()
    {
        // Zeke (a-local-player) has no melee ability at all — both of his are ranged — while
        // Huck (b-local-bot) has none but melee, so a bot playing both sides exercises the
        // grid-search and melee-closing paths in the same run.
        await using var live = await CreateLiveAsync();
        var seed = 1UL;
        var turns = 0;

        while (live.CurrentSnapshot.Outcome is ClientInProgressOutcome && turns < 400)
        {
            turns++;
            var transition = await live.SubmitBotDecisionAsync(ClientBotDifficulty.Standard, seed++);
            Assert.Equal(ClientTransitionDisposition.Accepted, transition.Disposition);
        }

        Assert.True(turns < 400, "the match must reach a terminal state well inside the cap");
        Assert.IsNotType<ClientInProgressOutcome>(live.CurrentSnapshot.Outcome);
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
        var createResponse = JsonSerializer.Deserialize<ClientCreateResponse>(
            session.CreateResponse.Span, ClientEnvelope.Options)!;
        var terrain = await session.TerrainAsync(ulong.MaxValue);
        return LiveMatch.Create(session, createResponse.Snapshot!, terrain, "test");
    }
}
