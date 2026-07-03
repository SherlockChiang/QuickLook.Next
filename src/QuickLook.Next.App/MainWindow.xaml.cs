using System.Numerics;
using System.IO;
using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI;
using Microsoft.UI.Composition;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.ApplicationModel.DataTransfer;
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
    private const double RasterInfoRailWidth = 246;
    private const double RasterToolbarHeight = 82;
    private const int SwitchDebounceMs = 110;

    private readonly NativeBridge _native = new();
    private readonly PreviewWindowController _windowController;
    private TextPreviewPresenter? _textPresenter;
    private ListingPreviewPresenter? _listingPresenter;
    private OfficePreviewPresenter? _officePresenter;
    private RasterPreviewPresenter? _rasterPresenter;
    private PdfPreviewPresenter? _pdfPresenter;
    private MediaPreviewPresenter? _mediaPresenter;
    private Compositor? _compositor;
    private TrayIconManager? _trayIcon;
    private RasterHostSupervisor? _supervisor;
    private string? _currentRequestId;
    private string? _currentPath;
    private bool _isStarted;
    private bool _previewVisible;
    private int _previewGeneration;
    private bool? _backgroundEfficiencyEnabled;
    private CancellationTokenSource? _switchDebounceCts;
    private CancellationTokenSource? _previewOperationCts;
    private bool _previewRevealPending;
    private bool _previewTemporarilyHidden;

    private static readonly string[] ByteUnits = ["B", "KB", "MB", "GB", "TB"];

    // Show the top status text (file name / errors) only while debugging; normal use is chromeless.
    private const bool ShowStatusBar = false;

    public MainWindow()
    {
        InitializeComponent();
        _windowController = new PreviewWindowController(this, () => WinRT.Interop.WindowNative.GetWindowHandle(this));
        _textPresenter = new TextPreviewPresenter(TextPreviewBlock, TextScrollViewer, () => RootGrid.ActualTheme);
        _officePresenter = new OfficePreviewPresenter(OfficeScrollViewer, OfficePagesPanel);
        _rasterPresenter = new RasterPreviewPresenter(PreviewRoot, ImageZoomText);
        _pdfPresenter = new PdfPreviewPresenter(
            PdfScrollViewer,
            PdfPagesPanel,
            DispatcherQueue,
            () => _compositor,
            () => _supervisor);
        _mediaPresenter = new MediaPreviewPresenter(MediaPreviewElement);
        _listingPresenter = new ListingPreviewPresenter(
            ListingTitle,
            ListingSummary,
            ListingBreadcrumbPanel,
            ListingListView,
            ListingNameHeader,
            ListingModifiedHeader,
            ListingTypeHeader,
            ListingSizeHeader,
            path => _native.TryPreviewFolderListing(path),
            () => _previewGeneration,
            () => CurrentPreviewToken,
            IsPreviewGenerationCurrent,
            OpenListingItem,
            LoadListingIconAsync);
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        Title = UiStrings.AppName;
        TrySetBackdrop();
        PreviewRoot.SizeChanged += OnRootSizeChanged;
        PreviewRoot.PointerWheelChanged += OnPreviewRootPointerWheelChanged;
        PreviewRoot.PointerPressed += OnPreviewRootPointerPressed;
        PreviewRoot.PointerMoved += OnPreviewRootPointerMoved;
        PreviewRoot.PointerReleased += OnPreviewRootPointerReleased;
        PreviewRoot.DoubleTapped += OnPreviewRootDoubleTapped;
        RootGrid.KeyDown += OnRootGridKeyDown;
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
        _listingPresenter.UpdateSortHeaders();
    }

    public async Task StartBackgroundAsync()
    {
        if (_isStarted) return;
        _isStarted = true;

        DiagLog.Write("App", $"background start; pid={Environment.ProcessId}");
        SetBackgroundEfficiency(enabled: true);
        StatusBar.Visibility = ShowStatusBar ? Visibility.Visible : Visibility.Collapsed;
        AutoStart.RepairIfConfigured();
        ApplyWindowIcon();
        EnsureTrayIcon();
        _windowController.ApplyNoActivateStyle();

        try
        {
            _supervisor = new RasterHostSupervisor(ResolveHostExePath(), DispatcherQueue);
            _supervisor.SetBackgroundEfficiency(_backgroundEfficiencyEnabled ?? true);
            _supervisor.SurfaceReceived += OnSurfaceReceived;
            _native.Start(OnNativeIntent);
            StatusText.Text = UiStrings.Ready.ToLowerInvariant();
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
            if (_rasterPresenter?.HasSurface == true && PreviewRoot.Visibility == Visibility.Visible)
                _rasterPresenter.ZoomBy(intent.Intent == PreviewIntent.ZoomIn ? 1.15 : 1.0 / 1.15);
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

                if (MediaPreviewPresenter.IsMediaProbe(probe))
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
                StatusText.Text = ShowErrorPreview(UiStrings.PreviewTimedOut);
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
        PreviewContentHost.IsHitTestVisible = false;
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
                _windowController.SetNoActivateStyle(enabled: false);
                Activate();
            }
            else
            {
                _windowController.ApplyNoActivateStyle();
            }
            _windowController.Raise(activate);
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
            title = UiStrings.AppName;

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
        PreviewPathText.Text = string.IsNullOrWhiteSpace(path) ? UiStrings.EmptyValue : path;
        _rasterPresenter?.UpdateZoomLabel();
    }

    private void ResetPreviewChrome()
    {
        Title = UiStrings.AppName;
        PreviewTitleText.Text = UiStrings.AppName;
        PreviewMetaText.Text = UiStrings.Ready;
        PreviewKindPillText.Text = UiStrings.ReadyKind;
        PreviewInfoRail.Visibility = Visibility.Collapsed;
        ImagePreviewToolbar.Visibility = Visibility.Collapsed;
        PreviewRoot.Margin = new Thickness(14, 0, 14, 14);
        PreviewDimensionsText.Text = UiStrings.EmptyValue;
        PreviewSizeText.Text = UiStrings.EmptyValue;
        PreviewTypeText.Text = UiStrings.EmptyValue;
        PreviewModifiedText.Text = UiStrings.EmptyValue;
        PreviewPathText.Text = UiStrings.EmptyValue;
        ImageZoomText.Text = UiStrings.FitZoom;
    }

    private static string BuildPreviewMetaLine(PreviewReady ready, string? path)
    {
        var parts = new List<string>();
        string dimensions = BuildDimensionsText(ready);
        if (dimensions != UiStrings.EmptyValue)
            parts.Add(dimensions);
        string size = FileSizeText(path);
        if (size != UiStrings.EmptyValue)
            parts.Add(size);
        parts.Add(PreviewTypeTextFor(ready, path));
        string modified = ModifiedText(path);
        if (modified != UiStrings.EmptyValue)
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
        return UiStrings.EmptyValue;
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
        return UiStrings.EmptyValue;
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
        return UiStrings.EmptyValue;
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
        ErrorText.Text = string.IsNullOrWhiteSpace(message) ? UiStrings.PreviewUnavailableMessage : message;
        ErrorPanel.Visibility = Visibility.Visible;
        PreviewTitleText.Text = UiStrings.PreviewUnavailableTitle;
        PreviewMetaText.Text = ErrorText.Text;
        PreviewKindPillText.Text = "ERROR";
        ResizeWindowForContent(520, 260, MaxTextWindowWidth, MaxTextWindowHeight);
        return "error: " + ErrorText.Text;
    }

    private void FadeInPreviewContent()
    {
        PreviewContentHost.Opacity = 1;
        PreviewContentHost.IsHitTestVisible = true;
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
            if (_pdfPresenter?.AttachSurface(surface, out string? pdfError) == false)
                StatusText.Text = pdfError ?? UiStrings.PdfPageFailed;
            return;
        }

        if (!_rasterPresenter!.AttachSurface(_compositor, surface, out string? error))
        {
            StatusText.Text = error ?? UiStrings.SurfaceFailed;
            return;
        }
        DispatcherQueue.TryEnqueue(_rasterPresenter.UpdateLayout);
    }

    private void OnRootSizeChanged(object sender, SizeChangedEventArgs e)
        => _rasterPresenter?.UpdateLayout();

    private string ShowRasterPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready, showRasterTools: true);
        PreviewRoot.Visibility = Visibility.Visible;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        RasterPreviewResult result = _rasterPresenter!.Render(ready, GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxImageWindowWidth, MaxImageWindowHeight);
        DispatcherQueue.TryEnqueue(_rasterPresenter.UpdateLayout);
        return result.Status;
    }

    private string ShowPdfDocument(string requestId, PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Visible;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        _rasterPresenter?.Clear();
        PdfPreviewResult result = _pdfPresenter!.Render(requestId, ready, GetMaxContentSize(MaxPdfWindowWidth, MaxPdfWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxPdfWindowWidth, MaxPdfWindowHeight);
        return result.Status;
    }

    private string ShowTextPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Visible;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        _rasterPresenter?.Clear();

        TextPreviewResult result = _textPresenter!.Render(ready, GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight));
        StartPreviewHeroLoad(ready);
        ResizeWindowForContent(result.Width, result.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return result.Status;
    }

    private string ShowOfficeLayoutPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Visible;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Collapsed;
        _rasterPresenter?.Clear();

        OfficePreviewResult result = _officePresenter!.Render(ready, GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return result.Status;
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
        _rasterPresenter?.Clear();

        MediaPreviewResult result = _mediaPresenter!.Render(ready, GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxImageWindowWidth, MaxImageWindowHeight);
        return result.Status;
    }

    private string ShowListingPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        PreviewRoot.Visibility = Visibility.Collapsed;
        PdfScrollViewer.Visibility = Visibility.Collapsed;
        TextScrollViewer.Visibility = Visibility.Collapsed;
        OfficeScrollViewer.Visibility = Visibility.Collapsed;
        MediaPreviewElement.Visibility = Visibility.Collapsed;
        ListingPanel.Visibility = Visibility.Visible;
        _rasterPresenter?.Clear();

        ListingPreviewResult result = _listingPresenter!.Render(ready, GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight));
        StartPreviewHeroLoad(ready);
        ResizeWindowForContent(result.Width, result.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return result.Status;
    }

    private void OnListingSortClick(object sender, RoutedEventArgs e)
        => _listingPresenter?.OnSortClick(sender);

    private void OnListingItemClick(object sender, ItemClickEventArgs e)
        => _listingPresenter?.OnItemClick(e);

    private async void OnListingListViewDoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        => await (_listingPresenter?.OnDoubleTappedAsync() ?? Task.CompletedTask);

    private async void OnListingListViewKeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
        => await (_listingPresenter?.OnKeyDownAsync(e) ?? Task.CompletedTask);

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

    private async Task<ImageSource?> LoadListingIconAsync(ListingRow row, int generation)
    {
        string? path = row.NativePath;
        if (string.IsNullOrWhiteSpace(path))
            return null;

        try
        {
            CancellationToken token = CurrentPreviewToken;
            NativeRasterImage? raster = await Task.Run(() =>
            {
                if (token.IsCancellationRequested)
                    return null;
                return _native.TryGetThumbnail(path, 32);
            }, token);

            if (!IsPreviewGenerationCurrent(generation, token) || raster is null)
                return null;

            return CreateBitmapSource(raster);
        }
        catch (OperationCanceledException)
        {
            return null;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "listing icon load failed: " + ex.Message);
            return null;
        }
    }

    private void ResetPreview()
    {
        _rasterPresenter?.Clear();
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
        _mediaPresenter?.Clear();
        _pdfPresenter?.Clear();
        OfficePagesPanel.Children.Clear();
        TextPreviewBlock.Blocks.Clear();
        ClearPreviewHeroImages();
        _listingPresenter?.Reset();
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
            "office" => string.IsNullOrEmpty(ext) ? UiStrings.OfficeEmbeddedImagePreview : $"{ext} embedded image preview",
            "package" => UiStrings.PackageHeroSubtitle,
            "executable" => UiStrings.ExecutableHeroSubtitle,
            "certificate" => UiStrings.CertificateHeroSubtitle,
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
            _windowController.Raise(activate: false);
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
        catch { _windowController.Hide(); }
        _previewTemporarilyHidden = true;
    }

    private void OnImageZoomOutClick(object sender, RoutedEventArgs e)
        => _rasterPresenter?.ZoomBy(1.0 / 1.15);

    private void OnImageZoomInClick(object sender, RoutedEventArgs e)
        => _rasterPresenter?.ZoomBy(1.15);

    private void OnImageZoomFitClick(object sender, RoutedEventArgs e)
        => _rasterPresenter?.ResetView();

    private void OnImageZoomPresetClick(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string raw } && double.TryParse(raw, out double zoom))
            _rasterPresenter?.SetZoom(zoom);
    }

    private void OnRootGridKeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
    {
        if (_rasterPresenter?.HasSurface != true || PreviewRoot.Visibility != Visibility.Visible)
            return;

        bool controlDown = (Microsoft.UI.Input.InputKeyboardSource
            .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Control)
            & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;

        if (e.Key == Windows.System.VirtualKey.Home
            || (controlDown && e.Key is Windows.System.VirtualKey.Number0 or Windows.System.VirtualKey.NumberPad0))
        {
            _rasterPresenter.ResetView();
            e.Handled = true;
        }
    }

    private void OnOpenFileLocationClick(object sender, RoutedEventArgs e)
        => OpenCurrentPreviewPath(revealInExplorer: true);

    private void OnOpenPreviewFileClick(object sender, RoutedEventArgs e)
        => OpenCurrentPreviewPath(revealInExplorer: false);

    private void OnCopyPreviewPathClick(object sender, RoutedEventArgs e)
    {
        string? path = _currentPath;
        if (string.IsNullOrWhiteSpace(path))
            return;

        try
        {
            var package = new DataPackage();
            package.SetText(path);
            Clipboard.SetContent(package);
            StatusText.Text = UiStrings.PathCopied;
            StatusBar.Visibility = Visibility.Visible;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "copy preview path failed: " + ex.Message);
        }
    }

    private void OpenCurrentPreviewPath(bool revealInExplorer)
    {
        string? path = _currentPath;
        if (string.IsNullOrWhiteSpace(path))
            return;

        try
        {
            if (revealInExplorer && System.IO.File.Exists(path))
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = "explorer.exe",
                    Arguments = "/select,\"" + path + "\"",
                    UseShellExecute = true,
                });
                return;
            }

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
                    FileName = path,
                    UseShellExecute = true,
                });
            }
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "open preview path failed: " + ex.Message);
        }
    }

    private void OnPreviewRootPointerPressed(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _rasterPresenter?.OnPointerPressed(e);

    private void OnPreviewRootPointerMoved(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _rasterPresenter?.OnPointerMoved(e);

    private void OnPreviewRootPointerReleased(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _rasterPresenter?.OnPointerReleased(e);

    private void OnPreviewRootPointerWheelChanged(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _rasterPresenter?.OnPointerWheelChanged(e);

    private void OnPreviewRootDoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        => _rasterPresenter?.OnDoubleTapped(e);

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
            _windowController.SetNoActivateStyle(enabled: false);
        else
            _windowController.ApplyNoActivateStyle();
        var appWindow = GetAppWindow();
        if (!_previewVisible && resizeToDefault)
            ResizeWindowForContent(560, 340, MaxTextWindowWidth, MaxTextWindowHeight, setTopmost: false);
        if (openingFromHidden)
            CenterPreviewWindowInCurrentDisplay(appWindow);
        try { appWindow.Show(activate); }
        catch
        {
            if (activate) Activate();
            else _windowController.ShowNoActivate();
        }
        _windowController.Raise(activate);
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
        PreviewContentHost.IsHitTestVisible = true;
        try { GetAppWindow().Hide(); }
        catch { _windowController.Hide(); }
        _windowController.ReleaseTopmost();
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

    private void EnsureTrayIcon()
    {
        _trayIcon ??= new TrayIconManager(
            WinRT.Interop.WindowNative.GetWindowHandle(this),
            ResolveAppIconPath,
            () => ShowPreviewWindow(activate: true),
            ShowTrayContextMenu,
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

    private void ShowTrayContextMenu(int screenX, int screenY)
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            var flyout = new MenuFlyout();

            var showItem = new MenuFlyoutItem { Text = UiStrings.TrayShowPreview };
            showItem.Click += (_, _) => ShowPreviewWindow(activate: true);
            flyout.Items.Add(showItem);

            var autoStartItem = new ToggleMenuFlyoutItem
            {
                Text = UiStrings.TrayAutoStart,
                IsChecked = AutoStart.IsEnabled(),
            };
            autoStartItem.Click += (_, _) => _trayIcon?.ToggleAutoStart();
            flyout.Items.Add(autoStartItem);

            var exitItem = new MenuFlyoutItem { Text = UiStrings.TrayExit };
            exitItem.Click += (_, _) => ExitApp();
            flyout.Items.Add(exitItem);

            var options = new Microsoft.UI.Xaml.Controls.Primitives.FlyoutShowOptions
            {
                Position = _windowController.ScreenToClientPoint(
                    screenX,
                    screenY,
                    RootGrid.XamlRoot?.RasterizationScale ?? 1.0),
            };
            flyout.ShowAt(RootGrid, options);
        });
    }

    private void ExitApp()
    {
        RemoveTrayIcon();
        _supervisor?.Stop();
        try { Microsoft.UI.Xaml.Application.Current.Exit(); }
        catch (Exception ex) { DiagLog.Write("App", "graceful exit failed: " + ex); }
    }

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
