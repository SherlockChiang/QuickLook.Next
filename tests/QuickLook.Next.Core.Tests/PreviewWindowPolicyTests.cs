using Xunit;

namespace QuickLook.Next.App;

public sealed class PreviewWindowPolicyTests
{
    [Theory]
    [InlineData("standard", (int)PreviewWindowLevel.Standard)]
    [InlineData("always-on-top", (int)PreviewWindowLevel.AlwaysOnTop)]
    public void Supported_window_levels_parse_deterministically(string setting, int expectedLevel)
    {
        Assert.True(PreviewWindowPolicy.IsSupportedLevel(setting));
        Assert.Equal((PreviewWindowLevel)expectedLevel, PreviewWindowPolicy.ParseLevel(setting));
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("topmost")]
    public void Invalid_window_levels_fail_validation_and_parse_to_standard(string? setting)
    {
        Assert.False(PreviewWindowPolicy.IsSupportedLevel(setting));
        Assert.Equal(PreviewWindowLevel.Standard, PreviewWindowPolicy.ParseLevel(setting));
    }

    [Fact]
    public void Focus_loss_closes_only_a_visible_inactive_preview_when_enabled()
        => Assert.True(
            PreviewWindowPolicy.ShouldCloseAfterDeactivation(
                closeOnFocusLoss: true,
                previewVisible: true,
                windowActivated: false,
                modalDialogOpen: false,
                ownedWindowOpen: false));

    [Theory]
    [InlineData(false, true, false, false, false)]
    [InlineData(true, false, false, false, false)]
    [InlineData(true, true, true, false, false)]
    [InlineData(true, true, false, true, false)]
    [InlineData(true, true, false, false, true)]
    public void Focus_loss_close_is_suppressed_for_nonterminal_activation_states(
        bool closeOnFocusLoss,
        bool previewVisible,
        bool windowActivated,
        bool modalDialogOpen,
        bool ownedWindowOpen)
        => Assert.False(
            PreviewWindowPolicy.ShouldCloseAfterDeactivation(
                closeOnFocusLoss,
                previewVisible,
                windowActivated,
                modalDialogOpen,
                ownedWindowOpen));
}
