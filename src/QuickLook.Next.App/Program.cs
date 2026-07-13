using System.Diagnostics;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;

namespace QuickLook.Next.App;

// Explicit Main (DISABLE_XAML_GENERATED_MAIN): the App owns startup so it can wire the native bridge and
// host supervision. Spike learning: the auto-generated Main did not fire OnLaunched reliably here.
public static class Program
{
    private static Mutex? _singleInstanceMutex;

    [STAThread]
    private static void Main(string[] args)
    {
        if (args is ["--restricted-host-probe-child"])
        {
            if (!HostProcessLauncher.IsCurrentProcessInJob()
                || !HostProcessLauncher.CurrentProcessHasOnlyTraversalPrivilege())
                Thread.Sleep(TimeSpan.FromSeconds(30));
            return;
        }
        if (args is ["--smoke-restricted-host-launch"])
        {
            using var job = new HostProcessJob((nint)(128L * 1024 * 1024));
            using Process child = HostProcessLauncher.StartRestricted(
                Environment.ProcessPath ?? throw new InvalidOperationException("Current process path is unavailable."),
                ["--restricted-host-probe-child"],
                job);
            child.WaitForExit(10_000);
            Environment.ExitCode = child.HasExited ? 0 : 3;
            return;
        }

        // Single-instance guard: if another instance is already running (holding the named pipe),
        // exit immediately instead of becoming a broken tray-zombie process.
        _singleInstanceMutex = new Mutex(initiallyOwned: true, name: @"Global\QuickLook.Next.App", out bool createdNew);
        if (!createdNew)
            return;

        WinRT.ComWrappersSupport.InitializeComWrappers();
        Application.Start(_ =>
        {
            var context = new DispatcherQueueSynchronizationContext(DispatcherQueue.GetForCurrentThread());
            System.Threading.SynchronizationContext.SetSynchronizationContext(context);
            new App();
        });
    }
}
