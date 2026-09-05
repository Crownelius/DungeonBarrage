using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Directional flow of the match wind.
/// </summary>
public enum WindDirection
{
    /// <summary>No discernible wind movement.</summary>
    Calm,

    /// <summary>Wind blowing towards the left (negative horizontal acceleration).</summary>
    BlowingLeft,

    /// <summary>Wind blowing towards the right (positive horizontal acceleration).</summary>
    BlowingRight,
}

/// <summary>
/// Ballistic sensitivity tier of a weapon to ambient wind acceleration.
/// </summary>
public enum WindSensitivityTier
{
    /// <summary>Completely immune to wind (e.g. service-pistol, line-repeater).</summary>
    Immune,

    /// <summary>Heavier projectile with reduced wind drift (e.g. mole-drill, returning-boomerang).</summary>
    Resistant,

    /// <summary>Standard ballistic projectile subject to normal wind drift (e.g. ramshot-cannon, frostfall-mortar).</summary>
    Standard,

    /// <summary>Lightweight projectile highly affected by wind drift (e.g. recurve-bow).</summary>
    High,
}

/// <summary>
/// Pure C# presentation model for the match wind anemometer HUD gauge.
/// </summary>
/// <param name="WindPerTick">Authoritative integer wind acceleration per simulation tick.</param>
/// <param name="Direction">Directional flow classification.</param>
/// <param name="NormalizedIntensity">Clamped intensity ratio from 0.0 (calm) to 1.0 (maximum typical wind).</param>
/// <param name="Sensitivity">Active weapon sensitivity tier.</param>
/// <param name="FormattedText">Human-readable wind velocity label.</param>
/// <param name="SensitivityBadge">Short badge text describing the weapon sensitivity.</param>
public sealed record WindDisplayModel(
    int WindPerTick,
    WindDirection Direction,
    float NormalizedIntensity,
    WindSensitivityTier Sensitivity,
    string FormattedText,
    string SensitivityBadge)
{
    private const float TypicalMaxWind = 80f;

    /// <summary>
    /// Builds a wind display model from the authoritative wind per tick and active weapon identifier.
    /// </summary>
    /// <param name="windPerTick">Authoritative wind acceleration per tick.</param>
    /// <param name="activeWeaponId">Equipped weapon identifier, or null.</param>
    /// <returns>A fully resolved wind display presentation model.</returns>
    public static WindDisplayModel Create(int windPerTick, string? activeWeaponId)
    {
        var direction = windPerTick switch
        {
            < 0 => WindDirection.BlowingLeft,
            > 0 => WindDirection.BlowingRight,
            _ => WindDirection.Calm,
        };

        var absSpeed = MathF.Abs(windPerTick);
        var normalizedIntensity = Math.Clamp(absSpeed / TypicalMaxWind, 0f, 1f);
        var sensitivity = ResolveSensitivity(activeWeaponId);

        var arrow = direction switch
        {
            WindDirection.BlowingLeft => "«",
            WindDirection.BlowingRight => "»",
            _ => "·",
        };

        var formattedText = direction switch
        {
            WindDirection.Calm => "CALM (0)",
            WindDirection.BlowingLeft => $"{arrow} {absSpeed:F0} WEST",
            WindDirection.BlowingRight => $"EAST {absSpeed:F0} {arrow}",
            _ => "CALM (0)",
        };

        var badge = sensitivity switch
        {
            WindSensitivityTier.Immune => "IMMUNE",
            WindSensitivityTier.Resistant => "HEAVY",
            WindSensitivityTier.Standard => "STD",
            WindSensitivityTier.High => "LIGHT",
            _ => "STD",
        };

        return new WindDisplayModel(
            windPerTick,
            direction,
            normalizedIntensity,
            sensitivity,
            formattedText,
            badge);
    }

    private static WindSensitivityTier ResolveSensitivity(string? weaponId)
    {
        if (string.IsNullOrWhiteSpace(weaponId))
        {
            return WindSensitivityTier.Standard;
        }

        var lower = weaponId.ToLowerInvariant();
        if (lower.Contains("pistol") || lower.Contains("repeater"))
        {
            return WindSensitivityTier.Immune;
        }

        if (lower.Contains("drill") || lower.Contains("boomerang") || lower.Contains("sprayer"))
        {
            return WindSensitivityTier.Resistant;
        }

        if (lower.Contains("bow"))
        {
            return WindSensitivityTier.High;
        }

        return WindSensitivityTier.Standard;
    }
}

/// <summary>
/// Pure C# presentation model for an in-match floating character status plate.
/// </summary>
/// <param name="PlayerId">Player identifier.</param>
/// <param name="Health">Current authoritative health points.</param>
/// <param name="MaxHealth">Maximum authoritative health points.</param>
/// <param name="HealthFraction">Clamped health fraction from 0.0 to 1.0.</param>
/// <param name="IsLowHealth">True when current health is at or below 25% of maximum.</param>
/// <param name="TrinketCharge">Current charges accumulated on the trinket.</param>
/// <param name="TrinketMaxCharge">Required charge count to fire the trinket (typically 2).</param>
/// <param name="TrinketReady">True when trinket charge has met or exceeded the threshold.</param>
/// <param name="CueLabel">Active combat presentation cue label, if any.</param>
public sealed record PlayerStatusPlateModel(
    string PlayerId,
    int Health,
    int MaxHealth,
    float HealthFraction,
    bool IsLowHealth,
    int TrinketCharge,
    int TrinketMaxCharge,
    bool TrinketReady,
    string? CueLabel)
{
    /// <summary>Number of compact pips used to present the authoritative 10,000-point charge.</summary>
    public const int DefaultTrinketMaxCharge = 2;

    /// <summary>Authoritative full-charge value published by the simulation.</summary>
    public const int AuthoritativeTrinketMaxCharge = 10_000;

    /// <summary>
    /// Builds a status plate presentation model from a player snapshot and optional combat cue.
    /// </summary>
    /// <param name="player">Authoritative player snapshot.</param>
    /// <param name="cue">Active presentation cue, or null.</param>
    /// <returns>A fully resolved player status plate presentation model.</returns>
    public static PlayerStatusPlateModel Create(ClientPlayerSnapshot player, ActorPresentationCue? cue = null)
    {
        ArgumentNullException.ThrowIfNull(player);

        var maxHp = Math.Max(1, (int)player.MaxHealth);
        var curHp = Math.Clamp((int)player.Health, 0, maxHp);
        var fraction = (float)curHp / maxHp;
        var isLow = fraction <= 0.25f && curHp > 0;

        var authoritativeCharge = Math.Clamp(
            (int)player.TrinketCharge,
            0,
            AuthoritativeTrinketMaxCharge);
        var charge = authoritativeCharge >= AuthoritativeTrinketMaxCharge
            ? DefaultTrinketMaxCharge
            : authoritativeCharge >= AuthoritativeTrinketMaxCharge / 2
                ? 1
                : 0;
        var ready = authoritativeCharge >= AuthoritativeTrinketMaxCharge;

        var cueText = cue?.Kind switch
        {
            ActorPresentationCueKind.Fire => "FIRE",
            ActorPresentationCueKind.Hit => "HIT",
            ActorPresentationCueKind.Defeat => "DOWN",
            _ => null,
        };

        return new PlayerStatusPlateModel(
            player.PlayerId,
            curHp,
            maxHp,
            fraction,
            isLow,
            charge,
            DefaultTrinketMaxCharge,
            ready,
            cueText);
    }
}
