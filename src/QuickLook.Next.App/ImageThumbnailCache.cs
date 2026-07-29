using Microsoft.UI.Xaml.Media;

namespace QuickLook.Next.App;

internal sealed class ImageThumbnailCache(int capacity)
{
    private const int MaxMetadataItems = 512;
    private const long MetadataLifetimeMilliseconds = 1000;
    private readonly Dictionary<CacheKey, CacheEntry> _cache = new();
    private readonly LinkedList<CacheKey> _lru = new();
    private readonly Dictionary<string, MetadataEntry> _metadata = new(StringComparer.OrdinalIgnoreCase);

    public bool Contains(string path, int size) => _cache.ContainsKey(CreateKey(path, size));

    public bool TryGet(string path, int size, out ImageSource? source)
    {
        CacheKey key = CreateKey(path, size);
        if (_cache.TryGetValue(key, out CacheEntry? entry))
        {
            source = entry.Source;
            Touch(entry);
            return true;
        }

        source = null;
        return false;
    }

    public void Add(string path, int size, ImageSource source)
    {
        CacheKey key = CreateKey(path, size);
        RemoveStaleVersions(key);
        if (_cache.TryGetValue(key, out CacheEntry? existing))
        {
            existing.Source = source;
            Touch(existing);
            return;
        }

        while (_cache.Count >= capacity && _lru.First is { } first)
        {
            _cache.Remove(first.Value);
            _lru.RemoveFirst();
        }

        LinkedListNode<CacheKey> node = _lru.AddLast(key);
        _cache[key] = new CacheEntry(source, node);
    }

    public void Remove(string path)
    {
        _metadata.Remove(path);
        foreach (CacheKey key in _cache.Keys.Where(key => key.Path.Equals(path, StringComparison.OrdinalIgnoreCase)).ToArray())
        {
            RemoveEntry(key);
        }
    }

    private void RemoveStaleVersions(CacheKey current)
    {
        foreach (CacheKey key in _cache.Keys.Where(key =>
                     key.Path.Equals(current.Path, StringComparison.OrdinalIgnoreCase)
                     && key.Size == current.Size
                     && key != current).ToArray())
        {
            RemoveEntry(key);
        }
    }

    private void Touch(CacheEntry entry)
    {
        _lru.Remove(entry.Node);
        _lru.AddLast(entry.Node);
    }

    private void RemoveEntry(CacheKey key)
    {
        if (_cache.Remove(key, out CacheEntry? entry))
        {
            _lru.Remove(entry.Node);
        }
    }

    private CacheKey CreateKey(string path, int size)
    {
        long now = Environment.TickCount64;
        if (_metadata.TryGetValue(path, out MetadataEntry cached)
            && now - cached.CreatedAt < MetadataLifetimeMilliseconds)
        {
            return new CacheKey(cached.NormalizedPath, cached.ModifiedTicks, cached.Length, size);
        }

        string normalizedPath = path.ToUpperInvariant();
        long modifiedTicks = 0;
        long length = 0;
        try
        {
            var info = new FileInfo(path);
            modifiedTicks = info.LastWriteTimeUtc.Ticks;
            length = info.Length;
        }
        catch
        {
        }

        if (_metadata.Count >= MaxMetadataItems)
        {
            string oldest = _metadata.MinBy(static pair => pair.Value.CreatedAt).Key;
            _metadata.Remove(oldest);
        }
        _metadata[path] = new MetadataEntry(normalizedPath, modifiedTicks, length, now);
        return new CacheKey(normalizedPath, modifiedTicks, length, size);
    }

    private sealed class CacheEntry(ImageSource source, LinkedListNode<CacheKey> node)
    {
        public ImageSource Source { get; set; } = source;
        public LinkedListNode<CacheKey> Node { get; } = node;
    }

    private readonly record struct MetadataEntry(string NormalizedPath, long ModifiedTicks, long Length, long CreatedAt);
    private readonly record struct CacheKey(string Path, long ModifiedTicks, long Length, int Size);
}
