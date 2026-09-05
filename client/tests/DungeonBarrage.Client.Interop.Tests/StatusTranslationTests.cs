using System.Text;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// How malformed input and native statuses reach managed callers.
/// </summary>
/// <remarks>
/// The distinction under test is that a <em>gameplay refusal</em> is a successful call carrying a
/// rejection envelope, while a <em>boundary failure</em> is an exception. Collapsing the two would
/// either turn ordinary rule enforcement into exceptions on the hot path, or hide a genuine
/// interop bug inside a response a caller would treat as normal.
/// </remarks>
public sealed class StatusTranslationTests
{
    [Fact]
    public void Malformed_json_is_refused_at_creation_with_the_envelope_status()
    {
        var garbage = Encoding.UTF8.GetBytes("{ this is not json ");

        var error = Assert.Throws<NativeSimulationException>(() => LocalMatchSession.Create(garbage));

        Assert.Equal(NativeStatus.MalformedEnvelope, error.Status);
        Assert.Equal("db_sim_match_create", error.Operation);
    }

    [Fact]
    public void Invalid_utf8_is_refused_rather_than_silently_replaced()
    {
        // A lone continuation byte. The ABI takes bytes, not a string, precisely so this is
        // rejected instead of being turned into U+FFFD by an eager decoder.
        var invalid = new byte[] { 0x7B, 0x80, 0x7D };

        var error = Assert.Throws<NativeSimulationException>(() => LocalMatchSession.Create(invalid));

        Assert.Equal(NativeStatus.MalformedEnvelope, error.Status);
    }

    [Fact]
    public void An_unsupported_schema_version_is_distinguished_from_malformed_input()
    {
        var text = Encoding.UTF8.GetString(Fixtures.Read("create-request.json").Span)
            .Replace("\"schemaVersion\":2", "\"schemaVersion\":999", StringComparison.Ordinal);

        var error = Assert.Throws<NativeSimulationException>(
            () => LocalMatchSession.Create(Encoding.UTF8.GetBytes(text)));

        // A version mismatch is actionable — the client and library disagree — where malformed
        // input is not. Reporting both the same way would make an upgrade problem look like a bug.
        Assert.Equal(NativeStatus.UnsupportedVersion, error.Status);
    }

    [Fact]
    public async Task A_gameplay_rejection_is_a_successful_call_not_an_exception()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        // A command from the wrong player: legal envelope, refused by the rules.
        var text = Encoding.UTF8.GetString(Fixtures.Read("commands/001-move.json").Span)
            .Replace("\"playerId\":\"a-local-player\"", "\"playerId\":\"b-local-bot\"", StringComparison.Ordinal);

        var response = await session.ApplyAsync(Encoding.UTF8.GetBytes(text));

        Assert.NotEmpty(response);
        var body = Encoding.UTF8.GetString(response);
        Assert.Contains("\"rejected\"", body, StringComparison.Ordinal);

        // The session survives a refusal and keeps working, which is the point of not throwing.
        var snapshot = await session.SnapshotAsync();
        Assert.NotEmpty(snapshot);
    }

    [Fact]
    public async Task A_malformed_command_on_a_live_session_does_not_poison_it()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        var error = await Assert.ThrowsAsync<NativeSimulationException>(
            () => session.ApplyAsync(Encoding.UTF8.GetBytes("not json")));
        Assert.Equal(NativeStatus.MalformedEnvelope, error.Status);

        // Only a contained panic poisons a handle. A merely malformed request must leave the
        // match playable, or one bad client message would end the session.
        var snapshot = await session.SnapshotAsync();
        Assert.NotEmpty(snapshot);
    }

    [Fact]
    public void Every_status_has_a_stable_name_and_only_panic_poisons()
    {
        Assert.Equal("ok", NativeStatus.Describe(NativeStatus.Ok));
        Assert.Equal("nullPointer", NativeStatus.Describe(NativeStatus.NullPointer));
        Assert.Equal("malformedEnvelope", NativeStatus.Describe(NativeStatus.MalformedEnvelope));
        Assert.Equal("unsupportedVersion", NativeStatus.Describe(NativeStatus.UnsupportedVersion));
        Assert.Equal("internalPanic", NativeStatus.Describe(NativeStatus.InternalPanic));
        Assert.Equal("responseTooLarge", NativeStatus.Describe(NativeStatus.ResponseTooLarge));
        Assert.Equal("unknown", NativeStatus.Describe(-9999));

        Assert.True(NativeStatus.PoisonsHandle(NativeStatus.InternalPanic));
        Assert.False(NativeStatus.PoisonsHandle(NativeStatus.MalformedEnvelope));
        Assert.False(NativeStatus.PoisonsHandle(NativeStatus.Ok));
    }

    [Fact]
    public void The_resolver_advertises_exactly_the_supported_runtime_identifiers()
    {
        Assert.Equal("db_sim_ffi.dll", NativeLibraryResolver.NativeFileName("win-x64"));
        Assert.Equal("libdb_sim_ffi.so", NativeLibraryResolver.NativeFileName("linux-x64"));
        Assert.Equal("libdb_sim_ffi.dylib", NativeLibraryResolver.NativeFileName("osx-x64"));
        Assert.Equal("libdb_sim_ffi.dylib", NativeLibraryResolver.NativeFileName("osx-arm64"));

        // An unadvertised RID resolves to nothing rather than guessing a file name, so an
        // unsupported platform fails while loading instead of inside a native call.
        Assert.Null(NativeLibraryResolver.NativeFileName("linux-arm64"));
        Assert.Null(NativeLibraryResolver.NativeFileName(""));
    }

    [Fact]
    public void Candidate_paths_are_absolute_and_anchored_to_the_assembly()
    {
        var candidates = NativeLibraryResolver.CandidatePaths();

        Assert.NotEmpty(candidates);
        foreach (var candidate in candidates)
        {
            // A relative path would resolve against the working directory, which an attacker or a
            // launcher can choose. CLIENT_SPEC 8.6 requires application-owned absolute paths.
            Assert.True(Path.IsPathFullyQualified(candidate), $"not absolute: {candidate}");
        }
    }
}
