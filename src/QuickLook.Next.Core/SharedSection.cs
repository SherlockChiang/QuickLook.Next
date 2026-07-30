using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace QuickLook.Next.Core;

/// <summary>
/// Owns an unnamed page-file-backed Windows section used for bounded host-to-App handoffs.
/// The owner keeps the section alive until the receiver acknowledges the transfer.
/// </summary>
public sealed class SharedSectionOwner : IDisposable
{
    private SharedSectionOwner(SafeSectionHandle handle, int capacity)
    {
        Handle = handle;
        Capacity = capacity;
    }

    public SafeSectionHandle Handle { get; }
    public int Capacity { get; }

    public static SharedSectionOwner Create(int capacity)
    {
        if (!OperatingSystem.IsWindows())
            throw new PlatformNotSupportedException("Shared sections require Windows.");
        if (capacity <= 0)
            throw new ArgumentOutOfRangeException(nameof(capacity));

        ulong size = checked((ulong)capacity);
        SafeSectionHandle handle = NativeMethods.CreateFileMapping(
            new nint(-1),
            0,
            NativeMethods.PageReadWrite,
            checked((uint)(size >> 32)),
            checked((uint)size),
            null);
        if (handle.IsInvalid)
        {
            int error = Marshal.GetLastWin32Error();
            handle.Dispose();
            throw new Win32Exception(error, "Could not create the shared section.");
        }

        return new SharedSectionOwner(handle, capacity);
    }

    public SharedSectionView MapWritable()
        => SharedSectionView.Map(Handle, Capacity, writable: true, ownedSection: null);

    public void Dispose() => Handle.Dispose();
}

/// <summary>
/// A mapped view whose span is valid only until this object is disposed.
/// </summary>
public sealed class SharedSectionView : IDisposable
{
    private readonly SafeSectionHandle? _ownedSection;
    private readonly SafeSectionViewHandle _view;
    private readonly bool _writable;
    private int _disposed;

    private SharedSectionView(
        SafeSectionViewHandle view,
        int length,
        bool writable,
        SafeSectionHandle? ownedSection)
    {
        _view = view;
        Length = length;
        _writable = writable;
        _ownedSection = ownedSection;
    }

    public int Length { get; }

    public nint Pointer
    {
        get
        {
            ObjectDisposedException.ThrowIf(Volatile.Read(ref _disposed) != 0, this);
            return _view.DangerousGetHandle();
        }
    }

    public unsafe ReadOnlySpan<byte> Bytes
    {
        get
        {
            ObjectDisposedException.ThrowIf(Volatile.Read(ref _disposed) != 0, this);
            return new ReadOnlySpan<byte>((void*)_view.DangerousGetHandle(), Length);
        }
    }

    public unsafe Span<byte> WritableBytes
    {
        get
        {
            ObjectDisposedException.ThrowIf(Volatile.Read(ref _disposed) != 0, this);
            if (!_writable)
                throw new InvalidOperationException("The shared section view is read-only.");
            return new Span<byte>((void*)_view.DangerousGetHandle(), Length);
        }
    }

    public static SharedSectionView DuplicateAndMapReadOnly(
        SafeProcessHandle sourceProcess,
        long sourceHandle,
        int expectedLength)
    {
        if (!OperatingSystem.IsWindows())
            throw new PlatformNotSupportedException("Shared sections require Windows.");
        ArgumentNullException.ThrowIfNull(sourceProcess);
        if (sourceProcess.IsInvalid || sourceProcess.IsClosed)
            throw new ArgumentException("The source process handle is unavailable.", nameof(sourceProcess));
        if (expectedLength <= 0)
            throw new ArgumentOutOfRangeException(nameof(expectedLength));

        SafeSectionHandle section = DuplicateReadOnlySection(sourceProcess, sourceHandle);
        try
        {
            return Map(section, expectedLength, writable: false, ownedSection: section);
        }
        catch
        {
            section.Dispose();
            throw;
        }
    }

    internal static SafeSectionHandle DuplicateReadOnlySection(
        SafeProcessHandle sourceProcess,
        long sourceHandle)
    {
        if (!OperatingSystem.IsWindows())
            throw new PlatformNotSupportedException("Shared sections require Windows.");
        ArgumentNullException.ThrowIfNull(sourceProcess);
        if (sourceProcess.IsInvalid || sourceProcess.IsClosed)
            throw new ArgumentException("The source process handle is unavailable.", nameof(sourceProcess));

        nint remoteHandle = checked((nint)sourceHandle);
        if (remoteHandle == 0 || remoteHandle == -1
            || !NativeMethods.DuplicateHandle(
                sourceProcess,
                remoteHandle,
                NativeMethods.GetCurrentProcess(),
                out nint duplicate,
                NativeMethods.SectionMapRead,
                false,
                0))
        {
            throw new Win32Exception(
                Marshal.GetLastWin32Error(),
                "Could not duplicate the shared section from the host.");
        }

        return new SafeSectionHandle(duplicate, ownsHandle: true);
    }

    internal static SharedSectionView Map(
        SafeSectionHandle section,
        int length,
        bool writable,
        SafeSectionHandle? ownedSection)
    {
        ArgumentNullException.ThrowIfNull(section);
        if (section.IsInvalid || section.IsClosed)
            throw new ArgumentException("The shared section handle is unavailable.", nameof(section));
        if (length <= 0)
            throw new ArgumentOutOfRangeException(nameof(length));

        SafeSectionViewHandle view = NativeMethods.MapViewOfFile(
            section,
            writable ? NativeMethods.FileMapWrite : NativeMethods.FileMapRead,
            0,
            0,
            checked((nuint)length));
        if (view.IsInvalid)
        {
            int error = Marshal.GetLastWin32Error();
            view.Dispose();
            throw new Win32Exception(error, "Could not map the shared section.");
        }

        return new SharedSectionView(view, length, writable, ownedSection);
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0)
            return;
        _view.Dispose();
        _ownedSection?.Dispose();
    }
}

public sealed class SafeSectionHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    public SafeSectionHandle()
        : base(ownsHandle: true)
    {
    }

    internal SafeSectionHandle(nint handle, bool ownsHandle)
        : base(ownsHandle)
    {
        SetHandle(handle);
    }

    protected override bool ReleaseHandle() => NativeMethods.CloseHandle(handle);
}

internal sealed class SafeSectionViewHandle : SafeHandleZeroOrMinusOneIsInvalid
{
    public SafeSectionViewHandle()
        : base(ownsHandle: true)
    {
    }

    protected override bool ReleaseHandle() => NativeMethods.UnmapViewOfFile(handle);
}

internal static partial class NativeMethods
{
    internal const uint PageReadWrite = 0x00000004;
    internal const uint FileMapWrite = 0x00000002;
    internal const uint FileMapRead = 0x00000004;
    internal const uint SectionMapRead = 0x0004;

    [DllImport("kernel32.dll", EntryPoint = "CreateFileMappingW", CharSet = CharSet.Unicode, SetLastError = true)]
    internal static extern SafeSectionHandle CreateFileMapping(
        nint file,
        nint securityAttributes,
        uint protect,
        uint maximumSizeHigh,
        uint maximumSizeLow,
        string? name);

    [DllImport("kernel32.dll", SetLastError = true)]
    internal static extern SafeSectionViewHandle MapViewOfFile(
        SafeSectionHandle section,
        uint desiredAccess,
        uint fileOffsetHigh,
        uint fileOffsetLow,
        nuint numberOfBytesToMap);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool UnmapViewOfFile(nint baseAddress);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool CloseHandle(nint handle);

    [DllImport("kernel32.dll")]
    internal static extern nint GetCurrentProcess();

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    internal static extern bool DuplicateHandle(
        SafeProcessHandle sourceProcess,
        nint sourceHandle,
        nint targetProcess,
        out nint targetHandle,
        uint desiredAccess,
        [MarshalAs(UnmanagedType.Bool)] bool inheritHandle,
        uint options);
}
