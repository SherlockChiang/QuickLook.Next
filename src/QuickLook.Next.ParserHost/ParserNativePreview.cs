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
    // Native Office heroes cap their longest edge at 768px; package icons cap at 512px.
    // Cover the largest current Hero packet so archive/image decode normally remains single-pass.
    private const int InitialRasterSectionBytes = 8 + (768 * 768 * 4);

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

    private delegate int NativeHandleRasterCall(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        nint outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern uint ql_abi_version();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong ql_capabilities();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_archive(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_archive_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

    public static void EnsureCompatible()
    {
        NativeAbi.EnsureCompatible(ql_abi_version());
        NativeAbi.EnsureCapabilities(ql_capabilities(), NativeAbi.ParserHandleInputs);
    }

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_office(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_office_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_preview_package_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

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
    private static extern int ql_preview_mail_handle(
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
    private static extern int ql_preview_ebook_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

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
    private static extern int ql_extract_archive_entry_to_output_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] entryPathUtf8,
        nuint entryPathLen,
        nint outputHandle,
        ulong outputCapacity,
        out ulong outWritten,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_package_icon_cancelable(
        byte[] pathUtf8,
        nuint pathLen,
        nint outBuf,
        nuint outCap,
        NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_package_icon_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        nint outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_office_image_cancelable(
        byte[] pathUtf8,
        nuint pathLen,
        nint outBuf,
        nuint outCap,
        NativeCancelCallback? cancelCb);
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_office_image_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        nint outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_extract_office_layout_image_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        byte[] imageRefUtf8,
        nuint imageRefLen,
        uint targetWidth,
        uint targetHeight,
        nint outBuf,
        nuint outCap,
        out nuint outRequired,
        NativeCancelCallback? cancelCb);

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
        bool isDatabase = kind.Equals("database", StringComparison.OrdinalIgnoreCase);
        bool isMail = kind.Equals("mail", StringComparison.OrdinalIgnoreCase);
        byte[]? infoKindBytes = isDatabase || isMail
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
                    int length = isDatabase
                        ? ql_preview_database_cancelable(
                            pathBytes,
                            (nuint)pathBytes.Length,
                            probe.Size,
                            probe.ModifiedUnix,
                            buffer,
                            (nuint)capacity,
                            cancel)
                        : isMail
                            ? ql_preview_info(
                                pathBytes,
                                (nuint)pathBytes.Length,
                                infoKindBytes!,
                                (nuint)infoKindBytes!.Length,
                                probe.Size,
                                probe.ModifiedUnix,
                                buffer,
                                (nuint)capacity)
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
            "archive" => ql_preview_archive_handle,
            "office" => ql_preview_office_handle,
            "package" => ql_preview_package_handle,
            "ebook" => ql_preview_ebook_handle,
            "mail" => ql_preview_mail_handle,
            _ => null,
        };
        long maxSourceLength = kind.Equals("archive", StringComparison.OrdinalIgnoreCase)
            ? NativeAbi.MaxArchiveHandleInputBytes
            : NativeAbi.MaxParserHandleInputBytes;
        if (handleCall is null
            || sourceLength < 0
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed)
            return (NativeAbi.StatusInvalidArgument, null);
        if (sourceLength > maxSourceLength)
            return (NativeAbi.StatusLimitExceeded, null);

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
            || kind.Equals("torrent", StringComparison.OrdinalIgnoreCase)
            || kind.Equals("archive", StringComparison.OrdinalIgnoreCase)
            || kind.Equals("office", StringComparison.OrdinalIgnoreCase)
            || kind.Equals("package", StringComparison.OrdinalIgnoreCase)
            || kind.Equals("ebook", StringComparison.OrdinalIgnoreCase)
            || kind.Equals("mail", StringComparison.OrdinalIgnoreCase);

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

    public static (int Status, long Written) TryExtractArchiveEntryToOutputHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        string entryPath,
        SafeFileHandle outputHandle,
        long outputCapacity,
        CancellationToken cancellationToken)
    {
        if (sourceLength < 0
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed
            || outputHandle.IsInvalid
            || outputHandle.IsClosed
            || outputCapacity is <= 0 or > NativeAbi.MaxArchiveEntryOutputBytes
            || string.IsNullOrWhiteSpace(entryPath))
        {
            return (NativeAbi.StatusInvalidArgument, 0);
        }

        logicalName = Path.GetFileName(logicalName);
        if (string.IsNullOrEmpty(logicalName))
            return (NativeAbi.StatusInvalidArgument, 0);
        byte[] logicalNameBytes = Encoding.UTF8.GetBytes(logicalName);
        if (logicalNameBytes.Length > NativeAbi.MaxLogicalNameUtf8Bytes)
            return (NativeAbi.StatusInvalidArgument, 0);

        byte[] entryPathBytes = Encoding.UTF8.GetBytes(entryPath);
        NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
        bool sourceAddRef = false;
        bool outputAddRef = false;
        try
        {
            cancellationToken.ThrowIfCancellationRequested();
            sourceHandle.DangerousAddRef(ref sourceAddRef);
            outputHandle.DangerousAddRef(ref outputAddRef);
            int status = ql_extract_archive_entry_to_output_handle(
                sourceHandle.DangerousGetHandle(),
                checked((ulong)sourceLength),
                logicalNameBytes,
                (nuint)logicalNameBytes.Length,
                entryPathBytes,
                (nuint)entryPathBytes.Length,
                outputHandle.DangerousGetHandle(),
                checked((ulong)outputCapacity),
                out ulong written,
                cancel);
            if (status != NativeAbi.StatusOk)
                return (status, 0);
            if (written > (ulong)outputCapacity)
                return (NativeAbi.StatusInternal, 0);
            return (status, checked((long)written));
        }
        catch (OperationCanceledException) { throw; }
        catch (ObjectDisposedException)
        {
            return (NativeAbi.StatusInvalidHandle, 0);
        }
        catch (OverflowException)
        {
            return (NativeAbi.StatusLengthMismatch, 0);
        }
        finally
        {
            if (outputAddRef) outputHandle.DangerousRelease();
            if (sourceAddRef) sourceHandle.DangerousRelease();
            GC.KeepAlive(cancel);
        }
    }

    public static NativeRasterSection? TryExtractHeroRaster(
        string kind,
        string path,
        CancellationToken cancellationToken)
    {
        NativeCancelableRasterSectionCall? call = kind.Equals("package", StringComparison.OrdinalIgnoreCase)
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
            int capacity = InitialRasterSectionBytes;
            while (capacity <= MaxRasterBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                SharedSectionOwner? section = SharedSectionOwner.Create(capacity);
                try
                {
                    using SharedSectionView view = section.MapWritable();
                    int length = call(
                        pathBytes,
                        (nuint)pathBytes.Length,
                        view.Pointer,
                        (nuint)capacity,
                        cancel);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (length < 0)
                    {
                        int required = -length;
                        if (required <= capacity || required > MaxRasterBytes)
                            return null;
                        capacity = required;
                        continue;
                    }
                    if (!TryReadRasterMetadata(
                        view.Bytes,
                        length,
                        out int width,
                        out int height))
                    {
                        return null;
                    }

                    var raster = new NativeRasterSection(section, length, width, height);
                    section = null;
                    return raster;
                }
                finally
                {
                    section?.Dispose();
                    GC.KeepAlive(cancel);
                }
            }
        }
        catch (OperationCanceledException) { throw; }
        catch { return null; }

        return null;
    }

    public static (int Status, NativeRasterSection? Raster) TryExtractOfficeHeroRasterHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken)
        => TryExtractHeroRasterHandle(
            ql_extract_office_image_handle,
            sourceHandle,
            sourceLength,
            logicalName,
            cancellationToken);

    public static (int Status, NativeRasterSection? Raster) TryExtractPackageHeroRasterHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken)
        => TryExtractHeroRasterHandle(
            ql_extract_package_icon_handle,
            sourceHandle,
            sourceLength,
            logicalName,
            cancellationToken);

    public static (int Status, NativeRasterSection? Raster) TryExtractOfficeLayoutImageHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        string imageRef,
        int targetWidth,
        int targetHeight,
        CancellationToken cancellationToken)
    {
        if (sourceLength < 0 || sourceHandle.IsInvalid || sourceHandle.IsClosed
            || targetWidth is <= 0 or > NativeAbi.MaxOfficeImageDimension
            || targetHeight is <= 0 or > NativeAbi.MaxOfficeImageDimension)
        {
            return (NativeAbi.StatusInvalidArgument, null);
        }

        logicalName = Path.GetFileName(logicalName);
        if (string.IsNullOrEmpty(logicalName) || !IsCanonicalOfficeImageRef(imageRef))
            return (NativeAbi.StatusInvalidArgument, null);

        byte[] logicalNameBytes = Encoding.UTF8.GetBytes(logicalName);
        byte[] imageRefBytes = Encoding.UTF8.GetBytes(imageRef);
        if (logicalNameBytes.Length > NativeAbi.MaxLogicalNameUtf8Bytes
            || imageRefBytes.Length > NativeAbi.MaxOfficeImageRefUtf8Bytes)
        {
            return (NativeAbi.StatusInvalidArgument, null);
        }

        NativeCancelCallback cancel = () => cancellationToken.IsCancellationRequested;
        bool addRef = false;
        try
        {
            sourceHandle.DangerousAddRef(ref addRef);
            int capacity = checked(8 + checked(targetWidth * targetHeight * 4));
            cancellationToken.ThrowIfCancellationRequested();
            SharedSectionOwner? section = SharedSectionOwner.Create(capacity);
            try
            {
                using SharedSectionView view = section.MapWritable();
                int status = ql_extract_office_layout_image_handle(
                    sourceHandle.DangerousGetHandle(),
                    checked((ulong)sourceLength),
                    logicalNameBytes,
                    (nuint)logicalNameBytes.Length,
                    imageRefBytes,
                    (nuint)imageRefBytes.Length,
                    checked((uint)targetWidth),
                    checked((uint)targetHeight),
                    view.Pointer,
                    (nuint)capacity,
                    out nuint required,
                    cancel);
                cancellationToken.ThrowIfCancellationRequested();
                if (status != NativeAbi.StatusOk)
                    return (status, null);
                if (required > (nuint)capacity
                    || required > int.MaxValue
                    || !TryReadRasterMetadata(
                        view.Bytes,
                        checked((int)required),
                        out int width,
                        out int height)
                    || width > targetWidth
                    || height > targetHeight
                    || width > NativeAbi.MaxOfficeImageDimension
                    || height > NativeAbi.MaxOfficeImageDimension)
                {
                    return (NativeAbi.StatusInternal, null);
                }

                var raster = new NativeRasterSection(
                    section,
                    checked((int)required),
                    width,
                    height);
                section = null;
                return (NativeAbi.StatusOk, raster);
            }
            finally
            {
                section?.Dispose();
            }
        }
        catch (OperationCanceledException) { throw; }
        catch (ObjectDisposedException)
        {
            return (NativeAbi.StatusInvalidHandle, null);
        }
        catch (OverflowException)
        {
            return (NativeAbi.StatusLengthMismatch, null);
        }
        catch
        {
            return (NativeAbi.StatusInternal, null);
        }
        finally
        {
            if (addRef) sourceHandle.DangerousRelease();
            GC.KeepAlive(cancel);
        }
    }

    private static (int Status, NativeRasterSection? Raster) TryExtractHeroRasterHandle(
        NativeHandleRasterCall handleCall,
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken)
    {
        if (sourceLength < 0 || sourceHandle.IsInvalid || sourceHandle.IsClosed)
            return (NativeAbi.StatusInvalidArgument, null);

        logicalName = Path.GetFileName(logicalName);
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
            int capacity = InitialRasterSectionBytes;
            while (capacity <= MaxRasterBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                SharedSectionOwner? section = SharedSectionOwner.Create(capacity);
                try
                {
                    using SharedSectionView view = section.MapWritable();
                    int status = handleCall(
                        sourceHandle.DangerousGetHandle(),
                        checked((ulong)sourceLength),
                        logicalNameBytes,
                        (nuint)logicalNameBytes.Length,
                        view.Pointer,
                        (nuint)capacity,
                        out nuint required,
                        cancel);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (status == NativeAbi.StatusOk
                        && required <= (nuint)capacity
                        && required <= int.MaxValue
                        && TryReadRasterMetadata(
                            view.Bytes,
                            checked((int)required),
                            out int width,
                            out int height))
                    {
                        var raster = new NativeRasterSection(
                            section,
                            checked((int)required),
                            width,
                            height);
                        section = null;
                        return (status, raster);
                    }
                    if (status == NativeAbi.StatusOk)
                        return (NativeAbi.StatusInternal, null);
                    if (status != NativeAbi.StatusBufferTooSmall)
                        return (status, null);
                    if (required <= (nuint)capacity || required > (nuint)MaxRasterBytes)
                        return (NativeAbi.StatusInternal, null);
                    capacity = checked((int)required);
                }
                finally
                {
                    section?.Dispose();
                }
            }
        }
        catch (ObjectDisposedException)
        {
            return (NativeAbi.StatusInvalidHandle, null);
        }
        catch (OverflowException)
        {
            return (NativeAbi.StatusLengthMismatch, null);
        }
        finally
        {
            if (addRef) sourceHandle.DangerousRelease();
            GC.KeepAlive(cancel);
        }

        return (NativeAbi.StatusBufferTooSmall, null);
    }

    internal static bool IsValidRaster(ReadOnlySpan<byte> raster, int length)
        => TryReadRasterMetadata(raster, length, out _, out _);

    private static bool TryReadRasterMetadata(
        ReadOnlySpan<byte> raster,
        int length,
        out int width,
        out int height)
    {
        width = 0;
        height = 0;
        if (length <= 8 || length > MaxRasterBytes || raster.Length < length)
            return false;

        try
        {
            width = BitConverter.ToInt32(raster[..4]);
            height = BitConverter.ToInt32(raster.Slice(4, 4));
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

    internal static bool IsCanonicalOfficeImageRef(string? imageRef)
    {
        if (string.IsNullOrWhiteSpace(imageRef)
            || imageRef.Length > NativeAbi.MaxOfficeImageRefUtf8Bytes
            || imageRef[0] == '/'
            || imageRef.Contains('\\')
            || imageRef.Contains(':'))
        {
            return false;
        }

        string[] segments = imageRef.Split('/');
        return segments.Length >= 3
            && segments[0] is "word" or "ppt" or "xl"
            && segments[1] == "media"
            && segments.All(static segment =>
                segment.Length > 0
                && segment is not "." and not "..");
    }

    private delegate int NativePreviewCall(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, NativeCancelCallback? cancelCb);
    private delegate int NativeCancelableRasterSectionCall(
        byte[] pathUtf8,
        nuint pathLen,
        nint outBuf,
        nuint outCap,
        NativeCancelCallback? cancelCb);
}

internal sealed class NativeRasterSection(
    SharedSectionOwner section,
    int packetLength,
    int width,
    int height) : IDisposable
{
    public SharedSectionOwner Section { get; } = section;
    public int PacketLength { get; } = packetLength;
    public int Width { get; } = width;
    public int Height { get; } = height;

    public void Dispose() => Section.Dispose();
}
