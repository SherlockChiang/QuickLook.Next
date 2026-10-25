using Microsoft.UI.Dispatching;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class UiThreadWatchdog : IDisposable
{
    private readonly DispatcherHeartbeat _heartbeat;

    public UiThreadWatchdog(DispatcherQueue dispatcherQueue)
    {
        _heartbeat = new DispatcherHeartbeat(
            action => dispatcherQueue.TryEnqueue(() => action()),
            elapsed => DiagLog.Write("App", $"ui watchdog: dispatcher unresponsive for {elapsed.TotalSeconds:0.0}s"),
            message => DiagLog.Write("App", "ui watchdog: " + message),
            TimeSpan.FromSeconds(1),
            TimeSpan.FromSeconds(2.5));
    }

    public void Dispose() => _heartbeat.Dispose();
}
