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
        AppStartupTiming.Start();
        if (args is ["--restricted-host-probe-child"])
        {
            try
            {
                if (!HostProcessLauncher.IsCurrentProcessInJob()) Environment.ExitCode = 10;
                else if (!HostProcessLauncher.CurrentProcessHasOnlyTraversalPrivilege()) Environment.ExitCode = 12;
                else
                {
                    Environment.ExitCode = HostProcessLauncher.CurrentProcessMitigationStatus() switch
                    {
                        7 => 0,
                        int status when status < 0 => 100 + Math.Min(99, -status),
                        int status when (status & 1) == 0 => 13,
                        int status when (status & 2) == 0 => 15,
                        _ => 16,
                    };
                }
                if (Environment.ExitCode == 0 && !HostProcessJob.CurrentProcessHasRequiredPolicy())
                    Environment.ExitCode = 14;
            }
            catch { Environment.ExitCode = 19; }
            return;
        }
        if (args is ["--smoke-restricted-host-launch"])
        {
            try
            {
                using var job = new HostProcessJob((nint)(128L * 1024 * 1024));
                using Process child = HostProcessLauncher.StartRestricted(
                    Environment.ProcessPath ?? throw new InvalidOperationException("Current process path is unavailable."),
                    ["--restricted-host-probe-child"],
                    job);
                Environment.ExitCode = child.WaitForExit(10_000) ? child.ExitCode : 3;
            }
            catch (System.ComponentModel.Win32Exception ex) { Environment.ExitCode = 1000 + ex.NativeErrorCode; }
            catch
            {
                Environment.ExitCode = 21;
            }
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
