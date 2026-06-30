using System.Runtime.InteropServices;
using QuickLook.Next.Contracts;
using Windows.Graphics.Imaging;

namespace QuickLook.Next.Plugin.Image;

public sealed class ImageProvider : IPreviewProvider
{
    private const uint MaxRasterDimension = 4096;

    private static readonly HashSet<string> Extensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".bmp", ".gif", ".ico", ".jfif", ".jpe", ".jpeg", ".jpg", ".png", ".tif", ".tiff",
        ".webp", ".heic", ".heif", ".avif", ".jxr", ".hdp", ".wdp", ".dds",
    };

    public bool CanHandle(FileProbe probe)
    {
        if (Extensions.Contains(probe.Extension)) return true;

        ReadOnlySpan<byte> magic = probe.MagicPrefix;
        return HasPrefix(magic, 0x89, (byte)'P', (byte)'N', (byte)'G')
               || HasPrefix(magic, 0xFF, 0xD8, 0xFF)
               || HasPrefix(magic, (byte)'G', (byte)'I', (byte)'F')
               || HasPrefix(magic, (byte)'B', (byte)'M')
               || HasPrefix(magic, (byte)'I', (byte)'I', 0x2A, 0x00)
               || HasPrefix(magic, (byte)'M', (byte)'M', 0x00, 0x2A)
               || HasPrefix(magic, 0x52, 0x49, 0x46, 0x46); // RIFF (WebP)
    }

    public async Task<PreviewResult> OpenAsync(string path, FileProbe probe, IPreviewContext context)
    {
        context.ReportStatus("ImageProvider: decoding image...");

        using var fs = File.OpenRead(path);
        var decoder = await BitmapDecoder.CreateAsync(fs.AsRandomAccessStream());

        var origWidth = (int)decoder.OrientedPixelWidth;
        var origHeight = (int)decoder.OrientedPixelHeight;
        var (targetWidth, targetHeight) = Constrain(origWidth, origHeight, MaxRasterDimension);

        var transform = new BitmapTransform
        {
            InterpolationMode = BitmapInterpolationMode.Fant,
            ScaledWidth = (uint)targetWidth,
            ScaledHeight = (uint)targetHeight,
        };

        var pixelData = await decoder.GetPixelDataAsync(
            BitmapPixelFormat.Bgra8,
            BitmapAlphaMode.Premultiplied,
            transform,
            ExifOrientationMode.RespectExifOrientation,
            ColorManagementMode.ColorManageToSRgb);

        var detachedBuffer = pixelData.DetachPixelData();
        byte[] bgra = detachedBuffer.ToArray();

        string title = targetWidth == origWidth && targetHeight == origHeight
            ? Path.GetFileName(path)
            : $"{Path.GetFileName(path)} — {origWidth}x{origHeight} scaled to {targetWidth}x{targetHeight}";

        return new PreviewResult("image", title)
        {
            PreferredWidth = targetWidth,
            PreferredHeight = targetHeight,
            Bgra = bgra,
            PixelWidth = targetWidth,
            PixelHeight = targetHeight,
        };
    }

    private static bool HasPrefix(ReadOnlySpan<byte> value, params byte[] prefix)
        => value.Length >= prefix.Length && value[..prefix.Length].SequenceEqual(prefix);

    private static (int Width, int Height) Constrain(int width, int height, uint maxDimension)
    {
        long largest = Math.Max(width, height);
        if (largest <= maxDimension) return (width, height);

        double scale = (double)maxDimension / largest;
        return (Math.Max(1, (int)Math.Round(width * scale)), Math.Max(1, (int)Math.Round(height * scale)));
    }
}
