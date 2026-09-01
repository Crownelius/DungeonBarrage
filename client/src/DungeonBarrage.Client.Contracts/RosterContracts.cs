namespace DungeonBarrage.Client.Contracts;

/// <summary>The one fighter plus the item catalog, for the loadout picker.</summary>
/// <remarks>
/// Mirrors <c>db-sim-ffi/src/wire.rs</c>'s <c>WireRoster</c> field for field. Static content,
/// not match state: fetched via <c>db_sim_roster</c>, which needs no live match handle.
/// </remarks>
/// <param name="SchemaVersion">Client-contract schema version.</param>
/// <param name="Fighter">The one crow fighter.</param>
/// <param name="Items">Equippable items. Each occupies exactly one slot.</param>
public sealed record ClientRosterResponse(
    uint SchemaVersion,
    ClientFighterDefinition Fighter,
    IReadOnlyList<ClientItemDefinition> Items);

/// <summary>The one playable fighter.</summary>
/// <param name="Id">Always <c>crow</c>.</param>
/// <param name="DisplayName">Player-facing name.</param>
/// <param name="MaxHealth">Starting and maximum health.</param>
/// <param name="RangeTier">Default reach class.</param>
/// <param name="MovementClass">Movement allowance class.</param>
public sealed record ClientFighterDefinition(
    string Id,
    string DisplayName,
    ushort MaxHealth,
    ClientRangeTier RangeTier,
    ClientMovementClass MovementClass);

/// <summary>One equippable item.</summary>
/// <param name="Id">Stable identifier, e.g. <c>"ramshot-cannon"</c>.</param>
/// <param name="DisplayName">Player-facing name.</param>
/// <param name="Slot">Which loadout slot it occupies.</param>
/// <param name="AmmoPolicy">Finite or unlimited.</param>
/// <param name="StartingAmmo">Starting charges.</param>
/// <param name="Ability">Attack used when the item is fired.</param>
public sealed record ClientItemDefinition(
    string Id,
    string DisplayName,
    ClientAbilitySlot Slot,
    ClientAmmoPolicy AmmoPolicy,
    ushort StartingAmmo,
    ClientAbilityDefinition Ability);

/// <summary>Legacy character-shaped view. Unused by the loadout picker.</summary>
public sealed record ClientCharacterDefinition(
    string Id,
    string DisplayName,
    ushort MaxHealth,
    ClientRangeTier RangeTier,
    ClientMovementClass MovementClass,
    ClientAbilityDefinition Basic,
    ClientAbilityDefinition? BasicAlt,
    ClientAbilityDefinition Special,
    IReadOnlyList<ClientPassivePreview> Passives);

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
