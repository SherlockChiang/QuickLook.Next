using Microsoft.UI.Dispatching;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class UiThreadWatchdog : IDisposable
{
    private static readonly TimeSpan ProbeInterval = TimeSpan.FromSeconds(1);
    private static readonly TimeSpan HangThreshold = TimeSpan.FromSeconds(2.5);

    private readonly DispatcherQueue _dispatcherQueue;
    private readonly CancellationTokenSource _cts = new();
    private long _lastAckTicks = DateTimeOffset.UtcNow.UtcTicks;
    private int _pendingProbe;
    private bool _disposed;

    public UiThreadWatchdog(DispatcherQueue dispatcherQueue)
    {
        _dispatcherQueue = dispatcherQueue;
        _ = RunAsync();
    }

    private async Task RunAsync()
    {
        using var timer = new PeriodicTimer(ProbeInterval);
        try
        {
            while (await timer.WaitForNextTickAsync(_cts.Token))
            {
                QueueProbe();
                TimeSpan sinceAck = DateTimeOffset.UtcNow - new DateTimeOffset(
                    Interlocked.Read(ref _lastAckTicks),
                    TimeSpan.Zero);
                if (sinceAck >= HangThreshold)
                {
                    DiagLog.Write(
                        "App",
                        $"ui watchdog: dispatcher unresponsive for {sinceAck.TotalSeconds:0.0}s; pendingProbe={Volatile.Read(ref _pendingProbe)}");
                }
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "ui watchdog failed: " + ex.Message);
        }
    }

    private void QueueProbe()
    {
        if (Interlocked.Exchange(ref _pendingProbe, 1) == 1)
            return;

        if (!_dispatcherQueue.TryEnqueue(() =>
            {
                Interlocked.Exchange(ref _lastAckTicks, DateTimeOffset.UtcNow.UtcTicks);
                Interlocked.Exchange(ref _pendingProbe, 0);
            }))
        {
            Interlocked.Exchange(ref _pendingProbe, 0);
            DiagLog.Write("App", "ui watchdog: dispatcher enqueue failed");
        }
    }

    public void Dispose()
    {
        if (_disposed)
            return;

        _disposed = true;
        _cts.Cancel();
        _cts.Dispose();
    }
}
