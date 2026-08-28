using System.Text.Json;

namespace DungeonBarrage.Client.App;

/// <summary>
/// CLI options selecting the C6 smoke path: full local match execution including character select,
/// bot decision turn handling, victory/draw outcome, and clean rematch.
/// </summary>
internal sealed record C6SmokeOptions(string ReportPath, string ScreenshotPath)
{
    private const string ReportArgument = "--c6-smoke-report";
    private const string ScreenshotArgument = "--c6-screenshot";

    internal static C6SmokeOptions? Parse(IReadOnlyList<string> arguments)
    {
        var argsList = arguments.Count > 0 ? arguments : Godot.OS.GetCmdlineArgs();
        string? report = null;
        string? screenshot = null;

        for (var index = 0; index < argsList.Count; index++)
        {
            switch (argsList[index])
            {
                case ReportArgument when index + 1 < argsList.Count:
                    report = argsList[++index];
                    break;
                case ScreenshotArgument when index + 1 < argsList.Count:
                    screenshot = argsList[++index];
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
                $"C6 smoke mode requires both {ReportArgument} and {ScreenshotArgument}.");
        }

        return new C6SmokeOptions(Path.GetFullPath(report), Path.GetFullPath(screenshot));
    }
}

/// <summary>
/// Machine-checkable evidence for CLIENT_SPEC §21 milestone C6: full local match flow,
/// character select, bot decisions, victory/draw results, and rematch.
/// </summary>
internal sealed record C6SmokeReport(
    bool Success,
    string? Error,
    string ClientVersion,
    string GodotVersion,
    int RosterCount,
    string HumanCharacterId,
    string BotCharacterId,
    bool InitialMatchCreated,
    bool HumanTurnExecuted,
    bool BotTurnExecuted,
    uint FinalTurnNumber,
    string FinalStateHash,
    bool RematchSessionCreated,
    bool RematchSessionDisposedCleanly,
    int ScreenshotWidth,
    int ScreenshotHeight)
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
