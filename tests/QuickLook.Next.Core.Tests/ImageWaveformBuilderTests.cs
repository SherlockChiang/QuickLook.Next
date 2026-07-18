using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class ImageWaveformBuilderTests
{
    [Fact]
    public void Create_unpremultiplies_channels_and_maps_intensity_to_rows()
    {
        byte[] halfAlphaRed = [0, 0, 128, 128];

        var waveform = ImageWaveformBuilder.Create(halfAlphaRed, 1, 1);

        int planeLength = waveform.Width * waveform.Height;
        Assert.Equal(255, waveform.RgbDensity[0]);
        Assert.Equal(255, waveform.RgbDensity[planeLength + (waveform.Height - 1) * waveform.Width]);
        Assert.Equal(255, waveform.RgbDensity[planeLength * 2 + (waveform.Height - 1) * waveform.Width]);
    }

    [Fact]
    public void Create_ignores_fully_transparent_pixels()
    {
        var waveform = ImageWaveformBuilder.Create([255, 255, 255, 0], 1, 1);

        Assert.All(waveform.RgbDensity, value => Assert.Equal(0, value));
    }
}
