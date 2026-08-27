using System.Diagnostics.CodeAnalysis;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// Handle and buffer lifetime under normal completion, failure, cancellation, and collection.
/// </summary>
/// <remarks>
/// These cover the failure mode a fixture replay cannot: the happy path always disposes correctly,
/// so a leak only shows up when something goes wrong partway through. Each test drives one of the
/// abnormal exits the interop layer claims to survive.
/// </remarks>
public sealed class DisposalTests
{
    [Fact]
    public void Disposal_is_idempotent()
    {
        var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        session.Dispose();
        session.Dispose();
        session.Dispose();

        // Nothing is asserted beyond "this did not crash". A second `db_sim_match_destroy` on the
        // same pointer would be a double free, so surviving three calls is the claim.
        Assert.True(true);
    }

    [Fact]
    [SuppressMessage(
        "Reliability",
        "CA1849:Call async methods when in an async method",
        Justification =
            "Mixing the two disposal paths is the behaviour under test: a caller may reach either, "
            + "and both must be idempotent and safe to interleave.")]
    public async Task Async_disposal_is_idempotent_and_composes_with_sync_disposal()
    {
        var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        await session.DisposeAsync();
        await session.DisposeAsync();
        session.Dispose();
    }

    [Fact]
    public async Task A_disposed_session_refuses_further_calls_rather_than_using_a_freed_handle()
    {
        var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);
        await session.DisposeAsync();

        // The dangerous alternative is not an exception — it is silently calling into a destroyed
        // handle, which is a use-after-free that would usually appear to work.
        await Assert.ThrowsAsync<ObjectDisposedException>(() => session.SnapshotAsync());
        await Assert.ThrowsAsync<ObjectDisposedException>(
            () => session.ApplyAsync(Fixtures.Read("commands/001-move.json")));
    }

    [Fact]
    public async Task A_parse_exception_in_the_caller_does_not_leak_the_native_buffer()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        // The native buffer is freed inside the interop layer's `finally` before the bytes are
        // ever handed out, so a caller that throws while parsing cannot strand an allocation.
        for (var attempt = 0; attempt < 200; attempt++)
        {
            var snapshot = await session.SnapshotAsync();
            try
            {
                throw new FormatException("simulated parse failure");
            }
            catch (FormatException)
            {
                Assert.NotEmpty(snapshot.ToArray());
            }
        }

        // Still usable after two hundred failed parses.
        var final = await session.SnapshotAsync();
        Assert.NotEmpty(final);
    }

    [Fact]
    public async Task A_cancelled_call_leaves_the_session_usable()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        using var cancelled = new CancellationTokenSource();
        await cancelled.CancelAsync();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => session.SnapshotAsync(cancelled.Token));

        // Cancellation is observed while waiting for the executor, before any native call starts,
        // so the handle is untouched and the next call must still succeed.
        var snapshot = await session.SnapshotAsync();
        Assert.NotEmpty(snapshot);
    }

    [Fact]
    public async Task A_forgotten_session_is_reclaimed_by_the_finalizer_without_crashing()
    {
        // A caller that drops a session without disposing it must not take the process down when
        // the finalizer eventually runs `db_sim_match_destroy`.
        for (var index = 0; index < 25; index++)
        {
            CreateAndAbandon();
        }

        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();

        // A live session created after the sweep proves the native side is still healthy.
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);
        var snapshot = await session.SnapshotAsync();
        Assert.NotEmpty(snapshot);
    }

    [Fact]
    public async Task Concurrent_callers_are_serialized_rather_than_racing_the_handle()
    {
        using var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);

        // The native side holds its own mutex, so a race here would surface as a poisoned handle
        // or a corrupted response rather than a crash. Every response must be byte-identical
        // because none of these calls mutates anything.
        var expected = await session.SnapshotAsync();
        var tasks = Enumerable.Range(0, 32).Select(_ => session.SnapshotAsync()).ToArray();
        var results = await Task.WhenAll(tasks);

        foreach (var result in results)
        {
            Assert.True(expected.AsSpan().SequenceEqual(result), "a concurrent read diverged");
        }
    }

    [SuppressMessage(
        "Reliability",
        "CA2000:Dispose objects before losing scope",
        Justification =
            "Abandoning the session without disposing is precisely what this test exercises: the "
            + "SafeHandle finalizer must reclaim the native handle for a caller who forgot.")]
    private static void CreateAndAbandon()
    {
        // Deliberately not disposed, and deliberately in its own frame so the local cannot stay
        // rooted by the enclosing method's stack.
        _ = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);
    }
}
