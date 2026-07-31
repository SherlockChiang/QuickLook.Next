using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class TitleBarInsetPolicyTests
{
    [Fact]
    public void Insets_are_added_to_each_base_at_one_hundred_percent()
    {
        TitleBarPadding padding = TitleBarInsetPolicy.Calculate(
            baseLeft: 14,
            baseRight: 6,
            leftInsetPixels: 3,
            rightInsetPixels: 138,
            rasterizationScale: 1);

        Assert.Equal(17, padding.Left);
        Assert.Equal(144, padding.Right);
    }

    [Fact]
    public void Physical_pixel_insets_are_converted_at_two_hundred_percent()
    {
        TitleBarPadding padding = TitleBarInsetPolicy.Calculate(
            baseLeft: 14,
            baseRight: 6,
            leftInsetPixels: 3,
            rightInsetPixels: 139,
            rasterizationScale: 2);

        Assert.Equal(16, padding.Left);
        Assert.Equal(76, padding.Right);
    }

    [Theory]
    [InlineData(1, 138)]
    [InlineData(1.5, 207)]
    [InlineData(2, 276)]
    public void Equivalent_physical_insets_keep_the_same_dip_padding(
        double scale,
        double rightInsetPixels)
    {
        TitleBarPadding padding = TitleBarInsetPolicy.Calculate(
            basePadding: 16,
            leftInsetPixels: 0,
            rightInsetPixels: rightInsetPixels,
            rasterizationScale: scale);

        Assert.Equal(16, padding.Left);
        Assert.Equal(154, padding.Right);
    }

    [Theory]
    [InlineData(0)]
    [InlineData(-2)]
    [InlineData(double.NaN)]
    [InlineData(double.PositiveInfinity)]
    [InlineData(double.NegativeInfinity)]
    public void Invalid_scale_falls_back_to_one(double scale)
    {
        TitleBarPadding padding = TitleBarInsetPolicy.Calculate(
            basePadding: 4,
            leftInsetPixels: 1.2,
            rightInsetPixels: 2.2,
            rasterizationScale: scale);

        Assert.Equal(6, padding.Left);
        Assert.Equal(7, padding.Right);
    }

    [Theory]
    [InlineData(-1)]
    [InlineData(double.NaN)]
    [InlineData(double.PositiveInfinity)]
    [InlineData(double.NegativeInfinity)]
    public void Invalid_insets_are_treated_as_zero(double inset)
    {
        TitleBarPadding padding = TitleBarInsetPolicy.Calculate(
            basePadding: 8,
            leftInsetPixels: inset,
            rightInsetPixels: inset,
            rasterizationScale: 1);

        Assert.Equal(8, padding.Left);
        Assert.Equal(8, padding.Right);
    }

    [Theory]
    [InlineData(-1)]
    [InlineData(double.NaN)]
    [InlineData(double.PositiveInfinity)]
    [InlineData(double.NegativeInfinity)]
    public void Invalid_base_padding_is_treated_as_zero(double basePadding)
    {
        TitleBarPadding padding = TitleBarInsetPolicy.Calculate(
            baseLeft: basePadding,
            baseRight: basePadding,
            leftInsetPixels: 2,
            rightInsetPixels: 3,
            rasterizationScale: 1);

        Assert.Equal(2, padding.Left);
        Assert.Equal(3, padding.Right);
    }

    [Fact]
    public void Fractional_base_is_preserved_while_fractional_insets_round_up()
    {
        TitleBarPadding padding = TitleBarInsetPolicy.Calculate(
            baseLeft: 4.25,
            baseRight: 5.5,
            leftInsetPixels: 5,
            rightInsetPixels: 7,
            rasterizationScale: 2);

        Assert.Equal(7.25, padding.Left);
        Assert.Equal(9.5, padding.Right);
    }
}
