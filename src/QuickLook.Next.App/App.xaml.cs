using Microsoft.UI.Xaml;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

public partial class App : Application
{
    private MainWindow? _window;

    public App()
    {
        DiagLog.Init(System.IO.Path.Combine(AppContext.BaseDirectory, "app.log"));
        UnhandledException += (_, e) => DiagLog.Write("App", "xaml unhandled: " + e.Exception);
        AppDomain.CurrentDomain.UnhandledException += (_, e) => DiagLog.Write("App", "domain unhandled: " + e.ExceptionObject);
        TaskScheduler.UnobservedTaskException += (_, e) => DiagLog.Write("App", "task unobserved: " + e.Exception);
        InitializeComponent();
    }

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        _window = new MainWindow();
        try
        {
            await _window.StartBackgroundAsync();
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "background start unhandled: " + ex);
        }
    }
}
