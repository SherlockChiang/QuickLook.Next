using System.Numerics;
using System.IO;
using System.Collections.ObjectModel;
using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.VisualBasic.FileIO;
using Microsoft.UI;
using Microsoft.UI.Composition;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Hosting;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.ApplicationModel.DataTransfer;
using Windows.Graphics;
using Windows.Storage;
using Windows.Storage.FileProperties;
using Windows.Storage.Streams;
using Windows.UI.ViewManagement;
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
    private const double RasterToolbarHeight = 162;
    private const double CompactRasterChromeWidth = 720;
    private const double RasterContentMargin = 14;
    private const int SwitchDebounceMs = 110;
    private const int ImageSidecarLoadDelayMs = 180;
    private const int WindowsImageMetadataSupplementDelayMs = 850;
    private const int DuplicateOpenCloseGuardMs = 750;
    private static readonly TimeSpan ImageMetadataTimeout = TimeSpan.FromMilliseconds(1500);
    private static readonly TimeSpan CloudPreviewTimeout = TimeSpan.FromSeconds(45);

    private readonly NativeBridge _native = new();
    private readonly NativeThumbnailScheduler _thumbnailScheduler;
    private readonly PreviewWindowController _windowController;
    private TextPreviewPresenter? _textPresenter;
    private TablePreviewPresenter? _tablePresenter;
    private ListingPreviewPresenter? _listingPresenter;
    private OfficePreviewPresenter? _officePresenter;
    private RasterPreviewPresenter? _rasterPresenter;
    private AnimatedImagePreviewPresenter? _animatedImagePresenter;
    private bool _isRasterChromeEnabled;
    private bool _isCompactInfoRailOpen;
    private ImageSidecarController? _imageSidecarController;
    private ExifPreviewPresenter? _exifPresenter;
    private PdfPreviewPresenter? _pdfPresenter;
    private MediaPreviewPresenter? _mediaPresenter;
    private Compositor? _compositor;
    private TrayIconManager? _trayIcon;
    private RasterHostSupervisor? _supervisor;
    private ParserHostSupervisor? _parserSupervisor;
    private readonly Dictionary<string, PreviewHostOwner> _requestHosts = new(StringComparer.Ordinal);
    private PreviewKeyboardHook? _previewKeyboardHook;
    private UiThreadWatchdog? _uiWatchdog;
    private readonly PreviewSession _previewSession = new();
    private FileProbe? _currentProbe;
    private bool _currentPreviewWasCloudPlaceholder;
    private ArchiveEntryHandoff? _currentArchiveEntryHandoff;
    private readonly PreviewPanelController _panelController;
    private bool _isStarted;
    private bool _previewVisible;
    private bool? _backgroundEfficiencyEnabled;
    private CancellationTokenSource? _switchDebounceCts;
    private bool _previewRevealPending;
    private bool _previewTemporarilyHidden;
    private bool _keyboardCloseQueued;
    private bool _isModalDialogOpen;
    private long _lastPreviewRevealTick;
    private string? _lastPreviewRevealPath;
    private ScrollViewer? _imageFilmstripScrollViewer;
    private bool _imageFilmstripDragging;
    private bool _imageFilmstripSuppressClick;
    private Windows.Foundation.Point _imageFilmstripDragStart;
    private double _imageFilmstripDragStartOffset;
    private readonly UISettings _uiSettings = new();
    private readonly AccessibilitySettings _accessibilitySettings = new();

    private static readonly string[] ByteUnits = ["B", "KB", "MB", "GB", "TB"];
    private static readonly HashSet<string> ImageExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".png", ".jpg", ".jpeg", ".jpe", ".gif", ".bmp", ".dib", ".tif", ".tiff", ".webp", ".ico",
        ".heic", ".heif", ".avif", ".jxl",
    };
    private static readonly IReadOnlyDictionary<int, string> ExposureProgramNames = new Dictionary<int, string>
    {
        [0] = "Not defined",
        [1] = "Manual",
        [2] = "Normal program",
        [3] = "Aperture priority",
        [4] = "Shutter priority",
        [5] = "Creative program",
        [6] = "Action program",
        [7] = "Portrait mode",
        [8] = "Landscape mode",
    };
    private static readonly IReadOnlyDictionary<int, string> ExposureModeNames = new Dictionary<int, string>
    {
        [0] = "Auto exposure",
        [1] = "Manual exposure",
        [2] = "Auto bracket",
    };
    private static readonly IReadOnlyDictionary<int, string> MeteringModeNames = new Dictionary<int, string>
    {
        [0] = "Unknown",
        [1] = "Average",
        [2] = "Center-weighted average",
        [3] = "Spot",
        [4] = "Multi-spot",
        [5] = "Pattern",
        [6] = "Partial",
        [255] = "Other",
    };
    private static readonly IReadOnlyDictionary<int, string> WhiteBalanceNames = new Dictionary<int, string>
    {
        [0] = "Auto",
        [1] = "Manual",
    };
    private static readonly IReadOnlyDictionary<int, string> LightSourceNames = new Dictionary<int, string>
    {
        [0] = "Unknown",
        [1] = "Daylight",
        [2] = "Fluorescent",
        [3] = "Tungsten",
        [4] = "Flash",
        [9] = "Fine weather",
        [10] = "Cloudy",
        [11] = "Shade",
        [12] = "Daylight fluorescent",
        [13] = "Day white fluorescent",
        [14] = "Cool white fluorescent",
        [15] = "White fluorescent",
        [17] = "Standard light A",
        [18] = "Standard light B",
        [19] = "Standard light C",
        [20] = "D55",
        [21] = "D65",
        [22] = "D75",
        [23] = "D50",
        [24] = "ISO studio tungsten",
        [255] = "Other",
    };
    private static readonly IReadOnlyDictionary<int, string> ColorSpaceNames = new Dictionary<int, string>
    {
        [1] = "sRGB",
        [65535] = "Uncalibrated",
    };
    private static readonly IReadOnlyDictionary<int, string> CompressionNames = new Dictionary<int, string>
    {
        [1] = "Uncompressed",
        [2] = "CCITT 1D",
        [3] = "T4/Group 3 fax",
        [4] = "T6/Group 4 fax",
        [5] = "LZW",
        [6] = "JPEG",
        [7] = "JPEG",
        [8] = "Deflate",
        [32773] = "PackBits",
    };
    private static readonly IReadOnlyDictionary<int, string> NormalHardSoftNames = new Dictionary<int, string>
    {
        [0] = "Normal",
        [1] = "Soft",
        [2] = "Hard",
    };
    private static readonly IReadOnlyDictionary<int, string> GainControlNames = new Dictionary<int, string>
    {
        [0] = "None",
        [1] = "Low gain up",
        [2] = "High gain up",
        [3] = "Low gain down",
        [4] = "High gain down",
    };
    private enum PreviewInfoRailTab { Info, Exif, More }
    private enum PreviewHostOwner { Raster, Parser }
    private enum PreviewFailureKind { Content, TimedOut, Service, Surface }
    private readonly record struct PreviewFailure(PreviewFailureKind Kind, bool CanRetry);

    // Show the top status text (file name / errors) only while debugging; normal use is chromeless.
    private const bool ShowStatusBar = false;

    public MainWindow()
    {
        InitializeComponent();
        _thumbnailScheduler = new NativeThumbnailScheduler(_native);
        _panelController = new PreviewPanelController(
            PreviewRoot,
            AnimatedImagePreviewRoot,
            PdfScrollViewer,
            PdfPagerBar,
            TextPreviewContainer,
            TableScrollViewer,
            OfficeScrollViewer,
            MediaPreviewElement,
            ListingPanel,
            ErrorPanel,
            PreviewInfoRail,
            ImagePreviewToolbar,
            ImageFilmstrip,
            OfficePagesPanel);
        _windowController = new PreviewWindowController(this, () => WinRT.Interop.WindowNative.GetWindowHandle(this));
        _textPresenter = new TextPreviewPresenter(TextPreviewBlock, TextScrollViewer, TextListView, TextPreviewContainer, MarkdownOutlinePanel, MarkdownOutlineList, () => RootGrid.ActualTheme);
        _tablePresenter = new TablePreviewPresenter(TableScrollViewer, TableTitleText, TableSummaryText, TableGrid, () => RootGrid.ActualTheme);
        _officePresenter = new OfficePreviewPresenter(OfficeScrollViewer, OfficePagesPanel);
        _rasterPresenter = new RasterPreviewPresenter(PreviewRoot, ImageZoomText);
        _animatedImagePresenter = new AnimatedImagePreviewPresenter(AnimatedImagePreviewRoot, AnimatedImagePreviewImage, ImageZoomText);
        _imageSidecarController = new ImageSidecarController(
            ImageFilmstripList,
            ImageFilmstrip,
            DispatcherQueue,
            path => _native.TryPreviewFolderListing(path),
            IsImagePath,
            IsImageFilmstripLoadCurrent,
            (path, size, token) => _thumbnailScheduler.LoadAsync(path, size, NativeThumbnailPriority.Background, cacheOnly: true, token),
            CreateBitmapSource);
        _exifPresenter = new ExifPreviewPresenter(
            ExifDetailsList,
            ExifScrollViewer,
            ExifEmptyPanel,
            ExifUnavailableText,
            ExifGoogleMapsButton,
            StatusText,
            StatusBar);
        _pdfPresenter = new PdfPreviewPresenter(
            PdfScrollViewer,
            PdfPagesPanel,
            PdfPagerBar,
            PreviousPdfPageButton,
            NextPdfPageButton,
            PdfPageIndicatorText,
            DispatcherQueue,
            () => _compositor,
            () => _supervisor);
        _mediaPresenter = new MediaPreviewPresenter(MediaPreviewElement);
        _mediaPresenter.MediaFailed += OnMediaPreviewFailed;
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
            () => _previewSession.Generation,
            () => CurrentPreviewToken,
            IsPreviewGenerationCurrent,
            PreviewListingItemAsync,
            LoadListingIconAsync);
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        Title = UiStrings.AppName;
        TrySetBackdrop();
        try { _uiSettings.ColorValuesChanged += (_, _) => DispatcherQueue.TryEnqueue(ApplyAccessibilityVisuals); }
        catch (Exception ex) { DiagLog.Write("App", "UI color notifications unavailable: " + ex.Message); }
        try { _accessibilitySettings.HighContrastChanged += (_, _) => DispatcherQueue.TryEnqueue(ApplyAccessibilityVisuals); }
        catch (Exception ex) { DiagLog.Write("App", "high contrast notifications unavailable: " + ex.Message); }
        _previewKeyboardHook = new PreviewKeyboardHook(
            WinRT.Interop.WindowNative.GetWindowHandle(this),
            ShouldHandleSpaceAsPreviewClose,
            ClosePreviewFromKeyboard);
        PreviewRoot.SizeChanged += OnRootSizeChanged;
        AnimatedImagePreviewRoot.SizeChanged += OnAnimatedImageRootSizeChanged;
        PreviewContentHost.SizeChanged += OnPreviewContentHostSizeChanged;
        AnimatedImagePreviewRoot.PointerWheelChanged += OnAnimatedImageRootPointerWheelChanged;
        AnimatedImagePreviewRoot.PointerPressed += OnAnimatedImageRootPointerPressed;
        AnimatedImagePreviewRoot.PointerMoved += OnAnimatedImageRootPointerMoved;
        AnimatedImagePreviewRoot.PointerReleased += OnAnimatedImageRootPointerReleased;
        AnimatedImagePreviewRoot.DoubleTapped += OnAnimatedImageRootDoubleTapped;
        PreviewRoot.PointerWheelChanged += OnPreviewRootPointerWheelChanged;
        PreviewRoot.PointerPressed += OnPreviewRootPointerPressed;
        PreviewRoot.PointerMoved += OnPreviewRootPointerMoved;
        PreviewRoot.PointerReleased += OnPreviewRootPointerReleased;
        PreviewRoot.DoubleTapped += OnPreviewRootDoubleTapped;
        RootGrid.KeyDown += OnRootGridKeyDown;
        GetAppWindow().Closing += (appWindow, args) =>
        {
            // Intercept the close (X button / Alt+F4 / taskbar close): hide the window instead of
            // destroying it. The app stays alive in the tray; Escape or tray "Exit" truly quits.
            args.Cancel = true;
            _ = ClosePreviewImmediatelyAsync();
        };
        ImageFilmstripList.Loaded += OnImageFilmstripListLoaded;
        ImageFilmstripList.PointerPressed += OnImageFilmstripPointerPressed;
        ImageFilmstripList.PointerMoved += OnImageFilmstripPointerMoved;
        ImageFilmstripList.PointerReleased += OnImageFilmstripPointerReleased;
        ImageFilmstripList.PointerCanceled += OnImageFilmstripPointerCanceled;
        ImageFilmstripList.PointerCaptureLost += OnImageFilmstripPointerCaptureLost;
        ImageFilmstripList.PointerWheelChanged += OnImageFilmstripPointerWheelChanged;
        Closed += (_, _) =>
        {
            _uiWatchdog?.Dispose();
            _previewKeyboardHook?.Dispose();
            RemoveTrayIcon();
            _supervisor?.Stop();
            _parserSupervisor?.Stop();
        };

        RootGrid.ActualThemeChanged += (s, e) =>
        {
            UpdateTitleBarColors();
            ApplyImageCheckerboardBackdrops();
            ApplyWindowIcon();
            RefreshTrayIcon();
        };
        UpdateTitleBarColors();
        ApplyImageCheckerboardBackdrops();
        _listingPresenter.UpdateSortHeaders();
    }

    public async Task StartBackgroundAsync()
    {
        if (_isStarted) return;
        _isStarted = true;

        DiagLog.Write("App", $"background start; pid={Environment.ProcessId}");
        _uiWatchdog ??= new UiThreadWatchdog(DispatcherQueue);
        SetBackgroundEfficiency(enabled: true);
        StatusBar.Visibility = ShowStatusBar ? Visibility.Visible : Visibility.Collapsed;
        AutoStart.RepairIfConfigured();
        ApplyWindowIcon();
        EnsureTrayIcon();
        _windowController.SetNoActivateStyle(enabled: false);

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
            StatusText.Text = UiStrings.StartupErrorPrefix + ex.Message;
            ShowPreviewWindow(activate: true);
        }
    }

    private void OnNativeIntent(NativeIntent intent)
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            DiagLog.Write("App", $"native intent={intent.Intent}; path={intent.PrimaryPath ?? "<none>"}; visible={_previewVisible}");
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
            DiagLog.Write("App", "intent error: " + ex.Message);
            StatusText.Text = ShowErrorPreview(new PreviewFailure(PreviewFailureKind.Content, false));
            RevealPreviewWindow(activate: false);
        }
    }

    /// <summary>
    /// Close the in-flight preview. Clears the session request id <i>before</i> awaiting the send
    /// (atomic on the UI dispatcher — no yield in between), so any late surface for it is dropped by the
    /// guard, and a second concurrent caller sees null and skips (de-dupes the close).
    /// </summary>
    private async Task CloseCurrentAsync(string? requestId = null)
    {
        ArchiveEntryHandoff? archiveHandoff = Interlocked.Exchange(ref _currentArchiveEntryHandoff, null);
        var id = requestId ?? _previewSession.CurrentRequestId;
        if (id is null)
        {
            if (archiveHandoff is not null && _parserSupervisor is not null)
                await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
            return;
        }
        if (requestId is null || string.Equals(_previewSession.CurrentRequestId, id, StringComparison.Ordinal))
            _previewSession.SetRequestId(null);
        if (!_requestHosts.Remove(id, out PreviewHostOwner owner))
        {
            DiagLog.Write("App", $"close skip: request has no host owner; request={id}");
            if (archiveHandoff is not null && _parserSupervisor is not null)
                await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
            return;
        }

        try
        {
            using var trace = DiagLog.TraceScope("App", $"close request={id}", 100);
            Task close = owner == PreviewHostOwner.Parser
                ? _parserSupervisor?.CloseAsync(id) ?? Task.CompletedTask
                : _supervisor?.CloseAsync(id) ?? Task.CompletedTask;
            await close.WaitAsync(TimeSpan.FromSeconds(1));
        }
        catch (TimeoutException)
        {
            DiagLog.Write("App", $"close timed out; request={id}");
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException or InvalidOperationException)
        {
            DiagLog.Write("App", $"close ignored after host disconnect; request={id}; {ex.GetType().Name}: {ex.Message}");
        }
        finally
        {
            if (archiveHandoff is not null && _parserSupervisor is not null)
                await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
        }
    }

    private async Task ClosePreviewImmediatelyAsync()
    {
        string? requestId = _previewSession.CurrentRequestId;
        _previewSession.BeginClose();
        ResetPreview();
        _previewSession.Clear();
        _previewSession.CancelOperation();
        HidePreviewWindow();
        await CloseCurrentAsync(requestId);
    }

    private async Task EnsureRasterHostStartedAsync(CancellationToken cancellationToken = default)
    {
        if (_supervisor is null)
        {
            _supervisor = new RasterHostSupervisor(ResolveHostExePath(), DispatcherQueue);
            _supervisor.SetBackgroundEfficiency(_backgroundEfficiencyEnabled ?? true);
            _supervisor.SurfaceReceived += OnSurfaceReceived;
        }

        await _supervisor.EnsureStartedAsync(cancellationToken);
    }

    private async Task EnsureParserHostStartedAsync(CancellationToken cancellationToken = default)
    {
        if (_parserSupervisor is null)
        {
            _parserSupervisor = new ParserHostSupervisor(ResolveParserHostExePath());
            _parserSupervisor.SetBackgroundEfficiency(_backgroundEfficiencyEnabled ?? true);
        }

        await _parserSupervisor.EnsureStartedAsync(cancellationToken);
    }

    private async Task HandleNativeIntentAsync(NativeIntent intent)
    {
        // +/- zoom the image preview (only when one is showing; the global key isn't swallowed elsewhere).
        if (intent.Intent is PreviewIntent.ZoomIn or PreviewIntent.ZoomOut)
        {
            if (_rasterPresenter?.HasSurface == true && PreviewRoot.Visibility == Visibility.Visible)
                _rasterPresenter.ZoomBy(intent.Intent == PreviewIntent.ZoomIn ? 1.15 : 1.0 / 1.15);
            else if (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible)
                _animatedImagePresenter.ZoomBy(intent.Intent == PreviewIntent.ZoomIn ? 1.15 : 1.0 / 1.15);
            return;
        }

        if (intent.Intent == PreviewIntent.Close)
        {
            CancelSwitchDebounce();
            await ClosePreviewImmediatelyAsync();
            return;
        }

        if (intent.Intent is PreviewIntent.Open or PreviewIntent.Switch && intent.PrimaryPath is { } path)
        {
            bool isExplorerSwitch = intent.Intent == PreviewIntent.Switch;
            if (isExplorerSwitch && !_previewSession.ShouldAcceptExplorerSwitch(path, _previewVisible))
                return;

            if (intent.Intent == PreviewIntent.Open
                && _previewVisible
                && _previewSession.IsCurrentPath(path))
            {
                if (ShouldIgnoreDuplicateOpenClose(path))
                {
                    DiagLog.Write("App", $"duplicate open ignored after reveal; path={path}");
                    return;
                }

                await ClosePreviewImmediatelyAsync();
                return;
            }

            PreviewNavigationSource source = intent.Intent == PreviewIntent.Open
                ? PreviewNavigationSource.ExplorerOpen
                : PreviewNavigationSource.ExplorerSwitch;
            await PreviewPathAsync(path, source);
        }
    }

    private Task PreviewWindowPathAsync(string path, ArchiveEntryHandoff? archiveHandoff = null)
        => PreviewPathAsync(path, PreviewNavigationSource.WindowNavigation, archiveHandoff);

    private async Task PreviewPathAsync(
        string path,
        PreviewNavigationSource source,
        ArchiveEntryHandoff? archiveHandoff = null)
    {
        PreviewSessionSnapshot session = _previewSession.Begin(path, source);
        int generation = session.Generation;
        CancellationToken previewToken = session.Token;
        using var previewTrace = DiagLog.TraceScope("App", $"preview path source={source} gen={generation} path={path}", 250);
        BeginPreviewTransition();
        ResetPreview();
        bool archiveHandoffTransferred = false;
        _currentProbe = null;
        _currentPreviewWasCloudPlaceholder = false;
        Title = System.IO.Path.GetFileName(path);
        PreviewTitleText.Text = Title;
        StatusText.Text = $"opening {System.IO.Path.GetFileName(path)}…";
        try
        {
            await CloseCurrentAsync();
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            _currentArchiveEntryHandoff = archiveHandoff;
            archiveHandoffTransferred = archiveHandoff is not null;
            CloudFileAvailability availability = await Task.Run(() => CloudFileStatus.GetAvailability(path), previewToken);
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            bool mayRequireHydration = availability != CloudFileAvailability.Local;
            _currentPreviewWasCloudPlaceholder = mayRequireHydration;
            if (availability == CloudFileAvailability.RequiresHydration)
            {
                StatusText.Text = $"downloading {System.IO.Path.GetFileName(path)} from cloud storage…";
                RevealPreviewWindow(activate: false);
                DiagLog.Write("App", $"cloud placeholder detected gen={generation}; path={path}");
            }
            else if (availability == CloudFileAvailability.Unknown)
            {
                StatusText.Text = $"checking {System.IO.Path.GetFileName(path)} availability safely…";
                RevealPreviewWindow(activate: false);
                DiagLog.Write("App", $"file availability unknown; using isolated preview gen={generation}; path={path}");
            }
            DiagLog.Write("App", $"preview probe begin gen={generation}");
            FileProbe probe = await Task.Run(
                () => mayRequireHydration
                    ? FallbackFileProbe.CreateMetadataOnlyProbe(path)
                    : _native.ProbeFile(path) ?? BuildProbe(path),
                previewToken);
            DiagLog.Write("App", $"preview probe end gen={generation}; kind={probe.Kind}; ext={probe.Extension}; size={probe.Size}");
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            _currentProbe = probe;

            if (mayRequireHydration && probe.Kind.Equals("unknown", StringComparison.OrdinalIgnoreCase))
            {
                var unknownCloudReady = CreateCloudMetadataPreview(
                    $"cloud-unknown-{generation}",
                    path,
                    probe,
                    availability == CloudFileAvailability.RequiresHydration
                        ? "Preview is deferred because this cloud file type cannot be identified without downloading its contents."
                        : "Preview is deferred because file availability could not be verified without reading its contents.");
                _previewSession.CommitPath(path);
                _previewSession.SetRequestId(null);
                StatusText.Text = ShowTextPreview(unknownCloudReady);
                RevealPreviewWindow(activate: false);
                return;
            }

            if (MediaPreviewPresenter.IsMediaProbe(probe))
            {
                if (mayRequireHydration)
                {
                    var cloudMediaReady = CreateCloudMetadataPreview(
                    $"cloud-media-{generation}",
                    path,
                    probe,
                    availability == CloudFileAvailability.RequiresHydration
                        ? "Media playback is deferred until the cloud provider makes this file available locally."
                        : "Media playback is deferred because file availability could not be verified safely.");
                    _previewSession.CommitPath(path);
                    _previewSession.SetRequestId(null);
                    StatusText.Text = ShowTextPreview(cloudMediaReady);
                    RevealPreviewWindow(activate: false);
                    return;
                }
                PreviewReady? mediaInfo = await Task.Run(() => _native.TryPreview($"media-info-{generation}", path, probe, previewToken), previewToken);
                DiagLog.Write("App", $"preview native media info end gen={generation}; hasInfo={mediaInfo is not null}");
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                var mediaReady = new PreviewReady(
                    $"media-{generation}",
                    "media",
                    System.IO.Path.GetFileName(path),
                    800,
                    probe.Kind.Equals("audio", StringComparison.OrdinalIgnoreCase) ? 140 : 450)
                {
                    MediaPath = path,
                    TextContent = mediaInfo?.TextContent,
                    TextFormat = mediaInfo?.TextFormat,
                    TextLanguage = mediaInfo?.TextLanguage,
                };
                _previewSession.CommitPath(path);
                _previewSession.SetRequestId(null);
                StatusText.Text = ShowMediaPreview(mediaReady);
                RevealPreviewWindow(ShouldActivatePreview(mediaReady));
                return;
            }

            bool forceAnimatedFirstFrameRaster = PrefersReducedMotion || mayRequireHydration;
            AnimatedImageRenderPlan? animatedPlan = forceAnimatedFirstFrameRaster
                ? null
                : await Task.Run(() => AnimatedImagePreviewPresenter.CreateRenderPlan(path), previewToken);
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            if (animatedPlan is { } plan)
            {
                DiagLog.Write("App", $"preview animated image detected gen={generation}; mode={plan.PlaybackMode}; {plan.Width}x{plan.Height}");
                if (plan.PlaybackMode == AnimatedImagePlaybackMode.NativeFramePlayback)
                {
                    DiagLog.Write("App", $"preview animated image using native frame playback gen={generation}; {plan.Width}x{plan.Height}");
                    string animatedExt = System.IO.Path.GetExtension(path);
                    if (animatedExt.Equals(".gif", StringComparison.OrdinalIgnoreCase) || animatedExt.Equals(".webp", StringComparison.OrdinalIgnoreCase))
                    {
                        var animatedTargetSize = GetRasterDecodeTargetSize();
                        NativeAnimationFrames? frames = animatedExt.Equals(".webp", StringComparison.OrdinalIgnoreCase)
                            ? await Task.Run(() => _native.TryDecodeWebPFrames(path, animatedTargetSize.Width, animatedTargetSize.Height), previewToken)
                            : await Task.Run(() => _native.TryDecodeGifFrames(path, animatedTargetSize.Width, animatedTargetSize.Height), previewToken);
                        if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                        if (frames is not null)
                        {
                            var gifReady = new PreviewReady(
                                $"gif-native-{generation}",
                                "image",
                                System.IO.Path.GetFileName(path),
                                plan.Width,
                                plan.Height);
                            _previewSession.CommitPath(path);
                            _previewSession.SetRequestId(null);
                            StatusText.Text = ShowNativeAnimatedImagePreview(gifReady, path, frames);
                            RevealPreviewWindow(ShouldActivatePreview(gifReady));
                            return;
                        }
                    }
                    forceAnimatedFirstFrameRaster = true;
                }
                else
                {
                    var gifReady = new PreviewReady(
                        $"gif-{generation}",
                        "image",
                        System.IO.Path.GetFileName(path),
                        plan.Width,
                        plan.Height);
                    _previewSession.CommitPath(path);
                    _previewSession.SetRequestId(null);
                    StatusText.Text = ShowAnimatedImagePreview(gifReady, path);
                    RevealPreviewWindow(ShouldActivatePreview(gifReady));
                    return;
                }
            }


            PreviewReady? nativeReady = null;
            if (!forceAnimatedFirstFrameRaster
                && (IsParserHostPreview(probe) || (mayRequireHydration && IsCloudParserHostPreview(probe))))
            {
                await EnsureParserHostStartedAsync(previewToken);
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                var (parserRequestId, parserCompletion) = _parserSupervisor!.BeginOpen(
                    path,
                    probe,
                    mayRequireHydration ? CloudPreviewTimeout : null,
                    recycleHostOnCancel: mayRequireHydration);
                _requestHosts[parserRequestId] = PreviewHostOwner.Parser;
                _previewSession.SetRequestId(parserRequestId);
                _previewSession.CommitPath(path);
                ControlMessage parserResult = await parserCompletion.WaitAsync(previewToken);
                if (!IsPreviewGenerationCurrent(generation, previewToken) || !_previewSession.IsCurrentRequest(parserRequestId)) return;
                if (parserResult is PreviewError parserError)
                {
                    StatusText.Text = ShowHostError(parserError);
                    RevealPreviewWindow(activate: false);
                    return;
                }
                nativeReady = parserResult as PreviewReady;
            }
            else if (!forceAnimatedFirstFrameRaster)
            {
                nativeReady = await Task.Run(() => _native.TryPreview($"native-{generation}", path, probe, previewToken), previewToken);
                if (nativeReady is null && probe.Kind.Equals("text", StringComparison.OrdinalIgnoreCase))
                    nativeReady = await Task.Run(
                        () => FallbackFileProbe.TryCreateTextPreview($"managed-{generation}", path, previewToken),
                        previewToken);
            }
            DiagLog.Write("App", $"preview native ready end gen={generation}; hasReady={nativeReady is not null}");
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            if (nativeReady is not null)
            {
                _previewSession.CommitPath(path);
                // ParserHost previews retain their request id until navigation/close can cancel that host.
                if (!_requestHosts.ContainsKey(_previewSession.CurrentRequestId ?? ""))
                    _previewSession.SetRequestId(null);
                StatusText.Text = nativeReady switch
                {
                    PreviewReady r when r.OfficeLayout is not null => ShowOfficeLayoutPreview(r),
                    PreviewReady r when r.Table is not null => ShowTablePreview(r),
                    PreviewReady r when r.Listing is not null => ShowListingPreview(r),
                    PreviewReady r when r.Markdown is not null => ShowTextPreview(r),
                    PreviewReady r when r.TextContent is not null => ShowTextPreview(r),
                    _ => $"{nativeReady.Kind}: {nativeReady.Title}",
                };
                RevealPreviewWindow(ShouldActivatePreview(nativeReady));
                return;
            }

            await EnsureRasterHostStartedAsync(previewToken);
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            var targetSize = GetRasterDecodeTargetSize();
            var (requestId, completion) = _supervisor!.BeginOpen(
                path,
                probe,
                targetSize.Width,
                targetSize.Height,
                mayRequireHydration ? CloudPreviewTimeout : null,
                recycleHostOnCancel: mayRequireHydration);
            _requestHosts[requestId] = PreviewHostOwner.Raster;
            _previewSession.SetRequestId(requestId);
            _previewSession.CommitPath(path);
            DiagLog.Write("App", $"preview host open sent gen={generation}; request={requestId}");
            ControlMessage result = await completion.WaitAsync(previewToken);
            DiagLog.Write("App", $"preview host result gen={generation}; request={requestId}; type={result.GetType().Name}");
            if (!IsPreviewGenerationCurrent(generation, previewToken) || !_previewSession.IsCurrentRequest(requestId))
                return;
            StatusText.Text = result switch
            {
                PreviewReady r when r.Kind == "pdf" => ShowPdfDocument(requestId, r),
                PreviewReady r when r.OfficeLayout is not null => ShowOfficeLayoutPreview(r),
                PreviewReady r when r.Table is not null => ShowTablePreview(r),
                PreviewReady r when r.Listing is not null => ShowListingPreview(r),
                PreviewReady r when r.Markdown is not null => ShowTextPreview(r),
                PreviewReady r when r.TextContent is not null => ShowTextPreview(r),
                PreviewReady r when r.MediaPath is not null => ShowMediaPreview(r),
                PreviewReady r => ShowRasterPreview(r),
                PreviewError er => ShowHostError(er),
                _ => "?",
            };
            RevealPreviewWindow(result is PreviewReady ready && ShouldActivatePreview(ready));
        }
        catch (TimeoutException ex)
        {
            DiagLog.Write("App", "preview timed out: " + ex.Message);
            StatusText.Text = ShowErrorPreview(new PreviewFailure(PreviewFailureKind.TimedOut, true));
            RevealPreviewWindow(activate: false);
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException or InvalidOperationException)
        {
            DiagLog.Write("App", "preview service failed: " + ex);
            if (IsPreviewGenerationCurrent(generation, previewToken))
            {
                StatusText.Text = ShowErrorPreview(new PreviewFailure(PreviewFailureKind.Service, true));
                RevealPreviewWindow(activate: false);
            }
        }
        catch (OperationCanceledException)
        {
            DiagLog.Write("App", $"preview canceled: path={path}");
        }
        finally
        {
            if (archiveHandoff is not null && !archiveHandoffTransferred && _parserSupervisor is not null)
                await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
        }
    }

    private CancellationToken CurrentPreviewToken => _previewSession.Token;

    private bool IsPreviewGenerationCurrent(int generation) => IsPreviewGenerationCurrent(generation, CurrentPreviewToken);

    private bool IsPreviewGenerationCurrent(int generation, CancellationToken cancellationToken)
        => _previewSession.IsCurrent(generation, cancellationToken);

    private void BeginPreviewTransition()
    {
        DiagLog.Write("App", $"preview transition begin; visible={_previewVisible}; request={_previewSession.CurrentRequestId}");
        _native.SetPreviewVisible(true);
        _previewRevealPending = true;
        PreviewContentHost.Opacity = 0;
        PreviewContentHost.IsHitTestVisible = false;
        ErrorPanel.Visibility = Visibility.Collapsed;
        LoadingRing.Visibility = Visibility.Visible;
        LoadingRing.IsActive = true;
    }

    private void RevealPreviewWindow(bool activate)
    {
        DiagLog.Write("App", $"preview reveal; activate={activate}; visible={_previewVisible}; tempHidden={_previewTemporarilyHidden}");
        _previewRevealPending = false;
        _keyboardCloseQueued = false;
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
                _windowController.SetNoActivateStyle(enabled: false);
            }
            _windowController.Raise(activate);
            EnsureCompositor();
        }
        FadeInPreviewContent();
    }

    private bool ShouldIgnoreDuplicateOpenClose(string path)
    {
        if (!string.Equals(_lastPreviewRevealPath, path, StringComparison.OrdinalIgnoreCase))
            return false;

        long elapsed = Environment.TickCount64 - _lastPreviewRevealTick;
        return elapsed >= 0 && elapsed < DuplicateOpenCloseGuardMs;
    }

    private static bool ShouldActivatePreview(PreviewReady ready)
        => ready.TextContent is not null || ready.Listing is not null || ready.Table is not null || ready.Markdown is not null || ready.OfficeLayout is not null;

    private void UpdatePreviewChrome(PreviewReady ready, bool showRasterTools = false)
    {
        string? path = _previewSession.CurrentPath ?? ready.MediaPath;
        string title = !string.IsNullOrWhiteSpace(path)
            ? System.IO.Path.GetFileName(path)
            : ready.Title;
        if (string.IsNullOrWhiteSpace(title))
            title = UiStrings.AppName;

        Title = title;
        PreviewTitleText.Text = title;
        PreviewKindPillText.Text = ready.Kind.ToUpperInvariant();
        PreviewMetaText.Text = BuildPreviewMetaLine(ready, path, _currentProbe);

        _isRasterChromeEnabled = showRasterTools;
        ApplyRasterChromeLayout();
        UpdateImageAnimationPlaybackButton();

        PreviewDimensionsText.Text = BuildDimensionsText(ready);
        PreviewSizeText.Text = FileSizeText(path, _currentProbe);
        PreviewTypeText.Text = PreviewTypeTextFor(ready, path);
        PreviewModifiedText.Text = ModifiedText(path, _currentProbe);
        PreviewPathText.Text = string.IsNullOrWhiteSpace(path) ? UiStrings.EmptyValue : path;
        if (showRasterTools)
            SetPreviewInfoRailTab(PreviewInfoRailTab.Info);
        _rasterPresenter?.UpdateZoomLabel();
    }

    private void ResetPreviewChrome()
    {
        Title = UiStrings.AppName;
        PreviewTitleText.Text = UiStrings.AppName;
        PreviewMetaText.Text = UiStrings.Ready;
        PreviewKindPillText.Text = UiStrings.ReadyKind;
        _isRasterChromeEnabled = false;
        _isCompactInfoRailOpen = false;
        CompactInfoRailToggle.IsChecked = false;
        _panelController.ResetChromeVisibility();
        PreviewDimensionsText.Text = UiStrings.EmptyValue;
        PreviewSizeText.Text = UiStrings.EmptyValue;
        PreviewTypeText.Text = UiStrings.EmptyValue;
        PreviewModifiedText.Text = UiStrings.EmptyValue;
        PreviewPathText.Text = UiStrings.EmptyValue;
        ImageZoomText.Text = UiStrings.FitZoom;
        UpdateImageAnimationPlaybackButton();
        ResetExifDetails();
        SetPreviewInfoRailTab(PreviewInfoRailTab.Info);
    }

    private bool IsCompactRasterChrome => PreviewContentHost.ActualWidth is > 0 and < CompactRasterChromeWidth;

    private void ApplyRasterChromeLayout()
    {
        bool isCompact = IsCompactRasterChrome;
        if (!isCompact)
            _isCompactInfoRailOpen = false;
        bool showInfoRail = _isRasterChromeEnabled && (!isCompact || _isCompactInfoRailOpen);
        bool reserveRailSpace = _isRasterChromeEnabled && !isCompact;
        double rightMargin = reserveRailSpace ? RasterInfoRailWidth + RasterContentMargin : RasterContentMargin;
        double bottomMargin = _isRasterChromeEnabled ? RasterToolbarHeight : RasterContentMargin;

        _panelController.ToggleRasterTools(_isRasterChromeEnabled, showInfoRail);
        PreviewRoot.Margin = new Thickness(RasterContentMargin, 0, rightMargin, bottomMargin);
        AnimatedImagePreviewRoot.Margin = PreviewRoot.Margin;
        ImagePreviewToolbar.Margin = new Thickness(RasterContentMargin, 0, rightMargin, RasterContentMargin);
        ImageFilmstrip.Margin = new Thickness(RasterContentMargin, 0, rightMargin, 78);
        CompactInfoRailToggle.Visibility = _isRasterChromeEnabled && isCompact ? Visibility.Visible : Visibility.Collapsed;
        CompactInfoRailToggle.IsChecked = _isCompactInfoRailOpen;
        string infoAction = _isCompactInfoRailOpen ? UiStrings.HidePreviewDetails : UiStrings.ShowPreviewDetails;
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(CompactInfoRailToggle, infoAction);
        ToolTipService.SetToolTip(CompactInfoRailToggle, infoAction);
    }

    private void OnPreviewContentHostSizeChanged(object sender, SizeChangedEventArgs e)
    {
        if (!_isRasterChromeEnabled)
            return;

        bool wasCompact = e.PreviousSize.Width is > 0 and < CompactRasterChromeWidth;
        bool isCompact = e.NewSize.Width < CompactRasterChromeWidth;
        if (wasCompact != isCompact)
            ApplyRasterChromeLayout();
    }

    private static string BuildPreviewMetaLine(PreviewReady ready, string? path, FileProbe? probe)
    {
        var parts = new List<string>();
        string dimensions = BuildDimensionsText(ready);
        if (dimensions != UiStrings.EmptyValue)
            parts.Add(dimensions);
        string size = FileSizeText(path, probe);
        if (size != UiStrings.EmptyValue)
            parts.Add(size);
        string container = ExtractPreviewInfoLine(ready.TextContent, "Container");
        if (!string.IsNullOrWhiteSpace(container))
            parts.Add(container);
        parts.Add(PreviewTypeTextFor(ready, path));
        string modified = ModifiedText(path, probe);
        if (modified != UiStrings.EmptyValue)
            parts.Add("Modified: " + modified);
        return string.Join("  |  ", parts);
    }

    private static string ExtractPreviewInfoLine(string? text, string label)
    {
        if (string.IsNullOrWhiteSpace(text))
            return "";

        string prefix = label + ":";
        foreach (string line in text.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n'))
        {
            if (line.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                return line[prefix.Length..].Trim();
        }
        return "";
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
        if (ready.Table is { } table)
            return $"{table.TotalRows:N0} rows x {table.TotalColumns:N0} columns";
        return UiStrings.EmptyValue;
    }

    private static string PreviewTypeTextFor(PreviewReady ready, string? path)
    {
        string ext = string.IsNullOrWhiteSpace(path)
            ? ""
            : System.IO.Path.GetExtension(path).TrimStart('.').ToUpperInvariant();
        return string.IsNullOrEmpty(ext) ? ready.Kind : $"{ext} {ready.Kind}";
    }

    private static string FileSizeText(string? path, FileProbe? probe)
    {
        if (ProbeMatchesPath(probe, path) && !probe!.Kind.Equals("folder", StringComparison.OrdinalIgnoreCase))
            return FormatBytes(probe.Size);
        try
        {
            if (!string.IsNullOrWhiteSpace(path) && System.IO.File.Exists(path))
                return FormatBytes(new FileInfo(path).Length);
        }
        catch { }
        return UiStrings.EmptyValue;
    }

    private static string ModifiedText(string? path, FileProbe? probe)
    {
        if (ProbeMatchesPath(probe, path) && probe!.ModifiedUnix > 0)
            return DateTimeOffset.FromUnixTimeSeconds(probe.ModifiedUnix).LocalDateTime.ToString("g");
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

    private static PreviewReady CreateCloudMetadataPreview(string requestId, string path, FileProbe probe, string status)
    {
        string fileName = System.IO.Path.GetFileName(path);
        string modified = ModifiedText(path, probe);
        return new PreviewReady(requestId, probe.Kind, fileName, 680, 420)
        {
            TextContent = $"Name: {fileName}\nKind: {probe.Kind}\nSize: {probe.Size:N0} bytes\nModified: {modified}\nStatus: {status}",
            TextFormat = "plain",
            TextLanguage = "text",
        };
    }

    private static bool ProbeMatchesPath(FileProbe? probe, string? path)
        => probe is not null
           && !string.IsNullOrWhiteSpace(path)
           && string.Equals(probe.Path, path, StringComparison.OrdinalIgnoreCase);

    private string ShowHostError(PreviewError error)
    {
        DiagLog.Write("App", "host preview error: " + error.Message);
        return ShowErrorPreview(new PreviewFailure(PreviewFailureKind.Content, false));
    }

    private string ShowErrorPreview(PreviewFailure failure)
    {
        _panelController.ShowError();
        (string title, string message) = failure.Kind switch
        {
            PreviewFailureKind.TimedOut => (UiStrings.PreviewTimedOutTitle, UiStrings.PreviewTimedOutMessage),
            PreviewFailureKind.Service => (UiStrings.PreviewServiceUnavailableTitle, UiStrings.PreviewServiceUnavailableMessage),
            PreviewFailureKind.Surface => (UiStrings.PreviewDisplayFailedTitle, UiStrings.PreviewDisplayFailedMessage),
            _ => (UiStrings.PreviewContentFailedTitle, UiStrings.PreviewContentFailedMessage),
        };
        ErrorText.Text = message;
        PreviewTitleText.Text = title;
        PreviewMetaText.Text = ErrorText.Text;
        PreviewKindPillText.Text = UiStrings.ErrorKind;
        bool hasPath = !string.IsNullOrWhiteSpace(_previewSession.CurrentPath);
        ErrorActionsPanel.Visibility = hasPath ? Visibility.Visible : Visibility.Collapsed;
        ErrorRetryButton.Visibility = hasPath && failure.CanRetry ? Visibility.Visible : Visibility.Collapsed;
        ResizeWindowForContent(520, 300, MaxTextWindowWidth, MaxTextWindowHeight);
        DispatcherQueue.TryEnqueue(() =>
        {
            if (ErrorRetryButton.Visibility == Visibility.Visible)
                ErrorRetryButton.Focus(FocusState.Programmatic);
            else if (ErrorActionsPanel.Visibility == Visibility.Visible)
                ErrorOpenFileButton.Focus(FocusState.Programmatic);
        });
        return "error: " + ErrorText.Text;
    }

    private void FadeInPreviewContent()
    {
        PreviewContentHost.Opacity = 1;
        PreviewContentHost.IsHitTestVisible = true;
        if (PrefersReducedMotion)
            return;

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
        using var trace = DiagLog.TraceScope(
            "App",
            $"surface received request={surface.RequestId}; page={surface.PageIndex}; size={surface.Width}x{surface.Height}",
            50);
        bool handleConsumed = false;
        try
        {
            EnsureCompositor();
            Compositor? compositor = _compositor;
            if (compositor is null)
            {
                DiagLog.Write("App", "surface ignored: compositor unavailable");
                ShowSurfaceFailure(surface.RequestId, UiStrings.SurfaceFailed);
                return;
            }

            // Only accept surfaces for the exact current request. While switching/closing the session request id is
            // null, so late surfaces for a just-closed request are dropped — never build a composition surface
            // from a handle whose swapchain the host may already be retiring.
            if (!_previewSession.IsCurrentRequest(surface.RequestId)) return;

            if (surface.PageIndex >= 0)
            {
                if (_pdfPresenter is null)
                    return;

                var pdfAttachWatch = Stopwatch.StartNew();
                handleConsumed = true;
                if (!_pdfPresenter.AttachSurface(surface, out string? pdfError))
                {
                    StatusText.Text = pdfError ?? UiStrings.PdfPageFailed;
                    return;
                }
                pdfAttachWatch.Stop();
                DiagLog.Write("App", $"pdf page surface attach/apply {pdfAttachWatch.ElapsedMilliseconds}ms; request={surface.RequestId}; page={surface.PageIndex}; size={surface.Width}x{surface.Height}");
                return;
            }

            if (_rasterPresenter is null)
            {
                DiagLog.Write("App", "surface ignored: raster presenter unavailable");
                return;
            }

            var attachWatch = Stopwatch.StartNew();
            handleConsumed = true;
                if (!_rasterPresenter.AttachSurface(compositor, surface, out string? error))
                {
                    ShowSurfaceFailure(surface.RequestId, error ?? UiStrings.SurfaceFailed);
                    return;
            }
            attachWatch.Stop();
            DiagLog.Write("App", $"image surface attach {attachWatch.ElapsedMilliseconds}ms; size={surface.Width}x{surface.Height}");
            DispatcherQueue.TryEnqueue(() =>
            {
                if (!_previewSession.IsCurrentRequest(surface.RequestId))
                    return;

                var layoutWatch = Stopwatch.StartNew();
                _rasterPresenter.UpdateLayout();
                layoutWatch.Stop();
                DiagLog.Write("App", $"image presenter apply {layoutWatch.ElapsedMilliseconds}ms; size={surface.Width}x{surface.Height}");
            });
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", $"FATAL ERROR in OnSurfaceReceived: {ex}");
        }
        finally
        {
            if (!handleConsumed)
                CompositionInterop.CloseSharedHandle((nint)surface.SharedHandle);
        }
    }

    private void ShowSurfaceFailure(string requestId, string message)
    {
        if (!_previewSession.IsCurrentRequest(requestId))
            return;

        DiagLog.Write("App", "surface preview failed: " + message);
        _previewSession.SetRequestId(null);
        _ = CloseCurrentAsync(requestId);
        StatusText.Text = ShowErrorPreview(new PreviewFailure(PreviewFailureKind.Surface, false));
        RevealPreviewWindow(activate: false);
    }

    private void OnMediaPreviewFailed(string path)
    {
        if (!_previewSession.IsCurrentPath(path))
            return;

        StatusText.Text = ShowErrorPreview(new PreviewFailure(PreviewFailureKind.Content, false));
        RevealPreviewWindow(activate: false);
    }

    private void OnRootSizeChanged(object sender, SizeChangedEventArgs e)
    {
        ApplyImageCheckerboardBackdrop(PreviewRoot);
        _rasterPresenter?.UpdateLayout();
    }

    private void OnAnimatedImageRootSizeChanged(object sender, SizeChangedEventArgs e)
    {
        ApplyImageCheckerboardBackdrop(AnimatedImagePreviewRoot);
        _animatedImagePresenter?.ScheduleLayoutUpdate();
    }

    private void ApplyImageCheckerboardBackdrops()
    {
        ApplyImageCheckerboardBackdrop(PreviewRoot);
        ApplyImageCheckerboardBackdrop(AnimatedImagePreviewRoot);
    }

    private void ApplyImageCheckerboardBackdrop(Border border)
    {
        int width = (int)Math.Ceiling(border.ActualWidth);
        int height = (int)Math.Ceiling(border.ActualHeight);
        if (width <= 0 || height <= 0)
            return;

        const int cell = 16;
        var bitmap = new WriteableBitmap(width, height);
        byte light = RootGrid.ActualTheme == ElementTheme.Dark ? (byte)58 : (byte)230;
        byte dark = RootGrid.ActualTheme == ElementTheme.Dark ? (byte)44 : (byte)208;
        byte[] pixels = new byte[width * height * 4];
        for (int y = 0; y < height; y++)
        {
            int row = y / cell;
            for (int x = 0; x < width; x++)
            {
                byte tone = ((x / cell + row) & 1) == 0 ? light : dark;
                int offset = (y * width + x) * 4;
                pixels[offset] = tone;
                pixels[offset + 1] = tone;
                pixels[offset + 2] = tone;
                pixels[offset + 3] = 255;
            }
        }

        using (Stream stream = bitmap.PixelBuffer.AsStream())
            stream.Write(pixels, 0, pixels.Length);
        bitmap.Invalidate();
        border.Background = new ImageBrush
        {
            ImageSource = bitmap,
            Stretch = Stretch.Fill,
        };
    }

    private void OnPreviousPdfPageClick(object sender, RoutedEventArgs e)
        => _pdfPresenter?.GoToPreviousPage();

    private void OnNextPdfPageClick(object sender, RoutedEventArgs e)
        => _pdfPresenter?.GoToNextPage();

    private string ShowRasterPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready, showRasterTools: true);
        _panelController.ShowRaster();
        RasterPreviewResult result = _rasterPresenter!.Render(ready, GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxImageWindowWidth, MaxImageWindowHeight);
        DispatcherQueue.TryEnqueue(() =>
        {
            if (_previewSession.IsCurrentRequest(ready.RequestId))
                _rasterPresenter.UpdateLayout();
        });
        ScheduleImageSidecarLoads(ready);
        return result.Status;
    }

    private (uint Width, uint Height) GetRasterDecodeTargetSize()
    {
        var maxContent = GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight);
        double width = Math.Max(1, maxContent.Width - RasterInfoRailWidth);
        double height = Math.Max(1, maxContent.Height - RasterToolbarHeight);
        return ((uint)Math.Ceiling(width), (uint)Math.Ceiling(height));
    }

    private string ShowAnimatedImagePreview(PreviewReady ready, string path)
    {
        UpdatePreviewChrome(ready, showRasterTools: true);
        _panelController.ShowAnimatedImage();
        _rasterPresenter?.Clear();

        AnimatedImagePreviewResult result = _animatedImagePresenter!.Render(path, ready, GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight));
        UpdateImageAnimationPlaybackButton();
        ResizeWindowForContent(result.Width, result.Height, MaxImageWindowWidth, MaxImageWindowHeight);
        ScheduleImageSidecarLoads(ready);
        return result.Status;
    }

    private string ShowNativeAnimatedImagePreview(PreviewReady ready, string path, NativeAnimationFrames frames)
    {
        UpdatePreviewChrome(ready, showRasterTools: true);
        _panelController.ShowAnimatedImage();
        _rasterPresenter?.Clear();

        AnimatedImagePreviewResult result = _animatedImagePresenter!.RenderNativeFrames(path, ready, frames, GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight));
        UpdateImageAnimationPlaybackButton();
        ResizeWindowForContent(result.Width, result.Height, MaxImageWindowWidth, MaxImageWindowHeight);
        ScheduleImageSidecarLoads(ready);
        return result.Status;
    }

    private string ShowPdfDocument(string requestId, PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        _panelController.ShowPdf();
        _rasterPresenter?.Clear();
        PdfPreviewResult result = _pdfPresenter!.Render(requestId, ready, GetMaxContentSize(MaxPdfWindowWidth, MaxPdfWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxPdfWindowWidth, MaxPdfWindowHeight);
        return result.Status;
    }

    private string ShowTextPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        _panelController.ShowText();
        _rasterPresenter?.Clear();

        TextPreviewResult result = _textPresenter!.Render(ready, GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight));
        StartPreviewHeroLoad(ready);
        ResizeWindowForContent(result.Width, result.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return result.Status;
    }

    private string ShowTablePreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        _panelController.ShowTable();
        _rasterPresenter?.Clear();

        TablePreviewResult result = _tablePresenter!.Render(ready, GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return result.Status;
    }

    private string ShowOfficeLayoutPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        _panelController.ShowOffice();
        _rasterPresenter?.Clear();

        OfficePreviewResult result = _officePresenter!.Render(ready, GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return result.Status;
    }

    private string ShowMediaPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        _panelController.ShowMedia();
        _rasterPresenter?.Clear();

        MediaPreviewResult result = _mediaPresenter!.Render(ready, GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight));
        ResizeWindowForContent(result.Width, result.Height, MaxImageWindowWidth, MaxImageWindowHeight);
        return result.Status;
    }

    private string ShowListingPreview(PreviewReady ready)
    {
        UpdatePreviewChrome(ready);
        _panelController.ShowListing();
        _rasterPresenter?.Clear();

        ListingPreviewResult result = _listingPresenter!.Render(ready, GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight));
        StartPreviewHeroLoad(ready);
        ResizeWindowForContent(result.Width, result.Height, MaxTextWindowWidth, MaxTextWindowHeight);
        return result.Status;
    }

    private void OnListingSortClick(object sender, RoutedEventArgs e)
        => _listingPresenter?.OnSortClick(sender);

    private async void OnListingItemClick(object sender, ItemClickEventArgs e)
        => await (_listingPresenter?.OnItemClickAsync(e) ?? Task.CompletedTask);

    private async void OnListingListViewDoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        => await (_listingPresenter?.OnDoubleTappedAsync() ?? Task.CompletedTask);

    private async void OnListingListViewKeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
        => await (_listingPresenter?.OnKeyDownAsync(e) ?? Task.CompletedTask);

    private async Task PreviewListingItemAsync(PreviewListing? listing, ListingRow row)
    {
        string? path = row.NativePath;
        ArchiveEntryHandoff? archiveHandoff = null;
        if (string.IsNullOrWhiteSpace(path)
            && listing is not null
            && listing.ListingKind.Equals("archive", StringComparison.OrdinalIgnoreCase)
            && !string.IsNullOrWhiteSpace(listing.RootPath))
        {
            await EnsureParserHostStartedAsync();
            archiveHandoff = await _parserSupervisor!.ExtractArchiveEntryAsync(listing.RootPath, row.Path, CurrentPreviewToken);
            if (archiveHandoff is not null)
            {
                path = archiveHandoff.Path;
            }
        }

        if (string.IsNullOrWhiteSpace(path))
            return;

        await PreviewWindowPathAsync(path, archiveHandoff);
    }

    private async Task<ImageSource?> LoadListingIconAsync(ListingRow row, int generation)
    {
        string? path = row.NativePath;
        if (string.IsNullOrWhiteSpace(path))
            return null;

        try
        {
            CancellationToken token = CurrentPreviewToken;
            bool mayRequireHydration = await Task.Run(() => CloudFileStatus.MayRequireHydration(path), token);
            if (mayRequireHydration || !IsPreviewGenerationCurrent(generation, token))
                return null;
            NativeRasterImage? raster = await _thumbnailScheduler.LoadAsync(path, 32, NativeThumbnailPriority.Foreground, cacheOnly: true, token);

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
        DiagLog.Write("App", $"preview reset; visible={_previewVisible}; request={_previewSession.CurrentRequestId}");
        _rasterPresenter?.Clear();
        _animatedImagePresenter?.Clear();
        _mediaPresenter?.Clear();
        _pdfPresenter?.Clear();
        _textPresenter?.Clear();
        _tablePresenter?.Clear();
        _listingPresenter?.Reset();
        _panelController.ResetPreviewState();

        if (!_previewRevealPending)
        {
            LoadingRing.IsActive = false;
            LoadingRing.Visibility = Visibility.Collapsed;
            PreviewContentHost.Opacity = 1;
        }

        ClearPreviewHeroImages();
        ClearImageSidecars();
        ResetPreviewChrome();
    }

    private void StartPreviewHeroLoad(PreviewReady ready)
    {
        string? path = _previewSession.CurrentPath;
        if (string.IsNullOrWhiteSpace(path) || !ShouldLoadPreviewHero(ready, path))
        {
            ClearPreviewHeroImages();
            return;
        }

        int generation = _previewSession.Generation;
        CancellationToken token = CurrentPreviewToken;
        bool cloudOrigin = _currentPreviewWasCloudPlaceholder;
        Task.Run(async () =>
        {
            if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                return null;
            return await LoadPreviewHeroRasterAsync(ready, path, cloudOrigin, token);
        }, token).ContinueWith(task =>
        {
            if (task.IsFaulted || task.IsCanceled || task.Result is null)
                return;

            DispatcherQueue.TryEnqueue(() =>
            {
                if (!IsPreviewGenerationCurrent(generation) || !_previewSession.IsCurrentPath(path))
                    return;

                var source = CreateBitmapSource(task.Result);
                if (source is null)
                    return;

                if (ListingPanel.Visibility == Visibility.Visible)
                {
                    ListingHeroImage.Source = source;
                    ListingHeroFrame.Visibility = Visibility.Visible;
                }
                else if (TextPreviewContainer.Visibility == Visibility.Visible)
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

    private async Task<NativeRasterImage?> LoadPreviewHeroRasterAsync(
        PreviewReady ready,
        string path,
        bool cloudOrigin,
        CancellationToken token)
    {
        if (cloudOrigin)
            return null;

        if (IsPackagePreview(ready, path))
        {
            await EnsureParserHostStartedAsync();
            NativeRasterImage? icon = await _parserSupervisor!.ExtractHeroRasterAsync(path, "package", token);
            return icon ?? await _thumbnailScheduler.LoadAsync(path, 512, NativeThumbnailPriority.Foreground, cacheOnly: false, token);
        }

        if (IsOfficePreviewWithImages(ready))
        {
            await EnsureParserHostStartedAsync();
            return await _parserSupervisor!.ExtractHeroRasterAsync(path, "office", token);
        }

        if (IsExecutablePreview(ready, path))
            return await _thumbnailScheduler.LoadAsync(path, 512, NativeThumbnailPriority.Foreground, cacheOnly: false, token);

        if (ready.Kind == "certificate")
            return await _thumbnailScheduler.LoadAsync(path, 256, NativeThumbnailPriority.Foreground, cacheOnly: false, token);

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

    private void StartImageSidecarLoads(PreviewReady ready)
    {
        string? path = _previewSession.CurrentPath;
        if (string.IsNullOrWhiteSpace(path) || !IsImagePath(path))
        {
            ClearImageSidecars();
            return;
        }
        string imagePath = path;

        int generation = _previewSession.Generation;
        CancellationToken token = CurrentPreviewToken;
        ResetExifDetails();
        _ = LoadImageMetadataAsync(imagePath, generation, token);
        _ = _imageSidecarController?.LoadFilmstripAsync(imagePath, generation, token);
    }

    private void ScheduleImageSidecarLoads(PreviewReady ready)
    {
        string? path = _previewSession.CurrentPath;
        if (string.IsNullOrWhiteSpace(path))
            return;
        if (_currentPreviewWasCloudPlaceholder)
        {
            DiagLog.Write("App", $"image sidecars skipped for cloud-origin preview: {path}");
            ClearImageSidecars();
            return;
        }

        int generation = _previewSession.Generation;
        CancellationToken token = CurrentPreviewToken;
        DispatcherQueue.TryEnqueue(() =>
        {
            if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                return;
            _ = StartImageSidecarLoadsAfterDelayAsync(ready, path, generation, token);
        });
    }

    private async Task StartImageSidecarLoadsAfterDelayAsync(PreviewReady ready, string path, int generation, CancellationToken token)
    {
        try
        {
            await Task.Delay(ImageSidecarLoadDelayMs, token);
            if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                return;
            bool stillRequiresHydration = await Task.Run(() => CloudFileStatus.MayRequireHydration(path), token);
            if (stillRequiresHydration)
            {
                DiagLog.Write("App", $"image sidecars skipped while cloud hydration remains pending: {path}");
                return;
            }

            DispatcherQueue.TryEnqueue(() =>
            {
                if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                    return;
                StartImageSidecarLoads(ready);
            });
        }
        catch (OperationCanceledException)
        {
        }
    }

    private void ClearImageSidecars()
    {
        _imageSidecarController?.Clear();
        ResetExifDetails();
    }

    private void ResetExifDetails()
        => _exifPresenter?.Reset();

    private async Task LoadImageMetadataAsync(string path, int generation, CancellationToken token)
    {
        try
        {
            using var trace = DiagLog.TraceScope("App", $"image metadata load gen={generation}; path={path}", 250);
            ImageMetadata? nativeMetadata = await Task.Run(() => _native.TryPreviewImageMetadata(path), token);
            if (nativeMetadata is not null && RenderNativeImageMetadata(path, generation, token, nativeMetadata))
            {
                if (ShouldSupplementNativeImageMetadata(nativeMetadata))
                    _ = LoadWindowsImageMetadataAfterDelayAsync(path, generation, token);
                return;
            }

            await LoadWindowsImageMetadataAsync(path, generation, token);
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "image metadata load failed: " + ex.Message);
        }
    }

    private async Task LoadWindowsImageMetadataAfterDelayAsync(string path, int generation, CancellationToken token)
    {
        try
        {
            await Task.Delay(WindowsImageMetadataSupplementDelayMs, token);
            if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                return;

            using var trace = DiagLog.TraceScope("App", $"windows image metadata supplement gen={generation}; path={path}", 250);
            await LoadWindowsImageMetadataAsync(path, generation, token);
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "windows image metadata supplement failed: " + ex.Message);
        }
    }

    private async Task LoadWindowsImageMetadataAsync(string path, int generation, CancellationToken token)
    {
        try
        {
            StorageFile file = await StorageFile
                .GetFileFromPathAsync(path)
                .AsTask(token)
                .WaitAsync(ImageMetadataTimeout, token);
            ImageProperties image = await file.Properties
                .GetImagePropertiesAsync()
                .AsTask(token)
                .WaitAsync(ImageMetadataTimeout, token);
            var names = new[]
            {
                "System.Image.HorizontalSize",
                "System.Image.VerticalSize",
                "System.Image.HorizontalResolution",
                "System.Image.VerticalResolution",
                "System.Image.BitDepth",
                "System.Image.ColorSpace",
                "System.Image.Compression",
                "System.Photo.CameraManufacturer",
                "System.Photo.CameraModel",
                "System.Photo.LensManufacturer",
                "System.Photo.LensModel",
                "System.Photo.FocalLength",
                "System.Photo.FocalLengthInFilm",
                "System.Photo.FNumber",
                "System.Photo.MaxAperture",
                "System.Photo.ExposureTime",
                "System.Photo.ISOSpeed",
                "System.Photo.ExposureBias",
                "System.Photo.ExposureProgram",
                "System.Photo.ExposureMode",
                "System.Photo.Flash",
                "System.Photo.MeteringMode",
                "System.Photo.WhiteBalance",
                "System.Photo.LightSource",
                "System.Photo.DigitalZoom",
                "System.Photo.SubjectDistance",
                "System.Photo.Contrast",
                "System.Photo.Saturation",
                "System.Photo.Sharpness",
                "System.Photo.GainControl",
                "System.Photo.PhotometricInterpretation",
                "System.Photo.EXIFVersion",
                "System.ApplicationName",
                "System.SoftwareUsed",
                "System.GPS.Altitude",
                "System.GPS.ImgDirection",
            };
            IDictionary<string, object> props = await RetrieveImagePropertiesAsync(file, names, token);
            token.ThrowIfCancellationRequested();

            var rows = new List<(string Label, string Value)>();
            AddIfValue(rows, "Dimensions", image.Width > 0 && image.Height > 0 ? $"{image.Width:N0} x {image.Height:N0}" : null);
            AddIfValue(rows, "Resolution", FormatResolution(
                PropText(props, "System.Image.HorizontalResolution"),
                PropText(props, "System.Image.VerticalResolution")));
            AddIfValue(rows, "Date taken", image.DateTaken.Year > 1900 ? image.DateTaken.LocalDateTime.ToString("G") : null);
            AddIfValue(rows, "Camera", JoinNonEmpty(
                image.CameraManufacturer,
                image.CameraModel,
                PropText(props, "System.Photo.CameraManufacturer"),
                PropText(props, "System.Photo.CameraModel")));
            AddIfValue(rows, "Lens", JoinNonEmpty(PropText(props, "System.Photo.LensManufacturer"), PropText(props, "System.Photo.LensModel")));
            AddIfValue(rows, "Focal length", FormatNumberWithUnit(PropText(props, "System.Photo.FocalLength"), "mm"));
            AddIfValue(rows, "35mm equivalent", FormatNumberWithUnit(PropText(props, "System.Photo.FocalLengthInFilm"), "mm"));
            AddIfValue(rows, "Aperture", FormatAperture(PropText(props, "System.Photo.FNumber")));
            AddIfValue(rows, "Max aperture", FormatAperture(PropText(props, "System.Photo.MaxAperture")));
            AddIfValue(rows, "Shutter speed", FormatExposure(PropText(props, "System.Photo.ExposureTime")));
            AddIfValue(rows, "ISO", PropText(props, "System.Photo.ISOSpeed"));
            AddIfValue(rows, "Exposure bias", FormatNumberWithUnit(PropText(props, "System.Photo.ExposureBias"), "EV"));
            AddIfValue(rows, "Exposure program", FormatExposureProgram(PropText(props, "System.Photo.ExposureProgram")));
            AddIfValue(rows, "Exposure mode", FormatExposureMode(PropText(props, "System.Photo.ExposureMode")));
            AddIfValue(rows, "Metering", FormatMeteringMode(PropText(props, "System.Photo.MeteringMode")));
            AddIfValue(rows, "White balance", FormatWhiteBalance(PropText(props, "System.Photo.WhiteBalance")));
            AddIfValue(rows, "Light source", FormatLightSource(PropText(props, "System.Photo.LightSource")));
            AddIfValue(rows, "Flash", FormatFlash(PropText(props, "System.Photo.Flash")));
            AddIfValue(rows, "Digital zoom", FormatNumberWithUnit(PropText(props, "System.Photo.DigitalZoom"), "x"));
            AddIfValue(rows, "Subject distance", FormatNumberWithUnit(PropText(props, "System.Photo.SubjectDistance"), "m"));
            AddIfValue(rows, "Orientation", image.Orientation.ToString());
            AddIfValue(rows, "Bit depth", FormatNumberWithUnit(PropText(props, "System.Image.BitDepth"), "bit"));
            AddIfValue(rows, "Color space", FormatColorSpace(PropText(props, "System.Image.ColorSpace")));
            AddIfValue(rows, "Compression", FormatCompression(PropText(props, "System.Image.Compression")));
            AddIfValue(rows, "Photometric", PropText(props, "System.Photo.PhotometricInterpretation"));
            AddIfValue(rows, "Contrast", FormatNormalHardSoft(PropText(props, "System.Photo.Contrast")));
            AddIfValue(rows, "Saturation", FormatNormalHardSoft(PropText(props, "System.Photo.Saturation")));
            AddIfValue(rows, "Sharpness", FormatNormalHardSoft(PropText(props, "System.Photo.Sharpness")));
            AddIfValue(rows, "Gain control", FormatGainControl(PropText(props, "System.Photo.GainControl")));
            AddIfValue(rows, "Location", FormatLocation(image.Latitude, image.Longitude));
            AddIfValue(rows, "Altitude", FormatNumberWithUnit(PropText(props, "System.GPS.Altitude"), "m"));
            AddIfValue(rows, "Direction", FormatNumberWithUnit(PropText(props, "System.GPS.ImgDirection"), "deg"));
            AddIfValue(rows, "Software", JoinNonEmpty(PropText(props, "System.ApplicationName"), PropText(props, "System.SoftwareUsed")));
            AddIfValue(rows, "EXIF version", PropText(props, "System.Photo.EXIFVersion"));

            if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                return;

            DispatcherQueue.TryEnqueue(() =>
            {
                if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                    return;
                _exifPresenter?.RenderRows(rows, image.Latitude, image.Longitude);
            });
        }
        catch (OperationCanceledException)
        {
        }
        catch (TimeoutException)
        {
            DiagLog.Write("App", $"image metadata timed out gen={generation}");
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "image metadata load failed: " + ex.Message);
        }
    }

    private static bool ShouldSupplementNativeImageMetadata(ImageMetadata metadata)
        => metadata.Width is null or 0
            || metadata.Height is null or 0
            || (string.IsNullOrWhiteSpace(metadata.Make) && string.IsNullOrWhiteSpace(metadata.Model))
            || (metadata.FNumber is null && metadata.ExposureTime is null && metadata.Iso is null && metadata.FocalLength is null)
            || metadata.MaxAperture is null
            || metadata.FocalLengthIn35mmFilm is null
            || metadata.ExposureProgram is null
            || metadata.ExposureMode is null
            || metadata.LightSource is null
            || metadata.DigitalZoomRatio is null
            || metadata.SubjectDistance is null
            || metadata.Contrast is null
            || metadata.Saturation is null
            || metadata.Sharpness is null
            || metadata.GainControl is null
            || string.IsNullOrWhiteSpace(metadata.ExifVersion);

    private bool RenderNativeImageMetadata(string path, int generation, CancellationToken token, ImageMetadata metadata)
    {
        var rows = new List<(string Label, string Value)>();
        AddIfValue(rows, "Format", metadata.Format);
        AddIfValue(rows, "Title", metadata.Title);
        AddIfValue(rows, "Comment", metadata.Comment);
        AddIfValue(rows, "Dimensions", metadata.Width is > 0 && metadata.Height is > 0 ? $"{metadata.Width.Value:N0} x {metadata.Height.Value:N0}" : null);
        AddIfValue(rows, "Bit depth", metadata.BitDepth?.ToString(CultureInfo.InvariantCulture));
        AddIfValue(rows, "Color type", metadata.ColorType);
        AddIfValue(rows, "Compression", metadata.Compression);
        AddIfValue(rows, "Alpha", metadata.HasAlpha.HasValue ? (metadata.HasAlpha.Value ? "yes" : "no") : null);
        AddIfValue(rows, "Interlace", metadata.Interlace);
        AddIfValue(rows, "Animated", metadata.Animated.HasValue ? (metadata.Animated.Value ? "yes" : "no") : null);
        AddIfValue(rows, "Frames", metadata.FrameCount?.ToString(CultureInfo.InvariantCulture));
        AddIfValue(rows, "Animation duration", metadata.DurationMs is > 0 ? $"{metadata.DurationMs.Value / 1000.0:0.###} s" : null);
        AddIfValue(rows, "Date taken", FormatExifDateTime(metadata.DateTime));
        AddIfValue(rows, "Camera", JoinNonEmpty(metadata.Make, metadata.Model));
        AddIfValue(rows, "Lens", JoinNonEmpty(metadata.LensMake, metadata.LensModel));
        AddIfValue(rows, "Focal length", FormatDoubleWithUnit(metadata.FocalLength, "mm"));
        AddIfValue(rows, "35mm equivalent", FormatDoubleWithUnit(metadata.FocalLengthIn35mmFilm, "mm"));
        AddIfValue(rows, "Aperture", metadata.FNumber is > 0 ? $"f/{metadata.FNumber.Value:0.0}" : null);
        AddIfValue(rows, "Max aperture", metadata.MaxAperture is > 0 ? $"f/{metadata.MaxAperture.Value:0.0}" : null);
        AddIfValue(rows, "Shutter speed", FormatExposureSeconds(metadata.ExposureTime));
        AddIfValue(rows, "ISO", metadata.Iso?.ToString(CultureInfo.InvariantCulture));
        AddIfValue(rows, "Exposure bias", FormatDoubleWithUnit(metadata.ExposureBias, "EV"));
        AddIfValue(rows, "Exposure program", FormatExifEnum(metadata.ExposureProgram, ExposureProgramNames));
        AddIfValue(rows, "Exposure mode", FormatExifEnum(metadata.ExposureMode, ExposureModeNames));
        AddIfValue(rows, "Metering", FormatExifEnum(metadata.MeteringMode, MeteringModeNames));
        AddIfValue(rows, "White balance", FormatExifEnum(metadata.WhiteBalance, WhiteBalanceNames));
        AddIfValue(rows, "Light source", FormatExifEnum(metadata.LightSource, LightSourceNames));
        AddIfValue(rows, "Flash", FormatFlash(metadata.Flash));
        AddIfValue(rows, "Digital zoom", FormatDoubleWithUnit(metadata.DigitalZoomRatio, "x"));
        AddIfValue(rows, "Subject distance", FormatDoubleWithUnit(metadata.SubjectDistance, "m"));
        AddIfValue(rows, "Orientation", metadata.Orientation?.ToString(CultureInfo.InvariantCulture));
        AddIfValue(rows, "Contrast", FormatExifEnum(metadata.Contrast, NormalHardSoftNames));
        AddIfValue(rows, "Saturation", FormatExifEnum(metadata.Saturation, NormalHardSoftNames));
        AddIfValue(rows, "Sharpness", FormatExifEnum(metadata.Sharpness, NormalHardSoftNames));
        AddIfValue(rows, "Gain control", FormatExifEnum(metadata.GainControl, GainControlNames));
        AddIfValue(rows, "Color space", FormatExifEnum(metadata.ColorSpace, ColorSpaceNames));
        AddIfValue(rows, "Location", FormatLocation(metadata.Latitude, metadata.Longitude));
        AddIfValue(rows, "Altitude", FormatDoubleWithUnit(metadata.Altitude, "m"));
        AddIfValue(rows, "Direction", FormatDoubleWithUnit(metadata.Direction, "deg"));
        AddIfValue(rows, "Software", metadata.Software);
        AddIfValue(rows, "Camera serial", metadata.CameraSerial);
        AddIfValue(rows, "Lens serial", metadata.LensSerial);
        AddIfValue(rows, "EXIF version", metadata.ExifVersion);

        if (rows.Count == 0)
            return false;
        if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
            return true;

        DispatcherQueue.TryEnqueue(() =>
        {
            if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                return;
            _exifPresenter?.RenderRows(rows, metadata.Latitude, metadata.Longitude);
        });
        return true;
    }

    private static string? FormatExifDateTime(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
            return null;
        string trimmed = value.Trim();
        return trimmed.Length >= 10 && trimmed[4] == ':' && trimmed[7] == ':'
            ? trimmed[..4] + "-" + trimmed[5..7] + "-" + trimmed[8..]
            : trimmed;
    }

    private static string? FormatDoubleWithUnit(double? value, string unit)
        => value.HasValue ? $"{value.Value:0.###} {unit}" : null;

    private static string? FormatExposureSeconds(double? value)
    {
        if (!value.HasValue || value.Value <= 0)
            return null;
        return value.Value < 1.0
            ? $"1/{Math.Round(1.0 / value.Value):0} s"
            : $"{value.Value:0.###} s";
    }

    private static string? FormatExifEnum(ushort? value, IReadOnlyDictionary<int, string> names)
        => value.HasValue ? FormatExifEnum(value.Value.ToString(CultureInfo.InvariantCulture), names) : null;

    private static string? FormatFlash(ushort? value)
        => value.HasValue ? FormatFlash(value.Value.ToString(CultureInfo.InvariantCulture)) : null;

    private bool IsImageFilmstripLoadCurrent(string path, int generation, CancellationToken token)
        => IsPreviewGenerationCurrent(generation, token) && _previewSession.IsCurrentPath(path);

    private static void AddIfValue(List<(string Label, string Value)> rows, string label, string? value)
    {
        if (!string.IsNullOrWhiteSpace(value) && value != "Unspecified")
            rows.Add((label, value));
    }

    private static string? JoinNonEmpty(params string?[] values)
    {
        string[] parts = values
            .Where(v => !string.IsNullOrWhiteSpace(v))
            .Select(v => v!.Trim())
            .Distinct(StringComparer.CurrentCultureIgnoreCase)
            .ToArray();
        return parts.Length == 0 ? null : string.Join(" ", parts);
    }

    private static string? PropText(IDictionary<string, object> props, string name)
        => props.TryGetValue(name, out object? value) ? FormatPropertyValue(value) : null;

    private static async Task<IDictionary<string, object>> RetrieveImagePropertiesAsync(
        StorageFile file,
        IReadOnlyList<string> names,
        CancellationToken token)
    {
        try
        {
            return await file.Properties
                .RetrievePropertiesAsync(names)
                .AsTask(token)
                .WaitAsync(ImageMetadataTimeout, token);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (TimeoutException)
        {
            return new Dictionary<string, object>();
        }
        catch
        {
            var result = new Dictionary<string, object>();
            DateTimeOffset deadline = DateTimeOffset.UtcNow + ImageMetadataTimeout;
            foreach (string name in names)
            {
                token.ThrowIfCancellationRequested();
                TimeSpan remaining = deadline - DateTimeOffset.UtcNow;
                if (remaining <= TimeSpan.Zero)
                    break;

                try
                {
                    IDictionary<string, object> one = await file.Properties
                        .RetrievePropertiesAsync([name])
                        .AsTask(token)
                        .WaitAsync(remaining < TimeSpan.FromMilliseconds(150) ? remaining : TimeSpan.FromMilliseconds(150), token);
                    if (one.TryGetValue(name, out object? value) && value is not null)
                        result[name] = value;
                }
                catch (OperationCanceledException)
                {
                    throw;
                }
                catch
                {
                    // Some Windows builds/codecs do not expose every canonical property.
                }
            }

            return result;
        }
    }

    private static string? FormatPropertyValue(object? value)
    {
        if (value is null)
            return null;
        if (value is string s)
            return string.IsNullOrWhiteSpace(s) ? null : s;
        if (value is string[] strings)
            return string.Join(", ", strings.Where(x => !string.IsNullOrWhiteSpace(x)));
        if (value is DateTimeOffset dto)
            return dto.LocalDateTime.ToString("G");
        if (value is double d)
            return d.ToString("0.##");
        if (value is float f)
            return f.ToString("0.##");
        return value.ToString();
    }

    private static string? FormatNumberWithUnit(string? raw, string unit)
        => string.IsNullOrWhiteSpace(raw) ? null : $"{raw} {unit}";

    private static string? FormatResolution(string? horizontal, string? vertical)
    {
        if (string.IsNullOrWhiteSpace(horizontal) && string.IsNullOrWhiteSpace(vertical))
            return null;
        if (string.Equals(horizontal, vertical, StringComparison.OrdinalIgnoreCase))
            return $"{horizontal} dpi";
        return JoinNonEmpty(horizontal, vertical) is { } joined ? $"{joined} dpi" : null;
    }

    private static string? FormatAperture(string? raw)
        => string.IsNullOrWhiteSpace(raw) ? null : $"f/{raw}";

    private static string? FormatExposure(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
            return null;
        if (!double.TryParse(raw, out double seconds) || seconds <= 0)
            return raw + " sec";
        return seconds < 1.0 ? $"1/{Math.Round(1.0 / seconds):0} sec" : $"{seconds:0.##} sec";
    }

    private static string? FormatExposureProgram(string? raw)
        => FormatExifEnum(raw, ExposureProgramNames);

    private static string? FormatExposureMode(string? raw)
        => FormatExifEnum(raw, ExposureModeNames);

    private static string? FormatMeteringMode(string? raw)
        => FormatExifEnum(raw, MeteringModeNames);

    private static string? FormatWhiteBalance(string? raw)
        => FormatExifEnum(raw, WhiteBalanceNames);

    private static string? FormatLightSource(string? raw)
        => FormatExifEnum(raw, LightSourceNames);

    private static string? FormatColorSpace(string? raw)
        => FormatExifEnum(raw, ColorSpaceNames);

    private static string? FormatCompression(string? raw)
        => FormatExifEnum(raw, CompressionNames);

    private static string? FormatNormalHardSoft(string? raw)
        => FormatExifEnum(raw, NormalHardSoftNames);

    private static string? FormatGainControl(string? raw)
        => FormatExifEnum(raw, GainControlNames);

    private static string? FormatExifEnum(string? raw, IReadOnlyDictionary<int, string> names)
    {
        if (string.IsNullOrWhiteSpace(raw))
            return null;
        string trimmed = raw.Trim();
        if (!int.TryParse(trimmed, out int value))
            return trimmed;
        return names.TryGetValue(value, out string? name) ? name : trimmed;
    }

    private static string? FormatFlash(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
            return null;
        if (!int.TryParse(raw, out int flags))
            return raw;

        var parts = new List<string>();
        parts.Add((flags & 0x1) != 0 ? "Fired" : "Did not fire");
        if ((flags & 0x18) == 0x18)
            parts.Add("Auto");
        if ((flags & 0x40) != 0)
            parts.Add("Red-eye reduction");
        if ((flags & 0x6) == 0x4)
            parts.Add("Return detected");
        else if ((flags & 0x6) == 0x6)
            parts.Add("Return not detected");
        return string.Join(", ", parts);
    }

    private static string? FormatLocation(double? latitude, double? longitude)
        => latitude is { } lat && longitude is { } lon ? $"{lat:0.#####}, {lon:0.#####}" : null;

    private static T? FindDescendant<T>(DependencyObject root)
        where T : DependencyObject
    {
        int count = VisualTreeHelper.GetChildrenCount(root);
        for (int i = 0; i < count; i++)
        {
            DependencyObject child = VisualTreeHelper.GetChild(root, i);
            if (child is T typed)
                return typed;
            if (FindDescendant<T>(child) is { } descendant)
                return descendant;
        }

        return null;
    }

    private static bool IsImagePath(string? path)
        => !string.IsNullOrWhiteSpace(path) && ImageExtensions.Contains(Path.GetExtension(path));

    private (double Width, double Height) GetMaxContentSize(double preferredMaxWidth, double preferredMaxHeight)
        => PreviewWindowSizer.GetMaxContentSize(GetWindowId(), preferredMaxWidth, preferredMaxHeight, RasterizationScale);

    private double RasterizationScale
    {
        get
        {
            double scale = RootGrid.XamlRoot?.RasterizationScale ?? 1.0;
            return double.IsFinite(scale) && scale > 0 ? scale : 1.0;
        }
    }

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
            maxHeight,
            RasterizationScale);
        DiagLog.Write("App", $"window resize content={contentWidth:0}x{contentHeight:0}; target={size.Width}x{size.Height}; visible={_previewVisible}; pending={_previewRevealPending}; topmost={setTopmost}");
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
        DiagLog.Write("App", "window temporarily hidden for transition resize");
        _previewTemporarilyHidden = true;
    }

    private void OnImageZoomOutClick(object sender, RoutedEventArgs e)
    {
        if (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible)
            _animatedImagePresenter.ZoomBy(1.0 / 1.15);
        else
            _rasterPresenter?.ZoomBy(1.0 / 1.15);
    }

    private void OnImageZoomInClick(object sender, RoutedEventArgs e)
    {
        if (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible)
            _animatedImagePresenter.ZoomBy(1.15);
        else
            _rasterPresenter?.ZoomBy(1.15);
    }

    private void OnImageZoomFitClick(object sender, RoutedEventArgs e)
    {
        if (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible)
            _animatedImagePresenter.ResetView();
        else
            _rasterPresenter?.ResetView();
    }

    private void OnImageActualSizeClick(object sender, RoutedEventArgs e)
    {
        if (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible)
            _animatedImagePresenter.SetActualSize();
        else
            _rasterPresenter?.SetActualSize();
    }

    private void OnImageAnimationPlaybackClick(object sender, RoutedEventArgs e)
    {
        _animatedImagePresenter?.TogglePlayback();
        UpdateImageAnimationPlaybackButton();
    }

    private void OnCompactInfoRailToggleClick(object sender, RoutedEventArgs e)
    {
        if (!IsCompactRasterChrome)
            return;

        _isCompactInfoRailOpen = CompactInfoRailToggle.IsChecked == true;
        ApplyRasterChromeLayout();
        if (_isCompactInfoRailOpen)
            InfoTabButton.Focus(FocusState.Programmatic);
    }

    private void UpdateImageAnimationPlaybackButton()
    {
        bool canToggle = !PrefersReducedMotion && _animatedImagePresenter?.CanTogglePlayback == true;
        ImageAnimationPlaybackButton.Visibility = canToggle ? Visibility.Visible : Visibility.Collapsed;
        bool paused = _animatedImagePresenter?.IsPlaybackPaused == true;
        ImageAnimationPlaybackIcon.Glyph = paused ? "\uE768" : "\uE769";
        string action = paused ? UiStrings.PlayAnimation : UiStrings.PauseAnimation;
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(ImageAnimationPlaybackButton, action);
        ToolTipService.SetToolTip(ImageAnimationPlaybackButton, action);
    }

    private void OnImageZoomPresetClick(object sender, RoutedEventArgs e)
    {
        if (sender is Button { Tag: string raw } && double.TryParse(raw, out double zoom))
        {
            if (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible)
                _animatedImagePresenter.SetZoom(zoom);
            else
                _rasterPresenter?.SetZoom(zoom);
        }
    }

    private void OnImageFilmstripListLoaded(object sender, RoutedEventArgs e)
        => _imageFilmstripScrollViewer = FindDescendant<ScrollViewer>(ImageFilmstripList);

    private void OnImageFilmstripPointerPressed(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        _imageFilmstripScrollViewer ??= FindDescendant<ScrollViewer>(ImageFilmstripList);
        if (_imageFilmstripScrollViewer is null)
            return;

        var point = e.GetCurrentPoint(ImageFilmstripList);
        if (!point.Properties.IsLeftButtonPressed)
            return;

        _imageFilmstripDragging = true;
        _imageFilmstripSuppressClick = false;
        _imageFilmstripDragStart = point.Position;
        _imageFilmstripDragStartOffset = _imageFilmstripScrollViewer.HorizontalOffset;
        ImageFilmstripList.CapturePointer(e.Pointer);
    }

    private void OnImageFilmstripPointerMoved(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (!_imageFilmstripDragging || _imageFilmstripScrollViewer is null)
            return;

        Windows.Foundation.Point point = e.GetCurrentPoint(ImageFilmstripList).Position;
        double delta = point.X - _imageFilmstripDragStart.X;
        if (Math.Abs(delta) > 5)
            _imageFilmstripSuppressClick = true;

        _imageFilmstripScrollViewer.ChangeView(_imageFilmstripDragStartOffset - delta, null, null, disableAnimation: true);
        e.Handled = true;
    }

    private void OnImageFilmstripPointerReleased(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (!_imageFilmstripDragging)
            return;

        _imageFilmstripDragging = false;
        try { ImageFilmstripList.ReleasePointerCapture(e.Pointer); } catch { }
    }

    private void OnImageFilmstripPointerCanceled(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => EndImageFilmstripDrag(e.Pointer);

    private void OnImageFilmstripPointerCaptureLost(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _imageFilmstripDragging = false;

    private void OnImageFilmstripPointerWheelChanged(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        _imageFilmstripScrollViewer ??= FindDescendant<ScrollViewer>(ImageFilmstripList);
        if (_imageFilmstripScrollViewer is null)
            return;

        int delta = e.GetCurrentPoint(ImageFilmstripList).Properties.MouseWheelDelta;
        if (delta == 0)
            return;

        _imageFilmstripScrollViewer.ChangeView(
            _imageFilmstripScrollViewer.HorizontalOffset - delta,
            null,
            null,
            disableAnimation: false);
        e.Handled = true;
    }

    private void EndImageFilmstripDrag(Microsoft.UI.Xaml.Input.Pointer pointer)
    {
        _imageFilmstripDragging = false;
        try { ImageFilmstripList.ReleasePointerCapture(pointer); } catch { }
    }

    private async void OnPreviousImageClick(object sender, RoutedEventArgs e)
        => await NavigateImageSiblingAsync(-1);

    private async void OnNextImageClick(object sender, RoutedEventArgs e)
        => await NavigateImageSiblingAsync(1);

    private async void OnImageFilmstripItemClick(object sender, ItemClickEventArgs e)
    {
        if (_imageFilmstripSuppressClick)
        {
            _imageFilmstripSuppressClick = false;
            return;
        }

        if (e.ClickedItem is ImageFilmstripItem item)
            await PreviewImagePathAsync(item.Path);
    }

    private async Task NavigateImageSiblingAsync(int delta)
    {
        string? currentPath = _previewSession.CurrentPath;
        if (string.IsNullOrWhiteSpace(currentPath))
            return;

        string? nextPath = _imageSidecarController?.GetRelativePath(currentPath, delta);
        if (string.IsNullOrWhiteSpace(nextPath))
            return;

        await PreviewImagePathAsync(nextPath);
    }

    private async Task PreviewImagePathAsync(string path)
    {
        if (string.IsNullOrWhiteSpace(path)
            || _previewSession.IsCurrentPath(path))
        {
            return;
        }

        _imageSidecarController?.SelectCurrent(path);
        await PreviewWindowPathAsync(path);
    }

    private void OnPreviewInfoTabClick(object sender, RoutedEventArgs e)
        => SetPreviewInfoRailTab(PreviewInfoRailTab.Info);

    private void OnPreviewExifTabClick(object sender, RoutedEventArgs e)
        => SetPreviewInfoRailTab(PreviewInfoRailTab.Exif);

    private void OnPreviewMoreTabClick(object sender, RoutedEventArgs e)
        => SetPreviewInfoRailTab(PreviewInfoRailTab.More);

    private void OnOpenExifLocationInMapsClick(object sender, RoutedEventArgs e)
        => _exifPresenter?.OpenLocationInGoogleMaps();

    private void SetPreviewInfoRailTab(PreviewInfoRailTab tab)
    {
        if (InfoDetailsPanel is null)
            return;

        InfoDetailsPanel.Visibility = tab == PreviewInfoRailTab.Info ? Visibility.Visible : Visibility.Collapsed;
        ExifDetailsPanel.Visibility = tab == PreviewInfoRailTab.Exif ? Visibility.Visible : Visibility.Collapsed;
        MoreActionsPanel.Visibility = tab == PreviewInfoRailTab.More ? Visibility.Visible : Visibility.Collapsed;
        InfoOpenFileLocationButton.Visibility = tab == PreviewInfoRailTab.Info ? Visibility.Visible : Visibility.Collapsed;

        SetPreviewInfoTabVisual(InfoTabButton, InfoTabUnderline, tab == PreviewInfoRailTab.Info);
        SetPreviewInfoTabVisual(ExifTabButton, ExifTabUnderline, tab == PreviewInfoRailTab.Exif);
        SetPreviewInfoTabVisual(MoreTabButton, MoreTabUnderline, tab == PreviewInfoRailTab.More);
    }

    private static void SetPreviewInfoTabVisual(Button button, FrameworkElement underline, bool selected)
    {
        underline.Visibility = selected ? Visibility.Visible : Visibility.Collapsed;
        button.Opacity = selected ? 1.0 : 0.72;
        button.BorderThickness = selected ? new Thickness(1) : new Thickness(0);
    }

    private void OnRootGridKeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
    {
        if (e.Key == Windows.System.VirtualKey.Space && ShouldHandleSpaceAsPreviewClose())
        {
            e.Handled = true;
            ClosePreviewFromKeyboard();
            return;
        }

        bool imagePreviewVisible =
            (_rasterPresenter?.HasSurface == true && PreviewRoot.Visibility == Visibility.Visible)
            || (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible);
        if (!imagePreviewVisible)
            return;

        bool controlDown = (Microsoft.UI.Input.InputKeyboardSource
            .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Control)
            & Windows.UI.Core.CoreVirtualKeyStates.Down) == Windows.UI.Core.CoreVirtualKeyStates.Down;

        if (e.Key == Windows.System.VirtualKey.Home
            || (controlDown && e.Key is Windows.System.VirtualKey.Number0 or Windows.System.VirtualKey.NumberPad0))
        {
            if (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible)
                _animatedImagePresenter.ResetView();
            else
                _rasterPresenter?.ResetView();
            e.Handled = true;
            return;
        }

        if (e.Key == Windows.System.VirtualKey.Left)
        {
            _ = NavigateImageSiblingAsync(-1);
            e.Handled = true;
            return;
        }

        if (e.Key == Windows.System.VirtualKey.Right)
        {
            _ = NavigateImageSiblingAsync(1);
            e.Handled = true;
        }
    }

    private void ClosePreviewFromKeyboard()
    {
        if (!IsPreviewActiveForClose())
            return;
        if (_keyboardCloseQueued)
            return;

        _keyboardCloseQueued = true;
        DiagLog.Write("App", "keyboard close queued");
        _ = HandleNativeIntentSafelyAsync(new NativeIntent(PreviewIntent.Close, []));
    }

    private bool IsPreviewActiveForClose()
        => _previewVisible || _previewRevealPending;

    private bool ShouldHandleSpaceAsPreviewClose()
    {
        if (!IsPreviewActiveForClose() || _isModalDialogOpen)
            return false;

        object? focused = RootGrid.XamlRoot is { } xamlRoot
            ? FocusManager.GetFocusedElement(xamlRoot)
            : null;
        return focused is not Control;
    }

    private void OnOpenFileLocationClick(object sender, RoutedEventArgs e)
        => OpenCurrentPreviewPath(revealInExplorer: true);

    private void OnOpenPreviewFileClick(object sender, RoutedEventArgs e)
        => OpenCurrentPreviewPath(revealInExplorer: false);

    private async void OnRetryPreviewClick(object sender, RoutedEventArgs e)
    {
        string? path = _previewSession.CurrentPath;
        if (!string.IsNullOrWhiteSpace(path))
            await PreviewWindowPathAsync(path);
    }

    private async void OnCopyPreviewFileClick(object sender, RoutedEventArgs e)
    {
        string? path = _previewSession.CurrentPath;
        if (string.IsNullOrWhiteSpace(path))
            return;

        try
        {
            var package = new DataPackage();
            if (System.IO.File.Exists(path))
            {
                StorageFile file = await StorageFile.GetFileFromPathAsync(path);
                package.RequestedOperation = DataPackageOperation.Copy;
                package.SetStorageItems([file]);
                StatusText.Text = UiStrings.FileCopied;
            }
            else
            {
                package.SetText(path);
                StatusText.Text = UiStrings.PathCopied;
            }
            Clipboard.SetContent(package);
            StatusBar.Visibility = Visibility.Visible;
            RefreshCurrentImageFilmstrip();
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "copy preview file failed: " + ex.Message);
        }
    }

    private async void OnDeletePreviewFileClick(object sender, RoutedEventArgs e)
    {
        string? path = _previewSession.CurrentPath;
        if (string.IsNullOrWhiteSpace(path) || !System.IO.File.Exists(path))
            return;

        var dialog = new ContentDialog
        {
            Title = UiStrings.DeleteFileTitle,
            Content = string.Format(CultureInfo.CurrentCulture, UiStrings.DeleteFileMessage, Path.GetFileName(path)),
            PrimaryButtonText = UiStrings.MoveToRecycleBin,
            CloseButtonText = UiStrings.Cancel,
            DefaultButton = ContentDialogButton.Close,
            XamlRoot = RootGrid.XamlRoot,
        };

        ContentDialogResult result;
        _isModalDialogOpen = true;
        try
        {
            result = await dialog.ShowAsync();
        }
        finally
        {
            _isModalDialogOpen = false;
        }

        if (result != ContentDialogResult.Primary)
            return;

        string? nextPath = _imageSidecarController?.NextPathAfterDelete(path);
        try
        {
            FileSystem.DeleteFile(path, UIOption.OnlyErrorDialogs, RecycleOption.SendToRecycleBin);
            _imageSidecarController?.RemovePath(path);
            StatusText.Text = UiStrings.MovedToRecycleBin;
            StatusBar.Visibility = Visibility.Visible;

            if (!string.IsNullOrWhiteSpace(nextPath) && System.IO.File.Exists(nextPath))
            {
                await PreviewWindowPathAsync(nextPath);
                return;
            }

            await ClosePreviewImmediatelyAsync();
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "delete preview file failed: " + ex.Message);
            StatusText.Text = ex.Message;
            StatusBar.Visibility = Visibility.Visible;
        }
    }

    private void RefreshCurrentImageFilmstrip()
    {
        string? path = _previewSession.CurrentPath;
        if (string.IsNullOrWhiteSpace(path) || !IsImagePath(path) || !System.IO.File.Exists(path))
            return;

        int generation = _previewSession.Generation;
        CancellationToken token = CurrentPreviewToken;
        _ = _imageSidecarController?.LoadFilmstripAsync(path, generation, token);
    }

    private void OpenCurrentPreviewPath(bool revealInExplorer)
    {
        string? path = _previewSession.CurrentPath;
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
                RefreshCurrentImageFilmstrip();
                return;
            }

            if (Directory.Exists(path))
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = path,
                    UseShellExecute = true,
                });
                RefreshCurrentImageFilmstrip();
                return;
            }

            if (System.IO.File.Exists(path))
            {
                System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo
                {
                    FileName = path,
                    UseShellExecute = true,
                });
                RefreshCurrentImageFilmstrip();
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

    private void OnAnimatedImageRootPointerPressed(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _animatedImagePresenter?.OnPointerPressed(e);

    private void OnAnimatedImageRootPointerMoved(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _animatedImagePresenter?.OnPointerMoved(e);

    private void OnAnimatedImageRootPointerReleased(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _animatedImagePresenter?.OnPointerReleased(e);

    private void OnAnimatedImageRootPointerWheelChanged(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _animatedImagePresenter?.OnPointerWheelChanged(e);

    private void OnAnimatedImageRootDoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        => _animatedImagePresenter?.OnDoubleTapped(e);

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
            SystemBackdrop = PrefersReducedTransparency ? null : new MicaBackdrop();
        }
        catch { SystemBackdrop = null; }
    }

    private bool IsHighContrast => _accessibilitySettings.HighContrast;
    private bool PrefersReducedTransparency => IsHighContrast || !_uiSettings.AdvancedEffectsEnabled;
    private bool PrefersReducedMotion => !_uiSettings.AnimationsEnabled;

    private void ApplyAccessibilityVisuals()
    {
        TrySetBackdrop();
        UpdateTitleBarColors();
    }

    private void UpdateTitleBarColors()
    {
        try
        {
            if (AppWindowTitleBar.IsCustomizationSupported())
            {
                var titleBar = GetAppWindow().TitleBar;
                if (IsHighContrast)
                {
                    titleBar.ButtonForegroundColor = null;
                    titleBar.ButtonHoverForegroundColor = null;
                    titleBar.ButtonHoverBackgroundColor = null;
                    titleBar.ButtonPressedForegroundColor = null;
                    titleBar.ButtonPressedBackgroundColor = null;
                    titleBar.ButtonInactiveForegroundColor = null;
                    titleBar.ButtonInactiveBackgroundColor = null;
                    titleBar.ButtonBackgroundColor = null;
                    return;
                }
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
        using var trace = DiagLog.TraceScope("App", $"window show activate={activate}; resizeDefault={resizeToDefault}; visible={_previewVisible}", 100);
        bool openingFromHidden = !_previewVisible;
        if (activate)
            _windowController.SetNoActivateStyle(enabled: false);
        else
            _windowController.SetNoActivateStyle(enabled: false);
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
        _lastPreviewRevealTick = Environment.TickCount64;
        _lastPreviewRevealPath = _previewSession.CurrentPath;
        SetBackgroundEfficiency(enabled: false);
        _native.SetPreviewVisible(true);
        PreviewContentHost.Opacity = 1;
    }

    private void HidePreviewWindow()
    {
        using var trace = DiagLog.TraceScope("App", $"window hide visible={_previewVisible}; request={_previewSession.CurrentRequestId}", 100);
        CancelSwitchDebounce();
        _keyboardCloseQueued = false;
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
        _parserSupervisor?.SetBackgroundEfficiency(enabled);
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
        _parserSupervisor?.Stop();
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

        byte[] magic = new byte[64];
        try
        {
            using var fs = System.IO.File.OpenRead(path);
            int n = fs.Read(magic, 0, magic.Length);
            if (n < magic.Length) Array.Resize(ref magic, n);
        }
        catch { /* probe is best-effort in the scaffold; the real probe comes from native */ }
        long size = 0;
        try { size = new FileInfo(path).Length; } catch { }
        return new FileProbe(path, System.IO.Path.GetExtension(path), magic)
        {
            Kind = FallbackFileProbe.IsText(path, magic, isEmptyFile: size == 0 && System.IO.File.Exists(path)) ? "text" : "unknown",
            Size = size,
        };
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

    private static string ResolveParserHostExePath()
    {
        string parserHost = System.IO.Path.Combine(AppContext.BaseDirectory, "ParserHost", "QuickLook.Next.ParserHost.exe");
        if (System.IO.File.Exists(parserHost)) return parserHost;
        string local = System.IO.Path.Combine(AppContext.BaseDirectory, "QuickLook.Next.ParserHost.exe");
        if (System.IO.File.Exists(local)) return local;
        return System.IO.Path.GetFullPath(System.IO.Path.Combine(AppContext.BaseDirectory,
            @"..\..\..\..\..\QuickLook.Next.ParserHost\bin\Debug\net10.0-windows10.0.19041.0\win-x64\QuickLook.Next.ParserHost.exe"));
    }

    private static bool IsParserHostPreview(FileProbe probe)
        => probe.Kind.Equals("archive", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("package", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("office", StringComparison.OrdinalIgnoreCase);

    private static bool IsCloudParserHostPreview(FileProbe probe)
        => probe.Kind.Equals("text", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("ebook", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("executable", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("torrent", StringComparison.OrdinalIgnoreCase)
           || probe.Kind.Equals("certificate", StringComparison.OrdinalIgnoreCase);

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
    private void TextWordWrapButton_Click(object sender, RoutedEventArgs e)
    {
        bool wrap = TextWordWrapButton.IsChecked == true;
        TextScrollViewer.HorizontalScrollBarVisibility = wrap ? ScrollBarVisibility.Disabled : ScrollBarVisibility.Auto;
        TextPreviewBlock.TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap;
        
        // Also update TextListView items if they support wrap
        if (TextListView.ItemsSource is ObservableCollection<TextLineItem> items)
        {
            foreach (var item in items)
            {
                if (item.Content is TextBlock tb)
                {
                    tb.TextWrapping = wrap ? TextWrapping.Wrap : TextWrapping.NoWrap;
                }
            }
        }
    }

    private void TextLineNumbersButton_Click(object sender, RoutedEventArgs e)
    {
        bool showLineNumbers = TextLineNumbersButton.IsChecked == true;
        // Since modifying DataTemplate properties runtime is tricky, we can find the TextBlocks
        // in the visual tree, or simply ignore it for now as a toggle is visually sufficient for a demo, 
        // or just toggle a global resource.
        // For now, toggle a static property or resource if needed, but a quick way is to just 
        // set the ListView ItemTemplate to a different one, or walk the visual tree.
        WalkVisualTreeToToggleLineNumbers(TextListView, showLineNumbers);
    }
    
    private void WalkVisualTreeToToggleLineNumbers(DependencyObject root, bool show)
    {
        int count = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChildrenCount(root);
        for (int i = 0; i < count; i++)
        {
            var child = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetChild(root, i);
            if (child is TextBlock tb && tb.Foreground is SolidColorBrush brush && 
                brush.Color == ((SolidColorBrush)Application.Current.Resources["TextFillColorSecondaryBrush"]).Color &&
                tb.Margin.Right == 16)
            {
                tb.Visibility = show ? Visibility.Visible : Visibility.Collapsed;
                if (Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(tb) is Grid g && g.ColumnDefinitions.Count > 0)
                {
                    g.ColumnDefinitions[0].Width = show ? new GridLength(60) : new GridLength(0);
                }
            }
            WalkVisualTreeToToggleLineNumbers(child, show);
        }
    }

    private void TextSearchButton_Click(object sender, RoutedEventArgs e)
    {
        // Placeholder for search logic
        // In a real app, this would open a find bar, but for now we can just show a flyout or focus.
        var dialog = new ContentDialog
        {
            Title = "Search (Coming Soon)",
            Content = "Search functionality is currently being refined.",
            CloseButtonText = UiStrings.DialogOk,
            XamlRoot = this.Content.XamlRoot
        };
        _ = dialog.ShowAsync();
    }
}
