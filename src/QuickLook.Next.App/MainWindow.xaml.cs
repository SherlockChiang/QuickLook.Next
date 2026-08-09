using System.Numerics;
using System.IO;
using System.Diagnostics;
using System.Globalization;
using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.VisualBasic.FileIO;
using Microsoft.UI;
using Microsoft.UI.Composition;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
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
    private const double MaxTextWindowWidth = 1440;
    private const double MaxTextWindowHeight = 1000;
    private const double RasterInfoRailWidth = 246;
    private const double RasterToolbarHeight = 162;
    private const double CompactRasterChromeWidth = 720;
    private const double MinRasterChromeContentWidth = 760;
    private const double RasterContentMargin = 14;
    private const int SwitchDebounceMs = 30;
    private const int ImageSidecarLoadDelayMs = 180;
    private const int DuplicateOpenCloseGuardMs = 750;
    private static readonly TimeSpan CloudPreviewTimeout = TimeSpan.FromSeconds(45);

    private readonly NativeBridge _native = new();
    private readonly NativeThumbnailScheduler _thumbnailScheduler;
    private readonly FolderListingCache _folderListingCache;
    private readonly PreviewWindowController _windowController;
    private readonly TitleBarInsetController _titleBarInsetController;
    private TextPreviewPresenter? _textPresenter;
    private TablePreviewPresenter? _tablePresenter;
    private ListingPreviewPresenter? _listingPresenter;
    private OfficePreviewPresenter? _officePresenter;
    private RasterPreviewPresenter? _rasterPresenter;
    private AnimatedImagePreviewPresenter? _animatedImagePresenter;
    private ImageWaveformPresenter? _imageWaveformPresenter;
    private bool _isRasterChromeEnabled;
    private bool _isCompactInfoRailOpen;
    private ImageSidecarController? _imageSidecarController;
    private ExifPreviewPresenter? _exifPresenter;
    private PdfPreviewPresenter? _pdfPresenter;
    private MediaPreviewPresenter? _mediaPresenter;
    private Compositor? _compositor;
    private TrayIconManager? _trayIcon;
    private SettingsWindow? _settingsWindow;
    private RasterHostSupervisor? _supervisor;
    private ParserHostSupervisor? _parserSupervisor;
    private ShellBrokerSupervisor? _shellBroker;
    private readonly Dictionary<string, PreviewHostOwner> _requestHosts = new(StringComparer.Ordinal);
    private PreviewKeyboardHook? _previewKeyboardHook;
    private UiThreadWatchdog? _uiWatchdog;
    private readonly PreviewSession _previewSession = new();
    private readonly CancellationTokenSource _lifetimeCts = new();
    private FileProbe? _currentProbe;
    private bool _currentPreviewWasCloudPlaceholder;
    private ArchiveEntryHandoff? _currentArchiveEntryHandoff;
    private readonly PreviewPanelController _panelController;
    private bool _isStarted;
    private bool _previewVisible;
    private bool _isFullscreen;
    private bool? _backgroundEfficiencyEnabled;
    private CancellationTokenSource? _switchDebounceCts;
    private bool _previewRevealPending;
    private bool _previewTemporarilyHidden;
    private bool _keyboardCloseQueued;
    private bool _isModalDialogOpen;
    private readonly SemaphoreSlim _modalDialogGate = new(1, 1);
    private long _lastPreviewRevealTick;
    private long _loadingShellShowStarted;
    private PreviewLifecycleTiming? _previewTiming;
    private string? _lastPreviewRevealPath;
    private ScrollViewer? _imageFilmstripScrollViewer;
    private bool _imageFilmstripDragging;
    private bool _imageFilmstripSuppressClick;
    private bool _suppressTextSearchTextChanged;
    private Windows.Foundation.Point _imageFilmstripDragStart;
    private double _imageFilmstripDragStartOffset;
    private readonly UISettings _uiSettings = new();
    private readonly AccessibilitySettings _accessibilitySettings = new();

    private static readonly string[] ByteSizeFormatResourceKeys =
    [
        "ByteSizeBytesFormat",
        "ByteSizeKilobytesFormat",
        "ByteSizeMegabytesFormat",
        "ByteSizeGigabytesFormat",
        "ByteSizeTerabytesFormat",
    ];
    private static readonly HashSet<string> ImageExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".png", ".jpg", ".jpeg", ".jpe", ".gif", ".bmp", ".dib", ".tif", ".tiff", ".webp", ".ico",
        ".heic", ".heif", ".avif", ".jxl", ".svg",
    };
    private static readonly IReadOnlyDictionary<int, string> ExposureProgramResourceKeys = new Dictionary<int, string>
    {
        [0] = "ImageMetadataValueNotDefined",
        [1] = "ImageMetadataValueManual",
        [2] = "ImageMetadataValueNormalProgram",
        [3] = "ImageMetadataValueAperturePriority",
        [4] = "ImageMetadataValueShutterPriority",
        [5] = "ImageMetadataValueCreativeProgram",
        [6] = "ImageMetadataValueActionProgram",
        [7] = "ImageMetadataValuePortraitMode",
        [8] = "ImageMetadataValueLandscapeMode",
    };
    private static readonly IReadOnlyDictionary<int, string> ExposureModeResourceKeys = new Dictionary<int, string>
    {
        [0] = "ImageMetadataValueAutoExposure",
        [1] = "ImageMetadataValueManualExposure",
        [2] = "ImageMetadataValueAutoBracket",
    };
    private static readonly IReadOnlyDictionary<int, string> MeteringModeResourceKeys = new Dictionary<int, string>
    {
        [0] = "ImageMetadataValueUnknown",
        [1] = "ImageMetadataValueAverage",
        [2] = "ImageMetadataValueCenterWeightedAverage",
        [3] = "ImageMetadataValueSpot",
        [4] = "ImageMetadataValueMultiSpot",
        [5] = "ImageMetadataValuePattern",
        [6] = "ImageMetadataValuePartial",
        [255] = "ImageMetadataValueOther",
    };
    private static readonly IReadOnlyDictionary<int, string> WhiteBalanceResourceKeys = new Dictionary<int, string>
    {
        [0] = "ImageMetadataValueAuto",
        [1] = "ImageMetadataValueManual",
    };
    private static readonly IReadOnlyDictionary<int, string> LightSourceResourceKeys = new Dictionary<int, string>
    {
        [0] = "ImageMetadataValueUnknown",
        [1] = "ImageMetadataValueDaylight",
        [2] = "ImageMetadataValueFluorescent",
        [3] = "ImageMetadataValueTungsten",
        [4] = "ImageMetadataValueFlash",
        [9] = "ImageMetadataValueFineWeather",
        [10] = "ImageMetadataValueCloudy",
        [11] = "ImageMetadataValueShade",
        [12] = "ImageMetadataValueDaylightFluorescent",
        [13] = "ImageMetadataValueDayWhiteFluorescent",
        [14] = "ImageMetadataValueCoolWhiteFluorescent",
        [15] = "ImageMetadataValueWhiteFluorescent",
        [17] = "ImageMetadataValueStandardLightA",
        [18] = "ImageMetadataValueStandardLightB",
        [19] = "ImageMetadataValueStandardLightC",
        [20] = "D55",
        [21] = "D65",
        [22] = "D75",
        [23] = "D50",
        [24] = "ImageMetadataValueIsoStudioTungsten",
        [255] = "ImageMetadataValueOther",
    };
    private static readonly IReadOnlyDictionary<int, string> ColorSpaceResourceKeys = new Dictionary<int, string>
    {
        [1] = "sRGB",
        [65535] = "ImageMetadataValueUncalibrated",
    };
    private static readonly IReadOnlyDictionary<int, string> NormalHardSoftResourceKeys = new Dictionary<int, string>
    {
        [0] = "ImageMetadataValueNormal",
        [1] = "ImageMetadataValueSoft",
        [2] = "ImageMetadataValueHard",
    };
    private static readonly IReadOnlyDictionary<int, string> GainControlResourceKeys = new Dictionary<int, string>
    {
        [0] = "ImageMetadataValueNone",
        [1] = "ImageMetadataValueLowGainUp",
        [2] = "ImageMetadataValueHighGainUp",
        [3] = "ImageMetadataValueLowGainDown",
        [4] = "ImageMetadataValueHighGainDown",
    };
    private static readonly IReadOnlyDictionary<string, string> ImageMetadataValueResourceKeys =
        new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase)
        {
            ["uncompressed"] = "ImageMetadataValueUncompressed",
            ["CCITT Group 3 1-D"] = "ImageMetadataValueCcittGroup3OneDimensional",
            ["Group 3 fax"] = "ImageMetadataValueGroup3Fax",
            ["Group 4 fax"] = "ImageMetadataValueGroup4Fax",
            ["LZW"] = "ImageMetadataValueLzw",
            ["JPEG"] = "ImageMetadataValueJpeg",
            ["Deflate"] = "ImageMetadataValueDeflate",
            ["PackBits"] = "ImageMetadataValuePackBits",
            ["grayscale"] = "ImageMetadataValueGrayscale",
            ["truecolor"] = "ImageMetadataValueTruecolor",
            ["indexed color"] = "ImageMetadataValueIndexedColor",
            ["grayscale with alpha"] = "ImageMetadataValueGrayscaleWithAlpha",
            ["truecolor with alpha"] = "ImageMetadataValueTruecolorWithAlpha",
            ["unknown"] = "ImageMetadataValueUnknown",
            ["none"] = "ImageMetadataValueNone",
            ["old JPEG"] = "ImageMetadataValueOldJpeg",
            ["white is zero"] = "ImageMetadataValueWhiteIsZero",
            ["black is zero"] = "ImageMetadataValueBlackIsZero",
            ["palette color"] = "ImageMetadataValuePaletteColor",
            ["transparency mask"] = "ImageMetadataValueTransparencyMask",
            ["separated"] = "ImageMetadataValueSeparated",
            ["sRGB"] = "ImageMetadataValueSrgb",
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
        TextSearchBox.PlaceholderText = UiStrings.TextSearchPlaceholder;
        AutomationProperties.SetName(TextSearchBox, UiStrings.TextSearchAccessibleName);
        AutomationProperties.SetName(TextSearchPreviousButton, UiStrings.TextSearchPreviousMatch);
        AutomationProperties.SetName(TextSearchNextButton, UiStrings.TextSearchNextMatch);
        AutomationProperties.SetName(TextSearchCloseButton, UiStrings.TextSearchClose);
        ToolTipService.SetToolTip(TextSearchPreviousButton, UiStrings.TextSearchPreviousMatch);
        ToolTipService.SetToolTip(TextSearchNextButton, UiStrings.TextSearchNextMatch);
        ToolTipService.SetToolTip(TextSearchCloseButton, UiStrings.TextSearchClose);
        ApplyTextSearchState(default);
        ListingFilterBox.PlaceholderText = UiStrings.ListingFilterPlaceholder;
        Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(ListingFilterBox, UiStrings.ListingFilterAccessibleName);
        _thumbnailScheduler = new NativeThumbnailScheduler(_native);
        _folderListingCache = new FolderListingCache(_native.TryPreviewFolderListing);
        _panelController = new PreviewPanelController(
            PreviewRoot,
            AnimatedImagePreviewRoot,
            PdfScrollViewer,
            PdfPagerBar,
            TextPreviewContainer,
            TablePreviewContainer,
            OfficeScrollViewer,
            MediaPreviewElement,
            ListingPanel,
            ErrorPanel,
            PreviewInfoRail,
            ImagePreviewToolbar,
            ImageFilmstrip,
            OfficePagesPanel);
        _windowController = new PreviewWindowController(this, () => WinRT.Interop.WindowNative.GetWindowHandle(this));
        _textPresenter = new TextPreviewPresenter(
            TextPreviewBlock,
            TextScrollViewer,
            TextListView,
            MarkdownListView,
            TextPreviewContainer,
            MarkdownOutlinePanel,
            MarkdownOutlineList,
            () => RootGrid.ActualTheme,
            () => (IsHighContrast, _uiSettings.GetColorValue(UIColorType.Background), _uiSettings.GetColorValue(UIColorType.Foreground)));
        _tablePresenter = new TablePreviewPresenter(
            TableScrollViewer,
            TableTitleText,
            TableSummaryText,
            TableGrid,
            TableSheetTabsScroller,
            TableSheetTabsPanel,
            () => RootGrid.ActualTheme,
            () => (IsHighContrast, _uiSettings.GetColorValue(UIColorType.Background), _uiSettings.GetColorValue(UIColorType.Foreground)));
        _officePresenter = new OfficePreviewPresenter(
            OfficeScrollViewer,
            OfficePagesPanel,
            () => (IsHighContrast, _uiSettings.GetColorValue(UIColorType.Background), _uiSettings.GetColorValue(UIColorType.Foreground)),
            LoadOfficeLayoutImageAsync);
        _rasterPresenter = new RasterPreviewPresenter(PreviewRoot, RasterFallbackImage, ImageZoomText);
        _imageWaveformPresenter = new ImageWaveformPresenter(ImageWaveformPanel, ImageWaveformImage);
        _animatedImagePresenter = new AnimatedImagePreviewPresenter(
            AnimatedImagePreviewRoot,
            AnimatedImagePreviewImage,
            ImageZoomText)
        {
            WaveformChanged = waveform => _imageWaveformPresenter.Show(waveform),
        };
        _imageSidecarController = new ImageSidecarController(
            ImageFilmstripList,
            ImageFilmstrip,
            DispatcherQueue,
            _folderListingCache.Get,
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
            ListingFilterBox,
            ListingListView,
            ListingNameHeader,
            ListingModifiedHeader,
            ListingTypeHeader,
            ListingSizeHeader,
            _folderListingCache.Get,
            () => _previewSession.Generation,
            () => CurrentPreviewToken,
            IsPreviewGenerationCurrent,
            PreviewListingItemAsync,
            LoadListingIconAsync);
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        _titleBarInsetController = new TitleBarInsetController(this, AppTitleBar);
        Title = UiStrings.AppName;
        TrySetBackdrop();
        try { _uiSettings.ColorValuesChanged += (_, _) => DispatcherQueue.TryEnqueue(ApplyAccessibilityVisuals); }
        catch (Exception ex) { DiagLog.Write("App", "UI color notifications unavailable: " + ex.Message); }
        try { _accessibilitySettings.HighContrastChanged += (_, _) => DispatcherQueue.TryEnqueue(ApplyAccessibilityVisuals); }
        catch (Exception ex) { DiagLog.Write("App", "high contrast notifications unavailable: " + ex.Message); }
        _previewKeyboardHook = new PreviewKeyboardHook(
            WinRT.Interop.WindowNative.GetWindowHandle(this),
            ShouldHandleSpaceAsPreviewClose,
            ClosePreviewFromKeyboard,
            OnWindowMouseWheel);
        PreviewRoot.SizeChanged += OnRootSizeChanged;
        AnimatedImagePreviewRoot.SizeChanged += OnAnimatedImageRootSizeChanged;
        PreviewContentHost.SizeChanged += OnPreviewContentHostSizeChanged;
        AnimatedImagePreviewRoot.PointerPressed += OnAnimatedImageRootPointerPressed;
        AnimatedImagePreviewRoot.PointerMoved += OnAnimatedImageRootPointerMoved;
        AnimatedImagePreviewRoot.PointerReleased += OnAnimatedImageRootPointerReleased;
        AnimatedImagePreviewRoot.PointerCanceled += OnAnimatedImageRootPointerCaptureLost;
        AnimatedImagePreviewRoot.PointerCaptureLost += OnAnimatedImageRootPointerCaptureLost;
        AnimatedImagePreviewRoot.DoubleTapped += OnAnimatedImageRootDoubleTapped;
        PreviewRoot.PointerPressed += OnPreviewRootPointerPressed;
        PreviewRoot.PointerMoved += OnPreviewRootPointerMoved;
        PreviewRoot.PointerReleased += OnPreviewRootPointerReleased;
        PreviewRoot.PointerCanceled += OnPreviewRootPointerCaptureLost;
        PreviewRoot.PointerCaptureLost += OnPreviewRootPointerCaptureLost;
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
        PreviewContentHost.AddHandler(
            UIElement.PointerWheelChangedEvent,
            new Microsoft.UI.Xaml.Input.PointerEventHandler(OnPreviewContentPointerWheelChanged),
            handledEventsToo: true);
        RootGrid.Loaded += (_, _) =>
        {
            if (RootGrid.XamlRoot is { } xamlRoot)
                xamlRoot.Changed += OnXamlRootChanged;
        };
        Closed += (_, _) =>
        {
            _lifetimeCts.Cancel();
            _uiWatchdog?.Dispose();
            _previewKeyboardHook?.Dispose();
            _native.Stop();
            RemoveTrayIcon();
            _supervisor?.Stop();
            _parserSupervisor?.Stop();
            _shellBroker?.Stop();
        };

        RootGrid.ActualThemeChanged += (s, e) =>
        {
            UpdateTitleBarColors();
            ApplyImageLetterboxBackgrounds();
            ApplyWindowIcon();
            RefreshTrayIcon();
        };
        UpdateTitleBarColors();
        ApplyImageLetterboxBackgrounds();
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
        ApplyWindowIcon();
        EnsureTrayIcon();
        _windowController.SetNoActivateStyle(enabled: false);

        try
        {
            _supervisor = new RasterHostSupervisor(ResolveHostExePath(), DispatcherQueue);
            _supervisor.SetBackgroundEfficiency(_backgroundEfficiencyEnabled ?? true);
            _supervisor.SurfaceReceived += OnSurfaceReceived;
            _supervisor.PageErrorReceived += OnPdfPageErrorReceived;
            _supervisor.ImageWaveformReceived += OnImageWaveformReceived;
            _native.Start(OnNativeIntent);
            AppStartupTiming.Mark("native-hook-ready");
            StatusText.Text = UiStrings.Ready;
            DiagLog.Write("App", "native hook installed; RasterHost is lazy");
            _ = RepairAutoStartAsync(_lifetimeCts.Token);
            _ = PrewarmPreviewHostsAsync(_lifetimeCts.Token);
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
            StatusText.Text = UiStrings.StartupFailedMessage;
            StatusBar.Visibility = Visibility.Visible;
            ShowPreviewWindow(activate: true);
        }
    }

    private static async Task RepairAutoStartAsync(CancellationToken cancellationToken)
    {
        try
        {
            await Task.Delay(2000, cancellationToken).ConfigureAwait(false);
            await AutoStart.RepairIfConfiguredAsync(cancellationToken).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "deferred autostart repair failed: " + ex.Message);
        }
    }

    private async Task PrewarmPreviewHostsAsync(CancellationToken cancellationToken)
    {
        try
        {
            await Task.Delay(1500, cancellationToken);
            await PrewarmHostAsync("RasterHost", 750, () => EnsureRasterHostStartedAsync(cancellationToken));
            await Task.Delay(500, cancellationToken);
            await PrewarmHostAsync("ParserHost", 500, () => EnsureParserHostStartedAsync(cancellationToken));
        }
        catch (OperationCanceledException)
        {
        }

        async Task PrewarmHostAsync(string hostName, int warningMilliseconds, Func<Task> start)
        {
            try
            {
                using (DiagLog.TraceScope("App", $"{hostName} idle prewarm", warningMilliseconds))
                    await start();
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                throw;
            }
            catch (Exception ex)
            {
                DiagLog.Write("App", $"{hostName} prewarm failed: {ex.Message}");
            }
        }
    }

    private void OnNativeIntent(NativeIntent intent)
    {
        long receivedAt = Stopwatch.GetTimestamp();
        DispatcherQueue.TryEnqueue(() =>
        {
            double queueDelayMs = Stopwatch.GetElapsedTime(receivedAt).TotalMilliseconds;
            DiagLog.Write("App", $"native intent={intent.Intent}; path={intent.PrimaryPath ?? "<none>"}; visible={_previewVisible}; uiQueue={queueDelayMs:0.0}ms");
            if (intent.Intent == PreviewIntent.Switch)
                DebounceSwitchIntent(intent, receivedAt);
            else
            {
                CancelSwitchDebounce();
                _ = HandleNativeIntentSafelyAsync(intent, receivedAt);
            }
        });
    }

    private void DebounceSwitchIntent(NativeIntent intent, long receivedAt)
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
                    _ = HandleNativeIntentSafelyAsync(intent, receivedAt);
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

    private async Task HandleNativeIntentSafelyAsync(NativeIntent intent, long receivedAt = 0)
    {
        try
        {
            await HandleNativeIntentAsync(intent, receivedAt);
        }
        catch (OperationCanceledException)
        {
            DiagLog.Write("App", "preview operation canceled");
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "intent handler FAILED: " + ex);
            DiagLog.Write("App", "intent error: " + ex.Message);
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
            if (archiveHandoff is not null)
            {
                if (_parserSupervisor is not null) await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
                else archiveHandoff.Dispose();
            }
            return;
        }
        if (requestId is null || string.Equals(_previewSession.CurrentRequestId, id, StringComparison.Ordinal))
            _previewSession.SetRequestId(null);
        if (!_requestHosts.Remove(id, out PreviewHostOwner owner))
        {
            DiagLog.Write("App", $"close skip: request has no host owner; request={id}");
            if (archiveHandoff is not null)
            {
                if (_parserSupervisor is not null) await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
                else archiveHandoff.Dispose();
            }
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
            if (archiveHandoff is not null)
            {
                if (_parserSupervisor is not null) await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
                else archiveHandoff.Dispose();
            }
        }
    }

    private async Task ClosePreviewImmediatelyAsync()
    {
        _previewTiming?.Complete("closed");
        CancelPreviewFrameCallbacks();
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
            _supervisor.PageErrorReceived += OnPdfPageErrorReceived;
            _supervisor.ImageWaveformReceived += OnImageWaveformReceived;
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

    private async Task HandleNativeIntentAsync(NativeIntent intent, long receivedAt)
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

        if (intent.Intent == PreviewIntent.Reload)
        {
            string? reloadPath = _previewSession.PendingPath ?? _previewSession.CurrentPath;
            if ((_previewVisible || _previewRevealPending) && !string.IsNullOrWhiteSpace(reloadPath))
                await PreviewPathAsync(reloadPath, _previewSession.Source, receivedAt: receivedAt);
            return;
        }

        if (intent.Intent == PreviewIntent.Fullscreen)
        {
            if (_previewVisible || _previewRevealPending)
                ToggleFullscreen();
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
            await PreviewPathAsync(path, source, receivedAt: receivedAt);
        }
    }

    private Task PreviewWindowPathAsync(string path, ArchiveEntryHandoff? archiveHandoff = null)
        => PreviewPathAsync(path, PreviewNavigationSource.WindowNavigation, archiveHandoff);

    private async Task PreviewPathAsync(
        string path,
        PreviewNavigationSource source,
        ArchiveEntryHandoff? archiveHandoff = null,
        long receivedAt = 0)
    {
        PreviewSessionSnapshot session = _previewSession.Begin(path, source);
        int generation = session.Generation;
        CancellationToken previewToken = session.Token;
        _previewTiming?.Complete("superseded");
        _previewTiming = new PreviewLifecycleTiming(generation, source, path, receivedAt);
        using var previewTrace = DiagLog.TraceScope("App", $"preview path source={source} gen={generation} path={path}", 250);
        BeginPreviewTransition();
        ResetPreview();
        bool archiveHandoffTransferred = false;
        _currentProbe = null;
        _currentPreviewWasCloudPlaceholder = false;
        Title = System.IO.Path.GetFileName(path);
        PreviewTitleText.Text = Title;
        StatusText.Text = UiStrings.Format(UiStrings.OpeningFileFormat, System.IO.Path.GetFileName(path));
        ShowPreviewLoadingShell();
        Microsoft.Win32.SafeHandles.SafeFileHandle? pinnedPreviewHandle = null;
        long pinnedPreviewLength = 0;
        try
        {
            Task closeTask = CloseCurrentAsync();
            Task<CloudFileAvailability> availabilityTask = Task.Run(
                () => CloudFileStatus.GetAvailability(path),
                previewToken);
            await Task.WhenAll(closeTask, availabilityTask);
            MarkPreviewPhase(generation, "availability-complete", $"availability={availabilityTask.Result}");
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            _currentArchiveEntryHandoff = archiveHandoff;
            archiveHandoffTransferred = archiveHandoff is not null;
            CloudFileAvailability availability = await availabilityTask;
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            if (availability != CloudFileAvailability.Local)
            {
                if (!await ConfirmCloudHydrationAsync(path, previewToken))
                {
                    if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                    FileProbe declinedProbe = FallbackFileProbe.CreateMetadataOnlyProbe(path);
                    var declined = CreateCloudMetadataPreview(
                        $"cloud-declined-{generation}",
                        path,
                        declinedProbe,
                        UiStrings.CloudDownloadDeclined);
                    _previewSession.CommitPath(path);
                    _previewSession.SetRequestId(null);
                    StatusText.Text = ShowTextPreview(declined);
                    RevealPreviewWindow(activate: false);
                    return;
                }
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                StatusText.Text = UiStrings.Format(UiStrings.DownloadingCloudFileFormat, System.IO.Path.GetFileName(path));
                RevealPreviewWindow(activate: false, finalContent: false);
                DiagLog.Write("App", $"cloud placeholder detected gen={generation}; path={path}");
                CloudHydrationResult hydration = await HydrateCloudFileAsync(
                    path, generation, previewToken);
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                if (hydration != CloudHydrationResult.Completed)
                {
                    FileProbe deferredProbe = FallbackFileProbe.CreateMetadataOnlyProbe(path);
                    var deferred = CreateCloudMetadataPreview(
                        $"cloud-deferred-{generation}",
                        path,
                        deferredProbe,
                        hydration == CloudHydrationResult.LimitExceeded
                            ? UiStrings.Format(
                                UiStrings.CloudDownloadTooLargeFormat,
                                FormatBytes(CloudHydrationPolicy.MaxDownloadBytes))
                            : UiStrings.CloudDownloadDeferred);
                    _previewSession.CommitPath(path);
                    _previewSession.SetRequestId(null);
                    StatusText.Text = ShowTextPreview(deferred);
                    RevealPreviewWindow(activate: false);
                    return;
                }
                availability = CloudFileAvailability.Local;
                DiagLog.Write("App", $"cloud hydration completed gen={generation}; path={path}");
            }
            bool mayRequireHydration = availability != CloudFileAvailability.Local;
            _currentPreviewWasCloudPlaceholder = mayRequireHydration;
            DiagLog.Write("App", $"preview probe begin gen={generation}");
            var preparedProbe = await Task.Run(
                () => PreparePreviewProbe(path, mayRequireHydration),
                previewToken);
            FileProbe probe = preparedProbe.Probe;
            pinnedPreviewHandle = preparedProbe.Handle;
            pinnedPreviewLength = preparedProbe.Length;
            DiagLog.Write(
                "App",
                $"preview probe end gen={generation}; authority={preparedProbe.Authority}; kind={probe.Kind}; ext={probe.Extension}; size={probe.Size}");
            MarkPreviewPhase(generation, "probe-complete", $"kind={probe.Kind}; ext={probe.Extension}; size={probe.Size}");
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            _currentProbe = probe;
            bool forceAnimatedFirstFrameRaster = PrefersReducedMotion || mayRequireHydration;
            PreviewRoute route = PreviewRoutePlanner.Plan(
                probe.Kind,
                mayRequireHydration,
                forceAnimatedFirstFrameRaster);

            if (route == PreviewRoute.CloudMetadata
                && probe.Kind.Equals("unknown", StringComparison.OrdinalIgnoreCase))
            {
                var unknownCloudReady = CreateCloudMetadataPreview(
                    $"cloud-unknown-{generation}",
                    path,
                    probe,
                    availability == CloudFileAvailability.RequiresHydration
                        ? UiStrings.CloudUnknownDeferred
                        : UiStrings.CloudAvailabilityUnknownDeferred);
                _previewSession.CommitPath(path);
                _previewSession.SetRequestId(null);
                StatusText.Text = ShowTextPreview(unknownCloudReady);
                RevealPreviewWindow(activate: false);
                return;
            }

            if (route == PreviewRoute.CloudMetadata
                && !MediaPreviewPresenter.IsMediaProbe(probe))
            {
                var deferred = CreateCloudMetadataPreview(
                    $"cloud-deferred-{generation}",
                    path,
                    probe,
                    UiStrings.CloudAvailabilityUnknownDeferred);
                _previewSession.CommitPath(path);
                _previewSession.SetRequestId(null);
                StatusText.Text = ShowTextPreview(deferred);
                RevealPreviewWindow(activate: false);
                return;
            }

            if (route is PreviewRoute.Media or PreviewRoute.CloudMetadata
                && MediaPreviewPresenter.IsMediaProbe(probe))
            {
                MarkPreviewPhase(generation, "route-selected", "route=media");
                if (mayRequireHydration)
                {
                    var cloudMediaReady = CreateCloudMetadataPreview(
                    $"cloud-media-{generation}",
                    path,
                    probe,
                    availability == CloudFileAvailability.RequiresHydration
                        ? UiStrings.CloudMediaDeferred
                        : UiStrings.CloudMediaAvailabilityUnknownDeferred);
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

            AnimatedImageRenderPlan? animatedPlan = route == PreviewRoute.RasterHost
                ? null
                : AnimatedImagePreviewPresenter.CreateRenderPlan(probe);
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            if (animatedPlan is not null)
            {
                MarkPreviewPhase(generation, "route-selected", "route=animation; mode=native-frames");
                DiagLog.Write("App", $"preview animated image candidate detected by Rust probe gen={generation}");
                forceAnimatedFirstFrameRaster = true;
                route = PreviewRoutePlanner.Plan(probe.Kind, mayRequireHydration, forceRaster: true);
            }


            PreviewReady? nativeReady = null;
            if (route == PreviewRoute.ParserHost)
            {
                MarkPreviewPhase(generation, "route-selected", "route=parser-host");
                await EnsureParserHostStartedAsync(previewToken);
                if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
                string parserRequestId;
                Task<ControlMessage> parserCompletion;
                if (mayRequireHydration)
                {
                    (parserRequestId, parserCompletion) = _parserSupervisor!.BeginOpen(
                        path,
                        probe,
                        CloudPreviewTimeout,
                        recycleHostOnCancel: true);
                }
                else if (pinnedPreviewHandle is not null)
                {
                    Microsoft.Win32.SafeHandles.SafeFileHandle parserSource = pinnedPreviewHandle;
                    pinnedPreviewHandle = null;
                    (parserRequestId, parserCompletion) = BeginPinnedParserOpen(
                        path,
                        probe,
                        parserSource,
                        pinnedPreviewLength);
                }
                else
                {
                    (parserRequestId, parserCompletion) =
                        _parserSupervisor!.BeginOpen(path, probe);
                }
                _requestHosts[parserRequestId] = PreviewHostOwner.Parser;
                _previewSession.SetRequestId(parserRequestId);
                _previewSession.CommitPath(path);
                ControlMessage parserResult = await parserCompletion.WaitAsync(previewToken);
                if (!IsPreviewGenerationCurrent(generation, previewToken) || !_previewSession.IsCurrentRequest(parserRequestId)) return;
                if (parserResult is PreviewError parserError)
                {
                    TryShowHostError(session, parserError);
                    return;
                }
                nativeReady = parserResult as PreviewReady;
            }
            else if (route == PreviewRoute.NativeThenRaster)
            {
                MarkPreviewPhase(generation, "route-selected", "route=native");
                nativeReady = await Task.Run(() => _native.TryPreview($"native-{generation}", path, probe, previewToken), previewToken);
                if (nativeReady is null && probe.Kind.Equals("text", StringComparison.OrdinalIgnoreCase))
                    nativeReady = await Task.Run(
                        () => FallbackFileProbe.TryCreateTextPreview($"managed-{generation}", path, previewToken),
                        previewToken);
                if (nativeReady is null && probe.Kind.Equals("database", StringComparison.OrdinalIgnoreCase))
                {
                    nativeReady = new PreviewReady(
                        $"database-{generation}",
                        "database",
                        System.IO.Path.GetFileName(path),
                        720,
                        500)
                    {
                        TextContent = UiStrings.DatabasePreviewUnavailable,
                        TextFormat = "plain",
                        TextLanguage = "text",
                    };
                }
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
                    _ => UiStrings.BuildPreviewStatus(nativeReady.Kind, nativeReady.Title),
                };
                RevealPreviewWindow(ShouldActivatePreview(nativeReady));
                return;
            }

            await EnsureRasterHostStartedAsync(previewToken);
            MarkPreviewPhase(generation, "route-selected", "route=raster-host");
            if (!IsPreviewGenerationCurrent(generation, previewToken)) return;
            var targetSize = GetRasterDecodeTargetSize();
            bool prepareAnimation = animatedPlan is not null;
            string requestId;
            Task<ControlMessage> completion;
            if (mayRequireHydration)
            {
                (requestId, completion) = _supervisor!.BeginOpen(
                    path,
                    probe,
                    targetSize.Width,
                    targetSize.Height,
                    CloudPreviewTimeout,
                    recycleHostOnCancel: true,
                    prepareAnimation: prepareAnimation);
            }
            else if (pinnedPreviewHandle is null)
            {
                (requestId, completion) = _supervisor!.BeginOpen(
                    path,
                    probe,
                    targetSize.Width,
                    targetSize.Height,
                    prepareAnimation: prepareAnimation);
            }
            else
            {
                Microsoft.Win32.SafeHandles.SafeFileHandle rasterSource = pinnedPreviewHandle;
                pinnedPreviewHandle = null;
                var pinnedRequest = BeginPinnedRasterOpen(
                    path,
                    probe,
                    rasterSource,
                    pinnedPreviewLength,
                    targetSize.Width,
                    targetSize.Height,
                    prepareAnimation);
                requestId = pinnedRequest.RequestId;
                completion = pinnedRequest.Completion;
            }
            _requestHosts[requestId] = PreviewHostOwner.Raster;
            _previewSession.SetRequestId(requestId);
            _previewSession.CommitPath(path);
            DiagLog.Write("App", $"preview host open sent gen={generation}; request={requestId}");
            ControlMessage result = await completion.WaitAsync(previewToken);
            DiagLog.Write("App", $"preview host result gen={generation}; request={requestId}; type={result.GetType().Name}");
            if (!IsPreviewGenerationCurrent(generation, previewToken) || !_previewSession.IsCurrentRequest(requestId))
                return;
            if (result is PreviewError
                && mayRequireHydration
                && probe.Kind.Equals("image", StringComparison.OrdinalIgnoreCase))
            {
                try
                {
                    _shellBroker ??= new ShellBrokerSupervisor(ResolveShellBrokerExePath());
                    await _shellBroker.EnsureStartedAsync(previewToken);
                    NativeRasterImage? shellRaster = await _shellBroker.GetThumbnailAsync(path, 512, previewToken);
                    if (shellRaster is not null)
                    {
                        var shellReady = new PreviewReady(
                            requestId,
                            "thumbnail",
                            System.IO.Path.GetFileName(path),
                            shellRaster.Width,
                            shellRaster.Height);
                        StatusText.Text = ShowRasterPreview(shellReady);
                        if (!_rasterPresenter!.AttachBitmap(shellRaster))
                            throw new InvalidDataException("ShellBroker returned an invalid raster packet.");
                        if (!Path.GetExtension(path).Equals(".gif", StringComparison.OrdinalIgnoreCase))
                        {
                            ImageWaveform waveform = await Task.Run(
                                () => ImageWaveformBuilder.Create(shellRaster.Bgra, shellRaster.Width, shellRaster.Height),
                                previewToken);
                            if (!IsPreviewGenerationCurrent(generation, previewToken)
                                || !_previewSession.IsCurrentRequest(requestId))
                                return;
                            _imageWaveformPresenter!.Show(waveform);
                        }
                        else
                        {
                            _imageWaveformPresenter!.Clear();
                        }
                        RevealPreviewWindow(ShouldActivatePreview(shellReady));
                        return;
                    }
                }
                catch (OperationCanceledException) { throw; }
                catch (Exception ex) { DiagLog.Write("App", "ShellBroker fallback failed: " + ex); }
            }
            if (result is PreviewError previewError)
            {
                TryShowHostError(session, previewError);
                return;
            }
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
                _ => "?",
            };
            RevealPreviewWindow(result is PreviewReady ready && ShouldActivatePreview(ready));
            if (result is PreviewReady
                && animatedPlan is not null)
            {
                _ = TryUpgradeRasterToNativeAnimationAsync(
                    path,
                    generation,
                    previewToken,
                    requestId);
            }
        }
        catch (TimeoutException ex)
        {
            DiagLog.Write("App", "preview timed out: " + ex.Message);
            TryShowErrorPreview(session, new PreviewFailure(PreviewFailureKind.TimedOut, true));
            CompletePreviewTiming(generation, "timed-out");
        }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException or InvalidOperationException)
        {
            DiagLog.Write("App", "preview service failed: " + ex);
            if (IsPreviewGenerationCurrent(generation, previewToken))
            {
                TryShowErrorPreview(session, new PreviewFailure(PreviewFailureKind.Service, true));
                CompletePreviewTiming(generation, "failed");
            }
        }
        catch (OperationCanceledException)
        {
            DiagLog.Write("App", $"preview canceled: path={path}");
            CompletePreviewTiming(generation, "canceled");
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "preview failed: " + ex);
            TryShowErrorPreview(session, new PreviewFailure(PreviewFailureKind.Content, false));
            CompletePreviewTiming(generation, "failed");
        }
        finally
        {
            pinnedPreviewHandle?.Dispose();
            if (archiveHandoff is not null && !archiveHandoffTransferred)
            {
                if (_parserSupervisor is not null) await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
                else archiveHandoff.Dispose();
            }
        }
    }

    private CancellationToken CurrentPreviewToken => _previewSession.Token;

    private bool IsPreviewGenerationCurrent(int generation) => IsPreviewGenerationCurrent(generation, CurrentPreviewToken);

    private bool IsPreviewGenerationCurrent(int generation, CancellationToken cancellationToken)
        => _previewSession.IsCurrent(generation, cancellationToken);

    private async Task TryUpgradeRasterToNativeAnimationAsync(
        string path,
        int generation,
        CancellationToken cancellationToken,
        string rasterRequestId)
    {
        NativeAnimationFrames? frames = null;
        bool framesOwnershipTransferred = false;
        long handoffStarted = Stopwatch.GetTimestamp();
        try
        {
            var targetSize = GetRasterDecodeTargetSize();
            frames = await _supervisor!.ExtractAnimationFramesAsync(
                rasterRequestId, targetSize.Width, targetSize.Height, cancellationToken);

            if (frames is null
                || frames.FrameCount <= 1
                || !IsPreviewGenerationCurrent(generation, cancellationToken)
                || !_previewSession.IsCurrentPath(path)
                || !_previewSession.IsCurrentRequest(rasterRequestId))
            {
                return;
            }

            var ready = new PreviewReady(
                $"animated-native-{generation}",
                "image",
                System.IO.Path.GetFileName(path),
                frames.Width,
                frames.Height);
            _previewSession.SetRequestId(null);
            // ShowNativeAnimatedImagePreview consumes the mapping on every path: the
            // presenter keeps it when rendering succeeds and disposes it before adoption.
            framesOwnershipTransferred = true;
            long initialElapsedMilliseconds = Math.Max(
                0,
                (long)Stopwatch.GetElapsedTime(handoffStarted).TotalMilliseconds);
            StatusText.Text = ShowNativeAnimatedImagePreview(
                ready,
                path,
                frames,
                scheduleSidecars: false,
                initialElapsedMilliseconds: initialElapsedMilliseconds);
            await CloseCurrentAsync(rasterRequestId);
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", $"animated image upgrade failed gen={generation}; {ex}");
        }
        finally
        {
            if (!framesOwnershipTransferred)
                frames?.Dispose();
        }
    }

    private void BeginPreviewTransition()
    {
        CancelPreviewFrameCallbacks();
        DiagLog.Write("App", $"preview transition begin; visible={_previewVisible}; request={_previewSession.CurrentRequestId}");
        _native.SetPreviewVisible(true);
        _previewRevealPending = true;
        PreviewContentHost.Opacity = 0;
        PreviewContentHost.IsHitTestVisible = false;
        ErrorPanel.Visibility = Visibility.Collapsed;
        LoadingRing.Visibility = Visibility.Visible;
        LoadingRing.IsActive = true;
    }

    private void ShowPreviewLoadingShell()
    {
        if (_previewVisible && !_previewTemporarilyHidden)
        {
            MarkPreviewPhase(_previewSession.Generation, "loading-indicator-visible");
            AnnouncePreviewLifecycle(
                LoadingRing,
                StatusText.Text,
                AutomationNotificationKind.Other,
                AutomationNotificationProcessing.MostRecent);
            return;
        }

        using var trace = DiagLog.TraceScope("App", "preview loading shell show", 50);
        _loadingShellShowStarted = Stopwatch.GetTimestamp();
        CompositionTarget.Rendering -= OnLoadingShellFirstFrame;
        CompositionTarget.Rendering += OnLoadingShellFirstFrame;
        ShowPreviewWindow(activate: false, resizeToDefault: true);
        AnnouncePreviewLifecycle(
            LoadingRing,
            StatusText.Text,
            AutomationNotificationKind.Other,
            AutomationNotificationProcessing.MostRecent);
        MarkPreviewPhase(_previewSession.Generation, "loading-shell-show-requested");
        _previewTemporarilyHidden = false;
    }

    private void OnLoadingShellFirstFrame(object? sender, object e)
    {
        CompositionTarget.Rendering -= OnLoadingShellFirstFrame;
        long started = _loadingShellShowStarted;
        _loadingShellShowStarted = 0;
        if (started != 0)
            DiagLog.Write("App", $"loading shell first frame {Stopwatch.GetElapsedTime(started).TotalMilliseconds:0.0}ms");
        MarkPreviewPhase(_previewSession.Generation, "loading-shell-first-frame");
    }

    private void RevealPreviewWindow(bool activate, bool finalContent = true)
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
        if (finalContent)
        {
            bool isError = ErrorPanel.Visibility == Visibility.Visible;
            AnnouncePreviewLifecycle(
                isError ? ErrorText : PreviewContentHost,
                isError ? ErrorText.Text : UiStrings.PreviewReadyAnnouncement,
                isError ? AutomationNotificationKind.ActionAborted : AutomationNotificationKind.ActionCompleted,
                isError ? AutomationNotificationProcessing.ImportantMostRecent : AutomationNotificationProcessing.MostRecent);
        }
        else
        {
            AnnouncePreviewLifecycle(
                PreviewContentHost,
                StatusText.Text,
                AutomationNotificationKind.Other,
                AutomationNotificationProcessing.MostRecent);
        }
        MarkPreviewPhase(_previewSession.Generation, finalContent ? "reveal-called" : "placeholder-reveal");
        if (finalContent)
        {
            CompositionTarget.Rendering -= OnPreviewFinalFirstFrame;
            CompositionTarget.Rendering += OnPreviewFinalFirstFrame;
        }
        _lastPreviewRevealTick = Environment.TickCount64;
        _lastPreviewRevealPath = _previewSession.CurrentPath;
    }

    private void OnPreviewFinalFirstFrame(object? sender, object e)
    {
        CompositionTarget.Rendering -= OnPreviewFinalFirstFrame;
        PreviewLifecycleTiming? timing = _previewTiming;
        if (timing is null || timing.Generation != _previewSession.Generation || timing.IsTerminal)
            return;
        timing.Mark("final-first-frame");
        timing.Complete("revealed");
    }

    private void MarkPreviewPhase(int generation, string phase, string? detail = null)
    {
        if (_previewTiming is { } timing && timing.Generation == generation)
            timing.Mark(phase, detail);
    }

    private void CompletePreviewTiming(int generation, string outcome)
    {
        if (_previewTiming is { } timing && timing.Generation == generation)
            timing.Complete(outcome);
    }

    private void AnnouncePreviewLifecycle(
        FrameworkElement element,
        string message,
        AutomationNotificationKind kind,
        AutomationNotificationProcessing processing)
    {
        if (string.IsNullOrWhiteSpace(message))
            return;
        int generation = _previewSession.Generation;
        DispatcherQueue.TryEnqueue(() =>
        {
            if (_previewSession.Generation != generation)
                return;
            AutomationPeer? peer = FrameworkElementAutomationPeer.FromElement(element)
                ?? FrameworkElementAutomationPeer.CreatePeerForElement(element);
            peer?.RaiseNotificationEvent(
                kind,
                processing,
                message,
                "preview.lifecycle");
        });
    }

    private void CancelPreviewFrameCallbacks()
    {
        CompositionTarget.Rendering -= OnLoadingShellFirstFrame;
        CompositionTarget.Rendering -= OnPreviewFinalFirstFrame;
        _loadingShellShowStarted = 0;
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
        PreviewKindPillText.Text = LocalizePreviewKind(ready.Kind);
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

    private void OnXamlRootChanged(Microsoft.UI.Xaml.XamlRoot sender, Microsoft.UI.Xaml.XamlRootChangedEventArgs args)
    {
        _rasterPresenter?.UpdateLayout();
        _animatedImagePresenter?.ScheduleLayoutUpdate();
        if (_isRasterChromeEnabled)
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
            parts.Add(UiStrings.Format(UiStrings.PreviewModifiedMetadataFormat, modified));
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
        if (ready.Kind == "database")
            return UiStrings.EmptyValue;
        if (ready.Kind == "pdf" && ready.PageCount > 0)
            return FormatPageCount(ready.PageCount);
        if (ready.PreferredWidth > 0 && ready.PreferredHeight > 0)
            return $"{ready.PreferredWidth:N0} x {ready.PreferredHeight:N0}";
        if (ready.OfficeLayout is { Pages.Length: > 0 } layout)
            return FormatPageCount(layout.Pages.Length);
        if (ready.Listing is { } listing)
            return ListingPreviewPresenter.BuildRootSummary(listing);
        if (ready.Table is { } table)
            return UiStrings.Format(UiStrings.TableDimensionsFormat, table.TotalRows, table.TotalColumns);
        return UiStrings.EmptyValue;
    }

    private static string FormatPageCount(int pageCount)
        => UiStrings.Format(pageCount == 1 ? UiStrings.PageCountSingularFormat : UiStrings.PageCountFormat, pageCount);

    private static string PreviewTypeTextFor(PreviewReady ready, string? path)
    {
        string ext = string.IsNullOrWhiteSpace(path)
            ? ""
            : System.IO.Path.GetExtension(path).TrimStart('.').ToUpperInvariant();
        string kind = LocalizePreviewKind(ready.Kind);
        return string.IsNullOrEmpty(ext) ? kind : $"{ext} {kind}";
    }

    private static string LocalizePreviewKind(string? kind)
        => UiStrings.LocalizePreviewKind(kind);

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
            TextContent = UiStrings.Format(
                UiStrings.CloudMetadataPreviewFormat,
                fileName,
                LocalizePreviewKind(probe.Kind),
                FormatBytes(probe.Size),
                modified,
                status),
            TextFormat = "plain",
            TextLanguage = "text",
        };
    }

    private async Task<bool> ConfirmCloudHydrationAsync(
        string path,
        CancellationToken cancellationToken)
    {
        await _modalDialogGate.WaitAsync(cancellationToken);
        try
        {
            var dialog = new ContentDialog
            {
                Title = UiStrings.CloudDownloadConsentTitle,
                Content = UiStrings.Format(
                    UiStrings.CloudDownloadConsentMessageFormat,
                    Path.GetFileName(path),
                    UiStrings.UnknownFileSize,
                    FormatBytes(CloudHydrationPolicy.MaxDownloadBytes)),
                PrimaryButtonText = UiStrings.DownloadForPreview,
                CloseButtonText = UiStrings.Cancel,
                DefaultButton = ContentDialogButton.Close,
                XamlRoot = RootGrid.XamlRoot,
            };
            _isModalDialogOpen = true;
            try
            {
                cancellationToken.ThrowIfCancellationRequested();
                var showOperation = dialog.ShowAsync();
                using CancellationTokenRegistration registration = cancellationToken.Register(
                    () => DispatcherQueue.TryEnqueue(dialog.Hide));
                ContentDialogResult result = await showOperation;
                return !cancellationToken.IsCancellationRequested && result == ContentDialogResult.Primary;
            }
            finally
            {
                _isModalDialogOpen = false;
            }
        }
        finally
        {
            _modalDialogGate.Release();
        }
    }

    private async Task<CloudHydrationResult> HydrateCloudFileAsync(
        string path,
        int generation,
        CancellationToken cancellationToken)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeout.CancelAfter(CloudPreviewTimeout);
        int progressActive = 1;
        IProgress<(long Downloaded, long Length)> progress = new Progress<(long Downloaded, long Length)>(value =>
        {
            if (Volatile.Read(ref progressActive) == 0
                || !IsPreviewGenerationCurrent(generation, cancellationToken))
                return;
            StatusText.Text = value.Length > 0
                ? UiStrings.Format(
                    UiStrings.DownloadingCloudFileProgressFormat,
                    Path.GetFileName(path),
                    CloudHydrationPolicy.ProgressPercent(value.Downloaded, value.Length),
                    FormatBytes(value.Downloaded),
                    FormatBytes(value.Length))
                : UiStrings.Format(
                    UiStrings.DownloadingCloudFileBytesFormat,
                    Path.GetFileName(path),
                    FormatBytes(value.Downloaded));
        });
        try
        {
            return await Task.Run(async () =>
            {
                StorageFile file = await StorageFile.GetFileFromPathAsync(path).AsTask(timeout.Token);
                BasicProperties properties = await file.GetBasicPropertiesAsync().AsTask(timeout.Token);
                long declaredLength = properties.Size > long.MaxValue ? -1 : (long)properties.Size;
                if (!CloudHydrationPolicy.IsDeclaredLengthAllowed(declaredLength))
                    return CloudHydrationResult.LimitExceeded;
                using IRandomAccessStreamWithContentType randomAccess =
                    await file.OpenReadAsync().AsTask(timeout.Token);
                using Stream stream = randomAccess.AsStreamForRead(bufferSize: 1);
                byte[] buffer = new byte[64 * 1024];
                long downloaded = 0;
                long lastProgress = 0;
                while (true)
                {
                    int nextRead = CloudHydrationPolicy.NextReadSize(downloaded, buffer.Length);
                    if (nextRead == 0)
                        return CloudHydrationResult.LimitExceeded;
                    int read = await stream.ReadAsync(buffer.AsMemory(0, nextRead), timeout.Token);
                    if (read == 0)
                        break;
                    downloaded += read;
                    if (downloaded > CloudHydrationPolicy.MaxDownloadBytes)
                        return CloudHydrationResult.LimitExceeded;
                    long now = Stopwatch.GetTimestamp();
                    if (lastProgress == 0 || Stopwatch.GetElapsedTime(lastProgress, now) >= TimeSpan.FromMilliseconds(250))
                    {
                        lastProgress = now;
                        progress.Report((downloaded, declaredLength));
                    }
                }
                return CloudHydrationResult.Completed;
            }, timeout.Token);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested)
        {
            return CloudHydrationResult.Deferred;
        }
        catch (Exception ex) when (ex is IOException
                                   or UnauthorizedAccessException
                                   or NotSupportedException
                                   or ArgumentException
                                   or System.Runtime.InteropServices.COMException)
        {
            DiagLog.Write("App", "cloud hydration failed: " + ex.Message);
            return CloudHydrationResult.Deferred;
        }
        finally
        {
            Interlocked.Exchange(ref progressActive, 0);
        }
    }

    private static bool ProbeMatchesPath(FileProbe? probe, string? path)
        => probe is not null
           && !string.IsNullOrWhiteSpace(path)
           && string.Equals(probe.Path, path, StringComparison.OrdinalIgnoreCase);

    private bool TryShowHostError(PreviewSessionSnapshot session, PreviewError error)
    {
        string? normalizedFormat = ImageCodecPolicy.NormalizeFormat('.' + error.Format);
        string knownCode = error.Code is PreviewErrorCodes.ImageCodecRequired or PreviewErrorCodes.ImageDecodeFailed
            ? error.Code
            : "unknown";
        DiagLog.Write("App", $"host preview error: code={knownCode}; format={normalizedFormat ?? "unknown"}");
        if (error.Code == PreviewErrorCodes.ImageCodecRequired
            && normalizedFormat is string format)
        {
            return TryShowErrorPreview(
                session,
                UiStrings.ImageCodecRequiredTitle,
                UiStrings.Format(UiStrings.ImageCodecRequiredMessageFormat, ImageFormatDisplayName(format)));
        }
        if (error.Code == PreviewErrorCodes.ImageDecodeFailed)
            return TryShowErrorPreview(session, UiStrings.ImageDecodeFailedTitle, UiStrings.ImageDecodeFailedMessage);
        return TryShowErrorPreview(session, new PreviewFailure(PreviewFailureKind.Content, false));
    }

    private static string ImageFormatDisplayName(string format)
        => format switch
        {
            "avif" => "AVIF",
            "heic" => "HEIC/HEIF",
            "jxl" => "JPEG XL",
            _ => format.ToUpperInvariant(),
        };

    private bool TryShowErrorPreview(PreviewSessionSnapshot session, PreviewFailure failure)
    {
        (string title, string message) = failure.Kind switch
        {
            PreviewFailureKind.TimedOut => (UiStrings.PreviewTimedOutTitle, UiStrings.PreviewTimedOutMessage),
            PreviewFailureKind.Service => (UiStrings.PreviewServiceUnavailableTitle, UiStrings.PreviewServiceUnavailableMessage),
            PreviewFailureKind.Surface => (UiStrings.PreviewDisplayFailedTitle, UiStrings.PreviewDisplayFailedMessage),
            _ => (UiStrings.PreviewContentFailedTitle, UiStrings.PreviewContentFailedMessage),
        };
        return TryShowErrorPreview(session, title, message, failure.CanRetry);
    }

    private bool TryShowErrorPreview(
        PreviewSessionSnapshot session,
        string title,
        string message,
        bool canRetry = false)
    {
        if (!_previewSession.TryBindError(session, canRetry, out PreviewErrorContext context))
            return false;

        _panelController.ShowError();
        ErrorText.Text = message;
        PreviewTitleText.Text = title;
        PreviewMetaText.Text = ErrorText.Text;
        PreviewKindPillText.Text = UiStrings.ErrorKind;
        ErrorActionsPanel.Visibility = Visibility.Visible;
        ErrorRetryButton.Visibility = canRetry ? Visibility.Visible : Visibility.Collapsed;
        ResizeWindowForContent(520, 300, MaxTextWindowWidth, MaxTextWindowHeight);
        StatusText.Text = UiStrings.Format(UiStrings.Get("PreviewErrorStatusFormat"), ErrorText.Text);
        RevealPreviewWindow(activate: false);
        DispatcherQueue.TryEnqueue(() =>
        {
            if (!_previewSession.IsCurrentError(context))
                return;
            if (ErrorRetryButton.Visibility == Visibility.Visible)
                ErrorRetryButton.Focus(FocusState.Programmatic);
            else if (ErrorActionsPanel.Visibility == Visibility.Visible)
                ErrorOpenFileButton.Focus(FocusState.Programmatic);
        });
        return true;
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
        animation.Duration = TimeSpan.FromMilliseconds(110);
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
                    DiagLog.Write("App", "pdf page surface attach failed: " + pdfError);
                    StatusText.Text = UiStrings.PdfPageFailed;
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
            var layoutWatch = Stopwatch.StartNew();
            _rasterPresenter.UpdateLayout();
            _imageWaveformPresenter?.Show(IsCurrentGifPreview() ? null : surface.Waveform);
            layoutWatch.Stop();
            DiagLog.Write("App", $"image presenter apply {layoutWatch.ElapsedMilliseconds}ms; size={surface.Width}x{surface.Height}");
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

    private void OnImageWaveformReceived(PreviewImageWaveform message)
    {
        if (_previewSession.IsCurrentRequest(message.RequestId) && !IsCurrentGifPreview())
            _imageWaveformPresenter?.Show(message.Waveform);
    }

    private bool IsCurrentGifPreview()
        => string.Equals(
            Path.GetExtension(_previewSession.CurrentPath),
            ".gif",
            StringComparison.OrdinalIgnoreCase);

    private void OnPdfPageErrorReceived(PreviewPageError error)
    {
        if (!_previewSession.IsCurrentRequest(error.RequestId)
            || _pdfPresenter?.HandlePageError(error) != true)
            return;

        StatusText.Text = error.TimedOut
            ? UiStrings.Format(UiStrings.PdfPageTimedOutStatusFormat, error.PageIndex + 1)
            : UiStrings.Format(UiStrings.PdfPageFailedStatusFormat, error.PageIndex + 1);
        AnnouncePreviewLifecycle(
            PreviewContentHost,
            StatusText.Text,
            AutomationNotificationKind.ActionAborted,
            AutomationNotificationProcessing.ImportantMostRecent);
    }

    private void ShowSurfaceFailure(string requestId, string message)
    {
        string? path = _previewSession.ActivePath;
        if (!_previewSession.IsCurrentRequest(requestId)
            || string.IsNullOrWhiteSpace(path)
            || !_previewSession.TryGetActiveSnapshot(path, out PreviewSessionSnapshot session))
            return;

        DiagLog.Write("App", "surface preview failed: " + message);
        _previewSession.SetRequestId(null);
        _ = CloseCurrentAsync(requestId);
        TryShowErrorPreview(session, new PreviewFailure(PreviewFailureKind.Surface, false));
    }

    private void OnMediaPreviewFailed(string path)
    {
        if (!_previewSession.TryGetActiveSnapshot(path, out PreviewSessionSnapshot session))
            return;

        TryShowErrorPreview(session, new PreviewFailure(PreviewFailureKind.Content, false));
    }

    private void OnRootSizeChanged(object sender, SizeChangedEventArgs e)
    {
        _rasterPresenter?.UpdateLayout();
    }

    private void OnAnimatedImageRootSizeChanged(object sender, SizeChangedEventArgs e)
    {
        _animatedImagePresenter?.ScheduleLayoutUpdate();
    }

    private void ApplyImageLetterboxBackgrounds()
    {
        Brush? background = null;
        try
        {
            string themeKey = IsHighContrast
                ? "HighContrast"
                : RootGrid.ActualTheme == ElementTheme.Light ? "Light" : "Dark";
            if (RootGrid.Resources.ThemeDictionaries[themeKey] is ResourceDictionary themeResources)
            {
                string brushKey = PrefersReducedTransparency ? "PreviewSurfaceBrush" : "PreviewHeroSurfaceBrush";
                background = themeResources[brushKey] as Brush;
            }
        }
        catch { }

        background ??= new SolidColorBrush(PrefersReducedTransparency
            ? RootGrid.ActualTheme == ElementTheme.Light ? Microsoft.UI.Colors.White : Microsoft.UI.ColorHelper.FromArgb(255, 31, 31, 31)
            : Microsoft.UI.Colors.Transparent);
        PreviewRoot.Background = background;
        AnimatedImagePreviewRoot.Background = background;
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
        ResizeWindowForContent(Math.Max(result.Width, MinRasterChromeContentWidth), result.Height, MaxImageWindowWidth, MaxImageWindowHeight);
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

    private string ShowNativeAnimatedImagePreview(
        PreviewReady ready,
        string path,
        NativeAnimationFrames frames,
        bool scheduleSidecars = true,
        long initialElapsedMilliseconds = 0)
    {
        bool presenterOwnsFrames = false;
        try
        {
            UpdatePreviewChrome(ready, showRasterTools: true);
            AnimatedImagePreviewResult result = _animatedImagePresenter!.RenderNativeFrames(
                path,
                ready,
                frames,
                GetMaxContentSize(MaxImageWindowWidth, MaxImageWindowHeight),
                enableWaveform: !string.Equals(
                    Path.GetExtension(path),
                    ".gif",
                    StringComparison.OrdinalIgnoreCase),
                initialElapsedMilliseconds: initialElapsedMilliseconds);
            presenterOwnsFrames = true;
            // Populate the first animation bitmap while the static surface remains visible.
            // Swapping panels only after Invalidate avoids a blank upgrade frame.
            _panelController.ShowAnimatedImage();
            _rasterPresenter?.Clear();
            _imageWaveformPresenter?.Clear();
            UpdateImageAnimationPlaybackButton();
            ResizeWindowForContent(Math.Max(result.Width, MinRasterChromeContentWidth), result.Height, MaxImageWindowWidth, MaxImageWindowHeight);
            if (scheduleSidecars)
                ScheduleImageSidecarLoads(ready);
            return result.Status;
        }
        finally
        {
            if (!presenterOwnsFrames)
                frames.Dispose();
        }
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

        bool wrap = TextWrappingPolicy.ShouldWrap(AppSettings.Current.TextWrapping, ready.TextFormat, ready.Markdown is not null);
        TextPreviewResult result = _textPresenter!.Render(
            ready,
            GetMaxContentSize(MaxTextWindowWidth, MaxTextWindowHeight),
            wrap,
            AppSettings.Current.TextSize,
            AppSettings.Current.TextLineNumbers);
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

    private async Task<NativeRasterImage?> LoadOfficeLayoutImageAsync(
        string parentPreviewRequestId,
        string imageRef,
        int targetWidth,
        int targetHeight,
        CancellationToken cancellationToken)
    {
        if (string.IsNullOrWhiteSpace(parentPreviewRequestId)
            || string.IsNullOrWhiteSpace(imageRef)
            || targetWidth is <= 0 or > NativeAbi.MaxOfficeImageDimension
            || targetHeight is <= 0 or > NativeAbi.MaxOfficeImageDimension
            || !_previewSession.IsCurrentRequest(parentPreviewRequestId))
        {
            return null;
        }

        try
        {
            await EnsureParserHostStartedAsync(cancellationToken);
            if (cancellationToken.IsCancellationRequested
                || !_previewSession.IsCurrentRequest(parentPreviewRequestId))
            {
                return null;
            }

            return await _parserSupervisor!.ExtractOfficeImageAsync(
                parentPreviewRequestId,
                imageRef,
                targetWidth,
                targetHeight,
                cancellationToken);
        }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
        {
            return null;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "Office layout image request failed: " + ex.Message);
            return null;
        }
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
        ShowListingHeroFallback(ready);
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
        int generation = _previewSession.Generation;
        CancellationToken token = CurrentPreviewToken;
        string? requestId = _previewSession.CurrentRequestId;
        if (!IsPreviewGenerationCurrent(generation, token))
            return;

        string? path = row.NativePath;
        ArchiveEntryHandoff? archiveHandoff = null;
        string? currentParserPreviewRequestId = null;
        if (!_currentPreviewWasCloudPlaceholder
            && _previewSession.CurrentRequestId is { } currentRequestId
            && _requestHosts.TryGetValue(currentRequestId, out PreviewHostOwner owner)
            && owner == PreviewHostOwner.Parser)
        {
            currentParserPreviewRequestId = currentRequestId;
        }
        // Direct HANDLE archive listings deliberately omit RootPath. Anchored package and legacy/cloud
        // listings retain a real path and must continue through the compatibility extraction branch.
        bool isParentBoundArchiveListing =
            listing is not null
            && string.IsNullOrWhiteSpace(listing.RootPath)
            && (string.Equals(_currentProbe?.Kind, "archive", StringComparison.OrdinalIgnoreCase)
                || string.Equals(_currentProbe?.Kind, "ebook", StringComparison.OrdinalIgnoreCase));
        string? archiveParentRequestId =
            isParentBoundArchiveListing
                ? currentParserPreviewRequestId
                : null;
        if (string.IsNullOrWhiteSpace(path)
            && listing is not null
            && listing.ListingKind.Equals("archive", StringComparison.OrdinalIgnoreCase)
            && (archiveParentRequestId is not null || !string.IsNullOrWhiteSpace(listing.RootPath)))
        {
            await EnsureParserHostStartedAsync();
            if (!IsPreviewGenerationCurrent(generation, token)
                || !string.Equals(_previewSession.CurrentRequestId, requestId, StringComparison.Ordinal))
            {
                return;
            }
            archiveHandoff = await _parserSupervisor!.ExtractArchiveEntryAsync(
                listing.RootPath,
                row.Path,
                archiveParentRequestId,
                token);
            if (!IsPreviewGenerationCurrent(generation, token)
                || !string.Equals(_previewSession.CurrentRequestId, requestId, StringComparison.Ordinal))
            {
                if (archiveHandoff is not null)
                    await _parserSupervisor.ReleaseArchiveEntryAsync(archiveHandoff);
                return;
            }
            if (archiveHandoff is not null)
            {
                path = archiveHandoff.Path;
            }
        }

        if (string.IsNullOrWhiteSpace(path))
            return;

        if (IsPreviewGenerationCurrent(generation, token)
            && string.Equals(_previewSession.CurrentRequestId, requestId, StringComparison.Ordinal))
        {
            await PreviewWindowPathAsync(path, archiveHandoff);
        }
        else if (archiveHandoff is not null)
        {
            await _parserSupervisor!.ReleaseArchiveEntryAsync(archiveHandoff);
        }
    }

    private async Task<ImageSource?> LoadListingIconAsync(
        ListingRow row,
        int generation,
        CancellationToken cancellationToken)
    {
        string? path = row.NativePath;
        if (string.IsNullOrWhiteSpace(path))
            return null;

        try
        {
            bool mayRequireHydration = await Task.Run(() => CloudFileStatus.MayRequireHydration(path), cancellationToken);
            if (mayRequireHydration || !IsPreviewGenerationCurrent(generation, cancellationToken))
                return null;
            NativeRasterImage? raster = await _thumbnailScheduler.LoadAsync(
                path,
                32,
                NativeThumbnailPriority.Foreground,
                cacheOnly: true,
                cancellationToken);

            if (!IsPreviewGenerationCurrent(generation, cancellationToken) || raster is null)
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
        ResetTextSearchUi();
        _rasterPresenter?.Clear();
        _animatedImagePresenter?.Clear();
        _imageWaveformPresenter?.Clear();
        _mediaPresenter?.Clear();
        _pdfPresenter?.Clear();
        _textPresenter?.Clear();
        _tablePresenter?.Clear();
        _officePresenter?.Clear();
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
            if (ready.Listing is not null && ListingPanel.Visibility == Visibility.Visible)
                ShowListingHeroFallback(ready);
            else
                ClearPreviewHeroImages();
            return;
        }

        int generation = _previewSession.Generation;
        CancellationToken token = CurrentPreviewToken;
        string? parentPreviewRequestId = _previewSession.CurrentRequestId;
        bool cloudOrigin = _currentPreviewWasCloudPlaceholder;
        Task.Run(async () =>
        {
            if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                return null;
            return await LoadPreviewHeroRasterAsync(
                ready,
                path,
                cloudOrigin,
                parentPreviewRequestId,
                generation,
                token);
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
                    ListingHeroImage.Visibility = Visibility.Visible;
                    ListingFolderHeroIcon.Visibility = Visibility.Collapsed;
                    ListingArchiveHeroIcon.Visibility = Visibility.Collapsed;
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
        string? parentPreviewRequestId,
        int generation,
        CancellationToken token)
    {
        if (cloudOrigin)
            return null;

        if (IsPackagePreview(ready, path))
        {
            await EnsureParserHostStartedAsync();
            if (!IsPreviewGenerationCurrent(generation, token)
                || !string.Equals(_previewSession.CurrentRequestId, parentPreviewRequestId, StringComparison.Ordinal))
                return null;
            NativeRasterImage? icon = await _parserSupervisor!.ExtractHeroRasterAsync(
                path, "package", parentPreviewRequestId, token);
            return icon ?? await _thumbnailScheduler.LoadAsync(path, 512, NativeThumbnailPriority.Foreground, cacheOnly: false, token);
        }

        if (IsOfficePreviewWithImages(ready))
        {
            await EnsureParserHostStartedAsync();
            if (!IsPreviewGenerationCurrent(generation, token)
                || !string.Equals(_previewSession.CurrentRequestId, parentPreviewRequestId, StringComparison.Ordinal))
                return null;
            return await _parserSupervisor!.ExtractHeroRasterAsync(
                path, "office", parentPreviewRequestId, token);
        }

        if (IsExecutablePreview(ready, path))
            return await _thumbnailScheduler.LoadAsync(path, 512, NativeThumbnailPriority.Foreground, cacheOnly: false, token);

        if (ready.Kind == "certificate")
            return await _thumbnailScheduler.LoadAsync(path, 256, NativeThumbnailPriority.Foreground, cacheOnly: false, token);

        return null;
    }

    private static bool ShouldLoadPreviewHero(PreviewReady ready, string path)
        => !IsDatabasePath(path)
           && ready.OfficeLayout is null
           && (IsPackagePreview(ready, path)
           || IsExecutablePreview(ready, path)
           || ready.Kind == "certificate"
           || IsOfficePreviewWithImages(ready));

    private static bool IsDatabasePath(string path)
    {
        string fileName = System.IO.Path.GetFileName(path);
        return fileName.EndsWith("-wal", StringComparison.OrdinalIgnoreCase)
            || fileName.EndsWith("-shm", StringComparison.OrdinalIgnoreCase)
            || System.IO.Path.GetExtension(path).ToLowerInvariant()
                is ".sqlite" or ".sqlite3" or ".db" or ".db3" or ".s3db" or ".sqlite-shm" or ".sqlite-wal" or ".mdb" or ".accdb";
    }

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

    private void ShowListingHeroFallback(PreviewReady ready)
    {
        bool isFolder = ready.Listing?.ListingKind.Equals(
            "folder",
            StringComparison.OrdinalIgnoreCase) == true;
        bool isArchive = ready.Listing?.ListingKind.Equals(
            "archive",
            StringComparison.OrdinalIgnoreCase) == true;
        ListingHeroImage.Source = null;
        ListingHeroImage.Visibility = Visibility.Collapsed;
        ListingFolderHeroIcon.Visibility = isFolder ? Visibility.Visible : Visibility.Collapsed;
        ListingArchiveHeroIcon.Visibility = isArchive ? Visibility.Visible : Visibility.Collapsed;
        ListingHeroFrame.Visibility = isFolder || isArchive ? Visibility.Visible : Visibility.Collapsed;
    }

    private static string BuildPreviewHeroSubtitle(PreviewReady ready, string path)
    {
        string ext = System.IO.Path.GetExtension(path).TrimStart('.').ToUpperInvariant();
        return ready.Kind switch
        {
            "office" => string.IsNullOrEmpty(ext)
                ? UiStrings.OfficeEmbeddedImagePreview
                : UiStrings.Format(UiStrings.OfficeEmbeddedImagePreviewFormat, ext),
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
        ListingHeroImage.Visibility = Visibility.Collapsed;
        ListingFolderHeroIcon.Visibility = Visibility.Collapsed;
        ListingArchiveHeroIcon.Visibility = Visibility.Collapsed;
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
        _ = LoadImageMetadataAsync(ready.RequestId, path, generation, token);
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

    private async Task LoadImageMetadataAsync(
        string previewRequestId,
        string path,
        int generation,
        CancellationToken token)
    {
        try
        {
            using var trace = DiagLog.TraceScope(
                "App",
                $"HANDLE image metadata load gen={generation}; request={previewRequestId}; path={path}",
                250);
            ImageMetadata? metadata = await _supervisor!.GetImageMetadataAsync(
                previewRequestId,
                token);
            if (metadata is null
                || !IsPreviewGenerationCurrent(generation, token)
                || !_previewSession.IsCurrentPath(path))
                return;
            DispatcherQueue.TryEnqueue(() =>
            {
                if (!IsPreviewGenerationCurrent(generation, token) || !_previewSession.IsCurrentPath(path))
                    return;
                RenderImageMetadata(metadata);
            });
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "HANDLE image metadata load failed: " + ex.Message);
        }
    }

    private void RenderImageMetadata(ImageMetadata metadata)
    {
        var rows = new List<(string Label, string Value)>();
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelFormat"), metadata.Format);
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelTitle"), metadata.Title);
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelComment"), metadata.Comment);
        AddIfValue(
            rows,
            UiStrings.Get("ImageMetadataLabelDimensions"),
            metadata.Width is > 0 && metadata.Height is > 0
                ? UiStrings.Format(
                    UiStrings.Get("ImageMetadataDimensionsFormat"),
                    metadata.Width.Value,
                    metadata.Height.Value)
                : null);
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelResolution"), FormatResolution(
            metadata.HorizontalResolution,
            metadata.VerticalResolution));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelBitDepth"), metadata.BitDepth?.ToString(CultureInfo.InvariantCulture));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelColorType"), LocalizeImageMetadataValue(metadata.ColorType));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelCompression"), LocalizeImageMetadataValue(metadata.Compression));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelAlpha"), FormatBoolean(metadata.HasAlpha));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelInterlace"), LocalizeImageMetadataValue(metadata.Interlace));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelAnimated"), FormatBoolean(metadata.Animated));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelFrames"), metadata.FrameCount?.ToString(CultureInfo.CurrentCulture));
        AddIfValue(
            rows,
            UiStrings.Get("ImageMetadataLabelAnimationDuration"),
            metadata.DurationMs is > 0
                ? UiStrings.Format(
                    UiStrings.Get("ImageMetadataDurationSecondsFormat"),
                    metadata.DurationMs.Value / 1000.0)
                : null);
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelDateTaken"), FormatExifDateTime(metadata.DateTime));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelCamera"), JoinNonEmpty(metadata.Make, metadata.Model));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelLens"), JoinNonEmpty(metadata.LensMake, metadata.LensModel));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelFocalLength"), FormatDoubleWithUnit(metadata.FocalLength, "ImageMetadataMillimetersFormat"));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelEquivalent35mm"), FormatDoubleWithUnit(metadata.FocalLengthIn35mmFilm, "ImageMetadataMillimetersFormat"));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelAperture"), FormatAperture(metadata.FNumber));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelMaxAperture"), FormatAperture(metadata.MaxAperture));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelShutterSpeed"), FormatExposureSeconds(metadata.ExposureTime));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelIso"), metadata.Iso?.ToString(CultureInfo.CurrentCulture));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelExposureBias"), FormatDoubleWithUnit(metadata.ExposureBias, "ImageMetadataExposureValueFormat"));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelExposureProgram"), FormatExifEnum(metadata.ExposureProgram, ExposureProgramResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelExposureMode"), FormatExifEnum(metadata.ExposureMode, ExposureModeResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelMetering"), FormatExifEnum(metadata.MeteringMode, MeteringModeResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelWhiteBalance"), FormatExifEnum(metadata.WhiteBalance, WhiteBalanceResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelLightSource"), FormatExifEnum(metadata.LightSource, LightSourceResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelFlash"), FormatFlash(metadata.Flash));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelDigitalZoom"), FormatDoubleWithUnit(metadata.DigitalZoomRatio, "ImageMetadataZoomRatioFormat"));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelSubjectDistance"), FormatDoubleWithUnit(metadata.SubjectDistance, "ImageMetadataMetersFormat"));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelOrientation"), metadata.Orientation?.ToString(CultureInfo.CurrentCulture));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelPhotometric"), LocalizeImageMetadataValue(metadata.PhotometricInterpretation));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelContrast"), FormatExifEnum(metadata.Contrast, NormalHardSoftResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelSaturation"), FormatExifEnum(metadata.Saturation, NormalHardSoftResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelSharpness"), FormatExifEnum(metadata.Sharpness, NormalHardSoftResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelGainControl"), FormatExifEnum(metadata.GainControl, GainControlResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelColorSpace"), FormatExifEnum(metadata.ColorSpace, ColorSpaceResourceKeys));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelLocation"), FormatLocation(metadata.Latitude, metadata.Longitude));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelAltitude"), FormatDoubleWithUnit(metadata.Altitude, "ImageMetadataMetersFormat"));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelDirection"), FormatDoubleWithUnit(metadata.Direction, "ImageMetadataDegreesFormat"));
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelSoftware"), metadata.Software);
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelCameraSerial"), metadata.CameraSerial);
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelLensSerial"), metadata.LensSerial);
        AddIfValue(rows, UiStrings.Get("ImageMetadataLabelExifVersion"), metadata.ExifVersion);

        if (rows.Count > 0)
            _exifPresenter?.RenderRows(rows, metadata.Latitude, metadata.Longitude);
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

    private static string? FormatDoubleWithUnit(double? value, string formatResourceKey)
        => value.HasValue
            ? UiStrings.Format(UiStrings.Get(formatResourceKey), value.Value)
            : null;

    private static string? FormatBoolean(bool? value)
        => value.HasValue
            ? UiStrings.Get(value.Value ? "ImageMetadataValueYes" : "ImageMetadataValueNo")
            : null;

    private static string? FormatAperture(double? value)
        => value is { } aperture && double.IsFinite(aperture) && aperture > 0
            ? UiStrings.Format(UiStrings.Get("ImageMetadataApertureFormat"), aperture)
            : null;

    private static string? FormatExposureSeconds(double? value)
    {
        if (!value.HasValue || value.Value <= 0)
            return null;
        return value.Value < 1.0
            ? UiStrings.Format(UiStrings.Get("ImageMetadataExposureFractionFormat"), Math.Round(1.0 / value.Value))
            : UiStrings.Format(UiStrings.Get("ImageMetadataExposureSecondsFormat"), value.Value);
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

    private static string? FormatResolution(double? horizontal, double? vertical)
    {
        if (!horizontal.HasValue && !vertical.HasValue)
            return null;
        if (horizontal.HasValue && vertical.HasValue
            && Math.Abs(horizontal.Value - vertical.Value) < 0.001)
            return UiStrings.Format(UiStrings.Get("ImageMetadataResolutionDpiFormat"), horizontal.Value);
        if (horizontal.HasValue && vertical.HasValue)
        {
            return UiStrings.Format(
                UiStrings.Get("ImageMetadataResolutionPairDpiFormat"),
                horizontal.Value,
                vertical.Value);
        }
        return UiStrings.Format(
            UiStrings.Get("ImageMetadataResolutionDpiFormat"),
            horizontal ?? vertical!.Value);
    }

    private static string? LocalizeImageMetadataValue(string? value)
    {
        if (string.IsNullOrWhiteSpace(value))
            return null;
        string trimmed = value.Trim();
        return ImageMetadataValueResourceKeys.TryGetValue(trimmed, out string? resourceKey)
            ? UiStrings.Get(resourceKey)
            : trimmed;
    }

    private static string? FormatExifEnum(string? raw, IReadOnlyDictionary<int, string> names)
    {
        if (string.IsNullOrWhiteSpace(raw))
            return null;
        string trimmed = raw.Trim();
        if (!int.TryParse(trimmed, out int value))
            return trimmed;
        return names.TryGetValue(value, out string? resourceKey)
            ? UiStrings.Get(resourceKey)
            : trimmed;
    }

    private static string? FormatFlash(string? raw)
    {
        if (string.IsNullOrWhiteSpace(raw))
            return null;
        if (!int.TryParse(raw, out int flags))
            return raw;

        var parts = new List<string>();
        parts.Add(UiStrings.Get((flags & 0x1) != 0
            ? "ImageMetadataFlashFired"
            : "ImageMetadataFlashDidNotFire"));
        if ((flags & 0x18) == 0x18)
            parts.Add(UiStrings.Get("ImageMetadataFlashAuto"));
        if ((flags & 0x40) != 0)
            parts.Add(UiStrings.Get("ImageMetadataFlashRedEyeReduction"));
        if ((flags & 0x6) == 0x4)
            parts.Add(UiStrings.Get("ImageMetadataFlashReturnDetected"));
        else if ((flags & 0x6) == 0x6)
            parts.Add(UiStrings.Get("ImageMetadataFlashReturnNotDetected"));
        return string.Join(UiStrings.Get("ImageMetadataValueSeparator"), parts);
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
        if (_isFullscreen)
            return;

        SizeInt32 size = PreviewWindowSizer.GetWindowSizeForContent(
            GetWindowId(),
            contentWidth,
            contentHeight,
            maxWidth,
            maxHeight,
            RasterizationScale);
        DiagLog.Write("App", $"window resize content={contentWidth:0}x{contentHeight:0}; target={size.Width}x{size.Height}; visible={_previewVisible}; pending={_previewRevealPending}; topmost={setTopmost}");
        TemporarilyHideWindowForTransitionResize();
        AppWindow appWindow = GetAppWindow();
        PointInt32? position = PreviewWindowSizer.GetCenteredPosition(GetWindowId(), size);
        if (position is { } point)
            appWindow.MoveAndResize(new RectInt32(point.X, point.Y, size.Width, size.Height));
        else
            appWindow.Resize(size);
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

    private void OnPreviewContentPointerWheelChanged(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
    {
        if (ImageFilmstrip.Visibility == Visibility.Visible
            && IsPointInside(e.GetCurrentPoint(ImageFilmstrip).Position, ImageFilmstrip))
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
            return;
        }

        if (PreviewRoot.Visibility == Visibility.Visible
            && IsPointInside(e.GetCurrentPoint(PreviewRoot).Position, PreviewRoot))
            _rasterPresenter?.OnPointerWheelChanged(e);
        else if (AnimatedImagePreviewRoot.Visibility == Visibility.Visible
            && IsPointInside(e.GetCurrentPoint(AnimatedImagePreviewRoot).Position, AnimatedImagePreviewRoot))
            _animatedImagePresenter?.OnPointerWheelChanged(e);
    }

    private static bool IsPointInside(Windows.Foundation.Point point, FrameworkElement element)
        => point.X >= 0 && point.Y >= 0 && point.X < element.ActualWidth && point.Y < element.ActualHeight;

    private bool OnWindowMouseWheel(int delta, int clientPixelX, int clientPixelY)
    {
        double scale = RasterizationScale;
        var rootPoint = new Windows.Foundation.Point(clientPixelX / scale, clientPixelY / scale);

        if (ImageFilmstrip.Visibility == Visibility.Visible && TryGetPointInElement(rootPoint, ImageFilmstrip, out _))
        {
            _imageFilmstripScrollViewer ??= FindDescendant<ScrollViewer>(ImageFilmstripList);
            if (_imageFilmstripScrollViewer is not { } scrollViewer)
                return false;
            scrollViewer.ChangeView(scrollViewer.HorizontalOffset - delta, null, null, disableAnimation: false);
            return true;
        }

        if (PreviewRoot.Visibility == Visibility.Visible && TryGetPointInElement(rootPoint, PreviewRoot, out var imagePoint))
        {
            _rasterPresenter?.OnMouseWheel(delta, imagePoint);
            return _rasterPresenter?.HasSurface == true;
        }
        if (AnimatedImagePreviewRoot.Visibility == Visibility.Visible
            && TryGetPointInElement(rootPoint, AnimatedImagePreviewRoot, out imagePoint))
        {
            _animatedImagePresenter?.OnMouseWheel(delta, imagePoint);
            return _animatedImagePresenter?.HasImage == true;
        }
        return false;
    }

    private bool TryGetPointInElement(
        Windows.Foundation.Point rootPoint,
        FrameworkElement element,
        out Windows.Foundation.Point elementPoint)
    {
        Windows.Foundation.Point origin = element.TransformToVisual(RootGrid).TransformPoint(default);
        elementPoint = new Windows.Foundation.Point(rootPoint.X - origin.X, rootPoint.Y - origin.Y);
        return IsPointInside(elementPoint, element);
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
        bool controlDown = (Microsoft.UI.Input.InputKeyboardSource
            .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Control)
            & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        bool shiftDown = (Microsoft.UI.Input.InputKeyboardSource
            .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Shift)
            & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        bool modifierDown = (Microsoft.UI.Input.InputKeyboardSource
            .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Shift) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0
            || (Microsoft.UI.Input.InputKeyboardSource
                .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Control) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0
            || (Microsoft.UI.Input.InputKeyboardSource
                .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Menu) & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        bool focusedControlUsesSpace = Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement() is Control;
        if (e.Key == Windows.System.VirtualKey.Space
            && !modifierDown
            && !focusedControlUsesSpace
            && ShouldHandleSpaceAsPreviewClose())
        {
            e.Handled = true;
            ClosePreviewFromKeyboard();
            return;
        }

        bool textPreviewVisible = TextPreviewContainer.Visibility == Visibility.Visible;
        if (textPreviewVisible && controlDown && e.Key == Windows.System.VirtualKey.F)
        {
            OpenTextSearch();
            e.Handled = true;
            return;
        }
        if (textPreviewVisible
            && TextSearchBar.Visibility == Visibility.Visible
            && e.Key == Windows.System.VirtualKey.F3
            && _textPresenter is { } textPresenter)
        {
            ApplyTextSearchState(textPresenter.MoveSearch(shiftDown ? -1 : 1));
            e.Handled = true;
            return;
        }
        if (textPreviewVisible
            && TextSearchBar.Visibility == Visibility.Visible
            && e.Key == Windows.System.VirtualKey.Escape)
        {
            CloseTextSearch();
            e.Handled = true;
            return;
        }

        if (ListingPanel.Visibility == Visibility.Visible && controlDown && e.Key == Windows.System.VirtualKey.F)
        {
            _listingPresenter?.FocusFilter();
            e.Handled = true;
            return;
        }

        bool imagePreviewVisible =
            (_rasterPresenter?.HasSurface == true && PreviewRoot.Visibility == Visibility.Visible)
            || (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible);
        if (!imagePreviewVisible)
            return;

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

        if (shiftDown && e.Key is Windows.System.VirtualKey.Left
            or Windows.System.VirtualKey.Right
            or Windows.System.VirtualKey.Up
            or Windows.System.VirtualKey.Down)
        {
            const double keyboardPanStep = 48;
            double x = e.Key switch
            {
                Windows.System.VirtualKey.Left => keyboardPanStep,
                Windows.System.VirtualKey.Right => -keyboardPanStep,
                _ => 0,
            };
            double y = e.Key switch
            {
                Windows.System.VirtualKey.Up => keyboardPanStep,
                Windows.System.VirtualKey.Down => -keyboardPanStep,
                _ => 0,
            };
            if (_animatedImagePresenter?.HasImage == true && AnimatedImagePreviewRoot.Visibility == Visibility.Visible)
                _animatedImagePresenter.PanBy(x, y);
            else
                _rasterPresenter?.PanBy(x, y);
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

    private void OpenTextSearch()
    {
        if (TextPreviewContainer.Visibility != Visibility.Visible || _textPresenter is not { } textPresenter)
            return;

        TextSearchBar.Visibility = Visibility.Visible;
        TextSearchBox.Focus(FocusState.Programmatic);
        TextSearchBox.SelectAll();
        ApplyTextSearchState(textPresenter.SetSearchQuery(TextSearchBox.Text));
    }

    private void OnTextSearchTextChanged(object sender, TextChangedEventArgs e)
    {
        if (_suppressTextSearchTextChanged || _textPresenter is not { } textPresenter)
            return;

        ApplyTextSearchState(textPresenter.SetSearchQuery(TextSearchBox.Text));
    }

    private void OnTextSearchPreviousClick(object sender, RoutedEventArgs e)
        => MoveTextSearch(-1);

    private void OnTextSearchNextClick(object sender, RoutedEventArgs e)
        => MoveTextSearch(1);

    private void MoveTextSearch(int delta)
    {
        if (_textPresenter is { } textPresenter)
            ApplyTextSearchState(textPresenter.MoveSearch(delta));
    }

    private void OnTextSearchCloseClick(object sender, RoutedEventArgs e)
        => CloseTextSearch();

    private void OnTextSearchBoxKeyDown(object sender, Microsoft.UI.Xaml.Input.KeyRoutedEventArgs e)
    {
        bool shiftDown = (Microsoft.UI.Input.InputKeyboardSource
            .GetKeyStateForCurrentThread(Windows.System.VirtualKey.Shift)
            & Windows.UI.Core.CoreVirtualKeyStates.Down) != 0;
        if (e.Key == Windows.System.VirtualKey.Enter)
        {
            MoveTextSearch(shiftDown ? -1 : 1);
            e.Handled = true;
        }
        else if (e.Key == Windows.System.VirtualKey.Escape)
        {
            CloseTextSearch();
            e.Handled = true;
        }
    }

    private void CloseTextSearch()
    {
        TextSearchBar.Visibility = Visibility.Collapsed;
        _suppressTextSearchTextChanged = true;
        try
        {
            TextSearchBox.Text = "";
        }
        finally
        {
            _suppressTextSearchTextChanged = false;
        }

        ApplyTextSearchState(_textPresenter?.ClearSearch() ?? default);
        FocusTextPreviewContent();
    }

    private void ResetTextSearchUi()
    {
        TextSearchBar.Visibility = Visibility.Collapsed;
        _suppressTextSearchTextChanged = true;
        try
        {
            TextSearchBox.Text = "";
        }
        finally
        {
            _suppressTextSearchTextChanged = false;
        }
        ApplyTextSearchState(default);
    }

    private void ApplyTextSearchState(TextSearchState state)
    {
        TextSearchCountText.Text = UiStrings.Format(UiStrings.TextSearchCountFormat, state.Current, state.Count);
        bool hasMatches = state.Count > 0;
        TextSearchPreviousButton.IsEnabled = hasMatches;
        TextSearchNextButton.IsEnabled = hasMatches;
    }

    private void FocusTextPreviewContent()
    {
        FrameworkElement focusTarget = MarkdownListView.Visibility == Visibility.Visible
            ? MarkdownListView
            : TextListView.Visibility == Visibility.Visible
                ? TextListView
                : TextPreviewBlock;
        focusTarget.Focus(FocusState.Programmatic);
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

        return true;
    }

    private void OnOpenFileLocationClick(object sender, RoutedEventArgs e)
        => OpenCurrentPreviewPath(revealInExplorer: true);

    private void OnOpenPreviewFileClick(object sender, RoutedEventArgs e)
        => OpenCurrentPreviewPath(revealInExplorer: false);

    private void OnOpenErrorPreviewFileClick(object sender, RoutedEventArgs e)
        => OpenPreviewPath(_previewSession.ErrorActionPath, revealInExplorer: false);

    private void OnRevealErrorPreviewFileClick(object sender, RoutedEventArgs e)
        => OpenPreviewPath(_previewSession.ErrorActionPath, revealInExplorer: true);

    private async void OnRetryPreviewClick(object sender, RoutedEventArgs e)
    {
        if (_previewSession.ErrorContext is not PreviewErrorContext context
            || !context.CanRetry
            || !_previewSession.IsCurrentError(context))
        {
            return;
        }

        await PreviewWindowPathAsync(context.Path);
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

        await _modalDialogGate.WaitAsync();
        try
        {
            _isModalDialogOpen = true;
            ContentDialogResult result = await dialog.ShowAsync();
            if (result != ContentDialogResult.Primary)
                return;
        }
        finally
        {
            _isModalDialogOpen = false;
            _modalDialogGate.Release();
        }

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
            StatusText.Text = UiStrings.Get("DeleteFileFailed");
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
        => OpenPreviewPath(_previewSession.CurrentPath, revealInExplorer);

    private void OpenPreviewPath(string? path, bool revealInExplorer)
    {
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
                if (_previewSession.IsCurrentPath(path))
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
                if (_previewSession.IsCurrentPath(path))
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
                if (_previewSession.IsCurrentPath(path))
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

    private void OnPreviewRootPointerCaptureLost(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _rasterPresenter?.OnPointerCaptureLost();

    private void OnPreviewRootDoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        => _rasterPresenter?.OnDoubleTapped(e);

    private void OnAnimatedImageRootPointerPressed(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _animatedImagePresenter?.OnPointerPressed(e);

    private void OnAnimatedImageRootPointerMoved(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _animatedImagePresenter?.OnPointerMoved(e);

    private void OnAnimatedImageRootPointerReleased(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _animatedImagePresenter?.OnPointerReleased(e);

    private void OnAnimatedImageRootPointerCaptureLost(object sender, Microsoft.UI.Xaml.Input.PointerRoutedEventArgs e)
        => _animatedImagePresenter?.OnPointerCaptureLost();

    private void OnAnimatedImageRootDoubleTapped(object sender, Microsoft.UI.Xaml.Input.DoubleTappedRoutedEventArgs e)
        => _animatedImagePresenter?.OnDoubleTapped(e);

    internal static string FormatBytes(long bytes)
    {
        double value = bytes;
        int unit = 0;
        while (value >= 1024 && unit < ByteSizeFormatResourceKeys.Length - 1)
        {
            value /= 1024;
            unit++;
        }
        return UiStrings.Format(
            UiStrings.Get(ByteSizeFormatResourceKeys[unit]),
            unit == 0 ? bytes : value);
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
    private bool PrefersReducedMotion => IsHighContrast || AppSettings.Current.Animation switch
    {
        "always" => false,
        "still" => true,
        _ => !_uiSettings.AnimationsEnabled,
    };

    private void ApplyAccessibilityVisuals()
    {
        TrySetBackdrop();
        ApplyImageLetterboxBackgrounds();
        UpdateTitleBarColors();
        _tablePresenter?.RefreshPalette();
        _officePresenter?.RefreshPalette();
        _textPresenter?.RefreshPalette();
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
        try { appWindow.Show(false); }
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
        if (!_previewRevealPending)
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
        ExitFullscreen();
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

    private void ToggleFullscreen()
    {
        try
        {
            if (_isFullscreen)
            {
                ExitFullscreen();
                return;
            }

            GetAppWindow().SetPresenter(AppWindowPresenterKind.FullScreen);
            AppTitleBar.Visibility = Visibility.Collapsed;
            _isFullscreen = true;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "fullscreen transition failed: " + ex.Message);
        }
    }

    private void ExitFullscreen()
    {
        if (!_isFullscreen)
            return;
        try
        {
            GetAppWindow().SetPresenter(AppWindowPresenterKind.Default);
            AppTitleBar.Visibility = Visibility.Visible;
            _isFullscreen = false;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "fullscreen exit failed: " + ex.Message);
        }
    }

    private WindowId GetWindowId()
        => Win32Interop.GetWindowIdFromWindow(WinRT.Interop.WindowNative.GetWindowHandle(this));

    private void EnsureTrayIcon()
    {
        _trayIcon ??= new TrayIconManager(
            WinRT.Interop.WindowNative.GetWindowHandle(this),
            ResolveTrayIconPath,
            () => ShowPreviewWindow(activate: true),
            ShowSettingsWindow,
            ExitApp,
            message => StatusText.Text = message);
        _trayIcon.Ensure();
    }

    private void ShowSettingsWindow()
    {
        if (_settingsWindow is null)
        {
            _settingsWindow = new SettingsWindow(
                ResolveAppIconPath,
                OnSettingsChanged,
                () => _native.LastHookFailure ?? _native.HookStatus,
                RetryNativeHook);
            _settingsWindow.Closed += (_, _) => _settingsWindow = null;
        }
        _settingsWindow.Activate();
    }

    private bool RetryNativeHook()
    {
        try
        {
            _native.Stop();
            _native.Start(OnNativeIntent);
            StatusText.Text = UiStrings.Ready;
            DiagLog.Write("App", "native hook restarted from settings");
            return true;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "native hook retry failed: " + ex.Message);
            return false;
        }
    }

    private void OnSettingsChanged()
    {
        RefreshTrayIcon();
        if (PrefersReducedMotion)
            _animatedImagePresenter?.PausePlayback();
        UpdateImageAnimationPlaybackButton();
        _textPresenter?.ApplyTextPreferences(
            AppSettings.Current.TextWrapping,
            AppSettings.Current.TextSize,
            AppSettings.Current.TextLineNumbers);
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
        _settingsWindow?.Close();
        _previewKeyboardHook?.Dispose();
        _native.Stop();
        _supervisor?.Stop();
        _parserSupervisor?.Stop();
        _shellBroker?.Stop();
        try { Microsoft.UI.Xaml.Application.Current.Exit(); }
        catch (Exception ex) { DiagLog.Write("App", "graceful exit failed: " + ex); }
    }

    private (
        FileProbe Probe,
        Microsoft.Win32.SafeHandles.SafeFileHandle? Handle,
        long Length,
        string Authority) PreparePreviewProbe(
            string path,
            bool metadataOnly)
    {
        if (metadataOnly)
        {
            return (
                FallbackFileProbe.CreateMetadataOnlyProbe(path),
                null,
                0,
                "cloud-metadata");
        }

        if (Directory.Exists(path))
        {
            return (
                _native.ProbeFile(path) ?? BuildProbe(path),
                null,
                0,
                "path-directory");
        }

        if (!_native.SupportsHandleProbe)
        {
            DiagLog.Write(
                "App",
                $"preview probe compatibility fallback: reason=missing-handle-capability; path={path}");
            return (
                _native.ProbeFile(path) ?? BuildProbe(path),
                null,
                0,
                "path-compatibility");
        }

        (Microsoft.Win32.SafeHandles.SafeFileHandle Handle, long Length) pinned;
        try
        {
            pinned = WindowsHandleTransfer.OpenPinnedReadOnlyFile(path);
        }
        catch (Exception ex) when (
            ex is System.ComponentModel.Win32Exception
                or IOException
                or UnauthorizedAccessException
                or InvalidDataException
                or NotSupportedException
                or ArgumentException)
        {
            DiagLog.Write(
                "App",
                $"preview probe compatibility fallback: reason=pin-failed; type={ex.GetType().Name}; path={path}");
            return (
                _native.ProbeFile(path) ?? BuildProbe(path),
                null,
                0,
                "path-compatibility");
        }

        try
        {
            FileProbe probe = _native.ProbeFileHandle(
                    pinned.Handle,
                    pinned.Length,
                    path)
                ?? throw new InvalidDataException("Native HANDLE probe returned no result.");
            if (probe.Size != pinned.Length)
                throw new InvalidDataException("Native HANDLE probe length did not match its pinned source.");
            return (probe, pinned.Handle, pinned.Length, "pinned-handle");
        }
        catch
        {
            pinned.Handle.Dispose();
            throw;
        }
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
        string extension = System.IO.Path.GetExtension(path);
        return new FileProbe(path, extension, magic)
        {
            Kind = extension.Equals(".svg", StringComparison.OrdinalIgnoreCase)
                ? "image"
                : FallbackFileProbe.IsText(path, magic, isEmptyFile: size == 0 && System.IO.File.Exists(path)) ? "text" : "unknown",
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

    private static string ResolveShellBrokerExePath()
    {
        string local = System.IO.Path.Combine(AppContext.BaseDirectory, "QuickLook.Next.ShellBroker.exe");
        if (System.IO.File.Exists(local)) return local;
        string broker = System.IO.Path.Combine(AppContext.BaseDirectory, "ShellBroker", "QuickLook.Next.ShellBroker.exe");
        if (System.IO.File.Exists(broker)) return broker;
        return System.IO.Path.GetFullPath(System.IO.Path.Combine(AppContext.BaseDirectory,
            @"..\..\..\..\..\QuickLook.Next.ShellBroker\bin\Debug\net10.0-windows10.0.19041.0\win-x64\QuickLook.Next.ShellBroker.exe"));
    }

    private static bool IsParserHostPreview(FileProbe probe)
        => PreviewFormatPolicy.UsesParserHost(probe.Kind);

    private (string RequestId, Task<ControlMessage> Completion) BeginPinnedParserOpen(
        string path,
        FileProbe verifiedProbe,
        Microsoft.Win32.SafeHandles.SafeFileHandle pinnedHandle,
        long pinnedLength)
    {
        (Microsoft.Win32.SafeHandles.SafeFileHandle Handle, long Length)? wal = null;
        (Microsoft.Win32.SafeHandles.SafeFileHandle Handle, long Length)? shm = null;
        try
        {
            if (verifiedProbe.Size != pinnedLength
                || !IsParserHostPreview(verifiedProbe))
            {
                throw new InvalidDataException("Pinned ParserHost input did not match its authoritative probe.");
            }
            if (verifiedProbe.Kind.Equals("database", StringComparison.OrdinalIgnoreCase))
            {
                if (IsSqliteMainDatabase(path, verifiedProbe))
                {
                    wal = WindowsHandleTransfer.TryOpenPinnedReadOnlyFile(path + "-wal");
                    shm = WindowsHandleTransfer.TryOpenPinnedReadOnlyFile(path + "-shm");
                }
                return _parserSupervisor!.BeginOpenSqliteHandles(
                    path,
                    verifiedProbe,
                    pinnedHandle,
                    pinnedLength,
                    wal?.Handle,
                    wal?.Length ?? 0,
                    shm?.Handle,
                    shm?.Length ?? 0);
            }
            return _parserSupervisor!.BeginOpenHandle(
                path,
                verifiedProbe,
                pinnedHandle,
                pinnedLength);
        }
        finally
        {
            shm?.Handle.Dispose();
            wal?.Handle.Dispose();
            pinnedHandle.Dispose();
        }
    }

    private static bool IsSqliteMainDatabase(string path, FileProbe probe)
        => !path.EndsWith("-wal", StringComparison.OrdinalIgnoreCase)
            && !path.EndsWith("-shm", StringComparison.OrdinalIgnoreCase)
            && probe.MagicPrefix is { } magic
            && magic.AsSpan().StartsWith("SQLite format 3\0"u8);

    private (string RequestId, Task<ControlMessage> Completion) BeginPinnedRasterOpen(
        string path,
        FileProbe verifiedProbe,
        Microsoft.Win32.SafeHandles.SafeFileHandle pinnedHandle,
        long pinnedLength,
        uint targetWidth,
        uint targetHeight,
        bool prepareAnimation)
    {
        try
        {
            if (verifiedProbe.Size != pinnedLength)
            {
                throw new InvalidDataException("Pinned RasterHost input did not match its authoritative probe.");
            }
            return _supervisor!.BeginPinnedOpen(
                path,
                verifiedProbe,
                pinnedHandle,
                targetWidth,
                targetHeight,
                prepareAnimation);
        }
        finally
        {
            pinnedHandle.Dispose();
        }
    }

    private string ResolveAppIconPath()
        => System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "QuickLookNext.ico");

    private string ResolveTrayIconPath()
    {
        string fileName = RootGrid.ActualTheme == ElementTheme.Light
            ? "QuickLookNextLight.ico"
            : "QuickLookNextDark.ico";
        return System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", fileName);
    }
}
