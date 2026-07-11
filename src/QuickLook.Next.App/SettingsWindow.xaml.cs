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
        GeneralNav.Content = UiStrings.SettingsGeneral;
        AboutNav.Content = UiStrings.SettingsAbout;
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
        appWindow.Resize(new SizeInt32(880, 700));
        appWindow.SetIcon(_resolveIconPath());
        if (appWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsResizable = true;
            presenter.IsMaximizable = false;
            presenter.SetBorderAndTitleBar(true, true);
        }

        string iconPath = _resolveIconPath();
        if (File.Exists(iconPath))
            AppIcon.Source = new BitmapImage(new Uri(iconPath));
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
