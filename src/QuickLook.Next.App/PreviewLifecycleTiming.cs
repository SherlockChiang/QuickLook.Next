using System.Diagnostics;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class PreviewLifecycleTiming
{
    private static long _nextId;
    private readonly long _started;
    private long _lastMark;
    private bool _terminal;

    public PreviewLifecycleTiming(
        int generation,
        PreviewNavigationSource source,
        string path,
        long receivedTimestamp)
    {
        CorrelationId = Interlocked.Increment(ref _nextId);
        Generation = generation;
        _started = receivedTimestamp == 0 ? Stopwatch.GetTimestamp() : receivedTimestamp;
        _lastMark = _started;
        Mark("preview-begin", $"source={source}; path={path}");
    }

    public long CorrelationId { get; }
    public int Generation { get; }
    public bool IsTerminal => _terminal;

    public void Mark(string phase, string? detail = null)
    {
        if (_terminal)
            return;
        long now = Stopwatch.GetTimestamp();
        double delta = Stopwatch.GetElapsedTime(_lastMark, now).TotalMilliseconds;
        double total = Stopwatch.GetElapsedTime(_started, now).TotalMilliseconds;
        _lastMark = now;
        DiagLog.Write(
            "Preview",
            $"cid={CorrelationId}; gen={Generation}; phase={phase}; delta={delta:0.0}ms; total={total:0.0}ms" +
            (string.IsNullOrWhiteSpace(detail) ? "" : $"; {detail}"));
    }

    public void Complete(string outcome)
    {
        if (_terminal)
            return;
        Mark("terminal", $"outcome={outcome}");
        _terminal = true;
    }
}
