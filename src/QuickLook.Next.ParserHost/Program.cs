using System.Collections.Concurrent;
using System.IO.Pipes;
using QuickLook.Next.Core;
using QuickLook.Next.ParserHost;

SupervisedHostProcessPolicy.SuppressInteractiveErrorUi();

string pipeName = GetArg(args, "--pipe") ?? "quicklook_next_parser";
string? sessionToken = GetArg(args, "--session-token");
string writableRoot = GetArg(args, "--writable-root") ?? "";
if (!Path.IsPathFullyQualified(writableRoot) || !Directory.Exists(writableRoot)
    || (File.GetAttributes(writableRoot) & FileAttributes.ReparsePoint) != 0) return;
string logRoot = Path.Combine(writableRoot, "logs");
if (!Directory.Exists(logRoot) || (File.GetAttributes(logRoot) & FileAttributes.ReparsePoint) != 0) return;

DiagLog.InitInDirectory(logRoot, "parser-host.log");
DiagLog.Write("ParserHost", $"start pid={Environment.ProcessId} pipe={pipeName}");

using var pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
PipeChannel channel;
try
{
    await pipe.ConnectAsync(15_000);
    channel = new PipeChannel(pipe);
    DiagLog.Write("ParserHost", "pipe connected");
}
catch (Exception ex)
{
    DiagLog.Write("ParserHost", "pipe connect FAILED: " + ex);
    return;
}

using var channelLifetime = channel;
try { ParserNativePreview.EnsureCompatible(); }
catch (Exception ex)
{
    DiagLog.Write("ParserHost", "native ABI check failed: " + ex.Message);
    return;
}
DiagLog.Write("ParserHost", "native ABI ready");
ProcessPowerMode.SetCurrentBackgroundEfficiency(enabled: true, "ParserHost");

var requests = new ConcurrentDictionary<string, CancellationTokenSource>();
var closedArchiveEntries = new ConcurrentDictionary<string, byte>();
var archiveHandoffGates = new ConcurrentDictionary<string, SemaphoreSlim>();
var heroRasters = new ConcurrentDictionary<string, NativeRasterSection>();
var heroHandoffGates = new ConcurrentDictionary<string, SemaphoreSlim>();
var officeImageRasters = new ConcurrentDictionary<string, NativeRasterSection>();
var officeImageHandoffGates = new ConcurrentDictionary<string, SemaphoreSlim>();
var officeImageParents = new ConcurrentDictionary<string, string>();
var retainedPreviewSources = new ConcurrentDictionary<string, RetainedPreviewSource>();
bool authenticated = false;
string? activePreviewRequestId = null;

while (true)
{
    ControlMessage? message;
    try { message = await channel.ReceiveAsync(); }
    catch (Exception ex) { DiagLog.Write("ParserHost", "receive ended: " + ex.Message); break; }
    if (message is null) break;

    switch (message)
    {
        case Hello hello when !authenticated:
            if (string.IsNullOrWhiteSpace(sessionToken)
                || !string.Equals(hello.SessionToken, sessionToken, StringComparison.Ordinal))
            {
                DiagLog.Write("ParserHost", "rejected unauthenticated pipe client");
                return;
            }
            try
            {
                WindowsHandleTransfer.VerifyNamedPipeServerProcess(pipe.SafePipeHandle, hello.AppProcessId);
            }
            catch (Exception ex)
            {
                DiagLog.Write("ParserHost", "rejected App process identity: " + ex.Message);
                return;
            }
            authenticated = true;
            await channel.SendAsync(new ParserReady());
            DiagLog.Write("ParserHost", "authenticated; sent parser.ready");
            break;

        case var _ when !authenticated:
            DiagLog.Write("ParserHost", "rejected control message before authentication");
            return;

        case Hello:
            DiagLog.Write("ParserHost", "rejected repeated authentication");
            return;

        case PreviewOpen open when IsValidRequestId(open.RequestId)
                                   && !string.IsNullOrWhiteSpace(open.Path)
                                   && IsValidProbe(open.Probe):
            if (string.Equals(
                    activePreviewRequestId,
                    open.RequestId,
                    StringComparison.Ordinal)
                || requests.ContainsKey(open.RequestId))
            {
                if (string.Equals(
                    activePreviewRequestId,
                    open.RequestId,
                    StringComparison.Ordinal))
                {
                    Cancel(open.RequestId);
                    await CloseOfficeImagesForParentAsync(open.RequestId);
                    DeleteRetainedPreviewSource(open.RequestId);
                }
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }
            if (activePreviewRequestId is not null)
            {
                Cancel(activePreviewRequestId);
                await CloseOfficeImagesForParentAsync(activePreviewRequestId);
                DeleteRetainedPreviewSource(activePreviewRequestId);
            }
            var cts = new CancellationTokenSource();
            if (!requests.TryAdd(open.RequestId, cts))
            {
                cts.Dispose();
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }
            activePreviewRequestId = open.RequestId;
            _ = Task.Run(async () =>
            {
                try
                {
                    string kind = open.Probe.Kind.ToLowerInvariant();
                    if (!PreviewFormatPolicy.UsesParserHost(kind))
                    {
                        await channel.SendAsync(new PreviewError(open.RequestId, "Unsupported ParserHost preview kind."));
                        return;
                    }
                    if (kind == "certificate")
                    {
                        await channel.SendAsync(CertificatePreview.Create(open.RequestId, open.Path, open.Probe.Size));
                        return;
                    }
                    string? json = ParserNativePreview.TryPreview(kind, open.Path, open.Probe, cts.Token);
                    cts.Token.ThrowIfCancellationRequested();
                    if (!PreviewReadyJson.TryParse(open.RequestId, json ?? "", out PreviewReady? ready, out string? error))
                        await channel.SendAsync(new PreviewError(open.RequestId, error ?? "Native parser returned no preview."));
                    else
                        await channel.SendAsync(ready!);
                }
                catch (OperationCanceledException) { }
                catch (Exception ex)
                {
                    DiagLog.Write("ParserHost", $"open failed request={open.RequestId}: {ex}");
                    try { await channel.SendAsync(new PreviewError(open.RequestId, ex.Message)); } catch { }
                }
                finally
                {
                    if (requests.TryRemove(open.RequestId, out var current))
                        current.Dispose();
                }
            });
            break;

        case PreviewOpenHandle open:
            Microsoft.Win32.SafeHandles.SafeFileHandle sourceHandle;
            try
            {
                sourceHandle = WindowsHandleTransfer.TakeLocalFileHandle(open.SourceHandle, open.SourceLength);
            }
            catch (Exception ex)
            {
                if (IsValidRequestId(open.RequestId))
                    await channel.SendAsync(new PreviewError(open.RequestId, ex.Message));
                else
                    DiagLog.Write("ParserHost", "rejected invalid handle preview request");
                break;
            }
            long maxHandleInputLength = string.Equals(
                open.Probe?.Kind,
                "archive",
                StringComparison.OrdinalIgnoreCase)
                ? NativeAbi.MaxArchiveHandleInputBytes
                : NativeAbi.MaxParserHandleInputBytes;
            if (!IsValidRequestId(open.RequestId)
                || open.SourceLength < 0
                || open.SourceLength > maxHandleInputLength
                || string.IsNullOrWhiteSpace(open.LogicalPath)
                || !IsValidProbe(open.Probe)
                || open.Probe.Size != open.SourceLength)
            {
                sourceHandle.Dispose();
                if (IsValidRequestId(open.RequestId))
                    await channel.SendAsync(new PreviewError(open.RequestId, "Invalid handle preview request."));
                else
                    DiagLog.Write("ParserHost", "rejected invalid handle preview request");
                break;
            }
            if (string.Equals(
                    activePreviewRequestId,
                    open.RequestId,
                    StringComparison.Ordinal)
                || requests.ContainsKey(open.RequestId))
            {
                if (string.Equals(
                    activePreviewRequestId,
                    open.RequestId,
                    StringComparison.Ordinal))
                {
                    Cancel(open.RequestId);
                    await CloseOfficeImagesForParentAsync(open.RequestId);
                    DeleteRetainedPreviewSource(open.RequestId);
                }
                sourceHandle.Dispose();
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }
            if (activePreviewRequestId is not null)
            {
                Cancel(activePreviewRequestId);
                await CloseOfficeImagesForParentAsync(activePreviewRequestId);
                DeleteRetainedPreviewSource(activePreviewRequestId);
            }
            var handleCts = new CancellationTokenSource();
            if (!requests.TryAdd(open.RequestId, handleCts))
            {
                sourceHandle.Dispose();
                handleCts.Dispose();
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }
            activePreviewRequestId = open.RequestId;
            _ = Task.Run(async () =>
            {
                var ownedSourceHandle = sourceHandle;
                bool sourceRetained = false;
                bool published = false;
                try
                {
                    string kind = open.Probe.Kind.ToLowerInvariant();
                    if (!PreviewFormatPolicy.UsesParserHost(kind))
                    {
                        await channel.SendAsync(new PreviewError(open.RequestId, "Unsupported ParserHost preview kind."));
                        return;
                    }
                    if (kind == "certificate")
                    {
                        PreviewReady certificateReady = await CertificatePreview.CreateFromHandleAsync(
                            open.RequestId,
                            open.LogicalPath,
                            ownedSourceHandle,
                            open.SourceLength,
                            handleCts.Token);
                        handleCts.Token.ThrowIfCancellationRequested();
                        await channel.SendAsync(certificateReady);
                        published = true;
                        return;
                    }
                    if (ParserNativePreview.UsesHandleInput(kind))
                    {
                        var handleResult = ParserNativePreview.TryPreviewHandle(
                            kind, ownedSourceHandle, open.SourceLength, open.LogicalPath, handleCts.Token);
                        handleCts.Token.ThrowIfCancellationRequested();
                        if (handleResult.Status != NativeAbi.StatusOk || handleResult.Json is null)
                        {
                            string failure = ParserNativePreview.DescribeHandleFailure(handleResult.Status);
                            DiagLog.Write(
                                "ParserHost",
                                $"native handle preview failed request={open.RequestId} status={handleResult.Status}");
                            await channel.SendAsync(new PreviewError(open.RequestId, failure));
                        }
                        else if (!PreviewReadyJson.TryParse(open.RequestId, handleResult.Json, out PreviewReady? handleReady, out string? handleError))
                            await channel.SendAsync(new PreviewError(open.RequestId, handleError ?? "Native handle parser returned no preview."));
                        else
                        {
                            if (kind is "archive" or "ebook" or "office" or "package")
                            {
                                string logicalName = Path.GetFileName(open.LogicalPath);
                                IReadOnlyDictionary<string, long>? officeLayoutImages = null;
                                RetainedPreviewFollowUps followUps;
                                if (kind == "office")
                                {
                                    if (!TryCollectOfficeLayoutImages(
                                        handleReady!,
                                        out Dictionary<string, long> collectedImages))
                                    {
                                        await channel.SendAsync(new PreviewError(
                                            open.RequestId,
                                            "Native Office layout returned invalid image references."));
                                        return;
                                    }
                                    officeLayoutImages = collectedImages;
                                    followUps = RetainedPreviewFollowUps.OfficeHero;
                                    if (collectedImages.Count > 0)
                                        followUps |= RetainedPreviewFollowUps.OfficeLayoutImage;
                                }
                                else
                                {
                                    followUps =
                                        kind == "package"
                                            ? RetainedPreviewFollowUps.PackageHero
                                            : string.Equals(
                                                handleReady?.Listing?.ListingKind,
                                                "archive",
                                                StringComparison.OrdinalIgnoreCase)
                                            && handleReady?.Listing?.CanPreviewEntries == true
                                            ? RetainedPreviewFollowUps.ArchiveEntry
                                            : RetainedPreviewFollowUps.None;
                                }
                                if (followUps != RetainedPreviewFollowUps.None)
                                {
                                    var retainedSource = new RetainedPreviewSource(
                                        ownedSourceHandle,
                                        open.SourceLength,
                                        logicalName,
                                        kind,
                                        followUps,
                                        officeLayoutImages);
                                    if (!retainedPreviewSources.TryAdd(open.RequestId, retainedSource))
                                    {
                                        await channel.SendAsync(new PreviewError(
                                            open.RequestId,
                                            "Could not retain preview source."));
                                        return;
                                    }
                                    sourceRetained = true;
                                    handleCts.Token.ThrowIfCancellationRequested();
                                }
                            }
                            await channel.SendAsync(handleReady!);
                            published = true;
                        }
                        return;
                    }
                    await channel.SendAsync(new PreviewError(
                        open.RequestId,
                        "HANDLE preview kind is not supported by ParserHost."));
                }
                catch (OperationCanceledException) { }
                catch (Exception ex)
                {
                    DiagLog.Write("ParserHost", $"handle open failed request={open.RequestId}: {ex}");
                    try { await channel.SendAsync(new PreviewError(open.RequestId, ex.Message)); } catch { }
                }
                finally
                {
                    if (requests.TryRemove(open.RequestId, out var current)) current.Dispose();
                    if (!published)
                    {
                        DeleteRetainedPreviewSource(open.RequestId);
                    }
                    if (!sourceRetained)
                        ownedSourceHandle.Dispose();
                }
            });
            break;

        case PreviewOpenSqliteHandles open:
            OwnedSqliteFileHandles sqliteHandles;
            try
            {
                sqliteHandles = WindowsHandleTransfer.TakeLocalSqliteFileHandles(
                    open.MainHandle,
                    open.MainLength,
                    open.WalHandle,
                    open.WalLength,
                    open.ShmHandle,
                    open.ShmLength);
            }
            catch (Exception ex)
            {
                if (IsValidRequestId(open.RequestId))
                    await channel.SendAsync(new PreviewError(open.RequestId, ex.Message));
                else
                    DiagLog.Write("ParserHost", "rejected invalid SQLite handle preview request");
                break;
            }
            if (!IsValidRequestId(open.RequestId)
                || open.MainLength is not (>= 0 and <= NativeAbi.MaxParserHandleInputBytes)
                || open.WalLength is not (>= 0 and <= NativeAbi.MaxSqliteWalBytes)
                || open.ShmLength is not (>= 0 and <= NativeAbi.MaxSqliteShmBytes)
                || string.IsNullOrWhiteSpace(open.LogicalPath)
                || !IsValidProbe(open.Probe)
                || !open.Probe.Kind.Equals("database", StringComparison.OrdinalIgnoreCase)
                || open.Probe.Size != open.MainLength)
            {
                sqliteHandles.Dispose();
                if (IsValidRequestId(open.RequestId))
                    await channel.SendAsync(new PreviewError(open.RequestId, "Invalid SQLite handle preview request."));
                else
                    DiagLog.Write("ParserHost", "rejected invalid SQLite handle preview request");
                break;
            }
            if (string.Equals(
                    activePreviewRequestId,
                    open.RequestId,
                    StringComparison.Ordinal)
                || requests.ContainsKey(open.RequestId))
            {
                if (string.Equals(
                    activePreviewRequestId,
                    open.RequestId,
                    StringComparison.Ordinal))
                {
                    Cancel(open.RequestId);
                    await CloseOfficeImagesForParentAsync(open.RequestId);
                    DeleteRetainedPreviewSource(open.RequestId);
                }
                sqliteHandles.Dispose();
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }
            if (activePreviewRequestId is not null)
            {
                Cancel(activePreviewRequestId);
                await CloseOfficeImagesForParentAsync(activePreviewRequestId);
                DeleteRetainedPreviewSource(activePreviewRequestId);
            }
            var sqliteCts = new CancellationTokenSource();
            if (!requests.TryAdd(open.RequestId, sqliteCts))
            {
                sqliteHandles.Dispose();
                sqliteCts.Dispose();
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }
            activePreviewRequestId = open.RequestId;
            _ = Task.Run(async () =>
            {
                using var ownedHandles = sqliteHandles;
                try
                {
                    var handleResult = ParserNativePreview.TryPreviewSqliteHandles(
                        ownedHandles.Main,
                        open.MainLength,
                        ownedHandles.Wal,
                        open.WalLength,
                        ownedHandles.Shm,
                        open.ShmLength,
                        open.LogicalPath,
                        sqliteCts.Token);
                    sqliteCts.Token.ThrowIfCancellationRequested();
                    if (handleResult.Status != NativeAbi.StatusOk || handleResult.Json is null)
                    {
                        string failure = ParserNativePreview.DescribeHandleFailure(handleResult.Status);
                        DiagLog.Write(
                            "ParserHost",
                            $"native SQLite handle preview failed request={open.RequestId} status={handleResult.Status}");
                        await channel.SendAsync(new PreviewError(open.RequestId, failure));
                    }
                    else if (!PreviewReadyJson.TryParse(
                        open.RequestId,
                        handleResult.Json,
                        out PreviewReady? ready,
                        out string? error))
                    {
                        await channel.SendAsync(new PreviewError(
                            open.RequestId,
                            error ?? "Native SQLite handle parser returned no preview."));
                    }
                    else
                    {
                        await channel.SendAsync(ready!);
                    }
                }
                catch (OperationCanceledException) { }
                catch (Exception ex)
                {
                    DiagLog.Write("ParserHost", $"SQLite handle open failed request={open.RequestId}: {ex}");
                    try { await channel.SendAsync(new PreviewError(open.RequestId, ex.Message)); } catch { }
                }
                finally
                {
                    if (requests.TryRemove(open.RequestId, out var current))
                        current.Dispose();
                }
            });
            break;

        case PreviewClose close when IsValidRequestId(close.RequestId):
            Cancel(close.RequestId);
            await CloseOfficeImagesForParentAsync(close.RequestId);
            DeleteRetainedPreviewSource(close.RequestId);
            break;

        case ArchiveEntryExtract extract:
            Microsoft.Win32.SafeHandles.SafeFileHandle outputHandle;
            try
            {
                // OutputHandle ownership transfers with the message. Adopt it before validating
                // any other field so malformed requests cannot leak a host-local HANDLE.
                outputHandle = WindowsHandleTransfer.TakeLocalFileHandle(extract.OutputHandle, 0);
            }
            catch (Exception ex)
            {
                if (IsValidRequestId(extract.RequestId))
                    await channel.SendAsync(new PreviewError(extract.RequestId, ex.Message));
                else
                    DiagLog.Write("ParserHost", "rejected invalid archive output HANDLE request");
                break;
            }
            if (!IsValidRequestId(extract.RequestId)
                || string.IsNullOrWhiteSpace(extract.EntryPath)
                || extract.OutputCapacity is <= 0 or > NativeAbi.MaxArchiveEntryOutputBytes)
            {
                outputHandle.Dispose();
                if (IsValidRequestId(extract.RequestId))
                    await channel.SendAsync(new PreviewError(extract.RequestId, "Invalid archive extraction request."));
                else
                    DiagLog.Write("ParserHost", "rejected invalid archive extraction request");
                break;
            }
            RetainedPreviewSourceLease? retainedArchiveLease = null;
            if (extract.ParentPreviewRequestId is { } parentRequestId)
            {
                if (!IsValidRequestId(parentRequestId)
                    || string.Equals(parentRequestId, extract.RequestId, StringComparison.Ordinal)
                    || !retainedPreviewSources.TryGetValue(parentRequestId, out RetainedPreviewSource? retainedArchiveSource)
                    || retainedArchiveSource is null
                    || !retainedArchiveSource.TryAcquire(
                        RetainedPreviewFollowUps.ArchiveEntry,
                        out retainedArchiveLease)
                    || retainedArchiveLease is null)
                {
                    outputHandle.Dispose();
                    await channel.SendAsync(new PreviewError(
                        extract.RequestId,
                        "Parent archive preview source is unavailable."));
                    break;
                }
                nint outputRawHandle = outputHandle.DangerousGetHandle();
                if (outputRawHandle == retainedArchiveSource.Handle.DangerousGetHandle()
                    || outputRawHandle == retainedArchiveLease.Handle.DangerousGetHandle())
                {
                    // This raw value was not transferred as a new owner; avoid closing the
                    // retained parent's existing HANDLE through a second SafeFileHandle wrapper.
                    outputHandle.SetHandleAsInvalid();
                    retainedArchiveLease.Dispose();
                    await channel.SendAsync(new PreviewError(
                        extract.RequestId,
                        "Archive output HANDLE must be distinct from the parent source."));
                    break;
                }
            }
            else if (string.IsNullOrWhiteSpace(extract.ArchivePath))
            {
                outputHandle.Dispose();
                await channel.SendAsync(new PreviewError(extract.RequestId, "Archive path is unavailable."));
                break;
            }
            closedArchiveEntries.TryRemove(extract.RequestId, out _);
            var extractCts = new CancellationTokenSource();
            var archiveHandoffGate = new SemaphoreSlim(1, 1);
            if (!requests.TryAdd(extract.RequestId, extractCts))
            {
                outputHandle.Dispose();
                retainedArchiveLease?.Dispose();
                extractCts.Dispose();
                archiveHandoffGate.Dispose();
                await channel.SendAsync(new PreviewError(extract.RequestId, "Duplicate request ID."));
                break;
            }
            if (!archiveHandoffGates.TryAdd(extract.RequestId, archiveHandoffGate))
            {
                outputHandle.Dispose();
                retainedArchiveLease?.Dispose();
                requests.TryRemove(extract.RequestId, out _);
                extractCts.Dispose();
                archiveHandoffGate.Dispose();
                break;
            }
            _ = Task.Run(async () =>
            {
                Microsoft.Win32.SafeHandles.SafeFileHandle? compatibilitySource = null;
                try
                {
                    Microsoft.Win32.SafeHandles.SafeFileHandle sourceHandle;
                    long sourceLength;
                    string logicalName;
                    if (retainedArchiveLease is not null)
                    {
                        sourceHandle = retainedArchiveLease.Handle;
                        sourceLength = retainedArchiveLease.Length;
                        logicalName = retainedArchiveLease.LogicalName;
                    }
                    else
                    {
                        var opened = WindowsHandleTransfer.OpenReadOnlyFile(extract.ArchivePath);
                        compatibilitySource = opened.Handle;
                        sourceHandle = opened.Handle;
                        sourceLength = opened.Length;
                        logicalName = Path.GetFileName(extract.ArchivePath);
                    }
                    if (sourceLength < 0 || sourceLength > NativeAbi.MaxArchiveHandleInputBytes)
                        throw new InvalidDataException("Archive source exceeded the HANDLE input limit.");

                    var handleResult = ParserNativePreview.TryExtractArchiveEntryToOutputHandle(
                        sourceHandle,
                        sourceLength,
                        logicalName,
                        extract.EntryPath,
                        outputHandle,
                        extract.OutputCapacity,
                        extractCts.Token);
                    // The App cannot transition its writer into a strict read-only anchor until all
                    // host-side writable duplicates are closed. Close ours before publishing.
                    outputHandle.Dispose();
                    if (handleResult.Status != NativeAbi.StatusOk)
                    {
                        DiagLog.Write(
                            "ParserHost",
                            $"native archive entry output HANDLE extraction failed request={extract.RequestId} status={handleResult.Status}");
                    }
                    extractCts.Token.ThrowIfCancellationRequested();
                    if (handleResult.Status != NativeAbi.StatusOk)
                        await channel.SendAsync(new PreviewError(extract.RequestId, "Archive entry extraction failed."));
                    else
                    {
                        await archiveHandoffGate.WaitAsync();
                        try
                        {
                            if (extractCts.IsCancellationRequested || closedArchiveEntries.ContainsKey(extract.RequestId))
                                return;
                            await channel.SendAsync(new ArchiveEntryExtracted(
                                extract.RequestId,
                                handleResult.Written,
                                extract.EntryPath));
                        }
                        finally
                        {
                            archiveHandoffGate.Release();
                        }
                    }
                }
                catch (OperationCanceledException) { }
                catch (Exception ex)
                {
                    DiagLog.Write("ParserHost", $"archive entry extraction failed request={extract.RequestId}: {ex}");
                    try { await channel.SendAsync(new PreviewError(extract.RequestId, ex.Message)); } catch { }
                }
                finally
                {
                    outputHandle.Dispose();
                    compatibilitySource?.Dispose();
                    retainedArchiveLease?.Dispose();
                    if (requests.TryRemove(extract.RequestId, out var current))
                        current.Dispose();
                    closedArchiveEntries.TryRemove(extract.RequestId, out _);
                    archiveHandoffGates.TryRemove(extract.RequestId, out _);
                }
            });
            break;

        case ArchiveEntryExtractClose close when IsValidRequestId(close.RequestId):
            if (archiveHandoffGates.TryGetValue(close.RequestId, out var archiveCloseGate))
                await archiveCloseGate.WaitAsync();
            try
            {
                if (requests.ContainsKey(close.RequestId))
                    closedArchiveEntries[close.RequestId] = 0;
                Cancel(close.RequestId);
            }
            finally
            {
                archiveCloseGate?.Release();
            }
            break;

        case OfficeImageOpen open:
            if (!IsValidRequestId(open.RequestId)
                || !IsValidRequestId(open.ParentPreviewRequestId)
                || string.Equals(open.RequestId, open.ParentPreviewRequestId, StringComparison.Ordinal)
                || open.TargetWidth is 0 or > NativeAbi.MaxOfficeImageDimension
                || open.TargetHeight is 0 or > NativeAbi.MaxOfficeImageDimension
                || !ParserNativePreview.IsCanonicalOfficeImageRef(open.ImageRef)
                || System.Text.Encoding.UTF8.GetByteCount(open.ImageRef)
                    > NativeAbi.MaxOfficeImageRefUtf8Bytes)
            {
                await channel.SendAsync(new PreviewError(open.RequestId, "Invalid Office image request."));
                break;
            }

            long officeImageByteLength = 0;
            RetainedPreviewSourceLease? retainedOfficeImageLease = null;
            if (!retainedPreviewSources.TryGetValue(
                    open.ParentPreviewRequestId,
                    out RetainedPreviewSource? retainedOfficeSource)
                || !retainedOfficeSource.TryAcquireOfficeLayoutImage(
                    open.ImageRef,
                    out officeImageByteLength,
                    out retainedOfficeImageLease)
                || retainedOfficeImageLease is null
                || officeImageByteLength is <= 0 or > NativeAbi.MaxOfficeImageSourceBytes)
            {
                retainedOfficeImageLease?.Dispose();
                await channel.SendAsync(new PreviewError(
                    open.RequestId,
                    "Parent Office image reference is unavailable."));
                break;
            }

            if (officeImageRasters.ContainsKey(open.RequestId))
            {
                retainedOfficeImageLease.Dispose();
                await channel.SendAsync(new PreviewError(
                    open.RequestId,
                    "Office image handoff has not been released."));
                break;
            }
            if (officeImageParents.Count(pair => pair.Value == open.ParentPreviewRequestId) >= 18)
            {
                retainedOfficeImageLease.Dispose();
                await channel.SendAsync(new PreviewError(
                    open.RequestId,
                    "Too many Office image requests are active for this preview."));
                break;
            }

            var officeImageCts = new CancellationTokenSource();
            var officeImageHandoffGate = new SemaphoreSlim(1, 1);
            if (!requests.TryAdd(open.RequestId, officeImageCts))
            {
                retainedOfficeImageLease.Dispose();
                officeImageCts.Dispose();
                officeImageHandoffGate.Dispose();
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }
            if (!officeImageHandoffGates.TryAdd(open.RequestId, officeImageHandoffGate))
            {
                retainedOfficeImageLease.Dispose();
                requests.TryRemove(open.RequestId, out _);
                officeImageCts.Dispose();
                officeImageHandoffGate.Dispose();
                break;
            }
            if (!officeImageParents.TryAdd(open.RequestId, open.ParentPreviewRequestId))
            {
                retainedOfficeImageLease.Dispose();
                officeImageHandoffGates.TryRemove(open.RequestId, out _);
                requests.TryRemove(open.RequestId, out _);
                officeImageCts.Dispose();
                officeImageHandoffGate.Dispose();
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }

            _ = Task.Run(async () =>
            {
                NativeRasterSection? raster = null;
                bool handoffDelivered = false;
                try
                {
                    var handleResult = ParserNativePreview.TryExtractOfficeLayoutImageHandle(
                        retainedOfficeImageLease.Handle,
                        retainedOfficeImageLease.Length,
                        retainedOfficeImageLease.LogicalName,
                        open.ImageRef,
                        checked((int)open.TargetWidth),
                        checked((int)open.TargetHeight),
                        officeImageCts.Token);
                    if (handleResult.Status != NativeAbi.StatusOk)
                    {
                        DiagLog.Write(
                            "ParserHost",
                            $"native Office image extraction failed request={open.RequestId} status={handleResult.Status}");
                    }
                    raster = handleResult.Raster;
                    officeImageCts.Token.ThrowIfCancellationRequested();
                    if (raster is null)
                    {
                        await channel.SendAsync(new PreviewError(
                            open.RequestId,
                            "Office image extraction failed."));
                        return;
                    }

                    await officeImageHandoffGate.WaitAsync();
                    try
                    {
                        officeImageCts.Token.ThrowIfCancellationRequested();
                        officeImageRasters[open.RequestId] = raster;
                        try
                        {
                            await channel.SendAsync(new OfficeImageReady(
                                open.RequestId,
                                raster.Section.Handle.DangerousGetHandle().ToInt64(),
                                raster.PacketLength,
                                raster.Width,
                                raster.Height));
                            raster = null;
                            handoffDelivered = true;
                        }
                        catch
                        {
                            if (officeImageRasters.TryRemove(
                                open.RequestId,
                                out NativeRasterSection? failed))
                            {
                                failed.Dispose();
                            }
                            throw;
                        }
                    }
                    finally
                    {
                        officeImageHandoffGate.Release();
                    }
                }
                catch (OperationCanceledException) { }
                catch (Exception ex)
                {
                    DiagLog.Write(
                        "ParserHost",
                        $"Office image extraction failed request={open.RequestId}: {ex}");
                    try { await channel.SendAsync(new PreviewError(open.RequestId, ex.Message)); } catch { }
                }
                finally
                {
                    retainedOfficeImageLease.Dispose();
                    raster?.Dispose();
                    if (requests.TryRemove(open.RequestId, out var current))
                        current.Dispose();
                    officeImageHandoffGates.TryRemove(open.RequestId, out _);
                    if (!handoffDelivered)
                        officeImageParents.TryRemove(open.RequestId, out _);
                }
            });
            break;

        case OfficeImageClose close when IsValidRequestId(close.RequestId):
            if (officeImageHandoffGates.TryGetValue(
                close.RequestId,
                out var officeImageCloseGate))
            {
                await officeImageCloseGate.WaitAsync();
            }
            try
            {
                Cancel(close.RequestId);
                if (officeImageRasters.TryRemove(close.RequestId, out var officeImageRaster))
                    officeImageRaster.Dispose();
                officeImageParents.TryRemove(close.RequestId, out _);
            }
            finally
            {
                officeImageCloseGate?.Release();
            }
            break;

        case HeroRasterExtract extract:
            if (!IsValidHeroKind(extract.Kind) || !IsValidRequestId(extract.RequestId))
            {
                await channel.SendAsync(new PreviewError(extract.RequestId, "Invalid hero raster request."));
                break;
            }
            RetainedPreviewSourceLease? retainedHeroLease = null;
            RetainedPreviewFollowUps retainedHeroOperation = extract.Kind.ToLowerInvariant() switch
            {
                "office" => RetainedPreviewFollowUps.OfficeHero,
                "package" => RetainedPreviewFollowUps.PackageHero,
                _ => RetainedPreviewFollowUps.None,
            };
            if (extract.ParentPreviewRequestId is { } heroParentRequestId)
            {
                if (!IsValidRequestId(heroParentRequestId)
                    || string.Equals(heroParentRequestId, extract.RequestId, StringComparison.Ordinal)
                    || retainedHeroOperation == RetainedPreviewFollowUps.None
                    || !retainedPreviewSources.TryGetValue(
                        heroParentRequestId,
                        out RetainedPreviewSource? retainedHeroSource)
                    || !retainedHeroSource.TryAcquire(
                        retainedHeroOperation,
                        out retainedHeroLease))
                {
                    await channel.SendAsync(new PreviewError(
                        extract.RequestId,
                        "Parent preview source is unavailable."));
                    break;
                }
            }
            else if (string.IsNullOrWhiteSpace(extract.Path))
            {
                await channel.SendAsync(new PreviewError(
                    extract.RequestId,
                    "Preview path is unavailable."));
                break;
            }
            if (heroRasters.ContainsKey(extract.RequestId))
            {
                retainedHeroLease?.Dispose();
                await channel.SendAsync(new PreviewError(extract.RequestId, "Hero raster handoff has not been released."));
                break;
            }
            var heroCts = new CancellationTokenSource();
            var heroHandoffGate = new SemaphoreSlim(1, 1);
            if (!requests.TryAdd(extract.RequestId, heroCts))
            {
                retainedHeroLease?.Dispose();
                heroCts.Dispose();
                heroHandoffGate.Dispose();
                await channel.SendAsync(new PreviewError(extract.RequestId, "Duplicate request ID."));
                break;
            }
            if (!heroHandoffGates.TryAdd(extract.RequestId, heroHandoffGate))
            {
                retainedHeroLease?.Dispose();
                requests.TryRemove(extract.RequestId, out _);
                heroCts.Dispose();
                heroHandoffGate.Dispose();
                break;
            }
            _ = Task.Run(async () =>
            {
                NativeRasterSection? raster = null;
                try
                {
                    if (retainedHeroLease is not null)
                    {
                        var handleResult = extract.Kind.Equals("package", StringComparison.OrdinalIgnoreCase)
                            ? ParserNativePreview.TryExtractPackageHeroRasterHandle(
                                retainedHeroLease.Handle,
                                retainedHeroLease.Length,
                                retainedHeroLease.LogicalName,
                                heroCts.Token)
                            : ParserNativePreview.TryExtractOfficeHeroRasterHandle(
                                retainedHeroLease.Handle,
                                retainedHeroLease.Length,
                                retainedHeroLease.LogicalName,
                                heroCts.Token);
                        if (handleResult.Status != NativeAbi.StatusOk)
                        {
                            DiagLog.Write(
                                "ParserHost",
                                $"native hero HANDLE extraction failed request={extract.RequestId} kind={extract.Kind} status={handleResult.Status}");
                        }
                        raster = handleResult.Raster;
                    }
                    else
                    {
                        raster = ParserNativePreview.TryExtractHeroRaster(
                            extract.Kind,
                            extract.Path,
                            heroCts.Token);
                    }
                    heroCts.Token.ThrowIfCancellationRequested();
                    if (raster is null)
                    {
                        await channel.SendAsync(new PreviewError(extract.RequestId, "Hero raster extraction failed."));
                        return;
                    }

                    await heroHandoffGate.WaitAsync();
                    try
                    {
                        heroCts.Token.ThrowIfCancellationRequested();
                        heroRasters[extract.RequestId] = raster;
                        try
                        {
                            await channel.SendAsync(new HeroRasterExtracted(
                                extract.RequestId,
                                raster.Section.Handle.DangerousGetHandle().ToInt64(),
                                raster.PacketLength,
                                raster.Width,
                                raster.Height));
                            raster = null; // The handoff dictionary owns it until close or disconnect.
                        }
                        catch
                        {
                            if (heroRasters.TryRemove(extract.RequestId, out NativeRasterSection? failed))
                                failed.Dispose();
                            throw;
                        }
                    }
                    finally
                    {
                        heroHandoffGate.Release();
                    }
                }
                catch (OperationCanceledException) { }
                catch (Exception ex)
                {
                    DiagLog.Write("ParserHost", $"hero raster extraction failed request={extract.RequestId}: {ex}");
                    try { await channel.SendAsync(new PreviewError(extract.RequestId, ex.Message)); } catch { }
                }
                finally
                {
                    retainedHeroLease?.Dispose();
                    raster?.Dispose();
                    if (requests.TryRemove(extract.RequestId, out var current))
                        current.Dispose();
                    heroHandoffGates.TryRemove(extract.RequestId, out _);
                }
            });
            break;

        case HeroRasterExtractClose close when IsValidRequestId(close.RequestId):
            if (heroHandoffGates.TryGetValue(close.RequestId, out var heroCloseGate))
                await heroCloseGate.WaitAsync();
            try
            {
                Cancel(close.RequestId);
                if (heroRasters.TryRemove(close.RequestId, out var raster))
                    raster.Dispose();
            }
            finally
            {
                heroCloseGate?.Release();
            }
            break;

        default:
            DiagLog.Write("ParserHost", $"rejected invalid control message: {message.GetType().Name}");
            return;
    }
}

foreach (string requestId in requests.Keys)
    Cancel(requestId);
foreach (var raster in heroRasters.Values)
    raster.Dispose();
foreach (var raster in officeImageRasters.Values)
    raster.Dispose();
officeImageParents.Clear();
foreach (string requestId in retainedPreviewSources.Keys)
    DeleteRetainedPreviewSource(requestId);

void Cancel(string requestId)
{
    if (requests.TryGetValue(requestId, out var cts))
    {
        try { cts.Cancel(); } catch (ObjectDisposedException) { }
    }
}

static string? GetArg(string[] values, string key)
{
    for (int i = 0; i < values.Length - 1; i++)
        if (values[i] == key) return values[i + 1];
    return null;
}

static bool IsValidHeroKind(string? kind)
    => string.Equals(kind, "package", StringComparison.OrdinalIgnoreCase)
        || string.Equals(kind, "office", StringComparison.OrdinalIgnoreCase);

static bool IsValidRequestId(string? requestId)
    => requestId is { Length: 32 } && requestId.All(static c => char.IsAsciiHexDigit(c));

static bool IsValidProbe(
    [System.Diagnostics.CodeAnalysis.NotNullWhen(true)] QuickLook.Next.Contracts.FileProbe? probe)
    => probe is not null
       && !string.IsNullOrWhiteSpace(probe.Path)
       && probe.Extension is not null
       && probe.MagicPrefix is not null
       && !string.IsNullOrWhiteSpace(probe.Kind)
       && probe.Size >= 0;

static bool TryCollectOfficeLayoutImages(
    PreviewReady ready,
    out Dictionary<string, long> images)
{
    images = new Dictionary<string, long>(StringComparer.Ordinal);
    if (ready.OfficeLayout is null)
        return true;

    foreach (var item in ready.OfficeLayout.Pages.SelectMany(static page => page.Items))
    {
        if (item.ImageRef is null)
        {
            if (item.ImageByteLength != 0)
            {
                images.Clear();
                return false;
            }
            continue;
        }
        if (!string.Equals(item.Kind, "image", StringComparison.OrdinalIgnoreCase)
            || !ParserNativePreview.IsCanonicalOfficeImageRef(item.ImageRef)
            || System.Text.Encoding.UTF8.GetByteCount(item.ImageRef)
                > NativeAbi.MaxOfficeImageRefUtf8Bytes
            || item.ImageByteLength is <= 0 or > NativeAbi.MaxOfficeImageSourceBytes)
        {
            images.Clear();
            return false;
        }

        if (images.TryGetValue(item.ImageRef, out long existingLength))
        {
            if (existingLength != item.ImageByteLength)
            {
                images.Clear();
                return false;
            }
            continue;
        }

        if (images.Count >= 18)
        {
            images.Clear();
            return false;
        }
        images.Add(item.ImageRef, item.ImageByteLength);
    }
    return true;
}

void DeleteRetainedPreviewSource(string requestId)
{
    if (retainedPreviewSources.TryRemove(requestId, out var source))
        source.Dispose();
}

async Task CloseOfficeImagesForParentAsync(string parentRequestId)
{
    string[] childRequestIds = officeImageParents
        .Where(pair => string.Equals(pair.Value, parentRequestId, StringComparison.Ordinal))
        .Select(static pair => pair.Key)
        .ToArray();
    foreach (string childRequestId in childRequestIds)
    {
        SemaphoreSlim? gate = null;
        if (officeImageHandoffGates.TryGetValue(childRequestId, out gate))
            await gate.WaitAsync();
        try
        {
            Cancel(childRequestId);
            if (officeImageRasters.TryRemove(childRequestId, out var raster))
                raster.Dispose();
            officeImageParents.TryRemove(childRequestId, out _);
        }
        finally
        {
            gate?.Release();
        }
    }
}
