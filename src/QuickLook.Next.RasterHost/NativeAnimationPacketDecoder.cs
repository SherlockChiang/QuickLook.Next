using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

internal static class NativeAnimationPacketDecoder
{
    private const string Dll = "quicklook_next_native";
    private const int MaxPacketBytes = 64 * 1024 * 1024 + 12;
    private const long MaxInputBytes = 256L * 1024 * 1024;
    private static readonly SemaphoreSlim DecodeGate = new(1, 1);
    private static CancellationToken _cancellationToken;
    private static readonly NativeCancelCallback CancelCallback = IsCanceled;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    private delegate int NativeAnimationCall(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        byte[] outBuf, nuint outCap, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_gif_frames_sized_cancelable(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        byte[] outBuf, nuint outCap, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_webp_frames_sized_cancelable(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        byte[] outBuf, nuint outCap, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_png_frames_sized_cancelable(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        byte[] outBuf, nuint outCap, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_gif_frames_handle(
        nint sourceHandle, ulong expectedLength, byte[] logicalNameUtf8, nuint logicalNameLen,
        uint targetWidth, uint targetHeight, byte[] outBuf, nuint outCap,
        out nuint outRequired, NativeCancelCallback cancelCallback);

    public static async Task<byte[]?> TryDecodeHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        uint targetWidth,
        uint targetHeight,
        CancellationToken cancellationToken)
    {
        if (sourceLength is < 0 or > MaxInputBytes
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed)
            return null;
        byte[] logicalNameBytes = Encoding.UTF8.GetBytes(Path.GetFileName(logicalName));
        if (logicalNameBytes.Length is 0 or > NativeAbi.MaxLogicalNameUtf8Bytes)
            return null;

        await DecodeGate.WaitAsync(cancellationToken);
        try
        {
            return await Task.Run(
                () => DecodeHandle(
                    sourceHandle,
                    sourceLength,
                    logicalNameBytes,
                    targetWidth,
                    targetHeight,
                    cancellationToken),
                CancellationToken.None);
        }
        finally
        {
            _cancellationToken = CancellationToken.None;
            DecodeGate.Release();
        }
    }

    public static async Task<byte[]?> TryDecodeAsync(
        string path, uint targetWidth, uint targetHeight, CancellationToken cancellationToken)
    {
        string extension = Path.GetExtension(path);
        NativeAnimationCall? call = extension.ToLowerInvariant() switch
        {
            ".gif" => ql_decode_gif_frames_sized_cancelable,
            ".webp" => ql_decode_webp_frames_sized_cancelable,
            ".png" => ql_decode_png_frames_sized_cancelable,
            _ => null,
        };
        if (call is null || !File.Exists(path) || new FileInfo(path).Length > MaxInputBytes)
            return null;

        await DecodeGate.WaitAsync(cancellationToken);
        try
        {
            return await Task.Run(() => Decode(call, path, targetWidth, targetHeight, cancellationToken), CancellationToken.None);
        }
        finally
        {
            _cancellationToken = CancellationToken.None;
            DecodeGate.Release();
        }
    }

    private static byte[]? Decode(
        NativeAnimationCall call, string path, uint targetWidth, uint targetHeight, CancellationToken cancellationToken)
    {
        byte[] pathBytes = Encoding.UTF8.GetBytes(path);
        int capacity = 8 * 1024 * 1024;
        while (capacity <= MaxPacketBytes)
        {
            cancellationToken.ThrowIfCancellationRequested();
            byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
            try
            {
                _cancellationToken = cancellationToken;
                int length = call(pathBytes, (nuint)pathBytes.Length, targetWidth, targetHeight,
                    buffer, (nuint)buffer.Length, CancelCallback);
                cancellationToken.ThrowIfCancellationRequested();
                if (length < 0)
                {
                    int needed = -length;
                    if (needed <= capacity || needed > MaxPacketBytes)
                        return null;
                    capacity = needed;
                    continue;
                }
                if (!IsValidPacket(buffer, length))
                    return null;
                return buffer.AsSpan(0, length).ToArray();
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(buffer);
            }
        }
        return null;
    }

    private static byte[]? DecodeHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        byte[] logicalNameBytes,
        uint targetWidth,
        uint targetHeight,
        CancellationToken cancellationToken)
    {
        bool addRef = false;
        int capacity = 8 * 1024 * 1024;
        try
        {
            sourceHandle.DangerousAddRef(ref addRef);
            while (capacity <= MaxPacketBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
                try
                {
                    _cancellationToken = cancellationToken;
                    int status = ql_decode_gif_frames_handle(
                        sourceHandle.DangerousGetHandle(),
                        checked((ulong)sourceLength),
                        logicalNameBytes,
                        (nuint)logicalNameBytes.Length,
                        targetWidth,
                        targetHeight,
                        buffer,
                        (nuint)capacity,
                        out nuint required,
                        CancelCallback);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (status == NativeAbi.StatusOk
                        && required <= (nuint)capacity
                        && IsValidPacket(buffer, checked((int)required)))
                        return buffer.AsSpan(0, checked((int)required)).ToArray();
                    if (status != NativeAbi.StatusBufferTooSmall
                        || required <= (nuint)capacity
                        || required > (nuint)MaxPacketBytes)
                        return null;
                    capacity = checked((int)required);
                }
                finally
                {
                    ArrayPool<byte>.Shared.Return(buffer);
                }
            }
        }
        catch (ObjectDisposedException)
        {
            return null;
        }
        finally
        {
            if (addRef) sourceHandle.DangerousRelease();
        }
        return null;
    }

    private static bool IsValidPacket(byte[] packet, int length)
    {
        if (length <= 12 || length > MaxPacketBytes)
            return false;
        try
        {
            int count = checked((int)BitConverter.ToUInt32(packet, 0));
            int width = checked((int)BitConverter.ToUInt32(packet, 4));
            int height = checked((int)BitConverter.ToUInt32(packet, 8));
            int frameBytes = checked(width * height * 4);
            return count is > 0 and <= 120
                && width is > 0 and <= 1024
                && height is > 0 and <= 1024
                && checked(12 + count * checked(4 + frameBytes)) == length;
        }
        catch (OverflowException)
        {
            return false;
        }
    }

    private static bool IsCanceled() => _cancellationToken.IsCancellationRequested;
}
