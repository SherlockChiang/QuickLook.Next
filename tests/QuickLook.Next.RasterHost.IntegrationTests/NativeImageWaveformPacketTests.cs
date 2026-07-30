using System.Buffers.Binary;
using System.Reflection;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class NativeImageWaveformPacketTests
{
    private const int HeaderBytes = 40;
    private const int DensityBytes =
        ImageWaveformBuilder.ScopeWidth * ImageWaveformBuilder.ScopeHeight * 3;

    [Fact]
    public void Native_waveform_packet_accepts_only_exact_bounded_layout()
    {
        byte[] valid = CreatePacket();
        object? parsed = Parse(valid);
        Assert.NotNull(parsed);
        object decoded = parsed;
        Assert.Equal(new byte[] { 3, 2, 1, 255 }, Assert.IsType<byte[]>(
            decoded.GetType().GetProperty("Bgra")!.GetValue(decoded)));
        var waveform = Assert.IsType<ImageWaveform>(
            decoded.GetType().GetProperty("Waveform")!.GetValue(decoded));
        Assert.True(ImageWaveformBuilder.IsValid(waveform));
        Assert.Equal(DensityBytes, waveform.RgbDensity.Length);

        Assert.Null(Parse(valid, valid.Length - 1));
        Assert.Null(Parse([.. valid, 0]));

        AssertRejected(valid, 0, 0);
        AssertRejected(valid, 0, 2049);
        AssertRejected(valid, 4, 0);
        AssertRejected(valid, 28, ImageWaveformBuilder.ScopeWidth - 1);
        AssertRejected(valid, 32, ImageWaveformBuilder.ScopeHeight + 1);
        AssertRejected(valid, 36, DensityBytes - 1);

        byte[] mismatchedRasterLength = (byte[])valid.Clone();
        BinaryPrimitives.WriteUInt32LittleEndian(mismatchedRasterLength.AsSpan(0, 4), 2);
        Assert.Null(Parse(mismatchedRasterLength));
    }

    private static void AssertRejected(byte[] valid, int offset, int value)
    {
        byte[] malformed = (byte[])valid.Clone();
        BinaryPrimitives.WriteUInt32LittleEndian(malformed.AsSpan(offset, 4), checked((uint)value));
        Assert.Null(Parse(malformed));
    }

    private static byte[] CreatePacket()
    {
        var packet = new byte[HeaderBytes + 4 + DensityBytes];
        Write(packet, 0, 1);
        Write(packet, 4, 1);
        Write(packet, 8, 1);
        Write(packet, 12, 1);
        Write(packet, 16, 2);
        Write(packet, 20, 3);
        Write(packet, 24, 4);
        Write(packet, 28, ImageWaveformBuilder.ScopeWidth);
        Write(packet, 32, ImageWaveformBuilder.ScopeHeight);
        Write(packet, 36, DensityBytes);
        packet[HeaderBytes] = 3;
        packet[HeaderBytes + 1] = 2;
        packet[HeaderBytes + 2] = 1;
        packet[HeaderBytes + 3] = 255;
        packet[^1] = 127;
        return packet;
    }

    private static void Write(byte[] packet, int offset, int value)
        => BinaryPrimitives.WriteUInt32LittleEndian(packet.AsSpan(offset, 4), checked((uint)value));

    private static object? Parse(byte[] packet, int? length = null)
    {
        string assemblyPath = Path.Combine(
            AppContext.BaseDirectory,
            "RasterHost",
            "QuickLook.Next.RasterHost.dll");
        Assembly assembly = Assembly.LoadFrom(assemblyPath);
        Type? decoder = assembly.GetType("QuickLook.Next.RasterHost.NativeImageDecoder");
        Assert.NotNull(decoder);
        MethodInfo? parser = decoder.GetMethod(
            "ParseDecodedImageWithWaveform",
            BindingFlags.NonPublic | BindingFlags.Static);
        Assert.NotNull(parser);
        return parser.Invoke(null, [packet, length ?? packet.Length]);
    }
}
