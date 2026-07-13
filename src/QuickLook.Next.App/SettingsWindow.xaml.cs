using System.Diagnostics;
using System.Reflection;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Graphics;

namespace QuickLook.Next.App;

public sealed partial class SettingsWindow : Window
{
    private const string RepositoryUrl = "https://github.com/SherlockChiang/QuickLook.Next";
    private readonly Func<string> _resolveIconPath;
    private readonly Action _settingsChanged;
    private bool _initializing = true;
    private bool _resizePending;

    public SettingsWindow(Func<string> resolveIconPath, Action settingsChanged)
    {
        _resolveIconPath = resolveIconPath;
        _settingsChanged = settingsChanged;
        InitializeComponent();

        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleBar);
        Title = UiStrings.SettingsTitle;
        ApplyStrings();
        ApplyWindowAppearance();
        Activated += OnActivated;

        AutoStartToggle.IsOn = AutoStart.IsEnabled();
        LanguageCombo.SelectedIndex = AppSettings.Current.Language switch
        {
            "en-US" => 1,
            "zh-CN" => 2,
            _ => 0,
        };
        _initializing = false;
    }

    private void ApplyStrings()
    {
        TitleBarText.Text = UiStrings.SettingsTitle;
        SettingsHeading.Text = UiStrings.SettingsTitle;
        GeneralHeading.Text = UiStrings.SettingsGeneral;
        GeneralDescription.Text = UiStrings.SettingsGeneralDescription;
        AutoStartTitle.Text = UiStrings.SettingsAutoStart;
        AutoStartDescription.Text = UiStrings.SettingsAutoStartDescription;
        LanguageTitle.Text = UiStrings.SettingsLanguage;
        LanguageDescription.Text = UiStrings.SettingsLanguageDescription;
        SystemLanguageItem.Content = UiStrings.SettingsSystemLanguage;
        RestartInfo.Title = UiStrings.SettingsRestartTitle;
        RestartInfo.Message = UiStrings.SettingsRestartMessage;
        AboutHeading.Text = UiStrings.SettingsAbout;
        AboutDescription.Text = UiStrings.SettingsAboutDescription;
        VersionText.Text = UiStrings.Format(UiStrings.SettingsVersionFormat, GetVersion());
        ProjectSourceText.Text = UiStrings.SettingsProjectSource;
        GitHubButtonText.Text = UiStrings.SettingsOpenGitHub;
        ReleasesButtonText.Text = UiStrings.SettingsViewReleases;
        LicenseText.Text = UiStrings.SettingsLicenseNotice;
    }

    private void ApplyWindowAppearance()
    {
        nint hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        WindowId windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
        AppWindow appWindow = AppWindow.GetFromWindowId(windowId);
        appWindow.SetIcon(_resolveIconPath());
        if (appWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsResizable = true;
            presenter.IsMaximizable = false;
            presenter.SetBorderAndTitleBar(true, true);
        }

        string iconPath = Path.Combine(AppContext.BaseDirectory, "Assets", "QuickLookNext.png");
        if (File.Exists(iconPath))
            AppIcon.Source = new BitmapImage(new Uri(iconPath));
    }

    private void OnActivated(object sender, WindowActivatedEventArgs args)
    {
        Activated -= OnActivated;
        QueueResizeToContent();
    }

    private void OnContentSizeChanged(object sender, SizeChangedEventArgs e) => QueueResizeToContent();

    private void QueueResizeToContent()
    {
        if (_resizePending)
            return;
        _resizePending = true;
        DispatcherQueue.TryEnqueue(() =>
        {
            _resizePending = false;
            nint hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            WindowId windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
            AppWindow appWindow = AppWindow.GetFromWindowId(windowId);
            DisplayArea? display = DisplayArea.GetFromWindowId(windowId, DisplayAreaFallback.Nearest);
            if (display is null)
                return;

            RectInt32 work = display.WorkArea;
            double scale = RootGrid.XamlRoot?.RasterizationScale ?? 1.0;
            if (!double.IsFinite(scale) || scale <= 0)
                scale = 1.0;
            double availableWidthDips = Math.Max(420, (work.Width - 32) / scale);
            double widthDips = Math.Min(720, availableWidthDips);
            ContentPanel.Measure(new Windows.Foundation.Size(widthDips, double.PositiveInfinity));
            double desiredHeightDips = 48 + ContentPanel.DesiredSize.Height;
            int width = Math.Min(work.Width - 32, (int)Math.Ceiling(widthDips * scale));
            int height = Math.Min(work.Height - 32, Math.Max((int)Math.Ceiling(360 * scale), (int)Math.Ceiling(desiredHeightDips * scale)));
            var bounds = new RectInt32(
                work.X + (work.Width - width) / 2,
                work.Y + (work.Height - height) / 2,
                width,
                height);
            appWindow.MoveAndResize(bounds);
        });
    }

    private void OnAutoStartToggled(object sender, RoutedEventArgs e)
    {
        if (_initializing)
            return;
        bool requested = AutoStartToggle.IsOn;
        if (AutoStart.SetEnabled(requested))
            return;
        _initializing = true;
        AutoStartToggle.IsOn = !requested;
        _initializing = false;
        RestartInfo.Severity = InfoBarSeverity.Error;
        RestartInfo.Title = UiStrings.SettingsSaveFailed;
        RestartInfo.Message = requested ? UiStrings.AutoStartEnableFailed : UiStrings.AutoStartDisableFailed;
        RestartInfo.IsOpen = true;
        QueueResizeToContent();
    }

    private void OnLanguageSelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        if (_initializing || LanguageCombo.SelectedItem is not ComboBoxItem { Tag: string language })
            return;
        if (!AppSettings.SaveLanguage(language))
        {
            RestartInfo.Severity = InfoBarSeverity.Error;
            RestartInfo.Title = UiStrings.SettingsSaveFailed;
            RestartInfo.Message = UiStrings.SettingsSaveFailedMessage;
            RestartInfo.IsOpen = true;
            return;
        }
        _settingsChanged();
        RestartInfo.Severity = InfoBarSeverity.Informational;
        RestartInfo.Title = UiStrings.SettingsRestartTitle;
        RestartInfo.Message = UiStrings.SettingsRestartMessage;
        RestartInfo.IsOpen = true;
    }

    private static string GetVersion()
        => Assembly.GetExecutingAssembly()
            .GetCustomAttribute<AssemblyInformationalVersionAttribute>()?
            .InformationalVersion.Split('+')[0]
            ?? Assembly.GetExecutingAssembly().GetName().Version?.ToString(3)
            ?? "0.0.0";

    private void OnGitHubClick(object sender, RoutedEventArgs e) => OpenUrl(RepositoryUrl);
    private void OnReleasesClick(object sender, RoutedEventArgs e) => OpenUrl(RepositoryUrl + "/releases");

    private static void OpenUrl(string url)
    {
        try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
        catch { }
    }
}
