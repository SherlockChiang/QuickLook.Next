using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostSvgTests
{
    [Fact]
    public async Task Svg_is_rendered_to_a_bounded_shared_surface()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
        string pipeName = $"quicklook_next_raster_svg_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
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
        string path = Path.Combine(Path.GetTempPath(), $"quicklook-next-{Guid.NewGuid():N}.svg");

        try
        {
            await File.WriteAllTextAsync(path,
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"400\" height=\"200\"><rect width=\"400\" height=\"200\" fill=\"#2463eb\"/></svg>",
                timeout.Token);
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
            var probe = new FileProbe(path, ".svg", "<svg"u8.ToArray())
            {
                Kind = "image",
                Size = new FileInfo(path).Length,
            };
            var pinnedInput = WindowsHandleTransfer.OpenPinnedReadOnlyFile(path);
            long hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(pinnedInput.Handle, host.SafeHandle);
            pinnedInput.Handle.Dispose();
            await channel.SendAsync(new PreviewOpenHandle(requestId, hostHandle, pinnedInput.Length, path, probe)
            {
                TargetWidth = 100,
                TargetHeight = 100,
            }, timeout.Token);

            while (true)
            {
                try
                {
                    await File.WriteAllTextAsync(path, "not the requested svg", timeout.Token);
                    break;
                }
                catch (IOException)
                {
                    await Task.Delay(10, timeout.Token);
                }
            }

            PreviewSurface? surface = null;
            PreviewReady? ready = null;
            while (surface is null || ready is null)
            {
                ControlMessage message = await channel.ReceiveAsync(timeout.Token)
                    ?? throw new EndOfStreamException("RasterHost closed before completing the SVG preview");
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
            }

            string anchoredPath = Path.Combine(
                Path.GetTempPath(), "QuickLookNext", "raster-inputs", host.Id.ToString(), "input-" + requestId, "source.svg");
            await using (var anchored = new FileStream(
                anchoredPath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete, 4096, useAsync: true))
            using (var reader = new StreamReader(anchored))
                Assert.StartsWith("<svg", await reader.ReadToEndAsync(timeout.Token));
            Assert.Equal((100u, 50u), (surface.Width, surface.Height));
            Assert.Equal("image", ready.Kind);
            Assert.Equal((100d, 50d), (ready.PreferredWidth, ready.PreferredHeight));
            await channel.SendAsync(new PreviewClose(requestId), timeout.Token);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            try { await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5)); }
            catch { try { host.Kill(entireProcessTree: true); } catch { } }
            try { File.Delete(path); } catch { }
        }
    }
}
