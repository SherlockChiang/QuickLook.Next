using System.Collections.Concurrent;
using QuickLook.Next.App;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class DispatcherHeartbeatTests
{
    [Fact]
    public async Task Reports_a_blocked_dispatcher_without_resuming_its_synchronization_context()
    {
        var context = new BlockedContext();
        var callbacks = new ConcurrentQueue<Action>();
        var detected = new TaskCompletionSource<TimeSpan>(TaskCreationOptions.RunContinuationsAsynchronously);
        SynchronizationContext? previous = SynchronizationContext.Current;
        DispatcherHeartbeat heartbeat;
        try
        {
            SynchronizationContext.SetSynchronizationContext(context);
            heartbeat = new DispatcherHeartbeat(
                action => { callbacks.Enqueue(action); return true; },
                elapsed => detected.TrySetResult(elapsed),
                message => detected.TrySetException(new Exception(message)),
                TimeSpan.FromMilliseconds(10), TimeSpan.FromMilliseconds(30));
        }
        finally { SynchronizationContext.SetSynchronizationContext(previous); }

        using (heartbeat)
        {
            TimeSpan elapsed = await detected.Task.WaitAsync(TimeSpan.FromSeconds(10));
            Assert.True(elapsed >= TimeSpan.FromMilliseconds(30));
            Assert.Equal(0, context.Posts);
            Assert.Single(callbacks);
        }
        await heartbeat.Completion.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.True(callbacks.TryDequeue(out Action? lateAcknowledgement));
        lateAcknowledgement();
        heartbeat.Dispose();
    }

    [Fact]
    public async Task Rejected_dispatcher_enqueue_is_reported_and_can_be_stopped()
    {
        var failure = new TaskCompletionSource<string>(TaskCreationOptions.RunContinuationsAsynchronously);
        var heartbeat = new DispatcherHeartbeat(
            _ => false, _ => { }, message => failure.TrySetResult(message),
            TimeSpan.FromMilliseconds(10), TimeSpan.FromSeconds(30));
        using (heartbeat)
            Assert.Equal("dispatcher enqueue failed", await failure.Task.WaitAsync(TimeSpan.FromSeconds(10)));
        await heartbeat.Completion.WaitAsync(TimeSpan.FromSeconds(10));
    }

    private sealed class BlockedContext : SynchronizationContext
    {
        private int _posts;
        public int Posts => Volatile.Read(ref _posts);
        public override void Post(SendOrPostCallback callback, object? state) => Interlocked.Increment(ref _posts);
    }
}
