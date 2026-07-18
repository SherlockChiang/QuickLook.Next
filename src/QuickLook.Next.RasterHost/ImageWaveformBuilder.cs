using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

internal static class ImageWaveformBuilder
{
    internal const int ScopeWidth = 192;
    internal const int ScopeHeight = 96;
    private const int ChannelCount = 3;

    public static ImageWaveform Create(byte[] bgra, int width, int height)
    {
        int planeLength = ScopeWidth * ScopeHeight;
        var counts = new int[planeLength * ChannelCount];
        int sampleStep = Math.Max(1, (int)Math.Ceiling(Math.Sqrt((width * (long)height) / 1_000_000d)));

        for (int y = 0; y < height; y += sampleStep)
        {
            int row = y * width * 4;
            for (int x = 0; x < width; x += sampleStep)
            {
                int pixel = row + x * 4;
                if (bgra[pixel + 3] == 0)
                    continue;

                int column = Math.Min(ScopeWidth - 1, x * ScopeWidth / width);
                byte alpha = bgra[pixel + 3];
                Add(counts, planeLength, 0, column, Unpremultiply(bgra[pixel + 2], alpha));
                Add(counts, planeLength, 1, column, Unpremultiply(bgra[pixel + 1], alpha));
                Add(counts, planeLength, 2, column, Unpremultiply(bgra[pixel], alpha));
            }
        }

        int maximum = counts.Max();
        var density = new byte[counts.Length];
        if (maximum > 0)
        {
            double denominator = Math.Log2(maximum + 1d);
            for (int i = 0; i < counts.Length; i++)
                density[i] = (byte)Math.Round(255d * Math.Log2(counts[i] + 1d) / denominator);
        }

        return new ImageWaveform(ScopeWidth, ScopeHeight, density);
    }

    private static void Add(int[] counts, int planeLength, int channel, int column, byte value)
    {
        int row = ScopeHeight - 1 - value * (ScopeHeight - 1) / 255;
        counts[channel * planeLength + row * ScopeWidth + column]++;
    }

    private static byte Unpremultiply(byte value, byte alpha)
        => alpha == 255 ? value : (byte)Math.Min(255, (value * 255 + alpha / 2) / alpha);
}
