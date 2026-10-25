namespace QuickLook.Next.App;

internal sealed class ImageThumbnailCache<TSource>(int capacity) where TSource : class
{
    private readonly Dictionary<CacheKey, CacheEntry> _cache = new();
    private readonly LinkedList<CacheKey> _lru = new();

    public bool Contains(ThumbnailFileSnapshot file, int size) => _cache.ContainsKey(CreateKey(file, size));

    public bool TryGet(ThumbnailFileSnapshot file, int size, out TSource? source)
    {
        CacheKey key = CreateKey(file, size);
        if (_cache.TryGetValue(key, out CacheEntry? entry))
        {
            source = entry.Source;
            Touch(entry);
            return true;
        }

        source = null;
        return false;
    }

    public void Add(ThumbnailFileSnapshot file, int size, TSource source)
    {
        CacheKey key = CreateKey(file, size);
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

    private static CacheKey CreateKey(ThumbnailFileSnapshot file, int size)
        => new(file.Path.ToUpperInvariant(), file.ModifiedTicks, file.Length, size);

    private sealed class CacheEntry(TSource source, LinkedListNode<CacheKey> node)
    {
        public TSource Source { get; set; } = source;
        public LinkedListNode<CacheKey> Node { get; } = node;
    }

    private readonly record struct CacheKey(string Path, long ModifiedTicks, long Length, int Size);
}
