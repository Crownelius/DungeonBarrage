using System.Text.Json;
using Xunit;

namespace DungeonBarrage.Client.Contracts.Tests;

/// <summary>The command DTOs against the frozen command fixtures the native ABI actually parses.</summary>
/// <remarks>
/// <para>
/// Unlike a response, JSON field <em>order</em> is not part of a command's contract: the client
/// authors these, `serde_json` parses them on the other side, and JSON objects are inherently
/// unordered — key order carries no meaning there. What must match is the value each key carries.
/// These tests compare parsed values, not bytes; a byte-identical assertion here would be testing
/// this serializer's incidental member ordering, not correctness.
/// </para>
/// <para>
/// The stronger proof — that a command built here is actually accepted by the real native parser
/// and produces the real frozen transition — lives in
/// <c>DungeonBarrage.Client.Interop.Tests.CommandRoundTripTests</c>, the project with a native
/// dependency to prove it against.
/// </para>
/// </remarks>
public sealed class CommandContractTests
{
    private static readonly string[] PassCommandFieldNames =
        ["commandId", "expectedSnapshotGeneration", "expectedTurnNumber", "kind", "playerId", "schemaVersion"];

    [Fact]
    public void The_frozen_move_command_matches_every_field()
    {
        var command = ClientMatchCommand.Move(
            commandId: "fixture-move-001",
            playerId: "a-local-player",
            expectedTurnNumber: 1,
            expectedSnapshotGeneration: 0,
            dx: 1024);

        AssertMatchesFixture(command, "commands/001-move.json");
    }

    [Fact]
    public void The_frozen_ability_command_matches_every_field()
    {
        var command = ClientMatchCommand.Ability(
            commandId: "fixture-ability-002",
            playerId: "a-local-player",
            expectedTurnNumber: 1,
            expectedSnapshotGeneration: 1,
            slot: ClientAbilitySlot.Basic,
            angleMillidegrees: 45_000,
            powerBasisPoints: 1_500,
            targetPlayerId: null,
            secondaryTargetPlayerId: null);

        AssertMatchesFixture(command, "commands/002-ability.json");
    }

    [Fact]
    public void A_pass_command_serializes_with_no_extra_fields()
    {
        var command = ClientMatchCommand.Pass(
            commandId: "pass-1",
            playerId: "a-local-player",
            expectedTurnNumber: 1,
            expectedSnapshotGeneration: 0);

        var json = JsonSerializer.Serialize(command, ClientEnvelope.Options);

        using var document = JsonDocument.Parse(json);
        var propertyNames = document.RootElement
            .EnumerateObject()
            .Select(p => p.Name)
            .OrderBy(name => name, StringComparer.Ordinal)
            .ToArray();
        Assert.Equal(
            PassCommandFieldNames.OrderBy(name => name, StringComparer.Ordinal),
            propertyNames);
    }

    [Fact]
    public void The_native_deny_unknown_fields_contract_is_matched_by_construction()
    {
        // db-sim-ffi's MatchCommandDto variants are #[serde(deny_unknown_fields)]: a command
        // carrying a field belonging to a different kind is a hard parse failure at the native
        // boundary, not a warning. Deserializing this client's own move-command output back
        // through the polymorphic base type must land on exactly the move variant, never a
        // union that could carry ability-only fields alongside it.
        var command = ClientMatchCommand.Move("id", "p", 1, 0, dx: 512);
        var json = JsonSerializer.Serialize(command, ClientEnvelope.Options);

        var decoded = JsonSerializer.Deserialize<ClientMatchCommand>(json, ClientEnvelope.Options);

        var move = Assert.IsType<ClientMoveCommand>(decoded);
        Assert.Equal(512, move.Dx);
    }

    private static void AssertMatchesFixture(ClientMatchCommand command, string fixtureRelativePath)
    {
        var expectedJson = Fixtures.Read(fixtureRelativePath);
        using var expected = JsonDocument.Parse(expectedJson);
        using var actual = JsonDocument.Parse(JsonSerializer.SerializeToUtf8Bytes(command, ClientEnvelope.Options));

        var expectedFields = expected.RootElement.EnumerateObject()
            .ToDictionary(p => p.Name, p => p.Value.GetRawText(), StringComparer.Ordinal);
        var actualFields = actual.RootElement.EnumerateObject()
            .ToDictionary(p => p.Name, p => p.Value.GetRawText(), StringComparer.Ordinal);

        Assert.Equal(expectedFields.Keys.OrderBy(k => k, StringComparer.Ordinal), actualFields.Keys.OrderBy(k => k, StringComparer.Ordinal));
        foreach (var (key, expectedValue) in expectedFields)
        {
            Assert.True(
                actualFields.TryGetValue(key, out var actualValue) && actualValue == expectedValue,
                $"{fixtureRelativePath}: field '{key}' expected {expectedValue}, got " +
                (actualFields.TryGetValue(key, out var got) ? got : "<missing>"));
        }
    }
}
