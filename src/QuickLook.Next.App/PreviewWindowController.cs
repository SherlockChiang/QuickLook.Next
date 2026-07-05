using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;

namespace QuickLook.Next.App;

internal sealed class PreviewWindowController
{
    private readonly Window _window;
    private readonly Func<nint> _hwndProvider;

    public PreviewWindowController(Window window, Func<nint> hwndProvider)
    {
        _window = window;
        _hwndProvider = hwndProvider;
    }

    public void Raise(bool activate)
    {
        nint hwnd = _hwndProvider();
        uint flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
        if (!activate)
            flags |= SWP_NOACTIVATE;

        SetNoActivateStyle(enabled: false);
        PulseTopmost(hwnd, flags);
        if (activate)
            _window.Activate();
    }

    public void ReleaseTopmost()
    {
        SetWindowPos(
            _hwndProvider(),
            HWND_NOTOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }

    public void SetNoActivateStyle(bool enabled)
    {
        nint hwnd = _hwndProvider();
        nint ex = GetWindowLongPtr(hwnd, GWL_EXSTYLE);
        nint next = enabled ? ex | WS_EX_NOACTIVATE : ex & ~WS_EX_NOACTIVATE;
        if (next != ex)
            SetWindowLongPtr(hwnd, GWL_EXSTYLE, next);
    }

    public void ShowNoActivate()
    {
        nint hwnd = _hwndProvider();
        SetNoActivateStyle(enabled: false);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        PulseTopmost(hwnd, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
    }

    public void Hide()
        => ShowWindow(_hwndProvider(), SW_HIDE);

    private static void PulseTopmost(nint hwnd, uint flags)
    {
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
        SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
    }

    private const int GWL_EXSTYLE = -20;
    private const int SW_HIDE = 0;
    private const int SW_SHOWNOACTIVATE = 4;
    private const uint SWP_NOSIZE = 0x0001;
    private const uint SWP_NOMOVE = 0x0002;
    private const uint SWP_NOACTIVATE = 0x0010;
    private const uint SWP_SHOWWINDOW = 0x0040;
    private static readonly nint HWND_NOTOPMOST = new(-2);
    private static readonly nint HWND_TOPMOST = new(-1);
    private static readonly nint WS_EX_NOACTIVATE = new(0x08000000);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW", SetLastError = true)]
    private static extern nint GetWindowLongPtr(nint hWnd, int nIndex);

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW", SetLastError = true)]
    private static extern nint SetWindowLongPtr(nint hWnd, int nIndex, nint dwNewLong);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool ShowWindow(nint hWnd, int nCmdShow);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetWindowPos(nint hWnd, nint hWndInsertAfter, int x, int y, int cx, int cy, uint flags);

}
