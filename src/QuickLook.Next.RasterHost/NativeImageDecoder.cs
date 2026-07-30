using System.Buffers;
using System.Buffers.Binary;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

internal sealed record NativeDecodedImage(
    byte[] Bgra,
    int Width,
    int Height,
    int OriginalWidth,
    int OriginalHeight)
{
    public int DecodeMilliseconds { get; init; }
    public int ResizeMilliseconds { get; init; }
    public int ConvertMilliseconds { get; init; }
    public ImageWaveform? Waveform { get; init; }
}

internal static class NativeImageDecoder
{
    private const string Dll = "quicklook_next_native";
    private const int HeaderBytes = 28;
    private const int WaveformHeaderBytes = 40;
    private const int WaveformChannelCount = 3;
    private const int WaveformDensityBytes =
        ImageWaveformBuilder.ScopeWidth * ImageWaveformBuilder.ScopeHeight * WaveformChannelCount;
    private const int MaxPreviewRasterDimension = 2048;
    private const int MaxSystemFailureNativeFallbackDimension = 1600;
    private const int MaxDecodedImageBytes = HeaderBytes + (MaxPreviewRasterDimension * MaxPreviewRasterDimension * 4);
    private const int MaxDecodedImageWithWaveformBytes =
        WaveformHeaderBytes
        + (MaxPreviewRasterDimension * MaxPreviewRasterDimension * 4)
        + WaveformDensityBytes;
    private const long MaxInputImageBytes = 256L * 1024 * 1024;
    private const long MaxNativeFallbackAfterSystemFailureBytes = 16L * 1024 * 1024;
    private static readonly SemaphoreSlim DecodeGate = new(1, 1);
    private static readonly NativeCancelCallback DecodeCancelCallback = IsDecodeCanceled;
    private static readonly IntPtr DecodeCancelCallbackPtr = Marshal.GetFunctionPointerForDelegate(DecodeCancelCallback);
    private static CancellationToken _decodeCancellationToken;
    private static ulong _capabilities;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern uint ql_abi_version();
    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong ql_capabilities();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_image(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);

    public static void EnsureCompatible()
    {
        NativeAbi.EnsureCompatible(ql_abi_version());
        ulong capabilities = ql_capabilities();
        // Image metadata is an optional ABI 3 sidecar. A host with the original ABI 3 raster
        // capabilities must still start and serve the first surface without it.
        NativeAbi.EnsureCapabilities(
            capabilities,
            NativeAbi.RasterHandleInputs & ~NativeAbi.HandleImageMetadata);
        _capabilities = capabilities;
    }

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_image_cancelable(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap, IntPtr cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_image_sized_cancelable(
        byte[] pathUtf8,
        nuint pathLen,
        uint targetWidth,
        uint targetHeight,
        byte[] outBuf,
        nuint outCap,
        IntPtr cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_image_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        uint targetWidth,
        uint targetHeight,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        IntPtr cancelCb);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_image_with_waveform_handle(
        nint sourceHandle,
        ulong expectedLength,
        byte[] logicalNameUtf8,
        nuint logicalNameLen,
        uint targetWidth,
        uint targetHeight,
        byte[] outBuf,
        nuint outCap,
        out nuint outRequired,
        IntPtr cancelCb);

    public static bool UsesHandleInput(string logicalPath, QuickLook.Next.Contracts.FileProbe probe)
        => probe.Kind.Equals("image", StringComparison.OrdinalIgnoreCase)
            && string.Equals(probe.Path, logicalPath, StringComparison.OrdinalIgnoreCase)
            && !string.IsNullOrWhiteSpace(probe.Extension)
            && Path.GetExtension(logicalPath).Equals(probe.Extension, StringComparison.OrdinalIgnoreCase);

    internal static bool SupportsGeneralHandleAnimation
        => (_capabilities & NativeAbi.HandleAnimation) != 0;

    internal static bool SupportsHandleImageWaveform
        => (_capabilities & NativeAbi.HandleImageWaveform) != 0;

    internal static bool SupportsHandleImageMetadata
        => (_capabilities & NativeAbi.HandleImageMetadata) != 0;

    public static bool SupportsHandleAnimation(string logicalPath, QuickLook.Next.Contracts.FileProbe probe)
    {
        string extension = Path.GetExtension(logicalPath).ToLowerInvariant();
        if (!UsesHandleInput(logicalPath, probe)
            || probe.IsAnimated is false)
            return false;

        return extension switch
        {
            ".gif" => true,
            ".webp" or ".png" => SupportsGeneralHandleAnimation,
            _ => false,
        };
    }

    public static bool RequiresSystemDecoderHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName)
    {
        string ext = Path.GetExtension(logicalName).ToLowerInvariant();
        if (ext is ".avif" or ".heic" or ".heif" or ".jxl")
            return true;
        if (ext is not (".jpg" or ".jpeg" or ".jpe"))
            return false;

        try
        {
            using SafeFileHandle probeHandle = WindowsHandleTransfer.ReopenReadOnlyFile(sourceHandle, sourceLength);
            using var stream = new FileStream(probeHandle, FileAccess.Read);
            return JpegRequiresSystemDecoder(stream);
        }
        catch { return false; }
    }

    public static bool SkipNativeHandleFallbackAfterSystemFailure(long sourceLength, string logicalName)
        => Path.GetExtension(logicalName).ToLowerInvariant() is ".png" or ".bmp" or ".webp"
            && sourceLength > MaxNativeFallbackAfterSystemFailureBytes;

    private static bool IsSvgMagic(byte[] magicPrefix)
    {
        ReadOnlySpan<byte> prefix = magicPrefix;
        if (prefix.StartsWith(new byte[] { 0xEF, 0xBB, 0xBF }))
            prefix = prefix[3..];
        while (!prefix.IsEmpty && prefix[0] is (byte)' ' or (byte)'\t' or (byte)'\r' or (byte)'\n')
            prefix = prefix[1..];
        return prefix.StartsWith("<svg"u8)
            || (prefix.StartsWith("<?xml"u8) && prefix.IndexOf("<svg"u8) >= 0);
    }

    public static async Task<NativeDecodedImage?> TryDecodeHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalPath,
        TimeSpan timeout,
        CancellationToken cancellationToken,
        uint targetWidth,
        uint targetHeight)
    {
        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);
        await DecodeGate.WaitAsync(timeoutCts.Token);
        try
        {
            return await Task.Run(
                () => TryDecodeHandle(
                    sourceHandle,
                    sourceLength,
                    logicalPath,
                    timeoutCts.Token,
                    targetWidth,
                    targetHeight),
                CancellationToken.None);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return null;
        }
        finally
        {
            DecodeGate.Release();
        }
    }

    private static NativeDecodedImage? TryDecodeHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalPath,
        CancellationToken cancellationToken,
        uint targetWidth,
        uint targetHeight)
    {
        if (sourceLength is < 0 or > MaxInputImageBytes
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed)
            return null;
        string logicalName = Path.GetFileName(logicalPath);
        byte[] logicalNameBytes = Encoding.UTF8.GetBytes(logicalName);
        if (logicalNameBytes.Length is 0 or > NativeAbi.MaxLogicalNameUtf8Bytes)
            return null;

        bool includeWaveform = SupportsHandleImageWaveform;
        int maximumPacketBytes = includeWaveform
            ? MaxDecodedImageWithWaveformBytes
            : MaxDecodedImageBytes;
        bool addRef = false;
        int capacity = 8 * 1024 * 1024;
        try
        {
            sourceHandle.DangerousAddRef(ref addRef);
            while (capacity <= maximumPacketBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(capacity);
                try
                {
                    _decodeCancellationToken = cancellationToken;
                    int status;
                    nuint required;
                    if (includeWaveform)
                    {
                        status = ql_decode_image_with_waveform_handle(
                            sourceHandle.DangerousGetHandle(),
                            checked((ulong)sourceLength),
                            logicalNameBytes,
                            (nuint)logicalNameBytes.Length,
                            targetWidth,
                            targetHeight,
                            buffer,
                            (nuint)capacity,
                            out required,
                            DecodeCancelCallbackPtr);
                    }
                    else
                    {
                        status = ql_decode_image_handle(
                            sourceHandle.DangerousGetHandle(),
                            checked((ulong)sourceLength),
                            logicalNameBytes,
                            (nuint)logicalNameBytes.Length,
                            targetWidth,
                            targetHeight,
                            buffer,
                            (nuint)capacity,
                            out required,
                            DecodeCancelCallbackPtr);
                    }
                    if (status == NativeAbi.StatusOk
                        && required <= (nuint)capacity)
                    {
                        return includeWaveform
                            ? ParseDecodedImageWithWaveform(buffer, checked((int)required))
                            : ParseDecodedImage(buffer, checked((int)required));
                    }
                    if (status != NativeAbi.StatusBufferTooSmall
                        || required <= (nuint)capacity
                        || required > (nuint)maximumPacketBytes)
                        return null;
                    capacity = checked((int)required);
                }
                finally
                {
                    _decodeCancellationToken = CancellationToken.None;
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

    public static async Task<NativeDecodedImage?> TryDecodeAsync(
        string path,
        TimeSpan timeout,
        CancellationToken cancellationToken,
        uint targetWidth = 0,
        uint targetHeight = 0,
        bool systemDecodeAlreadyFailed = false)
    {
        if (IsTooLarge(path))
            return null;

        cancellationToken.ThrowIfCancellationRequested();

        bool systemPreferred = ShouldPreferSystemDecoder(path);
        if (systemPreferred && !systemDecodeAlreadyFailed)
        {
            NativeDecodedImage? systemImage = await SystemImageDecoder.TryDecodeAsync(path, cancellationToken, targetWidth, targetHeight);
            if (systemImage is not null)
                return systemImage;
            DiagLog.Write("RasterHost", $"system image preferred decode failed; falling back to native path={path}");
            if (ShouldRequireSystemDecoder(path))
            {
                DiagLog.Write("RasterHost", $"native fallback skipped for system-required image path={path}");
                return null;
            }
            if (ShouldSkipNativeFallbackAfterSystemFailure(path))
            {
                DiagLog.Write("RasterHost", $"native fallback skipped after system decode failure; path={path}");
                return null;
            }
            targetWidth = BoundSystemFailureFallbackTarget(targetWidth);
            targetHeight = BoundSystemFailureFallbackTarget(targetHeight);
        }
        else if (systemPreferred)
        {
            if (ShouldRequireSystemDecoder(path) || ShouldSkipNativeFallbackAfterSystemFailure(path))
                return null;
            targetWidth = BoundSystemFailureFallbackTarget(targetWidth);
            targetHeight = BoundSystemFailureFallbackTarget(targetHeight);
        }

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);
        NativeDecodedImage? nativeImage;
        try
        {
            nativeImage = await DecodeOnGateAsync(path, timeoutCts.Token, targetWidth, targetHeight);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            nativeImage = null;
        }

        cancellationToken.ThrowIfCancellationRequested();
        return nativeImage ?? (systemDecodeAlreadyFailed
            ? null
            : await SystemImageDecoder.TryDecodeAsync(path, cancellationToken, targetWidth, targetHeight));
    }

    private static async Task<NativeDecodedImage?> DecodeOnGateAsync(
        string path,
        CancellationToken cancellationToken,
        uint targetWidth,
        uint targetHeight)
    {
        await DecodeGate.WaitAsync(cancellationToken);
        try
        {
            if (cancellationToken.IsCancellationRequested)
                return null;

            return await Task.Run(() =>
            {
                if (cancellationToken.IsCancellationRequested)
                    return null;
                return TryDecode(path, cancellationToken, targetWidth, targetHeight);
            }, CancellationToken.None);
        }
        finally
        {
            DecodeGate.Release();
        }
    }

    public static NativeDecodedImage? TryDecode(string path)
        => TryDecode(path, CancellationToken.None);

    public static NativeDecodedImage? TryDecode(
        string path,
        CancellationToken cancellationToken,
        uint targetWidth = 0,
        uint targetHeight = 0)
    {
        try
        {
            if (IsTooLarge(path))
                return null;

            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            int cap = 8 * 1024 * 1024;
            while (cap <= MaxDecodedImageBytes)
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] buffer = ArrayPool<byte>.Shared.Rent(cap);
                try
                {
                    _decodeCancellationToken = cancellationToken;
                    int n = ql_decode_image_sized_cancelable(
                        pathBytes,
                        (nuint)pathBytes.Length,
                        targetWidth,
                        targetHeight,
                        buffer,
                        (nuint)buffer.Length,
                        DecodeCancelCallbackPtr);
                    _decodeCancellationToken = CancellationToken.None;
                    if (n > HeaderBytes)
                    {
                        return ParseDecodedImage(buffer, n);
                    }

                    if (n < 0)
                    {
                        int needed = -n;
                        if (needed <= cap || needed > MaxDecodedImageBytes)
                            return null;
                        cap = needed;
                        continue;
                    }

                    return null;
                }
                finally
                {
                    _decodeCancellationToken = CancellationToken.None;
                    ArrayPool<byte>.Shared.Return(buffer);
                }
            }
        }
        catch
        {
            return null;
        }

        return null;
    }

    private static bool IsDecodeCanceled()
        => _decodeCancellationToken.IsCancellationRequested;

    private static NativeDecodedImage? ParseDecodedImage(byte[] buffer, int length)
        => ParseDecodedImagePacket(buffer, length, includesWaveform: false);

    private static NativeDecodedImage? ParseDecodedImageWithWaveform(byte[] buffer, int length)
        => ParseDecodedImagePacket(buffer, length, includesWaveform: true);

    private static NativeDecodedImage? ParseDecodedImagePacket(
        byte[] buffer,
        int length,
        bool includesWaveform)
    {
        int headerBytes = includesWaveform ? WaveformHeaderBytes : HeaderBytes;
        if (length <= headerBytes || length > buffer.Length)
            return null;

        uint widthRaw = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(0, 4));
        uint heightRaw = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(4, 4));
        uint originalWidthRaw = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(8, 4));
        uint originalHeightRaw = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(12, 4));
        uint decodeMsRaw = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(16, 4));
        uint resizeMsRaw = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(20, 4));
        uint convertMsRaw = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(24, 4));
        if (widthRaw is 0 or > MaxPreviewRasterDimension
            || heightRaw is 0 or > MaxPreviewRasterDimension
            || originalWidthRaw is 0 or > int.MaxValue
            || originalHeightRaw is 0 or > int.MaxValue
            || decodeMsRaw > int.MaxValue
            || resizeMsRaw > int.MaxValue
            || convertMsRaw > int.MaxValue)
            return null;

        int waveformDensityBytes = 0;
        if (includesWaveform)
        {
            uint waveformWidth = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(28, 4));
            uint waveformHeight = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(32, 4));
            uint densityLength = BinaryPrimitives.ReadUInt32LittleEndian(buffer.AsSpan(36, 4));
            if (waveformWidth != ImageWaveformBuilder.ScopeWidth
                || waveformHeight != ImageWaveformBuilder.ScopeHeight
                || densityLength != WaveformDensityBytes)
                return null;
            waveformDensityBytes = WaveformDensityBytes;
        }

        int width = (int)widthRaw;
        int height = (int)heightRaw;
        long pixelBytesLong = (long)width * height * 4;
        long expectedLength = headerBytes + pixelBytesLong + waveformDensityBytes;
        if (expectedLength != length)
            return null;

        int pixelBytes = checked((int)pixelBytesLong);
        var bgra = new byte[pixelBytes];
        Buffer.BlockCopy(buffer, headerBytes, bgra, 0, pixelBytes);

        ImageWaveform? waveform = null;
        if (includesWaveform)
        {
            var density = new byte[WaveformDensityBytes];
            Buffer.BlockCopy(buffer, headerBytes + pixelBytes, density, 0, density.Length);
            waveform = new ImageWaveform(
                ImageWaveformBuilder.ScopeWidth,
                ImageWaveformBuilder.ScopeHeight,
                density);
        }

        return new NativeDecodedImage(
            bgra,
            width,
            height,
            (int)originalWidthRaw,
            (int)originalHeightRaw)
        {
            DecodeMilliseconds = (int)decodeMsRaw,
            ResizeMilliseconds = (int)resizeMsRaw,
            ConvertMilliseconds = (int)convertMsRaw,
            Waveform = waveform,
        };
    }

    private static bool IsTooLarge(string path)
    {
        try
        {
            return new FileInfo(path).Length > MaxInputImageBytes;
        }
        catch
        {
            return false;
        }
    }

    private static bool ShouldPreferSystemDecoder(string path)
    {
        string ext = Path.GetExtension(path).ToLowerInvariant();
        return ext is ".png"
            or ".bmp"
            or ".webp"
            or ".jpg" or ".jpeg" or ".jpe"
            or ".tif" or ".tiff"
            or ".heic" or ".heif"
            or ".avif"
            or ".jxl";
    }

    private static bool ShouldRequireSystemDecoder(string path)
    {
        string ext = Path.GetExtension(path).ToLowerInvariant();
        if (ext is ".avif" or ".heic" or ".heif" or ".jxl")
            return true;
        if (ext is ".jpg" or ".jpeg" or ".jpe")
            return JpegRequiresSystemDecoder(path);
        return false;
    }

    private static bool JpegRequiresSystemDecoder(string path)
    {
        try
        {
            using FileStream stream = File.OpenRead(path);
            return JpegRequiresSystemDecoder(stream);
        }
        catch { }

        return false;
    }

    private static bool JpegRequiresSystemDecoder(Stream stream)
    {
        if (stream.ReadByte() != 0xFF || stream.ReadByte() != 0xD8)
            return false;

        while (stream.Position + 4 <= stream.Length)
        {
            if (stream.ReadByte() != 0xFF)
                return false;
            int marker = stream.ReadByte();
            if (marker is 0xDA or 0xD9 || marker < 0)
                return false;

            int hi = stream.ReadByte();
            int lo = stream.ReadByte();
            if (hi < 0 || lo < 0)
                return false;
            int length = (hi << 8) | lo;
            if (length < 2 || stream.Position + length - 2 > stream.Length)
                return false;
            if (marker is 0xEE)
                return true;
            stream.Position += length - 2;
        }
        return false;
    }

    private static bool ShouldSkipNativeFallbackAfterSystemFailure(string path)
    {
        string ext = Path.GetExtension(path).ToLowerInvariant();
        if (ext is not (".png" or ".bmp" or ".webp"))
            return false;

        try { return new FileInfo(path).Length > MaxNativeFallbackAfterSystemFailureBytes; }
        catch { return false; }
    }

    private static uint BoundSystemFailureFallbackTarget(uint target)
    {
        if (target == 0)
            return MaxSystemFailureNativeFallbackDimension;
        return Math.Min(target, MaxSystemFailureNativeFallbackDimension);
    }
}
