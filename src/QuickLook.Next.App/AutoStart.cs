using Microsoft.Win32;
using System.Runtime.InteropServices;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

/// <summary>Run-at-login via HKCU Run, with the user's Startup folder as a fallback.</summary>
internal static class AutoStart
{
    private const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    private const string ValueName = "QuickLookNext";
    private const string ShortcutName = "QuickLook Next.lnk";

    public static bool IsEnabled()
    {
        string exe = CurrentExePath();
        if (exe.Length == 0)
            return false;

        if (IsRunValueEnabled(exe))
            return true;

        return IsShortcutEnabled(exe);
    }

    public static bool SetEnabled(bool enabled)
    {
        try
        {
            if (enabled)
            {
                string exe = CurrentExePath();
                if (exe.Length == 0 || !File.Exists(exe))
                    return false;

                if (TrySetRunValue(exe))
                {
                    TryDeleteStartupShortcut();
                    DiagLog.Write("App", "autostart enabled via HKCU Run");
                    return true;
                }

                if (TryCreateStartupShortcut(exe))
                {
                    DiagLog.Write("App", $"autostart enabled via Startup shortcut fallback: {ShortcutPath}");
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

    public static void RepairIfConfigured()
    {
        try
        {
            if (!HasAnyEntry() || IsEnabled())
                return;

            DiagLog.Write("App", "autostart entry exists but points elsewhere; repairing");
            SetEnabled(enabled: true);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "autostart repair failed: " + ex.Message);
        }
    }

    private static string ShortcutPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.Startup),
        ShortcutName);

    private static string CurrentExePath()
        => Environment.ProcessPath ?? "";

    private static bool IsRunValueEnabled(string exe)
    {
        try
        {
            using RegistryKey? key = Registry.CurrentUser.OpenSubKey(RunKey);
            if (key?.GetValue(ValueName) is not string value)
                return false;

            return CommandTargetsPath(value, exe);
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "HKCU Run autostart read failed: " + ex.Message);
            return false;
        }
    }

    private static bool HasAnyEntry()
    {
        if (File.Exists(ShortcutPath))
            return true;

        try
        {
            using RegistryKey? key = Registry.CurrentUser.OpenSubKey(RunKey);
            return key?.GetValue(ValueName) is string;
        }
        catch
        {
            return false;
        }
    }

    private static bool IsShortcutEnabled(string exe)
    {
        if (!File.Exists(ShortcutPath))
            return false;

        string? target = TryGetStartupShortcutTarget();
        return !string.IsNullOrWhiteSpace(target)
            && string.Equals(Path.GetFullPath(target), Path.GetFullPath(exe), StringComparison.OrdinalIgnoreCase);
    }

    private static bool CommandTargetsPath(string command, string exe)
    {
        string normalized = command.Trim();
        if (normalized.StartsWith('"'))
        {
            int endQuote = normalized.IndexOf('"', 1);
            if (endQuote > 1)
                normalized = normalized[1..endQuote];
        }
        else
        {
            int firstSpace = normalized.IndexOf(' ');
            if (firstSpace > 0)
                normalized = normalized[..firstSpace];
        }

        try
        {
            return string.Equals(Path.GetFullPath(normalized), Path.GetFullPath(exe), StringComparison.OrdinalIgnoreCase);
        }
        catch
        {
            return false;
        }
    }

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

    private static string? TryGetStartupShortcutTarget()
    {
        try
        {
            Type? shellType = Type.GetTypeFromProgID("WScript.Shell");
            if (shellType is null)
                return null;

            object? shell = null;
            object? shortcut = null;
            try
            {
                shell = Activator.CreateInstance(shellType);
                if (shell is null) return null;
                shortcut = shellType.InvokeMember(
                    "CreateShortcut",
                    System.Reflection.BindingFlags.InvokeMethod,
                    null,
                    shell,
                    [ShortcutPath]);
                if (shortcut is null) return null;

                object? target = shortcut.GetType().InvokeMember(
                    "TargetPath",
                    System.Reflection.BindingFlags.GetProperty,
                    null,
                    shortcut,
                    []);
                return target as string;
            }
            finally
            {
                if (shortcut is not null && Marshal.IsComObject(shortcut)) Marshal.FinalReleaseComObject(shortcut);
                if (shell is not null && Marshal.IsComObject(shell)) Marshal.FinalReleaseComObject(shell);
            }
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "startup shortcut read failed: " + ex.Message);
            return null;
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
