using System.Globalization;
using QuickLook.Next.Core;
using QuickLook.Next.RasterHost;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class ImageDecodeDiagnosticsTests
{
    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void Missing_timings_are_not_reported_as_measured_zero(bool handleInput)
    {
        var image = new NativeDecodedImage(new byte[8], 2, 1, 4, 2);

        Assert.Null(image.NativeStageTimings);
        Assert.Equal(
            $"image {(handleInput ? "handle " : "")}raster 2x1 original=4x2; target=2x2; timingSource=none; decode=na resize=na convert=na bytes=8; waveform=deferred",
            ImageDecodeDiagnostics.Format(image, 2, 2, handleInput));
    }

    [Theory]
    [InlineData("en-US")]
    [InlineData("fr-FR")]
    [InlineData("ar-SA")]
    public void Native_timings_preserve_measured_zero_and_invariant_units(string culture)
    {
        CultureInfo previousCulture = CultureInfo.CurrentCulture;
        try
        {
            CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo(culture);
            var image = new NativeDecodedImage(new byte[8], 2, 1, 4, 2)
            {
                NativeStageTimings = new(0, 12, 3456),
                Waveform = ImageWaveformBuilder.Create(new byte[8], 2, 1),
            };

            Assert.Equal(
                "image handle raster 2x1 original=4x2; target=2x2; timingSource=native; decode=0ms resize=12ms convert=3456ms bytes=8; waveform=inline",
                ImageDecodeDiagnostics.Format(image, 2, 2, handleInput: true));
        }
        finally
        {
            CultureInfo.CurrentCulture = previousCulture;
        }
    }
}
