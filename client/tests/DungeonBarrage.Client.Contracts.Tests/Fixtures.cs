namespace DungeonBarrage.Client.Contracts.Tests;

/// <summary>Reads the canonical frozen native response fixtures copied beside the test assembly.</summary>
internal static class Fixtures
{
    /// <summary>Reads one response fixture as exact bytes.</summary>
    /// <param name="fileName">Fixture file name.</param>
    /// <returns>The exact fixture bytes.</returns>
    internal static ReadOnlyMemory<byte> Read(string fileName)
    {
        var fullPath = Path.Combine(AppContext.BaseDirectory, "fixtures", fileName);
        if (!File.Exists(fullPath))
        {
            throw new FileNotFoundException(
                $"Response fixture '{fileName}' was not copied beside the test binary. Expected: {fullPath}",
                fullPath);
        }

        return File.ReadAllBytes(fullPath);
    }
}
