using System.Collections.Concurrent;
using Windows.Graphics.Imaging;
using Windows.Storage;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

internal static class SystemImageDecoder
{
    private const uint MaxPreviewRasterDimension = 2048;
    private const int MaxDecodedImageBytes = (int)(MaxPreviewRasterDimension * MaxPreviewRasterDimension * 4);
    private const long MaxInputImageBytes = 512L * 1024 * 1024;
    private static readonly ConcurrentDictionary<string, byte> UnsupportedSystemCodecs = new(StringComparer.OrdinalIgnoreCase);

    public static async Task<NativeDecodedImage?> TryDecodeAsync(
        string path,
        CancellationToken cancellationToken,
        uint targetWidth = 0,
        uint targetHeight = 0)
    {
        try
        {
            string ext = Path.GetExtension(path).ToLowerInvariant();
            if (UnsupportedSystemCodecs.ContainsKey(ext))
            {
                DiagLog.Write("RasterHost", $"system image decode skipped unsupported ext={ext}; path={path}");
                return null;
            }

            if (IsTooLarge(path))
                return null;

            cancellationToken.ThrowIfCancellationRequested();
            StorageFile file = await StorageFile.GetFileFromPathAsync(path);
            using var stream = await file.OpenReadAsync();
            BitmapDecoder decoder = await BitmapDecoder.CreateAsync(stream);
            cancellationToken.ThrowIfCancellationRequested();

            uint originalWidth = decoder.OrientedPixelWidth > 0 ? decoder.OrientedPixelWidth : decoder.PixelWidth;
            uint originalHeight = decoder.OrientedPixelHeight > 0 ? decoder.OrientedPixelHeight : decoder.PixelHeight;
            if (originalWidth == 0 || originalHeight == 0)
                return null;

            var transform = new BitmapTransform();
            uint decodeTargetWidth = targetWidth > 0 ? Math.Min(targetWidth, MaxPreviewRasterDimension) : MaxPreviewRasterDimension;
            uint decodeTargetHeight = targetHeight > 0 ? Math.Min(targetHeight, MaxPreviewRasterDimension) : MaxPreviewRasterDimension;
            double scale = Math.Min(1.0, Math.Min(decodeTargetWidth / (double)originalWidth, decodeTargetHeight / (double)originalHeight));
            if (scale < 1.0)
            {
                transform.ScaledWidth = Math.Max(1, (uint)Math.Round(originalWidth * scale));
                transform.ScaledHeight = Math.Max(1, (uint)Math.Round(originalHeight * scale));
                transform.InterpolationMode = BitmapInterpolationMode.Fant;
            }

            PixelDataProvider pixels = await decoder.GetPixelDataAsync(
                BitmapPixelFormat.Bgra8,
                BitmapAlphaMode.Premultiplied,
                transform,
                ExifOrientationMode.RespectExifOrientation,
                ColorManagementMode.ColorManageToSRgb);
            cancellationToken.ThrowIfCancellationRequested();

            byte[] bgra = pixels.DetachPixelData();
            if (bgra.Length <= 0 || bgra.Length > MaxDecodedImageBytes || bgra.Length % 4 != 0)
                return null;

            int width = transform.ScaledWidth > 0 ? checked((int)transform.ScaledWidth) : checked((int)originalWidth);
            int height = transform.ScaledHeight > 0 ? checked((int)transform.ScaledHeight) : checked((int)originalHeight);
            if (bgra.Length != width * height * 4)
            {
                int pixelCount = bgra.Length / 4;
                if (width > 0 && pixelCount % width == 0)
                    height = pixelCount / width;
                else
                    return null;
            }

            DiagLog.Write(
                "RasterHost",
                $"system image decoded ext={ext}; target={targetWidth}x{targetHeight}; " +
                $"original={originalWidth}x{originalHeight}; output={width}x{height}; bytes={bgra.Length}");

            return new NativeDecodedImage(bgra, width, height, checked((int)originalWidth), checked((int)originalHeight));
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            CacheUnsupportedSystemCodec(path);
            DiagLog.Write("RasterHost", $"system image decode failed: {ex.Message}");
            return null;
        }
    }

    private static void CacheUnsupportedSystemCodec(string path)
    {
        string ext = Path.GetExtension(path).ToLowerInvariant();
        if (ext is ".avif" or ".heic" or ".heif" or ".jxl")
            UnsupportedSystemCodecs.TryAdd(ext, 0);
    }

    private static bool IsTooLarge(string path)
    {
        try { return new FileInfo(path).Length > MaxInputImageBytes; }
        catch { return false; }
    }
}
