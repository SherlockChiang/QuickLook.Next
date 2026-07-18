using System.Diagnostics;
using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Foundation;
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
    private DispatcherTimer? _nativeFrameTimer;
    private NativeAnimationFrames? _nativeFrames;
    private WriteableBitmap? _nativeFrameBitmap;
    private int _nativeFrameIndex;
    private int[]? _nativeFrameTimeline;
    private int _nativeAnimationDurationMs;
    private Stopwatch? _nativeFrameClock;
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
    public bool CanTogglePlayback => _nativeFrames?.Frames.Count > 1 && _nativeFrameTimer is not null;
    public bool IsPlaybackPaused { get; private set; }
    public Action<ImageWaveform>? WaveformChanged { get; init; }

    public AnimatedImagePreviewResult Render(string path, PreviewReady ready, (double Width, double Height) maxContent)
    {
        StopNativePlayback();
        _layoutVersion++;
        _currentPath = path;
        _openWatch = Stopwatch.StartNew();
        _sourceWidth = Math.Max(1, ready.PreferredWidth);
        _sourceHeight = Math.Max(1, ready.PreferredHeight);
        _image.Width = _sourceWidth;
        _image.Height = _sourceHeight;
        double imageMaxWidth = Math.Max(1, maxContent.Width - InfoRailWidth);
        double imageMaxHeight = Math.Max(1, maxContent.Height - ToolbarHeight);
        double scale = Math.Min(1.0, Math.Min(imageMaxWidth / _sourceWidth, imageMaxHeight / _sourceHeight));
        var bitmap = new BitmapImage(new Uri(path));
        if (scale < 1.0)
        {
            bitmap.DecodePixelWidth = Math.Max(1, (int)Math.Round(_sourceWidth * scale));
            bitmap.DecodePixelHeight = Math.Max(1, (int)Math.Round(_sourceHeight * scale));
        }
        _image.Source = bitmap;
        ResetView();
        ScheduleLayoutUpdate();

        double width = _sourceWidth * scale + InfoRailWidth;
        double height = _sourceHeight * scale + ToolbarHeight;
        return new AnimatedImagePreviewResult($"{ready.Kind}: {ready.Title}", width, height);
    }

    public AnimatedImagePreviewResult RenderNativeFrames(string path, PreviewReady ready, NativeAnimationFrames frames, (double Width, double Height) maxContent)
    {
        StopNativePlayback();
        _layoutVersion++;
        _currentPath = path;
        _openWatch = null;
        _nativeFrames = frames;
        _waveformVersion++;
        _waveformUpdatePending = false;
        IsPlaybackPaused = false;
        _nativeFrameBitmap = new WriteableBitmap(frames.Width, frames.Height);
        _nativeFrameIndex = 0;
        _nativeFrameTimeline = BuildFrameTimeline(frames);
        _nativeAnimationDurationMs = _nativeFrameTimeline.Length == 0 ? 0 : _nativeFrameTimeline[^1];
        _sourceWidth = Math.Max(1, frames.Width);
        _sourceHeight = Math.Max(1, frames.Height);
        _lastWaveformUpdateMilliseconds = -WaveformUpdateIntervalMilliseconds;
        _image.Width = frames.Width;
        _image.Height = frames.Height;
        double imageMaxWidth = Math.Max(1, maxContent.Width - InfoRailWidth);
        double imageMaxHeight = Math.Max(1, maxContent.Height - ToolbarHeight);
        double scale = Math.Min(1.0, Math.Min(imageMaxWidth / frames.Width, imageMaxHeight / frames.Height));
        PresentNativeFrame(0);
        ResetView();
        ScheduleLayoutUpdate();

        if (frames.Frames.Count > 1)
        {
            _nativeFrameClock = Stopwatch.StartNew();
            _nativeFrameTimer = new DispatcherTimer();
            _nativeFrameTimer.Tick += (_, _) => AdvanceNativeFrame();
            ScheduleNextNativeFrame();
        }

        double width = frames.Width * scale + InfoRailWidth;
        double height = frames.Height * scale + ToolbarHeight;
        return new AnimatedImagePreviewResult($"{ready.Kind}: {ready.Title}", width, height);
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
        if (!CanTogglePlayback || _nativeFrameClock is null || _nativeFrameTimer is null)
            return;

        if (IsPlaybackPaused)
        {
            _nativeFrameClock.Start();
            _nativeFrameTimer.Start();
            IsPlaybackPaused = false;
            AdvanceNativeFrame();
        }
        else
        {
            _nativeFrameTimer.Stop();
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
        if (_nativeFrames is null || _nativeFrames.Frames.Count == 0)
            return;

        if (_nativeFrameTimeline is null || _nativeAnimationDurationMs <= 0 || _nativeFrameClock is null)
            return;

        _nativeFrameTimer?.Stop();
        int elapsed = (int)(_nativeFrameClock.ElapsedMilliseconds % _nativeAnimationDurationMs);
        int frameIndex = FindFrameIndex(_nativeFrameTimeline, elapsed);
        if (frameIndex != _nativeFrameIndex)
        {
            _nativeFrameIndex = frameIndex;
            PresentNativeFrame(_nativeFrameIndex);
        }
        ScheduleNextNativeFrame();
    }

    private void ScheduleNextNativeFrame()
    {
        if (_nativeFrameTimer is null || _nativeFrameClock is null || _nativeFrameTimeline is null || _nativeAnimationDurationMs <= 0)
            return;

        int elapsed = (int)(_nativeFrameClock.ElapsedMilliseconds % _nativeAnimationDurationMs);
        int nextDeadline = _nativeFrameTimeline[_nativeFrameIndex];
        if (nextDeadline <= elapsed)
            nextDeadline += _nativeAnimationDurationMs;
        int delay = Math.Max(1, nextDeadline - elapsed);
        _nativeFrameTimer.Interval = TimeSpan.FromMilliseconds(delay);
        _nativeFrameTimer.Start();
    }

    private void PresentNativeFrame(int index)
    {
        if (_nativeFrames is null || index < 0 || index >= _nativeFrames.Frames.Count)
            return;

        if (_nativeFrameBitmap is null)
            _nativeFrameBitmap = new WriteableBitmap(_nativeFrames.Width, _nativeFrames.Height);

        using var stream = _nativeFrameBitmap.PixelBuffer.AsStream();
        stream.SetLength(0);
        byte[] bgra = _nativeFrames.Frames[index].Bgra;
        stream.Write(bgra, 0, bgra.Length);
        _nativeFrameBitmap.Invalidate();
        _image.Source = _nativeFrameBitmap;
        long elapsed = _nativeFrameClock?.ElapsedMilliseconds ?? 0;
        if (elapsed - _lastWaveformUpdateMilliseconds >= WaveformUpdateIntervalMilliseconds)
        {
            _lastWaveformUpdateMilliseconds = elapsed;
            QueueWaveformUpdate(bgra, _nativeFrames.Width, _nativeFrames.Height);
        }
    }

    private void QueueWaveformUpdate(byte[] bgra, int width, int height)
    {
        if (_waveformUpdatePending || WaveformChanged is null)
            return;

        _waveformUpdatePending = true;
        int version = _waveformVersion;
        _ = Task.Run(() => ImageWaveformBuilder.Create(bgra, width, height)).ContinueWith(task =>
        {
            _previewRoot.DispatcherQueue.TryEnqueue(() =>
            {
                if (version != _waveformVersion)
                    return;
                _waveformUpdatePending = false;
                if (task.IsCompletedSuccessfully)
                    WaveformChanged?.Invoke(task.Result);
            });
        }, TaskScheduler.Default);
    }

    private void StopNativePlayback()
    {
        if (_nativeFrameTimer is not null)
        {
            _nativeFrameTimer.Stop();
            _nativeFrameTimer = null;
        }
        _nativeFrames = null;
        _waveformVersion++;
        _waveformUpdatePending = false;
        _nativeFrameBitmap = null;
        _nativeFrameIndex = 0;
        _nativeFrameTimeline = null;
        _nativeAnimationDurationMs = 0;
        _nativeFrameClock = null;
        _lastWaveformUpdateMilliseconds = 0;
        IsPlaybackPaused = false;
    }

    private static int[] BuildFrameTimeline(NativeAnimationFrames frames)
    {
        var timeline = new int[frames.Frames.Count];
        int total = 0;
        for (int i = 0; i < frames.Frames.Count; i++)
        {
            total = checked(total + Math.Clamp(frames.Frames[i].DelayMilliseconds, 20, 1_000));
            timeline[i] = total;
        }
        return timeline;
    }

    private static int FindFrameIndex(int[] timeline, int elapsedMs)
    {
        int index = Array.BinarySearch(timeline, elapsedMs + 1);
        return index >= 0 ? index : ~index;
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

    public static (int Width, int Height)? TryReadAnimatedSize(string path)
        => Path.GetExtension(path).ToLowerInvariant() switch
        {
            ".gif" => TryReadGifSize(path),
            ".webp" => TryReadAnimatedWebPSize(path),
            _ => null,
        };

    private static (int Width, int Height)? TryReadGifSize(string path)
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

    public static AnimatedImageRenderPlan? CreateRenderPlan(string path)
    {
        if (TryReadAnimatedSize(path) is not { } size)
            return null;

        return new AnimatedImageRenderPlan(
            size.Width,
            size.Height,
            AnimatedImagePlaybackMode.NativeFramePlayback);
    }

    private static (int Width, int Height)? TryReadAnimatedWebPSize(string path)
    {
        try
        {
            using var stream = File.OpenRead(path);
            Span<byte> header = stackalloc byte[12];
            if (stream.Read(header) != header.Length
                || !header[..4].SequenceEqual("RIFF"u8)
                || !header[8..12].SequenceEqual("WEBP"u8))
            {
                return null;
            }

            Span<byte> chunkHeader = stackalloc byte[8];
            while (stream.Position + chunkHeader.Length <= stream.Length)
            {
                if (stream.Read(chunkHeader) != chunkHeader.Length)
                    return null;

                string chunk = System.Text.Encoding.ASCII.GetString(chunkHeader[..4]);
                uint size = BitConverter.ToUInt32(chunkHeader[4..8]);
                long payloadStart = stream.Position;
                long nextChunk = payloadStart + size + (size % 2);
                if (nextChunk < payloadStart || nextChunk > stream.Length + 1)
                    return null;

                if (chunk == "VP8X")
                {
                    Span<byte> data = stackalloc byte[10];
                    if (size < data.Length || stream.Read(data) != data.Length)
                        return null;

                    bool animated = (data[0] & 0x02) != 0;
                    int width = Read24(data[4], data[5], data[6]) + 1;
                    int height = Read24(data[7], data[8], data[9]) + 1;
                    return animated && width > 0 && height > 0 ? (width, height) : null;
                }

                if (chunk == "ANIM" || chunk == "ANMF")
                    return TryReadWebPStaticSize(path);

                stream.Position = nextChunk;
            }
        }
        catch
        {
        }

        return null;
    }

    private static (int Width, int Height)? TryReadWebPStaticSize(string path)
    {
        try
        {
            using var stream = File.OpenRead(path);
            stream.Position = 12;
            Span<byte> chunkHeader = stackalloc byte[8];
            while (stream.Position + chunkHeader.Length <= stream.Length)
            {
                if (stream.Read(chunkHeader) != chunkHeader.Length)
                    return null;

                string chunk = System.Text.Encoding.ASCII.GetString(chunkHeader[..4]);
                uint size = BitConverter.ToUInt32(chunkHeader[4..8]);
                long payloadStart = stream.Position;
                long nextChunk = payloadStart + size + (size % 2);
                if (nextChunk < payloadStart || nextChunk > stream.Length + 1)
                    return null;

                if (chunk == "VP8 " && size >= 10)
                {
                    byte[] data = new byte[10];
                    if (stream.Read(data, 0, data.Length) != data.Length)
                        return null;
                    if (data[3] == 0x9D && data[4] == 0x01 && data[5] == 0x2A)
                    {
                        int width = BitConverter.ToUInt16(data, 6) & 0x3FFF;
                        int height = BitConverter.ToUInt16(data, 8) & 0x3FFF;
                        return width > 0 && height > 0 ? (width, height) : null;
                    }
                }
                else if (chunk == "VP8L" && size >= 5)
                {
                    byte[] data = new byte[5];
                    if (stream.Read(data, 0, data.Length) != data.Length || data[0] != 0x2F)
                        return null;
                    int width = 1 + data[1] + ((data[2] & 0x3F) << 8);
                    int height = 1 + ((data[2] & 0xC0) >> 6) + (data[3] << 2) + ((data[4] & 0x0F) << 10);
                    return width > 0 && height > 0 ? (width, height) : null;
                }

                stream.Position = nextChunk;
            }
        }
        catch
        {
        }

        return null;
    }

    private static int Read24(byte b0, byte b1, byte b2)
        => b0 | (b1 << 8) | (b2 << 16);

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

internal enum AnimatedImagePlaybackMode
{
    WinUiAnimatedPlayback,
    NativeFramePlayback,
}

internal readonly record struct AnimatedImageRenderPlan(int Width, int Height, AnimatedImagePlaybackMode PlaybackMode);
