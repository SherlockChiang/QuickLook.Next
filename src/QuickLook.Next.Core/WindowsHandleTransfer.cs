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

    public static uint VerifyNamedPipeClientProcess(SafePipeHandle pipe, int expectedProcessId)
    {
        if (expectedProcessId <= 0
            || !GetNamedPipeClientProcessId(pipe, out uint clientProcessId)
            || clientProcessId != (uint)expectedProcessId)
            throw new InvalidOperationException("Named pipe client process did not match the launched broker process.");
        return clientProcessId;
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

    public static (SafeFileHandle Handle, long Length)? TryOpenPinnedReadOnlyFile(string path)
    {
        SafeFileHandle handle = CreateFile(path, GenericRead, FileShareRead, 0, OpenExisting, 0, 0);
        if (handle.IsInvalid)
        {
            int error = Marshal.GetLastWin32Error();
            handle.Dispose();
            if (error is 2 or 3)
                return null;
            throw new Win32Exception(error, "Could not pin the SQLite companion file.");
        }
        if (GetFileType(handle) != FileTypeDisk || !GetFileSizeEx(handle, out long length) || length < 0)
        {
            handle.Dispose();
            throw new InvalidDataException("Could not validate the pinned SQLite companion file.");
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

    public static string GetFileIdentity(SafeFileHandle source, long expectedLength)
    {
        if (!IsExpectedDiskFile(source, expectedLength)
            || !GetFileInformationByHandle(source, out ByHandleFileInformation info))
            throw new InvalidDataException("Could not identify the preview file.");
        ulong fileIndex = ((ulong)info.FileIndexHigh << 32) | info.FileIndexLow;
        ulong modified = ((ulong)info.LastWriteTimeHigh << 32) | info.LastWriteTimeLow;
        return $"{info.VolumeSerialNumber:X8}:{fileIndex:X16}:{expectedLength:X16}:{modified:X16}";
    }

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
        if (!IsExpectedDiskFile(handle, expectedLength))
        {
            handle.Dispose();
            throw new InvalidDataException("Preview input was not the expected disk file.");
        }
        return handle;
    }

    public static OwnedSqliteFileHandles TakeLocalSqliteFileHandles(
        long mainValue,
        long mainLength,
        long walValue,
        long walLength,
        long shmValue,
        long shmLength)
    {
        var adopted = new Dictionary<nint, SafeFileHandle>();
        bool duplicate = false;
        bool invalidRawValue = false;

        SafeFileHandle? Adopt(long value)
        {
            if (value == 0)
                return null;

            nint raw;
            try
            {
                raw = checked((nint)value);
            }
            catch (OverflowException)
            {
                invalidRawValue = true;
                return null;
            }
            if (adopted.TryGetValue(raw, out SafeFileHandle? existing))
            {
                duplicate = true;
                return existing;
            }

            var handle = new SafeFileHandle(raw, ownsHandle: true);
            adopted.Add(raw, handle);
            return handle;
        }

        try
        {
            // Adopt every distinct raw handle before validating any tuple. This guarantees that a
            // malformed later companion cannot leave an earlier or later host-local handle open.
            SafeFileHandle? main = Adopt(mainValue);
            SafeFileHandle? wal = Adopt(walValue);
            SafeFileHandle? shm = Adopt(shmValue);

            if (invalidRawValue)
                throw new InvalidDataException("SQLite input contained an invalid local handle value.");
            if (duplicate)
                throw new InvalidDataException("SQLite input handles must be distinct.");
            if (main is null || !IsExpectedDiskFile(main, mainLength))
                throw new InvalidDataException("SQLite main handle was not the expected disk file.");
            if (wal is null
                    ? walLength != 0
                    : !IsExpectedDiskFile(wal, walLength))
            {
                throw new InvalidDataException("SQLite WAL handle was not the expected disk file.");
            }
            if (shm is null
                    ? shmLength != 0
                    : !IsExpectedDiskFile(shm, shmLength))
            {
                throw new InvalidDataException("SQLite SHM handle was not the expected disk file.");
            }

            return new OwnedSqliteFileHandles(main, wal, shm);
        }
        catch
        {
            foreach (SafeFileHandle handle in adopted.Values)
                handle.Dispose();
            throw;
        }
    }

    private static bool IsExpectedDiskFile(SafeFileHandle handle, long expectedLength)
        => expectedLength >= 0
            && !handle.IsInvalid
            && !handle.IsClosed
            && GetFileType(handle) == FileTypeDisk
            && GetFileSizeEx(handle, out long length)
            && length == expectedLength;

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

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(SafePipeHandle pipe, out uint clientProcessId);

    [DllImport("kernel32.dll", EntryPoint = "CreateFileW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFile(
        string fileName, uint desiredAccess, uint shareMode, nint securityAttributes,
        uint creationDisposition, uint flagsAndAttributes, nint templateFile);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern uint GetFileType(SafeFileHandle file);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileSizeEx(SafeFileHandle file, out long fileSize);

    [StructLayout(LayoutKind.Sequential)]
    private struct ByHandleFileInformation
    {
        public uint FileAttributes;
        public uint CreationTimeLow;
        public uint CreationTimeHigh;
        public uint LastAccessTimeLow;
        public uint LastAccessTimeHigh;
        public uint LastWriteTimeLow;
        public uint LastWriteTimeHigh;
        public uint VolumeSerialNumber;
        public uint FileSizeHigh;
        public uint FileSizeLow;
        public uint NumberOfLinks;
        public uint FileIndexHigh;
        public uint FileIndexLow;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetFileInformationByHandle(
        SafeFileHandle file,
        out ByHandleFileInformation information);

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

public sealed class OwnedSqliteFileHandles(
    SafeFileHandle main,
    SafeFileHandle? wal,
    SafeFileHandle? shm) : IDisposable
{
    private int _disposed;

    public SafeFileHandle Main { get; } = main;
    public SafeFileHandle? Wal { get; } = wal;
    public SafeFileHandle? Shm { get; } = shm;

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0)
            return;
        Shm?.Dispose();
        Wal?.Dispose();
        Main.Dispose();
    }
}
