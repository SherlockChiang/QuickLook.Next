using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization.Metadata;

namespace QuickLook.Next.Core;

/// <summary>JSON (de)serialization for the control channel. Both peers must share these options.</summary>
public static class ProtocolJson
{
    public static readonly JsonSerializerOptions Options = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
        TypeInfoResolver = new DefaultJsonTypeInfoResolver()
    };

    static ProtocolJson() => Options.MakeReadOnly();

    public static string Serialize(ControlMessage message) => JsonSerializer.Serialize(message, Options);

    public static ControlMessage Deserialize(string json) =>
        JsonSerializer.Deserialize<ControlMessage>(json, Options)
        ?? throw new InvalidDataException("null control message");
}

/// <summary>
/// Line-delimited JSON control channel over a duplex stream (the named pipe). Compact JSON contains no
/// raw newlines, so '\n' framing is safe. Writes are serialized; one reader/one writer per channel.
/// </summary>
public sealed class PipeChannel : IDisposable
{
    public const int MaxControlLineChars = 4 * 1024 * 1024;

    private readonly StreamReader _reader;
    private readonly StreamWriter _writer;
    private readonly SemaphoreSlim _writeLock = new(1, 1);
    private readonly char[] _readBuffer = new char[8192];
    private int _readOffset;
    private int _readCount;

    public PipeChannel(Stream stream)
    {
        _reader = new StreamReader(stream, Encoding.UTF8, detectEncodingFromByteOrderMarks: false);
        _writer = new StreamWriter(stream, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false)) { AutoFlush = true };
    }

    public async Task SendAsync(ControlMessage message, CancellationToken ct = default)
    {
        string line = ProtocolJson.Serialize(message);
        if (line.Length > MaxControlLineChars)
            throw new InvalidDataException($"control message is too large ({line.Length:N0} chars)");

        await _writeLock.WaitAsync(ct).ConfigureAwait(false);
        try { await _writer.WriteLineAsync(line.AsMemory(), ct).ConfigureAwait(false); }
        finally { _writeLock.Release(); }
    }

    public async Task<ControlMessage?> ReceiveAsync(CancellationToken ct = default)
    {
        string? line = await ReadBoundedLineAsync(ct).ConfigureAwait(false);
        return line is null ? null : ProtocolJson.Deserialize(line);
    }

    private async Task<string?> ReadBoundedLineAsync(CancellationToken ct)
    {
        var line = new StringBuilder();
        while (true)
        {
            if (_readOffset == _readCount)
            {
                _readCount = await _reader.ReadAsync(_readBuffer.AsMemory(), ct).ConfigureAwait(false);
                _readOffset = 0;
                if (_readCount == 0)
                    return line.Length == 0 ? null : line.ToString();
            }

            int newline = Array.IndexOf(_readBuffer, '\n', _readOffset, _readCount - _readOffset);
            int end = newline >= 0 ? newline : _readCount;
            int length = end - _readOffset;
            if (line.Length > MaxControlLineChars - length)
                throw new InvalidDataException($"control message is too large (>{MaxControlLineChars:N0} chars)");
            line.Append(_readBuffer, _readOffset, length);
            _readOffset = newline >= 0 ? newline + 1 : end;

            if (newline >= 0)
            {
                if (line.Length > 0 && line[^1] == '\r')
                    line.Length--;
                return line.ToString();
            }
        }
    }

    public void Dispose()
    {
        SafeDispose(_reader);
        SafeDispose(_writer);
        _writeLock.Dispose();
    }

    private static void SafeDispose(IDisposable disposable)
    {
        try { disposable.Dispose(); }
        catch (IOException) { }
        catch (ObjectDisposedException) { }
    }
}
