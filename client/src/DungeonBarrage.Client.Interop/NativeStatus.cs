namespace DungeonBarrage.Client.Interop;

/// <summary>
/// ABI status codes returned by every native entry point.
/// </summary>
/// <remarks>
/// These mirror <c>db_sim_ffi::status</c> exactly. A gameplay <em>rejection</em> is never one of
/// these: it is carried inside an <see cref="Ok"/> response envelope, because a client asking for
/// something the rules refuse is a normal outcome, not a transport failure. Only the boundary
/// itself failing produces a negative code.
/// </remarks>
public static class NativeStatus
{
    /// <summary>The call completed; inspect the output envelope.</summary>
    public const int Ok = 0;

    /// <summary>A required pointer was null.</summary>
    public const int NullPointer = -1;

    /// <summary>Invalid UTF-8, malformed JSON or envelope, unknown field or enum.</summary>
    public const int MalformedEnvelope = -2;

    /// <summary>Unsupported envelope schema, simulation version, or content version.</summary>
    public const int UnsupportedVersion = -3;

    /// <summary>A panic or terminal invariant was contained. The handle is poisoned.</summary>
    public const int InternalPanic = -4;

    /// <summary>A response would exceed the documented 8 MiB cap.</summary>
    public const int ResponseTooLarge = -5;

    /// <summary>Stable diagnostic name for a status code.</summary>
    /// <param name="status">The status returned by a native call.</param>
    /// <returns>A short identifier suitable for an exception message or a log line.</returns>
    public static string Describe(int status) => status switch
    {
        Ok => "ok",
        NullPointer => "nullPointer",
        MalformedEnvelope => "malformedEnvelope",
        UnsupportedVersion => "unsupportedVersion",
        InternalPanic => "internalPanic",
        ResponseTooLarge => "responseTooLarge",
        _ => "unknown",
    };

    /// <summary>
    /// Whether this status leaves the owning handle permanently unusable.
    /// </summary>
    /// <remarks>
    /// The native side sets a poison bit on a contained panic and refuses every later call on that
    /// handle. Continuing to use the session after this would produce a stream of identical
    /// failures rather than a single clear one, so the managed layer stops as well.
    /// </remarks>
    /// <param name="status">The status returned by a native call.</param>
    /// <returns><see langword="true"/> when the handle is poisoned.</returns>
    public static bool PoisonsHandle(int status) => status == InternalPanic;
}
