using Windows.Graphics.Imaging;
using Windows.Storage;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

internal static class SystemImageDecoder
{
    private const uint MaxPreviewRasterDimension = 2048;
    private const int MaxDecodedImageBytes = (int)(MaxPreviewRasterDimension * MaxPreviewRasterDimension * 4);
    private const long MaxInputImageBytes = 512L * 1024 * 1024;

    public static async Task<NativeDecodedImage?> TryDecodeAsync(
        string path,
        CancellationToken cancellationToken,
        uint targetWidth = 0,
        uint targetHeight = 0)
    {
        try
        {
            if (IsTooLarge(path))
                return null;

            cancellationToken.ThrowIfCancellationRequested();
            StorageFile file = await StorageFile.GetFileFromPathAsync(path);
            using var stream = await file.OpenReadAsync();
            return await DecodeAsync(stream, path, cancellationToken, targetWidth, targetHeight);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", $"system image decode failed: {ex.Message}");
            return null;
        }
    }

    public static async Task<NativeDecodedImage?> TryDecodeHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken,
        uint targetWidth = 0,
        uint targetHeight = 0)
    {
        if (sourceLength is < 0 or > MaxInputImageBytes
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed)
            return null;

        try
        {
            cancellationToken.ThrowIfCancellationRequested();
            using SafeFileHandle decodeHandle = WindowsHandleTransfer.ReopenReadOnlyFile(sourceHandle, sourceLength);
            using var fileStream = new FileStream(decodeHandle, FileAccess.Read, 64 * 1024, isAsync: false);
            if (fileStream.Length != sourceLength)
                return null;
            using var stream = fileStream.AsRandomAccessStream();
            return await DecodeAsync(stream, logicalName, cancellationToken, targetWidth, targetHeight);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", $"system HANDLE image decode failed: {ex.Message}");
            return null;
        }
    }

    private static async Task<NativeDecodedImage?> DecodeAsync(
        Windows.Storage.Streams.IRandomAccessStream stream,
        string logicalName,
        CancellationToken cancellationToken,
        uint targetWidth,
        uint targetHeight)
    {
        string ext = Path.GetExtension(logicalName).ToLowerInvariant();
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

    private static bool IsTooLarge(string path)
    {
        try { return new FileInfo(path).Length > MaxInputImageBytes; }
        catch { return false; }
    }
}
