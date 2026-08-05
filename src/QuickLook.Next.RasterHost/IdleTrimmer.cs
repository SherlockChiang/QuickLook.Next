using System.Runtime;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

/// <summary>
/// Frees the host's accumulated caches after a period of inactivity. A preview utility is idle the vast
/// majority of the time; trimming returns the PDF page cache + retired GPU surfaces to the OS and compacts
/// the large-object heap — keeping resident memory low without paying a host cold-start on the next
/// preview (we deliberately keep the host process + plugins warm for instant previews).
/// </summary>
internal sealed class IdleTrimmer : IAsyncDisposable
{
    // Idle threshold defaults to 2 minutes; override with QL_IDLE_TRIM_SECONDS (e.g. for testing/tuning).
    private static readonly TimeSpan IdleThreshold = TimeSpan.FromSeconds(
        int.TryParse(Environment.GetEnvironmentVariable("QL_IDLE_TRIM_SECONDS"), out var s) && s > 0 ? s : 120);
    private static readonly TimeSpan CheckInterval = TimeSpan.FromMilliseconds(
        int.TryParse(Environment.GetEnvironmentVariable("QL_IDLE_TRIM_CHECK_MILLISECONDS"), out var ms)
            && ms is >= 50 and <= 15_000
            ? ms
            : 15_000);

    private readonly CompositionProducer _producer;
    private readonly Timer _timer;
    private readonly object _sync = new();
    private long _lastTicks;
    private bool _trimmed;
    private bool _previewActive;
    private bool _disposed;

    public IdleTrimmer(CompositionProducer producer)
    {
        _producer = producer;
        _lastTicks = DateTime.UtcNow.Ticks;
        _timer = new Timer(_ => Tick(), null, CheckInterval, CheckInterval);
    }

    /// <summary>Mark activity; called for every control message the host handles.</summary>
    public void Touch()
    {
        lock (_sync)
        {
            if (_disposed) return;
            TouchCore();
        }
    }

    public void SetPreviewActive(bool active)
    {
        lock (_sync)
        {
            if (_disposed) return;
            _previewActive = active;
            TouchCore();
        }
    }

    private void Tick()
    {
        lock (_sync)
        {
            if (_disposed || _previewActive) return;
            var idle = DateTime.UtcNow - new DateTime(_lastTicks, DateTimeKind.Utc);
            if (idle < IdleThreshold || _trimmed) return;
            _trimmed = true;

            try
            {
                PdfPreviewSession.ClearCache();
                _producer.ReleaseRetired();
                GCSettings.LargeObjectHeapCompactionMode = GCLargeObjectHeapCompactionMode.CompactOnce;
                GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
                GC.WaitForPendingFinalizers();
                GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
                DiagLog.Write("Host", "idle: trimmed caches + compacted GC");
            }
            catch (Exception ex) { DiagLog.Write("Host", "idle trim failed: " + ex.Message); }
        }
    }

    private void TouchCore()
    {
        _lastTicks = DateTime.UtcNow.Ticks;
        _trimmed = false;
    }

    public async ValueTask DisposeAsync()
    {
        lock (_sync)
        {
            if (_disposed) return;
            _disposed = true;
        }

        await _timer.DisposeAsync().ConfigureAwait(false);
    }
}
