using System.Text;
using System.Text.Json.Nodes;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using Xunit;

namespace DungeonBarrage.Client.Interop.Tests;

/// <summary>
/// The committed presentation manifest must track the loaded native <c>CONTENT_VERSION</c>.
/// Godot Confirm is the only production caller; this is the Godot-free gate so a content bump
/// that forgets the JSON fails in <c>dotnet test</c>, not only in an exported C6 smoke.
/// </summary>
public sealed class PresentationManifestTests
{
    [Fact]
    public void The_committed_manifest_matches_the_loaded_native_content_version()
    {
        var bytes = File.ReadAllBytes(ManifestPath());
        var request = SampleRequest(LocalMatchSession.ContentVersion);

        var manifest = PresentationManifest.Validate(bytes, request, LocalMatchSession.ContentVersion);

        Assert.Equal(1u, manifest.SchemaVersion);
        Assert.Equal(LocalMatchSession.ContentVersion, manifest.ContentVersion);
        var hasCrow = false;
        for (var i = 0; i < manifest.Characters.Count; i++)
        {
            if (manifest.Characters[i].CharacterId == "crow")
            {
                hasCrow = true;
                break;
            }
        }

        Assert.True(hasCrow, "the presentation manifest must still list the crow fighter");
    }

    [Fact]
    public void A_stale_manifest_content_version_is_refused_before_create()
    {
        var node = JsonNode.Parse(File.ReadAllText(ManifestPath()))
            ?? throw new InvalidOperationException("The committed presentation manifest is not JSON.");
        var native = LocalMatchSession.ContentVersion;
        Assert.NotEqual(0u, native);
        node["contentVersion"] = native - 1;

        var stale = Encoding.UTF8.GetBytes(node.ToJsonString());
        var request = SampleRequest(native);

        var error = Assert.Throws<InvalidDataException>(
            () => PresentationManifest.Validate(stale, request, native));

        Assert.Contains("must match", error.Message, StringComparison.Ordinal);
        Assert.DoesNotContain("crow fighter", error.Message, StringComparison.Ordinal);
    }

    private static string ManifestPath()
    {
        var path = Path.Combine(AppContext.BaseDirectory, "presentation-manifest-v1.json");
        if (!File.Exists(path))
        {
            throw new FileNotFoundException(
                "The test must copy the committed presentation-manifest-v1.json next to the assembly.",
                path);
        }

        return path;
    }

    private static ClientCreateRequest SampleRequest(uint contentVersion) =>
        LocalMatchEnvelope.HumanVsBot(
            simulationVersion: LocalMatchSession.SimulationVersion,
            contentVersion: contentVersion,
            seed: 12345,
            matchId: "manifest-gate",
            mapId: "crow-perch",
            humanCharacterId: LocalMatchEnvelope.LaunchDefaultCharacterId);
}
