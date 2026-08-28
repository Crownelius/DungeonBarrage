using System.Text.Json;

namespace DungeonBarrage.Client.App;

/// <summary>
/// CLI options selecting the C6-timeout smoke path: proves the local planning clock ends a
/// human's turn automatically, through the real production trigger, without any command ever
/// being submitted for it.
/// </summary>
internal sealed record C6TimeoutSmokeOptions(string ReportPath, string ScreenshotPath)
{
    private const string ReportArgument = "--c6t-smoke-report";
    private const string ScreenshotArgument = "--c6t-screenshot";

    /// <summary>Where the just-started match's own screenshot is written, showing the initial countdown.</summary>
    internal string StartScreenshotPath =>
        Path.Combine(
            Path.GetDirectoryName(ScreenshotPath) ?? string.Empty,
            Path.GetFileNameWithoutExtension(ScreenshotPath) + "-start" + Path.GetExtension(ScreenshotPath));

    internal static C6TimeoutSmokeOptions? Parse(IReadOnlyList<string> arguments)
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
                $"C6-timeout smoke mode requires both {ReportArgument} and {ScreenshotArgument}.");
        }

        return new C6TimeoutSmokeOptions(Path.GetFullPath(report), Path.GetFullPath(screenshot));
    }
}

/// <summary>
/// Machine-checkable evidence for CLIENT_SPEC §9.1's local planning clock: a human's turn that is
/// never acted on ends on its own, through <c>Main._Process</c>'s real automatic trigger — the
/// same trigger a real idle player would hit — not through a direct <c>SubmitTimeoutAsync</c> call.
/// </summary>
internal sealed record C6TimeoutSmokeReport(
    bool Success,
    string? Error,
    string ClientVersion,
    string GodotVersion,
    double ConfiguredDeadlineSeconds,
    bool CountdownWasVisibleAtStart,
    bool TimeoutTriggeredAutomatically,
    uint TurnNumberBeforeTimeout,
    uint TurnNumberAfterTimeout,
    string? ActivePlayerIdAfterTimeout,
    int StartScreenshotWidth,
    int StartScreenshotHeight,
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
