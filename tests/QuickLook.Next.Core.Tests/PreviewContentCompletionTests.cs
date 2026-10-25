using QuickLook.Next.App;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class PreviewContentCompletionTests
{
    [Fact]
    public async Task Does_not_report_ready_until_content_has_been_committed()
    {
        var rendering = new TaskCompletionSource();
        Task<bool> ready = PreviewContentCompletion.WaitAsync(() => rendering.Task, () => true, CancellationToken.None);
        Assert.False(ready.IsCompleted);
        rendering.SetResult();
        Assert.True(await ready);
    }

    [Fact]
    public async Task Replaced_render_must_finish_before_the_preview_is_ready()
    {
        var old = new TaskCompletionSource();
        var replacement = new TaskCompletionSource();
        Task current = old.Task;
        Task<bool> ready = PreviewContentCompletion.WaitAsync(() => current, () => true, CancellationToken.None);
        current = replacement.Task;
        old.SetException(new InvalidOperationException("superseded render"));
        Assert.False(ready.IsCompleted);
        replacement.SetResult();
        Assert.True(await ready);
    }

    [Fact]
    public async Task Closing_a_preview_does_not_wait_for_stale_rendering()
    {
        using var cancellation = new CancellationTokenSource();
        var rendering = new TaskCompletionSource();
        Task<bool> ready = PreviewContentCompletion.WaitAsync(
            () => rendering.Task, () => !cancellation.IsCancellationRequested, cancellation.Token);
        cancellation.Cancel();
        Assert.False(await ready.WaitAsync(TimeSpan.FromSeconds(5)));
        rendering.SetResult();
    }

    [Fact]
    public async Task Current_failure_reaches_the_preview_error_handler()
    {
        Task failed = Task.FromException(new InvalidOperationException("render failed"));
        await Assert.ThrowsAsync<InvalidOperationException>(() =>
            PreviewContentCompletion.WaitAsync(() => failed, () => true, CancellationToken.None));
    }
}
