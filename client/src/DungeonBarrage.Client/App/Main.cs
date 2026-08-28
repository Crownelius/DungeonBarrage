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

    private IReadOnlyList<ClientCharacterDefinition>? _roster;
    private int _selectedCharacterIndex;
    private int _selectedBotCharacterIndex = 1;
    private bool _inCharacterSelect;

    private ClientAbilitySlot _selectedAbilitySlot = ClientAbilitySlot.Basic;
    private int _selectedPassiveIndex;

    private bool _isAiming;
    private Vector2 _aimOrigin;
    private Vector2 _aimCurrent;
    private Vector2 _cameraOffset = Vector2.Zero;

    private ulong _inputLockedUntilMsec;
    private bool _isProcessingBotTurn;
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

        // Automatic bot turn processing when active player is bot-controlled. Outcome is never
        // actually null — it is always populated, with ClientInProgressOutcome as its "nothing
        // decided yet" value — so this must pattern-match the type, not check for null.
        if (_live is not null && !IsInputLocked() && !_isProcessingBotTurn &&
            _live.CurrentSnapshot.Outcome is ClientInProgressOutcome)
        {
            var activeId = _live.CurrentSnapshot.ActivePlayerId;
            if (activeId is "b-local-bot" || IsBotPlayer(activeId))
            {
                _ = ProcessBotTurnAsync();
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
        EnterCharacterSelect();
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

    private void EnterCharacterSelect()
    {
        try
        {
            _roster = RosterCatalog.Get().Characters;
            _selectedCharacterIndex = 0;
            _selectedBotCharacterIndex = Math.Min(1, _roster.Count - 1);
            _inCharacterSelect = true;
            _menuError = null;
        }
        catch (NativeSimulationException exception)
        {
            _menuError = exception.Message;
            _started = true;
        }

        QueueRedraw();
    }

    private void HandleCharacterSelectInput(InputEvent @event)
    {
        if (_roster is null or { Count: 0 })
        {
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

        var isConfirm = @event.IsActionPressed("ui_accept") ||
            (@event is InputEventMouseButton { Pressed: true, ButtonIndex: MouseButton.Left });
        if (isConfirm)
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
            "Press Enter or Click to Enter Character Selection.",
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

    private void DrawCharacterSelect()
    {
        var font = ThemeDB.FallbackFont;
        var pos = new Vector2(24, 36);

        DrawString(font, pos, "CHARACTER SELECT — LOCAL MATCH SETUP", fontSize: 20, modulate: MenuTextColor);
        pos.Y += 30;

        if (_roster is null or { Count: 0 })
        {
            DrawString(font, pos, "Loading Roster...", fontSize: 16, modulate: MenuTextColor);
            return;
        }

        // Left Panel: Roster List
        DrawString(font, pos, "Human Champion [UP/DOWN]:", fontSize: 14, modulate: Colors.Gold);
        pos.Y += 20;
        for (var i = 0; i < _roster.Count; i++)
        {
            var charDef = _roster[i];
            var isSelected = i == _selectedCharacterIndex;
            var isBot = i == _selectedBotCharacterIndex;
            var prefix = isSelected ? " > " : "   ";
            var tag = (isSelected ? " (YOU)" : "") + (isBot ? " (BOT)" : "");
            var color = isSelected ? Colors.Yellow : (isBot ? Colors.Coral : MenuTextColor);

            DrawString(font, pos, $"{prefix}{charDef.DisplayName}{tag}", fontSize: 14, modulate: color);
            pos.Y += 20;
        }

        // Right Panel: Champion Details
        var selectedChar = _roster[_selectedCharacterIndex];
        var detailPos = new Vector2(360, 66);
        DrawString(font, detailPos, $"Name: {selectedChar.DisplayName} ({selectedChar.Id})", fontSize: 16, modulate: Colors.Gold);
        detailPos.Y += 24;
        DrawString(font, detailPos, $"HP: {selectedChar.MaxHealth} | Range: {selectedChar.RangeTier} | Move: {selectedChar.MovementClass}", fontSize: 14, modulate: MenuTextColor);
        detailPos.Y += 22;
        DrawString(font, detailPos, $"Basic: {selectedChar.Basic.DisplayName} ({selectedChar.Basic.DamagePercent}% DMG)", fontSize: 14, modulate: MenuTextColor);
        detailPos.Y += 20;
        if (selectedChar.BasicAlt is not null)
        {
            DrawString(font, detailPos, $"BasicAlt: {selectedChar.BasicAlt.DisplayName} ({selectedChar.BasicAlt.DamagePercent}% DMG)", fontSize: 14, modulate: MenuTextColor);
            detailPos.Y += 20;
        }
        DrawString(font, detailPos, $"Special: {selectedChar.Special.DisplayName} ({selectedChar.Special.DamagePercent}% DMG)", fontSize: 14, modulate: MenuTextColor);
        detailPos.Y += 24;

        DrawString(font, detailPos, "Passives Preview:", fontSize: 14, modulate: Colors.Gold);
        detailPos.Y += 20;
        foreach (var p in selectedChar.Passives)
        {
            DrawString(font, detailPos, $" • {p.DisplayName}", fontSize: 12, modulate: LockedHintColor);
            detailPos.Y += 18;
        }

        // Footer Controls
        var footerPos = new Vector2(24, GetViewportRect().Size.Y - 30);
        DrawString(font, footerPos, "Controls: UP/DOWN (You) | LEFT/RIGHT (Bot Opponent) | ENTER / Click to START MATCH", fontSize: 14, modulate: Colors.Cyan);
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
            // Drives the exact same methods a real player's input does — EnterCharacterSelect,
            // then ConfirmCharacterAndStartDuel, then Rematch — rather than building requests by
            // hand and calling FixtureMatchBootstrapper.StartLive directly. A hand-built request
            // proves the backend accepts a well-formed request; it does not prove the interactive
            // character-select screen itself (DrawCharacterSelect/HandleCharacterSelectInput) ever
            // ran or rendered. This is the whole point of a C6 smoke test, per CLIENT_SPEC §20.5's
            // own rule: a real pixel is the proof, not a claim that the code compiles.
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
            var decisionSeed = 777uL;
            var turnsPlayed = 0;
            var botTurnExecuted = false;
            while (_live.CurrentSnapshot.Outcome is ClientInProgressOutcome && turnsPlayed < 300)
            {
                turnsPlayed++;
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
                HumanTurnExecuted: humanTurnExecuted,
                BotTurnExecuted: botTurnExecuted,
                MatchCompleted: matchCompleted,
                TurnsPlayed: turnsPlayed,
                FinalTurnNumber: finalSnapshot.TurnNumber,
                FinalStateHash: finalSnapshot.StateHash,
                RematchSessionCreated: rematchCreated,
                RematchSessionDisposedCleanly: rematchDisposedCleanly,
                ScreenshotWidth: screenshotWidth,
                ScreenshotHeight: screenshotHeight,
                CharacterSelectScreenshotWidth: characterSelectWidth,
                CharacterSelectScreenshotHeight: characterSelectHeight);
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
                HumanTurnExecuted: false,
                BotTurnExecuted: false,
                MatchCompleted: false,
                TurnsPlayed: 0,
                FinalTurnNumber: 0,
                FinalStateHash: string.Empty,
                RematchSessionCreated: false,
                RematchSessionDisposedCleanly: false,
                ScreenshotWidth: 0,
                ScreenshotHeight: 0,
                CharacterSelectScreenshotWidth: 0,
                CharacterSelectScreenshotHeight: 0);
        }
        finally
        {
            if (_live is not null)
            {
                await _live.DisposeAsync().ConfigureAwait(true);
                _live = null;
            }

            _inCharacterSelect = false;
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
