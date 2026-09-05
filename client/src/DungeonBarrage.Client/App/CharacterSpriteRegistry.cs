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
