using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class AnimatedImagePreviewPresenter
{
    private const double MinZoom = 0.1;
    private const double MaxZoom = 12.0;
    private const double InfoRailWidth = 246;
    private const double ToolbarHeight = 162;

    private readonly Border _previewRoot;
    private readonly Image _image;
    private readonly TextBlock _zoomText;
    private readonly CompositeTransform _transform = new();
    private double _sourceWidth;
    private double _sourceHeight;
    private double _zoom = 1.0;
    private double _panX;
    private double _panY;
    private bool _isPanning;
    private Windows.Foundation.Point _panStart;
    private double _panStartX;
    private double _panStartY;

    public AnimatedImagePreviewPresenter(Border previewRoot, Image image, TextBlock zoomText)
    {
        _previewRoot = previewRoot;
        _image = image;
        _zoomText = zoomText;
        _image.RenderTransform = _transform;
    }

    public bool HasImage => _image.Source is not null;

    public AnimatedImagePreviewResult Render(string path, PreviewReady ready, (double Width, double Height) maxContent)
    {
        ResetView();
        _sourceWidth = Math.Max(1, ready.PreferredWidth);
        _sourceHeight = Math.Max(1, ready.PreferredHeight);
        _image.Width = _sourceWidth;
        _image.Height = _sourceHeight;
        _image.Source = new BitmapImage(new Uri(path));

        double imageMaxWidth = Math.Max(1, maxContent.Width - InfoRailWidth);
        double imageMaxHeight = Math.Max(1, maxContent.Height - ToolbarHeight);
        double scale = Math.Min(1.0, Math.Min(imageMaxWidth / _sourceWidth, imageMaxHeight / _sourceHeight));
        double width = _sourceWidth * scale + InfoRailWidth;
        double height = _sourceHeight * scale + ToolbarHeight;
        return new AnimatedImagePreviewResult($"{ready.Kind}: {ready.Title}", width, height);
    }

    public void Clear()
    {
        _image.Source = null;
        _sourceWidth = 0;
        _sourceHeight = 0;
        ResetView();
    }

    public void UpdateLayout()
    {
        if (_image.Source is null || _sourceWidth <= 0 || _sourceHeight <= 0)
            return;

        double availableWidth = Math.Max(1, _previewRoot.ActualWidth);
        double availableHeight = Math.Max(1, _previewRoot.ActualHeight);
        double fitScale = Math.Min(availableWidth / _sourceWidth, availableHeight / _sourceHeight);
        double scale = fitScale * _zoom;
        double scaledWidth = _sourceWidth * scale;
        double scaledHeight = _sourceHeight * scale;

        double maxPanX = Math.Max(0, (scaledWidth - availableWidth) / 2);
        double maxPanY = Math.Max(0, (scaledHeight - availableHeight) / 2);
        _panX = Math.Clamp(_panX, -maxPanX, maxPanX);
        _panY = Math.Clamp(_panY, -maxPanY, maxPanY);

        _transform.ScaleX = scale;
        _transform.ScaleY = scale;
        _transform.TranslateX = Math.Round((availableWidth - scaledWidth) / 2 + _panX);
        _transform.TranslateY = Math.Round((availableHeight - scaledHeight) / 2 + _panY);
        UpdateZoomLabel();
    }

    public void UpdateZoomLabel()
        => _zoomText.Text = Math.Abs(_zoom - 1.0) < 0.01 ? UiStrings.FitZoom : $"{_zoom * 100:0}%";

    public void ResetView()
    {
        _zoom = 1.0;
        _panX = 0;
        _panY = 0;
        UpdateLayout();
        UpdateZoomLabel();
    }

    public void ZoomBy(double factor)
    {
        if (_image.Source is null)
            return;
        _zoom = Math.Clamp(_zoom * factor, MinZoom, MaxZoom);
        UpdateLayout();
    }

    public void SetZoom(double zoom)
    {
        if (_image.Source is null)
            return;
        _zoom = Math.Clamp(zoom, MinZoom, MaxZoom);
        _panX = 0;
        _panY = 0;
        UpdateLayout();
    }

    public void OnPointerPressed(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (_image.Source is null || _previewRoot.Visibility != Visibility.Visible)
            return;
        if (!e.GetCurrentPoint(_previewRoot).Properties.IsLeftButtonPressed)
            return;
        _isPanning = true;
        _panStart = e.GetCurrentPoint(_previewRoot).Position;
        _panStartX = _panX;
        _panStartY = _panY;
        _previewRoot.CapturePointer(e.Pointer);
    }

    public void OnPointerMoved(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (!_isPanning)
            return;
        var point = e.GetCurrentPoint(_previewRoot).Position;
        _panX = _panStartX + (point.X - _panStart.X);
        _panY = _panStartY + (point.Y - _panStart.Y);
        UpdateLayout();
    }

    public void OnPointerReleased(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (!_isPanning)
            return;
        _isPanning = false;
        _previewRoot.ReleasePointerCapture(e.Pointer);
    }

    public void OnPointerWheelChanged(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (_image.Source is null || _previewRoot.Visibility != Visibility.Visible)
            return;

        int delta = e.GetCurrentPoint(_previewRoot).Properties.MouseWheelDelta;
        if (delta == 0)
            return;

        ZoomBy(delta > 0 ? 1.15 : 1.0 / 1.15);
        e.Handled = true;
    }

    public void OnDoubleTapped(Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
    {
        if (_image.Source is null || _previewRoot.Visibility != Visibility.Visible)
            return;

        ResetView();
        e.Handled = true;
    }

    public static (int Width, int Height)? TryReadGifSize(string path)
    {
        try
        {
            Span<byte> header = stackalloc byte[10];
            using var stream = File.OpenRead(path);
            if (stream.Read(header) != header.Length)
                return null;
            bool gif = header[..6].SequenceEqual("GIF87a"u8) || header[..6].SequenceEqual("GIF89a"u8);
            if (!gif)
                return null;
            int width = BitConverter.ToUInt16(header[6..8]);
            int height = BitConverter.ToUInt16(header[8..10]);
            return width > 0 && height > 0 ? (width, height) : null;
        }
        catch
        {
            return null;
        }
    }
}

internal readonly record struct AnimatedImagePreviewResult(string Status, double Width, double Height);
