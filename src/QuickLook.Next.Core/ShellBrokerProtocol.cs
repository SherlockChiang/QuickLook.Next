using System.Buffers.Binary;
using System.Globalization;
using System.Text;
using QuickLook.Next.Contracts;

namespace QuickLook.Next.Core;

public static class ShellBrokerProtocol
{
    public const int MaxDimension = 512;
    public const int MaxErrorUtf8Bytes = 4096;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    public static ControlMessage Parse(string line)
    {
        string[] parts = line.Split('\t');
        if (parts is ["READY"])
            return new ShellBrokerReady();

        if (parts is ["THUMB", var requestId, var handleText, var lengthText, var widthText, var heightText]
            && IsValidRequestId(requestId)
            && long.TryParse(handleText, NumberStyles.None, CultureInfo.InvariantCulture, out long handle)
            && long.TryParse(lengthText, NumberStyles.None, CultureInfo.InvariantCulture, out long length)
            && int.TryParse(widthText, NumberStyles.None, CultureInfo.InvariantCulture, out int width)
            && int.TryParse(heightText, NumberStyles.None, CultureInfo.InvariantCulture, out int height))
        {
            var ready = new ShellThumbnailReady(requestId, handle, length, width, height);
            if (handle <= 0 || !TryGetPixelByteCount(ready, out _))
                throw new InvalidDataException("ShellBroker returned invalid thumbnail metadata.");
            return ready;
        }

        if (parts is ["ERROR", var errorRequestId, var payload] && IsValidRequestId(errorRequestId))
        {
            if (payload.Length > ((MaxErrorUtf8Bytes + 2) / 3) * 4)
                throw new InvalidDataException("ShellBroker returned an oversized error payload.");
            byte[] bytes;
            try { bytes = Convert.FromBase64String(payload); }
            catch (FormatException ex)
            {
                throw new InvalidDataException("ShellBroker returned an invalid error payload.", ex);
            }
            if (bytes.Length > MaxErrorUtf8Bytes)
                throw new InvalidDataException("ShellBroker returned an oversized error payload.");
            try { return new PreviewError(errorRequestId, StrictUtf8.GetString(bytes)); }
            catch (DecoderFallbackException ex)
            {
                throw new InvalidDataException("ShellBroker returned a non-UTF-8 error payload.", ex);
            }
        }

        throw new InvalidDataException("ShellBroker returned an invalid control message.");
    }

    public static bool TryGetPixelByteCount(ShellThumbnailReady ready, out int pixelBytes)
    {
        pixelBytes = 0;
        if (ready.Width is <= 0 or > MaxDimension || ready.Height is <= 0 or > MaxDimension)
            return false;
        long bytes = (long)ready.Width * ready.Height * 4;
        if (ready.PacketLength != 8 + bytes || bytes > int.MaxValue)
            return false;
        pixelBytes = (int)bytes;
        return true;
    }

    public static bool HeaderMatches(ShellThumbnailReady ready, ReadOnlySpan<byte> header)
        => header.Length >= 8
            && BinaryPrimitives.ReadInt32LittleEndian(header) == ready.Width
            && BinaryPrimitives.ReadInt32LittleEndian(header[4..]) == ready.Height;

    private static bool IsValidRequestId(string requestId)
        => requestId.Length == 32 && requestId.All(char.IsAsciiHexDigit);
}
