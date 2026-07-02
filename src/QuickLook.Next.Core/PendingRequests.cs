using System.Collections.Concurrent;

namespace QuickLook.Next.Core;

/// <summary>
/// Enforces the protocol invariant: every opened RequestId terminates in exactly one of
/// PreviewReady | PreviewError | timeout. The App opens a request, awaits the returned task, and the
/// channel dispatcher calls <see cref="TryComplete"/> when a terminal message arrives. A per-request
/// watchdog fails the task if the host doesn't answer in time (so a hung viewer never wedges the UI).
/// </summary>
public sealed class PendingRequests
{
    private sealed class Entry(TaskCompletionSource<ControlMessage> tcs, CancellationTokenSource watchdog) : IDisposable
    {
        public TaskCompletionSource<ControlMessage> Tcs { get; } = tcs;

        public CancellationTokenSource Watchdog { get; } = watchdog;

        public CancellationTokenRegistration WatchdogRegistration { get; set; }

        public void Dispose()
        {
            WatchdogRegistration.Dispose();
            Watchdog.Dispose();
        }
    }

    private readonly ConcurrentDictionary<string, Entry> _pending = new();

    /// <summary>Begin a request: returns its id and a task that completes on terminal response or timeout.</summary>
    public (string RequestId, Task<ControlMessage> Completion) Begin(TimeSpan timeout)
    {
        string id = Guid.NewGuid().ToString("n");
        var tcs = new TaskCompletionSource<ControlMessage>(TaskCreationOptions.RunContinuationsAsynchronously);
        var watchdog = new CancellationTokenSource();
        var entry = new Entry(tcs, watchdog);
        entry.WatchdogRegistration = watchdog.Token.Register(() =>
        {
            if (_pending.TryRemove(id, out var e))
            {
                e.Tcs.TrySetException(new TimeoutException($"preview request {id} timed out after {timeout.TotalMilliseconds:F0} ms"));
                e.Watchdog.Dispose();
            }
        });
        _pending[id] = entry;
        watchdog.CancelAfter(timeout);
        return (id, tcs.Task);
    }

    /// <summary>Resolve a request with its terminal message. Returns false for unknown/late ids.</summary>
    public bool TryComplete(string requestId, ControlMessage terminal)
    {
        if (_pending.TryRemove(requestId, out var e))
        {
            e.Dispose();
            return e.Tcs.TrySetResult(terminal);
        }
        return false;
    }

    /// <summary>Fail every outstanding request (e.g. the host crashed and the channel broke).</summary>
    public void FailAll(Exception reason)
    {
        foreach (var id in _pending.Keys)
            if (_pending.TryRemove(id, out var e))
            {
                e.Dispose();
                e.Tcs.TrySetException(reason);
            }
    }
}
