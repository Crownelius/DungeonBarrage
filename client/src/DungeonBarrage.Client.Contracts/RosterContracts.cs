namespace DungeonBarrage.Client.Contracts;

/// <summary>The fixed launch characters shown on the single-screen character picker.</summary>
/// <remarks>
/// Mirrors <c>db-sim-ffi/src/wire.rs</c>'s <c>WireRoster</c> field for field. Static content,
/// not match state: fetched via <c>db_sim_roster</c>, which needs no live match handle.
/// </remarks>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="Characters">Authority-owned fixed character kits.</param>
public sealed record ClientRosterResponse(
    uint SchemaVersion,
    IReadOnlyList<ClientCharacterDefinition> Characters);

/// <summary>One fixed character kit.</summary>
public sealed record ClientCharacterDefinition(
    string Id,
    string DisplayName,
    string Role,
    ushort MaxHealth,
    ClientMovementClass MovementClass,
    int MovementAllowance,
    ClientAbilityDefinition Shot1,
    ClientAbilityDefinition Shot2OrMelee,
    ClientAbilityDefinition Special);

/// <summary>Default reach class a character's abilities are tuned around.</summary>
public enum ClientRangeTier
{
    /// <summary>1.25 body widths.</summary>
    Melee,

    /// <summary>8 body widths.</summary>
    Tier1,

    /// <summary>16 body widths.</summary>
    Tier2,

    /// <summary>26 body widths.</summary>
    Tier3,
}

/// <summary>Movement allowance class.</summary>
public enum ClientMovementClass
{
    /// <summary>2.5 body widths per turn.</summary>
    Slow,

    /// <summary>4 body widths per turn.</summary>
    Normal,

    /// <summary>8 body widths per turn.</summary>
    Fast,
}

/// <summary>
/// One ability's selection-relevant shape. Deliberately excludes resolution internals
/// (projectile speed/gravity/wind, terrain effects) that mean nothing to a player picking a
/// character — those apply only once a match is already running.
/// </summary>
/// <param name="Id">Stable identifier, e.g. <c>"arzum-lunge"</c>.</param>
/// <param name="DisplayName">Player-facing name.</param>
/// <param name="Slot">Which ability slot it occupies.</param>
/// <param name="DamagePercent">Damage as a percentage of the shared base attack value.</param>
/// <param name="CritDamagePercent">Critical-hit damage percentage.</param>
/// <param name="CritChanceBasisPoints">Crit chance in basis points. Zero when it cannot crit.</param>
/// <param name="StrikesPerTurn">How many times this ability resolves in one turn.</param>
/// <param name="AttackShape">Whether this ability flies or resolves at fixed reach.</param>
/// <param name="Range">
/// Reach, fixed-point, present only for a <see cref="ClientAttackShape.Strike"/> ability. A
/// projectile's effective range depends on the player's own aim and power, so there is no
/// single number worth showing for one.
/// </param>
public sealed record ClientAbilityDefinition(
    string Id,
    string DisplayName,
    ClientAbilitySlot Slot,
    ushort DamagePercent,
    ushort CritDamagePercent,
    ushort CritChanceBasisPoints,
    byte StrikesPerTurn,
    ClientAttackShape AttackShape,
    int? Range);

/// <summary>The two attack shapes an ability can take.</summary>
public enum ClientAttackShape
{
    /// <summary>Flies through the world; range depends on aim and power.</summary>
    Projectile,

    /// <summary>Resolves immediately within a fixed reach.</summary>
    Strike,
}

/// <summary>A passive's name only — the choice itself happens mid-match, not at select time.</summary>
/// <param name="Id">Stable identifier, e.g. <c>"arzum-momentum"</c>.</param>
/// <param name="DisplayName">Player-facing name.</param>
public sealed record ClientPassivePreview(string Id, string DisplayName);
