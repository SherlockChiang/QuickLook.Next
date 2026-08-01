namespace QuickLook.Next.Core;

/// <summary>
/// Resolves the animation frame that should be visible at a monotonic elapsed time.
/// The caller can sample the timeline after a delayed render without accumulating drift.
/// </summary>
public sealed class AnimationPlaybackTimeline
{
    private readonly int[] _frameEndMilliseconds;

    public AnimationPlaybackTimeline(ReadOnlySpan<int> frameDelayMilliseconds)
    {
        if (frameDelayMilliseconds.IsEmpty)
            throw new ArgumentException("At least one frame delay is required.", nameof(frameDelayMilliseconds));

        _frameEndMilliseconds = new int[frameDelayMilliseconds.Length];
        int total = 0;
        for (int i = 0; i < frameDelayMilliseconds.Length; i++)
        {
            int delay = frameDelayMilliseconds[i];
            if (delay <= 0)
                throw new ArgumentOutOfRangeException(nameof(frameDelayMilliseconds), "Frame delays must be positive.");

            total = checked(total + delay);
            _frameEndMilliseconds[i] = total;
        }

        DurationMilliseconds = total;
    }

    public int FrameCount => _frameEndMilliseconds.Length;

    public int DurationMilliseconds { get; }

    public int GetFrameIndex(long elapsedMilliseconds)
    {
        if (elapsedMilliseconds < 0)
            throw new ArgumentOutOfRangeException(nameof(elapsedMilliseconds));

        int position = (int)(elapsedMilliseconds % DurationMilliseconds);
        int index = Array.BinarySearch(_frameEndMilliseconds, position + 1);
        return index >= 0 ? index : ~index;
    }
}
