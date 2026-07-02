using System.Text;
using System.Threading.Channels;

namespace QuickLook.Next.Core;

/// <summary>Minimal file logger for bring-up diagnostics. (Replace with proper logging later.)</summary>
public static class DiagLog
{
    private static readonly Channel<string> Lines = Channel.CreateUnbounded<string>(
        new UnboundedChannelOptions { SingleReader = true, SingleWriter = false });
    private static readonly object StartLock = new();
    private static string _path = "";
    private static Task? _writerTask;

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
        EnsureWriterStarted();
    }

    public static void Write(string tag, string message)
    {
        if (_path.Length == 0) return;
        string line = $"{DateTime.Now:HH:mm:ss.fff} [{tag}] {message}";
        Lines.Writer.TryWrite(line);
    }

    private static void EnsureWriterStarted()
    {
        lock (StartLock)
        {
            _writerTask ??= Task.Run(WriteLoopAsync);
        }
    }

    private static async Task WriteLoopAsync()
    {
        var batch = new List<string>(128);
        while (await Lines.Reader.WaitToReadAsync().ConfigureAwait(false))
        {
            batch.Clear();
            while (batch.Count < 128 && Lines.Reader.TryRead(out string? line))
                batch.Add(line);
            if (batch.Count == 0)
                continue;

            try
            {
                string path = _path;
                if (path.Length == 0)
                    continue;

                var text = new StringBuilder();
                foreach (string line in batch)
                    text.AppendLine(line);
                await File.AppendAllTextAsync(path, text.ToString()).ConfigureAwait(false);
            }
            catch
            {
                // Diagnostics must never take the app down.
            }
        }
    }
}
