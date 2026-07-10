using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class PendingRequestsTests
{
    [Fact]
    public async Task Cancel_completes_task_as_canceled_and_rejects_late_result()
    {
        var pending = new PendingRequests();
        var (id, completion) = pending.Begin(TimeSpan.FromSeconds(5));

        Assert.True(pending.Cancel(id));
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => completion);
        Assert.False(pending.TryComplete(id, new PreviewError(id, "late")));
    }

    [Fact]
    public async Task FailAll_fails_every_current_request()
    {
        var pending = new PendingRequests();
        var first = pending.Begin(TimeSpan.FromSeconds(5));
        var second = pending.Begin(TimeSpan.FromSeconds(5));
        var failure = new IOException("channel failed");

        pending.FailAll(failure);

        Assert.Same(failure, await Assert.ThrowsAsync<IOException>(() => first.Completion));
        Assert.Same(failure, await Assert.ThrowsAsync<IOException>(() => second.Completion));
        Assert.False(pending.Cancel(first.RequestId));
    }

    [Fact]
    public void Begin_rejects_invalid_timeout_without_affecting_future_requests()
    {
        var pending = new PendingRequests();

        Assert.Throws<ArgumentOutOfRangeException>(() => pending.Begin(TimeSpan.FromMilliseconds(-2)));
        var next = pending.Begin(TimeSpan.FromSeconds(1));
        Assert.True(pending.Cancel(next.RequestId));
    }

    [Fact]
    public async Task Completion_disarms_timeout_and_wins_once()
    {
        var pending = new PendingRequests();
        var (id, completion) = pending.Begin(TimeSpan.FromSeconds(1));
        var terminal = new PreviewError(id, "done");

        Assert.True(pending.TryComplete(id, terminal));
        Assert.Equal(terminal, await completion);
        Assert.False(pending.TryComplete(id, terminal));
        Assert.False(pending.Cancel(id));
    }
}
