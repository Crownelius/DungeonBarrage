using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Interpolates an authoritative projectile trace to a presentation tick.
/// </summary>
/// <remarks>
/// CLIENT_SPEC §13.4: one view per <c>projectileTrace</c> ID, interpolate between that
/// trace's samples, never extrapolate past the last sample, never merge traces.
/// </remarks>
public static class ProjectilePlayback
{
    /// <summary>
    /// Position of <paramref name="trace"/> at <paramref name="tick"/>, or
    /// <see langword="null"/> when the trace has no samples.
    /// </summary>
    /// <param name="trace">One complete authoritative path.</param>
    /// <param name="tick">Presentation tick, 0 at fire.</param>
    /// <returns>The sampled or linearly interpolated position.</returns>
    public static ClientPosition? PositionAt(ClientProjectileTrace trace, uint tick)
    {
        ArgumentNullException.ThrowIfNull(trace);
        if (trace.Samples.Count == 0)
        {
            return null;
        }

        var first = trace.Samples[0];
        if (tick <= first.Tick)
        {
            return first.Position;
        }

        var last = trace.Samples[trace.Samples.Count - 1];
        if (tick >= last.Tick)
        {
            return last.Position;
        }

        for (var i = 0; i < trace.Samples.Count - 1; i++)
        {
            var a = trace.Samples[i];
            var b = trace.Samples[i + 1];
            if (tick < a.Tick || tick > b.Tick)
            {
                continue;
            }

            var span = b.Tick - a.Tick;
            if (span == 0)
            {
                return b.Position;
            }

            var t = (tick - a.Tick) / (float)span;
            return new ClientPosition(
                a.Position.X + (int)MathF.Round((b.Position.X - a.Position.X) * t),
                a.Position.Y + (int)MathF.Round((b.Position.Y - a.Position.Y) * t));
        }

        return last.Position;
    }

    /// <summary>
    /// Presentation tick corresponding to <paramref name="elapsedMsec"/> at
    /// <paramref name="tickRate"/>, clamped to <paramref name="lockTicks"/>.
    /// </summary>
    /// <param name="elapsedMsec">Milliseconds since the shot was accepted.</param>
    /// <param name="tickRate">Transition <c>presentationTickRate</c>.</param>
    /// <param name="lockTicks">Transition <c>inputLockTicks</c>.</param>
    /// <returns>The tick the projectile view should show.</returns>
    public static uint TickAt(ulong elapsedMsec, uint tickRate, uint lockTicks)
    {
        if (tickRate == 0)
        {
            return lockTicks;
        }

        var ticks = elapsedMsec * tickRate / 1000UL;
        return ticks >= lockTicks ? lockTicks : (uint)ticks;
    }

    /// <summary>A flight shorter than this is stretched so the player can see the hit.</summary>
    public const uint MinimumFlightMsec = 1100;

    /// <summary>Hold the impact pose after the last flight tick.</summary>
    public const uint ImpactHoldMsec = 500;

    /// <summary>Presentation-only ticks used to carry a returning weapon back to the thrower.</summary>
    public const uint ReturnLegTicks = 24;

    /// <summary>Last sampled tick on one trace, or 0 when empty.</summary>
    /// <param name="trace">One complete authoritative path.</param>
    /// <returns>The last sample tick.</returns>
    public static uint LastSampleTick(ClientProjectileTrace trace)
    {
        ArgumentNullException.ThrowIfNull(trace);
        if (trace.Samples.Count == 0)
        {
            return trace.TerminalImpact.Tick;
        }

        var last = trace.Samples[trace.Samples.Count - 1].Tick;
        return last > trace.TerminalImpact.Tick ? last : trace.TerminalImpact.Tick;
    }

    /// <summary>Whether this ability is presented as flying out and coming back.</summary>
    /// <param name="abilityId">Authoritative ability id.</param>
    /// <returns><see langword="true"/> for the returning boomerang family.</returns>
    public static bool IsReturningWeapon(string? abilityId) =>
        abilityId is not null
        && abilityId.Contains("boomerang", StringComparison.OrdinalIgnoreCase);

    /// <summary>
    /// Tick rate that plays <paramref name="lastSampleTick"/> over at least
    /// <see cref="MinimumFlightMsec"/>, never faster than the authority rate.
    /// </summary>
    /// <param name="lastSampleTick">Last sample on the longest trace.</param>
    /// <param name="authorityTickRate">Transition <c>presentationTickRate</c>.</param>
    /// <returns>A tick rate the view can feed to <see cref="TickAt"/>.</returns>
    public static uint VisualTickRate(uint lastSampleTick, uint authorityTickRate)
    {
        var authority = authorityTickRate == 0 ? 60u : authorityTickRate;
        if (lastSampleTick == 0)
        {
            return authority;
        }

        var oneToOneMsec = lastSampleTick * 1000.0 / authority;
        if (oneToOneMsec >= MinimumFlightMsec)
        {
            return authority;
        }

        var slowed = lastSampleTick * 1000u / MinimumFlightMsec;
        return slowed == 0 ? 1u : slowed;
    }

    /// <summary>Wall-clock length of flight plus the impact hold.</summary>
    /// <param name="lastSampleTick">Last sample on the longest trace.</param>
    /// <param name="visualTickRate">Rate from <see cref="VisualTickRate"/>.</param>
    /// <returns>Milliseconds the view should keep input locked.</returns>
    public static ulong PlaybackMsec(uint lastSampleTick, uint visualTickRate)
    {
        var rate = visualTickRate == 0 ? 1u : visualTickRate;
        var flight = lastSampleTick == 0
            ? MinimumFlightMsec
            : (ulong)lastSampleTick * 1000UL / rate;
        if (flight < MinimumFlightMsec)
        {
            flight = MinimumFlightMsec;
        }

        return flight + ImpactHoldMsec;
    }

    /// <summary>
    /// Appends a straight catch-return from the terminal impact back to
    /// <paramref name="catcher"/>. Does not change <see cref="ClientProjectileTrace.TerminalImpact"/>,
    /// so the hit stays where the authority placed it.
    /// </summary>
    /// <param name="outbound">Authoritative outbound path.</param>
    /// <param name="catcher">Thrower position the view flies back to.</param>
    /// <param name="returnTicks">How many presentation ticks the return takes.</param>
    /// <returns>A new trace with the return samples appended.</returns>
    public static ClientProjectileTrace WithReturnTo(
        ClientProjectileTrace outbound,
        ClientPosition catcher,
        uint returnTicks)
    {
        ArgumentNullException.ThrowIfNull(outbound);
        ArgumentNullException.ThrowIfNull(catcher);
        if (returnTicks == 0 || outbound.Samples.Count == 0)
        {
            return outbound;
        }

        var samples = new List<ClientProjectileSample>(outbound.Samples);
        var last = samples[samples.Count - 1];
        for (var step = 1u; step <= returnTicks; step++)
        {
            var t = step / (float)returnTicks;
            samples.Add(new ClientProjectileSample(
                last.Tick + step,
                new ClientPosition(
                    last.Position.X + (int)MathF.Round((catcher.X - last.Position.X) * t),
                    last.Position.Y + (int)MathF.Round((catcher.Y - last.Position.Y) * t))));
        }

        return outbound with { Samples = samples };
    }
}
