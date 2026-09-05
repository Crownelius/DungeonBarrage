using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;

namespace DungeonBarrage.Client.Match;

internal sealed record SnapshotFrame(ClientMatchSnapshot Snapshot, TerrainRead Terrain);

internal sealed record MatchBootstrapResult(LocalMatchSession Session, SnapshotFrame Frame);
