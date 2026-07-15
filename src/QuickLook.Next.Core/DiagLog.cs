using System.Text;
using System.Threading.Channels;
using System.Diagnostics;

namespace QuickLook.Next.Core;

/// <summary>Minimal file logger for bring-up diagnostics. (Replace with proper logging later.)</summary>
public static class DiagLog
{
    private const int MaxQueuedLines = 4096;
    private const long MaxLogBytes = 4 * 1024 * 1024;
    private static readonly Channel<string> Lines = Channel.CreateBounded<string>(
        new BoundedChannelOptions(MaxQueuedLines)
        {
            SingleReader = true,
            SingleWriter = false,
            FullMode = BoundedChannelFullMode.DropOldest,
        });
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

    public static void InitInDirectory(string directory, string fileName)
    {
        if (string.IsNullOrWhiteSpace(fileName)
            || !string.Equals(fileName, Path.GetFileName(fileName), StringComparison.Ordinal))
            throw new ArgumentException("Log file name must not contain a path.", nameof(fileName));
        Directory.CreateDirectory(directory);
        _path = Path.Combine(directory, fileName);
        try { File.Delete(_path); } catch { }
        EnsureWriterStarted();
    }

    public static void Write(string tag, string message)
    {
        if (_path.Length == 0) return;
        string line = $"{DateTime.Now:HH:mm:ss.fff} [{tag}] {DiagnosticsRedactor.RedactPaths(message)}";
        Lines.Writer.TryWrite(line);
    }

    public static IDisposable TraceScope(string tag, string operation, int slowThresholdMs = 50)
    {
        Write(tag, operation + " begin");
        return new Scope(tag, operation, slowThresholdMs);
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
                try
                {
                    if (new FileInfo(path).Length >= MaxLogBytes)
                    {
                        string previous = path + ".previous";
                        File.Delete(previous);
                        File.Move(path, previous);
                    }
                }
                catch { }

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

    private sealed class Scope : IDisposable
    {
        private readonly string _tag;
        private readonly string _operation;
        private readonly int _slowThresholdMs;
        private readonly Stopwatch _watch = Stopwatch.StartNew();
        private bool _disposed;

        public Scope(string tag, string operation, int slowThresholdMs)
        {
            _tag = tag;
            _operation = operation;
            _slowThresholdMs = slowThresholdMs;
        }

        public void Dispose()
        {
            if (_disposed)
                return;

            _disposed = true;
            _watch.Stop();
            string suffix = _watch.ElapsedMilliseconds >= _slowThresholdMs ? " slow" : "";
            Write(_tag, $"{_operation} end {ElapsedText(_watch.Elapsed)}{suffix}");
        }
    }

    private static string ElapsedText(TimeSpan elapsed)
        => elapsed.TotalMilliseconds < 1000
            ? $"{elapsed.TotalMilliseconds:0}ms"
            : $"{elapsed.TotalSeconds:0.000}s";
}
