using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>An engine-neutral floating-point point in presentation space.</summary>
/// <param name="X">Horizontal presentation coordinate.</param>
/// <param name="Y">Vertical presentation coordinate.</param>
public readonly record struct PresentationPoint(float X, float Y);

/// <summary>An engine-neutral projected circle in presentation space.</summary>
/// <param name="Center">Projected authoritative center.</param>
/// <param name="Radius">Projected authoritative radius.</param>
public readonly record struct PresentationCircle(PresentationPoint Center, float Radius)
{
    /// <summary>Whether <paramref name="point"/> is inside or on this circle.</summary>
    /// <param name="point">Presentation point to test.</param>
    /// <returns><see langword="true"/> when the point is contained.</returns>
    public bool Contains(PresentationPoint point)
    {
        var dx = point.X - Center.X;
        var dy = point.Y - Center.Y;
        return (dx * dx) + (dy * dy) <= Radius * Radius;
    }
}

/// <summary>
/// The authoritative character collision body, kept free of Godot types so fixtures can prove
/// that a reported character impact overlaps the same body the client renders.
/// </summary>
/// <param name="Center">Authoritative fixed-point collision center.</param>
/// <param name="Radius">Authoritative fixed-point collision radius.</param>
public readonly record struct CharacterBodyGeometry(ClientPosition Center, int Radius)
{
    /// <summary>Creates validated geometry from one authoritative player snapshot.</summary>
    /// <param name="player">Player snapshot carrying the authoritative collider.</param>
    /// <returns>The validated collision body.</returns>
    /// <exception cref="ArgumentNullException"><paramref name="player"/> or its center is null.</exception>
    /// <exception cref="InvalidDataException">The authoritative radius is not positive.</exception>
    public static CharacterBodyGeometry FromPlayer(ClientPlayerSnapshot player)
    {
        ArgumentNullException.ThrowIfNull(player);
        ArgumentNullException.ThrowIfNull(player.CollisionCenter);
        if (player.CollisionRadius <= 0)
        {
            throw new InvalidDataException(
                $"Player '{player.PlayerId}' has invalid collision radius {player.CollisionRadius}.");
        }

        return new CharacterBodyGeometry(player.CollisionCenter, player.CollisionRadius);
    }

    /// <summary>Whether an authoritative fixed-point position is inside or on the body.</summary>
    /// <param name="point">Authoritative position to test.</param>
    /// <returns><see langword="true"/> when the point is contained.</returns>
    public bool Contains(ClientPosition point)
    {
        ArgumentNullException.ThrowIfNull(point);
        if (Radius <= 0)
        {
            return false;
        }

        var dx = (long)point.X - Center.X;
        var dy = (long)point.Y - Center.Y;
        if (Math.Abs(dx) > Radius || Math.Abs(dy) > Radius)
        {
            return false;
        }

        var radius = (long)Radius;
        return (dx * dx) + (dy * dy) <= radius * radius;
    }

    /// <summary>Projects this body through the shared fixed-point-to-presentation transform.</summary>
    /// <param name="positionScale">Authoritative fixed-point units per terrain cell.</param>
    /// <param name="pixelsPerCell">Presentation pixels per terrain cell.</param>
    /// <param name="worldOrigin">Presentation origin of simulation coordinate zero.</param>
    /// <param name="cameraOffset">Presentation-only camera translation.</param>
    /// <returns>The projected center and radius.</returns>
    public PresentationCircle Project(
        int positionScale,
        float pixelsPerCell,
        PresentationPoint worldOrigin,
        PresentationPoint cameraOffset)
    {
        if (Radius <= 0)
        {
            throw new InvalidDataException($"Collision radius {Radius} must be positive.");
        }

        return new PresentationCircle(
            WorldProjection.ToPresentation(
                Center,
                positionScale,
                pixelsPerCell,
                worldOrigin,
                cameraOffset),
            WorldProjection.LengthToPresentation(Radius, positionScale, pixelsPerCell));
    }
}

/// <summary>Shared engine-neutral fixed-point-to-presentation projection.</summary>
public static class WorldProjection
{
    /// <summary>Projects one authoritative point to presentation coordinates.</summary>
    /// <param name="position">Authoritative fixed-point point.</param>
    /// <param name="positionScale">Authoritative fixed-point units per terrain cell.</param>
    /// <param name="pixelsPerCell">Presentation pixels per terrain cell.</param>
    /// <param name="worldOrigin">Presentation origin of simulation coordinate zero.</param>
    /// <param name="cameraOffset">Presentation-only camera translation.</param>
    /// <returns>The projected point.</returns>
    public static PresentationPoint ToPresentation(
        ClientPosition position,
        int positionScale,
        float pixelsPerCell,
        PresentationPoint worldOrigin,
        PresentationPoint cameraOffset)
    {
        ArgumentNullException.ThrowIfNull(position);
        ValidateScale(positionScale, pixelsPerCell);
        ValidateFinite(worldOrigin, nameof(worldOrigin));
        ValidateFinite(cameraOffset, nameof(cameraOffset));

        return new PresentationPoint(
            worldOrigin.X + cameraOffset.X + (position.X / (float)positionScale * pixelsPerCell),
            worldOrigin.Y + cameraOffset.Y + (position.Y / (float)positionScale * pixelsPerCell));
    }

    /// <summary>Projects a non-negative authoritative length to presentation pixels.</summary>
    /// <param name="fixedLength">Length in authoritative fixed-point units.</param>
    /// <param name="positionScale">Authoritative fixed-point units per terrain cell.</param>
    /// <param name="pixelsPerCell">Presentation pixels per terrain cell.</param>
    /// <returns>The projected length in pixels.</returns>
    public static float LengthToPresentation(
        int fixedLength,
        int positionScale,
        float pixelsPerCell)
    {
        if (fixedLength < 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(fixedLength),
                fixedLength,
                "An authoritative length cannot be negative.");
        }

        ValidateScale(positionScale, pixelsPerCell);
        return fixedLength / (float)positionScale * pixelsPerCell;
    }

    private static void ValidateScale(int positionScale, float pixelsPerCell)
    {
        if (positionScale <= 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(positionScale),
                positionScale,
                "Position scale must be positive.");
        }

        if (!float.IsFinite(pixelsPerCell) || pixelsPerCell <= 0)
        {
            throw new ArgumentOutOfRangeException(
                nameof(pixelsPerCell),
                pixelsPerCell,
                "Pixels per cell must be finite and positive.");
        }
    }

    private static void ValidateFinite(PresentationPoint point, string parameterName)
    {
        if (!float.IsFinite(point.X) || !float.IsFinite(point.Y))
        {
            throw new ArgumentOutOfRangeException(
                parameterName,
                point,
                "Presentation coordinates must be finite.");
        }
    }
}
