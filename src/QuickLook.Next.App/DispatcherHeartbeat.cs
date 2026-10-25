using System.Diagnostics;

namespace QuickLook.Next.App;

// Only the acknowledgement runs on the dispatcher. Monitoring must continue
// while the UI's SynchronizationContext has stopped processing callbacks.
internal sealed class DispatcherHeartbeat : IDisposable
{
    private readonly Func<Action, bool> _tryEnqueue;
    private readonly Action<TimeSpan> _reportHang;
    private readonly Action<string> _reportFailure;
    private readonly TimeSpan _probeInterval;
    private readonly TimeSpan _hangThreshold;
    private readonly CancellationTokenSource _cts = new();
    private long _lastAck = Stopwatch.GetTimestamp();
    private int _pendingProbe;
    private int _disposed;

    public DispatcherHeartbeat(
        Func<Action, bool> tryEnqueue, Action<TimeSpan> reportHang, Action<string> reportFailure,
        TimeSpan probeInterval, TimeSpan hangThreshold)
    {
        _tryEnqueue = tryEnqueue;
        _reportHang = reportHang;
        _reportFailure = reportFailure;
        _probeInterval = probeInterval;
        _hangThreshold = hangThreshold;
        Completion = RunAsync(_cts.Token);
    }

    internal Task Completion { get; }

    private async Task RunAsync(CancellationToken token)
    {
        using var timer = new PeriodicTimer(_probeInterval);
        try
        {
            while (await timer.WaitForNextTickAsync(token).ConfigureAwait(false))
            {
                if (token.IsCancellationRequested) return;
                QueueProbe();
                TimeSpan elapsed = Stopwatch.GetElapsedTime(Interlocked.Read(ref _lastAck));
                if (elapsed >= _hangThreshold)
                    _reportHang(elapsed);
            }
        }
        catch (OperationCanceledException) when (token.IsCancellationRequested) { }
        catch (Exception ex)
        {
            _reportFailure(ex.Message);
        }
    }

    private void QueueProbe()
    {
        if (Interlocked.Exchange(ref _pendingProbe, 1) == 1) return;
        if (!_tryEnqueue(() =>
        {
            if (Volatile.Read(ref _disposed) != 0) return;
            Interlocked.Exchange(ref _lastAck, Stopwatch.GetTimestamp());
            Interlocked.Exchange(ref _pendingProbe, 0);
        }))
        {
            Interlocked.Exchange(ref _pendingProbe, 0);
            _reportFailure("dispatcher enqueue failed");
        }
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0) return;
        _cts.Cancel();
        _cts.Dispose();
    }
}
