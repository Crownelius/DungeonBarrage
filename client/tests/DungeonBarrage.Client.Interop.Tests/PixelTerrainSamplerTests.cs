using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public sealed class PixelTerrainSamplerTests
{
    [Fact]
    public void SolidColumns_matches_authority_ceiling_and_one_shot_threshold()
    {
        var full = Block(width: 4, health: 180, maxHealth: 180);
        Assert.Equal(4, PixelTerrainSampler.SolidColumns(full));

        var oneColumn = Block(width: 4, health: 45, maxHealth: 180);
        Assert.Equal(1, PixelTerrainSampler.SolidColumns(oneColumn));
        Assert.Equal(PixelRgba.OneShot, PixelTerrainSampler.Tint(1, oneColumn));

        var dead = Block(width: 4, health: 0, maxHealth: 180);
        Assert.Equal(0, PixelTerrainSampler.SolidColumns(dead));
    }

    [Fact]
    public void Tint_fades_with_missing_health_but_never_past_the_transparency_cap()
    {
        var half = Block(width: 4, health: 90, maxHealth: 180);
        var tint = PixelTerrainSampler.Tint(1, half);
        Assert.Equal(PixelRgba.Soil.R, tint.R);
        var expectedAlpha = 1f - (0.5f * PixelTerrainSampler.EnvironmentMaxTransparency);
        Assert.Equal((byte)Math.Clamp((int)MathF.Round(expectedAlpha * 255f), 0, 255), tint.A);
        Assert.True(tint.A >= (byte)Math.Round((1f - PixelTerrainSampler.EnvironmentMaxTransparency) * 255f));
    }

    [Fact]
    public void Exposed_top_is_the_walkable_lip_and_ignores_empty_cells()
    {
        // 3x3 with a soil column in the middle.
        var cells = new byte[]
        {
            0, 0, 0,
            0, 1, 0,
            0, 1, 0,
        };
        Assert.True(PixelTerrainSampler.IsExposedTop(cells, 3, 3, 1, 1));
        Assert.False(PixelTerrainSampler.IsExposedTop(cells, 3, 3, 1, 2));
        Assert.False(PixelTerrainSampler.IsExposedTop(cells, 3, 3, 0, 1));
        Assert.True(PixelTerrainSampler.IsExposedLeft(cells, 3, 3, 1, 1));
        Assert.True(PixelTerrainSampler.IsExposedRight(cells, 3, 3, 1, 1));
    }

    [Fact]
    public void BlockOwningCell_prefers_the_smaller_overlapping_span()
    {
        var perch = Block(id: 1, originX: 10, originY: 8, width: 4, height: 3, health: 180, maxHealth: 180);
        var stage = Block(id: 100, originX: 6, originY: 8, width: 52, height: 3, health: 999, maxHealth: 999);
        var owner = PixelTerrainSampler.BlockOwningCell([stage, perch], 11, 9);
        Assert.NotNull(owner);
        Assert.Equal(1u, owner.Id);
    }

    [Fact]
    public void PixelCellSize_snaps_to_integer_pixels_inside_the_16_48_window()
    {
        Assert.Equal(32f, PixelTerrainSampler.PixelCellSize(2048f, 960f, 64, 30));
        Assert.Equal(16f, PixelTerrainSampler.PixelCellSize(80f, 80f, 64, 30));
        Assert.Equal(48f, PixelTerrainSampler.PixelCellSize(10_000f, 10_000f, 10, 10));
    }

    [Fact]
    public void CollectWrapBlits_splits_when_the_sample_crosses_the_atlas_edge()
    {
        var buffer = new List<TextureBlit>();
        PixelTerrainSampler.CollectWrapBlits(
            destWidth: 32,
            destHeight: 32,
            worldX: 240,
            worldY: 240,
            texWidth: 256,
            texHeight: 256,
            buffer);
        Assert.Equal(4, buffer.Count);
        Assert.Equal(new TextureBlit(0, 0, 16, 16, 240, 240), buffer[0]);
        Assert.Equal(new TextureBlit(16, 0, 16, 16, 0, 240), buffer[1]);
        Assert.Equal(new TextureBlit(0, 16, 16, 16, 240, 0), buffer[2]);
        Assert.Equal(new TextureBlit(16, 16, 16, 16, 0, 0), buffer[3]);
    }

    [Fact]
    public void CollectWrapBlits_is_a_single_sample_when_the_cell_fits()
    {
        var buffer = new List<TextureBlit>();
        PixelTerrainSampler.CollectWrapBlits(32, 32, 64, 96, 256, 256, buffer);
        Assert.Equal([new TextureBlit(0, 0, 32, 32, 64, 96)], buffer);
    }

    private static ClientBlockSnapshot Block(
        uint id = 1,
        int originX = 0,
        int originY = 0,
        ushort width = 4,
        ushort height = 3,
        ushort health = 180,
        ushort maxHealth = 180) =>
        new(
            id,
            originX,
            originY,
            width,
            height,
            ClientMaterial.Soil,
            health,
            maxHealth,
            ClientErosionAxis.Columns);
}
