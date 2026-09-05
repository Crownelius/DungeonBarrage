using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public sealed class TacticalHudModelTests
{
    private static ClientPlayerSnapshot MakePlayer(ushort hp, ushort maxHp, ushort charge, ushort ammo = 3) =>
        new(
            PlayerId: "test-crow",
            Team: 0,
            Health: hp,
            IsEliminated: hp <= 0,
            MaxHealth: maxHp,
            Position: new ClientPosition(0, 0),
            CollisionCenter: new ClientPosition(0, -2048),
            CollisionRadius: 2048,
            CharacterId: "crow",
            Loadout: new ClientLoadout("ramshot-cannon", "ramshot-shell", "trench-spade", "ember-crown"),
            Ammo: new[] { new ClientAmmoCounter(ammo, ammo, ClientAmmoPolicy.Finite) },
            TrinketCharge: charge,
            Statuses: Array.Empty<ClientStatusSnapshot>(),
            Appearance: new ClientAppearance("default", Array.Empty<string>(), "default"));

    [Fact]
    public void Wind_display_model_categorizes_directions_and_magnitudes()
    {
        var calm = WindDisplayModel.Create(0, "ramshot-cannon");
        Assert.Equal(WindDirection.Calm, calm.Direction);
        Assert.Equal(0f, calm.NormalizedIntensity);
        Assert.Contains("CALM", calm.FormattedText);

        var left = WindDisplayModel.Create(-40, "ramshot-cannon");
        Assert.Equal(WindDirection.BlowingLeft, left.Direction);
        Assert.Equal(0.5f, left.NormalizedIntensity, 2);
        Assert.Contains("WEST", left.FormattedText);
        Assert.Contains("«", left.FormattedText);

        var right = WindDisplayModel.Create(80, "ramshot-cannon");
        Assert.Equal(WindDirection.BlowingRight, right.Direction);
        Assert.Equal(1.0f, right.NormalizedIntensity, 2);
        Assert.Contains("EAST", right.FormattedText);
        Assert.Contains("»", right.FormattedText);

        var extreme = WindDisplayModel.Create(-200, "ramshot-cannon");
        Assert.Equal(WindDirection.BlowingLeft, extreme.Direction);
        Assert.Equal(1.0f, extreme.NormalizedIntensity); // Clamped at 1.0
    }

    [Theory]
    [InlineData("service-pistol", WindSensitivityTier.Immune, "IMMUNE")]
    [InlineData("line-repeater", WindSensitivityTier.Immune, "IMMUNE")]
    [InlineData("mole-drill", WindSensitivityTier.Resistant, "HEAVY")]
    [InlineData("returning-boomerang", WindSensitivityTier.Resistant, "HEAVY")]
    [InlineData("tide-sprayer", WindSensitivityTier.Resistant, "HEAVY")]
    [InlineData("ramshot-cannon", WindSensitivityTier.Standard, "STD")]
    [InlineData("frostfall-mortar", WindSensitivityTier.Standard, "STD")]
    [InlineData("recurve-bow", WindSensitivityTier.High, "LIGHT")]
    [InlineData(null, WindSensitivityTier.Standard, "STD")]
    public void Wind_display_model_resolves_weapon_sensitivity(
        string? weaponId, WindSensitivityTier expectedTier, string expectedBadge)
    {
        var model = WindDisplayModel.Create(25, weaponId);
        Assert.Equal(expectedTier, model.Sensitivity);
        Assert.Equal(expectedBadge, model.SensitivityBadge);
    }

    [Fact]
    public void Player_status_plate_computes_health_and_charges_correctly()
    {
        var fullHp = MakePlayer(280, 280, charge: 0);
        var fullPlate = PlayerStatusPlateModel.Create(fullHp);
        Assert.Equal(1.0f, fullPlate.HealthFraction);
        Assert.False(fullPlate.IsLowHealth);
        Assert.False(fullPlate.TrinketReady);
        Assert.Equal(0, fullPlate.TrinketCharge);

        var halfCharge = MakePlayer(50, 280, charge: 5_000);
        var halfPlate = PlayerStatusPlateModel.Create(halfCharge);
        Assert.True(halfPlate.HealthFraction < 0.25f);
        Assert.True(halfPlate.IsLowHealth);
        Assert.False(halfPlate.TrinketReady);
        Assert.Equal(1, halfPlate.TrinketCharge);

        var lowHp = MakePlayer(50, 280, charge: 10_000);
        var lowPlate = PlayerStatusPlateModel.Create(lowHp);
        Assert.True(lowPlate.HealthFraction < 0.25f);
        Assert.True(lowPlate.IsLowHealth);
        Assert.True(lowPlate.TrinketReady);
        Assert.Equal(2, lowPlate.TrinketCharge);

    }

    [Fact]
    public void Player_status_plate_maps_cues()
    {
        var player = MakePlayer(280, 280, 0);

        var fireCue = new ActorPresentationCue("test-crow", ActorPresentationCueKind.Fire, 0f, 1, "ramshot-cannon");
        var firePlate = PlayerStatusPlateModel.Create(player, fireCue);
        Assert.Equal("FIRE", firePlate.CueLabel);

        var hitCue = new ActorPresentationCue("test-crow", ActorPresentationCueKind.Hit, 0f, 2, null);
        var hitPlate = PlayerStatusPlateModel.Create(player, hitCue);
        Assert.Equal("HIT", hitPlate.CueLabel);

        var defeatCue = new ActorPresentationCue("test-crow", ActorPresentationCueKind.Defeat, 0f, 3, null);
        var defeatPlate = PlayerStatusPlateModel.Create(player, defeatCue);
        Assert.Equal("DOWN", defeatPlate.CueLabel);
    }
}
