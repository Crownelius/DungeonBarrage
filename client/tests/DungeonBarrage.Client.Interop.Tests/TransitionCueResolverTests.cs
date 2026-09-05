using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// Event-derived combat feedback remains downstream of the authoritative transition and its
/// visual clock. These tests intentionally contain no Godot or native-library dependency.
/// </summary>
public sealed class TransitionCueResolverTests
{
    [Fact]
    public void Frozen_ability_emits_owner_fire_at_zero_and_target_hit_at_authoritative_impact()
    {
        var transition = Deserialize<ClientMatchTransition>("responses/002-ability.json");

        var fireFrame = TransitionCueResolver.Resolve(
            transition.Events,
            elapsedMsec: 0,
            visualTickRate: 15,
            reduceMotion: false);

        var fire = Assert.Single(fireFrame.ActorCues);
        Assert.Equal("a-local-player", fire.PlayerId);
        Assert.Equal(ActorPresentationCueKind.Fire, fire.Kind);
        Assert.Equal("crow-precision-57", fire.AbilityId);
        Assert.Equal(0f, fire.Age01);
        Assert.Empty(fireFrame.ImpactCues);

        var impactFrame = TransitionCueResolver.Resolve(
            transition.Events,
            elapsedMsec: ImpactMsec(9, visualTickRate: 15),
            visualTickRate: 15,
            reduceMotion: false);

        var hit = Assert.Single(impactFrame.ActorCues, cue => cue.Kind == ActorPresentationCueKind.Hit);
        Assert.Equal("b-local-bot", hit.PlayerId);
        Assert.Equal(ActorPresentationCueKind.Hit, hit.Kind);
        var impact = Assert.Single(impactFrame.ImpactCues);
        Assert.Equal(new ClientPosition(6447, 5888), impact.Position);
        Assert.Equal(ClientImpactCause.Character, impact.Cause);
        Assert.True(impactFrame.CameraImpulse.IsActive);
    }

    [Fact]
    public void Impact_timing_uses_the_visual_tick_rate_not_the_authority_rate()
    {
        var transition = Deserialize<ClientMatchTransition>("responses/002-ability.json");

        var beforeImpact = TransitionCueResolver.Resolve(
            transition.Events,
            elapsedMsec: 500,
            visualTickRate: 15,
            reduceMotion: false);
        Assert.Empty(beforeImpact.ImpactCues);

        var atImpact = TransitionCueResolver.Resolve(
            transition.Events,
            elapsedMsec: ImpactMsec(9, visualTickRate: 15),
            visualTickRate: 15,
            reduceMotion: false);
        Assert.Single(atImpact.ImpactCues);
    }

    [Fact]
    public void Health_gain_or_non_health_events_never_create_hit_feedback()
    {
        IReadOnlyList<ClientPresentationEvent> events =
        [
            new ClientHealthChangedEvent(
                PresentationTick: 0,
                Sequence: 0,
                PlayerId: "a-local-player",
                PreviousHealth: 200,
                NewHealth: 240,
                Breakdown: null),
            new ClientGaugeChangedEvent(
                PresentationTick: 0,
                Sequence: 1,
                PlayerId: "a-local-player",
                PreviousGauge: 10,
                NewGauge: 20,
                Delta: 10),
            new ClientTerrainChangedEvent(
                PresentationTick: 0,
                Sequence: 2,
                TerrainGeneration: 2,
                DirtyRectangles: []),
            new ClientImpactEvent(
                PresentationTick: 0,
                Sequence: 3,
                TraceId: 0,
                Impact: new ClientImpact(
                    new ClientPosition(1024, 2048),
                    Tick: 0,
                    Cause: ClientImpactCause.Terrain)),
        ];

        var frame = TransitionCueResolver.Resolve(
            events,
            elapsedMsec: 0,
            visualTickRate: 60,
            reduceMotion: false);

        Assert.DoesNotContain(frame.ActorCues, cue => cue.Kind == ActorPresentationCueKind.Hit);
        Assert.Single(frame.ImpactCues);
    }

    [Fact]
    public void Reduced_motion_keeps_authoritative_cues_but_suppresses_camera_impulse()
    {
        var transition = Deserialize<ClientMatchTransition>("responses/002-ability.json");

        var frame = TransitionCueResolver.Resolve(
            transition.Events,
            elapsedMsec: ImpactMsec(9, visualTickRate: 15),
            visualTickRate: 15,
            reduceMotion: true);

        Assert.Single(frame.ActorCues, cue => cue.Kind == ActorPresentationCueKind.Hit);
        Assert.Single(frame.ImpactCues);
        Assert.False(frame.CameraImpulse.IsActive);
    }

    [Fact]
    public void Cues_expire_before_the_next_input_window()
    {
        var transition = Deserialize<ClientMatchTransition>("responses/002-ability.json");
        var afterImpactHold = ImpactMsec(9, visualTickRate: 15) + TransitionCueResolver.ImpactDurationMsec;

        var frame = TransitionCueResolver.Resolve(
            transition.Events,
            elapsedMsec: afterImpactHold,
            visualTickRate: 15,
            reduceMotion: false);

        Assert.Empty(frame.ActorCues);
        Assert.Empty(frame.ImpactCues);
        Assert.False(frame.CameraImpulse.IsActive);
    }

    [Fact]
    public void Defeat_wins_over_concurrent_fire_and_hit_for_the_same_actor()
    {
        IReadOnlyList<ClientPresentationEvent> events =
        [
            new ClientProjectileTraceEvent(
                PresentationTick: 0,
                Sequence: 0,
                Trace: new ClientProjectileTrace(
                    TraceId: 0,
                    OwnerId: "a-local-player",
                    AbilityId: "ramshot-cannon",
                    Samples: [],
                    TerminalImpact: new ClientImpact(
                        new ClientPosition(0, 0),
                        Tick: 0,
                        Cause: ClientImpactCause.Expired))),
            new ClientHealthChangedEvent(
                PresentationTick: 0,
                Sequence: 1,
                PlayerId: "a-local-player",
                PreviousHealth: 100,
                NewHealth: 0,
                Breakdown: null),
            new ClientPlayerEliminatedEvent(
                PresentationTick: 0,
                Sequence: 2,
                PlayerId: "a-local-player",
                Cause: new ClientAuthoritativeResolutionEliminationCause()),
        ];

        var frame = TransitionCueResolver.Resolve(
            events,
            elapsedMsec: 0,
            visualTickRate: 60,
            reduceMotion: false);

        var cue = Assert.Single(frame.ActorCues);
        Assert.Equal(ActorPresentationCueKind.Defeat, cue.Kind);
        Assert.Equal(2u, cue.Sequence);
    }

    private static ulong ImpactMsec(uint presentationTick, uint visualTickRate) =>
        (ulong)presentationTick * 1000UL / visualTickRate;

    private static T Deserialize<T>(string relativePath)
        where T : class =>
        JsonSerializer.Deserialize<T>(Fixtures.Read(relativePath).Span, ClientEnvelope.Options)
        ?? throw new InvalidDataException($"Fixture '{relativePath}' decoded to null.");
}
