using System.Text.Json;

namespace DungeonBarrage.Client.App;

internal sealed record C4SmokeOptions(string ReportPath, string ScreenshotPath)
{
    private const string ReportArgument = "--c4-smoke-report";
    private const string ScreenshotArgument = "--c4-screenshot";

    internal static C4SmokeOptions? Parse(IReadOnlyList<string> arguments)
    {
        string? report = null;
        string? screenshot = null;

        for (var index = 0; index < arguments.Count; index++)
        {
            switch (arguments[index])
            {
                case ReportArgument when index + 1 < arguments.Count:
                    report = arguments[++index];
                    break;
                case ScreenshotArgument when index + 1 < arguments.Count:
                    screenshot = arguments[++index];
                    break;
            }
        }

        if (report is null && screenshot is null)
        {
            return null;
        }

        if (string.IsNullOrWhiteSpace(report) || string.IsNullOrWhiteSpace(screenshot))
        {
            throw new ArgumentException(
                $"C4 smoke mode requires both {ReportArgument} and {ScreenshotArgument}.");
        }

        return new C4SmokeOptions(Path.GetFullPath(report), Path.GetFullPath(screenshot));
    }
}

internal sealed record C4SmokeReport(
    bool Success,
    string? Error,
    string WorkingDirectory,
    string ExecutablePath,
    string ClientVersion,
    string GodotVersion,
    uint AbiVersion,
    uint SimulationVersion,
    uint ContentVersion,
    string MatchId,
    string StateHash,
    uint SnapshotGeneration,
    uint TerrainWidth,
    uint TerrainHeight,
    int TerrainByteCount,
    int SolidTerrainCellCount,
    int BlockCount,
    int PlayerCount,
    uint PositionScale,
    uint FixedTickRate,
    int ScreenshotWidth,
    int ScreenshotHeight,
    bool SessionDisposed,
    bool DisposedSessionRejectedReuse,
    IReadOnlyList<string> NativeLibraryCandidates)
{
    private static readonly JsonSerializerOptions SerializerOptions = new(JsonSerializerOptions.Default)
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        WriteIndented = true,
    };

    internal void Write(string path)
    {
        var directory = Path.GetDirectoryName(path);
        if (string.IsNullOrWhiteSpace(directory))
        {
            throw new InvalidOperationException($"Smoke report has no parent directory: {path}");
        }

        Directory.CreateDirectory(directory);
        File.WriteAllText(path, JsonSerializer.Serialize(this, SerializerOptions));
    }
}
