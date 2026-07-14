using Microsoft.UI.Xaml;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

public partial class App : Application
{
    private MainWindow? _window;

    public App()
    {
        DiagLog.Init(System.IO.Path.Combine(AppContext.BaseDirectory, "app.log"));
        AppSettings.ApplyLanguage();
        UnhandledException += (_, e) => DiagLog.Write("App", "xaml unhandled: " + e.Exception);
        AppDomain.CurrentDomain.UnhandledException += (_, e) => DiagLog.Write("App", "domain unhandled: " + e.ExceptionObject);
        TaskScheduler.UnobservedTaskException += (_, e) => DiagLog.Write("App", "task unobserved: " + e.Exception);
        InitializeComponent();
        AppStartupTiming.Mark("app-constructed");
    }

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            _window = new MainWindow();
            await _window.StartBackgroundAsync();
            AppStartupTiming.Mark("background-ready");
            if (FirstRunExperience.ShouldShow)
                WelcomeWindow.Show(
                    () => System.IO.Path.Combine(AppContext.BaseDirectory, "Assets", "QuickLookNext.ico"),
                    markFirstRun: true);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "background start unhandled: " + ex);
        }
    }
}
