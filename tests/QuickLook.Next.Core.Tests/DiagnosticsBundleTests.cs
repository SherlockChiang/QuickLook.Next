using System.IO.Compression;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class DiagnosticsBundleTests
{
    [Fact]
    public async Task Bundle_contains_only_fixed_metadata_entries()
    {
        using var output = new MemoryStream();
        await DiagnosticsBundle.WriteAsync(output, Snapshot(), new DateTimeOffset(2026, 7, 15, 12, 34, 56, TimeSpan.FromHours(8)));

        Assert.InRange(output.Length, 1, DiagnosticsBundle.MaxBundleBytes);
        using var archive = new ZipArchive(new MemoryStream(output.ToArray()), ZipArchiveMode.Read);
        Assert.Equal(2, archive.Entries.Count);
        Assert.Contains(archive.Entries, entry => entry.FullName == "diagnostics.json");
        Assert.Contains(archive.Entries, entry => entry.FullName == "README.txt");
        Assert.All(archive.Entries, entry =>
        {
            Assert.DoesNotContain("..", entry.FullName);
            Assert.DoesNotContain('\\', entry.FullName);
            Assert.False(Path.IsPathRooted(entry.FullName));
        });

        using JsonDocument json = JsonDocument.Parse(await ReadEntryAsync(archive, "diagnostics.json"));
        Assert.Equal(1, json.RootElement.GetProperty("schemaVersion").GetInt32());
        Assert.Equal("2026-07-15T04:34:00Z", json.RootElement.GetProperty("generatedUtc").GetString());
        Assert.False(json.RootElement.GetProperty("privacy").GetProperty("logContentsIncluded").GetBoolean());
        Assert.Equal(4, json.RootElement.GetProperty("diagnosticLogs").GetProperty("known").GetArrayLength());
    }

    [Fact]
    public async Task Bundle_rejects_free_form_canaries()
    {
        const string canary = @"C:\Users\Alice\Client Secret\medical-record.pdf";
        DiagnosticsSnapshot snapshot = Snapshot() with
        {
            ApplicationVersion = canary,
            LanguageMode = "password=CorrectHorseBatteryStaple",
            AnimationMode = "Bearer abcdef0123456789",
        };
        using var output = new MemoryStream();

        await DiagnosticsBundle.WriteAsync(output, snapshot, DateTimeOffset.UtcNow);

        string archiveBytes = Encoding.UTF8.GetString(output.ToArray());
        Assert.DoesNotContain(canary, archiveBytes, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("CorrectHorseBatteryStaple", archiveBytes, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain("abcdef0123456789", archiveBytes, StringComparison.OrdinalIgnoreCase);
        using var archive = new ZipArchive(new MemoryStream(output.ToArray()), ZipArchiveMode.Read);
        string json = await ReadEntryAsync(archive, "diagnostics.json");
        Assert.Contains("\"version\": \"unknown\"", json);
        Assert.Contains("\"languageMode\": \"unknown\"", json);
        Assert.Contains("\"animationMode\": \"unknown\"", json);
    }

    [Fact]
    public async Task Bundle_honors_cancellation_before_writing()
    {
        using var output = new MemoryStream();
        using var cancellation = new CancellationTokenSource();
        cancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() =>
            DiagnosticsBundle.WriteAsync(output, Snapshot(), DateTimeOffset.UtcNow, cancellation.Token));

        Assert.Equal(0, output.Length);
    }

    private static DiagnosticsSnapshot Snapshot() => new()
    {
        ApplicationVersion = "0.2.5-test",
        ProcessArchitecture = Architecture.X64,
        IsPackaged = false,
        FrameworkVersion = new Version(10, 0, 1),
        OsVersion = new Version(10, 0, 26100),
        SettingsSchemaVersion = 1,
        LanguageMode = "system",
        AnimationMode = "still",
        NativeBridgePresent = true,
        RasterHostPresent = true,
        ParserHostPresent = true,
        AppLog = new DiagnosticsLogState(true, 1234),
        PreviousAppLog = new DiagnosticsLogState(false, 999),
        RasterHostLog = new DiagnosticsLogState(true, 5678),
        PreviousRasterHostLog = new DiagnosticsLogState(false, 0),
    };

    private static async Task<string> ReadEntryAsync(ZipArchive archive, string name)
    {
        ZipArchiveEntry entry = Assert.Single(archive.Entries, entry => entry.FullName == name);
        using var reader = new StreamReader(entry.Open(), Encoding.UTF8);
        return await reader.ReadToEndAsync();
    }
}
