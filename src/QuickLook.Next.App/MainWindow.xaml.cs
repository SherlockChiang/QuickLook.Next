using System.Numerics;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI;
using Microsoft.UI.Composition;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Graphics;
using Windows.Storage.Streams;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

public sealed partial class MainWindow : Window
{
    private const double MaxImageWindowWidth = 1320;
    private const double MaxImageWindowHeight = 900;
    private const double MaxPdfWindowWidth = 1040;
    private const double MaxPdfWindowHeight = 900;
    private const double MaxTextWindowWidth = 1100;
    private const double MaxTextWindowHeight = 860;
    private const double PdfPageTargetWidth = 860;
    private const double MinImageZoom = 0.1;
    private const double MaxImageZoom = 12.0;
    private const double RasterInfoRailWidth = 246;
    private const double RasterToolbarHeight = 82;
    private const int MaxHighlightedChars = 256 * 1024;
    private const int MaxHighlightedRuns = 7000;
    private const int SwitchDebounceMs = 110;
    private const int PdfOffscreenSurfaceCachePages = 5;

    private readonly NativeBridge _native = new();
    private readonly Dictionary<int, Border> _pdfPageHosts = new();
    private readonly HashSet<int> _requestedPdfPages = new();
    private readonly Dictionary<int, long> _pdfPageLastTouched = new();
    private readonly Dictionary<TokenKind, SolidColorBrush> _tokenBrushes = new();
    private Compositor? _compositor;
    private SpriteVisual? _rasterSprite;
    private uint _rasterSurfaceWidth;
    private uint _rasterSurfaceHeight;
    private double _imageZoom = 1.0;
    private double _imagePanX;
    private double _imagePanY;
    private bool _isPanning;
    private Windows.Foundation.Point _panStart;
    private double _panStartX;
    private double _panStartY;
    private TrayIconManager? _trayIcon;
    private RasterHostSupervisor? _supervisor;
    private string? _currentRequestId;
    private string? _currentPath;
    private double _currentPdfScale = 1.0;
    private bool _isStarted;
    private bool _previewVisible;
    private bool? _brushThemeDark;
    private int _previewGeneration;
    private PreviewListing? _currentListing;
    private string _currentListingPath = "";
    private string _listingSortColumn = "name";
    private bool _listingSortAscending = true;
    private bool? _backgroundEfficiencyEnabled;
    private CancellationTokenSource? _switchDebounceCts;
    private CancellationTokenSource? _previewOperationCts;
    private long _pdfPageTouchTick;
    private bool _previewRevealPending;
    private bool _previewTemporarilyHidden;

    private static readonly SolidColorBrush OfficeWhiteBrush = new(Colors.White);
    private static readonly SolidColorBrush OfficeBlackBrush = new(Colors.Black);
    private static readonly SolidColorBrush UiGrayBrush = new(Colors.Gray);
    private static readonly SolidColorBrush OfficeBorderBrush = new(ColorHelper.FromArgb(255, 210, 210, 210));
    private static readonly SolidColorBrush OfficeCellBorderBrush = new(ColorHelper.FromArgb(255, 225, 225, 225));
    private static readonly string[] ByteUnits = ["B", "KB", "MB", "GB", "TB"];
    private static readonly Dictionary<string, FontFamily> FontFamilyCache = new(StringComparer.Ordinal);

    // Show the top status text (file name / errors) only while debugging; normal use is chromeless.
    private const bool ShowStatusBar = false;

    public MainWindow()
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        Title = "QuickLook Next";
        TrySetBackdrop();
        PreviewRoot.SizeChanged += OnRootSizeChanged;
        PreviewRoot.PointerWheelChanged += OnPreviewRootPointerWheelChanged;
        PreviewRoot.PointerPressed += OnPreviewRootPointerPressed;
        PreviewRoot.PointerMoved += OnPreviewRootPointerMoved;
        PreviewRoot.PointerReleased += OnPreviewRootPointerReleased;
        PreviewRoot.DoubleTapped += OnPreviewRootDoubleTapped;
        RootGrid.KeyDown += OnRootGridKeyDown;
        PdfScrollViewer.ViewChanged += (_, _) => RequestVisiblePdfPages();
        GetAppWindow().Closing += (_, args) =>
        {
            // Intercept the close (X button / Alt+F4 / taskbar close): hide the window instead of
            // destroying it. The app stays alive in the tray; Escape or tray "Exit" truly quits.
            args.Cancel = true;
            HidePreviewWindow();
        };
        Closed += (_, _) =>
        {
            RemoveTrayIcon();
            _supervisor?.Stop();
        };

        RootGrid.ActualThemeChanged += (s, e) =>
        {
            UpdateTitleBarColors();
            ApplyWindowIcon();
            RefreshTrayIcon();
        };
        UpdateTitleBarColors();
        UpdateListingSortHeaders();
    }

    public async Task StartBackgroundAsync()
    {
        if (_isStarted) return;
        _isStarted = true;

        DiagLog.Write("App", $"background start; pid={Environment.ProcessId}");
        SetBackgroundEfficiency(enabled: true);
        StatusBar.Visibility = ShowStatusBar ? Visibility.Visible : Visibility.Collapsed;
        ApplyWindowIcon();
        EnsureTrayIcon();
        ApplyNoActivateStyle();

        try
        {
            _supervisor = new RasterHostSupervisor(ResolveHostExePath(), DispatcherQueue);
            _supervisor.SetBackgroundEfficiency(_backgroundEfficiencyEnabled ?? true);
            _supervisor.SurfaceReceived += OnSurfaceReceived;
            _native.Start(OnNativeIntent);
            StatusText.Text = "ready";
            DiagLog.Write("App", "native hook installed; RasterHost is lazy");
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "startup FAILED: " + ex);
            // Pipe-in-use means another instance is already running — exit quietly instead of
            // becoming a broken tray-zombie process. (Don't match the message string — it's
            // localized; match by the pipe-creation stack frame instead.)
            if (ex is System.IO.IOException && ex.StackTrace?.Contains("NamedPipeServerStream", StringComparison.Ordinal) == true)
            {
                DiagLog.Write("App", "another instance holds the pipe — exiting");
                ExitApp();
                return;
            }
            StatusText.Text = "startup error: " + ex.Message;
            ShowPreviewWindow(activate: true);
        }
    }

    private void OnNativeIntent(NativeIntent intent)
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            if (intent.Intent == PreviewIntent.Switch)
                DebounceSwitchIntent(intent);
            else
            {
                CancelSwitchDebounce();
                _ = HandleNativeIntentSafelyAsync(intent);
            }
        });
    }

    private void DebounceSwitchIntent(NativeIntent intent)
    {
        if (!_previewVisible)
            return;

        CancelSwitchDebounce();
        var cts = new CancellationTokenSource();
        _switchDebounceCts = cts;
        Task.Delay(SwitchDebounceMs, cts.Token).ContinueWith(task =>
        {
            if (task.IsCanceled)
            {
                cts.Dispose();
                return;
            }
            if (!DispatcherQueue.TryEnqueue(() =>
            {
                try
                {
                    if (_switchDebounceCts != cts || cts.IsCancellationRequested)
                        return;
                    _switchDebounceCts = null;
                    _ = HandleNativeIntentSafelyAsync(intent);
                }
                finally
                {
                    cts.Dispose();
                }
            }))
            {
                cts.Dispose();
            }
        }, TaskScheduler.Default);
    }

    private void CancelSwitchDebounce()
    {
        if (_switchDebounceCts is null)
            return;

        try { _switchDebounceCts.Cancel(); }
        catch { }
        _switchDebounceCts.Dispose();
        _switchDebounceCts = null;
    }

    private async Task HandleNativeIntentSafelyAsync(NativeIntent intent)
    {
        try
        {
            await HandleNativeIntentAsync(intent);
        }
        catch (OperationCanceledException)
        {
            DiagLog.Write("App", "preview operation canceled");
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "intent handler FAILED: " + ex);
            StatusText.Text = ShowErrorPreview(ex.Message);
            RevealPreviewWindow(activate: false);
        }
    }

    /// <summary>
    /// Close the in-flight preview. Clears <see cref="_currentRequestId"/> <i>before</i> awaiting the send
    /// (atomic on the UI dispatcher — no yield in between), so any late surface for it is dropped by the
    /// guard, and a second concurrent caller sees null and skips (de-dupes the close).
    /// </summary>
    private async Task CloseCurrentAsync()
    {
        var id = _currentRequestId;
        if (id is null) return;
        _currentRequestId = null;
        await _supervisor!.CloseAsync(id);
    }

    private async Task EnsureRasterHostStartedAsync()
    {
        if (_supervisor is null)
        {
            _supervisor = new RasterHostSupervisor(ResolveHostExePath(), DispatcherQueue);
            _supervisor.SetBackgroundEfficiency(_backgroundEfficiencyEnabled ?? true);
            _supervisor.SurfaceReceived += OnSurfaceReceived;
        }

        await _supervisor.EnsureStartedAsync();
    }

    private async Task HandleNativeIntentAsync(NativeIntent intent)
    {
        // +/- zoom the image preview (only when one is showing; the global key isn't swallowed elsewhere).
        if (intent.Intent is PreviewIntent.ZoomIn or PreviewIntent.ZoomOut)
        {
            if (_rasterSprite is not null && PreviewRoot.Visibility == Visibility.Visible)
            {
                double factor = intent.Intent == PreviewIntent.ZoomIn ? 1.15 : 1.0 / 1.15;
                _imageZoom = Math.Clamp(_imageZoom * factor, MinImageZoom, MaxImageZoom);
                UpdateRasterSpriteLayout();
            }
            return;
        }

        if (intent.Intent == PreviewIntent.Close)
        {
            int generation = BeginPreviewGeneration();
            CancellationToken previewToken = CurrentPreviewToken;
            ResetPreview();
            await CloseCurrentAsync();
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            _currentPath = null;
            HidePreviewWindow();
            return;
        }

        if (intent.Intent is PreviewIntent.Open or PreviewIntent.Switch && intent.PrimaryPath is { } path)
        {
            if (intent.Intent == PreviewIntent.Switch && !_previewVisible)
                return;
            if (intent.Intent == PreviewIntent.Switch
                && _currentPath is not null
                && string.Equals(_currentPath, path, StringComparison.OrdinalIgnoreCase))
            {
                return;
            }

            if (intent.Intent == PreviewIntent.Open
                && _previewVisible
                && _currentPath is not null
                && string.Equals(_currentPath, path, StringComparison.OrdinalIgnoreCase))
            {
                int closeGeneration = BeginPreviewGeneration();
                CancellationToken closeToken = CurrentPreviewToken;
                ResetPreview();
                await CloseCurrentAsync();
                if (!IsPreviewGenerationCurrent(closeGeneration, closeToken)) return;
                _currentPath = null;
                HidePreviewWindow();
                return;
            }

            int generation = BeginPreviewGeneration();
            CancellationToken previewToken = CurrentPreviewToken;
            BeginPreviewTransition();
            ResetPreview();
            Title = System.IO.Path.GetFileName(path);
            PreviewTitleText.Text = Title;
            StatusText.Text = $"opening {System.IO.Path.GetFileName(path)}…";
            try
            {
                await CloseCurrentAsync();
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                FileProbe probe = await Task.Run(() => _native.ProbeFile(path) ?? BuildProbe(path), previewToken);
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;

                if (IsMediaProbe(probe))
                {
                    var mediaReady = new PreviewReady(
                        $"media-{generation}",
                        "media",
                        System.IO.Path.GetFileName(path),
                        800,
                        probe.Kind.Equals("audio", StringComparison.OrdinalIgnoreCase) ? 140 : 450)
                    {
                        MediaPath = path,
                    };
                    _currentPath = path;
                    _currentRequestId = null;
                    StatusText.Text = ShowMediaPreview(mediaReady);
                    RevealPreviewWindow(ShouldActivatePreview(mediaReady));
                    return;
                }

                PreviewReady? nativeReady = await Task.Run(() => _native.TryPreview($"native-{generation}", path, probe), previewToken);
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                if (nativeReady is not null)
                {
                    _currentPath = path;
                    _currentRequestId = null;
                    StatusText.Text = nativeReady switch
                    {
                        PreviewReady r when r.OfficeLayout is not null => ShowOfficeLayoutPreview(r),
                        PreviewReady r when r.Listing is not null => ShowListingPreview(r),
                        PreviewReady r when r.TextContent is not null => ShowTextPreview(r),
                        _ => $"{nativeReady.Kind}: {nativeReady.Title}",
                    };
                    RevealPreviewWindow(ShouldActivatePreview(nativeReady));
                    return;
                }

                await EnsureRasterHostStartedAsync();
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                var (requestId, completion) = _supervisor!.BeginOpen(path, probe);
                _currentRequestId = requestId;
                _currentPath = path;
                ControlMessage result = await completion.WaitAsync(previewToken);
                if (!IsPreviewGenerationCurrent(generation, previewToken) || _currentRequestId != requestId)
                    return;
                StatusText.Text = result switch
                {
                    PreviewReady r when r.Kind == "pdf" => ShowPdfDocument(requestId, r),
                    PreviewReady r when r.OfficeLayout is not null => ShowOfficeLayoutPreview(r),
                    PreviewReady r when r.Listing is not null => ShowListingPreview(r),
                    PreviewReady r when r.TextContent is not null => ShowTextPreview(r),
                    PreviewReady r when r.MediaPath is not null => ShowMediaPreview(r),
                    PreviewReady r => ShowRasterPreview(r),
                    PreviewError er => ShowErrorPreview(er.Message),
                    _ => "?",
                };
                RevealPreviewWindow(result is PreviewReady ready && ShouldActivatePreview(ready));
            }
            catch (TimeoutException)
            {
                StatusText.Text = ShowErrorPreview("preview timed out");
                RevealPreviewWindow(activate: false);
            }
            catch (OperationCanceledException)
            {
                DiagLog.Write("App", $"preview canceled: path={path}");
            }
        }
    }

    private int BeginPreviewGeneration()
    {
        CancelPreviewOperation();
        _previewOperationCts = new CancellationTokenSource();
        return ++_previewGeneration;
    }

    private CancellationToken CurrentPreviewToken => _previewOperationCts?.Token ?? CancellationToken.None;

    private bool IsPreviewGenerationCurrent(int generation) => IsPreviewGenerationCurrent(generation, CurrentPreviewToken);

    private bool IsPreviewGenerationCurrent(int generation, CancellationToken cancellationToken)
        => generation == _previewGeneration && !cancellationToken.IsCancellationRequested;

    private void CancelPreviewOperation()
    {
        if (_previewOperationCts is null)
            return;

        try { _previewOperationCts.Cancel(); }
        catch { }
        _previewOperationCts.Dispose();
        _previewOperationCts = null;
    }

    private void BeginPreviewTransition()
    {
        _previewRevealPending = true;
        PreviewContentHost.Opacity = 0;
        ErrorPanel.Visibility = Visibility.Collapsed;
        LoadingRing.Visibility = Visibility.Visible;
        LoadingRing.IsActive = true;
    }

    private void RevealPreviewWindow(bool activate)
    {
        _previewRevealPending = false;
        LoadingRing.IsActive = false;
        LoadingRing.Visibility = Visibility.Collapsed;
        if (!_previewVisible || _previewTemporarilyHidden)
        {
            ShowPreviewWindow(activate, resizeToDefault: false);
            _previewTemporarilyHidden = false;
        }
        else
        {
            if (activate)
            {
                SetNoActivateStyle(enabled: false);
                Activate();
            }
            else
            {
                ApplyNoActivateStyle();
            }
            RaisePreviewWindow(activate);
            EnsureCompositor();
        }
        FadeInPreviewContent();
    }

    private static bool ShouldActivatePreview(PreviewReady ready)
        => ready.TextContent is not null || ready.Listing is not null || ready.OfficeLayout is not null;

    private void UpdatePreviewChrome(PreviewReady ready, bool showRasterTools = false)
    {
        string? path = _currentPath ?? ready.MediaPath;
        string title = !string.IsNullOrWhiteSpace(path)
            ? System.IO.Path.GetFileName(path)
            : ready.Title;
        if (string.IsNullOrWhiteSpace(title))
            title = "QuickLook Next";

        Title = title;
        PreviewTitleText.Text = title;
        PreviewKindPillText.Text = ready.Kind.ToUpperInvariant();
        PreviewMetaText.Text = BuildPreviewMetaLine(ready, path);

        PreviewInfoRail.Visibility = showRasterTools ? Visibility.Visible : Visibility.Collapsed;
        ImagePreviewToolbar.Visibility = showRasterTools ? Visibility.Visible : Visibility.Collapsed;
        PreviewRoot.Margin = showRasterTools
            ? new Thickness(14, 0, RasterInfoRailWidth + 14, RasterToolbarHeight)
            : new Thickness(14, 0, 14, 14);

        PreviewDimensionsText.Text = BuildDimensionsText(ready);
        PreviewSizeText.Text = FileSizeText(path);
        PreviewTypeText.Text = PreviewTypeTextFor(ready, path);
        PreviewModifiedText.Text = ModifiedText(path);
        PreviewPathText.Text = string.IsNullOrWhiteSpace(path) ? "-" : path;
        UpdateImageZoomLabel();
    }

    private void ResetPreviewChrome()
    {
        Title = "QuickLook Next";
        PreviewTitleText.Text = "QuickLook Next";
        PreviewMetaText.Text = "Ready";
        PreviewKindPillText.Text = "READY";
        PreviewInfoRail.Visibility = Visibility.Collapsed;
        ImagePreviewToolbar.Visibility = Visibility.Collapsed;
        PreviewRoot.Margin = new Thickness(14, 0, 14, 14);
        PreviewDimensionsText.Text = "-";
        PreviewSizeText.Text = "-";
        PreviewTypeText.Text = "-";
        PreviewModifiedText.Text = "-";
        PreviewPathText.Text = "-";
        ImageZoomText.Text = "Fit";
    }

    private static string BuildPreviewMetaLine(PreviewReady ready, string? path)
    {
        var parts = new List<string>();
        string dimensions = BuildDimensionsText(ready);
        if (dimensions != "-")
            parts.Add(dimensions);
        string size = FileSizeText(path);
        if (size != "-")
            parts.Add(size);
        parts.Add(PreviewTypeTextFor(ready, path));
        string modified = ModifiedText(path);
        if (modified != "-")
            parts.Add("Modified: " + modified);
        return string.Join("  |  ", parts);
    }

    private static string BuildDimensionsText(PreviewReady ready)
    {
        if (ready.Kind == "pdf" && ready.PageCount > 0)
            return $"{ready.PageCount:N0} pages";
        if (ready.PreferredWidth > 0 && ready.PreferredHeight > 0)
            return $"{ready.PreferredWidth:N0} x {ready.PreferredHeight:N0}";
        if (ready.OfficeLayout is { Pages.Length: > 0 } layout)
            return $"{layout.Pages.Length:N0} pages";
        if (ready.Listing is { } listing)
            return listing.Summary;
        return "-";
    }

    private static string PreviewTypeTextFor(PreviewReady ready, string? path)
    {
        string ext = string.IsNullOrWhiteSpace(path)
            ? ""
            : System.IO.Path.GetExtension(path).TrimStart('.').ToUpperInvariant();
        return string.IsNullOrEmpty(ext) ? ready.Kind : $"{ext} {ready.Kind}";
    }

    private static string FileSizeText(string? path)
    {
        try
        {
            if (!string.IsNullOrWhiteSpace(path) && System.IO.File.Exists(path))
                return FormatBytes(new FileInfo(path).Length);
        }
        catch { }
        return "-";
    }

    private static string ModifiedText(string? path)
    {
        try
        {
            if (!string.IsNullOrWhiteSpace(path))
            {
                if (System.IO.File.Exists(path))
                    return new FileInfo(path).LastWriteTime.ToString("g");
                if (Directory.Exists(path))
                    return new DirectoryInfo(path).LastWriteTime.ToString("g");
            }
        }
        catch { }
        return "-";
    }

    private string ShowErrorPreview(string message)
    {
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        PreviewInfoRail.Visibility = Visibility.Collapsed;
        ImagePreviewToolbar.Visibility = Visibility.Collapsed;
        ErrorText.Text = string.IsNullOrWhiteSpace(message) ? "Unable to preview this file." : message;
        ErrorPanel.Visibility = Visibility.Visible;
        PreviewTitleText.Text = "Preview unavailable";
        PreviewMetaText.Text = ErrorText.Text;
        PreviewKindPillText.Text = "ERROR";
        ResizeWindowForContent(520, 260, MaxTextWindowWidth, MaxTextWindowHeight);
        return "error: " + ErrorText.Text;
    }

    private void FadeInPreviewContent()
    {
        PreviewContentHost.Opacity = 1;
        var visual = ElementCompositionPreview.GetElementVisual(PreviewContentHost);
        var compositor = visual.Compositor;
        var animation = compositor.CreateScalarKeyFrameAnimation();
        animation.InsertKeyFrame(0f, 0f);
        animation.InsertKeyFrame(
            1f,
            1f,
            compositor.CreateCubicBezierEasingFunction(
                new Vector2(0.1f, 0.9f),
                new Vector2(0.2f, 1f)));
        animation.Duration = TimeSpan.FromMilliseconds(180);
        visual.StartAnimation("Opacity", animation);
    }

    private void OnSurfaceReceived(PreviewSurface surface)
    {
        EnsureCompositor();
        if (_compositor is null) return;
        // Only accept surfaces for the exact current request. While switching/closing _currentRequestId is
        // null, so late surfaces for a just-closed request are dropped — never build a composition surface
        // from a handle whose swapchain the host may already be retiring.
        if (surface.RequestId != _currentRequestId) return;

        if (surface.PageIndex >= 0)
        {
            AttachPdfPageSurface(surface);
            return;
        }

        var (compSurface, hr) = CompositionInterop.CreateSurfaceForHandle(_compositor, (nint)surface.SharedHandle);
        if (hr < 0 || compSurface is null) { StatusText.Text = $"surface failed 0x{hr:X8}"; return; }

        // Dispose the previous sprite + brush to release GPU surface before creating a new one.
        DisposeRasterSprite();

        var brush = _compositor.CreateSurfaceBrush(compSurface);
        brush.Stretch = CompositionStretch.Fill;
        var sprite = _compositor.CreateSpriteVisual();
        sprite.Brush = brush;
        _rasterSprite = sprite;
        _rasterSurfaceWidth = surface.Width;
        _rasterSurfaceHeight = surface.Height;
        ElementCompositionPreview.SetElementChildVisual(PreviewRoot, sprite);
        DispatcherQueue.TryEnqueue(UpdateRasterSpriteLayout);
    }

    private void OnRootSizeChanged(object sender, SizeChangedEventArgs e)
    {
        UpdateRasterSpriteLayout();
    }

    private string ShowRasterPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready, showRasterTools: true);
        PreviewRoot.Visibility = Visibility.Visible;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        _imageZoom = 1.0;
        _imagePanX = 0;
        _imagePanY = 0;
        // Size the window to the image's aspect ratio (fit within max), so there's no empty letterbox.
        double w = ready.PreferredWidth, h = ready.PreferredHeight;
        if (w > 0 && h > 0)
        {
            var maxContent = GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight);
            double imageMaxWidth = Math.Max(1, maxContent.Width - RasterInfoRailWidth);
            double imageMaxHeight = Math.Max(1, maxContent.Height - RasterToolbarHeight);
            double scale = Math.Min(1.0, Math.Min(imageMaxWidth / w, imageMaxHeight / h));
            ResizeWindowForContent(
                w * scale + RasterInfoRailWidth,
                h * scale + RasterToolbarHeight,
                MaxImageWindowWidth,
                MaxImageWindowHeight);
        }
        else
        {
            ResizeWindowForContent(w + RasterInfoRailWidth, h + RasterToolbarHeight, MaxImageWindowWidth, MaxImageWindowHeight);
        }
        DispatcherQueue.TryEnqueue(UpdateRasterSpriteLayout);
        return $"{ready.Kind}: {ready.Title}";
    }

    private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush PdfPageBackground =
        new(Microsoft.UI.Colors.White);

    private string ShowPdfDocument(string requestId, PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Visible;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        ElementCompositionPreview.SetElementChildVisual(PreviewRoot, null);
        _rasterSprite = null;
        _rasterSurfaceWidth = 0;
        _rasterSurfaceHeight = 0;
        _imageZoom = 1.0;

        _pdfPageHosts.Clear();
        _requestedPdfPages.Clear();
        _pdfPageLastTouched.Clear();
        _pdfPageTouchTick = 0;
        PdfPagesPanel.Children.Clear();

        double pageWidth = Math.Max(1, ready.PageWidth > 0 ? ready.PageWidth : ready.PreferredWidth);
        double pageHeight = Math.Max(1, ready.PageHeight > 0 ? ready.PageHeight : ready.PreferredHeight);
        var maxPdfContent = GetMaxContentSize(MaxPdfWindowWidth, MaxPdfWindowHeight);
        double targetPageWidth = Math.Min(PdfPageTargetWidth, Math.Max(320, maxPdfContent.Width - 64));
        double targetPageHeight = Math.Max(320, maxPdfContent.Height - 96);
        _currentPdfScale = Math.Clamp(
            Math.Min(targetPageWidth / pageWidth, targetPageHeight / pageHeight),
            0.25,
            1.6);
        double displayWidth = Math.Round(pageWidth * _currentPdfScale);
        double displayHeight = Math.Round(pageHeight * _currentPdfScale);

        int pageCount = Math.Max(1, ready.PageCount);
        for (int i = 0; i < pageCount; i++)
        {
            var pageHost = new Border
            {
                Width = displayWidth,
                Height = displayHeight,
                Background = PdfPageBackground,
            };
            _pdfPageHosts[i] = pageHost;
            PdfPagesPanel.Children.Add(pageHost);
        }

        ResizeWindowForContent(
            Math.Min(maxPdfContent.Width, displayWidth + 64),
            Math.Min(maxPdfContent.Height, displayHeight + 96),
            MaxPdfWindowWidth,
            MaxPdfWindowHeight);
        Task.Delay(100).ContinueWith(_ => DispatcherQueue.TryEnqueue(RequestVisiblePdfPages));
        return $"pdf: {ready.Title}";
    }

    private string ShowTextPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        string text = TrimForDisplay(ready.TextContent ?? "");
        DiagLog.Write("App", $"text preview: format={ready.TextFormat}; language={ready.TextLanguage}; chars={ready.TextContent?.Length ?? 0}; displayed={text.Length}");
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Visible;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        ElementCompositionPreview.SetElementChildVisual(PreviewRoot, null);
        _rasterSprite = null;
        _rasterSurfaceWidth = 0;
        _rasterSurfaceHeight = 0;
        _imageZoom = 1.0;

        TextPreviewBlock.Blocks.Clear();
        TextPreviewBlock.IsTextSelectionEnabled = true;
        // Prose (markdown/plain) wraps to the viewport; code scrolls horizontally to keep line structure.
        bool wrap = ready.TextFormat is "markdown" or "plain";
        TextScrollViewer.HorizontalScrollBarVisibility = wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;
        TextPreviewBlock.FontFamily = FontFamilyFor(ready.TextFormat == "markdown" ? "Segoe UI" : "Cascadia Mono, Consolas");
        TextPreviewBlock.TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap;

        try
        {
            if (ready.TextFormat == "markdown")
                RenderMarkdown(text);
            else
                RenderCodeOrPlainText(text, ready.TextLanguage ?? "text");
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "text render FAILED; falling back to plain text: " + ex);
            TextPreviewBlock.Blocks.Clear();
            TextPreviewBlock.FontFamily = FontFamilyFor("Cascadia Mono, Consolas");
            TextPreviewBlock.TextWrapping = TextWrapping.NoWrap;
            TextScrollViewer.HorizontalScrollBarVisibility = ScrollBarVisibility.Auto;
            AddCodeBlock(text);
        }

        TextPreviewBlock.Focus(FocusState.Programmatic);
        StartPreviewHeroLoad(ready);
        var textSize = EstimateTextPreviewSize(text, ready.TextFormat, wrap);
        ResizeWindowForContent(textSize.Width, textSize.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return $"{ready.Kind}: {ready.Title}";
    }

    private string ShowOfficeLayoutPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        OfficeLayout layout = ready.OfficeLayout!;
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Visible;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        ElementCompositionPreview.SetElementChildVisual(PreviewRoot, null);
        _rasterSprite = null;
        _rasterSurfaceWidth = 0;
        _rasterSurfaceHeight = 0;
        _imageZoom = 1.0;

        OfficePagesPanel.Children.Clear();
        OfficeScrollViewer.ChangeView(0, 0, null, true);
        var maxContent = GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight);
        double maxPageWidth = Math.Max(360, maxContent.Width - 72);
        foreach (OfficePage page in layout.Pages.Take(16))
            OfficePagesPanel.Children.Add(CreateOfficePageView(layout, page, maxPageWidth));

        var first = layout.Pages.FirstOrDefault();
        double firstWidth = first?.Width > 0 ? first.Width : layout.Width;
        double firstHeight = first?.Height > 0 ? first.Height : layout.Height;
        double scale = OfficeLayoutScale(layout, firstWidth, maxPageWidth);
        double contentWidth = Math.Min(maxContent.Width, firstWidth * scale + 64);
        double contentHeight = Math.Min(maxContent.Height, firstHeight * scale + 112);
        ResizeWindowForContent(contentWidth, contentHeight, MaxTextWindowWidth, MaxTextWindowHeight);
        return $"{ready.Kind}: {ready.Title}";
    }

    private FrameworkElement CreateOfficePageView(OfficeLayout layout, OfficePage page, double maxPageWidth)
    {
        double pageWidth = Math.Max(320, page.Width > 0 ? page.Width : layout.Width);
        double pageHeight = Math.Max(180, page.Height > 0 ? page.Height : layout.Height);
        double scale = OfficeLayoutScale(layout, pageWidth, maxPageWidth);
        double viewWidth = pageWidth * scale;
        double viewHeight = pageHeight * scale;

        var stack = new StackPanel { Spacing = 6 };
        stack.Children.Add(new TextBlock
        {
            Text = page.Title,
            FontSize = 12,
            Foreground = UiGrayBrush,
            Margin = new Thickness(2, 0, 0, 0),
        });

        var canvas = new Canvas
        {
            Width = viewWidth,
            Height = viewHeight,
            Background = OfficeWhiteBrush,
        };

        if (layout.LayoutKind.Equals("workbook", StringComparison.OrdinalIgnoreCase))
        {
            foreach (OfficeCell cell in page.Cells)
                AddOfficeCell(canvas, cell, scale);
        }

        foreach (OfficeLayoutItem item in page.Items)
            AddOfficeLayoutItem(canvas, item, scale, layout.LayoutKind);

        stack.Children.Add(new Border
        {
            Width = viewWidth,
            Height = viewHeight,
            Background = OfficeWhiteBrush,
            BorderBrush = OfficeBorderBrush,
            BorderThickness = new Thickness(1),
            Child = canvas,
        });
        return stack;
    }

    private static double OfficeLayoutScale(OfficeLayout layout, double pageWidth, double maxPageWidth)
    {
        double target = layout.LayoutKind.Equals("presentation", StringComparison.OrdinalIgnoreCase)
            ? Math.Min(1.0, maxPageWidth / Math.Max(1, pageWidth))
            : Math.Min(1.0, maxPageWidth / Math.Max(1, pageWidth));
        return Math.Clamp(target, 0.35, 1.0);
    }

    private void AddOfficeCell(Canvas canvas, OfficeCell cell, double scale)
    {
        var border = new Border
        {
            Width = Math.Max(12, cell.Width * scale),
            Height = Math.Max(12, cell.Height * scale),
            BorderBrush = OfficeCellBorderBrush,
            BorderThickness = new Thickness(0, 0, 1, 1),
            Padding = new Thickness(5, 2, 5, 2),
            Child = new TextBlock
            {
                Text = cell.Text,
                FontSize = 12,
                TextTrimming = TextTrimming.CharacterEllipsis,
                Foreground = OfficeBlackBrush,
                VerticalAlignment = VerticalAlignment.Center,
            },
        };
        Canvas.SetLeft(border, cell.X * scale);
        Canvas.SetTop(border, cell.Y * scale);
        canvas.Children.Add(border);
    }

    private void AddOfficeLayoutItem(Canvas canvas, OfficeLayoutItem item, double scale, string layoutKind)
    {
        double x = item.X * scale;
        double y = item.Y * scale;
        double width = Math.Max(12, item.Width * scale);
        double height = Math.Max(12, item.Height * scale);

        if (item.Kind.Equals("image", StringComparison.OrdinalIgnoreCase)
            && !string.IsNullOrWhiteSpace(item.ImageBase64)
            && CreateImageSourceFromBase64(item.ImageBase64) is { } source)
        {
            var image = new Image
            {
                Source = source,
                Width = width,
                Height = height,
                Stretch = Stretch.Uniform,
            };
            Canvas.SetLeft(image, x);
            Canvas.SetTop(image, y);
            canvas.Children.Add(image);
            return;
        }

        if (!string.IsNullOrWhiteSpace(item.Text))
        {
            var text = new TextBlock
            {
                Text = item.Text,
                FontSize = layoutKind.Equals("presentation", StringComparison.OrdinalIgnoreCase) ? Math.Max(12, 15 * scale) : 12,
                TextWrapping = TextWrapping.Wrap,
                Foreground = OfficeBlackBrush,
                MaxWidth = width,
                MaxHeight = height,
            };
            Canvas.SetLeft(text, x);
            Canvas.SetTop(text, y);
            canvas.Children.Add(text);
        }
    }

    private static ImageSource? CreateImageSourceFromBase64(string base64)
    {
        try
        {
            byte[] bytes = Convert.FromBase64String(base64);
            var bitmap = new BitmapImage();
            using var memory = new MemoryStream(bytes);
            bitmap.SetSource(memory.AsRandomAccessStream());
            return bitmap;
        }
        catch
        {
            return null;
        }
    }

    private string ShowMediaPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Visible;
        ListingPanel.Visibility = Visibility.Collapsed;
        ElementCompositionPreview.SetElementChildVisual(PreviewRoot, null);
        _rasterSprite = null;
        _rasterSurfaceWidth = 0;
        _rasterSurfaceHeight = 0;
        _imageZoom = 1.0;

        try
        {
            var uri = new Uri(ready.MediaPath!);
            MediaPreviewElement.Source = Windows.Media.Core.MediaSource.CreateFromUri(uri);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "media load failed: " + ex);
        }

        double w = ready.PreferredWidth > 0 ? ready.PreferredWidth : 800;
        double h = ready.PreferredHeight > 0 ? ready.PreferredHeight : 450;
        var maxContent = GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight);
        double scale = w > 0 && h > 0
            ? Math.Min(1.0, Math.Min(maxContent.Width / w, maxContent.Height / h))
            : 1.0;
        ResizeWindowForContent(w * scale, h * scale, MaxImageWindowWidth, MaxImageWindowHeight);
        return $"{ready.Kind}: {ready.Title}";
    }

    private string ShowListingPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        _currentListing = ready.Listing;
        _currentListingPath = "";

        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Visible;
        ElementCompositionPreview.SetElementChildVisual(PreviewRoot, null);
        _rasterSprite = null;
        _rasterSurfaceWidth = 0;
        _rasterSurfaceHeight = 0;
        _imageZoom = 1.0;

        RenderListing();
        StartPreviewHeroLoad(ready);
        var listingSize = EstimateListingPreviewSize();
        ResizeWindowForContent(listingSize.Width, listingSize.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return $"{ready.Kind}: {ready.Title}";
    }

    private void RenderListing()
    {
        if (_currentListing is null)
            return;

        string title = _currentListing.RootName;
        if (!string.IsNullOrEmpty(_currentListingPath))
        {
            string path = _currentListingPath.TrimEnd('/');
            int slash = path.LastIndexOf('/');
            title = slash >= 0 ? path[(slash + 1)..] : path;
        }

        RenderListingBreadcrumb();

        var visibleItems = _currentListing.Items
            .Where(i => string.Equals(NormalizeListingPath(i.ParentPath), _currentListingPath, StringComparison.OrdinalIgnoreCase))
            .ToList();
        var rows = visibleItems
            .OrderBy(i => i, Comparer<PreviewListingItem>.Create(CompareListingItems))
            .Select(i => new ListingRow(i))
            .ToList();

        ListingTitle.Text = title;
        ListingSummary.Text = BuildListingSummary(_currentListing, visibleItems);
        ListingListView.ItemsSource = rows;
        UpdateListingSortHeaders();
    }

    private int CompareListingItems(PreviewListingItem left, PreviewListingItem right)
    {
        int folderCompare = right.IsFolder.CompareTo(left.IsFolder);
        if (folderCompare != 0)
            return folderCompare;

        int result = _listingSortColumn switch
        {
            "modified" => left.ModifiedUnix.CompareTo(right.ModifiedUnix),
            "type" => string.Compare(left.IsFolder ? "文件夹" : left.Type, right.IsFolder ? "文件夹" : right.Type, StringComparison.OrdinalIgnoreCase),
            "size" => left.Size.CompareTo(right.Size),
            _ => string.Compare(left.Name, right.Name, StringComparison.OrdinalIgnoreCase),
        };
        if (result == 0)
            result = string.Compare(left.Name, right.Name, StringComparison.OrdinalIgnoreCase);
        return _listingSortAscending ? result : -result;
    }

    private void UpdateListingSortHeaders()
    {
        SetListingHeader(ListingNameHeader, "name", "名称");
        SetListingHeader(ListingModifiedHeader, "modified", "修改日期");
        SetListingHeader(ListingTypeHeader, "type", "类型");
        SetListingHeader(ListingSizeHeader, "size", "大小");
    }

    private void SetListingHeader(Button button, string column, string label)
    {
        string arrow = _listingSortColumn == column ? (_listingSortAscending ? " ↑" : " ↓") : "";
        button.Content = label + arrow;
    }

    private void OnListingSortClick(object sender, RoutedEventArgs e)
    {
        if (sender is not Button { Tag: string column })
            return;

        if (_listingSortColumn == column)
            _listingSortAscending = !_listingSortAscending;
        else
        {
            _listingSortColumn = column;
            _listingSortAscending = column != "modified";
        }
        RenderListing();
    }

    private void RenderListingBreadcrumb()
    {
        ListingBreadcrumbPanel.Children.Clear();
        if (_currentListing is null)
            return;

        AddBreadcrumbButton(_currentListing.RootName, "");

        string current = _currentListingPath.TrimEnd('/');
        if (current.Length == 0)
            return;

        string acc = "";
        foreach (string part in current.Split('/', StringSplitOptions.RemoveEmptyEntries))
        {
            ListingBreadcrumbPanel.Children.Add(new TextBlock
            {
                Text = ">",
                VerticalAlignment = VerticalAlignment.Center,
                Foreground = UiGrayBrush,
            });
            acc = acc.Length == 0 ? part + "/" : acc + part + "/";
            AddBreadcrumbButton(part, acc);
        }
    }

    private void AddBreadcrumbButton(string text, string path)
    {
        var button = new Button
        {
            Content = text,
            Tag = path,
            Padding = new Thickness(8, 2, 8, 2),
            MinHeight = 26,
        };
        button.Click += OnListingBreadcrumbClick;
        ListingBreadcrumbPanel.Children.Add(button);
    }

    private void OnListingBreadcrumbClick(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string path })
        {
            _currentListingPath = NormalizeListingPath(path);
            RenderListing();
        }
    }

    private void OnListingItemClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is ListingRow row)
            ListingListView.SelectedItem = row;
    }

    private async void OnListingListViewDoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
    {
        if (ListingListView.SelectedItem is not ListingRow row)
            return;

        if (row.IsFolder)
            await NavigateIntoListingFolderAsync(row);
        else
            OpenListingItem(row);
    }

    private async void OnListingListViewKeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Back)
        {
            NavigateListingBack();
            e.Handled = true;
            return;
        }

        if (e.Key == Windows.System.VirtualKey.Enter && ListingListView.SelectedItem is ListingRow row)
        {
            if (row.IsFolder)
                await NavigateIntoListingFolderAsync(row);
            else
                OpenListingItem(row);
            e.Handled = true;
        }
    }

    private async Task NavigateIntoListingFolderAsync(ListingRow row)
    {
        _currentListingPath = NormalizeListingPath(row.Path);
        RenderListing();
        await TryLoadPhysicalFolderLevelAsync(row, _currentListingPath, _previewGeneration);
    }

    private void NavigateListingBack()
    {
        if (string.IsNullOrEmpty(_currentListingPath))
            return;

        string current = _currentListingPath.TrimEnd('/');
        int slash = current.LastIndexOf('/');
        _currentListingPath = slash < 0 ? "" : current[..(slash + 1)];
        RenderListing();
    }

    private void OpenListingItem(ListingRow row)
    {
        if (string.IsNullOrWhiteSpace(row.NativePath))
            return;

        try
        {
            System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(row.NativePath)
            {
                UseShellExecute = true,
            });
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "open listing item failed: " + ex);
        }
    }

    private async Task TryLoadPhysicalFolderLevelAsync(ListingRow row, string parentPath, int generation)
    {
        if (_currentListing is null
            || !_currentListing.ListingKind.Equals("folder", StringComparison.OrdinalIgnoreCase)
            || string.IsNullOrWhiteSpace(row.NativePath))
        {
            return;
        }

        bool alreadyLoaded = _currentListing.Items.Any(i =>
            string.Equals(NormalizeListingPath(i.ParentPath), parentPath, StringComparison.OrdinalIgnoreCase));
        if (alreadyLoaded)
            return;

        try
        {
            ListingSummary.Text = "正在读取文件夹...";
            var childListing = await Task.Run(() => _native.TryPreviewFolderListing(row.NativePath));
            if (!IsPreviewGenerationCurrent(generation) || _currentListing is null)
                return;
            if (childListing is null)
                return;

            var children = childListing.Items
                .Select(i => PrefixListingItem(parentPath, i))
                .ToArray();
            var merged = _currentListing.Items
                .Concat(children)
                .GroupBy(i => NormalizeListingPath(i.Path), StringComparer.OrdinalIgnoreCase)
                .Select(g => g.Last())
                .ToArray();

            _currentListing = _currentListing with
            {
                Items = merged,
                IsPartial = _currentListing.IsPartial || childListing.IsPartial,
            };
            if (string.Equals(_currentListingPath, parentPath, StringComparison.OrdinalIgnoreCase))
                RenderListing();
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "folder navigation failed: " + ex);
            ListingSummary.Text = "无法读取此文件夹";
        }
    }

    private static PreviewListingItem PrefixListingItem(string parentPath, PreviewListingItem item)
    {
        string itemPath = item.Path.Replace('\\', '/').TrimStart('/');
        string itemParent = item.ParentPath.Replace('\\', '/').TrimStart('/');
        return item with
        {
            Path = parentPath + itemPath,
            ParentPath = string.IsNullOrEmpty(itemParent) ? parentPath : parentPath + NormalizeListingPath(itemParent),
        };
    }

    private void AttachPdfPageSurface(PreviewSurface surface)
    {
        if (_compositor is null || !_pdfPageHosts.TryGetValue(surface.PageIndex, out var pageHost))
            return;

        var (compSurface, hr) = CompositionInterop.CreateSurfaceForHandle(_compositor, (nint)surface.SharedHandle);
        if (hr < 0 || compSurface is null)
        {
            StatusText.Text = $"pdf page failed 0x{hr:X8}";
            return;
        }

        pageHost.Width = surface.Width;
        pageHost.Height = surface.Height;
        TouchPdfPage(surface.PageIndex);
        // Dispose the previous sprite on this page host before attaching a new one.
        var oldChild = ElementCompositionPreview.GetElementChildVisual(pageHost);
        if (oldChild is SpriteVisual oldSprite)
        {
            try { (oldSprite.Brush as IDisposable)?.Dispose(); } catch { }
            try { oldSprite.Dispose(); } catch { }
        }
        var brush = _compositor.CreateSurfaceBrush(compSurface);
        brush.Stretch = CompositionStretch.Fill;
        var sprite = _compositor.CreateSpriteVisual();
        sprite.RelativeSizeAdjustment = Vector2.One;
        sprite.Brush = brush;
        ElementCompositionPreview.SetElementChildVisual(pageHost, sprite);
        DispatcherQueue.TryEnqueue(() => pageHost.InvalidateArrange());
    }

    private void RequestVisiblePdfPages()
    {
        if (_supervisor is null || _currentRequestId is null || PdfScrollViewer.Visibility != Visibility.Visible)
            return;

        // O(1) visible-page computation: all pages share the same height (displayHeight), so
        // the visible page range is a simple arithmetic calculation from the scroll offset.
        if (_pdfPageHosts.Count == 0) return;
        int pageCount = _pdfPageHosts.Count;
        double pageHeight = _pdfPageHosts[0].ActualHeight;
        if (pageHeight <= 0) return;

        double vp = Math.Max(1, PdfScrollViewer.ViewportHeight);
        double scrollOffset = PdfScrollViewer.VerticalOffset;
        int firstVisible = (int)Math.Floor(scrollOffset / pageHeight);
        int lastVisible = (int)Math.Ceiling((scrollOffset + vp) / pageHeight);

        // Expand the render window: render a few pages above/below the viewport for smooth scrolling.
        int renderFirst = Math.Max(0, firstVisible - 1);
        int renderLast = Math.Min(pageCount - 1, lastVisible + 2);

        for (int index = renderFirst; index <= renderLast; index++)
        {
            if (!_requestedPdfPages.Contains(index))
            {
                _requestedPdfPages.Add(index);
                TouchPdfPage(index);
                _ = _supervisor.RenderPageAsync(_currentRequestId, index, _currentPdfScale);
            }
            else
            {
                TouchPdfPage(index);
            }
        }

        TrimPdfPageSurfaceCache(renderFirst, renderLast);
    }

    private void TouchPdfPage(int pageIndex)
    {
        _pdfPageLastTouched[pageIndex] = ++_pdfPageTouchTick;
    }

    private void TrimPdfPageSurfaceCache(int protectedFirst, int protectedLast)
    {
        if (_currentRequestId is null)
            return;

        int protectedCount = Math.Max(0, protectedLast - protectedFirst + 1);
        int maxSurfaces = protectedCount + PdfOffscreenSurfaceCachePages;
        if (_requestedPdfPages.Count <= maxSurfaces)
            return;

        int excess = _requestedPdfPages.Count - maxSurfaces;
        while (excess-- > 0)
        {
            int oldestIndex = -1;
            long oldestTick = long.MaxValue;
            foreach (int index in _requestedPdfPages)
            {
                if (index >= protectedFirst && index <= protectedLast)
                    continue;
                long tick = _pdfPageLastTouched.TryGetValue(index, out long value) ? value : 0;
                if (tick < oldestTick)
                {
                    oldestTick = tick;
                    oldestIndex = index;
                }
            }
            if (oldestIndex < 0)
                break;
            ReleasePdfPageSurface(oldestIndex);
        }
    }

    private void ReleasePdfPageSurface(int pageIndex)
    {
        _requestedPdfPages.Remove(pageIndex);
        _pdfPageLastTouched.Remove(pageIndex);
        if (_pdfPageHosts.TryGetValue(pageIndex, out var host))
        {
            var oldChild = ElementCompositionPreview.GetElementChildVisual(host);
            if (oldChild is SpriteVisual oldSprite)
            {
                try { (oldSprite.Brush as IDisposable)?.Dispose(); } catch { }
                try { oldSprite.Dispose(); } catch { }
            }
            ElementCompositionPreview.SetElementChildVisual(host, null);
        }
        if (_currentRequestId is not null)
            _ = _supervisor?.ClosePageAsync(_currentRequestId, pageIndex);
    }

    private void ResetPreview()
    {
        DisposeRasterSprite();
        PreviewRoot.Visibility = Visibility.Visible;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        PreviewInfoRail.Visibility = Visibility.Collapsed;
        ImagePreviewToolbar.Visibility = Visibility.Collapsed;
        ErrorPanel.Visibility = Visibility.Collapsed;
        if (!_previewRevealPending)
        {
            LoadingRing.IsActive = false;
            LoadingRing.Visibility = Visibility.Collapsed;
            PreviewContentHost.Opacity = 1;
        }
        if (MediaPreviewElement.Source is not null)
        {
            MediaPreviewElement.MediaPlayer?.Pause();
            if (MediaPreviewElement.Source is IDisposable disposableSource)
                disposableSource.Dispose();
            MediaPreviewElement.Source = null;
        }
        PdfPagesPanel.Children.Clear();
        OfficePagesPanel.Children.Clear();
        TextPreviewBlock.Blocks.Clear();
        ClearPreviewHeroImages();
        ListingListView.ItemsSource = null;
        ListingBreadcrumbPanel.Children.Clear();
        _currentListing = null;
        _currentListingPath = "";
        _pdfPageHosts.Clear();
        _requestedPdfPages.Clear();
        _pdfPageLastTouched.Clear();
        _pdfPageTouchTick = 0;
        if (_compositor is not null)
            ElementCompositionPreview.SetElementChildVisual(PreviewRoot, null);
        ResetPreviewChrome();
    }

    private void StartPreviewHeroLoad(PreviewReady ready)
    {
        string? path = _currentPath;
        if (string.IsNullOrWhiteSpace(path) || !ShouldLoadPreviewHero(ready, path))
        {
            ClearPreviewHeroImages();
            return;
        }

        int generation = _previewGeneration;
        Task.Run(() => LoadPreviewHeroRaster(ready, path)).ContinueWith(task =>
        {
            if (task.IsFaulted || task.IsCanceled || task.Result is null)
                return;

            DispatcherQueue.TryEnqueue(() =>
            {
                if (!IsPreviewGenerationCurrent(generation) || !string.Equals(_currentPath, path, StringComparison.OrdinalIgnoreCase))
                    return;

                var source = CreateBitmapSource(task.Result);
                if (source is null)
                    return;

                if (ListingPanel.Visibility == Visibility.Visible)
                {
                    ListingHeroImage.Source = source;
                    ListingHeroFrame.Visibility = Visibility.Visible;
                }
                else if (TextScrollViewer.Visibility == Visibility.Visible)
                {
                    TextHeroImage.Source = source;
                    TextHeroTitle.Text = ready.Title;
                    TextHeroSubtitle.Text = BuildPreviewHeroSubtitle(ready, path);
                    TextHeroFrame.Visibility = Visibility.Visible;
                    TextHeroPanel.Visibility = Visibility.Visible;
                }
            });
        }, TaskScheduler.Default);
    }

    private NativeRasterImage? LoadPreviewHeroRaster(PreviewReady ready, string path)
    {
        if (IsPackagePreview(ready, path))
            return _native.TryExtractPackageIcon(path) ?? _native.TryGetThumbnail(path, 512);

        if (IsOfficePreviewWithImages(ready))
            return _native.TryExtractOfficeImage(path);

        if (IsExecutablePreview(ready, path))
            return _native.TryGetThumbnail(path, 512);

        if (ready.Kind == "certificate")
            return _native.TryGetThumbnail(path, 256);

        return null;
    }

    private static bool ShouldLoadPreviewHero(PreviewReady ready, string path)
        => ready.OfficeLayout is null
           && (IsPackagePreview(ready, path)
           || IsExecutablePreview(ready, path)
           || ready.Kind == "certificate"
           || IsOfficePreviewWithImages(ready));

    private static bool IsPackagePreview(PreviewReady ready, string path)
    {
        string ext = System.IO.Path.GetExtension(path).ToLowerInvariant();
        return ready.Kind == "package"
            || ext is ".apk" or ".apks" or ".aab" or ".msix" or ".msixbundle" or ".appx" or ".appxbundle";
    }

    private static bool IsExecutablePreview(PreviewReady ready, string path)
    {
        string ext = System.IO.Path.GetExtension(path).ToLowerInvariant();
        return ready.Kind == "executable"
            || ext is ".exe" or ".dll" or ".sys" or ".scr" or ".cpl" or ".ocx";
    }

    private static bool IsOfficePreviewWithImages(PreviewReady ready)
    {
        if (ready.Kind != "office" || string.IsNullOrWhiteSpace(ready.TextContent))
            return false;

        foreach (string line in ready.TextContent.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n'))
        {
            string trimmed = line.Trim();
            if (!trimmed.StartsWith("Images:", StringComparison.OrdinalIgnoreCase))
                continue;
            return !trimmed.Equals("Images: 0", StringComparison.OrdinalIgnoreCase);
        }
        return false;
    }

    private static string BuildPreviewHeroSubtitle(PreviewReady ready, string path)
    {
        string ext = System.IO.Path.GetExtension(path).TrimStart('.').ToUpperInvariant();
        return ready.Kind switch
        {
            "office" => string.IsNullOrEmpty(ext) ? "Embedded image preview" : $"{ext} embedded image preview",
            "package" => "App package icon",
            "executable" => "Application icon",
            "certificate" => "Certificate",
            _ => ext,
        };
    }

    private static ImageSource? CreateBitmapSource(NativeRasterImage raster)
    {
        if (raster.Width <= 0 || raster.Height <= 0 || raster.Bgra.Length < raster.Width * raster.Height * 4)
            return null;

        try
        {
            var bitmap = new WriteableBitmap(raster.Width, raster.Height);
            using (var stream = bitmap.PixelBuffer.AsStream())
            {
                stream.Write(raster.Bgra, 0, raster.Width * raster.Height * 4);
            }
            bitmap.Invalidate();
            return bitmap;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "preview hero bitmap failed: " + ex.Message);
            return null;
        }
    }

    private void ClearPreviewHeroImages()
    {
        TextHeroImage.Source = null;
        TextHeroTitle.Text = "";
        TextHeroSubtitle.Text = "";
        TextHeroPanel.Visibility = Visibility.Collapsed;
        TextHeroFrame.Visibility = Visibility.Collapsed;
        ListingHeroImage.Source = null;
        ListingHeroFrame.Visibility = Visibility.Collapsed;
    }

    private (double Width, double Height) EstimateTextPreviewSize(string text, string? format, bool wrap)
    {
        var maxContent = GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight);
        string[] lines = text.Length == 0 ? [""] : text.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n');
        int lineCount = Math.Max(1, lines.Length);
        int maxLineLength = lines
            .Take(500)
            .Select(line => line.Length)
            .DefaultIfEmpty(0)
            .Max();

        bool markdown = string.Equals(format, "markdown", StringComparison.OrdinalIgnoreCase);
        bool code = !wrap && !markdown;
        double charWidth = code ? 8.2 : 7.2;
        double lineHeight = code ? 20 : 22;

        double width;
        if (wrap)
        {
            double contentWeight = Math.Sqrt(Math.Min(text.Length, MaxHighlightedChars));
            width = 500 + Math.Min(360, contentWeight * 8);
            if (markdown)
                width = Math.Max(width, 620);
        }
        else
        {
            width = Math.Min(980, Math.Max(520, Math.Min(maxLineLength, 120) * charWidth + 96));
        }

        int desiredLines = lineCount <= 12 ? lineCount : Math.Min(lineCount, code ? 30 : 26);
        double height = desiredLines * lineHeight + (markdown ? 120 : 96);

        return (
            Math.Clamp(width, 460, maxContent.Width),
            Math.Clamp(height, 260, maxContent.Height));
    }

    private (double Width, double Height) EstimateListingPreviewSize()
    {
        var maxContent = GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight);
        if (_currentListing is null)
            return (Math.Min(720, maxContent.Width), Math.Min(480, maxContent.Height));

        var visibleItems = _currentListing.Items
            .Where(i => string.Equals(NormalizeListingPath(i.ParentPath), _currentListingPath, StringComparison.OrdinalIgnoreCase))
            .ToArray();

        int visibleRows = Math.Clamp(visibleItems.Length, 4, 16);
        int longestName = visibleItems
            .Take(200)
            .Select(i => i.Name.Length)
            .DefaultIfEmpty(_currentListing.RootName.Length)
            .Max();

        double nameColumn = Math.Clamp(longestName * 7.4 + 96, 300, 520);
        double width = nameColumn + 170 + 140 + 110 + 110;
        double height = 128 + visibleRows * 36;

        return (
            Math.Clamp(width, 620, maxContent.Width),
            Math.Clamp(height, 320, maxContent.Height));
    }

    private (double Width, double Height) GetMaxContentSize(double preferredMaxWidth, double preferredMaxHeight)
        => PreviewWindowSizer.GetMaxContentSize(GetWindowId(), preferredMaxWidth, preferredMaxHeight);

    private void ResizeWindowForContent(
        double contentWidth,
        double contentHeight,
        double maxWidth,
        double maxHeight,
        bool setTopmost = true)
    {
        SizeInt32 size = PreviewWindowSizer.GetWindowSizeForContent(
            GetWindowId(),
            contentWidth,
            contentHeight,
            maxWidth,
            maxHeight);
        TemporarilyHideWindowForTransitionResize();
        GetAppWindow().Resize(size);
        if (setTopmost && _previewVisible)
            RaisePreviewWindow(activate: false);
    }

    private void CenterPreviewWindowInCurrentDisplay(AppWindow appWindow)
    {
        PointInt32? position = PreviewWindowSizer.GetCenteredPosition(GetWindowId(), appWindow.Size);
        if (position is { } point)
            appWindow.Move(point);
    }

    private void TemporarilyHideWindowForTransitionResize()
    {
        if (!_previewRevealPending || !_previewVisible || _previewTemporarilyHidden)
            return;

        try { GetAppWindow().Hide(); }
        catch { ShowWindow(WinRT.Interop.WindowNative.GetWindowHandle(this), SW_HIDE); }
        _previewTemporarilyHidden = true;
    }

    /// <summary>Dispose the current raster sprite + its brush to release the GPU composition surface.</summary>
    private void DisposeRasterSprite()
    {
        if (_rasterSprite is not null)
        {
            try { (_rasterSprite.Brush as IDisposable)?.Dispose(); } catch { }
            try { _rasterSprite.Dispose(); } catch { }
            _rasterSprite = null;
        }
    }

    private void UpdateRasterSpriteLayout()
    {
        if (_rasterSprite is null || _rasterSurfaceWidth == 0 || _rasterSurfaceHeight == 0)
            return;

        double availableWidth = Math.Max(1, PreviewRoot.ActualWidth);
        double availableHeight = Math.Max(1, PreviewRoot.ActualHeight);
        double fitScale = Math.Min(availableWidth / _rasterSurfaceWidth, availableHeight / _rasterSurfaceHeight);
        double scale = fitScale * _imageZoom;
        double scaledWidth = _rasterSurfaceWidth * scale;
        double scaledHeight = _rasterSurfaceHeight * scale;

        // Clamp pan so the image edge can't be dragged past the viewport (no pan while it fits).
        double maxPanX = Math.Max(0, (scaledWidth - availableWidth) / 2);
        double maxPanY = Math.Max(0, (scaledHeight - availableHeight) / 2);
        _imagePanX = Math.Clamp(_imagePanX, -maxPanX, maxPanX);
        _imagePanY = Math.Clamp(_imagePanY, -maxPanY, maxPanY);

        // Swapchain-backed composition surfaces don't honor CompositionSurfaceBrush stretch, so size the
        // sprite to the surface's native pixels (whole image, 1:1) and scale the visual to fit/zoom.
        _rasterSprite.Size = new Vector2(_rasterSurfaceWidth, _rasterSurfaceHeight);
        _rasterSprite.Scale = new Vector3((float)scale, (float)scale, 1f);
        _rasterSprite.Offset = new Vector3(
            (float)Math.Round((availableWidth - scaledWidth) / 2 + _imagePanX),
            (float)Math.Round((availableHeight - scaledHeight) / 2 + _imagePanY),
            0);
        UpdateImageZoomLabel();
    }

    private void UpdateImageZoomLabel()
        => ImageZoomText.Text = Math.Abs(_imageZoom - 1.0) < 0.01 ? "Fit" : $"{_imageZoom * 100:0}%";

    private void ResetImageView()
    {
        _imageZoom = 1.0;
        _imagePanX = 0;
        _imagePanY = 0;
        UpdateRasterSpriteLayout();
        UpdateImageZoomLabel();
    }

    private void OnImageZoomOutClick(object sender, RoutedEventArgs e)
    {
        if (_rasterSprite is null) return;
        _imageZoom = Math.Clamp(_imageZoom / 1.15, MinImageZoom, MaxImageZoom);
        UpdateRasterSpriteLayout();
    }

    private void OnImageZoomInClick(object sender, RoutedEventArgs e)
    {
        if (_rasterSprite is null) return;
        _imageZoom = Math.Clamp(_imageZoom * 1.15, MinImageZoom, MaxImageZoom);
        UpdateRasterSpriteLayout();
    }

    private void OnImageZoomFitClick(object sender, RoutedEventArgs e)
        => ResetImageView();

    private void OnRootGridKeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
    {
        if (_rasterSprite is null || PreviewRoot.Visibility != Visibility.Visible)
            return;

        bool controlDown = (Microsoft.UI.Input.InputKeyboardSource
            .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Control)
            & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;

        if (e.Key == Windows.System.VirtualKey.Home
            || (controlDown && e.Key is Windows.System.VirtualKey.Number0 or Windows.System.VirtualKey.NumberPad0))
        {
            ResetImageView();
            e.Handled = true;
        }
    }

    private void OnOpenFileLocationClick(object sender, RoutedEventArgs e)
    {
        string? path = _currentPath;
        if (string.IsNullOrWhiteSpace(path))
            return;

        try
        {
            if (Directory.Exists(path))
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = path,
                    UseShellExecute = true,
                });
                return;
            }

            if (System.IO.File.Exists(path))
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = "explorer.exe",
                    Arguments = "/select,\"" + path + "\"",
                    UseShellExecute = true,
                });
            }
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "open file location failed: " + ex.Message);
        }
    }

    private void OnPreviewRootPointerPressed(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (_rasterSprite is null || PreviewRoot.Visibility != Visibility.Visible) return;
        if (!e.GetCurrentPoint(PreviewRoot).Properties.IsLeftButtonPressed) return;
        _isPanning = true;
        _panStart = e.GetCurrentPoint(PreviewRoot).Position;
        _panStartX = _imagePanX;
        _panStartY = _imagePanY;
        PreviewRoot.CapturePointer(e.Pointer);
    }

    private void OnPreviewRootPointerMoved(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (!_isPanning) return;
        var p = e.GetCurrentPoint(PreviewRoot).Position;
        _imagePanX = _panStartX + (p.X - _panStart.X);
        _imagePanY = _panStartY + (p.Y - _panStart.Y);
        UpdateRasterSpriteLayout();
    }

    private void OnPreviewRootPointerReleased(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (!_isPanning) return;
        _isPanning = false;
        PreviewRoot.ReleasePointerCapture(e.Pointer);
    }

    private void OnPreviewRootPointerWheelChanged(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (_rasterSprite is null || PreviewRoot.Visibility != Visibility.Visible)
            return;

        int delta = e.GetCurrentPoint(PreviewRoot).Properties.MouseWheelDelta;
        if (delta == 0) return;

        double factor = delta > 0 ? 1.15 : 1.0 / 1.15;
        _imageZoom = Math.Clamp(_imageZoom * factor, MinImageZoom, MaxImageZoom);
        UpdateRasterSpriteLayout();
        e.Handled = true;
    }

    private void OnPreviewRootDoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
    {
        if (_rasterSprite is null || PreviewRoot.Visibility != Visibility.Visible)
            return;

        ResetImageView();
        e.Handled = true;
    }

    private void RenderMarkdown(string text)
    {
        string[] lines = NormalizeLines(text);
        bool inCode = false;
        string code = "";
        string codeLanguage = "text";
        var paragraphBuffer = new List<string>();

        void FlushParagraph()
        {
            if (paragraphBuffer.Count == 0) return;
            var p = CreateParagraph(14, "Segoe UI", 0, 8);
            AddInlineMarkdown(p, string.Join(" ", paragraphBuffer));
            TextPreviewBlock.Blocks.Add(p);
            paragraphBuffer.Clear();
        }

        foreach (string raw in lines)
        {
            string line = raw.TrimEnd('\r');
            if (line.TrimStart().StartsWith("```", StringComparison.Ordinal))
            {
                if (inCode)
                {
                    AddMarkdownCodeBlock(code.TrimEnd('\n'), codeLanguage);
                    code = "";
                    codeLanguage = "text";
                    inCode = false;
                }
                else
                {
                    FlushParagraph();
                    inCode = true;
                    codeLanguage = SyntaxHighlighter.NormalizeLanguage(line.TrimStart()[3..].Trim());
                    if (codeLanguage.Length == 0)
                        codeLanguage = "text";
                }
                continue;
            }

            if (inCode)
            {
                code += raw + "\n";
                continue;
            }

            string trimmed = line.Trim();
            if (trimmed.Length == 0)
            {
                FlushParagraph();
                continue;
            }

            if (trimmed.StartsWith("#", StringComparison.Ordinal))
            {
                FlushParagraph();
                int level = Math.Min(6, trimmed.TakeWhile(c => c == '#').Count());
                string title = trimmed[level..].Trim();
                var p = CreateParagraph(level <= 1 ? 26 : level == 2 ? 22 : 18, "Segoe UI", 14, 8);
                p.Inlines.Add(new Bold { Inlines = { new Run { Text = title } } });
                TextPreviewBlock.Blocks.Add(p);
                continue;
            }

            if (trimmed.StartsWith("> ", StringComparison.Ordinal))
            {
                FlushParagraph();
                var p = CreateParagraph(14, "Segoe UI", 4, 8);
                p.Foreground = UiGrayBrush;
                p.Inlines.Add(new Run { Text = "│ " });
                AddInlineMarkdown(p, trimmed[2..]);
                TextPreviewBlock.Blocks.Add(p);
                continue;
            }

            if (trimmed.StartsWith("- ", StringComparison.Ordinal) || trimmed.StartsWith("* ", StringComparison.Ordinal))
            {
                FlushParagraph();
                var p = CreateParagraph(14, "Segoe UI", 2, 4);
                p.Inlines.Add(new Run { Text = "• " });
                AddInlineMarkdown(p, trimmed[2..]);
                TextPreviewBlock.Blocks.Add(p);
                continue;
            }

            paragraphBuffer.Add(trimmed);
        }

        FlushParagraph();
        if (inCode && code.Length > 0)
            AddMarkdownCodeBlock(code.TrimEnd('\n'), codeLanguage);
    }

    private void AddMarkdownCodeBlock(string code, string language)
    {
        if (language is "text" or "log")
            AddCodeBlock(code);
        else
            AddHighlightedCode(code, language);
    }

    private void RenderCodeOrPlainText(string text, string language)
    {
        var header = CreateParagraph(12, "Segoe UI", 0, 10);
        header.Foreground = UiGrayBrush;
        header.Inlines.Add(new Run { Text = language });
        TextPreviewBlock.Blocks.Add(header);

        string code = text.TrimEnd('\r', '\n');
        if (language is "text" or "log")
            AddCodeBlock(code);          // plain text: no highlighting
        else
            AddHighlightedCode(code, language);
    }

    private void AddHighlightedCode(string code, string language)
    {
        var p = CreateParagraph(13, "Cascadia Mono, Consolas", 2, 10);
        if (code.Length == 0)
        {
            p.Inlines.Add(new Run { Text = " " });
        }
        else if (code.Length > MaxHighlightedChars)
        {
            p.Foreground = BrushFor(TokenKind.Default);
            p.Inlines.Add(new Run
            {
                Text = code[..MaxHighlightedChars]
                    + $"\n\n[Syntax highlighting disabled after {MaxHighlightedChars:N0} characters]",
            });
        }
        else
        {
            int runs = 0;
            foreach (var (txt, kind) in SyntaxHighlighter.Highlight(code, language))
            {
                if (txt.Length == 0) continue;
                if (++runs > MaxHighlightedRuns)
                {
                    DiagLog.Write("App", $"highlight run limit hit: language={language}; chars={code.Length}; runs>{MaxHighlightedRuns}");
                    p.Inlines.Clear();
                    p.Foreground = BrushFor(TokenKind.Default);
                    p.Inlines.Add(new Run { Text = code + $"\n\n[Syntax highlighting disabled after {MaxHighlightedRuns:N0} spans]" });
                    break;
                }

                p.Inlines.Add(new Run { Text = txt, Foreground = BrushFor(kind) });
            }
        }
        TextPreviewBlock.Blocks.Add(p);
    }

    private SolidColorBrush BrushFor(TokenKind kind)
    {
        bool dark = RootGrid.ActualTheme != ElementTheme.Light;
        if (_brushThemeDark != dark) { _tokenBrushes.Clear(); _brushThemeDark = dark; }
        if (_tokenBrushes.TryGetValue(kind, out var brush))
            return brush;

        Windows.UI.Color c = kind switch
        {
            TokenKind.Keyword => dark ? Rgb(0x56, 0x9C, 0xD6) : Rgb(0x00, 0x00, 0xFF),
            TokenKind.Str => dark ? Rgb(0xCE, 0x91, 0x78) : Rgb(0xA3, 0x15, 0x15),
            TokenKind.Comment => dark ? Rgb(0x6A, 0x99, 0x55) : Rgb(0x00, 0x80, 0x00),
            TokenKind.Number => dark ? Rgb(0xB5, 0xCE, 0xA8) : Rgb(0x09, 0x86, 0x58),
            TokenKind.Type => dark ? Rgb(0x4E, 0xC9, 0xB0) : Rgb(0x2B, 0x91, 0xAF),
            TokenKind.Property => dark ? Rgb(0x9C, 0xDC, 0xFE) : Rgb(0x00, 0x16, 0x80),
            TokenKind.Punctuation => dark ? Rgb(0xD4, 0xD4, 0xD4) : Rgb(0x39, 0x39, 0x39),
            _ => ThemeTextColor(),
        };
        brush = new SolidColorBrush(c);
        _tokenBrushes[kind] = brush;
        return brush;
    }

    private static Windows.UI.Color Rgb(byte r, byte g, byte b) => ColorHelper.FromArgb(255, r, g, b);

    private static Windows.UI.Color ThemeTextColor()
    {
        try { return (Windows.UI.Color)Application.Current.Resources["TextFillColorPrimary"]; }
        catch { return Colors.Gainsboro; }
    }

    private void AddCodeBlock(string code)
    {
        code = TrimForDisplay(code);
        var p = CreateParagraph(13, "Cascadia Mono, Consolas", 2, 10);
        p.Foreground = BrushFor(TokenKind.Default);
        p.Inlines.Add(new Run { Text = code.Length == 0 ? " " : code });
        TextPreviewBlock.Blocks.Add(p);
    }

    private static string TrimForDisplay(string text)
        => text.Length <= MaxHighlightedChars
            ? text
            : text[..MaxHighlightedChars] + $"\n\n[Preview truncated at {MaxHighlightedChars:N0} characters]";

    private static string NormalizeListingPath(string path)
    {
        path = path.Replace('\\', '/').TrimStart('/');
        return path.Length > 0 && !path.EndsWith('/') ? path + "/" : path;
    }

    private string BuildListingSummary(PreviewListing listing, IReadOnlyCollection<PreviewListingItem> visibleItems)
    {
        if (string.IsNullOrEmpty(_currentListingPath))
            return listing.Summary + (listing.IsPartial ? " - 部分内容" : "");

        int folders = visibleItems.Count(i => i.IsFolder);
        int files = visibleItems.Count - folders;
        long bytes = visibleItems.Where(i => !i.IsFolder).Sum(i => i.Size);
        return $"{files:N0} files, {folders:N0} folders - {FormatBytes(bytes)}";
    }

    internal static string FormatBytes(long bytes)
    {
        double value = bytes;
        int unit = 0;
        while (value >= 1024 && unit < ByteUnits.Length - 1)
        {
            value /= 1024;
            unit++;
        }
        return unit == 0 ? $"{bytes:N0} B" : $"{value:0.##} {ByteUnits[unit]}";
    }

    private static Paragraph CreateParagraph(double fontSize, string fontFamily, double top, double bottom)
        => new()
        {
            FontSize = fontSize,
            FontFamily = FontFamilyFor(fontFamily),
            Margin = new Thickness(0, top, 0, bottom),
        };

    private static FontFamily FontFamilyFor(string fontFamily)
    {
        if (!FontFamilyCache.TryGetValue(fontFamily, out var cached))
        {
            cached = new FontFamily(fontFamily);
            FontFamilyCache[fontFamily] = cached;
        }
        return cached;
    }

    private static string[] NormalizeLines(string text)
        => text.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n');

    private void AddInlineMarkdown(Paragraph paragraph, string text)
    {
        int i = 0;
        while (i < text.Length)
        {
            if (text[i] == '`')
            {
                int end = text.IndexOf('`', i + 1);
                if (end > i)
                {
                    paragraph.Inlines.Add(new Run
                    {
                        Text = text[(i + 1)..end],
                        FontFamily = FontFamilyFor("Cascadia Mono, Consolas"),
                        Foreground = BrushFor(TokenKind.Keyword),
                    });
                    i = end + 1;
                    continue;
                }
            }
            if (i + 1 < text.Length && text[i] == '*' && text[i + 1] == '*')
            {
                int end = text.IndexOf("**", i + 2, StringComparison.Ordinal);
                if (end > i)
                {
                    paragraph.Inlines.Add(new Bold { Inlines = { new Run { Text = text[(i + 2)..end] } } });
                    i = end + 2;
                    continue;
                }
            }
            if (text[i] == '*')
            {
                int end = text.IndexOf('*', i + 1);
                if (end > i)
                {
                    paragraph.Inlines.Add(new Italic { Inlines = { new Run { Text = text[(i + 1)..end] } } });
                    i = end + 1;
                    continue;
                }
            }

            int next = NextMarkdownToken(text, i + 1);
            paragraph.Inlines.Add(new Run { Text = text[i..next] });
            i = next;
        }
    }

    private static int NextMarkdownToken(string text, int start)
    {
        int next = text.Length;
        foreach (char c in new[] { '`', '*' })
        {
            int at = text.IndexOf(c, start);
            if (at >= 0 && at < next) next = at;
        }
        return next;
    }

    private void EnsureCompositor()
    {
        _compositor ??= ElementCompositionPreview.GetElementVisual(PreviewRoot).Compositor;
    }


    private void TrySetBackdrop()
    {
        try
        {
            SystemBackdrop = new MicaBackdrop();
        }
        catch { /* no backdrop on older systems */ }
    }

    private void UpdateTitleBarColors()
    {
        try
        {
            if (AppWindowTitleBar.IsCustomizationSupported())
            {
                var titleBar = GetAppWindow().TitleBar;
                bool isDark = RootGrid.ActualTheme == ElementTheme.Dark;

                if (isDark)
                {
                    titleBar.ButtonForegroundColor = Microsoft.UI.Colors.White;
                    titleBar.ButtonHoverForegroundColor = Microsoft.UI.Colors.White;
                    titleBar.ButtonHoverBackgroundColor = Windows.UI.Color.FromArgb(0x1F, 0xFF, 0xFF, 0xFF);
                    titleBar.ButtonPressedForegroundColor = Microsoft.UI.Colors.White;
                    titleBar.ButtonPressedBackgroundColor = Windows.UI.Color.FromArgb(0x3F, 0xFF, 0xFF, 0xFF);
                    titleBar.ButtonInactiveForegroundColor = Microsoft.UI.Colors.DarkGray;
                    titleBar.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
                }
                else
                {
                    titleBar.ButtonForegroundColor = Microsoft.UI.Colors.Black;
                    titleBar.ButtonHoverForegroundColor = Microsoft.UI.Colors.Black;
                    titleBar.ButtonHoverBackgroundColor = Windows.UI.Color.FromArgb(0x1F, 0, 0, 0);
                    titleBar.ButtonPressedForegroundColor = Microsoft.UI.Colors.Black;
                    titleBar.ButtonPressedBackgroundColor = Windows.UI.Color.FromArgb(0x3F, 0, 0, 0);
                    titleBar.ButtonInactiveForegroundColor = Microsoft.UI.Colors.Gray;
                    titleBar.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
                }

                titleBar.ButtonBackgroundColor = Microsoft.UI.Colors.Transparent;
                titleBar.ButtonInactiveBackgroundColor = Microsoft.UI.Colors.Transparent;
            }
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "Failed to customize title bar: " + ex.Message);
        }
    }

    private void ShowPreviewWindow(bool activate, bool resizeToDefault = true)
    {
        bool openingFromHidden = !_previewVisible;
        if (activate)
            SetNoActivateStyle(enabled: false);
        else
            ApplyNoActivateStyle();
        var appWindow = GetAppWindow();
        if (!_previewVisible && resizeToDefault)
            ResizeWindowForContent(560, 340, MaxTextWindowWidth, MaxTextWindowHeight, setTopmost: false);
        if (openingFromHidden)
            CenterPreviewWindowInCurrentDisplay(appWindow);
        try { appWindow.Show(activate); }
        catch
        {
            if (activate) Activate();
            else ShowWindowNoActivate(WinRT.Interop.WindowNative.GetWindowHandle(this));
        }
        RaisePreviewWindow(activate);
        EnsureCompositor();
        _previewVisible = true;
        SetBackgroundEfficiency(enabled: false);
        _native.SetPreviewVisible(true);
        PreviewContentHost.Opacity = 1;
    }

    private void HidePreviewWindow()
    {
        CancelSwitchDebounce();
        _previewRevealPending = false;
        _previewTemporarilyHidden = false;
        LoadingRing.IsActive = false;
        LoadingRing.Visibility = Visibility.Collapsed;
        ErrorPanel.Visibility = Visibility.Collapsed;
        PreviewContentHost.Opacity = 1;
        try { GetAppWindow().Hide(); }
        catch { ShowWindow(WinRT.Interop.WindowNative.GetWindowHandle(this), SW_HIDE); }
        ReleasePreviewTopmost();
        _previewVisible = false;
        SetBackgroundEfficiency(enabled: true);
        _native.SetPreviewVisible(false);
    }

    private void SetBackgroundEfficiency(bool enabled)
    {
        if (_backgroundEfficiencyEnabled == enabled)
            return;

        _backgroundEfficiencyEnabled = enabled;
        ProcessPowerMode.SetCurrentBackgroundEfficiency(enabled, "App");
        _supervisor?.SetBackgroundEfficiency(enabled);
    }

    private AppWindow GetAppWindow()
        => AppWindow.GetFromWindowId(GetWindowId());

    private WindowId GetWindowId()
        => Win32Interop.GetWindowIdFromWindow(WinRT.Interop.WindowNative.GetWindowHandle(this));

    private void RaisePreviewWindow(bool activate)
    {
        nint hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        uint flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
        if (!activate)
            flags |= SWP_NOACTIVATE;

        // Pulse topmost only long enough to place the preview above Explorer, then immediately
        // release it so other apps can cover the preview normally.
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
        SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
        if (activate)
            Activate();
    }

    private void ReleasePreviewTopmost()
    {
        nint hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }

    private void EnsureTrayIcon()
    {
        _trayIcon ??= new TrayIconManager(
            WinRT.Interop.WindowNative.GetWindowHandle(this),
            ResolveAppIconPath,
            () => ShowPreviewWindow(activate: true),
            ExitApp,
            message => StatusText.Text = message);
        _trayIcon.Ensure();
    }

    private void RemoveTrayIcon()
        => _trayIcon?.Remove();

    private void ApplyWindowIcon()
    {
        string iconPath = ResolveAppIconPath();
        if (!System.IO.File.Exists(iconPath))
            return;

        try { GetAppWindow().SetIcon(iconPath); }
        catch (Exception ex) { DiagLog.Write("App", "window icon failed: " + ex.Message); }
    }

    private void RefreshTrayIcon()
        => _trayIcon?.Refresh();

    private void ShowTrayBalloon(string title, string message)
        => _trayIcon?.ShowBalloon(title, message);

    private void ExitApp()
    {
        RemoveTrayIcon();
        _supervisor?.Stop();
        try { Microsoft.UI.Xaml.Application.Current.Exit(); }
        catch (Exception ex) { DiagLog.Write("App", "graceful exit failed: " + ex); }
    }

    private void ApplyNoActivateStyle()
        => SetNoActivateStyle(enabled: true);

    private void SetNoActivateStyle(bool enabled)
    {
        nint hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        nint ex = GetWindowLongPtr(hwnd, GWL_EXSTYLE);
        nint next = enabled ? ex | WS_EX_NOACTIVATE : ex & ~WS_EX_NOACTIVATE;
        if (next != ex)
            SetWindowLongPtr(hwnd, GWL_EXSTYLE, next);
    }

    private static void ShowWindowNoActivate(nint hwnd)
    {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }

    private const int GWL_EXSTYLE = -20;
    private const int SW_HIDE = 0;
    private const int SW_SHOWNOACTIVATE = 4;
    private const uint SWP_NOSIZE = 0x0001;
    private const uint SWP_NOMOVE = 0x0002;
    private const uint SWP_NOACTIVATE = 0x0010;
    private const uint SWP_SHOWWINDOW = 0x0040;
    private static readonly nint HWND_TOPMOST = new(-1);
    private static readonly nint HWND_NOTOPMOST = new(-2);
    private static readonly nint WS_EX_NOACTIVATE = new(0x08000000);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW", SetLastError = true)]
    private static extern nint GetWindowLongPtr(nint hWnd, int nIndex);

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW", SetLastError = true)]
    private static extern nint SetWindowLongPtr(nint hWnd, int nIndex, nint dwNewLong);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool ShowWindow(nint hWnd, int nCmdShow);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetWindowPos(nint hWnd, nint hWndInsertAfter, int x, int y, int cx, int cy, uint flags);

    private static FileProbe BuildProbe(string path)
    {
        if (Directory.Exists(path))
        {
            long modified = 0;
            try { modified = new DateTimeOffset(new DirectoryInfo(path).LastWriteTimeUtc).ToUnixTimeSeconds(); } catch { }
            return new FileProbe(path, "", [])
            {
                Kind = "folder",
                ModifiedUnix = modified,
            };
        }

        byte[] magic = new byte[16];
        try
        {
            using var fs = System.IO.File.OpenRead(path);
            int n = fs.Read(magic, 0, magic.Length);
            if (n < magic.Length) Array.Resize(ref magic, n);
        }
        catch { /* probe is best-effort in the scaffold; the real probe comes from native */ }
        return new FileProbe(path, System.IO.Path.GetExtension(path), magic);
    }

    private static bool IsMediaProbe(FileProbe probe)
        => probe.Kind.Equals("video", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("audio", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("media", StringComparison.OrdinalIgnoreCase);

    private static string ResolveHostExePath()
    {
        // Deployed layout: RasterHost in a subfolder next to the App.
        string rasterHost = System.IO.Path.Combine(AppContext.BaseDirectory, "RasterHost", "QuickLook.Next.RasterHost.exe");
        if (System.IO.File.Exists(rasterHost)) return rasterHost;
        string local = System.IO.Path.Combine(AppContext.BaseDirectory, "QuickLook.Next.RasterHost.exe");
        if (System.IO.File.Exists(local)) return local;
        // dev fallback: sibling project build output (…/src/QuickLook.Next.App/bin/<cfg>/<tfm>/<rid> → up 5 to src)
        return System.IO.Path.GetFullPath(System.IO.Path.Combine(AppContext.BaseDirectory,
            @"..\..\..\..\..\QuickLook.Next.RasterHost\bin\Debug\net10.0-windows10.0.19041.0\win-x64\QuickLook.Next.RasterHost.exe"));
    }

    private string ResolveAppIconPath()
    {
        string fileName = RootGrid.ActualTheme == ElementTheme.Light
            ? "QuickLookNextLight.ico"
            : "QuickLookNextDark.ico";
        string themedPath = System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", fileName);
        return System.IO.File.Exists(themedPath)
            ? themedPath
            : System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "QuickLookNext.ico");
    }
}
