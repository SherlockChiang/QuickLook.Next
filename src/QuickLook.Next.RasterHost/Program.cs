using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Pipes;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;
using QuickLook.Next.RasterHost;

// RasterHost: the .NET surface process. It owns D3D shared-surface production plus Windows-only render
// bridges (PDF pages and shell thumbnails). Preview business logic should live in Rust or the App UI.
NativeImageDecoder.EnsureCompatible();


if (GetArg(args, "--smoke-system-image-corpus") is { } smokeCorpusDir)
{
    await SmokeSystemImageCorpusAsync(smokeCorpusDir, args.Contains("--require-system-codecs", StringComparer.OrdinalIgnoreCase));
    return;
}

string pipeName = GetArg(args, "--pipe") ?? "quicklook_next";
string? sessionToken = GetArg(args, "--session-token");

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
var pdfSessions = new ConcurrentDictionary<string, PdfPreviewSession>();
var pdfPageRenderCts = new ConcurrentDictionary<(string RequestId, int PageIndex, long PageGeneration), CancellationTokenSource>();
var openCts = new Dictionary<string, CancellationTokenSource>();
var openCtsLock = new object();
var surfacePublishGate = new SemaphoreSlim(1, 1);
var animationCts = new ConcurrentDictionary<string, CancellationTokenSource>();
var animationPackets = new ConcurrentDictionary<string, (string Path, SafeFileHandle Handle)>();
var animationParents = new ConcurrentDictionary<string, string>();
var animationHandoffGates = new ConcurrentDictionary<string, SemaphoreSlim>();
string inputRoot = Path.Combine(Path.GetTempPath(), "QuickLookNext", "raster-inputs");
var previewInputs = new ConcurrentDictionary<string, (string Path, FileStream Anchor)>();
TimeSpan imageDecodeTimeout = TimeSpan.FromMilliseconds(2500);
TimeSpan systemImageDecodeTimeout = TimeSpan.FromSeconds(2);
bool authenticated = false;
string? activeRequestId = null;
RasterOpen? activeOpen = null;
const uint MaxSurfaceDimension = 8192;
const ulong MaxSurfacePixels = 32UL * 1024 * 1024;
CleanupStaleAnimationPackets();
CleanupStalePreviewInputs(inputRoot);

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
            case Hello hello when !authenticated:
                if (string.IsNullOrWhiteSpace(sessionToken)
                    || !string.Equals(hello.SessionToken, sessionToken, StringComparison.Ordinal))
                {
                    DiagLog.Write("RasterHost", "rejected unauthenticated pipe client");
                    return;
                }
                try
                {
                    WindowsHandleTransfer.VerifyNamedPipeServerProcess(pipe.SafePipeHandle, hello.AppProcessId);
                    producer.Initialize();
                    await channel.SendAsync(new HostReady(producer.AdapterLuid));
                    authenticated = true;
                }
                catch (Exception ex)
                {
                    DiagLog.Write("RasterHost", "authentication initialization failed: " + ex.Message);
                    return;
                }
                DiagLog.Write("RasterHost", $"initialized; sent host.ready");
                break;

            case var _ when !authenticated:
                DiagLog.Write("RasterHost", "rejected control message before authentication");
                return;

            case Hello:
                DiagLog.Write("RasterHost", "rejected repeated authentication");
                return;

            case PreviewOpen open when IsValidRequestId(open.RequestId)
                                       && !string.IsNullOrWhiteSpace(open.Path)
                                       && IsValidProbe(open.Probe)
                                       && IsValidTargetSize(open.TargetWidth, open.TargetHeight):
                await surfacePublishGate.WaitAsync();
                try
                {
                    StartOpen(new RasterOpen(
                        open.RequestId, open.Path, open.Probe, open.TargetWidth, open.TargetHeight));
                }
                finally
                {
                    surfacePublishGate.Release();
                }
                break;

            case PreviewOpenHandle open:
                SafeFileHandle sourceHandle;
                try
                {
                    sourceHandle = WindowsHandleTransfer.TakeLocalFileHandle(open.SourceHandle, open.SourceLength);
                }
                catch (Exception ex)
                {
                    await channel.SendAsync(new PreviewError(open.RequestId, ex.Message));
                    break;
                }
                if (!IsValidRequestId(open.RequestId)
                    || open.SourceLength is not (>= 0 and <= 256L * 1024 * 1024)
                    || string.IsNullOrWhiteSpace(open.LogicalPath)
                    || !IsValidProbe(open.Probe)
                    || open.SourceLength != open.Probe.Size
                    || !IsValidTargetSize(open.TargetWidth, open.TargetHeight))
                {
                    sourceHandle.Dispose();
                    if (IsValidRequestId(open.RequestId))
                        await channel.SendAsync(new PreviewError(open.RequestId, "Invalid handle preview request."));
                    break;
                }
                await surfacePublishGate.WaitAsync();
                try
                {
                    StartOpen(
                        new RasterOpen(open.RequestId, open.LogicalPath, open.Probe, open.TargetWidth, open.TargetHeight),
                        sourceHandle,
                        open.SourceLength);
                }
                finally
                {
                    surfacePublishGate.Release();
                }
                break;

            case PreviewAnimationFramesOpen animation when IsValidRequestId(animation.RequestId)
                                                              && IsValidRequestId(animation.PreviewRequestId)
                                                              && IsValidAnimationTargetSize(animation.TargetWidth, animation.TargetHeight)
                                                              && activeOpen is { } parent
                                                              && string.Equals(animation.PreviewRequestId, activeRequestId, StringComparison.Ordinal)
                                                              && string.Equals(parent.RequestId, animation.PreviewRequestId, StringComparison.Ordinal):
                StartAnimationDecode(animation, parent.Path);
                break;

            case PreviewAnimationFramesClose animationClose when IsValidRequestId(animationClose.RequestId):
                await CloseAnimationAsync(animationClose.RequestId);
                break;

            case PreviewSurfaceRelease release when IsValidRequestId(release.TransferId):
                producer.ReleaseSurfaceTransfer(release.TransferId);
                break;

        case PreviewResize resize:
            if (!string.Equals(resize.RequestId, activeRequestId, StringComparison.Ordinal)
                || resize.Width == 0 || resize.Height == 0
                || resize.Width > MaxSurfaceDimension || resize.Height > MaxSurfaceDimension
                || (ulong)resize.Width * resize.Height > MaxSurfacePixels
                || !double.IsFinite(resize.Dpi) || resize.Dpi <= 0 || resize.Dpi > 960)
            {
                DiagLog.Write("RasterHost", $"rejected invalid resize: request={resize.RequestId} size={resize.Width}x{resize.Height} dpi={resize.Dpi}");
                break;
            }
            SurfaceTransfer rh = producer.CreateSurface(resize.Width, resize.Height);
            await channel.SendAsync(new PreviewSurface(
                resize.RequestId, rh.HostHandle, resize.Width, resize.Height, resize.Dpi, "B8G8R8A8_UNORM")
            {
                TransferId = rh.TransferId,
            });
            break;

        case PreviewPageOpen page when IsValidRequestId(page.RequestId)
                                       && page.PageIndex >= 0
                                       && page.PageGeneration > 0
                                       && double.IsFinite(page.Scale)
                                       && page.Scale > 0:
            _ = HandlePageOpenAsync(page);
            break;

        case PreviewPageClose pageClose when IsValidRequestId(pageClose.RequestId)
                                             && pageClose.PageIndex >= 0
                                             && pageClose.PageGeneration > 0:
            CancelPageRender(pageClose.RequestId, pageClose.PageIndex, pageClose.PageGeneration);
            _ = Task.Delay(250).ContinueWith(
                _ => producer.ReleasePage(pageClose.RequestId, pageClose.PageIndex, pageClose.PageGeneration),
                TaskContinuationOptions.OnlyOnRanToCompletion);
            break;

            case PreviewClose close when IsValidRequestId(close.RequestId):
                bool isActiveRequest = string.Equals(close.RequestId, activeRequestId, StringComparison.Ordinal);
                CancelOpen(close.RequestId);
                if (pdfSessions.TryRemove(close.RequestId, out var pdf))
                    await pdf.DisposeAsync();
                foreach (var key in pdfPageRenderCts.Keys.Where(k => k.RequestId == close.RequestId).ToArray())
                {
                    if (pdfPageRenderCts.TryRemove(key, out var cts))
                    {
                        try { cts.Cancel(); } catch (ObjectDisposedException) { }
                    }
                }
                if (isActiveRequest)
                {
                    activeRequestId = null;
                    activeOpen = null;
                    CancelAnimationsForParent(close.RequestId);
                    producer.Retire(); // defer GPU surface release until the next open (avoids compositor AV)
                }
                DeletePreviewInput(close.RequestId);
                break;

            default:
                DiagLog.Write("RasterHost", $"rejected invalid control message: {msg.GetType().Name}");
                return;
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
foreach (string requestId in animationCts.Keys)
    await CloseAnimationAsync(requestId);
foreach (PdfPreviewSession session in pdfSessions.Values)
    await session.DisposeAsync();
pdfSessions.Clear();
foreach (string requestId in previewInputs.Keys)
    DeletePreviewInput(requestId);
foreach (var packet in animationPackets.Values)
{
    packet.Handle.Dispose();
    DeleteAnimationPacket(packet.Path);
}

void StartOpen(RasterOpen open, SafeFileHandle? sourceHandle = null, long sourceLength = 0)
{
    string? previousRequestId = activeRequestId;
    if (previousRequestId is not null && !string.Equals(previousRequestId, open.RequestId, StringComparison.Ordinal))
        CancelAnimationsForParent(previousRequestId);
    activeRequestId = open.RequestId;
    activeOpen = sourceHandle is null ? open : null;
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
            if (sourceHandle is not null)
            {
                using (sourceHandle)
                {
                    var input = await CreatePreviewInputAsync(
                        open.RequestId, open.Path, sourceHandle, sourceLength, inputRoot, cts.Token);
                    if (input is null || !previewInputs.TryAdd(open.RequestId, input.Value))
                    {
                        input?.Anchor.Dispose();
                        if (input is not null) DeletePreviewInputPath(input.Value.Path);
                        await channel.SendAsync(new PreviewError(open.RequestId, "Could not anchor preview input."));
                        return;
                    }
                    open = open with { Path = input.Value.Path };
                }
                cts.Token.ThrowIfCancellationRequested();
                if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
                    return;
                activeOpen = open;
            }
            await HandleOpenAsync(open, cts.Token);
        }
        catch (OperationCanceledException)
        {
            DiagLog.Write("RasterHost", $"open canceled: request={open.RequestId}");
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", "open task ERROR: " + ex);
            try
            {
                await channel.SendAsync(IsImage(open.Probe)
                    ? CreateImagePreviewError(open.RequestId, open.Probe.Extension)
                    : new PreviewError(open.RequestId, ex.Message));
            }
            catch { }
        }
        finally
        {
            if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
                DeletePreviewInput(open.RequestId);
            lock (openCtsLock)
            {
                if (openCts.TryGetValue(open.RequestId, out var current) && ReferenceEquals(current, cts))
                    openCts.Remove(open.RequestId);
            }
            cts.Dispose();
        }
    });
}

void StartAnimationDecode(PreviewAnimationFramesOpen animation, string path)
{
    if (animationPackets.ContainsKey(animation.RequestId))
    {
        _ = channel.SendAsync(new PreviewError(animation.RequestId, "Animation frame packet has not been released."));
        return;
    }

    var cts = new CancellationTokenSource();
    var gate = new SemaphoreSlim(1, 1);
    if (!animationCts.TryAdd(animation.RequestId, cts)
        || !animationHandoffGates.TryAdd(animation.RequestId, gate))
    {
        animationCts.TryRemove(animation.RequestId, out _);
        cts.Dispose();
        gate.Dispose();
        _ = channel.SendAsync(new PreviewError(animation.RequestId, "Duplicate animation request ID."));
        return;
    }
    animationParents[animation.RequestId] = animation.PreviewRequestId;

    _ = Task.Run(async () =>
    {
        string? tempPath = null;
        try
        {
            byte[]? packet = await NativeAnimationPacketDecoder.TryDecodeAsync(
                path, animation.TargetWidth, animation.TargetHeight, cts.Token);
            cts.Token.ThrowIfCancellationRequested();
            if (packet is null)
            {
                await channel.SendAsync(new PreviewError(animation.RequestId, "Animation frame decode failed."));
                return;
            }

            tempPath = WriteAnimationPacket(animation.RequestId, packet);
            if (tempPath is null)
            {
                await channel.SendAsync(new PreviewError(animation.RequestId, "Animation frame handoff failed."));
                return;
            }

            int count = checked((int)BitConverter.ToUInt32(packet, 0));
            int width = checked((int)BitConverter.ToUInt32(packet, 4));
            int height = checked((int)BitConverter.ToUInt32(packet, 8));
            await gate.WaitAsync();
            try
            {
                cts.Token.ThrowIfCancellationRequested();
                if (!string.Equals(animation.PreviewRequestId, activeRequestId, StringComparison.Ordinal))
                    return;
                var transferred = WindowsHandleTransfer.OpenReadOnlyFile(tempPath);
                if (transferred.Length != packet.LongLength)
                {
                    transferred.Handle.Dispose();
                    throw new InvalidDataException("Animation packet changed before handle transfer.");
                }
                animationPackets[animation.RequestId] = (tempPath, transferred.Handle);
                await channel.SendAsync(new PreviewAnimationFramesReady(
                    animation.RequestId, animation.PreviewRequestId, transferred.Handle.DangerousGetHandle().ToInt64(), count, width, height, transferred.Length));
                tempPath = null;
            }
            finally
            {
                gate.Release();
            }
        }
        catch (OperationCanceledException) { }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", $"animation decode failed request={animation.RequestId}: {ex}");
            try { await channel.SendAsync(new PreviewError(animation.RequestId, ex.Message)); } catch { }
        }
        finally
        {
            if (tempPath is not null) DeleteAnimationPacket(tempPath);
            if (animationCts.TryRemove(animation.RequestId, out var current)) current.Dispose();
            if (!animationPackets.ContainsKey(animation.RequestId))
                animationParents.TryRemove(animation.RequestId, out _);
            animationHandoffGates.TryRemove(animation.RequestId, out _);
        }
    });
}

async Task CloseAnimationAsync(string requestId)
{
    animationCts.TryGetValue(requestId, out var cts);
    try { cts?.Cancel(); } catch (ObjectDisposedException) { }
    if (animationHandoffGates.TryGetValue(requestId, out var gate))
        await gate.WaitAsync();
    try
    {
        if (animationPackets.TryRemove(requestId, out var packet))
        {
            packet.Handle.Dispose();
            DeleteAnimationPacket(packet.Path);
        }
        animationParents.TryRemove(requestId, out _);
    }
    finally
    {
        gate?.Release();
    }
}

void CancelAnimationsForParent(string previewRequestId)
{
    foreach (var pair in animationParents)
        if (string.Equals(pair.Value, previewRequestId, StringComparison.Ordinal))
            _ = CloseAnimationAsync(pair.Key);
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

void CancelPageRender(string requestId, int pageIndex, long pageGeneration)
{
    var key = (requestId, pageIndex, pageGeneration);
    if (!pdfPageRenderCts.TryRemove(key, out var cts))
        return;

    try { cts.Cancel(); } catch (ObjectDisposedException) { }
}

async Task HandleOpenAsync(RasterOpen open, CancellationToken cancellationToken)
{
    DiagLog.Write("RasterHost", $"open path={open.Path} ext={open.Probe.Extension} kind={open.Probe.Kind} size={open.Probe.Size}");
    _ = Task.Delay(250, cancellationToken).ContinueWith(_ => producer.ReleaseRetired(), TaskContinuationOptions.OnlyOnRanToCompletion);
    try
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (IsPdf(open.Probe))
        {
            if (pdfSessions.TryRemove(open.RequestId, out var old)) await old.DisposeAsync();
            PdfPreviewSession? session = await PdfPreviewSession.OpenAsync(open.Path);
            try
            {
                cancellationToken.ThrowIfCancellationRequested();
                var first = session.FirstPageSize;
                uint pageCount = session.PageCount;
                var pageGeometries = session.PageGeometries;
                await surfacePublishGate.WaitAsync(cancellationToken);
                try
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
                        return;

                    pdfSessions[open.RequestId] = session;
                    session = null;
                    try
                    {
                        await channel.SendAsync(new PreviewReady(
                            open.RequestId,
                            "pdf",
                            $"{Path.GetFileName(open.Probe.Path)} — {pageCount} pages",
                            first.Width,
                            first.Height)
                        {
                            PageCount = checked((int)pageCount),
                            PageWidth = first.Width,
                            PageHeight = first.Height,
                            PdfPageGeometries = pageGeometries,
                        });
                    }
                    catch
                    {
                        if (pdfSessions.TryRemove(open.RequestId, out var failed)) await failed.DisposeAsync();
                        throw;
                    }
                }
                finally
                {
                    surfacePublishGate.Release();
                }
            }
            finally
            {
                if (session is not null)
                    await session.DisposeAsync();
            }
            return;
        }

        if (IsImage(open.Probe))
        {
            var image = await DecodeImageAsync(
                open.Path,
                imageDecodeTimeout,
                systemImageDecodeTimeout,
                cancellationToken,
                open.TargetWidth,
                open.TargetHeight);
            cancellationToken.ThrowIfCancellationRequested();
            if (image is not null)
            {
                DiagLog.Write(
                    "RasterHost",
                    $"image raster {image.Width}x{image.Height} original={image.OriginalWidth}x{image.OriginalHeight}; " +
                    $"native decode={image.DecodeMilliseconds}ms resize={image.ResizeMilliseconds}ms convert={image.ConvertMilliseconds}ms");
                var uploadWatch = Stopwatch.StartNew();
                await surfacePublishGate.WaitAsync(cancellationToken);
                try
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
                        return;
                    SurfaceTransfer imageHandle = producer.CreatePresentedSurface(image.Bgra, image.Width, image.Height);
                    uploadWatch.Stop();
                    DiagLog.Write("RasterHost", $"image surface upload/create {uploadWatch.ElapsedMilliseconds}ms; bytes={image.Bgra.Length}");
                    await channel.SendAsync(new PreviewSurface(
                        open.RequestId, imageHandle.HostHandle, (uint)image.Width, (uint)image.Height, 96.0, "B8G8R8A8_UNORM")
                    {
                        TransferId = imageHandle.TransferId,
                    });
                    string title = image.Width == image.OriginalWidth && image.Height == image.OriginalHeight
                        ? Path.GetFileName(open.Probe.Path)
                        : $"{Path.GetFileName(open.Probe.Path)} — {image.OriginalWidth}x{image.OriginalHeight} scaled to {image.Width}x{image.Height}";
                    await channel.SendAsync(new PreviewReady(open.RequestId, "image", title, image.Width, image.Height));
                }
                finally
                {
                    surfacePublishGate.Release();
                }
                return;
            }

            DiagLog.Write("RasterHost", "native image decode returned no raster; falling back to shell thumbnail");
        }

        if (await NativeThumbnail.TryGetAsync(open.Path, 1920, cancellationToken) is { } fallbackThumb)
        {
            cancellationToken.ThrowIfCancellationRequested();
            DiagLog.Write("RasterHost", $"shell thumbnail {fallbackThumb.Width}x{fallbackThumb.Height}");
            await surfacePublishGate.WaitAsync(cancellationToken);
            try
            {
                cancellationToken.ThrowIfCancellationRequested();
                if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
                    return;
                SurfaceTransfer fallbackHandle = producer.CreatePresentedSurface(fallbackThumb.Bgra, fallbackThumb.Width, fallbackThumb.Height);
                await channel.SendAsync(new PreviewSurface(
                    open.RequestId, fallbackHandle.HostHandle, (uint)fallbackThumb.Width, (uint)fallbackThumb.Height, 96.0, "B8G8R8A8_UNORM")
                {
                    TransferId = fallbackHandle.TransferId,
                });
                await channel.SendAsync(new PreviewReady(
                    open.RequestId, "thumbnail", Path.GetFileName(open.Probe.Path), fallbackThumb.Width, fallbackThumb.Height));
            }
            finally
            {
                surfacePublishGate.Release();
            }
            return;
        }

        if (IsImage(open.Probe))
        {
            await channel.SendAsync(CreateImagePreviewError(open.RequestId, open.Probe.Extension));
            return;
        }

        await channel.SendAsync(new PreviewError(open.RequestId, "No raster provider handled the file."));
    }
    catch (OperationCanceledException)
    {
        throw;
    }
    catch (Exception ex)
    {
        DiagLog.Write("RasterHost", "open ERROR: " + ex);
        await channel.SendAsync(IsImage(open.Probe)
            ? CreateImagePreviewError(open.RequestId, open.Probe.Extension)
            : new PreviewError(open.RequestId, ex.Message));
    }
}

static async Task<NativeDecodedImage?> DecodeImageAsync(
    string path,
    TimeSpan timeout,
    TimeSpan systemTimeout,
    CancellationToken cancellationToken,
    uint targetWidth,
    uint targetHeight)
{
    bool systemDecodeAttempted = PreferSystemImageDecoder(path);
    if (systemDecodeAttempted)
    {
        using var systemTrace = DiagLog.TraceScope("RasterHost", $"system image decode path={path}", 250);
        var systemImage = await DecodeSystemImageWithTimeoutAsync(path, systemTimeout, cancellationToken, targetWidth, targetHeight);
        if (systemImage is not null)
            return systemImage;
    }

    NativeDecodedImage? nativeImage;
    using (DiagLog.TraceScope("RasterHost", $"native image decode target={targetWidth}x{targetHeight} path={path}", 250))
        nativeImage = await NativeImageDecoder.TryDecodeAsync(
            path, timeout, cancellationToken, targetWidth, targetHeight, systemDecodeAttempted);
    if (nativeImage is not null)
        return nativeImage;

    return systemDecodeAttempted
        ? null
        : await DecodeSystemFallbackAsync(path, systemTimeout, cancellationToken, targetWidth, targetHeight);
}

static async Task<NativeDecodedImage?> DecodeSystemFallbackAsync(
    string path,
    TimeSpan timeout,
    CancellationToken cancellationToken,
    uint targetWidth,
    uint targetHeight)
{
    using var trace = DiagLog.TraceScope("RasterHost", $"system image fallback decode path={path}", 250);
    return await DecodeSystemImageWithTimeoutAsync(path, timeout, cancellationToken, targetWidth, targetHeight);
}

static async Task<NativeDecodedImage?> DecodeSystemImageWithTimeoutAsync(
    string path,
    TimeSpan timeout,
    CancellationToken cancellationToken,
    uint targetWidth,
    uint targetHeight)
{
    try
    {
        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);
        try
        {
            return await SystemImageDecoder.TryDecodeAsync(path, timeoutCts.Token, targetWidth, targetHeight);
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && timeoutCts.IsCancellationRequested)
        {
            DiagLog.Write("RasterHost", $"system image decode timed out path={path}; timeout={timeout.TotalMilliseconds:0}ms");
            return null;
        }
    }
    catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested) { throw; }
}

static bool PreferSystemImageDecoder(string path)
{
    string ext = Path.GetExtension(path).ToLowerInvariant();
    return ext is ".png" or ".bmp" or ".webp" or ".jpg" or ".jpeg" or ".jpe" or ".tif" or ".tiff" or ".heic" or ".heif" or ".avif" or ".jxl";
}

async Task HandlePageOpenAsync(PreviewPageOpen page)
{
    var key = (page.RequestId, page.PageIndex, page.PageGeneration);
    var pageCts = new CancellationTokenSource();
    if (!pdfPageRenderCts.TryAdd(key, pageCts))
    {
        pageCts.Dispose();
        DiagLog.Write("RasterHost", $"page render coalesced: request={page.RequestId} page={page.PageIndex}");
        return;
    }

    try
    {
        if (!pdfSessions.TryGetValue(page.RequestId, out var session))
        {
            await channel.SendAsync(new PreviewPageError(page.RequestId, page.PageIndex, page.PageGeneration, false, "PDF session is no longer available"));
            return;
        }

        var rendered = await session.RenderPageAsync(page.PageIndex, page.Scale, pageCts.Token);
        if (!pdfSessions.TryGetValue(page.RequestId, out var current) || !ReferenceEquals(current, session))
            return;
        if (!pdfPageRenderCts.TryGetValue(key, out var currentCts) || !ReferenceEquals(currentCts, pageCts))
            return;

        var uploadWatch = Stopwatch.StartNew();
        SurfaceTransfer handle = producer.CreatePresentedPageSurface(
            page.RequestId, page.PageIndex, page.PageGeneration, rendered.Bgra, rendered.Width, rendered.Height);
        uploadWatch.Stop();
        DiagLog.Write("RasterHost", $"pdf page surface upload/create {uploadWatch.ElapsedMilliseconds}ms; request={page.RequestId}; page={page.PageIndex}; bytes={rendered.Bgra.Length}");
        var sendWatch = Stopwatch.StartNew();
        await channel.SendAsync(new PreviewSurface(
            page.RequestId, handle.HostHandle, (uint)rendered.Width, (uint)rendered.Height, 96.0,
            "B8G8R8A8_UNORM", page.PageIndex, page.PageGeneration)
        {
            TransferId = handle.TransferId,
        });
        sendWatch.Stop();
        DiagLog.Write("RasterHost", $"pdf page surface send {sendWatch.ElapsedMilliseconds}ms; request={page.RequestId}; page={page.PageIndex}");
    }
    catch (OperationCanceledException)
    {
        DiagLog.Write("RasterHost", $"page render canceled: request={page.RequestId} page={page.PageIndex}");
    }
    catch (TimeoutException ex)
    {
        DiagLog.Write("RasterHost", $"page render timed out: request={page.RequestId} page={page.PageIndex}");
        await channel.SendAsync(new PreviewPageError(page.RequestId, page.PageIndex, page.PageGeneration, true, ex.Message));
    }
    catch (Exception ex)
    {
        DiagLog.Write("RasterHost", $"page render failed: {ex.Message}");
        await channel.SendAsync(new PreviewPageError(page.RequestId, page.PageIndex, page.PageGeneration, false, ex.Message));
    }
    finally
    {
        ((ICollection<KeyValuePair<(string RequestId, int PageIndex, long PageGeneration), CancellationTokenSource>>)pdfPageRenderCts)
            .Remove(new KeyValuePair<(string RequestId, int PageIndex, long PageGeneration), CancellationTokenSource>(key, pageCts));
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

static PreviewError CreateImagePreviewError(string requestId, string extension)
    => ImageCodecPolicy.RequiresSystemCodec(extension)
        ? new PreviewError(requestId, "A Windows image codec is required.")
        {
            Code = PreviewErrorCodes.ImageCodecRequired,
            Format = ImageCodecPolicy.NormalizeFormat(extension),
        }
        : new PreviewError(requestId, "Image preview failed.")
        {
            Code = PreviewErrorCodes.ImageDecodeFailed,
            Format = ImageCodecPolicy.NormalizeFormat(extension),
        };

static bool IsValidRequestId(string? requestId)
    => requestId is { Length: 32 } && requestId.All(static c => char.IsAsciiHexDigit(c));

static bool IsValidProbe(QuickLook.Next.Contracts.FileProbe? probe)
    => probe is not null
       && !string.IsNullOrWhiteSpace(probe.Path)
       && probe.Extension is not null
       && probe.MagicPrefix is not null
       && !string.IsNullOrWhiteSpace(probe.Kind)
       && probe.Size >= 0;

static bool IsValidTargetSize(uint width, uint height)
    => width <= MaxSurfaceDimension
       && height <= MaxSurfaceDimension
       && (width == 0 || height == 0 || (ulong)width * height <= MaxSurfacePixels);

static bool IsValidAnimationTargetSize(uint width, uint height)
    => IsValidTargetSize(width, height);

static async Task<(string Path, FileStream Anchor)?> CreatePreviewInputAsync(
    string requestId,
    string logicalPath,
    SafeFileHandle sourceHandle,
    long sourceLength,
    string root,
    CancellationToken cancellationToken)
{
    string extension = Path.GetExtension(logicalPath);
    if (extension.Length > 32 || extension.Any(static c => !char.IsAsciiLetterOrDigit(c) && c != '.'))
        extension = "";
    string directory = Path.Combine(root, "input-" + requestId);
    string path = Path.Combine(directory, "source" + extension.ToLowerInvariant());
    try
    {
        Directory.CreateDirectory(root);
        if ((File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0) return null;
        Directory.CreateDirectory(directory);
        if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0) return null;
        using var source = new FileStream(sourceHandle, FileAccess.Read);
        var anchor = new FileStream(path, new FileStreamOptions
        {
            Mode = FileMode.CreateNew,
            Access = FileAccess.ReadWrite,
            Share = FileShare.Read,
            Options = FileOptions.Asynchronous | FileOptions.WriteThrough,
        });
        try
        {
            await source.CopyToAsync(anchor, cancellationToken);
            await anchor.FlushAsync(cancellationToken);
            if (anchor.Length != sourceLength) throw new InvalidDataException("Preview input changed while anchoring.");
            anchor.Position = 0;
            return (path, anchor);
        }
        catch
        {
            anchor.Dispose();
            throw;
        }
    }
    catch (OperationCanceledException)
    {
        DeletePreviewInputPath(path);
        throw;
    }
    catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException)
    {
        DeletePreviewInputPath(path);
        return null;
    }
}

void DeletePreviewInput(string requestId)
{
    if (!previewInputs.TryRemove(requestId, out var input)) return;
    input.Anchor.Dispose();
    DeletePreviewInputPath(input.Path);
}

static void DeletePreviewInputPath(string path)
{
    try
    {
        File.Delete(path);
        string? directory = Path.GetDirectoryName(path);
        if (directory is not null) Directory.Delete(directory, recursive: false);
    }
    catch { }
}

static void CleanupStalePreviewInputs(string root)
{
    try
    {
        if (!Directory.Exists(root) || (File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0)
            return;
        foreach (string directory in Directory.EnumerateDirectories(root, "input-*"))
            if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) == 0)
                Directory.Delete(directory, recursive: true);
    }
    catch { }
}

static string? WriteAnimationPacket(string requestId, byte[] packet)
{
    try
    {
        string root = Path.Combine(Path.GetTempPath(), "QuickLookNext", "raster-animation");
        Directory.CreateDirectory(root);
        if ((File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0)
            return null;
        string directory = Path.Combine(root, "frames-" + requestId);
        Directory.CreateDirectory(directory);
        if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0)
            return null;
        string path = Path.Combine(directory, "frames.bin");
        using var stream = new FileStream(path, new FileStreamOptions
        {
            Mode = FileMode.CreateNew,
            Access = FileAccess.Write,
            Share = FileShare.None,
            Options = FileOptions.WriteThrough,
        });
        stream.Write(packet);
        return path;
    }
    catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException)
    {
        return null;
    }
}

static void DeleteAnimationPacket(string path)
{
    try
    {
        File.Delete(path);
        string? directory = Path.GetDirectoryName(path);
        if (directory is not null) Directory.Delete(directory, recursive: false);
    }
    catch { }
}

static void CleanupStaleAnimationPackets()
{
    try
    {
        string root = Path.Combine(Path.GetTempPath(), "QuickLookNext", "raster-animation");
        if (!Directory.Exists(root) || (File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0)
            return;
        foreach (string directory in Directory.EnumerateDirectories(root, "frames-*"))
            if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) == 0)
                Directory.Delete(directory, recursive: true);
    }
    catch { }
}

static async Task SmokeSystemImageCorpusAsync(string corpusDir, bool requireSystemCodecs)
{
    string[] files = ["jpeg-cmyk.jpg", "jpeg-wide-gamut-icc.jpg", "avif-still.avif", "heic-still.heic", "jxl-still.jxl"];
    int decoded = 0;
    var failures = new List<string>();
    foreach (string file in files)
    {
        string path = Path.Combine(corpusDir, file);
        if (!File.Exists(path))
        {
            failures.Add($"missing {file}");
            continue;
        }

        try
        {
            NativeDecodedImage? image = await SystemImageDecoder.TryDecodeAsync(path, CancellationToken.None, 512, 512);
            if (image is null)
            {
                string message = $"system codec did not decode {file}";
                if (requireSystemCodecs || file is "jpeg-cmyk.jpg" or "jpeg-wide-gamut-icc.jpg" or "avif-still.avif" or "heic-still.heic") failures.Add(message);
                else Console.WriteLine(message);
                continue;
            }
            decoded++;
            Console.WriteLine($"decoded {file}: {image.Width}x{image.Height} original={image.OriginalWidth}x{image.OriginalHeight}");
        }
        catch (Exception ex)
        {
            if (requireSystemCodecs || file is "jpeg-cmyk.jpg" or "jpeg-wide-gamut-icc.jpg" or "avif-still.avif" or "heic-still.heic") failures.Add($"{file}: {ex.Message}");
            else Console.WriteLine($"system codec failed {file}: {ex.Message}");
        }
    }

    Console.WriteLine($"system image corpus smoke decoded={decoded}/{files.Length}");
    if (failures.Count > 0)
    {
        foreach (string failure in failures)
            Console.Error.WriteLine(failure);
        Environment.ExitCode = 1;
    }
}

static string? GetArg(string[] a, string key)
{
    for (int i = 0; i < a.Length - 1; i++)
        if (a[i] == key) return a[i + 1];
    return null;
}

internal sealed record RasterOpen(
    string RequestId,
    string Path,
    QuickLook.Next.Contracts.FileProbe Probe,
    uint TargetWidth,
    uint TargetHeight);
