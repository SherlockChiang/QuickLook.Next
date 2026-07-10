using System.Collections.Concurrent;
using System.IO.Pipes;
using QuickLook.Next.Core;
using QuickLook.Next.ParserHost;

string pipeName = GetArg(args, "--pipe") ?? "quicklook_next_parser";
string? sessionToken = GetArg(args, "--session-token");

DiagLog.Init(Path.Combine(AppContext.BaseDirectory, "parser-host.log"));
DiagLog.Write("ParserHost", $"start pid={Environment.ProcessId} pipe={pipeName}");
ProcessPowerMode.SetCurrentBackgroundEfficiency(enabled: true, "ParserHost");

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
                    extractCts.Token.ThrowIfCancellationRequested();
                    if (string.IsNullOrWhiteSpace(path))
                        await channel.SendAsync(new PreviewError(extract.RequestId, "Archive entry extraction failed."));
                    else
                        await channel.SendAsync(new ArchiveEntryExtracted(extract.RequestId, path));
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
                }
            });
            break;

        case ArchiveEntryExtractClose close:
            Cancel(close.RequestId);
            break;
    }
}

foreach (string requestId in requests.Keys)
    Cancel(requestId);

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
