using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Godot;

namespace DungeonBarrage.Client.App;

/// <summary>
/// Loads the Kingdom-style 16-bit tiles and paints occupancy with world-locked UVs so the
/// cell grid disappears. Authority remains the occupancy mask; this class never invents solid cells.
/// </summary>
public sealed class PixelTerrainAtlas
{
    private const string TileRoot = "res://assets/tiles/";
    private readonly List<TextureBlit> _blits = new(4);
    private Texture2D? _soil;
    private Texture2D? _grass;
    private Texture2D? _stone;
    private Texture2D? _stoneCap;
    private Texture2D? _wood;
    private Texture2D? _sky;
    private Texture2D? _hills;
    private Texture2D? _forest;
    private bool _loaded;

    /// <summary>Whether at least the soil fill atlas is available.</summary>
    public bool HasFillTextures => _soil is not null;

    /// <summary>Loads tile and parallax textures once. Missing files fall back to solid tints.</summary>
    public void EnsureLoaded()
    {
        if (_loaded)
        {
            return;
        }

        _soil = LoadTexture("soil_fill");
        _grass = LoadTexture("grass_cap");
        _stone = LoadTexture("stone_fill");
        _stoneCap = LoadTexture("stone_cap");
        _wood = LoadTexture("wood_fill");
        _sky = LoadTexture("sky_dusk");
        _hills = LoadTexture("hills_far");
        _forest = LoadTexture("forest_mid");
        _loaded = true;
    }

    /// <summary>Paints the dusk sky and two parallax forest layers behind the arena.</summary>
    /// <param name="canvas">Scene canvas.</param>
    /// <param name="viewport">Full window rectangle.</param>
    /// <param name="arena">Fitted match arena in presentation pixels.</param>
    /// <param name="camera">Effective camera offset.</param>
    public void DrawBackdrop(CanvasItem canvas, Rect2 viewport, Rect2 arena, Vector2 camera)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        EnsureLoaded();

        canvas.DrawRect(viewport, new Color(0.31f, 0.46f, 0.54f));
        if (_sky is not null)
        {
            canvas.DrawTextureRect(_sky, arena, tile: false);
        }

        DrawParallaxLayer(canvas, _hills, arena, camera, parallax: 0.18f, heightFraction: 0.72f, extraWidth: 0.12f);
        DrawParallaxLayer(canvas, _forest, arena, camera, parallax: 0.42f, heightFraction: 0.48f, extraWidth: 0.18f);
    }

    /// <summary>Paints one occupied cell with a world-locked material atlas and optional grass cap.</summary>
    /// <param name="canvas">Scene canvas.</param>
    /// <param name="cells">Authoritative occupancy.</param>
    /// <param name="width">Mask width.</param>
    /// <param name="height">Mask height.</param>
    /// <param name="x">Cell X.</param>
    /// <param name="y">Cell Y.</param>
    /// <param name="material">Material byte at this cell.</param>
    /// <param name="block">Owning block, if any.</param>
    /// <param name="dest">Destination rectangle in presentation pixels, already camera-translated.</param>
    public void DrawCell(
        CanvasItem canvas,
        ReadOnlySpan<byte> cells,
        int width,
        int height,
        int x,
        int y,
        byte material,
        ClientBlockSnapshot? block,
        Rect2 dest)
    {
        ArgumentNullException.ThrowIfNull(canvas);
        EnsureLoaded();

        var tint = PixelTerrainSampler.Tint(material, block);
        var modulate = ToColor(tint);
        var fill = TextureFor(material);
        if (fill is null)
        {
            canvas.DrawRect(dest, modulate);
        }
        else
        {
            BlitWrapped(
                canvas,
                fill,
                dest,
                worldX: (int)MathF.Floor(x * dest.Size.X),
                worldY: (int)MathF.Floor(y * dest.Size.Y),
                modulate);
        }

        if (PixelTerrainSampler.IsExposedTop(cells, width, height, x, y))
        {
            var capTexture = material == 3 ? _stoneCap : _grass;
            var capHeight = Math.Max(3f, dest.Size.Y * (material == 3 ? 0.38f : 0.42f));
            var cap = new Rect2(dest.Position, new Vector2(dest.Size.X, capHeight));
            if (capTexture is not null)
            {
                BlitWrapped(
                    canvas,
                    capTexture,
                    cap,
                    worldX: (int)MathF.Floor(x * dest.Size.X),
                    worldY: 0,
                    modulate);
            }
            else
            {
                canvas.DrawRect(cap, modulate.Lightened(0.18f));
            }
        }

        if (PixelTerrainSampler.IsExposedLeft(cells, width, height, x, y))
        {
            var edge = new Rect2(dest.Position, new Vector2(Math.Max(2f, dest.Size.X * 0.12f), dest.Size.Y));
            canvas.DrawRect(edge, new Color(0f, 0f, 0f, 0.22f * (tint.A / 255f)));
        }

        if (PixelTerrainSampler.IsExposedRight(cells, width, height, x, y))
        {
            var edgeW = Math.Max(2f, dest.Size.X * 0.12f);
            var edge = new Rect2(dest.Position + new Vector2(dest.Size.X - edgeW, 0f), new Vector2(edgeW, dest.Size.Y));
            canvas.DrawRect(edge, new Color(0f, 0f, 0f, 0.22f * (tint.A / 255f)));
        }
    }

    private static void DrawParallaxLayer(
        CanvasItem canvas,
        Texture2D? texture,
        Rect2 arena,
        Vector2 camera,
        float parallax,
        float heightFraction,
        float extraWidth)
    {
        if (texture is null)
        {
            return;
        }

        var width = arena.Size.X * (1f + extraWidth);
        var height = arena.Size.Y * heightFraction;
        var dest = new Rect2(
            arena.Position.X - (arena.Size.X * extraWidth * 0.5f) + (camera.X * parallax),
            arena.Position.Y + arena.Size.Y - height + (camera.Y * parallax * 0.35f),
            width,
            height);
        canvas.DrawTextureRect(texture, dest, tile: false);
    }

    private void BlitWrapped(CanvasItem canvas, Texture2D texture, Rect2 dest, int worldX, int worldY, Color modulate)
    {
        var texW = texture.GetWidth();
        var texH = texture.GetHeight();
        var destW = Math.Max(1, (int)MathF.Round(dest.Size.X));
        var destH = Math.Max(1, (int)MathF.Round(dest.Size.Y));
        PixelTerrainSampler.CollectWrapBlits(destW, destH, worldX, worldY, texW, texH, _blits);
        for (var i = 0; i < _blits.Count; i++)
        {
            var blit = _blits[i];
            var slice = new Rect2(
                dest.Position.X + blit.DestX,
                dest.Position.Y + blit.DestY,
                blit.Width,
                blit.Height);
            var source = new Rect2(blit.SrcX, blit.SrcY, blit.Width, blit.Height);
            canvas.DrawTextureRectRegion(texture, slice, source, modulate);
        }
    }

    private Texture2D? TextureFor(byte material) =>
        material switch
        {
            2 => _wood ?? _soil,
            3 => _stone ?? _soil,
            _ => _soil,
        };

    private static Color ToColor(PixelRgba tint) =>
        new(tint.R / 255f, tint.G / 255f, tint.B / 255f, tint.A / 255f);

    private static Texture2D? LoadTexture(string stem)
    {
        var resPath = $"{TileRoot}{stem}.png";
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
                GD.PrintErr($"[PixelTerrain] GD.Load failed for {resPath}: {ex.Message}");
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
        }
        catch (Exception ex)
        {
            GD.PrintErr($"[PixelTerrain] ImageTexture fallback failed for {resPath}: {ex.Message}");
        }

        return null;
    }
}
