using System.Diagnostics.CodeAnalysis;
using System.IO;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Godot;

namespace DungeonBarrage.Client.App;

/// <summary>
/// Manages loading, caching, and rendering of character and weapon spritesheets.
/// </summary>
public sealed class CharacterSpriteRegistry
{
    public const int CellWidth = 192;
    public const int CellHeight = 160;
    public const int AnchorX = 96;
    public const int AnchorY = 145;
    public const float CharacterHeightPixels = 125f;

    private static readonly string[] KnownSheetKeys =
    [
        "crow_ramshot_cannon",
        "crow_frostfall",
        "crow_drill",
        "crow_bow",
        "crow_cinder",
        "crow_pistol",
        "crow_revolver",
        "crow_boomerang",
        "crow_flail",
        "crow_pickaxe",
        "crow_damage",
        "crow_flight",
        "crow_potion",
    ];

    private readonly Dictionary<string, Texture2D?> _textures = new(StringComparer.OrdinalIgnoreCase);

    private readonly record struct ProjectileFrame(string SheetKey, int Row, int Col);

    /// <summary>
    /// Preloads all known spritesheets into memory.
    /// </summary>
    public void PreloadAll()
    {
        foreach (var key in KnownSheetKeys)
        {
            var tex = GetTexture(key);
            GD.Print($"[SpriteRegistry] Preload '{key}': {(tex is not null ? $"SUCCESS ({tex.GetWidth()}x{tex.GetHeight()})" : "FAILED")}");
        }
    }

    /// <summary>
    /// Attempts to retrieve or load a spritesheet texture by key.
    /// </summary>
    public Texture2D? GetTexture(string sheetKey)
    {
        if (_textures.TryGetValue(sheetKey, out var cached))
        {
            return cached;
        }

        var texture = LoadTexture(sheetKey);
        _textures[sheetKey] = texture;
        return texture;
    }

    /// <summary>
    /// Draws an idle equipment portrait for menu and roster cards. This is presentation-only:
    /// the ability identifier selects an existing sheet, but never changes authoritative loadout.
    /// </summary>
    /// <returns><see langword="true"/> when a production sprite was available.</returns>
    public bool TryDrawPortrait(
        CanvasItem canvas,
        string characterId,
        string mainAbilityId,
        Rect2 bounds,
        ulong visualTimeMsec,
        bool facesRight = true)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentException.ThrowIfNullOrWhiteSpace(characterId);
        ArgumentException.ThrowIfNullOrWhiteSpace(mainAbilityId);

        if (string.Equals(characterId, "leslie", StringComparison.Ordinal))
        {
            DrawLeslieFrogPortrait(canvas, bounds, visualTimeMsec, facesRight);
            return true;
        }

        var menuLoadout = new ClientLoadout(mainAbilityId, mainAbilityId, mainAbilityId, mainAbilityId);
        var sheetKey = CharacterPresentationModel.ResolveSpriteSheetKey(menuLoadout, ClientAbilitySlot.Main);
        var texture = GetTexture(sheetKey);
        if (texture is null)
        {
            return false;
        }

        var idleColumn = (int)((visualTimeMsec / 240uL) % 4uL);
        var source = new Rect2(idleColumn * CellWidth, 0f, CellWidth, CellHeight);
        var fitScale = Math.Min(bounds.Size.X / CellWidth, bounds.Size.Y / CellHeight) * 0.94f;
        var destination = new Rect2(-AnchorX, -AnchorY, CellWidth, CellHeight);
        var anchor = new Vector2(bounds.GetCenter().X, bounds.End.Y - 2f);

        canvas.DrawSetTransform(anchor, 0f, new Vector2(facesRight ? fitScale : -fitScale, fitScale));
        canvas.DrawTextureRectRegion(texture, destination, source, Colors.White);
        canvas.DrawSetTransform(Vector2.Zero, 0f, Vector2.One);
        return true;
    }

    private static Texture2D? LoadTexture(string sheetKey)
    {
        var resPath = $"res://assets/sprites/{sheetKey}.png";

        if (ResourceLoader.Exists(resPath))
        {
            try
            {
                var loaded = GD.Load<Texture2D>(resPath);
                if (loaded is not null)
                {
                    return loaded;
                }
            }
            catch (Exception ex)
            {
                GD.PrintErr($"[SpriteRegistry] GD.Load failed for {resPath}: {ex.Message}");
            }
        }

        try
        {
            var globalPath = ProjectSettings.GlobalizePath(resPath);
            if (File.Exists(globalPath))
            {
                using var image = Image.LoadFromFile(globalPath);
                if (image is not null)
                {
                    return ImageTexture.CreateFromImage(image);
                }
            }
            else
            {
                GD.PrintErr($"[SpriteRegistry] File not found at global path: {globalPath}");
            }
        }
        catch (Exception ex)
        {
            GD.PrintErr($"[SpriteRegistry] ImageTexture fallback failed for {resPath}: {ex.Message}");
        }

        return null;
    }

    /// <summary>
    /// Renders an authoritative character sprite if the corresponding texture is available.
    /// </summary>
    /// <returns>True if the sprite was rendered; false if procedural fallback should be used.</returns>
    public bool TryDrawCharacter(
        CanvasItem canvas,
        CharacterPresentationModel model,
        string characterId,
        bool isEliminated,
        ActorPresentationCue? cue,
        Color teamColor,
        ulong visualTimeMsec,
        bool isAiming,
        bool isAirborne,
        bool isMoving,
        float? aimAngleRadians,
        float flinchX,
        float bobY,
        float flashIntensity)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentNullException.ThrowIfNull(model);
        ArgumentException.ThrowIfNullOrWhiteSpace(characterId);

        if (string.Equals(characterId, "leslie", StringComparison.Ordinal))
        {
            var anchor = new Vector2(
                model.Body.Center.X + flinchX,
                model.ShadowPivot.Y + bobY);
            DrawLeslieFrog(
                canvas,
                anchor,
                model.Body.Radius / 22f,
                model.FacingSign,
                isEliminated,
                flashIntensity);
            return true;
        }

        var frame = CharacterAnimationFrameResolver.Resolve(
            model,
            isEliminated,
            cue,
            visualTimeMsec,
            isAiming,
            isAirborne,
            isMoving,
            aimAngleRadians);

        var texture = GetTexture(frame.SheetKey);
        if (texture is null)
        {
            return false;
        }

        var anchorPos = new Vector2(model.ShadowPivot.X + flinchX, model.ShadowPivot.Y + bobY);
        var radius = model.Body.Radius;
        var targetHeight = radius * 2.3f;
        var scale = targetHeight / CharacterHeightPixels;
        var facingSign = model.FacingSign;

        Color modulate;
        if (isEliminated)
        {
            modulate = new Color(0.65f, 0.65f, 0.70f, 0.85f);
        }
        else if (flashIntensity > 0f)
        {
            modulate = Colors.White.Lerp(new Color(2.5f, 2.5f, 2.5f, 1f), flashIntensity);
        }
        else
        {
            modulate = Colors.White;
        }

        var srcRect = new Rect2(frame.Col * CellWidth, frame.Row * CellHeight, CellWidth, CellHeight);
        var destRect = new Rect2(-AnchorX, -AnchorY, CellWidth, CellHeight);

        canvas.DrawSetTransform(anchorPos, 0f, new Vector2(facingSign * scale, scale));
        canvas.DrawTextureRectRegion(texture, destRect, srcRect, modulate);
        canvas.DrawSetTransform(Vector2.Zero, 0f, Vector2.One);

        return true;
    }

    private static void DrawLeslieFrogPortrait(
        CanvasItem canvas,
        Rect2 bounds,
        ulong visualTimeMsec,
        bool facesRight)
    {
        var pulse = MathF.Sin(visualTimeMsec / 260f) * 2.5f;
        var scale = Math.Min(bounds.Size.X / 100f, bounds.Size.Y / 118f);
        var anchor = new Vector2(bounds.GetCenter().X, bounds.End.Y - 5f + pulse);
        DrawLeslieFrog(canvas, anchor, scale, facesRight ? 1f : -1f, false, 0f);
    }

    private static void DrawLeslieFrog(
        CanvasItem canvas,
        Vector2 groundAnchor,
        float scale,
        float facingSign,
        bool isEliminated,
        float flashIntensity)
    {
        var frogGreen = isEliminated
            ? new Color("#6d7278")
            : new Color("#68b83e").Lerp(Colors.White, flashIntensity * 0.72f);
        var frogLight = isEliminated
            ? new Color("#92969a")
            : new Color("#b4d96a").Lerp(Colors.White, flashIntensity * 0.62f);
        var frogDark = isEliminated ? new Color("#4f545b") : new Color("#285f37");
        var scarf = isEliminated ? RetroArcadeUi.Locked : RetroArcadeUi.Ember;
        var ink = isEliminated ? new Color("#3f4248") : RetroArcadeUi.Void;

        canvas.DrawSetTransform(groundAnchor, 0f, new Vector2(facingSign * scale, scale));

        canvas.DrawCircle(new Vector2(-24f, -8f), 15f, frogDark);
        canvas.DrawCircle(new Vector2(24f, -8f), 15f, frogDark);
        canvas.DrawCircle(new Vector2(0f, -30f), 31f, frogGreen);
        canvas.DrawCircle(new Vector2(0f, -13f), 24f, frogDark);
        canvas.DrawCircle(new Vector2(0f, -18f), 20f, frogGreen);

        canvas.DrawCircle(new Vector2(-16f, -55f), 12f, frogGreen);
        canvas.DrawCircle(new Vector2(16f, -55f), 12f, frogGreen);
        canvas.DrawCircle(new Vector2(-16f, -57f), 7f, RetroArcadeUi.Coin);
        canvas.DrawCircle(new Vector2(16f, -57f), 7f, RetroArcadeUi.Coin);
        canvas.DrawCircle(new Vector2(-14f, -57f), 3.2f, ink);
        canvas.DrawCircle(new Vector2(18f, -57f), 3.2f, ink);

        canvas.DrawLine(new Vector2(-12f, -35f), new Vector2(12f, -35f), ink, 2.4f);
        canvas.DrawLine(new Vector2(-10f, -34f), new Vector2(0f, -30f), frogLight, 1.8f);
        canvas.DrawLine(new Vector2(0f, -30f), new Vector2(10f, -34f), frogLight, 1.8f);

        canvas.DrawRect(new Rect2(-24f, -24f, 48f, 13f), scarf);
        canvas.DrawRect(new Rect2(-19f, -12f, 38f, 25f), new Color("#28364a"));
        canvas.DrawRect(new Rect2(-16f, -8f, 32f, 5f), new Color("#9b6a2f"));
        canvas.DrawCircle(new Vector2(0f, -5f), 3f, RetroArcadeUi.Coin);

        canvas.DrawLine(new Vector2(-12f, 9f), new Vector2(-18f, 20f), frogLight, 7f);
        canvas.DrawLine(new Vector2(12f, 9f), new Vector2(18f, 20f), frogLight, 7f);
        canvas.DrawCircle(new Vector2(-20f, 21f), 7f, frogGreen);
        canvas.DrawCircle(new Vector2(20f, 21f), 7f, frogGreen);

        var weaponY = -22f;
        canvas.DrawRect(new Rect2(13f, weaponY - 5f, 37f, 11f), new Color("#7d6b4b"));
        canvas.DrawRect(new Rect2(25f, weaponY - 3f, 31f, 7f), new Color("#b38a38"));
        canvas.DrawCircle(new Vector2(56f, weaponY + 0.5f), 6f, RetroArcadeUi.Coin);

        canvas.DrawSetTransform(Vector2.Zero, 0f, Vector2.One);
    }

    /// <summary>
    /// Draws an ammunition-only spritesheet cell at an authoritative projectile position.
    /// </summary>
    /// <remarks>
    /// Frame selection is presentation-only. Position and direction are derived from the Rust
    /// trace by the caller; this method never integrates ballistics or performs collision tests.
    /// </remarks>
    public bool TryDrawProjectile(
        CanvasItem canvas,
        string abilityId,
        Vector2 position,
        Vector2 direction,
        float radius,
        uint visualTick)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        ArgumentException.ThrowIfNullOrWhiteSpace(abilityId);

        var frame = ResolveProjectileFrame(abilityId, visualTick);
        if (frame is not { } projectileFrame || GetTexture(projectileFrame.SheetKey) is not { } texture)
        {
            return false;
        }

        var rotation = direction.LengthSquared() > 0.001f ? direction.Angle() : 0f;
        var targetHeight = Math.Max(radius * 2.2f, 10f);
        var scale = targetHeight / CellHeight;
        var source = new Rect2(
            projectileFrame.Col * CellWidth,
            projectileFrame.Row * CellHeight,
            CellWidth,
            CellHeight);
        var destination = new Rect2(
            -CellWidth * 0.5f,
            -CellHeight * 0.5f,
            CellWidth,
            CellHeight);

        canvas.DrawSetTransform(position, rotation, new Vector2(scale, scale));
        canvas.DrawTextureRectRegion(texture, destination, source, Colors.White);
        canvas.DrawSetTransform(Vector2.Zero, 0f, Vector2.One);
        return true;
    }

    private static ProjectileFrame? ResolveProjectileFrame(string abilityId, uint visualTick)
    {
        if (abilityId.Contains("ramshot", StringComparison.OrdinalIgnoreCase) ||
            abilityId.Contains("cannon", StringComparison.OrdinalIgnoreCase))
        {
            return new ProjectileFrame("crow_ramshot_cannon", 2, 2 + (int)(visualTick % 2));
        }

        if (abilityId.Contains("bow", StringComparison.OrdinalIgnoreCase))
        {
            return new ProjectileFrame("crow_bow", 3, 3);
        }

        if (abilityId.Contains("boomerang", StringComparison.OrdinalIgnoreCase))
        {
            return new ProjectileFrame("crow_boomerang", 3, 2);
        }

        if (abilityId.Contains("cinder", StringComparison.OrdinalIgnoreCase) ||
            abilityId.Contains("repeater", StringComparison.OrdinalIgnoreCase))
        {
            return new ProjectileFrame("crow_cinder", 3, 2);
        }

        return null;
    }
}
