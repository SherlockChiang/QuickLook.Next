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

    [Fact]
    public void Explorer_open_preview_activates_for_initial_space_open()
        => AssertFocusPolicy(
            PreviewNavigationSource.ExplorerOpen,
            contentNeedsFocus: true,
            expectedWindowActivation: true,
            expectedContentFocus: true);

    [Fact]
    public void Explorer_open_preview_activates_even_without_focusable_content()
        => AssertFocusPolicy(
            PreviewNavigationSource.ExplorerOpen,
            contentNeedsFocus: false,
            expectedWindowActivation: true,
            expectedContentFocus: true);

    [Fact]
    public void Explorer_switch_preview_keeps_shell_focus_for_follow_up()
        => AssertFocusPolicy(
            PreviewNavigationSource.ExplorerSwitch,
            contentNeedsFocus: true,
            expectedWindowActivation: false,
            expectedContentFocus: false);

    [Fact]
    public void Explorer_switch_preview_never_activates_even_without_focusable_content()
        => AssertFocusPolicy(
            PreviewNavigationSource.ExplorerSwitch,
            contentNeedsFocus: false,
            expectedWindowActivation: false,
            expectedContentFocus: false);

    [Fact]
    public void Window_navigation_preview_can_take_focus()
        => AssertFocusPolicy(
            PreviewNavigationSource.WindowNavigation,
            contentNeedsFocus: true,
            expectedWindowActivation: true,
            expectedContentFocus: true);

    [Fact]
    public void Window_navigation_activation_still_requires_content_focus_need()
        => AssertFocusPolicy(
            PreviewNavigationSource.WindowNavigation,
            contentNeedsFocus: false,
            expectedWindowActivation: false,
            expectedContentFocus: true);

    private static void AssertFocusPolicy(
        PreviewNavigationSource source,
        bool contentNeedsFocus,
        bool expectedWindowActivation,
        bool expectedContentFocus)
    {
        var session = new PreviewSession();
        session.Begin("item.txt", source);

        Assert.Equal(expectedWindowActivation, session.ShouldActivateWindow(contentNeedsFocus));
        Assert.Equal(expectedContentFocus, session.ShouldFocusContent());
    }
}
