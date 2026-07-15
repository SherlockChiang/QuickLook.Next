using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostCodecErrorTests
{
    [Fact]
    public async Task Image_failures_return_stable_path_free_codec_errors()
    {
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        string pipeName = $"quicklook_next_raster_codec_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
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

        try
        {
            await pipe.WaitForConnectionAsync(timeout.Token);
            using var channel = new PipeChannel(pipe);
            await channel.SendAsync(new Hello(Environment.ProcessId, token), timeout.Token);
            Assert.IsType<HostReady>(await channel.ReceiveAsync(timeout.Token));

            await AssertImageErrorAsync(channel, ".avif", PreviewErrorCodes.ImageCodecRequired, "avif", timeout.Token);
            await AssertImageErrorAsync(channel, ".png", PreviewErrorCodes.ImageDecodeFailed, "png", timeout.Token);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            try { await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5)); }
            catch { try { host.Kill(entireProcessTree: true); } catch { } }
        }
    }

    private static async Task AssertImageErrorAsync(
        PipeChannel channel,
        string extension,
        string expectedCode,
        string expectedFormat,
        CancellationToken cancellationToken)
    {
        string requestId = RandomNumberGenerator.GetHexString(32).ToLowerInvariant();
        string path = Path.Combine(Path.GetTempPath(), $"missing-{Guid.NewGuid():N}{extension}");
        var probe = new FileProbe(path, extension, []) { Kind = "image", Size = 1 };

        await channel.SendAsync(new PreviewOpen(requestId, path, probe)
        {
            TargetWidth = 256,
            TargetHeight = 256,
        }, cancellationToken);
        var error = Assert.IsType<PreviewError>(await channel.ReceiveAsync(cancellationToken));

        Assert.Equal(requestId, error.RequestId);
        Assert.Equal(expectedCode, error.Code);
        Assert.Equal(expectedFormat, error.Format);
        Assert.DoesNotContain(path, error.Message, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain(path, error.Code, StringComparison.OrdinalIgnoreCase);
        Assert.DoesNotContain(path, error.Format, StringComparison.OrdinalIgnoreCase);
    }
}
