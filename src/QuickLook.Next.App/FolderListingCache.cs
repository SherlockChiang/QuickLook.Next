using QuickLook.Next.Contracts;

namespace QuickLook.Next.App;

internal sealed class FolderListingCache(
    Func<string, PreviewListing?> load,
    int capacity = 8,
    TimeSpan? lifetime = null)
{
    private readonly object _gate = new();
    private readonly Dictionary<string, Entry> _entries = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<string, TaskCompletionSource<PreviewListing?>> _inflight = new(StringComparer.OrdinalIgnoreCase);
    private readonly TimeSpan _lifetime = lifetime ?? TimeSpan.FromSeconds(3);

    public PreviewListing? Get(string path)
    {
        string key;
        try { key = Path.TrimEndingDirectorySeparator(Path.GetFullPath(path)); }
        catch { return load(path); }

        TaskCompletionSource<PreviewListing?>? pending;
        bool ownsLoad = false;
        lock (_gate)
        {
            long now = Environment.TickCount64;
            if (_entries.TryGetValue(key, out Entry? cached) && now - cached.CreatedAt < _lifetime.TotalMilliseconds)
            {
                cached.LastAccess = now;
                return cached.Listing;
            }
            _entries.Remove(key);
            if (!_inflight.TryGetValue(key, out pending))
            {
                pending = new TaskCompletionSource<PreviewListing?>(TaskCreationOptions.RunContinuationsAsynchronously);
                _inflight[key] = pending;
                ownsLoad = true;
            }
        }

        if (!ownsLoad)
            return pending!.Task.GetAwaiter().GetResult();

        try
        {
            PreviewListing? listing = load(path);
            lock (_gate)
            {
                if (listing is not null)
                {
                    _entries[key] = new Entry(listing, Environment.TickCount64);
                    while (_entries.Count > capacity)
                    {
                        string oldest = _entries.MinBy(static pair => pair.Value.LastAccess).Key;
                        _entries.Remove(oldest);
                    }
                }
                _inflight.Remove(key);
            }
            pending!.TrySetResult(listing);
            return listing;
        }
        catch (Exception ex)
        {
            lock (_gate) _inflight.Remove(key);
            pending!.TrySetException(ex);
            throw;
        }
    }

    private sealed class Entry(PreviewListing listing, long createdAt)
    {
        public PreviewListing Listing { get; } = listing;
        public long CreatedAt { get; } = createdAt;
        public long LastAccess { get; set; } = createdAt;
    }
}
