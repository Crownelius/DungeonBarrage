using System.Text.Json.Serialization;

namespace DungeonBarrage.Client.Contracts;

/// <summary>Material values emitted for authoritative destructible blocks.</summary>
public enum ClientMaterial
{
    /// <summary>No solid material.</summary>
    Empty,

    /// <summary>Ordinary soil.</summary>
    Soil,

    /// <summary>Wood.</summary>
    Wood,

    /// <summary>Reinforced stone.</summary>
    ReinforcedStone,
}

/// <summary>The axis along which a block erodes.</summary>
public enum ClientErosionAxis
{
    /// <summary>Columns erode independently.</summary>
    Columns,

    /// <summary>Rows erode independently.</summary>
    Rows,
}

/// <summary>Closed authoritative status vocabulary.</summary>
public enum ClientStatusKind
{
    /// <summary>Knockback.</summary>
    Knockback,

    /// <summary>Chill.</summary>
    Chill,

    /// <summary>Cluster.</summary>
    Cluster,

    /// <summary>Embers.</summary>
    Embers,

    /// <summary>Tunnel.</summary>
    Tunnel,

    /// <summary>Return.</summary>
    Return,

    /// <summary>Recoil.</summary>
    Recoil,

    /// <summary>Self-inflicted damage.</summary>
    SelfDamage,

    /// <summary>Teleport.</summary>
    Teleport,

    /// <summary>Pull.</summary>
    Pull,

    /// <summary>Push.</summary>
    Push,

    /// <summary>Wall impact.</summary>
    WallImpact,

    /// <summary>Lockdown.</summary>
    Lockdown,

    /// <summary>Turret creation.</summary>
    SpawnTurret,

    /// <summary>Healing.</summary>
    Heal,

    /// <summary>Health transfer.</summary>
    HealthTransfer,

    /// <summary>Multiple strikes.</summary>
    MultiStrike,

    /// <summary>Guaranteed critical strike.</summary>
    GuaranteeCrit,

    /// <summary>Embedded projectile.</summary>
    EmbedProjectile,

    /// <summary>Chained detonation.</summary>
    ChainDetonate,

    /// <summary>Relocation.</summary>
    Relocate,

    /// <summary>Obscuring effect.</summary>
    Obscure,
}

/// <summary>Closed persistent-object vocabulary.</summary>
public enum ClientObjectKind
{
    /// <summary>A turret.</summary>
    Turret,

    /// <summary>An embedded knife.</summary>
    EmbeddedKnife,

    /// <summary>A gas cloud.</summary>
    GasCloud,
}

/// <summary>An authoritative fixed-point position.</summary>
/// <param name="X">Horizontal coordinate.</param>
/// <param name="Y">Vertical coordinate.</param>
public sealed record ClientPosition(int X, int Y);

/// <summary>One authoritative destructible-block snapshot.</summary>
/// <param name="Id">Stable block identifier.</param>
/// <param name="OriginCellX">Left cell coordinate.</param>
/// <param name="OriginCellY">Top cell coordinate.</param>
/// <param name="WidthCells">Width in cells.</param>
/// <param name="HeightCells">Height in cells.</param>
/// <param name="Material">Authoritative material.</param>
/// <param name="Health">Current block health.</param>
/// <param name="MaxHealth">Maximum block health.</param>
/// <param name="ErosionAxis">Authoritative erosion axis.</param>
public sealed record ClientBlockSnapshot(
    uint Id,
    int OriginCellX,
    int OriginCellY,
    ushort WidthCells,
    ushort HeightCells,
    ClientMaterial Material,
    ushort Health,
    ushort MaxHealth,
    ClientErosionAxis ErosionAxis);

/// <summary>One authoritative player status.</summary>
/// <param name="Kind">Status kind.</param>
/// <param name="Magnitude">Status magnitude.</param>
/// <param name="TurnsRemaining">Affected-player turns remaining.</param>
public sealed record ClientStatusSnapshot(
    ClientStatusKind Kind,
    int Magnitude,
    byte TurnsRemaining);

/// <summary>One player in an authoritative snapshot.</summary>
/// <param name="PlayerId">Match-local player identifier.</param>
/// <param name="Team">Team number.</param>
/// <param name="Health">Current health.</param>
/// <param name="IsEliminated">Whether the player is eliminated.</param>
/// <param name="MaxHealth">Maximum health.</param>
/// <param name="Position">Authoritative fixed-point ground pivot.</param>
/// <param name="CollisionCenter">Authoritative fixed-point center of the visible collision body.</param>
/// <param name="CollisionRadius">Authoritative collision-body radius in fixed-point units.</param>
/// <param name="Loadout">Equipped item identifiers.</param>
/// <param name="Ammo">Remaining ammunition per slot.</param>
/// <param name="TrinketCharge">Charge toward the equipped crown or anklet special.</param>
/// <param name="Statuses">Current statuses.</param>
/// <param name="Appearance">Cosmetic appearance.</param>
public sealed record ClientPlayerSnapshot(
    string PlayerId,
    byte Team,
    ushort Health,
    bool IsEliminated,
    ushort MaxHealth,
    ClientPosition Position,
    ClientPosition CollisionCenter,
    int CollisionRadius,
    ClientLoadout Loadout,
    IReadOnlyList<ClientAmmoCounter> Ammo,
    ushort TrinketCharge,
    IReadOnlyList<ClientStatusSnapshot> Statuses,
    ClientAppearance Appearance);

/// <summary>One authoritative persistent object.</summary>
/// <param name="Sequence">Stable match-local object sequence.</param>
/// <param name="OwnerId">Owning player identifier.</param>
/// <param name="Kind">Object kind.</param>
/// <param name="Position">Authoritative fixed-point position.</param>
/// <param name="Health">Current health.</param>
/// <param name="TurnsRemaining">Remaining lifetime.</param>
public sealed record ClientPersistentObjectSnapshot(
    uint Sequence,
    string OwnerId,
    ClientObjectKind Kind,
    ClientPosition Position,
    ushort Health,
    byte TurnsRemaining);

/// <summary>Closed authoritative match outcome.</summary>
[JsonPolymorphic(
    TypeDiscriminatorPropertyName = "kind",
    IgnoreUnrecognizedTypeDiscriminators = false,
    UnknownDerivedTypeHandling = JsonUnknownDerivedTypeHandling.FailSerialization)]
[JsonDerivedType(typeof(ClientInProgressOutcome), "inProgress")]
[JsonDerivedType(typeof(ClientVictoryOutcome), "victory")]
[JsonDerivedType(typeof(ClientDrawOutcome), "draw")]
public abstract record ClientMatchOutcome;

/// <summary>The match is still in progress.</summary>
public sealed record ClientInProgressOutcome : ClientMatchOutcome;

/// <summary>The match ended in victory.</summary>
/// <param name="Team">Winning team.</param>
public sealed record ClientVictoryOutcome(byte Team) : ClientMatchOutcome;

/// <summary>The match ended in a draw.</summary>
public sealed record ClientDrawOutcome : ClientMatchOutcome;

/// <summary>One complete authoritative match snapshot envelope.</summary>
/// <param name="SchemaVersion">Client schema version.</param>
/// <param name="AbiVersion">Native ABI version.</param>
/// <param name="SimulationVersion">Authoritative simulation version.</param>
/// <param name="ContentVersion">Authoritative content version.</param>
/// <param name="PositionScale">Fixed-point position units per terrain cell.</param>
/// <param name="FixedTickRate">Authoritative simulation ticks per second.</param>
/// <param name="MatchId">Match identifier.</param>
/// <param name="MapId">Map identifier.</param>
/// <param name="SnapshotGeneration">Session snapshot generation.</param>
/// <param name="Tick">Authoritative simulation tick.</param>
/// <param name="TurnNumber">Current turn number.</param>
/// <param name="Phase">Stable authoritative phase.</param>
/// <param name="ActivePlayerId">Active player, or null after terminal completion.</param>
/// <param name="CurrentAndUpcomingPlayerIds">Deterministic current and upcoming turn order.</param>
/// <param name="WindPerTick">Authoritative wind delta per tick.</param>
/// <param name="MovementRemaining">Remaining movement allowance.</param>
/// <param name="HasAttackedThisTurn">Whether the active player has attacked.</param>
/// <param name="InputOpensAt">Local or server timestamp at which input opens, when decorated.</param>
/// <param name="DeadlineAt">Local or server planning deadline, when decorated.</param>
/// <param name="Outcome">Authoritative match outcome.</param>
/// <param name="TerrainWidth">Terrain width in cells.</param>
/// <param name="TerrainHeight">Terrain height in cells.</param>
/// <param name="TerrainGeneration">Terrain byte generation.</param>
/// <param name="Blocks">Blocks sorted by identifier.</param>
/// <param name="Players">Players sorted by identifier.</param>
/// <param name="PersistentObjects">Persistent objects sorted by sequence.</param>
/// <param name="StateHash">Exact authoritative state hash.</param>
public sealed record ClientMatchSnapshot(
    uint SchemaVersion,
    uint AbiVersion,
    uint SimulationVersion,
    uint ContentVersion,
    int PositionScale,
    uint FixedTickRate,
    string MatchId,
    string MapId,
    ulong SnapshotGeneration,
    ulong Tick,
    uint TurnNumber,
    ClientMatchPhase Phase,
    string? ActivePlayerId,
    IReadOnlyList<string> CurrentAndUpcomingPlayerIds,
    int WindPerTick,
    int MovementRemaining,
    bool HasAttackedThisTurn,
    ulong? InputOpensAt,
    ulong? DeadlineAt,
    ClientMatchOutcome Outcome,
    uint TerrainWidth,
    uint TerrainHeight,
    uint TerrainGeneration,
    IReadOnlyList<ClientBlockSnapshot> Blocks,
    IReadOnlyList<ClientPlayerSnapshot> Players,
    IReadOnlyList<ClientPersistentObjectSnapshot> PersistentObjects,
    string StateHash);

/// <summary>A failed match-creation diagnostic.</summary>
/// <param name="Code">Stable diagnostic code.</param>
/// <param name="Message">Human-readable diagnostic message.</param>
public sealed record ClientCreateDiagnostic(string Code, string Message);

/// <summary>The complete response to a native match-creation request.</summary>
/// <param name="SchemaVersion">Client schema version.</param>
/// <param name="Created">Whether a live match was created.</param>
/// <param name="Diagnostic">Failure diagnostic, or null on success.</param>
/// <param name="Snapshot">Initial snapshot, or null on failure.</param>
public sealed record ClientCreateResponse(
    uint SchemaVersion,
    bool Created,
    ClientCreateDiagnostic? Diagnostic,
    ClientMatchSnapshot? Snapshot);
