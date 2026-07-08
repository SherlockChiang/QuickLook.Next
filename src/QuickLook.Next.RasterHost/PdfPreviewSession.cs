using Windows.Data.Pdf;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;
using WinColor = Windows.UI.Color;
using WinSize = Windows.Foundation.Size;

namespace QuickLook.Next.RasterHost;

internal sealed class PdfPreviewSession : IDisposable
{
    private const double MaxRenderDimension = 2200.0;
    private static readonly WinColor White = new() { A = 255, R = 255, G = 255, B = 255 };

    // Cross-session rendered-page cache: re-previewing the same PDF at the same zoom skips the
    // Windows.Data.Pdf render entirely (and the surface churn it drives). Keyed by path+mtime+page+dims,
    // bounded by total BGRA bytes with LRU eviction.
    private const long MaxCacheBytes = 128L * 1024 * 1024;
    private static readonly object _cacheLock = new();
    private static readonly Dictionary<string, CachedPage> _pageCache = new();
    private static readonly LinkedList<string> _cacheLru = new();
    private static long _cacheBytes;
    private static readonly object _inflightLock = new();
    private static readonly Dictionary<string, Task<(byte[] Bgra, int Width, int Height)>> _inflightRenders = new();
    private static readonly SemaphoreSlim RenderWorkerPool = new(2, 2);

    private sealed class CachedPage(byte[] bgra, int w, int h, LinkedListNode<string> node)
    {
        public byte[] Bgra { get; } = bgra;
        public int W { get; } = w;
        public int H { get; } = h;
        public LinkedListNode<string> Node { get; } = node;
    }

    private readonly PdfDocument _document;
    private readonly CancellationTokenSource _disposeCts = new();
    private readonly long _mtimeTicks;
    private bool _disposed;

    private PdfPreviewSession(string path, PdfDocument document, WinSize firstPageSize, long mtimeTicks)
    {
        Path = path;
        _document = document;
        FirstPageSize = firstPageSize;
        _mtimeTicks = mtimeTicks;
    }

    public string Path { get; }
    public uint PageCount => _document.PageCount;
    public WinSize FirstPageSize { get; }

    public static async Task<PdfPreviewSession> OpenAsync(string path)
    {
        StorageFile file = await StorageFile.GetFileFromPathAsync(path);
        long mtime = 0;
        try { mtime = new FileInfo(path).LastWriteTimeUtc.Ticks; } catch { }
        PdfDocument document = await PdfDocument.LoadFromFileAsync(file);
        if (document.PageCount == 0)
            throw new InvalidOperationException("PDF contains no pages.");

        using PdfPage first = document.GetPage(0);
        return new PdfPreviewSession(path, document, first.Size, mtime);
    }

    /// <summary>Drop all cached rendered pages (called on idle to return memory to the OS).</summary>
    public static void ClearCache()
    {
        lock (_cacheLock)
        {
            _pageCache.Clear();
            _cacheLru.Clear();
            _cacheBytes = 0;
        }
    }

    private static bool TryGetCached(string key, out (byte[] Bgra, int Width, int Height) result)
    {
        lock (_cacheLock)
        {
            if (_pageCache.TryGetValue(key, out var v))
            {
                _cacheLru.Remove(v.Node);
                _cacheLru.AddLast(v.Node);
                result = (v.Bgra, v.W, v.H);
                return true;
            }
        }
        result = default;
        return false;
    }

    private static void StoreCached(string key, byte[] bgra, int w, int h)
    {
        lock (_cacheLock)
        {
            if (_pageCache.TryGetValue(key, out var existing))
            {
                _cacheLru.Remove(existing.Node);
                _cacheLru.AddLast(existing.Node);
                return;
            }

            var node = _cacheLru.AddLast(key);
            _pageCache.Add(key, new CachedPage(bgra, w, h, node));
            _cacheBytes += bgra.LongLength;
            while (_cacheBytes > MaxCacheBytes && _cacheLru.Count > 1)
            {
                LinkedListNode<string>? oldNode = _cacheLru.First;
                if (oldNode is null)
                    break;

                _cacheLru.RemoveFirst();
                if (_pageCache.Remove(oldNode.Value, out var evicted))
                    _cacheBytes -= evicted.Bgra.LongLength;
            }
        }
    }

    private static Task<(byte[] Bgra, int Width, int Height)> GetOrStartInflight(
        string key,
        Func<Task<(byte[] Bgra, int Width, int Height)>> start)
    {
        lock (_inflightLock)
        {
            if (_inflightRenders.TryGetValue(key, out var existing))
                return existing;

            Task<(byte[] Bgra, int Width, int Height)> task = CompleteInflightAsync(key, start);
            _inflightRenders[key] = task;
            return task;
        }
    }

    private static async Task<(byte[] Bgra, int Width, int Height)> CompleteInflightAsync(
        string key,
        Func<Task<(byte[] Bgra, int Width, int Height)>> start)
    {
        try
        {
            var rendered = await start();
            StoreCached(key, rendered.Bgra, rendered.Width, rendered.Height);
            return rendered;
        }
        finally
        {
            lock (_inflightLock)
                _inflightRenders.Remove(key);
        }
    }

    /// <summary>
    /// Render one page directly to premultiplied BGRA. Uses BMP (uncompressed) as the intermediate
    /// format to eliminate the PNG compress/decompress round-trip — for a 2200×2800 page that's
    /// ~24MB of raw pixels vs CPU-expensive PNG codec that saves ~2-3MB we never persist.
    /// </summary>
    public async Task<(byte[] Bgra, int Width, int Height)> RenderPageAsync(int pageIndex, double scale, CancellationToken cancellationToken)
    {
        if (pageIndex < 0 || pageIndex >= _document.PageCount)
            throw new ArgumentOutOfRangeException(nameof(pageIndex));

        using var linkedCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _disposeCts.Token);
        CancellationToken token = linkedCts.Token;

        // Single GetPage call: read size and render from the same page object (was 2× GetPage before).
        await RenderWorkerPool.WaitAsync(token);
        try
        {
            token.ThrowIfCancellationRequested();
            using PdfPage page = _document.GetPage((uint)pageIndex);
            WinSize pageSize = page.Size;

            double targetScale = Math.Clamp(scale, 0.1, 4.0);
            targetScale = Math.Min(targetScale, MaxRenderDimension / Math.Max(pageSize.Width, pageSize.Height));
            targetScale = Math.Max(0.1, targetScale);
            uint targetW = Math.Max(1, (uint)Math.Round(pageSize.Width * targetScale));
            uint targetH = Math.Max(1, (uint)Math.Round(pageSize.Height * targetScale));

            string cacheKey = $"{Path}|{_mtimeTicks}|{pageIndex}|{targetW}x{targetH}";
            if (TryGetCached(cacheKey, out var cached)) return cached;

            return await GetOrStartInflight(cacheKey, () => RenderPageCoreAsync(_document, pageIndex, targetW, targetH))
                .WaitAsync(token);
        }
        finally
        {
            RenderWorkerPool.Release();
        }
    }

    private static async Task<(byte[] Bgra, int Width, int Height)> RenderPageCoreAsync(
        PdfDocument document,
        int pageIndex,
        uint targetW,
        uint targetH)
    {
        using PdfPage page = document.GetPage((uint)pageIndex);
        using var stream = new InMemoryRandomAccessStream();
        await page.RenderToStreamAsync(stream, new PdfPageRenderOptions
        {
            DestinationWidth = targetW,
            DestinationHeight = targetH,
            BackgroundColor = White,
        });

        stream.Seek(0);
        BitmapDecoder decoder = await BitmapDecoder.CreateAsync(stream);
        PixelDataProvider pixels = await decoder.GetPixelDataAsync(
            BitmapPixelFormat.Bgra8,
            BitmapAlphaMode.Premultiplied,
            new BitmapTransform(),
            ExifOrientationMode.IgnoreExifOrientation,
            ColorManagementMode.DoNotColorManage);

        byte[] bgra = pixels.DetachPixelData();
        return (bgra, (int)decoder.PixelWidth, (int)decoder.PixelHeight);
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        try { _disposeCts.Cancel(); } catch { }
        _disposeCts.Dispose();
    }
}
