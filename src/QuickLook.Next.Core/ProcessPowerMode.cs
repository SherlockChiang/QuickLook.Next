using System.Diagnostics;
using System.Runtime.InteropServices;

namespace QuickLook.Next.Core;

/// <summary>Small wrapper for Windows EcoQoS / Task Manager efficiency mode.</summary>
public static class ProcessPowerMode
{
    private const int ProcessPowerThrottling = 4;
    private const uint PROCESS_POWER_THROTTLING_CURRENT_VERSION = 1;
    private const uint PROCESS_POWER_THROTTLING_EXECUTION_SPEED = 0x1;

    public static void SetCurrentBackgroundEfficiency(bool enabled, string tag)
        => TrySet(GetCurrentProcess(), enabled, tag, "current");

    public static void SetProcessBackgroundEfficiency(Process? process, bool enabled, string tag)
    {
        if (process is null)
            return;

        try
        {
            if (process.HasExited)
                return;
            TrySet(process.Handle, enabled, tag, $"pid={process.Id}");
        }
        catch (Exception ex)
        {
            DiagLog.Write(tag, $"efficiency mode {(enabled ? "enable" : "disable")} skipped: {ex.Message}");
        }
    }

    private static void TrySet(nint processHandle, bool enabled, string tag, string target)
    {
        var state = new PROCESS_POWER_THROTTLING_STATE
        {
            Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask = enabled ? PROCESS_POWER_THROTTLING_EXECUTION_SPEED : 0,
        };

        bool ok = SetProcessInformation(
            processHandle,
            ProcessPowerThrottling,
            ref state,
            Marshal.SizeOf<PROCESS_POWER_THROTTLING_STATE>());
        if (ok)
        {
            DiagLog.Write(tag, $"efficiency mode {(enabled ? "enabled" : "disabled")} for {target}");
            return;
        }

        int error = Marshal.GetLastWin32Error();
        DiagLog.Write(tag, $"efficiency mode {(enabled ? "enable" : "disable")} failed for {target}: win32={error}");
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PROCESS_POWER_THROTTLING_STATE
    {
        public uint Version;
        public uint ControlMask;
        public uint StateMask;
    }

    [DllImport("kernel32.dll")]
    private static extern nint GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetProcessInformation(
        nint hProcess,
        int processInformationClass,
        ref PROCESS_POWER_THROTTLING_STATE processInformation,
        int processInformationSize);
}
