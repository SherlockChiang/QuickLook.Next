using System.Runtime.InteropServices;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class SupervisedHostProcessPolicyTests
{
    private const uint RequiredErrorMode = 0x0001 | 0x0002 | 0x8000;
    private const uint WerFaultReportingAlwaysShowUi = 0x0010;
    private const uint WerFaultReportingNoUi = 0x0020;

    [Fact]
    public void Suppression_sets_process_and_WER_no_UI_modes()
    {
        if (!OperatingSystem.IsWindows())
            return;

        uint originalErrorMode = GetErrorMode();
        Assert.True(WerGetFlags(GetCurrentProcess(), out uint originalWerFlags) >= 0);

        try
        {
            _ = SetErrorMode(originalErrorMode & ~RequiredErrorMode);
            Assert.True(WerSetFlags(
                (originalWerFlags | WerFaultReportingAlwaysShowUi)
                & ~WerFaultReportingNoUi) >= 0);

            SupervisedHostProcessPolicy.SuppressInteractiveErrorUi();

            Assert.Equal(RequiredErrorMode, GetErrorMode() & RequiredErrorMode);
            Assert.True(WerGetFlags(GetCurrentProcess(), out uint currentWerFlags) >= 0);
            Assert.Equal(WerFaultReportingNoUi, currentWerFlags & WerFaultReportingNoUi);
            Assert.Equal(0u, currentWerFlags & WerFaultReportingAlwaysShowUi);
        }
        finally
        {
            _ = SetErrorMode(originalErrorMode);
            _ = WerSetFlags(originalWerFlags);
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
