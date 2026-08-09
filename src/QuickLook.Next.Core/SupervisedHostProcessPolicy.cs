using System.Diagnostics.CodeAnalysis;
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
    private const uint WER_FAULT_REPORTING_ALWAYS_SHOW_UI = 0x0010;
    private const uint WER_FAULT_REPORTING_NO_UI = 0x0020;

    /// <summary>
    /// Suppresses interactive system error dialogs while preserving supervisor
    /// exit-code and log diagnostics. WER no-UI reporting remains best effort.
    /// </summary>
    public static void SuppressInteractiveErrorUi()
    {
        if (!OperatingSystem.IsWindows())
            return;

        // This policy is diagnostic hardening and must never prevent the host from starting.
        try
        {
            if (WerGetFlags(GetCurrentProcess(), out uint currentWerFlags) >= 0)
            {
                _ = WerSetFlags(
                    (currentWerFlags & ~WER_FAULT_REPORTING_ALWAYS_SHOW_UI)
                    | WER_FAULT_REPORTING_NO_UI);
            }
        }
        catch (Exception)
        {
        }

        try
        {
            uint currentErrorMode = GetErrorMode();
            // WER no-UI alone does not cover every UnhandledExceptionFilter path.
            // Background hosts must fail closed without an Application Error box,
            // even when that makes WER/local-dump collection best effort.
            _ = SetErrorMode(
                currentErrorMode
                | SEM_FAILCRITICALERRORS
                | SEM_NOGPFAULTERRORBOX
                | SEM_NOOPENFILEERRORBOX);
        }
        catch (Exception)
        {
        }
    }

    /// <summary>
    /// Ends a supervised leaf process after its logical cleanup boundary without running another CLR
    /// shutdown/finalizer pass. Windows reclaims all remaining process-scoped native state atomically.
    /// </summary>
    [DoesNotReturn]
    public static void ExitImmediately(int exitCode)
    {
        if (OperatingSystem.IsWindows())
            _ = TerminateProcess(GetCurrentProcess(), unchecked((uint)exitCode));

        // TerminateProcess does not return on success for the current process. Preserve a portable
        // fallback for tests or an unexpected native failure.
        Environment.Exit(exitCode);
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

    [DllImport("kernel32.dll", ExactSpelling = true, SetLastError = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool TerminateProcess(nint process, uint exitCode);

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int WerGetFlags(nint process, out uint flags);

    [DllImport("kernel32.dll", ExactSpelling = true)]
    [DefaultDllImportSearchPaths(DllImportSearchPath.System32)]
    private static extern int WerSetFlags(uint flags);
}
