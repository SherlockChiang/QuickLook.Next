using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostAnimationTests
{
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(20);

    [Fact]
    public async Task Gif_frames_are_handed_off_and_removed_on_close()
    {
        string pipeName = $"quicklook_next_raster_animation_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        string token = RandomNumberGenerator.GetHexString(32);
        await using var pipe = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
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

        try
        {
            using var timeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            File.Copy(Path.Combine(AppContext.BaseDirectory, "Fixtures", "animated.gif"), physicalPath);
            var pinnedInput = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            string previewRequestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var probe = new FileProbe(logicalPath, ".gif", File.ReadAllBytes(physicalPath)[..6])
            {
                Kind = "image",
                Size = new FileInfo(physicalPath).Length,
            };
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinnedInput.Handle, host.SafeHandle);
            pinnedInput.Handle.Dispose();
            await channel.SendAsync(new PreviewOpenHandle(previewRequestId, hostHandle, pinnedInput.Length, logicalPath, probe)
            {
                TargetWidth = 256,
                TargetHeight = 256,
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
            await channel.SendAsync(new PreviewAnimationFramesOpen(
                animationRequestId, previewRequestId, 2048, 2048), timeout.Token);
            PreviewAnimationFramesReady? frames = null;
            while (frames is null)
            {
                ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                    ?? throw new EndOfStreamException("RasterHost closed before returning animation frames");
                if (message is PreviewError error)
                    throw new Xunit.Sdk.XunitException(error.Message);
                frames = message as PreviewAnimationFramesReady;
            }
            Assert.Equal(previewRequestId, frames.PreviewRequestId);
            Assert.InRange(frames.FrameCount, 2, 120);
            Assert.InRange(frames.Width, 1, 1024);
            Assert.InRange(frames.Height, 1, 1024);
            using var frameHandle = WindowsHandleTransfer.DuplicateFileFromProcess(
                host.SafeHandle, frames.FileHandle, frames.PacketLength);
            using var frameStream = new FileStream(frameHandle, FileAccess.Read);
            Assert.Equal(frames.PacketLength, frameStream.Length);
            Span<byte> header = stackalloc byte[12];
            frameStream.ReadExactly(header);
            Assert.Equal(frames.FrameCount, (int)BitConverter.ToUInt32(header[..4]));

            string packetPath = Path.Combine(
                Path.GetTempPath(), "QuickLookNext", "raster-animation", "frames-" + animationRequestId, "frames.bin");
            await channel.SendAsync(new PreviewAnimationFramesClose(animationRequestId), timeout.Token);
            while (File.Exists(packetPath))
                await Task.Delay(25, timeout.Token);
            Assert.True(frameStream.CanRead);
            await channel.SendAsync(new PreviewClose(previewRequestId), timeout.Token);
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
