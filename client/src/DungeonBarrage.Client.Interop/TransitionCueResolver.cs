using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>Closed, event-derived actor feedback states for the presentation layer.</summary>
public enum ActorPresentationCueKind
{
    /// <summary>The actor has released an authoritative projectile trace.</summary>
    Fire,

    /// <summary>The actor has received an authoritative health decrease.</summary>
    Hit,

    /// <summary>The actor has received an authoritative elimination event.</summary>
    Defeat,
}

/// <summary>One transient, cosmetic actor cue derived from an authoritative event.</summary>
/// <param name="PlayerId">The affected actor.</param>
/// <param name="Kind">Closed feedback state.</param>
/// <param name="Age01">Normalized age from 0 at event release toward 1 at expiry.</param>
/// <param name="Sequence">The source transition-local event sequence.</param>
/// <param name="AbilityId">The firing ability when <paramref name="Kind"/> is <see cref="ActorPresentationCueKind.Fire"/>.</param>
/// <param name="Value">Optional damage or health change value associated with the cue.</param>
public sealed record ActorPresentationCue(
    string PlayerId,
    ActorPresentationCueKind Kind,
    float Age01,
    uint Sequence,
    string? AbilityId,
    int? Value = null);

/// <summary>One transient, cosmetic impact cue at an authoritative fixed-point position.</summary>
/// <param name="Position">Exact authoritative impact position.</param>
/// <param name="Cause">Closed authoritative impact cause.</param>
/// <param name="Age01">Normalized age from 0 at event release toward 1 at expiry.</param>
/// <param name="Sequence">The source transition-local event sequence.</param>
public sealed record ImpactPresentationCue(
    ClientPosition Position,
    ClientImpactCause Cause,
    float Age01,
    uint Sequence);

/// <summary>Temporary camera translation expressed in terrain-cell units.</summary>
/// <param name="CellX">Horizontal presentation translation in cells.</param>
/// <param name="CellY">Vertical presentation translation in cells.</param>
public readonly record struct PresentationCameraImpulse(float CellX, float CellY)
{
    /// <summary>No camera presentation translation.</summary>
    public static PresentationCameraImpulse None => default;

    /// <summary>Whether either presentation axis is non-zero.</summary>
    public bool IsActive => CellX != 0f || CellY != 0f;
}

/// <summary>
/// The presentation-only feedback active at one wall-clock point in a transition playback.
/// </summary>
/// <param name="ActorCues">At most one highest-priority cue for each actor.</param>
/// <param name="ImpactCues">All currently active authoritative impact cues in event order.</param>
/// <param name="CameraImpulse">A transient, optional camera translation for active impacts.</param>
public sealed record TransitionPresentationFrame(
    IReadOnlyList<ActorPresentationCue> ActorCues,
    IReadOnlyList<ImpactPresentationCue> ImpactCues,
    PresentationCameraImpulse CameraImpulse)
{
    /// <summary>An empty frame outside transition playback.</summary>
    public static TransitionPresentationFrame Empty { get; } = new(
        Array.Empty<ActorPresentationCue>(),
        Array.Empty<ImpactPresentationCue>(),
        PresentationCameraImpulse.None);

    /// <summary>Returns the active cosmetic cue for <paramref name="playerId"/>, if any.</summary>
    /// <param name="playerId">The actor identifier to search.</param>
    /// <returns>The highest-priority active actor cue, or <see langword="null"/>.</returns>
    public ActorPresentationCue? CueFor(string playerId)
    {
        ArgumentException.ThrowIfNullOrWhiteSpace(playerId);
        for (var index = 0; index < ActorCues.Count; index++)
        {
            var cue = ActorCues[index];
            if (string.Equals(cue.PlayerId, playerId, StringComparison.Ordinal))
            {
                return cue;
            }
        }

        return null;
    }
}

/// <summary>
/// Resolves transient combat feedback from immutable, ordered authoritative presentation events.
/// </summary>
/// <remarks>
/// This class is deliberately Godot-free and does not read snapshots or infer collision, damage,
/// movement paths, terrain changes, random outcomes, or command results. It only releases
/// cosmetic cues after the authority's event tick becomes visible on the existing visual clock.
/// </remarks>
public static class TransitionCueResolver
{
    /// <summary>How long a projectile-release cue remains visible.</summary>
    public const uint FireDurationMsec = 650;

    /// <summary>How long a health-decrease cue remains visible.</summary>
    public const uint HitDurationMsec = 300;

    /// <summary>How long an impact or elimination cue remains visible.</summary>
    public const uint ImpactDurationMsec = ProjectilePlayback.ImpactHoldMsec;

    /// <summary>
    /// Resolves the visual-only feedback that has become observable at <paramref name="elapsedMsec"/>.
    /// </summary>
    /// <param name="events">One authoritative transition's presentation events.</param>
    /// <param name="elapsedMsec">Wall-clock milliseconds since visual playback began.</param>
    /// <param name="visualTickRate">The playback clock rate, after any view-only slowdown.</param>
    /// <param name="reduceMotion">Whether shake must be suppressed while tactical cues remain visible.</param>
    /// <returns>A deterministic presentation frame with no authority-side effects.</returns>
    public static TransitionPresentationFrame Resolve(
        IReadOnlyList<ClientPresentationEvent> events,
        ulong elapsedMsec,
        uint visualTickRate,
        bool reduceMotion)
    {
        ArgumentNullException.ThrowIfNull(events);
        if (events.Count == 0)
        {
            return TransitionPresentationFrame.Empty;
        }

        var ordered = new List<OrderedEvent>(events.Count);
        for (var index = 0; index < events.Count; index++)
        {
            var presentationEvent = events[index]
                ?? throw new ArgumentException("Presentation events cannot contain null entries.", nameof(events));
            ordered.Add(new OrderedEvent(presentationEvent, index));
        }

        ordered.Sort(static (left, right) =>
        {
            var byTick = left.Event.PresentationTick.CompareTo(right.Event.PresentationTick);
            if (byTick != 0)
            {
                return byTick;
            }

            var bySequence = left.Event.Sequence.CompareTo(right.Event.Sequence);
            return bySequence != 0 ? bySequence : left.InputIndex.CompareTo(right.InputIndex);
        });

        var actorCues = new List<ActorPresentationCue>();
        var impactCues = new List<ImpactPresentationCue>();
        var impulseX = 0f;
        var impulseY = 0f;

        for (var index = 0; index < ordered.Count; index++)
        {
            var presentationEvent = ordered[index].Event;
            switch (presentationEvent)
            {
                case ClientProjectileTraceEvent traceEvent
                    when TryGetAge01(
                        traceEvent.PresentationTick,
                        elapsedMsec,
                        visualTickRate,
                        FireDurationMsec,
                        out var fireAge):
                    AddActorCue(
                        actorCues,
                        new ActorPresentationCue(
                            traceEvent.Trace.OwnerId,
                            ActorPresentationCueKind.Fire,
                            fireAge,
                            traceEvent.Sequence,
                            traceEvent.Trace.AbilityId));
                    break;

                case ClientHealthChangedEvent healthEvent
                    when healthEvent.NewHealth < healthEvent.PreviousHealth &&
                    TryGetAge01(
                        healthEvent.PresentationTick,
                        elapsedMsec,
                        visualTickRate,
                        HitDurationMsec,
                        out var hitAge):
                    AddActorCue(
                        actorCues,
                        new ActorPresentationCue(
                            healthEvent.PlayerId,
                            ActorPresentationCueKind.Hit,
                            hitAge,
                            healthEvent.Sequence,
                            AbilityId: null,
                            Value: (int)(healthEvent.PreviousHealth - healthEvent.NewHealth)));
                    break;

                case ClientPlayerEliminatedEvent eliminatedEvent
                    when TryGetAge01(
                        eliminatedEvent.PresentationTick,
                        elapsedMsec,
                        visualTickRate,
                        ImpactDurationMsec,
                        out var defeatAge):
                    AddActorCue(
                        actorCues,
                        new ActorPresentationCue(
                            eliminatedEvent.PlayerId,
                            ActorPresentationCueKind.Defeat,
                            defeatAge,
                            eliminatedEvent.Sequence,
                            AbilityId: null));
                    break;

                case ClientImpactEvent impactEvent
                    when TryGetAge01(
                        impactEvent.PresentationTick,
                        elapsedMsec,
                        visualTickRate,
                        ImpactDurationMsec,
                        out var impactAge):
                    var impactCue = new ImpactPresentationCue(
                        impactEvent.Impact.Position,
                        impactEvent.Impact.Cause,
                        impactAge,
                        impactEvent.Sequence);
                    impactCues.Add(impactCue);
                    if (!reduceMotion)
                    {
                        AddCameraImpulse(impactCue, ref impulseX, ref impulseY);
                    }

                    break;
            }
        }

        var impulse = reduceMotion
            ? PresentationCameraImpulse.None
            : new PresentationCameraImpulse(
                Math.Clamp(impulseX, -0.3f, 0.3f),
                Math.Clamp(impulseY, -0.2f, 0.2f));
        return new TransitionPresentationFrame(actorCues, impactCues, impulse);
    }

    private static void AddActorCue(List<ActorPresentationCue> cues, ActorPresentationCue candidate)
    {
        for (var index = 0; index < cues.Count; index++)
        {
            var existing = cues[index];
            if (!string.Equals(existing.PlayerId, candidate.PlayerId, StringComparison.Ordinal))
            {
                continue;
            }

            if (Priority(candidate.Kind) > Priority(existing.Kind) ||
                (candidate.Kind == existing.Kind && candidate.Sequence >= existing.Sequence))
            {
                cues[index] = candidate;
            }

            return;
        }

        cues.Add(candidate);
    }

    private static void AddCameraImpulse(
        ImpactPresentationCue cue,
        ref float impulseX,
        ref float impulseY)
    {
        var strength = 1f - cue.Age01;
        var horizontalDirection = (cue.Sequence & 1u) == 0 ? -1f : 1f;
        impulseX += horizontalDirection * 0.18f * strength;
        impulseY -= 0.1f * strength;
    }

    private static int Priority(ActorPresentationCueKind kind) => kind switch
    {
        ActorPresentationCueKind.Fire => 0,
        ActorPresentationCueKind.Hit => 1,
        ActorPresentationCueKind.Defeat => 2,
        _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, "Unknown actor presentation cue."),
    };

    private static bool TryGetAge01(
        uint presentationTick,
        ulong elapsedMsec,
        uint visualTickRate,
        uint durationMsec,
        out float age01)
    {
        var tickRate = visualTickRate == 0 ? 1u : visualTickRate;
        var eventMsec = (ulong)presentationTick * 1000UL / tickRate;
        if (elapsedMsec < eventMsec)
        {
            age01 = default;
            return false;
        }

        var ageMsec = elapsedMsec - eventMsec;
        if (ageMsec >= durationMsec)
        {
            age01 = default;
            return false;
        }

        age01 = ageMsec / (float)durationMsec;
        return true;
    }

    private sealed record OrderedEvent(ClientPresentationEvent Event, int InputIndex);
}
