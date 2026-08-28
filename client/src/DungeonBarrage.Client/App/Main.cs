using System.Diagnostics.CodeAnalysis;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;
using DungeonBarrage.Client.Match;
using Godot;

namespace DungeonBarrage.Client.App;

/// <summary>
/// The scene root: menu with build diagnostics, character selection across the full 9-starter roster,
/// playable authoritative match with human & bot turns, passive prompts, results, and rematch.
/// </summary>
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
            + "_ExitTree, which does dispose this field, not a Dispose method the analyzer recognizes.")]
    private LiveMatch? _live;
    private string? _menuError;
    private string? _liveError;
    private bool _started;

    private bool _inLocalSetup;
    private IReadOnlyList<ClientCharacterDefinition>? _roster;
    private int _selectedCharacterIndex;
    private int _selectedBotCharacterIndex = 1;
    private bool _inCharacterSelect;
    private CharacterTileAnimation[]? _tileAnimations;
    private int _hoveredCharacterIndex = -1;

    private const float TileSize = 76f;
    private const float TileGap = 12f;
    private const int TileColumns = 5;
    private const float TileFloatHeight = 14f;
    private const float TileFloatUpSeconds = 0.16f;
    private const float TileFloatDownSeconds = 0.24f;
    private static readonly Vector2 TileGridOrigin = new(40, 108);

    /// <summary>
    /// One character tile's float animation: a small non-interruptible state machine, not a
    /// Godot <c>Tween</c>. <see cref="Main"/> has no scene-graph nodes to attach one to — every
    /// screen so far is hand-drawn from a single <see cref="_Draw"/> — so the float/land motion
    /// is advanced manually each frame in <see cref="UpdateCharacterTileAnimations"/> instead.
    /// </summary>
    private sealed class CharacterTileAnimation
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

    private ClientAbilitySlot _selectedAbilitySlot = ClientAbilitySlot.Basic;
    private int _selectedPassiveIndex;

    private bool _isAiming;
    private Vector2 _aimOrigin;
    private Vector2 _aimCurrent;
    private Vector2 _cameraOffset = Vector2.Zero;

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
            QueueRedraw();
        }

        if (_inCharacterSelect)
        {
            UpdateCharacterTileAnimations((float)delta);
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
            _liveError = exception.Message;
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
            _liveError = exception.Message;
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

        if (_live is not null)
        {
            HandleLiveInput(@event);
            return;
        }

        if (_inCharacterSelect)
        {
            HandleCharacterSelectInput(@event);
            return;
        }

        if (_inLocalSetup)
        {
            HandleLocalSetupInput(@event);
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
        EnterLocalSetup();
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
        else if (_inCharacterSelect)
        {
            DrawCharacterSelect();
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
    /// Enters the map/mode/slots screen between the main menu and character select
    /// (CLIENT_SPEC §11's own flow: Boot → MainMenu → LocalSetup → CharacterSelect → Match).
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

        var isConfirm = @event.IsActionPressed("ui_accept") ||
            (@event is InputEventMouseButton { Pressed: true, ButtonIndex: MouseButton.Left });
        if (isConfirm)
        {
            GetViewport().SetInputAsHandled();
            _inLocalSetup = false;
            EnterCharacterSelect();
        }
    }

    private void DrawLocalSetup()
    {
        var font = ThemeDB.FallbackFont;

        DrawRect(new Rect2(0, 0, GetViewportRect().Size.X, 64), new Color(0.55f, 0.10f, 0.12f));
        DrawString(font, new Vector2(24, 40), "LOCAL MATCH SETUP", fontSize: 24, modulate: Colors.White);

        var pos = new Vector2(24, 110);
        DrawString(font, pos, "Map", fontSize: 14, modulate: Colors.Gold);
        pos.Y += 22;
        DrawString(font, pos, "Horizontal Test Array", fontSize: 18, modulate: MenuTextColor);
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
            "More maps and modes are not built yet — this screen exists so the flow and its",
            fontSize: 13,
            modulate: LockedHintColor);
        pos.Y += 18;
        DrawString(
            font,
            pos,
            "controls are real now, ready to grow once there is more than one option to pick.",
            fontSize: 13,
            modulate: LockedHintColor);

        var footerPos = new Vector2(24, GetViewportRect().Size.Y - 20);
        DrawString(font, footerPos, "ENTER / Click to continue to Character Select · ESC to go back", fontSize: 13, modulate: Colors.Cyan);
    }

    private void EnterCharacterSelect()
    {
        try
        {
            _roster = RosterCatalog.Get().Characters;
            _selectedCharacterIndex = 0;
            _selectedBotCharacterIndex = Math.Min(1, _roster.Count - 1);
            _inCharacterSelect = true;
            _menuError = null;
            _hoveredCharacterIndex = -1;
            _tileAnimations = new CharacterTileAnimation[_roster.Count];
            for (var i = 0; i < _tileAnimations.Length; i++)
            {
                _tileAnimations[i] = new CharacterTileAnimation();
            }
        }
        catch (NativeSimulationException exception)
        {
            _menuError = exception.Message;
            _started = true;
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
    private static Rect2 CharacterTileRestRect(int index)
    {
        var column = index % TileColumns;
        var row = index / TileColumns;
        var position = TileGridOrigin + new Vector2(column * (TileSize + TileGap), row * (TileSize + TileGap));
        return new Rect2(position, new Vector2(TileSize, TileSize));
    }

    private int? HitTestCharacterTile(Vector2 point)
    {
        if (_roster is null)
        {
            return null;
        }

        for (var i = 0; i < _roster.Count; i++)
        {
            if (CharacterTileRestRect(i).HasPoint(point))
            {
                return i;
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
    private void UpdateCharacterTileAnimations(float delta)
    {
        if (_tileAnimations is null)
        {
            return;
        }

        var anyAnimating = false;
        for (var i = 0; i < _tileAnimations.Length; i++)
        {
            var tile = _tileAnimations[i];
            var hoverDesired = i == _hoveredCharacterIndex || i == _selectedCharacterIndex;

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

    private void HandleCharacterSelectInput(InputEvent @event)
    {
        if (_roster is null or { Count: 0 })
        {
            return;
        }

        if (@event is InputEventMouseMotion motion)
        {
            _hoveredCharacterIndex = HitTestCharacterTile(motion.Position) ?? -1;
            return;
        }

        if (@event.IsActionPressed("ui_cancel"))
        {
            GetViewport().SetInputAsHandled();
            _inCharacterSelect = false;
            _inLocalSetup = true;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_up"))
        {
            GetViewport().SetInputAsHandled();
            _selectedCharacterIndex = (_selectedCharacterIndex - 1 + _roster.Count) % _roster.Count;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_down"))
        {
            GetViewport().SetInputAsHandled();
            _selectedCharacterIndex = (_selectedCharacterIndex + 1) % _roster.Count;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_left"))
        {
            GetViewport().SetInputAsHandled();
            _selectedBotCharacterIndex = (_selectedBotCharacterIndex - 1 + _roster.Count) % _roster.Count;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_right"))
        {
            GetViewport().SetInputAsHandled();
            _selectedBotCharacterIndex = (_selectedBotCharacterIndex + 1) % _roster.Count;
            QueueRedraw();
            return;
        }

        // A click on a tile picks it as the human champion; ENTER alone starts the match — a
        // click can no longer do double duty as "confirm," now that clicking has its own,
        // different meaning on this screen.
        if (@event is InputEventMouseButton { Pressed: true, ButtonIndex: MouseButton.Left } mouseButton)
        {
            if (HitTestCharacterTile(mouseButton.Position) is int clickedIndex)
            {
                GetViewport().SetInputAsHandled();
                _selectedCharacterIndex = clickedIndex;
                QueueRedraw();
            }

            return;
        }

        if (@event.IsActionPressed("ui_accept"))
        {
            GetViewport().SetInputAsHandled();
            ConfirmCharacterAndStartDuel();
        }
    }

    private void ConfirmCharacterAndStartDuel()
    {
        if (_roster is null || _selectedCharacterIndex >= _roster.Count)
        {
            return;
        }

        var human = _roster[_selectedCharacterIndex];
        var bot = _roster[_selectedBotCharacterIndex];
        var appearance = new ClientAppearance("default", ["default", "default", "default"], "default");
        var request = new ClientCreateRequest(
            SchemaVersion: 1,
            MatchId: $"local-duel-{_matchSeed}",
            SimulationVersion: LocalMatchSession.SimulationVersion,
            ContentVersion: LocalMatchSession.ContentVersion,
            Match: new ClientMatchConfig(
                Seed: _matchSeed,
                MapId: "horizontal-test-array",
                Mode: "turnBased",
                Players:
                [
                    new ClientPlayerConfig("a-local-player", Team: 0, human.Id, appearance),
                    new ClientPlayerConfig("b-local-bot", Team: 1, bot.Id, appearance),
                ]));

        try
        {
            _live = CreateLiveMatch(FixtureMatchBootstrapper.StartLive(request));
            _menuError = null;
        }
        catch (Exception exception) when (exception is InvalidDataException or NativeSimulationException)
        {
            _menuError = exception.Message;
        }

        _inCharacterSelect = _live is null;
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
        ConfirmCharacterAndStartDuel();
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
                    _selectedAbilitySlot = ClientAbilitySlot.Basic;
                    QueueRedraw();
                    return;
                case Key.Key2:
                    _selectedAbilitySlot = ClientAbilitySlot.BasicAlt;
                    QueueRedraw();
                    return;
                case Key.Key3:
                    _selectedAbilitySlot = ClientAbilitySlot.Special;
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
                    _selectedAbilitySlot,
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

    private void HandlePassiveSelectionInput(InputEvent @event)
    {
        if (_live is null || _roster is null)
        {
            return;
        }

        var activePlayer = _live.CurrentSnapshot.Players.FirstOrDefault(p => p.PlayerId == _live.CurrentSnapshot.ActivePlayerId);
        var charDef = _roster.FirstOrDefault(c => c.Id == activePlayer?.CharacterId);
        if (charDef is null or { Passives.Count: 0 })
        {
            return;
        }

        if (@event.IsActionPressed("ui_up"))
        {
            GetViewport().SetInputAsHandled();
            _selectedPassiveIndex = (_selectedPassiveIndex - 1 + charDef.Passives.Count) % charDef.Passives.Count;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_down"))
        {
            GetViewport().SetInputAsHandled();
            _selectedPassiveIndex = (_selectedPassiveIndex + 1) % charDef.Passives.Count;
            QueueRedraw();
            return;
        }

        if (@event.IsActionPressed("ui_accept") ||
            (@event is InputEventMouseButton { Pressed: true, ButtonIndex: MouseButton.Left }))
        {
            GetViewport().SetInputAsHandled();
            var chosenPassive = charDef.Passives[_selectedPassiveIndex].Id;
            _ = SubmitAndRedrawAsync(() => _live.SubmitPassiveChoiceAsync(chosenPassive));
        }
    }

    private static (int AngleMillidegrees, int PowerBasisPoints) ResolveAim(Vector2 origin, Vector2 release)
    {
        var drag = release - origin;
        if (drag.LengthSquared() < 1f)
        {
            return (0, 0);
        }

        var angleDegrees = Mathf.RadToDeg(Mathf.Atan2(-drag.Y, drag.X));
        var angleMillidegrees = Mathf.RoundToInt(angleDegrees * 1000f);

        var power = Mathf.Clamp(drag.Length() / DragPixelsPerPowerBasisPoint, 0, MaxPowerBasisPoints);
        return (angleMillidegrees, Mathf.RoundToInt(power));
    }

    private async Task<ClientMatchTransition?> SubmitAndRedrawAsync(Func<Task<ClientMatchTransition>> submit)
    {
        if (_live is null)
        {
            return null;
        }

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
            "Press Enter or Click to Enter Local Match Setup.",
            fontSize: 16,
            modulate: MenuTextColor);
        position.Y += 24;
        DrawString(
            font,
            position,
            "Full 9-champion roster available. Battle against AI Bot opponent.",
            fontSize: 14,
            modulate: LockedHintColor);

        if (_menuError is not null)
        {
            position.Y += 32;
            DrawString(font, position, $"Bootstrap failed: {_menuError}", fontSize: 16, modulate: Colors.OrangeRed);
        }
    }

    private static Color CharacterTileColor(int index, int count) =>
        Color.FromHsv(count <= 0 ? 0f : (float)index / count, 0.55f, 0.5f);

    private void DrawCharacterSelect()
    {
        var font = ThemeDB.FallbackFont;

        DrawRect(new Rect2(0, 0, GetViewportRect().Size.X, 64), new Color(0.55f, 0.10f, 0.12f));
        DrawString(font, new Vector2(24, 40), "CHARACTER SELECT", fontSize: 24, modulate: Colors.White);

        if (_roster is null or { Count: 0 })
        {
            DrawString(font, new Vector2(24, 100), "Loading roster…", fontSize: 16, modulate: MenuTextColor);
            return;
        }

        // The grid: one 76x76 tile per roster champion, floating on hover or when it is the
        // current human pick. Draw order matters here — later calls paint over earlier ones —
        // so the label goes on top of the tile, and every tile before its own float offset is
        // applied means a floated tile visually overlaps the row above it, matching the
        // reference image's own slight overlap when a character lifts off the grid line.
        for (var i = 0; i < _roster.Count; i++)
        {
            var charDef = _roster[i];
            var restRect = CharacterTileRestRect(i);
            var yOffset = _tileAnimations?[i].YOffset ?? 0f;
            var tileRect = new Rect2(restRect.Position + new Vector2(0, yOffset), restRect.Size);

            var isHuman = i == _selectedCharacterIndex;
            var isBot = i == _selectedBotCharacterIndex;

            DrawRect(tileRect, CharacterTileColor(i, _roster.Count));

            var borderColor = isHuman ? Colors.Yellow : isBot ? Colors.Coral : new Color(1, 1, 1, 0.25f);
            DrawRect(tileRect, borderColor, filled: false, width: isHuman || isBot ? 3f : 1f);

            var letter = char.ToUpperInvariant(charDef.DisplayName.Length > 0 ? charDef.DisplayName[0] : '?').ToString();
            var letterSize = font.GetStringSize(letter, fontSize: 30);
            var letterPos = tileRect.Position +
                new Vector2((tileRect.Size.X - letterSize.X) / 2f, (tileRect.Size.Y + letterSize.Y * 0.7f) / 2f);
            DrawString(font, letterPos, letter, fontSize: 30, modulate: Colors.White);

            if (isHuman)
            {
                var tagSize = font.GetStringSize("YOU", fontSize: 11);
                DrawString(font, tileRect.Position + new Vector2((tileRect.Size.X - tagSize.X) / 2f, -6), "YOU", fontSize: 11, modulate: Colors.Yellow);
            }
            else if (isBot)
            {
                var tagSize = font.GetStringSize("BOT", fontSize: 11);
                DrawString(font, tileRect.Position + new Vector2((tileRect.Size.X - tagSize.X) / 2f, -6), "BOT", fontSize: 11, modulate: Colors.Coral);
            }
        }

        // Detail panel: whatever is under the mouse takes priority over the keyboard pick, the
        // same way a real player expects hovering to preview before committing.
        var detailIndex = _hoveredCharacterIndex >= 0 ? _hoveredCharacterIndex : _selectedCharacterIndex;
        var detailChar = _roster[detailIndex];
        var detailPos = new Vector2(TileGridOrigin.X + (TileColumns * (TileSize + TileGap)) + 20, TileGridOrigin.Y);
        DrawString(font, detailPos, $"{detailChar.DisplayName} ({detailChar.Id})", fontSize: 18, modulate: Colors.Gold);
        detailPos.Y += 26;
        DrawString(font, detailPos, $"HP {detailChar.MaxHealth}   Range {detailChar.RangeTier}   Move {detailChar.MovementClass}", fontSize: 14, modulate: MenuTextColor);
        detailPos.Y += 24;
        DrawString(font, detailPos, $"Basic: {detailChar.Basic.DisplayName} ({detailChar.Basic.DamagePercent}% dmg, {detailChar.Basic.AttackShape})", fontSize: 14, modulate: MenuTextColor);
        detailPos.Y += 20;
        if (detailChar.BasicAlt is not null)
        {
            DrawString(font, detailPos, $"Basic Alt: {detailChar.BasicAlt.DisplayName} ({detailChar.BasicAlt.DamagePercent}% dmg, {detailChar.BasicAlt.AttackShape})", fontSize: 14, modulate: MenuTextColor);
            detailPos.Y += 20;
        }

        DrawString(font, detailPos, $"Special: {detailChar.Special.DisplayName} ({detailChar.Special.DamagePercent}% dmg, {detailChar.Special.AttackShape})", fontSize: 14, modulate: MenuTextColor);
        detailPos.Y += 26;
        DrawString(font, detailPos, "Passives (chosen mid-match):", fontSize: 13, modulate: Colors.Gold);
        detailPos.Y += 18;
        foreach (var passive in detailChar.Passives)
        {
            DrawString(font, detailPos, $" • {passive.DisplayName}", fontSize: 12, modulate: LockedHintColor);
            detailPos.Y += 17;
        }

        // Bottom cards: the human's pick and the bot's pick side by side, mirroring the
        // reference image's P1/CPU panels.
        var cardTop = TileGridOrigin.Y + (2 * (TileSize + TileGap)) + 30;
        var cardWidth = (GetViewportRect().Size.X - 72) / 2f;
        DrawSelectionCard(new Rect2(24, cardTop, cardWidth, 190), "PLAYER 1", _selectedCharacterIndex, new Color(0.55f, 0.10f, 0.12f));
        DrawSelectionCard(new Rect2(48 + cardWidth, cardTop, cardWidth, 190), "CPU OPPONENT", _selectedBotCharacterIndex, new Color(0.30f, 0.30f, 0.34f));

        var footerPos = new Vector2(24, GetViewportRect().Size.Y - 20);
        DrawString(
            font,
            footerPos,
            "UP/DOWN pick your champion · LEFT/RIGHT pick the bot's · hover or click a tile · ENTER to start · ESC to go back",
            fontSize: 13,
            modulate: Colors.Cyan);
    }

    private void DrawSelectionCard(Rect2 rect, string label, int characterIndex, Color background)
    {
        if (_roster is null || characterIndex >= _roster.Count)
        {
            return;
        }

        var font = ThemeDB.FallbackFont;
        var character = _roster[characterIndex];

        DrawRect(rect, background);
        DrawRect(rect, new Color(1, 1, 1, 0.6f), filled: false, width: 2f);

        var swatchRect = new Rect2(rect.Position + new Vector2(18, 18), new Vector2(TileSize, TileSize));
        DrawRect(swatchRect, CharacterTileColor(characterIndex, _roster.Count));
        DrawRect(swatchRect, Colors.White, filled: false, width: 2f);

        var textPos = rect.Position + new Vector2(18 + TileSize + 18, 40);
        DrawString(font, textPos, label, fontSize: 15, modulate: new Color(1, 1, 1, 0.85f));
        textPos.Y += 30;
        DrawString(font, textPos, character.DisplayName, fontSize: 22, modulate: Colors.White);
        textPos.Y += 26;
        DrawString(font, textPos, $"HP {character.MaxHealth}   {character.RangeTier} / {character.MovementClass}", fontSize: 13, modulate: new Color(1, 1, 1, 0.75f));
    }

    private void DrawLiveMatch(LiveMatch live)
    {
        var snapshot = live.CurrentSnapshot;
        DrawMatch(snapshot, live.CurrentTerrain);

        if (_isAiming)
        {
            DrawLine(_aimOrigin + _cameraOffset, _aimCurrent + _cameraOffset, AimLineColor, width: 2);
        }

        var font = ThemeDB.FallbackFont;
        var hudY = 8f;
        DrawString(
            font,
            new Vector2(8, hudY),
            $"active {snapshot.ActivePlayerId}   phase {snapshot.Phase}   slot [{_selectedAbilitySlot}]   wind {snapshot.WindPerTick}",
            fontSize: 14,
            modulate: MenuTextColor);

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

    private void DrawPassiveSelectModal()
    {
        if (_live is null || _roster is null)
        {
            return;
        }

        var activePlayer = _live.CurrentSnapshot.Players.FirstOrDefault(p => p.PlayerId == _live.CurrentSnapshot.ActivePlayerId);
        var charDef = _roster.FirstOrDefault(c => c.Id == activePlayer?.CharacterId);
        if (charDef is null or { Passives.Count: 0 })
        {
            return;
        }

        var rect = new Rect2(new Vector2(200, 150), new Vector2(400, 240));
        DrawRect(rect, new Color(0.12f, 0.14f, 0.20f, 0.95f));
        DrawRect(rect, Colors.Gold, filled: false, width: 2);

        var font = ThemeDB.FallbackFont;
        var pos = rect.Position + new Vector2(20, 30);
        DrawString(font, pos, "SPECIAL GAUGE FULL — SELECT PASSIVE", fontSize: 16, modulate: Colors.Gold);
        pos.Y += 30;

        for (var i = 0; i < charDef.Passives.Count; i++)
        {
            var isSelected = i == _selectedPassiveIndex;
            var prefix = isSelected ? " > " : "   ";
            var color = isSelected ? Colors.Yellow : MenuTextColor;
            DrawString(font, pos, $"{prefix}{charDef.Passives[i].DisplayName}", fontSize: 14, modulate: color);
            pos.Y += 24;
        }

        pos.Y += 20;
        DrawString(font, pos, "UP/DOWN to select | ENTER to confirm", fontSize: 12, modulate: LockedHintColor);
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
            $"turn {snapshot.TurnNumber}  gen {snapshot.SnapshotGeneration}  hash {snapshot.StateHash}  [1:Basic 2:Alt 3:Spec F:Cam P:Pass]",
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
                        new Rect2(x * PixelsPerCell + _cameraOffset.X, y * PixelsPerCell + _cameraOffset.Y, PixelsPerCell, PixelsPerCell),
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
                block.OriginCellX * PixelsPerCell + _cameraOffset.X,
                block.OriginCellY * PixelsPerCell + _cameraOffset.Y,
                block.WidthCells * PixelsPerCell,
                block.HeightCells * PixelsPerCell),
            color);
    }

    private void DrawPlayer(ClientPlayerSnapshot player, Color color, int index)
    {
        var center = ToPixels(player.Position) + _cameraOffset;
        DrawCircle(center, PlayerRadiusPixels, player.IsEliminated ? Colors.Gray : color);

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
            live = CreateLiveMatch(FixtureMatchBootstrapper.Start());
            _live = live;

            var beforeActivePlayer = live.CurrentSnapshot.ActivePlayerId;
            var defenderId = live.CurrentSnapshot.Players.First(p => p.PlayerId != beforeActivePlayer).PlayerId;
            var defenderHealthBefore = HealthOf(live.CurrentSnapshot, defenderId);

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
            // EnterCharacterSelect, ConfirmCharacterAndStartDuel, then Rematch — rather than
            // building requests by hand and calling FixtureMatchBootstrapper.StartLive directly.
            // A hand-built request proves the backend accepts well-formed input; it does not
            // prove the interactive screens themselves (DrawLocalSetup, DrawCharacterSelect/
            // HandleCharacterSelectInput) ever ran or rendered. This is the whole point of a C6
            // smoke test, per CLIENT_SPEC §20.5's own rule: a real pixel is the proof, not a
            // claim that the code compiles.
            EnterLocalSetup();
            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (localSetupWidth, localSetupHeight) = CaptureScreenshot(options.LocalSetupScreenshotPath);
            _inLocalSetup = false;

            EnterCharacterSelect();
            if (_roster is null || _roster.Count == 0)
            {
                throw new InvalidOperationException($"Character select failed to load a roster: {_menuError}");
            }

            var rosterCount = _roster.Count;
            var humanChar = _roster[_selectedCharacterIndex];
            var botChar = _roster[_selectedBotCharacterIndex];

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (characterSelectWidth, characterSelectHeight) = CaptureScreenshot(options.CharacterSelectScreenshotPath);

            // Exercises the hover-float animation through the exact input path real mouse
            // motion goes through, and proves the "cannot be interrupted" requirement: hover a
            // tile, let its float-up motion start, move the hover away before that motion
            // finishes, and confirm it still completes the float — never reverses mid-flight —
            // before landing begins.
            var hoverTileIndex = Math.Min(2, _roster.Count - 1);
            var hoverPoint = CharacterTileRestRect(hoverTileIndex).GetCenter();
            using (var hoverEvent = new InputEventMouseMotion { Position = hoverPoint })
            {
                HandleCharacterSelectInput(hoverEvent);
            }

            // The state transition into "animating toward floated" and the actual time-advance
            // happen in separate branches of UpdateCharacterTileAnimations (a tile only starts
            // consuming delta once IsAnimating is already true entering the call) — so this
            // needs two calls: one to begin the motion, a second to actually move partway
            // through it. A single call here would (incorrectly) start the animation without
            // visibly moving it at all.
            UpdateCharacterTileAnimations(0f);
            UpdateCharacterTileAnimations(TileFloatUpSeconds * 0.5f);
            var wasFloatingMidFlight = _tileAnimations![hoverTileIndex].IsAnimating &&
                _tileAnimations[hoverTileIndex].AnimatingTowardFloated &&
                _tileAnimations[hoverTileIndex].YOffset < -1f;

            using (var awayEvent = new InputEventMouseMotion { Position = new Vector2(-100, -100) })
            {
                HandleCharacterSelectInput(awayEvent);
            }

            UpdateCharacterTileAnimations(0.001f);
            var stillCompletingTheFloatAfterHoverLeft = _tileAnimations[hoverTileIndex].IsAnimating &&
                _tileAnimations[hoverTileIndex].AnimatingTowardFloated;

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            _ = CaptureScreenshot(options.CharacterSelectHoverScreenshotPath);

            UpdateCharacterTileAnimations(TileFloatUpSeconds);
            UpdateCharacterTileAnimations(TileFloatDownSeconds);
            var restedCleanlyAfterTheFullCycle = !_tileAnimations[hoverTileIndex].IsAnimating &&
                Mathf.Abs(_tileAnimations[hoverTileIndex].YOffset) < 0.01f;

            var hoverAnimationInterruptionTestPassed =
                wasFloatingMidFlight && stillCompletingTheFloatAfterHoverLeft && restedCleanlyAfterTheFullCycle;
            _hoveredCharacterIndex = -1;
            if (!hoverAnimationInterruptionTestPassed)
            {
                throw new InvalidOperationException(
                    "The character-select tile float animation was interrupted mid-flight instead of completing it.");
            }

            // Claims the same guard flag Main's own _Process loop checks before auto-firing a
            // bot turn (see ProcessBotTurnAsync). Without this, the moment this method awaits
            // anything below, _Process can race in and submit a SEPARATE bot decision through
            // the production auto-play path while this method is also mid-way through driving
            // its own turn — corrupting turn accounting and leaving stray input locks behind
            // that make later screens (like the passive-select modal) appear not to render.
            _isProcessingBotTurn = true;

            ConfirmCharacterAndStartDuel();
            if (_live is null)
            {
                throw new InvalidOperationException($"Character select confirmation failed to start a match: {_menuError}");
            }

            // 1. Human Move & Ability Turn
            var moveTrans = await _live.SubmitMoveAsync(MoveStepDx).ConfigureAwait(true);
            await WaitTicksAsync(moveTrans.InputLockTicks, moveTrans.PresentationTickRate).ConfigureAwait(true);

            var abilityTrans = await _live.SubmitAbilityAsync(
                ClientAbilitySlot.Basic,
                angleMillidegrees: 45_000,
                powerBasisPoints: 1_500,
                targetPlayerId: null).ConfigureAwait(true);
            await WaitTicksAsync(abilityTrans.InputLockTicks, abilityTrans.PresentationTickRate).ConfigureAwait(true);

            var humanTurnExecuted = moveTrans.Disposition == ClientTransitionDisposition.Accepted &&
                                    abilityTrans.Disposition == ClientTransitionDisposition.Accepted;

            // 2. Drive the match to genuine completion — after the human's one demonstration
            // turn, the bot plays every remaining turn for whichever player is active (bounded,
            // like the Rust bot::tests and BotDecisionTests full-duel proofs already do) until
            // Outcome actually leaves ClientInProgressOutcome. A single bot decision call proves
            // a bot CAN act; it does not prove C6's own gate — "completes... a bot match" — which
            // needs a real terminal outcome, not one action.
            //
            // One case is intercepted rather than left to the bot: if the human's own gauge
            // fills and PassiveSelection comes up for them specifically, it is routed through
            // the real HandlePassiveSelectionInput keyboard path (a synthetic Enter press) —
            // the same method a real player's ENTER key goes through — instead of the bot's
            // automatic handling. Letting the bot decide it would prove a decision gets made
            // somehow; it would not prove DrawPassiveSelectModal/HandlePassiveSelectionInput
            // themselves render and work.
            var decisionSeed = 777uL;
            var turnsPlayed = 0;
            var botTurnExecuted = false;
            var passivePromptShownForHuman = false;
            var passivePromptConfirmedThroughRealInput = false;
            while (_live.CurrentSnapshot.Outcome is ClientInProgressOutcome && turnsPlayed < 300)
            {
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
            }

            var matchCompleted = _live.CurrentSnapshot.Outcome is not ClientInProgressOutcome;

            QueueRedraw();
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            await ToSignal(GetTree(), SceneTree.SignalName.ProcessFrame);
            var (screenshotWidth, screenshotHeight) = CaptureScreenshot(options.ScreenshotPath);

            var finalSnapshot = _live.CurrentSnapshot;
            if (!matchCompleted)
            {
                throw new InvalidOperationException(
                    $"The match did not reach a terminal outcome within {turnsPlayed} bot decisions.");
            }

            // 3. Rematch — the real Rematch() method, not a hand-built second request.
            Rematch();
            if (_live is null)
            {
                throw new InvalidOperationException($"Rematch failed to start a fresh match: {_menuError}");
            }

            var rematchCreated = _live.CurrentSnapshot.TurnNumber == 1;
            await _live.DisposeAsync().ConfigureAwait(true);
            var rematchDisposedCleanly = true;
            _live = null;

            return new C6SmokeReport(
                Success: true,
                Error: null,
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                RosterCount: rosterCount,
                HumanCharacterId: humanChar.Id,
                BotCharacterId: botChar.Id,
                InitialMatchCreated: true,
                HoverAnimationInterruptionTestPassed: hoverAnimationInterruptionTestPassed,
                HumanTurnExecuted: humanTurnExecuted,
                BotTurnExecuted: botTurnExecuted,
                PassivePromptShownForHuman: passivePromptShownForHuman,
                PassivePromptConfirmedThroughRealInput: passivePromptConfirmedThroughRealInput,
                MatchCompleted: matchCompleted,
                TurnsPlayed: turnsPlayed,
                FinalTurnNumber: finalSnapshot.TurnNumber,
                FinalStateHash: finalSnapshot.StateHash,
                RematchSessionCreated: rematchCreated,
                RematchSessionDisposedCleanly: rematchDisposedCleanly,
                ScreenshotWidth: screenshotWidth,
                ScreenshotHeight: screenshotHeight,
                CharacterSelectScreenshotWidth: characterSelectWidth,
                CharacterSelectScreenshotHeight: characterSelectHeight,
                LocalSetupScreenshotWidth: localSetupWidth,
                LocalSetupScreenshotHeight: localSetupHeight);
        }
        catch (Exception exception)
        {
            return new C6SmokeReport(
                Success: false,
                Error: $"{exception.GetType().Name}: {exception.Message}",
                ClientVersion: diagnostics.ClientVersion,
                GodotVersion: diagnostics.GodotVersion,
                RosterCount: 0,
                HumanCharacterId: string.Empty,
                BotCharacterId: string.Empty,
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
                CharacterSelectScreenshotWidth: 0,
                CharacterSelectScreenshotHeight: 0,
                LocalSetupScreenshotWidth: 0,
                LocalSetupScreenshotHeight: 0);
        }
        finally
        {
            if (_live is not null)
            {
                await _live.DisposeAsync().ConfigureAwait(true);
                _live = null;
            }

            _inCharacterSelect = false;
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
            // A minimal boot to a live match — LocalSetup and character select's own screens are
            // already proven pixel-for-pixel by the C6 smoke path; this test exists to prove one
            // thing neither of those does: that an idle turn ends on its own, through the real
            // Main._Process trigger, without this test ever calling SubmitTimeoutAsync itself.
            EnterLocalSetup();
            _inLocalSetup = false;
            EnterCharacterSelect();
            if (_roster is null || _roster.Count == 0)
            {
                throw new InvalidOperationException($"Character select failed to load a roster: {_menuError}");
            }

            ConfirmCharacterAndStartDuel();
            if (_live is null)
            {
                throw new InvalidOperationException($"Character select confirmation failed to start a match: {_menuError}");
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

            _inCharacterSelect = false;
            _isProcessingBotTurn = false;
            _isProcessingTimeout = false;
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
