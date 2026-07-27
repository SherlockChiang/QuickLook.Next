using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostStaticImageHandleTests
{
    [Fact]
    public async Task Repeated_image_handle_previews_release_sources_without_linear_handle_growth()
    {
        const int warmupCycleCount = 16;
        const int measuredCycleCount = 32;
        const int handleGrowthBudget = 12;
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(45));
        string pipeName = $"quicklook_next_raster_cycle_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(
            pipeName,
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string hostPath = Path.Combine(AppContext.BaseDirectory, "RasterHost", "QuickLook.Next.RasterHost.exe");
        using Process host = Process.Start(new ProcessStartInfo(hostPath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            ArgumentList = { "--pipe", pipeName, "--session-token", token },
        }) ?? throw new InvalidOperationException("RasterHost did not start");
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-cycle-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-cycle-{Guid.NewGuid():N}.ico");
        byte[] image = await File.ReadAllBytesAsync(
            Path.Combine(AppContext.BaseDirectory, "Fixtures", "static.ico"),
            timeout.Token);

        try
        {
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            int baselineHandles = 0;
            int peakHandles = 0;
            for (int cycle = 0; cycle <= warmupCycleCount + measuredCycleCount; cycle++)
            {
                await File.WriteAllBytesAsync(physicalPath, image, timeout.Token);
                string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
                var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
                long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
                var probe = new FileProbe(logicalPath, ".ico", image[..Math.Min(16, image.Length)])
                {
                    Kind = "image",
                    Size = pinned.Length,
                };
                await channel.SendAsync(new PreviewOpenHandle(
                    requestId,
                    hostHandle,
                    pinned.Length,
                    logicalPath,
                    probe)
                {
                    TargetWidth = 64,
                    TargetHeight = 64,
                }, timeout.Token);
                pinned.Handle.Dispose();

                PreviewSurface? surface = null;
                PreviewReady? ready = null;
                PreviewImageWaveform? waveform = null;
                while (surface is null || ready is null || waveform is null)
                {
                    ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                        ?? throw new EndOfStreamException("RasterHost closed during cycle preview");
                    if (message is PreviewError error)
                        throw new Xunit.Sdk.XunitException(error.Message);
                    if (message is PreviewSurface receivedSurface)
                    {
                        surface = receivedSurface;
                        using var localSurfaceHandle = new Microsoft.Win32.SafeHandles.SafeFileHandle(
                            WindowsHandleTransfer.DuplicateHandleFromProcess(host.SafeHandle, surface.SharedHandle),
                            ownsHandle: true);
                        Assert.False(localSurfaceHandle.IsInvalid);
                        await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), timeout.Token);
                    }
                    ready = message as PreviewReady ?? ready;
                    waveform = message as PreviewImageWaveform ?? waveform;
                }

                await channel.SendAsync(new PreviewClose(requestId), timeout.Token);
                await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);
                host.Refresh();
                if (cycle == warmupCycleCount)
                    baselineHandles = host.HandleCount;
                else if (cycle > warmupCycleCount)
                    peakHandles = Math.Max(peakHandles, host.HandleCount);
            }

            Assert.InRange(peakHandles, 1, baselineHandles + handleGrowthBudget);
            Assert.False(File.Exists(logicalPath));
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            try { await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5)); }
            catch { try { host.Kill(entireProcessTree: true); } catch { } }
            try { File.Delete(physicalPath); } catch { }
        }
    }

    [Theory]
    [InlineData("ico")]
    [InlineData("png")]
    public async Task Image_handle_decodes_without_an_input_anchor_or_logical_path(string format)
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
        string pipeName = $"quicklook_next_raster_{format}_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(
            pipeName,
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string hostPath = Path.Combine(AppContext.BaseDirectory, "RasterHost", "QuickLook.Next.RasterHost.exe");
        using Process host = Process.Start(new ProcessStartInfo(hostPath)
        {
            UseShellExecute = false,
            CreateNoWindow = true,
            ArgumentList = { "--pipe", pipeName, "--session-token", token },
        }) ?? throw new InvalidOperationException("RasterHost did not start");
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-{Guid.NewGuid():N}.{format}");

        try
        {
            if (format == "ico")
                File.Copy(Path.Combine(AppContext.BaseDirectory, "Fixtures", "static.ico"), physicalPath);
            else
                await File.WriteAllBytesAsync(physicalPath, Convert.FromBase64String(
                    "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEElEQVR42mP4z8DwH4QZGBgAAL8BA/2t7mQAAAAASUVORK5CYII="), timeout.Token);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
            string extension = "." + format;
            byte[] magic = format == "ico" ? [0, 0, 1, 0] : [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
            var probe = new FileProbe(logicalPath, extension, magic)
            {
                Kind = "image",
                Size = pinned.Length,
            };
            await channel.SendAsync(new PreviewOpenHandle(
                requestId,
                hostHandle,
                pinned.Length,
                logicalPath,
                probe)
            {
                TargetWidth = 64,
                TargetHeight = 64,
            }, timeout.Token);
            pinned.Handle.Dispose();

            PreviewSurface? surface = null;
            PreviewReady? ready = null;
            PreviewImageWaveform? waveform = null;
            while (surface is null || ready is null || waveform is null)
            {
                ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                    ?? throw new EndOfStreamException("RasterHost closed before completing the image preview");
                if (message is PreviewError error)
                    throw new Xunit.Sdk.XunitException(error.Message);
                if (message is PreviewSurface receivedSurface)
                {
                    surface = receivedSurface;
                    using var localSurfaceHandle = new Microsoft.Win32.SafeHandles.SafeFileHandle(
                        WindowsHandleTransfer.DuplicateHandleFromProcess(host.SafeHandle, surface.SharedHandle),
                        ownsHandle: true);
                    Assert.False(localSurfaceHandle.IsInvalid);
                    await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), timeout.Token);
                }
                ready = message as PreviewReady ?? ready;
                waveform = message as PreviewImageWaveform ?? waveform;
            }

            Assert.Equal("image", ready.Kind);
            Assert.True(surface.Width > 0 && surface.Height > 0);
            Assert.Equal(requestId, waveform.RequestId);
            string inputDirectory = Path.Combine(
                Path.GetTempPath(),
                "QuickLookNext",
                "raster-inputs",
                host.Id.ToString(),
                "input-" + requestId);
            Assert.False(Directory.Exists(inputDirectory));
            Assert.False(TryOverwriteFile(physicalPath));
            await channel.SendAsync(new PreviewClose(requestId), timeout.Token);
            await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            try { await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5)); }
            catch { try { host.Kill(entireProcessTree: true); } catch { } }
            try { File.Delete(physicalPath); } catch { }
        }
    }

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
