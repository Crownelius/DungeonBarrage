using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Resolved sprite animation frame specification.
/// </summary>
/// <param name="SheetKey">The spritesheet identifier (e.g. "crow_ramshot_cannon", "crow_damage").</param>
/// <param name="Row">0-indexed row on the spritesheet.</param>
/// <param name="Col">0-indexed column frame on the spritesheet.</param>
public sealed record CharacterAnimationFrame(
    string SheetKey,
    int Row,
    int Col);

/// <summary>
/// Pure C# animation frame resolver mapping character state, cues, and motion into spritesheet frames.
/// </summary>
public static class CharacterAnimationFrameResolver
{
    /// <summary>
    /// Resolves the active spritesheet key, row, and column for a character.
    /// </summary>
    /// <param name="model">The character presentation model with weapon and geometry bindings.</param>
    /// <param name="isEliminated">Whether the character is authoritative dead.</param>
    /// <param name="cue">Transient presentation cue (Hit, Fire, Defeat), if any.</param>
    /// <param name="visualTimeMsec">Clock time in milliseconds for loop progression.</param>
    /// <param name="isAiming">Whether the player is actively in aiming stance.</param>
    /// <param name="isAirborne">Whether the character is hopping or airborne.</param>
    /// <param name="isMoving">Whether the character is walking horizontally.</param>
    /// <param name="aimAngleRadians">Current aim elevation angle, if aiming.</param>
    /// <returns>A fully resolved <see cref="CharacterAnimationFrame"/>.</returns>
    public static CharacterAnimationFrame Resolve(
        CharacterPresentationModel model,
        bool isEliminated,
        ActorPresentationCue? cue,
        ulong visualTimeMsec,
        bool isAiming,
        bool isAirborne,
        bool isMoving,
        float? aimAngleRadians = null)
    {
        ArgumentNullException.ThrowIfNull(model);

        // Priority 1: Defeat / Knockout prone
        if (isEliminated || cue is { Kind: ActorPresentationCueKind.Defeat })
        {
            var defeatCol = cue is { Kind: ActorPresentationCueKind.Defeat } defeatCue
                ? Math.Clamp((int)(defeatCue.Age01 * 5f), 0, 4)
                : 2; // Frame 2 is the prone knockout with stars
            return new CharacterAnimationFrame("crow_damage", 2, defeatCol);
        }

        // Priority 2: Taking damage / hit reaction
        if (cue is { Kind: ActorPresentationCueKind.Hit } hitCue)
        {
            var isHeavy = hitCue.Value.HasValue && hitCue.Value.Value > 15;
            var hitRow = isHeavy ? 1 : 0; // Row 1 heavy stagger & feathers; Row 0 hit sparks & wince
            var hitCol = Math.Clamp((int)(hitCue.Age01 * 5f), 0, 4);
            return new CharacterAnimationFrame("crow_damage", hitRow, hitCol);
        }

        // Priority 3: Healing potion
        if (model.SpriteSheetKey.Contains("potion", StringComparison.OrdinalIgnoreCase))
        {
            var potionCol = (int)((visualTimeMsec / 150uL) % 5uL);
            return new CharacterAnimationFrame("crow_potion", 1, potionCol);
        }

        // Priority 4: Airborne / Flight / Hopping
        if (isAirborne)
        {
            var flightCol = (int)((visualTimeMsec / 120uL) % 5uL);
            return new CharacterAnimationFrame("crow_flight", 1, flightCol);
        }

        // Priority 5: Firing / Strike Cue
        if (cue is { Kind: ActorPresentationCueKind.Fire } fireCue)
        {
            return ResolveFireFrame(model.SpriteSheetKey, fireCue.Age01);
        }

        // Priority 6: Aiming stance
        if (isAiming || aimAngleRadians.HasValue)
        {
            if (string.Equals(model.SpriteSheetKey, "crow_ramshot_cannon", StringComparison.Ordinal))
            {
                // The remaining cells on this row are cannon blast/ammunition art. They are
                // projectile sources, never valid replacements for the crow while aiming.
                return new CharacterAnimationFrame(model.SpriteSheetKey, 2, 0);
            }

            var aimCol = aimAngleRadians.HasValue
                ? Math.Clamp((int)(Math.Clamp((aimAngleRadians.Value + 0.6f) / 1.8f, 0f, 1f) * 5f), 0, 4)
                : (int)((visualTimeMsec / 200uL) % 2uL);

            return new CharacterAnimationFrame(model.SpriteSheetKey, 2, aimCol);
        }

        // Priority 7: Ground Movement / Walking
        if (isMoving)
        {
            var walkCol = (int)((visualTimeMsec / 140uL) % 5uL);
            return new CharacterAnimationFrame(model.SpriteSheetKey, 1, walkCol);
        }

        // Priority 8: Idle Ambient Breathing
        var idleCol = (int)((visualTimeMsec / 220uL) % 5uL);
        return new CharacterAnimationFrame(model.SpriteSheetKey, 0, idleCol);
    }

    private static CharacterAnimationFrame ResolveFireFrame(string sheetKey, float age01)
    {
        var age = Math.Clamp(age01, 0f, 1f);
        if (string.Equals(sheetKey, "crow_ramshot_cannon", StringComparison.Ordinal))
        {
            if (age < 0.4f)
            {
                return new CharacterAnimationFrame(sheetKey, 2, 1);
            }

            var recoveryCol = Math.Clamp((int)(((age - 0.4f) / 0.6f) * 2f), 0, 1);
            return new CharacterAnimationFrame(sheetKey, 3, recoveryCol);
        }

        if (string.Equals(sheetKey, "crow_cinder", StringComparison.Ordinal))
        {
            return new CharacterAnimationFrame(sheetKey, 3, age < 0.5f ? 0 : 1);
        }

        if (string.Equals(sheetKey, "crow_bow", StringComparison.Ordinal))
        {
            ReadOnlySpan<int> validColumns = [0, 1, 2, 4];
            return new CharacterAnimationFrame(sheetKey, 3, SelectColumn(validColumns, age));
        }

        if (string.Equals(sheetKey, "crow_boomerang", StringComparison.Ordinal))
        {
            ReadOnlySpan<int> validColumns = [0, 1, 4];
            return new CharacterAnimationFrame(sheetKey, 3, SelectColumn(validColumns, age));
        }

        return new CharacterAnimationFrame(sheetKey, 3, Math.Clamp((int)(age * 5f), 0, 4));
    }

    private static int SelectColumn(ReadOnlySpan<int> columns, float age01)
    {
        var index = Math.Clamp((int)(age01 * columns.Length), 0, columns.Length - 1);
        return columns[index];
    }
}
