using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class CoreBoundaryTests : IDisposable
{
    private readonly string _tempRoot = Path.Combine(Path.GetTempPath(), "QuickLookNextTests", Guid.NewGuid().ToString("n"));

    [Theory]
    [InlineData(FileAttributes.Offline)]
    [InlineData((FileAttributes)0x00040000)]
    [InlineData((FileAttributes)0x00400000)]
    [InlineData(FileAttributes.Archive | (FileAttributes)0x00400000)]
    public void Cloud_file_status_detects_recall_attributes(FileAttributes attributes)
        => Assert.True(CloudFileStatus.MayRequireHydration(attributes));

    [Fact]
    public void Cloud_file_status_ignores_local_file_attributes()
        => Assert.False(CloudFileStatus.MayRequireHydration(FileAttributes.Archive | FileAttributes.ReadOnly));

    [Theory]
    [InlineData(0x9000001A)]
    [InlineData(0x9000101A)]
    [InlineData(0x9000F01A)]
    public void Cloud_file_status_recognizes_cloud_reparse_tags(uint reparseTag)
        => Assert.True(CloudFileStatus.IsCloudReparseTag(reparseTag));

    [Theory]
    [InlineData(0xA000000C)]
    [InlineData(0x8000001B)]
    [InlineData(0u)]
    public void Cloud_file_status_rejects_unrelated_reparse_tags(uint reparseTag)
        => Assert.False(CloudFileStatus.IsCloudReparseTag(reparseTag));

    [Fact]
    public void Cloud_file_status_fails_closed_when_path_cannot_be_inspected()
    {
        string missingPath = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("n"), "missing.dat");

        Assert.Equal(CloudFileAvailability.Unknown, CloudFileStatus.GetAvailability(missingPath));
        Assert.True(CloudFileStatus.MayRequireHydration(missingPath));
    }

    [Theory]
    [InlineData("cloud.config", "text")]
    [InlineData("cloud.pdf", "pdf")]
    [InlineData("cloud.docx", "office")]
    [InlineData("cloud.png", "image")]
    [InlineData("cloud.zip", "archive")]
    [InlineData("cloud.mp4", "video")]
    [InlineData("cloud.cer", "certificate")]
    [InlineData("cloud.vendor", "unknown")]
    public void Metadata_only_probe_routes_without_content(string fileName, string expectedKind)
    {
        FileProbe probe = FallbackFileProbe.CreateMetadataOnlyProbe(fileName);
        Assert.Equal(expectedKind, probe.Kind);
        Assert.Empty(probe.MagicPrefix);
    }

    [Theory]
    [InlineData("app.config")]
    [InlineData("settings.cnf")]
    [InlineData("install.inf")]
    [InlineData("connection.rdp")]
    [InlineData("Dockerfile")]
    [InlineData(".editorconfig")]
    public void Fallback_probe_recognizes_known_text_configs(string fileName)
        => Assert.True(FallbackFileProbe.IsText(fileName, []));

    [Fact]
    public void Fallback_probe_sniffs_unknown_utf8_and_utf16_text()
    {
        Assert.True(FallbackFileProbe.IsText("settings.vendor", "feature=true\r\n"u8));
        Assert.True(FallbackFileProbe.IsText("settings.vendor", [0xFF, 0xFE, (byte)'W', 0, (byte)'i', 0]));
        Assert.True(FallbackFileProbe.IsText("settings.vendor", [], isEmptyFile: true));
        Assert.False(FallbackFileProbe.IsText("settings.vendor", [], isEmptyFile: false));
    }

    [Fact]
    public void Fallback_probe_rejects_unknown_binary_prefixes()
    {
        Assert.False(FallbackFileProbe.IsText("data.vendor", [0, 1, 2, 3]));
        Assert.False(FallbackFileProbe.IsText("data.vendor", [0xFF, 0xD9, 0x80]));
        Assert.False(FallbackFileProbe.IsText("data.vendor", "MZprintable header"u8));
        Assert.False(FallbackFileProbe.IsText("data.vendor", "%PDF-readable header"u8));
        Assert.False(FallbackFileProbe.IsText("payload.config", "MZprintable header"u8));
        Assert.False(FallbackFileProbe.IsText("document.config", "%PDF-readable header"u8));
        Assert.False(FallbackFileProbe.IsText("data.vendor", [0xFF, 0xFE, 0x00, 0xD8]));
    }

    [Fact]
    public void Fallback_text_preview_decodes_windows_1252_config()
    {
        Directory.CreateDirectory(_tempRoot);
        string path = Path.Combine(_tempRoot, "legacy.ini");
        File.WriteAllBytes(path, [.. "name=caf"u8.ToArray(), 0xE9]);

        PreviewReady ready = Assert.IsType<PreviewReady>(
            FallbackFileProbe.TryCreateTextPreview("request", path, CancellationToken.None));

        Assert.Equal("name=café", ready.TextContent);
        Assert.Equal("ini", ready.TextLanguage);
    }

    [Fact]
    public void Fallback_text_preview_is_bounded()
    {
        Directory.CreateDirectory(_tempRoot);
        string path = Path.Combine(_tempRoot, "large.config");
        File.WriteAllText(path, new string('a', 512 * 1024 + 10));

        PreviewReady ready = Assert.IsType<PreviewReady>(
            FallbackFileProbe.TryCreateTextPreview("request", path, CancellationToken.None));

        Assert.EndsWith("[Preview truncated at 524288 bytes]", ready.TextContent);
        Assert.Equal("xml", ready.TextLanguage);
    }

    [Fact]
    public void ProtocolJson_round_trips_hero_raster_message()
    {
        var message = new HeroRasterExtracted("0".PadLeft(32, '0'), 1234, 3080, 32, 24);
        string json = ProtocolJson.Serialize(message);

        Assert.Contains("\"type\":\"hero.raster.extracted\"", json);
        Assert.Equal(message, Assert.IsType<HeroRasterExtracted>(ProtocolJson.Deserialize(json)));
    }

    [Fact]
    public void ProtocolJson_round_trips_animation_handoff_message()
    {
        var message = new PreviewAnimationFramesReady(
            "0".PadLeft(32, '0'), "1".PadLeft(32, '1'), 1234, 3, 32, 24, 9232);
        string json = ProtocolJson.Serialize(message);

        Assert.Contains("\"type\":\"preview.animation.ready\"", json);
        Assert.Equal(message, Assert.IsType<PreviewAnimationFramesReady>(ProtocolJson.Deserialize(json)));
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

    [Fact]
    public void Animation_handoff_requires_matching_request_directory()
    {
        const string requestId = "0123456789abcdef0123456789abcdef";
        string directory = Path.Combine(_tempRoot, "QuickLookNext", "raster-animation", "frames-" + requestId);
        Directory.CreateDirectory(directory);
        string valid = Path.Combine(directory, "frames.bin");
        File.WriteAllText(valid, "x");

        Assert.True(TempHandoffPaths.IsRasterAnimationPath(valid, requestId, _tempRoot));
        Assert.False(TempHandoffPaths.IsRasterAnimationPath(valid, "f".PadLeft(32, 'f'), _tempRoot));
        Assert.False(TempHandoffPaths.IsRasterAnimationPath(valid, "not-hex", _tempRoot));

        string nested = Path.Combine(directory, "nested");
        Directory.CreateDirectory(nested);
        string nestedFile = Path.Combine(nested, "frames.bin");
        File.WriteAllText(nestedFile, "x");
        Assert.False(TempHandoffPaths.IsRasterAnimationPath(nestedFile, requestId, _tempRoot));
    }

    [Fact]
    public void Handoffs_reject_outside_and_nested_paths()
    {
        string archiveRoot = Path.Combine(_tempRoot, "QuickLookNext", "archive-preview");
        string nested = Path.Combine(archiveRoot, "extract-abc", "nested");
        Directory.CreateDirectory(nested);
        string nestedFile = Path.Combine(nested, "entry-file.txt");
        File.WriteAllText(nestedFile, "x");
        string outside = Path.Combine(_tempRoot, "entry-outside.txt");
        File.WriteAllText(outside, "x");

        Assert.False(TempHandoffPaths.IsArchiveExtractPath(nestedFile, _tempRoot));
        Assert.False(TempHandoffPaths.IsArchiveExtractPath(outside, _tempRoot));
    }

    [Fact]
    public void Hero_handoff_rejects_invalid_request_id_and_nested_file()
    {
        const string requestId = "0123456789abcdef0123456789abcdef";
        string nested = Path.Combine(_tempRoot, "QuickLookNext", "parser-raster", "raster-" + requestId, "nested");
        Directory.CreateDirectory(nested);
        string path = Path.Combine(nested, "hero.bgra");
        File.WriteAllText(path, "x");

        Assert.False(TempHandoffPaths.IsHeroRasterPath(path, requestId, _tempRoot));
        Assert.False(TempHandoffPaths.IsHeroRasterPath(path, "not-hex", _tempRoot));
    }

    [Fact]
    public void Archive_handoff_rejects_symbolic_link_when_supported()
    {
        string root = Path.Combine(_tempRoot, "QuickLookNext", "archive-preview");
        string target = Path.Combine(_tempRoot, "target");
        Directory.CreateDirectory(root);
        Directory.CreateDirectory(target);
        File.WriteAllText(Path.Combine(target, "entry-file.txt"), "x");
        string link = Path.Combine(root, "extract-link");
        try
        {
            Directory.CreateSymbolicLink(link, target);
        }
        catch (Exception ex) when (ex is UnauthorizedAccessException or IOException or PlatformNotSupportedException)
        {
            return;
        }

        Assert.False(TempHandoffPaths.IsArchiveExtractPath(Path.Combine(link, "entry-file.txt"), _tempRoot));
    }

    public void Dispose()
    {
        try { Directory.Delete(_tempRoot, recursive: true); } catch { }
    }
}
