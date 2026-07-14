using System.Diagnostics;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal static class AppStartupTiming
{
    private static long _started;
    private static long _lastMark;

    public static void Start()
    {
        _started = Stopwatch.GetTimestamp();
        _lastMark = _started;
    }

    public static void Mark(string phase)
    {
        long now = Stopwatch.GetTimestamp();
        if (_started == 0)
        {
            _started = now;
            _lastMark = now;
        }
        double delta = Stopwatch.GetElapsedTime(_lastMark, now).TotalMilliseconds;
        double total = Stopwatch.GetElapsedTime(_started, now).TotalMilliseconds;
        _lastMark = now;
        DiagLog.Write("Startup", $"phase={phase}; delta={delta:0.0}ms; total={total:0.0}ms");
    }
}
