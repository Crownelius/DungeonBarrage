using System.Reflection;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// Reads the frozen fixture corpus copied beside the test binary.
/// </summary>
/// <remarks>
/// Files are read as raw bytes and never as text. Reading them as strings would let the runtime
/// normalize a byte-order mark or line ending, and the comparison these tests perform is byte
/// equality against exactly what Rust froze.
/// </remarks>
internal static class Fixtures
{
    private const string MatchId = "horizontal-test-duel-v1";

    /// <summary>Reads one fixture file, relative to the duel fixture root.</summary>
    /// <param name="relativePath">Path such as <c>commands/001-move.json</c>.</param>
    /// <returns>The exact file bytes.</returns>
    internal static ReadOnlyMemory<byte> Read(string relativePath)
    {
        var full = Path.Combine(Root, relativePath.Replace('/', Path.DirectorySeparatorChar));
        if (!File.Exists(full))
        {
            throw new FileNotFoundException(
                $"Fixture '{relativePath}' was not copied beside the test binary. Expected: {full}",
                full);
        }

        return File.ReadAllBytes(full);
    }

    private static string Root
    {
        get
        {
            var directory = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location)
                ?? throw new InvalidOperationException("The test assembly has no directory.");
            return Path.Combine(directory, "fixtures", MatchId);
        }
    }
}
