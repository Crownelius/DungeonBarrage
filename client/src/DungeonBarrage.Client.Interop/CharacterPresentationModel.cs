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

    /// <summary>Arched recurve bow silhouette (e.g. recurve-bow, line-repeater).</summary>
    Bow,

    /// <summary>One-shot secondary ordnance projectile silhouette (e.g. ramshot-shell, frostfall-shell, mole-charge).</summary>
    Ordnance,
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

    /// <summary>Equipped weapon cosmetic accent for the currently active slot, or null.</summary>
    public EquipmentCosmeticAccent? WeaponAccent { get; }

    /// <summary>The currently active ability slot being presented.</summary>
    public ClientAbilitySlot ActiveSlot { get; }

    /// <summary>The aim elevation angle in radians relative to horizontal, if aiming.</summary>
    public float? AimAngleRadians { get; }

    /// <summary>The normalized directional aim unit vector in screen coordinates (accounting for facing), or null if not aiming.</summary>
    public PresentationPoint? AimVector { get; }

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
        EquipmentCosmeticAccent? weaponAccent,
        ClientAbilitySlot activeSlot,
        float? aimAngleRadians,
        PresentationPoint? aimVector)
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
        ActiveSlot = activeSlot;
        AimAngleRadians = aimAngleRadians;
        AimVector = aimVector;
    }

    /// <summary>
    /// Computes a subtle vertical idle breathing offset in display pixels.
    /// </summary>
    /// <param name="visualTimeMsec">Visual clock time in milliseconds.</param>
    /// <param name="reduceMotion">Whether reduced motion accessibility is enabled.</param>
    /// <returns>A signed pixel offset along the Y axis.</returns>
    public float BobOffsetY(ulong visualTimeMsec, bool reduceMotion)
    {
        if (reduceMotion)
        {
            return 0f;
        }

        return MathF.Sin(visualTimeMsec * 0.003f) * (Body.Radius * 0.05f);
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
    /// <param name="activeSlot">The currently active ability slot being framed.</param>
    /// <param name="aimAngleRadians">Current aim elevation angle in radians, or null if not aiming.</param>
    /// <returns>A fully resolved paper-doll presentation model.</returns>
    public static CharacterPresentationModel Create(
        ClientPlayerSnapshot player,
        int? opponentX,
        int positionScale,
        float cellSize,
        PresentationPoint worldOrigin,
        PresentationPoint cameraOffset,
        ClientAbilitySlot activeSlot = ClientAbilitySlot.Main,
        float? aimAngleRadians = null)
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
        var weaponAccent = ResolveWeaponAccent(player.Loadout, activeSlot);

        PresentationPoint? aimVector = null;
        if (aimAngleRadians is { } angle)
        {
            var vx = facing * MathF.Cos(angle);
            var vy = -MathF.Sin(angle);
            aimVector = new PresentationPoint(vx, vy);
        }

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
            weaponAccent,
            activeSlot,
            aimAngleRadians,
            aimVector);
    }

    private static EquipmentCosmeticAccent? ResolveTrinketAccent(string? itemId)
    {
        if (string.IsNullOrWhiteSpace(itemId))
        {
            return null;
        }

        var lower = itemId.ToLowerInvariant();
        if (lower.Contains("crown") || lower.Contains("diadem") || lower.Contains("circlet"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Crown, "#FFD700");
        }

        if (lower.Contains("anklet") || lower.Contains("gem") || lower.Contains("ring") ||
            lower.Contains("crest") || lower.Contains("pendant"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Gem, "#4FC3F7");
        }

        // Fallback for custom/unrecognized trinkets
        return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Crown, "#E0E0E0");
    }

    private static EquipmentCosmeticAccent? ResolveWeaponAccent(ClientLoadout loadout, ClientAbilitySlot activeSlot)
    {
        var itemId = activeSlot switch
        {
            ClientAbilitySlot.Main => loadout.Main,
            ClientAbilitySlot.Secondary => loadout.Secondary,
            ClientAbilitySlot.MeleeTool => loadout.MeleeTool,
            ClientAbilitySlot.Trinket => null,
            _ => loadout.Main,
        };

        if (string.IsNullOrWhiteSpace(itemId))
        {
            return null;
        }

        var lower = itemId.ToLowerInvariant();
        if (lower.Contains("bow") || lower.Contains("repeater"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Bow, "#8D6E63");
        }

        if (lower.Contains("shell") || lower.Contains("charge") || lower.Contains("mag") ||
            lower.Contains("belt") || lower.Contains("finisher") || lower.Contains("bladder") ||
            lower.Contains("capsule") || lower.Contains("bomb"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Ordnance, "#FF7043");
        }

        if (lower.Contains("spade") || lower.Contains("scythe") || lower.Contains("blade") ||
            lower.Contains("dagger") || lower.Contains("maul") || lower.Contains("pick") ||
            lower.Contains("longsword") || lower.Contains("cleaver") || lower.Contains("fan") ||
            lower.Contains("beak"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Blade, "#B0BEC5");
        }

        if (lower.Contains("cannon") || lower.Contains("mortar") || lower.Contains("howitzer") ||
            lower.Contains("ramshot") || lower.Contains("pistol") || lower.Contains("drill") ||
            lower.Contains("sprayer"))
        {
            return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Cannon, "#78909C");
        }

        // Fallback for custom/unrecognized weapons
        return new EquipmentCosmeticAccent(itemId, CosmeticAccentKind.Cannon, "#90A4AE");
    }
}
