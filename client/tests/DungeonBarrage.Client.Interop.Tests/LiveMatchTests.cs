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
    public async Task A_move_then_an_ability_deals_damage_and_hands_the_turn_over()
    {
        await using var live = await CreateLiveAsync();

        var beforeDefenderHealth = live.CurrentSnapshot.Players
            .First(p => p.PlayerId == "b-local-bot").Health;

        var move = await live.SubmitMoveAsync(1024);
        Assert.Equal(ClientTransitionDisposition.Accepted, move.Disposition);
        Assert.Equal(0u, move.InputLockTicks);
        Assert.Single(move.Events);
        Assert.IsType<ClientEntityMovedEvent>(move.Events[0]);

        var ability = await live.SubmitAbilityAsync(ClientAbilitySlot.Basic, 45_000, 1_500, null);
        Assert.Equal(ClientTransitionDisposition.Accepted, ability.Disposition);

        // The concrete gameplay facts a live-generated command id cannot change: real damage
        // landed, and the turn actually handed over to the other player.
        var afterDefenderHealth = live.CurrentSnapshot.Players
            .First(p => p.PlayerId == "b-local-bot").Health;
        Assert.True(afterDefenderHealth < beforeDefenderHealth, "the ability must have dealt damage");
        Assert.Equal("b-local-bot", live.CurrentSnapshot.ActivePlayerId);
        Assert.Equal(2u, live.CurrentSnapshot.TurnNumber);

        // The C5 gate itself: every view ends at the post-snapshot. Checked, not assumed true by
        // LiveMatch's own construction.
        Assert.Equal(ability.PostSnapshot.StateHash, live.CurrentSnapshot.StateHash);
    }

    [Fact]
    public async Task The_ability_reports_a_real_nonzero_input_lock_that_a_caller_must_honor()
    {
        await using var live = await CreateLiveAsync();
        _ = await live.SubmitMoveAsync(1024);

        var ability = await live.SubmitAbilityAsync(ClientAbilitySlot.Basic, 45_000, 1_500, null);

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
        var firstAbility = await first.SubmitAbilityAsync(ClientAbilitySlot.Basic, 45_000, 1_500, null);
        var secondAbility = await second.SubmitAbilityAsync(ClientAbilitySlot.Basic, 45_000, 1_500, null);

        Assert.Equal(firstAbility.PostStateHash, secondAbility.PostStateHash);
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
}
