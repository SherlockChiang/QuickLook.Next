namespace QuickLook.Next.Core;

/// <summary>Minimal file logger for bring-up diagnostics. (Replace with proper logging later.)</summary>
public static class DiagLog
{
    private static string _path = "";

    /// <summary>
    /// Per-user writable log directory. Must NOT be the app's base directory: when packaged as MSIX the
    /// install dir (<c>Program Files\WindowsApps\…</c>) is read-only, so writes there fail silently and we
    /// get no log. <see cref="Environment.SpecialFolder.LocalApplicationData"/> is writable in both the
    /// packaged and unpackaged cases.
    /// </summary>
    public static string LogDirectory { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "QuickLookNext", "logs");

    /// <summary>Begin a fresh log. <paramref name="file"/> may be a full path or bare name; only the file
    /// name is used — the log always lands in <see cref="LogDirectory"/>.</summary>
    public static void Init(string file)
    {
        try { Directory.CreateDirectory(LogDirectory); } catch { /* ignore */ }
        _path = Path.Combine(LogDirectory, Path.GetFileName(file));
        try { File.Delete(_path); } catch { /* ignore */ }
    }

    public static void Write(string tag, string message)
    {
        if (_path.Length == 0) return;
        string line = $"{DateTime.Now:HH:mm:ss.fff} [{tag}] {message}";
        try { File.AppendAllText(_path, line + Environment.NewLine); } catch { /* ignore */ }
    }
}
