using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class RasterHostAuthenticationTests
{
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(15);

    [Fact]
    public Task Host_rejects_bad_session_token()
        => AssertRejectedAsync((channel, token, cancellationToken) =>
            channel.SendAsync(new Hello(Environment.ProcessId, token + "bad"), cancellationToken));

    [Fact]
    public Task Host_rejects_control_message_before_authentication()
        => AssertRejectedAsync((channel, _, cancellationToken) =>
            channel.SendAsync(new PreviewClose("unauthenticated"), cancellationToken));

    private static async Task AssertRejectedAsync(Func<PipeChannel, string, CancellationToken, Task> send)
    {
        string pipeName = $"quicklook_next_raster_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
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
            using var connectTimeout = new CancellationTokenSource(Timeout);
            await pipe.WaitForConnectionAsync(connectTimeout.Token);
            using var protocolTimeout = new CancellationTokenSource(Timeout);
            using var channel = new PipeChannel(pipe);
            await send(channel, token, protocolTimeout.Token);
            Assert.Null(await channel.ReceiveAsync(protocolTimeout.Token));
            await host.WaitForExitAsync(protocolTimeout.Token);
        }
        finally
        {
            try { pipe.Dispose(); } catch { }
            try { await host.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5)); }
            catch { try { host.Kill(entireProcessTree: true); } catch { } }
        }
    }
}
