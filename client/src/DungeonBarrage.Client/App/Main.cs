using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using DungeonBarrage.Client.Match;
using Godot;

namespace DungeonBarrage.Client.App;

/// <summary>
/// The C4 render/export spike root: a menu with build diagnostics, and one static render of the
/// real horizontal-test duel snapshot using placeholder shapes.
/// </summary>
/// <remarks>
/// <para>
/// This is deliberately not a gameplay scene. CLIENT_SPEC's C4 gate is "render one authoritative
/// snapshot with placeholder assets and export it" (§21) — menu diagnostics, the real duel
/// bootstrapped through the real native library, and a static render of terrain/blocks/players.
/// Movement, aiming, and firing are C5 (§20.5 step 5 is explicitly outside this gate).
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

    private static readonly Color BackgroundColor = new(0.08f, 0.09f, 0.12f);
    private static readonly Color MenuTextColor = Colors.White;
    private static readonly Color TerrainSoilColor = new(0.45f, 0.32f, 0.18f);
    private static readonly Color TerrainWoodColor = new(0.55f, 0.42f, 0.20f);
    private static readonly Color TerrainStoneColor = new(0.55f, 0.55f, 0.58f);
    private static readonly Color BlockColor = new(0.30f, 0.55f, 0.30f);
    private static readonly Color BlockDamagedColor = new(0.65f, 0.35f, 0.20f);
    private static readonly Color PlayerAColor = new(0.25f, 0.55f, 0.95f);
    private static readonly Color PlayerBColor = new(0.90f, 0.35f, 0.30f);

    private BuildDiagnostics? _diagnostics;
    private MatchBootstrapResult? _match;
    private string? _menuError;
    private bool _started;

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

        QueueRedraw();
    }

    /// <inheritdoc />
    public override void _UnhandledInput(InputEvent @event)
    {
        ArgumentNullException.ThrowIfNull(@event);

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

        if (_match is null)
        {
            DrawMenu();
        }
        else
        {
            DrawMatch(_match.Frame);
        }
    }

    /// <inheritdoc />
    public override void _ExitTree()
    {
        // The one thing every path through this scene must still get right on the way out: no
        // native handle survives the process. Idempotent disposal (C3) means this is safe even
        // if smoke mode already disposed the session itself.
        _match?.Session.Dispose();
        base._ExitTree();
    }

    private void StartDuel()
    {
        try
        {
            _match = FixtureMatchBootstrapper.Start();
            _menuError = null;
        }
        catch (Exception exception) when (exception is InvalidDataException or NativeSimulationException)
        {
            // A failed bootstrap is diagnostic information for the menu, not a crash: the whole
            // point of showing build/version diagnostics first is to make a mismatch legible
            // instead of a silent black screen.
            _menuError = exception.Message;
        }

        _started = _match is not null;
        QueueRedraw();
    }

    private void DrawMenu()
    {
        var font = ThemeDB.FallbackFont;
        const int fontSize = 20;
        var position = new Vector2(24, 40);

        DrawString(font, position, "DUNGEON BARRAGE — C4 render spike", fontSize: fontSize, modulate: MenuTextColor);
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

        if (_menuError is not null)
        {
            position.Y += 32;
            DrawString(font, position, $"Bootstrap failed: {_menuError}", fontSize: 16, modulate: Colors.OrangeRed);
        }
    }

    private void DrawMatch(SnapshotFrame frame)
    {
        var snapshot = frame.Snapshot;
        DrawTerrain(snapshot, frame.Terrain);
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
            $"{player.CharacterId} {player.Health}/{player.MaxHealth}",
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
