using System.Diagnostics;
using System.Security.Cryptography;
using Windows.Data.Pdf;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;
using QuickLook.Next.Core;
using WinColor = Windows.UI.Color;
using WinSize = Windows.Foundation.Size;

namespace QuickLook.Next.RasterHost;

internal sealed class PdfPreviewSession : IDisposable
{
    private const double MaxRenderDimension = 2200.0;
    // Keep preview.ready comfortably below the bounded control-pipe message size.
    private const uint MaxPageGeometryCount = 256;
    private static readonly TimeSpan PageRenderTimeout = TimeSpan.FromSeconds(12);
    private const long MaxDiskCacheBytes = 256L * 1024 * 1024;
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
    private readonly SemaphoreSlim _documentRenderLock = new(1, 1);
    private readonly long _mtimeTicks;
    private bool _disposed;

    private PdfPreviewSession(string path, PdfDocument document, WinSize firstPageSize, PdfPageGeometry[]? pageGeometries, long mtimeTicks)
    {
        Path = path;
        _document = document;
        FirstPageSize = firstPageSize;
        PageGeometries = pageGeometries;
        _mtimeTicks = mtimeTicks;
    }

    public string Path { get; }
    public uint PageCount => _document.PageCount;
    public WinSize FirstPageSize { get; }
    public PdfPageGeometry[]? PageGeometries { get; }

    public static async Task<PdfPreviewSession> OpenAsync(string path)
    {
        var watch = Stopwatch.StartNew();
        StorageFile file = await StorageFile.GetFileFromPathAsync(path);
        long mtime = 0;
        try { mtime = new FileInfo(path).LastWriteTimeUtc.Ticks; } catch { }
        PdfDocument document = await PdfDocument.LoadFromFileAsync(file);
        if (document.PageCount == 0)
            throw new InvalidOperationException("PDF contains no pages.");

        using PdfPage first = document.GetPage(0);
        PdfPageGeometry[]? pageGeometries = TryGetPageGeometries(document);
        watch.Stop();
        DiagLog.Write("RasterHost", $"pdf open {watch.ElapsedMilliseconds}ms; pages={document.PageCount}; path={path}");
        return new PdfPreviewSession(path, document, first.Size, pageGeometries, mtime);
    }

    private static PdfPageGeometry[]? TryGetPageGeometries(PdfDocument document)
    {
        if (document.PageCount == 0 || document.PageCount > MaxPageGeometryCount)
            return null;

        try
        {
            var geometries = new PdfPageGeometry[checked((int)document.PageCount)];
            for (uint pageIndex = 0; pageIndex < document.PageCount; pageIndex++)
            {
                using PdfPage page = document.GetPage(pageIndex);
                WinSize size = page.Size;
                if (!double.IsFinite(size.Width) || !double.IsFinite(size.Height) || size.Width <= 0 || size.Height <= 0)
                    return null;
                geometries[pageIndex] = new PdfPageGeometry(size.Width, size.Height);
            }
            return geometries;
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", "pdf page geometry unavailable: " + ex.Message);
            return null;
        }
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

    private static bool TryGetDiskCached(string key, out (byte[] Bgra, int Width, int Height) result)
    {
        try
        {
            string path = DiskCachePath(key);
            if (!File.Exists(path))
            {
                result = default;
                return false;
            }

            byte[] bytes = File.ReadAllBytes(path);
            if (bytes.Length <= 8)
            {
                result = default;
                return false;
            }

            int width = BitConverter.ToInt32(bytes, 0);
            int height = BitConverter.ToInt32(bytes, 4);
            int pixelBytes = bytes.Length - 8;
            if (width <= 0 || height <= 0 || pixelBytes != width * height * 4)
            {
                result = default;
                return false;
            }

            var bgra = new byte[pixelBytes];
            System.Buffer.BlockCopy(bytes, 8, bgra, 0, pixelBytes);
            File.SetLastAccessTimeUtc(path, DateTime.UtcNow);
            result = (bgra, width, height);
            return true;
        }
        catch
        {
            result = default;
            return false;
        }
    }

    private static void StoreCached(string key, byte[] bgra, int w, int h, bool writeDisk = true)
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
        if (writeDisk)
            _ = Task.Run(() => StoreDiskCached(key, bgra, w, h));
    }

    private static void StoreDiskCached(string key, byte[] bgra, int width, int height)
    {
        try
        {
            Directory.CreateDirectory(DiskCacheDirectory);
            string path = DiskCachePath(key);
            using var stream = new FileStream(path, FileMode.Create, FileAccess.Write, FileShare.Read);
            Span<byte> header = stackalloc byte[8];
            BitConverter.TryWriteBytes(header[..4], width);
            BitConverter.TryWriteBytes(header[4..], height);
            stream.Write(header);
            stream.Write(bgra);
            TrimDiskCache();
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", "pdf disk cache write failed: " + ex.Message);
        }
    }

    private static string DiskCacheDirectory => System.IO.Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "QuickLook.Next",
        "pdf-page-cache");

    private static string DiskCachePath(string key)
    {
        byte[] hash = SHA256.HashData(System.Text.Encoding.UTF8.GetBytes(key));
        return System.IO.Path.Combine(DiskCacheDirectory, Convert.ToHexString(hash) + ".bgra");
    }

    private static void TrimDiskCache()
    {
        var directory = new DirectoryInfo(DiskCacheDirectory);
        FileInfo[] files = directory.Exists ? directory.GetFiles("*.bgra") : [];
        long total = files.Sum(file => file.Length);
        foreach (FileInfo file in files.OrderBy(file => file.LastAccessTimeUtc))
        {
            if (total <= MaxDiskCacheBytes)
                break;
            try
            {
                total -= file.Length;
                file.Delete();
            }
            catch { }
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
            await RenderWorkerPool.WaitAsync();
            var rendered = await start();
            StoreCached(key, rendered.Bgra, rendered.Width, rendered.Height);
            return rendered;
        }
        finally
        {
            RenderWorkerPool.Release();
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
        if (TryGetDiskCached(cacheKey, out cached))
        {
            StoreCached(cacheKey, cached.Bgra, cached.Width, cached.Height, writeDisk: false);
            return cached;
        }

        var waitWatch = Stopwatch.StartNew();
        var rendered = await GetOrStartInflight(cacheKey, () => RenderPageCoreAsync(pageIndex, targetW, targetH, token))
            .WaitAsync(PageRenderTimeout, token);
        waitWatch.Stop();
        DiagLog.Write("RasterHost", $"pdf page ready {waitWatch.ElapsedMilliseconds}ms; page={pageIndex}; size={rendered.Width}x{rendered.Height}; path={Path}");
        return rendered;
    }

    private async Task<(byte[] Bgra, int Width, int Height)> RenderPageCoreAsync(
        int pageIndex,
        uint targetW,
        uint targetH,
        CancellationToken cancellationToken)
    {
        await _documentRenderLock.WaitAsync(cancellationToken);
        try
        {
            cancellationToken.ThrowIfCancellationRequested();
            using PdfPage page = _document.GetPage((uint)pageIndex);
            using var stream = new InMemoryRandomAccessStream();
            var renderWatch = Stopwatch.StartNew();
            await page.RenderToStreamAsync(stream, new PdfPageRenderOptions
            {
                DestinationWidth = targetW,
                DestinationHeight = targetH,
                BackgroundColor = White,
            });
            renderWatch.Stop();
            DiagLog.Write("RasterHost", $"pdf page render {renderWatch.ElapsedMilliseconds}ms; page={pageIndex}; target={targetW}x{targetH}; path={Path}");

            stream.Seek(0);
            var decodeWatch = Stopwatch.StartNew();
            BitmapDecoder decoder = await BitmapDecoder.CreateAsync(stream);
            PixelDataProvider pixels = await decoder.GetPixelDataAsync(
                BitmapPixelFormat.Bgra8,
                BitmapAlphaMode.Premultiplied,
                new BitmapTransform(),
                ExifOrientationMode.IgnoreExifOrientation,
                ColorManagementMode.DoNotColorManage);

            byte[] bgra = pixels.DetachPixelData();
            decodeWatch.Stop();
            DiagLog.Write("RasterHost", $"pdf page bitmap decode {decodeWatch.ElapsedMilliseconds}ms; page={pageIndex}; bytes={bgra.Length}; path={Path}");
            return (bgra, (int)decoder.PixelWidth, (int)decoder.PixelHeight);
        }
        finally
        {
            _documentRenderLock.Release();
        }
    }

    public void Dispose()
    {
        if (_disposed) return;
        _disposed = true;
        try { _disposeCts.Cancel(); } catch { }
        _disposeCts.Dispose();
    }
}
