using System.Buffers.Binary;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Microsoft.Win32.SafeHandles;
using QuickLook.Next.Core;

namespace QuickLook.Next.RasterHost;

internal static class SystemImageDecoder
{
    private const uint MaxPreviewRasterDimension = 2048;
    private const int MaxDecodedImageBytes = (int)(MaxPreviewRasterDimension * MaxPreviewRasterDimension * 4);
    private const int MaxDecodedHdrImageBytes = (int)(MaxPreviewRasterDimension * MaxPreviewRasterDimension * 8);
    private const long MaxInputImageBytes = 512L * 1024 * 1024;

    public static async Task<NativeDecodedImage?> TryDecodeAsync(
        string path,
        CancellationToken cancellationToken,
        uint targetWidth = 0,
        uint targetHeight = 0,
        bool enableHdr = false)
    {
        try
        {
            if (IsTooLarge(path))
                return null;

            cancellationToken.ThrowIfCancellationRequested();
            if (enableHdr)
            {
                NativeDecodedImage? hdr = await TryDecodeHdrPathAsync(
                    path,
                    cancellationToken,
                    targetWidth,
                    targetHeight);
                if (hdr is not null)
                    return hdr;
            }

            StorageFile file = await StorageFile.GetFileFromPathAsync(path).AsTask(cancellationToken);
            using var stream = await file.OpenReadAsync().AsTask(cancellationToken);
            return await DecodeAsync(stream, path, cancellationToken, targetWidth, targetHeight);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", $"system image decode failed: {ex.Message}");
            return null;
        }
    }

    public static async Task<NativeDecodedImage?> TryDecodeHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken,
        uint targetWidth = 0,
        uint targetHeight = 0,
        bool enableHdr = false)
    {
        if (sourceLength is < 0 or > MaxInputImageBytes
            || sourceHandle.IsInvalid
            || sourceHandle.IsClosed)
            return null;

        try
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (enableHdr)
            {
                NativeDecodedImage? hdr = await TryDecodeHdrHandleAsync(
                    sourceHandle,
                    sourceLength,
                    logicalName,
                    cancellationToken,
                    targetWidth,
                    targetHeight);
                if (hdr is not null)
                    return hdr;
            }

            using SafeFileHandle decodeHandle = WindowsHandleTransfer.ReopenReadOnlyFile(sourceHandle, sourceLength);
            using var fileStream = new FileStream(decodeHandle, FileAccess.Read, 64 * 1024, isAsync: false);
            if (fileStream.Length != sourceLength)
                return null;
            using var stream = fileStream.AsRandomAccessStream();
            return await DecodeAsync(stream, logicalName, cancellationToken, targetWidth, targetHeight);
        }
        catch (OperationCanceledException)
        {
            throw;
        }
        catch (Exception ex)
        {
            DiagLog.Write("RasterHost", $"system HANDLE image decode failed: {ex.Message}");
            return null;
        }
    }

    private static async Task<NativeDecodedImage?> TryDecodeHdrHandleAsync(
        SafeFileHandle sourceHandle,
        long sourceLength,
        string logicalName,
        CancellationToken cancellationToken,
        uint targetWidth,
        uint targetHeight)
    {
        using SafeFileHandle decodeHandle = WindowsHandleTransfer.ReopenReadOnlyFile(sourceHandle, sourceLength);
        using var fileStream = new FileStream(decodeHandle, FileAccess.Read, 64 * 1024, isAsync: false);
        if (fileStream.Length != sourceLength)
            return null;

        SystemImageMetadataReader.IsoBmffHdrMetadata? containerHdr =
            SystemImageMetadataReader.TryReadIsoBmffHdrMetadata(
                fileStream,
                sourceLength,
                cancellationToken);
        if (containerHdr is not
            {
                IsComplete: true,
                TransferFunction: "SMPTE ST 2084",
            })
        {
            if (containerHdr is { IsComplete: true })
            {
                DiagLog.Write(
                    "RasterHost",
                    $"system HDR candidate downgraded to SDR; transfer={containerHdr.Value.TransferFunction}; " +
                    $"primaries={containerHdr.Value.ColorPrimaries}; bitDepth={containerHdr.Value.BitDepth}");
            }
            return null;
        }

        fileStream.Position = 0;
        using var stream = fileStream.AsRandomAccessStream();
        return await DecodeHdrAsync(
            stream,
            logicalName,
            containerHdr.Value,
            cancellationToken,
            targetWidth,
            targetHeight);
    }

    private static async Task<NativeDecodedImage?> TryDecodeHdrPathAsync(
        string path,
        CancellationToken cancellationToken,
        uint targetWidth,
        uint targetHeight)
    {
        FileInfo fileInfo = new(path);
        if (!fileInfo.Exists || fileInfo.Length > MaxInputImageBytes)
            return null;

        using (var metadataStream = new FileStream(
            path,
            FileMode.Open,
            FileAccess.Read,
            FileShare.ReadWrite | FileShare.Delete,
            64 * 1024,
            FileOptions.SequentialScan))
        {
            SystemImageMetadataReader.IsoBmffHdrMetadata? containerHdr =
                SystemImageMetadataReader.TryReadIsoBmffHdrMetadata(
                    metadataStream,
                    fileInfo.Length,
                    cancellationToken);
            if (containerHdr is not
                {
                    IsComplete: true,
                    TransferFunction: "SMPTE ST 2084",
                })
            {
                if (containerHdr is { IsComplete: true })
                {
                    DiagLog.Write(
                        "RasterHost",
                        $"system HDR candidate downgraded to SDR; transfer={containerHdr.Value.TransferFunction}; " +
                        $"primaries={containerHdr.Value.ColorPrimaries}; bitDepth={containerHdr.Value.BitDepth}");
                }
                return null;
            }

            StorageFile file = await StorageFile.GetFileFromPathAsync(path).AsTask(cancellationToken);
            using var stream = await file.OpenReadAsync().AsTask(cancellationToken);
            return await DecodeHdrAsync(
                stream,
                path,
                containerHdr.Value,
                cancellationToken,
                targetWidth,
                targetHeight);
        }
    }

    private static async Task<NativeDecodedImage?> DecodeHdrAsync(
        Windows.Storage.Streams.IRandomAccessStream stream,
        string logicalName,
        SystemImageMetadataReader.IsoBmffHdrMetadata containerHdr,
        CancellationToken cancellationToken,
        uint targetWidth,
        uint targetHeight)
    {
        BitmapDecoder decoder = await BitmapDecoder.CreateAsync(stream).AsTask(cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();

        uint originalWidth = decoder.OrientedPixelWidth > 0
            ? decoder.OrientedPixelWidth
            : decoder.PixelWidth;
        uint originalHeight = decoder.OrientedPixelHeight > 0
            ? decoder.OrientedPixelHeight
            : decoder.PixelHeight;
        if (originalWidth == 0 || originalHeight == 0)
            return null;

        BitmapTransform transform = BuildTransform(
            originalWidth,
            originalHeight,
            targetWidth,
            targetHeight);
        PixelDataProvider pixels = await decoder.GetPixelDataAsync(
            BitmapPixelFormat.Rgba16,
            BitmapAlphaMode.Straight,
            transform,
            ExifOrientationMode.RespectExifOrientation,
            ColorManagementMode.DoNotColorManage).AsTask(cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();

        byte[] rgba16 = pixels.DetachPixelData();
        int width = ResolveDecodedWidth(transform, originalWidth);
        int height = ResolveDecodedHeight(transform, originalHeight);
        int expected = checked(width * height * 8);
        if (rgba16.Length != expected || rgba16.Length > MaxDecodedHdrImageBytes)
            return null;

        (byte[] fp16, byte[] bgra) = ConvertPqToScRgb(rgba16, width, height);
        DiagLog.Write(
            "RasterHost",
            $"system HDR image decoded format=PQ/scRGB; target={targetWidth}x{targetHeight}; " +
            $"original={originalWidth}x{originalHeight}; output={width}x{height}; bytes={fp16.Length}");
        return new NativeDecodedImage(
            bgra,
            width,
            height,
            checked((int)originalWidth),
            checked((int)originalHeight))
        {
            Rgba16Float = fp16,
            SourceProfile = ImageColorProfileKind.PqBt2020,
            SourceBitDepth = containerHdr.BitDepth,
            SourceMetadataComplete = containerHdr.IsComplete,
        };
    }

    private static async Task<NativeDecodedImage?> DecodeAsync(
        Windows.Storage.Streams.IRandomAccessStream stream,
        string logicalName,
        CancellationToken cancellationToken,
        uint targetWidth,
        uint targetHeight)
    {
        string ext = Path.GetExtension(logicalName).ToLowerInvariant();
        BitmapDecoder decoder = await BitmapDecoder.CreateAsync(stream).AsTask(cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();

        uint originalWidth = decoder.OrientedPixelWidth > 0 ? decoder.OrientedPixelWidth : decoder.PixelWidth;
        uint originalHeight = decoder.OrientedPixelHeight > 0 ? decoder.OrientedPixelHeight : decoder.PixelHeight;
        if (originalWidth == 0 || originalHeight == 0)
            return null;

        BitmapTransform transform = BuildTransform(originalWidth, originalHeight, targetWidth, targetHeight);

        PixelDataProvider pixels = await decoder.GetPixelDataAsync(
            BitmapPixelFormat.Bgra8,
            BitmapAlphaMode.Premultiplied,
            transform,
            ExifOrientationMode.RespectExifOrientation,
            ColorManagementMode.ColorManageToSRgb).AsTask(cancellationToken);
        cancellationToken.ThrowIfCancellationRequested();

        byte[] bgra = pixels.DetachPixelData();
        if (bgra.Length <= 0 || bgra.Length > MaxDecodedImageBytes || bgra.Length % 4 != 0)
            return null;

        int width = ResolveDecodedWidth(transform, originalWidth);
        int height = ResolveDecodedHeight(transform, originalHeight);
        if (bgra.Length != width * height * 4)
        {
            int pixelCount = bgra.Length / 4;
            if (width > 0 && pixelCount % width == 0)
                height = pixelCount / width;
            else
                return null;
        }

        DiagLog.Write(
            "RasterHost",
            $"system image decoded ext={ext}; target={targetWidth}x{targetHeight}; " +
            $"original={originalWidth}x{originalHeight}; output={width}x{height}; bytes={bgra.Length}");
        return new NativeDecodedImage(bgra, width, height, checked((int)originalWidth), checked((int)originalHeight));
    }

    private static BitmapTransform BuildTransform(
        uint originalWidth,
        uint originalHeight,
        uint targetWidth,
        uint targetHeight)
    {
        var transform = new BitmapTransform();
        uint decodeTargetWidth = targetWidth > 0
            ? Math.Min(targetWidth, MaxPreviewRasterDimension)
            : MaxPreviewRasterDimension;
        uint decodeTargetHeight = targetHeight > 0
            ? Math.Min(targetHeight, MaxPreviewRasterDimension)
            : MaxPreviewRasterDimension;
        double scale = Math.Min(
            1.0,
            Math.Min(
                decodeTargetWidth / (double)originalWidth,
                decodeTargetHeight / (double)originalHeight));
        if (scale < 1.0)
        {
            transform.ScaledWidth = Math.Max(1, (uint)Math.Round(originalWidth * scale));
            transform.ScaledHeight = Math.Max(1, (uint)Math.Round(originalHeight * scale));
            transform.InterpolationMode = BitmapInterpolationMode.Fant;
        }
        return transform;
    }

    private static int ResolveDecodedWidth(BitmapTransform transform, uint originalWidth)
        => transform.ScaledWidth > 0 ? checked((int)transform.ScaledWidth) : checked((int)originalWidth);

    private static int ResolveDecodedHeight(BitmapTransform transform, uint originalHeight)
        => transform.ScaledHeight > 0 ? checked((int)transform.ScaledHeight) : checked((int)originalHeight);

    internal static (byte[] Rgba16Float, byte[] Bgra) ConvertPqToScRgb(
        byte[] rgba16,
        int width,
        int height)
    {
        int pixelCount = checked(width * height);
        if (width <= 0 || height <= 0 || rgba16.Length != checked(pixelCount * 8))
            throw new ArgumentException("RGBA16 input dimensions do not match.", nameof(rgba16));

        byte[] fp16 = new byte[checked(pixelCount * 8)];
        byte[] bgra = new byte[checked(pixelCount * 4)];
        for (int pixel = 0; pixel < pixelCount; pixel++)
        {
            int sourceOffset = pixel * 8;
            int fp16Offset = sourceOffset;
            int bgraOffset = pixel * 4;
            float alpha = BinaryPrimitives.ReadUInt16LittleEndian(rgba16.AsSpan(sourceOffset + 6, 2)) / 65535f;
            float red = PqCodeToScRgb(BinaryPrimitives.ReadUInt16LittleEndian(rgba16.AsSpan(sourceOffset, 2)));
            float green = PqCodeToScRgb(BinaryPrimitives.ReadUInt16LittleEndian(rgba16.AsSpan(sourceOffset + 2, 2)));
            float blue = PqCodeToScRgb(BinaryPrimitives.ReadUInt16LittleEndian(rgba16.AsSpan(sourceOffset + 4, 2)));

            WriteHalf(fp16, fp16Offset, red * alpha);
            WriteHalf(fp16, fp16Offset + 2, green * alpha);
            WriteHalf(fp16, fp16Offset + 4, blue * alpha);
            WriteHalf(fp16, fp16Offset + 6, alpha);

            bgra[bgraOffset] = ToSdrByte(blue, alpha);
            bgra[bgraOffset + 1] = ToSdrByte(green, alpha);
            bgra[bgraOffset + 2] = ToSdrByte(red, alpha);
            bgra[bgraOffset + 3] = (byte)Math.Round(alpha * 255f);
        }
        return (fp16, bgra);
    }

    private static float PqCodeToScRgb(ushort code)
    {
        const float C1 = 3424f / 4096f;
        const float C2 = 2413f / 128f;
        const float C3 = 2392f / 128f;
        const float M1 = 2610f / 16384f;
        const float M2 = 2523f / 32f;
        float normalized = code / 65535f;
        float powered = MathF.Pow(normalized, 1f / M2);
        float numerator = MathF.Max(powered - C1, 0f);
        float denominator = C2 - C3 * powered;
        float normalizedLuminance = denominator > 0f
            ? MathF.Pow(numerator / denominator, 1f / M1)
            : 0f;
        return Math.Clamp(normalizedLuminance * 10000f / 80f, 0f, 125f);
    }

    private static byte ToSdrByte(float scRgb, float alpha)
    {
        float toneMapped = scRgb / (1f + scRgb);
        float encoded = toneMapped <= 0.0031308f
            ? toneMapped * 12.92f
            : 1.055f * MathF.Pow(toneMapped, 1f / 2.4f) - 0.055f;
        return (byte)Math.Clamp(MathF.Round(encoded * alpha * 255f), 0f, 255f);
    }

    private static void WriteHalf(byte[] destination, int offset, float value)
        => BinaryPrimitives.WriteUInt16LittleEndian(
            destination.AsSpan(offset, 2),
            BitConverter.HalfToUInt16Bits((Half)value));

    private static bool IsTooLarge(string path)
    {
        try { return new FileInfo(path).Length > MaxInputImageBytes; }
        catch { return false; }
    }
}
