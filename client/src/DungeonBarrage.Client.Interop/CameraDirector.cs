namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Mode controlling how the camera resolves its presentation offset.
/// </summary>
public enum CameraMode
{
    /// <summary>Default view centered on the arena, subject to manual player pan.</summary>
    Manual,

    /// <summary>Camera follows an active projectile during transition playback.</summary>
    TrackPlayback,
}

/// <summary>
/// Pure C# presentation controller for camera framing, boundary clamping, and combat impulse composition.
/// </summary>
/// <remarks>
/// This class is deliberately Godot-free. It does not manipulate engine nodes or mutate authoritative
/// simulation state. It manages presentation-only coordinates in display pixels, composing manual pan,
/// arena clamping, transient combat impulses from <see cref="TransitionCueResolver"/>, and dynamic
/// projectile framing.
/// </remarks>
public sealed class CameraDirector
{
    private PresentationPoint _manualOffset = new(0f, 0f);
    private PresentationPoint _impulseOffset = new(0f, 0f);
    private PresentationPoint _playbackOffset = new(0f, 0f);
    private CameraMode _mode = CameraMode.Manual;

    private float _panLimitX = 400f;
    private float _panLimitY = 300f;

    /// <summary>Current camera operating mode.</summary>
    public CameraMode Mode => _mode;

    /// <summary>Player-initiated manual pan offset in presentation pixels.</summary>
    public PresentationPoint ManualOffset => _manualOffset;

    /// <summary>Transient combat impulse offset from impact events in presentation pixels.</summary>
    public PresentationPoint ImpulseOffset => _impulseOffset;

    /// <summary>Active playback tracking offset in presentation pixels.</summary>
    public PresentationPoint PlaybackOffset => _playbackOffset;

    /// <summary>Whether manual pan has been applied.</summary>
    public bool HasManualPan => _manualOffset.X != 0f || _manualOffset.Y != 0f;

    /// <summary>
    /// Combined presentation offset applied to the world origin:
    /// manual pan + playback tracking + transient combat impulse.
    /// </summary>
    public PresentationPoint EffectiveOffset => new(
        _manualOffset.X + _playbackOffset.X + _impulseOffset.X,
        _manualOffset.Y + _playbackOffset.Y + _impulseOffset.Y);

    /// <summary>
    /// Updates allowable pan limits based on arena and viewport dimensions.
    /// </summary>
    /// <param name="arenaWidthPixels">Total width of the arena in presentation pixels.</param>
    /// <param name="arenaHeightPixels">Total height of the arena in presentation pixels.</param>
    /// <param name="viewportWidth">Width of the viewport in presentation pixels.</param>
    /// <param name="viewportHeight">Height of the viewport in presentation pixels.</param>
    public void UpdateArenaLimits(
        float arenaWidthPixels,
        float arenaHeightPixels,
        float viewportWidth,
        float viewportHeight)
    {
        _panLimitX = Math.Max(100f, (arenaWidthPixels * 0.5f) + Math.Max(0f, viewportWidth * 0.25f));
        _panLimitY = Math.Max(80f, (arenaHeightPixels * 0.5f) + Math.Max(0f, viewportHeight * 0.25f));
        ClampManualOffset();
    }

    /// <summary>
    /// Adds a delta to the manual pan offset, clamping to the configured arena limits.
    /// </summary>
    /// <param name="deltaX">Horizontal pan in pixels.</param>
    /// <param name="deltaY">Vertical pan in pixels.</param>
    public void Pan(float deltaX, float deltaY)
    {
        _manualOffset = new PresentationPoint(
            Math.Clamp(_manualOffset.X + deltaX, -_panLimitX, _panLimitX),
            Math.Clamp(_manualOffset.Y + deltaY, -_panLimitY, _panLimitY));
    }

    /// <summary>
    /// Sets the transient combat impulse offset derived from <see cref="TransitionCueResolver"/>.
    /// </summary>
    /// <param name="impulse">Combat camera impulse in cell units.</param>
    /// <param name="cellSize">Current presentation cell size in pixels.</param>
    public void SetImpulse(PresentationCameraImpulse impulse, float cellSize)
    {
        _impulseOffset = new PresentationPoint(
            impulse.CellX * cellSize,
            impulse.CellY * cellSize);
    }

    /// <summary>
    /// Updates playback tracking. When an active projectile is near or beyond viewport edges,
    /// shifts the camera to keep the action in frame.
    /// </summary>
    /// <param name="projectileScreenPos">Current projectile position in screen space, or null if inactive.</param>
    /// <param name="viewportWidth">Viewport width in pixels.</param>
    /// <param name="viewportHeight">Viewport height in pixels.</param>
    public void TrackPlayback(PresentationPoint? projectileScreenPos, float viewportWidth, float viewportHeight)
    {
        if (projectileScreenPos is not { } pos || viewportWidth <= 0f || viewportHeight <= 0f)
        {
            _playbackOffset = new PresentationPoint(0f, 0f);
            _mode = CameraMode.Manual;
            return;
        }

        _mode = CameraMode.TrackPlayback;
        var centerX = viewportWidth * 0.5f;
        var centerY = viewportHeight * 0.5f;
        var deadzoneX = viewportWidth * 0.25f;
        var deadzoneY = viewportHeight * 0.25f;

        var shiftX = 0f;
        var shiftY = 0f;

        if (pos.X < centerX - deadzoneX)
        {
            shiftX = (centerX - deadzoneX) - pos.X;
        }
        else if (pos.X > centerX + deadzoneX)
        {
            shiftX = (centerX + deadzoneX) - pos.X;
        }

        if (pos.Y < centerY - deadzoneY)
        {
            shiftY = (centerY - deadzoneY) - pos.Y;
        }
        else if (pos.Y > centerY + deadzoneY)
        {
            shiftY = (centerY + deadzoneY) - pos.Y;
        }

        var clampedShiftX = Math.Clamp(shiftX, -_panLimitX * 0.6f, _panLimitX * 0.6f);
        var clampedShiftY = Math.Clamp(shiftY, -_panLimitY * 0.6f, _panLimitY * 0.6f);
        _playbackOffset = new PresentationPoint(clampedShiftX, clampedShiftY);
    }

    /// <summary>
    /// Resets manual pan and playback tracking to the default centered origin.
    /// </summary>
    public void Reset()
    {
        _manualOffset = new PresentationPoint(0f, 0f);
        _playbackOffset = new PresentationPoint(0f, 0f);
        _impulseOffset = new PresentationPoint(0f, 0f);
        _mode = CameraMode.Manual;
    }

    private void ClampManualOffset()
    {
        _manualOffset = new PresentationPoint(
            Math.Clamp(_manualOffset.X, -_panLimitX, _panLimitX),
            Math.Clamp(_manualOffset.Y, -_panLimitY, _panLimitY));
    }
}
