using System.Text;
using System.Text.Json;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// Replays the frozen duel fixture through the real release library.
/// </summary>
/// <remarks>
/// <para>
/// This is the C3 gate. The request files are fed through unchanged and the responses are compared
/// to the frozen files <em>byte for byte</em>, not field by field: a comparison that parses both
/// sides and checks properties would pass even if C# had quietly reordered keys, changed number
/// formatting, or dropped a field the managed DTOs do not model. The whole claim being tested is
/// that the managed layer is a transparent pipe over the authoritative core.
/// </para>
/// <para>
/// The same fixture already passes through the Rust ABI test. Running it again from C# is what
/// proves the boundary itself — marshalling, buffer ownership, encoding — introduces no change.
/// </para>
/// </remarks>
public sealed class FixtureParityTests
{
    /// <summary>The hash the whole replay must end on.</summary>
    /// <remarks>
    /// Frozen in `docs/HANDOFF.md` and asserted by the Rust ABI suite. If C# reaches a different
    /// value, the managed layer has altered a request on its way in.
    /// </remarks>
    private const string FinalStateHash = "d8686762470c0c36";

    [Fact]
    public void The_frozen_duel_replays_byte_for_byte_through_the_release_library()
    {
        var create = Fixtures.Read("create-request.json");

        using var session = LocalMatchSession.Create(create.Span);

        AssertBytesEqual(
            Fixtures.Read("responses/create.json"),
            session.CreateResponse,
            "responses/create.json");
    }

    [Fact]
    public async Task Every_frozen_response_matches_including_the_final_hash()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        AssertBytesEqual(
            Fixtures.Read("responses/create.json"),
            session.CreateResponse,
            "responses/create.json");

        var snapshot = await session.SnapshotAsync();
        AssertBytesEqual(
            Fixtures.Read("responses/snapshot-initial.json"),
            snapshot,
            "responses/snapshot-initial.json");

        var preview = await session.PreviewAsync(Fixtures.Read("previews/001-basic.json"));
        AssertBytesEqual(
            Fixtures.Read("responses/preview-basic.json"),
            preview,
            "responses/preview-basic.json");

        var move = await session.ApplyAsync(Fixtures.Read("commands/001-move.json"));
        AssertBytesEqual(
            Fixtures.Read("responses/001-move.json"),
            move,
            "responses/001-move.json");

        var ability = await session.ApplyAsync(Fixtures.Read("commands/002-ability.json"));
        AssertBytesEqual(
            Fixtures.Read("responses/002-ability.json"),
            ability,
            "responses/002-ability.json");

        // Read the terminal hash out of the response the managed layer actually received, rather
        // than trusting that matching the file implies it.
        using var document = JsonDocument.Parse(ability);
        var root = document.RootElement;
        Assert.Equal(FinalStateHash, root.GetProperty("postStateHash").GetString());
        Assert.Equal(
            FinalStateHash,
            root.GetProperty("postSnapshot").GetProperty("stateHash").GetString());
    }

    [Fact]
    public async Task A_preview_never_advances_the_session()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);
        var before = await session.SnapshotAsync();

        _ = await session.PreviewAsync(Fixtures.Read("previews/001-basic.json"));

        var after = await session.SnapshotAsync();

        // A preview that consumed RNG or advanced a generation would make aiming feedback
        // change the match, which is the one thing a preview must never do.
        AssertBytesEqual(before, after, "snapshot across a preview");
    }

    [Fact]
    public void The_loaded_library_reports_the_versions_the_fixture_was_frozen_against()
    {
        var create = Fixtures.Read("create-request.json");
        using var document = JsonDocument.Parse(create);
        var root = document.RootElement;

        // A version skew between the fixture and the loaded binary would produce confusing
        // downstream diffs; naming it here fails with the actual cause instead.
        Assert.Equal(root.GetProperty("simulationVersion").GetUInt32(), LocalMatchSession.SimulationVersion);
        Assert.Equal(root.GetProperty("contentVersion").GetUInt32(), LocalMatchSession.ContentVersion);
        Assert.Equal(1u, LocalMatchSession.AbiVersion);
    }

    private static void AssertBytesEqual(ReadOnlyMemory<byte> expected, ReadOnlyMemory<byte> actual, string label)
    {
        if (expected.Span.SequenceEqual(actual.Span))
        {
            return;
        }

        // Byte equality failed, so report where and show both sides as text — a raw length
        // mismatch alone would send a reader hunting through 4 KB of JSON.
        var expectedText = Encoding.UTF8.GetString(expected.Span);
        var actualText = Encoding.UTF8.GetString(actual.Span);
        var divergence = FirstDifference(expectedText, actualText);

        Assert.Fail(
            $"{label} diverged at index {divergence}.\n" +
            $"expected ({expected.Length} bytes): {Excerpt(expectedText, divergence)}\n" +
            $"actual   ({actual.Length} bytes): {Excerpt(actualText, divergence)}");
    }

    private static int FirstDifference(string left, string right)
    {
        var shared = Math.Min(left.Length, right.Length);
        for (var index = 0; index < shared; index++)
        {
            if (left[index] != right[index])
            {
                return index;
            }
        }

        return shared;
    }

    private static string Excerpt(string text, int around)
    {
        var start = Math.Max(0, around - 60);
        var length = Math.Min(160, text.Length - start);
        return text.Substring(start, length);
    }
}
