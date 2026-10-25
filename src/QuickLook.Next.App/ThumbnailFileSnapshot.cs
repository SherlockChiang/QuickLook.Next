using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal readonly record struct ThumbnailFileSnapshot(string Path, long ModifiedTicks, long Length, bool IsLocal);

internal static class ThumbnailFileSnapshots
{
    internal const int MaxBatchSize = 64;
    private static readonly SemaphoreSlim Gate = new(1, 1);

    public static async Task<IReadOnlyDictionary<string, ThumbnailFileSnapshot>> ReadAsync(
        IEnumerable<string> paths, CancellationToken token, Func<string, ThumbnailFileSnapshot>? read = null)
    {
        string[] batch = paths.Distinct(StringComparer.OrdinalIgnoreCase).Take(MaxBatchSize).ToArray();
        await Gate.WaitAsync(token).ConfigureAwait(false);
        try
        {
            return await Task.Run<IReadOnlyDictionary<string, ThumbnailFileSnapshot>>(() =>
            {
                var result = new Dictionary<string, ThumbnailFileSnapshot>(StringComparer.OrdinalIgnoreCase);
                foreach (string path in batch)
                {
                    token.ThrowIfCancellationRequested();
                    result[path] = (read ?? Read)(path);
                }
                token.ThrowIfCancellationRequested();
                return result;
            }, token).ConfigureAwait(false);
        }
        finally { Gate.Release(); }
    }

    private static ThumbnailFileSnapshot Read(string path)
    {
        try
        {
            if (CloudFileStatus.MayRequireHydration(path)) return new(path, 0, 0, false);
            var info = new FileInfo(path);
            return new(path, info.LastWriteTimeUtc.Ticks, info.Length, true);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return new(path, 0, 0, false);
        }
    }
}
