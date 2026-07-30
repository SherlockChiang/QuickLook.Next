using System.ComponentModel;
using System.Diagnostics;
using System.IO.Compression;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.ParserHost.IntegrationTests;

[Collection("ParserHost integration")]
public sealed class OfficeImageSharedSectionTests
{
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(20);
    private static readonly byte[] TestPng = Convert.FromBase64String(
        "iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAYAAADED76LAAAAEklEQVR4nGP4z8DwHx9mGBkKAMLXf4EvceABAAAAAElFTkSuQmCC");

    [Fact]
    public async Task Published_office_image_ref_opens_bounded_section_and_fail_closed_requests_are_rejected()
    {
        string tempDirectory = CreateTempDirectory();
        string sourcePath = Path.Combine(tempDirectory, "physical-office.bin");
        string logicalPath = Path.Combine(tempDirectory, "missing-logical.docx");
        CreateDocxWithImage(sourcePath);

        await using ParserHostTestSession session = await ParserHostTestSession.StartAsync(Timeout);
        try
        {
            (string previewRequestId, PreviewReady preview) =
                await OpenOfficeHandleAsync(session, sourcePath, logicalPath);
            OfficeLayoutItem image = GetOnlyImage(preview);
            Assert.Equal("word/media/image1.png", image.ImageRef);
            Assert.Equal(TestPng.Length, image.ImageByteLength);
            Assert.Null(image.ImageBase64);
            Assert.False(File.Exists(logicalPath));
            Assert.False(TryOverwriteFile(sourcePath, "the retained source must stay pinned"));

            await AssertOfficeImageRejectedAsync(
                session,
                new OfficeImageOpen(
                    NewRequestId(),
                    NewRequestId(),
                    image.ImageRef!,
                    64,
                    64));
            await AssertOfficeImageRejectedAsync(
                session,
                new OfficeImageOpen(
                    NewRequestId(),
                    previewRequestId,
                    "word/media/not-published.png",
                    64,
                    64));
            await AssertOfficeImageRejectedAsync(
                session,
                new OfficeImageOpen(
                    NewRequestId(),
                    previewRequestId,
                    "word/media/../image1.png",
                    64,
                    64));
            await AssertOfficeImageRejectedAsync(
                session,
                new OfficeImageOpen(
                    NewRequestId(),
                    previewRequestId,
                    image.ImageRef!,
                    NativeAbi.MaxOfficeImageDimension + 1u,
                    64));

            const uint targetWidth = 6;
            const uint targetHeight = 7;
            string imageRequestId = NewRequestId();
            await session.Channel.SendAsync(
                new OfficeImageOpen(
                    imageRequestId,
                    previewRequestId,
                    image.ImageRef!,
                    targetWidth,
                    targetHeight),
                session.Token);
            OfficeImageReady ready =
                Assert.IsType<OfficeImageReady>(await session.Channel.ReceiveAsync(session.Token));
            Assert.Equal(imageRequestId, ready.RequestId);
            Assert.InRange(ready.Width, 1, (int)targetWidth);
            Assert.InRange(ready.Height, 1, (int)targetHeight);
            Assert.Equal(8L + (long)ready.Width * ready.Height * 4, ready.PacketLength);
            Assert.InRange(ready.PacketLength, 12, NativeAbi.MaxOfficeImagePacketBytes);

            using SharedSectionView view = SharedSectionView.DuplicateAndMapReadOnly(
                session.Host.SafeHandle,
                ready.SectionHandle,
                checked((int)ready.PacketLength));
            Assert.Equal(ready.Width, BitConverter.ToInt32(view.Bytes[..4]));
            Assert.Equal(ready.Height, BitConverter.ToInt32(view.Bytes[4..8]));
            Assert.Contains(view.Bytes[8..].ToArray(), static value => value != 0);

            await session.Channel.SendAsync(new OfficeImageClose(imageRequestId), session.Token);
            await WaitUntilAsync(
                () => !CanDuplicateSection(
                    session.Host,
                    ready.SectionHandle,
                    checked((int)ready.PacketLength)),
                session.Token);
            Assert.Equal(ready.Width, BitConverter.ToInt32(view.Bytes[..4]));

            await session.Channel.SendAsync(new PreviewClose(previewRequestId), session.Token);
            await WaitUntilAsync(
                () => TryOverwriteFile(sourcePath, "released after parent close"),
                session.Token);
            await AssertOfficeImageRejectedAsync(
                session,
                new OfficeImageOpen(
                    NewRequestId(),
                    previewRequestId,
                    image.ImageRef!,
                    64,
                    64));

            AssertNoOfficeImageTempArtifacts(session.WritableRoot);
        }
        finally
        {
            TryDeleteDirectory(tempDirectory);
        }
    }

    [Fact]
    public async Task Repeated_office_image_sections_release_leases_handles_and_temp_artifacts()
    {
        const int cycleCount = 32;
        const int handleGrowthBudget = 12;
        string tempDirectory = CreateTempDirectory();
        string sourcePath = Path.Combine(tempDirectory, "physical-office-cycle.bin");
        string logicalPath = Path.Combine(tempDirectory, "missing-cycle.docx");
        CreateDocxWithImage(sourcePath);

        await using ParserHostTestSession session =
            await ParserHostTestSession.StartAsync(TimeSpan.FromSeconds(45));
        try
        {
            (string previewRequestId, PreviewReady preview) =
                await OpenOfficeHandleAsync(session, sourcePath, logicalPath);
            OfficeLayoutItem image = GetOnlyImage(preview);
            Assert.False(TryOverwriteFile(sourcePath, "parent must remain retained"));

            int baselineHandles = 0;
            int peakHandles = 0;
            for (int cycle = 0; cycle <= cycleCount; cycle++)
            {
                string imageRequestId = NewRequestId();
                await session.Channel.SendAsync(
                    new OfficeImageOpen(
                        imageRequestId,
                        previewRequestId,
                        image.ImageRef!,
                        64,
                        64),
                    session.Token);
                OfficeImageReady ready =
                    Assert.IsType<OfficeImageReady>(await session.Channel.ReceiveAsync(session.Token));
                Assert.InRange(ready.Width, 1, 64);
                Assert.InRange(ready.Height, 1, 64);
                Assert.Equal(8L + (long)ready.Width * ready.Height * 4, ready.PacketLength);

                using (SharedSectionView view = SharedSectionView.DuplicateAndMapReadOnly(
                    session.Host.SafeHandle,
                    ready.SectionHandle,
                    checked((int)ready.PacketLength)))
                {
                    Assert.Equal(ready.Width, BitConverter.ToInt32(view.Bytes[..4]));
                    Assert.Equal(ready.Height, BitConverter.ToInt32(view.Bytes[4..8]));
                    await session.Channel.SendAsync(
                        new OfficeImageClose(imageRequestId),
                        session.Token);
                    await WaitUntilAsync(
                        () => !CanDuplicateSection(
                            session.Host,
                            ready.SectionHandle,
                            checked((int)ready.PacketLength)),
                        session.Token);
                    Assert.Equal(ready.Width, BitConverter.ToInt32(view.Bytes[..4]));
                }

                session.Host.Refresh();
                if (cycle == 0)
                {
                    baselineHandles = session.Host.HandleCount;
                    peakHandles = baselineHandles;
                }
                else
                {
                    peakHandles = Math.Max(peakHandles, session.Host.HandleCount);
                }
            }

            Assert.InRange(peakHandles, 1, baselineHandles + handleGrowthBudget);
            Assert.False(File.Exists(logicalPath));
            Assert.False(TryOverwriteFile(sourcePath, "parent still retained"));
            AssertNoOfficeImageTempArtifacts(session.WritableRoot);

            await session.Channel.SendAsync(new PreviewClose(previewRequestId), session.Token);
            await WaitUntilAsync(
                () => TryOverwriteFile(sourcePath, "released after cycles"),
                session.Token);
        }
        finally
        {
            TryDeleteDirectory(tempDirectory);
        }
    }

    [Fact]
    public async Task Pipe_disconnect_releases_unclosed_office_image_section_and_parent_source()
    {
        string tempDirectory = CreateTempDirectory();
        string sourcePath = Path.Combine(tempDirectory, "physical-office-disconnect.bin");
        string logicalPath = Path.Combine(tempDirectory, "missing-disconnect.docx");
        CreateDocxWithImage(sourcePath);

        await using ParserHostTestSession session = await ParserHostTestSession.StartAsync(Timeout);
        SharedSectionView? localView = null;
        try
        {
            (string previewRequestId, PreviewReady preview) =
                await OpenOfficeHandleAsync(session, sourcePath, logicalPath);
            OfficeLayoutItem image = GetOnlyImage(preview);
            string imageRequestId = NewRequestId();
            await session.Channel.SendAsync(
                new OfficeImageOpen(
                    imageRequestId,
                    previewRequestId,
                    image.ImageRef!,
                    64,
                    64),
                session.Token);
            OfficeImageReady ready =
                Assert.IsType<OfficeImageReady>(await session.Channel.ReceiveAsync(session.Token));
            localView = SharedSectionView.DuplicateAndMapReadOnly(
                session.Host.SafeHandle,
                ready.SectionHandle,
                checked((int)ready.PacketLength));
            Assert.Equal(ready.Width, BitConverter.ToInt32(localView.Bytes[..4]));
            Assert.False(TryOverwriteFile(sourcePath, "still retained before disconnect"));

            await session.DisconnectAndWaitForExitAsync();

            Assert.True(session.Host.HasExited);
            Assert.Equal(ready.Width, BitConverter.ToInt32(localView.Bytes[..4]));
            await WaitUntilAsync(
                () => TryOverwriteFile(sourcePath, "released after disconnect"),
                CancellationToken.None,
                TimeSpan.FromSeconds(5));
            AssertNoOfficeImageTempArtifacts(session.WritableRoot);
        }
        finally
        {
            localView?.Dispose();
            TryDeleteDirectory(tempDirectory);
        }
    }

    private static async Task<(string RequestId, PreviewReady Ready)> OpenOfficeHandleAsync(
        ParserHostTestSession session,
        string sourcePath,
        string logicalPath)
    {
        string requestId = NewRequestId();
        var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(sourcePath);
        try
        {
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(
                pinned.Handle,
                session.Host.SafeHandle);
            var probe = new FileProbe(logicalPath, ".docx", [0x50, 0x4B, 0x03, 0x04])
            {
                Kind = "office",
                Size = pinned.Length,
            };
            await session.Channel.SendAsync(
                new PreviewOpenHandle(
                    requestId,
                    hostHandle,
                    pinned.Length,
                    logicalPath,
                    probe),
                session.Token);
        }
        finally
        {
            pinned.Handle.Dispose();
        }

        ControlMessage? response = await session.Channel.ReceiveAsync(session.Token);
        Assert.False(
            response is PreviewError,
            response is PreviewError error ? error.Message : null);
        PreviewReady ready = Assert.IsType<PreviewReady>(response);
        Assert.Equal(requestId, ready.RequestId);
        Assert.Equal("office", ready.Kind);
        return (requestId, ready);
    }

    private static OfficeLayoutItem GetOnlyImage(PreviewReady preview)
    {
        OfficeLayout layout = Assert.IsType<OfficeLayout>(preview.OfficeLayout);
        return Assert.Single(
            layout.Pages.SelectMany(static page => page.Items),
            static item => item.Kind.Equals(
                    "image",
                    StringComparison.OrdinalIgnoreCase));
    }

    private static async Task AssertOfficeImageRejectedAsync(
        ParserHostTestSession session,
        OfficeImageOpen request)
    {
        await session.Channel.SendAsync(request, session.Token);
        PreviewError error =
            Assert.IsType<PreviewError>(await session.Channel.ReceiveAsync(session.Token));
        Assert.Equal(request.RequestId, error.RequestId);
    }

    private static void CreateDocxWithImage(string path)
    {
        using var archive = ZipFile.Open(path, ZipArchiveMode.Create);
        WriteEntry(
            archive,
            "word/document.xml",
            "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>Office shared image marker</w:t></w:r></w:p></w:body></w:document>");
        using Stream image = archive.CreateEntry(
            "word/media/image1.png",
            CompressionLevel.NoCompression).Open();
        image.Write(TestPng);
    }

    private static void WriteEntry(ZipArchive archive, string name, string contents)
    {
        using StreamWriter writer = new(archive.CreateEntry(name).Open());
        writer.Write(contents);
    }

    private static void AssertNoOfficeImageTempArtifacts(string writableRoot)
    {
        Assert.False(Directory.Exists(Path.Combine(writableRoot, "parser-raster")));
        Assert.False(Directory.Exists(Path.Combine(writableRoot, "parser-office-image")));
        Assert.False(Directory.Exists(Path.Combine(writableRoot, "office-image")));
        Assert.Empty(Directory.EnumerateFiles(
            writableRoot,
            "*.png",
            SearchOption.AllDirectories));
        Assert.DoesNotContain(
            Directory.EnumerateDirectories(
                writableRoot,
                "*",
                SearchOption.AllDirectories),
            static path =>
            {
                string name = Path.GetFileName(path);
                return name.Contains("raster", StringComparison.OrdinalIgnoreCase)
                    || name.Contains("office-image", StringComparison.OrdinalIgnoreCase);
            });
    }

    private static async Task WaitUntilAsync(
        Func<bool> condition,
        CancellationToken cancellationToken,
        TimeSpan? timeout = null)
    {
        using var localTimeout = timeout is null
            ? null
            : new CancellationTokenSource(timeout.Value);
        using var linked = localTimeout is null
            ? null
            : CancellationTokenSource.CreateLinkedTokenSource(
                cancellationToken,
                localTimeout.Token);
        CancellationToken effectiveToken = linked?.Token ?? cancellationToken;
        while (!condition())
            await Task.Delay(25, effectiveToken);
    }

    private static bool CanDuplicateSection(Process host, long remoteHandle, int length)
    {
        try
        {
            using SharedSectionView view = SharedSectionView.DuplicateAndMapReadOnly(
                host.SafeHandle,
                remoteHandle,
                length);
            return true;
        }
        catch (Win32Exception)
        {
            return false;
        }
    }

    private static bool TryOverwriteFile(string path, string contents)
    {
        try
        {
            File.WriteAllText(path, contents);
            return true;
        }
        catch (IOException)
        {
            return false;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }
    }

    private static string CreateTempDirectory()
    {
        string path = Path.Combine(
            Path.GetTempPath(),
            "QuickLookNextParserHostTests",
            Guid.NewGuid().ToString("n"));
        Directory.CreateDirectory(path);
        return path;
    }

    private static void TryDeleteDirectory(string path)
    {
        try { Directory.Delete(path, recursive: true); } catch { }
    }

    private static string NewRequestId() => Guid.NewGuid().ToString("n");

    private sealed class ParserHostTestSession : IAsyncDisposable
    {
        private int _disconnected;

        private ParserHostTestSession(
            NamedPipeServerStream pipe,
            Process host,
            PipeChannel channel,
            CancellationTokenSource timeout)
        {
            Pipe = pipe;
            Host = host;
            Channel = channel;
            TimeoutSource = timeout;
            WritableRoot = GetWritableRoot(host);
        }

        private NamedPipeServerStream Pipe { get; }
        private CancellationTokenSource TimeoutSource { get; }
        public Process Host { get; }
        public PipeChannel Channel { get; }
        public CancellationToken Token => TimeoutSource.Token;
        public string WritableRoot { get; }

        public static async Task<ParserHostTestSession> StartAsync(TimeSpan timeout)
        {
            string pipeName =
                $"quicklook_next_parser_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
            string token = RandomNumberGenerator.GetHexString(32);
            var pipe = new NamedPipeServerStream(
                pipeName,
                PipeDirection.InOut,
                1,
                PipeTransmissionMode.Byte,
                PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
            Process? host = null;
            PipeChannel? channel = null;
            var timeoutSource = new CancellationTokenSource(timeout);
            try
            {
                host = StartHost(pipeName, token);
                await pipe.WaitForConnectionAsync(timeoutSource.Token);
                channel = new PipeChannel(pipe);
                await channel.SendAsync(
                    new Hello(Environment.ProcessId, token),
                    timeoutSource.Token);
                Assert.IsType<ParserReady>(
                    await channel.ReceiveAsync(timeoutSource.Token));
                return new ParserHostTestSession(
                    pipe,
                    host,
                    channel,
                    timeoutSource);
            }
            catch
            {
                channel?.Dispose();
                try { pipe.Dispose(); } catch { }
                if (host is not null)
                {
                    await StopHostAsync(host);
                    host.Dispose();
                }
                timeoutSource.Dispose();
                throw;
            }
        }

        public async Task DisconnectAndWaitForExitAsync()
        {
            if (Interlocked.Exchange(ref _disconnected, 1) == 0)
            {
                try { Channel.Dispose(); } catch { }
                try { Pipe.Dispose(); } catch { }
            }
            await Host.WaitForExitAsync(Token);
        }

        public async ValueTask DisposeAsync()
        {
            if (Interlocked.Exchange(ref _disconnected, 1) == 0)
            {
                try { Channel.Dispose(); } catch { }
                try { Pipe.Dispose(); } catch { }
            }
            await StopHostAsync(Host);
            Host.Dispose();
            TimeoutSource.Dispose();
        }

        private static Process StartHost(string pipeName, string token)
        {
            string hostPath = Path.Combine(
                AppContext.BaseDirectory,
                "ParserHost",
                "QuickLook.Next.ParserHost.exe");
            string writableRoot = Path.Combine(
                Path.GetTempPath(),
                "QuickLookNextParserHostTests",
                "host-" + Guid.NewGuid().ToString("n"));
            Directory.CreateDirectory(writableRoot);
            foreach (string child in new[] { "logs", "archive-preview" })
                Directory.CreateDirectory(Path.Combine(writableRoot, child));
            try
            {
                return Process.Start(new ProcessStartInfo(hostPath)
                {
                    UseShellExecute = false,
                    CreateNoWindow = true,
                    ArgumentList =
                    {
                        "--pipe",
                        pipeName,
                        "--session-token",
                        token,
                        "--writable-root",
                        writableRoot,
                    },
                }) ?? throw new InvalidOperationException("ParserHost did not start");
            }
            catch
            {
                TryDeleteDirectory(writableRoot);
                throw;
            }
        }

        private static async Task StopHostAsync(Process host)
        {
            string writableRoot = GetWritableRoot(host);
            try
            {
                await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5));
            }
            catch
            {
                try { host.Kill(entireProcessTree: true); } catch { }
            }
            try
            {
                await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5));
            }
            catch { }
            TryDeleteDirectory(writableRoot);
        }

        private static string GetWritableRoot(Process host)
        {
            IList<string> arguments = host.StartInfo.ArgumentList;
            int index = arguments.IndexOf("--writable-root");
            return index >= 0 && index + 1 < arguments.Count
                ? arguments[index + 1]
                : throw new InvalidOperationException(
                    "ParserHost writable root was not configured");
        }
    }
}
