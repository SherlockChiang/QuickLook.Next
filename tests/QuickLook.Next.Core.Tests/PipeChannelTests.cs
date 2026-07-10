using System.IO.Pipes;
using System.Text;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class PipeChannelTests
{
    private static readonly TimeSpan Timeout = TimeSpan.FromSeconds(15);

    [Fact]
    public async Task Channels_send_and_receive_in_both_directions()
    {
        await using var pipes = await ConnectedPipePair.CreateAsync();
        using var left = new PipeChannel(pipes.Server);
        using var right = new PipeChannel(pipes.Client);
        var fromLeft = new Hello(1234, "token");
        var fromRight = new PreviewError("request", "failed");

        Task<ControlMessage?> receiveLeft = left.ReceiveAsync();
        Task<ControlMessage?> receiveRight = right.ReceiveAsync();
        await Task.WhenAll(left.SendAsync(fromLeft), right.SendAsync(fromRight)).WaitAsync(Timeout);

        Assert.Equal(fromRight, Assert.IsType<PreviewError>(await receiveLeft.WaitAsync(Timeout)));
        Assert.Equal(fromLeft, Assert.IsType<Hello>(await receiveRight.WaitAsync(Timeout)));
    }

    [Fact]
    public async Task Receive_accepts_crlf_and_unterminated_final_line()
    {
        var first = new PreviewError("first", "one");
        var second = new PreviewClose("second");
        byte[] bytes = Encoding.UTF8.GetBytes(ProtocolJson.Serialize(first) + "\r\n" + ProtocolJson.Serialize(second));
        using var receiver = new PipeChannel(new MemoryStream(bytes, writable: true));

        Assert.Equal(first, Assert.IsType<PreviewError>(await receiver.ReceiveAsync().WaitAsync(Timeout)));
        Assert.Equal(second, Assert.IsType<PreviewClose>(await receiver.ReceiveAsync().WaitAsync(Timeout)));
        Assert.Null(await receiver.ReceiveAsync().WaitAsync(Timeout));
    }

    [Fact]
    public async Task Send_rejects_oversized_output_and_channel_remains_usable()
    {
        await using var pipes = await ConnectedPipePair.CreateAsync();
        using var sender = new PipeChannel(pipes.Server);
        using var receiver = new PipeChannel(pipes.Client);
        var oversized = new PreviewError("large", new string('x', PipeChannel.MaxControlLineChars));

        await Assert.ThrowsAsync<InvalidDataException>(() => sender.SendAsync(oversized));
        var valid = new PreviewClose("valid");
        Task<ControlMessage?> receive = receiver.ReceiveAsync();
        await sender.SendAsync(valid).WaitAsync(Timeout);
        Assert.Equal(valid, Assert.IsType<PreviewClose>(await receive.WaitAsync(Timeout)));
    }

    [Fact]
    public async Task Receive_honors_cancellation_and_channel_remains_usable()
    {
        await using var pipes = await ConnectedPipePair.CreateAsync();
        using var receiver = new PipeChannel(pipes.Server);
        using var sender = new PipeChannel(pipes.Client);
        using var cancellation = new CancellationTokenSource();
        Task<ControlMessage?> canceled = receiver.ReceiveAsync(cancellation.Token);

        cancellation.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => canceled);
        var valid = new PreviewError("valid", "ok");
        Task<ControlMessage?> receive = receiver.ReceiveAsync();
        await sender.SendAsync(valid).WaitAsync(Timeout);
        Assert.Equal(valid, Assert.IsType<PreviewError>(await receive.WaitAsync(Timeout)));
    }

    private sealed class ConnectedPipePair : IAsyncDisposable
    {
        private bool _clientDisposed;
        private ConnectedPipePair(NamedPipeServerStream server, NamedPipeClientStream client) => (Server, Client) = (server, client);
        public NamedPipeServerStream Server { get; }
        public NamedPipeClientStream Client { get; }

        public static async Task<ConnectedPipePair> CreateAsync()
        {
            string name = $"QuickLook.Next.Tests.{Environment.ProcessId}.{Guid.NewGuid():N}";
            var server = new NamedPipeServerStream(name, PipeDirection.InOut, 1, PipeTransmissionMode.Byte, PipeOptions.Asynchronous);
            var client = new NamedPipeClientStream(".", name, PipeDirection.InOut, PipeOptions.Asynchronous);
            try
            {
                using var cancellation = new CancellationTokenSource(Timeout);
                await Task.WhenAll(server.WaitForConnectionAsync(cancellation.Token), client.ConnectAsync(cancellation.Token)).WaitAsync(Timeout);
                return new ConnectedPipePair(server, client);
            }
            catch
            {
                client.Dispose();
                server.Dispose();
                throw;
            }
        }

        public void DisposeClient()
        {
            if (!_clientDisposed)
            {
                _clientDisposed = true;
                Client.Dispose();
            }
        }

        public ValueTask DisposeAsync()
        {
            DisposeClient();
            Server.Dispose();
            return ValueTask.CompletedTask;
        }
    }
}
