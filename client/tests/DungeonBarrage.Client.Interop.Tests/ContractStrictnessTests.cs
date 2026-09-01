using System.Text;
using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// The managed DTOs against the frozen envelopes they describe.
/// </summary>
/// <remarks>
/// These check strictness as much as correctness. A serializer that quietly ignores an unknown
/// field will happily keep working after the core adds one, and the mismatch then surfaces as
/// missing gameplay rather than a decode failure — far from its cause.
/// </remarks>
public sealed class ContractStrictnessTests
{
    [Fact]
    public void The_frozen_creation_request_round_trips_through_the_managed_dto()
    {
        var bytes = Fixtures.Read("create-request.json");

        var request = JsonSerializer.Deserialize<ClientCreateRequest>(bytes.Span, ClientEnvelope.Options);

        Assert.NotNull(request);
        Assert.Equal("fixture-horizontal-duel-v1", request.MatchId);
        Assert.Equal("horizontal-test-array", request.Match.MapId);
        Assert.Equal(2, request.Match.Players.Count);
        Assert.Equal("ramshot-cannon", request.Match.Players[0].Loadout.Main);
        Assert.Equal("ramshot-cannon", request.Match.Players[1].Loadout.Main);

        // Re-serializing must reproduce the original bytes exactly. Anything less means the DTO
        // is lossy, and a lossy DTO cannot be used to author a request.
        // The fixture files are newline-terminated on disk; that terminator belongs to the
        // *file*, not the envelope. Note the native responses carry it too, which is why
        // FixtureParityTests compares whole files untrimmed.
        var envelope = TrimTrailingNewline(bytes);
        var reserialized = JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options);
        Assert.True(
            envelope.Span.SequenceEqual(reserialized),
            $"round trip diverged.\nexpected: {Encoding.UTF8.GetString(envelope.Span)}\n"
            + $"actual:   {Encoding.UTF8.GetString(reserialized)}");
    }

    private static ReadOnlyMemory<byte> TrimTrailingNewline(ReadOnlyMemory<byte> bytes)
    {
        var span = bytes.Span;
        var length = bytes.Length;
        while (length > 0 && (span[length - 1] == 10 || span[length - 1] == 13))
        {
            length--;
        }

        return bytes[..length];
    }

    [Fact]
    public void An_unknown_field_is_refused_rather_than_ignored()
    {
        var text = Encoding.UTF8.GetString(Fixtures.Read("create-request.json").Span)
            .Replace("{\"schemaVersion\":1", "{\"unexpectedField\":true,\"schemaVersion\":1", StringComparison.Ordinal);

        // The core is the only authority on envelope shape. A field the managed layer does not
        // model means the two disagree, and that must be loud.
        Assert.Throws<JsonException>(
            () => JsonSerializer.Deserialize<ClientCreateRequest>(text, ClientEnvelope.Options));
    }

    [Fact]
    public void Numbers_are_not_accepted_as_strings()
    {
        var text = Encoding.UTF8.GetString(Fixtures.Read("create-request.json").Span)
            .Replace("\"seed\":12345", "\"seed\":\"12345\"", StringComparison.Ordinal);

        // Quoted numbers are how a hand-edited or third-party envelope silently changes meaning.
        Assert.Throws<JsonException>(
            () => JsonSerializer.Deserialize<ClientCreateRequest>(text, ClientEnvelope.Options));
    }

    [Fact]
    public void Closed_enums_reject_an_unknown_discriminant()
    {
        Assert.Throws<JsonException>(
            () => JsonSerializer.Deserialize<ClientAbilitySlot>("\"ultimate\"", ClientEnvelope.Options));

        // Integer fallback is disabled: an unknown numeric discriminant must not decode into a
        // valid-looking enum value.
        Assert.Throws<JsonException>(
            () => JsonSerializer.Deserialize<ClientAbilitySlot>("0", ClientEnvelope.Options));
    }

    [Fact]
    public void Closed_enums_use_the_frozen_camel_case_wire_names()
    {
        Assert.Equal("\"secondary\"", JsonSerializer.Serialize(ClientAbilitySlot.Secondary, ClientEnvelope.Options));
        Assert.Equal(
            "\"aimingAndSelection\"",
            JsonSerializer.Serialize(ClientMatchPhase.AimingAndSelection, ClientEnvelope.Options));
        Assert.Equal(
            "\"duplicateReplay\"",
            JsonSerializer.Serialize(ClientTransitionDisposition.DuplicateReplay, ClientEnvelope.Options));
    }

    [Fact]
    public void The_phase_vocabulary_matches_what_the_core_actually_emits()
    {
        // Read the phase out of a real response rather than asserting against a hand-copied list,
        // so a renamed phase fails here instead of at runtime in the client.
        using var document = JsonDocument.Parse(Fixtures.Read("responses/snapshot-initial.json"));
        var phase = document.RootElement.GetProperty("phase").GetString();

        var decoded = JsonSerializer.Deserialize<ClientMatchPhase>($"\"{phase}\"", ClientEnvelope.Options);
        Assert.Equal(ClientMatchPhase.Movement, decoded);
    }
}
