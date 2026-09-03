namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Click-anywhere slingshot aim. Drag <b>away</b> from the other crow; release fires the
/// opposite way.
/// </summary>
/// <remarks>
/// <list type="bullet">
/// <item>Crow on the left: drag left (and down for a high lob).</item>
/// <item>Crow on the right: drag right.</item>
/// <item>The rubber-band's length is power. A 20-cell line is 100% of this turn's maximum.</item>
/// <item>Walking spends movement; this turn's maximum is that remaining fraction, floored at 10%.</item>
/// <item>Launch angle is the opposite of the drag, wrapped into [0, 360_000).</item>
/// </list>
/// </remarks>
public static class AimSolver
{
    /// <summary>Presentation pixels per authoritative cell. Must match the Godot view.</summary>
    public const int PixelsPerCell = 12;

    /// <summary>Drawn-line length that maps to 100% of this turn's maximum power.</summary>
    public const int FullPowerPixels = 20 * PixelsPerCell;

    /// <summary>Authority maximum when no movement has been spent.</summary>
    public const int MaxPowerBasisPoints = 10_000;

    /// <summary>After a full-move turn the shot still has this much power, 10%.</summary>
    public const int MinimumMaxPowerBasisPoints = 1_000;

    /// <summary>Below this, release is a cancel.</summary>
    public const int MinimumFirePowerBasisPoints = 300;

    /// <summary>The drag must travel this far away from the opponent.</summary>
    public const float MinimumHorizontalPixels = 12f;

    /// <summary>Crow Normal class: 4 body-widths, 4 cells each, in fixed-point units.</summary>
    public static int CrowMovementAllowance(int positionScale) => 16 * positionScale;

    /// <summary>
    /// Whether the actor is on the left of the opponent and therefore drags left (away).
    /// </summary>
    /// <param name="actorX">Actor fixed-point X.</param>
    /// <param name="opponentX">Living opponent fixed-point X, or <see langword="null"/>.</param>
    /// <returns><see langword="true"/> when the actor is on the left.</returns>
    public static bool FacesRight(int actorX, int? opponentX)
    {
        if (opponentX is not { } other)
        {
            return true;
        }

        return actorX <= other;
    }

    /// <summary>
    /// Maximum launch power this turn after walking. 20-cell line still means 100% of this value.
    /// </summary>
    /// <param name="movementRemaining">Snapshot <c>movementRemaining</c>.</param>
    /// <param name="movementAllowance">Full walk budget for the crow this turn.</param>
    /// <returns>Basis points in [1000, 10000].</returns>
    public static int MaxPowerAfterMovement(int movementRemaining, int movementAllowance)
    {
        if (movementAllowance <= 0)
        {
            return MaxPowerBasisPoints;
        }

        var remaining = Math.Max(0, movementRemaining);
        var scaled = (int)((long)MaxPowerBasisPoints * remaining / movementAllowance);
        return Math.Max(scaled, MinimumMaxPowerBasisPoints);
    }

    /// <summary>
    /// Builds the command from the rubber-band the player drew. Fire direction is opposite
    /// the drag (slingshot).
    /// </summary>
    /// <param name="originX">Mouse-down X.</param>
    /// <param name="originY">Mouse-down Y. +Y is down.</param>
    /// <param name="cursorX">Current pointer X.</param>
    /// <param name="cursorY">Current pointer Y. +Y is down.</param>
    /// <param name="facesRight"><see langword="true"/> when the crow is on the left.</param>
    /// <param name="maxPowerBasisPoints">This turn's maximum, after walking.</param>
    /// <returns>Wrapped millidegrees, power, and whether release fires.</returns>
    public static AimSolution FromDrag(
        float originX,
        float originY,
        float cursorX,
        float cursorY,
        bool facesRight,
        int maxPowerBasisPoints) =>
        FromDrag(originX, originY, cursorX, cursorY, facesRight, maxPowerBasisPoints, PixelsPerCell);

    /// <summary>
    /// Builds the command from the rubber-band the player drew, using the live cell size so
    /// a 20-cell pull is 100% even after the view scales the map to the window.
    /// </summary>
    /// <param name="originX">Mouse-down X.</param>
    /// <param name="originY">Mouse-down Y. +Y is down.</param>
    /// <param name="cursorX">Current pointer X.</param>
    /// <param name="cursorY">Current pointer Y. +Y is down.</param>
    /// <param name="facesRight"><see langword="true"/> when the crow is on the left.</param>
    /// <param name="maxPowerBasisPoints">This turn's maximum, after walking.</param>
    /// <param name="pixelsPerCell">Presentation pixels per authoritative cell.</param>
    /// <returns>Wrapped millidegrees, power, and whether release fires.</returns>
    public static AimSolution FromDrag(
        float originX,
        float originY,
        float cursorX,
        float cursorY,
        bool facesRight,
        int maxPowerBasisPoints,
        float pixelsPerCell)
    {
        var cap = Math.Clamp(maxPowerBasisPoints, MinimumMaxPowerBasisPoints, MaxPowerBasisPoints);
        var cell = pixelsPerCell > 1f ? pixelsPerCell : PixelsPerCell;
        var fullPowerPixels = 20f * cell;
        var dx = cursorX - originX;
        var dy = cursorY - originY;
        var length = MathF.Sqrt((dx * dx) + (dy * dy));
        if (length < 1f)
        {
            return new AimSolution(facesRight ? 0 : 180_000, 0, CanFire: false, facesRight, cap);
        }

        // Away from the opponent: left crow pulls left, right crow pulls right.
        var minHorizontal = MathF.Max(cell, MinimumHorizontalPixels);
        var draggedAway = facesRight ? dx <= -minHorizontal : dx >= minHorizontal;

        // Opposite of the pull is the launch vector.
        var fireDx = -dx;
        var fireDy = -dy;
        var degrees = MathF.Atan2(-fireDy, fireDx) * (180f / MathF.PI);
        var millidegrees = (int)MathF.Round(degrees * 1000f);
        millidegrees %= 360_000;
        if (millidegrees < 0)
        {
            millidegrees += 360_000;
        }

        var power = (int)MathF.Round(length / fullPowerPixels * cap);
        if (power > cap)
        {
            power = cap;
        }

        var canFire = draggedAway && power >= MinimumFirePowerBasisPoints;
        return new AimSolution(millidegrees, power, canFire, facesRight, cap);
    }
}

/// <summary>One quantized aim the Godot view can display and submit.</summary>
/// <param name="AngleMillidegrees">Launch angle in [0, 360_000).</param>
/// <param name="PowerBasisPoints">Launch power, from line length, capped by this turn's maximum.</param>
/// <param name="CanFire">Whether release should submit.</param>
/// <param name="FacesRight">Whether the crow is on the left (must drag left, away).</param>
/// <param name="MaxPowerBasisPoints">This turn's maximum after walking.</param>
public readonly record struct AimSolution(
    int AngleMillidegrees,
    int PowerBasisPoints,
    bool CanFire,
    bool FacesRight,
    int MaxPowerBasisPoints)
{
    /// <summary>Whole degrees for the HUD, 0..359.</summary>
    public int AngleDegrees => AngleMillidegrees / 1000;

    /// <summary>Whole percent of this turn's maximum for the HUD, 0..100.</summary>
    public int PowerPercent =>
        MaxPowerBasisPoints <= 0 ? 0 : PowerBasisPoints * 100 / MaxPowerBasisPoints;
}
