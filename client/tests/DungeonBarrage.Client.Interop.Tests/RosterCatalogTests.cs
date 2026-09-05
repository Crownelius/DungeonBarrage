using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary><see cref="RosterCatalog"/> against the real native library.</summary>
public sealed class RosterCatalogTests
{
    [Fact]
    public void Get_returns_the_four_complete_character_kits_without_a_match()
    {
        var roster = RosterCatalog.Get();

        Assert.Equal(ClientContract.CurrentSchemaVersion, roster.SchemaVersion);
        Assert.Equal(["leslie", "crow", "erus", "kreena"], roster.Characters.Select(c => c.Id));
        Assert.All(roster.Characters, character =>
        {
            Assert.True(character.MaxHealth > 0);
            Assert.True(character.MovementAllowance > 0);
            Assert.Equal(ClientAbilitySlot.Main, character.Shot1.Slot);
            Assert.Equal(ClientAbilitySlot.Secondary, character.Shot2OrMelee.Slot);
            Assert.Equal(ClientAbilitySlot.Trinket, character.Special.Slot);
        });
    }

    [Fact]
    public void Get_exposes_each_character_tactical_shape()
    {
        var roster = RosterCatalog.Get();

        var leslie = Assert.Single(roster.Characters, c => c.Id == "leslie");
        Assert.Equal("Ant Glob", leslie.Shot1.DisplayName);
        Assert.Equal(ClientAttackShape.Strike, leslie.Shot2OrMelee.AttackShape);
        Assert.NotNull(leslie.Shot2OrMelee.Range);

        var crow = Assert.Single(roster.Characters, c => c.Id == "crow");
        Assert.Equal("5.7 High-Velocity Precision", crow.Shot1.DisplayName);
        Assert.Equal("Aerial Barrage", crow.Special.DisplayName);

        var erus = Assert.Single(roster.Characters, c => c.Id == "erus");
        Assert.Equal("Celestial Staff Battery", erus.Special.DisplayName);

        var kreena = Assert.Single(roster.Characters, c => c.Id == "kreena");
        Assert.Equal("Global Magic Arrow", kreena.Special.DisplayName);
    }

    [Fact]
    public void Get_is_repeatable_and_deterministic()
    {
        var first = RosterCatalog.Get();
        var second = RosterCatalog.Get();

        Assert.Equal(first.Characters.Select(character => character.Id), second.Characters.Select(character => character.Id));
    }
}
