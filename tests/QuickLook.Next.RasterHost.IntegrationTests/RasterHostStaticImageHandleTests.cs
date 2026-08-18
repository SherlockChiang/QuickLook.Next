using System.Buffers.Binary;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostStaticImageHandleTests
{
    [Fact]
    public async Task Repeated_system_codec_previews_return_resources_after_idle_trim()
    {
        const int warmupCycleCount = 8;
        const int measuredCycleCount = 24;
        const int handleRecoveryBudget = 16;
        const long privateByteRecoveryBudget = 32L * 1024 * 1024;
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(45));
        string pipeName = $"quicklook_next_raster_wic_cycle_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(
            pipeName,
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
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
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-wic-cycle-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-wic-cycle-{Guid.NewGuid():N}.png");
        byte[] image = Convert.FromBase64String(
            "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEElEQVR42mP4z8DwH4QZGBgAAL8BA/2t7mQAAAAASUVORK5CYII=");
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
                await File.WriteAllBytesAsync(physicalPath, image, timeout.Token);
                string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
                var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
                long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
                var probe = new FileProbe(logicalPath, ".png", image[..8])
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
                        ?? throw new EndOfStreamException("RasterHost closed during system-codec cycle preview");
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

            Assert.True(peakHandles > baselineHandles + handleRecoveryBudget);
            Assert.True(peakPrivateBytes >= baselinePrivateBytes);
            await WaitUntilAsync(() =>
            {
                host.Refresh();
                return host.HandleCount <= baselineHandles + handleRecoveryBudget
                    && host.PrivateMemorySize64 <= baselinePrivateBytes + privateByteRecoveryBudget;
            }, timeout.Token);
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
    public async Task Repeated_image_handle_previews_release_sources_without_linear_handle_growth()
    {
        // Hosted .NET/D3D stacks can continue creating bounded worker/runtime handles after the
        // first few previews. Warm through that startup ramp before measuring the 32-cycle slope;
        // a per-preview leak still exceeds the unchanged budget during the measured window.
        const int warmupCycleCount = 64;
        const int baselineWindowCycleCount = 8;
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
        bool hostExited = false;

        try
        {
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            int baselinePeakHandles = 0;
            int peakHandles = 0;
            int lastMeasuredHandles = 0;
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
                int handleCount = host.HandleCount;
                if (cycle > warmupCycleCount - baselineWindowCycleCount && cycle <= warmupCycleCount)
                    baselinePeakHandles = Math.Max(baselinePeakHandles, handleCount);
                else if (cycle > warmupCycleCount)
                {
                    peakHandles = Math.Max(peakHandles, handleCount);
                    lastMeasuredHandles = handleCount;
                }
            }

            Assert.True(baselinePeakHandles > 0, "The warmup handle-count window was not sampled.");
            Assert.True(
                peakHandles <= baselinePeakHandles + handleGrowthBudget,
                $"RasterHost handle growth exceeded the bounded budget: warmupPeak={baselinePeakHandles}, " +
                $"measuredPeak={peakHandles}, measuredLast={lastMeasuredHandles}, budget={handleGrowthBudget}.");
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

    [Theory]
    [InlineData("ico")]
    [InlineData("png")]
    [InlineData("jpg")]
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
        bool hostExited = false;

        try
        {
            if (format == "ico")
                File.Copy(Path.Combine(AppContext.BaseDirectory, "Fixtures", "static.ico"), physicalPath);
            else if (format == "jpg")
                File.Copy(Path.Combine(AppContext.BaseDirectory, "Fixtures", "static.jpg"), physicalPath);
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
            byte[] magic = format switch
            {
                "ico" => [0, 0, 1, 0],
                "jpg" => [0xFF, 0xD8, 0xFF],
                _ => [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
            };
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
            var messageOrder = new List<Type>();
            while (surface is null || ready is null || waveform is null)
            {
                ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                    ?? throw new EndOfStreamException("RasterHost closed before completing the image preview");
                messageOrder.Add(message.GetType());
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
            Assert.True(ImageWaveformBuilder.IsValid(waveform.Waveform));
            Assert.Equal(
                ImageWaveformBuilder.ScopeWidth * ImageWaveformBuilder.ScopeHeight * 3,
                waveform.Waveform.RgbDensity.Length);
            Assert.True(
                messageOrder.IndexOf(typeof(PreviewSurface))
                    < messageOrder.IndexOf(typeof(PreviewImageWaveform)));
            Assert.True(
                messageOrder.IndexOf(typeof(PreviewReady))
                    < messageOrder.IndexOf(typeof(PreviewImageWaveform)));
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
    public async Task Image_metadata_child_keeps_an_independent_handle_lease_after_parent_close()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
        string pipeName = $"quicklook_next_raster_metadata_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
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
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-metadata-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-metadata-{Guid.NewGuid():N}.png");
        byte[] image = Convert.FromBase64String(
            "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEElEQVR42mP4z8DwH4QZGBgAAL8BA/2t7mQAAAAASUVORK5CYII=");
        bool hostExited = false;

        try
        {
            await File.WriteAllBytesAsync(physicalPath, image, timeout.Token);
            Assert.False(File.Exists(logicalPath));
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            string parentRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            try
            {
                long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
                var probe = new FileProbe(
                    logicalPath,
                    ".png",
                    [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
                {
                    Kind = "image",
                    Size = pinned.Length,
                };
                await channel.SendAsync(new PreviewOpenHandle(
                    parentRequestId,
                    hostHandle,
                    pinned.Length,
                    logicalPath,
                    probe)
                {
                    TargetWidth = 64,
                    TargetHeight = 64,
                }, timeout.Token);
            }
            finally
            {
                pinned.Handle.Dispose();
            }

            await ReceiveStaticImagePreviewAsync(
                channel,
                host,
                parentRequestId,
                timeout.Token);
            Assert.False(TryOverwriteFile(physicalPath));

            string metadataRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            await channel.SendAsync(
                new PreviewImageMetadataOpen(metadataRequestId, parentRequestId),
                timeout.Token);
            // The metadata-open message is ordered before this close on the same pipe. RasterHost
            // must therefore acquire a child lease before releasing the retained parent source.
            await channel.SendAsync(new PreviewClose(parentRequestId), timeout.Token);

            PreviewImageMetadataReady? metadataReady = null;
            while (metadataReady is null)
            {
                ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                    ?? throw new EndOfStreamException("RasterHost closed before returning HANDLE image metadata.");
                if (message is PreviewError error && error.RequestId == metadataRequestId)
                    throw new Xunit.Sdk.XunitException(error.Message);
                if (message is PreviewImageMetadataReady ready
                    && ready.RequestId == metadataRequestId)
                {
                    metadataReady = ready;
                }
                else if (message is PreviewSurface surface)
                {
                    await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), timeout.Token);
                }
            }

            Assert.Equal(parentRequestId, metadataReady.PreviewRequestId);
            Assert.Equal("PNG", metadataReady.Metadata.Format);
            Assert.Equal((uint)2, metadataReady.Metadata.Width);
            Assert.Equal((uint)1, metadataReady.Metadata.Height);

            await channel.SendAsync(
                new PreviewImageMetadataClose(metadataRequestId),
                timeout.Token);
            await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);
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
    public async Task Windows_metadata_supplement_reads_bmp_from_the_retained_handle_after_parent_close()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
        string pipeName = $"quicklook_next_raster_wic_metadata_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
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
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-wic-metadata-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-wic-metadata-{Guid.NewGuid():N}.bmp");
        byte[] image = CreateOnePixelBmp();
        bool hostExited = false;

        try
        {
            await File.WriteAllBytesAsync(physicalPath, image, timeout.Token);
            Assert.False(File.Exists(logicalPath));
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            string parentRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            try
            {
                long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(
                    pinned.Handle,
                    host.SafeHandle);
                var probe = new FileProbe(logicalPath, ".bmp", image[..16])
                {
                    Kind = "image",
                    Size = pinned.Length,
                };
                await channel.SendAsync(new PreviewOpenHandle(
                    parentRequestId,
                    hostHandle,
                    pinned.Length,
                    logicalPath,
                    probe)
                {
                    TargetWidth = 64,
                    TargetHeight = 64,
                }, timeout.Token);
            }
            finally
            {
                pinned.Handle.Dispose();
            }

            await ReceiveStaticImagePreviewAsync(channel, host, parentRequestId, timeout.Token);
            Assert.False(TryOverwriteFile(physicalPath));

            string metadataRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            await channel.SendAsync(
                new PreviewImageMetadataOpen(metadataRequestId, parentRequestId),
                timeout.Token);
            await channel.SendAsync(new PreviewClose(parentRequestId), timeout.Token);

            PreviewImageMetadataReady? metadataReady = null;
            while (metadataReady is null)
            {
                ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                    ?? throw new EndOfStreamException("RasterHost closed before returning WIC HANDLE image metadata.");
                if (message is PreviewError error && error.RequestId == metadataRequestId)
                    throw new Xunit.Sdk.XunitException(error.Message);
                if (message is PreviewImageMetadataReady ready
                    && ready.RequestId == metadataRequestId)
                {
                    metadataReady = ready;
                }
                else if (message is PreviewSurface surface)
                {
                    await channel.SendAsync(
                        new PreviewSurfaceRelease(surface.TransferId),
                        timeout.Token);
                }
            }

            Assert.Equal(parentRequestId, metadataReady.PreviewRequestId);
            Assert.Equal("BMP", metadataReady.Metadata.Format);
            Assert.Equal((uint)1, metadataReady.Metadata.Width);
            Assert.Equal((uint)1, metadataReady.Metadata.Height);
            Assert.InRange(
                metadataReady.Metadata.HorizontalResolution ?? 0,
                95.9,
                96.1);
            Assert.InRange(
                metadataReady.Metadata.VerticalResolution ?? 0,
                95.9,
                96.1);

            await channel.SendAsync(
                new PreviewImageMetadataClose(metadataRequestId),
                timeout.Token);
            await WaitUntilAsync(() => TryOverwriteFile(physicalPath), timeout.Token);
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
    public async Task Windows_property_handler_reads_a_missing_logical_image_from_the_retained_handle()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
        string physicalPath = Path.Combine(
            Path.GetTempPath(),
            $"quicklook-next-property-handler-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(
            Path.GetTempPath(),
            $"missing-property-handler-{Guid.NewGuid():N}.bmp");
        byte[] image = CreateOnePixelBmp();

        try
        {
            await File.WriteAllBytesAsync(physicalPath, image, timeout.Token);
            Assert.False(File.Exists(logicalPath));
            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            try
            {
                ImageMetadata? metadata =
                    await WindowsPropertyHandlerMetadataReader.TryReadHandleAsync(
                        pinned.Handle,
                        pinned.Length,
                        Path.GetFileName(logicalPath),
                        TimeSpan.FromSeconds(5),
                        timeout.Token);

                Assert.NotNull(metadata);
                Assert.Equal((uint)1, metadata.Width);
                Assert.Equal((uint)1, metadata.Height);
                Assert.InRange(metadata.HorizontalResolution ?? 0, 95.9, 96.1);
                Assert.InRange(metadata.VerticalResolution ?? 0, 95.9, 96.1);
            }
            finally
            {
                pinned.Handle.Dispose();
            }
            Assert.False(File.Exists(logicalPath));
        }
        finally
        {
            try { File.Delete(physicalPath); } catch { }
        }
    }

    [Fact]
    public void Windows_property_handler_stream_uses_raw_pointer_read_write_abi()
    {
        Type streamInterface = typeof(WindowsPropertyHandlerMetadataReader.IRawComStream);
        Assert.Equal(
            new Guid("0000000C-0000-0000-C000-000000000046"),
            streamInterface.GUID);

        var read = streamInterface.GetMethod(
            nameof(WindowsPropertyHandlerMetadataReader.IRawComStream.Read));
        Assert.NotNull(read);
        Assert.Equal(typeof(int), read.ReturnType);
        Assert.Equal(
            [typeof(nint), typeof(uint), typeof(nint)],
            read.GetParameters().Select(static parameter => parameter.ParameterType));

        var write = streamInterface.GetMethod(
            nameof(WindowsPropertyHandlerMetadataReader.IRawComStream.Write));
        Assert.NotNull(write);
        Assert.Equal(typeof(int), write.ReturnType);
        Assert.Equal(
            [typeof(nint), typeof(uint), typeof(nint)],
            write.GetParameters().Select(static parameter => parameter.ParameterType));
        Assert.DoesNotContain(
            streamInterface.GetMethods().SelectMany(static method => method.GetParameters()),
            static parameter => parameter.ParameterType == typeof(byte[]));
    }

    [Fact]
    public void Windows_property_handler_stream_rejects_oversized_read_before_touching_source()
    {
        byte[] sourceBytes = [1, 2, 3, 4];
        using var source = new MemoryStream(sourceBytes, writable: false);
        using var stream = new WindowsPropertyHandlerMetadataReader.ReadOnlyComStream(
            source,
            source.Length,
            CancellationToken.None);
        nint buffer = Marshal.AllocHGlobal(sourceBytes.Length);
        nint bytesRead = Marshal.AllocHGlobal(sizeof(uint));
        try
        {
            Marshal.WriteInt32(bytesRead, -1);
            int oversized = stream.Read(
                buffer,
                WindowsPropertyHandlerMetadataReader.ReadOnlyComStream.MaxSingleReadBytes + 1,
                bytesRead);

            Assert.Equal(unchecked((int)0x80030009), oversized);
            Assert.Equal(0, Marshal.ReadInt32(bytesRead));
            Assert.Equal(0, source.Position);

            int success = stream.Read(buffer, (uint)sourceBytes.Length, bytesRead);
            Assert.Equal(0, success);
            Assert.Equal(sourceBytes.Length, Marshal.ReadInt32(bytesRead));
            Assert.Equal(sourceBytes.Length, source.Position);
            byte[] copied = new byte[sourceBytes.Length];
            Marshal.Copy(buffer, copied, 0, copied.Length);
            Assert.Equal(sourceBytes, copied);
        }
        finally
        {
            Marshal.FreeHGlobal(bytesRead);
            Marshal.FreeHGlobal(buffer);
        }
    }

    [Fact]
    public async Task System_metadata_drain_watchdog_has_a_hard_grace_bound()
    {
        var pending = new TaskCompletionSource(
            TaskCreationOptions.RunContinuationsAsynchronously);
        var stopwatch = Stopwatch.StartNew();

        bool drained = await SystemImageMetadataReader.DrainsWithinGraceAsync(
            pending.Task,
            TimeSpan.FromMilliseconds(50));

        stopwatch.Stop();
        Assert.False(drained);
        Assert.InRange(stopwatch.Elapsed, TimeSpan.FromMilliseconds(30), TimeSpan.FromSeconds(2));
        pending.TrySetResult();
        Assert.True(await SystemImageMetadataReader.DrainsWithinGraceAsync(
            pending.Task,
            TimeSpan.FromSeconds(1)));
    }

    [Fact]
    public void Image_metadata_merge_precedence_is_native_then_property_handler_then_wic()
    {
        var native = new ImageMetadata
        {
            Format = "PNG",
            Title = "native",
            Width = 10,
        };
        var propertyHandler = new ImageMetadata
        {
            Format = "Property",
            Title = "property",
            Width = 20,
            Height = 21,
            VerticalResolution = 72,
        };
        var wic = new ImageMetadata
        {
            Format = "WIC",
            Width = 30,
            Height = 31,
            HorizontalResolution = 96,
            VerticalResolution = 97,
        };

        ImageMetadata? merged = SystemImageMetadataReader.Merge(
            WindowsPropertyHandlerMetadataReader.Merge(native, propertyHandler),
            wic);

        Assert.NotNull(merged);
        Assert.Equal("PNG", merged.Format);
        Assert.Equal("native", merged.Title);
        Assert.Equal((uint)10, merged.Width);
        Assert.Equal((uint)21, merged.Height);
        Assert.Equal(96, merged.HorizontalResolution);
        Assert.Equal(72, merged.VerticalResolution);
    }

    [Fact]
    public async Task Image_metadata_child_with_missing_parent_fails_closed()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        string pipeName = $"quicklook_next_raster_metadata_missing_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
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
        bool hostExited = false;

        try
        {
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            string metadataRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            string missingParentRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            await channel.SendAsync(
                new PreviewImageMetadataOpen(metadataRequestId, missingParentRequestId),
                timeout.Token);

            var error = Assert.IsType<PreviewError>(
                await channel.ReceiveAsync(timeout.Token));
            Assert.Equal(metadataRequestId, error.RequestId);
            Assert.Contains("no longer available", error.Message, StringComparison.OrdinalIgnoreCase);
            await channel.SendAsync(
                new PreviewImageMetadataClose(metadataRequestId),
                timeout.Token);
        }
        finally
        {
            hostExited = await RasterHostProcessTestHelper.CompleteAsync(pipe, host);
        }
        RasterHostProcessTestHelper.AssertCleanExit(host, hostExited);
    }

    private static async Task ReceiveStaticImagePreviewAsync(
        PipeChannel channel,
        Process host,
        string requestId,
        CancellationToken cancellationToken)
    {
        PreviewSurface? surface = null;
        PreviewReady? ready = null;
        PreviewImageWaveform? waveform = null;
        while (surface is null || ready is null || waveform is null)
        {
            ControlMessage message = await channel.ReceiveAsync(cancellationToken)
                ?? throw new EndOfStreamException("RasterHost closed before completing the parent image preview.");
            if (message is PreviewError error)
                throw new Xunit.Sdk.XunitException(error.Message);
            if (message is PreviewSurface receivedSurface)
            {
                surface = receivedSurface;
                using var localSurfaceHandle = new Microsoft.Win32.SafeHandles.SafeFileHandle(
                    WindowsHandleTransfer.DuplicateHandleFromProcess(host.SafeHandle, surface.SharedHandle),
                    ownsHandle: true);
                Assert.False(localSurfaceHandle.IsInvalid);
                await channel.SendAsync(
                    new PreviewSurfaceRelease(surface.TransferId),
                    cancellationToken);
            }
            ready = message as PreviewReady ?? ready;
            waveform = message as PreviewImageWaveform ?? waveform;
        }

        Assert.Equal(requestId, ready.RequestId);
        Assert.Equal(requestId, surface.RequestId);
        Assert.Equal(requestId, waveform.RequestId);
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

    private static byte[] CreateOnePixelBmp()
    {
        byte[] bytes = new byte[58];
        bytes[0] = (byte)'B';
        bytes[1] = (byte)'M';
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(2), bytes.Length);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(10), 54);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(14), 40);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(18), 1);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(22), 1);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(26), 1);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(28), 24);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(34), 4);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(38), 3780);
        BinaryPrimitives.WriteInt32LittleEndian(bytes.AsSpan(42), 3780);
        bytes[56] = 0xFF;
        return bytes;
    }

    private static async Task WaitUntilAsync(Func<bool> condition, CancellationToken cancellationToken)
    {
        while (!condition())
            await Task.Delay(20, cancellationToken);
    }
}
