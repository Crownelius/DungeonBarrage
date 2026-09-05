using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

public sealed class AimPreviewPresentationResolverTests
{
    [Fact]
    public void Empty_preview_has_no_guide()
    {
        Assert.Null(AimPreviewPresentationResolver.Resolve(Array.Empty<ClientProjectileTrace>()));
    }

    [Fact]
    public void A_character_impact_is_the_single_positive_guide()
    {
        var terrain = Trace(0, ClientImpactCause.Terrain);
        var character = Trace(3, ClientImpactCause.Character);

        var guide = AimPreviewPresentationResolver.Resolve([terrain, character]);

        Assert.NotNull(guide);
        Assert.Same(character, guide.Trace);
        Assert.True(guide.HitsTarget);
    }

    [Fact]
    public void Lowest_trace_id_is_stable_for_hits_and_misses()
    {
        var highHit = Trace(9, ClientImpactCause.Character);
        var lowHit = Trace(2, ClientImpactCause.Character);
        var lowerMiss = Trace(1, ClientImpactCause.OutOfBounds);

        var guide = AimPreviewPresentationResolver.Resolve([highHit, lowerMiss, lowHit]);

        Assert.NotNull(guide);
        Assert.Same(lowHit, guide.Trace);
        Assert.True(guide.HitsTarget);

        guide = AimPreviewPresentationResolver.Resolve([Trace(7, ClientImpactCause.Expired), lowerMiss]);
        Assert.NotNull(guide);
        Assert.Same(lowerMiss, guide.Trace);
        Assert.False(guide.HitsTarget);
    }

    private static ClientProjectileTrace Trace(uint id, ClientImpactCause cause) => new(
        TraceId: id,
        OwnerId: "actor",
        AbilityId: "test",
        Samples:
        [
            new ClientProjectileSample(0, new ClientPosition(0, 0)),
            new ClientProjectileSample(1, new ClientPosition(1, 1)),
        ],
        TerminalImpact: new ClientImpact(new ClientPosition(1, 1), 1, cause));
}
