using System.Reflection;
using DungeonBarrage.Client.Interop;
using Godot;

namespace DungeonBarrage.Client.App;

internal sealed record BuildDiagnostics(
    string ClientVersion,
    string GodotVersion,
    uint AbiVersion,
    uint SimulationVersion,
    uint ContentVersion)
{
    internal static BuildDiagnostics Capture()
    {
        var assembly = typeof(BuildDiagnostics).Assembly;
        var clientVersion = assembly
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?
            .InformationalVersion
            ?? assembly.GetName().Version?.ToString()
            ?? "unknown";
        var godotVersion = Engine.GetVersionInfo()["string"].ToString();

        return new BuildDiagnostics(
            clientVersion,
            godotVersion,
            LocalMatchSession.AbiVersion,
            LocalMatchSession.SimulationVersion,
            LocalMatchSession.ContentVersion);
    }

    internal string DisplayText =>
        $"Client {ClientVersion}  |  Godot {GodotVersion}\n" +
        $"Native ABI {AbiVersion}  |  Simulation {SimulationVersion}  |  Content {ContentVersion}";
}
