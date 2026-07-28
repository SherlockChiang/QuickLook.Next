using System.Buffers.Binary;
using System.Text;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class ShellBrokerProtocolTests
{
    private const string RequestId = "0123456789abcdef0123456789abcdef";

    [Fact]
    public void Parses_ready_thumbnail_and_error_messages()
    {
        Assert.IsType<ShellBrokerReady>(ShellBrokerProtocol.Parse("READY"));

        var ready = Assert.IsType<ShellThumbnailReady>(
            ShellBrokerProtocol.Parse($"THUMB\t{RequestId}\t1234\t4104\t32\t32"));
        Assert.Equal((1234L, 4104L, 32, 32),
            (ready.FileHandle, ready.PacketLength, ready.Width, ready.Height));

        string payload = Convert.ToBase64String(Encoding.UTF8.GetBytes("provider failed"));
        var error = Assert.IsType<PreviewError>(
            ShellBrokerProtocol.Parse($"ERROR\t{RequestId}\t{payload}"));
        Assert.Equal("provider failed", error.Message);
    }

    [Theory]
    [InlineData("THUMB\t0123456789abcdef0123456789abcdeg\t1\t12\t1\t1")]
    [InlineData("THUMB\t0123456789abcdef0123456789abcdef\t0\t12\t1\t1")]
    [InlineData("THUMB\t0123456789abcdef0123456789abcdef\t1\t12\t0\t1")]
    [InlineData("THUMB\t0123456789abcdef0123456789abcdef\t1\t12\t513\t1")]
    [InlineData("THUMB\t0123456789abcdef0123456789abcdef\t1\t11\t1\t1")]
    [InlineData("THUMB\t0123456789abcdef0123456789abcdef\t+1\t12\t1\t1")]
    [InlineData("THUMB\t0123456789abcdef0123456789abcdef\t1\t12\t1,0\t1")]
    [InlineData("ERROR\tinvalid\tZmFpbA==")]
    [InlineData("ERROR\t0123456789abcdef0123456789abcdef\tnot-base64")]
    [InlineData("UNKNOWN")]
    public void Rejects_malformed_control_messages(string line)
        => Assert.Throws<InvalidDataException>(() => ShellBrokerProtocol.Parse(line));

    [Fact]
    public void Rejects_non_utf8_and_oversized_error_payloads()
    {
        string invalidUtf8 = Convert.ToBase64String([0xc3, 0x28]);
        Assert.Throws<InvalidDataException>(() =>
            ShellBrokerProtocol.Parse($"ERROR\t{RequestId}\t{invalidUtf8}"));

        string oversized = Convert.ToBase64String(new byte[ShellBrokerProtocol.MaxErrorUtf8Bytes + 1]);
        Assert.Throws<InvalidDataException>(() =>
            ShellBrokerProtocol.Parse($"ERROR\t{RequestId}\t{oversized}"));
    }

    [Theory]
    [InlineData(1, 1, 12, true, 4)]
    [InlineData(512, 512, 1048584, true, 1048576)]
    [InlineData(513, 1, 2060, false, 0)]
    [InlineData(int.MaxValue, int.MaxValue, long.MaxValue, false, 0)]
    [InlineData(32, 32, 4103, false, 0)]
    public void Validates_thumbnail_dimensions_and_checked_packet_length(
        int width, int height, long packetLength, bool expected, int expectedPixels)
    {
        var ready = new ShellThumbnailReady(RequestId, 1, packetLength, width, height);

        Assert.Equal(expected, ShellBrokerProtocol.TryGetPixelByteCount(ready, out int pixelBytes));
        Assert.Equal(expectedPixels, pixelBytes);
    }

    [Fact]
    public void Header_validation_requires_matching_little_endian_dimensions()
    {
        var ready = new ShellThumbnailReady(RequestId, 1, 4104, 32, 32);
        byte[] header = new byte[8];
        BinaryPrimitives.WriteInt32LittleEndian(header, 32);
        BinaryPrimitives.WriteInt32LittleEndian(header.AsSpan(4), 32);

        Assert.True(ShellBrokerProtocol.HeaderMatches(ready, header));
        header[4] = 31;
        Assert.False(ShellBrokerProtocol.HeaderMatches(ready, header));
        Assert.False(ShellBrokerProtocol.HeaderMatches(ready, header.AsSpan(0, 7)));
    }
}
