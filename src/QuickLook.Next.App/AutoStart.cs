using Microsoft.Win32;
using System.Runtime.InteropServices;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

/// <summary>Run-at-login via the user's Startup folder, with HKCU Run as a fallback.</summary>
internal static class AutoStart
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "QuickLookNext";
    private const string ShortcutName = "QuickLook Next.lnk";

    public static bool IsEnabled()
    {
        if (File.Exists(ShortcutPath))
            return true;

        using RegistryKey? key = Registry.CurrentUser.OpenSubKey(RunKey);
        return key?.GetValue(ValueName) is string;
    }

    public static bool SetEnabled(bool enabled)
    {
        try
        {
            if (enabled)
            {
                string exe = Environment.ProcessPath ?? "";
                if (exe.Length == 0 || !File.Exists(exe))
                    return false;

                if (TryCreateStartupShortcut(exe))
                {
                    DeleteRunValue();
                    DiagLog.Write("App", $"autostart enabled via Startup shortcut: {ShortcutPath}");
                    return true;
                }

                if (TrySetRunValue(exe))
                {
                    DiagLog.Write("App", "autostart enabled via HKCU Run fallback");
                    return true;
                }

                return false;
            }

            TryDeleteStartupShortcut();
            DeleteRunValue();
            DiagLog.Write("App", "autostart disabled");
            return true;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "autostart update FAILED: " + ex);
            return false;
        }
    }

    private static string ShortcutPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.Startup),
        ShortcutName);

    private static bool TryCreateStartupShortcut(string exe)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(ShortcutPath)!);
            Type? shellType = Type.GetTypeFromProgID("WScript.Shell");
            if (shellType is null)
                return false;

            object? shell = null;
            object? shortcut = null;
            try
            {
                shell = Activator.CreateInstance(shellType);
                if (shell is null) return false;
                shortcut = shellType.InvokeMember(
                    "CreateShortcut",
                    System.Reflection.BindingFlags.InvokeMethod,
                    null,
                    shell,
                    [ShortcutPath]);
                if (shortcut is null) return false;

                Type shortcutType = shortcut.GetType();
                shortcutType.InvokeMember("TargetPath", System.Reflection.BindingFlags.SetProperty, null, shortcut, [exe]);
                shortcutType.InvokeMember("WorkingDirectory", System.Reflection.BindingFlags.SetProperty, null, shortcut, [Path.GetDirectoryName(exe) ?? ""]);
                shortcutType.InvokeMember("IconLocation", System.Reflection.BindingFlags.SetProperty, null, shortcut, [$"{exe},0"]);
                shortcutType.InvokeMember("Description", System.Reflection.BindingFlags.SetProperty, null, shortcut, ["QuickLook Next"]);
                shortcutType.InvokeMember("Save", System.Reflection.BindingFlags.InvokeMethod, null, shortcut, []);
                return File.Exists(ShortcutPath);
            }
            finally
            {
                if (shortcut is not null && Marshal.IsComObject(shortcut)) Marshal.FinalReleaseComObject(shortcut);
                if (shell is not null && Marshal.IsComObject(shell)) Marshal.FinalReleaseComObject(shell);
            }
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "startup shortcut creation failed: " + ex.Message);
            return false;
        }
    }

    private static void TryDeleteStartupShortcut()
    {
        try
        {
            if (File.Exists(ShortcutPath))
                File.Delete(ShortcutPath);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "startup shortcut delete failed: " + ex.Message);
        }
    }

    private static bool TrySetRunValue(string exe)
    {
        try
        {
            using RegistryKey? key = Registry.CurrentUser.CreateSubKey(RunKey);
            if (key is null) return false;
            key.SetValue(ValueName, $"\"{exe}\"");
            return true;
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "HKCU Run autostart failed: " + ex.Message);
            return false;
        }
    }

    private static void DeleteRunValue()
    {
        using RegistryKey? key = Registry.CurrentUser.CreateSubKey(RunKey);
        key?.DeleteValue(ValueName, throwOnMissingValue: false);
    }
}
