namespace QuickLook.Next.Core;

public sealed record DiagnosticsKnownLogs(
    DiagnosticsLogState AppLog,
    DiagnosticsLogState PreviousAppLog,
    DiagnosticsLogState RasterHostLog,
    DiagnosticsLogState PreviousRasterHostLog);

public static class DiagnosticsLogInventory
{
    public static DiagnosticsKnownLogs InspectKnownLogs()
        => InspectKnownLogsInDirectory(DiagLog.LogDirectory);

    internal static DiagnosticsKnownLogs InspectKnownLogsInDirectory(string directory)
        => new(
            Inspect(directory, "app.log"),
            Inspect(directory, "app.log.previous"),
            Inspect(directory, "raster-host.log"),
            Inspect(directory, "raster-host.log.previous"));

    private static DiagnosticsLogState Inspect(string directory, string fileName)
    {
        try
        {
            var file = new FileInfo(Path.Combine(directory, fileName));
            if (!file.Exists || (file.Attributes & FileAttributes.ReparsePoint) != 0)
                return default;
            return new DiagnosticsLogState(true, Math.Max(0, file.Length));
        }
        catch
        {
            return default;
        }
    }
}
