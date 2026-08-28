using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary><see cref="RosterCatalog"/> against the real native library.</summary>
public sealed class RosterCatalogTests
{
    [Fact]
    public void Get_returns_the_nine_starters_with_no_match_created()
    {
        // No LocalMatchSession anywhere in this test — the whole point is that a roster
        // listing needs no live match to exist first.
        var roster = RosterCatalog.Get();

        Assert.Equal(1u, roster.SchemaVersion);
        Assert.Equal(9, roster.Characters.Count);

        var ids = roster.Characters.Select(c => c.Id).ToArray();
        foreach (var expected in new[]
                 {
                     "arzum", "emi", "karl", "huck", "numa", "aleph", "zeke", "roberto", "natomica",
                 })
        {
            Assert.Contains(expected, ids);
        }
    }

    [Fact]
    public void Get_reports_a_strike_ability_with_its_range_and_a_projectile_without_one()
    {
        var roster = RosterCatalog.Get();

        var huck = Assert.Single(roster.Characters, c => c.Id == "huck");
        Assert.Equal(ClientAttackShape.Strike, huck.Basic.AttackShape);
        Assert.NotNull(huck.Basic.Range);
        Assert.Equal(3, huck.Passives.Count);

        var zeke = Assert.Single(roster.Characters, c => c.Id == "zeke");
        Assert.Equal(ClientAttackShape.Projectile, zeke.Basic.AttackShape);
        Assert.Null(zeke.Basic.Range);
    }

    [Fact]
    public void Get_is_repeatable_and_deterministic()
    {
        var first = RosterCatalog.Get();
        var second = RosterCatalog.Get();

        // Compares ids in order rather than the records themselves: a record's compiler-generated
        // Equals delegates to List<T>'s reference equality for the nested Characters/Passives
        // collections, so two independently deserialized responses would compare unequal even
        // with identical content. Id order is exactly what "deterministic" needs to prove here.
        Assert.Equal(
            first.Characters.Select(c => c.Id),
            second.Characters.Select(c => c.Id));
    }
}
