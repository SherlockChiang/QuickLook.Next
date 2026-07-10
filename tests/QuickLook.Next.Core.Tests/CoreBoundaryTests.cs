using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class CoreBoundaryTests : IDisposable
{
    private readonly string _tempRoot = Path.Combine(Path.GetTempPath(), "QuickLookNextTests", Guid.NewGuid().ToString("n"));

    [Fact]
    public void ProtocolJson_round_trips_hero_raster_message()
    {
        var message = new HeroRasterExtracted("0".PadLeft(32, '0'), "C:\\temp\\hero.bgra", 32, 24);
        string json = ProtocolJson.Serialize(message);

        Assert.Contains("\"type\":\"hero.raster.extracted\"", json);
        Assert.Equal(message, Assert.IsType<HeroRasterExtracted>(ProtocolJson.Deserialize(json)));
    }

    [Fact]
    public async Task Pending_request_times_out_and_rejects_late_result()
    {
        var pending = new PendingRequests();
        var (id, completion) = pending.Begin(TimeSpan.FromMilliseconds(50));

        await Assert.ThrowsAsync<TimeoutException>(() => completion);
        Assert.False(pending.TryComplete(id, new PreviewError(id, "late")));
    }

    [Fact]
    public void Archive_handoff_requires_direct_extract_child_and_entry_name()
    {
        string directory = Path.Combine(_tempRoot, "QuickLookNext", "archive-preview", "extract-abc");
        Directory.CreateDirectory(directory);
        string valid = Path.Combine(directory, "entry-file.txt");
        string invalid = Path.Combine(directory, "file.txt");
        File.WriteAllText(valid, "x");
        File.WriteAllText(invalid, "x");

        Assert.True(TempHandoffPaths.IsArchiveExtractPath(valid, _tempRoot));
        Assert.False(TempHandoffPaths.IsArchiveExtractPath(invalid, _tempRoot));
    }

    [Fact]
    public void Hero_handoff_requires_matching_request_directory()
    {
        const string requestId = "0123456789abcdef0123456789abcdef";
        string directory = Path.Combine(_tempRoot, "QuickLookNext", "parser-raster", "raster-" + requestId);
        Directory.CreateDirectory(directory);
        string valid = Path.Combine(directory, "hero.bgra");
        File.WriteAllText(valid, "x");

        Assert.True(TempHandoffPaths.IsHeroRasterPath(valid, requestId, _tempRoot));
        Assert.False(TempHandoffPaths.IsHeroRasterPath(valid, "f".PadLeft(32, 'f'), _tempRoot));
    }

    public void Dispose()
    {
        try { Directory.Delete(_tempRoot, recursive: true); } catch { }
    }
}
