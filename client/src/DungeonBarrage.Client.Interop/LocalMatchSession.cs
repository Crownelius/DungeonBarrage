using System.Text;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Raised when a native call fails at the ABI boundary.
/// </summary>
/// <remarks>
/// A gameplay rejection is <em>not</em> one of these. The rules refusing a command is a normal
/// outcome carried inside a successful response envelope; this type means the boundary itself
/// failed, which is a bug or a corrupted install rather than something a player did.
/// </remarks>
public sealed class NativeSimulationException : Exception
{
    /// <summary>Creates an exception for a failing native status.</summary>
    /// <param name="operation">The ABI entry point that failed.</param>
    /// <param name="status">The status it returned.</param>
    public NativeSimulationException(string operation, int status)
        : base($"Native call '{operation}' failed with status {status} ({NativeStatus.Describe(status)}).")
    {
        Operation = operation;
        Status = status;
    }

    /// <summary>Creates an exception with no additional context.</summary>
    public NativeSimulationException()
        : base("A native simulation call failed.")
    {
        Operation = "unknown";
    }

    /// <summary>Creates an exception with a message.</summary>
    /// <param name="message">The message.</param>
    public NativeSimulationException(string message)
        : base(message)
    {
        Operation = "unknown";
    }

    /// <summary>Creates an exception with a message and an inner cause.</summary>
    /// <param name="message">The message.</param>
    /// <param name="innerException">The cause.</param>
    public NativeSimulationException(string message, Exception innerException)
        : base(message, innerException)
    {
        Operation = "unknown";
    }

    /// <summary>The ABI entry point that failed.</summary>
    public string Operation { get; }

    /// <summary>The raw ABI status.</summary>
    public int Status { get; }
}

/// <summary>
/// Owns one live local match and serializes every call into it.
/// </summary>
/// <remarks>
/// <para>
/// The native handle is not re-entrant and is not intended to be shared. This type is the single
/// owner: it holds the <see cref="MatchSafeHandle"/>, admits one call at a time, and disposes the
/// handle exactly once. Callers get an ordinary async API and never see a pointer.
/// </para>
/// <para>
/// Every native response is copied into managed memory and freed inside <c>finally</c>, so a
/// parse failure, a cancellation, or an exception thrown by a caller's continuation cannot leak
/// the native allocation.
/// </para>
/// </remarks>
public sealed class LocalMatchSession : IAsyncDisposable, IDisposable
{
    private readonly SemaphoreSlim _gate = new(1, 1);
    private readonly MatchSafeHandle _handle;
    private bool _disposed;
    private bool _poisoned;

    private LocalMatchSession(MatchSafeHandle handle, byte[] createResponse)
    {
        _handle = handle;
        CreateResponse = createResponse;
    }

    /// <summary>The exact response bytes produced by match creation.</summary>
    public ReadOnlyMemory<byte> CreateResponse { get; }

    /// <summary>ABI version reported by the loaded native library.</summary>
    public static uint AbiVersion
    {
        get
        {
            DbSimNative.EnsureInitialized();
            return DbSimNative.AbiVersion();
        }
    }

    /// <summary>Simulation version reported by the loaded native library.</summary>
    public static uint SimulationVersion
    {
        get
        {
            DbSimNative.EnsureInitialized();
            return DbSimNative.SimulationVersion();
        }
    }

    /// <summary>Content version reported by the loaded native library.</summary>
    public static uint ContentVersion
    {
        get
        {
            DbSimNative.EnsureInitialized();
            return DbSimNative.ContentVersion();
        }
    }

    /// <summary>
    /// Creates a match from an exact creation-request envelope.
    /// </summary>
    /// <param name="createRequestJson">UTF-8 bytes of the creation request, passed through unchanged.</param>
    /// <returns>The owning session.</returns>
    /// <exception cref="NativeSimulationException">The boundary refused the request.</exception>
    public static unsafe LocalMatchSession Create(ReadOnlySpan<byte> createRequestJson)
    {
        DbSimNative.EnsureInitialized();

        MatchSafeHandle? handle = null;
        nint raw = nint.Zero;
        var response = default(DbSimBuffer);
        try
        {
            handle = new MatchSafeHandle();
            int status;
            fixed (byte* json = createRequestJson)
            {
                status = DbSimNative.MatchCreate(
                    json,
                    (nuint)createRequestJson.Length,
                    &raw,
                    &response);
            }

            if (status != NativeStatus.Ok)
            {
                throw new NativeSimulationException("db_sim_match_create", status);
            }

            if (raw == nint.Zero)
            {
                // An OK status with no handle would leave the caller with a session object that
                // owns nothing, failing later and further from the cause.
                throw new NativeSimulationException("db_sim_match_create", NativeStatus.InternalPanic);
            }

            handle.Adopt(raw);
            raw = nint.Zero;
            var session = new LocalMatchSession(handle, Copy(response));

            // Ownership has moved to the session, so the cleanup below must not also dispose it.
            handle = null;
            return session;
        }
        finally
        {
            // A handle produced before a failure must still be destroyed; nothing else can reach
            // it once this factory does not return.
            if (raw != nint.Zero)
            {
                DbSimNative.MatchDestroy(raw);
            }

            handle?.Dispose();
            DbSimNative.BufferFree(&response);
        }
    }

    /// <summary>Applies one command envelope and returns the transition bytes.</summary>
    /// <param name="commandJson">UTF-8 bytes of the command, passed through unchanged.</param>
    /// <param name="cancellationToken">Cancels waiting for the session to become free.</param>
    /// <returns>The exact transition response bytes.</returns>
    public Task<byte[]> ApplyAsync(ReadOnlyMemory<byte> commandJson, CancellationToken cancellationToken = default)
        => WithBytesAsync("db_sim_match_apply", commandJson, ApplyCore, cancellationToken);

    /// <summary>Requests a read-only preview and returns the preview bytes.</summary>
    /// <param name="requestJson">UTF-8 bytes of the preview request, passed through unchanged.</param>
    /// <param name="cancellationToken">Cancels waiting for the session to become free.</param>
    /// <returns>The exact preview response bytes.</returns>
    public Task<byte[]> PreviewAsync(ReadOnlyMemory<byte> requestJson, CancellationToken cancellationToken = default)
        => WithBytesAsync("db_sim_match_preview", requestJson, PreviewCore, cancellationToken);

    /// <summary>Reads the current authoritative snapshot.</summary>
    /// <param name="cancellationToken">Cancels waiting for the session to become free.</param>
    /// <returns>The exact snapshot response bytes.</returns>
    public Task<byte[]> SnapshotAsync(CancellationToken cancellationToken = default)
        => WithoutBytesAsync("db_sim_match_snapshot", SnapshotCore, cancellationToken);

    /// <summary>Reads terrain cells changed since <paramref name="knownGeneration"/>.</summary>
    /// <param name="knownGeneration">The terrain generation the caller already holds.</param>
    /// <param name="cancellationToken">Cancels waiting for the session to become free.</param>
    /// <returns>The terrain dimensions, current generation, and cell bytes.</returns>
    public async Task<TerrainRead> TerrainAsync(ulong knownGeneration, CancellationToken cancellationToken = default)
    {
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            ThrowIfUnusable();
            return TerrainCore(knownGeneration);
        }
        finally
        {
            _gate.Release();
        }
    }

    /// <inheritdoc />
    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        _handle.Dispose();
        _gate.Dispose();
    }

    /// <inheritdoc />
    public ValueTask DisposeAsync()
    {
        // Disposal is idempotent and does no async work of its own: the native destructor cannot
        // block or fail. `IAsyncDisposable` exists so callers can `await using` uniformly.
        Dispose();
        return ValueTask.CompletedTask;
    }

    private unsafe byte[] ApplyCore(ReadOnlySpan<byte> json)
    {
        var buffer = default(DbSimBuffer);
        try
        {
            int status;
            fixed (byte* ptr = json)
            {
                status = DbSimNative.MatchApply(_handle, ptr, (nuint)json.Length, &buffer);
            }

            Check("db_sim_match_apply", status);
            return Copy(buffer);
        }
        finally
        {
            DbSimNative.BufferFree(&buffer);
        }
    }

    private unsafe byte[] PreviewCore(ReadOnlySpan<byte> json)
    {
        var buffer = default(DbSimBuffer);
        try
        {
            int status;
            fixed (byte* ptr = json)
            {
                status = DbSimNative.MatchPreview(_handle, ptr, (nuint)json.Length, &buffer);
            }

            Check("db_sim_match_preview", status);
            return Copy(buffer);
        }
        finally
        {
            DbSimNative.BufferFree(&buffer);
        }
    }

    private unsafe byte[] SnapshotCore()
    {
        var buffer = default(DbSimBuffer);
        try
        {
            var status = DbSimNative.MatchSnapshot(_handle, &buffer);
            Check("db_sim_match_snapshot", status);
            return Copy(buffer);
        }
        finally
        {
            DbSimNative.BufferFree(&buffer);
        }
    }

    private unsafe TerrainRead TerrainCore(ulong knownGeneration)
    {
        // Distinct zeroed locals per call. Reusing one across calls, or letting two calls share a
        // buffer local, is how a still-live allocation gets overwritten and leaked.
        var buffer = default(DbSimBuffer);
        uint width = 0;
        uint height = 0;
        ulong generation = 0;
        try
        {
            var status = DbSimNative.MatchTerrain(
                _handle,
                knownGeneration,
                &width,
                &height,
                &generation,
                &buffer);

            Check("db_sim_match_terrain", status);
            return new TerrainRead(width, height, generation, Copy(buffer));
        }
        finally
        {
            DbSimNative.BufferFree(&buffer);
        }
    }

    private static unsafe byte[] Copy(in DbSimBuffer buffer)
    {
        if (!buffer.HasPayload)
        {
            return [];
        }

        if (buffer.Len > (nuint)DbSimNative.MaxResponseBytes)
        {
            // The native side enforces the same ceiling; checking again before allocating means a
            // corrupted length can never turn into an enormous managed allocation.
            throw new NativeSimulationException("db_sim_response", NativeStatus.ResponseTooLarge);
        }

        return new ReadOnlySpan<byte>((void*)buffer.Ptr, checked((int)buffer.Len)).ToArray();
    }

    private void Check(string operation, int status)
    {
        if (status == NativeStatus.Ok)
        {
            return;
        }

        if (NativeStatus.PoisonsHandle(status))
        {
            _poisoned = true;
        }

        throw new NativeSimulationException(operation, status);
    }

    private void ThrowIfUnusable()
    {
        ObjectDisposedException.ThrowIf(_disposed, this);

        if (_poisoned)
        {
            throw new NativeSimulationException("db_sim_session", NativeStatus.InternalPanic);
        }
    }

    private async Task<byte[]> WithBytesAsync(
        string operation,
        ReadOnlyMemory<byte> json,
        Func<ReadOnlyMemory<byte>, byte[]> body,
        CancellationToken cancellationToken)
    {
        _ = operation;
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            ThrowIfUnusable();
            return body(json);
        }
        finally
        {
            _gate.Release();
        }
    }

    private async Task<byte[]> WithoutBytesAsync(
        string operation,
        Func<byte[]> body,
        CancellationToken cancellationToken)
    {
        _ = operation;
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            ThrowIfUnusable();
            return body();
        }
        finally
        {
            _gate.Release();
        }
    }

    private byte[] ApplyCore(ReadOnlyMemory<byte> json) => ApplyCore(json.Span);

    private byte[] PreviewCore(ReadOnlyMemory<byte> json) => PreviewCore(json.Span);

    /// <summary>Encodes text as UTF-8 for callers holding a JSON string rather than bytes.</summary>
    /// <param name="json">The envelope text.</param>
    /// <returns>UTF-8 bytes.</returns>
    public static byte[] Utf8(string json) => Encoding.UTF8.GetBytes(json);
}

/// <summary>One terrain read: dimensions, generation, and raw cell bytes.</summary>
/// <param name="Width">Terrain width in cells.</param>
/// <param name="Height">Terrain height in cells.</param>
/// <param name="Generation">Authoritative terrain generation for these cells.</param>
/// <param name="Cells">Raw cell bytes, empty when the caller's generation was already current.</param>
public readonly record struct TerrainRead(
    uint Width,
    uint Height,
    ulong Generation,
    ReadOnlyMemory<byte> Cells);
