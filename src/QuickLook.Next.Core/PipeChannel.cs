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
    private const int MaxControlLineChars = 4 * 1024 * 1024;

    private readonly StreamReader _reader;
    private readonly StreamWriter _writer;
    private readonly SemaphoreSlim _writeLock = new(1, 1);

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
        var one = new char[1];
        while (true)
        {
            int read = await _reader.ReadAsync(one.AsMemory(0, 1), ct).ConfigureAwait(false);
            if (read == 0)
                return line.Length == 0 ? null : line.ToString();

            char c = one[0];
            if (c == '\n')
            {
                if (line.Length > 0 && line[^1] == '\r')
                    line.Length--;
                return line.ToString();
            }

            if (line.Length >= MaxControlLineChars)
                throw new InvalidDataException($"control message is too large (>{MaxControlLineChars:N0} chars)");
            line.Append(c);
        }
    }

    public void Dispose()
    {
        _reader.Dispose();
        _writer.Dispose();
        _writeLock.Dispose();
    }
}
