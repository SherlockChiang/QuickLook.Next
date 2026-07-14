using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Windows.Graphics;

namespace QuickLook.Next.App;

public sealed partial class WelcomeWindow : Window
{
    private static WelcomeWindow? _current;

    private WelcomeWindow(Func<string> resolveIconPath)
    {
        InitializeComponent();
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleBar);
        Title = UiStrings.WelcomeTitle;
        TitleBarText.Text = UiStrings.AppName;
        HeadingText.Text = UiStrings.WelcomeHeading;
        IntroductionText.Text = UiStrings.WelcomeIntroduction;
        OpenShortcutText.Text = UiStrings.WelcomeOpenShortcut;
        CloseShortcutText.Text = UiStrings.WelcomeCloseShortcut;
        NavigationShortcutText.Text = UiStrings.WelcomeNavigationShortcut;
        TrayBehaviorText.Text = UiStrings.WelcomeTrayBehavior;
        HelpHintText.Text = UiStrings.WelcomeHelpHint;
        StartButton.Content = UiStrings.WelcomeStart;

        nint hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
        WindowId windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
        AppWindow appWindow = AppWindow.GetFromWindowId(windowId);
        string iconPath = resolveIconPath();
        if (File.Exists(iconPath))
            appWindow.SetIcon(iconPath);
        if (appWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.IsResizable = true;
            presenter.IsMaximizable = false;
        }
        CenterWindow(appWindow, windowId);
        Closed += (_, _) => _current = null;
    }

    public static void Show(Func<string> resolveIconPath, bool markFirstRun = false)
    {
        _current ??= new WelcomeWindow(resolveIconPath);
        _current.Activate();
        if (markFirstRun)
            FirstRunExperience.MarkShown();
    }

    private static void CenterWindow(AppWindow appWindow, WindowId windowId)
    {
        DisplayArea? display = DisplayArea.GetFromWindowId(windowId, DisplayAreaFallback.Nearest);
        if (display is null)
            return;
        RectInt32 work = display.WorkArea;
        int width = Math.Min(680, work.Width - 32);
        int height = Math.Min(520, work.Height - 32);
        appWindow.MoveAndResize(new RectInt32(
            work.X + (work.Width - width) / 2,
            work.Y + (work.Height - height) / 2,
            width,
            height));
    }

    private void OnStartClick(object sender, RoutedEventArgs e) => Close();
}
