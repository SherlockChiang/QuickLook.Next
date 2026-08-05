using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Pipes;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using QuickLook.Next.RasterHost;

SupervisedHostProcessPolicy.SuppressInteractiveErrorUi();

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
var openAndPageWorkers = new ConcurrentDictionary<long, Task>();
long nextOpenAndPageWorkerId = 0;
var surfacePublishGate = new SemaphoreSlim(1, 1);
var animationCts = new ConcurrentDictionary<string, CancellationTokenSource>();
var animationPackets = new ConcurrentDictionary<string, NativeAnimationPacket>();
var preparedGifDecodes = new ConcurrentDictionary<string, PreparedGifDecodeState>();
var animationParents = new ConcurrentDictionary<string, string>();
var animationHandoffGates = new ConcurrentDictionary<string, SemaphoreSlim>();
var metadataRequests = new ConcurrentDictionary<string, ImageMetadataRequestState>();
var retainedRasterSources = new ConcurrentDictionary<string, RetainedRasterSource>();
TimeSpan imageDecodeTimeout = TimeSpan.FromMilliseconds(2500);
TimeSpan systemImageDecodeTimeout = TimeSpan.FromSeconds(2);
TimeSpan imageMetadataTimeout = TimeSpan.FromMilliseconds(1500);
bool authenticated = false;
string? activeRequestId = null;
RasterOpen? activeOpen = null;
const uint MaxSurfaceDimension = 8192;
const ulong MaxSurfacePixels = 32UL * 1024 * 1024;

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
                    goto Shutdown;
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
                    goto Shutdown;
                }
                DiagLog.Write("RasterHost", $"initialized; sent host.ready");
                break;

            case var _ when !authenticated:
                DiagLog.Write("RasterHost", "rejected control message before authentication");
                goto Shutdown;

            case Hello:
                DiagLog.Write("RasterHost", "rejected repeated authentication");
                goto Shutdown;

            case PreviewOpen open when IsValidRequestId(open.RequestId)
                                       && !string.IsNullOrWhiteSpace(open.Path)
                                       && IsValidProbe(open.Probe)
                                       && IsValidTargetSize(open.TargetWidth, open.TargetHeight):
                await surfacePublishGate.WaitAsync();
                try
                {
                    StartOpen(new RasterOpen(
                        open.RequestId,
                        open.Path,
                        open.Probe,
                        open.TargetWidth,
                        open.TargetHeight,
                        open.PrepareAnimation));
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
                    || !string.Equals(open.Probe.Path, open.LogicalPath, StringComparison.OrdinalIgnoreCase)
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
                        new RasterOpen(
                            open.RequestId,
                            open.LogicalPath,
                            open.Probe,
                            open.TargetWidth,
                            open.TargetHeight,
                            open.PrepareAnimation),
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
                PreparedGifDecodeState? preparedGifDecode =
                    await TryTakePreparedGifDecodeAsync(animation);
                if (preparedGifDecode is not null)
                {
                    StartPreparedAnimationHandoff(animation, preparedGifDecode);
                }
                else if (retainedRasterSources.TryGetValue(animation.PreviewRequestId, out var retainedSource))
                {
                    if (retainedSource.TryAcquire(
                            RetainedRasterOperations.Animation,
                            out RetainedRasterSourceLease? animationLease)
                        && animationLease is not null)
                        StartAnimationDecode(animation, null, animationLease);
                    else
                        await channel.SendAsync(new PreviewError(animation.RequestId, "Animation source is no longer available."));
                }
                else if (NativeImageDecoder.UsesHandleInput(parent.Path, parent.Probe))
                {
                    await channel.SendAsync(new PreviewError(animation.RequestId, "Animation source is no longer available."));
                }
                else
                {
                    StartAnimationDecode(animation, parent.Path, null);
                }
                break;

            case PreviewAnimationFramesClose animationClose when IsValidRequestId(animationClose.RequestId):
                await CloseAnimationAsync(animationClose.RequestId);
                break;

            case PreviewImageMetadataOpen metadata
                when IsValidRequestId(metadata.RequestId)
                     && IsValidRequestId(metadata.PreviewRequestId):
                if (!retainedRasterSources.TryGetValue(metadata.PreviewRequestId, out var metadataParent)
                    || !metadataParent.TryAcquire(
                        RetainedRasterOperations.Metadata,
                        out RetainedRasterSourceLease? metadataLease)
                    || metadataLease is null)
                {
                    await channel.SendAsync(new PreviewError(
                        metadata.RequestId,
                        "Image metadata source is no longer available."));
                }
                else
                {
                    StartImageMetadataRead(metadata, metadataLease);
                }
                break;

            case PreviewImageMetadataClose metadataClose when IsValidRequestId(metadataClose.RequestId):
                await CloseImageMetadataAsync(metadataClose.RequestId);
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
                TrackOpenOrPageWorker(HandlePageOpenAsync(page));
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
                    await DisposePdfSessionAsync(pdf, close.RequestId);
                foreach (var key in pdfPageRenderCts.Keys.Where(k => k.RequestId == close.RequestId).ToArray())
                {
                    if (pdfPageRenderCts.TryRemove(key, out var cts))
                    {
                        try { cts.Cancel(); } catch (ObjectDisposedException) { }
                    }
                }
                if (isActiveRequest)
                {
                    await surfacePublishGate.WaitAsync();
                    try
                    {
                        if (string.Equals(close.RequestId, activeRequestId, StringComparison.Ordinal))
                        {
                            activeRequestId = null;
                            activeOpen = null;
                            idleTrimmer.SetPreviewActive(false);
                            CancelAnimationsForParent(close.RequestId);
                            DeletePreparedGifDecode(close.RequestId);
                            producer.Retire(); // defer GPU surface release until the next open (avoids compositor AV)
                        }
                    }
                    finally
                    {
                        surfacePublishGate.Release();
                    }
                }
                DeleteRetainedRasterSource(close.RequestId);
                break;

            default:
                DiagLog.Write("RasterHost", $"rejected invalid control message: {msg.GetType().Name}");
                goto Shutdown;
        }
    }
    catch (Exception ex) { DiagLog.Write("RasterHost", "handler error: " + ex.Message); }
}

Shutdown:
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
CancellationTokenSource[] remainingPageCts = pdfPageRenderCts.Values.ToArray();
foreach (var cts in remainingPageCts)
{
    try { cts.Cancel(); } catch { }
}
await DrainOpenAndPageWorkersAsync();
foreach (string requestId in animationCts.Keys)
    await CloseAnimationAsync(requestId);
ImageMetadataRequestState[] remainingMetadataRequests = metadataRequests.Values.ToArray();
foreach (ImageMetadataRequestState request in remainingMetadataRequests)
{
    metadataRequests.TryRemove(request.RequestId, out _);
    request.Cancel();
}
await Task.WhenAll(remainingMetadataRequests.Select(static request => request.Worker));
foreach (PdfPreviewSession session in pdfSessions.Values)
    await DisposePdfSessionAsync(session, "pipe-disconnect");
pdfSessions.Clear();
foreach (string requestId in retainedRasterSources.Keys)
    DeleteRetainedRasterSource(requestId);
foreach (string requestId in preparedGifDecodes.Keys)
    DeletePreparedGifDecode(requestId);
foreach (var packet in animationPackets.Values)
    packet.Dispose();

// The App owns this supervised process and pipe EOF is terminal. Windows can reclaim the remaining
// process-scoped WinRT/DXGI graph atomically; unwinding that graph through CLR shutdown can otherwise
// let injected graphics layers raise a non-continuable bare FACILITY_DXGI exception after all managed
// work has drained.
DiagLog.Write("RasterHost", "pipe cleanup complete; exiting process");
Environment.Exit(0);

void TrackOpenOrPageWorker(Task worker)
{
    long workerId = Interlocked.Increment(ref nextOpenAndPageWorkerId);
    openAndPageWorkers[workerId] = worker;
    _ = worker.ContinueWith(
        completed =>
        {
            openAndPageWorkers.TryRemove(workerId, out _);
            if (completed.IsFaulted)
                DiagLog.Write("RasterHost", "open/page worker ERROR: " + completed.Exception);
        },
        CancellationToken.None,
        TaskContinuationOptions.ExecuteSynchronously,
        TaskScheduler.Default);
}

async Task DrainOpenAndPageWorkersAsync()
{
    Task[] workers = openAndPageWorkers.Values.ToArray();
    if (workers.Length == 0)
        return;

    try
    {
        await Task.WhenAll(workers).WaitAsync(TimeSpan.FromSeconds(5));
    }
    catch (TimeoutException)
    {
        // Matching the existing PDF render-drain policy, do not tear down the producer while a
        // canceled worker may still be publishing or releasing a D3D/WinRT resource.
        DiagLog.Write("RasterHost", $"open/page worker drain timed out; exiting host: workers={workers.Length}");
        Environment.Exit(31);
    }
    catch (Exception ex)
    {
        DiagLog.Write("RasterHost", "open/page worker drain failed: " + ex);
    }
}

void StartOpen(RasterOpen open, SafeFileHandle? sourceHandle = null, long sourceLength = 0)
{
    // A new open means the App has completed the previous Close -> Open transition, so surfaces
    // retired by that previous preview are no longer needed by its compositor visual.
    producer.ReleaseRetired();
    string? previousRequestId = activeRequestId;
    if (previousRequestId is not null && !string.Equals(previousRequestId, open.RequestId, StringComparison.Ordinal))
    {
        CancelAnimationsForParent(previousRequestId);
        DeletePreparedGifDecode(previousRequestId);
        DeleteRetainedRasterSource(previousRequestId);
    }
    activeRequestId = open.RequestId;
    activeOpen = sourceHandle is null ? open : null;
    idleTrimmer.SetPreviewActive(true);
    string[] existing;
    lock (openCtsLock)
        existing = openCts.Keys.ToArray();
    foreach (string requestId in existing)
        CancelOpen(requestId);

    var cts = new CancellationTokenSource();
    lock (openCtsLock)
        openCts[open.RequestId] = cts;
    Task worker = Task.Run(async () =>
    {
        try
        {
            if (sourceHandle is not null)
            {
                try
                {
                    if (IsPdf(open.Probe))
                    {
                        cts.Token.ThrowIfCancellationRequested();
                        if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
                            return;
                        activeOpen = open;
                        await OpenPdfSessionAsync(
                            open,
                            () => PdfPreviewSession.OpenHandleAsync(
                                sourceHandle,
                                sourceLength,
                                Path.GetFileName(open.Path)),
                            cts.Token);
                        return;
                    }
                    if (NativeImageDecoder.UsesHandleInput(open.Path, open.Probe))
                    {
                        var retainedSource = new RetainedRasterSource(
                            sourceHandle,
                            sourceLength,
                            Path.GetFileName(open.Path),
                            NativeImageDecoder.SupportsHandleAnimation(open.Path, open.Probe)
                                ? RetainedRasterOperations.StaticImage
                                    | RetainedRasterOperations.Animation
                                    | RetainedRasterOperations.Metadata
                                : RetainedRasterOperations.StaticImage
                                    | RetainedRasterOperations.Metadata);
                        if (!retainedRasterSources.TryAdd(open.RequestId, retainedSource))
                        {
                            retainedSource.Dispose();
                            await channel.SendAsync(new PreviewError(open.RequestId, "Could not retain raster input."));
                            return;
                        }
                        sourceHandle = null;
                        cts.Token.ThrowIfCancellationRequested();
                        if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
                            return;
                        activeOpen = open;
                        if (!retainedSource.TryAcquire(
                                RetainedRasterOperations.StaticImage,
                                out RetainedRasterSourceLease? lease)
                            || lease is null)
                        {
                            DeleteRetainedRasterSource(open.RequestId);
                            await surfacePublishGate.WaitAsync(cts.Token);
                            try
                            {
                                if (string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
                                    await channel.SendAsync(new PreviewError(open.RequestId, "Could not lease raster input."));
                            }
                            finally
                            {
                                surfacePublishGate.Release();
                            }
                            return;
                        }
                        using (lease)
                            if (!await HandleImageOpenAsync(open, lease, cts.Token))
                                DeleteRetainedRasterSource(open.RequestId);
                        return;
                    }
                    await channel.SendAsync(new PreviewError(
                        open.RequestId,
                        "HANDLE preview kind is not supported by RasterHost."));
                    return;
                }
                finally
                {
                    sourceHandle?.Dispose();
                }
            }
            await HandleOpenAsync(open, cts.Token);
        }
        catch (OperationCanceledException)
        {
            DiagLog.Write("RasterHost", $"open canceled: request={open.RequestId}");
        }
        catch (Exception ex)
        {
            DeletePreparedGifDecode(open.RequestId);
            DeleteRetainedRasterSource(open.RequestId);
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
            {
                DeletePreparedGifDecode(open.RequestId);
                DeleteRetainedRasterSource(open.RequestId);
            }
            lock (openCtsLock)
            {
                if (openCts.TryGetValue(open.RequestId, out var current) && ReferenceEquals(current, cts))
                    openCts.Remove(open.RequestId);
            }
            cts.Dispose();
        }
    });
    TrackOpenOrPageWorker(worker);
}

async Task<bool> HandleImageOpenAsync(
    RasterOpen open,
    RetainedRasterSourceLease source,
    CancellationToken cancellationToken)
{
    NativeDecodedImage? image = null;
    bool nativeHandleDecodeAttempted = false;
    if (ShouldPrepareGifAnimation(open))
    {
        // Queue the exact-object first frame before the longer animation decode. The two decoders
        // have independent gates, so the first surface is not delayed by the full frame packet.
        Task<NativeDecodedImage?> firstFrameTask = NativeImageDecoder.TryDecodeHandleAsync(
            source.Handle,
            source.Length,
            source.LogicalName,
            imageDecodeTimeout,
            cancellationToken,
            open.TargetWidth,
            open.TargetHeight);
        nativeHandleDecodeAttempted = true;
        StartPreparedGifHandleDecode(open, source);
        image = await firstFrameTask;
    }
    if (PreferSystemImageDecoder(source.LogicalName))
    {
        image = await DecodeSystemImageHandleWithTimeoutAsync(
            source,
            systemImageDecodeTimeout,
            cancellationToken,
            open.TargetWidth,
            open.TargetHeight);
    }
    if (image is null
        && !nativeHandleDecodeAttempted
        && !NativeImageDecoder.RequiresSystemDecoderHandle(
            source.Handle, source.Length, source.LogicalName)
        && !NativeImageDecoder.SkipNativeHandleFallbackAfterSystemFailure(
            source.Length, source.LogicalName))
    {
        image = await NativeImageDecoder.TryDecodeHandleAsync(
            source.Handle,
            source.Length,
            source.LogicalName,
            imageDecodeTimeout,
            cancellationToken,
            open.TargetWidth,
            open.TargetHeight);
    }
    if (image is null && !PreferSystemImageDecoder(source.LogicalName))
    {
        image = await DecodeSystemImageHandleWithTimeoutAsync(
            source,
            systemImageDecodeTimeout,
            cancellationToken,
            open.TargetWidth,
            open.TargetHeight);
    }
    cancellationToken.ThrowIfCancellationRequested();
    if (image is null)
    {
        DeletePreparedGifDecode(open.RequestId);
        await channel.SendAsync(CreateImagePreviewError(open.RequestId, open.Probe.Extension));
        return false;
    }

    await surfacePublishGate.WaitAsync(cancellationToken);
    try
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
            return false;
        SurfaceTransfer imageHandle = producer.CreatePresentedSurface(image.Bgra, image.Width, image.Height);
        await channel.SendAsync(new PreviewSurface(
            open.RequestId,
            imageHandle.HostHandle,
            (uint)image.Width,
            (uint)image.Height,
            96.0,
            "B8G8R8A8_UNORM")
        {
            TransferId = imageHandle.TransferId,
        });
        string title = image.Width == image.OriginalWidth && image.Height == image.OriginalHeight
            ? Path.GetFileName(open.Probe.Path)
            : $"{Path.GetFileName(open.Probe.Path)} — {image.OriginalWidth}x{image.OriginalHeight} scaled to {image.Width}x{image.Height}";
        await channel.SendAsync(new PreviewReady(
            open.RequestId,
            "image",
            title,
            image.Width,
            image.Height));
    }
    finally
    {
        surfacePublishGate.Release();
    }

    return await PublishImageWaveformAsync(open, image, cancellationToken);
}

async Task<bool> PublishImageWaveformAsync(
    RasterOpen open,
    NativeDecodedImage image,
    CancellationToken cancellationToken)
{
    if (Path.GetExtension(open.Path).Equals(".gif", StringComparison.OrdinalIgnoreCase))
    {
        DiagLog.Write(
            "RasterHost",
            $"GIF RGB waveform intentionally skipped: request={open.RequestId}");
        return true;
    }

    // Native HANDLE decoding can produce this fixed-size analysis while it converts pixels.
    // Compatibility decoders retain the bounded background scan, but readiness and the first
    // surface are always sent before either waveform path is published.
    ImageWaveform waveform = image.Waveform ?? await Task.Run(
        () => ImageWaveformBuilder.Create(image.Bgra, image.Width, image.Height),
        cancellationToken);
    if (!ImageWaveformBuilder.IsValid(waveform))
    {
        DiagLog.Write("RasterHost", $"discarded malformed image waveform: request={open.RequestId}");
        return false;
    }

    await surfacePublishGate.WaitAsync(cancellationToken);
    try
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (!string.Equals(open.RequestId, activeRequestId, StringComparison.Ordinal))
            return false;
        await channel.SendAsync(new PreviewImageWaveform(open.RequestId, waveform));
        return true;
    }
    finally
    {
        surfacePublishGate.Release();
    }
}

static async Task<NativeDecodedImage?> DecodeSystemImageHandleWithTimeoutAsync(
    RetainedRasterSourceLease source,
    TimeSpan timeout,
    CancellationToken cancellationToken,
    uint targetWidth,
    uint targetHeight)
{
    using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
    timeoutCts.CancelAfter(timeout);
    try
    {
        return await SystemImageDecoder.TryDecodeHandleAsync(
            source.Handle,
            source.Length,
            source.LogicalName,
            timeoutCts.Token,
            targetWidth,
            targetHeight);
    }
    catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && timeoutCts.IsCancellationRequested)
    {
        DiagLog.Write("RasterHost", $"system HANDLE image decode timed out; timeout={timeout.TotalMilliseconds:0}ms");
        return null;
    }
}

void StartImageMetadataRead(
    PreviewImageMetadataOpen metadata,
    RetainedRasterSourceLease source)
{
    var request = new ImageMetadataRequestState(metadata.RequestId, metadata.PreviewRequestId, source);
    if (!metadataRequests.TryAdd(metadata.RequestId, request))
    {
        request.Dispose();
        _ = channel.SendAsync(new PreviewError(
            metadata.RequestId,
            "Image metadata request is already active."));
        return;
    }

    request.Worker = Task.Run(async () =>
    {
        try
        {
            Task<NativeImageMetadataResult> nativeTask =
                NativeImageMetadataReader.TryReadHandleAsync(
                    request.Source.Handle,
                    request.Source.Length,
                    request.Source.LogicalName,
                    imageMetadataTimeout,
                    request.Cancellation.Token);
            Task<ImageMetadata?> propertyHandlerTask =
                WindowsPropertyHandlerMetadataReader.TryReadHandleAsync(
                    request.Source.Handle,
                    request.Source.Length,
                    request.Source.LogicalName,
                    imageMetadataTimeout,
                    request.Cancellation.Token);
            Task<ImageMetadata?> systemTask =
                SystemImageMetadataReader.TryReadHandleAsync(
                    request.Source.Handle,
                    request.Source.Length,
                    request.Source.LogicalName,
                    imageMetadataTimeout,
                    request.Cancellation.Token);
            await Task.WhenAll(nativeTask, propertyHandlerTask, systemTask);
            NativeImageMetadataResult result = await nativeTask;
            ImageMetadata? metadataResult = SystemImageMetadataReader.Merge(
                WindowsPropertyHandlerMetadataReader.Merge(
                    result.Metadata,
                    await propertyHandlerTask),
                await systemTask);
            request.Cancellation.Token.ThrowIfCancellationRequested();
            if (!metadataRequests.TryGetValue(metadata.RequestId, out var current)
                || !ReferenceEquals(current, request))
            {
                return;
            }

            if (metadataResult is not null)
            {
                await channel.SendAsync(new PreviewImageMetadataReady(
                    metadata.RequestId,
                    metadata.PreviewRequestId,
                    metadataResult),
                    request.Cancellation.Token);
            }
            else if (!result.IsSupported)
            {
                await channel.SendAsync(new PreviewError(
                    metadata.RequestId,
                    "Image metadata is not available in this RasterHost."),
                    request.Cancellation.Token);
            }
            else
            {
                await channel.SendAsync(new PreviewError(
                    metadata.RequestId,
                    NativeImageMetadataReader.DescribeStatus(result.Status)),
                    request.Cancellation.Token);
            }
        }
        catch (OperationCanceledException)
        {
            DiagLog.Write(
                "RasterHost",
                $"image metadata canceled: request={metadata.RequestId} parent={metadata.PreviewRequestId}");
        }
        catch (Exception ex)
        {
            DiagLog.Write(
                "RasterHost",
                $"image metadata failed: request={metadata.RequestId} parent={metadata.PreviewRequestId}: {ex}");
            if (metadataRequests.TryGetValue(metadata.RequestId, out var current)
                && ReferenceEquals(current, request)
                && !request.Cancellation.IsCancellationRequested)
            {
                try
                {
                    await channel.SendAsync(new PreviewError(
                        metadata.RequestId,
                        "Image metadata extraction failed."),
                        request.Cancellation.Token);
                }
                catch { }
            }
        }
        finally
        {
            ((ICollection<KeyValuePair<string, ImageMetadataRequestState>>)metadataRequests)
                .Remove(new KeyValuePair<string, ImageMetadataRequestState>(metadata.RequestId, request));
            request.Dispose();
        }
    });
}

async Task CloseImageMetadataAsync(string requestId)
{
    if (metadataRequests.TryRemove(requestId, out ImageMetadataRequestState? request))
    {
        request.Cancel();
        await request.Worker;
    }
}

void StartPreparedGifHandleDecode(RasterOpen open, RetainedRasterSourceLease source)
{
    PreparedGifDecodeState? state = null;
    try
    {
        state = PreparedGifDecodeState.StartHandle(
            source.Handle,
            source.Length,
            source.LogicalName,
            open.TargetWidth,
            open.TargetHeight);
        if (preparedGifDecodes.TryAdd(open.RequestId, state))
            return;
    }
    catch (Exception ex) when (ex is IOException
        or ObjectDisposedException
        or UnauthorizedAccessException
        or System.ComponentModel.Win32Exception)
    {
        DiagLog.Write(
            "RasterHost",
            $"could not start concurrent GIF HANDLE decode request={open.RequestId}: {ex.Message}");
    }
    state?.CancelAndDispose();
}

void StartPreparedGifPathDecode(RasterOpen open)
{
    var state = PreparedGifDecodeState.StartPath(
        open.Path,
        open.TargetWidth,
        open.TargetHeight);
    if (!preparedGifDecodes.TryAdd(open.RequestId, state))
        state.CancelAndDispose();
}

async Task<PreparedGifDecodeState?> TryTakePreparedGifDecodeAsync(
    PreviewAnimationFramesOpen animation)
{
    if (!preparedGifDecodes.TryGetValue(
            animation.PreviewRequestId,
            out PreparedGifDecodeState? candidate))
        return null;

    if (!candidate.MatchesTarget(animation.TargetWidth, animation.TargetHeight))
    {
        if (preparedGifDecodes.TryRemove(
                animation.PreviewRequestId,
                out PreparedGifDecodeState? mismatched))
            await mismatched.CancelAndDisposeAsync();
        DiagLog.Write(
            "RasterHost",
            $"discarded prepared GIF packet for target mismatch: parent={animation.PreviewRequestId}; " +
            $"prepared={candidate.TargetWidth}x{candidate.TargetHeight}; " +
            $"requested={animation.TargetWidth}x{animation.TargetHeight}");
        return null;
    }

    if (preparedGifDecodes.TryRemove(
            animation.PreviewRequestId,
            out PreparedGifDecodeState? matched))
    {
        return matched;
    }
    return null;
}

void StartPreparedAnimationHandoff(
    PreviewAnimationFramesOpen animation,
    PreparedGifDecodeState preparedDecode)
{
    if (animationPackets.ContainsKey(animation.RequestId))
    {
        preparedDecode.CancelAndDispose();
        _ = channel.SendAsync(new PreviewError(
            animation.RequestId,
            "Animation frame packet has not been released."));
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
        preparedDecode.CancelAndDispose();
        _ = channel.SendAsync(new PreviewError(animation.RequestId, "Duplicate animation request ID."));
        return;
    }
    animationParents[animation.RequestId] = animation.PreviewRequestId;

    _ = Task.Run(async () =>
    {
        NativeAnimationPacket? packet = null;
        try
        {
            packet = await preparedDecode.TakeAsync(cts.Token);
            cts.Token.ThrowIfCancellationRequested();
            if (packet is null)
            {
                await channel.SendAsync(new PreviewError(
                    animation.RequestId,
                    "Prepared GIF frame decode failed."));
                return;
            }
            DiagLog.Write(
                "RasterHost",
                $"reused concurrently prepared GIF packet: request={animation.RequestId}; " +
                $"frames={packet.FrameCount}; size={packet.Width}x{packet.Height}; bytes={packet.PacketLength}; " +
                $"decode={preparedDecode.ElapsedMilliseconds}ms");
            await gate.WaitAsync();
            try
            {
                cts.Token.ThrowIfCancellationRequested();
                if (!string.Equals(animation.PreviewRequestId, activeRequestId, StringComparison.Ordinal))
                    return;

                animationPackets[animation.RequestId] = packet;
                try
                {
                    await channel.SendAsync(new PreviewAnimationFramesReady(
                        animation.RequestId,
                        animation.PreviewRequestId,
                        packet.Section.Handle.DangerousGetHandle().ToInt64(),
                        packet.FrameCount,
                        packet.Width,
                        packet.Height,
                        packet.PacketLength));
                    packet = null;
                }
                catch
                {
                    if (animationPackets.TryRemove(
                            animation.RequestId,
                            out NativeAnimationPacket? failed))
                        failed.Dispose();
                    throw;
                }
            }
            finally
            {
                gate.Release();
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception ex)
        {
            DiagLog.Write(
                "RasterHost",
                $"prepared GIF handoff failed request={animation.RequestId}: {ex}");
            try { await channel.SendAsync(new PreviewError(animation.RequestId, ex.Message)); } catch { }
        }
        finally
        {
            packet?.Dispose();
            if (animationCts.TryRemove(animation.RequestId, out var current))
                current.Dispose();
            if (!animationPackets.ContainsKey(animation.RequestId))
                animationParents.TryRemove(animation.RequestId, out _);
            animationHandoffGates.TryRemove(animation.RequestId, out _);
        }
    });
}

void StartAnimationDecode(
    PreviewAnimationFramesOpen animation,
    string? path,
    RetainedRasterSourceLease? source)
{
    if (animationPackets.ContainsKey(animation.RequestId))
    {
        _ = channel.SendAsync(new PreviewError(animation.RequestId, "Animation frame packet has not been released."));
        source?.Dispose();
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
        source?.Dispose();
        _ = channel.SendAsync(new PreviewError(animation.RequestId, "Duplicate animation request ID."));
        return;
    }
    animationParents[animation.RequestId] = animation.PreviewRequestId;

    _ = Task.Run(async () =>
    {
        NativeAnimationPacket? packet = null;
        try
        {
            packet = source is null
                ? await NativeAnimationPacketDecoder.TryDecodeAsync(
                    path!, animation.TargetWidth, animation.TargetHeight, cts.Token)
                : await NativeAnimationPacketDecoder.TryDecodeHandleAsync(
                    source.Handle,
                    source.Length,
                    source.LogicalName,
                    animation.TargetWidth,
                    animation.TargetHeight,
                    cts.Token);
            cts.Token.ThrowIfCancellationRequested();
            if (packet is null)
            {
                await channel.SendAsync(new PreviewError(animation.RequestId, "Animation frame decode failed."));
                return;
            }
            await gate.WaitAsync();
            try
            {
                cts.Token.ThrowIfCancellationRequested();
                if (!string.Equals(animation.PreviewRequestId, activeRequestId, StringComparison.Ordinal))
                    return;

                animationPackets[animation.RequestId] = packet;
                try
                {
                    await channel.SendAsync(new PreviewAnimationFramesReady(
                        animation.RequestId,
                        animation.PreviewRequestId,
                        packet.Section.Handle.DangerousGetHandle().ToInt64(),
                        packet.FrameCount,
                        packet.Width,
                        packet.Height,
                        packet.PacketLength));
                    packet = null; // The handoff dictionary owns it until close or disconnect.
                }
                catch
                {
                    if (animationPackets.TryRemove(animation.RequestId, out NativeAnimationPacket? failed))
                        failed.Dispose();
                    throw;
                }
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
            source?.Dispose();
            packet?.Dispose();
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
            packet.Dispose();
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
    try
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (IsPdf(open.Probe))
        {
            await OpenPdfSessionAsync(open, () => PdfPreviewSession.OpenAsync(open.Path), cancellationToken);
            return;
        }

        if (IsImage(open.Probe))
        {
            Task<NativeDecodedImage?> imageTask = DecodeImageAsync(
                open.Path,
                imageDecodeTimeout,
                systemImageDecodeTimeout,
                cancellationToken,
                open.TargetWidth,
                open.TargetHeight);
            if (ShouldPrepareGifAnimation(open))
                StartPreparedGifPathDecode(open);

            NativeDecodedImage? image = await imageTask;
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
                    { TransferId = imageHandle.TransferId });
                    string title = image.Width == image.OriginalWidth && image.Height == image.OriginalHeight
                        ? Path.GetFileName(open.Probe.Path)
                        : $"{Path.GetFileName(open.Probe.Path)} — {image.OriginalWidth}x{image.OriginalHeight} scaled to {image.Width}x{image.Height}";
                    await channel.SendAsync(new PreviewReady(open.RequestId, "image", title, image.Width, image.Height));
                }
                finally
                {
                    surfacePublishGate.Release();
                }
                await PublishImageWaveformAsync(open, image, cancellationToken);
                return;
            }

            DiagLog.Write("RasterHost", "path image decode returned no raster");
            DeletePreparedGifDecode(open.RequestId);
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

async Task OpenPdfSessionAsync(
    RasterOpen open,
    Func<Task<PdfPreviewSession>> openSession,
    CancellationToken cancellationToken)
{
    if (pdfSessions.TryRemove(open.RequestId, out var old))
        await DisposePdfSessionAsync(old, open.RequestId);
    PdfPreviewSession? session = await openSession();
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
                if (pdfSessions.TryRemove(open.RequestId, out var failed))
                    await DisposePdfSessionAsync(failed, open.RequestId);
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
            await DisposePdfSessionAsync(session, open.RequestId);
    }
}

static async Task DisposePdfSessionAsync(PdfPreviewSession session, string requestId)
{
    try { await session.DisposeAsync(); }
    catch (TimeoutException)
    {
        DiagLog.Write("RasterHost", $"PDF render drain timed out; exiting host: request={requestId}");
        Environment.Exit(31);
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
        await surfacePublishGate.WaitAsync(pageCts.Token);
        try
        {
            pageCts.Token.ThrowIfCancellationRequested();
            if (!string.Equals(page.RequestId, activeRequestId, StringComparison.Ordinal))
                return;
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
        finally
        {
            surfacePublishGate.Release();
        }
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

static bool ShouldPrepareGifAnimation(RasterOpen open)
    => open.PrepareAnimation
       && open.Probe.Kind.Equals("image", StringComparison.OrdinalIgnoreCase)
       && Path.GetExtension(open.Path).Equals(".gif", StringComparison.OrdinalIgnoreCase)
       && open.Probe.IsAnimated is not false;

void DeletePreparedGifDecode(string requestId)
{
    if (preparedGifDecodes.TryRemove(requestId, out PreparedGifDecodeState? state))
        state.CancelAndDispose();
}

void DeleteRetainedRasterSource(string requestId)
{
    if (retainedRasterSources.TryRemove(requestId, out var source))
        source.Dispose();
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
    uint TargetHeight,
    bool PrepareAnimation);

internal sealed class PreparedGifDecodeState
{
    private readonly CancellationTokenSource _cancellation;
    private readonly Task<NativeAnimationPacket?> _worker;
    private readonly long _startedTimestamp;
    private int _claimed;

    private PreparedGifDecodeState(
        uint targetWidth,
        uint targetHeight,
        CancellationTokenSource cancellation,
        Task<NativeAnimationPacket?> worker,
        long startedTimestamp)
    {
        TargetWidth = targetWidth;
        TargetHeight = targetHeight;
        _cancellation = cancellation;
        _worker = worker;
        _startedTimestamp = startedTimestamp;
    }

    public uint TargetWidth { get; }
    public uint TargetHeight { get; }
    public long ElapsedMilliseconds
        => Math.Max(0, (long)Stopwatch.GetElapsedTime(_startedTimestamp).TotalMilliseconds);

    public static PreparedGifDecodeState StartHandle(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        uint targetWidth,
        uint targetHeight)
    {
        SafeFileHandle decodeHandle = WindowsHandleTransfer.ReopenReadOnlyFile(
            sourceHandle,
            sourceLength);
        var cancellation = new CancellationTokenSource();
        long started = Stopwatch.GetTimestamp();
        Task<NativeAnimationPacket?> worker = DecodeHandleAsync(
            decodeHandle,
            sourceLength,
            logicalName,
            targetWidth,
            targetHeight,
            cancellation.Token);
        return new PreparedGifDecodeState(
            targetWidth,
            targetHeight,
            cancellation,
            worker,
            started);
    }

    public static PreparedGifDecodeState StartPath(
        string path,
        uint targetWidth,
        uint targetHeight)
    {
        var cancellation = new CancellationTokenSource();
        long started = Stopwatch.GetTimestamp();
        Task<NativeAnimationPacket?> worker = DecodePathAsync(
            path,
            targetWidth,
            targetHeight,
            cancellation.Token);
        return new PreparedGifDecodeState(
            targetWidth,
            targetHeight,
            cancellation,
            worker,
            started);
    }

    public bool MatchesTarget(uint targetWidth, uint targetHeight)
        => TargetWidth == targetWidth && TargetHeight == targetHeight;

    public async Task<NativeAnimationPacket?> TakeAsync(CancellationToken cancellationToken)
    {
        if (Interlocked.Exchange(ref _claimed, 1) != 0)
            return null;

        CancellationTokenRegistration registration = cancellationToken.Register(
            static state =>
            {
                try { ((CancellationTokenSource)state!).Cancel(); }
                catch (ObjectDisposedException) { }
            },
            _cancellation);
        try
        {
            return await _worker;
        }
        catch (OperationCanceledException) when (_cancellation.IsCancellationRequested)
        {
            return null;
        }
        finally
        {
            registration.Dispose();
            _cancellation.Dispose();
        }
    }

    public void CancelAndDispose()
        => _ = CancelAndDisposeAsync();

    public async Task CancelAndDisposeAsync()
    {
        if (Interlocked.Exchange(ref _claimed, 1) != 0)
            return;
        try { _cancellation.Cancel(); }
        catch (ObjectDisposedException) { }
        await DisposeWorkerAsync();
    }

    private async Task DisposeWorkerAsync()
    {
        try
        {
            NativeAnimationPacket? packet = await _worker;
            packet?.Dispose();
        }
        catch (OperationCanceledException)
        {
        }
        finally
        {
            _cancellation.Dispose();
        }
    }

    private static async Task<NativeAnimationPacket?> DecodeHandleAsync(
        SafeFileHandle handle,
        long sourceLength,
        string logicalName,
        uint targetWidth,
        uint targetHeight,
        CancellationToken cancellationToken)
    {
        using (handle)
        {
            return await NativeAnimationPacketDecoder.TryDecodeHandleAsync(
                handle,
                sourceLength,
                logicalName,
                targetWidth,
                targetHeight,
                cancellationToken);
        }
    }

    private static async Task<NativeAnimationPacket?> DecodePathAsync(
        string path,
        uint targetWidth,
        uint targetHeight,
        CancellationToken cancellationToken)
        => await NativeAnimationPacketDecoder.TryDecodeAsync(
            path,
            targetWidth,
            targetHeight,
            cancellationToken);
}

internal sealed class ImageMetadataRequestState(
    string requestId,
    string previewRequestId,
    RetainedRasterSourceLease source) : IDisposable
{
    private int _disposed;

    public string RequestId { get; } = requestId;
    public string PreviewRequestId { get; } = previewRequestId;
    public RetainedRasterSourceLease Source { get; } = source;
    public CancellationTokenSource Cancellation { get; } = new();
    public Task Worker { get; set; } = Task.CompletedTask;

    public void Cancel()
    {
        try { Cancellation.Cancel(); }
        catch (ObjectDisposedException) { }
    }

    public void Dispose()
    {
        if (Interlocked.Exchange(ref _disposed, 1) != 0)
            return;
        Source.Dispose();
        Cancellation.Dispose();
    }
}
