using System.Text.Json;
using DungeonBarrage.Client.Contracts;

namespace DungeonBarrage.Client.Interop;

/// <summary>
/// Reads the launch roster from the native library.
/// </summary>
/// <remarks>
/// Deliberately not a <see cref="LocalMatchSession"/> member: the roster is static content, not
/// match state, and <c>db_sim_roster</c> needs no live handle, no submission gate, and nothing
/// to poison — there is no session for this to naturally belong to.
/// </remarks>
public static class RosterCatalog
{
    /// <summary>Fetches the full launch roster.</summary>
    /// <returns>Every starter character, in launch-roster order.</returns>
    /// <exception cref="NativeSimulationException">The native call failed.</exception>
    public static unsafe ClientRosterResponse Get()
    {
        DbSimNative.EnsureInitialized();

        var buffer = default(DbSimBuffer);
        try
        {
            var status = DbSimNative.Roster(&buffer);
            if (status != NativeStatus.Ok)
            {
                throw new NativeSimulationException("db_sim_roster", status);
            }

            var bytes = Copy(buffer);
            return JsonSerializer.Deserialize<ClientRosterResponse>(bytes, ClientEnvelope.Options)
                ?? throw new InvalidDataException("The native roster response decoded to null.");
        }
        finally
        {
            DbSimNative.BufferFree(&buffer);
        }
    }

    private static unsafe byte[] Copy(in DbSimBuffer buffer)
    {
        if (!buffer.HasPayload)
        {
            return [];
        }

        if (buffer.Len > (nuint)DbSimNative.MaxResponseBytes)
        {
            // The native side enforces the same ceiling; checking again before allocating means
            // a corrupted length can never turn into an enormous managed allocation.
            throw new NativeSimulationException("db_sim_response", NativeStatus.ResponseTooLarge);
        }

        return new ReadOnlySpan<byte>((void*)buffer.Ptr, checked((int)buffer.Len)).ToArray();
    }
}
