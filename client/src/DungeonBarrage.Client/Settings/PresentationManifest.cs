using DungeonBarrage.Client.Contracts;
using DungeonBarrage.Client.Interop;

namespace DungeonBarrage.Client.Settings;

/// <summary>
/// Godot file loader for the presentation manifest. Version and appearance checks live in
/// <see cref="DungeonBarrage.Client.Interop.PresentationManifest"/> so they can run without the engine.
/// </summary>
internal static class PresentationManifest
{
    private const string ResourcePath = "res://Settings/presentation-manifest-v1.json";

    /// <summary>
    /// Reads <c>presentation-manifest-v1.json</c> from the Godot project and runs the same
    /// validation Confirm uses.
    /// </summary>
    /// <param name="request">The create envelope about to be submitted.</param>
    /// <param name="nativeContentVersion">Native <c>CONTENT_VERSION</c>.</param>
    /// <returns>The decoded manifest.</returns>
    internal static PresentationManifestDocument LoadAndValidate(
        ClientCreateRequest request,
        uint nativeContentVersion)
    {
        using var file = Godot.FileAccess.Open(ResourcePath, Godot.FileAccess.ModeFlags.Read);
        if (file is null)
        {
            throw new InvalidDataException(
                $"The presentation manifest could not be opened: {ResourcePath} " +
                $"({Godot.FileAccess.GetOpenError()}).");
        }

        // `GetLength()` returns `ulong`; `GetBuffer` takes `long`. A manifest large enough to
        // overflow that conversion could not exist as a Godot text resource in the first place,
        // so this is a defensive bound rather than a realistic runtime path.
        var bytes = file.GetBuffer(checked((long)file.GetLength()));
        return DungeonBarrage.Client.Interop.PresentationManifest.Validate(
            bytes,
            request,
            nativeContentVersion);
    }
}
