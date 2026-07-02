using System.Runtime.InteropServices;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class TrayIconManager
{
    private readonly nint _hwnd;
    private readonly Func<string> _resolveIconPath;
    private readonly Action _showPreview;
    private readonly Action<int, int> _showTrayMenu;
    private readonly Action<string> _setStatus;

    private bool _trayIconAdded;
    private WndProcDelegate? _wndProc;
    private nint _oldWndProc;
    private nint _trayIconHandle;
    private bool _ownsTrayIconHandle;
    private string? _trayIconPath;

    public TrayIconManager(
        nint hwnd,
        Func<string> resolveIconPath,
        Action showPreview,
        Action<int, int> showTrayMenu,
        Action<string> setStatus)
    {
        _hwnd = hwnd;
        _resolveIconPath = resolveIconPath;
        _showPreview = showPreview;
        _showTrayMenu = showTrayMenu;
        _setStatus = setStatus;
    }

    public void Ensure()
    {
        if (_trayIconAdded) return;

        var data = CreateNotifyIconData();
        _trayIconAdded = Shell_NotifyIcon(NIM_ADD, ref data);
    }

    public void Remove()
    {
        if (_trayIconAdded)
        {
            var data = CreateNotifyIconData();
            Shell_NotifyIcon(NIM_DELETE, ref data);
            _trayIconAdded = false;
        }

        ReleaseTrayIconHandle();
    }

    public void Refresh()
    {
        if (!_trayIconAdded)
            return;

        ReleaseTrayIconHandle();
        var data = CreateNotifyIconData();
        Shell_NotifyIcon(NIM_MODIFY, ref data);
    }

    public void ShowBalloon(string title, string message)
    {
        if (!_trayIconAdded)
            return;

        var data = CreateNotifyIconData();
        data.uFlags |= NIF_INFO;
        data.szInfoTitle = title;
        data.szInfo = message;
        data.dwInfoFlags = NIIF_WARNING;
        Shell_NotifyIcon(NIM_MODIFY, ref data);
    }

    private NOTIFYICONDATA CreateNotifyIconData()
    {
        EnsureTrayWndProc();
        return new NOTIFYICONDATA
        {
            cbSize = (uint)Marshal.SizeOf<NOTIFYICONDATA>(),
            hWnd = _hwnd,
            uID = 1,
            uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage = WM_TRAYICON,
            hIcon = GetTrayIconHandle(),
            szTip = "QuickLook Next",
            szInfo = "",
            szInfoTitle = "",
        };
    }

    private nint GetTrayIconHandle()
    {
        string iconPath = _resolveIconPath();
        if (_trayIconHandle != nint.Zero && string.Equals(_trayIconPath, iconPath, StringComparison.OrdinalIgnoreCase))
            return _trayIconHandle;

        ReleaseTrayIconHandle();
        if (File.Exists(iconPath))
        {
            _trayIconHandle = LoadImage(nint.Zero, iconPath, IMAGE_ICON, 0, 0, LR_LOADFROMFILE | LR_DEFAULTSIZE);
            _ownsTrayIconHandle = _trayIconHandle != nint.Zero;
            if (_ownsTrayIconHandle)
                _trayIconPath = iconPath;
        }

        if (_trayIconHandle == nint.Zero)
        {
            _trayIconHandle = LoadIcon(nint.Zero, IDI_APPLICATION);
            _ownsTrayIconHandle = false;
            _trayIconPath = null;
        }

        return _trayIconHandle;
    }

    private void ReleaseTrayIconHandle()
    {
        if (_ownsTrayIconHandle && _trayIconHandle != nint.Zero)
            DestroyIcon(_trayIconHandle);

        _trayIconHandle = nint.Zero;
        _ownsTrayIconHandle = false;
        _trayIconPath = null;
    }

    private void EnsureTrayWndProc()
    {
        if (_wndProc is not null) return;
        _wndProc = TrayWndProc;
        _oldWndProc = SetWindowLongPtr(_hwnd, GWLP_WNDPROC, Marshal.GetFunctionPointerForDelegate(_wndProc));
    }

    private nint TrayWndProc(nint hwnd, uint msg, nint wParam, nint lParam)
    {
        if (msg == WM_TRAYICON)
        {
            if (lParam == WM_LBUTTONDBLCLK)
                _showPreview();
            else if (lParam == WM_RBUTTONUP)
                ShowTrayMenu();
            return nint.Zero;
        }

        return CallWindowProc(_oldWndProc, hwnd, msg, wParam, lParam);
    }

    private void ShowTrayMenu()
    {
        GetCursorPos(out POINT pt);
        _showTrayMenu(pt.X, pt.Y);
    }

    public void ToggleAutoStart()
    {
        bool enable = !AutoStart.IsEnabled();
        if (AutoStart.SetEnabled(enable))
            return;

        string message = enable ? "开机自启开启失败" : "开机自启关闭失败";
        _setStatus(message);
        DiagLog.Write("App", message);
        ShowBalloon("QuickLook Next", message);
    }

    private delegate nint WndProcDelegate(nint hwnd, uint msg, nint wParam, nint lParam);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NOTIFYICONDATA
    {
        public uint cbSize;
        public nint hWnd;
        public uint uID;
        public uint uFlags;
        public uint uCallbackMessage;
        public nint hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string szTip;
        public uint dwState;
        public uint dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)]
        public string szInfo;
        public uint uVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)]
        public string szInfoTitle;
        public uint dwInfoFlags;
        public Guid guidItem;
        public nint hBalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct POINT { public int X; public int Y; }

    private const int GWLP_WNDPROC = -4;
    private const uint IMAGE_ICON = 1;
    private const uint NIM_ADD = 0x00000000;
    private const uint NIM_MODIFY = 0x00000001;
    private const uint NIM_DELETE = 0x00000002;
    private const uint NIF_MESSAGE = 0x00000001;
    private const uint NIF_ICON = 0x00000002;
    private const uint NIF_TIP = 0x00000004;
    private const uint NIF_INFO = 0x00000010;
    private const uint NIIF_WARNING = 0x00000002;
    private const uint WM_APP = 0x8000;
    private const uint WM_TRAYICON = WM_APP + 101;
    private const uint LR_LOADFROMFILE = 0x00000010;
    private const uint LR_DEFAULTSIZE = 0x00000040;
    private static readonly nint IDI_APPLICATION = new(32512);
    private static readonly nint WM_LBUTTONDBLCLK = new(0x0203);
    private static readonly nint WM_RBUTTONUP = new(0x0205);

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW", SetLastError = true)]
    private static extern nint SetWindowLongPtr(nint hWnd, int nIndex, nint dwNewLong);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern nint CallWindowProc(nint lpPrevWndFunc, nint hWnd, uint msg, nint wParam, nint lParam);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool Shell_NotifyIcon(uint dwMessage, ref NOTIFYICONDATA lpData);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern nint LoadIcon(nint hInstance, nint lpIconName);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern nint LoadImage(nint hinst, string lpszName, uint uType, int cxDesired, int cyDesired, uint fuLoad);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyIcon(nint hIcon);

    [DllImport("user32.dll")] private static extern bool GetCursorPos(out POINT lpPoint);
}
