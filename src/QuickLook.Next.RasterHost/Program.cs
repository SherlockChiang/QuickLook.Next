using System.IO.Pipes;
using QuickLook.Next.Core;
using QuickLook.Next.RasterHost;

// RasterHost: the .NET surface process. It owns D3D shared-surface production plus Windows-only render
// bridges (PDF pages and shell thumbnails). Preview business logic should live in Rust or the App UI.

string pipeName = GetArg(args, "--pipe") ?? "quicklook_next";

DiagLog.Init(Path.Combine(AppContext.BaseDirectory, "raster-host.log"));
DiagLog.Write("RasterHost", $"start pid={Environment.ProcessId} pipe={pipeName}");
ProcessPowerMode.SetCurrentBackgroundEfficiency(enabled: true, "RasterHost");

using var pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
PipeChannel channel;
try
{
    await pipe.ConnectAsync(5000);
    channel = new PipeChannel(pipe);
    DiagLog.Write("RasterHost", "connected to App pipe");
}
catch (Exception ex) { DiagLog.Write("RasterHost", "pipe connect FAILED: " + ex); return; }

using var channelLifetime = channel;
using var producer = new CompositionProducer();
using var idleTrimmer = new IdleTrimmer(producer);
var pdfSessions = new Dictionary<string, PdfPreviewSession>();
var pdfPageRenderCts = new Dictionary<(string RequestId, int PageIndex), CancellationTokenSource>();
var openCts = new Dictionary<string, CancellationTokenSource>();
var openCtsLock = new object();
TimeSpan imageDecodeTimeout = TimeSpan.FromSeconds(8);
TimeSpan systemImageDecodeTimeout = TimeSpan.FromSeconds(2);

while (true)
{
    ControlMessage? msg;
    try { msg = await channel.ReceiveAsync(); }
    catch (Exception ex) { DiagLog.Write("RasterHost", "receive ended: " + ex.Message); break; }
    if (msg is null) { DiagLog.Write("RasterHost", "pipe EOF"); break; }
    idleTrimmer.Touch();
    DiagLog.Write("RasterHost", "recv " + msg.GetType().Name);

    try
    {
        switch (msg)
        {
            case Hello hello:
                producer.Initialize(hello.AppProcessId);
                await channel.SendAsync(new HostReady(producer.AdapterLuid));
                DiagLog.Write("RasterHost", $"initialized; sent host.ready");
                break;

            case PreviewOpen open:
                StartOpen(open);
                break;

        case PreviewResize resize:
            long rh = producer.CreateSurface(resize.Width, resize.Height);
            await channel.SendAsync(new PreviewSurface(
                resize.RequestId, rh, resize.Width, resize.Height, resize.Dpi, "B8G8R8A8_UNORM"));
            break;

        case PreviewPageOpen page:
            await HandlePageOpenAsync(page);
            break;

        case PreviewPageClose pageClose:
            CancelPageRender(pageClose.RequestId, pageClose.PageIndex);
            _ = Task.Delay(250).ContinueWith(_ => producer.ReleasePage(pageClose.PageIndex), TaskContinuationOptions.OnlyOnRanToCompletion);
            break;

            case PreviewClose close:
                CancelOpen(close.RequestId);
                if (pdfSessions.Remove(close.RequestId, out var pdf))
                    pdf.Dispose();
                foreach (var key in pdfPageRenderCts.Keys.Where(k => k.RequestId == close.RequestId).ToArray())
                {
                    if (pdfPageRenderCts.TryGetValue(key, out var cts))
                    {
                        try { cts.Cancel(); } catch (ObjectDisposedException) { }
                        pdfPageRenderCts.Remove(key);
                    }
                }
                producer.Retire(); // defer GPU surface release until the next open (avoids compositor AV)
                break;
        }
    }
    catch (Exception ex) { DiagLog.Write("RasterHost", "handler error: " + ex.Message); }
}

CancellationTokenSource[] remainingOpenCts;
lock (openCtsLock)
{
    remainingOpenCts = openCts.Values.ToArray();
    openCts.Clear();
}
foreach (var cts in remainingOpenCts)
{
    try { cts.Cancel(); } catch { }
}

void StartOpen(PreviewOpen open)
{
    string[] existing;
    lock (openCtsLock)
        existing = openCts.Keys.ToArray();
    foreach (string requestId in existing)
        CancelOpen(requestId);

    var cts = new CancellationTokenSource();
    lock (openCtsLock)
        openCts[open.RequestId] = cts;
    _ = Task.Run(async () =>
    {
        try
        {
            await HandleOpenAsync(open, cts.Token);
        }
        catch (OperationCanceledException)
        {
            DiagLog.Write("RasterHost", $"open canceled: request={open.RequestId}");
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", "open task ERROR: " + ex);
            try { await channel.SendAsync(new PreviewError(open.RequestId, ex.Message)); } catch { }
        }
        finally
        {
            lock (openCtsLock)
            {
                if (openCts.TryGetValue(open.RequestId, out var current) && ReferenceEquals(current, cts))
                    openCts.Remove(open.RequestId);
            }
            cts.Dispose();
        }
    });
}

void CancelOpen(string requestId)
{
    CancellationTokenSource? cts;
    lock (openCtsLock)
    {
        if (!openCts.Remove(requestId, out cts))
            return;
    }

    try { cts.Cancel(); } catch { }
}

void CancelPageRender(string requestId, int pageIndex)
{
    var key = (requestId, pageIndex);
    if (!pdfPageRenderCts.Remove(key, out var cts))
        return;

    try { cts.Cancel(); } catch (ObjectDisposedException) { }
}

async Task HandleOpenAsync(PreviewOpen open, CancellationToken cancellationToken)
{
    DiagLog.Write("RasterHost", $"open path={open.Path} ext={open.Probe.Extension} kind={open.Probe.Kind} size={open.Probe.Size}");
    _ = Task.Delay(250, cancellationToken).ContinueWith(_ => producer.ReleaseRetired(), TaskContinuationOptions.OnlyOnRanToCompletion);
    try
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (IsPdf(open.Probe))
        {
            if (pdfSessions.Remove(open.RequestId, out var old)) old.Dispose();
            var session = await PdfPreviewSession.OpenAsync(open.Path);
            cancellationToken.ThrowIfCancellationRequested();
            pdfSessions[open.RequestId] = session;
            var first = session.FirstPageSize;
            await channel.SendAsync(new PreviewReady(
                open.RequestId,
                "pdf",
                $"{Path.GetFileName(open.Path)} — {session.PageCount} pages",
                first.Width,
                first.Height)
            {
                PageCount = checked((int)session.PageCount),
                PageWidth = first.Width,
                PageHeight = first.Height,
            });
            return;
        }

        if (IsImage(open.Probe))
        {
            var image = await DecodeImageAsync(open.Path, imageDecodeTimeout, systemImageDecodeTimeout, cancellationToken);
            cancellationToken.ThrowIfCancellationRequested();
            if (image is not null)
            {
                DiagLog.Write("RasterHost", $"image raster {image.Width}x{image.Height} original={image.OriginalWidth}x{image.OriginalHeight}");
                long imageHandle = producer.CreatePresentedSurface(image.Bgra, image.Width, image.Height);
                cancellationToken.ThrowIfCancellationRequested();
                await channel.SendAsync(new PreviewSurface(
                    open.RequestId, imageHandle, (uint)image.Width, (uint)image.Height, 96.0, "B8G8R8A8_UNORM"));
                string title = image.Width == image.OriginalWidth && image.Height == image.OriginalHeight
                    ? Path.GetFileName(open.Path)
                    : $"{Path.GetFileName(open.Path)} — {image.OriginalWidth}x{image.OriginalHeight} scaled to {image.Width}x{image.Height}";
                await channel.SendAsync(new PreviewReady(open.RequestId, "image", title, image.Width, image.Height));
                return;
            }

            DiagLog.Write("RasterHost", "native image decode returned no raster; falling back to shell thumbnail");
        }

        if (NativeThumbnail.TryGet(open.Path, 1920) is { } fallbackThumb)
        {
            cancellationToken.ThrowIfCancellationRequested();
            DiagLog.Write("RasterHost", $"shell thumbnail {fallbackThumb.Width}x{fallbackThumb.Height}");
            long fallbackHandle = producer.CreatePresentedSurface(fallbackThumb.Bgra, fallbackThumb.Width, fallbackThumb.Height);
            cancellationToken.ThrowIfCancellationRequested();
            await channel.SendAsync(new PreviewSurface(
                open.RequestId, fallbackHandle, (uint)fallbackThumb.Width, (uint)fallbackThumb.Height, 96.0, "B8G8R8A8_UNORM"));
            await channel.SendAsync(new PreviewReady(
                open.RequestId, "thumbnail", Path.GetFileName(open.Path), fallbackThumb.Width, fallbackThumb.Height));
            return;
        }

        await channel.SendAsync(new PreviewError(open.RequestId, "no raster provider handled the file"));
    }
    catch (OperationCanceledException)
    {
        throw;
    }
    catch (Exception ex)
    {
        DiagLog.Write("RasterHost", "open ERROR: " + ex);
        await channel.SendAsync(new PreviewError(open.RequestId, ex.Message));
    }
}

static async Task<NativeDecodedImage?> DecodeImageAsync(string path, TimeSpan timeout, TimeSpan systemTimeout, CancellationToken cancellationToken)
{
    if (PreferSystemImageDecoder(path))
    {
        using var systemTrace = DiagLog.TraceScope("RasterHost", $"system image decode path={path}", 250);
        var systemImage = await DecodeSystemImageWithTimeoutAsync(path, systemTimeout, cancellationToken);
        if (systemImage is not null)
            return systemImage;
    }

    NativeDecodedImage? nativeImage;
    using (DiagLog.TraceScope("RasterHost", $"native image decode path={path}", 250))
        nativeImage = await NativeImageDecoder.TryDecodeAsync(path, timeout, cancellationToken);
    if (nativeImage is not null)
        return nativeImage;

    return PreferSystemImageDecoder(path)
        ? null
        : await DecodeSystemFallbackAsync(path, systemTimeout, cancellationToken);
}

static async Task<NativeDecodedImage?> DecodeSystemFallbackAsync(string path, TimeSpan timeout, CancellationToken cancellationToken)
{
    using var trace = DiagLog.TraceScope("RasterHost", $"system image fallback decode path={path}", 250);
    return await DecodeSystemImageWithTimeoutAsync(path, timeout, cancellationToken);
}

static async Task<NativeDecodedImage?> DecodeSystemImageWithTimeoutAsync(string path, TimeSpan timeout, CancellationToken cancellationToken)
{
    try
    {
        return await SystemImageDecoder.TryDecodeAsync(path, cancellationToken).WaitAsync(timeout, cancellationToken);
    }
    catch (TimeoutException)
    {
        DiagLog.Write("RasterHost", $"system image decode timed out path={path}; timeout={timeout.TotalMilliseconds:0}ms");
        return null;
    }
}

static bool PreferSystemImageDecoder(string path)
{
    string ext = Path.GetExtension(path).ToLowerInvariant();
    return ext is ".tif" or ".tiff" or ".heic" or ".heif" or ".avif" or ".jxl";
}

async Task HandlePageOpenAsync(PreviewPageOpen page)
{
    var key = (page.RequestId, page.PageIndex);
    if (pdfPageRenderCts.Remove(key, out var previousCts))
    {
        try { previousCts.Cancel(); } catch (ObjectDisposedException) { }
        previousCts.Dispose();
    }
    var pageCts = new CancellationTokenSource();
    pdfPageRenderCts[key] = pageCts;

    try
    {
        if (!pdfSessions.TryGetValue(page.RequestId, out var session))
            return;

        var rendered = await session.RenderPageAsync(page.PageIndex, page.Scale, pageCts.Token);
        if (!pdfSessions.TryGetValue(page.RequestId, out var current) || !ReferenceEquals(current, session))
            return;
        if (!pdfPageRenderCts.TryGetValue(key, out var currentCts) || !ReferenceEquals(currentCts, pageCts))
            return;

        long handle = producer.CreatePresentedPageSurface(page.PageIndex, rendered.Bgra, rendered.Width, rendered.Height);
        await channel.SendAsync(new PreviewSurface(
            page.RequestId, handle, (uint)rendered.Width, (uint)rendered.Height, 96.0, "B8G8R8A8_UNORM", page.PageIndex));
    }
    catch (OperationCanceledException)
    {
        DiagLog.Write("RasterHost", $"page render canceled: request={page.RequestId} page={page.PageIndex}");
    }
    catch (Exception ex)
    {
        DiagLog.Write("RasterHost", $"page render failed: {ex.Message}");
    }
    finally
    {
        if (pdfPageRenderCts.TryGetValue(key, out var currentCts) && ReferenceEquals(currentCts, pageCts))
            pdfPageRenderCts.Remove(key);
        pageCts.Dispose();
    }
}

static bool IsPdf(QuickLook.Next.Contracts.FileProbe probe)
    => probe.Kind.Equals("pdf", StringComparison.OrdinalIgnoreCase)
       || probe.Extension.Equals(".pdf", StringComparison.OrdinalIgnoreCase)
       || (probe.MagicPrefix.Length >= 4
           && probe.MagicPrefix[0] == (byte)'%'
           && probe.MagicPrefix[1] == (byte)'P'
            && probe.MagicPrefix[2] == (byte)'D'
            && probe.MagicPrefix[3] == (byte)'F');

static bool IsImage(QuickLook.Next.Contracts.FileProbe probe)
    => probe.Kind.Equals("image", StringComparison.OrdinalIgnoreCase);

static string? GetArg(string[] a, string key)
{
    for (int i = 0; i < a.Length - 1; i++)
        if (a[i] == key) return a[i + 1];
    return null;
}
