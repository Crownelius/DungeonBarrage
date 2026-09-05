using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// A transient display particle representing sparks, embers, or smoke.
/// </summary>
public sealed class EffectParticle
{
    /// <summary>Particle center in presentation pixels.</summary>
    public PresentationPoint Position { get; set; }

    /// <summary>Velocity in presentation pixels per second.</summary>
    public PresentationPoint Velocity { get; set; }

    /// <summary>Elapsed age in seconds.</summary>
    public float LifeSeconds { get; set; }

    /// <summary>Total duration in seconds.</summary>
    public float MaxLifeSeconds { get; set; }

    /// <summary>Display radius/size in presentation pixels.</summary>
    public float Size { get; set; }

    /// <summary>Primary color in hex RGBA/RGB format.</summary>
    public string ColorHex { get; set; } = "#FFFFFF";

    /// <summary>Downward gravity acceleration in pixels per second squared.</summary>
    public float GravityY { get; set; }

    /// <summary>Whether the particle is still actively rendering.</summary>
    public bool IsAlive => LifeSeconds < MaxLifeSeconds;

    /// <summary>Current normalized opacity from 1.0 down to 0.0.</summary>
    public float Alpha => Math.Clamp(1f - (LifeSeconds / Math.Max(0.001f, MaxLifeSeconds)), 0f, 1f);
}

/// <summary>
/// An expanding shockwave ring centered at an impact point.
/// </summary>
public sealed class ShockwaveRing
{
    /// <summary>Shockwave center in presentation pixels.</summary>
    public PresentationPoint Center { get; set; }

    /// <summary>Starting radius in presentation pixels.</summary>
    public float StartRadius { get; set; }

    /// <summary>Maximum expanded radius in presentation pixels.</summary>
    public float MaxRadius { get; set; }

    /// <summary>Elapsed age in seconds.</summary>
    public float LifeSeconds { get; set; }

    /// <summary>Total duration in seconds.</summary>
    public float MaxLifeSeconds { get; set; }

    /// <summary>Primary color in hex RGBA/RGB format.</summary>
    public string ColorHex { get; set; } = "#FFAA33";

    /// <summary>Whether the shockwave is still actively rendering.</summary>
    public bool IsAlive => LifeSeconds < MaxLifeSeconds;

    /// <summary>Normalized expansion progress from 0.0 to 1.0.</summary>
    public float Progress => Math.Clamp(LifeSeconds / Math.Max(0.001f, MaxLifeSeconds), 0f, 1f);

    /// <summary>Current expanded radius in presentation pixels.</summary>
    public float CurrentRadius => StartRadius + ((MaxRadius - StartRadius) * Progress);

    /// <summary>Current normalized opacity from 1.0 down to 0.0.</summary>
    public float Alpha => Math.Clamp(1f - Progress, 0f, 1f);
}

/// <summary>
/// A persistent visual reticle / crater marker that remains visible regardless of graphics tier or reduced motion.
/// </summary>
public sealed class TargetMarker
{
    /// <summary>Target center in presentation pixels.</summary>
    public PresentationPoint Position { get; set; }

    /// <summary>Target marker radius in presentation pixels.</summary>
    public float Radius { get; set; }

    /// <summary>Elapsed age in seconds.</summary>
    public float LifeSeconds { get; set; }

    /// <summary>Total duration in seconds.</summary>
    public float MaxLifeSeconds { get; set; }

    /// <summary>Primary color in hex RGBA/RGB format.</summary>
    public string ColorHex { get; set; } = "#FF4422";

    /// <summary>Whether the target marker is still actively rendering.</summary>
    public bool IsAlive => LifeSeconds < MaxLifeSeconds;

    /// <summary>Current normalized opacity from 1.0 down to 0.0.</summary>
    public float Alpha => Math.Clamp(1f - (LifeSeconds / Math.Max(0.001f, MaxLifeSeconds)), 0f, 1f);
}

/// <summary>
/// A transient floating combat text displaying damage numbers or critical hits.
/// </summary>
public sealed class FloatingDamageText
{
    /// <summary>Current display center in presentation pixels.</summary>
    public PresentationPoint Position { get; set; }

    /// <summary>Formatted text content (e.g., "-25").</summary>
    public string Text { get; set; } = string.Empty;

    /// <summary>Text color in hex format.</summary>
    public string ColorHex { get; set; } = "#FF5252";

    /// <summary>Elapsed age in seconds.</summary>
    public float LifeSeconds { get; set; }

    /// <summary>Total duration in seconds.</summary>
    public float MaxLifeSeconds { get; set; } = 0.85f;

    /// <summary>Whether this damage instance was a critical hit.</summary>
    public bool IsCrit { get; set; }

    /// <summary>Whether the text is still actively rendering.</summary>
    public bool IsAlive => LifeSeconds < MaxLifeSeconds;

    /// <summary>Current normalized opacity from 1.0 down to 0.0.</summary>
    public float Alpha => Math.Clamp(1f - (LifeSeconds / Math.Max(0.001f, MaxLifeSeconds)), 0f, 1f);
}

/// <summary>
/// Tiered, disposal-safe presentation effect system driven by authoritative combat events.
/// </summary>
/// <remarks>
/// This class is strictly Godot-free. It uses pre-allocated, bounded object pools to prevent
/// runtime heap allocations during combat. It guarantees that tactical target markers and hit
/// indicators remain visible on low-performance tiers and under reduced-motion settings, while
/// scaling particle bursts and shockwaves proportionally.
/// </remarks>
public sealed class CombatEffectSystem
{
    private const int MaxParticles = 256;
    private const int MaxShockwaves = 32;
    private const int MaxTargetMarkers = 16;
    private const int MaxDamageTexts = 16;

    private readonly List<EffectParticle> _particles = new(MaxParticles);
    private readonly List<ShockwaveRing> _shockwaves = new(MaxShockwaves);
    private readonly List<TargetMarker> _targetMarkers = new(MaxTargetMarkers);
    private readonly List<FloatingDamageText> _damageTexts = new(MaxDamageTexts);

    /// <summary>Active particles.</summary>
    public IReadOnlyList<EffectParticle> ActiveParticles => _particles;

    /// <summary>Active shockwave rings.</summary>
    public IReadOnlyList<ShockwaveRing> ActiveShockwaves => _shockwaves;

    /// <summary>Active target markers.</summary>
    public IReadOnlyList<TargetMarker> ActiveTargetMarkers => _targetMarkers;

    /// <summary>Active floating damage texts.</summary>
    public IReadOnlyList<FloatingDamageText> ActiveDamageTexts => _damageTexts;

    /// <summary>
    /// Advances active effects by the given delta time, updating positions and pruning expired items.
    /// </summary>
    /// <param name="deltaSeconds">Time elapsed since the previous update.</param>
    public void Update(float deltaSeconds)
    {
        if (deltaSeconds <= 0f)
        {
            return;
        }

        // Update particles
        for (var i = _particles.Count - 1; i >= 0; i--)
        {
            var p = _particles[i];
            p.LifeSeconds += deltaSeconds;
            if (!p.IsAlive)
            {
                _particles.RemoveAt(i);
                continue;
            }

            p.Position = new PresentationPoint(
                p.Position.X + (p.Velocity.X * deltaSeconds),
                p.Position.Y + (p.Velocity.Y * deltaSeconds));

            // Velocity damping with gravity
            var newVx = p.Velocity.X * (1f - (2.5f * deltaSeconds));
            var newVy = (p.Velocity.Y + (p.GravityY * deltaSeconds)) * (1f - (1.5f * deltaSeconds));
            p.Velocity = new PresentationPoint(newVx, newVy);
        }

        // Update shockwaves
        for (var i = _shockwaves.Count - 1; i >= 0; i--)
        {
            var s = _shockwaves[i];
            s.LifeSeconds += deltaSeconds;
            if (!s.IsAlive)
            {
                _shockwaves.RemoveAt(i);
            }
        }

        // Update target markers
        for (var i = _targetMarkers.Count - 1; i >= 0; i--)
        {
            var m = _targetMarkers[i];
            m.LifeSeconds += deltaSeconds;
            if (!m.IsAlive)
            {
                _targetMarkers.RemoveAt(i);
            }
        }

        // Update floating damage texts
        for (var i = _damageTexts.Count - 1; i >= 0; i--)
        {
            var dt = _damageTexts[i];
            dt.LifeSeconds += deltaSeconds;
            if (!dt.IsAlive)
            {
                _damageTexts.RemoveAt(i);
                continue;
            }

            dt.Position = new PresentationPoint(
                dt.Position.X,
                dt.Position.Y - (30f * deltaSeconds));
        }
    }

    /// <summary>
    /// Spawns an impact effect (shockwaves, burst particles, target marker) scaled by graphics tier and motion setting.
    /// </summary>
    /// <param name="position">Screen-space impact center.</param>
    /// <param name="cellSize">Display cell size in pixels.</param>
    /// <param name="tier">Performance tier controlling particle count and complexity.</param>
    /// <param name="reduceMotion">Whether motion reduction is enabled.</param>
    public void SpawnImpact(
        PresentationPoint position,
        float cellSize,
        ClientPerformanceTier tier,
        bool reduceMotion)
    {
        // 1. Target Marker is ALWAYS spawned (guaranteed tactical visibility across all tiers)
        if (_targetMarkers.Count < MaxTargetMarkers)
        {
            _targetMarkers.Add(new TargetMarker
            {
                Position = position,
                Radius = cellSize * 0.75f,
                LifeSeconds = 0f,
                MaxLifeSeconds = 0.65f,
                ColorHex = "#FF5533",
            });
        }

        // 2. Shockwave Ring
        if (_shockwaves.Count < MaxShockwaves)
        {
            var maxRadius = reduceMotion ? cellSize * 1.0f : cellSize * 1.8f;
            var duration = reduceMotion ? 0.35f : 0.45f;
            _shockwaves.Add(new ShockwaveRing
            {
                Center = position,
                StartRadius = cellSize * 0.25f,
                MaxRadius = maxRadius,
                LifeSeconds = 0f,
                MaxLifeSeconds = duration,
                ColorHex = "#FFA726",
            });

            if (tier == ClientPerformanceTier.High && !reduceMotion && _shockwaves.Count < MaxShockwaves)
            {
                _shockwaves.Add(new ShockwaveRing
                {
                    Center = position,
                    StartRadius = cellSize * 0.1f,
                    MaxRadius = cellSize * 1.1f,
                    LifeSeconds = 0f,
                    MaxLifeSeconds = 0.30f,
                    ColorHex = "#FFF59D",
                });
            }
        }

        // 3. Particles
        if (reduceMotion || tier == ClientPerformanceTier.Low)
        {
            // Reduced motion or Low tier: omit high-velocity particles to prevent disorientation and lag
            return;
        }

        var particleCount = tier == ClientPerformanceTier.Medium ? 6 : 14;
        var baseSpeed = cellSize * 4f;

        for (var i = 0; i < particleCount && _particles.Count < MaxParticles; i++)
        {
            var angle = (MathF.Tau / particleCount) * i;
            var speed = baseSpeed * (0.6f + (0.8f * ((i % 3) / 2f)));
            _particles.Add(new EffectParticle
            {
                Position = position,
                Velocity = new PresentationPoint(MathF.Cos(angle) * speed, MathF.Sin(angle) * speed),
                LifeSeconds = 0f,
                MaxLifeSeconds = 0.35f + (0.15f * ((i % 2))),
                Size = Math.Max(2f, cellSize * 0.18f),
                ColorHex = i % 2 == 0 ? "#FFCA28" : "#FF7043",
            });
        }
    }

    /// <summary>
    /// Spawns muzzle flash particles scaled by performance tier and motion setting.
    /// </summary>
    /// <param name="muzzle">Muzzle anchor in presentation pixels.</param>
    /// <param name="facingSign">Facing direction multiplier (+1 right, -1 left).</param>
    /// <param name="cellSize">Display cell size in pixels.</param>
    /// <param name="tier">Performance tier.</param>
    /// <param name="reduceMotion">Whether reduced motion is enabled.</param>
    public void SpawnMuzzleFire(
        PresentationPoint muzzle,
        float facingSign,
        float cellSize,
        ClientPerformanceTier tier,
        bool reduceMotion)
    {
        if (reduceMotion || tier == ClientPerformanceTier.Low)
        {
            return;
        }

        var count = tier == ClientPerformanceTier.Medium ? 3 : 7;
        for (var i = 0; i < count && _particles.Count < MaxParticles; i++)
        {
            var spreadAngle = (-0.3f + (0.6f * (i / Math.Max(1f, count - 1f)))) * 0.5f;
            var vx = facingSign * MathF.Cos(spreadAngle) * cellSize * 5f;
            var vy = MathF.Sin(spreadAngle) * cellSize * 3f;

            _particles.Add(new EffectParticle
            {
                Position = muzzle,
                Velocity = new PresentationPoint(vx, vy),
                LifeSeconds = 0f,
                MaxLifeSeconds = 0.22f,
                Size = Math.Max(2f, cellSize * 0.14f),
                ColorHex = "#FFE082",
            });
        }
    }

    /// <summary>
    /// Spawns hit spark particles at a defender body scaled by performance tier and motion setting.
    /// </summary>
    /// <param name="targetCenter">Center of affected target in presentation pixels.</param>
    /// <param name="cellSize">Display cell size in pixels.</param>
    /// <param name="tier">Performance tier.</param>
    /// <param name="reduceMotion">Whether reduced motion is enabled.</param>
    public void SpawnHitSparks(
        PresentationPoint targetCenter,
        float cellSize,
        ClientPerformanceTier tier,
        bool reduceMotion)
    {
        // Hit indicator is always added
        if (_targetMarkers.Count < MaxTargetMarkers)
        {
            _targetMarkers.Add(new TargetMarker
            {
                Position = targetCenter,
                Radius = cellSize * 0.55f,
                LifeSeconds = 0f,
                MaxLifeSeconds = 0.40f,
                ColorHex = "#EF5350",
            });
        }

        if (reduceMotion || tier == ClientPerformanceTier.Low)
        {
            return;
        }

        var count = tier == ClientPerformanceTier.Medium ? 4 : 8;
        for (var i = 0; i < count && _particles.Count < MaxParticles; i++)
        {
            var angle = (MathF.Tau / count) * i;
            var speed = cellSize * 2.8f;
            _particles.Add(new EffectParticle
            {
                Position = targetCenter,
                Velocity = new PresentationPoint(MathF.Cos(angle) * speed, MathF.Sin(angle) * speed),
                LifeSeconds = 0f,
                MaxLifeSeconds = 0.28f,
                Size = Math.Max(2f, cellSize * 0.12f),
                ColorHex = "#FF8A65",
            });
        }
    }

    /// <summary>
    /// Spawns a floating damage number above a damaged unit.
    /// </summary>
    /// <param name="position">Anchor position in presentation pixels.</param>
    /// <param name="damage">Damage value to display.</param>
    /// <param name="isCrit">Whether the attack was a critical hit.</param>
    public void SpawnDamageNumber(PresentationPoint position, int damage, bool isCrit = false)
    {
        if (damage <= 0)
        {
            return;
        }

        if (_damageTexts.Count >= MaxDamageTexts)
        {
            _damageTexts.RemoveAt(0);
        }

        _damageTexts.Add(new FloatingDamageText
        {
            Position = new PresentationPoint(position.X, position.Y - 10f),
            Text = $"-{damage}",
            ColorHex = isCrit ? "#FFD54F" : "#FF5252",
            LifeSeconds = 0f,
            MaxLifeSeconds = isCrit ? 1.0f : 0.8f,
            IsCrit = isCrit,
        });
    }

    /// <summary>
    /// Spawns crumbling terrain debris particles with downward gravity.
    /// </summary>
    /// <param name="impactPoint">Center of terrain destruction in presentation pixels.</param>
    /// <param name="cellSize">Display cell size in pixels.</param>
    /// <param name="tier">Performance tier.</param>
    /// <param name="reduceMotion">Whether reduced motion is enabled.</param>
    public void SpawnTerrainDebris(
        PresentationPoint impactPoint,
        float cellSize,
        ClientPerformanceTier tier,
        bool reduceMotion)
    {
        if (reduceMotion || tier == ClientPerformanceTier.Low)
        {
            return;
        }

        var count = tier == ClientPerformanceTier.Medium ? 8 : 16;
        var colors = new[] { "#8D6E63", "#795548", "#5D4037", "#A1887F", "#6D4C41" };

        for (var i = 0; i < count && _particles.Count < MaxParticles; i++)
        {
            var norm = (float)i / Math.Max(1, count - 1);
            var angle = -MathF.PI * (0.15f + (0.7f * norm)); // upward spread arc
            var speed = cellSize * (2.2f + (2.5f * ((i % 4) / 3f)));
            var size = Math.Max(2.5f, cellSize * (0.12f + (0.10f * ((i % 3) / 2f))));

            _particles.Add(new EffectParticle
            {
                Position = impactPoint,
                Velocity = new PresentationPoint(MathF.Cos(angle) * speed, MathF.Sin(angle) * speed),
                LifeSeconds = 0f,
                MaxLifeSeconds = 0.55f + (0.25f * ((i % 2))),
                Size = size,
                ColorHex = colors[i % colors.Length],
                GravityY = cellSize * 14f, // downward acceleration
            });
        }
    }

    /// <summary>
    /// Clears all active effects and resets pools to empty.
    /// </summary>
    public void Clear()
    {
        _particles.Clear();
        _shockwaves.Clear();
        _targetMarkers.Clear();
        _damageTexts.Clear();
    }
}
