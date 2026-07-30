using System.IO.Pipes;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;
using QuickLook.Next.ShellBroker;

string pipeName = GetArg(args, "--pipe") ?? "";
string sessionToken = GetArg(args, "--session-token") ?? "";
string writableRoot = GetArg(args, "--writable-root") ?? "";
try
{
    if (string.IsNullOrWhiteSpace(pipeName)
        || string.IsNullOrWhiteSpace(sessionToken)
        || !Path.IsPathFullyQualified(writableRoot)
        || !Directory.Exists(writableRoot)
        || (File.GetAttributes(writableRoot) & FileAttributes.ReparsePoint) != 0)
        return;
}
catch (Exception ex)
{
    try { File.WriteAllText(Path.Combine(writableRoot, "startup-failure.txt"), ex.ToString()); } catch { }
    return;
}
using var pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut, PipeOptions.None);
try { pipe.Connect(5000); }
catch (Exception ex)
{
    try { File.WriteAllText(Path.Combine(writableRoot, "startup-failure.txt"), ex.ToString()); } catch { }
    return;
}
using var channel = new BrokerChannel(pipe);
var requestLock = new object();
ShellRequest? activeRequest = null;
bool authenticated = false;

try
{
    while (channel.Receive() is { } message)
    {
        string[] parts = message.Split('\t');
        switch (parts)
        {
            case ["HELLO", var processIdText, var receivedToken] when !authenticated:
                if (!string.Equals(receivedToken, sessionToken, StringComparison.Ordinal)
                    || !int.TryParse(processIdText, out int appProcessId))
                    throw new InvalidDataException("ShellBroker authentication failed.");
                WindowsHandleTransfer.VerifyNamedPipeServerProcess(pipe.SafePipeHandle, appProcessId);
                authenticated = true;
                channel.Send("READY");
                break;
            case var _ when !authenticated:
            case ["HELLO", ..]:
                throw new InvalidDataException("ShellBroker received an invalid authentication message.");
            case ["OPEN", var requestId, var sizeText, var encodedPath]
                when IsValidRequestId(requestId)
                     && int.TryParse(sizeText, out int size)
                     && size is >= 16 and <= 512:
                string path;
                try { path = System.Text.Encoding.UTF8.GetString(Convert.FromBase64String(encodedPath)); }
                catch { throw new InvalidDataException("ShellBroker path payload is invalid."); }
                if (!Path.IsPathFullyQualified(path) || path.Length > 32767)
                    throw new InvalidDataException("ShellBroker path must be absolute and bounded.");
                var request = new ShellRequest(requestId);
                lock (requestLock)
                {
                    if (activeRequest is not null)
                    {
                        request.Dispose();
                        channel.SendError(requestId, "ShellBroker already has an active request.");
                        break;
                    }
                    activeRequest = request;
                }
                await request.HandoffGate.WaitAsync();
                string? packetPath = null;
                try
                {
                    byte[]? packet = NativeThumbnail.TryGetPacket(path, size, request.Cancellation.Token);
                    if (packet is null)
                    {
                        channel.SendError(requestId, "Shell thumbnail provider returned no image.");
                        request.HandoffGate.Release();
                        break;
                    }
                    string directory = Path.Combine(writableRoot, "thumbnail-" + requestId);
                    Directory.CreateDirectory(directory);
                    packetPath = Path.Combine(directory, "thumbnail.bgra");
                    File.WriteAllBytes(packetPath, packet);
                    var transferred = WindowsHandleTransfer.OpenReadOnlyFile(packetPath);
                    request.PacketPath = packetPath;
                    request.PacketHandle = transferred.Handle;
                    packetPath = null;
                    channel.Send($"THUMB\t{requestId}\t{transferred.Handle.DangerousGetHandle().ToInt64()}\t{transferred.Length}\t{BitConverter.ToInt32(packet, 0)}\t{BitConverter.ToInt32(packet, 4)}");
                }
                catch (Exception ex)
                {
                    try { channel.SendError(requestId, ex.Message); } catch { }
                }
                finally
                {
                    if (packetPath is not null) DeletePacket(packetPath);
                    if (request.HandoffGate.CurrentCount == 0) request.HandoffGate.Release();
                }
                break;
            case ["CLOSE", var requestId] when IsValidRequestId(requestId):
                ShellRequest? closing;
                lock (requestLock)
                    closing = activeRequest is not null
                              && string.Equals(activeRequest.RequestId, requestId, StringComparison.Ordinal)
                        ? activeRequest
                        : null;
                if (closing is null) break;
                closing.Cancellation.Cancel();
                await closing.HandoffGate.WaitAsync();
                try
                {
                    if (closing.PacketHandle is not null)
                    {
                        closing.PacketHandle.Dispose();
                        if (closing.PacketPath is not null) DeletePacket(closing.PacketPath);
                    }
                }
                finally
                {
                    lock (requestLock)
                        if (ReferenceEquals(activeRequest, closing)) activeRequest = null;
                    closing.HandoffGate.Release();
                    closing.Dispose();
                }
                break;
            default:
                throw new InvalidDataException("ShellBroker received an invalid control message.");
        }
    }
}
catch (Exception ex)
{
    try { File.WriteAllText(Path.Combine(writableRoot, "startup-failure.txt"), ex.ToString()); } catch { }
}

if (activeRequest is not null)
{
    activeRequest.Cancellation.Cancel();
    activeRequest.PacketHandle?.Dispose();
    if (activeRequest.PacketPath is not null) DeletePacket(activeRequest.PacketPath);
    activeRequest.Dispose();
}

static bool IsValidRequestId(string? requestId)
    => requestId is { Length: 32 } && requestId.All(static c => char.IsAsciiHexDigit(c));

static string? GetArg(string[] values, string key)
{
    for (int i = 0; i < values.Length - 1; i++)
        if (values[i] == key) return values[i + 1];
    return null;
}

static void DeletePacket(string path)
{
    try
    {
        File.Delete(path);
        string? directory = Path.GetDirectoryName(path);
        if (directory is not null) Directory.Delete(directory, recursive: false);
    }
    catch { }
}

sealed class BrokerChannel(Stream stream) : IDisposable
{
    private readonly StreamReader _reader = new(
        stream, System.Text.Encoding.UTF8, detectEncodingFromByteOrderMarks: false, leaveOpen: true);
    private readonly StreamWriter _writer = new(
        stream, new System.Text.UTF8Encoding(false), leaveOpen: true)
    { AutoFlush = true };
    private readonly object _writeLock = new();

    public string? Receive()
    {
        var line = new System.Text.StringBuilder();
        while (true)
        {
            int value = _reader.Read();
            if (value < 0)
                return line.Length == 0 ? null : line.ToString();
            if (value == '\n')
            {
                if (line.Length > 0 && line[^1] == '\r') line.Length--;
                return line.ToString();
            }
            if (line.Length >= PipeChannel.MaxControlLineChars)
                throw new InvalidDataException("ShellBroker control message is too large.");
            line.Append((char)value);
        }
    }

    public void Send(string line)
    {
        if (line.Length > PipeChannel.MaxControlLineChars)
            throw new InvalidDataException("ShellBroker control message is too large.");
        lock (_writeLock) _writer.WriteLine(line);
    }

    public void SendError(string requestId, string message)
        => Send($"ERROR\t{requestId}\t{Convert.ToBase64String(System.Text.Encoding.UTF8.GetBytes(message))}");

    public void Dispose()
    {
        try { _reader.Dispose(); } catch { }
        try { _writer.Dispose(); } catch { }
    }
}

sealed class ShellRequest(string requestId) : IDisposable
{
    public string RequestId { get; } = requestId;
    public CancellationTokenSource Cancellation { get; } = new();
    public SemaphoreSlim HandoffGate { get; } = new(1, 1);
    public string? PacketPath { get; set; }
    public SafeFileHandle? PacketHandle { get; set; }

    public void Dispose()
    {
        PacketHandle?.Dispose();
        Cancellation.Dispose();
        HandoffGate.Dispose();
    }
}
