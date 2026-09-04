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
            var fireCol = Math.Clamp((int)(fireCue.Age01 * 5f), 0, 4);
            return new CharacterAnimationFrame(model.SpriteSheetKey, 3, fireCol);
        }

        // Priority 6: Aiming stance
        if (isAiming || aimAngleRadians.HasValue)
        {
            int aimCol;
            if (aimAngleRadians.HasValue)
            {
                var normalizedAngle = Math.Clamp((aimAngleRadians.Value + 0.6f) / 1.8f, 0f, 1f);
                aimCol = Math.Clamp((int)(normalizedAngle * 5f), 0, 4);
            }
            else
            {
                aimCol = (int)((visualTimeMsec / 200uL) % 2uL);
            }

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
}
