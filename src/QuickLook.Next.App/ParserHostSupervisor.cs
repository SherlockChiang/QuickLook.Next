using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Microsoft.UI.Dispatching;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

/// <summary>Supervises the JSON-only native parser process. It intentionally has no surface support.</summary>
internal sealed class ParserHostSupervisor
{
    private static readonly TimeSpan PreviewTimeout = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan HostConnectTimeout = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan ResourceTelemetryInterval = TimeSpan.FromMinutes(1);
    private readonly string _hostExePath;
    private readonly PendingRequests _pending = new();
    private readonly ConcurrentDictionary<string, byte> _recycleOnCancel = new();
    private readonly SemaphoreSlim _startLock = new(1, 1);
    private NamedPipeServerStream? _server;
    private PipeChannel? _channel;
    private Process? _host;
    private HostProcessJob? _job;
    private string? _sessionToken;
    private int _generation;
    private bool _stopping;
    private bool _backgroundEfficiencyEnabled = true;
    private TaskCompletionSource _ready = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private readonly CancellationTokenSource _telemetryCts = new();
    private readonly Task _telemetryTask;
    private int _timeoutCount;

    public ParserHostSupervisor(string hostExePath)
    {
        _hostExePath = hostExePath;
        _telemetryTask = RunResourceTelemetryAsync(_telemetryCts.Token);
    }

    public async Task EnsureStartedAsync(CancellationToken cancellationToken = default)
    {
        if (IsConnected) return;
        await _startLock.WaitAsync(cancellationToken);
        try { if (!IsConnected) await StartAsync(cancellationToken); }
        finally { _startLock.Release(); }
    }

    private async Task StartAsync(CancellationToken cancellationToken)
    {
        _stopping = false;
        int generation = ++_generation;
        DiagLog.Write("App", $"ParserHost starting gen={generation}; restart={generation > 1}");
        _ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        _channel?.Dispose();
        _server?.Dispose();
        TryKillHost();
        try { _host?.Dispose(); } catch { }
        _host = null;
        string pipeName = $"quicklook_next_parser_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        _sessionToken = RandomNumberGenerator.GetHexString(32);
        _server = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        var job = new HostProcessJob((nint)(512L * 1024 * 1024));
        try
        {
            _host = HostProcessLauncher.StartRestricted(
                _hostExePath, ["--pipe", pipeName, "--session-token", _sessionToken], job);
            _job = job;
        }
        catch
        {
            try { if (_host is { HasExited: false }) _host.Kill(entireProcessTree: true); } catch { }
            try { _host?.Dispose(); } catch { }
            _host = null;
            job.Dispose();
            throw;
        }
        ProcessPowerMode.SetProcessBackgroundEfficiency(_host, _backgroundEfficiencyEnabled, "App");
        LogHostResources("started", generation, _host);
        _host.EnableRaisingEvents = true;
        _host.Exited += (_, _) => OnHostExited(generation);
        try
        {
            using var connectCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            connectCts.CancelAfter(HostConnectTimeout);
            await _server.WaitForConnectionAsync(connectCts.Token);
            if (!GetNamedPipeClientProcessId(_server.SafePipeHandle.DangerousGetHandle(), out uint clientPid) || clientPid != _host.Id)
                throw new InvalidOperationException("ParserHost pipe client did not match the launched process");
        }
        catch { TryKillHost(); throw; }

        _channel = new PipeChannel(_server);
        await _channel.SendAsync(new Hello(Environment.ProcessId, _sessionToken));
        _ = ReadLoopAsync(_channel, generation);
        using var readyCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        readyCts.CancelAfter(HostConnectTimeout);
        await _ready.Task.WaitAsync(readyCts.Token);
    }

    private bool IsConnected
    {
        get { try { return _channel is not null && _host is { HasExited: false }; } catch { return false; } }
    }

    public (string RequestId, Task<ControlMessage> Completion) BeginOpen(
        string path,
        FileProbe probe,
        TimeSpan? timeout = null,
        bool recycleHostOnCancel = false)
    {
        if (_channel is null) throw new InvalidOperationException("ParserHost not connected");
        var (requestId, completion) = _pending.Begin(timeout ?? PreviewTimeout);
        if (recycleHostOnCancel)
            _recycleOnCancel[requestId] = 0;
        _ = StopOnTimeoutAsync(completion, requestId);
        _ = SendOpenAsync(requestId, path, probe);
        return (requestId, completion);
    }

    public (string RequestId, Task<ControlMessage> Completion) BeginOpenHandle(
        string logicalPath,
        FileProbe probe,
        Microsoft.Win32.SafeHandles.SafeFileHandle sourceHandle,
        long sourceLength,
        TimeSpan? timeout = null)
    {
        if (_channel is null || _host is null) throw new InvalidOperationException("ParserHost not connected");
        var (requestId, completion) = _pending.Begin(timeout ?? PreviewTimeout);
        _ = StopOnTimeoutAsync(completion, requestId);
        long hostHandle;
        try
        {
            hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(sourceHandle, _host.SafeHandle);
        }
        catch
        {
            _pending.Cancel(requestId);
            throw;
        }
        _ = SendOpenHandleAsync(requestId, hostHandle, sourceLength, logicalPath, probe);
        return (requestId, completion);
    }

    private async Task SendOpenHandleAsync(
        string requestId, long sourceHandle, long sourceLength, string logicalPath, FileProbe probe)
    {
        try
        {
            await (_channel?.SendAsync(new PreviewOpenHandle(
                requestId, sourceHandle, sourceLength, logicalPath, probe))
                ?? Task.FromException(new InvalidOperationException("ParserHost not connected")));
        }
        catch (Exception ex)
        {
            _pending.TryComplete(requestId, new PreviewError(requestId, ex.Message));
            RecycleHost("handle preview request could not be delivered");
        }
    }

    private async Task SendOpenAsync(string requestId, string path, FileProbe probe)
    {
        try { await (_channel?.SendAsync(new PreviewOpen(requestId, path, probe)) ?? Task.FromException(new InvalidOperationException("ParserHost not connected"))); }
        catch (Exception ex)
        {
            _recycleOnCancel.TryRemove(requestId, out _);
            _pending.TryComplete(requestId, new PreviewError(requestId, ex.Message));
        }
    }

    public Task CloseAsync(string requestId)
    {
        bool wasPending = _pending.Cancel(requestId);
        bool recycleHost = wasPending && _recycleOnCancel.TryRemove(requestId, out _);
        return CloseCoreAsync(requestId, recycleHost);
    }

    private async Task CloseCoreAsync(string requestId, bool recycleHost)
    {
        try
        {
            if (_channel is not null)
                await _channel.SendAsync(new PreviewClose(requestId));
        }
        finally
        {
            if (recycleHost)
            {
                DiagLog.Write("App", $"recycling ParserHost after cloud preview cancellation: request={requestId}");
                RecycleHost("cloud preview canceled while opening");
            }
        }
    }

    public async Task<ArchiveEntryHandoff?> ExtractArchiveEntryAsync(string archivePath, string entryPath, CancellationToken cancellationToken)
    {
        if (_channel is null) throw new InvalidOperationException("ParserHost not connected");
        var (requestId, completion) = _pending.Begin(PreviewTimeout);
        _ = StopOnTimeoutAsync(completion, requestId);
        ArchiveEntryHandoff? handoff = null;
        try
        {
            await _channel.SendAsync(new ArchiveEntryExtract(requestId, archivePath, entryPath), cancellationToken);
            ControlMessage response = await completion.WaitAsync(cancellationToken);
            if (response is ArchiveEntryExtracted extracted)
                handoff = CreateArchiveEntryHandoff(extracted);
            return handoff;
        }
        finally
        {
            _pending.Cancel(requestId);
            if (handoff is null)
            {
                try { await (_channel?.SendAsync(new ArchiveEntryExtractClose(requestId)) ?? Task.CompletedTask); }
                catch (Exception ex) when (ex is IOException or ObjectDisposedException or InvalidOperationException) { }
            }
        }
    }

    public async Task ReleaseArchiveEntryAsync(ArchiveEntryHandoff handoff)
    {
        try { await (_channel?.SendAsync(new ArchiveEntryExtractClose(handoff.RequestId)) ?? Task.CompletedTask); }
        catch (Exception ex) when (ex is IOException or ObjectDisposedException or InvalidOperationException) { }
        finally { handoff.Dispose(); }
    }

    public async Task<NativeRasterImage?> ExtractHeroRasterAsync(
        string path, string kind, string? parentPreviewRequestId, CancellationToken cancellationToken)
    {
        if (_channel is null) throw new InvalidOperationException("ParserHost not connected");
        var (requestId, completion) = _pending.Begin(PreviewTimeout);
        _ = StopOnTimeoutAsync(completion, requestId);
        try
        {
            await _channel.SendAsync(new HeroRasterExtract(requestId, path, kind)
            {
                ParentPreviewRequestId = parentPreviewRequestId,
            }, cancellationToken);
            ControlMessage response = await completion.WaitAsync(cancellationToken);
            return response is HeroRasterExtracted extracted
                ? ReadHeroRaster(extracted)
                : null;
        }
        finally
        {
            _pending.Cancel(requestId);
            try { await (_channel?.SendAsync(new HeroRasterExtractClose(requestId)) ?? Task.CompletedTask); }
            catch (Exception ex) when (ex is IOException or ObjectDisposedException or InvalidOperationException) { }
        }
    }

    private async Task StopOnTimeoutAsync(Task<ControlMessage> completion, string requestId)
    {
        try
        {
            await completion;
        }
        catch (TimeoutException)
        {
            _recycleOnCancel.TryRemove(requestId, out _);
            int timeoutCount = Interlocked.Increment(ref _timeoutCount);
            int generation = _generation;
            LogHostResources("timeout", generation);
            DiagLog.Write("App", $"ParserHost request timed out; terminating host: request={requestId}; gen={generation}; timeoutCount={timeoutCount}");
            RecycleHost($"request timed out: {requestId}");
        }
        catch
        {
            // Terminal errors and cancellation do not require a process restart.
        }
    }

    private async Task ReadLoopAsync(PipeChannel channel, int generation)
    {
        try
        {
            while (generation == _generation)
            {
                ControlMessage? message = await channel.ReceiveAsync();
                if (message is null)
                    throw new EndOfStreamException("ParserHost pipe closed");
                switch (message)
                {
                    case ParserReady:
                        DiagLog.Write("App", "ParserHost ready");
                        _ready.TrySetResult();
                        break;
                    case PreviewReady ready:
                        _recycleOnCancel.TryRemove(ready.RequestId, out _);
                        _pending.TryComplete(ready.RequestId, ready);
                        break;
                    case PreviewError error:
                        _recycleOnCancel.TryRemove(error.RequestId, out _);
                        _pending.TryComplete(error.RequestId, error);
                        break;
                    case ArchiveEntryExtracted extracted: _pending.TryComplete(extracted.RequestId, extracted); break;
                    case HeroRasterExtracted extracted:
                        _pending.TryComplete(extracted.RequestId, extracted);
                        break;
                }
            }
        }
        catch (Exception ex)
        {
            if (generation != _generation)
                return;
            _ready.TrySetException(ex);
            _recycleOnCancel.Clear();
            _pending.FailAll(ex);
        }
    }

    public void SetBackgroundEfficiency(bool enabled)
    {
        _backgroundEfficiencyEnabled = enabled;
        ProcessPowerMode.SetProcessBackgroundEfficiency(_host, enabled, "App");
    }

    private void OnHostExited(int generation)
    {
        if (_stopping || generation != _generation)
            return;

        int? exitCode = null;
        try { exitCode = _host?.ExitCode; } catch { }
        DiagLog.Write("App", $"ParserHost exited gen={generation}; pid={_host?.Id}; exitCode={exitCode?.ToString() ?? "unknown"}; timeouts={Volatile.Read(ref _timeoutCount)}");
        _recycleOnCancel.Clear();
        _pending.FailAll(new InvalidOperationException("ParserHost exited"));
    }

    public void Stop()
    {
        _stopping = true;
        try { _telemetryCts.Cancel(); } catch { }
        ++_generation;
        _recycleOnCancel.Clear();
        _pending.FailAll(new OperationCanceledException("ParserHost stopped"));
        _ready.TrySetCanceled();
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        TryKillHost();
        try { _host?.Dispose(); } catch { }
        _host = null;
        try { _telemetryCts.Dispose(); } catch { }
    }

    private async Task RunResourceTelemetryAsync(CancellationToken cancellationToken)
    {
        using var timer = new PeriodicTimer(ResourceTelemetryInterval);
        try
        {
            while (await timer.WaitForNextTickAsync(cancellationToken).ConfigureAwait(false))
                LogHostResources("periodic", _generation);
        }
        catch (OperationCanceledException) { }
        catch (Exception ex) { DiagLog.Write("App", "ParserHost resource telemetry failed: " + ex.Message); }
    }

    private void LogHostResources(string reason, int generation, Process? host = null)
    {
        host ??= _host;
        if (host is null || generation != _generation)
            return;

        try
        {
            host.Refresh();
            if (host.HasExited)
                return;

            DiagLog.Write("App", $"ParserHost resources reason={reason}; gen={generation}; pid={host.Id}; privateMiB={host.PrivateMemorySize64 / (1024.0 * 1024.0):0.0}; cpuMs={host.TotalProcessorTime.TotalMilliseconds:0}; handles={host.HandleCount}; timeouts={Volatile.Read(ref _timeoutCount)}");
        }
        catch (Exception ex) when (ex is InvalidOperationException or System.ComponentModel.Win32Exception or NotSupportedException)
        {
            DiagLog.Write("App", $"ParserHost resource sample skipped reason={reason}; gen={generation}: {ex.Message}");
        }
    }

    private void TryKillHost()
    {
        try { _job?.Dispose(); } catch { }
        _job = null;
        try { if (_host is { HasExited: false }) _host.Kill(entireProcessTree: true); } catch { }
    }

    private void RecycleHost(string reason)
    {
        DiagLog.Write("App", $"ParserHost recycle: reason={reason}; gen={_generation}");
        ++_generation;
        _recycleOnCancel.Clear();
        _pending.FailAll(new OperationCanceledException(reason));
        _ready.TrySetCanceled();
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        TryKillHost();
    }

    private NativeRasterImage? ReadHeroRaster(HeroRasterExtracted extracted)
    {
        const int maxRasterBytes = 16 * 1024 * 1024;
        const int maxDimension = 4096;
        try
        {
            if (_host is null)
                return null;
            using var handle = WindowsHandleTransfer.DuplicateFileFromProcess(
                _host.SafeHandle, extracted.FileHandle, extracted.PacketLength);
            if (extracted.PacketLength is <= 8 or > maxRasterBytes)
                return null;
            using var stream = new FileStream(handle, FileAccess.Read);
            if (stream.Length != extracted.PacketLength)
                return null;

            byte[] raster = new byte[checked((int)stream.Length)];
            int offset = 0;
            while (offset < raster.Length)
            {
                int read = stream.Read(raster, offset, raster.Length - offset);
                if (read == 0) return null;
                offset += read;
            }

            int width = BitConverter.ToInt32(raster, 0);
            int height = BitConverter.ToInt32(raster, 4);
            int pixelBytes = checked(width * height * 4);
            if (width is <= 0 or > maxDimension
                || height is <= 0 or > maxDimension
                || extracted.Width != width
                || extracted.Height != height
                || raster.Length != 8 + pixelBytes)
                return null;

            byte[] bgra = new byte[pixelBytes];
            Buffer.BlockCopy(raster, 8, bgra, 0, pixelBytes);
            return new NativeRasterImage(bgra, width, height);
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException or OverflowException)
        {
            return null;
        }
    }

    private ArchiveEntryHandoff? CreateArchiveEntryHandoff(ArchiveEntryExtracted extracted)
    {
        const long maxArchiveEntryBytes = 64L * 1024 * 1024;
        if (_host is null || extracted.FileLength is < 0 or > maxArchiveEntryBytes)
            return null;

        string requestDirectory = Path.Combine(Path.GetTempPath(), "QuickLookNext", "app-preview", extracted.RequestId);
        string extension = Path.GetExtension(extracted.LogicalName);
        if (extension.Length > 32 || extension.Any(static c => !char.IsAsciiLetterOrDigit(c) && c != '.'))
            extension = "";
        string path = Path.Combine(requestDirectory, "entry" + extension.ToLowerInvariant());
        try
        {
            using var sourceHandle = WindowsHandleTransfer.DuplicateFileFromProcess(
                _host.SafeHandle, extracted.FileHandle, extracted.FileLength);
            using var source = new FileStream(sourceHandle, FileAccess.Read);
            string root = Path.GetDirectoryName(requestDirectory)!;
            Directory.CreateDirectory(root);
            if ((File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0)
                return null;
            Directory.CreateDirectory(requestDirectory);
            if ((File.GetAttributes(requestDirectory) & FileAttributes.ReparsePoint) != 0)
                return null;
            var anchor = new FileStream(path, FileMode.CreateNew, FileAccess.ReadWrite, FileShare.Read);
            try
            {
                source.CopyTo(anchor);
                anchor.Flush(flushToDisk: true);
                if (anchor.Length != extracted.FileLength)
                    throw new InvalidDataException("Archive entry changed during anchored copy.");
                anchor.Position = 0;
                return new ArchiveEntryHandoff(extracted.RequestId, path, anchor);
            }
            catch
            {
                anchor.Dispose();
                throw;
            }
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException or OverflowException)
        {
            try { Directory.Delete(requestDirectory, recursive: true); } catch { }
            return null;
        }
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(nint pipe, out uint clientProcessId);
}

internal sealed class ArchiveEntryHandoff(
    string requestId,
    string path,
    FileStream anchor) : IDisposable
{
    private FileStream? _anchor = anchor;
    public string RequestId { get; } = requestId;
    public string Path { get; } = path;

    public void Dispose()
    {
        Interlocked.Exchange(ref _anchor, null)?.Dispose();
        try { File.Delete(Path); } catch { }
        try
        {
            string? directory = System.IO.Path.GetDirectoryName(Path);
            if (directory is not null) Directory.Delete(directory, recursive: false);
        }
        catch { }
    }
}
