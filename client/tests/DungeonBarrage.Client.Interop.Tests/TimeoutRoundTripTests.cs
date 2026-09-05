using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// <see cref="ClientAuthorityTimeout"/> DTOs, applied through the real release native library via
/// <see cref="LocalMatchSession.TimeoutAsync"/>.
/// </summary>
/// <remarks>
/// The strong check for a timeout claim: unlike an ordinary command, it must never be reachable
/// through <see cref="LocalMatchSession.ApplyAsync"/> at all, so this also proves that boundary
/// holds against the real native parser rather than just against a doc comment.
/// </remarks>
public sealed class TimeoutRoundTripTests
{
    [Fact]
    public async Task A_timeout_built_from_the_dto_ends_the_active_players_turn()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        var timeout = ClientAuthorityTimeout.Create(
            actionId: "fixture-timeout-001",
            playerId: "a-local-player",
            expectedTurnNumber: 1,
            expectedSnapshotGeneration: 0);
        var timeoutJson = JsonSerializer.SerializeToUtf8Bytes(timeout, ClientEnvelope.Options);

        var response = await session.TimeoutAsync(timeoutJson);

        using var document = JsonDocument.Parse(response);
        var root = document.RootElement;
        Assert.Equal("accepted", root.GetProperty("disposition").GetString());
        var events = root.GetProperty("events").EnumerateArray();
        Assert.Contains(
            events,
            element => element.GetProperty("kind").GetString() == "turnEnded"
                && element.GetProperty("reason").GetString() == "timedOut");
        Assert.Equal(
            "b-local-bot",
            root.GetProperty("postSnapshot").GetProperty("activePlayerId").GetString());
    }

    [Fact]
    public async Task A_timeout_shaped_payload_is_refused_by_apply_not_silently_accepted()
    {
        // The whole point of AuthorityTimeout being a distinct native export, rather than a
        // MatchCommandKind variant, is structural: no client-decodable command JSON can reach it.
        // Proves that against the real native command parser, not just by reading the doc comment.
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        var timeout = ClientAuthorityTimeout.Create(
            actionId: "fixture-timeout-001",
            playerId: "a-local-player",
            expectedTurnNumber: 1,
            expectedSnapshotGeneration: 0);
        var timeoutJson = JsonSerializer.SerializeToUtf8Bytes(timeout, ClientEnvelope.Options);

        var error = await Assert.ThrowsAsync<NativeSimulationException>(
            () => session.ApplyAsync(timeoutJson));

        Assert.Equal(NativeStatus.MalformedEnvelope, error.Status);
    }
}
