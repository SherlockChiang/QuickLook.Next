using Microsoft.UI.Xaml.Media;

namespace QuickLook.Next.App;

internal sealed class ImageThumbnailCache(int capacity)
{
    private readonly Dictionary<CacheKey, ImageSource> _cache = new();
    private readonly LinkedList<CacheKey> _lru = new();

    public bool Contains(string path, int size) => _cache.ContainsKey(CreateKey(path, size));

    public bool TryGet(string path, int size, out ImageSource? source)
    {
        CacheKey key = CreateKey(path, size);
        if (_cache.TryGetValue(key, out source))
        {
            Touch(key);
            return true;
        }

        source = null;
        return false;
    }

    public void Add(string path, int size, ImageSource source)
    {
        CacheKey key = CreateKey(path, size);
        RemoveStaleVersions(key);
        if (_cache.ContainsKey(key))
        {
            _cache[key] = source;
            Touch(key);
            return;
        }

        while (_cache.Count >= capacity && _lru.First is { } first)
        {
            _cache.Remove(first.Value);
            _lru.RemoveFirst();
        }

        _cache[key] = source;
        _lru.AddLast(key);
    }

    public void Remove(string path)
    {
        foreach (CacheKey key in _cache.Keys.Where(key => key.Path.Equals(path, StringComparison.OrdinalIgnoreCase)).ToArray())
        {
            _cache.Remove(key);
            RemoveLruEntry(key);
        }
    }

    private void RemoveStaleVersions(CacheKey current)
    {
        foreach (CacheKey key in _cache.Keys.Where(key =>
                     key.Path.Equals(current.Path, StringComparison.OrdinalIgnoreCase)
                     && key.Size == current.Size
                     && key != current).ToArray())
        {
            _cache.Remove(key);
            RemoveLruEntry(key);
        }
    }

    private void Touch(CacheKey key)
    {
        RemoveLruEntry(key);
        _lru.AddLast(key);
    }

    private void RemoveLruEntry(CacheKey key)
    {
        for (LinkedListNode<CacheKey>? node = _lru.First; node is not null; node = node.Next)
        {
            if (node.Value != key)
                continue;

            _lru.Remove(node);
            return;
        }
    }

    private static CacheKey CreateKey(string path, int size)
    {
        try
        {
            var info = new FileInfo(path);
            return new CacheKey(path.ToUpperInvariant(), info.LastWriteTimeUtc.Ticks, info.Length, size);
        }
        catch
        {
            return new CacheKey(path.ToUpperInvariant(), 0, 0, size);
        }
    }

    private readonly record struct CacheKey(string Path, long ModifiedTicks, long Length, int Size);
}
