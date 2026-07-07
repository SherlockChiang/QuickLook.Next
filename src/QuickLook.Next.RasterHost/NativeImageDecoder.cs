using System.Buffers;
using System.Runtime.InteropServices;
using System.Text;

namespace QuickLook.Next.RasterHost;

internal sealed record NativeDecodedImage(
    byte[] Bgra,
    int Width,
    int Height,
    int OriginalWidth,
    int OriginalHeight);

internal static class NativeImageDecoder
{
    private const string Dll = "quicklook_next_native";
    private const int HeaderBytes = 16;
    private const int MaxPreviewRasterDimension = 2048;
    private const int MaxDecodedImageBytes = HeaderBytes + (MaxPreviewRasterDimension * MaxPreviewRasterDimension * 4);
    private const long MaxInputImageBytes = 256L * 1024 * 1024;
    private static readonly SemaphoreSlim DecodeGate = new(1, 1);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int ql_decode_image(byte[] pathUtf8, nuint pathLen, byte[] outBuf, nuint outCap);

    public static async Task<NativeDecodedImage?> TryDecodeAsync(string path, TimeSpan timeout, CancellationToken cancellationToken)
    {
        if (IsTooLarge(path))
            return null;

        cancellationToken.ThrowIfCancellationRequested();

        if (ShouldPreferSystemDecoder(path))
        {
            NativeDecodedImage? systemImage = await SystemImageDecoder.TryDecodeAsync(path, cancellationToken);
            if (systemImage is not null)
                return systemImage;
        }

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        Task<NativeDecodedImage?> decodeTask = DecodeOnGateAsync(path, cancellationToken);
        Task delayTask = Task.Delay(timeout, timeoutCts.Token);
        Task completed = await Task.WhenAny(decodeTask, delayTask);
        if (completed != decodeTask)
            return await SystemImageDecoder.TryDecodeAsync(path, cancellationToken);

        timeoutCts.Cancel();
        NativeDecodedImage? nativeImage = await decodeTask;
        return nativeImage ?? await SystemImageDecoder.TryDecodeAsync(path, cancellationToken);
    }

    private static async Task<NativeDecodedImage?> DecodeOnGateAsync(string path, CancellationToken cancellationToken)
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
                return TryDecode(path);
            }, CancellationToken.None);
        }
        finally
        {
            DecodeGate.Release();
        }
    }

    public static NativeDecodedImage? TryDecode(string path)
    {
        try
        {
            if (IsTooLarge(path))
                return null;

            byte[] pathBytes = Encoding.UTF8.GetBytes(path);
            int cap = 8 * 1024 * 1024;
            while (cap <= MaxDecodedImageBytes)
            {
                byte[] buffer = ArrayPool<byte>.Shared.Rent(cap);
                try
                {
                    int n = ql_decode_image(pathBytes, (nuint)pathBytes.Length, buffer, (nuint)buffer.Length);
                    if (n > HeaderBytes)
                    {
                        int width = checked((int)BitConverter.ToUInt32(buffer, 0));
                        int height = checked((int)BitConverter.ToUInt32(buffer, 4));
                        int originalWidth = checked((int)BitConverter.ToUInt32(buffer, 8));
                        int originalHeight = checked((int)BitConverter.ToUInt32(buffer, 12));
                        int pixelBytes = n - HeaderBytes;
                        if (width <= 0 || height <= 0 || pixelBytes != width * height * 4)
                            return null;

                        var bgra = new byte[pixelBytes];
                        Buffer.BlockCopy(buffer, HeaderBytes, bgra, 0, pixelBytes);
                        return new NativeDecodedImage(bgra, width, height, originalWidth, originalHeight);
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
        return ext is ".jpg" or ".jpeg" or ".jpe"
            or ".tif" or ".tiff"
            or ".heic" or ".heif"
            or ".avif"
            or ".webp";
    }
}
