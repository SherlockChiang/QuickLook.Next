using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Microsoft.UI.Dispatching;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

/// <summary>
/// Owns the App↔RasterHost boundary: launches the host, owns the control pipe (App is the server),
/// performs the Hello/HostReady handshake, dispatches messages, and restarts the host on crash
/// (App-as-supervisor, validated in Spike 1). Preview opens are tracked with a watchdog (Core).
/// </summary>
internal sealed class RasterHostSupervisor
{
    private static readonly TimeSpan PreviewTimeout = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan HostConnectTimeout = TimeSpan.FromSeconds(5);

    private readonly string _hostExePath;
    private readonly DispatcherQueue _ui;
    private readonly PendingRequests _pending = new();
    private readonly ConcurrentDictionary<string, byte> _cloudOriginRequests = new();
    private readonly ConcurrentDictionary<(string RequestId, int PageIndex, long PageGeneration), byte> _pendingCloudPages = new();

    private NamedPipeServerStream? _server;
    private PipeChannel? _channel;
    private Process? _host;
    private HostProcessJob? _job;
    private string? _sessionToken;
    private int _generation;
    private int _restartAttempts;
    private bool _stopping;
    private bool _backgroundEfficiencyEnabled = true;
    private readonly SemaphoreSlim _startLock = new(1, 1);
    private readonly object _stateLock = new();
    private TaskCompletionSource _ready = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private string? _activeRequestId;
    private string? _activePath;

    /// <summary>Raised on the UI thread when the host hands over a (new) shared surface to compose.</summary>
    public event Action<PreviewSurface>? SurfaceReceived;
    public event Action<PreviewPageError>? PageErrorReceived;

    public long AdapterLuid { get; private set; }

    public RasterHostSupervisor(string hostExePath, DispatcherQueue ui)
    {
        _hostExePath = hostExePath;
        _ui = ui;
    }

    private async Task StartCoreAsync(CancellationToken cancellationToken)
    {
        using var trace = DiagLog.TraceScope("App", "host start", 500);
        _stopping = false;
        int gen = ++_generation;
        _ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        // Dispose the old channel + process + pipe before creating new ones (prevents handle leaks on restart).
        _channel?.Dispose();
        _channel = null;
        try { _job?.Dispose(); } catch { }
        _job = null;
        try { _host?.Dispose(); } catch { }
        _host = null;
        _server?.Dispose();
        string pipeName = $"quicklook_next_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        _sessionToken = RandomNumberGenerator.GetHexString(32);
        _server = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1,
            PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);

        DiagLog.Write("App", $"launching host: {_hostExePath} (exists={File.Exists(_hostExePath)})");
        var job = new HostProcessJob((nint)(1024L * 1024 * 1024));
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
        if (_host is not null) { _host.EnableRaisingEvents = true; _host.Exited += (_, _) => OnHostExited(gen); }
        DiagLog.Write("App", $"host pid={_host?.Id}; waiting for pipe connection");

        using var connectCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        connectCts.CancelAfter(HostConnectTimeout);
        try
        {
            await _server.WaitForConnectionAsync(connectCts.Token);
            if (!GetNamedPipeClientProcessId(_server.SafePipeHandle.DangerousGetHandle(), out uint clientPid)
                || _host is null
                || clientPid != _host.Id)
            {
                throw new InvalidOperationException("RasterHost pipe client did not match the launched process");
            }
            DiagLog.Write("App", $"host pipe connected gen={gen}");
        }
        catch
        {
            TryKillHost();
            throw;
        }

        _channel = new PipeChannel(_server);
        await _channel.SendAsync(new Hello(Environment.ProcessId, _sessionToken));
        DiagLog.Write("App", "host connected; sent hello");
        _restartAttempts = 0;
        _ = ReadLoopAsync(_channel, gen);
        try
        {
            using var readyCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            readyCts.CancelAfter(HostConnectTimeout);
            await _ready.Task.WaitAsync(readyCts.Token);
        }
        catch
        {
            _channel?.Dispose();
            _channel = null;
            TryKillHost();
            throw;
        }
    }

    public async Task EnsureStartedAsync(CancellationToken cancellationToken = default)
    {
        if (IsConnected)
            return;

        DiagLog.Write("App", "host ensure start waiting");
        await _startLock.WaitAsync(cancellationToken);
        try
        {
            if (!IsConnected)
                await StartCoreAsync(cancellationToken);
        }
        finally
        {
            _startLock.Release();
        }
    }

    private bool IsConnected
    {
        get
        {
            try { return _channel is not null && _host is { HasExited: false }; }
            catch { return false; }
        }
    }

    private async Task ReadLoopAsync(PipeChannel channel, int gen)
    {
        try
        {
            DiagLog.Write("App", $"host read loop start gen={gen}");
            while (true)
            {
                ControlMessage? msg = await channel.ReceiveAsync();
                if (msg is null || gen != _generation)
                {
                    DiagLog.Write("App", $"host read loop stop gen={gen}; msgNull={msg is null}; currentGen={_generation}");
                    if (msg is null && gen == _generation)
                        throw new EndOfStreamException("RasterHost pipe closed");
                    break;
                }
                Dispatch(msg);
            }
        }
        catch (Exception ex)
        {
            if (gen != _generation)
                return;
            DiagLog.Write("App", "host read loop failed: " + ex.Message);
            _ready.TrySetException(ex);
            ClearCloudRequestState();
            _pending.FailAll(ex);
        }
    }

    private void Dispatch(ControlMessage msg)
    {
        DiagLog.Write("App", "host recv " + msg.GetType().Name);
        switch (msg)
        {
            case HostReady ready:
                AdapterLuid = ready.AdapterLuid;
                _ready.TrySetResult();
                break;
            case PreviewSurface surface:
                if (surface.PageIndex >= 0)
                    _pendingCloudPages.TryRemove((surface.RequestId, surface.PageIndex, surface.PageGeneration), out _);
                _ui.TryEnqueue(() => SurfaceReceived?.Invoke(surface));
                break;
            case PreviewReady ready:
                _pending.TryComplete(ready.RequestId, ready);
                break;
            case PreviewAnimationFramesReady animation:
                if (!_pending.TryComplete(animation.RequestId, animation))
                    WindowsHandleTransfer.CloseReceivedFileHandle(animation.FileHandle);
                break;
            case PreviewError error:
                RemoveCloudRequestState(error.RequestId);
                _pending.TryComplete(error.RequestId, error);
                break;
            case PreviewPageError pageError:
                _pendingCloudPages.TryRemove((pageError.RequestId, pageError.PageIndex, pageError.PageGeneration), out _);
                if (pageError.TimedOut)
                    RecycleHost(pageError.RequestId, $"PDF page timed out: page={pageError.PageIndex}; generation={pageError.PageGeneration}");
                _ui.TryEnqueue(() => PageErrorReceived?.Invoke(pageError));
                break;
        }
    }

    /// <summary>Open a file for preview; resolves on PreviewReady, PreviewError, or a watchdog timeout.</summary>
    public (string RequestId, Task<ControlMessage> Completion) BeginOpen(
        string path,
        FileProbe probe,
        uint targetWidth = 0,
        uint targetHeight = 0,
        TimeSpan? timeout = null,
        bool recycleHostOnCancel = false)
    {
        if (_channel is null) throw new InvalidOperationException("RasterHost not connected");
        var (requestId, completion) = _pending.Begin(timeout ?? PreviewTimeout);
        if (recycleHostOnCancel)
            _cloudOriginRequests[requestId] = 0;
        _ = StopOnTimeoutAsync(completion, requestId);
        lock (_stateLock)
        {
            _activeRequestId = requestId;
            _activePath = path;
        }
        _ = SendOpenAsync(requestId, path, probe, targetWidth, targetHeight);
        return (requestId, completion);
    }

    private async Task StopOnTimeoutAsync(Task<ControlMessage> completion, string requestId)
    {
        try
        {
            await completion;
        }
        catch (TimeoutException)
        {
            RemoveCloudRequestState(requestId);
            DiagLog.Write("App", $"RasterHost request timed out; terminating host: request={requestId}; gen={_generation}");
            TryKillHost();
        }
        catch
        {
            // Terminal errors and cancellation do not require a process restart.
        }
    }

    private async Task SendOpenAsync(string requestId, string path, FileProbe probe, uint targetWidth, uint targetHeight)
    {
        try
        {
            if (_channel is null) throw new InvalidOperationException("RasterHost not connected");
            using var trace = DiagLog.TraceScope("App", $"host send open request={requestId}; target={targetWidth}x{targetHeight}; path={path}", 100);
            await _channel.SendAsync(new PreviewOpen(requestId, path, probe)
            {
                TargetWidth = targetWidth,
                TargetHeight = targetHeight,
            });
        }
        catch (Exception ex)
        {
            RemoveCloudRequestState(requestId);
            _pending.TryComplete(requestId, new PreviewError(requestId, ex.Message));
        }
    }

    public async Task ResizeAsync(string requestId, uint width, uint height, double dpi)
    {
        if (_channel is not null)
        {
            DiagLog.Write("App", $"host send resize request={requestId}; size={width}x{height}; dpi={dpi:0.##}");
            await _channel.SendAsync(new PreviewResize(requestId, width, height, dpi));
        }
    }

    public async Task RenderPageAsync(string requestId, int pageIndex, long pageGeneration, double scale)
    {
        var key = (requestId, pageIndex, pageGeneration);
        bool trackCloudPage = _cloudOriginRequests.ContainsKey(requestId);
        if (trackCloudPage)
            _pendingCloudPages[key] = 0;
        if (_channel is not null)
        {
            try
            {
                DiagLog.Write("App", $"host send page open request={requestId}; page={pageIndex}; scale={scale:0.###}");
                await _channel.SendAsync(new PreviewPageOpen(requestId, pageIndex, pageGeneration, scale));
            }
            catch
            {
                if (trackCloudPage)
                    _pendingCloudPages.TryRemove(key, out _);
                throw;
            }
        }
        else if (trackCloudPage)
            _pendingCloudPages.TryRemove(key, out _);
    }

    public async Task ClosePageAsync(string requestId, int pageIndex, long pageGeneration)
    {
        if (_pendingCloudPages.TryRemove((requestId, pageIndex, pageGeneration), out _))
        {
            RecycleHost(requestId, $"cloud PDF page canceled: page={pageIndex}; generation={pageGeneration}");
            return;
        }
        if (_channel is not null)
        {
            DiagLog.Write("App", $"host send page close request={requestId}; page={pageIndex}");
            await _channel.SendAsync(new PreviewPageClose(requestId, pageIndex, pageGeneration));
        }
    }

    public async Task<NativeAnimationFrames?> ExtractAnimationFramesAsync(
        string previewRequestId,
        uint targetWidth,
        uint targetHeight,
        CancellationToken cancellationToken)
    {
        if (_channel is null)
            throw new InvalidOperationException("RasterHost not connected");

        var (requestId, completion) = _pending.Begin(PreviewTimeout);
        try
        {
            await _channel.SendAsync(new PreviewAnimationFramesOpen(
                requestId, previewRequestId, targetWidth, targetHeight));
            ControlMessage terminal = await completion.WaitAsync(cancellationToken);
            if (terminal is PreviewError)
                return null;
            if (terminal is not PreviewAnimationFramesReady ready)
                return null;
            if (!string.Equals(ready.PreviewRequestId, previewRequestId, StringComparison.Ordinal))
            {
                WindowsHandleTransfer.CloseReceivedFileHandle(ready.FileHandle);
                return null;
            }
            return ReadAnimationFrames(ready);
        }
        catch (TimeoutException)
        {
            RecycleHost(previewRequestId, "animation frame decode timed out");
            throw;
        }
        finally
        {
            _pending.Cancel(requestId);
            try
            {
                if (_channel is not null)
                    await _channel.SendAsync(new PreviewAnimationFramesClose(requestId));
            }
            catch { }
        }
    }

    private static NativeAnimationFrames? ReadAnimationFrames(PreviewAnimationFramesReady ready)
    {
        const long maxPacketBytes = 64L * 1024 * 1024 + 12;
        try
        {
            using var handle = WindowsHandleTransfer.TakeReceivedFileHandle(ready.FileHandle);
            if (ready.PacketLength <= 12 || ready.PacketLength > maxPacketBytes
                || ready.FrameCount is <= 0 or > 120
                || ready.Width is <= 0 or > 1024
                || ready.Height is <= 0 or > 1024)
                return null;
            using var stream = new FileStream(handle, FileAccess.Read);
            if (stream.Length != ready.PacketLength)
                return null;
            Span<byte> header = stackalloc byte[12];
            stream.ReadExactly(header);
            int count = checked((int)BitConverter.ToUInt32(header[..4]));
            int width = checked((int)BitConverter.ToUInt32(header[4..8]));
            int height = checked((int)BitConverter.ToUInt32(header[8..12]));
            int frameBytes = checked(width * height * 4);
            long expectedLength = checked(12L + count * (4L + frameBytes));
            if (count != ready.FrameCount || width != ready.Width || height != ready.Height
                || expectedLength != stream.Length)
                return null;

            var frames = new List<NativeAnimationFrame>(count);
            Span<byte> delayBytes = stackalloc byte[4];
            for (int i = 0; i < count; i++)
            {
                stream.ReadExactly(delayBytes);
                int delay = checked((int)BitConverter.ToUInt32(delayBytes));
                if (delay is < 20 or > 1000)
                    return null;
                var bgra = new byte[frameBytes];
                stream.ReadExactly(bgra);
                frames.Add(new NativeAnimationFrame(delay, bgra));
            }
            return stream.Position == stream.Length ? new NativeAnimationFrames(width, height, frames) : null;
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or UnauthorizedAccessException or NotSupportedException or OverflowException)
        {
            return null;
        }
    }

    public async Task CloseAsync(string requestId)
    {
        bool wasPending = _pending.Cancel(requestId);
        bool cloudOrigin = _cloudOriginRequests.TryRemove(requestId, out _);
        bool recycleHost = wasPending && cloudOrigin;
        RemovePendingCloudPages(requestId);
        lock (_stateLock)
        {
            if (_activeRequestId == requestId)
            {
                _activeRequestId = null;
                _activePath = null;
            }
        }
        try
        {
            if (_channel is not null)
            {
                DiagLog.Write("App", $"host send close request={requestId}");
                await _channel.SendAsync(new PreviewClose(requestId));
            }
        }
        finally
        {
            if (recycleHost)
                RecycleHost(requestId, "cloud preview canceled while opening");
        }
    }

    public void SetBackgroundEfficiency(bool enabled)
    {
        _backgroundEfficiencyEnabled = enabled;
        ProcessPowerMode.SetProcessBackgroundEfficiency(_host, enabled, "App");
    }

    private void OnHostExited(int gen)
    {
        if (_stopping || gen != _generation) return;
        (string? requestId, string? path) = GetRestartContext();
        DiagLog.Write("App", $"host exited gen={gen}; request={requestId}; scheduling restart");
        ClearCloudRequestState();
        _pending.FailAll(new InvalidOperationException("RasterHost exited"));
        _ = RestartAsync(gen, requestId, path);
    }

    private async Task RestartAsync(int gen, string? requestId, string? path)
    {
        try
        {
            int attempt = Math.Min(Interlocked.Increment(ref _restartAttempts), 5);
            await Task.Delay(TimeSpan.FromMilliseconds(250 * attempt * attempt));
            if (!IsRestartContextCurrent(gen, requestId, path)) return;

            _ui.TryEnqueue(() => _ = StartRestartOnUiAsync(gen, requestId, path));
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "host restart scheduling FAILED: " + ex);
            _pending.FailAll(ex);
        }
    }

    private async Task StartRestartOnUiAsync(int gen, string? requestId, string? path)
    {
        try
        {
            await _startLock.WaitAsync();
            try
            {
                if (!IsConnected && IsRestartContextCurrent(gen, requestId, path))
                    await StartCoreAsync(CancellationToken.None);
            }
            finally
            {
                _startLock.Release();
            }
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "host restart FAILED: " + ex);
            _pending.FailAll(ex);
        }
    }

    private (string? RequestId, string? Path) GetRestartContext()
    {
        lock (_stateLock)
            return (_activeRequestId, _activePath);
    }

    private bool IsRestartContextCurrent(int gen, string? requestId, string? path)
    {
        if (_stopping || gen != _generation || requestId is null || path is null)
            return false;

        lock (_stateLock)
            return _activeRequestId == requestId && string.Equals(_activePath, path, StringComparison.OrdinalIgnoreCase);
    }

    public void Stop()
    {
        DiagLog.Write("App", "host stop");
        _stopping = true;
        ++_generation;
        ClearCloudRequestState();
        _ready.TrySetCanceled();
        _pending.FailAll(new OperationCanceledException("RasterHost stopped"));
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        TryKillHost();
    }

    private void TryKillHost()
    {
        try { _job?.Dispose(); } catch { }
        _job = null;
        try
        {
            if (_host is { HasExited: false })
                _host.Kill();
        }
        catch { }
    }

    private void RecycleHost(string requestId, string reason)
    {
        DiagLog.Write("App", $"recycling RasterHost: request={requestId}; reason={reason}");
        lock (_stateLock)
        {
            if (_activeRequestId == requestId)
            {
                _activeRequestId = null;
                _activePath = null;
            }
        }
        ++_generation;
        ClearCloudRequestState();
        _pending.FailAll(new OperationCanceledException(reason));
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        TryKillHost();
    }

    private void RemoveCloudRequestState(string requestId)
    {
        _cloudOriginRequests.TryRemove(requestId, out _);
        RemovePendingCloudPages(requestId);
    }

    private void RemovePendingCloudPages(string requestId)
    {
        foreach (var key in _pendingCloudPages.Keys)
            if (key.RequestId == requestId)
                _pendingCloudPages.TryRemove(key, out _);
    }

    private void ClearCloudRequestState()
    {
        _cloudOriginRequests.Clear();
        _pendingCloudPages.Clear();
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(nint pipe, out uint clientProcessId);
}
