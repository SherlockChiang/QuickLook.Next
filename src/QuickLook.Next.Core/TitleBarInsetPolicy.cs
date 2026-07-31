namespace QuickLook.Next.Core;

public readonly record struct TitleBarPadding(double Left, double Right);

public static class TitleBarInsetPolicy
{
    public static TitleBarPadding Calculate(
        double basePadding,
        double leftInsetPixels,
        double rightInsetPixels,
        double rasterizationScale)
        => Calculate(
            basePadding,
            basePadding,
            leftInsetPixels,
            rightInsetPixels,
            rasterizationScale);

    public static TitleBarPadding Calculate(
        double baseLeft,
        double baseRight,
        double leftInsetPixels,
        double rightInsetPixels,
        double rasterizationScale)
    {
        double scale = double.IsFinite(rasterizationScale) && rasterizationScale > 0
            ? rasterizationScale
            : 1;

        return new TitleBarPadding(
            CalculateSide(baseLeft, leftInsetPixels, scale),
            CalculateSide(baseRight, rightInsetPixels, scale));
    }

    private static double CalculateSide(double basePadding, double insetPixels, double scale)
    {
        double safeBase = double.IsFinite(basePadding) && basePadding > 0 ? basePadding : 0;
        double safeInset = double.IsFinite(insetPixels) && insetPixels > 0 ? insetPixels : 0;
        double insetDips = Math.Ceiling(safeInset / scale);
        double padding = safeBase + insetDips;
        return double.IsFinite(padding) ? padding : safeBase;
    }
}
