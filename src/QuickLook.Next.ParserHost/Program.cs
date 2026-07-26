using System.Collections.Concurrent;
using System.IO.Pipes;
using QuickLook.Next.Core;
using QuickLook.Next.ParserHost;

string pipeName = GetArg(args, "--pipe") ?? "quicklook_next_parser";
string? sessionToken = GetArg(args, "--session-token");
string writableRoot = GetArg(args, "--writable-root") ?? "";
if (!Path.IsPathFullyQualified(writableRoot) || !Directory.Exists(writableRoot)
    || (File.GetAttributes(writableRoot) & FileAttributes.ReparsePoint) != 0) return;
string logRoot = Path.Combine(writableRoot, "logs");
string inputRoot = Path.Combine(writableRoot, "parser-input");
string archiveRoot = Path.Combine(writableRoot, "archive-preview");
string rasterRoot = Path.Combine(writableRoot, "parser-raster");
foreach (string child in new[] { logRoot, inputRoot, archiveRoot, rasterRoot })
    if (!Directory.Exists(child) || (File.GetAttributes(child) & FileAttributes.ReparsePoint) != 0) return;
Environment.SetEnvironmentVariable("QUICKLOOK_NEXT_ARCHIVE_ROOT", archiveRoot);

DiagLog.InitInDirectory(logRoot, "parser-host.log");
DiagLog.Write("ParserHost", $"start pid={Environment.ProcessId} pipe={pipeName}");
try { ParserNativePreview.EnsureCompatible(); }
catch (Exception ex) { DiagLog.Write("ParserHost", "native ABI check failed: " + ex.Message); return; }
ProcessPowerMode.SetCurrentBackgroundEfficiency(enabled: true, "ParserHost");
CleanupStaleHeroRasters(rasterRoot);
CleanupStalePreviewInputs(inputRoot);

using var pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
PipeChannel channel;
try
{
    await pipe.ConnectAsync(5000);
    channel = new PipeChannel(pipe);
}
catch (Exception ex)
{
    DiagLog.Write("ParserHost", "pipe connect FAILED: " + ex);
    return;
}

using var channelLifetime = channel;
var requests = new ConcurrentDictionary<string, CancellationTokenSource>();
var archiveEntries = new ConcurrentDictionary<string, (string Path, Microsoft.Win32.SafeHandles.SafeFileHandle Handle)>();
var closedArchiveEntries = new ConcurrentDictionary<string, byte>();
var archiveHandoffGates = new ConcurrentDictionary<string, SemaphoreSlim>();
var heroRasters = new ConcurrentDictionary<string, (string Path, Microsoft.Win32.SafeHandles.SafeFileHandle Handle)>();
var heroHandoffGates = new ConcurrentDictionary<string, SemaphoreSlim>();
var previewInputs = new ConcurrentDictionary<string, (string Path, FileStream Anchor)>();
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
            if (activePreviewRequestId is not null)
            {
                Cancel(activePreviewRequestId);
                DeletePreviewInput(activePreviewRequestId);
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
            if (!IsValidRequestId(open.RequestId)
                || open.SourceLength is not (>= 0 and <= NativeAbi.MaxParserHandleInputBytes)
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
            if (activePreviewRequestId is not null)
            {
                Cancel(activePreviewRequestId);
                DeletePreviewInput(activePreviewRequestId);
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
                                RetainedPreviewFollowUps followUps =
                                    kind == "office"
                                        ? RetainedPreviewFollowUps.OfficeHero
                                        : kind == "package"
                                        ? RetainedPreviewFollowUps.PackageHero
                                        : string.Equals(
                                            handleReady?.Listing?.ListingKind,
                                            "archive",
                                            StringComparison.OrdinalIgnoreCase)
                                        && handleReady?.Listing?.CanPreviewEntries == true
                                        ? RetainedPreviewFollowUps.ArchiveEntry
                                        : RetainedPreviewFollowUps.None;
                                if (followUps != RetainedPreviewFollowUps.None)
                                {
                                    var retainedSource = new RetainedPreviewSource(
                                        ownedSourceHandle,
                                        open.SourceLength,
                                        logicalName,
                                        kind,
                                        followUps);
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
                    var input = CreatePreviewInput(open.RequestId, open.LogicalPath, ownedSourceHandle, open.SourceLength, inputRoot);
                    if (input is null || !previewInputs.TryAdd(open.RequestId, input.Value))
                    {
                        input?.Anchor.Dispose();
                        if (input is not null) DeletePreviewInputPath(input.Value.Path);
                        await channel.SendAsync(new PreviewError(open.RequestId, "Could not anchor preview input."));
                        return;
                    }
                    handleCts.Token.ThrowIfCancellationRequested();
                    if (kind == "certificate")
                    {
                        await channel.SendAsync(CertificatePreview.Create(open.RequestId, input.Value.Path, open.SourceLength));
                        published = true;
                        return;
                    }
                    string? json = ParserNativePreview.TryPreview(kind, input.Value.Path, open.Probe, handleCts.Token);
                    handleCts.Token.ThrowIfCancellationRequested();
                    if (!PreviewReadyJson.TryParse(open.RequestId, json ?? "", out PreviewReady? ready, out string? error))
                        await channel.SendAsync(new PreviewError(open.RequestId, error ?? "Native parser returned no preview."));
                    else
                    {
                        await channel.SendAsync(ready!);
                        published = true;
                    }
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
                        DeletePreviewInput(open.RequestId);
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
            if (activePreviewRequestId is not null)
            {
                Cancel(activePreviewRequestId);
                DeletePreviewInput(activePreviewRequestId);
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
            DeletePreviewInput(close.RequestId);
            DeleteRetainedPreviewSource(close.RequestId);
            break;

        case ArchiveEntryExtract extract when IsValidRequestId(extract.RequestId)
                                              && !string.IsNullOrWhiteSpace(extract.EntryPath):
            RetainedPreviewSourceLease? retainedArchiveLease = null;
            if (extract.ParentPreviewRequestId is { } parentRequestId)
            {
                if (!IsValidRequestId(parentRequestId)
                    || string.Equals(parentRequestId, extract.RequestId, StringComparison.Ordinal)
                    || !retainedPreviewSources.TryGetValue(parentRequestId, out RetainedPreviewSource? retainedArchiveSource)
                    || !retainedArchiveSource.TryAcquire(
                        RetainedPreviewFollowUps.ArchiveEntry,
                        out retainedArchiveLease))
                {
                    await channel.SendAsync(new PreviewError(
                        extract.RequestId,
                        "Parent archive preview source is unavailable."));
                    break;
                }
            }
            else if (string.IsNullOrWhiteSpace(extract.ArchivePath))
            {
                await channel.SendAsync(new PreviewError(extract.RequestId, "Archive path is unavailable."));
                break;
            }
            if (archiveEntries.ContainsKey(extract.RequestId))
            {
                retainedArchiveLease?.Dispose();
                await channel.SendAsync(new PreviewError(extract.RequestId, "Archive handoff has not been released."));
                break;
            }
            closedArchiveEntries.TryRemove(extract.RequestId, out _);
            var extractCts = new CancellationTokenSource();
            var archiveHandoffGate = new SemaphoreSlim(1, 1);
            if (!requests.TryAdd(extract.RequestId, extractCts))
            {
                retainedArchiveLease?.Dispose();
                extractCts.Dispose();
                archiveHandoffGate.Dispose();
                await channel.SendAsync(new PreviewError(extract.RequestId, "Duplicate request ID."));
                break;
            }
            if (!archiveHandoffGates.TryAdd(extract.RequestId, archiveHandoffGate))
            {
                retainedArchiveLease?.Dispose();
                requests.TryRemove(extract.RequestId, out _);
                extractCts.Dispose();
                archiveHandoffGate.Dispose();
                break;
            }
            _ = Task.Run(async () =>
            {
                string? pendingArchiveEntryPath = null;
                bool handoffDelivered = false;
                try
                {
                    string? path;
                    if (retainedArchiveLease is not null)
                    {
                        var handleResult = ParserNativePreview.TryExtractArchiveEntryHandle(
                            retainedArchiveLease.Handle,
                            retainedArchiveLease.Length,
                            retainedArchiveLease.LogicalName,
                            extract.EntryPath,
                            extractCts.Token);
                        if (handleResult.Status != NativeAbi.StatusOk)
                        {
                            DiagLog.Write(
                                "ParserHost",
                                $"native archive entry HANDLE extraction failed request={extract.RequestId} status={handleResult.Status}");
                        }
                        path = handleResult.Path;
                    }
                    else
                    {
                        path = ParserNativePreview.TryExtractArchiveEntry(
                            extract.ArchivePath,
                            extract.EntryPath,
                            extractCts.Token);
                    }
                    if (!string.IsNullOrWhiteSpace(path)
                        && (extractCts.IsCancellationRequested || closedArchiveEntries.ContainsKey(extract.RequestId)))
                    {
                        DeleteArchiveEntry(path);
                        return;
                    }
                    extractCts.Token.ThrowIfCancellationRequested();
                    if (string.IsNullOrWhiteSpace(path))
                        await channel.SendAsync(new PreviewError(extract.RequestId, "Archive entry extraction failed."));
                    else
                    {
                        pendingArchiveEntryPath = path;
                        await archiveHandoffGate.WaitAsync();
                        try
                        {
                            var transferred = WindowsHandleTransfer.OpenReadOnlyFile(path);
                            archiveEntries[extract.RequestId] = (path, transferred.Handle);
                            pendingArchiveEntryPath = null;
                            if (extractCts.IsCancellationRequested || closedArchiveEntries.ContainsKey(extract.RequestId))
                            {
                                if (archiveEntries.TryRemove(extract.RequestId, out var closedEntry))
                                {
                                    closedEntry.Handle.Dispose();
                                    DeleteArchiveEntry(closedEntry.Path);
                                }
                                return;
                            }
                            await channel.SendAsync(new ArchiveEntryExtracted(
                                extract.RequestId,
                                transferred.Handle.DangerousGetHandle().ToInt64(),
                                transferred.Length,
                                extract.EntryPath));
                            handoffDelivered = true;
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
                    retainedArchiveLease?.Dispose();
                    if (!handoffDelivered
                        && archiveEntries.TryRemove(extract.RequestId, out var failedEntry))
                    {
                        failedEntry.Handle.Dispose();
                        DeleteArchiveEntry(failedEntry.Path);
                    }
                    if (pendingArchiveEntryPath is not null)
                        DeleteArchiveEntry(pendingArchiveEntryPath);
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
                if (archiveEntries.TryRemove(close.RequestId, out var archiveEntry))
                {
                    archiveEntry.Handle.Dispose();
                    DeleteArchiveEntry(archiveEntry.Path);
                }
            }
            finally
            {
                archiveCloseGate?.Release();
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
                string? tempPath = null;
                bool handoffDelivered = false;
                try
                {
                    byte[]? raster;
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
                    if (raster is null || !ParserNativePreview.IsValidRaster(raster, raster.Length))
                    {
                        await channel.SendAsync(new PreviewError(extract.RequestId, "Hero raster extraction failed."));
                        return;
                    }

                    tempPath = WriteHeroRaster(extract.RequestId, raster, rasterRoot);
                    heroCts.Token.ThrowIfCancellationRequested();
                    if (tempPath is null)
                    {
                        await channel.SendAsync(new PreviewError(extract.RequestId, "Hero raster handoff failed."));
                        return;
                    }

                    int width = BitConverter.ToInt32(raster, 0);
                    int height = BitConverter.ToInt32(raster, 4);
                    await heroHandoffGate.WaitAsync();
                    try
                    {
                        heroCts.Token.ThrowIfCancellationRequested();
                        var transferred = WindowsHandleTransfer.OpenReadOnlyFile(tempPath);
                        if (transferred.Length != raster.LongLength)
                        {
                            transferred.Handle.Dispose();
                            throw new InvalidDataException("Hero raster changed before handle transfer.");
                        }
                        heroRasters[extract.RequestId] = (tempPath, transferred.Handle);
                        await channel.SendAsync(new HeroRasterExtracted(
                            extract.RequestId, transferred.Handle.DangerousGetHandle().ToInt64(), transferred.Length, width, height));
                        handoffDelivered = true;
                        tempPath = null; // retained until the App acknowledges consumption.
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
                    if (!handoffDelivered
                        && heroRasters.TryRemove(extract.RequestId, out var failedRaster))
                    {
                        failedRaster.Handle.Dispose();
                        DeleteHeroRaster(failedRaster.Path);
                    }
                    if (tempPath is not null) DeleteHeroRaster(tempPath);
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
                {
                    raster.Handle.Dispose();
                    DeleteHeroRaster(raster.Path);
                }
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
foreach (var entry in archiveEntries.Values)
{
    entry.Handle.Dispose();
    DeleteArchiveEntry(entry.Path);
}
foreach (var raster in heroRasters.Values)
{
    raster.Handle.Dispose();
    DeleteHeroRaster(raster.Path);
}
foreach (string requestId in previewInputs.Keys)
    DeletePreviewInput(requestId);
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

static bool IsValidProbe(QuickLook.Next.Contracts.FileProbe? probe)
    => probe is not null
       && !string.IsNullOrWhiteSpace(probe.Path)
       && probe.Extension is not null
       && probe.MagicPrefix is not null
       && !string.IsNullOrWhiteSpace(probe.Kind)
       && probe.Size >= 0;

static string? WriteHeroRaster(string requestId, byte[] raster, string root)
{
    try
    {
        if ((File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0)
            return null;

        string directory = Path.Combine(root, "raster-" + requestId);
        Directory.CreateDirectory(directory);
        if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0)
            return null;

        string path = Path.Combine(directory, "hero.bgra");
        using var stream = new FileStream(path, new FileStreamOptions
        {
            Mode = FileMode.CreateNew,
            Access = FileAccess.Write,
            Share = FileShare.None,
            Options = FileOptions.WriteThrough,
        });
        stream.Write(raster);
        return path;
    }
    catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException)
    {
        return null;
    }
}

static void DeleteHeroRaster(string path)
{
    try
    {
        File.Delete(path);
        string? directory = Path.GetDirectoryName(path);
        if (directory is not null) Directory.Delete(directory, recursive: false);
    }
    catch { }
}

static void DeleteArchiveEntry(string path)
{
    try
    {
        File.Delete(path);
        string? directory = Path.GetDirectoryName(path);
        if (directory is not null) Directory.Delete(directory, recursive: false);
    }
    catch { }
}

static void CleanupStaleHeroRasters(string root)
{
    try
    {
        if (!Directory.Exists(root) || (File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0)
            return;

        foreach (string directory in Directory.EnumerateDirectories(root, "raster-*"))
        {
            if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) == 0)
                Directory.Delete(directory, recursive: true);
        }
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

(string Path, FileStream Anchor)? CreatePreviewInput(
    string requestId,
    string logicalPath,
    Microsoft.Win32.SafeHandles.SafeFileHandle sourceHandle,
    long sourceLength,
    string root)
{
    string fileName = Path.GetFileName(logicalPath);
    string extension = fileName.EndsWith("-wal", StringComparison.OrdinalIgnoreCase)
        ? "-wal"
        : fileName.EndsWith("-shm", StringComparison.OrdinalIgnoreCase)
            ? "-shm"
            : Path.GetExtension(logicalPath);
    if (extension.Length > 32 || extension.Any(static c => !char.IsAsciiLetterOrDigit(c) && c is not '.' and not '-'))
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
        var anchor = new FileStream(path, FileMode.CreateNew, FileAccess.ReadWrite, FileShare.Read);
        try
        {
            source.CopyTo(anchor);
            anchor.Flush(flushToDisk: true);
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

void DeleteRetainedPreviewSource(string requestId)
{
    if (retainedPreviewSources.TryRemove(requestId, out var source))
        source.Dispose();
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
