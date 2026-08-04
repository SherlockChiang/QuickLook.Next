using Xunit;

namespace QuickLook.Next.App;

public sealed class PreviewSessionTests
{
    [Fact]
    public void FirstOpenFailureBindsActionsToThePendingPath()
    {
        var session = new PreviewSession();
        PreviewSessionSnapshot snapshot = session.Begin("B.txt", PreviewNavigationSource.WindowNavigation);

        Assert.Null(session.CurrentPath);
        Assert.Equal("B.txt", session.ActivePath);
        Assert.True(session.TryBindError(snapshot, canRetry: true, out PreviewErrorContext context));
        Assert.Equal("B.txt", context.Path);
        Assert.Equal("B.txt", session.ErrorActionPath);
        Assert.True(session.IsCurrentError(context));
    }

    [Fact]
    public void EarlyFailureAfterSwitchKeepsCleanupPathButUsesFailingPathForActions()
    {
        var session = new PreviewSession();
        session.CommitPath(session.Begin("A.txt", PreviewNavigationSource.ExplorerOpen).Path);
        PreviewSessionSnapshot snapshot = session.Begin("B.txt", PreviewNavigationSource.ExplorerSwitch);

        Assert.Equal("A.txt", session.CurrentPath);
        Assert.Equal("B.txt", session.PendingPath);
        Assert.True(session.TryBindError(snapshot, canRetry: true, out _));
        Assert.Equal("B.txt", session.ErrorActionPath);
        Assert.False(session.TryGetActiveSnapshot("A.txt", out _));
    }

    [Fact]
    public void OlderGenerationCannotReplaceCurrentErrorContext()
    {
        var session = new PreviewSession();
        PreviewSessionSnapshot oldSnapshot = session.Begin("A.txt", PreviewNavigationSource.ExplorerOpen);
        PreviewSessionSnapshot currentSnapshot = session.Begin("B.txt", PreviewNavigationSource.ExplorerSwitch);

        Assert.True(session.TryBindError(currentSnapshot, canRetry: false, out PreviewErrorContext current));
        Assert.False(session.TryBindError(oldSnapshot, canRetry: true, out _));
        Assert.True(session.IsCurrentError(current));
        Assert.Equal("B.txt", session.ErrorActionPath);
    }

    [Fact]
    public void SamePathFromOlderGenerationIsStillRejected()
    {
        var session = new PreviewSession();
        PreviewSessionSnapshot oldSnapshot = session.Begin("same.txt", PreviewNavigationSource.ExplorerOpen);
        Assert.True(session.TryBindError(oldSnapshot, canRetry: true, out PreviewErrorContext oldContext));

        PreviewSessionSnapshot currentSnapshot = session.Begin("same.txt", PreviewNavigationSource.WindowNavigation);

        Assert.False(session.TryBindError(oldSnapshot, canRetry: true, out _));
        Assert.False(session.IsCurrentError(oldContext));
        Assert.True(session.TryBindError(currentSnapshot, canRetry: false, out _));
    }

    [Fact]
    public void CommittingTheFailingPathKeepsItsErrorContextValid()
    {
        var session = new PreviewSession();
        PreviewSessionSnapshot snapshot = session.Begin("B.txt", PreviewNavigationSource.WindowNavigation);
        Assert.True(session.TryBindError(snapshot, canRetry: true, out PreviewErrorContext context));

        session.CommitPath("B.txt");

        Assert.True(session.IsCurrentError(context));
        Assert.Equal("B.txt", session.ErrorActionPath);
    }

    [Fact]
    public void NavigationCloseAndClearInvalidateErrorActions()
    {
        var session = new PreviewSession();
        PreviewSessionSnapshot snapshot = session.Begin("B.txt", PreviewNavigationSource.WindowNavigation);
        Assert.True(session.TryBindError(snapshot, canRetry: true, out PreviewErrorContext context));

        session.BeginClose();
        Assert.Null(session.ErrorContext);
        Assert.Null(session.ErrorActionPath);
        Assert.False(session.IsCurrentError(context));

        snapshot = session.Begin("C.txt", PreviewNavigationSource.WindowNavigation);
        Assert.True(session.TryBindError(snapshot, canRetry: false, out _));
        session.Clear();
        Assert.Null(session.ErrorContext);
        Assert.Null(session.ErrorActionPath);
    }
}
