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

    [Fact]
    public void Cancellation_records_a_request_phase_before_terminal_outcome()
    {
        var timing = new PreviewLifecycleTiming(
            4,
            PreviewNavigationSource.ExplorerSwitch,
            @"C:\Private\old.png",
            Stopwatch.GetTimestamp());

        timing.Cancel("superseded");

        Assert.Equal("terminal", timing.CurrentPhase);
        Assert.True(timing.IsTerminal);
    }
}
