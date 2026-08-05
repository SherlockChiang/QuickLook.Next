using System.Buffers.Binary;
using System.Diagnostics;
using System.IO.Compression;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostAnimationTests
{
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(20);

    [Theory]
    [InlineData("gif", false)]
    [InlineData("gif", true)]
    [InlineData("webp", false)]
    [InlineData("png", false)]
    public async Task Animated_frames_are_section_backed_and_released_on_close(
        string extension,
        bool requestMismatchedTarget)
    {
        string pipeName = $"quicklook_next_raster_animation_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-{Guid.NewGuid():N}.{extension}");
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
            using var timeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            if (extension == "png")
                File.WriteAllBytes(physicalPath, CreateAnimatedPng());
            else
                File.Copy(Path.Combine(AppContext.BaseDirectory, "Fixtures", $"animated.{extension}"), physicalPath);
            var pinnedInput = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            string previewRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            byte[] content = File.ReadAllBytes(physicalPath);
            var probe = new FileProbe(logicalPath, $".{extension}", content[..Math.Min(content.Length, 64)])
            {
                Kind = "image",
                Size = new FileInfo(physicalPath).Length,
                IsAnimated = true,
            };
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinnedInput.Handle, host.SafeHandle);
            pinnedInput.Handle.Dispose();
            await channel.SendAsync(new PreviewOpenHandle(previewRequestId, hostHandle, pinnedInput.Length, logicalPath, probe)
            {
                TargetWidth = 256,
                TargetHeight = 256,
                PrepareAnimation = extension == "gif",
            }, timeout.Token);

            PreviewReady? previewReady = null;
            while (previewReady is null)
            {
                ControlMessage? received = await channel.ReceiveAsync(timeout.Token);
                Assert.NotNull(received);
                ControlMessage message = received;
                if (message is PreviewError error)
                    throw new Xunit.Sdk.XunitException(error.Message);
                if (message is PreviewSurface surface)
                {
                    Assert.Matches("^[0-9a-f]{32}$", surface.TransferId);
                    using var localSurfaceHandle = new Microsoft.Win32.SafeHandles.SafeFileHandle(
                        WindowsHandleTransfer.DuplicateHandleFromProcess(host.SafeHandle, surface.SharedHandle),
                        ownsHandle: true);
                    Assert.False(localSurfaceHandle.IsInvalid);
                    await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), timeout.Token);
                }
                if (extension == "gif")
                    Assert.False(message is PreviewImageWaveform, "GIF must not publish an RGB waveform.");
                previewReady = message as PreviewReady;
            }

            string inputDirectory = Path.Combine(
                Path.GetTempPath(),
                "QuickLookNext",
                "raster-inputs",
                host.Id.ToString(),
                "input-" + previewRequestId);
            Assert.False(Directory.Exists(inputDirectory));
            Assert.False(TryOverwriteFile(physicalPath));

            string animationRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            uint animationTarget = extension == "gif"
                ? requestMismatchedTarget ? 128u : 256u
                : 2048u;
            await channel.SendAsync(new PreviewAnimationFramesOpen(
                animationRequestId,
                previewRequestId,
                animationTarget,
                animationTarget), timeout.Token);
            PreviewAnimationFramesReady? frames = null;
            while (frames is null)
            {
                ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                    ?? throw new EndOfStreamException("RasterHost closed before returning animation frames");
                if (message is PreviewError error)
                    throw new Xunit.Sdk.XunitException(error.Message);
                if (extension == "gif")
                    Assert.False(message is PreviewImageWaveform, "GIF must not publish an RGB waveform.");
                frames = message as PreviewAnimationFramesReady;
            }
            Assert.Equal(previewRequestId, frames.PreviewRequestId);
            Assert.InRange(frames.FrameCount, 2, 120);
            Assert.InRange(frames.Width, 1, 1024);
            Assert.InRange(frames.Height, 1, 1024);
            if (extension == "gif")
            {
                Assert.Equal((int)animationTarget, frames.Width);
                Assert.Equal((int)animationTarget, frames.Height);
            }
            Assert.InRange(frames.PacketLength, 13, 64L * 1024 * 1024 + 12);
            using SharedSectionView frameView = SharedSectionView.DuplicateAndMapReadOnly(
                host.SafeHandle,
                frames.SectionHandle,
                checked((int)frames.PacketLength));
            Assert.Equal(frames.FrameCount, (int)BitConverter.ToUInt32(frameView.Bytes[..4]));
            int frameBytes = checked(frames.Width * frames.Height * 4);
            long expectedPacketLength = checked(
                12L + frames.FrameCount * (4L + frameBytes));
            Assert.Equal(expectedPacketLength, frames.PacketLength);
            byte[] firstFrame = frameView.Bytes.Slice(16, frameBytes).ToArray();
            int lastFrameOffset = checked(
                12 + (frames.FrameCount - 1) * (4 + frameBytes) + 4);
            byte[] lastFrame = frameView.Bytes.Slice(lastFrameOffset, frameBytes).ToArray();
            Assert.False(
                firstFrame.AsSpan().SequenceEqual(lastFrame),
                $"{extension} animation packet returned identical first and last frames.");

            string packetDirectory = Path.Combine(
                Path.GetTempPath(), "QuickLookNext", "raster-animation", "frames-" + animationRequestId);
            Assert.False(Directory.Exists(packetDirectory));
            using var closeTimeout =
                new CancellationTokenSource(TimeSpan.FromSeconds(5));
            await channel.SendAsync(
                new PreviewAnimationFramesClose(animationRequestId),
                closeTimeout.Token);
            await WaitUntilAsync(
                () => !CanDuplicateSection(host, frames.SectionHandle, checked((int)frames.PacketLength)),
                closeTimeout.Token);
            Assert.Equal(frames.FrameCount, (int)BitConverter.ToUInt32(frameView.Bytes[..4]));
            Assert.False(Directory.Exists(packetDirectory));
            await channel.SendAsync(
                new PreviewClose(previewRequestId),
                closeTimeout.Token);
            await WaitUntilAsync(
                () => TryOverwriteFile(physicalPath),
                closeTimeout.Token);
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
    [InlineData(false, true)]
    [InlineData(true, false)]
    public async Task Gif_static_first_frame_never_starts_or_publishes_rgb_waveform(
        bool prepareAnimation,
        bool isAnimated)
    {
        string pipeName = $"quicklook_next_raster_gif_still_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(
            pipeName,
            PipeDirection.InOut,
            1,
            PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        string physicalPath = Path.Combine(Path.GetTempPath(), $"quicklook-next-{Guid.NewGuid():N}.bin");
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-{Guid.NewGuid():N}.gif");
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
            using var timeout = new CancellationTokenSource(Timeout);
            File.Copy(Path.Combine(AppContext.BaseDirectory, "Fixtures", "animated.gif"), physicalPath);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
            var probe = new FileProbe(logicalPath, ".gif", [0x47, 0x49, 0x46, 0x38, 0x39, 0x61])
            {
                Kind = "image",
                Size = pinned.Length,
                IsAnimated = isAnimated,
            };
            await channel.SendAsync(new PreviewOpenHandle(
                requestId,
                hostHandle,
                pinned.Length,
                logicalPath,
                probe)
            {
                TargetWidth = 128,
                TargetHeight = 128,
                PrepareAnimation = prepareAnimation,
            }, timeout.Token);
            pinned.Handle.Dispose();

            PreviewSurface? surface = null;
            PreviewReady? ready = null;
            while (surface is null || ready is null)
            {
                ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                    ?? throw new EndOfStreamException("RasterHost closed during GIF static preview");
                Assert.False(message is PreviewImageWaveform, "GIF static first frame must not publish RGB waveform data.");
                if (message is PreviewError error)
                    throw new Xunit.Sdk.XunitException(error.Message);
                if (message is PreviewSurface receivedSurface)
                {
                    surface = receivedSurface;
                    await channel.SendAsync(new PreviewSurfaceRelease(surface.TransferId), timeout.Token);
                }
                ready = message as PreviewReady ?? ready;
            }

            using (var quiet = new CancellationTokenSource(TimeSpan.FromMilliseconds(300)))
            {
                await Assert.ThrowsAnyAsync<OperationCanceledException>(
                    async () => await channel.ReceiveAsync(quiet.Token));
            }
            await channel.SendAsync(new PreviewClose(requestId), timeout.Token);
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
        catch (System.ComponentModel.Win32Exception)
        {
            return false;
        }
    }

    private static byte[] CreateAnimatedPng()
    {
        using var output = new MemoryStream();
        output.Write([0x89, (byte)'P', (byte)'N', (byte)'G', 0x0D, 0x0A, 0x1A, 0x0A]);

        Span<byte> ihdr = stackalloc byte[13];
        BinaryPrimitives.WriteUInt32BigEndian(ihdr[..4], 2);
        BinaryPrimitives.WriteUInt32BigEndian(ihdr[4..8], 1);
        ihdr[8] = 8;
        ihdr[9] = 6;
        WritePngChunk(output, "IHDR"u8, ihdr);

        Span<byte> animationControl = stackalloc byte[8];
        BinaryPrimitives.WriteUInt32BigEndian(animationControl[..4], 2);
        WritePngChunk(output, "acTL"u8, animationControl);

        WriteFrameControl(output, sequence: 0, delayNumerator: 1);
        WritePngChunk(output, "IDAT"u8, CompressPngFrame(
            [255, 0, 0, 255, 255, 0, 0, 255]));

        WriteFrameControl(output, sequence: 1, delayNumerator: 2);
        byte[] secondFrame = CompressPngFrame(
            [0, 255, 0, 255, 0, 255, 0, 255]);
        byte[] frameData = new byte[4 + secondFrame.Length];
        BinaryPrimitives.WriteUInt32BigEndian(frameData.AsSpan(0, 4), 2);
        secondFrame.CopyTo(frameData, 4);
        WritePngChunk(output, "fdAT"u8, frameData);
        WritePngChunk(output, "IEND"u8, []);
        return output.ToArray();
    }

    private static void WriteFrameControl(Stream output, uint sequence, ushort delayNumerator)
    {
        Span<byte> control = stackalloc byte[26];
        BinaryPrimitives.WriteUInt32BigEndian(control[..4], sequence);
        BinaryPrimitives.WriteUInt32BigEndian(control[4..8], 2);
        BinaryPrimitives.WriteUInt32BigEndian(control[8..12], 1);
        BinaryPrimitives.WriteUInt16BigEndian(control[20..22], delayNumerator);
        BinaryPrimitives.WriteUInt16BigEndian(control[22..24], 10);
        WritePngChunk(output, "fcTL"u8, control);
    }

    private static byte[] CompressPngFrame(ReadOnlySpan<byte> rgba)
    {
        using var output = new MemoryStream();
        using (var zlib = new ZLibStream(output, CompressionLevel.SmallestSize, leaveOpen: true))
        {
            zlib.WriteByte(0);
            zlib.Write(rgba);
        }
        return output.ToArray();
    }

    private static void WritePngChunk(Stream output, ReadOnlySpan<byte> type, ReadOnlySpan<byte> data)
    {
        Span<byte> length = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(length, checked((uint)data.Length));
        output.Write(length);
        output.Write(type);
        output.Write(data);

        uint crc = 0xFFFF_FFFF;
        foreach (byte value in type)
            crc = UpdateCrc32(crc, value);
        foreach (byte value in data)
            crc = UpdateCrc32(crc, value);
        Span<byte> checksum = stackalloc byte[4];
        BinaryPrimitives.WriteUInt32BigEndian(checksum, ~crc);
        output.Write(checksum);
    }

    private static uint UpdateCrc32(uint crc, byte value)
    {
        crc ^= value;
        for (int bit = 0; bit < 8; bit++)
            crc = (crc >> 1) ^ (0xEDB8_8320u & (uint)-(int)(crc & 1));
        return crc;
    }

}
