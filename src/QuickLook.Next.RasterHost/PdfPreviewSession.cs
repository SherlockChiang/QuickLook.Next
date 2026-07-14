using System.Diagnostics;
using System.Security.Cryptography;
using System.Threading.Channels;
using Windows.Data.Pdf;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;
using QuickLook.Next.Core;
using WinColor = Windows.UI.Color;
using WinSize = Windows.Foundation.Size;

namespace QuickLook.Next.RasterHost;

internal sealed class PdfPreviewSession : IAsyncDisposable
{
    private const double MaxRenderDimension = 2200.0;
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
    private static readonly Channel<DiskCacheWrite> DiskCacheWrites = Channel.CreateBounded<DiskCacheWrite>(
        new BoundedChannelOptions(16)
        {
            SingleReader = true,
            SingleWriter = false,
            FullMode = BoundedChannelFullMode.DropWrite,
        });
    private static readonly Task DiskCacheWriter = Task.Run(ProcessDiskCacheWritesAsync);
    private static bool _diskCacheInitialized;
    private static long _diskCacheBytes;

    private sealed record DiskCacheWrite(string Key, byte[] Bgra, int Width, int Height);

    private sealed class CachedPage(byte[] bgra, int w, int h, LinkedListNode<string> node)
    {
        public byte[] Bgra { get; } = bgra;
        public int W { get; } = w;
        public int H { get; } = h;
        public LinkedListNode<string> Node { get; } = node;
    }

    private PdfDocument? _document;
    private readonly CancellationTokenSource _disposeCts = new();
    private readonly SemaphoreSlim _documentRenderLock = new(1, 1);
    private readonly object _lifetimeLock = new();
    private readonly long _mtimeTicks;
    private bool _disposed;
    private int _activeOperations;
    private TaskCompletionSource? _operationsDrained;

    private PdfPreviewSession(string path, PdfDocument document, WinSize firstPageSize, PdfPageGeometry[]? pageGeometries, long mtimeTicks)
    {
        Path = path;
        _document = document;
        FirstPageSize = firstPageSize;
        PageGeometries = pageGeometries;
        _mtimeTicks = mtimeTicks;
    }

    public string Path { get; }
    public uint PageCount => Document.PageCount;
    public WinSize FirstPageSize { get; }
    public PdfPageGeometry[]? PageGeometries { get; }

    private PdfDocument Document => _document ?? throw new ObjectDisposedException(nameof(PdfPreviewSession));

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
        watch.Stop();
        DiagLog.Write("RasterHost", $"pdf open {watch.ElapsedMilliseconds}ms; pages={document.PageCount}; path={path}");
        return new PdfPreviewSession(path, document, first.Size, pageGeometries: null, mtime);
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

            using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
            Span<byte> header = stackalloc byte[8];
            stream.ReadExactly(header);
            int width = BitConverter.ToInt32(header[..4]);
            int height = BitConverter.ToInt32(header[4..]);
            long expectedBytes = (long)width * height * 4;
            if (width <= 0 || height <= 0 || expectedBytes > int.MaxValue || stream.Length - 8 != expectedBytes)
            {
                result = default;
                return false;
            }

            var bgra = new byte[(int)expectedBytes];
            stream.ReadExactly(bgra);
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
            DiskCacheWrites.Writer.TryWrite(new DiskCacheWrite(key, bgra, w, h));
    }

    private static async Task ProcessDiskCacheWritesAsync()
    {
        InitializeDiskCache();
        await foreach (DiskCacheWrite write in DiskCacheWrites.Reader.ReadAllAsync())
            StoreDiskCached(write);
    }

    private static void InitializeDiskCache()
    {
        if (_diskCacheInitialized)
            return;

        _diskCacheInitialized = true;
        try
        {
            var directory = new DirectoryInfo(DiskCacheDirectory);
            _diskCacheBytes = directory.Exists
                ? directory.GetFiles("*.bgra").Sum(file => file.Length)
                : 0;
            if (_diskCacheBytes > MaxDiskCacheBytes)
                TrimDiskCache();
        }
        catch
        {
            _diskCacheBytes = 0;
        }
    }

    private static void StoreDiskCached(DiskCacheWrite write)
    {
        string? temporaryPath = null;
        try
        {
            Directory.CreateDirectory(DiskCacheDirectory);
            string path = DiskCachePath(write.Key);
            long previousLength = 0;
            try { previousLength = new FileInfo(path).Length; } catch { }
            temporaryPath = path + $".{Environment.ProcessId}.{Guid.NewGuid():N}.tmp";
            using var stream = new FileStream(
                temporaryPath,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                64 * 1024,
                FileOptions.SequentialScan);
            Span<byte> header = stackalloc byte[8];
            BitConverter.TryWriteBytes(header[..4], write.Width);
            BitConverter.TryWriteBytes(header[4..], write.Height);
            stream.Write(header);
            stream.Write(write.Bgra);
            stream.Flush(flushToDisk: false);
            stream.Close();
            File.Move(temporaryPath, path, overwrite: true);
            temporaryPath = null;
            _diskCacheBytes = Math.Max(0, _diskCacheBytes - previousLength + 8L + write.Bgra.LongLength);
            if (_diskCacheBytes > MaxDiskCacheBytes)
                TrimDiskCache();
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", "pdf disk cache write failed: " + ex.Message);
        }
        finally
        {
            if (temporaryPath is not null)
            {
                try { File.Delete(temporaryPath); } catch { }
            }
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
        foreach (FileInfo file in files.OrderBy(file => file.LastAccessTimeUtc))
        {
            if (_diskCacheBytes <= MaxDiskCacheBytes)
                break;
            try
            {
                long length = file.Length;
                file.Delete();
                _diskCacheBytes = Math.Max(0, _diskCacheBytes - length);
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
    public Task<(byte[] Bgra, int Width, int Height)> RenderPageAsync(
        int pageIndex,
        double scale,
        CancellationToken cancellationToken)
    {
        EnterOperation();
        return RenderPageWithLifetimeAsync(pageIndex, scale, cancellationToken);
    }

    private async Task<(byte[] Bgra, int Width, int Height)> RenderPageWithLifetimeAsync(
        int pageIndex,
        double scale,
        CancellationToken cancellationToken)
    {
        try
        {
            return await RenderPageOperationAsync(pageIndex, scale, cancellationToken);
        }
        finally
        {
            ExitOperation();
        }
    }

    private async Task<(byte[] Bgra, int Width, int Height)> RenderPageOperationAsync(
        int pageIndex,
        double scale,
        CancellationToken cancellationToken)
    {
        PdfDocument document = Document;
        if (pageIndex < 0 || pageIndex >= document.PageCount)
            throw new ArgumentOutOfRangeException(nameof(pageIndex));

        using var linkedCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _disposeCts.Token);
        CancellationToken token = linkedCts.Token;

        // Single GetPage call: read size and render from the same page object (was 2× GetPage before).
        token.ThrowIfCancellationRequested();
        using PdfPage page = document.GetPage((uint)pageIndex);
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
            using PdfPage page = Document.GetPage((uint)pageIndex);
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

    private void EnterOperation()
    {
        lock (_lifetimeLock)
        {
            ObjectDisposedException.ThrowIf(_disposed, this);
            _activeOperations++;
        }
    }

    private void ExitOperation()
    {
        TaskCompletionSource? drained = null;
        lock (_lifetimeLock)
        {
            _activeOperations--;
            if (_disposed && _activeOperations == 0)
                drained = _operationsDrained;
        }
        drained?.TrySetResult();
    }

    public async ValueTask DisposeAsync()
    {
        Task? waitForOperations = null;
        lock (_lifetimeLock)
        {
            if (_disposed)
                return;
            _disposed = true;
            if (_activeOperations > 0)
            {
                _operationsDrained = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
                waitForOperations = _operationsDrained.Task;
            }
        }

        try { _disposeCts.Cancel(); } catch { }
        if (waitForOperations is not null)
            await waitForOperations.ConfigureAwait(false);

        _document = null;
        _documentRenderLock.Dispose();
        _disposeCts.Dispose();
    }
}
