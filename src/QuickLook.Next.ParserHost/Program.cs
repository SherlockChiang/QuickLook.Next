using System.Collections.Concurrent;
using System.IO.Pipes;
using QuickLook.Next.Core;
using QuickLook.Next.ParserHost;

string pipeName = GetArg(args, "--pipe") ?? "quicklook_next_parser";
string? sessionToken = GetArg(args, "--session-token");

DiagLog.Init(Path.Combine(AppContext.BaseDirectory, "parser-host.log"));
DiagLog.Write("ParserHost", $"start pid={Environment.ProcessId} pipe={pipeName}");
ProcessPowerMode.SetCurrentBackgroundEfficiency(enabled: true, "ParserHost");
CleanupStaleHeroRasters();
CleanupStalePreviewInputs();

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
                    if (kind is not ("archive" or "package" or "office" or "text" or "ebook" or "executable" or "torrent" or "certificate"))
                    {
                        await channel.SendAsync(new PreviewError(open.RequestId, "Unsupported ParserHost preview kind."));
                        return;
                    }
                    if (kind == "certificate")
                    {
                        await channel.SendAsync(CertificatePreview.Create(open.RequestId, open.Path, open.Probe.Size));
                        return;
                    }
                    string? json = ParserNativePreview.TryPreview(kind, open.Path, cts.Token);
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

        case PreviewOpenHandle open when IsValidRequestId(open.RequestId)
                                           && open.SourceLength is >= 0 and <= 256L * 1024 * 1024
                                           && !string.IsNullOrWhiteSpace(open.LogicalPath)
                                           && IsValidProbe(open.Probe):
            if (activePreviewRequestId is not null)
            {
                Cancel(activePreviewRequestId);
                DeletePreviewInput(activePreviewRequestId);
            }
            var handleCts = new CancellationTokenSource();
            if (!requests.TryAdd(open.RequestId, handleCts))
            {
                handleCts.Dispose();
                await channel.SendAsync(new PreviewError(open.RequestId, "Duplicate request ID."));
                break;
            }
            activePreviewRequestId = open.RequestId;
            _ = Task.Run(async () =>
            {
                bool published = false;
                try
                {
                    using var sourceHandle = WindowsHandleTransfer.TakeLocalFileHandle(open.SourceHandle, open.SourceLength);
                    var input = CreatePreviewInput(open.RequestId, open.LogicalPath, sourceHandle, open.SourceLength);
                    if (input is null || !previewInputs.TryAdd(open.RequestId, input.Value))
                    {
                        input?.Anchor.Dispose();
                        if (input is not null) DeletePreviewInputPath(input.Value.Path);
                        await channel.SendAsync(new PreviewError(open.RequestId, "Could not anchor preview input."));
                        return;
                    }
                    handleCts.Token.ThrowIfCancellationRequested();
                    string kind = open.Probe.Kind.ToLowerInvariant();
                    if (kind is not ("archive" or "package" or "office" or "text" or "ebook" or "executable" or "torrent" or "certificate"))
                    {
                        await channel.SendAsync(new PreviewError(open.RequestId, "Unsupported ParserHost preview kind."));
                        return;
                    }
                    if (kind == "certificate")
                    {
                        await channel.SendAsync(CertificatePreview.Create(open.RequestId, input.Value.Path, open.SourceLength));
                        published = true;
                        return;
                    }
                    string? json = ParserNativePreview.TryPreview(kind, input.Value.Path, handleCts.Token);
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
                    if (!published) DeletePreviewInput(open.RequestId);
                }
            });
            break;

        case PreviewClose close when IsValidRequestId(close.RequestId):
            Cancel(close.RequestId);
            DeletePreviewInput(close.RequestId);
            break;

        case ArchiveEntryExtract extract when IsValidRequestId(extract.RequestId)
                                              && !string.IsNullOrWhiteSpace(extract.ArchivePath)
                                              && !string.IsNullOrWhiteSpace(extract.EntryPath):
            if (archiveEntries.ContainsKey(extract.RequestId))
            {
                await channel.SendAsync(new PreviewError(extract.RequestId, "Archive handoff has not been released."));
                break;
            }
            closedArchiveEntries.TryRemove(extract.RequestId, out _);
            var extractCts = new CancellationTokenSource();
            var archiveHandoffGate = new SemaphoreSlim(1, 1);
            if (!requests.TryAdd(extract.RequestId, extractCts))
            {
                extractCts.Dispose();
                archiveHandoffGate.Dispose();
                await channel.SendAsync(new PreviewError(extract.RequestId, "Duplicate request ID."));
                break;
            }
            if (!archiveHandoffGates.TryAdd(extract.RequestId, archiveHandoffGate))
            {
                requests.TryRemove(extract.RequestId, out _);
                extractCts.Dispose();
                archiveHandoffGate.Dispose();
                break;
            }
            _ = Task.Run(async () =>
            {
                try
                {
                    string? path = ParserNativePreview.TryExtractArchiveEntry(extract.ArchivePath, extract.EntryPath, extractCts.Token);
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
                        await archiveHandoffGate.WaitAsync();
                        try
                        {
                            var transferred = WindowsHandleTransfer.OpenReadOnlyFile(path);
                            archiveEntries[extract.RequestId] = (path, transferred.Handle);
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
            if (heroRasters.ContainsKey(extract.RequestId))
            {
                await channel.SendAsync(new PreviewError(extract.RequestId, "Hero raster handoff has not been released."));
                break;
            }
            var heroCts = new CancellationTokenSource();
            var heroHandoffGate = new SemaphoreSlim(1, 1);
            if (!requests.TryAdd(extract.RequestId, heroCts))
            {
                heroCts.Dispose();
                heroHandoffGate.Dispose();
                await channel.SendAsync(new PreviewError(extract.RequestId, "Duplicate request ID."));
                break;
            }
            if (!heroHandoffGates.TryAdd(extract.RequestId, heroHandoffGate))
            {
                requests.TryRemove(extract.RequestId, out _);
                heroCts.Dispose();
                heroHandoffGate.Dispose();
                break;
            }
            _ = Task.Run(async () =>
            {
                string? tempPath = null;
                try
                {
                    string heroPath;
                    if (extract.ParentPreviewRequestId is { } parentRequestId)
                    {
                        if (!IsValidRequestId(parentRequestId)
                            || !previewInputs.TryGetValue(parentRequestId, out var parentInput))
                        {
                            await channel.SendAsync(new PreviewError(extract.RequestId, "Parent preview input is unavailable."));
                            return;
                        }
                        heroPath = parentInput.Path;
                    }
                    else
                    {
                        heroPath = extract.Path;
                    }
                    byte[]? raster = ParserNativePreview.TryExtractHeroRaster(extract.Kind, heroPath, heroCts.Token);
                    heroCts.Token.ThrowIfCancellationRequested();
                    if (raster is null || !ParserNativePreview.IsValidRaster(raster, raster.Length))
                    {
                        await channel.SendAsync(new PreviewError(extract.RequestId, "Hero raster extraction failed."));
                        return;
                    }

                    tempPath = WriteHeroRaster(extract.RequestId, raster);
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

static string? WriteHeroRaster(string requestId, byte[] raster)
{
    try
    {
        string root = Path.Combine(Path.GetTempPath(), "QuickLookNext", "parser-raster");
        Directory.CreateDirectory(root);
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

static void CleanupStaleHeroRasters()
{
    try
    {
        string root = Path.Combine(Path.GetTempPath(), "QuickLookNext", "parser-raster");
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

static void CleanupStalePreviewInputs()
{
    try
    {
        string root = Path.Combine(Path.GetTempPath(), "QuickLookNext", "parser-input");
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
    long sourceLength)
{
    string extension = Path.GetExtension(logicalPath);
    if (extension.Length > 32 || extension.Any(static c => !char.IsAsciiLetterOrDigit(c) && c != '.'))
        extension = "";
    string root = Path.Combine(Path.GetTempPath(), "QuickLookNext", "parser-input");
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
