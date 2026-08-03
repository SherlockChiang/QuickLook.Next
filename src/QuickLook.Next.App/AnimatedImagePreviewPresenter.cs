using System.Diagnostics;
using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Foundation;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class AnimatedImagePreviewPresenter
{
    private const double MinZoom = 0.1;
    private const double MaxZoom = 12.0;
    private const double InfoRailWidth = 246;
    private const double ToolbarHeight = 162;
    private const int WaveformUpdateIntervalMilliseconds = 100;

    private readonly Border _previewRoot;
    private readonly Image _image;
    private readonly TextBlock _zoomText;
    private readonly CompositeTransform _transform = new();
    private readonly RectangleGeometry _clip = new();
    private double _sourceWidth;
    private double _sourceHeight;
    private double _zoom = 1.0;
    private double _panX;
    private double _panY;
    private bool _isPanning;
    private Windows.Foundation.Point _panStart;
    private double _panStartX;
    private double _panStartY;
    private int _layoutVersion;
    private NativeAnimationFrames? _nativeFrames;
    private WriteableBitmap? _nativeFrameBitmap;
    private int _nativeFrameIndex;
    private AnimationPlaybackTimeline? _nativeFrameTimeline;
    private Stopwatch? _nativeFrameClock;
    private long _nativePlaybackOffsetMilliseconds;
    private bool _nativeRenderingSubscribed;
    private bool _nativeWaveformEnabled;
    private long _lastWaveformUpdateMilliseconds;
    private bool _waveformUpdatePending;
    private int _waveformVersion;
    private Stopwatch? _openWatch;
    private string _currentPath = "";

    public AnimatedImagePreviewPresenter(Border previewRoot, Image image, TextBlock zoomText)
    {
        _previewRoot = previewRoot;
        _image = image;
        _zoomText = zoomText;
        _image.Stretch = Stretch.Fill;
        _image.RenderTransform = _transform;
        _previewRoot.Clip = _clip;
        _previewRoot.Loaded += (_, _) => ScheduleLayoutUpdate();
        _image.ImageOpened += (_, _) =>
        {
            if (_openWatch is { } watch)
            {
                watch.Stop();
                DiagLog.Write("App", $"animated image opened {watch.ElapsedMilliseconds}ms; path={_currentPath}");
                _openWatch = null;
            }
            SyncDecodedImageSize();
            ScheduleLayoutUpdate();
        };
    }

    public bool HasImage => _image.Source is not null;
    public bool CanTogglePlayback => _nativeFrames?.FrameCount > 1 && _nativeFrameClock is not null;
    public bool IsPlaybackPaused { get; private set; }
    public Action<ImageWaveform>? WaveformChanged { get; init; }

    public AnimatedImagePreviewResult RenderNativeFrames(
        string path,
        PreviewReady ready,
        NativeAnimationFrames frames,
        (double Width, double Height) maxContent,
        bool enableWaveform,
        long initialElapsedMilliseconds = 0)
    {
        StopNativePlayback();
        _nativeFrames = frames;
        try
        {
            _layoutVersion++;
            _currentPath = path;
            _openWatch = null;
            _waveformVersion++;
            _waveformUpdatePending = false;
            _nativeWaveformEnabled = enableWaveform
                && !Path.GetExtension(path).Equals(".gif", StringComparison.OrdinalIgnoreCase);
            IsPlaybackPaused = false;
            _nativeFrameBitmap = new WriteableBitmap(frames.Width, frames.Height);
            _nativeFrameTimeline = BuildFrameTimeline(frames);
            _nativePlaybackOffsetMilliseconds = Math.Max(0, initialElapsedMilliseconds);
            if (frames.FrameCount > 1)
                _nativeFrameClock = Stopwatch.StartNew();
            _nativeFrameIndex = _nativeFrameTimeline.GetFrameIndex(
                _nativePlaybackOffsetMilliseconds);
            _sourceWidth = Math.Max(1, frames.Width);
            _sourceHeight = Math.Max(1, frames.Height);
            _lastWaveformUpdateMilliseconds = 0;
            _image.Width = frames.Width;
            _image.Height = frames.Height;
            double imageMaxWidth = Math.Max(1, maxContent.Width - InfoRailWidth);
            double imageMaxHeight = Math.Max(1, maxContent.Height - ToolbarHeight);
            double scale = Math.Min(1.0, Math.Min(imageMaxWidth / frames.Width, imageMaxHeight / frames.Height));
            PresentNativeFrame(_nativeFrameIndex);
            ResetView();
            ScheduleLayoutUpdate();

            if (frames.FrameCount > 1)
            {
                SubscribeNativeRendering();
            }

            double width = frames.Width * scale + InfoRailWidth;
            double height = frames.Height * scale + ToolbarHeight;
            return new AnimatedImagePreviewResult($"{ready.Kind}: {ready.Title}", width, height);
        }
        catch
        {
            StopNativePlayback();
            throw;
        }
    }

    public void Clear()
    {
        _layoutVersion++;
        StopNativePlayback();
        _openWatch = null;
        _currentPath = "";
        _image.Source = null;
        _sourceWidth = 0;
        _sourceHeight = 0;
        ResetView();
    }

    public void TogglePlayback()
    {
        if (!CanTogglePlayback || _nativeFrameClock is null)
            return;

        if (IsPlaybackPaused)
        {
            _nativeFrameClock.Start();
            SubscribeNativeRendering();
            IsPlaybackPaused = false;
            AdvanceNativeFrame();
        }
        else
        {
            UnsubscribeNativeRendering();
            _nativeFrameClock.Stop();
            IsPlaybackPaused = true;
        }
    }

    public void PausePlayback()
    {
        if (CanTogglePlayback && !IsPlaybackPaused)
            TogglePlayback();
    }

    public void UpdateLayout()
    {
        if (_image.Source is null || _sourceWidth <= 0 || _sourceHeight <= 0)
            return;

        double availableWidth = _previewRoot.ActualWidth;
        double availableHeight = _previewRoot.ActualHeight;
        if (availableWidth <= 1 || availableHeight <= 1)
            return;

        _clip.Rect = new Rect(0, 0, availableWidth, availableHeight);
        double fitScale = Math.Min(1.0, Math.Min(availableWidth / _sourceWidth, availableHeight / _sourceHeight));
        double scale = fitScale * _zoom;
        double scaledWidth = _sourceWidth * scale;
        double scaledHeight = _sourceHeight * scale;

        double maxPanX = Math.Max(0, (scaledWidth - availableWidth) / 2);
        double maxPanY = Math.Max(0, (scaledHeight - availableHeight) / 2);
        _panX = Math.Clamp(_panX, -maxPanX, maxPanX);
        _panY = Math.Clamp(_panY, -maxPanY, maxPanY);

        _image.Width = scaledWidth;
        _image.Height = scaledHeight;
        _image.Stretch = Stretch.Fill;
        _transform.ScaleX = 1;
        _transform.ScaleY = 1;
        _transform.TranslateX = Math.Round(_panX);
        _transform.TranslateY = Math.Round(_panY);
        UpdateZoomLabel();
    }

    private void AdvanceNativeFrame()
    {
        if (_nativeFrames is null || _nativeFrames.FrameCount == 0)
            return;

        if (_nativeFrameTimeline is null || _nativeFrameClock is null)
            return;

        int frameIndex = _nativeFrameTimeline.GetFrameIndex(GetPlaybackElapsedMilliseconds());
        if (frameIndex != _nativeFrameIndex)
        {
            _nativeFrameIndex = frameIndex;
            PresentNativeFrame(_nativeFrameIndex);
        }
    }

    private void OnNativeFrameRendering(object? sender, object e)
        => AdvanceNativeFrame();

    private void SubscribeNativeRendering()
    {
        if (_nativeRenderingSubscribed)
            return;

        CompositionTarget.Rendering += OnNativeFrameRendering;
        _nativeRenderingSubscribed = true;
    }

    private void UnsubscribeNativeRendering()
    {
        if (!_nativeRenderingSubscribed)
            return;

        CompositionTarget.Rendering -= OnNativeFrameRendering;
        _nativeRenderingSubscribed = false;
    }

    private void PresentNativeFrame(int index)
    {
        NativeAnimationFrames? frames = _nativeFrames;
        if (frames is null || index < 0 || index >= frames.FrameCount)
            return;

        if (_nativeFrameBitmap is null)
            _nativeFrameBitmap = new WriteableBitmap(frames.Width, frames.Height);

        // PixelBuffer is a fixed-size WinRT buffer. Resizing it (SetLength) can throw a
        // COMException, and it must be unmapped before Invalidate asks XAML to consume it.
        using (var stream = _nativeFrameBitmap.PixelBuffer.AsStream())
        {
            stream.Position = 0;
            if (!frames.TryWriteFrame(index, stream))
                return;
        }
        _nativeFrameBitmap.Invalidate();
        if (!ReferenceEquals(_image.Source, _nativeFrameBitmap))
            _image.Source = _nativeFrameBitmap;

        long elapsed = GetPlaybackElapsedMilliseconds();
        if (_nativeWaveformEnabled
            && _nativeFrameClock is not null
            && elapsed - _lastWaveformUpdateMilliseconds >= WaveformUpdateIntervalMilliseconds)
        {
            _lastWaveformUpdateMilliseconds = elapsed;
            QueueWaveformUpdate(frames, index);
        }
    }

    private void QueueWaveformUpdate(NativeAnimationFrames frames, int frameIndex)
    {
        if (!_nativeWaveformEnabled || _waveformUpdatePending || WaveformChanged is null)
            return;

        _waveformUpdatePending = true;
        int version = _waveformVersion;
        _ = Task.Run(() => frames.CreateWaveform(frameIndex)).ContinueWith(task =>
        {
            _previewRoot.DispatcherQueue.TryEnqueue(() =>
            {
                if (version != _waveformVersion)
                    return;
                _waveformUpdatePending = false;
                if (_nativeWaveformEnabled && task.IsCompletedSuccessfully && task.Result is { } waveform)
                    WaveformChanged?.Invoke(waveform);
            });
        }, TaskScheduler.Default);
    }

    private void StopNativePlayback()
    {
        UnsubscribeNativeRendering();
        NativeAnimationFrames? frames = _nativeFrames;
        _nativeFrames = null;
        _waveformVersion++;
        _waveformUpdatePending = false;
        _nativeWaveformEnabled = false;
        frames?.Dispose();
        _nativeFrameBitmap = null;
        _nativeFrameIndex = 0;
        _nativeFrameTimeline = null;
        _nativeFrameClock = null;
        _nativePlaybackOffsetMilliseconds = 0;
        _lastWaveformUpdateMilliseconds = 0;
        IsPlaybackPaused = false;
    }

    private long GetPlaybackElapsedMilliseconds()
    {
        long elapsed = _nativeFrameClock?.ElapsedMilliseconds ?? 0;
        return _nativePlaybackOffsetMilliseconds > long.MaxValue - elapsed
            ? long.MaxValue
            : _nativePlaybackOffsetMilliseconds + elapsed;
    }

    private static AnimationPlaybackTimeline BuildFrameTimeline(NativeAnimationFrames frames)
    {
        var delays = new int[frames.FrameCount];
        for (int i = 0; i < frames.FrameCount; i++)
            delays[i] = Math.Clamp(frames.GetDelayMilliseconds(i), 20, 1_000);
        return new AnimationPlaybackTimeline(delays);
    }

    public void ScheduleLayoutUpdate()
    {
        if (_image.Source is null)
            return;

        int version = _layoutVersion;
        _previewRoot.DispatcherQueue.TryEnqueue(() =>
        {
            if (version != _layoutVersion)
                return;

            var layoutWatch = Stopwatch.StartNew();
            UpdateLayout();
            layoutWatch.Stop();
            DiagLog.Write("App", $"animated image layout apply {layoutWatch.ElapsedMilliseconds}ms; path={_currentPath}");
            QueueDelayedLayoutUpdate(50, version);
            QueueDelayedLayoutUpdate(150, version);
        });
    }

    private void QueueDelayedLayoutUpdate(int delayMs, int version)
    {
        _ = Task.Delay(delayMs).ContinueWith(_ =>
        {
            _previewRoot.DispatcherQueue.TryEnqueue(() =>
            {
                if (version == _layoutVersion)
                    UpdateLayout();
            });
        }, TaskScheduler.Default);
    }

    public void UpdateZoomLabel()
    {
        if (Math.Abs(_zoom - 1.0) < 0.01)
        {
            _zoomText.Text = UiStrings.FitZoom;
            return;
        }

        _zoomText.Text = $"{ActualScale() * 100:0}%";
    }

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

    private void ZoomAt(double factor, Windows.Foundation.Point point)
    {
        if (_image.Source is null)
            return;

        double previousZoom = _zoom;
        _zoom = Math.Clamp(_zoom * factor, MinZoom, MaxZoom);
        double appliedFactor = _zoom / previousZoom;
        double centerX = _previewRoot.ActualWidth / 2;
        double centerY = _previewRoot.ActualHeight / 2;
        _panX = (point.X - centerX) * (1 - appliedFactor) + _panX * appliedFactor;
        _panY = (point.Y - centerY) * (1 - appliedFactor) + _panY * appliedFactor;
        UpdateLayout();
    }

    public void SetZoom(double zoom)
    {
        if (_image.Source is null)
            return;
        double fitScale = FitScale();
        _zoom = Math.Clamp(zoom / Math.Max(0.001, fitScale), MinZoom, MaxZoom);
        _panX = 0;
        _panY = 0;
        UpdateLayout();
    }

    public void SetActualSize()
        => SetZoom(1.0);

    public void PanBy(double x, double y)
    {
        if (_image.Source is null)
            return;
        _panX += x;
        _panY += y;
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

    public void OnPointerCaptureLost()
        => _isPanning = false;

    public void OnPointerWheelChanged(Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (_image.Source is null || _previewRoot.Visibility != Visibility.Visible)
            return;

        var point = e.GetCurrentPoint(_previewRoot);
        int delta = point.Properties.MouseWheelDelta;
        if (delta == 0)
            return;

        OnMouseWheel(delta, point.Position);
        e.Handled = true;
    }

    public void OnMouseWheel(int delta, Windows.Foundation.Point point)
    {
        if (_image.Source is null || delta == 0)
            return;
        ZoomAt(delta > 0 ? 1.15 : 1.0 / 1.15, point);
    }

    public void OnDoubleTapped(Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
    {
        if (_image.Source is null || _previewRoot.Visibility != Visibility.Visible)
            return;

        ResetView();
        e.Handled = true;
    }

    public static AnimatedImageRenderPlan? CreateRenderPlan(FileProbe probe)
    {
        if (!probe.Kind.Equals("image", StringComparison.OrdinalIgnoreCase)
            || probe.IsAnimated is false)
        {
            return null;
        }

        return probe.Extension.ToLowerInvariant() is ".gif" or ".webp" or ".png"
            ? new AnimatedImageRenderPlan()
            : null;
    }

    private void SyncDecodedImageSize()
    {
        if (_image.Source is not BitmapSource bitmap)
            return;

        if (bitmap.PixelWidth <= 0 || bitmap.PixelHeight <= 0)
            return;

        if (Math.Abs(_sourceWidth - bitmap.PixelWidth) < 0.1
            && Math.Abs(_sourceHeight - bitmap.PixelHeight) < 0.1)
        {
            return;
        }

        _sourceWidth = bitmap.PixelWidth;
        _sourceHeight = bitmap.PixelHeight;
        _image.Width = _sourceWidth;
        _image.Height = _sourceHeight;
    }

    private double FitScale()
    {
        if (_sourceWidth <= 0 || _sourceHeight <= 0)
            return 1.0;

        double availableWidth = Math.Max(1, _previewRoot.ActualWidth);
        double availableHeight = Math.Max(1, _previewRoot.ActualHeight);
        return Math.Min(1.0, Math.Min(availableWidth / _sourceWidth, availableHeight / _sourceHeight));
    }

    private double ActualScale()
        => FitScale() * _zoom;
}

internal readonly record struct AnimatedImagePreviewResult(string Status, double Width, double Height);

internal readonly record struct AnimatedImageRenderPlan;
