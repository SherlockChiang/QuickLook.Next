using System.Collections.Concurrent;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class PdfPageRenderCancellationTests
{
    [Fact]
    public void New_generation_cancels_all_older_waiters_for_only_the_same_document_and_page()
    {
        using var oldest = new CancellationTokenSource();
        using var older = new CancellationTokenSource();
        using var current = new CancellationTokenSource();
        using var newer = new CancellationTokenSource();
        using var otherPage = new CancellationTokenSource();
        using var otherDocument = new CancellationTokenSource();
        var requests = new ConcurrentDictionary<(string, int, long), CancellationTokenSource>();
        requests[("document", 0, 1)] = oldest;
        requests[("document", 0, 2)] = older;
        requests[("document", 0, 3)] = current;
        requests[("document", 0, 4)] = newer;
        requests[("document", 1, 1)] = otherPage;
        requests[("other", 0, 1)] = otherDocument;

        PdfPageRenderCancellation.CancelSuperseded(requests, "document", 0, 3);

        Assert.True(oldest.IsCancellationRequested);
        Assert.True(older.IsCancellationRequested);
        Assert.False(current.IsCancellationRequested);
        Assert.False(newer.IsCancellationRequested);
        Assert.False(otherPage.IsCancellationRequested);
        Assert.False(otherDocument.IsCancellationRequested);
        Assert.False(requests.ContainsKey(("document", 0, 1)));
        Assert.False(requests.ContainsKey(("document", 0, 2)));
        Assert.Equal(4, requests.Count);
        Assert.True(older.Token.IsCancellationRequested); // The worker still owns disposal.
    }

    [Fact]
    public void Repeated_or_older_generation_does_not_cancel_the_current_waiter()
    {
        using var current = new CancellationTokenSource();
        var requests = new ConcurrentDictionary<(string, int, long), CancellationTokenSource>();
        requests[("document", 0, 5)] = current;

        PdfPageRenderCancellation.CancelSuperseded(requests, "document", 0, 5);
        PdfPageRenderCancellation.CancelSuperseded(requests, "document", 0, 4);

        Assert.False(current.IsCancellationRequested);
        Assert.Same(current, Assert.Single(requests).Value);
    }

    [Fact]
    public void Superseding_tolerates_a_waiter_that_finished_and_disposed_its_token()
    {
        var completed = new CancellationTokenSource();
        completed.Dispose();
        var requests = new ConcurrentDictionary<(string, int, long), CancellationTokenSource>();
        requests[("document", 0, 1)] = completed;

        PdfPageRenderCancellation.CancelSuperseded(requests, "document", 0, 2);
        PdfPageRenderCancellation.CancelSuperseded(requests, "document", 0, 2);

        Assert.Empty(requests);
    }

    [Fact]
    public async Task Superseding_cancels_only_the_old_waiter_not_its_shared_render()
    {
        using var oldWaiter = new CancellationTokenSource();
        using var newWaiter = new CancellationTokenSource();
        var requests = new ConcurrentDictionary<(string, int, long), CancellationTokenSource>();
        requests[("document", 0, 1)] = oldWaiter;
        requests[("document", 0, 2)] = newWaiter;
        var render = new TaskCompletionSource<int>(TaskCreationOptions.RunContinuationsAsynchronously);
        Task<int> oldResult = render.Task.WaitAsync(oldWaiter.Token);
        Task<int> newResult = render.Task.WaitAsync(newWaiter.Token);

        PdfPageRenderCancellation.CancelSuperseded(requests, "document", 0, 2);

        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => oldResult);
        Assert.False(render.Task.IsCompleted);
        Assert.False(newResult.IsCompleted);
        render.SetResult(42);
        Assert.Equal(42, await newResult);
        Assert.Same(newWaiter, Assert.Single(requests).Value);
    }
}
