using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using System.Text;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.ShellBroker.IntegrationTests;

public sealed class ShellBrokerIntegrationTests
{
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(15);

    [Fact]
    public async Task Host_rejects_bad_session_token()
    {
        await using BrokerSession session = await BrokerSession.StartAsync();
        await session.Channel.SendAsync($"HELLO\t{Environment.ProcessId}\t{session.Token}bad");
        Assert.Null(await session.Channel.ReceiveAsync());
        await session.Host.WaitForExitAsync().WaitAsync(Timeout);
    }

    [Fact]
    public async Task Host_rejects_control_message_before_authentication()
    {
        await using BrokerSession session = await BrokerSession.StartAsync();
        await session.Channel.SendAsync($"CLOSE\t{RequestId()}");
        Assert.Null(await session.Channel.ReceiveAsync());
        await session.Host.WaitForExitAsync().WaitAsync(Timeout);
    }

    [Fact]
    public async Task Host_rejects_wrong_pipe_server_process_id()
    {
        await using BrokerSession session = await BrokerSession.StartAsync();
        await session.Channel.SendAsync($"HELLO\t{int.MaxValue}\t{session.Token}");
        Assert.Null(await session.Channel.ReceiveAsync());
        await session.Host.WaitForExitAsync().WaitAsync(Timeout);
    }

    [Theory]
    [InlineData("OPEN\tinvalid\t128\tYQ==")]
    [InlineData("OPEN\t00000000000000000000000000000000\t128\tcmVsYXRpdmUuaWNv")]
    [InlineData("OPEN\t00000000000000000000000000000000\t513\tYQ==")]
    [InlineData("OPEN\t00000000000000000000000000000000\t128\tnot-base64")]
    [InlineData("UNKNOWN")]
    public async Task Authenticated_host_fails_closed_on_invalid_control_messages(string message)
    {
        await using BrokerSession session = await BrokerSession.StartAuthenticatedAsync();
        await session.Channel.SendAsync(message);
        Assert.Null(await session.Channel.ReceiveAsync());
        await session.Host.WaitForExitAsync().WaitAsync(Timeout);
    }

    [Fact]
    public async Task Thumbnail_packet_is_bounded_app_pulled_and_cleaned_on_close()
    {
        await using BrokerSession session = await BrokerSession.StartAuthenticatedAsync();
        string requestId = RequestId();
        await session.OpenAsync(requestId, FixturePath, 128);
        ThumbnailResponse response = ParseThumbnail(await session.Channel.ReceiveAsync());

        Assert.Equal(requestId, response.RequestId);
        Assert.InRange(response.Width, 1, 128);
        Assert.InRange(response.Height, 1, 128);
        Assert.Equal(8L + response.Width * response.Height * 4L, response.PacketLength);
        Assert.InRange(response.PacketLength, 12, 8 + 128 * 128 * 4);

        using var copiedHandle = WindowsHandleTransfer.DuplicateFileFromProcess(
            session.Host.SafeHandle, response.FileHandle, response.PacketLength);
        await session.Channel.SendAsync($"CLOSE\t{requestId}");
        await WaitUntilAsync(
            () => !Directory.Exists(Path.Combine(session.WritableRoot, "thumbnail-" + requestId)),
            Timeout);
        Assert.False(session.Host.HasExited);
        Assert.ThrowsAny<Exception>(() => WindowsHandleTransfer.DuplicateFileFromProcess(
            session.Host.SafeHandle, response.FileHandle, response.PacketLength));

        using var stream = new FileStream(copiedHandle, FileAccess.Read);
        byte[] packet = new byte[response.PacketLength];
        stream.ReadExactly(packet);
        Assert.Equal(response.Width, BitConverter.ToInt32(packet, 0));
        Assert.Equal(response.Height, BitConverter.ToInt32(packet, 4));
    }

    [Fact]
    public async Task Abrupt_pipe_disconnect_releases_active_handoff_and_packet_directory()
    {
        await using BrokerSession session = await BrokerSession.StartAuthenticatedAsync();
        string requestId = RequestId();
        await session.OpenAsync(requestId, FixturePath, 128);
        ThumbnailResponse response = ParseThumbnail(await session.Channel.ReceiveAsync());
        using var copiedHandle = WindowsHandleTransfer.DuplicateFileFromProcess(
            session.Host.SafeHandle, response.FileHandle, response.PacketLength);
        string packetDirectory = Path.Combine(session.WritableRoot, "thumbnail-" + requestId);
        Assert.True(Directory.Exists(packetDirectory));

        await session.DisconnectAsync();

        Assert.Equal(0, session.Host.ExitCode);
        Assert.False(Directory.Exists(packetDirectory));
        using var stream = new FileStream(copiedHandle, FileAccess.Read);
        byte[] header = new byte[8];
        stream.ReadExactly(header);
        Assert.Equal(response.Width, BitConverter.ToInt32(header, 0));
        Assert.Equal(response.Height, BitConverter.ToInt32(header, 4));
    }

    [Fact]
    public async Task Invalid_message_after_handoff_exits_and_cleans_packet_directory()
    {
        await using BrokerSession session = await BrokerSession.StartAuthenticatedAsync();
        string requestId = RequestId();
        await session.OpenAsync(requestId, FixturePath, 64);
        _ = ParseThumbnail(await session.Channel.ReceiveAsync());
        string packetDirectory = Path.Combine(session.WritableRoot, "thumbnail-" + requestId);
        Assert.True(Directory.Exists(packetDirectory));

        await session.Channel.SendAsync("UNKNOWN");
        Assert.Null(await session.Channel.ReceiveAsync());
        await session.Host.WaitForExitAsync().WaitAsync(Timeout);

        Assert.Equal(0, session.Host.ExitCode);
        Assert.False(Directory.Exists(packetDirectory));
    }

    [Fact]
    public async Task Second_open_is_rejected_until_first_handoff_closes()
    {
        await using BrokerSession session = await BrokerSession.StartAuthenticatedAsync();
        string first = RequestId();
        await session.OpenAsync(first, FixturePath, 64);
        _ = ParseThumbnail(await session.Channel.ReceiveAsync());

        string second = RequestId();
        await session.OpenAsync(second, FixturePath, 64);
        string[] error = (await session.Channel.ReceiveAsync() ?? "").Split('\t');
        Assert.Equal(["ERROR", second], error[..2]);
        Assert.Contains("active request", Decode(error[2]), StringComparison.OrdinalIgnoreCase);

        await session.Channel.SendAsync($"CLOSE\t{first}");
        await WaitUntilAsync(
            () => !Directory.Exists(Path.Combine(session.WritableRoot, "thumbnail-" + first)),
            Timeout);

        string third = RequestId();
        await session.OpenAsync(third, FixturePath, 64);
        _ = ParseThumbnail(await session.Channel.ReceiveAsync());
        await session.Channel.SendAsync($"CLOSE\t{third}");
        await WaitUntilAsync(
            () => !Directory.Exists(Path.Combine(session.WritableRoot, "thumbnail-" + third)),
            Timeout);
    }

    [Fact]
    public async Task Repeated_handoffs_do_not_leak_handles_or_packet_directories()
    {
        const int warmupCycles = 8;
        const int cycles = 32;
        const int handleGrowthBudget = 12;
        await using BrokerSession session = await BrokerSession.StartAuthenticatedAsync();

        for (int cycle = 0; cycle < warmupCycles; cycle++)
            await ExecuteHandoffAsync(session);
        session.Host.Refresh();
        int baselineHandles = session.Host.HandleCount;
        int peakHandles = baselineHandles;

        for (int cycle = 0; cycle < cycles; cycle++)
        {
            await ExecuteHandoffAsync(session);
            session.Host.Refresh();
            peakHandles = Math.Max(peakHandles, session.Host.HandleCount);
        }

        Assert.InRange(peakHandles, 1, baselineHandles + handleGrowthBudget);
        Assert.Empty(Directory.EnumerateDirectories(session.WritableRoot, "thumbnail-*"));
    }

    private static async Task ExecuteHandoffAsync(BrokerSession session)
    {
        string requestId = RequestId();
        await session.OpenAsync(requestId, FixturePath, 64);
        ThumbnailResponse response = ParseThumbnail(await session.Channel.ReceiveAsync());
        using (WindowsHandleTransfer.DuplicateFileFromProcess(
                   session.Host.SafeHandle, response.FileHandle, response.PacketLength))
        {
        }
        await session.Channel.SendAsync($"CLOSE\t{requestId}");
        await WaitUntilAsync(
            () => !Directory.Exists(Path.Combine(session.WritableRoot, "thumbnail-" + requestId)),
            Timeout);
        Assert.False(session.Host.HasExited);
        Assert.ThrowsAny<Exception>(() => WindowsHandleTransfer.DuplicateFileFromProcess(
            session.Host.SafeHandle, response.FileHandle, response.PacketLength));
    }

    private static string FixturePath => Path.Combine(AppContext.BaseDirectory, "QuickLookNext.ico");

    private static string RequestId() => Guid.NewGuid().ToString("n");

    private static ThumbnailResponse ParseThumbnail(string? message)
    {
        string[] parts = (message ?? "").Split('\t');
        Assert.Equal(6, parts.Length);
        Assert.Equal("THUMB", parts[0]);
        return new ThumbnailResponse(
            parts[1],
            long.Parse(parts[2]),
            long.Parse(parts[3]),
            int.Parse(parts[4]),
            int.Parse(parts[5]));
    }

    private static string Decode(string value)
        => Encoding.UTF8.GetString(Convert.FromBase64String(value));

    private static async Task WaitUntilAsync(Func<bool> predicate, TimeSpan timeout)
    {
        using var cancellation = new CancellationTokenSource(timeout);
        while (!predicate())
            await Task.Delay(25, cancellation.Token);
    }

    private sealed record ThumbnailResponse(
        string RequestId, long FileHandle, long PacketLength, int Width, int Height);

    private sealed class BrokerSession : IAsyncDisposable
    {
        private readonly NamedPipeServerStream _pipe;

        private BrokerSession(
            NamedPipeServerStream pipe,
            TestChannel channel,
            Process host,
            string token,
            string writableRoot)
        {
            _pipe = pipe;
            Channel = channel;
            Host = host;
            Token = token;
            WritableRoot = writableRoot;
        }

        public TestChannel Channel { get; }
        public Process Host { get; }
        public string Token { get; }
        public string WritableRoot { get; }

        public static async Task<BrokerSession> StartAuthenticatedAsync()
        {
            BrokerSession session = await StartAsync();
            try
            {
                await session.Channel.SendAsync($"HELLO\t{Environment.ProcessId}\t{session.Token}");
                Assert.Equal("READY", await session.Channel.ReceiveAsync());
                return session;
            }
            catch
            {
                await session.DisposeAsync();
                throw;
            }
        }

        public static async Task<BrokerSession> StartAsync()
        {
            string pipeName = $"quicklook_next_shell_test_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
            string token = RandomNumberGenerator.GetHexString(32);
            string writableRoot = Path.Combine(
                Path.GetTempPath(), "QuickLookNextShellBrokerTests", Guid.NewGuid().ToString("n"));
            Directory.CreateDirectory(writableRoot);
            var pipe = new NamedPipeServerStream(
                pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
                PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
            Process host = StartHost(pipeName, token, writableRoot);
            try
            {
                await pipe.WaitForConnectionAsync().WaitAsync(Timeout);
                return new BrokerSession(pipe, new TestChannel(pipe), host, token, writableRoot);
            }
            catch
            {
                pipe.Dispose();
                await StopHostAsync(host);
                try { Directory.Delete(writableRoot, recursive: true); } catch { }
                throw;
            }
        }

        public Task OpenAsync(string requestId, string path, int size)
        {
            string encodedPath = Convert.ToBase64String(Encoding.UTF8.GetBytes(path));
            return Channel.SendAsync($"OPEN\t{requestId}\t{size}\t{encodedPath}");
        }

        public async Task DisconnectAsync()
        {
            Channel.Dispose();
            _pipe.Dispose();
            await Host.WaitForExitAsync().WaitAsync(Timeout);
        }

        public async ValueTask DisposeAsync()
        {
            try { Channel.Dispose(); } catch { }
            try { _pipe.Dispose(); } catch { }
            await StopHostAsync(Host);
            Directory.Delete(WritableRoot, recursive: true);
        }

        private static Process StartHost(string pipeName, string token, string writableRoot)
        {
            string executable = Path.Combine(
                AppContext.BaseDirectory, "ShellBroker", "QuickLook.Next.ShellBroker.exe");
            Assert.True(File.Exists(executable), $"Missing staged ShellBroker: {executable}");
            var startInfo = new ProcessStartInfo(executable)
            {
                UseShellExecute = false,
                CreateNoWindow = true,
            };
            foreach (string argument in new[]
                     { "--pipe", pipeName, "--session-token", token, "--writable-root", writableRoot })
                startInfo.ArgumentList.Add(argument);
            return Process.Start(startInfo) ?? throw new InvalidOperationException("Failed to start ShellBroker.");
        }

        private static async Task StopHostAsync(Process host)
        {
            try
            {
                if (!host.HasExited) host.Kill(entireProcessTree: true);
                await host.WaitForExitAsync().WaitAsync(Timeout);
            }
            finally { host.Dispose(); }
        }
    }

    private sealed class TestChannel(Stream stream) : IDisposable
    {
        private readonly StreamReader _reader = new(stream, Encoding.UTF8, false, leaveOpen: true);
        private readonly StreamWriter _writer = new(stream, new UTF8Encoding(false), leaveOpen: true)
        {
            AutoFlush = true,
        };

        public Task SendAsync(string line)
            => _writer.WriteLineAsync(line);

        public async Task<string?> ReceiveAsync()
            => await _reader.ReadLineAsync().WaitAsync(Timeout);

        public void Dispose()
        {
            _reader.Dispose();
            _writer.Dispose();
        }
    }
}
