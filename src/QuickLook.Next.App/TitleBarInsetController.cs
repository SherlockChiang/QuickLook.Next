using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class TitleBarInsetController : IDisposable
{
    private readonly Window _window;
    private readonly Grid _titleBar;
    private readonly Thickness _basePadding;
    private readonly AppWindow _appWindow;
    private XamlRoot? _xamlRoot;
    private int _updateQueued;
    private int _disposed;

    public TitleBarInsetController(Window window, Grid titleBar)
    {
        _window = window;
        _titleBar = titleBar;
        _basePadding = titleBar.Padding;

        nint hwnd = WinRT.Interop.WindowNative.GetWindowHandle(window);
        WindowId windowId = Win32Interop.GetWindowIdFromWindow(hwnd);
        _appWindow = AppWindow.GetFromWindowId(windowId);

        _titleBar.Loaded += OnTitleBarLoaded;
        _appWindow.Changed += OnAppWindowChanged;
        _window.Closed += OnWindowClosed;

        HookXamlRoot(_titleBar.XamlRoot);
        QueueUpdate();
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0)
            return;

        _titleBar.Loaded -= OnTitleBarLoaded;
        _appWindow.Changed -= OnAppWindowChanged;
        _window.Closed -= OnWindowClosed;
        HookXamlRoot(null);
    }

    private void OnTitleBarLoaded(object sender, RoutedEventArgs args)
    {
        HookXamlRoot(_titleBar.XamlRoot);
        QueueUpdate();
    }

    private void OnAppWindowChanged(AppWindow sender, AppWindowChangedEventArgs args)
        => QueueUpdate();

    private void OnXamlRootChanged(XamlRoot sender, XamlRootChangedEventArgs args)
        => QueueUpdate();

    private void OnWindowClosed(object sender, WindowEventArgs args)
        => Dispose();

    private void HookXamlRoot(XamlRoot? xamlRoot)
    {
        if (ReferenceEquals(_xamlRoot, xamlRoot))
            return;

        if (_xamlRoot is not null)
            _xamlRoot.Changed -= OnXamlRootChanged;

        _xamlRoot = xamlRoot;
        if (_xamlRoot is not null && Volatile.Read(ref _disposed) == 0)
            _xamlRoot.Changed += OnXamlRootChanged;
    }

    private void QueueUpdate()
    {
        if (Volatile.Read(ref _disposed) != 0
            || Interlocked.Exchange(ref _updateQueued, 1) != 0)
            return;

        if (!_window.DispatcherQueue.TryEnqueue(() =>
            {
                Interlocked.Exchange(ref _updateQueued, 0);
                if (Volatile.Read(ref _disposed) != 0)
                    return;

                HookXamlRoot(_titleBar.XamlRoot);
                ApplyPadding();
            }))
        {
            Interlocked.Exchange(ref _updateQueued, 0);
        }
    }

    private void ApplyPadding()
    {
        if (_xamlRoot is null)
            return;

        AppWindowTitleBar appTitleBar = _appWindow.TitleBar;
        TitleBarPadding padding = TitleBarInsetPolicy.Calculate(
            _basePadding.Left,
            _basePadding.Right,
            appTitleBar.LeftInset,
            appTitleBar.RightInset,
            _xamlRoot.RasterizationScale);
        var next = new Thickness(
            padding.Left,
            _basePadding.Top,
            padding.Right,
            _basePadding.Bottom);

        if (_titleBar.Padding != next)
            _titleBar.Padding = next;
    }
}
