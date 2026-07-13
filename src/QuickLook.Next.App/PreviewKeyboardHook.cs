using System.Runtime.InteropServices;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class PreviewKeyboardHook : IDisposable
{
    private const uint WM_KEYDOWN = 0x0100;
    private const uint WM_KEYUP = 0x0101;
    private const uint WM_SYSKEYDOWN = 0x0104;
    private const uint WM_SYSKEYUP = 0x0105;
    private const int VK_SPACE = 0x20;
    private const int VK_SHIFT = 0x10;
    private const int VK_CONTROL = 0x11;
    private const int VK_MENU = 0x12;

    private readonly nint _hwnd;
    private readonly Func<bool> _shouldHandleSpace;
    private readonly Action _onSpace;
    private readonly SUBCLASSPROC _subclassProc;
    private readonly nuint _subclassId;
    private bool _installed;
    private bool _disposed;
    private bool _spaceDownHandled;

    public PreviewKeyboardHook(nint hwnd, Func<bool> shouldHandleSpace, Action onSpace)
    {
        _hwnd = hwnd;
        _shouldHandleSpace = shouldHandleSpace;
        _onSpace = onSpace;
        _subclassProc = WndProc;
        _subclassId = (nuint)GetHashCode();
        _installed = SetWindowSubclass(_hwnd, _subclassProc, _subclassId, nint.Zero);
        DiagLog.Write("App", $"keyboard hook install hwnd=0x{_hwnd:X}; installed={_installed}; lastError={Marshal.GetLastWin32Error()}");
    }

    public void Dispose()
    {
        if (_disposed)
            return;

        _disposed = true;
        if (_installed)
        {
            bool removed = RemoveWindowSubclass(_hwnd, _subclassProc, _subclassId);
            DiagLog.Write("App", $"keyboard hook remove hwnd=0x{_hwnd:X}; removed={removed}; lastError={Marshal.GetLastWin32Error()}");
            _installed = false;
        }
    }

    private nint WndProc(nint hwnd, uint msg, nint wParam, nint lParam, nuint subclassId, nint refData)
    {
        if ((msg == WM_KEYUP || msg == WM_SYSKEYUP) && wParam == VK_SPACE)
            _spaceDownHandled = false;

        if ((msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN || msg == WM_KEYUP || msg == WM_SYSKEYUP)
            && wParam == VK_SPACE
            && !ModifierKeyDown()
            && _shouldHandleSpace())
        {
            if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN)
            {
                if (!_spaceDownHandled)
                {
                    _spaceDownHandled = true;
                    DiagLog.Write("App", "keyboard hook handled Space down");
                    _onSpace();
                }
            }
            return 0;
        }

        return DefSubclassProc(hwnd, msg, wParam, lParam);
    }

    private static bool ModifierKeyDown()
        => KeyDown(VK_SHIFT) || KeyDown(VK_CONTROL) || KeyDown(VK_MENU);

    private static bool KeyDown(int key) => (GetKeyState(key) & 0x8000) != 0;

    private delegate nint SUBCLASSPROC(nint hWnd, uint uMsg, nint wParam, nint lParam, nuint uIdSubclass, nint dwRefData);

    [DllImport("comctl32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetWindowSubclass(nint hWnd, SUBCLASSPROC pfnSubclass, nuint uIdSubclass, nint dwRefData);

    [DllImport("comctl32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool RemoveWindowSubclass(nint hWnd, SUBCLASSPROC pfnSubclass, nuint uIdSubclass);

    [DllImport("comctl32.dll")]
    private static extern nint DefSubclassProc(nint hWnd, uint uMsg, nint wParam, nint lParam);

    [DllImport("user32.dll")]
    private static extern short GetKeyState(int nVirtKey);
}
