using System.Reflection;
using System.Runtime.InteropServices;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Resolves the native simulation library from application-owned absolute paths.
/// </summary>
/// <remarks>
/// <para>
/// The OS default search path is deliberately not used (CLIENT_SPEC 8.6). On Windows it includes
/// directories a non-administrator can often write to, so a file named <c>db_sim_ffi.dll</c>
/// dropped beside the executable — or anywhere earlier in the search order — would be loaded in
/// place of the real one. Since this library <em>is</em> the game rules, that is a full
/// authoritative-logic replacement, not a cosmetic hijack. Resolving an absolute path under the
/// assembly directory removes the search entirely.
/// </para>
/// <para>
/// Only RIDs this build actually advertises are consulted, and only the matching one is tried. A
/// fallback to "whatever loads" would let a wrong-architecture or wrong-platform binary be picked
/// up and fail somewhere far less obvious.
/// </para>
/// </remarks>
public static class NativeLibraryResolver
{
    private static readonly object Gate = new();
    private static bool _registered;

    /// <summary>Registers the resolver once per process. Safe to call repeatedly.</summary>
    public static void EnsureRegistered()
    {
        lock (Gate)
        {
            if (_registered)
            {
                return;
            }

            NativeLibrary.SetDllImportResolver(typeof(NativeLibraryResolver).Assembly, Resolve);
            _registered = true;
        }
    }

    /// <summary>
    /// The runtime identifier this process needs, or <see langword="null"/> on an unsupported
    /// platform or architecture.
    /// </summary>
    /// <remarks>
    /// Returns null rather than guessing. An unsupported combination must fail while loading the
    /// library, with a message naming the platform, instead of later inside a native call.
    /// </remarks>
    public static string? CurrentRuntimeIdentifier()
    {
        if (OperatingSystem.IsWindows() && RuntimeInformation.ProcessArchitecture == Architecture.X64)
        {
            return "win-x64";
        }

        if (OperatingSystem.IsLinux() && RuntimeInformation.ProcessArchitecture == Architecture.X64)
        {
            return "linux-x64";
        }

        if (OperatingSystem.IsMacOS())
        {
            return RuntimeInformation.ProcessArchitecture switch
            {
                Architecture.X64 => "osx-x64",
                Architecture.Arm64 => "osx-arm64",
                _ => null,
            };
        }

        return null;
    }

    /// <summary>The platform file name for a runtime identifier.</summary>
    /// <param name="runtimeIdentifier">One of the advertised RIDs.</param>
    /// <returns>The expected native file name, or <see langword="null"/> if unrecognized.</returns>
    public static string? NativeFileName(string runtimeIdentifier) => runtimeIdentifier switch
    {
        "win-x64" => "db_sim_ffi.dll",
        "linux-x64" => "libdb_sim_ffi.so",
        "osx-x64" or "osx-arm64" => "libdb_sim_ffi.dylib",
        _ => null,
    };

    /// <summary>
    /// Absolute paths that may contain the native library, in probe order.
    /// </summary>
    /// <remarks>
    /// Both entries are anchored to the assembly's own directory, never the working directory: a
    /// process launched from elsewhere must resolve the same file, and an attacker who controls
    /// the working directory must not gain a load path.
    /// </remarks>
    /// <returns>Candidate absolute paths.</returns>
    public static IReadOnlyList<string> CandidatePaths()
    {
        var rid = CurrentRuntimeIdentifier();
        if (rid is null)
        {
            return [];
        }

        var fileName = NativeFileName(rid);
        if (fileName is null)
        {
            return [];
        }

        var assemblyDirectory = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
        if (string.IsNullOrEmpty(assemblyDirectory))
        {
            assemblyDirectory = AppContext.BaseDirectory;
        }

        var currentDir = Directory.GetCurrentDirectory();

        return
        [
            // Where `dotnet publish` places a RID-specific native asset.
            Path.GetFullPath(Path.Combine(assemblyDirectory, "runtimes", rid, "native", fileName)),

            // Where a plain build copies it.
            Path.GetFullPath(Path.Combine(assemblyDirectory, fileName)),

            // Probe current working directory
            Path.GetFullPath(Path.Combine(currentDir, fileName)),

            // Probe target release folder
            Path.GetFullPath(Path.Combine(currentDir, "target", "release", fileName)),

            // Probe client native folder
            Path.GetFullPath(Path.Combine(currentDir, "client", "native", rid, fileName)),
        ];
    }

    private static nint Resolve(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (!string.Equals(libraryName, DbSimNative.Library, StringComparison.Ordinal))
        {
            // Not ours. Returning zero defers to the default resolver rather than claiming an
            // import this component knows nothing about.
            return nint.Zero;
        }

        var rid = CurrentRuntimeIdentifier();
        if (rid is null)
        {
            throw new PlatformNotSupportedException(
                $"Dungeon Barrage has no native simulation library for " +
                $"{RuntimeInformation.OSDescription} on {RuntimeInformation.ProcessArchitecture}.");
        }

        var candidates = CandidatePaths();
        foreach (var candidate in candidates)
        {
            if (File.Exists(candidate) && NativeLibrary.TryLoad(candidate, out var loaded))
            {
                return loaded;
            }
        }

        throw new DllNotFoundException(
            $"Could not load the native simulation library for {rid}. Searched: " +
            string.Join("; ", candidates));
    }
}
