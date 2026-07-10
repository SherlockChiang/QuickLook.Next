using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace QuickLook.Next.App;

/// <summary>Owns the resource boundary for one isolated host process tree.</summary>
internal sealed class HostProcessJob : IDisposable
{
    private const uint ActiveProcess = 0x00000008;
    private const uint ProcessMemory = 0x00000100;
    private const uint JobMemory = 0x00000200;
    private const uint KillOnClose = 0x00002000;
    private const int ExtendedLimitInformation = 9;
    private SafeJobHandle? _handle;

    public HostProcessJob(nint memoryLimit)
    {
        _handle = CreateJobObject(nint.Zero, null);
        if (_handle.IsInvalid)
            throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateJobObject failed");

        var limits = new JobObjectExtendedLimitInformation
        {
            BasicLimitInformation = new JobObjectBasicLimitInformation
            {
                LimitFlags = KillOnClose | ActiveProcess | ProcessMemory | JobMemory,
                ActiveProcessLimit = 1,
            },
            ProcessMemoryLimit = memoryLimit,
            JobMemoryLimit = memoryLimit,
        };
        if (!SetInformationJobObject(_handle, ExtendedLimitInformation, ref limits,
                (uint)Marshal.SizeOf<JobObjectExtendedLimitInformation>()))
        {
            int error = Marshal.GetLastWin32Error();
            _handle.Dispose();
            _handle = null;
            throw new Win32Exception(error, "SetInformationJobObject failed");
        }
    }

    public void Assign(Process process)
    {
        SafeJobHandle handle = _handle ?? throw new ObjectDisposedException(nameof(HostProcessJob));
        if (!AssignProcessToJobObject(handle, process.Handle))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "AssignProcessToJobObject failed");
    }

    public void Dispose()
    {
        Interlocked.Exchange(ref _handle, null)?.Dispose();
        GC.SuppressFinalize(this);
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct JobObjectBasicLimitInformation
    {
        public long PerProcessUserTimeLimit;
        public long PerJobUserTimeLimit;
        public uint LimitFlags;
        public nint MinimumWorkingSetSize;
        public nint MaximumWorkingSetSize;
        public uint ActiveProcessLimit;
        public nuint Affinity;
        public uint PriorityClass;
        public uint SchedulingClass;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct IoCounters
    {
        public ulong ReadOperationCount;
        public ulong WriteOperationCount;
        public ulong OtherOperationCount;
        public ulong ReadTransferCount;
        public ulong WriteTransferCount;
        public ulong OtherTransferCount;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct JobObjectExtendedLimitInformation
    {
        public JobObjectBasicLimitInformation BasicLimitInformation;
        public IoCounters IoInfo;
        public nint ProcessMemoryLimit;
        public nint JobMemoryLimit;
        public nint PeakProcessMemoryUsed;
        public nint PeakJobMemoryUsed;
    }

    private sealed class SafeJobHandle : SafeHandleZeroOrMinusOneIsInvalid
    {
        public SafeJobHandle() : base(ownsHandle: true) { }
        protected override bool ReleaseHandle() => CloseHandle(handle);
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeJobHandle CreateJobObject(nint jobAttributes, string? name);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetInformationJobObject(SafeJobHandle job, int informationClass,
        ref JobObjectExtendedLimitInformation information, uint informationLength);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool AssignProcessToJobObject(SafeJobHandle job, nint process);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(nint handle);
}
