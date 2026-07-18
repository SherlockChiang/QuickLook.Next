using System.Diagnostics;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class CoreBoundaryTests : IDisposable
{
    [Fact]
    public void Text_line_index_preserves_mixed_separators_and_trailing_line()
    {
        const string text = "alpha\r\nbeta\rgamma\n";
        TextLineIndex index = TextLineIndex.Create(text);

        Assert.Equal(
            [new TextLineRange(1, 0, 5), new(2, 7, 4), new(3, 12, 5), new(4, 18, 0)],
            index.Lines);
        Assert.Equal(0, index.FindLineIndex(0));
        Assert.Equal(1, index.FindLineIndex(7));
        Assert.Equal(3, index.FindLineIndex(text.Length));
    }

    [Fact]
    public void Table_presentation_policy_bounds_untrusted_rows_cells_and_text()
    {
        var source = new PreviewTable("csv")
        {
            Headers = Enumerable.Range(0, TablePresentationPolicy.MaxColumns + 1).Select(i => $"H{i}").ToArray(),
            Rows = Enumerable.Range(0, TablePresentationPolicy.MaxRows + 1)
                .Select(_ => new PreviewTableRow([new string('x', TablePresentationPolicy.MaxCellCharacters + 1)]))
                .ToArray(),
            TotalRows = TablePresentationPolicy.MaxRows + 1,
            TotalColumns = TablePresentationPolicy.MaxColumns + 1,
        };

        PreviewTable bounded = TablePresentationPolicy.Bound(source);

        Assert.True(bounded.IsPartial);
        Assert.Equal(TablePresentationPolicy.MaxColumns, bounded.Headers.Length);
        Assert.True(bounded.Rows.Length <= TablePresentationPolicy.MaxRows);
        Assert.All(bounded.Rows, row => Assert.All(row.Cells, cell => Assert.True(cell.Length <= TablePresentationPolicy.MaxCellCharacters)));
        Assert.True(bounded.Rows.Sum(row => row.Cells.Sum(cell => cell.Length))
            + bounded.Headers.Sum(header => header.Length) <= TablePresentationPolicy.MaxCharacters);
    }
    private readonly string _tempRoot = Path.Combine(Path.GetTempPath(), "QuickLookNextTests", Guid.NewGuid().ToString("n"));

    [Fact]
    public void Text_search_matches_case_insensitively_without_overlap()
        => Assert.Equal([0, 4], TextSearchIndex.FindMatches("Testtest", "test"));

    [Theory]
    [InlineData("automatic", "plain", false, true)]
    [InlineData("automatic", "code", false, false)]
    [InlineData("always", "code", false, true)]
    [InlineData("never", "plain", false, false)]
    [InlineData("never", "markdown", false, true)]
    [InlineData("never", "code", true, true)]
    public void Text_wrapping_policy_preserves_markdown_layout(
        string mode,
        string format,
        bool structuredMarkdown,
        bool expected)
        => Assert.Equal(expected, TextWrappingPolicy.ShouldWrap(mode, format, structuredMarkdown));

    [Fact]
    public void Listing_filter_matches_names_only_in_current_level()
    {
        PreviewListingItem[] items =
        [
            new("Root.txt", "Root.txt", "", false),
            new("Docs", "Docs/", "", true),
            new("Guide.txt", "Docs/Guide.txt", "Docs/", false),
            new("Image.png", "Docs/Image.png", "Docs/", false),
        ];

        IReadOnlyList<PreviewListingItem> root = ListingFilter.CurrentLevel(items, "", "root");
        IReadOnlyList<PreviewListingItem> docs = ListingFilter.CurrentLevel(items, "Docs/", ".txt");

        Assert.Equal("Root.txt", Assert.Single(root).Name);
        Assert.Equal("Guide.txt", Assert.Single(docs).Name);
    }

    [Fact]
    public void Listing_filter_caps_untrusted_input_items()
    {
        PreviewListingItem[] items = Enumerable.Range(0, ListingFilter.MaxItems + 1)
            .Select(index => new PreviewListingItem($"Item {index}", $"Item {index}", "", false))
            .ToArray();

        Assert.Equal(ListingFilter.MaxItems, ListingFilter.CurrentLevel(items, "", "").Count);
    }

    [Theory]
    [InlineData("archive")]
    [InlineData("PACKAGE")]
    [InlineData("office")]
    [InlineData("text")]
    [InlineData("ebook")]
    [InlineData("executable")]
    [InlineData("torrent")]
    [InlineData("certificate")]
    public void Parser_host_policy_accepts_registered_kinds(string kind)
        => Assert.True(PreviewFormatPolicy.UsesParserHost(kind));

    [Theory]
    [InlineData("image")]
    [InlineData("pdf")]
    [InlineData("unknown")]
    public void Parser_host_policy_rejects_unregistered_kinds(string kind)
        => Assert.False(PreviewFormatPolicy.UsesParserHost(kind));

    [Fact]
    public void Native_abi_rejects_mismatched_versions()
    {
        NativeAbi.EnsureCompatible(NativeAbi.Version);
        Assert.Throws<InvalidOperationException>(() => NativeAbi.EnsureCompatible(NativeAbi.Version + 1));
    }

    [Theory]
    [InlineData("file.vhdx", "disk-image")]
    [InlineData("font.woff2", "font")]
    [InlineData("mail.eml", "mail")]
    [InlineData("data.sqlite", "database")]
    [InlineData("dump.mdmp", "dump")]
    [InlineData("library.so", "elf")]
    public void Metadata_probe_preserves_registered_native_kinds(string path, string expectedKind)
        => Assert.Equal(expectedKind, FallbackFileProbe.CreateMetadataOnlyProbe(path).Kind);

    [Fact]
    public void Preview_listing_json_preserves_encrypted_archive_metadata()
    {
        const string json = """
            {"kind":"archive","title":"secure.zip","listing":{"rootName":"secure.zip","rootPath":"","listingKind":"archive","summary":"1 file","isPartial":false,"encryptedFileCount":1,"items":[{"name":"secret.txt","path":"secret.txt","parentPath":"","isFolder":false,"size":6,"packedSize":6,"modifiedUnix":0,"type":"TXT File","isEncrypted":true}]}}
            """;

        Assert.True(PreviewReadyJson.TryParse("request", json, out PreviewReady? ready, out string? error), error);
        Assert.NotNull(ready?.Listing);
        Assert.Equal(1, ready.Listing.EncryptedFileCount);
        Assert.True(Assert.Single(ready.Listing.Items).IsEncrypted);
    }

    [Theory]
    [InlineData(".AVIF", "avif", true)]
    [InlineData(".heif", "heic", true)]
    [InlineData(".JXL", "jxl", true)]
    [InlineData(".jpeg", "jpeg", false)]
    [InlineData(".webp", "webp", false)]
    [InlineData("C:\\private\\image.avif", null, false)]
    [InlineData(".unknown", null, false)]
    public void Image_codec_policy_normalizes_only_allowlisted_extensions(
        string extension,
        string? expectedFormat,
        bool requiresSystemCodec)
    {
        Assert.Equal(expectedFormat, ImageCodecPolicy.NormalizeFormat(extension));
        Assert.Equal(requiresSystemCodec, ImageCodecPolicy.RequiresSystemCodec(extension));
    }

    [Fact]
    public void Markdown_search_index_uses_visible_ast_content()
    {
        var document = new PreviewMarkdown
        {
            IsPartial = true,
            Blocks =
            [
                new PreviewMarkdownBlock("heading")
                {
                    Inlines = [new PreviewMarkdownInline("link") { Text = "Docs", Url = "https://example.test" }],
                },
                new PreviewMarkdownBlock("unorderedList")
                {
                    Children = [new PreviewMarkdownBlock("item") { Text = "First item" }],
                },
                new PreviewMarkdownBlock("table")
                {
                    TableHeaders = ["Name"],
                    TableRows = [["QuickLook"]],
                },
            ],
        };

        Assert.Equal(
            "Docs (https://example.test)\nFirst item\nName\nQuickLook\nPartial",
            TextSearchIndex.BuildMarkdownVisibleText(document, "Partial"));
    }

    [Fact]
    public void Markdown_table_search_index_obeys_cell_budget()
    {
        var table = new PreviewMarkdownBlock("table")
        {
            TableHeaders = Enumerable.Range(0, 100).Select(index => $"H{index}").ToArray(),
            TableRows = Enumerable.Range(0, 200)
                .Select(row => Enumerable.Range(0, 100).Select(column => $"{row}:{column}").ToArray())
                .ToArray(),
        };

        string[] cells = TextSearchIndex.MarkdownTableText(table).Split('\n');

        Assert.Equal(TextSearchIndex.MaxMarkdownTableCells, cells.Length);
        Assert.Contains("H63", cells);
        Assert.DoesNotContain("H64", cells);
    }

    [Fact]
    public void Markdown_inline_search_index_obeys_depth_budget()
    {
        PreviewMarkdownInline inline = new("text") { Text = "leaf" };
        for (int depth = 0; depth < 1000; depth++)
            inline = new PreviewMarkdownInline("strong") { Text = $"depth-{depth}", Children = [inline] };

        string text = TextSearchIndex.MarkdownInlineText([inline], "root");

        Assert.StartsWith("depth-", text);
        Assert.DoesNotContain("leaf", text);
    }

    [Fact]
    public void Markdown_search_segments_map_visible_offsets()
    {
        var document = new PreviewMarkdown
        {
            Blocks =
            [
                new PreviewMarkdownBlock("paragraph") { Text = "Alpha" },
                new PreviewMarkdownBlock("table")
                {
                    TableHeaders = ["Name"],
                    TableRows = [["QuickLook"]],
                },
            ],
        };

        MarkdownVisibleTextIndex index = TextSearchIndex.BuildMarkdownVisibleTextIndex(document, "Partial");

        Assert.Equal("Alpha\nName\nQuickLook", index.Text);
        Assert.Equal(
            [new MarkdownVisibleSegment(0, "Alpha"), new(6, "Name"), new(11, "QuickLook")],
            index.Segments);
    }

    [Fact]
    public void Markdown_search_index_excludes_unrendered_blocks_and_indexes_notice()
    {
        var document = new PreviewMarkdown
        {
            Blocks =
            [
                new PreviewMarkdownBlock("unorderedList")
                {
                    Children = Enumerable.Range(0, TextSearchIndex.MaxMarkdownBlocks + 1)
                        .Select(index => new PreviewMarkdownBlock("item") { Text = $"Item {index}" })
                        .ToArray(),
                },
            ],
        };

        MarkdownVisibleTextIndex index = TextSearchIndex.BuildMarkdownVisibleTextIndex(document, "Partial");

        Assert.Equal(TextSearchIndex.MaxMarkdownBlocks + 1, index.Segments.Count);
        Assert.DoesNotContain($"Item {TextSearchIndex.MaxMarkdownBlocks}", index.Text);
        Assert.Equal("Partial", index.Segments[^1].Text);
    }

    [Fact]
    public void Markdown_search_segments_preserve_offsets_after_empty_blocks()
    {
        var document = new PreviewMarkdown
        {
            Blocks =
            [
                new PreviewMarkdownBlock("paragraph"),
                new PreviewMarkdownBlock("paragraph") { Text = "Visible" },
            ],
        };

        MarkdownVisibleTextIndex index = TextSearchIndex.BuildMarkdownVisibleTextIndex(document, "Partial");

        Assert.Equal("\nVisible", index.Text);
        Assert.Equal(1, index.Segments[1].Start);
    }

    [Fact]
    public void Markdown_presentation_flattens_lists_tables_and_outline_indices()
    {
        var document = new PreviewMarkdown
        {
            Blocks =
            [
                new PreviewMarkdownBlock("heading") { Level = 2, Text = "Title" },
                new PreviewMarkdownBlock("unorderedList")
                {
                    Children = [new PreviewMarkdownBlock("listItem") { Text = "One" }, new PreviewMarkdownBlock("listItem") { Text = "Two" }],
                },
                new PreviewMarkdownBlock("table")
                {
                    TableHeaders = ["A", "B"],
                    TableRows = [["1", "2"], ["3", "4"]],
                },
            ],
        };

        MarkdownPresentation presentation = MarkdownPresentationPolicy.Flatten(document, "Partial");

        Assert.Equal(6, presentation.Items.Count);
        Assert.Equal(Enumerable.Range(0, 6), presentation.Items.Select(item => item.Index));
        Assert.Equal(["heading", "listItem", "listItem", "tableHeader", "tableRow", "tableRow"],
            presentation.Items.Select(item => item.Block.Kind));
        Assert.Equal(["", "- ", "- ", "", "", ""], presentation.Items.Select(item => item.Prefix));
        Assert.Equal("Title\nOne\nTwo\nA\nB\n1\n2\n3\n4", presentation.Text);
        Assert.Equal(presentation.Text, TextSearchIndex.BuildMarkdownVisibleText(document, "Partial"));
    }

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
    [InlineData("cloud.svg", "image")]
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
    public void ProtocolJson_round_trips_bounded_image_waveform()
    {
        var message = new PreviewSurface("request", 1234, 64, 32, 96, "B8G8R8A8_UNORM")
        {
            TransferId = "0".PadLeft(32, '0'),
            Waveform = new ImageWaveform(2, 2, new byte[12]),
        };

        string json = ProtocolJson.Serialize(message);

        PreviewSurface decoded = Assert.IsType<PreviewSurface>(ProtocolJson.Deserialize(json));
        Assert.Equal(message.RequestId, decoded.RequestId);
        Assert.Equal(message.TransferId, decoded.TransferId);
        Assert.NotNull(decoded.Waveform);
        Assert.Equal((2, 2), (decoded.Waveform.Width, decoded.Waveform.Height));
        Assert.Equal(message.Waveform.RgbDensity, decoded.Waveform.RgbDensity);
    }

    [Theory]
    [InlineData("null")]
    [InlineData("{\"width\":191,\"height\":96,\"rgbDensity\":\"AA==\"}")]
    [InlineData("{\"width\":192,\"height\":96,\"rgbDensity\":\"AA==\"}")]
    public void ImageWaveform_validation_rejects_malformed_protocol_payloads(string waveformJson)
    {
        string json = $$"""
            {"type":"preview.surface","requestId":"request","sharedHandle":1234,"width":64,"height":32,"dpi":96,"format":"B8G8R8A8_UNORM","waveform":{{waveformJson}}}
            """;

        PreviewSurface surface = Assert.IsType<PreviewSurface>(ProtocolJson.Deserialize(json));

        Assert.False(ImageWaveformBuilder.IsValid(surface.Waveform));
    }

    [Fact]
    public void ProtocolJson_round_trips_archive_handle_message()
    {
        var message = new ArchiveEntryExtracted("2".PadLeft(32, '2'), 1234, 4096, "folder/report.pdf");
        string json = ProtocolJson.Serialize(message);

        Assert.Contains("\"type\":\"archive.entry.extracted\"", json);
        Assert.DoesNotContain("tempPath", json, StringComparison.OrdinalIgnoreCase);
        Assert.Equal(message, Assert.IsType<ArchiveEntryExtracted>(ProtocolJson.Deserialize(json)));
    }

    [Fact]
    public void ProtocolJson_round_trips_preview_open_handle_message()
    {
        var probe = new FileProbe("C:\\logical.txt", ".txt", "text"u8.ToArray())
        {
            Kind = "text",
            Size = 4,
        };
        var message = new PreviewOpenHandle("3".PadLeft(32, '3'), 1234, 4, probe.Path, probe)
        {
            TargetWidth = 800,
            TargetHeight = 600,
        };
        string json = ProtocolJson.Serialize(message);

        Assert.Contains("\"type\":\"preview.open.handle\"", json);
        PreviewOpenHandle roundTrip = Assert.IsType<PreviewOpenHandle>(ProtocolJson.Deserialize(json));
        Assert.Equal(message.RequestId, roundTrip.RequestId);
        Assert.Equal(message.SourceHandle, roundTrip.SourceHandle);
        Assert.Equal(message.SourceLength, roundTrip.SourceLength);
        Assert.Equal(message.LogicalPath, roundTrip.LogicalPath);
        Assert.Equal(message.TargetWidth, roundTrip.TargetWidth);
        Assert.Equal(message.TargetHeight, roundTrip.TargetHeight);
        Assert.Equal(message.Probe.Kind, roundTrip.Probe.Kind);
        Assert.Equal(message.Probe.MagicPrefix, roundTrip.Probe.MagicPrefix);
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
    public void Read_only_handoff_handle_survives_source_close_and_path_delete()
    {
        Directory.CreateDirectory(_tempRoot);
        string path = Path.Combine(_tempRoot, "handoff.bin");
        byte[] expected = "immutable handoff"u8.ToArray();
        File.WriteAllBytes(path, expected);

        var source = WindowsHandleTransfer.OpenReadOnlyFile(path);
        using var duplicate = WindowsHandleTransfer.DuplicateFileFromProcess(
            Process.GetCurrentProcess().SafeHandle,
            source.Handle.DangerousGetHandle().ToInt64(),
            expected.Length);
        using var writeDuplicate = WindowsHandleTransfer.DuplicateFileFromProcess(
            Process.GetCurrentProcess().SafeHandle,
            source.Handle.DangerousGetHandle().ToInt64(),
            expected.Length);
        source.Handle.Dispose();
        File.Delete(path);

        using var stream = new FileStream(duplicate, FileAccess.Read);
        var actual = new byte[expected.Length];
        stream.ReadExactly(actual);
        Assert.Equal(expected, actual);
        Assert.Throws<UnauthorizedAccessException>(() => RandomAccess.Write(writeDuplicate, [0], 0));
    }

    [Fact]
    public void Handoff_handle_rejects_mismatched_length()
    {
        Directory.CreateDirectory(_tempRoot);
        string path = Path.Combine(_tempRoot, "handoff-length.bin");
        File.WriteAllBytes(path, [1, 2, 3]);
        var source = WindowsHandleTransfer.OpenReadOnlyFile(path);
        using (source.Handle)
        {
            Assert.Throws<InvalidDataException>(() => WindowsHandleTransfer.DuplicateFileFromProcess(
                Process.GetCurrentProcess().SafeHandle,
                source.Handle.DangerousGetHandle().ToInt64(),
                source.Length + 1));
        }
    }

    [Fact]
    public void Reopened_anchor_is_read_only_and_survives_the_writable_handle()
    {
        Directory.CreateDirectory(_tempRoot);
        string path = Path.Combine(_tempRoot, "reopened-anchor.bin");
        byte[] expected = "reopened anchor"u8.ToArray();
        using var writable = new FileStream(path, FileMode.CreateNew, FileAccess.ReadWrite,
            FileShare.Read | FileShare.Write | FileShare.Delete);
        writable.Write(expected);
        writable.Flush(flushToDisk: true);

        using var transitional = WindowsHandleTransfer.ReopenTransitionalReadOnlyFile(writable.SafeFileHandle, expected.Length);
        writable.Dispose();
        using var anchor = WindowsHandleTransfer.ReopenReadOnlyFile(transitional, expected.Length);
        using var stream = new FileStream(anchor, FileAccess.Read);
        var actual = new byte[expected.Length];
        stream.ReadExactly(actual);

        Assert.Equal(expected, actual);
        Assert.Throws<UnauthorizedAccessException>(() => RandomAccess.Write(anchor, [0], 0));
    }

    [Fact]
    public void Read_shared_anchor_blocks_path_replacement()
    {
        Directory.CreateDirectory(_tempRoot);
        string path = Path.Combine(_tempRoot, "anchored.txt");
        using var anchor = new FileStream(path, FileMode.CreateNew, FileAccess.ReadWrite, FileShare.Read);
        anchor.Write("anchored"u8);
        anchor.Flush();

        Assert.Throws<IOException>(() => File.Delete(path));
        Assert.Throws<IOException>(() => File.WriteAllText(path, "replaced"));
        using var reader = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
        using var text = new StreamReader(reader);
        Assert.Equal("anchored", text.ReadToEnd());
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
