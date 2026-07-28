using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class CloudHydrationPolicyTests
{
    [Theory]
    [InlineData(0, true)]
    [InlineData(268435456, true)]
    [InlineData(268435457, false)]
    [InlineData(-1, false)]
    public void Declared_length_is_bounded(long length, bool expected)
        => Assert.Equal(expected, CloudHydrationPolicy.IsDeclaredLengthAllowed(length));

    [Theory]
    [InlineData(0, 65536, 65536)]
    [InlineData(268435455, 65536, 2)]
    [InlineData(268435456, 65536, 1)]
    [InlineData(268435457, 65536, 0)]
    [InlineData(-1, 65536, 0)]
    [InlineData(0, 0, 0)]
    public void Next_read_allows_one_detection_byte_beyond_the_limit(
        long downloaded, int bufferLength, int expected)
        => Assert.Equal(expected, CloudHydrationPolicy.NextReadSize(downloaded, bufferLength));

    [Theory]
    [InlineData(0, 100, 0)]
    [InlineData(50, 100, 50)]
    [InlineData(100, 100, 100)]
    [InlineData(200, 100, 100)]
    [InlineData(9223372036854775807, 9223372036854775807, 100)]
    [InlineData(50, 0, 0)]
    public void Progress_is_clamped(long downloaded, long length, int expected)
        => Assert.Equal(expected, CloudHydrationPolicy.ProgressPercent(downloaded, length));
}
