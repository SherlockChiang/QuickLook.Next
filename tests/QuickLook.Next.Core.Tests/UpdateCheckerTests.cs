using System.Net;
using QuickLook.Next.App;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class UpdateCheckerTests
{
    [Fact]
    public async Task Deadline_cancels_a_body_that_stalls_after_headers_and_disposes_it()
    {
        var body = new StalledBody();
        using var client = CreateClient(body);
        Task check = UpdateChecker.CheckAsync(client, TimeSpan.FromMilliseconds(100), CancellationToken.None);

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => check.WaitAsync(TimeSpan.FromSeconds(10)));
        Assert.True(body.Disposed);
    }

    [Fact]
    public async Task Caller_cancellation_interrupts_body_read_and_disposes_it()
    {
        var body = new StalledBody();
        using var client = CreateClient(body);
        using var cancellation = new CancellationTokenSource();
        Task check = UpdateChecker.CheckAsync(client, TimeSpan.FromMinutes(1), cancellation.Token);
        await body.ReadStarted.Task.WaitAsync(TimeSpan.FromSeconds(10));

        cancellation.Cancel();

        await Assert.ThrowsAnyAsync<OperationCanceledException>(
            () => check.WaitAsync(TimeSpan.FromSeconds(10)));
        Assert.True(body.Disposed);
    }

    [Fact]
    public async Task Unknown_length_response_cannot_exceed_the_byte_budget()
    {
        using var body = new NonSeekableBody(new byte[16 * 1024 + 1]);
        using var client = CreateClient(body);

        await Assert.ThrowsAsync<InvalidDataException>(() =>
            UpdateChecker.CheckAsync(client, TimeSpan.FromSeconds(5), CancellationToken.None));
    }

    private static HttpClient CreateClient(Stream body)
        => new(new ResponseHandler(body)) { Timeout = Timeout.InfiniteTimeSpan };

    private sealed class ResponseHandler(Stream body) : HttpMessageHandler
    {
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
            => Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = new StreamContent(body) });
    }

    private class NonSeekableBody(byte[] bytes) : MemoryStream(bytes)
    {
        public override bool CanSeek => false;
    }

    private sealed class StalledBody : NonSeekableBody
    {
        public StalledBody() : base([]) { }

        public TaskCompletionSource ReadStarted { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public bool Disposed { get; private set; }

        public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
        {
            ReadStarted.TrySetResult();
            await Task.Delay(Timeout.InfiniteTimeSpan, cancellationToken);
            return 0;
        }

        protected override void Dispose(bool disposing)
        {
            Disposed = true;
            base.Dispose(disposing);
        }
    }
}
