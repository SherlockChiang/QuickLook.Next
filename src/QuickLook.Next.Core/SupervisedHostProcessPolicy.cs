using System.Runtime.InteropServices;

namespace QuickLook.Next.Core;

/// <summary>
/// Applies process-wide fault-reporting policy for background processes that are
/// monitored and restarted by the App.
/// </summary>
public static class SupervisedHostProcessPolicy
{
    private const uint SEM_FAILCRITICALERRORS = 0x0001;
    private const uint SEM_NOGPFAULTERRORBOX = 0x0002;
    private const uint SEM_NOOPENFILEERRORBOX = 0x8000;
    private const uint WER_FAULT_REPORTING_NO_UI = 0x0020;

    /// <summary>
    /// Suppresses interactive system error dialogs while preserving WER reporting,
    /// local dumps, and the process exit code for supervisor diagnostics.
    /// </summary>
    public static void SuppressInteractiveErrorUi()
    {
        if (!OperatingSystem.IsWindows())
            return;

        // This policy is diagnostic hardening and must never prevent the host from starting.
        try
        {
            uint currentErrorMode = GetErrorMode();
            // SEM_NOGPFAULTERRORBOX suppresses WER itself, not just its UI.
            _ = SetErrorMode(
                (currentErrorMode & ~SEM_NOGPFAULTERRORBOX)
                | SEM_FAILCRITICALERRORS
                | SEM_NOOPENFILEERRORBOX);
        }
        catch (Exception)
        {
        }

        try
        {
            if (WerGetFlags(GetCurrentProcess(), out uint currentWerFlags) >= 0)
                _ = WerSetFlags(currentWerFlags | WER_FAULT_REPORTING_NO_UI);
        }
        catch (Exception)
        {
        }
    }

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern uint GetErrorMode();

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern uint SetErrorMode(uint mode);

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern nint GetCurrentProcess();

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int WerGetFlags(nint process, out uint flags);

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int WerSetFlags(uint flags);
}
