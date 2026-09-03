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

    [Fact]
    public void Equipment_accents_switch_with_active_slot()
    {
        var player = new ClientPlayerSnapshot(
            PlayerId: "p1",
            Team: 0,
            Health: 280,
            IsEliminated: false,
            MaxHealth: 280,
            Position: new ClientPosition(0, 0),
            CollisionCenter: new ClientPosition(0, -2048),
            CollisionRadius: 2048,
            Loadout: new ClientLoadout(
                Main: "ramshot-cannon",
                Secondary: "frostfall-shell",
                MeleeTool: "trench-spade",
                Trinket: "ember-crown"),
            Ammo: Array.Empty<ClientAmmoCounter>(),
            TrinketCharge: 0,
            Statuses: Array.Empty<ClientStatusSnapshot>(),
            Appearance: new ClientAppearance("default", Array.Empty<string>(), "default"));

        // Main slot
        var mainModel = CharacterPresentationModel.Create(
            player, null, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0),
            activeSlot: ClientAbilitySlot.Main);
        Assert.NotNull(mainModel.WeaponAccent);
        Assert.Equal(CosmeticAccentKind.Cannon, mainModel.WeaponAccent.Kind);

        // Secondary slot
        var secModel = CharacterPresentationModel.Create(
            player, null, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0),
            activeSlot: ClientAbilitySlot.Secondary);
        Assert.NotNull(secModel.WeaponAccent);
        Assert.Equal(CosmeticAccentKind.Ordnance, secModel.WeaponAccent.Kind);

        // Melee slot
        var meleeModel = CharacterPresentationModel.Create(
            player, null, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0),
            activeSlot: ClientAbilitySlot.MeleeTool);
        Assert.NotNull(meleeModel.WeaponAccent);
        Assert.Equal(CosmeticAccentKind.Blade, meleeModel.WeaponAccent.Kind);

        // Trinket slot
        var trinketModel = CharacterPresentationModel.Create(
            player, null, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0),
            activeSlot: ClientAbilitySlot.Trinket);
        Assert.Null(trinketModel.WeaponAccent);
        Assert.NotNull(trinketModel.TrinketAccent);
        Assert.Equal(CosmeticAccentKind.Crown, trinketModel.TrinketAccent.Kind);
    }

    [Fact]
    public void Bow_and_ordnance_kinds_resolve_expected_accents()
    {
        var bowPlayer = MakePlayer(x: 0, y: 0, "recurve-bow", "ember-crown");
        var bowModel = CharacterPresentationModel.Create(
            bowPlayer, null, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0));
        Assert.NotNull(bowModel.WeaponAccent);
        Assert.Equal(CosmeticAccentKind.Bow, bowModel.WeaponAccent.Kind);

        var ordPlayer = MakePlayer(x: 0, y: 0, "ramshot-shell", "ember-crown");
        var ordModel = CharacterPresentationModel.Create(
            ordPlayer, null, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0));
        Assert.NotNull(ordModel.WeaponAccent);
        Assert.Equal(CosmeticAccentKind.Ordnance, ordModel.WeaponAccent.Kind);
    }

    [Fact]
    public void Aim_vector_reflects_elevation_angle_and_facing()
    {
        var player = MakePlayer(x: 10 * 1024, y: 10 * 1024, "ramshot-cannon", "ember-crown");
        var angle = MathF.PI / 4f; // 45 degrees up

        // Facing right (opponent to the right)
        var rightModel = CharacterPresentationModel.Create(
            player, opponentX: 50 * 1024, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0),
            aimAngleRadians: angle);
        Assert.NotNull(rightModel.AimVector);
        Assert.True(rightModel.AimVector.Value.X > 0f);
        Assert.True(rightModel.AimVector.Value.Y < 0f); // Screen Y is down, so negative Y is up

        // Facing left (opponent to the left)
        var leftModel = CharacterPresentationModel.Create(
            player, opponentX: 0, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0),
            aimAngleRadians: angle);
        Assert.NotNull(leftModel.AimVector);
        Assert.True(leftModel.AimVector.Value.X < 0f);
        Assert.True(leftModel.AimVector.Value.Y < 0f);

        // Not aiming
        var neutralModel = CharacterPresentationModel.Create(
            player, opponentX: 50 * 1024, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0));
        Assert.Null(neutralModel.AimVector);
        Assert.Null(neutralModel.AimAngleRadians);
    }

    [Fact]
    public void Bob_offset_respects_reduced_motion()
    {
        var player = MakePlayer(x: 0, y: 0, "ramshot-cannon", "ember-crown");
        var model = CharacterPresentationModel.Create(
            player, null, 1024, 12f, new PresentationPoint(0, 0), new PresentationPoint(0, 0));

        // Reduced motion always returns 0
        Assert.Equal(0f, model.BobOffsetY(500, reduceMotion: true));
        Assert.Equal(0f, model.BobOffsetY(1500, reduceMotion: true));

        // Normal motion provides subtle cyclic offset
        var offset = model.BobOffsetY(500, reduceMotion: false);
        Assert.NotEqual(0f, offset);
        Assert.InRange(MathF.Abs(offset), 0f, model.Body.Radius * 0.1f);
    }
}
