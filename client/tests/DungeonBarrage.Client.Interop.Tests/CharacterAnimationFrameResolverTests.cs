using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public sealed class CharacterAnimationFrameResolverTests
{
    private static CharacterPresentationModel CreateModel(string mainWeapon)
    {
        var player = new ClientPlayerSnapshot(
            PlayerId: "p1",
            Team: 0,
            Health: 100,
            IsEliminated: false,
            MaxHealth: 100,
            Position: new ClientPosition(0, 0),
            CollisionCenter: new ClientPosition(0, -2048),
            CollisionRadius: 2048,
            Loadout: new ClientLoadout(mainWeapon, "service-pistol", "heavy-flail", "ember-crown"),
            Ammo: Array.Empty<ClientAmmoCounter>(),
            TrinketCharge: 0,
            Statuses: Array.Empty<ClientStatusSnapshot>(),
            Appearance: new ClientAppearance("default", Array.Empty<string>(), "default"));

        return CharacterPresentationModel.Create(
            player,
            opponentX: 50 * 1024,
            positionScale: 1024,
            cellSize: 12f,
            worldOrigin: new PresentationPoint(0, 0),
            cameraOffset: new PresentationPoint(0, 0));
    }

    [Fact]
    public void Idle_resolves_row_0_and_cycles_columns()
    {
        var model = CreateModel("ramshot-cannon");
        var frame0 = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: null, visualTimeMsec: 0, isAiming: false, isAirborne: false, isMoving: false);
        Assert.Equal("crow_ramshot_cannon", frame0.SheetKey);
        Assert.Equal(0, frame0.Row);
        Assert.Equal(0, frame0.Col);

        var frame2 = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: null, visualTimeMsec: 440, isAiming: false, isAirborne: false, isMoving: false);
        Assert.Equal(0, frame2.Row);
        Assert.Equal(2, frame2.Col);
    }

    [Fact]
    public void Moving_resolves_row_1()
    {
        var model = CreateModel("frostfall-mortar");
        var frame = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: null, visualTimeMsec: 140, isAiming: false, isAirborne: false, isMoving: true);
        Assert.Equal("crow_frostfall", frame.SheetKey);
        Assert.Equal(1, frame.Row);
        Assert.Equal(1, frame.Col);
    }

    [Fact]
    public void Aiming_resolves_row_2_with_angle_elevation()
    {
        var model = CreateModel("recurve-bow");
        // Aiming upward (~60 degrees / 1.05 rad)
        var highAim = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: null, visualTimeMsec: 0, isAiming: true, isAirborne: false, isMoving: false, aimAngleRadians: 1.05f);
        Assert.Equal("crow_bow", highAim.SheetKey);
        Assert.Equal(2, highAim.Row);
        Assert.True(highAim.Col >= 3);

        // Aiming low/horizontal (-0.3 rad)
        var lowAim = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: null, visualTimeMsec: 0, isAiming: true, isAirborne: false, isMoving: false, aimAngleRadians: -0.3f);
        Assert.Equal("crow_bow", lowAim.SheetKey);
        Assert.Equal(2, lowAim.Row);
        Assert.True(lowAim.Col <= 1);
    }

    [Fact]
    public void Firing_cue_resolves_row_3()
    {
        var model = CreateModel("mole-drill");
        var fireCue = new ActorPresentationCue("p1", ActorPresentationCueKind.Fire, Age01: 0.4f, Sequence: 1, AbilityId: "mole-drill");
        var frame = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: fireCue, visualTimeMsec: 0, isAiming: false, isAirborne: false, isMoving: false);
        Assert.Equal("crow_drill", frame.SheetKey);
        Assert.Equal(3, frame.Row);
        Assert.Equal(2, frame.Col);
    }

    [Fact]
    public void Airborne_resolves_flight_sheet()
    {
        var model = CreateModel("ramshot-cannon");
        var frame = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: null, visualTimeMsec: 240, isAiming: false, isAirborne: true, isMoving: false);
        Assert.Equal("crow_flight", frame.SheetKey);
        Assert.Equal(1, frame.Row);
        Assert.Equal(2, frame.Col);
    }

    [Fact]
    public void Hit_cue_resolves_crow_damage()
    {
        var model = CreateModel("ramshot-cannon");
        var lightHit = new ActorPresentationCue("p1", ActorPresentationCueKind.Hit, Age01: 0.2f, Sequence: 1, AbilityId: null, Value: 10);
        var lightFrame = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: lightHit, visualTimeMsec: 0, isAiming: false, isAirborne: false, isMoving: false);
        Assert.Equal("crow_damage", lightFrame.SheetKey);
        Assert.Equal(0, lightFrame.Row);
        Assert.Equal(1, lightFrame.Col);

        var heavyHit = new ActorPresentationCue("p1", ActorPresentationCueKind.Hit, Age01: 0.6f, Sequence: 2, AbilityId: null, Value: 35);
        var heavyFrame = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: false, cue: heavyHit, visualTimeMsec: 0, isAiming: false, isAirborne: false, isMoving: false);
        Assert.Equal("crow_damage", heavyFrame.SheetKey);
        Assert.Equal(1, heavyFrame.Row);
        Assert.Equal(3, heavyFrame.Col);
    }

    [Fact]
    public void Defeat_resolves_crow_damage_row_2()
    {
        var model = CreateModel("ramshot-cannon");
        var frame = CharacterAnimationFrameResolver.Resolve(
            model, isEliminated: true, cue: null, visualTimeMsec: 0, isAiming: false, isAirborne: false, isMoving: false);
        Assert.Equal("crow_damage", frame.SheetKey);
        Assert.Equal(2, frame.Row);
        Assert.Equal(2, frame.Col);
    }
}
