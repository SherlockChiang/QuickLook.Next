using System.Diagnostics;
using Xunit;

namespace QuickLook.Next.App;

public sealed class PreviewLifecycleTimingTests
{
    [Fact]
    public void CurrentPhaseTracksOnlyTheLatestAcceptedLifecycleMark()
    {
        var timing = new PreviewLifecycleTiming(
            3,
            PreviewNavigationSource.ExplorerOpen,
            @"C:\Private\report.pdf",
            Stopwatch.GetTimestamp());

        Assert.True(timing.CorrelationId > 0);
        Assert.Equal("preview-begin", timing.CurrentPhase);

        timing.Mark("probe-complete", "kind=pdf");
        Assert.Equal("probe-complete", timing.CurrentPhase);

        timing.Complete("failed");
        Assert.Equal("terminal", timing.CurrentPhase);
        Assert.True(timing.IsTerminal);

        timing.Mark("late-phase");
        Assert.Equal("terminal", timing.CurrentPhase);
    }
}
