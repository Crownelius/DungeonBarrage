using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// The single authoritative trajectory selected for the uncluttered aim guide.
/// </summary>
/// <param name="Trace">One trace returned by the Rust preview.</param>
/// <param name="HitsTarget">
/// Whether Rust reported a character as the terminal impact. Projectile integration excludes
/// the shooter, so this is a living opponent rather than a screen-space collision guess.
/// </param>
public sealed record AimPreviewPresentation(
    ClientProjectileTrace Trace,
    bool HitsTarget);

/// <summary>
/// Reduces a possibly multi-projectile authoritative preview to one tactical guide.
/// </summary>
/// <remarks>
/// This resolver never integrates a path or tests geometry. It prefers the lowest-id trace
/// that Rust says hits a character; if none hit, it shows the lowest-id miss. That keeps the
/// one-line UI promise while preserving the useful answer for cluster/repeater attacks.
/// </remarks>
public static class AimPreviewPresentationResolver
{
    /// <summary>Chooses the one trace the Godot view should draw.</summary>
    public static AimPreviewPresentation? Resolve(IReadOnlyList<ClientProjectileTrace> traces)
    {
        ArgumentNullException.ThrowIfNull(traces);

        ClientProjectileTrace? first = null;
        ClientProjectileTrace? firstHit = null;
        for (var index = 0; index < traces.Count; index++)
        {
            var trace = traces[index]
                ?? throw new ArgumentException("Projectile traces cannot contain null entries.", nameof(traces));
            if (first is null || trace.TraceId < first.TraceId)
            {
                first = trace;
            }

            if (trace.TerminalImpact.Cause == ClientImpactCause.Character &&
                (firstHit is null || trace.TraceId < firstHit.TraceId))
            {
                firstHit = trace;
            }
        }

        var selected = firstHit ?? first;
        return selected is null
            ? null
            : new AimPreviewPresentation(selected, firstHit is not null);
    }
}
