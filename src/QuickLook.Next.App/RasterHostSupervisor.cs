using System.Diagnostics;
using System.IO.Pipes;
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
    private const string PipeName = "quicklook_next";
    private static readonly TimeSpan PreviewTimeout = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan HostConnectTimeout = TimeSpan.FromSeconds(5);

    private readonly string _hostExePath;
    private readonly DispatcherQueue _ui;
    private readonly PendingRequests _pending = new();

    private NamedPipeServerStream? _server;
    private PipeChannel? _channel;
    private Process? _host;
    private int _generation;
    private int _restartAttempts;
    private bool _stopping;
    private bool _backgroundEfficiencyEnabled = true;
    private readonly SemaphoreSlim _startLock = new(1, 1);

    /// <summary>Raised on the UI thread when the host hands over a (new) shared surface to compose.</summary>
    public event Action<PreviewSurface>? SurfaceReceived;

    public long AdapterLuid { get; private set; }

    public RasterHostSupervisor(string hostExePath, DispatcherQueue ui)
    {
        _hostExePath = hostExePath;
        _ui = ui;
    }

    public async Task StartAsync()
    {
        using var trace = DiagLog.TraceScope("App", "host start", 500);
        _stopping = false;
        int gen = ++_generation;
        // Dispose the old channel + process + pipe before creating new ones (prevents handle leaks on restart).
        _channel?.Dispose();
        _channel = null;
        try { _host?.Dispose(); } catch { }
        _host = null;
        _server?.Dispose();
        _server = new NamedPipeServerStream(PipeName, PipeDirection.InOut, 1,
            PipeTransmissionMode.Byte, PipeOptions.Asynchronous);

        DiagLog.Write("App", $"launching host: {_hostExePath} (exists={File.Exists(_hostExePath)})");
        var psi = new ProcessStartInfo(_hostExePath)
        {
            Arguments = $"--pipe {PipeName}",
            UseShellExecute = false,
            CreateNoWindow = true,
            WindowStyle = ProcessWindowStyle.Hidden,
        };
        _host = Process.Start(psi);
        if (_host is null)
            throw new InvalidOperationException("RasterHost process did not start");
        ProcessPowerMode.SetProcessBackgroundEfficiency(_host, _backgroundEfficiencyEnabled, "App");
        if (_host is not null) { _host.EnableRaisingEvents = true; _host.Exited += (_, _) => OnHostExited(gen); }
        DiagLog.Write("App", $"host pid={_host?.Id}; waiting for pipe connection");

        using var connectCts = new CancellationTokenSource(HostConnectTimeout);
        try
        {
            await _server.WaitForConnectionAsync(connectCts.Token);
            DiagLog.Write("App", $"host pipe connected gen={gen}");
        }
        catch
        {
            TryKillHost();
            throw;
        }

        _channel = new PipeChannel(_server);
        await _channel.SendAsync(new Hello(Environment.ProcessId));
        DiagLog.Write("App", "host connected; sent hello");
        _restartAttempts = 0;
        _ = ReadLoopAsync(_channel, gen);
    }

    public async Task EnsureStartedAsync()
    {
        if (IsConnected)
            return;

        DiagLog.Write("App", "host ensure start waiting");
        await _startLock.WaitAsync();
        try
        {
            if (!IsConnected)
                await StartAsync();
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
                    break;
                }
                Dispatch(msg);
            }
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "host read loop failed: " + ex.Message);
            _pending.FailAll(ex);
        }
    }

    private void Dispatch(ControlMessage msg)
    {
        DiagLog.Write("App", "host recv " + msg.GetType().Name);
        switch (msg)
        {
            case HostReady ready: AdapterLuid = ready.AdapterLuid; break;
            case PreviewSurface surface: _ui.TryEnqueue(() => SurfaceReceived?.Invoke(surface)); break;
            case PreviewReady ready: _pending.TryComplete(ready.RequestId, ready); break;
            case PreviewError error: _pending.TryComplete(error.RequestId, error); break;
        }
    }

    /// <summary>Open a file for preview; resolves on PreviewReady, PreviewError, or a watchdog timeout.</summary>
    public (string RequestId, Task<ControlMessage> Completion) BeginOpen(string path, FileProbe probe)
    {
        if (_channel is null) throw new InvalidOperationException("RasterHost not connected");
        var (requestId, completion) = _pending.Begin(PreviewTimeout);
        _ = SendOpenAsync(requestId, path, probe);
        return (requestId, completion);
    }

    private async Task SendOpenAsync(string requestId, string path, FileProbe probe)
    {
        try
        {
            if (_channel is null) throw new InvalidOperationException("RasterHost not connected");
            using var trace = DiagLog.TraceScope("App", $"host send open request={requestId}; path={path}", 100);
            await _channel.SendAsync(new PreviewOpen(requestId, path, probe));
        }
        catch (Exception ex)
        {
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

    public async Task RenderPageAsync(string requestId, int pageIndex, double scale)
    {
        if (_channel is not null)
        {
            DiagLog.Write("App", $"host send page open request={requestId}; page={pageIndex}; scale={scale:0.###}");
            await _channel.SendAsync(new PreviewPageOpen(requestId, pageIndex, scale));
        }
    }

    public async Task ClosePageAsync(string requestId, int pageIndex)
    {
        if (_channel is not null)
        {
            DiagLog.Write("App", $"host send page close request={requestId}; page={pageIndex}");
            await _channel.SendAsync(new PreviewPageClose(requestId, pageIndex));
        }
    }

    public async Task CloseAsync(string requestId)
    {
        if (_channel is not null)
        {
            DiagLog.Write("App", $"host send close request={requestId}");
            await _channel.SendAsync(new PreviewClose(requestId));
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
        DiagLog.Write("App", $"host exited gen={gen}; scheduling restart");
        _pending.FailAll(new InvalidOperationException("RasterHost exited"));
        _ = RestartAsync(gen);
    }

    private async Task RestartAsync(int gen)
    {
        try
        {
            int attempt = Math.Min(Interlocked.Increment(ref _restartAttempts), 5);
            await Task.Delay(TimeSpan.FromMilliseconds(250 * attempt * attempt));
            if (_stopping || gen != _generation) return;

            _ui.TryEnqueue(() => _ = StartRestartOnUiAsync(gen));
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "host restart scheduling FAILED: " + ex);
            _pending.FailAll(ex);
        }
    }

    private async Task StartRestartOnUiAsync(int gen)
    {
        try
        {
            if (!_stopping && gen == _generation)
                await StartAsync();
        }
        catch (Exception ex)
        {
            DiagLog.Write("App", "host restart FAILED: " + ex);
            _pending.FailAll(ex);
        }
    }

    public void Stop()
    {
        DiagLog.Write("App", "host stop");
        _stopping = true;
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        TryKillHost();
    }

    private void TryKillHost()
    {
        try
        {
            if (_host is { HasExited: false })
                _host.Kill();
        }
        catch { }
    }
}
