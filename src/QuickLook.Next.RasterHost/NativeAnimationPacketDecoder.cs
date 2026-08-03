using System.Buffers.Binary;
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
    private static readonly AnimationOutputCallback OutputCallback = AllocateDirectOutput;
    private static SharedSectionOwner? _directOutputSection;
    private static SharedSectionView? _directOutputView;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nint AnimationOutputCallback(nuint requiredBytes);

    private delegate int NativeAnimationCall(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        nint outBuf, nuint outCap, NativeCancelCallback cancelCallback);

    private delegate int NativeAnimationHandleCall(
        nint sourceHandle, ulong expectedLength, byte[] logicalNameUtf8, nuint logicalNameLen,
        uint targetWidth, uint targetHeight, nint outBuf, nuint outCap,
        out nuint outRequired, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_gif_frames_sized_direct(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        out nuint outRequired, AnimationOutputCallback outputCallback,
        NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_gif_frames_sized_cancelable(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        nint outBuf, nuint outCap, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_webp_frames_sized_cancelable(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        nint outBuf, nuint outCap, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_png_frames_sized_cancelable(
        byte[] pathUtf8, nuint pathLen, uint targetWidth, uint targetHeight,
        nint outBuf, nuint outCap, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_gif_frames_handle_direct(
        nint sourceHandle, ulong expectedLength, byte[] logicalNameUtf8, nuint logicalNameLen,
        uint targetWidth, uint targetHeight, out nuint outRequired,
        AnimationOutputCallback outputCallback, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_gif_frames_handle(
        nint sourceHandle, ulong expectedLength, byte[] logicalNameUtf8, nuint logicalNameLen,
        uint targetWidth, uint targetHeight, nint outBuf, nuint outCap,
        out nuint outRequired, NativeCancelCallback cancelCallback);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_animation_frames_handle(
        nint sourceHandle, ulong expectedLength, byte[] logicalNameUtf8, nuint logicalNameLen,
        uint targetWidth, uint targetHeight, nint outBuf, nuint outCap,
        out nuint outRequired, NativeCancelCallback cancelCallback);

    public static async Task<NativeAnimationPacket?> TryDecodeHandleAsync(
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
        string extension = Path.GetExtension(logicalName).ToLowerInvariant();
        bool useGeneralHandleDecoder = extension is ".webp" or ".png";
        if (extension != ".gif"
            && (!useGeneralHandleDecoder || !NativeImageDecoder.SupportsGeneralHandleAnimation))
            return null;
        byte[] logicalNameBytes = Encoding.UTF8.GetBytes(Path.GetFileName(logicalName));
        if (logicalNameBytes.Length is 0 or > NativeAbi.MaxLogicalNameUtf8Bytes)
            return null;

        await DecodeGate.WaitAsync(cancellationToken);
        try
        {
            return await Task.Run(
                () => extension == ".gif"
                    && NativeImageDecoder.SupportsDirectGifAnimationOutput
                    ? DecodeGifHandleDirect(
                        sourceHandle,
                        sourceLength,
                        logicalNameBytes,
                        targetWidth,
                        targetHeight,
                        cancellationToken)
                    : DecodeHandle(
                        extension == ".gif"
                            ? ql_decode_gif_frames_handle
                            : ql_decode_animation_frames_handle,
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

    public static async Task<NativeAnimationPacket?> TryDecodeAsync(
        string path, uint targetWidth, uint targetHeight, CancellationToken cancellationToken)
    {
        string extension = Path.GetExtension(path);
        string normalizedExtension = extension.ToLowerInvariant();
        NativeAnimationCall? call = normalizedExtension switch
        {
            ".gif" => ql_decode_gif_frames_sized_cancelable,
            ".webp" => ql_decode_webp_frames_sized_cancelable,
            ".png" => ql_decode_png_frames_sized_cancelable,
            _ => null,
        };
        if (call is null
            || !File.Exists(path)
            || new FileInfo(path).Length > MaxInputBytes)
            return null;

        await DecodeGate.WaitAsync(cancellationToken);
        try
        {
            return await Task.Run(
                () => normalizedExtension == ".gif"
                    && NativeImageDecoder.SupportsDirectGifAnimationOutput
                    ? DecodeGifDirect(path, targetWidth, targetHeight, cancellationToken)
                    : Decode(call!, path, targetWidth, targetHeight, cancellationToken),
                CancellationToken.None);
        }
        finally
        {
            _cancellationToken = CancellationToken.None;
            DecodeGate.Release();
        }
    }

    private static NativeAnimationPacket? DecodeGifDirect(
        string path,
        uint targetWidth,
        uint targetHeight,
        CancellationToken cancellationToken)
    {
        byte[] pathBytes = Encoding.UTF8.GetBytes(path);
        ResetDirectOutput();
        try
        {
            _cancellationToken = cancellationToken;
            int status = ql_decode_gif_frames_sized_direct(
                pathBytes,
                (nuint)pathBytes.Length,
                targetWidth,
                targetHeight,
                out nuint required,
                OutputCallback,
                CancelCallback);
            cancellationToken.ThrowIfCancellationRequested();
            return CompleteDirectOutput(status, required);
        }
        finally
        {
            DisposeDirectOutput();
        }
    }

    private static NativeAnimationPacket? DecodeGifHandleDirect(
        SafeFileHandle sourceHandle,
        long sourceLength,
        byte[] logicalNameBytes,
        uint targetWidth,
        uint targetHeight,
        CancellationToken cancellationToken)
    {
        bool addRef = false;
        ResetDirectOutput();
        try
        {
            sourceHandle.DangerousAddRef(ref addRef);
            _cancellationToken = cancellationToken;
            int status = ql_decode_gif_frames_handle_direct(
                sourceHandle.DangerousGetHandle(),
                checked((ulong)sourceLength),
                logicalNameBytes,
                (nuint)logicalNameBytes.Length,
                targetWidth,
                targetHeight,
                out nuint required,
                OutputCallback,
                CancelCallback);
            cancellationToken.ThrowIfCancellationRequested();
            return CompleteDirectOutput(status, required);
        }
        catch (ObjectDisposedException)
        {
            return null;
        }
        finally
        {
            if (addRef)
                sourceHandle.DangerousRelease();
            DisposeDirectOutput();
        }
    }

    private static nint AllocateDirectOutput(nuint requiredBytes)
    {
        if (requiredBytes is <= 12 or > MaxPacketBytes
            || requiredBytes > int.MaxValue
            || _directOutputSection is not null
            || _directOutputView is not null)
            return 0;

        try
        {
            var section = SharedSectionOwner.Create(checked((int)requiredBytes));
            SharedSectionView? view = null;
            try
            {
                view = section.MapWritable();
                _directOutputSection = section;
                _directOutputView = view;
                return view.Pointer;
            }
            catch
            {
                view?.Dispose();
                section.Dispose();
                return 0;
            }
        }
        catch
        {
            return 0;
        }
    }

    private static NativeAnimationPacket? CompleteDirectOutput(int status, nuint required)
    {
        SharedSectionOwner? section = _directOutputSection;
        SharedSectionView? view = _directOutputView;
        if (status != NativeAbi.StatusOk
            || section is null
            || view is null
            || required != (nuint)section.Capacity
            || !TryReadPacketMetadata(
                view.Bytes,
                checked((int)required),
                out int count,
                out int width,
                out int height))
        {
            return null;
        }

        _directOutputSection = null;
        return new NativeAnimationPacket(
            section,
            checked((int)required),
            count,
            width,
            height);
    }

    private static void ResetDirectOutput()
    {
        DisposeDirectOutput();
        _directOutputSection = null;
        _directOutputView = null;
    }

    private static void DisposeDirectOutput()
    {
        _directOutputView?.Dispose();
        _directOutputView = null;
        _directOutputSection?.Dispose();
        _directOutputSection = null;
    }

    private static NativeAnimationPacket? Decode(
        NativeAnimationCall call, string path, uint targetWidth, uint targetHeight, CancellationToken cancellationToken)
    {
        byte[] pathBytes = Encoding.UTF8.GetBytes(path);
        int capacity = 8 * 1024 * 1024;
        while (capacity <= MaxPacketBytes)
        {
            cancellationToken.ThrowIfCancellationRequested();
            SharedSectionOwner? section = SharedSectionOwner.Create(capacity);
            try
            {
                using SharedSectionView view = section.MapWritable();
                _cancellationToken = cancellationToken;
                int length = call(
                    pathBytes,
                    (nuint)pathBytes.Length,
                    targetWidth,
                    targetHeight,
                    view.Pointer,
                    (nuint)capacity,
                    CancelCallback);
                cancellationToken.ThrowIfCancellationRequested();
                if (length < 0)
                {
                    int needed = -length;
                    if (needed <= capacity || needed > MaxPacketBytes)
                        return null;
                    capacity = needed;
                    continue;
                }
                if (!TryReadPacketMetadata(
                    view.Bytes,
                    length,
                    out int count,
                    out int width,
                    out int height))
                {
                    return null;
                }

                var packet = new NativeAnimationPacket(
                    section,
                    length,
                    count,
                    width,
                    height);
                section = null;
                return packet;
            }
            finally
            {
                section?.Dispose();
            }
        }
        return null;
    }

    private static NativeAnimationPacket? DecodeHandle(
        NativeAnimationHandleCall call,
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
                SharedSectionOwner? section = SharedSectionOwner.Create(capacity);
                try
                {
                    using SharedSectionView view = section.MapWritable();
                    _cancellationToken = cancellationToken;
                    int status = call(
                        sourceHandle.DangerousGetHandle(),
                        checked((ulong)sourceLength),
                        logicalNameBytes,
                        (nuint)logicalNameBytes.Length,
                        targetWidth,
                        targetHeight,
                        view.Pointer,
                        (nuint)capacity,
                        out nuint required,
                        CancelCallback);
                    cancellationToken.ThrowIfCancellationRequested();
                    if (status == NativeAbi.StatusOk
                        && required <= (nuint)capacity
                        && required <= int.MaxValue
                        && TryReadPacketMetadata(
                            view.Bytes,
                            checked((int)required),
                            out int count,
                            out int width,
                            out int height))
                    {
                        var packet = new NativeAnimationPacket(
                            section,
                            checked((int)required),
                            count,
                            width,
                            height);
                        section = null;
                        return packet;
                    }
                    if (status != NativeAbi.StatusBufferTooSmall
                        || required <= (nuint)capacity
                        || required > (nuint)MaxPacketBytes)
                        return null;
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
            return null;
        }
        finally
        {
            if (addRef) sourceHandle.DangerousRelease();
        }
        return null;
    }

    private static bool TryReadPacketMetadata(
        ReadOnlySpan<byte> packet,
        int length,
        out int count,
        out int width,
        out int height)
    {
        count = 0;
        width = 0;
        height = 0;
        if (length <= 12 || length > MaxPacketBytes || length > packet.Length)
            return false;
        try
        {
            count = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(packet));
            width = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(packet[4..]));
            height = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(packet[8..]));
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

internal sealed class NativeAnimationPacket(
    SharedSectionOwner section,
    int packetLength,
    int frameCount,
    int width,
    int height) : IDisposable
{
    public SharedSectionOwner Section { get; } = section;
    public int PacketLength { get; } = packetLength;
    public int FrameCount { get; } = frameCount;
    public int Width { get; } = width;
    public int Height { get; } = height;

    public void Dispose() => Section.Dispose();
}
