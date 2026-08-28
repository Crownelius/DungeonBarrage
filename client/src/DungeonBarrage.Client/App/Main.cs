using System.Diagnostics.CodeAnalysis;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using DungeonBarrage.Client.Match;
using Godot;

namespace DungeonBarrage.Client.App;

/// <summary>
/// The scene root: a menu with build diagnostics, and one playable authoritative turn of the real
/// horizontal-test duel using placeholder shapes.
/// </summary>
/// <remarks>
/// <para>
/// C4's gate was a static render of one snapshot; this is C5's gate — CLIENT_SPEC §20.5 step 5:
/// move, fire one real shot, play its transition, and reconcile to the post-snapshot, with input
/// locked during playback. The control scheme (arrow keys to move, click-drag-release to aim and
/// fire) is a placeholder choice, not a claimed final one — CLIENT_SPEC §22 leaves input feel
/// open, and both phases accept every command kind (<see cref="ClientMatchPhase.Movement"/> and
/// <see cref="ClientMatchPhase.AimingAndSelection"/> both pass
/// <c>MatchPhase::accepts_ability_command</c> in the authoritative core), so there is no separate
/// "confirm movement" step to model.
/// </para>
/// <para>
/// Placeholder pixels-per-cell: art direction is an explicitly open decision (CLIENT_SPEC §22.1).
/// <see cref="PixelsPerCell"/> is a placeholder chosen only to fit the fixture map in the window;
/// it is not a claimed final value.
/// </para>
/// </remarks>
public partial class Main : Node2D
{
    private const int PixelsPerCell = 12;
    private const int PlayerRadiusPixels = 10;

    /// <summary>One authoritative fixed-point cell's worth of horizontal move step.</summary>
    private const int MoveStepDx = 1024;

    /// <summary>Placeholder drag-to-power scale: pixels of drag per basis point of power.</summary>
    private const float DragPixelsPerPowerBasisPoint = 0.5f;

    private const int MaxPowerBasisPoints = 10_000;

    private static readonly Color BackgroundColor = new(0.08f, 0.09f, 0.12f);
    private static readonly Color MenuTextColor = Colors.White;
    private static readonly Color TerrainSoilColor = new(0.45f, 0.32f, 0.18f);
    private static readonly Color TerrainWoodColor = new(0.55f, 0.42f, 0.20f);
    private static readonly Color TerrainStoneColor = new(0.55f, 0.55f, 0.58f);
    private static readonly Color BlockColor = new(0.30f, 0.55f, 0.30f);
    private static readonly Color BlockDamagedColor = new(0.65f, 0.35f, 0.20f);
    private static readonly Color PlayerAColor = new(0.25f, 0.55f, 0.95f);
    private static readonly Color PlayerBColor = new(0.90f, 0.35f, 0.30f);
    private static readonly Color AimLineColor = new(0.95f, 0.85f, 0.25f);
    private static readonly Color LockedHintColor = new(0.6f, 0.6f, 0.65f);

    private BuildDiagnostics? _diagnostics;
    private MatchBootstrapResult? _match;
    [SuppressMessage(
        "Design",
        "CA2213:Disposable fields should be disposed",
        Justification =
            "This is a Godot Node2D, not a .NET IDisposable type: its teardown lifecycle is "
            + "_ExitTree, which does dispose this field (see the override below), not a Dispose "
            + "method the analyzer recognizes.")]
    private LiveMatch? _live;
    private string? _menuError;
    private string? _liveError;
    private bool _started;

    private bool _isAiming;
    private Vector2 _aimOrigin;
    private Vector2 _aimCurrent;

    private ulong _inputLockedUntilMsec;

    /// <inheritdoc />
    // `async void` rather than `async Task`: `_Ready` is an engine-invoked override with a
    // fixed `void` signature Godot itself calls, not something this code calls and can await.
    // This is the documented Godot C# pattern for a lifecycle callback that needs to yield to a
    // later frame; the alternative is blocking the thread instead of yielding, and screenshotting
    // a genuine engine frame requires yielding to the frame loop that produces one.
#pragma warning disable CA1849 // no synchronous alternative exists for waiting on a frame
    public override async void _Ready()
#pragma warning restore CA1849
    {
        _diagnostics = BuildDiagnostics.Capture();

        var smokeOptions = C4SmokeOptions.Parse(OS.GetCmdlineUserArgs());
        if (smokeOptions is not null)
        {
            await RunSmokeAndQuitAsync(smokeOptions).ConfigureAwait(true);
            return;
        }

        var c5SmokeOptions = C5SmokeOptions.Parse(OS.GetCmdlineUserArgs());
        if (c5SmokeOptions is not null)
        {
            await RunC5SmokeAndQuitAsync(c5SmokeOptions).ConfigureAwait(true);
            return;
        }

        QueueRedraw();
    }

    /// <inheritdoc />
    public override void _Process(double delta)
    {
        // Input unlocking has no visible effect on its own — the state it gates already
        // reconciled the instant the command was accepted (see LiveMatch). This redraw is only
        // so the "locked" hint in the HUD disappears the frame the lock actually lifts, instead
        // of lagging one click behind.
        if (_live is not null && IsInputLocked() != _wasLockedLastFrame)
        {
            _wasLockedLastFrame = IsInputLocked();
            QueueRedraw();
        }

        if (_isAiming)
        {
            QueueRedraw();
        }
    }

    private bool _wasLockedLastFrame;

    /// <inheritdoc />
    public override void _UnhandledInput(InputEvent @event)
    {
        ArgumentNullException.ThrowIfNull(@event);

        if (_live is not null)
        {
            HandleLiveInput(@event);
            return;
        }

        if (_started)
        {
            return;
        }

        var isActivation = @event.IsActionPressed("ui_accept") ||
            (@event is InputEventMouseButton { Pressed: true, ButtonIndex: MouseButton.Left });
        if (!isActivation)
        {
            return;
        }

        GetViewport().SetInputAsHandled();
        StartDuel();
    }

    /// <inheritdoc />
    public override void _Draw()
    {
        DrawRect(new Rect2(Vector2.Zero, GetViewportRect().Size), BackgroundColor);

        if (_live is not null)
        {
            DrawLiveMatch(_live);
        }
        else if (_match is not null)
        {
            DrawMatch(_match.Frame.Snapshot, _match.Frame.Terrain);
        }
        else
        {
            DrawMenu();
        }
    }

    /// <inheritdoc />
    public override void _ExitTree()
    {
        // The one thing every path through this scene must still get right on the way out: no
        // native handle survives the process. Idempotent disposal (C3) means this is safe even
        // if smoke mode already disposed the session itself.
        _match?.Session.Dispose();
        _live?.Dispose();
        base._ExitTree();
    }

    private void StartDuel()
    {
        try
        {
            _live = CreateLiveMatch(FixtureMatchBootstrapper.Start());
            _menuError = null;
        }
        catch (Exception exception) when (exception is InvalidDataException or NativeSimulationException)
        {
            // A failed bootstrap is diagnostic information for the menu, not a crash: the whole
            // point of showing build/version diagnostics first is to make a mismatch legible
            // instead of a silent black screen.
            _menuError = exception.Message;
        }

        _started = _live is not null;
        QueueRedraw();
    }

    /// <summary>
    /// Glues the Godot-specific bootstrap result to the Godot-free <see cref="LiveMatch"/>.
    /// </summary>
    /// <remarks>
    /// <see cref="LiveMatch"/> lives in the Interop project specifically so it stays testable
    /// without the engine; it therefore cannot reference <see cref="MatchBootstrapResult"/> (a
    /// Godot-project type). This is the one place that bridges the two, by passing the
    /// bootstrap's own pieces through to <see cref="LiveMatch.Create"/> directly.
    /// </remarks>
    /// <param name="bootstrap">A freshly created match and its initial frame.</param>
    /// <returns>The live wrapper. Ownership of the session transfers to the returned instance.</returns>
    private static LiveMatch CreateLiveMatch(MatchBootstrapResult bootstrap) =>
        LiveMatch.Create(
            bootstrap.Session,
            bootstrap.Frame.Snapshot,
            bootstrap.Frame.Terrain,
            bootstrap.Frame.Snapshot.MatchId);

    private bool IsInputLocked() => Time.GetTicksMsec() < _inputLockedUntilMsec;

    private void HandleLiveInput(InputEvent @event)
    {
        if (_live is null || IsInputLocked() || _live.CurrentSnapshot.HasAttackedThisTurn)
        {
            return;
        }

        if (@event.IsActionPressed("ui_left"))
        {
            GetViewport().SetInputAsHandled();
            _ = SubmitAndRedrawAsync(() => _live.SubmitMoveAsync(-MoveStepDx));
            return;
        }

        if (@event.IsActionPressed("ui_right"))
        {
            GetViewport().SetInputAsHandled();
            _ = SubmitAndRedrawAsync(() => _live.SubmitMoveAsync(MoveStepDx));
            return;
        }

        if (@event is InputEventMouseButton { ButtonIndex: MouseButton.Left } mouseButton)
        {
            GetViewport().SetInputAsHandled();
            if (mouseButton.Pressed)
            {
                _isAiming = true;
                _aimOrigin = mouseButton.Position;
                _aimCurrent = mouseButton.Position;
                QueueRedraw();
            }
            else if (_isAiming)
            {
                _isAiming = false;
                var (angleMillidegrees, powerBasisPoints) = ResolveAim(_aimOrigin, mouseButton.Position);
                _ = SubmitAndRedrawAsync(() => _live.SubmitAbilityAsync(
                    ClientAbilitySlot.Basic,
                    angleMillidegrees,
                    powerBasisPoints,
                    targetPlayerId: null));
            }

            return;
        }

        if (@event is InputEventMouseMotion motion && _isAiming)
        {
            _aimCurrent = motion.Position;
            QueueRedraw();
        }
    }

    /// <summary>
    /// Converts a screen drag into an angle/power pair.
    /// </summary>
    /// <remarks>
    /// A placeholder mapping, not a verified one: the mechanically load-bearing values in this
    /// codebase are the fixture's own frozen <c>angleMillidegrees</c>/<c>powerBasisPoints</c>,
    /// exercised byte-for-byte by <c>CommandRoundTripTests</c> and by the C5 smoke path below —
    /// neither depends on this mapping's exact direction being right. This exists only so an
    /// interactive human has something to aim with; feel is a C6+ concern.
    /// </remarks>
    private static (int AngleMillidegrees, int PowerBasisPoints) ResolveAim(Vector2 origin, Vector2 release)
    {
        var drag = release - origin;
        if (drag.LengthSquared() < 1f)
        {
            return (0, 0);
        }

        // Screen Y is positive-downward, matching the authoritative convention (FixedPoint's own
        // doc comment), so an upward drag has negative Y — negating here reads that as a
        // positive launch angle, the usual artillery-game sense of "up is positive".
        var angleDegrees = Mathf.RadToDeg(Mathf.Atan2(-drag.Y, drag.X));
        var angleMillidegrees = Mathf.RoundToInt(angleDegrees * 1000f);

        var power = Mathf.Clamp(drag.Length() / DragPixelsPerPowerBasisPoint, 0, MaxPowerBasisPoints);
        return (angleMillidegrees, Mathf.RoundToInt(power));
    }

    /// <summary>
    /// Submits one command through the real input-lock timer and returns its transition.
    /// </summary>
    /// <remarks>
    /// The single path both a real click and the C5 smoke automation go through, so a smoke run
    /// proves the same lock state machine a human's input is actually gated by — not a parallel
    /// data-only check that the transition merely <em>reported</em> a lock duration.
    /// </remarks>
    /// <param name="submit">Submits the command against the live match.</param>
    /// <returns>The accepted transition, or <see langword="null"/> if it was rejected.</returns>
    private async Task<ClientMatchTransition?> SubmitAndRedrawAsync(Func<Task<ClientMatchTransition>> submit)
    {
        if (_live is null)
        {
            return null;
        }

        // Locked immediately, before the (asynchronous) native call even starts: a second click
        // during the round trip to the native library must not queue a second command against a
        // snapshot generation the first one is about to advance past.
        _inputLockedUntilMsec = Time.GetTicksMsec() + LockDurationMsecFor(_live);
        _liveError = null;
        ClientMatchTransition? transition = null;
        try
        {
            transition = await submit().ConfigureAwait(true);
            return transition;
        }
        catch (Exception exception) when (exception is MatchCommandRejectedException or NativeSimulationException)
        {
            _liveError = exception.Message;
            _inputLockedUntilMsec = Time.GetTicksMsec();
            return null;
        }
        finally
        {
            // Now that a real transition exists, lock for its own reported duration rather than
            // the previous transition's — the estimate above exists only to close the window
            // between click and native response.
            if (transition is not null)
            {
                _inputLockedUntilMsec = Time.GetTicksMsec() + LockDurationMsecFor(_live);
            }

            QueueRedraw();
        }
    }

    private static ulong LockDurationMsecFor(LiveMatch live) =>
        live.PresentationTickRate == 0
            ? 0
            : (ulong)(live.LastInputLockTicks * 1000.0 / live.PresentationTickRate);

    private void DrawMenu()
    {
        var font = ThemeDB.FallbackFont;
        const int fontSize = 20;
        var position = new Vector2(24, 40);

        DrawString(font, position, "DUNGEON BARRAGE", fontSize: fontSize, modulate: MenuTextColor);
        position.Y += 32;
        DrawString(
            font,
            position,
            _diagnostics?.DisplayText ?? "diagnostics unavailable",
            fontSize: 16,
            modulate: MenuTextColor);
        position.Y += 48;
        DrawString(
            font,
            position,
            "Click or press Enter to start the horizontal-test duel.",
            fontSize: 16,
            modulate: MenuTextColor);
        position.Y += 24;
        DrawString(
            font,
            position,
            "Left/Right to move. Click-drag from anywhere, release to aim and fire.",
            fontSize: 14,
            modulate: LockedHintColor);

        if (_menuError is not null)
        {
            position.Y += 32;
            DrawString(font, position, $"Bootstrap failed: {_menuError}", fontSize: 16, modulate: Colors.OrangeRed);
        }
    }

    private void DrawLiveMatch(LiveMatch live)
    {
        var snapshot = live.CurrentSnapshot;
        DrawMatch(snapshot, live.CurrentTerrain);

        if (_isAiming)
        {
            DrawLine(_aimOrigin, _aimCurrent, AimLineColor, width: 2);
        }

        var font = ThemeDB.FallbackFont;
        var hudY = 8f;
        DrawString(
            font,
            new Vector2(8, hudY),
            $"active {snapshot.ActivePlayerId}   phase {snapshot.Phase}" +
            (snapshot.HasAttackedThisTurn ? "   (attacked — turn resolving)" : string.Empty),
            fontSize: 14,
            modulate: MenuTextColor);

        if (IsInputLocked())
        {
            hudY += 18;
            DrawString(font, new Vector2(8, hudY), "input locked — playing transition", fontSize: 12, modulate: LockedHintColor);
        }

        if (_liveError is not null)
        {
            hudY += 18;
            DrawString(font, new Vector2(8, hudY), $"rejected: {_liveError}", fontSize: 12, modulate: Colors.OrangeRed);
        }

        if (snapshot.Outcome is ClientVictoryOutcome victory)
        {
            DrawString(
                font,
                new Vector2(8, hudY + 24),
                $"team {victory.Team} wins",
                fontSize: 18,
                modulate: Colors.Gold);
        }
        else if (snapshot.Outcome is ClientDrawOutcome)
        {
            DrawString(font, new Vector2(8, hudY + 24), "draw", fontSize: 18, modulate: Colors.Gold);
        }
    }

    private void DrawMatch(ClientMatchSnapshot snapshot, TerrainRead terrain)
    {
        DrawTerrain(snapshot, terrain);
        foreach (var block in snapshot.Blocks)
        {
            DrawBlock(block);
        }

        for (var index = 0; index < snapshot.Players.Count; index++)
        {
            DrawPlayer(snapshot.Players[index], index == 0 ? PlayerAColor : PlayerBColor, index);
        }

        var font = ThemeDB.FallbackFont;
        DrawString(
            font,
            new Vector2(8, GetViewportRect().Size.Y - 12),
            $"turn {snapshot.TurnNumber}  gen {snapshot.SnapshotGeneration}  hash {snapshot.StateHash}",
            fontSize: 14,
            modulate: MenuTextColor);
    }

    private void DrawTerrain(ClientMatchSnapshot snapshot, TerrainRead terrain)
    {
        var cells = terrain.Cells.Span;
        var width = (int)snapshot.TerrainWidth;
        var height = (int)snapshot.TerrainHeight;
        if (cells.Length != width * height)
        {
            // The fixture always supplies a matching read (FixtureMatchBootstrapper validates
            // this before a frame is ever constructed); this guards the draw call itself against
            // a future caller that skips that validation.
            return;
        }

        for (var y = 0; y < height; y++)
        {
            for (var x = 0; x < width; x++)
            {
                var material = cells[(y * width) + x];
                var color = material switch
                {
                    1 => TerrainSoilColor,
                    2 => TerrainWoodColor,
                    3 => TerrainStoneColor,
                    _ => (Color?)null,
                };
                if (color is { } solid)
                {
                    DrawRect(
                        new Rect2(x * PixelsPerCell, y * PixelsPerCell, PixelsPerCell, PixelsPerCell),
                        solid);
                }
            }
        }
    }

    private void DrawBlock(ClientBlockSnapshot block)
    {
        var color = block.Health < block.MaxHealth ? BlockDamagedColor : BlockColor;
        DrawRect(
            new Rect2(
                block.OriginCellX * PixelsPerCell,
                block.OriginCellY * PixelsPerCell,
                block.WidthCells * PixelsPerCell,
                block.HeightCells * PixelsPerCell),
            color);
    }

    private void DrawPlayer(ClientPlayerSnapshot player, Color color, int index)
    {
        var center = ToPixels(player.Position);
        DrawCircle(center, PlayerRadiusPixels, player.IsEliminated ? Colors.Gray : color);

        // Placeholder labels stagger by index rather than sharing one fixed offset above the
        // circle: two starting characters are often close enough on a small map that a shared
        // offset overlaps their names into one unreadable run.
        var labelY = -PlayerRadiusPixels - 4 - (index * 16);
        var font = ThemeDB.FallbackFont;
        DrawString(
            font,
            center + new Vector2(-PlayerRadiusPixels, labelY),
            $"{player.CharacterId} {player.Health}/{player.MaxHealth}  gauge {player.SpecialGauge}",
            fontSize: 12,
            modulate: MenuTextColor);
    }

    private static Vector2 ToPixels(ClientPosition position, int positionScale = 1024) =>
        new(
            position.X / (float)positionScale * PixelsPerCell,
            position.Y / (float)positionScale * PixelsPerCell);

    private async Task RunSmokeAndQuitAsync(C4SmokeOptions options)
    {
        // A smoke run is unattended: nothing will ever click past a hang. Every exit from this
        // method — including one from `report.Write` itself, such as an unwritable report path
        // — must still reach `Quit`, so it is the only thing in a `finally`.
        var exitCode = 1;
        try
        {
            var report = await RunSmokeAsync(options).ConfigureAwait(true);
            report.Write(options.ReportPath);
            exitCode = report.Success ? 0 : 1;
        }
        finally
        {
            GetTree().Quit(exitCode);
        }
    }

    private async Task<C4SmokeReport> RunSmokeAsync(C4SmokeOptions options)
    {
        var diagnostics = _diagnostics ?? BuildDiagnostics.Capture();
        var candidates = NativeLibraryResolver.CandidatePaths();

        try
        {
            var result = FixtureMatchBootstrapper.Start();
            var snapshot = result.Frame.Snapshot;
            var terrain = result.Frame.Terrain;

            var solidCells = 0;
            foreach (var cell in terrain.Cells.Span)
            {
                if (cell != 0)
                {
                    solidCells++;
                }
            }

            _match = result;
            QueueRedraw();

            // `_Ready` runs before the engine's first process/draw cycle, so the `QueueRedraw`
            // above has not painted anything yet: capturing immediately reproduced the exact
            // blank-gray frame this comment now prevents. Two frames, not one: the first is
            // where `_Draw` actually executes the draw calls queued above; the second is where
            // the viewport texture is guaranteed to reflect a swapchain image that included
            // them, rather than whatever the previous frame (nothing, on the very first tick)
            // had committed.
            // `ToSignal` returns Godot's own `SignalAwaiter`, not a `Task`; it has no
            // `ConfigureAwait` and Godot's single-threaded scripting model gives no other
            // context to switch to regardless.
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);

            var (screenshotWidth, screenshotHeight) = CaptureScreenshot(options.ScreenshotPath);

            var sessionDisposed = false;
            var disposedSessionRejectedReuse = false;
            try
            {
                await result.Session.DisposeAsync().ConfigureAwait(true);
                sessionDisposed = true;
                try
                {
                    _ = await result.Session.SnapshotAsync().ConfigureAwait(true);
                }
                catch (ObjectDisposedException)
                {
                    disposedSessionRejectedReuse = true;
                }
            }
            finally
            {
                _match = null;
            }

            return new C4SmokeReport(
                Success: true,
                Error: null,
                WorkingDirectory: Directory.GetCurrentDirectory(),
                ExecutablePath: OS.GetExecutablePath(),
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                AbiVersion: diagnostics.AbiVersion,
                SimulationVersion: diagnostics.SimulationVersion,
                ContentVersion: diagnostics.ContentVersion,
                MatchId: snapshot.MatchId,
                StateHash: snapshot.StateHash,
                SnapshotGeneration: (uint)snapshot.SnapshotGeneration,
                TerrainWidth: snapshot.TerrainWidth,
                TerrainHeight: snapshot.TerrainHeight,
                TerrainByteCount: terrain.Cells.Length,
                SolidTerrainCellCount: solidCells,
                BlockCount: snapshot.Blocks.Count,
                PlayerCount: snapshot.Players.Count,
                PositionScale: (uint)snapshot.PositionScale,
                FixedTickRate: snapshot.FixedTickRate,
                ScreenshotWidth: screenshotWidth,
                ScreenshotHeight: screenshotHeight,
                SessionDisposed: sessionDisposed,
                DisposedSessionRejectedReuse: disposedSessionRejectedReuse,
                NativeLibraryCandidates: candidates);
        }
        catch (Exception exception)
        {
            // Deliberately unfiltered. A smoke run's entire job is to turn a failure into a
            // report and a clean exit rather than an engine-logged exception that leaves the
            // process hanging with nothing left to click and nothing calling Quit — which is
            // exactly what happened here once already, against a stale native library.
            return new C4SmokeReport(
                Success: false,
                Error: $"{exception.GetType().Name}: {exception.Message}",
                WorkingDirectory: Directory.GetCurrentDirectory(),
                ExecutablePath: OS.GetExecutablePath(),
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                AbiVersion: diagnostics.AbiVersion,
                SimulationVersion: diagnostics.SimulationVersion,
                ContentVersion: diagnostics.ContentVersion,
                MatchId: string.Empty,
                StateHash: string.Empty,
                SnapshotGeneration: 0,
                TerrainWidth: 0,
                TerrainHeight: 0,
                TerrainByteCount: 0,
                SolidTerrainCellCount: 0,
                BlockCount: 0,
                PlayerCount: 0,
                PositionScale: 0,
                FixedTickRate: 0,
                ScreenshotWidth: 0,
                ScreenshotHeight: 0,
                SessionDisposed: false,
                DisposedSessionRejectedReuse: false,
                NativeLibraryCandidates: candidates);
        }
    }

    private async Task RunC5SmokeAndQuitAsync(C5SmokeOptions options)
    {
        var exitCode = 1;
        try
        {
            var report = await RunC5SmokeAsync(options).ConfigureAwait(true);
            report.Write(options.ReportPath);
            exitCode = report.Success ? 0 : 1;
        }
        finally
        {
            GetTree().Quit(exitCode);
        }
    }

    /// <summary>
    /// Scripts exactly the CLIENT_SPEC §20.5 step 5 turn: move, fire the fixture's own ability,
    /// and check every gate clause mechanically.
    /// </summary>
    /// <remarks>
    /// <para>
    /// This uses the fixture's exact frozen angle/power rather than a UI-drawn drag, for the same
    /// reason the C4 smoke path uses the exact fixture creation request: the mechanical proof
    /// must not depend on a placeholder input mapping being "close enough", only on the real
    /// command DTOs and the real native library.
    /// </para>
    /// <para>
    /// This does <em>not</em> assert either post-transition hash against the frozen fixture's
    /// (<c>378081bb2e830a5d</c> / <c>d8686762470c0c36</c>). <c>hash_state</c> deliberately folds
    /// the set of accepted command ids into the authoritative state hash
    /// (<c>db-sim-core/src/hash.rs</c>, domain <c>0x04</c>), and <see cref="LiveMatch"/> mints its
    /// own ids rather than replaying the fixture's literal ones — so its hash can never equal the
    /// frozen one, by design. An earlier version of this method asserted that equality and failed;
    /// the fix was here, not in <see cref="LiveMatch"/>. What is checked instead is what actually
    /// is invariant regardless of command id: acceptance, real damage, and turn handoff.
    /// </para>
    /// <para>
    /// The lock check runs against the <em>ability</em>, not the move: a move has zero
    /// presentation events to play back (<c>InputLockTicks == 0</c> for
    /// <c>commands/001-move.json</c>'s own frozen response), so there is nothing to lock for and
    /// checking it proves nothing. The ability's projectile flight gives it a real multi-tick
    /// window, which is what CLIENT_SPEC's "input is locked during playback" clause is about.
    /// </para>
    /// </remarks>
    private async Task<C5SmokeReport> RunC5SmokeAsync(C5SmokeOptions options)
    {
        var diagnostics = _diagnostics ?? BuildDiagnostics.Capture();

        LiveMatch? live = null;
        try
        {
            live = CreateLiveMatch(FixtureMatchBootstrapper.Start());
            _live = live;

            var beforeActivePlayer = live.CurrentSnapshot.ActivePlayerId;
            var defenderId = live.CurrentSnapshot.Players.First(p => p.PlayerId != beforeActivePlayer).PlayerId;
            var defenderHealthBefore = HealthOf(live.CurrentSnapshot, defenderId);

            // Goes through `SubmitAndRedrawAsync` — the exact method a real click invokes — so
            // every lock check below reads the actual `_inputLockedUntilMsec` timer a human's
            // next click would be gated by, not a parallel data-only read of a reported tick count.
            var moveTransition = await SubmitAndRedrawAsync(() => live.SubmitMoveAsync(MoveStepDx))
                .ConfigureAwait(true)
                ?? throw new InvalidOperationException("The fixture move was rejected.");
            await WaitTicksAsync(moveTransition.InputLockTicks, moveTransition.PresentationTickRate).ConfigureAwait(true);

            var abilityTransition = await SubmitAndRedrawAsync(() => live.SubmitAbilityAsync(
                ClientAbilitySlot.Basic,
                angleMillidegrees: 45_000,
                powerBasisPoints: 1_500,
                targetPlayerId: null))
                .ConfigureAwait(true)
                ?? throw new InvalidOperationException("The fixture ability was rejected.");
            var lockedImmediatelyAfterAbility = IsInputLocked();

            // "Input is locked during playback": wait out the same duration a real player would
            // be blocked for, then confirm the lock actually lifts rather than staying locked
            // forever — the exact failure mode a stuck timer would produce.
            await WaitTicksAsync(abilityTransition.InputLockTicks, abilityTransition.PresentationTickRate)
                .ConfigureAwait(true);
            var unlockedAfterWaiting = !IsInputLocked();

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (screenshotWidth, screenshotHeight) = CaptureScreenshot(options.ScreenshotPath);

            var final = live.CurrentSnapshot;
            var defenderHealthAfter = HealthOf(final, defenderId);

            return new C5SmokeReport(
                Success: true,
                Error: null,
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                BeforeActivePlayerId: beforeActivePlayer,
                MoveAccepted: moveTransition.Disposition == ClientTransitionDisposition.Accepted,
                MoveEventCount: moveTransition.Events.Count,
                MoveDx: MoveStepDx,
                MoveInputLockTicks: moveTransition.InputLockTicks,
                AbilityAccepted: abilityTransition.Disposition == ClientTransitionDisposition.Accepted,
                AbilityEventCount: abilityTransition.Events.Count,
                AbilityInputLockTicks: abilityTransition.InputLockTicks,
                InputLockedImmediatelyAfterAbility: lockedImmediatelyAfterAbility,
                InputUnlockedAfterWaitingOutTheAbilityLock: unlockedAfterWaiting,
                DefenderPlayerId: defenderId,
                DefenderHealthBeforeAbility: defenderHealthBefore,
                DefenderHealthAfterAbility: defenderHealthAfter,
                AbilityDealtRealDamage: defenderHealthAfter < defenderHealthBefore,
                FinalSnapshotMatchesAbilityPostSnapshot: final.StateHash == abilityTransition.PostSnapshot.StateHash,
                AfterActivePlayerId: final.ActivePlayerId,
                TurnHandedOverToTheOtherPlayer: final.ActivePlayerId != beforeActivePlayer,
                TurnNumberAfter: final.TurnNumber,
                ScreenshotWidth: screenshotWidth,
                ScreenshotHeight: screenshotHeight);
        }
        catch (Exception exception)
        {
            // Same reasoning as the C4 smoke path's unfiltered catch: an unattended run must
            // always produce a report and a clean exit, never an engine-logged hang.
            return new C5SmokeReport(
                Success: false,
                Error: $"{exception.GetType().Name}: {exception.Message}",
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                BeforeActivePlayerId: null,
                MoveAccepted: false,
                MoveEventCount: 0,
                MoveDx: 0,
                MoveInputLockTicks: 0,
                AbilityAccepted: false,
                AbilityEventCount: 0,
                AbilityInputLockTicks: 0,
                InputLockedImmediatelyAfterAbility: false,
                InputUnlockedAfterWaitingOutTheAbilityLock: false,
                DefenderPlayerId: null,
                DefenderHealthBeforeAbility: 0,
                DefenderHealthAfterAbility: 0,
                AbilityDealtRealDamage: false,
                FinalSnapshotMatchesAbilityPostSnapshot: false,
                AfterActivePlayerId: null,
                TurnHandedOverToTheOtherPlayer: false,
                TurnNumberAfter: 0,
                ScreenshotWidth: 0,
                ScreenshotHeight: 0);
        }
        finally
        {
            if (live is not null)
            {
                await live.DisposeAsync().ConfigureAwait(true);
            }

            _live = null;
        }
    }

    private static ushort HealthOf(ClientMatchSnapshot snapshot, string playerId) =>
        snapshot.Players.First(p => p.PlayerId == playerId).Health;

    private static async Task WaitTicksAsync(uint ticks, uint tickRate)
    {
        if (ticks == 0 || tickRate == 0)
        {
            return;
        }

        var seconds = ticks / (double)tickRate;
        await Task.Delay(TimeSpan.FromSeconds(seconds)).ConfigureAwait(true);
    }

    /// <summary>
    /// Captures the rendered frame to <paramref name="path"/>.
    /// </summary>
    /// <remarks>
    /// A headless run (`--headless`) has no display driver, so <c>GetViewport().GetTexture()</c>
    /// returns no real pixels there. Rather than let that surface as a confusing failure deep in
    /// image encoding, it is detected up front and reported as a zero-size screenshot: honest
    /// about what a headless run can prove (bootstrap, data shape, disposal) versus what it
    /// cannot (that a pixel actually painted). CLIENT_SPEC §20.5 draws the same line: headless
    /// covers navigation and data; a real windowed run is required for the graphics claim.
    /// </remarks>
    private (int Width, int Height) CaptureScreenshot(string path)
    {
        if (DisplayServer.GetName() == "headless")
        {
            return (0, 0);
        }

        // The caller has already awaited two real `ProcessFrame` ticks past the `QueueRedraw()`
        // that requested this content, so the viewport texture reflects a presented frame that
        // included it rather than whatever was on screen before `_Ready` ran.
        using var image = GetViewport().GetTexture().GetImage();
        var directory = Path.GetDirectoryName(path);
        if (!string.IsNullOrWhiteSpace(directory))
        {
            Directory.CreateDirectory(directory);
        }

        image.SavePng(path);
        return (image.GetWidth(), image.GetHeight());
    }
}
