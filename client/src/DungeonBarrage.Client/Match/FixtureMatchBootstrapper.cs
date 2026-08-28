using System.Reflection;
using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using DungeonBarrage.Client.Settings;

namespace DungeonBarrage.Client.Match;

internal static class FixtureMatchBootstrapper
{
    private const string CreateRequestResource =
        "DungeonBarrage.Client.Fixtures.HorizontalTestDuelCreateRequest.json";

    internal static MatchBootstrapResult Start()
    {
        var requestBytes = ReadEmbedded(CreateRequestResource);
        var request = JsonSerializer.Deserialize<ClientCreateRequest>(requestBytes, ClientEnvelope.Options)
            ?? throw new InvalidDataException("The fixture creation request decoded to null.");

        _ = PresentationManifest.LoadAndValidate(request, LocalMatchSession.ContentVersion);

        LocalMatchSession? session = null;
        try
        {
            session = LocalMatchSession.Create(requestBytes);
            var response = JsonSerializer.Deserialize<ClientCreateResponse>(
                session.CreateResponse.Span,
                ClientEnvelope.Options)
                ?? throw new InvalidDataException("The native creation response decoded to null.");

            if (!response.Created || response.Snapshot is null || response.Diagnostic is not null)
            {
                throw new InvalidDataException(
                    response.Diagnostic is null
                        ? "Native creation returned an inconsistent success envelope."
                        : $"Native creation was rejected: {response.Diagnostic.Code}: " +
                          response.Diagnostic.Message);
            }

            var snapshot = response.Snapshot;
            if (snapshot.PositionScale == 0 || snapshot.FixedTickRate == 0)
            {
                throw new InvalidDataException(
                    "The authoritative snapshot supplied a zero coordinate or tick scale.");
            }

            if (snapshot.SimulationVersion != request.SimulationVersion ||
                snapshot.ContentVersion != request.ContentVersion)
            {
                throw new InvalidDataException(
                    "The native snapshot versions do not match the validated creation request.");
            }

            var terrain = session.TerrainAsync(ulong.MaxValue).GetAwaiter().GetResult();
            ValidateTerrain(snapshot, terrain);

            var result = new MatchBootstrapResult(session, new SnapshotFrame(snapshot, terrain));
            session = null;
            return result;
        }
        finally
        {
            session?.Dispose();
        }
    }

    private static byte[] ReadEmbedded(string name)
    {
        using var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(name)
            ?? throw new FileNotFoundException($"Embedded fixture resource is missing: {name}");
        using var buffer = new MemoryStream();
        stream.CopyTo(buffer);
        return buffer.ToArray();
    }

    private static void ValidateTerrain(ClientMatchSnapshot snapshot, TerrainRead terrain)
    {
        if (terrain.Width != snapshot.TerrainWidth ||
            terrain.Height != snapshot.TerrainHeight ||
            terrain.Generation != snapshot.TerrainGeneration)
        {
            throw new InvalidDataException(
                "The separately read terrain does not belong to the creation snapshot.");
        }

        var expectedLength = checked((int)(terrain.Width * terrain.Height));
        if (terrain.Cells.Length != expectedLength)
        {
            throw new InvalidDataException(
                $"Terrain returned {terrain.Cells.Length} cells; expected {expectedLength}.");
        }
    }
}
