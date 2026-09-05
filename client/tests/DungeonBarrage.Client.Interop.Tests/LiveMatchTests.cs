using System.Diagnostics.CodeAnalysis;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using DungeonBarrage.Client.Match;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// <see cref="LiveMatch"/> against the real native library and the real fixture — the same
/// move-then-fire sequence <c>CommandRoundTripTests</c> proves with hand-built commands, but
/// driven the way a real caller (Godot's <c>Main</c>) actually drives it: by reading
/// <see cref="LiveMatch.CurrentSnapshot"/> at submit time and letting the class generate its own
/// command ids, rather than hardcoding the fixture's exact ones.
/// </summary>
/// <remarks>
/// <para>
/// <b>A note on why this does not assert a frozen hash.</b> An early version of this test did,
/// and failed — not because anything was broken, but because the assumption was wrong.
/// <c>hash_state</c> deliberately folds <c>processed_command_ids</c> into the state hash
/// (<c>db-sim-core/src/hash.rs</c>, domain <c>0x04</c>, with its own
/// <c>adding_a_processed_command_id_changes_hash</c> test proving it is intentional): the exact
/// set of accepted idempotency keys is part of authoritative state, not incidental to it. A
/// <see cref="LiveMatch"/>-driven session generates its own ids, so its hash can never equal one
/// frozen from the fixture's literal <c>"fixture-move-001"</c>/<c>"fixture-ability-002"</c> ids —
/// by design, not by defect. What these tests check instead is what is actually invariant:
/// disposition, concrete gameplay facts (damage, turn handoff), and reconciliation.
/// </para>
/// </remarks>
public sealed class LiveMatchTests
{
    [Fact]
    public async Task A_move_then_an_ability_deals_real_damage_and_reconciles_to_the_post_snapshot()
    {
        await using var live = await CreateLiveAsync();

        var beforeDefenderHealth = live.CurrentSnapshot.Players
            .First(p => p.PlayerId == "b-local-bot").Health;

        var move = await live.SubmitMoveAsync(1024);
        Assert.Equal(ClientTransitionDisposition.Accepted, move.Disposition);
        Assert.Equal(0u, move.InputLockTicks);
        Assert.Single(move.Events);
        Assert.IsType<ClientEntityMovedEvent>(move.Events[0]);

        var ability = await live.SubmitAbilityAsync(ClientAbilitySlot.Main, 0, 2_500, null);
        Assert.Equal(ClientTransitionDisposition.Accepted, ability.Disposition);

        // The concrete gameplay facts a live-generated command id cannot change: real damage
        // landed, and the turn actually handed over to the other player.
        var afterDefenderHealth = live.CurrentSnapshot.Players
            .First(p => p.PlayerId == "b-local-bot").Health;
        Assert.True(afterDefenderHealth < beforeDefenderHealth, "the ability must have dealt damage");

        // CONTENT_VERSION 6: this 15% lob no longer dumps the shooter into the void, so the
        // fixture now hands the turn over instead of ending the match.
        Assert.IsType<ClientInProgressOutcome>(live.CurrentSnapshot.Outcome);
        Assert.Equal("b-local-bot", live.CurrentSnapshot.ActivePlayerId);

        // The C5 gate itself: every view ends at the post-snapshot. Checked, not assumed true by
        // LiveMatch's own construction.
        Assert.Equal(ability.PostSnapshot.StateHash, live.CurrentSnapshot.StateHash);
    }

    /// <summary>
    /// Restores the turn-handover assertions on a map where a turn can actually hand over.
    /// </summary>
    [Fact]
    public async Task A_completed_turn_hands_over_to_the_other_player()
    {
        await using var live = await CreateLiveOnAsync("crow-perch");

        _ = await live.SubmitMoveAsync(1024);
        var ability = await live.SubmitAbilityAsync(ClientAbilitySlot.Main, 45_000, 1_500, null);
        Assert.Equal(ClientTransitionDisposition.Accepted, ability.Disposition);

        Assert.IsType<ClientInProgressOutcome>(live.CurrentSnapshot.Outcome);
        Assert.Equal("b-local-bot", live.CurrentSnapshot.ActivePlayerId);
        Assert.Equal(2u, live.CurrentSnapshot.TurnNumber);
        Assert.Equal(ability.PostSnapshot.StateHash, live.CurrentSnapshot.StateHash);
    }

    [Fact]
    public async Task The_ability_reports_a_real_nonzero_input_lock_that_a_caller_must_honor()
    {
        await using var live = await CreateLiveAsync();
        _ = await live.SubmitMoveAsync(1024);

        var ability = await live.SubmitAbilityAsync(ClientAbilitySlot.Main, 45_000, 1_500, null);

        // Unlike the move above (0 ticks — nothing to play back), a strike with a projectile
        // flight genuinely has something to lock input for. This is the number
        // Main._inputLockedUntilMsec is computed from.
        Assert.True(ability.InputLockTicks > 0, "a resolved strike must report a real lock window");
        Assert.True(ability.PresentationTickRate > 0);
    }

    [Fact]
    public async Task The_same_scripted_sequence_is_deterministic_across_independent_sessions()
    {
        // Not a match against the frozen fixture (see the class remarks) — a match against
        // itself. Two freshly created sessions, given the exact same generated command ids (by
        // starting both LiveMatch instances from ordinal zero) and the exact same inputs, must
        // reach the exact same hash. If they didn't, "deterministic simulation" would be a lie.
        await using var first = await CreateLiveAsync();
        await using var second = await CreateLiveAsync();

        _ = await first.SubmitMoveAsync(1024);
        _ = await second.SubmitMoveAsync(1024);
        var firstAbility = await first.SubmitAbilityAsync(ClientAbilitySlot.Main, 45_000, 1_500, null);
        var secondAbility = await second.SubmitAbilityAsync(ClientAbilitySlot.Main, 45_000, 1_500, null);

        Assert.Equal(firstAbility.PostStateHash, secondAbility.PostStateHash);
    }

    [Fact]
    public async Task A_preview_returns_a_trace_and_does_not_change_the_match()
    {
        await using var live = await CreateLiveOnAsync("crow-perch");
        var hashBefore = live.CurrentSnapshot.StateHash;

        var preview = await live.PreviewAbilityAsync(ClientAbilitySlot.Main, 45_000, 1_500);

        Assert.NotNull(preview);
        Assert.True(preview.Legal, "a 45° 15% ramshot on crow-perch must be a legal guide");
        Assert.NotEmpty(preview.ProjectileTraces);
        Assert.True(preview.ProjectileTraces[0].Samples.Count >= 2, "a guide must have a path, not a single point");
        Assert.Equal(hashBefore, live.CurrentSnapshot.StateHash);
    }

    [Fact]
    public async Task A_shot_straight_down_is_accepted_instead_of_rejected_as_a_negative_angle()
    {
        await using var live = await CreateLiveOnAsync("crow-perch");
        _ = await live.SubmitMoveAsync(1024);
        var ability = await live.SubmitAbilityAsync(ClientAbilitySlot.Main, 270_000, 1_500, null);
        Assert.Equal(ClientTransitionDisposition.Accepted, ability.Disposition);
        Assert.NotEmpty(ability.Events);
        Assert.Contains(ability.Events, e => e is ClientProjectileTraceEvent);
    }

    [Fact]
    public async Task Every_normal_projectile_action_emits_a_trace_that_reaches_its_impact()
    {
        var roster = RosterCatalog.Get();
        var projectileItems = roster.Characters
            .SelectMany(character => new[]
            {
                (CharacterId: character.Id, Ability: character.Shot1),
                (CharacterId: character.Id, Ability: character.Shot2OrMelee),
            })
            .Where(entry => entry.Ability.AttackShape == ClientAttackShape.Projectile)
            .ToList();
        Assert.True(projectileItems.Count >= 5, "launch roster must expose its normal projectile actions");

        foreach (var item in projectileItems)
        {
            var slot = item.Ability.Slot;
            await using var live = await CreateLiveOnAsync("crow-perch", item.CharacterId);
            _ = await live.SubmitMoveAsync(1024);
            var ability = await live.SubmitAbilityAsync(slot, 45_000, 5_000, null);
            Assert.Equal(ClientTransitionDisposition.Accepted, ability.Disposition);

            var traces = ability.Events.OfType<ClientProjectileTraceEvent>().Select(e => e.Trace).ToList();
            Assert.True(traces.Count > 0, $"{item.Ability.Id} must publish at least one projectile trace");
            foreach (var trace in traces)
            {
                Assert.True(trace.Samples.Count >= 2, $"{item.Ability.Id} must sample more than the origin");
                Assert.Equal(0u, trace.Samples[0].Tick);
                var last = trace.Samples[trace.Samples.Count - 1];
                Assert.Equal(trace.TerminalImpact.Tick, last.Tick);
                Assert.True(
                    ProjectilePlayback.LastSampleTick(trace) > 0,
                    $"{item.Ability.Id} flight must last more than a single tick so playback can show the hit");
            }
        }
    }

    [Fact]
    public async Task A_jump_spends_walk_allowance_and_lands()
    {
        await using var live = await CreateLiveOnAsync("crow-perch");
        var before = live.CurrentSnapshot;
        var yBefore = before.Players.First(p => p.PlayerId == before.ActivePlayerId).Position.Y;
        var moveBefore = before.MovementRemaining;

        var jump = await live.SubmitJumpAsync();
        Assert.Equal(ClientTransitionDisposition.Accepted, jump.Disposition);

        var after = live.CurrentSnapshot;
        var yAfter = after.Players.First(p => p.PlayerId == before.ActivePlayerId).Position.Y;
        Assert.True(after.MovementRemaining < moveBefore, "a successful jump spends walk allowance");
        Assert.True(
            yAfter >= yBefore - 256,
            "gravity must land the crow; they must not hang at the apex");
    }

    [SuppressMessage(
        "Reliability",
        "CA2000:Dispose objects before losing scope",
        Justification =
            "Ownership transfers to the returned LiveMatch, which every caller disposes via " +
            "'await using'.")]
    private static async Task<LiveMatch> CreateLiveAsync()
    {
        var session = LocalMatchSession.Create(Fixtures.Read("create-request.json").Span);
        var createResponse = System.Text.Json.JsonSerializer.Deserialize<ClientCreateResponse>(
            session.CreateResponse.Span, ClientEnvelope.Options)!;
        var terrain = await session.TerrainAsync(ulong.MaxValue);
        return LiveMatch.Create(session, createResponse.Snapshot!, terrain, "test");
    }

    [SuppressMessage(
        "Reliability",
        "CA2000:Dispose objects before losing scope",
        Justification =
            "Ownership transfers to the returned LiveMatch, which every caller disposes via " +
            "'await using'.")]
    private static Task<LiveMatch> CreateLiveOnAsync(string mapId) =>
        CreateLiveOnAsync(mapId, LocalMatchEnvelope.LaunchDefaultCharacterId);

    [SuppressMessage(
        "Reliability",
        "CA2000:Dispose objects before losing scope",
        Justification =
            "Ownership transfers to the returned LiveMatch, which every caller disposes via " +
            "'await using'.")]
    private static async Task<LiveMatch> CreateLiveOnAsync(string mapId, string humanCharacterId)
    {
        var request = LocalMatchEnvelope.HumanVsBot(
            LocalMatchSession.SimulationVersion,
            LocalMatchSession.ContentVersion,
            seed: 12345,
            matchId: "test-match",
            mapId: mapId,
            humanCharacterId: humanCharacterId);
        var session = LocalMatchSession.Create(
            System.Text.Json.JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options));
        var createResponse = System.Text.Json.JsonSerializer.Deserialize<ClientCreateResponse>(
            session.CreateResponse.Span, ClientEnvelope.Options)!;
        var terrain = await session.TerrainAsync(ulong.MaxValue);
        return LiveMatch.Create(session, createResponse.Snapshot!, terrain, "test");
    }

    [Fact]
    public async Task Crows_precision_shot_hits_the_opponent_on_crow_perch()
    {
        await using var live = await CreateLiveOnAsync("crow-perch");
        _ = await live.SubmitMoveAsync(1024);
        var defenderId = live.CurrentSnapshot.Players.First(p => p.PlayerId != live.CurrentSnapshot.ActivePlayerId).PlayerId;
        var hpBefore = live.CurrentSnapshot.Players.First(p => p.PlayerId == defenderId).Health;
        var transition = await live.SubmitAbilityAsync(ClientAbilitySlot.Main, 0, 2_500, null);
        var hpAfter = live.CurrentSnapshot.Players.First(p => p.PlayerId == defenderId).Health;
        Assert.Equal(ClientTransitionDisposition.Accepted, transition.Disposition);
        Assert.True(hpAfter < hpBefore, $"Expected damage, before={hpBefore}, after={hpAfter}");
        var hitEvents = transition.Events.OfType<ClientHealthChangedEvent>().ToList();
        Assert.NotEmpty(hitEvents);
    }
}
