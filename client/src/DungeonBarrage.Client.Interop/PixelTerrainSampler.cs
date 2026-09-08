using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>Packed 8-bit presentation color used by the pixel-terrain sampler.</summary>
/// <param name="R">Red channel.</param>
/// <param name="G">Green channel.</param>
/// <param name="B">Blue channel.</param>
/// <param name="A">Alpha channel.</param>
public readonly record struct PixelRgba(byte R, byte G, byte B, byte A)
{
    /// <summary>Soil fill fallback when the dirt atlas is missing.</summary>
    public static PixelRgba Soil { get; } = new(115, 82, 46, 255);

    /// <summary>Wood fill fallback when the timber atlas is missing.</summary>
    public static PixelRgba Wood { get; } = new(140, 107, 51, 255);

    /// <summary>Reinforced-stone fill fallback when the masonry atlas is missing.</summary>
    public static PixelRgba Stone { get; } = new(140, 140, 148, 255);

    /// <summary>One remaining solid column: the environment is one shot from collapse.</summary>
    public static PixelRgba OneShot { get; } = new(219, 41, 31, 140);
}

/// <summary>One source-to-destination blit that respects texture wrap on both axes.</summary>
/// <param name="DestX">Destination left, in presentation pixels relative to the cell origin.</param>
/// <param name="DestY">Destination top, in presentation pixels relative to the cell origin.</param>
/// <param name="Width">Blit width in pixels.</param>
/// <param name="Height">Blit height in pixels.</param>
/// <param name="SrcX">Source left in the atlas, already wrapped into <c>[0, texW)</c>.</param>
/// <param name="SrcY">Source top in the atlas, already wrapped into <c>[0, texH)</c>.</param>
public readonly record struct TextureBlit(int DestX, int DestY, int Width, int Height, int SrcX, int SrcY);

/// <summary>
/// Godot-free occupancy, damage-tint, and wrap-sampling helpers for the 16-bit map presentation.
/// Collision, collapse, and health remain authority-owned; this type only decides how cells look.
/// </summary>
public static class PixelTerrainSampler
{
    /// <summary>Maximum missing-health fade. Matches the combat HUD contract: 45% transparency cap.</summary>
    public const float EnvironmentMaxTransparency = 0.45f;

    /// <summary>Integer pixel size of one terrain cell after snapping.</summary>
    public const int MinCellPixels = 16;

    /// <summary>Upper bound for one terrain cell so the HUD still fits at 1280×720.</summary>
    public const int MaxCellPixels = 48;

    /// <summary>
    /// Remaining solid columns from health, matching the authority's
    /// <c>ceil(width * hp / max)</c>.
    /// </summary>
    /// <param name="block">Authoritative block snapshot.</param>
    /// <returns>Column count in <c>[0, WidthCells]</c>.</returns>
    public static int SolidColumns(ClientBlockSnapshot block)
    {
        ArgumentNullException.ThrowIfNull(block);
        if (block.WidthCells == 0 || block.Health == 0 || block.MaxHealth == 0)
        {
            return 0;
        }

        var product = (int)block.WidthCells * block.Health;
        var columns = (product + block.MaxHealth - 1) / block.MaxHealth;
        return Math.Min(columns, block.WidthCells);
    }

    /// <summary>Chooses the smallest living block that contains the cell, if any.</summary>
    /// <param name="blocks">Snapshot blocks.</param>
    /// <param name="x">Cell X.</param>
    /// <param name="y">Cell Y.</param>
    /// <returns>The owning block, or <see langword="null"/> for background terrain.</returns>
    public static ClientBlockSnapshot? BlockOwningCell(IReadOnlyList<ClientBlockSnapshot> blocks, int x, int y)
    {
        ArgumentNullException.ThrowIfNull(blocks);
        ClientBlockSnapshot? found = null;
        var foundArea = int.MaxValue;
        for (var i = 0; i < blocks.Count; i++)
        {
            var block = blocks[i];
            if (block.Health == 0)
            {
                continue;
            }

            if (x < block.OriginCellX || y < block.OriginCellY)
            {
                continue;
            }

            if (x >= block.OriginCellX + block.WidthCells || y >= block.OriginCellY + block.HeightCells)
            {
                continue;
            }

            var area = block.WidthCells * block.HeightCells;
            if (area < foundArea)
            {
                found = block;
                foundArea = area;
            }
        }

        return found;
    }

    /// <summary>Whether the cell is inside the mask and occupied.</summary>
    /// <param name="cells">Row-major occupancy bytes.</param>
    /// <param name="width">Mask width.</param>
    /// <param name="height">Mask height.</param>
    /// <param name="x">Cell X.</param>
    /// <param name="y">Cell Y.</param>
    /// <returns><see langword="true"/> when the cell is solid.</returns>
    public static bool CellIsSolid(ReadOnlySpan<byte> cells, int width, int height, int x, int y)
    {
        if (x < 0 || y < 0 || x >= width || y >= height)
        {
            return false;
        }

        var index = (y * width) + x;
        return index >= 0 && index < cells.Length && cells[index] != 0;
    }

    /// <summary>Whether the cell is solid and the cell above it is empty (a walkable lip).</summary>
    /// <param name="cells">Row-major occupancy bytes.</param>
    /// <param name="width">Mask width.</param>
    /// <param name="height">Mask height.</param>
    /// <param name="x">Cell X.</param>
    /// <param name="y">Cell Y.</param>
    /// <returns><see langword="true"/> when a grass cap should be drawn.</returns>
    public static bool IsExposedTop(ReadOnlySpan<byte> cells, int width, int height, int x, int y) =>
        CellIsSolid(cells, width, height, x, y) && !CellIsSolid(cells, width, height, x, y - 1);

    /// <summary>Whether the cell is solid and the west neighbor is empty.</summary>
    /// <param name="cells">Row-major occupancy bytes.</param>
    /// <param name="width">Mask width.</param>
    /// <param name="height">Mask height.</param>
    /// <param name="x">Cell X.</param>
    /// <param name="y">Cell Y.</param>
    /// <returns><see langword="true"/> when a west cliff shade should be drawn.</returns>
    public static bool IsExposedLeft(ReadOnlySpan<byte> cells, int width, int height, int x, int y) =>
        CellIsSolid(cells, width, height, x, y) && !CellIsSolid(cells, width, height, x - 1, y);

    /// <summary>Whether the cell is solid and the east neighbor is empty.</summary>
    /// <param name="cells">Row-major occupancy bytes.</param>
    /// <param name="width">Mask width.</param>
    /// <param name="height">Mask height.</param>
    /// <param name="x">Cell X.</param>
    /// <param name="y">Cell Y.</param>
    /// <returns><see langword="true"/> when an east cliff shade should be drawn.</returns>
    public static bool IsExposedRight(ReadOnlySpan<byte> cells, int width, int height, int x, int y) =>
        CellIsSolid(cells, width, height, x, y) && !CellIsSolid(cells, width, height, x + 1, y);

    /// <summary>Presentation tint for one occupied cell, including the one-shot red warning.</summary>
    /// <param name="material">Authoritative material byte.</param>
    /// <param name="block">Owning block, if any.</param>
    /// <returns>Premultiplied-ready RGBA used as a modulate over the atlas sample.</returns>
    public static PixelRgba Tint(byte material, ClientBlockSnapshot? block)
    {
        if (block is { MaxHealth: > 0 } living)
        {
            if (SolidColumns(living) <= 1)
            {
                return PixelRgba.OneShot;
            }

            var healthFrac = Math.Clamp(living.Health / (float)living.MaxHealth, 0f, 1f);
            var alpha = 1f - ((1f - healthFrac) * EnvironmentMaxTransparency);
            var packed = (byte)Math.Clamp((int)MathF.Round(alpha * 255f), 0, 255);
            var fill = FillFor(material);
            return fill with { A = packed };
        }

        return FillFor(material);
    }

    /// <summary>
    /// Snaps the fitted cell size to an integer pixel multiple so nearest-neighbor tiles stay crisp.
    /// </summary>
    /// <param name="availableWidth">Arena width in presentation pixels.</param>
    /// <param name="availableHeight">Arena height in presentation pixels.</param>
    /// <param name="widthCells">Authoritative mask width.</param>
    /// <param name="heightCells">Authoritative mask height.</param>
    /// <returns>Integer pixels per cell in <c>[MinCellPixels, MaxCellPixels]</c>.</returns>
    public static float PixelCellSize(float availableWidth, float availableHeight, int widthCells, int heightCells)
    {
        var width = Math.Max(1, widthCells);
        var height = Math.Max(1, heightCells);
        var raw = Math.Min(availableWidth / width, availableHeight / height);
        var snapped = MathF.Floor(Math.Clamp(raw, MinCellPixels, MaxCellPixels));
        return Math.Max(MinCellPixels, snapped);
    }

    /// <summary>
    /// Splits one destination rectangle into at most four atlas samples so world-locked UVs wrap
    /// instead of stretching a seam across a cell.
    /// </summary>
    /// <param name="destWidth">Destination width in pixels. Must be positive.</param>
    /// <param name="destHeight">Destination height in pixels. Must be positive.</param>
    /// <param name="worldX">World-space pixel X used as the atlas U origin.</param>
    /// <param name="worldY">World-space pixel Y used as the atlas V origin.</param>
    /// <param name="texWidth">Atlas width in pixels. Must be positive.</param>
    /// <param name="texHeight">Atlas height in pixels. Must be positive.</param>
    /// <param name="buffer">Caller-owned blit list. Cleared and filled by this method.</param>
    public static void CollectWrapBlits(
        int destWidth,
        int destHeight,
        int worldX,
        int worldY,
        int texWidth,
        int texHeight,
        IList<TextureBlit> buffer)
    {
        ArgumentNullException.ThrowIfNull(buffer);
        buffer.Clear();
        if (destWidth <= 0 || destHeight <= 0 || texWidth <= 0 || texHeight <= 0)
        {
            return;
        }

        var srcX = PositiveMod(worldX, texWidth);
        var srcY = PositiveMod(worldY, texHeight);
        var firstW = Math.Min(destWidth, texWidth - srcX);
        var firstH = Math.Min(destHeight, texHeight - srcY);
        buffer.Add(new TextureBlit(0, 0, firstW, firstH, srcX, srcY));

        if (firstW < destWidth)
        {
            buffer.Add(new TextureBlit(firstW, 0, destWidth - firstW, firstH, 0, srcY));
        }

        if (firstH < destHeight)
        {
            buffer.Add(new TextureBlit(0, firstH, firstW, destHeight - firstH, srcX, 0));
        }

        if (firstW < destWidth && firstH < destHeight)
        {
            buffer.Add(new TextureBlit(firstW, firstH, destWidth - firstW, destHeight - firstH, 0, 0));
        }
    }

    private static PixelRgba FillFor(byte material) =>
        material switch
        {
            2 => PixelRgba.Wood,
            3 => PixelRgba.Stone,
            _ => PixelRgba.Soil,
        };

    private static int PositiveMod(int value, int modulus)
    {
        var remainder = value % modulus;
        return remainder < 0 ? remainder + modulus : remainder;
    }
}
