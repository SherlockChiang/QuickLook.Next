using System.Diagnostics;
using System.IO.Pipes;
using System.Security.Cryptography;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

internal sealed class ShellBrokerSupervisor(string brokerExePath)
{
    private static readonly TimeSpan StartTimeout = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(45);
    private readonly PendingRequests _pending = new();
    private readonly SemaphoreSlim _startLock = new(1, 1);
    private NamedPipeServerStream? _server;
    private ShellBrokerChannel? _channel;
    private Process? _broker;
    private HostProcessJob? _job;
    private string? _writableRoot;
    private TaskCompletionSource _ready = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private int _generation;

    public async Task EnsureStartedAsync(CancellationToken cancellationToken)
    {
        if (IsConnected) return;
        await _startLock.WaitAsync(cancellationToken);
        try
        {
            if (IsConnected) return;
            StopCore();
            int generation = ++_generation;
            _ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            string pipeName = $"quicklook_next_shell_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
            string sessionToken = RandomNumberGenerator.GetHexString(32);
            _writableRoot = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "QuickLookNext", "ShellBroker", "work", "broker-" + RandomNumberGenerator.GetHexString(16).ToLowerInvariant());
            Directory.CreateDirectory(_writableRoot);
            HostProcessLauncher.GrantRestrictedWriteAccess(_writableRoot);
            _server = HostProcessLauncher.CreateWriteRestrictedPipe(pipeName);
            var job = new HostProcessJob((nint)(256L * 1024 * 1024));
            try
            {
                _broker = HostProcessLauncher.StartRestricted(
                    brokerExePath,
                    ["--pipe", pipeName, "--session-token", sessionToken, "--writable-root", _writableRoot],
                    job,
                    restrictWrites: true);
                _job = job;
                using var connectCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
                connectCts.CancelAfter(StartTimeout);
                await _server.WaitForConnectionAsync(connectCts.Token);
                WindowsHandleTransfer.VerifyNamedPipeClientProcess(_server.SafePipeHandle, _broker.Id);
                _channel = new ShellBrokerChannel(_server);
                TaskCompletionSource ready = _ready;
                _ = ReadLoopAsync(_channel, generation, ready);
                await _channel.SendAsync($"HELLO\t{Environment.ProcessId}\t{sessionToken}", connectCts.Token);
                await ready.Task.WaitAsync(connectCts.Token);
            }
            catch (Exception ex)
            {
                int? exitCode = null;
                try { if (_broker is { HasExited: true }) exitCode = _broker.ExitCode; } catch { }
                string? detail = null;
                try
                {
                    string failurePath = Path.Combine(_writableRoot!, "startup-failure.txt");
                    if (File.Exists(failurePath)) detail = File.ReadAllText(failurePath);
                }
                catch { }
                job.Dispose();
                StopCore();
                throw new InvalidOperationException(
                    $"ShellBroker failed to start; exitCode={exitCode?.ToString() ?? "running"}; detail={detail ?? "none"}.", ex);
            }
        }
        finally
        {
            _startLock.Release();
        }
    }

    public async Task<NativeRasterImage?> GetThumbnailAsync(string path, int size, CancellationToken cancellationToken)
    {
        if (_channel is null || _broker is null)
            throw new InvalidOperationException("ShellBroker is not connected.");
        var (requestId, completion) = _pending.Begin(RequestTimeout);
        bool receivedTerminal = false;
        bool receivedThumbnail = false;
        bool validatedThumbnail = false;
        try
        {
            string encodedPath = Convert.ToBase64String(System.Text.Encoding.UTF8.GetBytes(path));
            await _channel.SendAsync($"OPEN\t{requestId}\t{Math.Clamp(size, 16, 512)}\t{encodedPath}", cancellationToken);
            ControlMessage response = await completion.WaitAsync(cancellationToken);
            receivedTerminal = true;
            if (response is not ShellThumbnailReady ready
                || !ShellBrokerProtocol.TryGetPixelByteCount(ready, out int pixelBytes))
                return null;
            receivedThumbnail = true;
            using var handle = WindowsHandleTransfer.DuplicateFileFromProcess(
                _broker.SafeHandle, ready.FileHandle, ready.PacketLength);
            using var stream = new FileStream(handle, FileAccess.Read);
            byte[] header = new byte[8];
            stream.ReadExactly(header);
            if (!ShellBrokerProtocol.HeaderMatches(ready, header))
                return null;
            var bgra = new byte[pixelBytes];
            stream.ReadExactly(bgra);
            if (stream.Position != stream.Length)
                return null;
            validatedThumbnail = true;
            return new NativeRasterImage(bgra, ready.Width, ready.Height);
        }
        finally
        {
            _pending.Cancel(requestId);
            if (!receivedTerminal
                || cancellationToken.IsCancellationRequested
                || (receivedThumbnail && !validatedThumbnail))
                Stop();
            else
                try { await (_channel?.SendAsync($"CLOSE\t{requestId}") ?? Task.CompletedTask); } catch { }
        }
    }

    private async Task ReadLoopAsync(
        ShellBrokerChannel channel,
        int generation,
        TaskCompletionSource readyCompletion)
    {
        try
        {
            while (generation == _generation && await channel.ReceiveAsync() is { } message)
            {
                ControlMessage parsed = ShellBrokerProtocol.Parse(message);
                bool accepted = parsed switch
                {
                    ShellBrokerReady => readyCompletion.TrySetResult(),
                    ShellThumbnailReady ready => _pending.TryComplete(ready.RequestId, ready),
                    PreviewError error => _pending.TryComplete(error.RequestId, error),
                    _ => false,
                };
                if (!accepted)
                    throw new InvalidDataException("ShellBroker returned an unsolicited control message.");
            }
            if (generation == _generation)
            {
                string? detail = null;
                try
                {
                    string failurePath = Path.Combine(_writableRoot!, "startup-failure.txt");
                    if (File.Exists(failurePath)) detail = File.ReadAllText(failurePath);
                }
                catch { }
                int? exitCode = null;
                try
                {
                    if (_broker is not null)
                    {
                        await _broker.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(1));
                        exitCode = _broker.ExitCode;
                    }
                }
                catch { }
                throw new EndOfStreamException(
                    $"ShellBroker disconnected; exitCode={exitCode?.ToString() ?? "running"}; detail={detail ?? "none"}.");
            }
        }
        catch (Exception ex)
        {
            readyCompletion.TrySetException(ex);
            await _startLock.WaitAsync();
            try
            {
                if (generation != _generation)
                    return;
                _pending.FailAll(ex);
                ++_generation;
                StopCore();
            }
            finally { _startLock.Release(); }
        }
    }

    private bool IsConnected
    {
        get { try { return _channel is not null && _broker is { HasExited: false }; } catch { return false; } }
    }

    public void Stop()
    {
        ++_generation;
        StopCore();
    }

    private void StopCore()
    {
        _pending.FailAll(new OperationCanceledException("ShellBroker stopped."));
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        try { if (_broker is { HasExited: false }) _broker.Kill(entireProcessTree: true); } catch { }
        try { _broker?.Dispose(); } catch { }
        _broker = null;
        try { _job?.Dispose(); } catch { }
        _job = null;
        if (_writableRoot is { } root)
        {
            try { Directory.Delete(root, recursive: true); } catch { }
            _writableRoot = null;
        }
    }
}

internal sealed class ShellBrokerChannel(Stream stream) : IDisposable
{
    private readonly StreamReader _reader = new(stream, System.Text.Encoding.UTF8, detectEncodingFromByteOrderMarks: false);
    private readonly StreamWriter _writer = new(stream, new System.Text.UTF8Encoding(false)) { AutoFlush = true };
    private readonly SemaphoreSlim _writeLock = new(1, 1);

    public async Task<string?> ReceiveAsync(CancellationToken cancellationToken = default)
    {
        var line = new System.Text.StringBuilder();
        var buffer = new char[1];
        while (true)
        {
            int read = await _reader.ReadAsync(buffer.AsMemory(), cancellationToken);
            if (read == 0) return line.Length == 0 ? null : line.ToString();
            if (buffer[0] == '\n')
            {
                if (line.Length > 0 && line[^1] == '\r') line.Length--;
                return line.ToString();
            }
            if (line.Length >= PipeChannel.MaxControlLineChars)
                throw new InvalidDataException("ShellBroker control message is too large.");
            line.Append(buffer[0]);
        }
    }

    public async Task SendAsync(string line, CancellationToken cancellationToken = default)
    {
        if (line.Length > PipeChannel.MaxControlLineChars)
            throw new InvalidDataException("ShellBroker control message is too large.");
        await _writeLock.WaitAsync(cancellationToken);
        try { await _writer.WriteLineAsync(line.AsMemory(), cancellationToken); }
        finally { _writeLock.Release(); }
    }

    public void Dispose()
    {
        _reader.Dispose();
        _writer.Dispose();
        _writeLock.Dispose();
    }
}
