using System.Runtime.InteropServices;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Owns one live native match handle.
/// </summary>
/// <remarks>
/// <para>
/// A <see cref="SafeHandle"/> rather than a raw <see cref="nint"/> because the runtime is allowed
/// to collect an object while one of its methods is still executing. With a raw pointer, a session
/// that goes out of scope mid-call can be finalized while native code is still using the handle,
/// which is a use-after-free that reproduces only under GC pressure. Passing the handle itself to
/// each native method makes the marshaller hold a reference for the duration of the call, so the
/// finalizer cannot run underneath it (CLIENT_SPEC 8.5).
/// </para>
/// <para>
/// Ownership is exclusive: exactly one <see cref="LocalMatchSession"/> owns a handle, and calls on
/// it are serialized by that session. The native side additionally holds a mutex, so a broken
/// caller that bypasses the executor is refused rather than racing.
/// </para>
/// </remarks>
public sealed class MatchSafeHandle : SafeHandle
{
    /// <summary>Creates an invalid handle for the native layer to populate.</summary>
    public MatchSafeHandle()
        : base(nint.Zero, ownsHandle: true)
    {
    }

    /// <inheritdoc />
    public override bool IsInvalid => handle == nint.Zero;

    /// <summary>
    /// Adopts a raw pointer returned by <c>db_sim_match_create</c>.
    /// </summary>
    /// <remarks>
    /// Only the create path may call this, and only with a pointer the native side just produced.
    /// The handle takes ownership immediately, so no window exists in which a successful create
    /// has allocated a session that nothing is responsible for destroying.
    /// </remarks>
    /// <param name="value">The raw native pointer.</param>
    internal void Adopt(nint value) => SetHandle(value);

    /// <inheritdoc />
    protected override bool ReleaseHandle()
    {
        // `db_sim_match_destroy` ignores null and takes ownership of anything else. It cannot
        // fail, which is why it returns void: a destructor that could fail would leave the caller
        // with no correct action to take.
        DbSimNative.MatchDestroy(handle);
        SetHandle(nint.Zero);
        return true;
    }
}
