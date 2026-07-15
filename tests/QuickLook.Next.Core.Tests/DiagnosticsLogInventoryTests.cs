using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class DiagnosticsLogInventoryTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), "QuickLookNextDiagnosticsTests", Guid.NewGuid().ToString("n"));

    [Fact]
    public void Inventory_checks_only_four_fixed_log_names_without_reading_contents()
    {
        Directory.CreateDirectory(_root);
        File.WriteAllBytes(Path.Combine(_root, "app.log"), new byte[17]);
        File.WriteAllBytes(Path.Combine(_root, "raster-host.log.previous"), new byte[29]);
        File.WriteAllText(Path.Combine(_root, "secret.txt"), "password=do-not-collect");
        Directory.CreateDirectory(Path.Combine(_root, "nested"));
        File.WriteAllText(Path.Combine(_root, "nested", "parser-host.log"), "do-not-collect");
        using FileStream locked = new(
            Path.Combine(_root, "app.log"),
            FileMode.Open,
            FileAccess.Read,
            FileShare.None);

        DiagnosticsKnownLogs logs = DiagnosticsLogInventory.InspectKnownLogsInDirectory(_root);

        Assert.Equal(new DiagnosticsLogState(true, 17), logs.AppLog);
        Assert.Equal(default, logs.PreviousAppLog);
        Assert.Equal(default, logs.RasterHostLog);
        Assert.Equal(new DiagnosticsLogState(true, 29), logs.PreviousRasterHostLog);
    }

    public void Dispose()
    {
        try { Directory.Delete(_root, recursive: true); } catch { }
    }
}
