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
var archiveEntries = new ConcurrentDictionary<string, string>();
var closedArchiveEntries = new ConcurrentDictionary<string, byte>();
var heroRasters = new ConcurrentDictionary<string, string>();
bool authenticated = false;

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
            authenticated = true;
            await channel.SendAsync(new ParserReady());
            break;

        case var _ when !authenticated:
            DiagLog.Write("ParserHost", "rejected control message before authentication");
            return;

        case PreviewOpen open when authenticated:
            foreach (string requestId in requests.Keys)
                Cancel(requestId);
            var cts = new CancellationTokenSource();
            if (!requests.TryAdd(open.RequestId, cts))
            {
                cts.Dispose();
                break;
            }
            _ = Task.Run(async () =>
            {
                try
                {
                    string kind = open.Probe.Kind.ToLowerInvariant();
                    if (kind is not ("archive" or "package" or "office"))
                    {
                        await channel.SendAsync(new PreviewError(open.RequestId, "ParserHost only handles archive, package, and office previews."));
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

        case PreviewClose close:
            Cancel(close.RequestId);
            break;

        case ArchiveEntryExtract extract:
            closedArchiveEntries.TryRemove(extract.RequestId, out _);
            var extractCts = new CancellationTokenSource();
            if (!requests.TryAdd(extract.RequestId, extractCts))
            {
                extractCts.Dispose();
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
                        archiveEntries[extract.RequestId] = path;
                        if (extractCts.IsCancellationRequested || closedArchiveEntries.ContainsKey(extract.RequestId))
                        {
                            if (archiveEntries.TryRemove(extract.RequestId, out string? closedPath))
                                DeleteArchiveEntry(closedPath);
                            return;
                        }
                        await channel.SendAsync(new ArchiveEntryExtracted(extract.RequestId, path));
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
                }
            });
            break;

        case ArchiveEntryExtractClose close:
            if (requests.ContainsKey(close.RequestId))
                closedArchiveEntries[close.RequestId] = 0;
            Cancel(close.RequestId);
            if (archiveEntries.TryRemove(close.RequestId, out string? archiveEntryPath) && archiveEntryPath is not null)
                DeleteArchiveEntry(archiveEntryPath);
            break;

        case HeroRasterExtract extract:
            if (!IsValidHeroKind(extract.Kind) || !IsValidRequestId(extract.RequestId))
            {
                await channel.SendAsync(new PreviewError(extract.RequestId, "Invalid hero raster request."));
                break;
            }
            var heroCts = new CancellationTokenSource();
            if (!requests.TryAdd(extract.RequestId, heroCts))
            {
                heroCts.Dispose();
                break;
            }
            _ = Task.Run(async () =>
            {
                string? tempPath = null;
                try
                {
                    byte[]? raster = ParserNativePreview.TryExtractHeroRaster(extract.Kind, extract.Path, heroCts.Token);
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
                    heroRasters[extract.RequestId] = tempPath;
                    await channel.SendAsync(new HeroRasterExtracted(extract.RequestId, tempPath, width, height));
                    tempPath = null; // retained until the App acknowledges consumption.
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
                }
            });
            break;

        case HeroRasterExtractClose close:
            Cancel(close.RequestId);
            if (heroRasters.TryRemove(close.RequestId, out string? tempPath))
                DeleteHeroRaster(tempPath);
            break;
    }
}

foreach (string requestId in requests.Keys)
    Cancel(requestId);
foreach (string tempPath in archiveEntries.Values)
    DeleteArchiveEntry(tempPath);
foreach (string tempPath in heroRasters.Values)
    DeleteHeroRaster(tempPath);

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

static bool IsValidHeroKind(string kind)
    => kind.Equals("package", StringComparison.OrdinalIgnoreCase)
        || kind.Equals("office", StringComparison.OrdinalIgnoreCase);

static bool IsValidRequestId(string requestId)
    => requestId.Length == 32 && requestId.All(static c => char.IsAsciiHexDigit(c));

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
