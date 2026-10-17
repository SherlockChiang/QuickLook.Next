using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Contracts;
using QuickLook.Next.Core;
using Windows.Graphics.Imaging;

namespace QuickLook.Next.RasterHost;

/// <summary>
/// Windows-codec metadata supplement bound to the retained source HANDLE.
/// The logical name is only a display hint; the decoder always reads the independently reopened
/// file object and never resolves or opens a path.
/// </summary>
internal static class SystemImageMetadataReader
{
    private const long MaxInputImageBytes = 512L * 1024 * 1024;
    private const int MaxIsoBmffProbeBytes = 8 * 1024 * 1024;
    private const int MaxIsoBmffDepth = 8;
    private const int MaxIsoBmffBoxes = 4096;
    private const int SystemMetadataTimeoutExitCode = 33;
    private static readonly TimeSpan DrainGrace = TimeSpan.FromMilliseconds(250);
    private static readonly SemaphoreSlim MetadataGate = new(1, 1);

    public static async Task<ImageMetadata?> TryReadHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        TimeSpan timeout,
        CancellationToken cancellationToken)
    {
        if (sourceLength is < 0 or > MaxInputImageBytes
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed
            || timeout <= TimeSpan.Zero)
        {
            return null;
        }

        using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
        timeoutCts.CancelAfter(timeout);
        bool enteredGate = false;
        Task<ImageMetadata?>? worker = null;
        try
        {
            await MetadataGate.WaitAsync(timeoutCts.Token);
            enteredGate = true;
            worker = Task.Run(
                () => ReadHandleAsync(
                    sourceHandle,
                    sourceLength,
                    logicalName,
                    timeoutCts.Token),
                CancellationToken.None);
            return await worker.WaitAsync(timeoutCts.Token);
        }
        catch (OperationCanceledException) when (
            !cancellationToken.IsCancellationRequested
            && timeoutCts.IsCancellationRequested)
        {
            if (worker is not null)
                await DrainWorkerOrExitAsync(worker);
            DiagLog.Write(
                "RasterHost",
                $"system HANDLE image metadata timed out; timeout={timeout.TotalMilliseconds:0}ms");
            return null;
        }
        catch (OperationCanceledException)
        {
            if (worker is not null)
                await DrainWorkerOrExitAsync(worker);
            throw;
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", $"system HANDLE image metadata failed: {ex.Message}");
            return null;
        }
        finally
        {
            if (enteredGate)
                MetadataGate.Release();
        }
    }

    public static ImageMetadata? Merge(ImageMetadata? native, ImageMetadata? system)
    {
        if (native is null)
            return system;
        if (system is null)
            return native;

        return native with
        {
            Format = FirstText(native.Format, system.Format),
            Width = Positive(native.Width) ?? Positive(system.Width),
            Height = Positive(native.Height) ?? Positive(system.Height),
            HorizontalResolution = PositiveFinite(native.HorizontalResolution)
                ?? PositiveFinite(system.HorizontalResolution),
            VerticalResolution = PositiveFinite(native.VerticalResolution)
                ?? PositiveFinite(system.VerticalResolution),
            PhotometricInterpretation = FirstText(
                native.PhotometricInterpretation,
                system.PhotometricInterpretation),
            BitDepth = native.BitDepth is > 0 ? native.BitDepth : system.BitDepth,
            HdrTransferFunction = FirstText(native.HdrTransferFunction, system.HdrTransferFunction),
            HdrColorPrimaries = FirstText(native.HdrColorPrimaries, system.HdrColorPrimaries),
            HdrMetadataComplete = native.HdrMetadataComplete ?? system.HdrMetadataComplete,
            ColorType = FirstText(native.ColorType, system.ColorType),
            HasAlpha = native.HasAlpha ?? system.HasAlpha,
            Animated = native.Animated ?? system.Animated,
            FrameCount = Positive(native.FrameCount) ?? Positive(system.FrameCount),
        };
    }

    private static async Task<ImageMetadata?> ReadHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        using SafeFileHandle metadataHandle =
            WindowsHandleTransfer.ReopenReadOnlyFile(sourceHandle, sourceLength);
        using var fileStream =
            new FileStream(metadataHandle, FileAccess.Read, 64 * 1024, isAsync: false);
        if (fileStream.Length != sourceLength)
            return null;

        IsoBmffHdrMetadata? containerHdr =
            TryReadIsoBmffHdrMetadata(fileStream, sourceLength, cancellationToken);
        fileStream.Position = 0;

        using var stream = fileStream.AsRandomAccessStream();
        BitmapDecoder decoder = await BitmapDecoder
            .CreateAsync(stream)
            .AsTask(cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();

        uint width = decoder.OrientedPixelWidth > 0
            ? decoder.OrientedPixelWidth
            : decoder.PixelWidth;
        uint height = decoder.OrientedPixelHeight > 0
            ? decoder.OrientedPixelHeight
            : decoder.PixelHeight;
        if (width == 0 || height == 0)
            return null;

        BitmapPixelFormat pixelFormat = decoder.BitmapPixelFormat;
        BitmapAlphaMode alphaMode = decoder.BitmapAlphaMode;
        uint frameCount = decoder.FrameCount;
        return new ImageMetadata
        {
            Format = ResolveFormat(decoder.DecoderInformation.FileExtensions, logicalName),
            Width = width,
            Height = height,
            HorizontalResolution = PositiveFinite(decoder.DpiX),
            VerticalResolution = PositiveFinite(decoder.DpiY),
            PhotometricInterpretation = PhotometricName(pixelFormat),
            BitDepth = containerHdr?.BitDepth ?? ChannelBitDepth(pixelFormat),
            HdrTransferFunction = containerHdr?.TransferFunction,
            HdrColorPrimaries = containerHdr?.ColorPrimaries,
            HdrMetadataComplete = containerHdr?.IsComplete,
            ColorType = ColorTypeName(pixelFormat),
            HasAlpha = alphaMode switch
            {
                BitmapAlphaMode.Premultiplied or BitmapAlphaMode.Straight => true,
                BitmapAlphaMode.Ignore => false,
                _ => null,
            },
            Animated = frameCount > 0 ? frameCount > 1 : null,
            FrameCount = frameCount > 0 ? frameCount : null,
        };
    }

    /// <summary>
    /// Reads only the ISO-BMFF colour description and codec bit depth. These fields are
    /// defined by ISO/IEC 23001-8 and are more authoritative for HEIF/AVIF than a file
    /// extension or a generic Windows colour-space label. The probe is intentionally
    /// bounded and fails closed when a box is malformed or incomplete.
    /// </summary>
    internal static IsoBmffHdrMetadata? TryReadIsoBmffHdrMetadata(
        Stream source,
        long sourceLength,
        CancellationToken cancellationToken)
    {
        if (sourceLength < 8 || !source.CanRead || !source.CanSeek)
            return null;

        int length = (int)Math.Min(sourceLength, MaxIsoBmffProbeBytes);
        byte[] buffer = new byte[length];
        int read = source.Read(buffer, 0, Math.Min(12, buffer.Length));
        if (read < 8 || !buffer.AsSpan(4, 4).SequenceEqual("ftyp"u8))
            return null;

        while (read < buffer.Length)
        {
            cancellationToken.ThrowIfCancellationRequested();
            int count = source.Read(buffer, read, buffer.Length - read);
            if (count <= 0)
                break;
            read += count;
        }

        if (read < 8)
            return null;

        var parser = new IsoBmffHdrParser(buffer.AsSpan(0, read));
        return parser.Parse();
    }

    private static async Task DrainWorkerOrExitAsync(Task worker)
    {
        if (await DrainsWithinGraceAsync(worker, DrainGrace))
            return;

        DiagLog.Write(
            "RasterHost",
            "system HANDLE image metadata did not drain after cancellation; exiting host.");
        SupervisedHostProcessPolicy.ExitImmediately(SystemMetadataTimeoutExitCode);
    }

    internal static async Task<bool> DrainsWithinGraceAsync(
        Task worker,
        TimeSpan grace)
    {
        ArgumentNullException.ThrowIfNull(worker);
        if (grace <= TimeSpan.Zero)
            throw new ArgumentOutOfRangeException(nameof(grace));

        try
        {
            await worker.WaitAsync(grace);
            return true;
        }
        catch (TimeoutException)
        {
            return false;
        }
        catch
        {
            // A completed faulted/canceled worker has still drained.
            return true;
        }
    }

    private static string ResolveFormat(
        IReadOnlyList<string> codecExtensions,
        string logicalName)
    {
        string logicalExtension = Path.GetExtension(Path.GetFileName(logicalName));
        string extension = codecExtensions.FirstOrDefault(
                value => string.Equals(value, logicalExtension, StringComparison.OrdinalIgnoreCase))
            ?? codecExtensions
            .FirstOrDefault(static value => !string.IsNullOrWhiteSpace(value))
            ?? logicalExtension;
        return extension.Trim().TrimStart('.').ToUpperInvariant() switch
        {
            "JPG" or "JPE" or "JFIF" => "JPEG",
            "TIF" => "TIFF",
            "DIB" => "BMP",
            "" => "Image",
            string value => value,
        };
    }

    private static byte? ChannelBitDepth(BitmapPixelFormat format) => format switch
    {
        BitmapPixelFormat.Gray8 or BitmapPixelFormat.Rgba8 or BitmapPixelFormat.Bgra8 => 8,
        BitmapPixelFormat.Gray16 or BitmapPixelFormat.Rgba16 => 16,
        _ => null,
    };

    private static string? ColorTypeName(BitmapPixelFormat format) => format switch
    {
        BitmapPixelFormat.Gray8 or BitmapPixelFormat.Gray16 => "grayscale",
        BitmapPixelFormat.Rgba8 or BitmapPixelFormat.Rgba16 => "RGBA",
        BitmapPixelFormat.Bgra8 => "BGRA",
        BitmapPixelFormat.Nv12 or BitmapPixelFormat.P010 or BitmapPixelFormat.Yuy2 => "YCbCr",
        _ => null,
    };

    private static string? PhotometricName(BitmapPixelFormat format) => format switch
    {
        BitmapPixelFormat.Gray8 or BitmapPixelFormat.Gray16 => "black is zero",
        BitmapPixelFormat.Rgba8 or BitmapPixelFormat.Rgba16 or BitmapPixelFormat.Bgra8 => "RGB",
        BitmapPixelFormat.Nv12 or BitmapPixelFormat.P010 or BitmapPixelFormat.Yuy2 => "YCbCr",
        _ => null,
    };

    private static string? FirstText(string? primary, string? fallback)
        => !string.IsNullOrWhiteSpace(primary) ? primary : fallback;

    private static uint? Positive(uint? value) => value is > 0 ? value : null;

    private static double? PositiveFinite(double? value)
        => value is > 0 && double.IsFinite(value.Value) ? value : null;

    internal readonly record struct IsoBmffHdrMetadata(
        string TransferFunction,
        string ColorPrimaries,
        byte BitDepth)
    {
        public bool IsComplete => BitDepth >= 10;
    }

    private ref struct IsoBmffHdrParser
    {
        private readonly ReadOnlySpan<byte> _data;
        private string? _transfer;
        private string? _primaries;
        private byte? _bitDepth;
        private int _boxCount;

        public IsoBmffHdrParser(ReadOnlySpan<byte> data) => _data = data;

        public IsoBmffHdrMetadata? Parse()
        {
            Scan(0, _data.Length, depth: 0);
            if (_transfer is null || _primaries is null || _bitDepth is null)
                return null;
            return new IsoBmffHdrMetadata(_transfer, _primaries, _bitDepth.Value);
        }

        private void Scan(int start, int end, int depth)
        {
            if (depth > MaxIsoBmffDepth || _boxCount >= MaxIsoBmffBoxes)
                return;

            int offset = start;
            while (offset + 8 <= end && _boxCount++ < MaxIsoBmffBoxes)
            {
                uint size32 = ReadUInt32(offset);
                ReadOnlySpan<byte> type = _data.Slice(offset + 4, 4);
                int headerSize = 8;
                long boxSize = size32;
                if (size32 == 1)
                {
                    if (offset + 16 > end)
                        return;
                    ulong extendedSize = ReadUInt64(offset + 8);
                    if (extendedSize > long.MaxValue)
                        return;
                    boxSize = (long)extendedSize;
                    headerSize = 16;
                }
                else if (size32 == 0)
                {
                    boxSize = end - offset;
                }

                if (boxSize < headerSize || boxSize > end - offset)
                    return;

                int boxEnd = checked(offset + (int)boxSize);
                int payloadStart = offset + headerSize;
                if (type.SequenceEqual("colr"u8))
                    ReadColorBox(payloadStart, boxEnd);
                else if (type.SequenceEqual("hvcC"u8))
                    ReadHevcConfig(payloadStart, boxEnd);
                else if (type.SequenceEqual("av1C"u8))
                    ReadAv1Config(payloadStart, boxEnd);

                if (ShouldScanChildren(type))
                {
                    int childStart = type.SequenceEqual("meta"u8)
                        ? Math.Min(payloadStart + 4, boxEnd)
                        : payloadStart;
                    Scan(childStart, boxEnd, depth + 1);
                }

                if (boxEnd <= offset)
                    return;
                offset = boxEnd;
            }
        }

        private void ReadColorBox(int start, int end)
        {
            if (end - start < 11 || !_data.Slice(start, 4).SequenceEqual("nclx"u8))
                return;

            ushort primaries = ReadUInt16(start + 4);
            ushort transfer = ReadUInt16(start + 6);
            _primaries ??= primaries == 9 ? "BT.2020" : null;
            _transfer ??= transfer switch
            {
                16 => "SMPTE ST 2084",
                18 => "ARIB STD-B67",
                _ => null,
            };
        }

        private void ReadHevcConfig(int start, int end)
        {
            if (end - start < 18)
                return;
            byte lumaMinus8 = (byte)(_data[start + 17] & 0x07);
            byte depth = (byte)(8 + lumaMinus8);
            if (depth is >= 8 and <= 16)
                _bitDepth = Math.Max(_bitDepth.GetValueOrDefault(), depth);
        }

        private void ReadAv1Config(int start, int end)
        {
            if (end - start < 3 || (_data[start] & 0x80) == 0)
                return;
            byte flags = _data[start + 2];
            if ((flags & 0x40) == 0)
            {
                _bitDepth = Math.Max(_bitDepth.GetValueOrDefault(), (byte)8);
                return;
            }
            byte depth = (byte)((flags & 0x20) != 0 ? 12 : 10);
            _bitDepth = Math.Max(_bitDepth.GetValueOrDefault(), depth);
        }

        private static bool ShouldScanChildren(ReadOnlySpan<byte> type)
            => !type.SequenceEqual("mdat"u8)
                && !type.SequenceEqual("idat"u8)
                && !type.SequenceEqual("free"u8)
                && !type.SequenceEqual("skip"u8)
                && !type.SequenceEqual("wide"u8);

        private uint ReadUInt32(int offset)
            => System.Buffers.Binary.BinaryPrimitives.ReadUInt32BigEndian(_data.Slice(offset, 4));

        private ulong ReadUInt64(int offset)
            => System.Buffers.Binary.BinaryPrimitives.ReadUInt64BigEndian(_data.Slice(offset, 8));

        private ushort ReadUInt16(int offset)
            => System.Buffers.Binary.BinaryPrimitives.ReadUInt16BigEndian(_data.Slice(offset, 2));
    }
}
