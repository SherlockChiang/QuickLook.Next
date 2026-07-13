using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace QuickLook.Next.Core;

public static class WindowsHandleTransfer
{
    private const uint ProcessDuplicateHandle = 0x0040;
    private const uint GenericRead = 0x80000000;
    private const uint FileShareRead = 0x00000001;
    private const uint FileShareDelete = 0x00000004;
    private const uint OpenExisting = 3;
    private const uint DuplicateSameAccess = 0x00000002;
    private const uint FileTypeDisk = 0x0001;

    public static SafeProcessHandle OpenAuthenticatedPipeServerProcess(SafePipeHandle pipe, int expectedProcessId)
    {
        uint serverProcessId = VerifyNamedPipeServerProcess(pipe, expectedProcessId);

        SafeProcessHandle process = OpenProcess(ProcessDuplicateHandle, false, serverProcessId);
        if (process.IsInvalid)
        {
            process.Dispose();
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not open the App process for handle transfer.");
        }
        return process;
    }

    public static uint VerifyNamedPipeServerProcess(SafePipeHandle pipe, int expectedProcessId)
    {
        if (expectedProcessId <= 0
            || !GetNamedPipeServerProcessId(pipe, out uint serverProcessId)
            || serverProcessId != (uint)expectedProcessId)
            throw new InvalidOperationException("Named pipe server process did not match the authenticated App process.");
        return serverProcessId;
    }

    public static (SafeFileHandle Handle, long Length) OpenReadOnlyFile(string path)
    {
        SafeFileHandle handle = CreateFile(path, GenericRead, FileShareRead | FileShareDelete, 0, OpenExisting, 0, 0);
        if (handle.IsInvalid)
        {
            handle.Dispose();
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not open the handoff file.");
        }
        if (GetFileType(handle) != FileTypeDisk || !GetFileSizeEx(handle, out long length) || length < 0)
        {
            handle.Dispose();
            throw new InvalidDataException("Could not validate the handoff file.");
        }
        return (handle, length);
    }

    public static SafeFileHandle DuplicateFileFromProcess(SafeProcessHandle sourceProcess, long sourceHandle, long expectedLength)
    {
        nint remoteHandle = checked((nint)sourceHandle);
        if (remoteHandle == 0 || remoteHandle == -1
            || !DuplicateHandle(sourceProcess, remoteHandle, GetCurrentProcess(), out nint duplicate, 0, false, DuplicateSameAccess))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not duplicate the handoff file from the host.");
        var handle = new SafeFileHandle(duplicate, ownsHandle: true);
        if (GetFileType(handle) != FileTypeDisk || !GetFileSizeEx(handle, out long length) || length != expectedLength)
        {
            handle.Dispose();
            throw new InvalidDataException("Host handoff handle was not the expected disk file.");
        }
        return handle;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint serverProcessId);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern SafeProcessHandle OpenProcess(uint desiredAccess, [MarshalAs(UnmanagedType.Bool)] bool inheritHandle, uint processId);

    [DllImport("kernel32.dll", EntryPoint = "CreateFileW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(
        string fileName, uint desiredAccess, uint shareMode, nint securityAttributes,
        uint creationDisposition, uint flagsAndAttributes, nint templateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint GetFileType(SafeFileHandle file);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileSizeEx(SafeFileHandle file, out long fileSize);

    [DllImport("kernel32.dll")]
    private static extern nint GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DuplicateHandle(
        SafeProcessHandle sourceProcess, nint sourceHandle, nint targetProcess,
        out nint targetHandle, uint desiredAccess, [MarshalAs(UnmanagedType.Bool)] bool inheritHandle, uint options);
}
