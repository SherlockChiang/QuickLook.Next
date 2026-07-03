using System.Numerics;
using Microsoft.UI.Composition;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class RasterPreviewPresenter
{
    private const double MinImageZoom = 0.1;
    private const double MaxImageZoom = 12.0;
    private const double InfoRailWidth = 246;
    private const double ToolbarHeight = 82;

    private readonly Border _previewRoot;
    private readonly TextBlock _zoomText;
    private SpriteVisual? _sprite;
    private uint _surfaceWidth;
    private uint _surfaceHeight;
    private double _zoom = 1.0;
    private double _panX;
    private double _panY;
    private bool _isPanning;
    private Windows.Foundation.Point _panStart;
    private double _panStartX;
    private double _panStartY;

    public RasterPreviewPresenter(Border previewRoot, TextBlock zoomText)
    {
        _previewRoot = previewRoot;
        _zoomText = zoomText;
    }

    public bool HasSurface => _sprite is not null;

    public RasterPreviewResult Render(PreviewReady ready, (double Width, double Height) maxContent)
    {
        ResetView();

        double w = ready.PreferredWidth;
        double h = ready.PreferredHeight;
        double width;
        double height;
        if (w > 0 && h > 0)
        {
            double imageMaxWidth = Math.Max(1, maxContent.Width - InfoRailWidth);
            double imageMaxHeight = Math.Max(1, maxContent.Height - ToolbarHeight);
            double scale = Math.Min(1.0, Math.Min(imageMaxWidth / w, imageMaxHeight / h));
            width = w * scale + InfoRailWidth;
            height = h * scale + ToolbarHeight;
        }
        else
        {
            width = w + InfoRailWidth;
            height = h + ToolbarHeight;
        }

        return new RasterPreviewResult($"{ready.Kind}: {ready.Title}", width, height);
    }

    public bool AttachSurface(Compositor compositor, PreviewSurface surface, out string? error)
    {
        var (compSurface, hr) = CompositionInterop.CreateSurfaceForHandle(compositor, (nint)surface.SharedHandle);
        if (hr < 0 || compSurface is null)
        {
            error = $"surface failed 0x{hr:X8}";
            return false;
        }

        DisposeSprite();

        var brush = compositor.CreateSurfaceBrush(compSurface);
        brush.Stretch = CompositionStretch.Fill;
        var sprite = compositor.CreateSpriteVisual();
        sprite.Brush = brush;
        _sprite = sprite;
        _surfaceWidth = surface.Width;
        _surfaceHeight = surface.Height;
        ElementCompositionPreview.SetElementChildVisual(_previewRoot, sprite);
        error = null;
        return true;
    }

    public void Clear()
    {
        DisposeSprite();
        ElementCompositionPreview.SetElementChildVisual(_previewRoot, null);
        _surfaceWidth = 0;
        _surfaceHeight = 0;
        ResetView();
    }

    public void UpdateLayout()
    {
        if (_sprite is null || _surfaceWidth == 0 || _surfaceHeight == 0)
            return;

        double availableWidth = Math.Max(1, _previewRoot.ActualWidth);
        double availableHeight = Math.Max(1, _previewRoot.ActualHeight);
        double fitScale = Math.Min(availableWidth / _surfaceWidth, availableHeight / _surfaceHeight);
        double scale = fitScale * _zoom;
        double scaledWidth = _surfaceWidth * scale;
        double scaledHeight = _surfaceHeight * scale;

        double maxPanX = Math.Max(0, (scaledWidth - availableWidth) / 2);
        double maxPanY = Math.Max(0, (scaledHeight - availableHeight) / 2);
        _panX = Math.Clamp(_panX, -maxPanX, maxPanX);
        _panY = Math.Clamp(_panY, -maxPanY, maxPanY);

        _sprite.Size = new Vector2(_surfaceWidth, _surfaceHeight);
        _sprite.Scale = new Vector3((float)scale, (float)scale, 1f);
        _sprite.Offset = new Vector3(
            (float)Math.Round((availableWidth - scaledWidth) / 2 + _panX),
            (float)Math.Round((availableHeight - scaledHeight) / 2 + _panY),
            0);
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
        if (_sprite is null) return;
        _zoom = Math.Clamp(_zoom * factor, MinImageZoom, MaxImageZoom);
        UpdateLayout();
    }

    public void SetZoom(double zoom)
    {
        if (_sprite is null) return;
        _zoom = Math.Clamp(zoom, MinImageZoom, MaxImageZoom);
        _panX = 0;
        _panY = 0;
        UpdateLayout();
    }

    public void OnPointerPressed(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (_sprite is null || _previewRoot.Visibility != Visibility.Visible) return;
        if (!e.GetCurrentPoint(_previewRoot).Properties.IsLeftButtonPressed) return;
        _isPanning = true;
        _panStart = e.GetCurrentPoint(_previewRoot).Position;
        _panStartX = _panX;
        _panStartY = _panY;
        _previewRoot.CapturePointer(e.Pointer);
    }

    public void OnPointerMoved(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (!_isPanning) return;
        var p = e.GetCurrentPoint(_previewRoot).Position;
        _panX = _panStartX + (p.X - _panStart.X);
        _panY = _panStartY + (p.Y - _panStart.Y);
        UpdateLayout();
    }

    public void OnPointerReleased(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (!_isPanning) return;
        _isPanning = false;
        _previewRoot.ReleasePointerCapture(e.Pointer);
    }

    public void OnPointerWheelChanged(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (_sprite is null || _previewRoot.Visibility != Visibility.Visible)
            return;

        int delta = e.GetCurrentPoint(_previewRoot).Properties.MouseWheelDelta;
        if (delta == 0) return;

        ZoomBy(delta > 0 ? 1.15 : 1.0 / 1.15);
        e.Handled = true;
    }

    public void OnDoubleTapped(Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
    {
        if (_sprite is null || _previewRoot.Visibility != Visibility.Visible)
            return;

        ResetView();
        e.Handled = true;
    }

    private void DisposeSprite()
    {
        if (_sprite is null)
            return;

        try { (_sprite.Brush as IDisposable)?.Dispose(); } catch { }
        try { _sprite.Dispose(); } catch { }
        _sprite = null;
    }
}

internal readonly record struct RasterPreviewResult(string Status, double Width, double Height);
