namespace QuickLook.Next.Core;

public static class MarkdownViewportPolicy
{
    public const double DefaultContentInset = 24;

    public static double TargetOffset(
        double currentOffset,
        double headingTopInViewport,
        double maximumOffset,
        double contentInset = DefaultContentInset)
    {
        double maximum = IsFiniteNonNegative(maximumOffset) ? maximumOffset : 0;
        double current = IsFiniteNonNegative(currentOffset)
            ? Math.Min(currentOffset, maximum)
            : 0;
        double inset = IsFiniteNonNegative(contentInset) ? contentInset : 0;
        if (!double.IsFinite(headingTopInViewport))
            return current;

        double target = current + headingTopInViewport - inset;
        if (!double.IsFinite(target))
            return current;
        return Math.Clamp(target, 0, maximum);
    }

    public static double TrailingPadding(
        double viewportHeight,
        double contentInset = DefaultContentInset,
        double minimumPadding = DefaultContentInset)
    {
        double height = IsFiniteNonNegative(viewportHeight) ? viewportHeight : 0;
        double inset = IsFiniteNonNegative(contentInset) ? contentInset : 0;
        double minimum = IsFiniteNonNegative(minimumPadding) ? minimumPadding : 0;
        return Math.Max(minimum, height - inset);
    }

    private static bool IsFiniteNonNegative(double value)
        => double.IsFinite(value) && value >= 0;
}
