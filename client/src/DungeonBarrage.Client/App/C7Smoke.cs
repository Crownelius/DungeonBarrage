using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;

namespace DungeonBarrage.Client.App;

internal sealed record C7SmokeOptions(string ReportPath, string ScreenshotPath)
{
    private const string ReportArgument = "--c7-smoke-report";
    private const string ScreenshotArgument = "--c7-screenshot";

    internal string SettingsScreenshotPath =>
        Path.Combine(
            Path.GetDirectoryName(ScreenshotPath) ?? string.Empty,
            Path.GetFileNameWithoutExtension(ScreenshotPath) + "-settings" + Path.GetExtension(ScreenshotPath));

    internal static C7SmokeOptions? Parse(IReadOnlyList<string> arguments)
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
                $"C7 smoke mode requires both {ReportArgument} and {ScreenshotArgument}.");
        }

        return new C7SmokeOptions(Path.GetFullPath(report), Path.GetFullPath(screenshot));
    }
}

internal sealed record C7SmokeReport(
    bool Success,
    string? Error,
    string ClientVersion,
    string GodotVersion,
    bool SettingsRecoveryVerified,
    bool AudioClampingVerified,
    bool AccessibilityScalingVerified,
    bool LocalizationVerified,
    bool PerformanceTierSwitchVerified,
    bool MultiPlatformExportPresetsVerified,
    int ScreenshotWidth,
    int ScreenshotHeight,
    int SettingsScreenshotWidth,
    int SettingsScreenshotHeight)
{
    private static readonly JsonSerializerOptions SerializerOptions = new() { WriteIndented = true };

    internal void Write(string path)
    {
        var directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrWhiteSpace(directory))
        {
            Directory.CreateDirectory(directory);
        }

        File.WriteAllText(path, JsonSerializer.Serialize(this, SerializerOptions));
    }
}
