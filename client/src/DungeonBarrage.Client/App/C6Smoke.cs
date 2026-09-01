using System.Text.Json;

namespace DungeonBarrage.Client.App;

/// <summary>
/// CLI options selecting the C6 smoke path: full local match execution including loadout select,
/// bot decision turn handling, victory/draw outcome, and clean rematch.
/// </summary>
internal sealed record C6SmokeOptions(string ReportPath, string ScreenshotPath)
{
    private const string ReportArgument = "--c6-smoke-report";
    private const string ScreenshotArgument = "--c6-screenshot";

    /// <summary>
    /// Where the loadout-select screen's own screenshot is written: derived from
    /// <see cref="ScreenshotPath"/> rather than a third CLI argument, so the two-flag contract
    /// C4 and C5's smoke modes already established stays uniform across all three.
    /// </summary>
    internal string LoadoutSelectScreenshotPath =>
        Path.Combine(
            Path.GetDirectoryName(ScreenshotPath) ?? string.Empty,
            Path.GetFileNameWithoutExtension(ScreenshotPath) + "-loadout-select" + Path.GetExtension(ScreenshotPath));

    /// <summary>Where the mid-hover-float frame is written, derived the same way.</summary>
    internal string LoadoutSelectHoverScreenshotPath =>
        Path.Combine(
            Path.GetDirectoryName(ScreenshotPath) ?? string.Empty,
            Path.GetFileNameWithoutExtension(ScreenshotPath) + "-loadout-select-hover" + Path.GetExtension(ScreenshotPath));

    /// <summary>Where the LocalSetup screen's own screenshot is written, derived the same way.</summary>
    internal string LocalSetupScreenshotPath =>
        Path.Combine(
            Path.GetDirectoryName(ScreenshotPath) ?? string.Empty,
            Path.GetFileNameWithoutExtension(ScreenshotPath) + "-local-setup" + Path.GetExtension(ScreenshotPath));

    /// <summary>
    /// Where the passive-selection modal's screenshot is written, if the human's own gauge
    /// fills during the run — derived the same way. Whether this happens at all depends on the
    /// match's real combat outcome, not something this smoke path forces, so its absence alone
    /// is not a failure.
    /// </summary>
    internal string PassivePromptScreenshotPath =>
        Path.Combine(
            Path.GetDirectoryName(ScreenshotPath) ?? string.Empty,
            Path.GetFileNameWithoutExtension(ScreenshotPath) + "-passive-prompt" + Path.GetExtension(ScreenshotPath));

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
/// loadout select, bot decisions, victory/draw results, and rematch.
/// </summary>
internal sealed record C6SmokeReport(
    bool Success,
    string? Error,
    string ClientVersion,
    string GodotVersion,
    int RosterCount,
    string HumanMainItemId,
    string BotMainItemId,
    bool InitialMatchCreated,
    bool HoverAnimationInterruptionTestPassed,
    bool HumanTurnExecuted,
    bool BotTurnExecuted,
    bool PassivePromptShownForHuman,
    bool PassivePromptConfirmedThroughRealInput,
    bool MatchCompleted,
    int TurnsPlayed,
    uint FinalTurnNumber,
    string FinalStateHash,
    bool RematchSessionCreated,
    bool RematchSessionDisposedCleanly,
    int ScreenshotWidth,
    int ScreenshotHeight,
    int LoadoutSelectScreenshotWidth,
    int LoadoutSelectScreenshotHeight,
    int LocalSetupScreenshotWidth,
    int LocalSetupScreenshotHeight,
    string MapsCompleted,
    bool AllPlayableMapsCompleted,
    bool StackedBlocksFell)
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
