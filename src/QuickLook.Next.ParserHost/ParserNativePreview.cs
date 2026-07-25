using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.ParserHost;

internal static class ParserNativePreview
{
    private const string Dll = "quicklook_next_native";
    // Keep native JSON within the control-pipe framing limit before forwarding it to the App.
    private const int MaxPreviewJsonBytes = PipeChannel.MaxControlLineChars;
    internal const int MaxRasterBytes = 16 * 1024 * 1024;
    internal const int MaxRasterDimension = 4096;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    private delegate int NativeHandlePreviewCall(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern uint ql_abi_version();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong ql_capabilities();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_archive(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    public static void EnsureCompatible()
    {
        NativeAbi.EnsureCompatible(ql_abi_version());
        NativeAbi.EnsureCapabilities(ql_capabilities(), NativeAbi.ParserHandleInputs);
    }

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_office(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_text(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_text_cancelable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_text_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_executable_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_torrent_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_sqlite_handles(
        nint mainHandle,
        ulong mainExpectedLength,
        nint walHandle,
        ulong walExpectedLength,
        nint shmHandle,
        ulong shmExpectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_ebook(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_ebook_cancelable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_executable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_executable_cancelable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_torrent(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_torrent_cancelable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_info(
        byte[] pathUtf8,
        nuint pathLen,
        byte[] kindUtf8,
        nuint kindLen,
        long size,
        long modifiedUnix,
        byte[] outBuf,
        nuint outCap);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_database_cancelable(
        byte[] pathUtf8,
        nuint pathLen,
        long size,
        long modifiedUnix,
        byte[] outBuf,
        nuint outCap,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_archive_entry(
        byte[] archivePathUtf8,
        nuint archivePathLen,
        byte[] entryPathUtf8,
        nuint entryPathLen,
        byte[] outBuf,
        nuint outCap);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_archive_entry_cancelable(
        byte[] archivePathUtf8,
        nuint archivePathLen,
        byte[] entryPathUtf8,
        nuint entryPathLen,
        byte[] outBuf,
        nuint outCap,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_package_icon(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_package_icon_cancelable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_office_image(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_office_image_cancelable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);

    public static string? TryPreview(string kind, string path, FileProbe probe, CancellationToken cancellationToken)
    {
        NativePreviewCall? simpleCall = kind.ToLowerInvariant() switch
        {
            "text" => ql_preview_text_cancelable,
            "ebook" => ql_preview_ebook_cancelable,
            "executable" => ql_preview_executable_cancelable,
            "torrent" => ql_preview_torrent_cancelable,
            _ => null,
        };
        NativePreviewCall call = kind.Equals("office", StringComparison.OrdinalIgnoreCase)
            ? ql_preview_office
            : ql_preview_archive;
        byte[] pathBytes = Encoding.UTF8.GetBytes(path);
        byte[]? infoKindBytes = kind.Equals("database", StringComparison.OrdinalIgnoreCase)
            ? Encoding.UTF8.GetBytes(kind)
            : null;
        NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
        try
        {
            int capacity = 64 * 1024;
            while (capacity <= MaxPreviewJsonBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
                try
                {
                    int length = infoKindBytes is not null
                        ? ql_preview_database_cancelable(
                            pathBytes,
                            (nuint)pathBytes.Length,
                            probe.Size,
                            probe.ModifiedUnix,
                            buffer,
                            (nuint)capacity,
                            cancel)
                        : simpleCall is not null
                            ? simpleCall(pathBytes, (nuint)pathBytes.Length, buffer, (nuint)capacity, cancel)
                            : call(pathBytes, (nuint)pathBytes.Length, buffer, (nuint)capacity, cancel);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (length > 0 && length <= capacity)
                        return Encoding.UTF8.GetString(buffer, 0, length);
                    if (length >= 0)
                        return null;

                    int required = -length;
                    if (required <= capacity || required > MaxPreviewJsonBytes)
                        return null;
                    capacity = required;
                }
                finally
                {
                    ArrayPool<byte>.Shared.Return(buffer);
                }
            }
        }
        finally
        {
            GC.KeepAlive(cancel);
        }

        return null;
    }

    public static (int Status, string? Json) TryPreviewHandle(
        string kind,
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalPath,
        CancellationToken cancellationToken)
    {
        NativeHandlePreviewCall? handleCall = kind.ToLowerInvariant() switch
        {
            "text" => ql_preview_text_handle,
            "executable" => ql_preview_executable_handle,
            "torrent" => ql_preview_torrent_handle,
            _ => null,
        };
        if (handleCall is null
            || sourceLength < 0
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed)
            return (NativeAbi.StatusInvalidArgument, null);

        string logicalName = Path.GetFileName(logicalPath);
        if (string.IsNullOrEmpty(logicalName))
            return (NativeAbi.StatusInvalidArgument, null);
        byte[] logicalNameBytes = Encoding.UTF8.GetBytes(logicalName);
        if (logicalNameBytes.Length > NativeAbi.MaxLogicalNameUtf8Bytes)
            return (NativeAbi.StatusInvalidArgument, null);
        NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
        bool addRef = false;
        try
        {
            sourceHandle.DangerousAddRef(ref addRef);
            int capacity = 64 * 1024;
            while (capacity <= MaxPreviewJsonBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
                try
                {
                    int status = handleCall(
                        sourceHandle.DangerousGetHandle(),
                        checked((ulong)sourceLength),
                        logicalNameBytes,
                        (nuint)logicalNameBytes.Length,
                        buffer,
                        (nuint)capacity,
                        out nuint required,
                        cancel);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (status == NativeAbi.StatusOk && required > 0 && required <= (nuint)capacity)
                        return (status, Encoding.UTF8.GetString(buffer, 0, checked((int)required)));
                    if (status == NativeAbi.StatusOk)
                        return (NativeAbi.StatusInternal, null);
                    if (status != NativeAbi.StatusBufferTooSmall)
                        return (status, null);
                    if (required <= (nuint)capacity)
                        return (NativeAbi.StatusInternal, null);
                    if (required > (nuint)MaxPreviewJsonBytes)
                        return (status, null);
                    capacity = checked((int)required);
                }
                finally
                {
                    ArrayPool<byte>.Shared.Return(buffer);
                }
            }
        }
        finally
        {
            if (addRef) sourceHandle.DangerousRelease();
            GC.KeepAlive(cancel);
        }

        return (NativeAbi.StatusBufferTooSmall, null);
    }

    public static (int Status, string? Json) TryPreviewSqliteHandles(
        SafeFileHandle mainHandle,
        long mainLength,
        SafeFileHandle? walHandle,
        long walLength,
        SafeFileHandle? shmHandle,
        long shmLength,
        string logicalPath,
        CancellationToken cancellationToken)
    {
        if (mainLength < 0
            || walLength < 0
            || shmLength < 0
            || mainHandle.IsInvalid
            || mainHandle.IsClosed
            || walHandle is null && walLength != 0
            || walHandle is not null && (walHandle.IsInvalid || walHandle.IsClosed)
            || shmHandle is null && shmLength != 0
            || shmHandle is not null && (shmHandle.IsInvalid || shmHandle.IsClosed))
        {
            return (NativeAbi.StatusInvalidArgument, null);
        }
        if (mainLength > NativeAbi.MaxParserHandleInputBytes
            || walLength > NativeAbi.MaxSqliteWalBytes
            || shmLength > NativeAbi.MaxSqliteShmBytes)
        {
            return (NativeAbi.StatusLimitExceeded, null);
        }

        string logicalName = Path.GetFileName(logicalPath);
        if (string.IsNullOrEmpty(logicalName))
            return (NativeAbi.StatusInvalidArgument, null);
        byte[] logicalNameBytes = Encoding.UTF8.GetBytes(logicalName);
        if (logicalNameBytes.Length > NativeAbi.MaxLogicalNameUtf8Bytes)
            return (NativeAbi.StatusInvalidArgument, null);

        NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
        bool mainAddRef = false;
        bool walAddRef = false;
        bool shmAddRef = false;
        try
        {
            mainHandle.DangerousAddRef(ref mainAddRef);
            walHandle?.DangerousAddRef(ref walAddRef);
            shmHandle?.DangerousAddRef(ref shmAddRef);
            int capacity = 64 * 1024;
            while (capacity <= MaxPreviewJsonBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
                try
                {
                    int status = ql_preview_sqlite_handles(
                        mainHandle.DangerousGetHandle(),
                        checked((ulong)mainLength),
                        walHandle?.DangerousGetHandle() ?? 0,
                        checked((ulong)walLength),
                        shmHandle?.DangerousGetHandle() ?? 0,
                        checked((ulong)shmLength),
                        logicalNameBytes,
                        (nuint)logicalNameBytes.Length,
                        buffer,
                        (nuint)capacity,
                        out nuint required,
                        cancel);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (status == NativeAbi.StatusOk && required > 0 && required <= (nuint)capacity)
                        return (status, Encoding.UTF8.GetString(buffer, 0, checked((int)required)));
                    if (status == NativeAbi.StatusOk)
                        return (NativeAbi.StatusInternal, null);
                    if (status != NativeAbi.StatusBufferTooSmall)
                        return (status, null);
                    if (required <= (nuint)capacity)
                        return (NativeAbi.StatusInternal, null);
                    if (required > (nuint)MaxPreviewJsonBytes)
                        return (status, null);
                    capacity = checked((int)required);
                }
                finally
                {
                    ArrayPool<byte>.Shared.Return(buffer);
                }
            }
        }
        finally
        {
            if (shmAddRef) shmHandle!.DangerousRelease();
            if (walAddRef) walHandle!.DangerousRelease();
            if (mainAddRef) mainHandle.DangerousRelease();
            GC.KeepAlive(cancel);
        }

        return (NativeAbi.StatusBufferTooSmall, null);
    }

    public static bool UsesHandleInput(string kind)
        => kind.Equals("text", StringComparison.OrdinalIgnoreCase)
            || kind.Equals("executable", StringComparison.OrdinalIgnoreCase)
            || kind.Equals("torrent", StringComparison.OrdinalIgnoreCase);

    public static string DescribeHandleFailure(int status)
        => status switch
        {
            NativeAbi.StatusInvalidArgument => "Native handle parser rejected its arguments.",
            NativeAbi.StatusBufferTooSmall => "Native handle parser output exceeded the host limit.",
            NativeAbi.StatusCancelled => "Native handle parser was cancelled.",
            NativeAbi.StatusMalformed => "Native handle parser rejected malformed content.",
            NativeAbi.StatusIo => "Native handle parser could not read the input.",
            NativeAbi.StatusInvalidHandle => "Native handle parser rejected the input handle.",
            NativeAbi.StatusLengthMismatch => "Native handle parser detected an input length mismatch.",
            NativeAbi.StatusInternal => "Native handle parser failed internally.",
            NativeAbi.StatusLimitExceeded => "Native handle parser input exceeded its safety limit.",
            _ => $"Native handle parser returned unknown status {status}.",
        };

    public static string? TryExtractArchiveEntry(string archivePath, string entryPath, CancellationToken cancellationToken)
    {
        const int maxPathBytes = 32 * 1024;
        try
        {
            cancellationToken.ThrowIfCancellationRequested();
            byte[] archiveBytes = Encoding.UTF8.GetBytes(archivePath);
            byte[] entryBytes = Encoding.UTF8.GetBytes(entryPath);
            NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
            byte[] buffer = ArrayPool<byte>.Shared.Rent(maxPathBytes);
            try
            {
                int length = ql_extract_archive_entry_cancelable(
                    archiveBytes, (nuint)archiveBytes.Length,
                    entryBytes, (nuint)entryBytes.Length,
                    buffer, (nuint)maxPathBytes, cancel);
                cancellationToken.ThrowIfCancellationRequested();
                return length > 0 && length <= maxPathBytes
                    ? Encoding.UTF8.GetString(buffer, 0, length)
                    : null;
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(buffer);
                GC.KeepAlive(cancel);
            }
        }
        catch (OperationCanceledException) { throw; }
        catch { return null; }
    }

    public static byte[]? TryExtractHeroRaster(string kind, string path, CancellationToken cancellationToken)
    {
        NativeCancelableRasterCall? call = kind.Equals("package", StringComparison.OrdinalIgnoreCase)
            ? ql_extract_package_icon_cancelable
            : kind.Equals("office", StringComparison.OrdinalIgnoreCase)
            ? ql_extract_office_image_cancelable
            : null;
        if (call is null)
            return null;

        try
        {
            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
            int capacity = 2 * 1024 * 1024;
            while (capacity <= MaxRasterBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
                try
                {
                    int length = call(pathBytes, (nuint)pathBytes.Length, buffer, (nuint)capacity, cancel);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (length < 0)
                    {
                        int required = -length;
                        if (required <= capacity || required > MaxRasterBytes)
                            return null;
                        capacity = required;
                        continue;
                    }
                    if (!IsValidRaster(buffer, length))
                        return null;

                    byte[] raster = new byte[length];
                    Buffer.BlockCopy(buffer, 0, raster, 0, length);
                    return raster;
                }
                finally
                {
                    ArrayPool<byte>.Shared.Return(buffer);
                    GC.KeepAlive(cancel);
                }
            }
        }
        catch (OperationCanceledException) { throw; }
        catch { return null; }

        return null;
    }

    internal static bool IsValidRaster(ReadOnlySpan<byte> raster, int length)
    {
        if (length <= 8 || length > MaxRasterBytes || raster.Length < length)
            return false;

        try
        {
            int width = BitConverter.ToInt32(raster[..4]);
            int height = BitConverter.ToInt32(raster.Slice(4, 4));
            int pixels = checked(width * height * 4);
            return width is > 0 and <= MaxRasterDimension
                && height is > 0 and <= MaxRasterDimension
                && length == 8 + pixels;
        }
        catch (OverflowException)
        {
            return false;
        }
    }

    private delegate int NativePreviewCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
    private delegate int NativeRasterCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);
    private delegate int NativeCancelableRasterCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
}
