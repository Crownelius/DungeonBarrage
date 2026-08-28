using System.Text.Json;
using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;

namespace DungeonBarrage.Client.Match;

/// <summary>
/// Raised when the authority refuses a command.
/// </summary>
/// <remarks>
/// A gameplay rejection is a normal outcome the boundary answers successfully (C3's own
/// distinction, carried forward here): the authoritative core refusing a wrong-phase or
/// wrong-turn command is not a bug. This exists so a caller submitting through
/// <see cref="LiveMatch.SubmitAsync"/> — which expects an accepted result to reconcile against —
/// gets a typed signal rather than reconciling to a snapshot the command never actually produced.
/// </remarks>
public sealed class MatchCommandRejectedException : Exception
{
    /// <summary>Creates an exception with no additional context.</summary>
    public MatchCommandRejectedException()
        : base("The command was not accepted.")
    {
    }

    /// <summary>Creates an exception with a message.</summary>
    /// <param name="message">The message.</param>
    public MatchCommandRejectedException(string message)
        : base(message)
    {
    }

    /// <summary>Creates an exception with a message and an inner cause.</summary>
    /// <param name="message">The message.</param>
    /// <param name="innerException">The cause.</param>
    public MatchCommandRejectedException(string message, Exception innerException)
        : base(message, innerException)
    {
    }

    /// <summary>The transition's actual disposition, when known.</summary>
    public string Disposition { get; init; } = "unknown";

    /// <summary>Creates an exception naming the rejecting disposition.</summary>
    /// <param name="disposition">The transition's actual disposition.</param>
    /// <returns>The exception, ready to throw.</returns>
    public static MatchCommandRejectedException ForDisposition(string disposition) =>
        new($"The command was not accepted (disposition: {disposition}).") { Disposition = disposition };
}

/// <summary>
/// Owns one live match's authoritative state and reconciles it after every command.
/// </summary>
/// <remarks>
/// <para>
/// This is the C5 layer C4's Godot-project <c>FixtureMatchBootstrapper</c> deliberately stopped short
/// of: C4 renders one static snapshot and disposes; this submits real commands and keeps the view
/// truthful across a sequence of them. The reconciliation rule is exactly CLIENT_SPEC's C5 gate:
/// every view ends at <c>PostSnapshot</c> — never at a locally predicted or animated intermediate
/// the client invented — so <see cref="CurrentSnapshot"/> is only ever assigned from a transition
/// the authority actually returned.
/// </para>
/// <para>
/// Terrain is re-read only when the transition actually reports a
/// <see cref="ClientTerrainChangedEvent"/>. The native ABI has no partial-fetch call — a refresh
/// always returns the complete cell buffer — so this does not shrink any one read; it skips reads
/// entirely for the (common) case of a command that changed nothing about the terrain, which is
/// what CLIENT_SPEC's C5 item asks for: not re-reading the whole mask *every* time.
/// </para>
/// </remarks>
public sealed class LiveMatch : IDisposable, IAsyncDisposable
{
    private readonly LocalMatchSession _session;
    private uint _nextCommandOrdinal;

    private LiveMatch(
        LocalMatchSession session,
        ClientMatchSnapshot snapshot,
        TerrainRead terrain,
        string matchLocalPrefix)
    {
        _session = session;
        CurrentSnapshot = snapshot;
        CurrentTerrain = terrain;
        _matchLocalPrefix = matchLocalPrefix;
        LastEvents = [];
    }

    private readonly string _matchLocalPrefix;

    /// <summary>The most recently reconciled authoritative snapshot.</summary>
    public ClientMatchSnapshot CurrentSnapshot { get; private set; }

    /// <summary>The most recently read terrain cells.</summary>
    public TerrainRead CurrentTerrain { get; private set; }

    /// <summary>The events the most recent accepted command produced, in presentation-tick order.</summary>
    public IReadOnlyList<ClientPresentationEvent> LastEvents { get; private set; }

    /// <summary>Ticks a caller must lock input for after the most recent accepted command.</summary>
    public uint LastInputLockTicks { get; private set; }

    /// <summary>Presentation ticks per second, from the most recent accepted command's transition.</summary>
    public uint PresentationTickRate { get; private set; } = 60;

    /// <summary>
    /// Wraps an already-created match for live command submission.
    /// </summary>
    /// <remarks>
    /// Deliberately independent of any bootstrap type: this project has no Godot dependency, and
    /// the Godot project's own <c>FixtureMatchBootstrapper</c> — which does depend on Godot, for
    /// its manifest validation — is the natural caller, passing the pieces of its own result
    /// straight through. Keeping the dependency this way around, rather than referencing a
    /// Godot-project type from here, is what keeps <see cref="LiveMatch"/> itself headlessly
    /// testable.
    /// </remarks>
    /// <param name="session">A freshly created, live match session.</param>
    /// <param name="initialSnapshot">The session's initial authoritative snapshot.</param>
    /// <param name="initialTerrain">The session's initial terrain read.</param>
    /// <param name="matchLocalPrefix">A stable prefix for this instance's generated command ids.</param>
    /// <returns>The live wrapper. Ownership of the session transfers to the returned instance.</returns>
    public static LiveMatch Create(
        LocalMatchSession session,
        ClientMatchSnapshot initialSnapshot,
        TerrainRead initialTerrain,
        string matchLocalPrefix)
    {
        ArgumentNullException.ThrowIfNull(session);
        ArgumentNullException.ThrowIfNull(initialSnapshot);
        ArgumentNullException.ThrowIfNull(matchLocalPrefix);
        return new LiveMatch(session, initialSnapshot, initialTerrain, matchLocalPrefix);
    }

    /// <summary>Submits a horizontal move for the active player.</summary>
    /// <param name="dx">Requested signed fixed-point horizontal displacement.</param>
    /// <returns>The accepted transition.</returns>
    /// <exception cref="MatchCommandRejectedException">The authority refused the command.</exception>
    public Task<ClientMatchTransition> SubmitMoveAsync(int dx) =>
        SubmitAsync(ordinal => ClientMatchCommand.Move(
            CommandId(ordinal),
            CurrentSnapshot.ActivePlayerId ?? throw new InvalidOperationException("No active player."),
            CurrentSnapshot.TurnNumber,
            CurrentSnapshot.SnapshotGeneration,
            dx));

    /// <summary>Submits an ability for the active player.</summary>
    /// <param name="slot">Character ability slot.</param>
    /// <param name="angleMillidegrees">Launch angle in integer millidegrees.</param>
    /// <param name="powerBasisPoints">Launch power in basis points.</param>
    /// <param name="targetPlayerId">Optional primary player target.</param>
    /// <returns>The accepted transition.</returns>
    /// <exception cref="MatchCommandRejectedException">The authority refused the command.</exception>
    public Task<ClientMatchTransition> SubmitAbilityAsync(
        ClientAbilitySlot slot,
        int angleMillidegrees,
        int powerBasisPoints,
        string? targetPlayerId) =>
        SubmitAsync(ordinal => ClientMatchCommand.Ability(
            CommandId(ordinal),
            CurrentSnapshot.ActivePlayerId ?? throw new InvalidOperationException("No active player."),
            CurrentSnapshot.TurnNumber,
            CurrentSnapshot.SnapshotGeneration,
            slot,
            angleMillidegrees,
            powerBasisPoints,
            targetPlayerId,
            secondaryTargetPlayerId: null));

    /// <summary>Submits the one-time passive choice for the active player.</summary>
    /// <param name="passiveId">Stable passive definition identifier.</param>
    /// <returns>The accepted transition.</returns>
    /// <exception cref="MatchCommandRejectedException">The authority refused the command.</exception>
    public Task<ClientMatchTransition> SubmitPassiveChoiceAsync(string passiveId) =>
        SubmitAsync(ordinal => ClientMatchCommand.PassiveChoice(
            CommandId(ordinal),
            CurrentSnapshot.ActivePlayerId ?? throw new InvalidOperationException("No active player."),
            CurrentSnapshot.TurnNumber,
            CurrentSnapshot.SnapshotGeneration,
            passiveId));

    /// <summary>Submits a pass for the active player.</summary>
    /// <returns>The accepted transition.</returns>
    /// <exception cref="MatchCommandRejectedException">The authority refused the command.</exception>
    public Task<ClientMatchTransition> SubmitPassAsync() =>
        SubmitAsync(ordinal => ClientMatchCommand.Pass(
            CommandId(ordinal),
            CurrentSnapshot.ActivePlayerId ?? throw new InvalidOperationException("No active player."),
            CurrentSnapshot.TurnNumber,
            CurrentSnapshot.SnapshotGeneration));

    /// <summary>
    /// Asks the native bot coordinator what the active player would do, then submits that
    /// decision through the same ordinary command path a human's own input goes through.
    /// </summary>
    /// <remarks>
    /// One call drives exactly one action — a plain reposition, one ability, one passive
    /// choice, or a pass — never a whole turn. The Rust bot's own calling contract
    /// (<c>db_sim_core::bot::decide</c>'s doc comment) is that a full bot turn takes at most
    /// two decisions: an optional move, then a follow-up ability or pass against the
    /// post-move state. A caller drives that same two-call shape by invoking this method
    /// again after the first result, exactly as a human's own move-then-fire submissions do.
    /// </remarks>
    /// <param name="difficulty">Search resolution and aim-error preset.</param>
    /// <param name="decisionSeed">
    /// Seeds only the bot's own aim-jitter/passive-tie-break RNG, never the match's
    /// authoritative RNG — pass a fresh value per call so repeated decisions do not jitter
    /// identically.
    /// </param>
    /// <returns>The accepted transition for whichever action the bot decided on.</returns>
    /// <exception cref="MatchCommandRejectedException">The authority refused the command.</exception>
    public async Task<ClientMatchTransition> SubmitBotDecisionAsync(ClientBotDifficulty difficulty, ulong decisionSeed)
    {
        var activePlayerId = CurrentSnapshot.ActivePlayerId
            ?? throw new InvalidOperationException("No active player.");
        var request = new ClientBotDecisionRequest(1, activePlayerId, difficulty, decisionSeed);
        var requestBytes = JsonSerializer.SerializeToUtf8Bytes(request, ClientEnvelope.Options);
        var responseBytes = await _session.DecideBotActionAsync(requestBytes).ConfigureAwait(true);
        var decision = JsonSerializer.Deserialize<ClientBotDecision>(responseBytes, ClientEnvelope.Options)
            ?? throw new InvalidDataException("The native bot decision response decoded to null.");

        // SecondaryTargetPlayerId is never surfaced here: the bot never sets one today (its own
        // Ability decisions always carry `null`), and SubmitAbilityAsync's own signature already
        // omits it for the same reason human input never supplies one through this path.
        return decision switch
        {
            ClientBotMoveDecision move =>
                await SubmitMoveAsync(move.Dx).ConfigureAwait(true),
            ClientBotAbilityDecision ability =>
                await SubmitAbilityAsync(ability.Slot, ability.AngleMillidegrees, ability.PowerBasisPoints, ability.TargetPlayerId)
                    .ConfigureAwait(true),
            ClientBotPassiveChoiceDecision passive =>
                await SubmitPassiveChoiceAsync(passive.PassiveId).ConfigureAwait(true),
            ClientBotPassDecision =>
                await SubmitPassAsync().ConfigureAwait(true),
            _ => throw new InvalidDataException($"Unrecognized bot decision type: {decision.GetType()}"),
        };
    }

    /// <summary>Disposes the underlying session.</summary>
    public ValueTask DisposeAsync() => _session.DisposeAsync();

    /// <summary>
    /// Disposes the underlying session synchronously.
    /// </summary>
    /// <remarks>
    /// For an engine callback such as <c>Node._ExitTree</c>: a <c>void</c>-signature override
    /// where blocking on a <see cref="ValueTask"/> is unsafe (CA2012 — it is not guaranteed to
    /// block until the operation actually completes) but an async fire-and-forget disposal
    /// during tree teardown is its own hazard, since the node may already be gone by the time a
    /// continuation resumes. <see cref="LocalMatchSession"/> does no real asynchronous work on
    /// disposal — <c>IAsyncDisposable</c> exists there only so callers can <c>await using</c>
    /// uniformly — so the synchronous path is exactly as complete as the asynchronous one.
    /// </remarks>
    public void Dispose() => _session.Dispose();

    private string CommandId(uint ordinal) => $"{_matchLocalPrefix}-cmd-{ordinal}";

    private async Task<ClientMatchTransition> SubmitAsync(Func<uint, ClientMatchCommand> build)
    {
        // Each command gets a fresh id even on a rejection, so a caller correcting a rejected
        // input (say, an out-of-range angle) and resubmitting cannot accidentally reuse an id the
        // ledger already has an answer for and get that stale answer replayed back.
        var command = build(_nextCommandOrdinal++);
        var requestBytes = JsonSerializer.SerializeToUtf8Bytes(command, ClientEnvelope.Options);
        var responseBytes = await _session.ApplyAsync(requestBytes).ConfigureAwait(true);

        var transition = JsonSerializer.Deserialize<ClientMatchTransition>(responseBytes, ClientEnvelope.Options)
            ?? throw new InvalidDataException("The native transition response decoded to null.");

        if (transition.Disposition != ClientTransitionDisposition.Accepted)
        {
            throw MatchCommandRejectedException.ForDisposition(transition.Disposition.ToString());
        }

        // The reconciliation rule, stated in code: the view's state is exactly what the authority
        // returned for this command, never a value this class predicted or interpolated.
        CurrentSnapshot = transition.PostSnapshot;
        LastEvents = transition.Events;
        LastInputLockTicks = transition.InputLockTicks;
        PresentationTickRate = transition.PresentationTickRate;

        if (transition.Events.Any(e => e is ClientTerrainChangedEvent))
        {
            CurrentTerrain = await _session.TerrainAsync(CurrentTerrain.Generation).ConfigureAwait(true);
        }

        return transition;
    }
}
