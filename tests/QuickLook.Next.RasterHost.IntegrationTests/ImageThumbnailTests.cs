using QuickLook.Next.Core;
using QuickLook.Next.RasterHost;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class ImageThumbnailTests
{
    [Fact]
    public void Small_surface_preserves_premultiplied_pixels_and_protocol_round_trips()
    {
        byte[] pixels = [20, 40, 60, 128, 0, 0, 0, 0];
        var image = new NativeDecodedImage(pixels, 2, 1, 2, 1);
        var thumbnail = Assert.IsType<PreviewImageThumbnail>(ImageThumbnailBuilder.Create("current", image, default));
        var roundTrip = Assert.IsType<PreviewImageThumbnail>(ProtocolJson.Deserialize(ProtocolJson.Serialize(thumbnail)));

        Assert.True(roundTrip.HasValidPixels());
        Assert.Equal("current", roundTrip.RequestId);
        Assert.Equal(pixels, roundTrip.Bgra);
        Assert.NotSame(pixels, thumbnail.Bgra);
    }

    [Theory]
    [InlineData(2048, 1024, 96, 48)]
    [InlineData(1, 2048, 1, 96)]
    [InlineData(2048, 1, 96, 1)]
    public void Surface_thumbnail_is_bounded_and_retains_color(int width, int height, int expectedWidth, int expectedHeight)
    {
        byte[] pixels = new byte[width * height * 4];
        for (int i = 0; i < pixels.Length; i += 4)
        {
            pixels[i + 1] = 180;
            pixels[i + 3] = 255;
        }
        var image = new NativeDecodedImage(pixels, width, height, width, height);
        var thumbnail = Assert.IsType<PreviewImageThumbnail>(ImageThumbnailBuilder.Create("current", image, default));

        Assert.Equal(expectedWidth, thumbnail.Width);
        Assert.Equal(expectedHeight, thumbnail.Height);
        Assert.True(thumbnail.HasValidPixels());
        for (int i = 0; i < thumbnail.Bgra.Length; i += 4)
            Assert.Equal(new byte[] { 0, 180, 0, 255 }, thumbnail.Bgra[i..(i + 4)]);
    }

    [Theory]
    [InlineData(0, 1, 0)]
    [InlineData(1, -1, 0)]
    [InlineData(int.MaxValue, int.MaxValue, 0)]
    [InlineData(1, 1, 3)]
    [InlineData(1, 1, 5)]
    public void Malformed_surface_is_rejected_before_allocation(int width, int height, int bytes)
        => Assert.Null(ImageThumbnailBuilder.Create("current", new(new byte[bytes], width, height, width, height), default));

    [Fact]
    public void Canceled_surface_does_not_publish_a_thumbnail()
        => Assert.Throws<OperationCanceledException>(() => ImageThumbnailBuilder.Create(
            "closed", new(new byte[4], 1, 1, 1, 1), new CancellationToken(true)));

    [Theory]
    [InlineData(0, 1, 0)]
    [InlineData(97, 1, 388)]
    [InlineData(1, 97, 388)]
    [InlineData(int.MaxValue, int.MaxValue, 0)]
    [InlineData(1, 1, 3)]
    [InlineData(1, 1, 5)]
    public void Host_payload_dimensions_and_exact_length_are_validated(int width, int height, int bytes)
        => Assert.False(new PreviewImageThumbnail("current", width, height, new byte[bytes]).HasValidPixels());

    [Fact]
    public void Null_host_pixels_are_rejected()
        => Assert.False(new PreviewImageThumbnail("current", 1, 1, null!).HasValidPixels());
}
