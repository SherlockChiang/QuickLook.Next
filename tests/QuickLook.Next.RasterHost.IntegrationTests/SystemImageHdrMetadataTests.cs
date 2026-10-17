using System.Buffers.Binary;
using QuickLook.Next.RasterHost;
using Xunit;

namespace QuickLook.Next.RasterHost.IntegrationTests;

public sealed class SystemImageHdrMetadataTests
{
    [Fact]
    public void Reads_nested_hevc_pq_bt2020_metadata()
    {
        byte[] data = Container(
            Box("ftyp", "heic"u8.ToArray()),
            Box("meta", [0, 0, 0, 0], Box("iprp", Box("ipco",
                Box("colr", Nclx(primaries: 9, transfer: 16)),
                Box("hvcC", HevcConfig(bitDepthMinus8: 2))))));

        using var stream = new MemoryStream(data, writable: false);
        var metadata = SystemImageMetadataReader.TryReadIsoBmffHdrMetadata(
            stream,
            data.Length,
            CancellationToken.None);

        Assert.NotNull(metadata);
        Assert.Equal("SMPTE ST 2084", metadata.Value.TransferFunction);
        Assert.Equal("BT.2020", metadata.Value.ColorPrimaries);
        Assert.Equal((byte)10, metadata.Value.BitDepth);
        Assert.True(metadata.Value.IsComplete);
    }

    [Fact]
    public void Reads_av1_hlg_bt2020_twelve_bit_metadata()
    {
        byte[] data = Container(
            Box("ftyp", "avif"u8.ToArray()),
            Box("colr", Nclx(primaries: 9, transfer: 18)),
            Box("av1C", Av1Config(highBitDepth: true, twelveBit: true)));

        using var stream = new MemoryStream(data, writable: false);
        var metadata = SystemImageMetadataReader.TryReadIsoBmffHdrMetadata(
            stream,
            data.Length,
            CancellationToken.None);

        Assert.NotNull(metadata);
        Assert.Equal("ARIB STD-B67", metadata.Value.TransferFunction);
        Assert.Equal((byte)12, metadata.Value.BitDepth);
        Assert.True(metadata.Value.IsComplete);
    }

    [Fact]
    public void Ignores_non_hdr_or_truncated_boxes()
    {
        byte[] sdr = Container(
            Box("ftyp", "heic"u8.ToArray()),
            Box("colr", Nclx(primaries: 1, transfer: 13)),
            Box("hvcC", HevcConfig(bitDepthMinus8: 2)));
        using var sdrStream = new MemoryStream(sdr, writable: false);
        Assert.Null(SystemImageMetadataReader.TryReadIsoBmffHdrMetadata(
            sdrStream,
            sdr.Length,
            CancellationToken.None));

        byte[] truncated = [0, 0, 0, 32, (byte)'c', (byte)'o', (byte)'l', (byte)'r', (byte)'n'];
        using var truncatedStream = new MemoryStream(truncated, writable: false);
        Assert.Null(SystemImageMetadataReader.TryReadIsoBmffHdrMetadata(
            truncatedStream,
            truncated.Length,
            CancellationToken.None));
    }

    [Fact]
    public void Converts_pq_rgba16_to_premultiplied_scrgb_half_and_bounded_sdr_preview()
    {
        byte[] source = new byte[8];
        BinaryPrimitives.WriteUInt16LittleEndian(source.AsSpan(0, 2), ushort.MaxValue);
        BinaryPrimitives.WriteUInt16LittleEndian(source.AsSpan(2, 2), ushort.MaxValue);
        BinaryPrimitives.WriteUInt16LittleEndian(source.AsSpan(4, 2), ushort.MaxValue);
        BinaryPrimitives.WriteUInt16LittleEndian(source.AsSpan(6, 2), ushort.MaxValue);

        (byte[] fp16, byte[] bgra) = SystemImageDecoder.ConvertPqToScRgb(source, 1, 1);
        float scRgbWhite = (float)BitConverter.UInt16BitsToHalf(
            BinaryPrimitives.ReadUInt16LittleEndian(fp16.AsSpan(0, 2)));

        Assert.InRange(scRgbWhite, 120f, 125f);
        Assert.Equal(8, fp16.Length);
        Assert.Equal(4, bgra.Length);
        Assert.Equal(255, bgra[3]);
        Assert.True(bgra[0] > 0 && bgra[1] > 0 && bgra[2] > 0);
    }

    private static byte[] Container(params byte[][] boxes)
        => boxes.SelectMany(static value => value).ToArray();

    private static byte[] Box(string type, params byte[][] payloads)
    {
        byte[] payload = payloads.SelectMany(static value => value).ToArray();
        byte[] result = new byte[8 + payload.Length];
        BinaryPrimitives.WriteUInt32BigEndian(result.AsSpan(0, 4), checked((uint)result.Length));
        result[4] = (byte)type[0];
        result[5] = (byte)type[1];
        result[6] = (byte)type[2];
        result[7] = (byte)type[3];
        payload.CopyTo(result, 8);
        return result;
    }

    private static byte[] Nclx(ushort primaries, ushort transfer)
    {
        byte[] value = new byte[11];
        "nclx"u8.CopyTo(value);
        BinaryPrimitives.WriteUInt16BigEndian(value.AsSpan(4, 2), primaries);
        BinaryPrimitives.WriteUInt16BigEndian(value.AsSpan(6, 2), transfer);
        BinaryPrimitives.WriteUInt16BigEndian(value.AsSpan(8, 2), 9);
        value[10] = 0x80;
        return value;
    }

    private static byte[] HevcConfig(byte bitDepthMinus8)
    {
        byte[] value = new byte[24];
        value[17] = bitDepthMinus8;
        return value;
    }

    private static byte[] Av1Config(bool highBitDepth, bool twelveBit)
        => [0x81, 0, (byte)((highBitDepth ? 0x40 : 0) | (twelveBit ? 0x20 : 0))];
}
