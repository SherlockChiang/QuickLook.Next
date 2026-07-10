using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace QuickLook.Next.App;

internal sealed class PreviewPanelController
{
    private readonly FrameworkElement _previewRoot;
    private readonly FrameworkElement _animatedImagePreviewRoot;
    private readonly FrameworkElement _pdfScrollViewer;
    private readonly FrameworkElement _pdfPagerBar;
    private readonly FrameworkElement _textPreviewContainer;
    private readonly FrameworkElement _tableScrollViewer;
    private readonly FrameworkElement _officeScrollViewer;
    private readonly FrameworkElement _mediaPreviewElement;
    private readonly FrameworkElement _listingPanel;
    private readonly FrameworkElement _errorPanel;
    private readonly FrameworkElement _previewInfoRail;
    private readonly FrameworkElement _imagePreviewToolbar;
    private readonly FrameworkElement _imageFilmstrip;
    private readonly Panel _officePagesPanel;

    public PreviewPanelController(
        FrameworkElement previewRoot,
        FrameworkElement animatedImagePreviewRoot,
        FrameworkElement pdfScrollViewer,
        FrameworkElement pdfPagerBar,
        FrameworkElement textPreviewContainer,
        FrameworkElement tableScrollViewer,
        FrameworkElement officeScrollViewer,
        FrameworkElement mediaPreviewElement,
        FrameworkElement listingPanel,
        FrameworkElement errorPanel,
        FrameworkElement previewInfoRail,
        FrameworkElement imagePreviewToolbar,
        FrameworkElement imageFilmstrip,
        Panel officePagesPanel)
    {
        _previewRoot = previewRoot;
        _animatedImagePreviewRoot = animatedImagePreviewRoot;
        _pdfScrollViewer = pdfScrollViewer;
        _pdfPagerBar = pdfPagerBar;
        _textPreviewContainer = textPreviewContainer;
        _tableScrollViewer = tableScrollViewer;
        _officeScrollViewer = officeScrollViewer;
        _mediaPreviewElement = mediaPreviewElement;
        _listingPanel = listingPanel;
        _errorPanel = errorPanel;
        _previewInfoRail = previewInfoRail;
        _imagePreviewToolbar = imagePreviewToolbar;
        _imageFilmstrip = imageFilmstrip;
        _officePagesPanel = officePagesPanel;
    }

    private void HideAllCorePanels()
    {
        _previewRoot.Visibility = Visibility.Collapsed;
        _animatedImagePreviewRoot.Visibility = Visibility.Collapsed;
        _pdfScrollViewer.Visibility = Visibility.Collapsed;
        _pdfPagerBar.Visibility = Visibility.Collapsed;
        _textPreviewContainer.Visibility = Visibility.Collapsed;
        _tableScrollViewer.Visibility = Visibility.Collapsed;
        _officeScrollViewer.Visibility = Visibility.Collapsed;
        _mediaPreviewElement.Visibility = Visibility.Collapsed;
        _listingPanel.Visibility = Visibility.Collapsed;
    }

    public void ShowError()
    {
        HideAllCorePanels();
        _errorPanel.Visibility = Visibility.Visible;
    }

    public void HideError()
    {
        _errorPanel.Visibility = Visibility.Collapsed;
    }

    public void ShowRaster()
    {
        HideAllCorePanels();
        _previewRoot.Visibility = Visibility.Visible;
    }

    public void ShowAnimatedImage()
    {
        HideAllCorePanels();
        _animatedImagePreviewRoot.Visibility = Visibility.Visible;
    }

    public void ShowPdf()
    {
        HideAllCorePanels();
        _pdfScrollViewer.Visibility = Visibility.Visible;
        _pdfPagerBar.Visibility = Visibility.Visible;
    }

    public void ShowText()
    {
        HideAllCorePanels();
        _textPreviewContainer.Visibility = Visibility.Visible;
    }

    public void ShowTable()
    {
        HideAllCorePanels();
        _tableScrollViewer.Visibility = Visibility.Visible;
    }

    public void ShowOffice()
    {
        HideAllCorePanels();
        _officeScrollViewer.Visibility = Visibility.Visible;
    }

    public void ShowMedia()
    {
        HideAllCorePanels();
        _mediaPreviewElement.Visibility = Visibility.Visible;
    }

    public void ShowListing()
    {
        HideAllCorePanels();
        _listingPanel.Visibility = Visibility.Visible;
    }

    public void ToggleRasterTools(bool show, bool showPreviewInfoRail = true)
    {
        _previewInfoRail.Visibility = show && showPreviewInfoRail ? Visibility.Visible : Visibility.Collapsed;
        _imagePreviewToolbar.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
        if (!show)
            _imageFilmstrip.Visibility = Visibility.Collapsed;
    }

    public void ResetChromeVisibility()
    {
        _previewInfoRail.Visibility = Visibility.Collapsed;
        _imagePreviewToolbar.Visibility = Visibility.Collapsed;
        _imageFilmstrip.Visibility = Visibility.Collapsed;
        _previewRoot.Margin = new Thickness(14, 0, 14, 14);
    }

    public void ResetPreviewState()
    {
        ShowRaster(); // Default state before content loads
        ResetChromeVisibility();
        HideError();
        _officePagesPanel.Children.Clear();
    }
}
