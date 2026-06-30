using System.Runtime;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

/// <summary>
/// Frees the host's accumulated caches after a period of inactivity. A preview utility is idle the vast
/// majority of the time; trimming returns the PDF page cache + retired GPU surfaces to the OS and compacts
/// the large-object heap — keeping resident memory low without paying a host cold-start on the next
/// preview (we deliberately keep the host process + plugins warm for instant previews).
/// </summary>
internal sealed class IdleTrimmer : IDisposable
{
    // Idle threshold defaults to 2 minutes; override with QL_IDLE_TRIM_SECONDS (e.g. for testing/tuning).
    private static readonly TimeSpan IdleThreshold = TimeSpan.FromSeconds(
        int.TryParse(Environment.GetEnvironmentVariable("QL_IDLE_TRIM_SECONDS"), out var s) && s > 0 ? s : 120);
    private static readonly TimeSpan CheckInterval = TimeSpan.FromSeconds(15);

    private readonly CompositionProducer _producer;
    private readonly Timer _timer;
    private long _lastTicks;
    private int _trimmed; // 0 = active since last trim, 1 = already trimmed this idle period

    public IdleTrimmer(CompositionProducer producer)
    {
        _producer = producer;
        _lastTicks = DateTime.UtcNow.Ticks;
        _timer = new Timer(_ => Tick(), null, CheckInterval, CheckInterval);
    }

    /// <summary>Mark activity; called for every control message the host handles.</summary>
    public void Touch()
    {
        Interlocked.Exchange(ref _lastTicks, DateTime.UtcNow.Ticks);
        Interlocked.Exchange(ref _trimmed, 0);
    }

    private void Tick()
    {
        var idle = DateTime.UtcNow - new DateTime(Interlocked.Read(ref _lastTicks), DateTimeKind.Utc);
        if (idle < IdleThreshold) return;
        if (Interlocked.Exchange(ref _trimmed, 1) == 1) return; // trim once per idle stretch

        // Safe even while a preview is still shown: clearing the page cache only forces a re-render on the
        // next scroll, and ReleaseRetired only frees surfaces from already-closed previews.
        try
        {
            PdfPreviewSession.ClearCache();
            _producer.ReleaseRetired();
            GCSettings.LargeObjectHeapCompactionMode = GCLargeObjectHeapCompactionMode.CompactOnce;
            GC.Collect(GC.MaxGeneration, GCCollectionMode.Forced, blocking: true, compacting: true);
            DiagLog.Write("Host", "idle: trimmed caches + compacted GC");
        }
        catch (Exception ex) { DiagLog.Write("Host", "idle trim failed: " + ex.Message); }
    }

    public void Dispose() => _timer.Dispose();
}
