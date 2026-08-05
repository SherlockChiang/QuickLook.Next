using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using System.Text;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostPdfTests
{
    [Fact]
    public async Task Repeated_pdf_sessions_return_page_cache_and_projection_resources_after_idle_trim()
    {
        const int warmupCycleCount = 4;
        const int measuredCycleCount = 24;
        const int handleRecoveryBudget = 24;
        const long privateByteRecoveryBudget = 48L * 1024 * 1024;
        const long minimumMeasuredCacheGrowth = 4L * 1024 * 1024;
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(60));
        string pipeName = $"quicklook_next_raster_pdf_cycle_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string hostPath = Path.Combine(AppContext.BaseDirectory, "RasterHost", "QuickLook.Next.RasterHost.exe");
        var startInfo = new ProcessStartInfo(hostPath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            ArgumentList = { "--pipe", pipeName, "--session-token", token },
        };
        startInfo.Environment["QL_IDLE_TRIM_SECONDS"] = "1";
        startInfo.Environment["QL_IDLE_TRIM_CHECK_MILLISECONDS"] = "100";
        using Process host = Process.Start(startInfo) ?? throw new InvalidOperationException("RasterHost did not start");
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-pdf-cycle-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-pdf-cycle-{Guid.NewGuid():N}.pdf");
        bool hostExited = false;

        try
        {
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            int baselineHandles = 0;
            int peakHandles = 0;
            long baselinePrivateBytes = 0;
            long peakPrivateBytes = 0;
            for (int cycle = 0; cycle <= warmupCycleCount + measuredCycleCount; cycle++)
            {
                await File.WriteAllBytesAsync(physicalPath, CreateOnePagePdf(cycle + 1), timeout.Token);
                var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
                long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
                string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
                var probe = new FileProbe(logicalPath, ".pdf", "%PDF"u8.ToArray())
                {
                    Kind = "pdf",
                    Size = pinned.Length,
                };
                await channel.SendAsync(
                    new PreviewOpenHandle(requestId, hostHandle, pinned.Length, logicalPath, probe),
                    timeout.Token);
                pinned.Handle.Dispose();

                PreviewReady ready = Assert.IsType<PreviewReady>(await ReceiveUntilAsync<PreviewReady>(channel, timeout.Token));
                Assert.Equal(1, ready.PageCount);
                long generation = cycle + 1;
                await channel.SendAsync(new PreviewPageOpen(requestId, 0, generation, 1), timeout.Token);
                PreviewSurface surface = Assert.IsType<PreviewSurface>(
                    await ReceiveUntilAsync<PreviewSurface>(channel, timeout.Token));
                Assert.Equal((requestId, 0, generation), (surface.RequestId, surface.PageIndex, surface.PageGeneration));
                using (var localSurface = new Microsoft.Win32.SafeHandles.SafeFileHandle(
                    WindowsHandleTransfer.DuplicateHandleFromProcess(host.SafeHandle, surface.SharedHandle), ownsHandle: true))
                    Assert.False(localSurface.IsInvalid);
                await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), timeout.Token);
                await channel.SendAsync(new PreviewPageClose(requestId, 0, generation), timeout.Token);
                await channel.SendAsync(new PreviewClose(requestId), timeout.Token);
                await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);

                host.Refresh();
                if (cycle == warmupCycleCount)
                {
                    baselineHandles = host.HandleCount;
                    baselinePrivateBytes = host.PrivateMemorySize64;
                }
                else if (cycle > warmupCycleCount)
                {
                    peakHandles = Math.Max(peakHandles, host.HandleCount);
                    peakPrivateBytes = Math.Max(peakPrivateBytes, host.PrivateMemorySize64);
                }
            }

            Assert.True(peakPrivateBytes >= baselinePrivateBytes + minimumMeasuredCacheGrowth);
            await WaitUntilAsync(() =>
            {
                host.Refresh();
                return host.HandleCount <= baselineHandles + handleRecoveryBudget
                    && host.PrivateMemorySize64 <= baselinePrivateBytes + privateByteRecoveryBudget;
            }, timeout.Token);
            await Task.Delay(TimeSpan.FromSeconds(5), timeout.Token);
            Assert.False(host.HasExited, "RasterHost exited while the PDF idle-trim pipe remained connected.");
            Assert.True(peakHandles >= baselineHandles);
            Assert.False(File.Exists(logicalPath));
        }
        finally
        {
            try
            {
                hostExited = await RasterHostProcessTestHelper.CompleteAsync(pipe, host);
            }
            finally
            {
                try { File.Delete(physicalPath); } catch { }
            }
        }
        RasterHostProcessTestHelper.AssertCleanExit(host, hostExited);
    }

    [Fact]
    public async Task Handle_backed_pdf_renders_a_page_without_an_input_anchor_or_logical_path()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        string pipeName = $"quicklook_next_raster_pdf_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string hostPath = Path.Combine(AppContext.BaseDirectory, "RasterHost", "QuickLook.Next.RasterHost.exe");
        using Process host = Process.Start(new ProcessStartInfo(hostPath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            ArgumentList = { "--pipe", pipeName, "--session-token", token },
        }) ?? throw new InvalidOperationException("RasterHost did not start");
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-{Guid.NewGuid():N}.pdf");
        bool hostExited = false;

        try
        {
            await File.WriteAllBytesAsync(physicalPath, CreateOnePagePdf(), timeout.Token);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
            pinned.Handle.Dispose();
            string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var probe = new FileProbe(logicalPath, ".pdf", "%PDF"u8.ToArray())
            {
                Kind = "pdf",
                Size = pinned.Length,
            };
            await channel.SendAsync(new PreviewOpenHandle(requestId, hostHandle, pinned.Length, logicalPath, probe), timeout.Token);

            string inputDirectory = Path.Combine(
                Path.GetTempPath(), "QuickLookNext", "raster-inputs", host.Id.ToString(), "input-" + requestId);
            var ready = Assert.IsType<PreviewReady>(await ReceiveUntilAsync<PreviewReady>(channel, timeout.Token));
            Assert.Equal("pdf", ready.Kind);
            Assert.Equal(1, ready.PageCount);
            Assert.Equal(400d, ready.PageWidth, precision: 3);
            Assert.Equal(266.667d, ready.PageHeight, precision: 3);
            Assert.False(Directory.Exists(inputDirectory));
            Assert.False(File.Exists(logicalPath));
            Assert.False(TryOverwriteFile(physicalPath));

            await channel.SendAsync(new PreviewPageOpen(requestId, 1, 1, 1), timeout.Token);
            var pageError = Assert.IsType<PreviewPageError>(await ReceiveUntilAsync<PreviewPageError>(channel, timeout.Token));
            Assert.Equal((requestId, 1, 1L), (pageError.RequestId, pageError.PageIndex, pageError.PageGeneration));
            Assert.False(pageError.TimedOut);

            await channel.SendAsync(new PreviewPageOpen(requestId, 0, 2, 1), timeout.Token);
            var surface = Assert.IsType<PreviewSurface>(await ReceiveUntilAsync<PreviewSurface>(channel, timeout.Token));
            Assert.Equal((requestId, 0, 2L), (surface.RequestId, surface.PageIndex, surface.PageGeneration));
            Assert.Equal(400u, surface.Width);
            Assert.Equal(267u, surface.Height);
            using (var localSurface = new Microsoft.Win32.SafeHandles.SafeFileHandle(
                WindowsHandleTransfer.DuplicateHandleFromProcess(host.SafeHandle, surface.SharedHandle), ownsHandle: true))
                Assert.False(localSurface.IsInvalid);
            await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), timeout.Token);

            await channel.SendAsync(new PreviewClose(requestId), timeout.Token);
            await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);

            await File.WriteAllTextAsync(physicalPath, "not a pdf", timeout.Token);
            var malformed = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            long malformedHostHandle = WindowsHandleTransfer.DuplicateFileToProcess(malformed.Handle, host.SafeHandle);
            malformed.Handle.Dispose();
            string malformedRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var malformedProbe = new FileProbe(logicalPath, ".pdf", "%PDF"u8.ToArray())
            {
                Kind = "pdf",
                Size = malformed.Length,
            };
            await channel.SendAsync(new PreviewOpenHandle(
                malformedRequestId,
                malformedHostHandle,
                malformed.Length,
                logicalPath,
                malformedProbe), timeout.Token);
            var malformedError = Assert.IsType<PreviewError>(await channel.ReceiveAsync(timeout.Token));
            Assert.Equal(malformedRequestId, malformedError.RequestId);
            Assert.False(Directory.Exists(Path.Combine(
                Path.GetTempPath(), "QuickLookNext", "raster-inputs", host.Id.ToString(), "input-" + malformedRequestId)));
            await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);
        }
        finally
        {
            try
            {
                hostExited = await RasterHostProcessTestHelper.CompleteAsync(pipe, host);
            }
            finally
            {
                try { File.Delete(physicalPath); } catch { }
            }
        }
        RasterHostProcessTestHelper.AssertCleanExit(host, hostExited);
    }

    [Fact]
    public async Task Closing_inflight_pdf_render_drains_projection_before_next_session()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(45));
        string pipeName = $"quicklook_next_raster_pdf_close_render_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string hostPath = Path.Combine(AppContext.BaseDirectory, "RasterHost", "QuickLook.Next.RasterHost.exe");
        using Process host = Process.Start(new ProcessStartInfo(hostPath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            ArgumentList = { "--pipe", pipeName, "--session-token", token },
        }) ?? throw new InvalidOperationException("RasterHost did not start");
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-pdf-inflight-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-pdf-inflight-{Guid.NewGuid():N}.pdf");
        bool hostExited = false;

        try
        {
            await File.WriteAllBytesAsync(physicalPath, CreateOnePagePdf(pageWidth: 2200, pageHeight: 2200), timeout.Token);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            string firstRequestId = await OpenPinnedPdfAsync(
                channel, host, physicalPath, logicalPath, timeout.Token);
            host.Refresh();
            int preRenderHandles = host.HandleCount;
            await channel.SendAsync(new PreviewPageOpen(firstRequestId, 0, 1, 4), timeout.Token);
            await WaitUntilAsync(() =>
            {
                host.Refresh();
                return host.HandleCount > preRenderHandles;
            }, timeout.Token);
            await channel.SendAsync(new PreviewClose(firstRequestId), timeout.Token);
            await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);
            Assert.False(host.HasExited);

            await File.WriteAllBytesAsync(physicalPath, CreateOnePagePdf(), timeout.Token);
            string secondRequestId = await OpenPinnedPdfAsync(
                channel, host, physicalPath, logicalPath, timeout.Token);
            await channel.SendAsync(new PreviewClose(secondRequestId), timeout.Token);
            await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);
            Assert.False(host.HasExited);
        }
        finally
        {
            try
            {
                hostExited = await RasterHostProcessTestHelper.CompleteAsync(pipe, host);
            }
            finally
            {
                try { File.Delete(physicalPath); } catch { }
            }
        }
        RasterHostProcessTestHelper.AssertCleanExit(host, hostExited);
    }

    [Fact]
    public async Task Canceling_first_waiter_does_not_cancel_shared_pdf_render()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(45));
        string pipeName = $"quicklook_next_raster_pdf_waiter_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string hostPath = Path.Combine(AppContext.BaseDirectory, "RasterHost", "QuickLook.Next.RasterHost.exe");
        using Process host = Process.Start(new ProcessStartInfo(hostPath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            ArgumentList = { "--pipe", pipeName, "--session-token", token },
        }) ?? throw new InvalidOperationException("RasterHost did not start");
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-pdf-waiter-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-pdf-waiter-{Guid.NewGuid():N}.pdf");
        bool hostExited = false;

        try
        {
            await File.WriteAllBytesAsync(physicalPath, CreateOnePagePdf(pageWidth: 2200, pageHeight: 2200), timeout.Token);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            string requestId = await OpenPinnedPdfAsync(channel, host, physicalPath, logicalPath, timeout.Token);
            host.Refresh();
            int preRenderHandles = host.HandleCount;
            await channel.SendAsync(new PreviewPageOpen(requestId, 0, 1, 4), timeout.Token);
            await WaitUntilAsync(() =>
            {
                host.Refresh();
                return host.HandleCount > preRenderHandles;
            }, timeout.Token);
            await channel.SendAsync(new PreviewPageOpen(requestId, 0, 2, 4), timeout.Token);
            await channel.SendAsync(new PreviewPageClose(requestId, 0, 1), timeout.Token);

            PreviewSurface secondSurface = await ReceiveSurfaceAsync(channel, requestId, 2, timeout.Token);
            Assert.Equal((requestId, 0, 2L),
                (secondSurface.RequestId, secondSurface.PageIndex, secondSurface.PageGeneration));
            await channel.SendAsync(new PreviewSurfaceRelease(secondSurface.TransferId), timeout.Token);
            await channel.SendAsync(new PreviewClose(requestId), timeout.Token);
            await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);
            Assert.False(host.HasExited);
        }
        finally
        {
            try
            {
                hostExited = await RasterHostProcessTestHelper.CompleteAsync(pipe, host);
            }
            finally
            {
                try { File.Delete(physicalPath); } catch { }
            }
        }
        RasterHostProcessTestHelper.AssertCleanExit(host, hostExited);
    }

    private static async Task<string> OpenPinnedPdfAsync(
        PipeChannel channel,
        Process host,
        string physicalPath,
        string logicalPath,
        CancellationToken cancellationToken)
    {
        var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
        long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
        string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
        var probe = new FileProbe(logicalPath, ".pdf", "%PDF"u8.ToArray())
        {
            Kind = "pdf",
            Size = pinned.Length,
        };
        await channel.SendAsync(
            new PreviewOpenHandle(requestId, hostHandle, pinned.Length, logicalPath, probe),
            cancellationToken);
        pinned.Handle.Dispose();
        PreviewReady ready = Assert.IsType<PreviewReady>(
            await ReceiveUntilAsync<PreviewReady>(channel, cancellationToken));
        Assert.Equal(requestId, ready.RequestId);
        return requestId;
    }

    private static async Task<ControlMessage> ReceiveUntilAsync<T>(PipeChannel channel, CancellationToken cancellationToken)
        where T : ControlMessage
    {
        while (true)
        {
            ControlMessage message = await channel.ReceiveAsync(cancellationToken)
                ?? throw new EndOfStreamException("RasterHost closed before completing the PDF request.");
            if (message is PreviewError error)
                throw new Xunit.Sdk.XunitException(error.Message);
            if (message is T)
                return message;
        }
    }

    private static async Task<PreviewSurface> ReceiveSurfaceAsync(
        PipeChannel channel,
        string requestId,
        long pageGeneration,
        CancellationToken cancellationToken)
    {
        while (true)
        {
            ControlMessage message = await channel.ReceiveAsync(cancellationToken)
                ?? throw new EndOfStreamException("RasterHost closed before completing the PDF page request.");
            if (message is PreviewError error)
                throw new Xunit.Sdk.XunitException(error.Message);
            if (message is PreviewPageError pageError && pageError.PageGeneration == pageGeneration)
                throw new Xunit.Sdk.XunitException(pageError.Message);
            if (message is PreviewSurface surface)
            {
                if (surface.RequestId == requestId && surface.PageGeneration == pageGeneration)
                    return surface;
                await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), cancellationToken);
            }
        }
    }

    private static byte[] CreateOnePagePdf(
        int trailingCommentBytes = 0,
        int pageWidth = 300,
        int pageHeight = 200)
    {
        string[] objects =
        [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            $"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pageWidth} {pageHeight}] /Resources << >> /Contents 4 0 R >>",
            "<< /Length 0 >>\nstream\n\nendstream",
        ];
        using var stream = new MemoryStream();
        WriteAscii(stream, "%PDF-1.4\n");
        var offsets = new List<long> { 0 };
        for (int i = 0; i < objects.Length; i++)
        {
            offsets.Add(stream.Position);
            WriteAscii(stream, $"{i + 1} 0 obj\n{objects[i]}\nendobj\n");
        }
        long xref = stream.Position;
        WriteAscii(stream, $"xref\n0 {objects.Length + 1}\n0000000000 65535 f \n");
        for (int i = 1; i < offsets.Count; i++)
            WriteAscii(stream, $"{offsets[i]:D10} 00000 n \n");
        WriteAscii(stream, $"trailer\n<< /Size {objects.Length + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n");
        if (trailingCommentBytes > 0)
            WriteAscii(stream, "%" + new string('x', trailingCommentBytes) + "\n");
        return stream.ToArray();
    }

    private static void WriteAscii(Stream stream, string value)
        => stream.Write(Encoding.ASCII.GetBytes(value));

    private static bool TryOverwriteFile(string path)
    {
        try
        {
            File.WriteAllText(path, "released");
            return true;
        }
        catch (IOException)
        {
            return false;
        }
    }

    private static async Task WaitUntilAsync(Func<bool> condition, CancellationToken cancellationToken)
    {
        while (!condition())
            await Task.Delay(20, cancellationToken);
    }
}
