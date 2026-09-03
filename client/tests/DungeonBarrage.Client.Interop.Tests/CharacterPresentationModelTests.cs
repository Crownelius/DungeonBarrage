using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public sealed class CharacterPresentationModelTests
{
    private static ClientPlayerSnapshot MakePlayer(int x, int y, string main, string trinket) =>
        new(
            PlayerId: "player-1",
            Team: 0,
            Health: 280,
            IsEliminated: false,
            MaxHealth: 280,
            Position: new ClientPosition(x, y),
            CollisionCenter: new ClientPosition(x, y - (2 * 1024)),
            CollisionRadius: 2 * 1024,
            Loadout: new ClientLoadout(main, "shell", "spade", trinket),
            Ammo: Array.Empty<ClientAmmoCounter>(),
            TrinketCharge: 0,
            Statuses: Array.Empty<ClientStatusSnapshot>(),
            Appearance: new ClientAppearance("default", Array.Empty<string>(), "default"));

    [Fact]
    public void Crow_faces_right_when_opponent_is_to_the_right()
    {
        var player = MakePlayer(x: 10 * 1024, y: 10 * 1024, "ramshot-cannon", "ember-crown");
        var model = CharacterPresentationModel.Create(
            player,
            opponentX: 50 * 1024,
            positionScale: 1024,
            cellSize: 12f,
            worldOrigin: new PresentationPoint(0f, 0f),
            cameraOffset: new PresentationPoint(0f, 0f));

        Assert.True(model.FacesRight);
        Assert.Equal(1f, model.FacingSign);
        Assert.True(model.BeakSocket.X > model.Body.Center.X);
        Assert.True(model.EyeSocket.X < model.Body.Center.X);
    }

    [Fact]
    public void Crow_faces_left_when_opponent_is_to_the_left()
    {
        var player = MakePlayer(x: 50 * 1024, y: 10 * 1024, "ramshot-cannon", "ember-crown");
        var model = CharacterPresentationModel.Create(
            player,
            opponentX: 10 * 1024,
            positionScale: 1024,
            cellSize: 12f,
            worldOrigin: new PresentationPoint(0f, 0f),
            cameraOffset: new PresentationPoint(0f, 0f));

        Assert.False(model.FacesRight);
        Assert.Equal(-1f, model.FacingSign);
        Assert.True(model.BeakSocket.X < model.Body.Center.X);
        Assert.True(model.EyeSocket.X > model.Body.Center.X);
    }

    [Fact]
    public void Sockets_are_anchored_consistently_with_geometry()
    {
        var player = MakePlayer(x: 20 * 1024, y: 15 * 1024, "ramshot-cannon", "ember-crown");
        var model = CharacterPresentationModel.Create(
            player,
            opponentX: 40 * 1024,
            positionScale: 1024,
            cellSize: 12f,
            worldOrigin: new PresentationPoint(100f, 100f),
            cameraOffset: new PresentationPoint(10f, 5f));

        var cy = model.Body.Center.Y;
        var r = model.Body.Radius;

        // Crown socket is near top of head
        Assert.True(model.CrownSocket.Y < cy);
        Assert.InRange(model.CrownSocket.Y, cy - r, cy - (r * 0.5f));

        // Shadow pivot is near ground contact
        Assert.True(model.ShadowPivot.Y > cy);
        Assert.InRange(model.ShadowPivot.Y, cy + (r * 0.5f), cy + r);

        // Beak polygon has 3 points
        Assert.Equal(3, model.BeakPolygon.Count);
    }

    [Fact]
    public void Equipment_cosmetic_accents_resolve_expected_kinds()
    {
        var crownPlayer = MakePlayer(x: 0, y: 0, "ramshot-cannon", "ember-crown");
        var crownModel = CharacterPresentationModel.Create(
            crownPlayer, opponentX: null, 1024, 12f, new PresentationPoint(0f, 0f), new PresentationPoint(0f, 0f));

        Assert.NotNull(crownModel.TrinketAccent);
        Assert.Equal(CosmeticAccentKind.Crown, crownModel.TrinketAccent.Kind);
        Assert.NotNull(crownModel.WeaponAccent);
        Assert.Equal(CosmeticAccentKind.Cannon, crownModel.WeaponAccent.Kind);

        var ankletPlayer = MakePlayer(x: 0, y: 0, "trench-spade", "frost-anklet");
        var ankletModel = CharacterPresentationModel.Create(
            ankletPlayer, opponentX: null, 1024, 12f, new PresentationPoint(0f, 0f), new PresentationPoint(0f, 0f));

        Assert.NotNull(ankletModel.TrinketAccent);
        Assert.Equal(CosmeticAccentKind.Gem, ankletModel.TrinketAccent.Kind);
        Assert.NotNull(ankletModel.WeaponAccent);
        Assert.Equal(CosmeticAccentKind.Blade, ankletModel.WeaponAccent.Kind);
    }
}
