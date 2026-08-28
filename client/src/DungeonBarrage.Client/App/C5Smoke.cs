using System.Text.Json;

namespace DungeonBarrage.Client.App;

/// <summary>
/// CLI options selecting the C5 smoke path: move, fire, and verify one complete authoritative
/// turn mechanically, mirroring <see cref="C4SmokeOptions"/>'s shape and parsing rules exactly.
/// </summary>
internal sealed record C5SmokeOptions(string ReportPath, string ScreenshotPath)
{
    private const string ReportArgument = "--c5-smoke-report";
    private const string ScreenshotArgument = "--c5-screenshot";

    internal static C5SmokeOptions? Parse(IReadOnlyList<string> arguments)
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
                $"C5 smoke mode requires both {ReportArgument} and {ScreenshotArgument}.");
        }

        return new C5SmokeOptions(Path.GetFullPath(report), Path.GetFullPath(screenshot));
    }
}

/// <summary>
/// Machine-checkable evidence for CLIENT_SPEC §20.5 step 5: move, fire one real shot, play its
/// transition, and reconcile to its post-snapshot — with input locked during playback.
/// </summary>
/// <remarks>
/// Deliberately does not report a "matches the frozen fixture hash" field. <c>hash_state</c>
/// folds the accepted-command-id set into the authoritative state hash
/// (<c>db-sim-core/src/hash.rs</c>, domain <c>0x04</c>), and this path's <c>LiveMatch</c> mints
/// its own ids rather than replaying the fixture's literal <c>"fixture-move-001"</c> /
/// <c>"fixture-ability-002"</c> — so its hash can never equal the frozen one, by design, not by
/// defect. What is checked instead is what is actually invariant regardless of command id:
/// acceptance, real damage, turn handoff, and reconciliation.
/// </remarks>
/// <param name="Success">Whether the whole scripted turn completed without an unexpected failure.</param>
/// <param name="Error">The failure, when <paramref name="Success"/> is <see langword="false"/>.</param>
/// <param name="ClientVersion">The running client's informational version.</param>
/// <param name="GodotVersion">The running engine's version string.</param>
/// <param name="BeforeActivePlayerId">Active player before the move, from the initial snapshot.</param>
/// <param name="MoveAccepted">Whether the move command's disposition was <c>accepted</c>.</param>
/// <param name="MoveEventCount">Presentation events the move transition carried.</param>
/// <param name="MoveDx">The exact <c>dx</c> submitted — the fixture's own frozen value.</param>
/// <param name="MoveInputLockTicks">
/// The move's own reported lock window. Expected to be zero: a plain reposition has no
/// projectile flight to play back, so there is nothing to lock input for.
/// </param>
/// <param name="AbilityAccepted">Whether the ability command's disposition was <c>accepted</c>.</param>
/// <param name="AbilityEventCount">Presentation events the ability transition carried.</param>
/// <param name="AbilityInputLockTicks">
/// The ability's own reported lock window — expected to be nonzero, since a resolved strike
/// with a projectile flight has real playback duration.
/// </param>
/// <param name="InputLockedImmediatelyAfterAbility">
/// Whether the real UI lock timer (<c>Main._inputLockedUntilMsec</c>, via
/// <c>SubmitAndRedrawAsync</c> — the same method a real click goes through) reported input
/// locked in the instant after the ability was accepted. Checked against the ability, not the
/// move, because the move's own lock window is zero and would prove nothing.
/// </param>
/// <param name="InputUnlockedAfterWaitingOutTheAbilityLock">
/// Whether that same real lock timer correctly reports input unlocked after waiting out the
/// exact duration the ability's transition reported — proof the lock is not stuck forever.
/// </param>
/// <param name="DefenderPlayerId">The player the ability targeted by proximity.</param>
/// <param name="DefenderHealthBeforeAbility">Defender health immediately before the ability.</param>
/// <param name="DefenderHealthAfterAbility">Defender health immediately after reconciliation.</param>
/// <param name="AbilityDealtRealDamage">
/// Whether the defender's health actually decreased — the concrete gameplay fact a live-generated
/// command id cannot change, standing in for the frozen-hash comparison this path cannot use.
/// </param>
/// <param name="FinalSnapshotMatchesAbilityPostSnapshot">
/// Whether the view's reconciled state (<c>LiveMatch.CurrentSnapshot</c>) is byte-identical to
/// the hash the ability transition's own <c>PostSnapshot</c> reported — the C5 gate's "every
/// view ends at the post-snapshot" clause, checked rather than assumed true by construction.
/// </param>
/// <param name="AfterActivePlayerId">Active player after the turn handed over.</param>
/// <param name="TurnHandedOverToTheOtherPlayer">
/// Whether the active player actually changed — proof the ability really ended the turn.
/// </param>
/// <param name="TurnNumberAfter">Turn number after the scripted turn completed.</param>
/// <param name="ScreenshotWidth">Captured screenshot width; zero under <c>--headless</c>.</param>
/// <param name="ScreenshotHeight">Captured screenshot height; zero under <c>--headless</c>.</param>
internal sealed record C5SmokeReport(
    bool Success,
    string? Error,
    string ClientVersion,
    string GodotVersion,
    string? BeforeActivePlayerId,
    bool MoveAccepted,
    int MoveEventCount,
    int MoveDx,
    uint MoveInputLockTicks,
    bool AbilityAccepted,
    int AbilityEventCount,
    uint AbilityInputLockTicks,
    bool InputLockedImmediatelyAfterAbility,
    bool InputUnlockedAfterWaitingOutTheAbilityLock,
    string? DefenderPlayerId,
    ushort DefenderHealthBeforeAbility,
    ushort DefenderHealthAfterAbility,
    bool AbilityDealtRealDamage,
    bool FinalSnapshotMatchesAbilityPostSnapshot,
    string? AfterActivePlayerId,
    bool TurnHandedOverToTheOtherPlayer,
    uint TurnNumberAfter,
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
