namespace QuickLook.Next.Core;

public static class MarkdownViewportPolicy
{
    public const double DefaultContentInset = 24;
    public const int MaximumRealizationAttempts = 3;

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
        bool hasOutline,
        double contentInset = DefaultContentInset,
        double minimumPadding = DefaultContentInset)
    {
        double minimum = IsFiniteNonNegative(minimumPadding) ? minimumPadding : 0;
        if (!hasOutline)
            return minimum;

        double height = IsFiniteNonNegative(viewportHeight) ? viewportHeight : 0;
        double inset = IsFiniteNonNegative(contentInset) ? contentInset : 0;
        return Math.Max(minimum, height - inset);
    }

    public static bool ShouldRetryRealization(
        int completedAttempt,
        bool containerRealized,
        bool renderIsCurrent)
        => renderIsCurrent
            && !containerRealized
            && completedAttempt >= 0
            && completedAttempt + 1 < MaximumRealizationAttempts;

    private static bool IsFiniteNonNegative(double value)
        => double.IsFinite(value) && value >= 0;
}
