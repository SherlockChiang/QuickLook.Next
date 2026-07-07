using Microsoft.UI.Xaml.Media;

namespace QuickLook.Next.App;

internal sealed class ImageThumbnailCache(int capacity)
{
    private readonly Dictionary<string, ImageSource> _cache = new(StringComparer.OrdinalIgnoreCase);
    private readonly LinkedList<string> _lru = new();

    public bool Contains(string path) => _cache.ContainsKey(path);

    public bool TryGet(string path, out ImageSource? source)
    {
        if (_cache.TryGetValue(path, out source))
        {
            Touch(path);
            return true;
        }

        source = null;
        return false;
    }

    public void Add(string path, ImageSource source)
    {
        if (_cache.ContainsKey(path))
        {
            _cache[path] = source;
            Touch(path);
            return;
        }

        while (_cache.Count >= capacity && _lru.First is { } first)
        {
            _cache.Remove(first.Value);
            _lru.RemoveFirst();
        }

        _cache[path] = source;
        _lru.AddLast(path);
    }

    public void Remove(string path)
    {
        _cache.Remove(path);
        RemoveLruEntry(path);
    }

    private void Touch(string path)
    {
        RemoveLruEntry(path);
        _lru.AddLast(path);
    }

    private void RemoveLruEntry(string path)
    {
        for (LinkedListNode<string>? node = _lru.First; node is not null; node = node.Next)
        {
            if (!string.Equals(node.Value, path, StringComparison.OrdinalIgnoreCase))
                continue;

            _lru.Remove(node);
            return;
        }
    }
}
