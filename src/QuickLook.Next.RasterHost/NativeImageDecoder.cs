using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;
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
}

internal static class NativeImageDecoder
{
    private const string Dll = "quicklook_next_native";
    private const int HeaderBytes = 28;
    private const int MaxPreviewRasterDimension = 2048;
    private const int MaxDecodedImageBytes = HeaderBytes + (MaxPreviewRasterDimension * MaxPreviewRasterDimension * 4);
    private const long MaxInputImageBytes = 256L * 1024 * 1024;
    private static readonly SemaphoreSlim DecodeGate = new(1, 1);
    private static readonly NativeCancelCallback DecodeCancelCallback = IsDecodeCanceled;
    private static readonly IntPtr DecodeCancelCallbackPtr = Marshal.GetFunctionPointerForDelegate(DecodeCancelCallback);
    private static CancellationToken _decodeCancellationToken;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    private delegate bool NativeCancelCallback();

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_image(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);

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

    public static async Task<NativeDecodedImage?> TryDecodeAsync(
        string path,
        TimeSpan timeout,
        CancellationToken cancellationToken,
        uint targetWidth = 0,
        uint targetHeight = 0)
    {
        if (IsTooLarge(path))
            return null;

        cancellationToken.ThrowIfCancellationRequested();

        if (ShouldPreferSystemDecoder(path))
        {
            NativeDecodedImage? systemImage = await SystemImageDecoder.TryDecodeAsync(path, cancellationToken, targetWidth, targetHeight);
            if (systemImage is not null)
                return systemImage;
            DiagLog.Write("RasterHost", $"system image preferred decode failed; falling back to native path={path}");
        }

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        Task<NativeDecodedImage?> decodeTask = DecodeOnGateAsync(path, cancellationToken, targetWidth, targetHeight);
        Task delayTask = Task.Delay(timeout, timeoutCts.Token);
        Task completed = await Task.WhenAny(decodeTask, delayTask);
        if (completed != decodeTask)
            return await SystemImageDecoder.TryDecodeAsync(path, cancellationToken, targetWidth, targetHeight);

        timeoutCts.Cancel();
        NativeDecodedImage? nativeImage = await decodeTask;
        return nativeImage ?? await SystemImageDecoder.TryDecodeAsync(path, cancellationToken, targetWidth, targetHeight);
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
                        int width = checked((int)BitConverter.ToUInt32(buffer, 0));
                        int height = checked((int)BitConverter.ToUInt32(buffer, 4));
                        int originalWidth = checked((int)BitConverter.ToUInt32(buffer, 8));
                        int originalHeight = checked((int)BitConverter.ToUInt32(buffer, 12));
                        int decodeMs = checked((int)BitConverter.ToUInt32(buffer, 16));
                        int resizeMs = checked((int)BitConverter.ToUInt32(buffer, 20));
                        int convertMs = checked((int)BitConverter.ToUInt32(buffer, 24));
                        int pixelBytes = n - HeaderBytes;
                        if (width <= 0 || height <= 0 || pixelBytes != width * height * 4)
                            return null;

                        var bgra = new byte[pixelBytes];
                        Buffer.BlockCopy(buffer, HeaderBytes, bgra, 0, pixelBytes);
                        return new NativeDecodedImage(bgra, width, height, originalWidth, originalHeight)
                        {
                            DecodeMilliseconds = decodeMs,
                            ResizeMilliseconds = resizeMs,
                            ConvertMilliseconds = convertMs,
                        };
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
            or ".jpg" or ".jpeg"
            or ".tif" or ".tiff"
            or ".heic" or ".heif"
            or ".avif"
            or ".jxl";
    }
}
