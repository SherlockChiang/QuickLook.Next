using QuickLook.Next.Core;
using Xunit;

namespace QuickLook.Next.Core.Tests;

public sealed class AnimationPlaybackTimelineTests
{
    [Fact]
    public void GetFrameIndex_UsesHalfOpenFrameIntervalsAndLoops()
    {
        var timeline = new AnimationPlaybackTimeline([40, 60, 100]);

        Assert.Equal(3, timeline.FrameCount);
        Assert.Equal(200, timeline.DurationMilliseconds);
        Assert.Equal(0, timeline.GetFrameIndex(0));
        Assert.Equal(0, timeline.GetFrameIndex(39));
        Assert.Equal(1, timeline.GetFrameIndex(40));
        Assert.Equal(1, timeline.GetFrameIndex(99));
        Assert.Equal(2, timeline.GetFrameIndex(100));
        Assert.Equal(2, timeline.GetFrameIndex(199));
        Assert.Equal(0, timeline.GetFrameIndex(200));
        Assert.Equal(1, timeline.GetFrameIndex(240));
    }

    [Fact]
    public void GetFrameIndex_CatchesUpAfterDelayedRender()
    {
        var timeline = new AnimationPlaybackTimeline([30, 30, 30, 30]);

        Assert.Equal(1, timeline.GetFrameIndex(31));
        Assert.Equal(3, timeline.GetFrameIndex(119));
        Assert.Equal(0, timeline.GetFrameIndex(120));
        Assert.Equal(2, timeline.GetFrameIndex(190));
    }

    [Fact]
    public void GetFrameIndex_HandlesMinimumAndMaximumFrameDelays()
    {
        var timeline = new AnimationPlaybackTimeline([20, 1_000]);

        Assert.Equal(0, timeline.GetFrameIndex(19));
        Assert.Equal(1, timeline.GetFrameIndex(20));
        Assert.Equal(1, timeline.GetFrameIndex(1_019));
        Assert.Equal(0, timeline.GetFrameIndex(1_020));
    }

    [Fact]
    public void GetFrameIndex_SupportsElapsedTimesBeyondIntRange()
    {
        var timeline = new AnimationPlaybackTimeline([20, 30]);
        long elapsed = (long)int.MaxValue + 24;

        Assert.Equal(timeline.GetFrameIndex(elapsed % timeline.DurationMilliseconds), timeline.GetFrameIndex(elapsed));
    }

    [Fact]
    public void Constructor_RejectsMissingOrInvalidDelays()
    {
        Assert.Throws<ArgumentException>(() => new AnimationPlaybackTimeline([]));
        Assert.Throws<ArgumentOutOfRangeException>(() => new AnimationPlaybackTimeline([20, 0]));
        Assert.Throws<ArgumentOutOfRangeException>(() => new AnimationPlaybackTimeline([-1]));
        Assert.Throws<OverflowException>(() => new AnimationPlaybackTimeline([int.MaxValue, 1]));
    }

    [Fact]
    public void GetFrameIndex_RejectsNegativeElapsedTime()
    {
        var timeline = new AnimationPlaybackTimeline([20]);

        Assert.Throws<ArgumentOutOfRangeException>(() => timeline.GetFrameIndex(-1));
    }
}
