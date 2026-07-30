using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using Microsoft.UI.Dispatching;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;

namespace QuickLook.Next.App;

/// <summary>Supervises the JSON-only native parser process. It intentionally has no surface support.</summary>
internal sealed class ParserHostSupervisor
{
    private static readonly TimeSpan PreviewTimeout = TimeSpan.FromSeconds(5);
    private static readonly TimeSpan HostConnectTimeout = TimeSpan.FromSeconds(15);
    private static readonly TimeSpan ResourceTelemetryInterval = TimeSpan.FromMinutes(1);
    private readonly string _hostExePath;
    private readonly PendingRequests _pending = new();
    private readonly ConcurrentDictionary<string, byte> _recycleOnCancel = new();
    private readonly ConcurrentDictionary<string, Task> _handleOpenSends = new();
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
    private string? _writableRoot;

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
        _handleOpenSends.Clear();
        DiagLog.Write("App", $"ParserHost starting gen={generation}; restart={generation > 1}");
        var generationReady =
            new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        _ready = generationReady;
        _channel?.Dispose();
        _server?.Dispose();
        TryKillHost();
        try { _host?.Dispose(); } catch { }
        _host = null;
        CleanupWritableRoot();
        string pipeName = $"quicklook_next_parser_{Environment.ProcessId}_{RandomNumberGenerator.GetHexString(16)}";
        _sessionToken = RandomNumberGenerator.GetHexString(32);
        _writableRoot = CreateWritableRoot();
        string writableRoot = _writableRoot;
        _server = HostProcessLauncher.CreateWriteRestrictedPipe(pipeName);
        var job = new HostProcessJob((nint)(512L * 1024 * 1024));
        try
        {
            _host = HostProcessLauncher.StartRestricted(
                _hostExePath,
                ["--pipe", pipeName, "--session-token", _sessionToken, "--writable-root", writableRoot],
                job,
                restrictWrites: true);
            _job = job;
        }
        catch
        {
            try { if (_host is { HasExited: false }) _host.Kill(entireProcessTree: true); } catch { }
            try { _host?.Dispose(); } catch { }
            _host = null;
            job.Dispose();
            CleanupWritableRoot();
            throw;
        }
        ProcessPowerMode.SetProcessBackgroundEfficiency(_host, _backgroundEfficiencyEnabled, "App");
        LogHostResources("started", generation, _host);
        _host.EnableRaisingEvents = true;
        _host.Exited += (_, _) => OnHostExited(generation, writableRoot);
        try
        {
            using var connectCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            connectCts.CancelAfter(HostConnectTimeout);
            await _server.WaitForConnectionAsync(connectCts.Token);
            if (!GetNamedPipeClientProcessId(_server.SafePipeHandle.DangerousGetHandle(), out uint clientPid) || clientPid != _host.Id)
                throw new InvalidOperationException("ParserHost pipe client did not match the launched process");
            DiagLog.Write("App", $"ParserHost pipe connected gen={generation}; pid={_host.Id}");
        }
        catch
        {
            TryKillHost();
            try { _server?.Dispose(); } catch { }
            _server = null;
            try { _host?.Dispose(); } catch { }
            _host = null;
            PreserveHostLog(writableRoot, generation);
            CleanupWritableRoot(writableRoot);
            throw;
        }

        try
        {
            _channel = new PipeChannel(_server);
            await _channel.SendAsync(new Hello(Environment.ProcessId, _sessionToken));
            _ = ReadLoopAsync(_channel, generation, generationReady);
            using var readyCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
            readyCts.CancelAfter(HostConnectTimeout);
            await generationReady.Task.WaitAsync(readyCts.Token);
            DiagLog.Write("App", $"ParserHost ready gen={generation}; pid={_host.Id}");
        }
        catch
        {
            TryKillHost();
            try { _channel?.Dispose(); } catch { }
            _channel = null;
            try { _server?.Dispose(); } catch { }
            _server = null;
            try { _host?.Dispose(); } catch { }
            _host = null;
            PreserveHostLog(writableRoot, generation);
            CleanupWritableRoot(writableRoot);
            throw;
        }
    }

    private bool IsConnected
    {
        get
        {
            try
            {
                return _channel is not null
                    && _host is { HasExited: false }
                    && _ready.Task.IsCompletedSuccessfully;
            }
            catch { return false; }
        }
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
        long maxLength = probe.Kind.Equals("archive", StringComparison.OrdinalIgnoreCase)
            ? NativeAbi.MaxArchiveHandleInputBytes
            : NativeAbi.MaxParserHandleInputBytes;
        if (sourceLength is < 0 || sourceLength > maxLength)
            throw new ArgumentOutOfRangeException(nameof(sourceLength));
        PipeChannel channel = _channel;
        Process host = _host;
        int generation = _generation;
        var (requestId, completion) = _pending.Begin(timeout ?? PreviewTimeout);
        _ = StopOnTimeoutAsync(completion, requestId);
        long hostHandle;
        try
        {
            hostHandle = WindowsHandleTransfer.DuplicateFileToProcess(sourceHandle, host.SafeHandle);
        }
        catch
        {
            _pending.Cancel(requestId);
            throw;
        }
        Task sendTask = SendOpenHandleAsync(
            channel, generation, requestId, hostHandle, sourceLength, logicalPath, probe);
        RegisterHandleOpenSend(requestId, sendTask);
        return (requestId, completion);
    }

    private async Task SendOpenHandleAsync(
        PipeChannel channel,
        int generation,
        string requestId,
        long sourceHandle,
        long sourceLength,
        string logicalPath,
        FileProbe probe)
    {
        try
        {
            await channel.SendAsync(new PreviewOpenHandle(
                requestId, sourceHandle, sourceLength, logicalPath, probe));
        }
        catch (Exception ex)
        {
            _pending.TryComplete(requestId, new PreviewError(requestId, ex.Message));
            if (generation == _generation)
                RecycleHost("handle preview request could not be delivered");
        }
    }

    public (string RequestId, Task<ControlMessage> Completion) BeginOpenSqliteHandles(
        string logicalPath,
        FileProbe probe,
        Microsoft.Win32.SafeHandles.SafeFileHandle mainHandle,
        long mainLength,
        Microsoft.Win32.SafeHandles.SafeFileHandle? walHandle,
        long walLength,
        Microsoft.Win32.SafeHandles.SafeFileHandle? shmHandle,
        long shmLength,
        TimeSpan? timeout = null)
    {
        if (_channel is null || _host is null) throw new InvalidOperationException("ParserHost not connected");
        if (mainLength is < 0 or > NativeAbi.MaxParserHandleInputBytes)
            throw new ArgumentOutOfRangeException(nameof(mainLength));
        if (walLength is < 0 or > NativeAbi.MaxSqliteWalBytes
            || walHandle is null && walLength != 0)
        {
            throw new ArgumentOutOfRangeException(nameof(walLength));
        }
        if (shmLength is < 0 or > NativeAbi.MaxSqliteShmBytes
            || shmHandle is null && shmLength != 0)
        {
            throw new ArgumentOutOfRangeException(nameof(shmLength));
        }

        PipeChannel channel = _channel;
        Process host = _host;
        int generation = _generation;
        var (requestId, completion) = _pending.Begin(timeout ?? PreviewTimeout);
        _ = StopOnTimeoutAsync(completion, requestId);
        long remoteMain = 0;
        long remoteWal = 0;
        long remoteShm = 0;
        int duplicatedCount = 0;
        try
        {
            remoteMain = WindowsHandleTransfer.DuplicateFileToProcess(mainHandle, host.SafeHandle);
            duplicatedCount++;
            if (walHandle is not null)
            {
                remoteWal = WindowsHandleTransfer.DuplicateFileToProcess(walHandle, host.SafeHandle);
                duplicatedCount++;
            }
            if (shmHandle is not null)
            {
                remoteShm = WindowsHandleTransfer.DuplicateFileToProcess(shmHandle, host.SafeHandle);
                duplicatedCount++;
            }
        }
        catch
        {
            _pending.Cancel(requestId);
            if (duplicatedCount > 0 && generation == _generation)
                RecycleHost("SQLite handle preview was only partially duplicated");
            throw;
        }

        Task sendTask = SendOpenSqliteHandlesAsync(
            channel,
            generation,
            new PreviewOpenSqliteHandles(
                requestId,
                remoteMain,
                mainLength,
                remoteWal,
                walLength,
                remoteShm,
                shmLength,
                logicalPath,
                probe));
        RegisterHandleOpenSend(requestId, sendTask);
        return (requestId, completion);
    }

    private async Task SendOpenSqliteHandlesAsync(
        PipeChannel channel,
        int generation,
        PreviewOpenSqliteHandles open)
    {
        try
        {
            await channel.SendAsync(open);
        }
        catch (Exception ex)
        {
            _pending.TryComplete(open.RequestId, new PreviewError(open.RequestId, ex.Message));
            if (generation == _generation)
                RecycleHost("SQLite handle preview request could not be delivered");
        }
    }

    private void RegisterHandleOpenSend(string requestId, Task sendTask)
    {
        _handleOpenSends[requestId] = sendTask;
        _ = TrackHandleOpenSendAsync(requestId, sendTask);
    }

    private async Task TrackHandleOpenSendAsync(string requestId, Task sendTask)
    {
        try
        {
            await sendTask;
        }
        catch
        {
            // Send methods convert delivery failures to terminal responses and recycle the host.
        }
        finally
        {
            if (_handleOpenSends.TryGetValue(requestId, out Task? current)
                && ReferenceEquals(current, sendTask))
            {
                _handleOpenSends.TryRemove(requestId, out _);
            }
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
        PipeChannel? channel = _channel;
        int generation = _generation;
        try
        {
            if (_handleOpenSends.TryGetValue(requestId, out Task? openSend))
            {
                try { await openSend; }
                catch { }
            }
            if (channel is not null && generation == _generation)
                await channel.SendAsync(new PreviewClose(requestId));
        }
        finally
        {
            if (recycleHost && generation == _generation)
            {
                DiagLog.Write("App", $"recycling ParserHost after cloud preview cancellation: request={requestId}");
                RecycleHost("cloud preview canceled while opening");
            }
        }
    }

    public async Task<ArchiveEntryHandoff?> ExtractArchiveEntryAsync(
        string archivePath,
        string entryPath,
        string? parentPreviewRequestId,
        CancellationToken cancellationToken)
    {
        PipeChannel channel = _channel ?? throw new InvalidOperationException("ParserHost not connected");
        Process host = _host ?? throw new InvalidOperationException("ParserHost process is unavailable");
        int generation = _generation;
        var (requestId, completion) = _pending.Begin(PreviewTimeout);
        _ = StopOnTimeoutAsync(completion, requestId);
        ArchiveEntryHandoff? output = null;
        bool delivered = false;
        try
        {
            output = CreateArchiveEntryOutput(requestId, entryPath);
            long remoteOutputHandle;
            try
            {
                remoteOutputHandle = WindowsHandleTransfer.DuplicateFileToProcess(
                    output.OutputHandle,
                    host.SafeHandle);
            }
            catch
            {
                _pending.Cancel(requestId);
                throw;
            }

            try
            {
                await channel.SendAsync(new ArchiveEntryExtract(
                    requestId,
                    archivePath,
                    entryPath,
                    remoteOutputHandle,
                    NativeAbi.MaxArchiveEntryOutputBytes)
                {
                    ParentPreviewRequestId = parentPreviewRequestId,
                }, cancellationToken);
                delivered = true;
            }
            catch
            {
                // A duplicated remote HANDLE cannot be rolled back reliably after a failed send.
                // Process teardown closes it and prevents a later message from reusing the value.
                if (generation == _generation)
                    RecycleHost("archive output HANDLE request could not be delivered");
                throw;
            }

            ControlMessage response = await completion.WaitAsync(cancellationToken);
            if (response is ArchiveEntryExtracted extracted
                && generation == _generation
                && string.Equals(extracted.LogicalName, entryPath, StringComparison.Ordinal)
                && output.SealReadOnly(extracted.FileLength))
            {
                ArchiveEntryHandoff handoff = output;
                output = null;
                return handoff;
            }
            return null;
        }
        finally
        {
            _pending.Cancel(requestId);
            if (output is not null)
            {
                if (delivered && generation == _generation)
                {
                    try { await channel.SendAsync(new ArchiveEntryExtractClose(requestId)); }
                    catch (Exception ex) when (ex is IOException or ObjectDisposedException or InvalidOperationException) { }
                }
                output.Dispose();
            }
        }
    }

    public Task ReleaseArchiveEntryAsync(ArchiveEntryHandoff handoff)
    {
        handoff.Dispose();
        return Task.CompletedTask;
    }

    private static ArchiveEntryHandoff CreateArchiveEntryOutput(string requestId, string entryPath)
    {
        string requestDirectory = Path.Combine(
            Path.GetTempPath(),
            "QuickLookNext",
            "app-preview",
            requestId);
        string extension = Path.GetExtension(entryPath);
        if (extension.Length > 32
            || extension.Any(static c => !char.IsAsciiLetterOrDigit(c) && c != '.'))
        {
            extension = "";
        }
        string path = Path.Combine(requestDirectory, "entry" + extension.ToLowerInvariant());
        try
        {
            string root = Path.GetDirectoryName(requestDirectory)!;
            Directory.CreateDirectory(root);
            if ((File.GetAttributes(root) & FileAttributes.ReparsePoint) != 0)
                throw new IOException("Archive output root cannot be a reparse point.");
            Directory.CreateDirectory(requestDirectory);
            if ((File.GetAttributes(requestDirectory) & FileAttributes.ReparsePoint) != 0)
                throw new IOException("Archive output directory cannot be a reparse point.");
            var writer = new FileStream(
                path,
                FileMode.CreateNew,
                FileAccess.ReadWrite,
                FileShare.ReadWrite | FileShare.Delete);
            return new ArchiveEntryHandoff(requestId, path, writer);
        }
        catch
        {
            try { Directory.Delete(requestDirectory, recursive: true); } catch { }
            throw;
        }
    }

    public async Task<NativeRasterImage?> ExtractHeroRasterAsync(
        string path, string kind, string? parentPreviewRequestId, CancellationToken cancellationToken)
    {
        PipeChannel channel = _channel ?? throw new InvalidOperationException("ParserHost not connected");
        Process sourceHost = _host ?? throw new InvalidOperationException("ParserHost process is unavailable");
        int sourceGeneration = _generation;
        var (requestId, completion) = _pending.Begin(PreviewTimeout);
        _ = StopOnTimeoutAsync(completion, requestId);
        try
        {
            await channel.SendAsync(new HeroRasterExtract(requestId, path, kind)
            {
                ParentPreviewRequestId = parentPreviewRequestId,
            }, cancellationToken);
            ControlMessage response = await completion.WaitAsync(cancellationToken);
            return response is HeroRasterExtracted extracted && sourceGeneration == _generation
                ? ReadHeroRaster(extracted, sourceHost)
                : null;
        }
        finally
        {
            _pending.Cancel(requestId);
            try { await channel.SendAsync(new HeroRasterExtractClose(requestId)); }
            catch (Exception ex) when (ex is IOException or ObjectDisposedException or InvalidOperationException) { }
        }
    }

    public async Task<NativeRasterImage?> ExtractOfficeImageAsync(
        string parentPreviewRequestId,
        string imageRef,
        int targetWidth,
        int targetHeight,
        CancellationToken cancellationToken)
    {
        if (!IsValidRequestId(parentPreviewRequestId))
            throw new ArgumentException("A valid parent preview request ID is required.", nameof(parentPreviewRequestId));
        if (!IsCanonicalOfficeImageRef(imageRef)
            || Encoding.UTF8.GetByteCount(imageRef) > NativeAbi.MaxOfficeImageRefUtf8Bytes)
        {
            throw new ArgumentException("A canonical Office image reference is required.", nameof(imageRef));
        }
        if (targetWidth is <= 0 or > NativeAbi.MaxOfficeImageDimension)
            throw new ArgumentOutOfRangeException(nameof(targetWidth));
        if (targetHeight is <= 0 or > NativeAbi.MaxOfficeImageDimension)
            throw new ArgumentOutOfRangeException(nameof(targetHeight));

        PipeChannel channel = _channel ?? throw new InvalidOperationException("ParserHost not connected");
        Process sourceHost = _host ?? throw new InvalidOperationException("ParserHost process is unavailable");
        int sourceGeneration = _generation;
        var (requestId, completion) = _pending.Begin(PreviewTimeout);
        _ = StopOnTimeoutAsync(completion, requestId);
        try
        {
            await channel.SendAsync(new OfficeImageOpen(
                requestId,
                parentPreviewRequestId,
                imageRef,
                checked((uint)targetWidth),
                checked((uint)targetHeight)), cancellationToken);
            ControlMessage response = await completion.WaitAsync(cancellationToken);
            return response is OfficeImageReady ready
                && sourceGeneration == _generation
                && ready.Width <= targetWidth
                && ready.Height <= targetHeight
                ? ReadOfficeImageRaster(ready, sourceHost)
                : null;
        }
        finally
        {
            _pending.Cancel(requestId);
            try { await channel.SendAsync(new OfficeImageClose(requestId)); }
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

    private async Task ReadLoopAsync(
        PipeChannel channel,
        int generation,
        TaskCompletionSource generationReady)
    {
        try
        {
            while (generation == _generation)
            {
                ControlMessage? message = await channel.ReceiveAsync();
                if (generation != _generation)
                    return;
                if (message is null)
                    throw new EndOfStreamException("ParserHost pipe closed");
                switch (message)
                {
                    case ParserReady:
                        DiagLog.Write("App", "ParserHost ready");
                        generationReady.TrySetResult();
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
                    case OfficeImageReady ready:
                        _pending.TryComplete(ready.RequestId, ready);
                        break;
                }
            }
        }
        catch (Exception ex)
        {
            if (generation != _generation)
                return;
            generationReady.TrySetException(ex);
            _recycleOnCancel.Clear();
            _handleOpenSends.Clear();
            _pending.FailAll(ex);
        }
    }

    public void SetBackgroundEfficiency(bool enabled)
    {
        _backgroundEfficiencyEnabled = enabled;
        ProcessPowerMode.SetProcessBackgroundEfficiency(_host, enabled, "App");
    }

    private void OnHostExited(int generation, string writableRoot)
    {
        try
        {
            if (_stopping || generation != _generation)
                return;

            int? exitCode = null;
            try { exitCode = _host?.ExitCode; } catch { }
            DiagLog.Write("App", $"ParserHost exited gen={generation}; pid={_host?.Id}; exitCode={exitCode?.ToString() ?? "unknown"}; timeouts={Volatile.Read(ref _timeoutCount)}");
            _recycleOnCancel.Clear();
            _handleOpenSends.Clear();
            _pending.FailAll(new InvalidOperationException("ParserHost exited"));
        }
        finally
        {
            PreserveHostLog(writableRoot, generation);
            CleanupWritableRoot(writableRoot);
        }
    }

    public void Stop()
    {
        _stopping = true;
        try { _telemetryCts.Cancel(); } catch { }
        ++_generation;
        _recycleOnCancel.Clear();
        _handleOpenSends.Clear();
        _pending.FailAll(new OperationCanceledException("ParserHost stopped"));
        _ready.TrySetCanceled();
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        TryKillHost();
        try { _host?.Dispose(); } catch { }
        _host = null;
        CleanupWritableRoot();
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
        try { _host?.WaitForExit(1000); } catch { }
    }

    private void RecycleHost(string reason)
    {
        DiagLog.Write("App", $"ParserHost recycle: reason={reason}; gen={_generation}");
        ++_generation;
        _recycleOnCancel.Clear();
        _handleOpenSends.Clear();
        _pending.FailAll(new OperationCanceledException(reason));
        _ready.TrySetCanceled();
        try { _channel?.Dispose(); } catch { }
        _channel = null;
        try { _server?.Dispose(); } catch { }
        _server = null;
        TryKillHost();
        CleanupWritableRoot();
    }

    private static string CreateWritableRoot()
    {
        string root = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "QuickLookNext", "ParserHost", "work", "host-" + RandomNumberGenerator.GetHexString(16).ToLowerInvariant());
        try
        {
            Directory.CreateDirectory(root);
            HostProcessLauncher.GrantRestrictedWriteAccess(root);
            foreach (string child in new[] { "logs" })
                Directory.CreateDirectory(Path.Combine(root, child));
            return root;
        }
        catch
        {
            try { Directory.Delete(root, recursive: true); } catch { }
            throw;
        }
    }

    private void CleanupWritableRoot(string? expectedRoot = null)
    {
        string? root;
        if (expectedRoot is null)
        {
            root = Interlocked.Exchange(ref _writableRoot, null);
        }
        else
        {
            Interlocked.CompareExchange(ref _writableRoot, null, expectedRoot);
            root = expectedRoot;
        }
        if (root is null) return;
        try
        {
            if (Directory.Exists(root) && (File.GetAttributes(root) & FileAttributes.ReparsePoint) == 0)
                Directory.Delete(root, recursive: true);
        }
        catch { }
    }

    private static void PreserveHostLog(string writableRoot, int generation)
    {
        try
        {
            string source = Path.Combine(writableRoot, "logs", "parser-host.log");
            if (!File.Exists(source))
                return;
            Directory.CreateDirectory(DiagLog.LogDirectory);
            File.Copy(source, Path.Combine(DiagLog.LogDirectory, $"parser-host-{generation}.log"), overwrite: true);
        }
        catch { }
    }

    private static NativeRasterImage? ReadHeroRaster(
        HeroRasterExtracted extracted,
        Process sourceHost)
        => ReadRasterSection(
            extracted.SectionHandle,
            extracted.PacketLength,
            extracted.Width,
            extracted.Height,
            sourceHost,
            maxRasterBytes: 16 * 1024 * 1024,
            maxDimension: 4096);

    private static NativeRasterImage? ReadOfficeImageRaster(
        OfficeImageReady ready,
        Process sourceHost)
        => ReadRasterSection(
            ready.SectionHandle,
            ready.PacketLength,
            ready.Width,
            ready.Height,
            sourceHost,
            NativeAbi.MaxOfficeImagePacketBytes,
            NativeAbi.MaxOfficeImageDimension);

    private static NativeRasterImage? ReadRasterSection(
        long sectionHandle,
        long packetLength,
        int reportedWidth,
        int reportedHeight,
        Process sourceHost,
        long maxRasterBytes,
        int maxDimension)
    {
        try
        {
            if (sourceHost.HasExited
                || packetLength is <= 8
                || packetLength > maxRasterBytes
                || packetLength > int.MaxValue
                || reportedWidth is <= 0
                || reportedWidth > maxDimension
                || reportedHeight is <= 0
                || reportedHeight > maxDimension)
                return null;

            using SharedSectionView view = SharedSectionView.DuplicateAndMapReadOnly(
                sourceHost.SafeHandle,
                sectionHandle,
                checked((int)packetLength));
            ReadOnlySpan<byte> raster = view.Bytes;
            int width = BitConverter.ToInt32(raster[..4]);
            int height = BitConverter.ToInt32(raster[4..8]);
            int pixelBytes = checked(width * height * 4);
            if (width <= 0
                || width > maxDimension
                || height <= 0
                || height > maxDimension
                || reportedWidth != width
                || reportedHeight != height
                || raster.Length != 8 + pixelBytes)
                return null;

            byte[] bgra = raster[8..].ToArray();
            return new NativeRasterImage(bgra, width, height);
        }
        catch (Exception ex) when (ex is ArgumentException
            or System.ComponentModel.Win32Exception
            or InvalidOperationException
            or IOException
            or UnauthorizedAccessException
            or NotSupportedException
            or OverflowException)
        {
            return null;
        }
    }

    private static bool IsValidRequestId(string? requestId)
        => requestId is { Length: 32 }
            && requestId.All(static c => char.IsAsciiHexDigit(c));

    private static bool IsCanonicalOfficeImageRef(string? imageRef)
    {
        if (string.IsNullOrWhiteSpace(imageRef)
            || imageRef.Length > NativeAbi.MaxOfficeImageRefUtf8Bytes
            || imageRef[0] == '/'
            || imageRef.Contains('\\')
            || imageRef.Contains(':'))
        {
            return false;
        }

        string[] segments = imageRef.Split('/');
        return segments.Length >= 3
            && segments[0] is "word" or "ppt" or "xl"
            && segments[1] == "media"
            && segments.All(static segment =>
                segment.Length > 0
                && segment is not "." and not "..");
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
    public Microsoft.Win32.SafeHandles.SafeFileHandle OutputHandle
        => _anchor?.SafeFileHandle
            ?? throw new ObjectDisposedException(nameof(ArchiveEntryHandoff));

    public bool SealReadOnly(long expectedLength)
    {
        if (expectedLength is < 0 or > NativeAbi.MaxArchiveEntryOutputBytes)
            return false;
        FileStream? writer = _anchor;
        if (writer is null || !writer.CanWrite)
            return false;

        Microsoft.Win32.SafeHandles.SafeFileHandle? transitional = null;
        Microsoft.Win32.SafeHandles.SafeFileHandle? readOnly = null;
        try
        {
            if (writer.Length != expectedLength)
                return false;
            transitional = WindowsHandleTransfer.ReopenTransitionalReadOnlyFile(
                writer.SafeFileHandle,
                expectedLength);
            writer.Dispose();
            _anchor = null;
            readOnly = WindowsHandleTransfer.ReopenReadOnlyFile(transitional, expectedLength);
            transitional.Dispose();
            transitional = null;
            _anchor = new FileStream(readOnly, FileAccess.Read);
            readOnly = null;
            return true;
        }
        catch (Exception ex) when (ex is ArgumentException
                                   or System.ComponentModel.Win32Exception
                                   or IOException
                                   or ObjectDisposedException
                                   or UnauthorizedAccessException
                                   or NotSupportedException
                                   or OverflowException)
        {
            return false;
        }
        finally
        {
            transitional?.Dispose();
            readOnly?.Dispose();
        }
    }

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
