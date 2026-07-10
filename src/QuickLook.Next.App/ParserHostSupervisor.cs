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
    private readonly string _hostExePath;
    private readonly PendingRequests _pending = new();
    private readonly SemaphoreSlim _startLock = new(1, 1);
    private NamedPipeServerStream? _server;
    private PipeChannel? _channel;
    private Process? _host;
    private string? _sessionToken;
    private int _generation;
    private bool _stopping;
    private bool _backgroundEfficiencyEnabled = true;
    private TaskCompletionSource _ready = new(TaskCreationOptions.RunContinuationsAsynchronously);

    public ParserHostSupervisor(string hostExePath) => _hostExePath = hostExePath;

    public async Task EnsureStartedAsync()
    {
        if (IsConnected) return;
        await _startLock.WaitAsync();
        try { if (!IsConnected) await StartAsync(); }
        finally { _startLock.Release(); }
    }

    private async Task StartAsync()
    {
        _stopping = false;
        int generation = ++_generation;
        _ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        _channel?.Dispose();
        _server?.Dispose();
        try { _host?.Dispose(); } catch { }
        string pipeName = $"quicklook_next_parser_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        _sessionToken = RandomNumberGenerator.GetHexString(32);
        _server = new NamedPipeServerStream(pipeName, PipeDirection.InOut, 1, PipeTransmissionMode.Byte,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
        var startInfo = new ProcessStartInfo(_hostExePath) { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden };
        startInfo.ArgumentList.Add("--pipe");
        startInfo.ArgumentList.Add(pipeName);
        startInfo.ArgumentList.Add("--session-token");
        startInfo.ArgumentList.Add(_sessionToken);
        _host = Process.Start(startInfo) ?? throw new InvalidOperationException("ParserHost process did not start");
        ProcessPowerMode.SetProcessBackgroundEfficiency(_host, _backgroundEfficiencyEnabled, "App");
        _host.EnableRaisingEvents = true;
        _host.Exited += (_, _) => OnHostExited(generation);
        try
        {
            using var connectCts = new CancellationTokenSource(HostConnectTimeout);
            await _server.WaitForConnectionAsync(connectCts.Token);
            if (!GetNamedPipeClientProcessId(_server.SafePipeHandle.DangerousGetHandle(), out uint clientPid) || clientPid != _host.Id)
                throw new InvalidOperationException("ParserHost pipe client did not match the launched process");
        }
        catch { TryKillHost(); throw; }

        _channel = new PipeChannel(_server);
        await _channel.SendAsync(new Hello(Environment.ProcessId, _sessionToken));
        _ = ReadLoopAsync(_channel, generation);
        using var readyCts = new CancellationTokenSource(HostConnectTimeout);
        await _ready.Task.WaitAsync(readyCts.Token);
    }

    private bool IsConnected
    {
        get { try { return _channel is not null && _host is { HasExited: false }; } catch { return false; } }
    }

    public (string RequestId, Task<ControlMessage> Completion) BeginOpen(string path, FileProbe probe)
    {
        if (_channel is null) throw new InvalidOperationException("ParserHost not connected");
        var (requestId, completion) = _pending.Begin(PreviewTimeout);
        _ = SendOpenAsync(requestId, path, probe);
        return (requestId, completion);
    }

    private async Task SendOpenAsync(string requestId, string path, FileProbe probe)
    {
        try { await (_channel?.SendAsync(new PreviewOpen(requestId, path, probe)) ?? Task.FromException(new InvalidOperationException("ParserHost not connected"))); }
        catch (Exception ex) { _pending.TryComplete(requestId, new PreviewError(requestId, ex.Message)); }
    }

    public Task CloseAsync(string requestId) => _channel?.SendAsync(new PreviewClose(requestId)) ?? Task.CompletedTask;

    private async Task ReadLoopAsync(PipeChannel channel, int generation)
    {
        try
        {
            while (generation == _generation && await channel.ReceiveAsync() is { } message)
            {
                switch (message)
                {
                    case ParserReady:
                        DiagLog.Write("App", "ParserHost ready");
                        _ready.TrySetResult();
                        break;
                    case PreviewReady ready: _pending.TryComplete(ready.RequestId, ready); break;
                    case PreviewError error: _pending.TryComplete(error.RequestId, error); break;
                }
            }
        }
        catch (Exception ex)
        {
            _ready.TrySetException(ex);
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
        if (!_stopping && generation == _generation)
            _pending.FailAll(new InvalidOperationException("ParserHost exited"));
    }

    public void Stop()
    {
        _stopping = true;
        ++_generation;
        _pending.FailAll(new OperationCanceledException("ParserHost stopped"));
        _ready.TrySetCanceled();
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        TryKillHost();
    }

    private void TryKillHost()
    {
        try { if (_host is { HasExited: false }) _host.Kill(); } catch { }
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool GetNamedPipeClientProcessId(nint pipe, out uint clientProcessId);
}
