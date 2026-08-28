using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// The only place in the client that declares native imports (CLIENT_SPEC 8.5).
/// </summary>
/// <remarks>
/// <para>
/// Every declaration uses source-generated <see cref="LibraryImportAttribute"/> with explicit byte
/// pointers. No implicit string marshalling appears anywhere: the ABI takes UTF-8 bytes and a
/// length, and letting the runtime pick an encoding would silently corrupt any non-ASCII
/// identifier and would depend on the host's ANSI code page.
/// </para>
/// <para>
/// Native methods take <see cref="MatchSafeHandle"/> rather than <see cref="nint"/> so the
/// marshaller holds a dangerous reference for the duration of each call.
/// </para>
/// </remarks>
internal static partial class DbSimNative
{
    /// <summary>Import name, resolved to an absolute path by <see cref="NativeLibraryResolver"/>.</summary>
    internal const string Library = "db_sim_ffi";

    /// <summary>Documented response ceiling. A larger response is refused by the native side.</summary>
    internal const int MaxResponseBytes = 8 * 1024 * 1024;

    static DbSimNative() => NativeLibraryResolver.EnsureRegistered();

    /// <summary>Forces the static constructor to run before any import is attempted.</summary>
    [MethodImpl(MethodImplOptions.NoInlining)]
    internal static void EnsureInitialized() => RuntimeHelpers.RunClassConstructor(typeof(DbSimNative).TypeHandle);

    [LibraryImport(Library, EntryPoint = "db_sim_abi_version")]
    internal static partial uint AbiVersion();

    [LibraryImport(Library, EntryPoint = "db_sim_simulation_version")]
    internal static partial uint SimulationVersion();

    [LibraryImport(Library, EntryPoint = "db_sim_content_version")]
    internal static partial uint ContentVersion();

    /// <summary>Serializes the full launch roster. Takes no handle: static content, not match state.</summary>
    [LibraryImport(Library, EntryPoint = "db_sim_roster")]
    internal static unsafe partial int Roster(DbSimBuffer* rosterOut);

    [LibraryImport(Library, EntryPoint = "db_sim_match_create")]
    internal static unsafe partial int MatchCreate(
        byte* configJson,
        nuint configLen,
        nint* handleOut,
        DbSimBuffer* responseOut);

    [LibraryImport(Library, EntryPoint = "db_sim_match_apply")]
    internal static unsafe partial int MatchApply(
        MatchSafeHandle handle,
        byte* commandJson,
        nuint commandLen,
        DbSimBuffer* transitionOut);

    [LibraryImport(Library, EntryPoint = "db_sim_match_snapshot")]
    internal static unsafe partial int MatchSnapshot(
        MatchSafeHandle handle,
        DbSimBuffer* snapshotOut);

    [LibraryImport(Library, EntryPoint = "db_sim_match_terrain")]
    internal static unsafe partial int MatchTerrain(
        MatchSafeHandle handle,
        ulong knownGeneration,
        uint* widthOut,
        uint* heightOut,
        ulong* generationOut,
        DbSimBuffer* cellsOut);

    [LibraryImport(Library, EntryPoint = "db_sim_match_preview")]
    internal static unsafe partial int MatchPreview(
        MatchSafeHandle handle,
        byte* requestJson,
        nuint requestLen,
        DbSimBuffer* previewOut);

    [LibraryImport(Library, EntryPoint = "db_sim_match_bot_decide")]
    internal static unsafe partial int MatchBotDecide(
        MatchSafeHandle handle,
        byte* requestJson,
        nuint requestLen,
        DbSimBuffer* decisionOut);

    /// <summary>
    /// Destroys a handle. Takes a raw pointer because <see cref="SafeHandle.ReleaseHandle"/> runs
    /// when the wrapper is already being torn down and must not resurrect it.
    /// </summary>
    /// <param name="handle">The raw native handle, or null.</param>
    [LibraryImport(Library, EntryPoint = "db_sim_match_destroy")]
    internal static partial void MatchDestroy(nint handle);

    /// <summary>
    /// Frees one native buffer and writes the zero representation back into it.
    /// </summary>
    /// <remarks>
    /// Because the native side clears the pointer and length, calling this twice on the same
    /// local is harmless — which is what lets every caller free unconditionally in a
    /// <c>finally</c> without first proving the call succeeded.
    /// </remarks>
    /// <param name="buffer">The buffer to free.</param>
    [LibraryImport(Library, EntryPoint = "db_sim_buffer_free")]
    internal static unsafe partial void BufferFree(DbSimBuffer* buffer);
}
