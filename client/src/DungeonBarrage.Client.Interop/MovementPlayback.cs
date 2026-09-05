using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Smooth visual movement interpolation for character ground pivots during transition playback.
/// </summary>
/// <remarks>
/// OpenBound Clean-Room Movement Contract:
/// <see cref="ClientEntityMovedEvent.Start"/> and <see cref="ClientEntityMovedEvent.End"/> are ground pivots.
/// Body center is <see cref="ClientPlayerSnapshot.CollisionCenter"/> with <see cref="ClientPlayerSnapshot.CollisionRadius"/>,
/// projected through <see cref="CharacterBodyGeometry"/>. Visual interpolation smoothly lerps this ground pivot,
/// never modifying the collision radius or inventing replacement hitboxes.
/// </remarks>
public static class MovementPlayback
{
    /// <summary>
    /// Searches <paramref name="events"/> for the entity movement event belonging to <paramref name="playerId"/>.
    /// </summary>
    /// <param name="events">Collection of presentation events from the transition.</param>
    /// <param name="playerId">Identifier of the player to find movement for.</param>
    /// <returns>The first matching <see cref="ClientEntityMovedEvent"/>, or <see langword="null"/> if none occurred.</returns>
    public static ClientEntityMovedEvent? FindMovementEvent(
        IReadOnlyList<ClientPresentationEvent> events,
        string playerId)
    {
        ArgumentNullException.ThrowIfNull(events);
        ArgumentNullException.ThrowIfNull(playerId);

        for (var i = 0; i < events.Count; i++)
        {
            if (events[i] is ClientEntityMovedEvent moved && string.Equals(moved.PlayerId, playerId, StringComparison.Ordinal))
            {
                return moved;
            }
        }

        return null;
    }

    /// <summary>
    /// Interpolates <paramref name="player"/>'s ground pivot and collision center between
    /// <see cref="ClientEntityMovedEvent.Start"/> and <see cref="ClientEntityMovedEvent.End"/>
    /// based on visual playback tick progression.
    /// </summary>
    /// <param name="player">The base player snapshot (typically from the pre-transition state).</param>
    /// <param name="moveEvent">The authoritative movement event specifying ground start and end pivots.</param>
    /// <param name="currentTick">Current visual presentation tick.</param>
    /// <param name="lockTicks">Total input lock / playback ticks for this transition.</param>
    /// <param name="reduceMotion">Whether reduced motion accessibility mode is active.</param>
    /// <returns>A new <see cref="ClientPlayerSnapshot"/> with updated <see cref="ClientPlayerSnapshot.Position"/>
    /// and <see cref="ClientPlayerSnapshot.CollisionCenter"/>, keeping <see cref="ClientPlayerSnapshot.CollisionRadius"/> intact.</returns>
    public static ClientPlayerSnapshot InterpolatePlayer(
        ClientPlayerSnapshot player,
        ClientEntityMovedEvent moveEvent,
        uint currentTick,
        uint lockTicks,
        bool reduceMotion = false)
    {
        ArgumentNullException.ThrowIfNull(player);
        ArgumentNullException.ThrowIfNull(moveEvent);

        if (reduceMotion || lockTicks <= moveEvent.PresentationTick)
        {
            var isPostImpact = currentTick >= moveEvent.PresentationTick;
            var targetPos = isPostImpact ? moveEvent.End : moveEvent.Start;
            var targetOffsetX = targetPos.X - moveEvent.Start.X;
            var targetOffsetY = targetPos.Y - moveEvent.Start.Y;
            return player with
            {
                Position = targetPos,
                CollisionCenter = new ClientPosition(
                    player.CollisionCenter.X + targetOffsetX,
                    player.CollisionCenter.Y + targetOffsetY),
            };
        }

        float t;
        if (currentTick <= moveEvent.PresentationTick)
        {
            t = 0f;
        }
        else
        {
            var moveDurationTicks = lockTicks - moveEvent.PresentationTick;
            t = Math.Clamp((float)(currentTick - moveEvent.PresentationTick) / moveDurationTicks, 0f, 1f);
        }

        // Smoothstep cubic easing: 3t^2 - 2t^3
        var smoothT = t * t * (3f - (2f * t));

        var currentX = (int)MathF.Round(moveEvent.Start.X + ((moveEvent.End.X - moveEvent.Start.X) * smoothT));
        var currentY = (int)MathF.Round(moveEvent.Start.Y + ((moveEvent.End.Y - moveEvent.Start.Y) * smoothT));

        var currentPos = new ClientPosition(currentX, currentY);
        var offsetX = currentX - moveEvent.Start.X;
        var offsetY = currentY - moveEvent.Start.Y;

        var currentCenter = new ClientPosition(
            player.CollisionCenter.X + offsetX,
            player.CollisionCenter.Y + offsetY);

        return player with
        {
            Position = currentPos,
            CollisionCenter = currentCenter,
        };
    }
}
