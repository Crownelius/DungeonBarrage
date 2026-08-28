using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// Command DTOs built by <see cref="ClientMatchCommand"/>, applied through the real release
/// native library.
/// </summary>
/// <remarks>
/// <see cref="CommandContractTests"/> in the Contracts project compares field values against the
/// frozen command fixtures, which is the strongest check available without a native dependency.
/// This is the stronger check: it proves the JSON these DTOs actually produce is accepted by the
/// real `serde_json`-backed native parser and drives the match to the exact frozen outcome — the
/// thing that ultimately matters about a command DTO, not that its bytes happen to match a file.
/// </remarks>
public sealed class CommandRoundTripTests
{
    [Fact]
    public async Task A_move_command_built_from_the_dto_reaches_the_frozen_post_state()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        var command = ClientMatchCommand.Move(
            commandId: "fixture-move-001",
            playerId: "a-local-player",
            expectedTurnNumber: 1,
            expectedSnapshotGeneration: 0,
            dx: 1024);
        var commandJson = JsonSerializer.SerializeToUtf8Bytes(command, ClientEnvelope.Options);

        var response = await session.ApplyAsync(commandJson);

        using var document = JsonDocument.Parse(response);
        var root = document.RootElement;
        Assert.Equal("accepted", root.GetProperty("disposition").GetString());
        Assert.Equal("378081bb2e830a5d", root.GetProperty("postStateHash").GetString());
    }

    [Fact]
    public async Task An_ability_command_built_from_the_dto_reaches_the_frozen_post_state()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        var move = ClientMatchCommand.Move("fixture-move-001", "a-local-player", 1, 0, dx: 1024);
        _ = await session.ApplyAsync(JsonSerializer.SerializeToUtf8Bytes(move, ClientEnvelope.Options));

        var ability = ClientMatchCommand.Ability(
            commandId: "fixture-ability-002",
            playerId: "a-local-player",
            expectedTurnNumber: 1,
            expectedSnapshotGeneration: 1,
            slot: ClientAbilitySlot.Basic,
            angleMillidegrees: 45_000,
            powerBasisPoints: 1_500,
            targetPlayerId: null,
            secondaryTargetPlayerId: null);
        var response = await session.ApplyAsync(JsonSerializer.SerializeToUtf8Bytes(ability, ClientEnvelope.Options));

        using var document = JsonDocument.Parse(response);
        var root = document.RootElement;
        Assert.Equal("accepted", root.GetProperty("disposition").GetString());
        Assert.Equal("d8686762470c0c36", root.GetProperty("postStateHash").GetString());
    }

    [Fact]
    public async Task A_malformed_kind_is_refused_at_the_native_boundary_not_silently_reinterpreted()
    {
        // The native side's deny_unknown_fields contract is only meaningful if a genuinely wrong
        // shape is actually refused. Hand-authoring the one malformed case CommandContractTests
        // cannot express through the typed builders: an ability-only field sent on a move.
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);
        var malformed = LocalMatchSession.Utf8(
            "{\"schemaVersion\":1,\"commandId\":\"bad-1\",\"playerId\":\"a-local-player\"," +
            "\"expectedTurnNumber\":1,\"expectedSnapshotGeneration\":0,\"kind\":\"move\"," +
            "\"dx\":1024,\"slot\":\"basic\"}");

        var error = await Assert.ThrowsAsync<NativeSimulationException>(
            () => session.ApplyAsync(malformed));

        Assert.Equal(NativeStatus.MalformedEnvelope, error.Status);
    }
}
