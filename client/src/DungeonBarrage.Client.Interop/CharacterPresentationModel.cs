using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Cosmetic ornament shape for equipped gear sockets.
/// </summary>
public enum CosmeticAccentKind
{
    /// <summary>No cosmetic accent.</summary>
    None,

    /// <summary>Spiked crown atop the head (e.g. ember-crown, sovereign-crown).</summary>
    Crown,

    /// <summary>Gleaming crystal/jewel anklet or crest (e.g. frost-anklet).</summary>
    Gem,

    /// <summary>Heavy artillery cannon or barrel silhouette (e.g. ramshot-cannon, siege-howitzer).</summary>
    Cannon,

    /// <summary>Blade or spade tool silhouette (e.g. trench-spade, scrap-scythe).</summary>
    Blade,
}

/// <summary>
/// Resolved cosmetic decoration for one equipment slot.
/// </summary>
/// <param name="ItemId">The equipped item identifier.</param>
/// <param name="Kind">Ornament classification for presentation.</param>
/// <param name="PrimaryColorHex">Accent color in RGBA hex (e.g. "#E5A93C").</param>
public sealed record EquipmentCosmeticAccent(
    string ItemId,
    CosmeticAccentKind Kind,
    string PrimaryColorHex);

/// <summary>
/// Pure C# paper-doll presentation model for a crow fighter.
/// </summary>
/// <remarks>
/// This class is deliberately Godot-free. It anchors all visual adornments, sockets, and
/// dynamic facing directly to authoritative <see cref="CharacterBodyGeometry"/> without modifying
/// the underlying collision circle.
/// </remarks>
public sealed class CharacterPresentationModel
{
    /// <summary>The projected collision body (center and radius in display pixels).</summary>
    public PresentationCircle Body { get; }

    /// <summary>Whether the fighter faces right (away from opponent when aiming).</summary>
    public bool FacesRight { get; }

    /// <summary>Facing sign multiplier: +1.0 for right, -1.0 for left.</summary>
    public float FacingSign => FacesRight ? 1f : -1f;

    /// <summary>Eye center in presentation pixels.</summary>
    public PresentationPoint EyeSocket { get; }

    /// <summary>Beak root anchor in presentation pixels.</summary>
    public PresentationPoint BeakSocket { get; }

    /// <summary>Crown / headwear anchor in presentation pixels.</summary>
    public PresentationPoint CrownSocket { get; }

    /// <summary>Weapon / hand tool anchor in presentation pixels.</summary>
    public PresentationPoint WeaponSocket { get; }

    /// <summary>Ground shadow anchor in presentation pixels.</summary>
    public PresentationPoint ShadowPivot { get; }

    /// <summary>Beak polygon triangle vertices in presentation pixels (root, tip, bottom).</summary>
    public IReadOnlyList<PresentationPoint> BeakPolygon { get; }

    /// <summary>Equipped trinket / crown cosmetic accent, or null.</summary>
    public EquipmentCosmeticAccent? TrinketAccent { get; }

    /// <summary>Equipped main weapon cosmetic accent, or null.</summary>
    public EquipmentCosmeticAccent? WeaponAccent { get; }

    private CharacterPresentationModel(
        PresentationCircle body,
        bool facesRight,
        PresentationPoint eyeSocket,
        PresentationPoint beakSocket,
        PresentationPoint crownSocket,
        PresentationPoint weaponSocket,
        PresentationPoint shadowPivot,
        IReadOnlyList<PresentationPoint> beakPolygon,
        EquipmentCosmeticAccent? trinketAccent,
        EquipmentCosmeticAccent? weaponAccent)
    {
        Body = body;
        FacesRight = facesRight;
        EyeSocket = eyeSocket;
        BeakSocket = beakSocket;
        CrownSocket = crownSocket;
        WeaponSocket = weaponSocket;
        ShadowPivot = shadowPivot;
        BeakPolygon = beakPolygon;
        TrinketAccent = trinketAccent;
        WeaponAccent = weaponAccent;
    }

    /// <summary>
    /// Builds a presentation model from a player snapshot, opponent position, and display projection parameters.
    /// </summary>
    /// <param name="player">The player snapshot.</param>
    /// <param name="opponentX">X coordinate of the nearest living opponent, or null.</param>
    /// <param name="positionScale">Authoritative fixed-point position scale.</param>
    /// <param name="cellSize">Current display pixels per terrain cell.</param>
    /// <param name="worldOrigin">Arena origin on screen.</param>
    /// <param name="cameraOffset">Current effective camera offset.</param>
    /// <returns>A fully resolved paper-doll presentation model.</returns>
    public static CharacterPresentationModel Create(
        ClientPlayerSnapshot player,
        int? opponentX,
        int positionScale,
        float cellSize,
        PresentationPoint worldOrigin,
        PresentationPoint cameraOffset)
    {
        ArgumentNullException.ThrowIfNull(player);
        var body = CharacterBodyGeometry.FromPlayer(player);
        var projected = body.Project(positionScale, cellSize, worldOrigin, cameraOffset);

        var facesRight = AimSolver.FacesRight(player.Position.X, opponentX);
        var facing = facesRight ? 1f : -1f;

        var cx = projected.Center.X;
        var cy = projected.Center.Y;
        var r = projected.Radius;

        var eyeSocket = new PresentationPoint(cx - (facing * r * 0.22f), cy - (r * 0.22f));
        var beakSocket = new PresentationPoint(cx + (facing * r * 0.95f), cy - (r * 0.05f));
        var crownSocket = new PresentationPoint(cx, cy - (r * 0.90f));
        var weaponSocket = new PresentationPoint(cx + (facing * r * 0.65f), cy + (r * 0.25f));
        var shadowPivot = new PresentationPoint(cx, cy + (r * 0.85f));

        var beakPolygon = new PresentationPoint[]
        {
            beakSocket,
            new(cx + (facing * r * 1.45f), cy + (r * 0.08f)),
            new(cx + (facing * r * 0.85f), cy + (r * 0.22f)),
        };

        var trinketAccent = ResolveTrinketAccent(player.Loadout.Trinket);
        var weaponAccent = ResolveWeaponAccent(player.Loadout.Main);

        return new CharacterPresentationModel(
            projected,
            facesRight,
            eyeSocket,
            beakSocket,
            crownSocket,
            weaponSocket,
            shadowPivot,
            beakPolygon,
            trinketAccent,
            weaponAccent);
    }

    private static EquipmentCosmeticAccent? ResolveTrinketAccent(string? itemId)
    {
        if (string.IsNullOrWhiteSpace(itemId))
        {
            return null;
        }

        var lower = itemId.ToLowerInvariant();
        if (lower.Contains("crown") || lower.Contains("diadem"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Crown, "#FFD700");
        }

        if (lower.Contains("anklet") || lower.Contains("gem") || lower.Contains("ring"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Gem, "#4FC3F7");
        }

        // Fallback for custom/unrecognized trinkets
        return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Crown, "#E0E0E0");
    }

    private static EquipmentCosmeticAccent? ResolveWeaponAccent(string? itemId)
    {
        if (string.IsNullOrWhiteSpace(itemId))
        {
            return null;
        }

        var lower = itemId.ToLowerInvariant();
        if (lower.Contains("cannon") || lower.Contains("mortar") || lower.Contains("howitzer") || lower.Contains("ramshot"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Cannon, "#78909C");
        }

        if (lower.Contains("spade") || lower.Contains("scythe") || lower.Contains("blade") || lower.Contains("dagger"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Blade, "#B0BEC5");
        }

        // Fallback for custom/unrecognized weapons
        return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Cannon, "#90A4AE");
    }
}
