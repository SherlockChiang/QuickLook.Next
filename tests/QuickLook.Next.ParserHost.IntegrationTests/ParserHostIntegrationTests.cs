using System.Diagnostics;
using System.IO.Compression;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.ParserHost.IntegrationTests;

public sealed class ParserHostIntegrationTests
{
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(15);

    [Fact]
    public async Task Host_rejects_bad_session_token_without_becoming_ready()
    {
        string pipeName = $"quicklook_next_parser_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        using Process host = StartHost(pipeName, token);

        try
        {
            using var timeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token + "bad"), timeout.Token);

            Assert.Null(await channel.ReceiveAsync(timeout.Token));
            await host.WaitForExitAsync(timeout.Token);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            await StopHostAsync(host);
        }
    }

    [Fact]
    public async Task Authenticated_host_previews_generated_zip()
    {
        string tempDirectory = Path.Combine(Path.GetTempPath(), "QuickLookNextParserHostTests", Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(tempDirectory);
        string zipPath = Path.Combine(tempDirectory, "sample.zip");
        using (var archive = ZipFile.Open(zipPath, ZipArchiveMode.Create))
        {
            using StreamWriter writer = new(archive.CreateEntry("folder/integration-marker.txt").Open());
            writer.Write("parser host integration");
        }

        string pipeName = $"quicklook_next_parser_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        using Process host = StartHost(pipeName, token);

        try
        {
            using var timeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<ParserReady>(await channel.ReceiveAsync(timeout.Token));

            string requestId = Guid.NewGuid().ToString("n");
            var probe = new FileProbe(zipPath, ".zip", [0x50, 0x4B, 0x03, 0x04])
            {
                Kind = "archive",
                Size = new FileInfo(zipPath).Length,
            };
            await channel.SendAsync(new PreviewOpen(requestId, zipPath, probe), timeout.Token);
            ControlMessage? response = await channel.ReceiveAsync(timeout.Token);
            PreviewReady ready = Assert.IsType<PreviewReady>(response);
            Assert.Equal(requestId, ready.RequestId);
            Assert.Equal("archive", ready.Kind);
            Assert.Contains(ready.Listing!.Items, item => item.Name.Contains("integration-marker.txt", StringComparison.Ordinal));
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            await StopHostAsync(host);
            try { Directory.Delete(tempDirectory, recursive: true); } catch { }
        }
    }

    [Fact]
    public async Task Archive_entry_close_removes_successful_handoff()
    {
        string tempDirectory = Path.Combine(Path.GetTempPath(), "QuickLookNextParserHostTests", Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(tempDirectory);
        string zipPath = Path.Combine(tempDirectory, "extract.zip");
        const string entryName = "folder/extract-marker.txt";
        const string contents = "archive extraction integration";
        using (var archive = ZipFile.Open(zipPath, ZipArchiveMode.Create))
        {
            using StreamWriter writer = new(archive.CreateEntry(entryName).Open());
            writer.Write(contents);
        }

        string pipeName = $"quicklook_next_parser_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        using Process host = StartHost(pipeName, token);
        string? handoffPath = null;
        try
        {
            using var timeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<ParserReady>(await channel.ReceiveAsync(timeout.Token));

            string requestId = Guid.NewGuid().ToString("n");
            await channel.SendAsync(new ArchiveEntryExtract(requestId, zipPath, entryName), timeout.Token);
            ArchiveEntryExtracted extracted = Assert.IsType<ArchiveEntryExtracted>(await channel.ReceiveAsync(timeout.Token));
            handoffPath = extracted.TempPath;
            string handoffDirectory = Path.GetDirectoryName(handoffPath)!;
            Assert.True(TempHandoffPaths.IsArchiveExtractPath(handoffPath));
            Assert.Equal(contents, await File.ReadAllTextAsync(handoffPath, timeout.Token));

            await channel.SendAsync(new ArchiveEntryExtractClose(requestId), timeout.Token);
            await WaitUntilAsync(() => !File.Exists(handoffPath) && !Directory.Exists(handoffDirectory), timeout.Token);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            await StopHostAsync(host);
            if (handoffPath is not null) try { Directory.Delete(Path.GetDirectoryName(handoffPath)!, recursive: true); } catch { }
            try { Directory.Delete(tempDirectory, recursive: true); } catch { }
        }
    }

    [Fact]
    public async Task Generated_docx_returns_office_text_and_layout()
    {
        string tempDirectory = Path.Combine(Path.GetTempPath(), "QuickLookNextParserHostTests", Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(tempDirectory);
        string docxPath = Path.Combine(tempDirectory, "sample.docx");
        using (var archive = ZipFile.Open(docxPath, ZipArchiveMode.Create))
        {
            WriteEntry(archive, "[Content_Types].xml",
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>");
            WriteEntry(archive, "_rels/.rels",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>");
            WriteEntry(archive, "word/document.xml",
                "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>ParserHost DOCX marker</w:t></w:r></w:p><w:p><w:r><w:t>Second integration paragraph</w:t></w:r></w:p></w:body></w:document>");
        }

        string pipeName = $"quicklook_next_parser_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        using Process host = StartHost(pipeName, token);
        try
        {
            using var timeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<ParserReady>(await channel.ReceiveAsync(timeout.Token));

            string requestId = Guid.NewGuid().ToString("n");
            var probe = new FileProbe(docxPath, ".docx", [0x50, 0x4B, 0x03, 0x04])
            {
                Kind = "office",
                Size = new FileInfo(docxPath).Length,
            };
            await channel.SendAsync(new PreviewOpen(requestId, docxPath, probe), timeout.Token);
            PreviewReady ready = Assert.IsType<PreviewReady>(await channel.ReceiveAsync(timeout.Token));

            Assert.Equal("office", ready.Kind);
            Assert.Contains("ParserHost DOCX marker", ready.TextContent);
            Assert.Contains("Second integration paragraph", ready.TextContent);
            OfficeLayout layout = Assert.IsType<OfficeLayout>(ready.OfficeLayout);
            Assert.Equal("document", layout.LayoutKind);
            Assert.NotEmpty(layout.Pages);
            Assert.Contains(layout.Pages.SelectMany(page => page.Items),
                item => item.Text?.Contains("ParserHost DOCX marker", StringComparison.Ordinal) == true);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            await StopHostAsync(host);
            try { Directory.Delete(tempDirectory, recursive: true); } catch { }
        }
    }

    [Fact]
    public async Task Closing_inflight_archive_extract_suppresses_response_and_cleans_temp_file()
    {
        string tempDirectory = Path.Combine(Path.GetTempPath(), "QuickLookNextParserHostTests", Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(tempDirectory);
        string zipPath = Path.Combine(tempDirectory, "cancel.zip");
        string entryName = "cancel-" + Guid.NewGuid().ToString("n") + ".bin";
        using (var archive = ZipFile.Open(zipPath, ZipArchiveMode.Create))
        using (Stream output = archive.CreateEntry(entryName, CompressionLevel.NoCompression).Open())
        {
            byte[] block = new byte[64 * 1024];
            RandomNumberGenerator.Fill(block);
            for (int i = 0; i < 128; i++)
                output.Write(block);
        }

        string extractionRoot = Path.Combine(Path.GetTempPath(), "QuickLookNext", "archive-preview");
        HashSet<string> rootsBefore = EnumerateExtractionRoots(extractionRoot);
        string pipeName = $"quicklook_next_parser_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        using Process host = StartHost(pipeName, token);
        try
        {
            using var timeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<ParserReady>(await channel.ReceiveAsync(timeout.Token));

            string canceledId = Guid.NewGuid().ToString("n");
            await channel.SendAsync(new ArchiveEntryExtract(canceledId, zipPath, entryName), timeout.Token);
            await channel.SendAsync(new ArchiveEntryExtractClose(canceledId), timeout.Token);

            string previewId = Guid.NewGuid().ToString("n");
            var probe = new FileProbe(zipPath, ".zip", [0x50, 0x4B, 0x03, 0x04])
            {
                Kind = "archive",
                Size = new FileInfo(zipPath).Length,
            };
            await channel.SendAsync(new PreviewOpen(previewId, zipPath, probe), timeout.Token);
            PreviewReady ready = Assert.IsType<PreviewReady>(await channel.ReceiveAsync(timeout.Token));
            Assert.Equal(previewId, ready.RequestId);

            await WaitUntilAsync(() => EnumerateExtractionRoots(extractionRoot).IsSubsetOf(rootsBefore), timeout.Token);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            await StopHostAsync(host);
            try { Directory.Delete(tempDirectory, recursive: true); } catch { }
        }
    }

    private static Process StartHost(string pipeName, string token)
    {
        string hostPath = Path.Combine(AppContext.BaseDirectory, "ParserHost", "QuickLook.Next.ParserHost.exe");
        return Process.Start(new ProcessStartInfo(hostPath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            ArgumentList = { "--pipe", pipeName, "--session-token", token },
        }) ?? throw new InvalidOperationException("ParserHost did not start");
    }

    private static async Task StopHostAsync(Process host)
    {
        try { await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5)); }
        catch { try { host.Kill(entireProcessTree: true); } catch { } }
    }

    private static async Task WaitUntilAsync(Func<bool> condition, CancellationToken cancellationToken)
    {
        while (!condition())
            await Task.Delay(25, cancellationToken);
    }

    private static void WriteEntry(ZipArchive archive, string name, string contents)
    {
        using StreamWriter writer = new(archive.CreateEntry(name).Open());
        writer.Write(contents);
    }

    private static HashSet<string> EnumerateExtractionRoots(string root)
        => Directory.Exists(root)
            ? Directory.EnumerateDirectories(root, "extract-*").ToHashSet(StringComparer.OrdinalIgnoreCase)
            : [];
}
