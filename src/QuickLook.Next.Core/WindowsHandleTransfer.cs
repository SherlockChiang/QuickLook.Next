using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace QuickLook.Next.Core;

public static class WindowsHandleTransfer
{
    private const uint GenericRead = 0x80000000;
    private const uint FileShareRead = 0x00000001;
    private const uint FileShareWrite = 0x00000002;
    private const uint FileShareDelete = 0x00000004;
    private const uint OpenExisting = 3;
    private const uint DuplicateSameAccess = 0x00000002;
    private const uint FileTypeDisk = 0x0001;

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

    public static (SafeFileHandle Handle, long Length) OpenPinnedReadOnlyFile(string path)
    {
        SafeFileHandle handle = CreateFile(path, GenericRead, FileShareRead, 0, OpenExisting, 0, 0);
        if (handle.IsInvalid)
        {
            handle.Dispose();
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not pin the preview file.");
        }
        if (GetFileType(handle) != FileTypeDisk || !GetFileSizeEx(handle, out long length) || length < 0)
        {
            handle.Dispose();
            throw new InvalidDataException("Could not validate the pinned preview file.");
        }
        return (handle, length);
    }

    public static long DuplicateFileToProcess(SafeFileHandle source, SafeProcessHandle targetProcess)
    {
        if (!DuplicateHandle(GetCurrentProcess(), source, targetProcess, out nint duplicate, 0, false, DuplicateSameAccess))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not duplicate the preview file into the host.");
        return duplicate.ToInt64();
    }

    public static SafeFileHandle ReopenTransitionalReadOnlyFile(SafeFileHandle source, long expectedLength)
        => ReopenReadOnlyFile(source, expectedLength, FileShareRead | FileShareWrite | FileShareDelete);

    public static SafeFileHandle ReopenReadOnlyFile(SafeFileHandle source, long expectedLength)
        => ReopenReadOnlyFile(source, expectedLength, FileShareRead | FileShareDelete);

    private static SafeFileHandle ReopenReadOnlyFile(SafeFileHandle source, long expectedLength, uint shareMode)
    {
        SafeFileHandle handle = ReOpenFile(source, GenericRead, shareMode, 0);
        if (handle.IsInvalid)
        {
            handle.Dispose();
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not create the read-only preview anchor.");
        }
        if (GetFileType(handle) != FileTypeDisk || !GetFileSizeEx(handle, out long length) || length != expectedLength)
        {
            handle.Dispose();
            throw new InvalidDataException("Read-only preview anchor was not the expected disk file.");
        }
        return handle;
    }

    public static SafeFileHandle TakeLocalFileHandle(long value, long expectedLength)
    {
        nint raw = checked((nint)value);
        if (raw == 0 || raw == -1)
            throw new InvalidDataException("Received an invalid local file handle.");
        var handle = new SafeFileHandle(raw, ownsHandle: true);
        if (GetFileType(handle) != FileTypeDisk || !GetFileSizeEx(handle, out long length) || length != expectedLength)
        {
            handle.Dispose();
            throw new InvalidDataException("Preview input was not the expected disk file.");
        }
        return handle;
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

    public static nint DuplicateHandleFromProcess(SafeProcessHandle sourceProcess, long sourceHandle)
    {
        nint remoteHandle = checked((nint)sourceHandle);
        if (remoteHandle == 0 || remoteHandle == -1
            || !DuplicateHandle(sourceProcess, remoteHandle, GetCurrentProcess(), out nint duplicate, 0, false, DuplicateSameAccess))
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Could not duplicate the handle from the host.");
        return duplicate;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeServerProcessId(SafePipeHandle pipe, out uint serverProcessId);

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

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool DuplicateHandle(
        nint sourceProcess, SafeFileHandle sourceHandle, SafeProcessHandle targetProcess,
        out nint targetHandle, uint desiredAccess, [MarshalAs(UnmanagedType.Bool)] bool inheritHandle, uint options);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern SafeFileHandle ReOpenFile(
        SafeFileHandle originalFile, uint desiredAccess, uint shareMode, uint flagsAndAttributes);

}
