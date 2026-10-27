using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

/// <summary>Small presentation sidecar sampled from the surface's existing SDR pixels; no file I/O.</summary>
internal static class ImageThumbnailBuilder
{
    public static PreviewImageThumbnail? Create(
        string requestId, NativeDecodedImage image, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        // Match the host's decoded surface bound before any multiplication or indexing.
        if (image.Width is <= 0 or > 2048 || image.Height is <= 0 or > 2048
            || image.Bgra.Length != image.Width * image.Height * 4)
            return null;

        double scale = Math.Min(1.0, (double)PreviewImageThumbnail.MaxEdge / Math.Max(image.Width, image.Height));
        int width = Math.Max(1, (int)(image.Width * scale));
        int height = Math.Max(1, (int)(image.Height * scale));
        byte[] pixels = new byte[width * height * 4];
        for (int y = 0; y < height; y++)
        {
            cancellationToken.ThrowIfCancellationRequested();
            int sourceY = Math.Min(image.Height - 1, (2 * y + 1) * image.Height / (2 * height));
            for (int x = 0; x < width; x++)
            {
                int sourceX = Math.Min(image.Width - 1, (2 * x + 1) * image.Width / (2 * width));
                image.Bgra.AsSpan((sourceY * image.Width + sourceX) * 4, 4)
                    .CopyTo(pixels.AsSpan((y * width + x) * 4, 4));
            }
        }
        return new PreviewImageThumbnail(requestId, width, height, pixels);
    }
}
