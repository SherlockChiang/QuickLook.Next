using Xunit;

namespace QuickLook.Next.App;

public sealed class PreviewNavigationPolicyTests
{
    [Theory]
    [InlineData((int)PreviewNavigationKey.Left)]
    [InlineData((int)PreviewNavigationKey.Up)]
    [InlineData((int)PreviewNavigationKey.PageUp)]
    public void Pdf_previous_keys_move_by_one_page(int keyValue)
        => Assert.Equal(
            PreviewNavigationAction.PreviousPdfPage,
            PreviewNavigationPolicy.Resolve(
                pdfVisible: true,
                imageVisible: false,
                modifierDown: false,
                (PreviewNavigationKey)keyValue));

    [Theory]
    [InlineData((int)PreviewNavigationKey.Right)]
    [InlineData((int)PreviewNavigationKey.Down)]
    [InlineData((int)PreviewNavigationKey.PageDown)]
    public void Pdf_next_keys_move_by_one_page(int keyValue)
        => Assert.Equal(
            PreviewNavigationAction.NextPdfPage,
            PreviewNavigationPolicy.Resolve(
                pdfVisible: true,
                imageVisible: false,
                modifierDown: false,
                (PreviewNavigationKey)keyValue));

    [Theory]
    [InlineData((int)PreviewNavigationKey.Left, (int)PreviewNavigationAction.PreviousImage)]
    [InlineData((int)PreviewNavigationKey.Right, (int)PreviewNavigationAction.NextImage)]
    public void Image_horizontal_keys_move_between_siblings(
        int keyValue,
        int expectedValue)
        => Assert.Equal(
            (PreviewNavigationAction)expectedValue,
            PreviewNavigationPolicy.Resolve(
                pdfVisible: false,
                imageVisible: true,
                modifierDown: false,
                (PreviewNavigationKey)keyValue));

    [Fact]
    public void Pdf_navigation_has_priority_when_transient_image_state_is_still_present()
        => Assert.Equal(
            PreviewNavigationAction.NextPdfPage,
            PreviewNavigationPolicy.Resolve(
                pdfVisible: true,
                imageVisible: true,
                modifierDown: false,
                PreviewNavigationKey.Right));

    [Theory]
    [InlineData(true, false, (int)PreviewNavigationKey.Right)]
    [InlineData(false, true, (int)PreviewNavigationKey.Left)]
    public void Modifier_chords_remain_owned_by_zoom_pan_and_controls(
        bool pdfVisible,
        bool imageVisible,
        int keyValue)
        => Assert.Equal(
            PreviewNavigationAction.None,
            PreviewNavigationPolicy.Resolve(
                pdfVisible,
                imageVisible,
                modifierDown: true,
                (PreviewNavigationKey)keyValue));

    [Fact]
    public void Hidden_previews_do_not_consume_navigation_keys()
        => Assert.Equal(
            PreviewNavigationAction.None,
            PreviewNavigationPolicy.Resolve(
                pdfVisible: false,
                imageVisible: false,
                modifierDown: false,
                PreviewNavigationKey.Right));
}
