using System.Globalization;

namespace QuickLook.Next.RasterHost;

internal sealed record NativeImageStageTimings(
    int DecodeMilliseconds,
    int ResizeMilliseconds,
    int ConvertMilliseconds);

internal static class ImageDecodeDiagnostics
{
    public static string Format(
        NativeDecodedImage image,
        uint targetWidth,
        uint targetHeight,
        bool handleInput)
    {
        string stages = image.NativeStageTimings is { } timings
            ? string.Create(CultureInfo.InvariantCulture,
                $"timingSource=native; decode={timings.DecodeMilliseconds}ms resize={timings.ResizeMilliseconds}ms convert={timings.ConvertMilliseconds}ms")
            : "timingSource=none; decode=na resize=na convert=na";

        return string.Create(CultureInfo.InvariantCulture,
            $"image {(handleInput ? "handle " : "")}raster {image.Width}x{image.Height} original={image.OriginalWidth}x{image.OriginalHeight}; target={targetWidth}x{targetHeight}; {stages} bytes={image.Bgra.Length}; waveform={(image.Waveform is null ? "deferred" : "inline")}");
    }
}
