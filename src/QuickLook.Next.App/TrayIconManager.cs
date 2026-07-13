using System.Runtime.InteropServices;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class TrayIconManager
{
    private readonly nint _hwnd;
    private readonly Func<string> _resolveIconPath;
    private readonly Action _showPreview;
    private readonly Action _showSettings;
    private readonly Action _exitApp;
    private readonly Action<string> _setStatus;

    private bool _trayIconAdded;
    private WndProcDelegate? _wndProc;
    private nint _oldWndProc;
    private nint _trayIconHandle;
    private uint _taskbarCreatedMessage;
    private bool _ownsTrayIconHandle;
    private string? _trayIconPath;

    public TrayIconManager(
        nint hwnd,
        Func<string> resolveIconPath,
        Action showPreview,
        Action showSettings,
        Action exitApp,
        Action<string> setStatus)
    {
        _hwnd = hwnd;
        _resolveIconPath = resolveIconPath;
        _showPreview = showPreview;
        _showSettings = showSettings;
        _exitApp = exitApp;
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
            szTip = UiStrings.AppName,
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
        _taskbarCreatedMessage = RegisterWindowMessage("TaskbarCreated");
        _oldWndProc = SetWindowLongPtr(_hwnd, GWLP_WNDPROC, Marshal.GetFunctionPointerForDelegate(_wndProc));
    }

    private nint TrayWndProc(nint hwnd, uint msg, nint wParam, nint lParam)
    {
        if (_taskbarCreatedMessage != 0 && msg == _taskbarCreatedMessage)
        {
            _trayIconAdded = false;
            Ensure();
            return nint.Zero;
        }

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
        nint menu = CreatePopupMenu();
        if (menu == nint.Zero)
            return;

        try
        {
            AppendMenu(menu, MF_STRING, TrayCommandShowPreview, UiStrings.TrayShowPreview);
            AppendMenu(menu, MF_STRING, TrayCommandSettings, UiStrings.TraySettings);
            AppendMenu(menu, MF_STRING | (AutoStart.IsEnabled() ? MF_CHECKED : MF_UNCHECKED), TrayCommandAutoStart, UiStrings.TrayAutoStart);
            AppendMenu(menu, MF_SEPARATOR, UIntPtr.Zero, string.Empty);
            AppendMenu(menu, MF_STRING, TrayCommandExit, UiStrings.TrayExit);

            SetForegroundWindow(_hwnd);
            uint command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                pt.X,
                pt.Y,
                0,
                _hwnd,
                nint.Zero);
            PostMessage(_hwnd, WM_NULL, nint.Zero, nint.Zero);

            switch (command)
            {
                case TrayCommandShowPreviewValue:
                    _showPreview();
                    break;
                case TrayCommandAutoStartValue:
                    ToggleAutoStart();
                    break;
                case TrayCommandSettingsValue:
                    _showSettings();
                    break;
                case TrayCommandExitValue:
                    _exitApp();
                    break;
            }
        }
        finally
        {
            DestroyMenu(menu);
        }
    }

    public void ToggleAutoStart()
    {
        bool enable = !AutoStart.IsEnabled();
        if (AutoStart.SetEnabled(enable))
            return;

        string message = enable ? UiStrings.AutoStartEnableFailed : UiStrings.AutoStartDisableFailed;
        _setStatus(message);
        DiagLog.Write("App", message);
        ShowBalloon(UiStrings.AppName, message);
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
    private const uint WM_NULL = 0x0000;
    private const uint LR_LOADFROMFILE = 0x00000010;
    private const uint LR_DEFAULTSIZE = 0x00000040;
    private const uint MF_STRING = 0x00000000;
    private const uint MF_CHECKED = 0x00000008;
    private const uint MF_UNCHECKED = 0x00000000;
    private const uint MF_SEPARATOR = 0x00000800;
    private const uint TPM_RIGHTBUTTON = 0x0002;
    private const uint TPM_RETURNCMD = 0x0100;
    private const uint TrayCommandShowPreviewValue = 1;
    private const uint TrayCommandAutoStartValue = 2;
    private const uint TrayCommandExitValue = 3;
    private const uint TrayCommandSettingsValue = 4;
    private static readonly UIntPtr TrayCommandShowPreview = new(TrayCommandShowPreviewValue);
    private static readonly UIntPtr TrayCommandAutoStart = new(TrayCommandAutoStartValue);
    private static readonly UIntPtr TrayCommandExit = new(TrayCommandExitValue);
    private static readonly UIntPtr TrayCommandSettings = new(TrayCommandSettingsValue);
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

    [DllImport("user32.dll", SetLastError = true)]
    private static extern nint CreatePopupMenu();

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool AppendMenu(nint hMenu, uint uFlags, UIntPtr uIDNewItem, string lpNewItem);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyMenu(nint hMenu);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetForegroundWindow(nint hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern uint TrackPopupMenu(nint hMenu, uint uFlags, int x, int y, int nReserved, nint hWnd, nint prcRect);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool PostMessage(nint hWnd, uint msg, nint wParam, nint lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern uint RegisterWindowMessage(string lpString);
}
