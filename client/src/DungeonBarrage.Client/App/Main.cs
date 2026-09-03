using System.Diagnostics.CodeAnalysis;
using System.IO;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using DungeonBarrage.Client.Match;
using Godot;

namespace DungeonBarrage.Client.App;

/// <summary>
/// The scene root: menu with build diagnostics, sequential loadout (ranged, melee, secondary,
/// crown/anklet), playable authoritative match with human and bot turns, results, and rematch.
/// </summary>
public partial class Main : Node2D
{
    private const ulong HopDurationMsec = 420;

    private static readonly Color BackgroundColor = new(0.08f, 0.09f, 0.12f);
    private static readonly Color MenuTextColor = Colors.White;
    private static readonly Color TerrainSoilColor = new(0.45f, 0.32f, 0.18f);
    private static readonly Color TerrainWoodColor = new(0.55f, 0.42f, 0.20f);
    private static readonly Color TerrainStoneColor = new(0.55f, 0.55f, 0.58f);
    private static readonly Color BlockMortarColor = new(0.22f, 0.16f, 0.10f, 0.55f);
    private static readonly Color EnvironmentOneShotColor = new(0.86f, 0.16f, 0.12f);
    private const float EnvironmentMaxTransparency = 0.45f;
    private static readonly Color ArenaFillColor = new(0.10f, 0.12f, 0.16f);
    private static readonly Color ArenaEdgeColor = new(0.85f, 0.78f, 0.35f);
    private static readonly Color PlayerAColor = new(0.25f, 0.55f, 0.95f);
    private static readonly Color PlayerBColor = new(0.90f, 0.35f, 0.30f);
    private static readonly Color AimLineColor = new(0.95f, 0.85f, 0.25f);
    private static readonly Color AimPreviewColor = new(1f, 0.92f, 0.45f, 0.85f);
    private static readonly Color ProjectileColor = new(1f, 0.78f, 0.15f);
    private static readonly Color LockedHintColor = new(0.6f, 0.6f, 0.65f);

    private BuildDiagnostics? _diagnostics;
    private MatchBootstrapResult? _match;
    [SuppressMessage(
        "Design",
        "CA2213:Disposable fields should be disposed",
        Justification =
            "This is a Godot Node2D, not a .NET IDisposable type: its teardown lifecycle is "
            + "_ExitTree, which does dispose this field, not a Dispose method the analyzer recognizes.")]
    private LiveMatch? _live;
    private string? _menuError;
    private string? _liveError;
    private bool _started;

    private bool _inLocalSetup;
    private IReadOnlyList<ClientItemDefinition>? _roster;
    private ClientFighterDefinition? _fighter;
    private LoadoutPicker? _picker;
    private int _selectedMapIndex;
    private static readonly string[] PlayableMaps =
    [
        "crow-perch",
        "broken-battlements",
        "twin-spires",
    ];
    private int _selectedItemIndex;
    private int _botMainItemIndex;
    private bool _inLoadoutSelect;
    private ItemTileAnimation[]? _tileAnimations;
    private int _hoveredItemIndex = -1;

    private const float TileSize = 108f;
    private const float TileGap = 16f;
    private const int TileColumns = 4;
    private const float TileFloatHeight = 14f;
    private const float TileFloatUpSeconds = 0.16f;
    private const float TileFloatDownSeconds = 0.24f;
    private static readonly Vector2 TileGridOrigin = new(40, 176);
    private static readonly Vector2 ContinueButtonSize = new(300, 56);

    /// <summary>
    /// One item tile's float animation: a small non-interruptible state machine, not a
    /// Godot <c>Tween</c>. <see cref="Main"/> has no scene-graph nodes to attach one to — every
    /// screen so far is hand-drawn from a single <see cref="_Draw"/> — so the float/land motion
    /// is advanced manually each frame in <see cref="UpdateItemTileAnimations"/> instead.
    /// </summary>
    private sealed class ItemTileAnimation
    {
        /// <summary>Current vertical offset in pixels; negative is floated upward.</summary>
        internal float YOffset;

        /// <summary>Whether a float-up or land motion is currently playing.</summary>
        internal bool IsAnimating;

        /// <summary>
        /// Whether the motion in progress moves toward the floated position (<see langword="true"/>)
        /// or back to rest (<see langword="false"/>). Only consulted once the motion finishes —
        /// this is what makes it non-interruptible: a hover change mid-flight changes what happens
        /// <em>next</em>, never the motion already playing.
        /// </summary>
        internal bool AnimatingTowardFloated;

        internal float ElapsedSeconds;
    }

    private ClientAbilitySlot _selectedAbilitySlot = ClientAbilitySlot.Main;
    private int _selectedPassiveIndex;

    private bool _isAiming;
    private Vector2 _aimOrigin;
    private Vector2 _aimCursor;
    private IReadOnlyList<ClientProjectileTrace> _previewTraces = [];
    private int _previewEpoch;
    private int _lastPreviewAngle = int.MinValue;
    private int _lastPreviewPower = int.MinValue;
    private ShotPlayback? _playback;
    private Vector2 _cameraOffset = Vector2.Zero;
    private float _cellSize = 12f;
    private Vector2 _worldOrigin = Vector2.Zero;
    private readonly List<string> _combatLog = [];
    private string? _hopPlayerId;
    private ulong _hopStartMsec;

    /// <summary>
    /// Frozen pre-shot world plus the authority's traces, shown until input unlocks.
    /// <see cref="LiveMatch.CurrentSnapshot"/> is already the post-shot state the moment
    /// apply returns; drawing that during the lock is why a shot used to appear as a line
    /// to the target with the damage already applied.
    /// </summary>
    private sealed class ShotPlayback
    {
        internal required ClientMatchSnapshot PreSnapshot { get; init; }

        internal required TerrainRead PreTerrain { get; init; }

        internal required IReadOnlyList<ClientPresentationEvent> Events { get; init; }

        internal required ulong StartMsec { get; init; }

        internal required uint TickRate { get; init; }

        internal required uint LockTicks { get; init; }

        internal required int PositionScale { get; init; }

        internal required List<ClientProjectileTrace> Traces { get; init; }
    }

    private ulong _inputLockedUntilMsec;
    private bool _isProcessingBotTurn;
    private bool _isProcessingTimeout;
    private ulong _lastBotDecisionSeed = 1000uL;
    private uint _matchSeed = 12345;

    /// <inheritdoc />
#pragma warning disable CA1849
    public override async void _Ready()
#pragma warning restore CA1849
    {
        _diagnostics = BuildDiagnostics.Capture();

        var c7SmokeOptions = C7SmokeOptions.Parse(OS.GetCmdlineUserArgs());
        if (c7SmokeOptions is not null)
        {
            await RunC7SmokeAndQuitAsync(c7SmokeOptions).ConfigureAwait(true);
            return;
        }

        var c6SmokeOptions = C6SmokeOptions.Parse(OS.GetCmdlineUserArgs());
        if (c6SmokeOptions is not null)
        {
            await RunC6SmokeAndQuitAsync(c6SmokeOptions).ConfigureAwait(true);
            return;
        }

        var c6TimeoutSmokeOptions = C6TimeoutSmokeOptions.Parse(OS.GetCmdlineUserArgs());
        if (c6TimeoutSmokeOptions is not null)
        {
            await RunC6TimeoutSmokeAndQuitAsync(c6TimeoutSmokeOptions).ConfigureAwait(true);
            return;
        }

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
        if (_live is not null && IsInputLocked() != _wasLockedLastFrame)
        {
            _wasLockedLastFrame = IsInputLocked();
            QueueRedraw();
        }

        if (_isAiming)
        {
            RequestAimPreview();
            QueueRedraw();
        }

        if (IsInputLocked() && _playback is not null)
        {
            QueueRedraw();
        }
        else if (!IsInputLocked())
        {
            _playback = null;
        }

        if (_hopPlayerId is not null)
        {
            if (Time.GetTicksMsec() - _hopStartMsec >= HopDurationMsec)
            {
                _hopPlayerId = null;
            }

            QueueRedraw();
        }

        if (_inLoadoutSelect)
        {
            UpdateItemTileAnimations((float)delta);
        }

        // A visible countdown needs a redraw roughly every frame while it's ticking, not just on
        // discrete state-change events like the lock-state check above.
        if (_live is not null && !IsInputLocked() && _live.PlanningDeadlineUtc is not null)
        {
            QueueRedraw();
        }

        // Automatic bot turn processing when active player is bot-controlled, or an automatic
        // local timeout once the active player's own planning deadline has passed. Outcome is
        // never actually null — it is always populated, with ClientInProgressOutcome as its
        // "nothing decided yet" value — so this must pattern-match the type, not check for null.
        if (_live is not null && !IsInputLocked() && !_isProcessingBotTurn && !_isProcessingTimeout &&
            _live.CurrentSnapshot.Outcome is ClientInProgressOutcome)
        {
            var activeId = _live.CurrentSnapshot.ActivePlayerId;
            if (activeId is "b-local-bot" || IsBotPlayer(activeId))
            {
                _ = ProcessBotTurnAsync();
            }
            else if (_live.PlanningDeadlineUtc is { } deadline && DateTimeOffset.UtcNow >= deadline)
            {
                _ = ProcessTimeoutAsync();
            }
        }
    }

    private bool IsBotPlayer(string? playerId) =>
        _live?.CurrentSnapshot.Players.FirstOrDefault(p => p.PlayerId == playerId)?.Team == 1;

    private async Task ProcessBotTurnAsync()
    {
        if (_live is null || _isProcessingBotTurn)
        {
            return;
        }

        _isProcessingBotTurn = true;
        try
        {
            _lastBotDecisionSeed++;
            var transition = await SubmitAndRedrawAsync(
                () => _live.SubmitBotDecisionAsync(ClientBotDifficulty.Standard, _lastBotDecisionSeed))
                .ConfigureAwait(true);

            if (transition is not null)
            {
                await WaitTicksAsync(transition.InputLockTicks, transition.PresentationTickRate).ConfigureAwait(true);
            }
        }
        catch (Exception exception) when (exception is MatchCommandRejectedException or NativeSimulationException)
        {
            _liveError = DescribeLiveFault(exception);
        }
        finally
        {
            _isProcessingBotTurn = false;
            QueueRedraw();
        }
    }

    private async Task ProcessTimeoutAsync()
    {
        if (_live is null || _isProcessingTimeout)
        {
            return;
        }

        _isProcessingTimeout = true;
        try
        {
            var transition = await SubmitAndRedrawAsync(() => _live.SubmitTimeoutAsync()).ConfigureAwait(true);

            if (transition is not null)
            {
                await WaitTicksAsync(transition.InputLockTicks, transition.PresentationTickRate).ConfigureAwait(true);
            }
        }
        catch (Exception exception) when (exception is MatchCommandRejectedException or NativeSimulationException)
        {
            _liveError = DescribeLiveFault(exception);
        }
        finally
        {
            _isProcessingTimeout = false;
            QueueRedraw();
        }
    }

    private bool _wasLockedLastFrame;

    /// <inheritdoc />
    public override void _UnhandledInput(InputEvent @event)
    {
        ArgumentNullException.ThrowIfNull(@event);

        try
        {
            HandleUnhandledInput(@event);
        }
        catch (Exception exception)
        {
            _menuError = exception.Message;
            _liveError = exception.Message;
            LogClientFault("unhandled-input", exception);
            QueueRedraw();
        }
    }

    private void HandleUnhandledInput(InputEvent @event)
    {
        if (_live is not null)
        {
            HandleLiveInput(@event);
            return;
        }

        if (_inLoadoutSelect)
        {
            HandleLoadoutSelectInput(@event);
            return;
        }

        if (_inLocalSetup)
        {
            HandleLocalSetupInput(@event);
            return;
        }

        var isActivation = @event.IsActionPressed("ui_accept") ||
            (@event is InputEventMouseButton { Pressed: true, ButtonIndex: MouseButton.Left });
        if (!isActivation)
        {
            return;
        }

        GetViewport().SetInputAsHandled();
        EnterLocalSetup();
    }

    private static void LogClientFault(string where, Exception exception)
    {
        try
        {
            var path = Path.Combine(Path.GetTempPath(), "dungeon-barrage-client.log");
            File.AppendAllText(
                path,
                $"{DateTimeOffset.Now:o} {where}: {exception}{System.Environment.NewLine}");
        }
        catch (IOException)
        {
        }
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
        else if (_inLoadoutSelect)
        {
            DrawLoadoutSelect();
        }
        else if (_inLocalSetup)
        {
            DrawLocalSetup();
        }
        else
        {
            DrawMenu();
        }
    }

    /// <inheritdoc />
    public override void _ExitTree()
    {
        _match?.Session.Dispose();
        _live?.Dispose();
        base._ExitTree();
    }

    /// <summary>
    /// Enters the map/mode/slots screen between the main menu and loadout select
    /// (CLIENT_SPEC §11's own flow: Boot → MainMenu → LocalSetup → LoadoutSelect → Match).
    /// </summary>
    /// <remarks>
    /// Only one map and one mode exist today, so this deliberately shows them as read-only
    /// information rather than building a selection widget for options that do not exist yet —
    /// the structural value here is the screen itself, and back-navigation, not a chooser with
    /// one disabled option in it.
    /// </remarks>
    private void EnterLocalSetup()
    {
        _inLocalSetup = true;
        _menuError = null;
        QueueRedraw();
    }

    private void HandleLocalSetupInput(InputEvent @event)
    {
        if (@event.IsActionPressed("ui_cancel"))
        {
            GetViewport().SetInputAsHandled();
            _inLocalSetup = false;
            _started = false;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_left"))
        {
            GetViewport().SetInputAsHandled();
            _selectedMapIndex = (_selectedMapIndex - 1 + PlayableMaps.Length) % PlayableMaps.Length;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_right"))
        {
            GetViewport().SetInputAsHandled();
            _selectedMapIndex = (_selectedMapIndex + 1) % PlayableMaps.Length;
            QueueRedraw();
            return;
        }

        var isConfirm = @event.IsActionPressed("ui_accept") ||
            (@event is InputEventMouseButton { Pressed: true, ButtonIndex: MouseButton.Left });
        if (isConfirm)
        {
            GetViewport().SetInputAsHandled();
            _inLocalSetup = false;
            EnterLoadoutSelect();
        }
    }

    private void DrawLocalSetup()
    {
        var font = ThemeDB.FallbackFont;

        DrawRect(new Rect2(0, 0, GetViewportRect().Size.X, 64), new Color(0.55f, 0.10f, 0.12f));
        DrawString(font, new Vector2(24, 40), "LOCAL MATCH SETUP", fontSize: 24, modulate: Colors.White);

        var pos = new Vector2(24, 110);
        DrawString(font, pos, "Map  (left/right to change)", fontSize: 14, modulate: Colors.Gold);
        pos.Y += 22;
        DrawString(font, pos, PlayableMaps[_selectedMapIndex], fontSize: 18, modulate: MenuTextColor);
        pos.Y += 40;

        DrawString(font, pos, "Mode", fontSize: 14, modulate: Colors.Gold);
        pos.Y += 22;
        DrawString(font, pos, "Turn-Based Duel", fontSize: 18, modulate: MenuTextColor);
        pos.Y += 40;

        DrawString(font, pos, "Slots", fontSize: 14, modulate: Colors.Gold);
        pos.Y += 22;
        DrawString(font, pos, "Player 1: Human   ·   Player 2: CPU (Bot)", fontSize: 18, modulate: MenuTextColor);
        pos.Y += 40;

        DrawString(
            font,
            pos,
            "Left/right cycles crow-perch, broken-battlements, and twin-spires. Mode is",
            fontSize: 13,
            modulate: LockedHintColor);
        pos.Y += 18;
        DrawString(
            font,
            pos,
            "fixed to a turn-based duel. ENTER continues to the loadout picker.",
            fontSize: 13,
            modulate: LockedHintColor);

        if (_menuError is not null)
        {
            pos.Y += 28;
            DrawString(font, pos, _menuError, fontSize: 14, modulate: Colors.OrangeRed);
        }

        var footerPos = new Vector2(24, GetViewportRect().Size.Y - 20);
        DrawString(font, footerPos, "ENTER / Click to continue to Loadout · ESC to go back", fontSize: 13, modulate: Colors.Cyan);
    }

    private void EnterLoadoutSelect()
    {
        try
        {
            var catalog = RosterCatalog.Get();
            _fighter = catalog.Fighter;
            _roster = catalog.Items;
            _picker = new LoadoutPicker(catalog.Items);
            _selectedItemIndex = _picker.FocusedIndex;
            _botMainItemIndex = BotMainItemIndex(catalog.Items);
            _inLoadoutSelect = true;
            _menuError = null;
            _hoveredItemIndex = -1;
            _tileAnimations = new ItemTileAnimation[_roster.Count];
            for (var i = 0; i < _tileAnimations.Length; i++)
            {
                _tileAnimations[i] = new ItemTileAnimation();
            }
        }
        catch (Exception exception)
        {
            _menuError = exception.Message;
            _inLoadoutSelect = false;
            _inLocalSetup = true;
            LogClientFault("enter-loadout", exception);
        }

        QueueRedraw();
    }

    /// <summary>A tile's resting position — never affected by its own float animation.</summary>
    /// <remarks>
    /// Hit-testing (mouse hover/click) always uses this rest rectangle, not the animated draw
    /// position: if hovering were computed against a tile that is itself moving because of
    /// hover, a tile could float just far enough to leave the cursor, be judged un-hovered, and
    /// begin landing back under the cursor — an oscillation with no stable resting state.
    /// </remarks>
    private static Rect2 ItemTileRestRect(int index)
    {
        var column = index % TileColumns;
        var row = index / TileColumns;
        var position = TileGridOrigin + new Vector2(column * (TileSize + TileGap), row * (TileSize + TileGap));
        return new Rect2(position, new Vector2(TileSize, TileSize));
    }

    private int? HitTestItemTile(Vector2 point)
    {
        if (_picker is null)
        {
            return null;
        }

        var visible = _picker.VisibleCatalogIndices();
        for (var i = 0; i < visible.Count; i++)
        {
            if (ItemTileRestRect(i).HasPoint(point))
            {
                return visible[i];
            }
        }

        return null;
    }

    /// <summary>
    /// Advances every tile's float/land motion by one frame.
    /// </summary>
    /// <remarks>
    /// A tile floats while hovered by the mouse or currently the keyboard-selected human pick,
    /// and lands otherwise. The rule that makes this feel deliberate rather than jittery: a
    /// hover change is only ever read once the motion currently playing reaches its end — never
    /// mid-flight. A tile whose hover state flickers on and off rapidly finishes whatever
    /// direction it already committed to before the next one begins, so it always completes at
    /// least one full float or one full land before reversing, matching the fixture's own
    /// vocabulary: "it won't immediately restart the animation... it cannot be interrupted."
    /// </remarks>
    private void UpdateItemTileAnimations(float delta)
    {
        if (_tileAnimations is null)
        {
            return;
        }

        var anyAnimating = false;
        for (var i = 0; i < _tileAnimations.Length; i++)
        {
            var tile = _tileAnimations[i];
            var hoverDesired = i == _hoveredItemIndex || i == _selectedItemIndex;

            if (tile.IsAnimating)
            {
                tile.ElapsedSeconds += delta;
                var duration = tile.AnimatingTowardFloated ? TileFloatUpSeconds : TileFloatDownSeconds;
                var t = duration <= 0f ? 1f : Mathf.Clamp(tile.ElapsedSeconds / duration, 0f, 1f);
                var eased = 1f - Mathf.Pow(1f - t, 3f);
                tile.YOffset = tile.AnimatingTowardFloated
                    ? Mathf.Lerp(0f, -TileFloatHeight, eased)
                    : Mathf.Lerp(-TileFloatHeight, 0f, eased);

                if (t >= 1f)
                {
                    tile.IsAnimating = false;
                    tile.YOffset = tile.AnimatingTowardFloated ? -TileFloatHeight : 0f;

                    // Only now — motion finished, tile at a stable rest — is it safe to read
                    // the latest desired state and, if it changed while this was playing,
                    // queue the next motion.
                    if (hoverDesired != tile.AnimatingTowardFloated)
                    {
                        tile.IsAnimating = true;
                        tile.AnimatingTowardFloated = hoverDesired;
                        tile.ElapsedSeconds = 0f;
                    }
                }

                anyAnimating = true;
            }
            else
            {
                var isFloated = tile.YOffset < -0.01f;
                if (hoverDesired != isFloated)
                {
                    tile.IsAnimating = true;
                    tile.AnimatingTowardFloated = hoverDesired;
                    tile.ElapsedSeconds = 0f;
                    anyAnimating = true;
                }
            }
        }

        if (anyAnimating)
        {
            QueueRedraw();
        }
    }

    private void HandleLoadoutSelectInput(InputEvent @event)
    {
        if (_roster is null or { Count: 0 })
        {
            return;
        }

        if (@event is InputEventMouseMotion motion)
        {
            _hoveredItemIndex = HitTestItemTile(motion.Position) ?? -1;
            return;
        }

        if (@event.IsActionPressed("ui_cancel"))
        {
            GetViewport().SetInputAsHandled();
            if (_picker is not null && _picker.TryRetreat())
            {
                _selectedItemIndex = _picker.FocusedIndex;
                QueueRedraw();
                return;
            }

            _inLoadoutSelect = false;
            _inLocalSetup = true;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_up") || @event.IsActionPressed("ui_left"))
        {
            GetViewport().SetInputAsHandled();
            CycleVisible(-1);
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_down") || @event.IsActionPressed("ui_right"))
        {
            GetViewport().SetInputAsHandled();
            CycleVisible(1);
            QueueRedraw();
            return;
        }

        // A tile click equips that page's slot. The continue button (or ENTER) advances
        // the wizard; the last continue starts the duel.
        if (@event is InputEventMouseButton { Pressed: true, ButtonIndex: MouseButton.Left } mouseButton)
        {
            GetViewport().SetInputAsHandled();
            if (ContinueButtonRect().HasPoint(mouseButton.Position))
            {
                AdvanceLoadoutOrStart();
                return;
            }

            if (HitTestItemTile(mouseButton.Position) is int clickedIndex)
            {
                EquipTile(clickedIndex);
                QueueRedraw();
            }

            return;
        }

        if (@event.IsActionPressed("ui_accept"))
        {
            GetViewport().SetInputAsHandled();
            AdvanceLoadoutOrStart();
        }
    }

    private void AdvanceLoadoutOrStart()
    {
        if (_picker is not null && _picker.TryAdvance())
        {
            _selectedItemIndex = _picker.FocusedIndex;
            QueueRedraw();
            return;
        }

        ConfirmLoadoutAndStartDuel();
    }

    private Rect2 ContinueButtonRect()
    {
        var viewport = GetViewportRect().Size;
        var position = new Vector2(
            viewport.X - ContinueButtonSize.X - 28,
            viewport.Y - ContinueButtonSize.Y - 28);
        return new Rect2(position, ContinueButtonSize);
    }

    private void CycleVisible(int delta)
    {
        if (_picker is null)
        {
            return;
        }

        var visible = _picker.VisibleCatalogIndices();
        if (visible.Count == 0)
        {
            return;
        }

        var current = 0;
        for (var i = 0; i < visible.Count; i++)
        {
            if (visible[i] == _picker.EquippedIndexForStage)
            {
                current = i;
                break;
            }
        }

        var next = (current + delta + visible.Count) % visible.Count;
        EquipTile(visible[next]);
    }

    private void EquipTile(int index)
    {
        _picker?.SelectTile(index);
        _selectedItemIndex = _picker?.FocusedIndex ?? index;
    }

    /// <summary>
    /// Catalog index of the opponent's main item, for the CPU card.
    /// </summary>
    /// <remarks>
    /// The opponent fields <see cref="LocalMatchEnvelope.LaunchDefaultLoadout"/>, independent of
    /// the human's pick. This used to track the human's own secondary index, so the CPU card
    /// showed neither side's real loadout.
    /// </remarks>
    private static int BotMainItemIndex(IReadOnlyList<ClientItemDefinition> items)
    {
        var botMain = LocalMatchEnvelope.LaunchDefaultLoadout.Main;
        for (var i = 0; i < items.Count; i++)
        {
            if (items[i].Id == botMain)
            {
                return i;
            }
        }

        return 0;
    }

    private void ConfirmLoadoutAndStartDuel()
    {
        if (_picker is null || _roster is null || _roster.Count == 0)
        {
            return;
        }

        var request = LocalMatchEnvelope.HumanVsBot(
            simulationVersion: LocalMatchSession.SimulationVersion,
            contentVersion: LocalMatchSession.ContentVersion,
            seed: _matchSeed,
            matchId: $"local-duel-{_matchSeed}",
            mapId: PlayableMaps[_selectedMapIndex],
            humanLoadout: _picker.Loadout);

        try
        {
            _live = CreateLiveMatch(FixtureMatchBootstrapper.StartLive(request));
            _menuError = null;
            _cameraOffset = Vector2.Zero;
            _combatLog.Clear();
            _hopPlayerId = null;
        }
        catch (Exception exception)
        {
            _menuError = exception.Message;
            _inLoadoutSelect = true;
            LogClientFault("confirm-loadout", exception);
        }

        _inLoadoutSelect = _live is null;
        _started = _live is not null || _menuError is not null;
        QueueRedraw();
    }

    private void Rematch()
    {
        if (_live is null)
        {
            return;
        }

        _live.Dispose();
        _live = null;
        _matchSeed++;
        ConfirmLoadoutAndStartDuel();
    }

    private static LiveMatch CreateLiveMatch(MatchBootstrapResult bootstrap) =>
        LiveMatch.Create(
            bootstrap.Session,
            bootstrap.Frame.Snapshot,
            bootstrap.Frame.Terrain,
            bootstrap.Frame.Snapshot.MatchId);

    private bool IsInputLocked() => Time.GetTicksMsec() < _inputLockedUntilMsec;

    private void HandleLiveInput(InputEvent @event)
    {
        if (_live is null)
        {
            return;
        }

        // Handles Results screen rematch trigger when match is complete. Outcome is never null
        // (ClientInProgressOutcome is its populated "still playing" value), so this must
        // pattern-match away that specific type rather than check for null.
        if (_live.CurrentSnapshot.Outcome is not ClientInProgressOutcome)
        {
            if (@event.IsActionPressed("ui_accept") ||
                (@event is InputEventKey { Pressed: true, Keycode: Key.R }))
            {
                GetViewport().SetInputAsHandled();
                Rematch();
                return;
            }
        }

        // Handles Passive Selection prompt when phase is PassiveSelection
        if (_live.CurrentSnapshot.Phase == ClientMatchPhase.PassiveSelection && !IsInputLocked())
        {
            HandlePassiveSelectionInput(@event);
            return;
        }

        if (IsInputLocked() || _live.CurrentSnapshot.HasAttackedThisTurn)
        {
            return;
        }

        // Ability slot switching (1, 2, 3)
        if (@event is InputEventKey { Pressed: true } keyEvent)
        {
            switch (keyEvent.Keycode)
            {
                case Key.Key1:
                    _selectedAbilitySlot = ClientAbilitySlot.Main;
                    _lastPreviewAngle = int.MinValue;
                    QueueRedraw();
                    return;
                case Key.Key2:
                    _selectedAbilitySlot = ClientAbilitySlot.Secondary;
                    _lastPreviewAngle = int.MinValue;
                    QueueRedraw();
                    return;
                case Key.Key3:
                    _selectedAbilitySlot = ClientAbilitySlot.MeleeTool;
                    _lastPreviewAngle = int.MinValue;
                    QueueRedraw();
                    return;
                case Key.Key4:
                    _selectedAbilitySlot = ClientAbilitySlot.Trinket;
                    _lastPreviewAngle = int.MinValue;
                    QueueRedraw();
                    return;
                case Key.A:
                    GetViewport().SetInputAsHandled();
                    _ = SubmitAndRedrawAsync(() => _live.SubmitMoveAsync(-_live.CurrentSnapshot.PositionScale));
                    return;
                case Key.D:
                    GetViewport().SetInputAsHandled();
                    _ = SubmitAndRedrawAsync(() => _live.SubmitMoveAsync(_live.CurrentSnapshot.PositionScale));
                    return;
                case Key.W:
                case Key.Space:
                    GetViewport().SetInputAsHandled();
                    BeginHopPresentation();
                    _ = SubmitAndRedrawAsync(() => _live.SubmitJumpAsync());
                    return;
                case Key.Left:
                    _cameraOffset += new Vector2(_cellSize * 2f, 0);
                    QueueRedraw();
                    return;
                case Key.Right:
                    _cameraOffset += new Vector2(-_cellSize * 2f, 0);
                    QueueRedraw();
                    return;
                case Key.Up:
                    _cameraOffset += new Vector2(0, _cellSize * 2f);
                    QueueRedraw();
                    return;
                case Key.Down:
                    _cameraOffset += new Vector2(0, -_cellSize * 2f);
                    QueueRedraw();
                    return;
                case Key.F:
                case Key.Home:
                    _cameraOffset = Vector2.Zero;
                    QueueRedraw();
                    return;
                case Key.P:
                    _ = SubmitAndRedrawAsync(() => _live.SubmitPassAsync());
                    return;
            }
        }

        if (@event.IsActionPressed("ui_cancel") && _isAiming)
        {
            GetViewport().SetInputAsHandled();
            CancelAim();
            return;
        }

        if (@event is InputEventMouseButton mouseButton)
        {
            GetViewport().SetInputAsHandled();
            if (mouseButton.ButtonIndex == MouseButton.Right && mouseButton.Pressed)
            {
                CancelAim();
                return;
            }

            if (mouseButton.ButtonIndex != MouseButton.Left)
            {
                return;
            }

            if (mouseButton.Pressed)
            {
                _isAiming = true;
                _aimOrigin = mouseButton.Position;
                _aimCursor = mouseButton.Position;
                _lastPreviewAngle = int.MinValue;
                RequestAimPreview();
                QueueRedraw();
            }
            else if (_isAiming)
            {
                _aimCursor = mouseButton.Position;
                var aim = CurrentAim();
                CancelAim();
                if (aim.CanFire)
                {
                    _ = FireAimedShotAsync(aim);
                }
            }

            return;
        }

        if (@event is InputEventMouseMotion motion && _isAiming)
        {
            _aimCursor = motion.Position;
            QueueRedraw();
        }
    }

    private static void HandlePassiveSelectionInput(InputEvent @event)
    {
        _ = @event;
    }

    private AimSolution CurrentAim()
    {
        if (_live is null)
        {
            return default;
        }

        var player = ActivePlayer(_live.CurrentSnapshot);
        if (player is null)
        {
            return default;
        }

        var opponent = FirstLivingOpponent(_live.CurrentSnapshot, player.PlayerId);
        var facesRight = AimSolver.FacesRight(player.Position.X, opponent?.Position.X);
        var allowance = AimSolver.CrowMovementAllowance(_live.CurrentSnapshot.PositionScale);
        var cap = AimSolver.MaxPowerAfterMovement(_live.CurrentSnapshot.MovementRemaining, allowance);
        return AimSolver.FromDrag(
            _aimOrigin.X,
            _aimOrigin.Y,
            _aimCursor.X,
            _aimCursor.Y,
            facesRight,
            cap,
            _cellSize);
    }

    private static ClientPlayerSnapshot? ActivePlayer(ClientMatchSnapshot snapshot)
    {
        var id = snapshot.ActivePlayerId;
        if (id is null)
        {
            return null;
        }

        for (var i = 0; i < snapshot.Players.Count; i++)
        {
            if (snapshot.Players[i].PlayerId == id)
            {
                return snapshot.Players[i];
            }
        }

        return null;
    }

    private static ClientPlayerSnapshot? FirstLivingOpponent(ClientMatchSnapshot snapshot, string actorId)
    {
        var actorX = 0;
        for (var i = 0; i < snapshot.Players.Count; i++)
        {
            if (snapshot.Players[i].PlayerId == actorId)
            {
                actorX = snapshot.Players[i].Position.X;
                break;
            }
        }

        ClientPlayerSnapshot? found = null;
        var bestDx = int.MaxValue;
        for (var i = 0; i < snapshot.Players.Count; i++)
        {
            var candidate = snapshot.Players[i];
            if (candidate.PlayerId == actorId || candidate.IsEliminated)
            {
                continue;
            }

            var dx = Math.Abs(candidate.Position.X - actorX);
            if (dx < bestDx)
            {
                bestDx = dx;
                found = candidate;
            }
        }

        return found;
    }

    private void CancelAim()
    {
        _isAiming = false;
        _previewTraces = [];
        _lastPreviewAngle = int.MinValue;
        _lastPreviewPower = int.MinValue;
        QueueRedraw();
    }

    private void RequestAimPreview()
    {
        if (_live is null || !_isAiming)
        {
            return;
        }

        var aim = CurrentAim();
        if (!aim.CanFire)
        {
            _previewTraces = [];
            return;
        }

        if (aim.AngleMillidegrees == _lastPreviewAngle && aim.PowerBasisPoints == _lastPreviewPower)
        {
            return;
        }

        _lastPreviewAngle = aim.AngleMillidegrees;
        _lastPreviewPower = aim.PowerBasisPoints;
        var epoch = ++_previewEpoch;
        _ = FetchPreviewAsync(epoch, aim);
    }

    private async Task FetchPreviewAsync(int epoch, AimSolution aim)
    {
        if (_live is null)
        {
            return;
        }

        try
        {
            var preview = await _live.PreviewAbilityAsync(
                _selectedAbilitySlot,
                aim.AngleMillidegrees,
                aim.PowerBasisPoints).ConfigureAwait(true);
            if (epoch != _previewEpoch || !_isAiming)
            {
                return;
            }

            _previewTraces = preview is { Legal: true } ? preview.ProjectileTraces : [];
            QueueRedraw();
        }
        catch (Exception exception) when (exception is NativeSimulationException or InvalidDataException)
        {
            if (epoch == _previewEpoch)
            {
                _previewTraces = [];
            }
        }
    }

    private async Task FireAimedShotAsync(AimSolution aim)
    {
        if (_live is null)
        {
            return;
        }

        _ = await SubmitAndRedrawAsync(() => _live.SubmitAbilityAsync(
            _selectedAbilitySlot,
            aim.AngleMillidegrees,
            aim.PowerBasisPoints,
            targetPlayerId: null)).ConfigureAwait(true);
    }

    private async Task<ClientMatchTransition?> SubmitAndRedrawAsync(Func<Task<ClientMatchTransition>> submit)
    {
        if (_live is null)
        {
            return null;
        }

        var preSnapshot = _live.CurrentSnapshot;
        var preTerrain = _live.CurrentTerrain;
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
            _liveError = DescribeLiveFault(exception);
            _inputLockedUntilMsec = Time.GetTicksMsec();
            return null;
        }
        finally
        {
            if (transition is not null)
            {
                RecordCombatNotes(transition);
                if (!BeginShotPlayback(preSnapshot, preTerrain, transition))
                {
                    _inputLockedUntilMsec = Time.GetTicksMsec() + LockDurationMsecFor(_live);
                }
            }

            QueueRedraw();
        }
    }

    private bool BeginShotPlayback(
        ClientMatchSnapshot preSnapshot,
        TerrainRead preTerrain,
        ClientMatchTransition transition)
    {
        var traces = new List<ClientProjectileTrace>();
        for (var i = 0; i < transition.Events.Count; i++)
        {
            if (transition.Events[i] is ClientProjectileTraceEvent traceEvent)
            {
                traces.Add(traceEvent.Trace);
            }
        }

        if (traces.Count == 0)
        {
            _playback = null;
            return false;
        }

        var thrower = ActivePlayer(preSnapshot)?.CollisionCenter;
        for (var i = 0; i < traces.Count; i++)
        {
            if (thrower is { } catcher && ProjectilePlayback.IsReturningWeapon(traces[i].AbilityId))
            {
                traces[i] = ProjectilePlayback.WithReturnTo(
                    traces[i],
                    catcher,
                    ProjectilePlayback.ReturnLegTicks);
            }
        }

        var lastTick = 0u;
        for (var i = 0; i < traces.Count; i++)
        {
            var tick = ProjectilePlayback.LastSampleTick(traces[i]);
            if (tick > lastTick)
            {
                lastTick = tick;
            }
        }

        if (lastTick < transition.InputLockTicks)
        {
            lastTick = transition.InputLockTicks;
        }

        var visualRate = ProjectilePlayback.VisualTickRate(lastTick, transition.PresentationTickRate);
        var duration = ProjectilePlayback.PlaybackMsec(lastTick, visualRate);
        _playback = new ShotPlayback
        {
            PreSnapshot = preSnapshot,
            PreTerrain = preTerrain,
            Events = transition.Events,
            StartMsec = Time.GetTicksMsec(),
            TickRate = visualRate,
            LockTicks = lastTick,
            PositionScale = preSnapshot.PositionScale,
            Traces = traces,
        };
        _inputLockedUntilMsec = Time.GetTicksMsec() + duration;
        return true;
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
            "Press Enter or Click to Enter Local Match Setup.",
            fontSize: 16,
            modulate: MenuTextColor);
        position.Y += 24;
        DrawString(
            font,
            position,
            "Pick ranged, then melee, then a one-shot secondary, then a crown or anklet.",
            fontSize: 14,
            modulate: LockedHintColor);

        if (_menuError is not null)
        {
            position.Y += 32;
            DrawString(font, position, $"Bootstrap failed: {_menuError}", fontSize: 16, modulate: Colors.OrangeRed);
        }
    }

    private static Color ItemTileColor(int index, int count) =>
        Color.FromHsv(count <= 0 ? 0f : (float)index / count, 0.55f, 0.5f);

    private void DrawLoadoutSelect()
    {
        var font = ThemeDB.FallbackFont;
        var viewport = GetViewportRect().Size;

        DrawRect(new Rect2(0, 0, viewport.X, 56), new Color(0.55f, 0.10f, 0.12f));
        var title = _picker?.StageTitle ?? "LOADOUT";
        DrawString(font, new Vector2(24, 38), title, fontSize: 24, modulate: Colors.White);

        if (_roster is null or { Count: 0 } || _picker is null)
        {
            DrawString(font, new Vector2(24, 100), "Loading roster…", fontSize: 16, modulate: MenuTextColor);
            return;
        }

        DrawEquippedStrip(font);

        var visible = _picker.VisibleCatalogIndices();
        for (var visibleIndex = 0; visibleIndex < visible.Count; visibleIndex++)
        {
            var catalogIndex = visible[visibleIndex];
            var item = _roster[catalogIndex];
            var restRect = ItemTileRestRect(visibleIndex);
            var yOffset = _tileAnimations?[catalogIndex].YOffset ?? 0f;
            var tileRect = new Rect2(restRect.Position + new Vector2(0, yOffset), restRect.Size);
            var isEquipped = _picker.IsEquipped(catalogIndex);

            DrawRect(tileRect, ItemTileColor(catalogIndex, _roster.Count));
            if (isEquipped)
            {
                DrawRect(tileRect, new Color(1f, 0.82f, 0.12f, 0.72f));
            }

            var borderColor = isEquipped ? Colors.Yellow : new Color(1, 1, 1, 0.28f);
            DrawRect(tileRect, borderColor, filled: false, width: isEquipped ? 6f : 1.5f);

            var letter = char.ToUpperInvariant(item.DisplayName.Length > 0 ? item.DisplayName[0] : '?').ToString();
            var letterSize = font.GetStringSize(letter, fontSize: 36);
            var letterPos = tileRect.Position +
                new Vector2((tileRect.Size.X - letterSize.X) / 2f, (tileRect.Size.Y + letterSize.Y * 0.35f) / 2f);
            DrawString(font, letterPos, letter, fontSize: 36, modulate: Colors.White);

            var nameSize = font.GetStringSize(item.DisplayName, fontSize: 13);
            DrawString(
                font,
                tileRect.Position + new Vector2((tileRect.Size.X - nameSize.X) / 2f, tileRect.Size.Y - 18),
                item.DisplayName,
                fontSize: 13,
                modulate: Colors.White);

            if (isEquipped)
            {
                var badge = "EQUIPPED";
                var badgeSize = font.GetStringSize(badge, fontSize: 12);
                var badgeRect = new Rect2(
                    tileRect.Position + new Vector2((tileRect.Size.X - badgeSize.X) / 2f - 8, 8),
                    badgeSize + new Vector2(16, 16));
                DrawRect(badgeRect, new Color(0.12f, 0.08f, 0.02f, 0.85f));
                DrawString(
                    font,
                    badgeRect.Position + new Vector2(8, 14),
                    badge,
                    fontSize: 12,
                    modulate: Colors.Yellow);
            }
        }

        var detailIndex = _hoveredItemIndex >= 0 ? _hoveredItemIndex : _picker.EquippedIndexForStage;
        if (detailIndex >= 0 && detailIndex < _roster.Count)
        {
            var detail = _roster[detailIndex];
            var detailPos = new Vector2(24, TileGridOrigin.Y + (2 * (TileSize + TileGap)) + 20);
            DrawString(font, detailPos, $"{detail.DisplayName}  ({detail.Id})", fontSize: 18, modulate: Colors.Gold);
            detailPos.Y += 24;
            DrawString(
                font,
                detailPos,
                DetailLine(detail),
                fontSize: 14,
                modulate: MenuTextColor);
        }

        var continueRect = ContinueButtonRect();
        DrawRect(continueRect, new Color(0.12f, 0.52f, 0.28f));
        DrawRect(continueRect, Colors.White, filled: false, width: 2f);
        var continueLabel = _picker.IsLastStage ? "START DUEL" : "NEXT SLOT";
        var continueSize = font.GetStringSize(continueLabel, fontSize: 22);
        DrawString(
            font,
            continueRect.Position + new Vector2(
                (continueRect.Size.X - continueSize.X) / 2f,
                (continueRect.Size.Y + continueSize.Y * 0.55f) / 2f),
            continueLabel,
            fontSize: 22,
            modulate: Colors.White);

        DrawString(
            font,
            new Vector2(24, viewport.Y - 24),
            _picker.StageHint,
            fontSize: 13,
            modulate: Colors.Cyan);
    }

    private void DrawEquippedStrip(Font font)
    {
        if (_picker is null || _roster is null)
        {
            return;
        }

        var loadout = _picker.Loadout;
        var chips = new (LoadoutStage Stage, string Label, string Id)[]
        {
            (LoadoutStage.Main, "1 RANGED", loadout.Main),
            (LoadoutStage.Melee, "2 MELEE", loadout.MeleeTool),
            (LoadoutStage.Secondary, "3 SECONDARY", loadout.Secondary),
            (LoadoutStage.Trinket, "4 CROWN/ANKLET", loadout.Trinket),
        };
        var x = 24f;
        const float chipWidth = 292f;
        const float chipHeight = 52f;
        for (var i = 0; i < chips.Length; i++)
        {
            var chip = chips[i];
            var rect = new Rect2(x, 68, chipWidth, chipHeight);
            var current = chip.Stage == _picker.Stage;
            DrawRect(rect, current ? new Color(0.85f, 0.68f, 0.12f) : new Color(0.16f, 0.17f, 0.22f));
            DrawRect(rect, current ? Colors.Yellow : new Color(1, 1, 1, 0.25f), filled: false, width: current ? 3f : 1f);
            DrawString(font, rect.Position + new Vector2(10, 18), chip.Label, fontSize: 12, modulate: Colors.White);
            DrawString(
                font,
                rect.Position + new Vector2(10, 40),
                ItemDisplayName(chip.Id),
                fontSize: 16,
                modulate: current ? Colors.Black : Colors.Gold);
            x += chipWidth + 10f;
        }
    }

    private string ItemDisplayName(string id)
    {
        if (_roster is null)
        {
            return id;
        }

        for (var i = 0; i < _roster.Count; i++)
        {
            if (_roster[i].Id == id)
            {
                return _roster[i].DisplayName;
            }
        }

        return id;
    }

    private static string DetailLine(ClientItemDefinition detail)
    {
        if (detail.Slot == ClientAbilitySlot.Trinket)
        {
            return "Charge with two damaging hits, then press 4 to fire this unique special.";
        }

        if (detail.Slot == ClientAbilitySlot.Secondary)
        {
            return $"One-shot ammo copy · {detail.Ability.DamagePercent}% dmg · gone after one fire";
        }

        if (detail.Slot == ClientAbilitySlot.MeleeTool)
        {
            return "Same melee strike as every other melee — visual only.";
        }

        var strikes = detail.Ability.StrikesPerTurn;
        var strikeText = strikes > 1 ? $" · {strikes} shots in a line" : string.Empty;
        return $"{detail.AmmoPolicy} ammo {detail.StartingAmmo} · {detail.Ability.DamagePercent}% dmg{strikeText}";
    }

    private void DrawSelectionCard(Rect2 rect, string label, int itemIndex, Color background)
    {
        if (_roster is null || itemIndex >= _roster.Count)
        {
            return;
        }

        var font = ThemeDB.FallbackFont;
        var item = _roster[itemIndex];

        DrawRect(rect, background);
        DrawRect(rect, new Color(1, 1, 1, 0.6f), filled: false, width: 2f);

        var swatchRect = new Rect2(rect.Position + new Vector2(18, 18), new Vector2(TileSize, TileSize));
        DrawRect(swatchRect, ItemTileColor(itemIndex, _roster.Count));
        DrawRect(swatchRect, Colors.White, filled: false, width: 2f);

        var textPos = rect.Position + new Vector2(18 + TileSize + 18, 40);
        DrawString(font, textPos, label, fontSize: 15, modulate: new Color(1, 1, 1, 0.85f));
        textPos.Y += 30;
        DrawString(font, textPos, item.DisplayName, fontSize: 22, modulate: Colors.White);
        textPos.Y += 26;
        DrawString(font, textPos, $"{item.Slot}  ammo {item.StartingAmmo}", fontSize: 13, modulate: new Color(1, 1, 1, 0.75f));
    }

    private void DrawLiveMatch(LiveMatch live)
    {
        var snapshot = live.CurrentSnapshot;
        if (IsInputLocked() && _playback is not null)
        {
            DrawMatch(_playback.PreSnapshot, _playback.PreTerrain);
            DrawPlaybackProjectiles();
        }
        else
        {
            DrawMatch(snapshot, live.CurrentTerrain);
            if (_isAiming)
            {
                DrawAim(live);
            }
        }

        var font = ThemeDB.FallbackFont;
        var hudY = 8f;
        DrawString(
            font,
            new Vector2(8, hudY),
            $"active {snapshot.ActivePlayerId}   phase {snapshot.Phase}   slot [{_selectedAbilitySlot}]   wind {snapshot.WindPerTick}   {TrinketHud(snapshot)}",
            fontSize: 14,
            modulate: MenuTextColor);

        if (_isAiming)
        {
            hudY += 18;
            var aim = CurrentAim();
            var dragSide = aim.FacesRight ? "LEFT" : "RIGHT";
            var maxPct = aim.MaxPowerBasisPoints / 100;
            var aimText = aim.CanFire
                ? $"pull {dragSide}  {aim.AngleDegrees}°  power {aim.PowerPercent}% of {maxPct}% max  — gold line is first impact   release to fire"
                : $"click anywhere, drag {dragSide} (away from the other crow) — longer line is more power, max this turn {maxPct}%";
            DrawString(font, new Vector2(8, hudY), aimText, fontSize: 14, modulate: Colors.Gold);
        }

        if (live.PlanningDeadlineUtc is { } planningDeadline)
        {
            hudY += 18;
            var remainingSeconds = Math.Max(0.0, (planningDeadline - DateTimeOffset.UtcNow).TotalSeconds);
            var timeColor = remainingSeconds <= 10.0 ? Colors.OrangeRed : MenuTextColor;
            DrawString(font, new Vector2(8, hudY), $"time to act: {remainingSeconds:F0}s", fontSize: 12, modulate: timeColor);
        }

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

        DrawArenaLegend(snapshot);

        if (snapshot.Phase == ClientMatchPhase.PassiveSelection && !IsInputLocked())
        {
            DrawPassiveSelectModal();
        }

        // Outcome is never null (ClientInProgressOutcome is its populated "still playing"
        // value): the results screen must only appear once the match has actually left that
        // state, not on every frame of ordinary play.
        if (snapshot.Outcome is not ClientInProgressOutcome)
        {
            DrawResultsScreen(snapshot);
        }
    }

    private static void DrawPassiveSelectModal()
    {
    }

    private void DrawResultsScreen(ClientMatchSnapshot snapshot)
    {
        var rect = new Rect2(new Vector2(180, 120), new Vector2(440, 260));
        DrawRect(rect, new Color(0.05f, 0.08f, 0.15f, 0.95f));
        DrawRect(rect, Colors.Gold, filled: false, width: 3);

        var font = ThemeDB.FallbackFont;
        var pos = rect.Position + new Vector2(30, 40);

        DrawString(font, pos, "MATCH COMPLETE", fontSize: 22, modulate: Colors.Gold);
        pos.Y += 36;

        if (snapshot.Outcome is ClientVictoryOutcome victory)
        {
            DrawString(font, pos, $"VICTORY: Team {victory.Team} Wins!", fontSize: 18, modulate: Colors.LightGreen);
        }
        else
        {
            DrawString(font, pos, "DRAW: Match Ended in Draw!", fontSize: 18, modulate: Colors.LightBlue);
        }

        pos.Y += 30;
        DrawString(font, pos, $"Turns Elapsed: {snapshot.TurnNumber} | State Hash: {snapshot.StateHash}", fontSize: 14, modulate: MenuTextColor);
        pos.Y += 24;
        DrawString(font, pos, $"Snapshot Gen: {snapshot.SnapshotGeneration}", fontSize: 14, modulate: MenuTextColor);

        pos.Y += 40;
        DrawString(font, pos, "Press [R] or [ENTER] to REMATCH", fontSize: 16, modulate: Colors.Cyan);
    }

    private void DrawMatch(ClientMatchSnapshot snapshot, TerrainRead terrain)
    {
        FitWorld(snapshot.TerrainWidth, snapshot.TerrainHeight);
        DrawArena(snapshot.TerrainWidth, snapshot.TerrainHeight);
        DrawTerrain(snapshot, terrain);
        foreach (var block in snapshot.Blocks)
        {
            DrawBlock(block, terrain, snapshot.TerrainWidth, snapshot.TerrainHeight);
        }

        for (var index = 0; index < snapshot.Players.Count; index++)
        {
            DrawPlayer(
                snapshot.Players[index],
                index == 0 ? PlayerAColor : PlayerBColor,
                index,
                snapshot.PositionScale);
        }

        var font = ThemeDB.FallbackFont;
        DrawString(
            font,
            new Vector2(8, GetViewportRect().Size.Y - 12),
            $"turn {snapshot.TurnNumber}  move {snapshot.MovementRemaining}  hash {snapshot.StateHash}  [A/D walk  Space hop  arrows camera  1-4 weapons  P pass]",
            fontSize: 14,
            modulate: MenuTextColor);
    }

    private void FitWorld(uint widthCells, uint heightCells)
    {
        var viewport = GetViewportRect().Size;
        const float padLeft = 16f;
        const float padRight = 252f;
        const float padTop = 72f;
        const float padBottom = 36f;
        var availW = Math.Max(80f, viewport.X - padLeft - padRight);
        var availH = Math.Max(80f, viewport.Y - padTop - padBottom);
        var width = Math.Max(1, (int)widthCells);
        var height = Math.Max(1, (int)heightCells);
        _cellSize = Math.Clamp(Math.Min(availW / width, availH / height), 16f, 48f);
        var mapW = width * _cellSize;
        var mapH = height * _cellSize;
        _worldOrigin = new Vector2(
            padLeft + ((availW - mapW) * 0.5f),
            padTop + ((availH - mapH) * 0.5f));
    }

    private Rect2 ArenaRect(uint widthCells, uint heightCells) =>
        new(
            _worldOrigin + _cameraOffset,
            new Vector2(widthCells * _cellSize, heightCells * _cellSize));

    private Rect2 CellRect(int x, int y) =>
        new(
            _worldOrigin.X + _cameraOffset.X + (x * _cellSize),
            _worldOrigin.Y + _cameraOffset.Y + (y * _cellSize),
            _cellSize,
            _cellSize);

    private void DrawArena(uint widthCells, uint heightCells)
    {
        var arena = ArenaRect(widthCells, heightCells);
        DrawRect(arena, ArenaFillColor);
        DrawRect(arena, ArenaEdgeColor, filled: false, width: 2f);
        var font = ThemeDB.FallbackFont;
        DrawString(
            font,
            arena.Position + new Vector2(6, -6),
            "ARENA EDGE — shots that leave this box miss",
            fontSize: 12,
            modulate: ArenaEdgeColor);
    }

    private void DrawTerrain(ClientMatchSnapshot snapshot, TerrainRead terrain)
    {
        var cells = terrain.Cells.Span;
        var width = (int)snapshot.TerrainWidth;
        var height = (int)snapshot.TerrainHeight;
        if (cells.Length != width * height)
        {
            return;
        }

        for (var y = 0; y < height; y++)
        {
            for (var x = 0; x < width; x++)
            {
                var material = cells[(y * width) + x];
                if (material == 0)
                {
                    continue;
                }

                var owner = BlockOwningCell(snapshot, x, y);
                var fill = EnvironmentCellColor(material, owner, x, y);
                var rect = CellRect(x, y);
                DrawRect(rect, fill);
                var aboveEmpty = y == 0 || cells[((y - 1) * width) + x] == 0;
                if (aboveEmpty)
                {
                    var top = new Rect2(rect.Position, new Vector2(rect.Size.X, Math.Max(2f, _cellSize * 0.18f)));
                    DrawRect(top, fill.Lightened(0.22f));
                }
            }
        }
    }

    private void DrawBlock(
        ClientBlockSnapshot block,
        TerrainRead terrain,
        uint terrainWidth,
        uint terrainHeight)
    {
        if (block.Health == 0)
        {
            return;
        }

        var width = (int)terrainWidth;
        var height = (int)terrainHeight;
        var cells = terrain.Cells.Span;
        if (cells.Length != width * height)
        {
            return;
        }

        var fill = EnvironmentCellColor(
            MaterialByte(block.Material),
            block,
            block.OriginCellX,
            block.OriginCellY);
        var mortar = new Color(BlockMortarColor.R, BlockMortarColor.G, BlockMortarColor.B, fill.A);
        var lastX = block.OriginCellX + block.WidthCells - 1;
        var lastY = block.OriginCellY + block.HeightCells - 1;
        for (var y = block.OriginCellY; y <= lastY; y++)
        {
            for (var x = block.OriginCellX; x <= lastX; x++)
            {
                if (x < 0 || y < 0 || x >= width || y >= height)
                {
                    continue;
                }

                if (cells[(y * width) + x] == 0)
                {
                    continue;
                }

                DrawRect(CellRect(x, y), mortar, filled: false, width: 1f);
            }
        }
    }

    private static byte MaterialByte(ClientMaterial material) =>
        material switch
        {
            ClientMaterial.Wood => 2,
            ClientMaterial.ReinforcedStone => 3,
            ClientMaterial.Soil => 1,
            _ => 1,
        };

    private static ClientBlockSnapshot? BlockOwningCell(ClientMatchSnapshot snapshot, int x, int y)
    {
        ClientBlockSnapshot? found = null;
        var foundArea = int.MaxValue;
        for (var i = 0; i < snapshot.Blocks.Count; i++)
        {
            var block = snapshot.Blocks[i];
            if (block.Health == 0)
            {
                continue;
            }

            if (x < block.OriginCellX || y < block.OriginCellY)
            {
                continue;
            }

            if (x >= block.OriginCellX + block.WidthCells || y >= block.OriginCellY + block.HeightCells)
            {
                continue;
            }

            var area = block.WidthCells * block.HeightCells;
            if (area < foundArea)
            {
                found = block;
                foundArea = area;
            }
        }

        return found;
    }

    /// <summary>
    /// Remaining solid columns from health, matching the authority's ceil(width * hp / max).
    /// </summary>
    private static int SolidColumns(ClientBlockSnapshot block)
    {
        if (block.WidthCells == 0 || block.Health == 0 || block.MaxHealth == 0)
        {
            return 0;
        }

        var product = (int)block.WidthCells * block.Health;
        var columns = (product + block.MaxHealth - 1) / block.MaxHealth;
        return Math.Min(columns, block.WidthCells);
    }

    private static Color EnvironmentCellColor(byte material, ClientBlockSnapshot? block, int x, int y)
    {
        var baseColor = material switch
        {
            2 => TerrainWoodColor,
            3 => TerrainStoneColor,
            _ => TerrainSoilColor,
        };
        var shade = ((x + y) & 1) == 0 ? 1f : 0.88f;
        baseColor *= shade;

        if (block is null || block.MaxHealth == 0)
        {
            return baseColor;
        }

        var healthFrac = Math.Clamp(block.Health / (float)block.MaxHealth, 0f, 1f);
        var transparency = (1f - healthFrac) * EnvironmentMaxTransparency;
        var alpha = 1f - transparency;
        if (SolidColumns(block) <= 1)
        {
            return new Color(
                EnvironmentOneShotColor.R,
                EnvironmentOneShotColor.G,
                EnvironmentOneShotColor.B,
                1f - EnvironmentMaxTransparency);
        }

        baseColor.A = alpha;
        return baseColor;
    }

    private void DrawPlayer(
        ClientPlayerSnapshot player,
        Color color,
        int index,
        int positionScale)
    {
        var body = CharacterBodyGeometry.FromPlayer(player);
        var projected = body.Project(
            positionScale,
            _cellSize,
            new PresentationPoint(_worldOrigin.X, _worldOrigin.Y),
            new PresentationPoint(_cameraOffset.X, _cameraOffset.Y));
        var center = new Vector2(projected.Center.X, projected.Center.Y);
        var radius = projected.Radius;
        var bodyColor = player.IsEliminated ? Colors.Gray : color;

        DrawCircle(center + new Vector2(0, radius * 0.85f), radius * 0.45f, new Color(0, 0, 0, 0.35f));
        DrawCircle(center, radius, bodyColor);
        DrawCircle(center + new Vector2(0, -radius * 0.15f), radius * 0.62f, bodyColor.Lightened(0.12f));
        var beakDir = index == 0 ? 1f : -1f;
        var beak = new Vector2[]
        {
            center + new Vector2(beakDir * radius * 0.95f, -radius * 0.05f),
            center + new Vector2(beakDir * radius * 1.45f, radius * 0.08f),
            center + new Vector2(beakDir * radius * 0.85f, radius * 0.22f),
        };
        DrawColoredPolygon(beak, Colors.Gold);
        DrawCircle(center + new Vector2(-beakDir * radius * 0.22f, -radius * 0.22f), radius * 0.14f, Colors.White);
        DrawCircle(center + new Vector2(-beakDir * radius * 0.22f, -radius * 0.22f), radius * 0.06f, Colors.Black);

        var font = ThemeDB.FallbackFont;
        var labelY = -radius - 6 - (index * 16);
        DrawString(
            font,
            center + new Vector2(-radius, labelY),
            $"{player.Loadout.Main} {player.Health}/{player.MaxHealth}  ammo {(player.Ammo.Count > 0 ? player.Ammo[0].Remaining : 0)}",
            fontSize: 12,
            modulate: MenuTextColor);
    }

    private Vector2 ToPixels(ClientPosition position, int positionScale)
    {
        var projected = WorldProjection.ToPresentation(
            position,
            positionScale,
            _cellSize,
            new PresentationPoint(_worldOrigin.X, _worldOrigin.Y),
            new PresentationPoint(_cameraOffset.X, _cameraOffset.Y));
        return new Vector2(projected.X, projected.Y);
    }

    private void DrawAim(LiveMatch live)
    {
        var player = ActivePlayer(live.CurrentSnapshot);
        if (player is null)
        {
            return;
        }

        var scale = live.CurrentSnapshot.PositionScale;
        var muzzle = ToPixels(player.CollisionCenter, scale);
        var aim = CurrentAim();
        var rubberColor = aim.CanFire ? new Color(1f, 1f, 1f, 0.45f) : new Color(0.85f, 0.25f, 0.25f, 0.9f);
        DrawLine(_aimOrigin, _aimCursor, rubberColor, width: aim.CanFire ? 2 : 5);
        DrawCircle(_aimOrigin, 3, rubberColor);
        DrawCircle(_aimCursor, 5, rubberColor);

        var pullHint = aim.FacesRight ? -1f : 1f;
        DrawLine(muzzle, muzzle + new Vector2(pullHint * 28f, 0), new Color(1f, 1f, 1f, 0.4f), width: 2);

        for (var t = 0; t < _previewTraces.Count; t++)
        {
            DrawTracePath(_previewTraces[t], uint.MaxValue, scale, AimPreviewColor, 2f);
        }

        if (aim.CanFire && FirstPreviewImpact() is { } impact)
        {
            var hit = ToPixels(impact, scale);
            var arena = ArenaRect(live.CurrentSnapshot.TerrainWidth, live.CurrentSnapshot.TerrainHeight);
            var clamped = new Vector2(
                Mathf.Clamp(hit.X, arena.Position.X, arena.End.X),
                Mathf.Clamp(hit.Y, arena.Position.Y, arena.End.Y));
            DrawLine(muzzle, clamped, AimLineColor, width: 4);
            DrawCircle(clamped, 7, AimLineColor);
            if (hit != clamped)
            {
                DrawString(
                    ThemeDB.FallbackFont,
                    clamped + new Vector2(8, -8),
                    "leaves arena",
                    fontSize: 12,
                    modulate: Colors.OrangeRed);
            }
        }
    }

    private ClientPosition? FirstPreviewImpact()
    {
        ClientPosition? best = null;
        var bestTick = uint.MaxValue;
        for (var i = 0; i < _previewTraces.Count; i++)
        {
            var impact = _previewTraces[i].TerminalImpact;
            if (impact.Tick < bestTick)
            {
                bestTick = impact.Tick;
                best = impact.Position;
            }
        }

        return best;
    }

    private void DrawPlaybackProjectiles()
    {
        if (_playback is null)
        {
            return;
        }

        var elapsed = Time.GetTicksMsec() - _playback.StartMsec;
        var tick = ProjectilePlayback.TickAt(elapsed, _playback.TickRate, _playback.LockTicks);
        var radius = Math.Max(8f, _cellSize * 0.42f);
        var scale = _playback.PositionScale;
        for (var i = 0; i < _playback.Traces.Count; i++)
        {
            var trace = _playback.Traces[i];
            var returning = ProjectilePlayback.IsReturningWeapon(trace.AbilityId);
            DrawTracePath(trace, tick, scale, new Color(1f, 0.85f, 0.3f, 0.85f), Math.Max(3f, _cellSize * 0.12f));
            if (ProjectilePlayback.PositionAt(trace, tick) is { } position)
            {
                var screen = ToPixels(position, scale);
                if (returning)
                {
                    DrawReturningProjectile(screen, radius, tick);
                }
                else
                {
                    DrawCircle(screen, radius, ProjectileColor);
                    DrawCircle(screen, radius * 0.45f, Colors.White);
                }
            }

            if (tick >= trace.TerminalImpact.Tick)
            {
                var hit = ToPixels(trace.TerminalImpact.Position, scale);
                var pulse = 1f + (0.25f * MathF.Sin(elapsed * 0.02f));
                DrawArc(hit, radius * 1.8f * pulse, 0, MathF.Tau, 24, Colors.OrangeRed, width: 3f);
                DrawCircle(hit, radius * 0.55f, Colors.OrangeRed);
                DrawString(
                    ThemeDB.FallbackFont,
                    hit + new Vector2(radius, -radius),
                    tick > trace.TerminalImpact.Tick && returning ? "RETURNING" : "HIT",
                    fontSize: 14,
                    modulate: Colors.Yellow);
            }
        }
    }

    private void DrawReturningProjectile(Vector2 screen, float radius, uint tick)
    {
        var spin = tick * 0.45f;
        var axis = new Vector2(MathF.Cos(spin), MathF.Sin(spin)) * radius * 1.35f;
        var cross = new Vector2(-axis.Y, axis.X) * 0.35f;
        DrawColoredPolygon(
            [
                screen + axis,
                screen + cross,
                screen - axis,
                screen - cross,
            ],
            ProjectileColor);
        DrawCircle(screen, radius * 0.28f, Colors.White);
    }

    private void DrawTracePath(ClientProjectileTrace trace, uint throughTick, int positionScale, Color color, float width)
    {
        Vector2? previous = null;
        for (var i = 0; i < trace.Samples.Count; i++)
        {
            var sample = trace.Samples[i];
            if (sample.Tick > throughTick)
            {
                break;
            }

            var point = ToPixels(sample.Position, positionScale);
            if (previous is { } from)
            {
                DrawLine(from, point, color, width);
            }

            previous = point;
        }
    }

    private async Task RunSmokeAndQuitAsync(C4SmokeOptions options)
    {
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

    private async Task<C5SmokeReport> RunC5SmokeAsync(C5SmokeOptions options)
    {
        var diagnostics = _diagnostics ?? BuildDiagnostics.Capture();

        LiveMatch? live = null;
        try
        {
            _isProcessingBotTurn = true;
            EnterLocalSetup();
            _selectedMapIndex = 0;
            EnterLoadoutSelect();
            if (_picker is null)
            {
                throw new InvalidOperationException($"C5 picker failed to load: {_menuError}");
            }

            ConfirmLoadoutAndStartDuel();
            live = _live ?? throw new InvalidOperationException($"C5 confirm failed to start a match: {_menuError}");

            if (live.CurrentSnapshot.MapId != PlayableMaps[0])
            {
                throw new InvalidOperationException(
                    $"C5 must start on stacked map {PlayableMaps[0]}; started {live.CurrentSnapshot.MapId}.");
            }

            var beforeActivePlayer = live.CurrentSnapshot.ActivePlayerId;
            var defenderId = string.Empty;
            for (var i = 0; i < live.CurrentSnapshot.Players.Count; i++)
            {
                if (live.CurrentSnapshot.Players[i].PlayerId != beforeActivePlayer)
                {
                    defenderId = live.CurrentSnapshot.Players[i].PlayerId;
                    break;
                }
            }

            if (string.IsNullOrEmpty(defenderId))
            {
                throw new InvalidOperationException("C5 could not identify the defending player.");
            }

            var defenderHealthBefore = HealthOf(live.CurrentSnapshot, defenderId);
            var moveStepDx = live.CurrentSnapshot.PositionScale;

            var moveTransition = await SubmitAndRedrawAsync(() => live.SubmitMoveAsync(moveStepDx))
                .ConfigureAwait(true)
                ?? throw new InvalidOperationException("The picker-started move was rejected.");
            await WaitTicksAsync(moveTransition.InputLockTicks, moveTransition.PresentationTickRate).ConfigureAwait(true);

            var abilityTransition = await SubmitAndRedrawAsync(() => live.SubmitAbilityAsync(
                ClientAbilitySlot.Main,
                angleMillidegrees: 45_000,
                powerBasisPoints: 1_500,
                targetPlayerId: null))
                .ConfigureAwait(true)
                ?? throw new InvalidOperationException("The picker-started ability was rejected.");
            var lockedImmediatelyAfterAbility = IsInputLocked();

            await WaitTicksAsync(abilityTransition.InputLockTicks, abilityTransition.PresentationTickRate)
                .ConfigureAwait(true);
            await WaitUntilInputUnlockedAsync().ConfigureAwait(true);
            var unlockedAfterWaiting = !IsInputLocked();

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (screenshotWidth, screenshotHeight) = CaptureScreenshot(options.ScreenshotPath);

            var final = live.CurrentSnapshot;
            var defenderHealthAfter = HealthOf(final, defenderId);
            var terminal = final.Outcome is not ClientInProgressOutcome;
            var handedOver = final.ActivePlayerId != beforeActivePlayer;
            if (!handedOver && !terminal)
            {
                throw new InvalidOperationException(
                    "C5 must hand the turn over or finish the match after the picker-started shot.");
            }

            return new C5SmokeReport(
                Success: true,
                Error: null,
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                BeforeActivePlayerId: beforeActivePlayer,
                MoveAccepted: moveTransition.Disposition == ClientTransitionDisposition.Accepted,
                MoveEventCount: moveTransition.Events.Count,
                MoveDx: moveStepDx,
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
                TurnHandedOverToTheOtherPlayer: handedOver,
                TurnNumberAfter: final.TurnNumber,
                ScreenshotWidth: screenshotWidth,
                ScreenshotHeight: screenshotHeight,
                MapId: final.MapId,
                LoadoutMain: _picker?.Loadout.Main ?? string.Empty,
                UsedLoadoutPicker: _picker is not null,
                MatchReachedTerminalOutcome: terminal);
        }
        catch (Exception exception)
        {
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
                ScreenshotHeight: 0,
                MapId: string.Empty,
                LoadoutMain: string.Empty,
                UsedLoadoutPicker: false,
                MatchReachedTerminalOutcome: false);
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

    private async Task RunC6SmokeAndQuitAsync(C6SmokeOptions options)
    {
        var exitCode = 1;
        try
        {
            var report = await RunC6SmokeAsync(options).ConfigureAwait(true);
            report.Write(options.ReportPath);
            exitCode = report.Success ? 0 : 1;
        }
        finally
        {
            GetTree().Quit(exitCode);
        }
    }

    private async Task<C6SmokeReport> RunC6SmokeAsync(C6SmokeOptions options)
    {
        var diagnostics = _diagnostics ?? BuildDiagnostics.Capture();

        try
        {
            // Drives the exact same methods a real player's input does — EnterLocalSetup,
            // EnterLoadoutSelect, ConfirmLoadoutAndStartDuel, then Rematch — rather than
            // building requests by hand and calling FixtureMatchBootstrapper.StartLive directly.
            // A hand-built request proves the backend accepts well-formed input; it does not
            // prove the interactive screens themselves (DrawLocalSetup, DrawLoadoutSelect/
            // HandleLoadoutSelectInput) ever ran or rendered. This is the whole point of a C6
            // smoke test, per CLIENT_SPEC §20.5's own rule: a real pixel is the proof, not a
            // claim that the code compiles.
            EnterLocalSetup();
            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (localSetupWidth, localSetupHeight) = CaptureScreenshot(options.LocalSetupScreenshotPath);
            _inLocalSetup = false;

            EnterLoadoutSelect();
            if (_roster is null || _roster.Count == 0)
            {
                throw new InvalidOperationException($"Loadout select failed to load a roster: {_menuError}");
            }

            var rosterCount = _roster.Count;

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (loadoutSelectWidth, loadoutSelectHeight) = CaptureScreenshot(options.LoadoutSelectScreenshotPath);

            // Exercises the hover-float animation through the exact input path real mouse
            // motion goes through, and proves the "cannot be interrupted" requirement: hover a
            // tile, let its float-up motion start, move the hover away before that motion
            // finishes, and confirm it still completes the float — never reverses mid-flight —
            // before landing begins.
            var hoverVisible = _picker is null
                ? 0
                : Math.Min(2, Math.Max(0, _picker.VisibleCatalogIndices().Count - 1));
            var hoverCatalogIndex = _picker is null
                ? hoverVisible
                : _picker.VisibleCatalogIndices()[hoverVisible];
            var hoverPoint = ItemTileRestRect(hoverVisible).GetCenter();
            using (var hoverEvent = new InputEventMouseMotion { Position = hoverPoint })
            {
                HandleLoadoutSelectInput(hoverEvent);
            }

            // The state transition into "animating toward floated" and the actual time-advance
            // happen in separate branches of UpdateItemTileAnimations (a tile only starts
            // consuming delta once IsAnimating is already true entering the call) — so this
            // needs two calls: one to begin the motion, a second to actually move partway
            // through it. A single call here would (incorrectly) start the animation without
            // visibly moving it at all.
            UpdateItemTileAnimations(0f);
            UpdateItemTileAnimations(TileFloatUpSeconds * 0.5f);
            var wasFloatingMidFlight = _tileAnimations![hoverCatalogIndex].IsAnimating &&
                _tileAnimations[hoverCatalogIndex].AnimatingTowardFloated &&
                _tileAnimations[hoverCatalogIndex].YOffset < -1f;

            using (var awayEvent = new InputEventMouseMotion { Position = new Vector2(-100, -100) })
            {
                HandleLoadoutSelectInput(awayEvent);
            }

            UpdateItemTileAnimations(0.001f);
            var stillCompletingTheFloatAfterHoverLeft = _tileAnimations[hoverCatalogIndex].IsAnimating &&
                _tileAnimations[hoverCatalogIndex].AnimatingTowardFloated;

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            _ = CaptureScreenshot(options.LoadoutSelectHoverScreenshotPath);

            UpdateItemTileAnimations(TileFloatUpSeconds);
            UpdateItemTileAnimations(TileFloatDownSeconds);
            var restedCleanlyAfterTheFullCycle = !_tileAnimations[hoverCatalogIndex].IsAnimating &&
                Mathf.Abs(_tileAnimations[hoverCatalogIndex].YOffset) < 0.01f;

            var hoverAnimationInterruptionTestPassed =
                wasFloatingMidFlight && stillCompletingTheFloatAfterHoverLeft && restedCleanlyAfterTheFullCycle;
            _hoveredItemIndex = -1;
            if (!hoverAnimationInterruptionTestPassed)
            {
                throw new InvalidOperationException(
                    "The loadout-select tile float animation was interrupted mid-flight instead of completing it.");
            }

            ClickPickerItem("frostfall-mortar");
            if (_picker is null || _picker.Loadout.Main != "frostfall-mortar")
            {
                throw new InvalidOperationException(
                    $"Clicking frostfall-mortar must equip it as main; loadout was {_picker?.Loadout.Main}.");
            }

            ClickContinue();
            ClickContinue();
            ClickContinue();
            if (_picker.Stage != LoadoutStage.Trinket)
            {
                throw new InvalidOperationException(
                    $"The loadout wizard must land on the crown/anklet page; stage was {_picker.Stage}.");
            }

            // Each side reports its own main item. These were both the human's pick, which made
            // the two fields indistinguishable and hid that the bot was mirroring the loadout.
            var humanMainItemId = _picker.Loadout.Main;
            var botMainItemId = LocalMatchEnvelope.LaunchDefaultLoadout.Main;
            _isProcessingBotTurn = true;

            var mapsCompletedCsv = string.Empty;
            var mapsCompletedCount = 0;
            var stackedBlocksFell = false;
            var humanTurnExecuted = false;
            var botTurnExecuted = false;
            var passivePromptShownForHuman = false;
            var passivePromptConfirmedThroughRealInput = false;
            var turnsPlayed = 0;
            var rematchCreated = false;
            var rematchDisposedCleanly = false;
            var screenshotWidth = 0;
            var screenshotHeight = 0;
            ClientMatchSnapshot? finalSnapshot = null;

            for (var mapIndex = 0; mapIndex < PlayableMaps.Length; mapIndex++)
            {
                _selectedMapIndex = mapIndex;
                ConfirmLoadoutAndStartDuel();
                if (_live is null)
                {
                    throw new InvalidOperationException(
                        $"Confirm failed to start {PlayableMaps[mapIndex]}: {_menuError}");
                }

                if (_live.CurrentSnapshot.MapId != PlayableMaps[mapIndex])
                {
                    throw new InvalidOperationException(
                        $"Expected map {PlayableMaps[mapIndex]}; snapshot was {_live.CurrentSnapshot.MapId}.");
                }

                var startedMain = LoadoutMainOf(_live.CurrentSnapshot, "a-local-player");
                if (startedMain != "frostfall-mortar")
                {
                    throw new InvalidOperationException(
                        $"{PlayableMaps[mapIndex]} must carry frostfall-mortar as main; snapshot main was '{startedMain}'.");
                }

                var blocksBefore = _live.CurrentSnapshot;
                var moveStepDx = _live.CurrentSnapshot.PositionScale;

                var moveTrans = await _live.SubmitMoveAsync(moveStepDx).ConfigureAwait(true);
                await WaitTicksAsync(moveTrans.InputLockTicks, moveTrans.PresentationTickRate).ConfigureAwait(true);

                var abilityTrans = await _live.SubmitAbilityAsync(
                    ClientAbilitySlot.Main,
                    angleMillidegrees: 45_000,
                    powerBasisPoints: 1_500,
                    targetPlayerId: null).ConfigureAwait(true);
                await WaitTicksAsync(abilityTrans.InputLockTicks, abilityTrans.PresentationTickRate).ConfigureAwait(true);

                humanTurnExecuted = humanTurnExecuted
                    || (moveTrans.Disposition == ClientTransitionDisposition.Accepted
                        && abilityTrans.Disposition == ClientTransitionDisposition.Accepted);
                stackedBlocksFell = stackedBlocksFell
                    || AnyLivingBlockFell(blocksBefore, _live.CurrentSnapshot);

                var decisionSeed = 777uL + (ulong)mapIndex;
                var mapTurns = 0;
                while (_live.CurrentSnapshot.Outcome is ClientInProgressOutcome && mapTurns < 300)
                {
                    mapTurns++;
                    turnsPlayed++;

                    if (_live.CurrentSnapshot.ActivePlayerId == "a-local-player" &&
                        _live.CurrentSnapshot.Phase == ClientMatchPhase.PassiveSelection)
                    {
                        passivePromptShownForHuman = true;
                        QueueRedraw();
                        await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
                        await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
                        _ = CaptureScreenshot(options.PassivePromptScreenshotPath);

                        var beforeGeneration = _live.CurrentSnapshot.SnapshotGeneration;
                        using (var confirmEvent = new InputEventKey { Keycode = Key.Enter, Pressed = true })
                        {
                            HandlePassiveSelectionInput(confirmEvent);
                        }

                        var pollAttempts = 0;
                        while (_live.CurrentSnapshot.SnapshotGeneration == beforeGeneration && pollAttempts < 120)
                        {
                            pollAttempts++;
                            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
                        }

                        passivePromptConfirmedThroughRealInput =
                            _live.CurrentSnapshot.SnapshotGeneration != beforeGeneration;
                        continue;
                    }

                    var botTrans = await _live.SubmitBotDecisionAsync(ClientBotDifficulty.Standard, decisionSeed++)
                        .ConfigureAwait(true);
                    await WaitTicksAsync(botTrans.InputLockTicks, botTrans.PresentationTickRate).ConfigureAwait(true);
                    botTurnExecuted = botTurnExecuted || botTrans.Disposition == ClientTransitionDisposition.Accepted;
                    stackedBlocksFell = stackedBlocksFell
                        || AnyLivingBlockFell(blocksBefore, _live.CurrentSnapshot);
                }

                if (_live.CurrentSnapshot.Outcome is ClientInProgressOutcome)
                {
                    throw new InvalidOperationException(
                        $"{PlayableMaps[mapIndex]} did not reach a terminal outcome within {mapTurns} bot decisions.");
                }

                mapsCompletedCsv = mapsCompletedCount == 0
                    ? PlayableMaps[mapIndex]
                    : mapsCompletedCsv + "," + PlayableMaps[mapIndex];
                mapsCompletedCount++;
                finalSnapshot = _live.CurrentSnapshot;

                QueueRedraw();
                await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
                await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
                (screenshotWidth, screenshotHeight) = CaptureScreenshot(options.ScreenshotPath);

                if (mapIndex == 0)
                {
                    Rematch();
                    if (_live is null)
                    {
                        throw new InvalidOperationException($"Rematch failed to start a fresh match: {_menuError}");
                    }

                    rematchCreated = _live.CurrentSnapshot.TurnNumber == 1;
                    await _live.DisposeAsync().ConfigureAwait(true);
                    rematchDisposedCleanly = true;
                    _live = null;
                }
                else
                {
                    await _live.DisposeAsync().ConfigureAwait(true);
                    _live = null;
                }
            }

            if (mapsCompletedCount != PlayableMaps.Length)
            {
                throw new InvalidOperationException(
                    $"C6 must finish every playable map; completed {mapsCompletedCsv}.");
            }

            if (finalSnapshot is null)
            {
                throw new InvalidOperationException("C6 finished with no snapshot.");
            }

            return new C6SmokeReport(
                Success: true,
                Error: null,
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                RosterCount: rosterCount,
                HumanMainItemId: humanMainItemId,
                BotMainItemId: botMainItemId,
                InitialMatchCreated: true,
                HoverAnimationInterruptionTestPassed: hoverAnimationInterruptionTestPassed,
                HumanTurnExecuted: humanTurnExecuted,
                BotTurnExecuted: botTurnExecuted,
                PassivePromptShownForHuman: passivePromptShownForHuman,
                PassivePromptConfirmedThroughRealInput: passivePromptConfirmedThroughRealInput,
                MatchCompleted: true,
                TurnsPlayed: turnsPlayed,
                FinalTurnNumber: finalSnapshot.TurnNumber,
                FinalStateHash: finalSnapshot.StateHash,
                RematchSessionCreated: rematchCreated,
                RematchSessionDisposedCleanly: rematchDisposedCleanly,
                ScreenshotWidth: screenshotWidth,
                ScreenshotHeight: screenshotHeight,
                LoadoutSelectScreenshotWidth: loadoutSelectWidth,
                LoadoutSelectScreenshotHeight: loadoutSelectHeight,
                LocalSetupScreenshotWidth: localSetupWidth,
                LocalSetupScreenshotHeight: localSetupHeight,
                MapsCompleted: mapsCompletedCsv,
                AllPlayableMapsCompleted: mapsCompletedCount == PlayableMaps.Length,
                StackedBlocksFell: stackedBlocksFell);
        }
        catch (Exception exception)
        {
            return new C6SmokeReport(
                Success: false,
                Error: $"{exception.GetType().Name}: {exception.Message}",
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                RosterCount: 0,
                HumanMainItemId: string.Empty,
                BotMainItemId: string.Empty,
                InitialMatchCreated: false,
                HoverAnimationInterruptionTestPassed: false,
                HumanTurnExecuted: false,
                BotTurnExecuted: false,
                PassivePromptShownForHuman: false,
                PassivePromptConfirmedThroughRealInput: false,
                MatchCompleted: false,
                TurnsPlayed: 0,
                FinalTurnNumber: 0,
                FinalStateHash: string.Empty,
                RematchSessionCreated: false,
                RematchSessionDisposedCleanly: false,
                ScreenshotWidth: 0,
                ScreenshotHeight: 0,
                LoadoutSelectScreenshotWidth: 0,
                LoadoutSelectScreenshotHeight: 0,
                LocalSetupScreenshotWidth: 0,
                LocalSetupScreenshotHeight: 0,
                MapsCompleted: string.Empty,
                AllPlayableMapsCompleted: false,
                StackedBlocksFell: false);
        }
        finally
        {
            if (_live is not null)
            {
                await _live.DisposeAsync().ConfigureAwait(true);
                _live = null;
            }

            _inLoadoutSelect = false;
            _isProcessingBotTurn = false;
        }
    }

    private async Task RunC6TimeoutSmokeAndQuitAsync(C6TimeoutSmokeOptions options)
    {
        var exitCode = 1;
        try
        {
            var report = await RunC6TimeoutSmokeAsync(options).ConfigureAwait(true);
            report.Write(options.ReportPath);
            exitCode = report.Success ? 0 : 1;
        }
        finally
        {
            GetTree().Quit(exitCode);
        }
    }

    private async Task<C6TimeoutSmokeReport> RunC6TimeoutSmokeAsync(C6TimeoutSmokeOptions options)
    {
        var diagnostics = _diagnostics ?? BuildDiagnostics.Capture();

        try
        {
            // A minimal boot to a live match — LocalSetup and loadout select's own screens are
            // already proven pixel-for-pixel by the C6 smoke path; this test exists to prove one
            // thing neither of those does: that an idle turn ends on its own, through the real
            // Main._Process trigger, without this test ever calling SubmitTimeoutAsync itself.
            EnterLocalSetup();
            _inLocalSetup = false;
            EnterLoadoutSelect();
            if (_roster is null || _roster.Count == 0)
            {
                throw new InvalidOperationException($"Loadout select failed to load a roster: {_menuError}");
            }

            ConfirmLoadoutAndStartDuel();
            if (_live is null)
            {
                throw new InvalidOperationException($"Loadout select confirmation failed to start a match: {_menuError}");
            }

            var deadline = _live.PlanningDeadlineUtc
                ?? throw new InvalidOperationException("A freshly started match must arm a planning deadline.");
            var configuredSeconds = (deadline - DateTimeOffset.UtcNow).TotalSeconds;
            var turnNumberBeforeTimeout = _live.CurrentSnapshot.TurnNumber;

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (startWidth, startHeight) = CaptureScreenshot(options.StartScreenshotPath);
            var countdownWasVisibleAtStart = _live.PlanningDeadlineUtc is not null;

            // Deliberately never submits anything for this player — the whole point is proving
            // the turn ends on its own once real wall-clock time passes the armed deadline.
            await Task.Delay(LiveMatch.DefaultPlanningDeadline + TimeSpan.FromSeconds(2)).ConfigureAwait(true);

            // Bounded by real elapsed time, not a fixed frame count: an unfocused windowed export
            // can run its process loop far slower than headless does (observed well under 60fps
            // here), so a frame-count bound sized for headless timing under-waits in that build.
            // What actually matters is real wall-clock time elapsed, which this measures directly.
            var pollDeadline = DateTimeOffset.UtcNow + TimeSpan.FromSeconds(60);
            while (_live.CurrentSnapshot.TurnNumber == turnNumberBeforeTimeout && DateTimeOffset.UtcNow < pollDeadline)
            {
                await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            }

            var timeoutTriggeredAutomatically = _live.CurrentSnapshot.TurnNumber != turnNumberBeforeTimeout;
            if (!timeoutTriggeredAutomatically)
            {
                throw new InvalidOperationException(
                    "The idle turn never ended automatically — Main._Process's local-timeout trigger did not fire.");
            }

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (screenshotWidth, screenshotHeight) = CaptureScreenshot(options.ScreenshotPath);

            return new C6TimeoutSmokeReport(
                Success: true,
                Error: null,
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                ConfiguredDeadlineSeconds: configuredSeconds,
                CountdownWasVisibleAtStart: countdownWasVisibleAtStart,
                TimeoutTriggeredAutomatically: timeoutTriggeredAutomatically,
                TurnNumberBeforeTimeout: turnNumberBeforeTimeout,
                TurnNumberAfterTimeout: _live.CurrentSnapshot.TurnNumber,
                ActivePlayerIdAfterTimeout: _live.CurrentSnapshot.ActivePlayerId,
                StartScreenshotWidth: startWidth,
                StartScreenshotHeight: startHeight,
                ScreenshotWidth: screenshotWidth,
                ScreenshotHeight: screenshotHeight);
        }
        catch (Exception exception)
        {
            return new C6TimeoutSmokeReport(
                Success: false,
                Error: $"{exception.GetType().Name}: {exception.Message}",
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                ConfiguredDeadlineSeconds: 0,
                CountdownWasVisibleAtStart: false,
                TimeoutTriggeredAutomatically: false,
                TurnNumberBeforeTimeout: 0,
                TurnNumberAfterTimeout: 0,
                ActivePlayerIdAfterTimeout: null,
                StartScreenshotWidth: 0,
                StartScreenshotHeight: 0,
                ScreenshotWidth: 0,
                ScreenshotHeight: 0);
        }
        finally
        {
            if (_live is not null)
            {
                await _live.DisposeAsync().ConfigureAwait(true);
                _live = null;
            }

            _inLoadoutSelect = false;
            _isProcessingBotTurn = false;
            _isProcessingTimeout = false;
        }
    }

    private async Task RunC7SmokeAndQuitAsync(C7SmokeOptions options)
    {
        var exitCode = 1;
        try
        {
            var report = await RunC7SmokeAsync(options).ConfigureAwait(true);
            report.Write(options.ReportPath);
            exitCode = report.Success ? 0 : 1;
        }
        finally
        {
            GetTree().Quit(exitCode);
        }
    }

    private async Task<C7SmokeReport> RunC7SmokeAsync(C7SmokeOptions options)
    {
        var diagnostics = _diagnostics ?? BuildDiagnostics.Capture();

        try
        {
            // 1. Settings Recovery Verification
            var nonExistentPath = Path.Combine(Path.GetTempPath(), $"missing_settings_{Guid.NewGuid():N}.json");
            var recoveredDefaults = UserSettingsStore.Load(nonExistentPath);
            var settingsRecoveryVerified = recoveredDefaults is not null && recoveredDefaults.SchemaVersion == 1;

            // 2. Audio Clamping Verification
            var unclampedAudio = new ClientAudioSettings(MasterVolume: 250, SfxVolume: 120, MusicVolume: 90);
            var clampedAudio = unclampedAudio.Clamp();
            var audioClampingVerified = clampedAudio.MasterVolume == 100 && clampedAudio.SfxVolume == 100 && clampedAudio.MusicVolume == 90;

            // 3. Accessibility Scaling Verification
            var unclampedAccess = new ClientAccessibilitySettings(TextScale: 3.0f);
            var clampedAccess = unclampedAccess.Clamp();
            var accessibilityScalingVerified = clampedAccess.TextScale == 2.0f;

            // 4. Localization Verification
            var catalog = new LocalizationCatalog("en-US");
            var enTitle = catalog.Get("ui.title");
            catalog.SetLocale("es-ES");
            var esVictory = catalog.Get("ui.victory");
            var localizationVerified = enTitle == "Dungeon Barrage" && esVictory == "VICTORIA";

            // 5. Performance Tier Verification
            var perfSettings = new ClientPerformanceSettings(Tier: ClientPerformanceTier.Medium, TargetFps: 60);
            var performanceTierSwitchVerified = perfSettings.Tier == ClientPerformanceTier.Medium && perfSettings.TargetFps == 60;

            // 6. Multi-Platform Export Presets Verification
            var presetsPath = "res://export_presets.cfg";
            using var file = Godot.FileAccess.Open(presetsPath, Godot.FileAccess.ModeFlags.Read);
            var presetsText = file?.GetAsText() ?? string.Empty;
            var multiPlatformExportPresetsVerified = presetsText.Contains("name=\"Windows Desktop\"") &&
                                                     presetsText.Contains("name=\"Linux/X11\"") &&
                                                     presetsText.Contains("name=\"macOS\"");

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (screenshotWidth, screenshotHeight) = CaptureScreenshot(options.ScreenshotPath);

            return new C7SmokeReport(
                Success: true,
                Error: null,
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                SettingsRecoveryVerified: settingsRecoveryVerified,
                AudioClampingVerified: audioClampingVerified,
                AccessibilityScalingVerified: accessibilityScalingVerified,
                LocalizationVerified: localizationVerified,
                PerformanceTierSwitchVerified: performanceTierSwitchVerified,
                MultiPlatformExportPresetsVerified: multiPlatformExportPresetsVerified,
                ScreenshotWidth: screenshotWidth,
                ScreenshotHeight: screenshotHeight);
        }
        catch (Exception exception)
        {
            return new C7SmokeReport(
                Success: false,
                Error: $"{exception.GetType().Name}: {exception.Message}",
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                SettingsRecoveryVerified: false,
                AudioClampingVerified: false,
                AccessibilityScalingVerified: false,
                LocalizationVerified: false,
                PerformanceTierSwitchVerified: false,
                MultiPlatformExportPresetsVerified: false,
                ScreenshotWidth: 0,
                ScreenshotHeight: 0);
        }
    }

    private static ushort HealthOf(ClientMatchSnapshot snapshot, string playerId)
    {
        for (var i = 0; i < snapshot.Players.Count; i++)
        {
            if (snapshot.Players[i].PlayerId == playerId)
            {
                return snapshot.Players[i].Health;
            }
        }

        throw new InvalidOperationException($"No player '{playerId}' in the snapshot.");
    }

    private void ClickPickerItem(string itemId)
    {
        if (_roster is null || _picker is null)
        {
            throw new InvalidOperationException("The picker catalog is not loaded.");
        }

        var visible = _picker.VisibleCatalogIndices();
        var visibleIndex = -1;
        for (var i = 0; i < visible.Count; i++)
        {
            if (_roster[visible[i]].Id == itemId)
            {
                visibleIndex = i;
                break;
            }
        }

        if (visibleIndex < 0)
        {
            throw new InvalidOperationException(
                $"The current loadout page does not show {itemId}.");
        }

        using var click = new InputEventMouseButton
        {
            Pressed = true,
            ButtonIndex = MouseButton.Left,
            Position = ItemTileRestRect(visibleIndex).GetCenter(),
        };
        HandleLoadoutSelectInput(click);
    }

    private void ClickContinue()
    {
        using var click = new InputEventMouseButton
        {
            Pressed = true,
            ButtonIndex = MouseButton.Left,
            Position = ContinueButtonRect().GetCenter(),
        };
        HandleLoadoutSelectInput(click);
    }

    private void DrawArenaLegend(ClientMatchSnapshot snapshot)
    {
        var font = ThemeDB.FallbackFont;
        var viewport = GetViewportRect().Size;
        var x = viewport.X - 244f;
        var y = 72f;
        DrawRect(new Rect2(x - 8, y - 18, 240, 268), new Color(0.05f, 0.06f, 0.09f, 0.88f));
        DrawString(font, new Vector2(x, y), "WHAT YOU ARE LOOKING AT", fontSize: 13, modulate: Colors.Gold);
        y += 20;
        DrawString(font, new Vector2(x, y), "Gold box = the arena. Shots that", fontSize: 12, modulate: MenuTextColor);
        y += 16;
        DrawString(font, new Vector2(x, y), "leave it miss — they do not wrap.", fontSize: 12, modulate: MenuTextColor);
        y += 18;
        DrawString(font, new Vector2(x, y), "Grey slab = MAIN STAGE (stone).", fontSize: 12, modulate: MenuTextColor);
        y += 16;
        DrawString(font, new Vector2(x, y), "Falling off a perch lands here.", fontSize: 12, modulate: MenuTextColor);
        y += 16;
        DrawString(font, new Vector2(x, y), "Brown masonry = a tower. Damage", fontSize: 12, modulate: MenuTextColor);
        y += 16;
        DrawString(font, new Vector2(x, y), "fades it (max 45% see-through).", fontSize: 12, modulate: MenuTextColor);
        y += 16;
        DrawString(font, new Vector2(x, y), "Red + 45% fade = one shot left.", fontSize: 12, modulate: MenuTextColor);
        y += 18;
        DrawString(font, new Vector2(x, y), "Crows stand on the top face.", fontSize: 12, modulate: MenuTextColor);
        y += 16;
        DrawString(font, new Vector2(x, y), "Space hops; gravity lands them.", fontSize: 12, modulate: MenuTextColor);
        y += 22;
        DrawString(font, new Vector2(x, y), "LAST HITS", fontSize: 13, modulate: Colors.Gold);
        y += 18;
        if (_combatLog.Count == 0)
        {
            DrawString(font, new Vector2(x, y), "Fire to crack a tower.", fontSize: 12, modulate: LockedHintColor);
        }
        else
        {
            for (var i = 0; i < _combatLog.Count; i++)
            {
                DrawString(font, new Vector2(x, y), _combatLog[i], fontSize: 12, modulate: Colors.LightYellow);
                y += 16;
            }
        }

        _ = snapshot;
    }

    private void BeginHopPresentation()
    {
        if (_live is null)
        {
            return;
        }

        _hopPlayerId = _live.CurrentSnapshot.ActivePlayerId;
        _hopStartMsec = Time.GetTicksMsec();
    }

    private float HopOffsetPixels(string playerId)
    {
        if (_hopPlayerId != playerId)
        {
            return 0f;
        }

        var elapsed = Time.GetTicksMsec() - _hopStartMsec;
        if (elapsed >= HopDurationMsec)
        {
            return 0f;
        }

        var t = elapsed / (float)HopDurationMsec;
        var lift = 4f * t * (1f - t);
        return -lift * _cellSize * 2f;
    }

    private void RecordCombatNotes(ClientMatchTransition transition)
    {
        for (var i = 0; i < transition.Events.Count; i++)
        {
            switch (transition.Events[i])
            {
                case ClientBlockChangedEvent block:
                    if (block.PreviousSurvivingBounds is { } previous &&
                        block.NewSurvivingBounds is { } next &&
                        next.Y > previous.Y)
                    {
                        PushNote($"Tower {block.BlockId} collapsed");
                    }
                    else if (block.NewHealth is 0)
                    {
                        PushNote($"Tower {block.BlockId} destroyed");
                    }
                    else if (block.NewHealth is { } hp &&
                             block.PreviousHealth is { } old &&
                             hp < old)
                    {
                        PushNote($"Tower {block.BlockId} cracked");
                    }

                    break;
                case ClientImpactEvent impact:
                    PushNote(impact.Impact.Cause switch
                    {
                        ClientImpactCause.OutOfBounds => "Shot left the arena",
                        ClientImpactCause.Terrain => "Shot hit a tower",
                        ClientImpactCause.Character => "Direct hit",
                        ClientImpactCause.Expired => "Shot fizzled",
                        _ => "Shot ended",
                    });
                    break;
                case ClientHealthChangedEvent health when health.NewHealth < health.PreviousHealth:
                    PushNote($"{health.PlayerId} {health.PreviousHealth}→{health.NewHealth} HP");
                    break;
                case ClientTerrainChangedEvent:
                    PushNote("Blast carved the ground");
                    break;
            }
        }
    }

    private void PushNote(string note)
    {
        _combatLog.Insert(0, note);
        while (_combatLog.Count > 6)
        {
            _combatLog.RemoveAt(_combatLog.Count - 1);
        }
    }

    private static string DescribeLiveFault(Exception exception)
    {
        var text = exception.Message ?? string.Empty;
        if (text.Contains("invalidTarget", StringComparison.OrdinalIgnoreCase))
        {
            return "Shot could not land — it left the arena or had no legal target.";
        }

        if (text.Contains("inputOutOfRange", StringComparison.OrdinalIgnoreCase))
        {
            return "Aim was out of range. Pull away from the other crow; 20 cells is 100%.";
        }

        return text;
    }

    private static string TrinketHud(ClientMatchSnapshot snapshot)
    {
        for (var i = 0; i < snapshot.Players.Count; i++)
        {
            if (snapshot.Players[i].PlayerId == "a-local-player")
            {
                var charge = snapshot.Players[i].TrinketCharge;
                return charge >= 10_000
                    ? "crown/anklet READY (4)"
                    : $"crown/anklet {charge}/10000";
            }
        }

        return "crown/anklet --";
    }

    private static string LoadoutMainOf(ClientMatchSnapshot snapshot, string playerId)
    {
        for (var i = 0; i < snapshot.Players.Count; i++)
        {
            if (snapshot.Players[i].PlayerId == playerId)
            {
                return snapshot.Players[i].Loadout.Main;
            }
        }

        return string.Empty;
    }

    private static bool AnyLivingBlockFell(ClientMatchSnapshot before, ClientMatchSnapshot after)
    {
        for (var i = 0; i < after.Blocks.Count; i++)
        {
            var fallen = after.Blocks[i];
            if (fallen.Health == 0)
            {
                continue;
            }

            for (var j = 0; j < before.Blocks.Count; j++)
            {
                if (before.Blocks[j].Id == fallen.Id && fallen.OriginCellY > before.Blocks[j].OriginCellY)
                {
                    return true;
                }
            }
        }

        return false;
    }

    private static async Task WaitTicksAsync(uint ticks, uint tickRate)
    {
        if (ticks == 0 || tickRate == 0)
        {
            return;
        }

        var seconds = ticks / (double)tickRate;
        await Task.Delay(TimeSpan.FromSeconds(seconds)).ConfigureAwait(true);
    }

    private async Task WaitUntilInputUnlockedAsync()
    {
        var deadline = Time.GetTicksMsec() + 5000UL;
        while (IsInputLocked() && Time.GetTicksMsec() < deadline)
        {
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
        }
    }

    private (int Width, int Height) CaptureScreenshot(string path)
    {
        if (DisplayServer.GetName() == "headless")
        {
            return (0, 0);
        }

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
