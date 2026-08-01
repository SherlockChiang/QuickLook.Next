using System.Buffers;
using QuickLook.Next.Core;

namespace QuickLook.Next.Core;

public static class ImageWaveformBuilder
{
    public const int ScopeWidth = 192;
    public const int ScopeHeight = 96;
    private const int ChannelCount = 3;

    public static bool IsValid(ImageWaveform? waveform)
        => waveform is not null
            && waveform.Width == ScopeWidth
            && waveform.Height == ScopeHeight
            && waveform.RgbDensity is not null
            && waveform.RgbDensity.Length == ScopeWidth * ScopeHeight * ChannelCount;

    public static ImageWaveform Create(byte[] bgra, int width, int height)
        => Create(bgra.AsSpan(), width, height);

    public static ImageWaveform Create(ReadOnlySpan<byte> bgra, int width, int height)
    {
        int planeLength = ScopeWidth * ScopeHeight;
        int countLength = planeLength * ChannelCount;
        int[] counts = ArrayPool<int>.Shared.Rent(countLength);
        counts.AsSpan(0, countLength).Clear();

        try
        {
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

            int maximum = 0;
            for (int i = 0; i < countLength; i++)
                maximum = Math.Max(maximum, counts[i]);

            var density = new byte[countLength];
            if (maximum > 0)
            {
                double denominator = Math.Log2(maximum + 1d);
                for (int i = 0; i < countLength; i++)
                    density[i] = (byte)Math.Round(255d * Math.Log2(counts[i] + 1d) / denominator);
            }

            return new ImageWaveform(ScopeWidth, ScopeHeight, density);
        }
        finally
        {
            ArrayPool<int>.Shared.Return(counts, clearArray: true);
        }
    }

    private static void Add(int[] counts, int planeLength, int channel, int column, byte value)
    {
        int row = ScopeHeight - 1 - value * (ScopeHeight - 1) / 255;
        counts[channel * planeLength + row * ScopeWidth + column]++;
    }

    private static byte Unpremultiply(byte value, byte alpha)
        => alpha == 255 ? value : (byte)Math.Min(255, (value * 255 + alpha / 2) / alpha);
}
