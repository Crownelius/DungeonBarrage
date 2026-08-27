using System.Runtime.InteropServices;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Exact managed mirror of the native <c>DbOwnedBuffer</c>.
/// </summary>
/// <remarks>
/// <para>
/// The native side declares this as <c>#[repr(C)] { ptr: *mut u8, len: usize }</c>. The field
/// order, types, and count must match exactly; <see cref="nuint"/> is the marshalling of Rust's
/// pointer-sized <c>usize</c>.
/// </para>
/// <para>
/// Every instance must be zero-initialized before it is handed to a native call
/// (CLIENT_SPEC 8.5). A caller that reuses a local still holding a previous, already-freed
/// pointer would hand the native side a dangling value to overwrite — or, worse, one it never
/// overwrites on an early-return status, leaving the caller to free the same allocation twice.
/// This is a <c>struct</c> with no constructor precisely so <c>default</c> is the zero
/// representation, and it is never cached or reused across calls.
/// </para>
/// </remarks>
[StructLayout(LayoutKind.Sequential)]
internal struct DbSimBuffer
{
    /// <summary>First byte of the Rust-owned allocation, or null for an empty buffer.</summary>
    public nint Ptr;

    /// <summary>Exact allocation length in bytes.</summary>
    public nuint Len;

    /// <summary>Whether this buffer currently owns an allocation.</summary>
    public readonly bool HasPayload => Ptr != nint.Zero && Len != 0;
}
