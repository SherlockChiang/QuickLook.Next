using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class MarkdownViewportPolicyTests
{
    [Theory]
    [InlineData(200, 300, 1000, 24, 476)]
    [InlineData(200, -80, 1000, 24, 96)]
    [InlineData(0, 10, 1000, 24, 0)]
    [InlineData(900, 300, 1000, 24, 1000)]
    public void Heading_target_aligns_leading_edge_and_clamps_to_scroll_range(
        double currentOffset,
        double headingTop,
        double maximumOffset,
        double inset,
        double expected)
        => Assert.Equal(
            expected,
            MarkdownViewportPolicy.TargetOffset(currentOffset, headingTop, maximumOffset, inset));

    [Fact]
    public void Invalid_heading_geometry_keeps_the_current_valid_offset()
        => Assert.Equal(
            200,
            MarkdownViewportPolicy.TargetOffset(200, double.NaN, 1000));

    [Fact]
    public void Partially_visible_heading_is_still_moved_to_the_leading_edge()
        => Assert.Equal(
            956,
            MarkdownViewportPolicy.TargetOffset(
                currentOffset: 400,
                headingTopInViewport: 580,
                maximumOffset: 2000,
                contentInset: 24));

    [Theory]
    [InlineData(600, true, 24, 24, 576)]
    [InlineData(30, true, 24, 24, 24)]
    [InlineData(double.NaN, true, 24, 24, 24)]
    [InlineData(600, false, 24, 24, 24)]
    public void Trailing_padding_only_leaves_alignment_room_for_outline_documents(
        double viewportHeight,
        bool hasOutline,
        double inset,
        double minimum,
        double expected)
        => Assert.Equal(
            expected,
            MarkdownViewportPolicy.TrailingPadding(viewportHeight, hasOutline, inset, minimum));

    [Fact]
    public void Final_heading_has_enough_trailing_space_to_reach_the_leading_edge()
    {
        const double viewportHeight = 600;
        const double headingTop = 1200;
        const double contentEnd = 1240;
        double trailingPadding = MarkdownViewportPolicy.TrailingPadding(viewportHeight, hasOutline: true);
        double maximumOffset = contentEnd + trailingPadding - viewportHeight;
        double targetOffset = headingTop - MarkdownViewportPolicy.DefaultContentInset;

        Assert.True(maximumOffset >= targetOffset);
    }

    [Theory]
    [InlineData(0, false, true, true)]
    [InlineData(1, false, true, true)]
    [InlineData(2, false, true, false)]
    [InlineData(0, true, true, false)]
    [InlineData(0, false, false, false)]
    [InlineData(-1, false, true, false)]
    public void Realization_retry_is_bounded_and_stops_for_realized_or_stale_content(
        int completedAttempt,
        bool containerRealized,
        bool renderIsCurrent,
        bool expected)
        => Assert.Equal(
            expected,
            MarkdownViewportPolicy.ShouldRetryRealization(
                completedAttempt,
                containerRealized,
                renderIsCurrent));
}
