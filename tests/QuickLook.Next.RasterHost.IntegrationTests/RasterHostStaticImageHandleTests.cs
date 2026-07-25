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
    public async Task Ico_handle_decodes_without_an_input_anchor_or_logical_path()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
        string pipeName = $"quicklook_next_raster_ico_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
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
        string logicalPath = Path.Combine(Path.GetTempPath(), $"missing-{Guid.NewGuid():N}.ico");

        try
        {
            File.Copy(Path.Combine(AppContext.BaseDirectory, "Fixtures", "static.ico"), physicalPath);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(physicalPath);
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinned.Handle, host.SafeHandle);
            var probe = new FileProbe(logicalPath, ".ico", [0, 0, 1, 0])
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
                    ?? throw new EndOfStreamException("RasterHost closed before completing the ICO preview");
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
