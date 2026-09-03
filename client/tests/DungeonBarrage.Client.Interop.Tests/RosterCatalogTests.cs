using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary><see cref="RosterCatalog"/> against the real native library.</summary>
public sealed class RosterCatalogTests
{
    [Fact]
    public void Get_returns_the_crow_and_thirty_two_items_with_no_match_created()
    {
        // No LocalMatchSession anywhere in this test — the whole point is that a roster
        // listing needs no live match to exist first.
        var roster = RosterCatalog.Get();

        Assert.Equal(1u, roster.SchemaVersion);
        Assert.Equal("crow", roster.Fighter.Id);
        Assert.Equal((ushort)280, roster.Fighter.MaxHealth);
        Assert.Equal(32, roster.Items.Count);

        var ids = roster.Items.Select(item => item.Id).ToArray();
        foreach (var expected in new[]
                 {
                     "ramshot-cannon", "recurve-bow", "line-repeater", "returning-boomerang",
                     "ramshot-shell", "trench-spade", "longsword", "ember-crown", "gale-anklet",
                 })
        {
            Assert.Contains(expected, ids);
        }
    }

    [Fact]
    public void Get_reports_a_strike_item_with_its_range_and_a_projectile_without_one()
    {
        var roster = RosterCatalog.Get();

        var sword = Assert.Single(roster.Items, item => item.Id == "longsword");
        Assert.Equal(ClientAttackShape.Strike, sword.Ability.AttackShape);
        Assert.NotNull(sword.Ability.Range);
        Assert.Equal(ClientAbilitySlot.MeleeTool, sword.Slot);
        Assert.Equal(ClientAmmoPolicy.Finite, sword.AmmoPolicy);

        var crown = Assert.Single(roster.Items, item => item.Id == "ember-crown");
        Assert.Equal(ClientAbilitySlot.Trinket, crown.Slot);
        Assert.Equal(ClientAmmoPolicy.Unlimited, crown.AmmoPolicy);

        var ramshot = Assert.Single(roster.Items, item => item.Id == "ramshot-cannon");
        Assert.Equal(ClientAttackShape.Projectile, ramshot.Ability.AttackShape);
        Assert.Null(ramshot.Ability.Range);
        Assert.Equal(ClientAbilitySlot.Main, ramshot.Slot);
    }

    [Fact]
    public void Get_is_repeatable_and_deterministic()
    {
        var first = RosterCatalog.Get();
        var second = RosterCatalog.Get();

        Assert.Equal(first.Fighter.Id, second.Fighter.Id);
        Assert.Equal(
            first.Items.Select(item => item.Id),
            second.Items.Select(item => item.Id));
    }
}
