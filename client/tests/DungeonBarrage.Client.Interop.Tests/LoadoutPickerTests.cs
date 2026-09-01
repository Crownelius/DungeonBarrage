using System.Text;
using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// The select screen's picker writes the clicked item into the create envelope.
/// </summary>
/// <remarks>
/// This is the C4 gate the kit-era character index missed: Confirm must send the equipped
/// items, not a leftover default triangle. Uses the real native catalog so the ids are the
/// same ones Godot displays.
/// </remarks>
public sealed class LoadoutPickerTests
{
    [Fact]
    public async Task Selecting_frostfall_mortar_puts_it_on_the_create_envelope_main_slot()
    {
        var items = RosterCatalog.Get().Items;
        var picker = new LoadoutPicker(items);

        Assert.Equal("ramshot-cannon", picker.Loadout.Main);
        Assert.Equal("recurve-bow", picker.Loadout.Secondary);
        Assert.Equal("trench-spade", picker.Loadout.MeleeTool);

        var frostIndex = IndexOf(items, "frostfall-mortar");
        Assert.Equal(ClientAbilitySlot.Main, items[frostIndex].Slot);

        picker.SelectTile(frostIndex);

        Assert.Equal("frostfall-mortar", picker.Loadout.Main);
        Assert.Equal("recurve-bow", picker.Loadout.Secondary);
        Assert.Equal("trench-spade", picker.Loadout.MeleeTool);
        Assert.Equal(frostIndex, picker.FocusedIndex);
        Assert.Equal(frostIndex, picker.MainIndex);

        var request = LocalMatchEnvelope.HumanVsBot(
            simulationVersion: LocalMatchSession.SimulationVersion,
            contentVersion: LocalMatchSession.ContentVersion,
            seed: 12345,
            matchId: "picker-frostfall",
            mapId: "crow-perch",
            loadout: picker.Loadout);

        Assert.Equal("frostfall-mortar", request.Match.Players[0].Loadout.Main);
        Assert.Equal("frostfall-mortar", request.Match.Players[1].Loadout.Main);

        var jsonBytes = JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options);
        var json = Encoding.UTF8.GetString(jsonBytes);
        Assert.Contains("\"main\":\"frostfall-mortar\"", json, StringComparison.Ordinal);
        Assert.DoesNotContain("characterId", json, StringComparison.Ordinal);
        Assert.DoesNotContain("ramshot-cannon", json, StringComparison.Ordinal);

        using var session = LocalMatchSession.Create(jsonBytes);
        var created = JsonSerializer.Deserialize<ClientCreateResponse>(
            session.CreateResponse.Span, ClientEnvelope.Options);
        Assert.NotNull(created);
        Assert.True(created.Created);
        Assert.NotNull(created.Snapshot);
        Assert.Equal("frostfall-mortar", created.Snapshot.Players[0].Loadout.Main);
        Assert.Equal("frostfall-mortar", created.Snapshot.Players[1].Loadout.Main);
        Assert.Equal("recurve-bow", created.Snapshot.Players[0].Loadout.Secondary);
        Assert.Equal("trench-spade", created.Snapshot.Players[0].Loadout.MeleeTool);

        var move = ClientMatchCommand.Move(
            commandId: "picker-frostfall-move",
            playerId: created.Snapshot.ActivePlayerId ?? "a-local-player",
            expectedTurnNumber: created.Snapshot.TurnNumber,
            expectedSnapshotGeneration: created.Snapshot.SnapshotGeneration,
            dx: 1024);
        var moveTransition = JsonSerializer.Deserialize<ClientMatchTransition>(
            await session.ApplyAsync(JsonSerializer.SerializeToUtf8Bytes(move, ClientEnvelope.Options)),
            ClientEnvelope.Options);
        Assert.NotNull(moveTransition);
        Assert.True(
            moveTransition.Disposition == ClientTransitionDisposition.Accepted,
            $"frostfall crow-perch move was {moveTransition.Disposition}: {moveTransition.RejectionReason}");

        var ability = ClientMatchCommand.Ability(
            commandId: "picker-frostfall-ability",
            playerId: moveTransition.PostSnapshot.ActivePlayerId ?? "a-local-player",
            expectedTurnNumber: moveTransition.PostSnapshot.TurnNumber,
            expectedSnapshotGeneration: moveTransition.PostSnapshot.SnapshotGeneration,
            slot: ClientAbilitySlot.Main,
            angleMillidegrees: 45_000,
            powerBasisPoints: 1_500,
            targetPlayerId: null,
            secondaryTargetPlayerId: null);
        var abilityTransition = JsonSerializer.Deserialize<ClientMatchTransition>(
            await session.ApplyAsync(JsonSerializer.SerializeToUtf8Bytes(ability, ClientEnvelope.Options)),
            ClientEnvelope.Options);
        Assert.NotNull(abilityTransition);
        Assert.True(
            abilityTransition.Disposition == ClientTransitionDisposition.Accepted,
            $"frostfall crow-perch main was {abilityTransition.Disposition}: {abilityTransition.RejectionReason}");
    }

    [Fact]
    public void Selecting_a_secondary_item_does_not_replace_main()
    {
        var items = RosterCatalog.Get().Items;
        var picker = new LoadoutPicker(items);
        var longsword = IndexOf(items, "longsword");
        Assert.Equal(ClientAbilitySlot.Secondary, items[longsword].Slot);

        picker.SelectTile(longsword);

        Assert.Equal("ramshot-cannon", picker.Loadout.Main);
        Assert.Equal("longsword", picker.Loadout.Secondary);
        Assert.Equal("trench-spade", picker.Loadout.MeleeTool);
    }

    [Fact]
    public async Task Every_catalog_item_lands_on_its_slot_and_fires_with_a_null_target()
    {
        var items = RosterCatalog.Get().Items;
        Assert.True(items.Count >= 9, "the native catalog must still publish the nine launch items");

        for (var i = 0; i < items.Count; i++)
        {
            var item = items[i];
            var picker = new LoadoutPicker(items);
            picker.SelectTile(i);

            switch (item.Slot)
            {
                case ClientAbilitySlot.Main:
                    Assert.Equal(item.Id, picker.Loadout.Main);
                    Assert.Equal("recurve-bow", picker.Loadout.Secondary);
                    Assert.Equal("trench-spade", picker.Loadout.MeleeTool);
                    break;
                case ClientAbilitySlot.Secondary:
                    Assert.Equal("ramshot-cannon", picker.Loadout.Main);
                    Assert.Equal(item.Id, picker.Loadout.Secondary);
                    Assert.Equal("trench-spade", picker.Loadout.MeleeTool);
                    break;
                case ClientAbilitySlot.MeleeTool:
                    Assert.Equal("ramshot-cannon", picker.Loadout.Main);
                    Assert.Equal("recurve-bow", picker.Loadout.Secondary);
                    Assert.Equal(item.Id, picker.Loadout.MeleeTool);
                    break;
                default:
                    throw new InvalidOperationException($"Unexpected slot {item.Slot} for '{item.Id}'.");
            }

            var request = LocalMatchEnvelope.HumanVsBot(
                simulationVersion: LocalMatchSession.SimulationVersion,
                contentVersion: LocalMatchSession.ContentVersion,
                seed: 12345,
                matchId: $"picker-{item.Id}",
                mapId: "crow-perch",
                loadout: picker.Loadout);

            Assert.Equal(picker.Loadout.Main, request.Match.Players[0].Loadout.Main);
            Assert.Equal(picker.Loadout.Secondary, request.Match.Players[0].Loadout.Secondary);
            Assert.Equal(picker.Loadout.MeleeTool, request.Match.Players[0].Loadout.MeleeTool);
            Assert.DoesNotContain(
                "characterId",
                Encoding.UTF8.GetString(JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options)),
                StringComparison.Ordinal);

            var jsonBytes = JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options);
            using var session = LocalMatchSession.Create(jsonBytes);
            var created = JsonSerializer.Deserialize<ClientCreateResponse>(
                session.CreateResponse.Span, ClientEnvelope.Options);
            Assert.NotNull(created);
            Assert.True(created.Created, $"{item.Id} must create");
            Assert.NotNull(created.Snapshot);
            Assert.Equal(picker.Loadout.Main, created.Snapshot.Players[0].Loadout.Main);
            Assert.Equal(picker.Loadout.Secondary, created.Snapshot.Players[0].Loadout.Secondary);
            Assert.Equal(picker.Loadout.MeleeTool, created.Snapshot.Players[0].Loadout.MeleeTool);

            var ability = ClientMatchCommand.Ability(
                commandId: $"aim-{item.Id}",
                playerId: created.Snapshot.ActivePlayerId ?? "a-local-player",
                expectedTurnNumber: created.Snapshot.TurnNumber,
                expectedSnapshotGeneration: created.Snapshot.SnapshotGeneration,
                slot: item.Slot,
                angleMillidegrees: 45_000,
                powerBasisPoints: 1_500,
                targetPlayerId: null,
                secondaryTargetPlayerId: null);
            var transition = JsonSerializer.Deserialize<ClientMatchTransition>(
                await session.ApplyAsync(JsonSerializer.SerializeToUtf8Bytes(ability, ClientEnvelope.Options)),
                ClientEnvelope.Options);
            Assert.NotNull(transition);
            Assert.True(
                transition.Disposition == ClientTransitionDisposition.Accepted,
                $"{item.Id} with targetPlayerId=null was {transition.Disposition}: {transition.RejectionReason}");
        }
    }

    private static int IndexOf(IReadOnlyList<ClientItemDefinition> items, string id)
    {
        for (var i = 0; i < items.Count; i++)
        {
            if (items[i].Id == id)
            {
                return i;
            }
        }

        throw new InvalidOperationException($"The native catalog does not contain '{id}'.");
    }
}
